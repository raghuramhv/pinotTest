//! Pinot Query Server - High-Performance Rust Implementation
//!
//! This crate provides a high-performance scatter-gather query server implementation
//! with advanced memory backpressure management for Apache Pinot.
//!
//! # Architecture
//!
//! The query server implements a multi-stage pipelined execution model:
//!
//! 1. **Mailbox System**: Bounded async channels for inter-process communication
//! 2. **Block Exchange**: Efficient data block distribution with hash/broadcast/random partitioning
//! 3. **Memory Backpressure**: Lock-free bounded queues with configurable limits
//! 4. **gRPC Transport**: High-performance streaming with connection pooling
//! 5. **Operator Pipeline**: Composable operators for query execution
//!
//! # Performance Features
//!
//! - Zero-copy data passing where possible
//! - Lock-free bounded queues for backpressure
//! - Efficient gRPC channel pooling with idle timeout
//! - Streaming block serialization/deserialization
//! - Memory-aware scheduling
//! - SIMD-accelerated aggregations (from pinot-core-rust)

#![allow(dead_code)]

// Use jemalloc for better memory management
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub mod backpressure;
pub mod block;
pub mod channel;
pub mod config;
pub mod error;
pub mod exchange;
pub mod jni;
pub mod mailbox;
pub mod metrics;
pub mod operator;
pub mod scheduler;

// Re-exports
pub use backpressure::MemoryBackpressureManager;
pub use block::{DataBlock, MseBlock, SerializedBlock};
pub use channel::ChannelManager;
pub use config::QueryServerConfig;
pub use error::{QueryError, Result};
pub use exchange::{BlockExchange, ExchangeType};
pub use mailbox::{MailboxId, MailboxService, ReceivingMailbox, SendingMailbox};
pub use metrics::QueryMetrics;
pub use operator::{OpChain, Operator};
pub use scheduler::QueryScheduler;

/// Query server version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default maximum block size for network transmission (4MB)
pub const MAX_BLOCK_SIZE_BYTES: usize = 4 * 1024 * 1024;

/// Default maximum pending blocks in mailbox queue
pub const DEFAULT_MAX_PENDING_BLOCKS: usize = 5;

/// Default mailbox expiry time in seconds
pub const DEFAULT_MAILBOX_EXPIRY_SECONDS: u64 = 300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(MAX_BLOCK_SIZE_BYTES, 4 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_PENDING_BLOCKS, 5);
        assert_eq!(DEFAULT_MAILBOX_EXPIRY_SECONDS, 300);
    }
}
