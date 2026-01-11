//! Error types for the Pinot broker.

use std::fmt;
use thiserror::Error;

/// Result type for broker operations.
pub type Result<T> = std::result::Result<T, BrokerError>;

/// Errors that can occur in broker operations.
#[derive(Error, Debug)]
pub enum BrokerError {
    /// Query timeout
    #[error("Query timed out after {0}ms")]
    Timeout(u64),

    /// No servers available for query
    #[error("No servers available for table '{0}'")]
    NoServersAvailable(String),

    /// All servers failed
    #[error("All {0} servers failed to respond")]
    AllServersFailed(usize),

    /// Partial server failure
    #[error("Partial failure: {succeeded} of {total} servers responded")]
    PartialFailure { succeeded: usize, total: usize },

    /// Routing error
    #[error("Routing error: {0}")]
    Routing(String),

    /// Segment not found
    #[error("Segment not found: {0}")]
    SegmentNotFound(String),

    /// Table not found
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// Schema mismatch between servers
    #[error("Schema mismatch from server {server}: {details}")]
    SchemaMismatch { server: String, details: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Connection error
    #[error("Connection error to {server}: {details}")]
    Connection { server: String, details: String },

    /// Server error
    #[error("Server error from {server}: {message}")]
    ServerError { server: String, message: String },

    /// Query cancelled
    #[error("Query {0} was cancelled")]
    Cancelled(u64),

    /// Invalid query
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Authorization error
    #[error("Authorization failed: {0}")]
    Authorization(String),

    /// Resource exhausted
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Reduce error during result merging
    #[error("Reduce error: {0}")]
    Reduce(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl BrokerError {
    /// Returns true if this error is retriable.
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            BrokerError::Timeout(_)
                | BrokerError::Connection { .. }
                | BrokerError::PartialFailure { .. }
        )
    }

    /// Returns true if this is a client error (4xx equivalent).
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            BrokerError::InvalidQuery(_)
                | BrokerError::Authorization(_)
                | BrokerError::TableNotFound(_)
                | BrokerError::SegmentNotFound(_)
        )
    }

    /// Returns true if this is a server error (5xx equivalent).
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            BrokerError::Internal(_)
                | BrokerError::AllServersFailed(_)
                | BrokerError::ResourceExhausted(_)
        )
    }

    /// Returns an error code for metrics/logging.
    pub fn error_code(&self) -> &'static str {
        match self {
            BrokerError::Timeout(_) => "TIMEOUT",
            BrokerError::NoServersAvailable(_) => "NO_SERVERS",
            BrokerError::AllServersFailed(_) => "ALL_FAILED",
            BrokerError::PartialFailure { .. } => "PARTIAL_FAILURE",
            BrokerError::Routing(_) => "ROUTING_ERROR",
            BrokerError::SegmentNotFound(_) => "SEGMENT_NOT_FOUND",
            BrokerError::TableNotFound(_) => "TABLE_NOT_FOUND",
            BrokerError::SchemaMismatch { .. } => "SCHEMA_MISMATCH",
            BrokerError::Serialization(_) => "SERIALIZATION_ERROR",
            BrokerError::Deserialization(_) => "DESERIALIZATION_ERROR",
            BrokerError::Connection { .. } => "CONNECTION_ERROR",
            BrokerError::ServerError { .. } => "SERVER_ERROR",
            BrokerError::Cancelled(_) => "CANCELLED",
            BrokerError::InvalidQuery(_) => "INVALID_QUERY",
            BrokerError::Authorization(_) => "AUTHORIZATION_ERROR",
            BrokerError::ResourceExhausted(_) => "RESOURCE_EXHAUSTED",
            BrokerError::Configuration(_) => "CONFIGURATION_ERROR",
            BrokerError::Internal(_) => "INTERNAL_ERROR",
            BrokerError::Reduce(_) => "REDUCE_ERROR",
            BrokerError::Io(_) => "IO_ERROR",
            BrokerError::Json(_) => "JSON_ERROR",
        }
    }
}

/// Query processing exception from a server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryProcessingException {
    /// Error code
    pub error_code: i32,
    /// Error message
    pub message: String,
    /// Server that generated the exception
    pub server: Option<String>,
}

impl fmt::Display for QueryProcessingException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref server) = self.server {
            write!(f, "[{}] Error {}: {}", server, self.error_code, self.message)
        } else {
            write!(f, "Error {}: {}", self.error_code, self.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BrokerError::Timeout(5000);
        assert_eq!(err.to_string(), "Query timed out after 5000ms");

        let err = BrokerError::NoServersAvailable("myTable".to_string());
        assert_eq!(err.to_string(), "No servers available for table 'myTable'");
    }

    #[test]
    fn test_error_categories() {
        assert!(BrokerError::Timeout(1000).is_retriable());
        assert!(BrokerError::Connection {
            server: "s1".to_string(),
            details: "refused".to_string()
        }
        .is_retriable());

        assert!(BrokerError::InvalidQuery("bad".to_string()).is_client_error());
        assert!(BrokerError::Authorization("denied".to_string()).is_client_error());

        assert!(BrokerError::Internal("oops".to_string()).is_server_error());
        assert!(BrokerError::AllServersFailed(5).is_server_error());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(BrokerError::Timeout(1000).error_code(), "TIMEOUT");
        assert_eq!(
            BrokerError::NoServersAvailable("t".to_string()).error_code(),
            "NO_SERVERS"
        );
    }

    #[test]
    fn test_query_processing_exception() {
        let exc = QueryProcessingException {
            error_code: 500,
            message: "Internal error".to_string(),
            server: Some("server1".to_string()),
        };
        assert!(exc.to_string().contains("server1"));
        assert!(exc.to_string().contains("500"));
    }
}
