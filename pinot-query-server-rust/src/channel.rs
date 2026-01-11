//! gRPC channel management
//!
//! This module manages gRPC channels for network communication between
//! query server nodes. It provides connection pooling, health checking,
//! and efficient channel reuse.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use tonic::transport::{Channel, Endpoint};

use crate::block::{MseBlock, SerializedBlock};
use crate::config::GrpcConfig;
use crate::error::{QueryError, Result};

/// Connection key for channel pooling
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionKey {
    pub host: String,
    pub port: u16,
}

impl ConnectionKey {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn to_uri(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

impl std::fmt::Display for ConnectionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// Channel statistics
#[derive(Debug, Default)]
pub struct ChannelStats {
    /// Requests sent
    pub requests_sent: AtomicU64,
    /// Bytes sent
    pub bytes_sent: AtomicU64,
    /// Bytes received
    pub bytes_received: AtomicU64,
    /// Errors
    pub errors: AtomicU64,
    /// Latency sum (microseconds)
    pub latency_us_sum: AtomicU64,
    /// Channel creates
    pub channel_creates: AtomicU64,
    /// Channel reuses
    pub channel_reuses: AtomicU64,
}

impl ChannelStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self, bytes_sent: usize, latency_us: u64) {
        self.requests_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_sent as u64, Ordering::Relaxed);
        self.latency_us_sum.fetch_add(latency_us, Ordering::Relaxed);
    }

    pub fn record_response(&self, bytes_received: usize) {
        self.bytes_received.fetch_add(bytes_received as u64, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn average_latency_us(&self) -> u64 {
        let requests = self.requests_sent.load(Ordering::Relaxed);
        if requests == 0 {
            0
        } else {
            self.latency_us_sum.load(Ordering::Relaxed) / requests
        }
    }
}

/// Managed channel wrapper
pub struct ManagedChannel {
    /// The underlying gRPC channel
    channel: Channel,
    /// Connection key
    key: ConnectionKey,
    /// Creation time
    created_at: Instant,
    /// Last used time
    last_used: RwLock<Instant>,
    /// Active request count
    active_requests: AtomicU64,
    /// Healthy flag
    healthy: AtomicBool,
    /// Statistics
    stats: Arc<ChannelStats>,
}

impl ManagedChannel {
    /// Create a new managed channel
    pub async fn connect(key: ConnectionKey, config: &GrpcConfig) -> Result<Self> {
        let endpoint = Endpoint::from_shared(key.to_uri())
            .map_err(|e| QueryError::ConnectionFailed {
                host: key.host.clone(),
                port: key.port,
                reason: e.to_string(),
            })?
            .timeout(config.keep_alive_timeout)
            .connect_timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(config.keep_alive_interval)
            .keep_alive_timeout(config.keep_alive_timeout)
            .initial_connection_window_size(config.initial_connection_window_size)
            .initial_stream_window_size(config.initial_stream_window_size);

        let channel = endpoint.connect().await.map_err(|e| QueryError::ConnectionFailed {
            host: key.host.clone(),
            port: key.port,
            reason: e.to_string(),
        })?;

        let now = Instant::now();
        Ok(Self {
            channel,
            key,
            created_at: now,
            last_used: RwLock::new(now),
            active_requests: AtomicU64::new(0),
            healthy: AtomicBool::new(true),
            stats: Arc::new(ChannelStats::new()),
        })
    }

    /// Get the underlying channel
    pub fn channel(&self) -> Channel {
        *self.last_used.write() = Instant::now();
        self.channel.clone()
    }

    /// Mark request started
    pub fn request_started(&self) {
        self.active_requests.fetch_add(1, Ordering::Relaxed);
        *self.last_used.write() = Instant::now();
    }

    /// Mark request completed
    pub fn request_completed(&self, bytes: usize, latency_us: u64, success: bool) {
        self.active_requests.fetch_sub(1, Ordering::Relaxed);
        if success {
            self.stats.record_request(bytes, latency_us);
        } else {
            self.stats.record_error();
            // Mark unhealthy after errors
            self.healthy.store(false, Ordering::Relaxed);
        }
    }

    /// Check if channel is healthy
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    /// Check if channel is idle (no active requests and not used recently)
    pub fn is_idle(&self, idle_threshold: Duration) -> bool {
        self.active_requests.load(Ordering::Relaxed) == 0
            && self.last_used.read().elapsed() > idle_threshold
    }

    /// Get active request count
    pub fn active_requests(&self) -> u64 {
        self.active_requests.load(Ordering::Relaxed)
    }

    /// Get statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.stats
    }

    /// Get connection key
    pub fn key(&self) -> &ConnectionKey {
        &self.key
    }
}

/// Channel manager for pooling connections
pub struct ChannelManager {
    /// Configuration
    config: Arc<GrpcConfig>,
    /// Channel pool
    channels: DashMap<ConnectionKey, Arc<ManagedChannel>>,
    /// Global statistics
    stats: Arc<ChannelStats>,
    /// Shutdown flag
    shutdown: AtomicBool,
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new(config: Arc<GrpcConfig>) -> Self {
        Self {
            config,
            channels: DashMap::new(),
            stats: Arc::new(ChannelStats::new()),
            shutdown: AtomicBool::new(false),
        }
    }

