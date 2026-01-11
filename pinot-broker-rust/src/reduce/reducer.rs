//! Broker reduce service for result aggregation.

use crate::error::QueryProcessingException;
use crate::reduce::aggregator::{AggregationFunction, Aggregator};
use crate::reduce::merger::DataTableMerger;
use crate::types::{
    BrokerResponse, DataSchema, DataTable, DataValue, ExecutionStats, ResultTable,
    ServerInstance, ServerResponse,
};
use crate::{BrokerError, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Configuration for reduce operations.
#[derive(Debug, Clone)]
pub struct ReduceConfig {
    /// Maximum rows in result
    pub max_rows: usize,
    /// Group-by trim threshold
    pub groupby_trim_threshold: usize,
    /// Enable parallel reduce
    pub enable_parallel: bool,
    /// Number of reduce threads
    pub num_threads: usize,
}

impl Default for ReduceConfig {
    fn default() -> Self {
        Self {
            max_rows: 100_000,
            groupby_trim_threshold: 1_000_000,
            enable_parallel: true,
            num_threads: num_cpus::get(),
        }
    }
}

/// Broker reduce service for merging server responses.
pub struct BrokerReduceService {
    config: ReduceConfig,
    merger: DataTableMerger,
}

impl BrokerReduceService {
    pub fn new(config: ReduceConfig) -> Self {
        Self {
            merger: DataTableMerger::new(config.max_rows),
            config,
        }
    }

    /// Reduce responses from multiple servers into a single broker response.
    pub fn reduce(
        &self,
        responses: Vec<(ServerInstance, ServerResponse)>,
        num_servers_queried: usize,
    ) -> Result<BrokerResponse> {
        let start = Instant::now();
        let mut broker_response = BrokerResponse::new();

        broker_response.num_servers_queried = num_servers_queried;

        // Separate successful and failed responses
        let (successful, failed): (Vec<_>, Vec<_>) = responses
            .into_iter()
            .partition(|(_, resp)| resp.is_success());

        broker_response.num_servers_responded = successful.len();

        // Collect exceptions from failed responses
        for (server, resp) in failed {
            if let Some(error) = resp.error {
                broker_response.exceptions.push(QueryProcessingException {
                    error_code: -1,
                    message: error.to_string(),
                    server: Some(server.address()),
                });
            }
        }

        // Extract data tables and aggregate stats
        let mut data_tables: Vec<DataTable> = Vec::new();
        let mut stats = ExecutionStats::default();

        for (server, resp) in successful {
            if let Some(dt) = resp.data_table {
                // Collect exceptions from data table
                for exc in &dt.exceptions {
                    let mut exc_clone = exc.clone();
                    exc_clone.server = Some(server.address());
                    broker_response.exceptions.push(exc_clone);
                }

                // Aggregate stats from metadata
                stats.num_docs_scanned += dt.num_docs_scanned();
                stats.total_docs += dt.total_docs();
                stats.total_server_time_ms = stats.total_server_time_ms.max(dt.time_used_ms());

                data_tables.push(dt);
            }
        }

        // Merge data tables
        if !data_tables.is_empty() {
            let result_table = self.merge_data_tables(data_tables)?;
            broker_response.result_table = Some(result_table);
        }

        // Set stats
        broker_response.num_docs_scanned = stats.num_docs_scanned;
        broker_response.total_docs = stats.total_docs;
        broker_response.time_used_ms = stats.total_server_time_ms;
        broker_response.broker_reduce_time_ms = start.elapsed().as_millis() as u64;

        Ok(broker_response)
    }

    /// Merge multiple data tables into a result table.
    fn merge_data_tables(&self, tables: Vec<DataTable>) -> Result<ResultTable> {
        if tables.is_empty() {
            return Err(BrokerError::Reduce("No data tables to merge".to_string()));
        }

        // Validate schemas are compatible and save first schema
        let first_schema = tables[0].schema.clone();
        for (i, table) in tables.iter().enumerate().skip(1) {
            if !first_schema.is_compatible(&table.schema) {
                return Err(BrokerError::SchemaMismatch {
                    server: format!("table_{}", i),
                    details: "Incompatible schemas".to_string(),
                });
            }
        }

        // Merge rows
        let merged_rows = if self.config.enable_parallel && tables.len() > 1 {
            self.parallel_merge(tables)
        } else {
            self.sequential_merge(tables)
        };

        // Apply limit
        let limited_rows = if merged_rows.len() > self.config.max_rows {
            merged_rows.into_iter().take(self.config.max_rows).collect()
        } else {
            merged_rows
        };

        Ok(ResultTable::new(first_schema, limited_rows))
    }

    fn sequential_merge(&self, tables: Vec<DataTable>) -> Vec<Vec<DataValue>> {
        let mut all_rows = Vec::new();
        for table in tables {
            all_rows.extend(table.rows);
        }
        all_rows
    }

    fn parallel_merge(&self, tables: Vec<DataTable>) -> Vec<Vec<DataValue>> {
        tables
            .into_par_iter()
            .flat_map(|t| t.rows)
            .collect()
    }

    /// Reduce with GROUP BY aggregation.
    pub fn reduce_with_groupby(
        &self,
        responses: Vec<(ServerInstance, ServerResponse)>,
        group_by_columns: &[usize],
        aggregations: &[(usize, AggregationFunction)],
        num_servers_queried: usize,
    ) -> Result<BrokerResponse> {
        let start = Instant::now();
        let mut broker_response = BrokerResponse::new();

        broker_response.num_servers_queried = num_servers_queried;

        // Extract successful responses
        let successful: Vec<_> = responses
            .into_iter()
            .filter(|(_, resp)| resp.is_success())
            .collect();

        broker_response.num_servers_responded = successful.len();

        // Extract data tables
        let mut data_tables: Vec<DataTable> = Vec::new();
        for (_, resp) in successful {
            if let Some(dt) = resp.data_table {
                data_tables.push(dt);
            }
        }

        if data_tables.is_empty() {
            broker_response.broker_reduce_time_ms = start.elapsed().as_millis() as u64;
            return Ok(broker_response);
        }

        // Perform aggregation
        let schema = data_tables[0].schema.clone();
        let aggregator = Aggregator::new(group_by_columns.to_vec(), aggregations.to_vec());

        let result_rows = aggregator.aggregate(data_tables)?;

        // Apply limit
        let limited_rows = if result_rows.len() > self.config.max_rows {
            result_rows.into_iter().take(self.config.max_rows).collect()
        } else {
            result_rows
        };

        broker_response.result_table = Some(ResultTable::new(schema, limited_rows));
        broker_response.broker_reduce_time_ms = start.elapsed().as_millis() as u64;

        Ok(broker_response)
    }

    /// Reduce with ORDER BY.
    pub fn reduce_with_orderby(
        &self,
        responses: Vec<(ServerInstance, ServerResponse)>,
        order_by_columns: &[(usize, bool)], // (column_index, is_ascending)
        limit: usize,
        num_servers_queried: usize,
    ) -> Result<BrokerResponse> {
        let start = Instant::now();

        // First do basic reduce
        let mut broker_response = self.reduce(responses, num_servers_queried)?;

        // Apply ordering
        if let Some(ref mut result_table) = broker_response.result_table {
            self.sort_rows(&mut result_table.rows, order_by_columns);

            // Apply limit
            if result_table.rows.len() > limit {
                result_table.rows.truncate(limit);
            }
        }

        broker_response.broker_reduce_time_ms = start.elapsed().as_millis() as u64;
        Ok(broker_response)
    }

    fn sort_rows(&self, rows: &mut Vec<Vec<DataValue>>, order_by: &[(usize, bool)]) {
        rows.sort_by(|a, b| {
            for (col_idx, ascending) in order_by {
                let cmp = self.compare_values(&a[*col_idx], &b[*col_idx]);
                if cmp != std::cmp::Ordering::Equal {
                    return if *ascending { cmp } else { cmp.reverse() };
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    fn compare_values(&self, a: &DataValue, b: &DataValue) -> std::cmp::Ordering {
        match (a, b) {
            (DataValue::Null, DataValue::Null) => std::cmp::Ordering::Equal,
            (DataValue::Null, _) => std::cmp::Ordering::Less,
            (_, DataValue::Null) => std::cmp::Ordering::Greater,
            (DataValue::Int(x), DataValue::Int(y)) => x.cmp(y),
            (DataValue::Long(x), DataValue::Long(y)) => x.cmp(y),
            (DataValue::Float(x), DataValue::Float(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::Double(x), DataValue::Double(y)) => {
                x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
            }
            (DataValue::String(x), DataValue::String(y)) => x.cmp(y),
            (DataValue::Boolean(x), DataValue::Boolean(y)) => x.cmp(y),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

impl Default for BrokerReduceService {
    fn default() -> Self {
        Self::new(ReduceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDataType, TableType};

    fn create_server(name: &str) -> ServerInstance {
        ServerInstance::new(name.to_string(), 8099, TableType::Offline)
    }

    fn create_schema() -> DataSchema {
        DataSchema::new(
            vec!["id".to_string(), "name".to_string(), "value".to_string()],
            vec![ColumnDataType::Long, ColumnDataType::String, ColumnDataType::Double],
        )
    }

    fn create_data_table(rows: Vec<Vec<DataValue>>) -> DataTable {
        let mut dt = DataTable::new(create_schema());
        for row in rows {
            dt.add_row(row);
        }
        dt
    }

    fn create_response(server: ServerInstance, rows: Vec<Vec<DataValue>>) -> (ServerInstance, ServerResponse) {
        let dt = create_data_table(rows);
        (server.clone(), ServerResponse::success(server, dt, 100))
    }

    #[test]
    fn test_reduce_single_response() {
        let service = BrokerReduceService::default();

        let rows = vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string()), DataValue::Double(1.0)],
            vec![DataValue::Long(2), DataValue::String("b".to_string()), DataValue::Double(2.0)],
        ];

        let responses = vec![create_response(create_server("host1"), rows)];

        let result = service.reduce(responses, 1).unwrap();

        assert_eq!(result.num_servers_queried, 1);
        assert_eq!(result.num_servers_responded, 1);
        assert!(result.result_table.is_some());
        assert_eq!(result.result_table.unwrap().num_rows(), 2);
    }

    #[test]
    fn test_reduce_multiple_responses() {
        let service = BrokerReduceService::default();

        let rows1 = vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string()), DataValue::Double(1.0)],
        ];
        let rows2 = vec![
            vec![DataValue::Long(2), DataValue::String("b".to_string()), DataValue::Double(2.0)],
        ];

        let responses = vec![
            create_response(create_server("host1"), rows1),
            create_response(create_server("host2"), rows2),
        ];

        let result = service.reduce(responses, 2).unwrap();

        assert_eq!(result.num_servers_queried, 2);
        assert_eq!(result.num_servers_responded, 2);
        assert_eq!(result.result_table.unwrap().num_rows(), 2);
    }

    #[test]
    fn test_reduce_with_failed_server() {
        let service = BrokerReduceService::default();

        let rows = vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string()), DataValue::Double(1.0)],
        ];

        let responses = vec![
            create_response(create_server("host1"), rows),
            (
                create_server("host2"),
                ServerResponse::error(create_server("host2"), BrokerError::Timeout(1000)),
            ),
        ];

        let result = service.reduce(responses, 2).unwrap();

        assert_eq!(result.num_servers_queried, 2);
        assert_eq!(result.num_servers_responded, 1);
        assert_eq!(result.exceptions.len(), 1);
    }

    #[test]
    fn test_reduce_with_orderby() {
        let service = BrokerReduceService::default();

        let rows1 = vec![
            vec![DataValue::Long(3), DataValue::String("c".to_string()), DataValue::Double(3.0)],
            vec![DataValue::Long(1), DataValue::String("a".to_string()), DataValue::Double(1.0)],
        ];
        let rows2 = vec![
            vec![DataValue::Long(2), DataValue::String("b".to_string()), DataValue::Double(2.0)],
        ];

        let responses = vec![
            create_response(create_server("host1"), rows1),
            create_response(create_server("host2"), rows2),
        ];

        let result = service
            .reduce_with_orderby(responses, &[(0, true)], 10, 2)
            .unwrap();

        let table = result.result_table.unwrap();
        assert_eq!(table.num_rows(), 3);

        // Check ordering (ascending by first column)
        if let DataValue::Long(v) = &table.rows[0][0] {
            assert_eq!(*v, 1);
        }
        if let DataValue::Long(v) = &table.rows[1][0] {
            assert_eq!(*v, 2);
        }
        if let DataValue::Long(v) = &table.rows[2][0] {
            assert_eq!(*v, 3);
        }
    }

    #[test]
    fn test_reduce_with_limit() {
        let config = ReduceConfig {
            max_rows: 2,
            ..Default::default()
        };
        let service = BrokerReduceService::new(config);

        let rows = vec![
            vec![DataValue::Long(1), DataValue::String("a".to_string()), DataValue::Double(1.0)],
            vec![DataValue::Long(2), DataValue::String("b".to_string()), DataValue::Double(2.0)],
            vec![DataValue::Long(3), DataValue::String("c".to_string()), DataValue::Double(3.0)],
        ];

        let responses = vec![create_response(create_server("host1"), rows)];

        let result = service.reduce(responses, 1).unwrap();

        assert_eq!(result.result_table.unwrap().num_rows(), 2);
    }

    #[test]
    fn test_empty_responses() {
        let service = BrokerReduceService::default();

        let result = service.reduce(vec![], 0).unwrap();

        assert_eq!(result.num_servers_queried, 0);
        assert_eq!(result.num_servers_responded, 0);
        assert!(result.result_table.is_none());
    }
}
