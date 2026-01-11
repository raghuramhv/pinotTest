//! Error types for the query server

use thiserror::Error;

/// Result type alias using QueryError
pub type Result<T> = std::result::Result<T, QueryError>;

/// Query server errors
#[derive(Error, Debug)]
pub enum QueryError {
    /// Mailbox-related errors
    #[error("Mailbox error: {0}")]
    Mailbox(String),

    /// Mailbox is full (backpressure)
    #[error("Mailbox full: {mailbox_id}, pending blocks: {pending}")]
    MailboxFull { mailbox_id: String, pending: usize },

    /// Mailbox timeout
    #[error("Mailbox timeout after {timeout_ms}ms: {mailbox_id}")]
    MailboxTimeout { mailbox_id: String, timeout_ms: u64 },

    /// Mailbox not found
    #[error("Mailbox not found: {0}")]
    MailboxNotFound(String),

    /// Mailbox already closed
    #[error("Mailbox already closed: {0}")]
    MailboxClosed(String),

    /// Channel-related errors
    #[error("Channel error: {0}")]
    Channel(String),

    /// Connection failed
    #[error("Connection failed to {host}:{port}: {reason}")]
    ConnectionFailed {
        host: String,
        port: u16,
        reason: String,
    },

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization errors
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Memory pressure errors
    #[error("Memory pressure: used {used_bytes} bytes, limit {limit_bytes} bytes")]
    MemoryPressure { used_bytes: usize, limit_bytes: usize },

    /// Query timeout
    #[error("Query timeout after {timeout_ms}ms: {query_id}")]
    QueryTimeout { query_id: String, timeout_ms: u64 },

    /// Query cancelled
    #[error("Query cancelled: {query_id}, reason: {reason}")]
    QueryCancelled { query_id: String, reason: String },

    /// Operator errors
    #[error("Operator error in {operator}: {message}")]
    Operator { operator: String, message: String },

    /// Exchange errors
    #[error("Exchange error: {0}")]
    Exchange(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// gRPC transport error
    #[error("gRPC error: {0}")]
    Grpc(String),

    /// Early termination signal
    #[error("Early termination: {0}")]
    EarlyTermination(String),
}

impl QueryError {
    /// Check if this error indicates backpressure
    pub fn is_backpressure(&self) -> bool {
        matches!(
            self,
            QueryError::MailboxFull { .. }
                | QueryError::MailboxTimeout { .. }
                | QueryError::MemoryPressure { .. }
        )
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            QueryError::MailboxTimeout { .. }
                | QueryError::ConnectionFailed { .. }
                | QueryError::MemoryPressure { .. }
        )
    }

    /// Check if this is a cancellation
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            QueryError::QueryCancelled { .. } | QueryError::EarlyTermination(_)
        )
    }
}

impl From<tonic::Status> for QueryError {
    fn from(status: tonic::Status) -> Self {
        QueryError::Grpc(status.message().to_string())
    }
}

impl From<tonic::transport::Error> for QueryError {
    fn from(err: tonic::transport::Error) -> Self {
        QueryError::Grpc(err.to_string())
    }
}

impl From<bincode::Error> for QueryError {
    fn from(err: bincode::Error) -> Self {
        QueryError::Serialization(err.to_string())
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for QueryError {
    fn from(err: crossbeam_channel::SendError<T>) -> Self {
        QueryError::Mailbox(format!("Send error: {}", err))
    }
}

impl From<crossbeam_channel::RecvError> for QueryError {
    fn from(err: crossbeam_channel::RecvError) -> Self {
        QueryError::Mailbox(format!("Receive error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = QueryError::MailboxFull {
            mailbox_id: "test-123".to_string(),
            pending: 5,
        };
        assert!(err.to_string().contains("test-123"));
        assert!(err.is_backpressure());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_error_categories() {
        let timeout = QueryError::MailboxTimeout {
            mailbox_id: "test".to_string(),
            timeout_ms: 1000,
        };
        assert!(timeout.is_backpressure());
        assert!(timeout.is_retryable());

        let cancelled = QueryError::QueryCancelled {
            query_id: "q1".to_string(),
            reason: "user request".to_string(),
        };
        assert!(cancelled.is_cancelled());
        assert!(!cancelled.is_retryable());
    }
}
