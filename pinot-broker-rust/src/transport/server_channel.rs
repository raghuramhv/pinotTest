//! Server channel for communication with Pinot servers.

use crate::types::{DataTable, InstanceRequest, ServerInstance, ServerResponse};
use crate::{BrokerError, Result};
use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

/// Configuration for server channels.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Read timeout
    pub read_timeout: Duration,
    /// Write timeout
    pub write_timeout: Duration,
    /// Enable TCP no delay
    pub tcp_nodelay: bool,
    /// Max in-flight requests per channel
    pub max_in_flight: usize,
    /// Enable TLS
    pub enable_tls: bool,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(60),
            write_timeout: Duration::from_secs(10),
            tcp_nodelay: true,
            max_in_flight: 100,
            enable_tls: false,
        }
    }
}

/// A channel to a single server.
pub struct ServerChannel {
    /// Server instance
    server: ServerInstance,
    /// Configuration
    config: ChannelConfig,
    /// Whether the channel is connected
    connected: AtomicBool,
    /// In-flight request semaphore
    in_flight_semaphore: Semaphore,
    /// Total bytes sent
    bytes_sent: AtomicU64,
    /// Total bytes received
    bytes_received: AtomicU64,
    /// Total requests
    total_requests: AtomicU64,
    /// Failed requests
    failed_requests: AtomicU64,
    /// Last activity time
    last_activity: RwLock<Instant>,
}

impl ServerChannel {
    pub fn new(server: ServerInstance, config: ChannelConfig) -> Self {
        let max_in_flight = config.max_in_flight;
        Self {
            server,
            config,
            connected: AtomicBool::new(false),
            in_flight_semaphore: Semaphore::new(max_in_flight),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            last_activity: RwLock::new(Instant::now()),
        }
    }

    /// Send a request and receive a response.
    pub async fn send_request(&self, request: &InstanceRequest) -> Result<ServerResponse> {
        let _permit = self
            .in_flight_semaphore
            .acquire()
            .await
            .map_err(|_| BrokerError::ResourceExhausted("Too many in-flight requests".to_string()))?;

        self.total_requests.fetch_add(1, Ordering::Relaxed);
        let start = Instant::now();

        // Serialize request
        let request_bytes = self.serialize_request(request)?;
        self.bytes_sent
            .fetch_add(request_bytes.len() as u64, Ordering::Relaxed);

        // Connect and send
        let result = self.send_and_receive(&request_bytes).await;

        // Update last activity
        *self.last_activity.write() = Instant::now();

        match result {
            Ok((response_bytes, data_table)) => {
                self.bytes_received
                    .fetch_add(response_bytes as u64, Ordering::Relaxed);

                let latency = start.elapsed();

                Ok(ServerResponse {
                    server: self.server.clone(),
                    data_table: Some(data_table),
                    response_size: response_bytes,
                    response_delay_ms: latency.as_millis() as u64,
                    deserialization_time_ms: 0, // Set by deserialize
                    error: None,
                })
            }
            Err(e) => {
                self.failed_requests.fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }

    async fn send_and_receive(&self, request_bytes: &[u8]) -> Result<(usize, DataTable)> {
        // Connect with timeout
        let mut stream = tokio::time::timeout(
            self.config.connect_timeout,
            TcpStream::connect(self.server.address()),
        )
        .await
        .map_err(|_| BrokerError::Connection {
            server: self.server.address(),
            details: "Connection timeout".to_string(),
        })?
        .map_err(|e| BrokerError::Connection {
            server: self.server.address(),
            details: e.to_string(),
        })?;

        if self.config.tcp_nodelay {
            stream.set_nodelay(true).ok();
        }

        self.connected.store(true, Ordering::Relaxed);

        // Write request length and data
        let len = request_bytes.len() as u32;
        tokio::time::timeout(self.config.write_timeout, async {
            stream.write_all(&len.to_be_bytes()).await?;
            stream.write_all(request_bytes).await?;
            stream.flush().await
        })
        .await
        .map_err(|_| BrokerError::Connection {
            server: self.server.address(),
            details: "Write timeout".to_string(),
        })?
        .map_err(|e| BrokerError::Connection {
            server: self.server.address(),
            details: e.to_string(),
        })?;

        // Read response length
        let mut len_buf = [0u8; 4];
        tokio::time::timeout(self.config.read_timeout, stream.read_exact(&mut len_buf))
            .await
            .map_err(|_| BrokerError::Connection {
                server: self.server.address(),
                details: "Read timeout".to_string(),
            })?
            .map_err(|e| BrokerError::Connection {
                server: self.server.address(),
                details: e.to_string(),
            })?;

        let response_len = u32::from_be_bytes(len_buf) as usize;

        // Read response data
        let mut response_buf = vec![0u8; response_len];
        tokio::time::timeout(
            self.config.read_timeout,
            stream.read_exact(&mut response_buf),
        )
        .await
        .map_err(|_| BrokerError::Connection {
            server: self.server.address(),
            details: "Read timeout".to_string(),
        })?
        .map_err(|e| BrokerError::Connection {
            server: self.server.address(),
            details: e.to_string(),
        })?;

        // Deserialize response
        let data_table = self.deserialize_response(&response_buf)?;

        Ok((response_len, data_table))
    }

    fn serialize_request(&self, request: &InstanceRequest) -> Result<Vec<u8>> {
        bincode::serialize(request).map_err(|e| BrokerError::Serialization(e.to_string()))
    }

    fn deserialize_response(&self, bytes: &[u8]) -> Result<DataTable> {
        bincode::deserialize(bytes).map_err(|e| BrokerError::Deserialization(e.to_string()))
    }

    /// Get channel statistics.
    pub fn stats(&self) -> ChannelStats {
        ChannelStats {
            server: self.server.clone(),
            connected: self.connected.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            in_flight: self.config.max_in_flight - self.in_flight_semaphore.available_permits(),
            last_activity: *self.last_activity.read(),
        }
    }

    /// Check if the channel is healthy.
    pub fn is_healthy(&self) -> bool {
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let total = self.total_requests.load(Ordering::Relaxed);

        if total == 0 {
            return true;
        }

        // Healthy if failure rate < 50%
        (failed as f64 / total as f64) < 0.5
    }
}

/// Channel statistics.
#[derive(Debug, Clone)]
pub struct ChannelStats {
    pub server: ServerInstance,
    pub connected: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub total_requests: u64,
    pub failed_requests: u64,
    pub in_flight: usize,
    pub last_activity: Instant,
}

/// Manager for server channels.
pub struct ServerChannels {
    /// Channels by server
    channels: DashMap<ServerInstance, Arc<ServerChannel>>,
    /// Configuration
    config: ChannelConfig,
}

impl ServerChannels {
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            channels: DashMap::new(),
            config,
        }
    }

