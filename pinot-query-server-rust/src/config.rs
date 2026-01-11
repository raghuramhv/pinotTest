//! Query server configuration

use crate::{DEFAULT_MAILBOX_EXPIRY_SECONDS, DEFAULT_MAX_PENDING_BLOCKS, MAX_BLOCK_SIZE_BYTES};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Query server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryServerConfig {
    /// Server host binding
    pub host: String,

    /// Server port
    pub port: u16,

    /// Maximum pending blocks in mailbox queue (backpressure threshold)
    pub max_pending_blocks: usize,

    /// Maximum block size for network transmission in bytes
    pub max_block_size_bytes: usize,

    /// Mailbox expiry time
    pub mailbox_expiry: Duration,

    /// Query timeout
    pub query_timeout: Duration,

    /// gRPC configuration
    pub grpc: GrpcConfig,

    /// Memory configuration
    pub memory: MemoryConfig,

    /// Thread pool configuration
    pub thread_pool: ThreadPoolConfig,

    /// Metrics configuration
    pub metrics: MetricsConfig,
}

/// gRPC configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    /// Maximum inbound message size in bytes
    pub max_inbound_message_size: usize,

    /// Maximum outbound message size in bytes
    pub max_outbound_message_size: usize,

    /// Keep-alive interval
    pub keep_alive_interval: Duration,

    /// Keep-alive timeout
    pub keep_alive_timeout: Duration,

    /// Connection idle timeout
    pub idle_timeout: Duration,

    /// Enable TLS
    pub tls_enabled: bool,

    /// TLS certificate path
    pub tls_cert_path: Option<String>,

    /// TLS key path
    pub tls_key_path: Option<String>,

    /// Maximum concurrent streams per connection
    pub max_concurrent_streams: u32,

    /// Initial connection window size
    pub initial_connection_window_size: u32,

    /// Initial stream window size
    pub initial_stream_window_size: u32,
}

/// Memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum memory usage in bytes (0 = unlimited)
    pub max_memory_bytes: usize,

    /// Memory pressure threshold (0.0 - 1.0)
    pub pressure_threshold: f64,

    /// Critical memory threshold (0.0 - 1.0)
    pub critical_threshold: f64,

    /// Enable memory tracking
    pub tracking_enabled: bool,

    /// Memory check interval
    pub check_interval: Duration,

    /// Enable aggressive GC under pressure
    pub aggressive_gc_enabled: bool,
}

/// Thread pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadPoolConfig {
    /// Number of worker threads (0 = auto-detect based on CPU cores)
    pub worker_threads: usize,

    /// Maximum blocking threads
    pub max_blocking_threads: usize,

    /// Thread stack size in bytes
    pub stack_size: usize,

    /// Enable work stealing
    pub work_stealing_enabled: bool,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,

    /// Metrics export port
    pub export_port: u16,

    /// Histogram buckets for latency
    pub latency_buckets: Vec<f64>,

    /// Enable detailed per-query metrics
    pub per_query_metrics: bool,
}

impl Default for QueryServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8099,
            max_pending_blocks: DEFAULT_MAX_PENDING_BLOCKS,
            max_block_size_bytes: MAX_BLOCK_SIZE_BYTES,
            mailbox_expiry: Duration::from_secs(DEFAULT_MAILBOX_EXPIRY_SECONDS),
            query_timeout: Duration::from_secs(300),
            grpc: GrpcConfig::default(),
            memory: MemoryConfig::default(),
            thread_pool: ThreadPoolConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            max_inbound_message_size: 64 * 1024 * 1024, // 64MB
            max_outbound_message_size: 64 * 1024 * 1024,
            keep_alive_interval: Duration::from_secs(60),
            keep_alive_timeout: Duration::from_secs(20),
            idle_timeout: Duration::from_secs(365 * 24 * 60 * 60), // 1 year (effectively disable)
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            max_concurrent_streams: 1000,
            initial_connection_window_size: 64 * 1024 * 1024, // 64MB
            initial_stream_window_size: 16 * 1024 * 1024,     // 16MB
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 0, // Unlimited by default
            pressure_threshold: 0.7,
            critical_threshold: 0.9,
            tracking_enabled: true,
            check_interval: Duration::from_millis(100),
            aggressive_gc_enabled: true,
        }
    }
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        let num_cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);

        Self {
            worker_threads: num_cpus * 2,
            max_blocking_threads: 512,
            stack_size: 2 * 1024 * 1024, // 2MB
            work_stealing_enabled: true,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            export_port: 9090,
            latency_buckets: vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
            per_query_metrics: false,
        }
    }
}

impl QueryServerConfig {
    /// Create a new configuration builder
    pub fn builder() -> QueryServerConfigBuilder {
        QueryServerConfigBuilder::default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pending_blocks == 0 {
            return Err("max_pending_blocks must be > 0".to_string());
        }

        if self.max_block_size_bytes == 0 {
            return Err("max_block_size_bytes must be > 0".to_string());
        }

        if self.memory.pressure_threshold >= self.memory.critical_threshold {
            return Err("pressure_threshold must be < critical_threshold".to_string());
        }

        if self.memory.critical_threshold > 1.0 {
            return Err("critical_threshold must be <= 1.0".to_string());
        }

        Ok(())
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(host) = std::env::var("PINOT_QUERY_SERVER_HOST") {
            config.host = host;
        }

        if let Ok(port) = std::env::var("PINOT_QUERY_SERVER_PORT") {
            if let Ok(p) = port.parse() {
                config.port = p;
            }
        }

        if let Ok(max_blocks) = std::env::var("PINOT_MAX_PENDING_BLOCKS") {
            if let Ok(n) = max_blocks.parse() {
                config.max_pending_blocks = n;
            }
        }

        if let Ok(max_mem) = std::env::var("PINOT_MAX_MEMORY_BYTES") {
            if let Ok(n) = max_mem.parse() {
                config.memory.max_memory_bytes = n;
            }
        }

        config
    }
}

/// Builder for QueryServerConfig
#[derive(Default)]
pub struct QueryServerConfigBuilder {
    config: QueryServerConfig,
}

impl QueryServerConfigBuilder {
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    pub fn max_pending_blocks(mut self, n: usize) -> Self {
        self.config.max_pending_blocks = n;
        self
    }

    pub fn max_block_size_bytes(mut self, n: usize) -> Self {
        self.config.max_block_size_bytes = n;
        self
    }

    pub fn max_memory_bytes(mut self, n: usize) -> Self {
        self.config.memory.max_memory_bytes = n;
        self
    }

    pub fn query_timeout(mut self, timeout: Duration) -> Self {
        self.config.query_timeout = timeout;
        self
    }

    pub fn build(self) -> Result<QueryServerConfig, String> {
        self.config.validate()?;
        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = QueryServerConfig::default();
        assert_eq!(config.max_pending_blocks, 5);
        assert_eq!(config.max_block_size_bytes, 4 * 1024 * 1024);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_builder() {
        let config = QueryServerConfig::builder()
            .host("localhost")
            .port(9000)
            .max_pending_blocks(10)
            .build()
            .unwrap();

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 9000);
        assert_eq!(config.max_pending_blocks, 10);
    }

    #[test]
    fn test_validation() {
        let mut config = QueryServerConfig::default();
        config.max_pending_blocks = 0;
        assert!(config.validate().is_err());

        config.max_pending_blocks = 5;
        config.memory.pressure_threshold = 0.95;
        config.memory.critical_threshold = 0.9;
        assert!(config.validate().is_err());
    }
}