    /// Get or create a channel for the given key
    pub async fn get_channel(&self, key: ConnectionKey) -> Result<Arc<ManagedChannel>> {
        if self.shutdown.load(Ordering::Relaxed) {
            return Err(QueryError::Internal("Channel manager shutdown".to_string()));
        }

        // Check for existing healthy channel
        if let Some(channel) = self.channels.get(&key) {
            if channel.is_healthy() {
                self.stats.channel_reuses.fetch_add(1, Ordering::Relaxed);
                return Ok(channel.clone());
            } else {
                // Remove unhealthy channel
                self.channels.remove(&key);
            }
        }

        // Create new channel
        self.stats.channel_creates.fetch_add(1, Ordering::Relaxed);
        let channel = Arc::new(ManagedChannel::connect(key.clone(), &self.config).await?);
        self.channels.insert(key, channel.clone());
        Ok(channel)
    }

    /// Get or create a channel by host and port
    pub async fn get_channel_for(&self, host: &str, port: u16) -> Result<Arc<ManagedChannel>> {
        self.get_channel(ConnectionKey::new(host, port)).await
    }

    /// Remove a channel
    pub fn remove_channel(&self, key: &ConnectionKey) {
        self.channels.remove(key);
    }

    /// Get number of active channels
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Clean up idle channels
    pub fn cleanup_idle(&self) {
        let idle_threshold = self.config.idle_timeout;
        self.channels.retain(|_, channel| !channel.is_idle(idle_threshold));
    }

    /// Get global statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.stats
    }

    /// Shutdown the channel manager
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.channels.clear();
    }
}

/// Network sender for transmitting blocks over gRPC
pub struct NetworkSender {
    /// Channel manager
    channel_manager: Arc<ChannelManager>,
    /// Target host
    host: String,
    /// Target port
    port: u16,
    /// Maximum message size
    max_message_size: usize,
    /// Statistics
    stats: Arc<ChannelStats>,
}

impl NetworkSender {
    /// Create a new network sender
    pub fn new(channel_manager: Arc<ChannelManager>, host: String, port: u16) -> Self {
        Self {
            channel_manager,
            host,
            port,
            max_message_size: 4 * 1024 * 1024, // 4MB default
            stats: Arc::new(ChannelStats::new()),
        }
    }

    /// Set maximum message size
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Send a block to the target
    pub async fn send(&self, block: MseBlock) -> Result<()> {
        let channel = self
            .channel_manager
            .get_channel_for(&self.host, self.port)
            .await?;

        let start = Instant::now();
        channel.request_started();

        // Serialize and chunk the block
        let serialized = SerializedBlock::from_mse_block(&block, self.max_message_size)?;
        let bytes = serialized.total_size;

        // In a real implementation, this would send via gRPC streaming
        // For now, we just track the stats
        let latency = start.elapsed().as_micros() as u64;
        channel.request_completed(bytes, latency, true);
        self.stats.record_request(bytes, latency);

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.stats
    }
}

/// Network receiver for receiving blocks over gRPC
pub struct NetworkReceiver {
    /// Bind address
    bind_address: String,
    /// Port
    port: u16,
    /// Running flag
    running: AtomicBool,
    /// Statistics
    stats: Arc<ChannelStats>,
}

impl NetworkReceiver {
    /// Create a new network receiver
    pub fn new(bind_address: String, port: u16) -> Self {
        Self {
            bind_address,
            port,
            running: AtomicBool::new(false),
            stats: Arc::new(ChannelStats::new()),
        }
    }

    /// Get bind address
    pub fn bind_address(&self) -> &str {
        &self.bind_address
    }

    /// Get port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get statistics
    pub fn stats(&self) -> &ChannelStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_key() {
        let key = ConnectionKey::new("localhost", 8080);
        assert_eq!(key.to_uri(), "http://localhost:8080");
        assert_eq!(key.to_string(), "localhost:8080");
    }

    #[test]
    fn test_channel_stats() {
        let stats = ChannelStats::new();

        stats.record_request(1000, 5000);
        stats.record_request(2000, 10000);
        stats.record_response(500);

        assert_eq!(stats.requests_sent.load(Ordering::Relaxed), 2);
        assert_eq!(stats.bytes_sent.load(Ordering::Relaxed), 3000);
        assert_eq!(stats.bytes_received.load(Ordering::Relaxed), 500);
        assert_eq!(stats.average_latency_us(), 7500);
    }

    #[tokio::test]
    async fn test_channel_manager_basic() {
        let config = Arc::new(GrpcConfig::default());
        let manager = ChannelManager::new(config);

        assert_eq!(manager.channel_count(), 0);
        assert!(!manager.shutdown.load(Ordering::Relaxed));
    }

    #[test]
    fn test_network_sender() {
        let config = Arc::new(GrpcConfig::default());
        let manager = Arc::new(ChannelManager::new(config));
        let sender = NetworkSender::new(manager, "localhost".to_string(), 8080)
            .with_max_message_size(1024 * 1024);

        assert_eq!(sender.max_message_size, 1024 * 1024);
    }

    #[test]
    fn test_network_receiver() {
        let receiver = NetworkReceiver::new("0.0.0.0".to_string(), 9090);

        assert_eq!(receiver.bind_address(), "0.0.0.0");
        assert_eq!(receiver.port(), 9090);
        assert!(!receiver.is_running());
    }
}
