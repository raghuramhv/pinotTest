//! Data table merger for combining results from multiple servers.

use crate::types::{DataSchema, DataTable, DataValue, ResultTable};
use crate::{BrokerError, Result};
use rayon::prelude::*;
use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// Merger for combining data tables.
pub struct DataTableMerger {
    /// Maximum rows to return
    max_rows: usize,
}

impl DataTableMerger {
    pub fn new(max_rows: usize) -> Self {
        Self { max_rows }
    }

    /// Merge multiple data tables into one.
    pub fn merge(&self, tables: Vec<DataTable>) -> Result<ResultTable> {
        if tables.is_empty() {
            return Err(BrokerError::Reduce("No tables to merge".to_string()));
        }

        // Validate schemas
        let schema = tables[0].schema.clone();
        for (i, table) in tables.iter().enumerate().skip(1) {
            if !schema.is_compatible(&table.schema) {
                return Err(BrokerError::SchemaMismatch {
                    server: format!("table_{}", i),
                    details: format!(
                        "Expected schema {:?}, got {:?}",
                        schema.column_names, table.schema.column_names
                    ),
                });
            }
        }

        // Collect all rows
        let total_rows: usize = tables.iter().map(|t| t.num_rows()).sum();
        let mut all_rows = Vec::with_capacity(total_rows.min(self.max_rows));

        for table in tables {
            if all_rows.len() >= self.max_rows {
                break;
            }
            let remaining = self.max_rows - all_rows.len();
            all_rows.extend(table.rows.into_iter().take(remaining));
        }

        Ok(ResultTable::new(schema, all_rows))
    }

    /// Merge with ordering (k-way merge).
    pub fn merge_ordered(
        &self,
        tables: Vec<DataTable>,
        order_by: &[(usize, bool)], // (column_index, is_ascending)
    ) -> Result<ResultTable> {
        if tables.is_empty() {
            return Err(BrokerError::Reduce("No tables to merge".to_string()));
        }

        let schema = tables[0].schema.clone();

        // Use k-way merge with a min-heap
        let mut result_rows = Vec::with_capacity(self.max_rows);
        let mut heap = BinaryHeap::new();

        // Initialize heap with first row from each table
        for (table_idx, table) in tables.iter().enumerate() {
            if !table.rows.is_empty() {
                heap.push(MergeEntry {
                    row: table.rows[0].clone(),
                    table_idx,
                    row_idx: 0,
                    order_by: order_by.to_vec(),
                });
            }
        }

        // Extract rows in order
        while !heap.is_empty() && result_rows.len() < self.max_rows {
            let entry = heap.pop().unwrap();
            result_rows.push(entry.row);

            // Add next row from same table
            let next_row_idx = entry.row_idx + 1;
            if next_row_idx < tables[entry.table_idx].rows.len() {
                heap.push(MergeEntry {
                    row: tables[entry.table_idx].rows[next_row_idx].clone(),
                    table_idx: entry.table_idx,
                    row_idx: next_row_idx,
                    order_by: order_by.to_vec(),
                });
            }
        }

        Ok(ResultTable::new(schema, result_rows))
    }

    /// Merge with deduplication.
    pub fn merge_distinct(
        &self,
        tables: Vec<DataTable>,
        key_columns: &[usize],
    ) -> Result<ResultTable> {
        if tables.is_empty() {
            return Err(BrokerError::Reduce("No tables to merge".to_string()));
        }

        let schema = tables[0].schema.clone();

        // Use a set to track seen keys
        let mut seen = ahash::AHashSet::new();
        let mut result_rows = Vec::with_capacity(self.max_rows);

        for table in tables {
            if result_rows.len() >= self.max_rows {
                break;
            }

            for row in table.rows {
                if result_rows.len() >= self.max_rows {
                    break;
                }

                // Extract key
                let key: Vec<u8> = key_columns
                    .iter()
                    .flat_map(|&idx| {
                        let value = row.get(idx).unwrap_or(&DataValue::Null);
                        self.hash_value(value)
                    })
                    .collect();

                if seen.insert(key) {
                    result_rows.push(row);
                }
            }
        }

        Ok(ResultTable::new(schema, result_rows))
    }

    fn hash_value(&self, value: &DataValue) -> Vec<u8> {
        match value {
            DataValue::Null => vec![0],
            DataValue::Int(v) => v.to_le_bytes().to_vec(),
            DataValue::Long(v) => v.to_le_bytes().to_vec(),
            DataValue::Float(v) => v.to_le_bytes().to_vec(),
            DataValue::Double(v) => v.to_le_bytes().to_vec(),
            DataValue::Boolean(v) => vec![if *v { 1 } else { 0 }],
            DataValue::String(s) => s.as_bytes().to_vec(),
            DataValue::Bytes(b) => b.clone(),
            _ => vec![0], // Arrays - simplified
        }
    }
}

impl Default for DataTableMerger {
    fn default() -> Self {
        Self::new(100_000)
    }
}

