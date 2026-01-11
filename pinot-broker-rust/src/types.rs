//! Core types for the Pinot broker.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Server instance identifier.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInstance {
    /// Hostname
    pub hostname: String,
    /// Port
    pub port: u16,
    /// Table type (OFFLINE or REALTIME)
    pub table_type: TableType,
    /// Whether TLS is enabled
    pub tls_enabled: bool,
}

impl ServerInstance {
    pub fn new(hostname: String, port: u16, table_type: TableType) -> Self {
        Self {
            hostname,
            port,
            table_type,
            tls_enabled: false,
        }
    }

    pub fn with_tls(mut self, enabled: bool) -> Self {
        self.tls_enabled = enabled;
        self
    }

    /// Get the address string.
    pub fn address(&self) -> String {
        format!("{}:{}", self.hostname, self.port)
    }
}

impl std::fmt::Display for ServerInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} ({:?})", self.hostname, self.port, self.table_type)
    }
}

/// Table type.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableType {
    Offline,
    Realtime,
}

/// Segment identifier.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentId {
    /// Table name
    pub table_name: String,
    /// Segment name
    pub segment_name: String,
}

impl SegmentId {
    pub fn new(table_name: String, segment_name: String) -> Self {
        Self {
            table_name,
            segment_name,
        }
    }
}

impl std::fmt::Display for SegmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.table_name, self.segment_name)
    }
}

/// Segments to query from a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SegmentsToQuery {
    /// Required segments
    pub segments: Vec<String>,
    /// Optional segments (for graceful degradation)
    pub optional_segments: Vec<String>,
}

impl SegmentsToQuery {
    pub fn new(segments: Vec<String>) -> Self {
        Self {
            segments,
            optional_segments: Vec::new(),
        }
    }

    pub fn with_optional(mut self, optional: Vec<String>) -> Self {
        self.optional_segments = optional;
        self
    }

    pub fn total_count(&self) -> usize {
        self.segments.len() + self.optional_segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.optional_segments.is_empty()
    }
}

/// Routing table mapping servers to segments.
#[derive(Debug, Clone, Default)]
pub struct RoutingTable {
    /// Server to segments mapping
    pub server_to_segments: HashMap<ServerInstance, SegmentsToQuery>,
    /// Unavailable segments
    pub unavailable_segments: Vec<String>,
    /// Number of segments pruned by broker
    pub num_pruned_segments: usize,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get total number of servers.
    pub fn num_servers(&self) -> usize {
        self.server_to_segments.len()
    }

    /// Get total number of segments to query.
    pub fn num_segments(&self) -> usize {
        self.server_to_segments
            .values()
            .map(|s| s.total_count())
            .sum()
    }

    /// Check if routing is empty.
    pub fn is_empty(&self) -> bool {
        self.server_to_segments.is_empty()
    }
}

/// Broker request for a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerRequest {
    /// Request ID
    pub request_id: u64,
    /// SQL query
    pub sql: String,
    /// Table name
    pub table_name: String,
    /// Query options
    pub query_options: QueryOptions,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

impl BrokerRequest {
    pub fn new(request_id: u64, sql: String, table_name: String) -> Self {
        Self {
            request_id,
            sql,
            table_name,
            query_options: QueryOptions::default(),
            timeout_ms: 60_000,
        }
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_options(mut self, options: QueryOptions) -> Self {
        self.query_options = options;
        self
    }
}

/// Query options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryOptions {
    /// Enable trace
    pub enable_trace: bool,
    /// Skip unavailable segments
    pub skip_unavailable_segments: bool,
    /// Use multi-stage engine
    pub use_multistage_engine: bool,
    /// Custom options
    pub custom: HashMap<String, String>,
}

/// Instance request sent to a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRequest {
    /// Request ID
    pub request_id: u64,
    /// Broker ID
    pub broker_id: String,
    /// Segments to query
    pub segments: Vec<String>,
    /// Optional segments
    pub optional_segments: Vec<String>,
    /// Enable trace
    pub enable_trace: bool,
    /// Serialized query
    pub query_bytes: Vec<u8>,
}

