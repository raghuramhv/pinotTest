//! Adaptive server selection based on runtime metrics.

use crate::types::{QueryStats, SegmentsToQuery, ServerInstance};
use crate::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Adaptive server selector that routes based on latency and load.
pub struct AdaptiveServerSelector {
    /// Per-server statistics
    server_stats: RwLock<HashMap<ServerInstance, ServerMetrics>>,
    /// Selection strategy
    strategy: SelectionStrategy,
    /// EMA decay factor for latency (0.0 to 1.0)
    ema_decay: f64,
    /// Maximum age for stats before considering them stale
    max_stats_age: Duration,
}

/// Server performance metrics.
#[derive(Debug)]
pub struct ServerMetrics {
    /// Exponential moving average of latency (microseconds)
    latency_ema_us: AtomicU64,
    /// Current in-flight requests
    in_flight_requests: AtomicUsize,
    /// Total requests
    total_requests: AtomicU64,
    /// Failed requests
    failed_requests: AtomicU64,
    /// Last update time
    last_update: parking_lot::Mutex<Instant>,
}

impl ServerMetrics {
    pub fn new() -> Self {
        Self {
            latency_ema_us: AtomicU64::new(1000), // 1ms default
            in_flight_requests: AtomicUsize::new(0),
            total_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            last_update: parking_lot::Mutex::new(Instant::now()),
        }
    }

    pub fn latency_ms(&self) -> f64 {
        self.latency_ema_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight_requests.load(Ordering::Relaxed)
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        let failed = self.failed_requests.load(Ordering::Relaxed);
        1.0 - (failed as f64 / total as f64)
    }

    pub fn update_latency(&self, latency_us: u64, decay: f64) {
        let current = self.latency_ema_us.load(Ordering::Relaxed);
        let new_ema = (decay * latency_us as f64 + (1.0 - decay) * current as f64) as u64;
        self.latency_ema_us.store(new_ema, Ordering::Relaxed);
        *self.last_update.lock() = Instant::now();
    }