    /// Get or create a channel for a server.
    pub fn get_or_create(&self, server: &ServerInstance) -> Arc<ServerChannel> {
        self.channels
            .entry(server.clone())
            .or_insert_with(|| Arc::new(ServerChannel::new(server.clone(), self.config.clone())))
            .clone()
    }

    /// Send a request to a server.
    pub async fn send_request(
        &self,
        server: &ServerInstance,
        request: &InstanceRequest,
    ) -> Result<ServerResponse> {
        let channel = self.get_or_create(server);
        channel.send_request(request).await
    }

    /// Get all channel statistics.
    pub fn all_stats(&self) -> Vec<ChannelStats> {
        self.channels
            .iter()
            .map(|entry| entry.value().stats())
            .collect()
    }

    /// Remove unhealthy channels.
    pub fn remove_unhealthy(&self) {
        self.channels.retain(|_, channel| channel.is_healthy());
    }

    /// Get number of channels.
    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }
}

impl Default for ServerChannels {
    fn default() -> Self {
        Self::new(ChannelConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TableType;

    fn create_server() -> ServerInstance {
        ServerInstance::new("localhost".to_string(), 8099, TableType::Offline)
    }

    #[test]
    fn test_channel_config_default() {
        let config = ChannelConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert!(config.tcp_nodelay);
        assert_eq!(config.max_in_flight, 100);
    }

    #[test]
    fn test_server_channel_new() {
        let server = create_server();
        let config = ChannelConfig::default();
        let channel = ServerChannel::new(server.clone(), config);

        assert_eq!(channel.server.hostname, "localhost");
        assert!(!channel.connected.load(Ordering::Relaxed));
    }

    #[test]
    fn test_server_channel_stats() {
        let server = create_server();
        let config = ChannelConfig::default();
        let channel = ServerChannel::new(server, config);

        let stats = channel.stats();
        assert!(!stats.connected);
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.total_requests, 0);
    }

    #[test]
    fn test_server_channel_healthy() {
        let server = create_server();
        let config = ChannelConfig::default();
        let channel = ServerChannel::new(server, config);

        assert!(channel.is_healthy()); // No requests yet

        // Simulate some failures
        channel.total_requests.store(10, Ordering::Relaxed);
        channel.failed_requests.store(3, Ordering::Relaxed);
        assert!(channel.is_healthy()); // 30% failure rate

        channel.failed_requests.store(6, Ordering::Relaxed);
        assert!(!channel.is_healthy()); // 60% failure rate
    }

    #[test]
    fn test_server_channels_get_or_create() {
        let channels = ServerChannels::default();
        let server = create_server();

        let channel1 = channels.get_or_create(&server);
        let channel2 = channels.get_or_create(&server);

        // Should return the same channel
        assert!(Arc::ptr_eq(&channel1, &channel2));
        assert_eq!(channels.num_channels(), 1);
    }

    #[test]
    fn test_server_channels_multiple_servers() {
        let channels = ServerChannels::default();

        let server1 = ServerInstance::new("host1".to_string(), 8099, TableType::Offline);
        let server2 = ServerInstance::new("host2".to_string(), 8099, TableType::Offline);

        channels.get_or_create(&server1);
        channels.get_or_create(&server2);

        assert_eq!(channels.num_channels(), 2);
    }

    #[test]
    fn test_all_stats() {
        let channels = ServerChannels::default();

        let server1 = ServerInstance::new("host1".to_string(), 8099, TableType::Offline);
        let server2 = ServerInstance::new("host2".to_string(), 8099, TableType::Offline);

        channels.get_or_create(&server1);
        channels.get_or_create(&server2);

        let stats = channels.all_stats();
        assert_eq!(stats.len(), 2);
    }
}
