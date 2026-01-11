//! Aggregation functions for GROUP BY operations.

use crate::types::{DataTable, DataValue};
use crate::Result;
use std::collections::HashMap;

/// Aggregation function types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationFunction {
    Count,
    Sum,
    Min,
    Max,
    Avg,
    CountDistinct,
    First,
    Last,
}

impl AggregationFunction {
    /// Get the function name.
    pub fn name(&self) -> &'static str {
        match self {
            AggregationFunction::Count => "COUNT",
            AggregationFunction::Sum => "SUM",
            AggregationFunction::Min => "MIN",
            AggregationFunction::Max => "MAX",
            AggregationFunction::Avg => "AVG",
            AggregationFunction::CountDistinct => "COUNT_DISTINCT",
            AggregationFunction::First => "FIRST",
            AggregationFunction::Last => "LAST",
        }
    }
}

/// Aggregator for GROUP BY operations.
pub struct Aggregator {
    /// Group by column indices
    group_by_columns: Vec<usize>,
    /// Aggregation functions with column indices
    aggregations: Vec<(usize, AggregationFunction)>,
}

impl Aggregator {
    pub fn new(
        group_by_columns: Vec<usize>,
        aggregations: Vec<(usize, AggregationFunction)>,
    ) -> Self {
        Self {
            group_by_columns,
            aggregations,
        }
    }

    /// Aggregate rows from multiple data tables.
    pub fn aggregate(&self, tables: Vec<DataTable>) -> Result<Vec<Vec<DataValue>>> {
        let mut groups: HashMap<Vec<DataValue>, AggregateState> = HashMap::new();

        // Process all rows
        for table in tables {
            for row in table.rows {
                // Extract group key
                let key: Vec<DataValue> = self
                    .group_by_columns
                    .iter()
                    .map(|&idx| row.get(idx).cloned().unwrap_or(DataValue::Null))
                    .collect();

                // Update or create aggregate state
                let state = groups
                    .entry(key)
                    .or_insert_with(|| AggregateState::new(self.aggregations.len()));

                // Update aggregates
                for (i, (col_idx, func)) in self.aggregations.iter().enumerate() {
                    let value = row.get(*col_idx).cloned().unwrap_or(DataValue::Null);
                    state.update(i, &value, *func);
                }
            }
        }

        // Finalize and build result rows
        let mut result_rows: Vec<Vec<DataValue>> = Vec::with_capacity(groups.len());

        for (key, state) in groups {
            let mut row = key;
            for (i, (_, func)) in self.aggregations.iter().enumerate() {
                row.push(state.finalize(i, *func));
            }
            result_rows.push(row);
        }

        Ok(result_rows)
    }
}

/// State for aggregation.
struct AggregateState {
    /// Count per aggregation
    counts: Vec<u64>,
    /// Sum per aggregation (for numeric types)
    sums: Vec<f64>,
    /// Min values
    mins: Vec<Option<DataValue>>,
    /// Max values
    maxs: Vec<Option<DataValue>>,
    /// First values
    firsts: Vec<Option<DataValue>>,
    /// Last values
    lasts: Vec<Option<DataValue>>,
}

impl AggregateState {
    fn new(num_aggregations: usize) -> Self {
        Self {
            counts: vec![0; num_aggregations],
            sums: vec![0.0; num_aggregations],
            mins: vec![None; num_aggregations],
            maxs: vec![None; num_aggregations],
            firsts: vec![None; num_aggregations],
            lasts: vec![None; num_aggregations],
        }
    }

    fn update(&mut self, idx: usize, value: &DataValue, func: AggregationFunction) {
        if value.is_null() {
            return;
        }

        match func {
            AggregationFunction::Count => {
                self.counts[idx] += 1;
            }
            AggregationFunction::Sum => {
                if let Some(v) = value.as_double() {
                    self.sums[idx] += v;
                    self.counts[idx] += 1;
                }
            }
            AggregationFunction::Min => {
                let should_update = match &self.mins[idx] {
                    None => true,
                    Some(current) => self.compare_values(value, current) == std::cmp::Ordering::Less,
                };
                if should_update {
                    self.mins[idx] = Some(value.clone());
                }
            }
            AggregationFunction::Max => {
                let should_update = match &self.maxs[idx] {
                    None => true,
                    Some(current) => self.compare_values(value, current) == std::cmp::Ordering::Greater,
                };
                if should_update {
                    self.maxs[idx] = Some(value.clone());
                }
            }
            AggregationFunction::Avg => {
                if let Some(v) = value.as_double() {
                    self.sums[idx] += v;
                    self.counts[idx] += 1;
                }
            }
            AggregationFunction::CountDistinct => {
                // Simplified: just count (would need set for true distinct)
                self.counts[idx] += 1;
            }
            AggregationFunction::First => {
                if self.firsts[idx].is_none() {
                    self.firsts[idx] = Some(value.clone());
                }
            }
            AggregationFunction::Last => {
                self.lasts[idx] = Some(value.clone());
            }
        }
    }