/// Entry in the merge heap.
struct MergeEntry {
    row: Vec<DataValue>,
    table_idx: usize,
    row_idx: usize,
    order_by: Vec<(usize, bool)>,
}

impl PartialEq for MergeEntry {
    fn eq(&self, other: &Self) -> bool {
        self.row == other.row
    }
}

impl Eq for MergeEntry {}

impl PartialOrd for MergeEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MergeEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order for min-heap behavior (BinaryHeap is a max-heap)
        for (col_idx, ascending) in &self.order_by {
            let cmp = compare_values(&self.row[*col_idx], &other.row[*col_idx]);
            if cmp != Ordering::Equal {
                let result = if *ascending { cmp } else { cmp.reverse() };
                return result.reverse(); // Reverse for min-heap
            }
        }
        Ordering::Equal
    }
}

fn compare_values(a: &DataValue, b: &DataValue) -> Ordering {
    match (a, b) {
        (DataValue::Null, DataValue::Null) => Ordering::Equal,
        (DataValue::Null, _) => Ordering::Less,
        (_, DataValue::Null) => Ordering::Greater,
        (DataValue::Int(x), DataValue::Int(y)) => x.cmp(y),
        (DataValue::Long(x), DataValue::Long(y)) => x.cmp(y),
        (DataValue::Float(x), DataValue::Float(y)) => {
            x.partial_cmp(y).unwrap_or(Ordering::Equal)
        }
        (DataValue::Double(x), DataValue::Double(y)) => {
            x.partial_cmp(y).unwrap_or(Ordering::Equal)
        }
        (DataValue::String(x), DataValue::String(y)) => x.cmp(y),
        (DataValue::Boolean(x), DataValue::Boolean(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnDataType;

    fn create_schema() -> DataSchema {
        DataSchema::new(
            vec!["id".to_string(), "name".to_string()],
            vec![ColumnDataType::Long, ColumnDataType::String],
        )
    }

    fn create_table(rows: Vec<Vec<DataValue>>) -> DataTable {
        let mut table = DataTable::new(create_schema());
        for row in rows {
            table.add_row(row);
        }
        table
    }

    #[test]
    fn test_simple_merge() {
        let merger = DataTableMerger::new(100);

        let table1 = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())],
            vec![DataValue::Long(2), DataValue::String("b".to_string())],
        ]);

        let table2 = create_table(vec![
            vec![DataValue::Long(3), DataValue::String("c".to_string())],
        ]);

        let result = merger.merge(vec![table1, table2]).unwrap();

        assert_eq!(result.num_rows(), 3);
    }

    #[test]
    fn test_merge_with_limit() {
        let merger = DataTableMerger::new(2);

        let table = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())],
            vec![DataValue::Long(2), DataValue::String("b".to_string())],
            vec![DataValue::Long(3), DataValue::String("c".to_string())],
        ]);

        let result = merger.merge(vec![table]).unwrap();

        assert_eq!(result.num_rows(), 2);
    }

    #[test]
    fn test_merge_ordered() {
        let merger = DataTableMerger::new(100);

        let table1 = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())],
            vec![DataValue::Long(3), DataValue::String("c".to_string())],
            vec![DataValue::Long(5), DataValue::String("e".to_string())],
        ]);

        let table2 = create_table(vec![
            vec![DataValue::Long(2), DataValue::String("b".to_string())],
            vec![DataValue::Long(4), DataValue::String("d".to_string())],
        ]);

        let result = merger
            .merge_ordered(vec![table1, table2], &[(0, true)])
            .unwrap();

        assert_eq!(result.num_rows(), 5);

        // Verify ordering
        for i in 0..4 {
            if let (DataValue::Long(a), DataValue::Long(b)) = (&result.rows[i][0], &result.rows[i + 1][0]) {
                assert!(a < b, "Expected {} < {}", a, b);
            }
        }
    }

    #[test]
    fn test_merge_distinct() {
        let merger = DataTableMerger::new(100);

        let table1 = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())],
            vec![DataValue::Long(2), DataValue::String("b".to_string())],
        ]);

        let table2 = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())], // Duplicate
            vec![DataValue::Long(3), DataValue::String("c".to_string())],
        ]);

        let result = merger
            .merge_distinct(vec![table1, table2], &[0])
            .unwrap();

        assert_eq!(result.num_rows(), 3);
    }

    #[test]
    fn test_empty_tables() {
        let merger = DataTableMerger::new(100);
        let result = merger.merge(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_schema_mismatch() {
        let merger = DataTableMerger::new(100);

        let table1 = create_table(vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string())],
        ]);

        let schema2 = DataSchema::new(
            vec!["different".to_string()],
            vec![ColumnDataType::Long],
        );
        let mut table2 = DataTable::new(schema2);
        table2.add_row(vec![DataValue::Long(1)]);

        let result = merger.merge(vec![table1, table2]);
        assert!(matches!(result, Err(BrokerError::SchemaMismatch { .. })));
    }
}