/// Data schema for query results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSchema {
    /// Column names
    pub column_names: Vec<String>,
    /// Column types
    pub column_types: Vec<ColumnDataType>,
}

impl DataSchema {
    pub fn new(column_names: Vec<String>, column_types: Vec<ColumnDataType>) -> Self {
        Self {
            column_names,
            column_types,
        }
    }

    pub fn num_columns(&self) -> usize {
        self.column_names.len()
    }

    /// Check if this schema is compatible with another.
    pub fn is_compatible(&self, other: &DataSchema) -> bool {
        self.column_names == other.column_names && self.column_types == other.column_types
    }
}

/// Column data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnDataType {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Timestamp,
    String,
    Json,
    Bytes,
    Object,
    IntArray,
    LongArray,
    FloatArray,
    DoubleArray,
    StringArray,
    BooleanArray,
    TimestampArray,
    BytesArray,
    Unknown,
}

/// A value in a data table row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataValue {
    Null,
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Boolean(bool),
    String(String),
    Bytes(Vec<u8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    FloatArray(Vec<f32>),
    DoubleArray(Vec<f64>),
    StringArray(Vec<String>),
    BooleanArray(Vec<bool>),
}

impl Eq for DataValue {}

impl std::hash::Hash for DataValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            DataValue::Null => {}
            DataValue::Int(v) => v.hash(state),
            DataValue::Long(v) => v.hash(state),
            DataValue::Float(v) => v.to_bits().hash(state),
            DataValue::Double(v) => v.to_bits().hash(state),
            DataValue::Boolean(v) => v.hash(state),
            DataValue::String(v) => v.hash(state),
            DataValue::Bytes(v) => v.hash(state),
            DataValue::IntArray(v) => v.hash(state),
            DataValue::LongArray(v) => v.hash(state),
            DataValue::FloatArray(v) => {
                for f in v {
                    f.to_bits().hash(state);
                }
            }
            DataValue::DoubleArray(v) => {
                for f in v {
                    f.to_bits().hash(state);
                }
            }
            DataValue::StringArray(v) => v.hash(state),
            DataValue::BooleanArray(v) => v.hash(state),
        }
    }
}

