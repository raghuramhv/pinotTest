//! Query router for scatter-gather operations.

use crate::routing::RoutingManager;
use crate::transport::async_response::AsyncQueryResponse;
use crate::transport::server_channel::{ChannelConfig, ServerChannels};
use crate::types::{BrokerRequest, InstanceRequest, RoutingTable, ServerInstance, ServerResponse};
use crate::{BrokerError, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Query router that handles scatter-gather operations.
pub struct QueryRouter {
    /// Server channels for plain connections
    channels: Arc<ServerChannels>,
    /// Server channels for TLS connections
    tls_channels: Option<Arc<ServerChannels>>,
    /// Broker ID
    broker_id: String,
    /// Request ID generator
    request_id_counter: AtomicU64,
    /// Maximum concurrent queries
    max_concurrent_queries: usize,
    /// Concurrent query semaphore
    query_semaphore: Semaphore,
    /// Default timeout
    default_timeout: Duration,
}

impl QueryRouter {
    pub fn new(broker_id: String, config: QueryRouterConfig) -> Self {
        let channel_config = ChannelConfig {
            connect_timeout: config.connect_timeout,
            read_timeout: config.query_timeout,
            write_timeout: config.connect_timeout,
            tcp_nodelay: true,
            max_in_flight: config.max_in_flight_per_server,
            enable_tls: false,
        };

        let channels = Arc::new(ServerChannels::new(channel_config.clone()));

        let tls_channels = if config.enable_tls {
            let mut tls_config = channel_config;
            tls_config.enable_tls = true;
            Some(Arc::new(ServerChannels::new(tls_config)))
        } else {
            None
        };

        Self {
            channels,
            tls_channels,
            broker_id,
            request_id_counter: AtomicU64::new(1),
            max_concurrent_queries: config.max_concurrent_queries,
            query_semaphore: Semaphore::new(config.max_concurrent_queries),
            default_timeout: config.query_timeout,
        }
    }

    /// Submit a query to multiple servers based on routing table.
    pub async fn submit_query(
        &self,
        request: &BrokerRequest,
        routing: &RoutingTable,
        timeout: Duration,
    ) -> Result<Arc<AsyncQueryResponse>> {
        // Acquire query permit
        let _permit = self
            .query_semaphore
            .acquire()
            .await
            .map_err(|_| BrokerError::ResourceExhausted("Too many concurrent queries".to_string()))?;

        let request_id = self.next_request_id();
        let num_servers = routing.num_servers();

        if num_servers == 0 {
            return Err(BrokerError::NoServersAvailable(request.table_name.clone()));
        }

        // Create async response
        let async_response = Arc::new(AsyncQueryResponse::new(request_id, num_servers, timeout));

        // Send requests to all servers
        let futures: Vec<_> = routing
            .server_to_segments
            .iter()
            .map(|(server, segments)| {
                let instance_request = InstanceRequest {
                    request_id,
                    broker_id: self.broker_id.clone(),
                    segments: segments.segments.clone(),
                    optional_segments: segments.optional_segments.clone(),
                    enable_trace: request.query_options.enable_trace,
                    query_bytes: request.sql.as_bytes().to_vec(),
                };

                let channels = self.get_channels(server);
                let server = server.clone();
                let response = async_response.clone();

                async move {
                    match channels.send_request(&server, &instance_request).await {
                        Ok(server_response) => {
                            response.receive_response(server_response);
                        }
                        Err(e) => {
                            response.receive_error(server, e);
                        }
                    }
                }
            })
            .collect();

        // Spawn all requests concurrently
        for future in futures {
            tokio::spawn(future);
        }

        Ok(async_response)
    }

    /// Submit a query and wait for all responses.
    pub async fn submit_query_sync(
        &self,
        request: &BrokerRequest,
        routing: &RoutingTable,
        timeout: Duration,
    ) -> Result<Vec<(ServerInstance, ServerResponse)>> {
        let async_response = self.submit_query(request, routing, timeout).await?;

        // Wait for all responses
        async_response.wait_for_responses().await?;

        // Return responses
        Ok(async_response.get_responses())
    }

    fn get_channels(&self, server: &ServerInstance) -> Arc<ServerChannels> {
        if server.tls_enabled {
            self.tls_channels
                .as_ref()
                .cloned()
                .unwrap_or_else(|| self.channels.clone())
        } else {
            self.channels.clone()
        }
    }

    fn next_request_id(&self) -> u64 {
        self.request_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Cancel a query.
    pub async fn cancel_query(&self, request_id: u64) -> Result<()> {
        // In a full implementation, this would send cancel requests to all servers
        // For now, we just log the cancellation
        tracing::info!("Cancelling query {}", request_id);
        Ok(())
    }

    /// Get statistics.
    pub fn stats(&self) -> QueryRouterStats {
        let channel_stats = self.channels.all_stats();
        let tls_channel_stats = self
            .tls_channels
            .as_ref()
            .map(|c| c.all_stats())
            .unwrap_or_default();

        QueryRouterStats {
            num_channels: channel_stats.len() + tls_channel_stats.len(),
            available_permits: self.query_semaphore.available_permits(),
            max_concurrent_queries: self.max_concurrent_queries,
            total_requests: channel_stats
                .iter()
                .chain(tls_channel_stats.iter())
                .map(|s| s.total_requests)
                .sum(),
            total_bytes_sent: channel_stats
                .iter()
                .chain(tls_channel_stats.iter())
                .map(|s| s.bytes_sent)
                .sum(),
            total_bytes_received: channel_stats
                .iter()
                .chain(tls_channel_stats.iter())
                .map(|s| s.bytes_received)
                .sum(),
        }
    }
}

/// Configuration for QueryRouter.
#[derive(Debug, Clone)]
pub struct QueryRouterConfig {
    /// Query timeout
    pub query_timeout: Duration,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Maximum concurrent queries
    pub max_concurrent_queries: usize,
    /// Maximum in-flight requests per server
    pub max_in_flight_per_server: usize,
    /// Enable TLS
    pub enable_tls: bool,
}

impl Default for QueryRouterConfig {
    fn default() -> Self {
        Self {
            query_timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(5),
            max_concurrent_queries: 1000,
            max_in_flight_per_server: 100,
            enable_tls: false,
        }
    }
}

/// Query router statistics.
#[derive(Debug, Clone)]
pub struct QueryRouterStats {
    pub num_channels: usize,
    pub available_permits: usize,
    pub max_concurrent_queries: usize,
    pub total_requests: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SegmentsToQuery, TableType};
    use std::collections::HashMap;

    fn create_routing_table() -> RoutingTable {
        let mut server_to_segments = HashMap::new();
        server_to_segments.insert(
            ServerInstance::new("host1".to_string(), 8099, TableType::Offline),
            SegmentsToQuery::new(vec!["s1".to_string(), "s2".to_string()]),
        );
        server_to_segments.insert(
            ServerInstance::new("host2".to_string(), 8099, TableType::Offline),
            SegmentsToQuery::new(vec!["s3".to_string()]),
        );

        RoutingTable {
            server_to_segments,
            unavailable_segments: vec![],
            num_pruned_segments: 0,
        }
    }

    #[test]
    fn test_query_router_config_default() {
        let config = QueryRouterConfig::default();
        assert_eq!(config.query_timeout, Duration::from_secs(60));
        assert_eq!(config.max_concurrent_queries, 1000);
    }

    #[test]
    fn test_query_router_new() {
        let config = QueryRouterConfig::default();
        let router = QueryRouter::new("broker1".to_string(), config);

        assert_eq!(router.broker_id, "broker1");
        let stats = router.stats();
        assert_eq!(stats.max_concurrent_queries, 1000);
    }

    #[test]
    fn test_request_id_generation() {
        let config = QueryRouterConfig::default();
        let router = QueryRouter::new("broker1".to_string(), config);

        let id1 = router.next_request_id();
        let id2 = router.next_request_id();
        let id3 = router.next_request_id();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[tokio::test]
    async fn test_submit_query_empty_routing() {
        let config = QueryRouterConfig::default();
        let router = QueryRouter::new("broker1".to_string(), config);

        let request = BrokerRequest::new(
            1,
            "SELECT * FROM myTable".to_string(),
            "myTable".to_string(),
        );

        let routing = RoutingTable {
            server_to_segments: HashMap::new(),
            unavailable_segments: vec![],
            num_pruned_segments: 0,
        };

        let result = router
            .submit_query(&request, &routing, Duration::from_secs(10))
            .await;

        assert!(matches!(result, Err(BrokerError::NoServersAvailable(_))));
    }

    #[test]
    fn test_stats() {
        let config = QueryRouterConfig::default();
        let router = QueryRouter::new("broker1".to_string(), config);

        let stats = router.stats();
        assert_eq!(stats.num_channels, 0);
        assert_eq!(stats.available_permits, 1000);
    }
}