    pub fn increment_in_flight(&self) {
        self.in_flight_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_in_flight(&self) {
        self.in_flight_requests.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_success(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.last_update.lock().elapsed() > max_age
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Strategy for selecting servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select based on lowest latency
    LowestLatency,
    /// Select based on fewest in-flight requests
    FewestInFlight,
    /// Hybrid of latency and in-flight
    Hybrid,
    /// Random selection with weights
    WeightedRandom,
}

impl AdaptiveServerSelector {
    pub fn new(strategy: SelectionStrategy) -> Self {
        Self {
            server_stats: RwLock::new(HashMap::new()),
            strategy,
            ema_decay: 0.3, // 30% weight to new sample
            max_stats_age: Duration::from_secs(60),
        }
    }

    pub fn with_ema_decay(mut self, decay: f64) -> Self {
        self.ema_decay = decay.clamp(0.0, 1.0);
        self
    }

    pub fn with_max_stats_age(mut self, age: Duration) -> Self {
        self.max_stats_age = age;
        self
    }

    /// Get or create metrics for a server.
    fn get_or_create_metrics(&self, server: &ServerInstance) -> ServerMetrics {
        let stats = self.server_stats.read();
        if let Some(metrics) = stats.get(server) {
            return ServerMetrics {
                latency_ema_us: AtomicU64::new(metrics.latency_ema_us.load(Ordering::Relaxed)),
                in_flight_requests: AtomicUsize::new(metrics.in_flight_requests.load(Ordering::Relaxed)),
                total_requests: AtomicU64::new(metrics.total_requests.load(Ordering::Relaxed)),
                failed_requests: AtomicU64::new(metrics.failed_requests.load(Ordering::Relaxed)),
                last_update: parking_lot::Mutex::new(*metrics.last_update.lock()),
            };
        }
        drop(stats);

        let mut stats = self.server_stats.write();
        stats.entry(server.clone()).or_insert_with(ServerMetrics::new);
        ServerMetrics::new()
    }

    /// Record the start of a request to a server.
    pub fn record_request_start(&self, server: &ServerInstance) {
        let stats = self.server_stats.read();
        if let Some(metrics) = stats.get(server) {
            metrics.increment_in_flight();
        } else {
            drop(stats);
            let mut stats = self.server_stats.write();
            stats
                .entry(server.clone())
                .or_insert_with(ServerMetrics::new)
                .increment_in_flight();
        }
    }

    /// Record the completion of a request to a server.
    pub fn record_request_complete(
        &self,
        server: &ServerInstance,
        latency_us: u64,
        success: bool,
    ) {
        let stats = self.server_stats.read();
        if let Some(metrics) = stats.get(server) {
            metrics.decrement_in_flight();
            metrics.update_latency(latency_us, self.ema_decay);
            if success {
                metrics.record_success();
            } else {
                metrics.record_failure();
            }
        } else {
            drop(stats);
            let mut stats = self.server_stats.write();
            let metrics = stats
                .entry(server.clone())
                .or_insert_with(ServerMetrics::new);
            // Don't decrement in_flight for new entries (wasn't incremented by record_request_start)
            metrics.update_latency(latency_us, self.ema_decay);
            if success {
                metrics.record_success();
            } else {
                metrics.record_failure();
            }
        }
    }

    /// Select the best server from a list of candidates.
    pub fn select_best(&self, candidates: &[ServerInstance]) -> Option<ServerInstance> {
        if candidates.is_empty() {
            return None;
        }
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }

        let stats = self.server_stats.read();

        match self.strategy {
            SelectionStrategy::LowestLatency => {
                self.select_lowest_latency(candidates, &stats)
            }
            SelectionStrategy::FewestInFlight => {
                self.select_fewest_in_flight(candidates, &stats)
            }
            SelectionStrategy::Hybrid => {
                self.select_hybrid(candidates, &stats)
            }
            SelectionStrategy::WeightedRandom => {
                self.select_weighted_random(candidates, &stats)
            }
        }
    }

    fn select_lowest_latency(
        &self,
        candidates: &[ServerInstance],
        stats: &HashMap<ServerInstance, ServerMetrics>,
    ) -> Option<ServerInstance> {
        candidates
            .iter()
            .min_by(|a, b| {
                let lat_a = stats
                    .get(*a)
                    .map(|m| m.latency_ms())
                    .unwrap_or(f64::MAX);
                let lat_b = stats
                    .get(*b)
                    .map(|m| m.latency_ms())
                    .unwrap_or(f64::MAX);
                lat_a.partial_cmp(&lat_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    fn select_fewest_in_flight(
        &self,
        candidates: &[ServerInstance],
        stats: &HashMap<ServerInstance, ServerMetrics>,
    ) -> Option<ServerInstance> {
        candidates
            .iter()
            .min_by_key(|s| {
                stats
                    .get(*s)
                    .map(|m| m.in_flight())
                    .unwrap_or(0)
            })
            .cloned()
    }

    fn select_hybrid(
        &self,
        candidates: &[ServerInstance],
        stats: &HashMap<ServerInstance, ServerMetrics>,
    ) -> Option<ServerInstance> {
        // Score = latency_ms + (in_flight * 10)
        // Lower is better
        candidates
            .iter()
            .min_by(|a, b| {
                let score_a = stats
                    .get(*a)
                    .map(|m| m.latency_ms() + (m.in_flight() as f64 * 10.0))
                    .unwrap_or(f64::MAX);
                let score_b = stats
                    .get(*b)
                    .map(|m| m.latency_ms() + (m.in_flight() as f64 * 10.0))
                    .unwrap_or(f64::MAX);
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    fn select_weighted_random(
        &self,
        candidates: &[ServerInstance],
        stats: &HashMap<ServerInstance, ServerMetrics>,
    ) -> Option<ServerInstance> {
        use rand::Rng;

        // Weight inversely proportional to latency
        let weights: Vec<f64> = candidates
            .iter()
            .map(|s| {
                let latency = stats
                    .get(s)
                    .map(|m| m.latency_ms())
                    .unwrap_or(1.0);
                1.0 / (latency + 1.0) // Add 1 to avoid division by zero
            })
            .collect();

        let total_weight: f64 = weights.iter().sum();
        if total_weight <= 0.0 {
            return candidates.first().cloned();
        }

        let mut rng = rand::thread_rng();
        let mut random_value = rng.gen::<f64>() * total_weight;

        for (i, weight) in weights.iter().enumerate() {
            random_value -= weight;
            if random_value <= 0.0 {
                return Some(candidates[i].clone());
            }
        }

        candidates.last().cloned()
    }

    /// Get statistics for all servers.
    pub fn get_all_stats(&self) -> Vec<QueryStats> {
        let stats = self.server_stats.read();
        stats
            .iter()
            .map(|(server, metrics)| QueryStats {
                server: server.clone(),
                latency_ms: metrics.latency_ema_us.load(Ordering::Relaxed) / 1000,
                in_flight_requests: metrics.in_flight_requests.load(Ordering::Relaxed),
                success_rate: metrics.success_rate(),
                last_update: *metrics.last_update.lock(),
            })
            .collect()
    }

    /// Clear stale statistics.
    pub fn clear_stale(&self) {
        let mut stats = self.server_stats.write();
        stats.retain(|_, metrics| !metrics.is_stale(self.max_stats_age));
    }
}

impl Default for AdaptiveServerSelector {
    fn default() -> Self {
        Self::new(SelectionStrategy::Hybrid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TableType;

    fn create_servers() -> Vec<ServerInstance> {
        vec![
            ServerInstance::new("host1".to_string(), 8099, TableType::Offline),
            ServerInstance::new("host2".to_string(), 8099, TableType::Offline),
            ServerInstance::new("host3".to_string(), 8099, TableType::Offline),
        ]
    }

    #[test]
    fn test_server_metrics() {
        let metrics = ServerMetrics::new();
        assert_eq!(metrics.in_flight(), 0);
        assert_eq!(metrics.success_rate(), 1.0);

        metrics.increment_in_flight();
        assert_eq!(metrics.in_flight(), 1);

        metrics.decrement_in_flight();
        assert_eq!(metrics.in_flight(), 0);

        metrics.record_success();
        metrics.record_success();
        metrics.record_failure();
        assert!((metrics.success_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_latency_update() {
        let metrics = ServerMetrics::new();
        metrics.update_latency(5000, 0.5); // 5ms

        let latency = metrics.latency_ms();
        assert!(latency > 1.0 && latency < 5.0);
    }

    #[test]
    fn test_adaptive_selector_lowest_latency() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::LowestLatency);
        let servers = create_servers();

        // Record different latencies
        selector.record_request_complete(&servers[0], 5000, true); // 5ms
        selector.record_request_complete(&servers[1], 1000, true); // 1ms
        selector.record_request_complete(&servers[2], 3000, true); // 3ms

        let selected = selector.select_best(&servers).unwrap();
        assert_eq!(selected.hostname, "host2"); // Lowest latency
    }

    #[test]
    fn test_adaptive_selector_fewest_in_flight() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::FewestInFlight);
        let servers = create_servers();

        // Add in-flight requests
        selector.record_request_start(&servers[0]);
        selector.record_request_start(&servers[0]);
        selector.record_request_start(&servers[1]);
        // servers[2] has 0 in-flight

        let selected = selector.select_best(&servers).unwrap();
        assert_eq!(selected.hostname, "host3"); // Fewest in-flight
    }

    #[test]
    fn test_adaptive_selector_hybrid() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::Hybrid)
            .with_ema_decay(1.0); // Use 100% decay to get exact latency values
        let servers = create_servers();

        // host1: medium latency, no in-flight
        selector.record_request_complete(&servers[0], 5000, true);

        // host2: lowest latency, no in-flight - should win
        selector.record_request_complete(&servers[1], 1000, true);

        // host3: high latency, no in-flight
        selector.record_request_complete(&servers[2], 50000, true);

        let selected = selector.select_best(&servers).unwrap();
        // host2 should win with lowest latency
        assert_eq!(selected.hostname, "host2");
    }

    #[test]
    fn test_empty_candidates() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::LowestLatency);
        assert!(selector.select_best(&[]).is_none());
    }

    #[test]
    fn test_single_candidate() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::LowestLatency);
        let servers = vec![ServerInstance::new(
            "host1".to_string(),
            8099,
            TableType::Offline,
        )];

        let selected = selector.select_best(&servers).unwrap();
        assert_eq!(selected.hostname, "host1");
    }

    #[test]
    fn test_get_all_stats() {
        let selector = AdaptiveServerSelector::new(SelectionStrategy::Hybrid);
        let servers = create_servers();

        selector.record_request_complete(&servers[0], 1000, true);
        selector.record_request_complete(&servers[1], 2000, true);

        let stats = selector.get_all_stats();
        assert_eq!(stats.len(), 2);
    }
}
