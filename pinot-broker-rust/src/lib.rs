//! High-performance Rust implementation of Apache Pinot Broker components.
//!
//! This crate provides optimized implementations of:
//! - Query routing and segment selection
//! - Adaptive server selection with latency-based routing
//! - Result merging and aggregation
//! - Connection pooling with async I/O
//! - Memory-efficient data table handling
//! - Access control (RLS/CLS)
//! - Query quota management
//! - Query cancellation and timeout handling

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub mod access_control;
pub mod cancellation;
pub mod config;
pub mod error;
pub mod jni;
pub mod metrics;
pub mod quota;
pub mod reduce;
pub mod routing;
pub mod transport;
pub mod types;

pub use config::BrokerConfig;
pub use error::{BrokerError, Result};
pub use types::*;

/// Default query timeout in milliseconds
pub const DEFAULT_QUERY_TIMEOUT_MS: u64 = 60_000;

/// Default max response size in bytes (100MB)
pub const DEFAULT_MAX_RESPONSE_SIZE: usize = 100 * 1024 * 1024;

/// Default connection timeout in milliseconds
pub const DEFAULT_CONNECTION_TIMEOUT_MS: u64 = 5_000;

/// Default number of connections per server
pub const DEFAULT_CONNECTIONS_PER_SERVER: usize = 1;

/// Default reduce thread pool size
pub const DEFAULT_REDUCE_THREADS: usize = 4;

/// Maximum concurrent queries per broker
pub const DEFAULT_MAX_CONCURRENT_QUERIES: usize = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_QUERY_TIMEOUT_MS, 60_000);
        assert_eq!(DEFAULT_MAX_RESPONSE_SIZE, 100 * 1024 * 1024);
        assert_eq!(DEFAULT_CONNECTION_TIMEOUT_MS, 5_000);
    }
}
