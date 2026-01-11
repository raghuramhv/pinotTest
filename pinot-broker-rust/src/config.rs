//! Broker configuration.

use crate::{
    DEFAULT_CONNECTIONS_PER_SERVER, DEFAULT_CONNECTION_TIMEOUT_MS, DEFAULT_MAX_CONCURRENT_QUERIES,
    DEFAULT_MAX_RESPONSE_SIZE, DEFAULT_QUERY_TIMEOUT_MS, DEFAULT_REDUCE_THREADS,
};
use serde::{Deserialize, Serialize};

/// Broker configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerConfig {
    /// Query timeout in milliseconds
    pub query_timeout_ms: u64,

    /// Connection timeout in milliseconds
    pub connection_timeout_ms: u64,

    /// Maximum response size in bytes
    pub max_response_size: usize,

    /// Number of connections per server
    pub connections_per_server: usize,

    /// Number of threads for reduce operations
    pub reduce_threads: usize,

    /// Maximum concurrent queries
    pub max_concurrent_queries: usize,

    /// Enable adaptive server selection
    pub enable_adaptive_server_selection: bool,

    /// Enable TLS for server connections
    pub enable_tls: bool,

    /// Routing configuration
    pub routing: RoutingConfig,

    /// Reduce configuration
    pub reduce: ReduceConfig,

    /// Metrics configuration
    pub metrics: MetricsConfig,
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            query_timeout_ms: DEFAULT_QUERY_TIMEOUT_MS,
            connection_timeout_ms: DEFAULT_CONNECTION_TIMEOUT_MS,
            max_response_size: DEFAULT_MAX_RESPONSE_SIZE,
            connections_per_server: DEFAULT_CONNECTIONS_PER_SERVER,
            reduce_threads: DEFAULT_REDUCE_THREADS,
            max_concurrent_queries: DEFAULT_MAX_CONCURRENT_QUERIES,
            enable_adaptive_server_selection: true,
            enable_tls: false,
            routing: RoutingConfig::default(),
            reduce: ReduceConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl BrokerConfig {
    /// Create a new builder for BrokerConfig.
    pub fn builder() -> BrokerConfigBuilder {
        BrokerConfigBuilder::default()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> crate::Result<()> {
        if self.query_timeout_ms == 0 {
            return Err(crate::BrokerError::Configuration(
                "query_timeout_ms must be > 0".to_string(),
            ));
        }
        if self.connection_timeout_ms == 0 {
            return Err(crate::BrokerError::Configuration(
                "connection_timeout_ms must be > 0".to_string(),
            ));
        }
        if self.max_response_size == 0 {
            return Err(crate::BrokerError::Configuration(
                "max_response_size must be > 0".to_string(),
            ));
        }
        if self.reduce_threads == 0 {
            return Err(crate::BrokerError::Configuration(
                "reduce_threads must be > 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Builder for BrokerConfig.
#[derive(Debug, Default)]
pub struct BrokerConfigBuilder {
    config: BrokerConfig,
}

impl BrokerConfigBuilder {
    pub fn query_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.query_timeout_ms = timeout;
        self
    }

    pub fn connection_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.connection_timeout_ms = timeout;
        self
    }

    pub fn max_response_size(mut self, size: usize) -> Self {
        self.config.max_response_size = size;
        self
    }

    pub fn connections_per_server(mut self, count: usize) -> Self {
        self.config.connections_per_server = count;
        self
    }

    pub fn reduce_threads(mut self, count: usize) -> Self {
        self.config.reduce_threads = count;
        self
    }

    pub fn max_concurrent_queries(mut self, count: usize) -> Self {
        self.config.max_concurrent_queries = count;
        self
    }

    pub fn enable_adaptive_server_selection(mut self, enable: bool) -> Self {
        self.config.enable_adaptive_server_selection = enable;
        self
    }

    pub fn enable_tls(mut self, enable: bool) -> Self {
        self.config.enable_tls = enable;
        self
    }

    pub fn routing(mut self, routing: RoutingConfig) -> Self {
        self.config.routing = routing;
        self
    }

    pub fn reduce(mut self, reduce: ReduceConfig) -> Self {
        self.config.reduce = reduce;
        self
    }

    pub fn build(self) -> crate::Result<BrokerConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

/// Routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Instance selector type
    pub instance_selector_type: InstanceSelectorType,

    /// Enable partition metadata manager for partition-based pruning
    pub enable_partition_metadata: bool,

    /// Parallelism for routing assignment changes
    pub assignment_change_parallelism: usize,

    /// Time in seconds before marking a segment as old (for new segment handling)
    pub new_segment_expiry_seconds: u64,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            instance_selector_type: InstanceSelectorType::Balanced,
            enable_partition_metadata: true,
            assignment_change_parallelism: 4,
            new_segment_expiry_seconds: 300, // 5 minutes
        }
    }
}

/// Instance selector type for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceSelectorType {
    /// Balanced round-robin selection
    Balanced,
    /// Replica group based selection
    ReplicaGroup,
    /// Strict replica group selection
    StrictReplicaGroup,
    /// Adaptive selection based on metrics
    Adaptive,
}

/// Reduce configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReduceConfig {
    /// Maximum number of rows in result
    pub max_rows_in_result: usize,

    /// Group-by trim threshold
    pub groupby_trim_threshold: usize,

    /// Minimum segment group trim size
    pub min_segment_group_trim_size: usize,

    /// Enable parallel reduce
    pub enable_parallel_reduce: bool,

    /// Enable streaming reduce for gRPC
    pub enable_streaming_reduce: bool,
}

impl Default for ReduceConfig {
    fn default() -> Self {
        Self {
            max_rows_in_result: 100_000,
            groupby_trim_threshold: 1_000_000,
            min_segment_group_trim_size: 5000,
            enable_parallel_reduce: true,
            enable_streaming_reduce: false,
        }
    }
}

/// Metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,

    /// Metrics prefix
    pub prefix: String,

    /// Enable latency histograms
    pub enable_histograms: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix: "pinot_broker".to_string(),
            enable_histograms: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BrokerConfig::default();
        assert_eq!(config.query_timeout_ms, DEFAULT_QUERY_TIMEOUT_MS);
        assert_eq!(config.connection_timeout_ms, DEFAULT_CONNECTION_TIMEOUT_MS);
        assert!(config.enable_adaptive_server_selection);
    }

    #[test]
    fn test_builder() {
        let config = BrokerConfig::builder()
            .query_timeout_ms(30000)
            .connection_timeout_ms(10000)
            .max_concurrent_queries(500)
            .build()
            .unwrap();

        assert_eq!(config.query_timeout_ms, 30000);
        assert_eq!(config.connection_timeout_ms, 10000);
        assert_eq!(config.max_concurrent_queries, 500);
    }

    #[test]
    fn test_validation() {
        let result = BrokerConfig::builder().query_timeout_ms(0).build();
        assert!(result.is_err());

        let result = BrokerConfig::builder().reduce_threads(0).build();
        assert!(result.is_err());
    }

    #[test]
    fn test_routing_config() {
        let config = RoutingConfig::default();
        assert_eq!(config.instance_selector_type, InstanceSelectorType::Balanced);
        assert!(config.enable_partition_metadata);
    }
}