impl DataValue {
    /// Get the value as an i64 if possible.
    pub fn as_long(&self) -> Option<i64> {
        match self {
            DataValue::Int(v) => Some(*v as i64),
            DataValue::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// Get the value as an f64 if possible.
    pub fn as_double(&self) -> Option<f64> {
        match self {
            DataValue::Float(v) => Some(*v as f64),
            DataValue::Double(v) => Some(*v),
            DataValue::Int(v) => Some(*v as f64),
            DataValue::Long(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Check if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, DataValue::Null)
    }
}

/// Data table containing query results from a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTable {
    /// Schema
    pub schema: DataSchema,
    /// Rows
    pub rows: Vec<Vec<DataValue>>,
    /// Metadata
    pub metadata: HashMap<String, String>,
    /// Exceptions
    pub exceptions: Vec<crate::error::QueryProcessingException>,
}

impl DataTable {
    pub fn new(schema: DataSchema) -> Self {
        Self {
            schema,
            rows: Vec::new(),
            metadata: HashMap::new(),
            exceptions: Vec::new(),
        }
    }

    pub fn num_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn num_columns(&self) -> usize {
        self.schema.num_columns()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Add a row to the table.
    pub fn add_row(&mut self, row: Vec<DataValue>) {
        self.rows.push(row);
    }

    /// Get metadata value.
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Set metadata value.
    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Get number of documents scanned.
    pub fn num_docs_scanned(&self) -> u64 {
        self.metadata
            .get("numDocsScanned")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    /// Get total number of documents.
    pub fn total_docs(&self) -> u64 {
        self.metadata
            .get("totalDocs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    /// Get time used in milliseconds.
    pub fn time_used_ms(&self) -> u64 {
        self.metadata
            .get("timeUsedMs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }
}

/// Server response containing data table and timing info.
#[derive(Debug)]
pub struct ServerResponse {
    /// The server that responded
    pub server: ServerInstance,
    /// Response data table
    pub data_table: Option<DataTable>,
    /// Response size in bytes
    pub response_size: usize,
    /// Network latency in milliseconds
    pub response_delay_ms: u64,
    /// Deserialization time in milliseconds
    pub deserialization_time_ms: u64,
    /// Error if the request failed
    pub error: Option<crate::BrokerError>,
}

impl ServerResponse {
    pub fn success(server: ServerInstance, data_table: DataTable, response_size: usize) -> Self {
        Self {
            server,
            data_table: Some(data_table),
            response_size,
            response_delay_ms: 0,
            deserialization_time_ms: 0,
            error: None,
        }
    }

    pub fn error(server: ServerInstance, error: crate::BrokerError) -> Self {
        Self {
            server,
            data_table: None,
            response_size: 0,
            response_delay_ms: 0,
            deserialization_time_ms: 0,
            error: Some(error),
        }
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none() && self.data_table.is_some()
    }
}

/// Broker response returned to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerResponse {
    /// Result table
    pub result_table: Option<ResultTable>,
    /// Number of servers queried
    pub num_servers_queried: usize,
    /// Number of servers responded
    pub num_servers_responded: usize,
    /// Number of segments queried
    pub num_segments_queried: usize,
    /// Number of segments processed
    pub num_segments_processed: usize,
    /// Number of segments matched
    pub num_segments_matched: usize,
    /// Number of consuming segments queried
    pub num_consuming_segments_queried: usize,
    /// Number of documents scanned
    pub num_docs_scanned: u64,
    /// Number of entries scanned in filter
    pub num_entries_scanned_in_filter: u64,
    /// Number of entries scanned post filter
    pub num_entries_scanned_post_filter: u64,
    /// Number of groups limit reached
    pub num_groups_limit_reached: bool,
    /// Total documents
    pub total_docs: u64,
    /// Time used in milliseconds
    pub time_used_ms: u64,
    /// Broker reduce time in milliseconds
    pub broker_reduce_time_ms: u64,
    /// Exceptions from processing
    pub exceptions: Vec<crate::error::QueryProcessingException>,
    /// Trace info if enabled
    pub trace_info: Option<String>,
}

impl BrokerResponse {
    pub fn new() -> Self {
        Self {
            result_table: None,
            num_servers_queried: 0,
            num_servers_responded: 0,
            num_segments_queried: 0,
            num_segments_processed: 0,
            num_segments_matched: 0,
            num_consuming_segments_queried: 0,
            num_docs_scanned: 0,
            num_entries_scanned_in_filter: 0,
            num_entries_scanned_post_filter: 0,
            num_groups_limit_reached: false,
            total_docs: 0,
            time_used_ms: 0,
            broker_reduce_time_ms: 0,
            exceptions: Vec::new(),
            trace_info: None,
        }
    }

    pub fn has_exceptions(&self) -> bool {
        !self.exceptions.is_empty()
    }
}

impl Default for BrokerResponse {
    fn default() -> Self {
        Self::new()
    }
}

/// Result table with rows and schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultTable {
    /// Schema
    pub schema: DataSchema,
    /// Rows
    pub rows: Vec<Vec<DataValue>>,
}

impl ResultTable {
    pub fn new(schema: DataSchema, rows: Vec<Vec<DataValue>>) -> Self {
        Self { schema, rows }
    }

    pub fn num_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn num_columns(&self) -> usize {
        self.schema.num_columns()
    }
}

/// Execution statistics from servers.
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    /// Number of segments queried
    pub num_segments_queried: usize,
    /// Number of segments processed
    pub num_segments_processed: usize,
    /// Number of segments matched
    pub num_segments_matched: usize,
    /// Number of segments pruned by server
    pub num_segments_pruned_by_server: usize,
    /// Number of documents scanned
    pub num_docs_scanned: u64,
    /// Total documents
    pub total_docs: u64,
    /// Number of entries scanned in filter
    pub num_entries_scanned_in_filter: u64,
    /// Number of entries scanned post filter
    pub num_entries_scanned_post_filter: u64,
    /// Num groups limit reached
    pub num_groups_limit_reached: bool,
    /// Total server time in milliseconds
    pub total_server_time_ms: u64,
}

impl ExecutionStats {
    pub fn merge(&mut self, other: &ExecutionStats) {
        self.num_segments_queried += other.num_segments_queried;
        self.num_segments_processed += other.num_segments_processed;
        self.num_segments_matched += other.num_segments_matched;
        self.num_segments_pruned_by_server += other.num_segments_pruned_by_server;
        self.num_docs_scanned += other.num_docs_scanned;
        self.total_docs += other.total_docs;
        self.num_entries_scanned_in_filter += other.num_entries_scanned_in_filter;
        self.num_entries_scanned_post_filter += other.num_entries_scanned_post_filter;
        self.num_groups_limit_reached |= other.num_groups_limit_reached;
        self.total_server_time_ms = self.total_server_time_ms.max(other.total_server_time_ms);
    }
}

/// Query statistics for adaptive server selection.
#[derive(Debug, Clone)]
pub struct QueryStats {
    /// Server instance
    pub server: ServerInstance,
    /// Latency in milliseconds
    pub latency_ms: u64,
    /// In-flight requests
    pub in_flight_requests: usize,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Last update time
    pub last_update: Instant,
}

impl QueryStats {
    pub fn new(server: ServerInstance) -> Self {
        Self {
            server,
            latency_ms: 0,
            in_flight_requests: 0,
            success_rate: 1.0,
            last_update: Instant::now(),
        }
    }

    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.last_update.elapsed() > max_age
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_instance() {
        let server = ServerInstance::new("localhost".to_string(), 8099, TableType::Offline);
        assert_eq!(server.address(), "localhost:8099");
        assert!(!server.tls_enabled);

        let server_tls = server.clone().with_tls(true);
        assert!(server_tls.tls_enabled);
    }

    #[test]
    fn test_segments_to_query() {
        let segments = SegmentsToQuery::new(vec!["s1".to_string(), "s2".to_string()]);
        assert_eq!(segments.total_count(), 2);
        assert!(!segments.is_empty());

        let with_optional =
            segments.with_optional(vec!["opt1".to_string()]);
        assert_eq!(with_optional.total_count(), 3);
    }

    #[test]
    fn test_routing_table() {
        let mut routing = RoutingTable::new();
        assert!(routing.is_empty());

        let server = ServerInstance::new("host1".to_string(), 8099, TableType::Offline);
        routing.server_to_segments.insert(
            server,
            SegmentsToQuery::new(vec!["s1".to_string(), "s2".to_string()]),
        );

        assert_eq!(routing.num_servers(), 1);
        assert_eq!(routing.num_segments(), 2);
    }

    #[test]
    fn test_data_schema() {
        let schema = DataSchema::new(
            vec!["col1".to_string(), "col2".to_string()],
            vec![ColumnDataType::Int, ColumnDataType::String],
        );
        assert_eq!(schema.num_columns(), 2);
    }

    #[test]
    fn test_data_value() {
        let v = DataValue::Long(42);
        assert_eq!(v.as_long(), Some(42));
        assert_eq!(v.as_double(), Some(42.0));
        assert!(!v.is_null());

        let null = DataValue::Null;
        assert!(null.is_null());
    }

    #[test]
    fn test_data_table() {
        let schema = DataSchema::new(
            vec!["id".to_string()],
            vec![ColumnDataType::Long],
        );
        let mut table = DataTable::new(schema);
        table.add_row(vec![DataValue::Long(1)]);
        table.add_row(vec![DataValue::Long(2)]);

        assert_eq!(table.num_rows(), 2);
        assert_eq!(table.num_columns(), 1);
    }

    #[test]
    fn test_execution_stats_merge() {
        let mut stats1 = ExecutionStats {
            num_docs_scanned: 100,
            total_docs: 1000,
            ..Default::default()
        };
        let stats2 = ExecutionStats {
            num_docs_scanned: 200,
            total_docs: 2000,
            ..Default::default()
        };

        stats1.merge(&stats2);
        assert_eq!(stats1.num_docs_scanned, 300);
        assert_eq!(stats1.total_docs, 3000);
    }
}