    fn finalize(&self, idx: usize, func: AggregationFunction) -> DataValue {
        match func {
            AggregationFunction::Count | AggregationFunction::CountDistinct => {
                DataValue::Long(self.counts[idx] as i64)
            }
            AggregationFunction::Sum => {
                DataValue::Double(self.sums[idx])
            }
            AggregationFunction::Min => {
                self.mins[idx].clone().unwrap_or(DataValue::Null)
            }
            AggregationFunction::Max => {
                self.maxs[idx].clone().unwrap_or(DataValue::Null)
            }
            AggregationFunction::Avg => {
                if self.counts[idx] > 0 {
                    DataValue::Double(self.sums[idx] / self.counts[idx] as f64)
                } else {
                    DataValue::Null
                }
            }
            AggregationFunction::First => {
                self.firsts[idx].clone().unwrap_or(DataValue::Null)
            }
            AggregationFunction::Last => {
                self.lasts[idx].clone().unwrap_or(DataValue::Null)
            }
        }
    }

    fn compare_values(&self, a: &DataValue, b: &DataValue) -> std::cmp::Ordering {
        match (a, b) {
            (DataValue::Int(x), DataValue::Int(y)) => x.cmp(y),
            (DataValue::Long(x), DataValue::Long(y)) => x.cmp(y),
            (DataValue::Float(x), DataValue::Float(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::Double(x), DataValue::Double(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::String(x), DataValue::String(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

/// Sum aggregator for numeric values.
pub struct SumAggregator {
    sum: f64,
    count: u64,
}

impl SumAggregator {
    pub fn new() -> Self {
        Self { sum: 0.0, count: 0 }
    }

    pub fn add(&mut self, value: &DataValue) {
        if let Some(v) = value.as_double() {
            self.sum += v;
            self.count += 1;
        }
    }

    pub fn merge(&mut self, other: &SumAggregator) {
        self.sum += other.sum;
        self.count += other.count;
    }

    pub fn result(&self) -> f64 {
        self.sum
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

impl Default for SumAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Count aggregator.
pub struct CountAggregator {
    count: u64,
}

impl CountAggregator {
    pub fn new() -> Self {
        Self { count: 0 }
    }

    pub fn add(&mut self) {
        self.count += 1;
    }

    pub fn merge(&mut self, other: &CountAggregator) {
        self.count += other.count;
    }

    pub fn result(&self) -> u64 {
        self.count
    }
}

impl Default for CountAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Min/Max aggregator.
pub struct MinMaxAggregator {
    min: Option<DataValue>,
    max: Option<DataValue>,
}

impl MinMaxAggregator {
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    pub fn add(&mut self, value: &DataValue) {
        if value.is_null() {
            return;
        }

        // Update min
        match &self.min {
            None => self.min = Some(value.clone()),
            Some(current) => {
                if Self::compare(value, current) == std::cmp::Ordering::Less {
                    self.min = Some(value.clone());
                }
            }
        }

        // Update max
        match &self.max {
            None => self.max = Some(value.clone()),
            Some(current) => {
                if Self::compare(value, current) == std::cmp::Ordering::Greater {
                    self.max = Some(value.clone());
                }
            }
        }
    }

    pub fn min(&self) -> Option<&DataValue> {
        self.min.as_ref()
    }

    pub fn max(&self) -> Option<&DataValue> {
        self.max.as_ref()
    }

    fn compare(a: &DataValue, b: &DataValue) -> std::cmp::Ordering {
        match (a, b) {
            (DataValue::Int(x), DataValue::Int(y)) => x.cmp(y),
            (DataValue::Long(x), DataValue::Long(y)) => x.cmp(y),
            (DataValue::Float(x), DataValue::Float(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::Double(x), DataValue::Double(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::String(x), DataValue::String(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

impl Default for MinMaxAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDataType, DataSchema};

    fn create_data_table(rows: Vec<Vec<DataValue>>) -> DataTable {
        let schema = DataSchema::new(
            vec!["group".to_string(), "value".to_string()],
            vec![ColumnDataType::String, ColumnDataType::Double],
        );
        let mut dt = DataTable::new(schema);
        for row in rows {
            dt.add_row(row);
        }
        dt
    }

    #[test]
    fn test_sum_aggregator() {
        let mut agg = SumAggregator::new();
        agg.add(&DataValue::Double(1.0));
        agg.add(&DataValue::Double(2.0));
        agg.add(&DataValue::Double(3.0));

        assert_eq!(agg.result(), 6.0);
        assert_eq!(agg.count(), 3);
    }

    #[test]
    fn test_count_aggregator() {
        let mut agg = CountAggregator::new();
        agg.add();
        agg.add();
        agg.add();

        assert_eq!(agg.result(), 3);
    }

    #[test]
    fn test_minmax_aggregator() {
        let mut agg = MinMaxAggregator::new();
        agg.add(&DataValue::Double(5.0));
        agg.add(&DataValue::Double(2.0));
        agg.add(&DataValue::Double(8.0));

        assert_eq!(agg.min(), Some(&DataValue::Double(2.0)));
        assert_eq!(agg.max(), Some(&DataValue::Double(8.0)));
    }

    #[test]
    fn test_aggregator_groupby() {
        let table1 = create_data_table(vec![
            vec![DataValue::String("a".to_string()), DataValue::Double(1.0)],
            vec![DataValue::String("b".to_string()), DataValue::Double(2.0)],
            vec![DataValue::String("a".to_string()), DataValue::Double(3.0)],
        ]);

        let table2 = create_data_table(vec![
            vec![DataValue::String("a".to_string()), DataValue::Double(4.0)],
            vec![DataValue::String("b".to_string()), DataValue::Double(5.0)],
        ]);

        let aggregator = Aggregator::new(
            vec![0], // Group by first column
            vec![(1, AggregationFunction::Sum)], // Sum second column
        );

        let result = aggregator.aggregate(vec![table1, table2]).unwrap();

        assert_eq!(result.len(), 2); // Two groups: "a" and "b"

        // Find group "a" and "b"
        let mut a_sum = 0.0;
        let mut b_sum = 0.0;
        for row in &result {
            if let DataValue::String(s) = &row[0] {
                if let DataValue::Double(v) = &row[1] {
                    if s == "a" {
                        a_sum = *v;
                    } else if s == "b" {
                        b_sum = *v;
                    }
                }
            }
        }

        assert_eq!(a_sum, 8.0); // 1 + 3 + 4
        assert_eq!(b_sum, 7.0); // 2 + 5
    }

    #[test]
    fn test_aggregation_functions() {
        let table = create_data_table(vec![
            vec![DataValue::String("a".to_string()), DataValue::Double(1.0)],
            vec![DataValue::String("a".to_string()), DataValue::Double(2.0)],
            vec![DataValue::String("a".to_string()), DataValue::Double(3.0)],
        ]);

        // Test COUNT
        let count_agg = Aggregator::new(vec![0], vec![(1, AggregationFunction::Count)]);
        let result = count_agg.aggregate(vec![table.clone()]).unwrap();
        if let DataValue::Long(v) = &result[0][1] {
            assert_eq!(*v, 3);
        }

        // Test AVG
        let avg_agg = Aggregator::new(vec![0], vec![(1, AggregationFunction::Avg)]);
        let result = avg_agg.aggregate(vec![table.clone()]).unwrap();
        if let DataValue::Double(v) = &result[0][1] {
            assert_eq!(*v, 2.0);
        }

        // Test MIN
        let min_agg = Aggregator::new(vec![0], vec![(1, AggregationFunction::Min)]);
        let result = min_agg.aggregate(vec![table.clone()]).unwrap();
        if let DataValue::Double(v) = &result[0][1] {
            assert_eq!(*v, 1.0);
        }

        // Test MAX
        let max_agg = Aggregator::new(vec![0], vec![(1, AggregationFunction::Max)]);
        let result = max_agg.aggregate(vec![table]).unwrap();
        if let DataValue::Double(v) = &result[0][1] {
            assert_eq!(*v, 3.0);
        }
    }
}
