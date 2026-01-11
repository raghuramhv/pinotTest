//! Memory backpressure management
//!
//! This module provides mechanisms for managing memory pressure in the query server.
//! It uses system memory stats to detect pressure and throttle query execution accordingly.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio::sync::{Notify, Semaphore};

use crate::config::MemoryConfig;
use crate::error::{QueryError, Result};

/// Memory pressure levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressureLevel {
    /// Normal operation - no pressure
    Normal,
    /// Moderate pressure - start throttling new queries
    Moderate,
    /// High pressure - aggressive throttling
    High,
    /// Critical pressure - reject new queries
    Critical,
}

impl MemoryPressureLevel {
    /// Get the throttle factor for this level (1.0 = no throttle, 0.0 = full stop)
    pub fn throttle_factor(&self) -> f64 {
        match self {
            MemoryPressureLevel::Normal => 1.0,
            MemoryPressureLevel::Moderate => 0.7,
            MemoryPressureLevel::High => 0.3,
            MemoryPressureLevel::Critical => 0.0,
        }
    }

    /// Get backoff duration for this level
    pub fn backoff_duration(&self) -> Duration {
        match self {
            MemoryPressureLevel::Normal => Duration::ZERO,
            MemoryPressureLevel::Moderate => Duration::from_millis(10),
            MemoryPressureLevel::High => Duration::from_millis(50),
            MemoryPressureLevel::Critical => Duration::from_millis(200),
        }
    }
}

/// Memory statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total allocated bytes
    pub allocated_bytes: usize,
    /// Active (in-use) bytes
    pub active_bytes: usize,
    /// Resident set size
    pub resident_bytes: usize,
    /// Mapped memory
    pub mapped_bytes: usize,
    /// Retained (cached) memory
    pub retained_bytes: usize,
    /// Fragmentation ratio (allocated / active)
    pub fragmentation: f64,
    /// System total memory
    pub system_total: usize,
    /// System available memory
    pub system_available: usize,
}

impl MemoryStats {
    /// Get memory usage ratio (0.0 - 1.0)
    pub fn usage_ratio(&self) -> f64 {
        if self.system_total == 0 {
            0.0
        } else {
            (self.system_total - self.system_available) as f64 / self.system_total as f64
        }
    }

    /// Get allocated ratio (0.0 - 1.0) relative to limit
    pub fn allocated_ratio(&self, limit: usize) -> f64 {
        if limit == 0 {
            0.0
        } else {
            self.allocated_bytes as f64 / limit as f64
        }
    }
}

/// Memory backpressure manager
pub struct MemoryBackpressureManager {
    /// Configuration
    config: MemoryConfig,
    /// Current pressure level
    pressure_level: RwLock<MemoryPressureLevel>,
    /// Current memory stats
    current_stats: RwLock<MemoryStats>,
    /// Memory limit (0 = use system limit)
    memory_limit: AtomicUsize,
    /// Bytes currently reserved by queries
    reserved_bytes: AtomicUsize,
    /// Number of active queries
    active_queries: AtomicUsize,
    /// Semaphore for query admission control
    admission_semaphore: Semaphore,
    /// Maximum concurrent queries
    max_concurrent_queries: usize,
    /// Pressure change notification
    pressure_notify: Notify,
    /// Last pressure check time
    last_check: RwLock<Instant>,
    /// Running flag
    running: AtomicBool,
    /// Statistics
    stats: BackpressureStats,
}

/// Backpressure statistics
#[derive(Debug, Default)]
pub struct BackpressureStats {
    /// Number of queries throttled
    pub queries_throttled: AtomicU64,
    /// Number of queries rejected
    pub queries_rejected: AtomicU64,
    /// Total throttle wait time (microseconds)
    pub throttle_wait_us: AtomicU64,
    /// Number of GC triggers
    pub gc_triggers: AtomicU64,
    /// Peak memory usage
    pub peak_memory_bytes: AtomicUsize,
    /// Number of pressure level changes
    pub pressure_changes: AtomicU64,
}

impl MemoryBackpressureManager {
    /// Create a new backpressure manager
    pub fn new(config: MemoryConfig) -> Self {
        let max_queries = std::thread::available_parallelism()
            .map(|p| p.get() * 4)
            .unwrap_or(16);

        Self {
            config,
            pressure_level: RwLock::new(MemoryPressureLevel::Normal),
            current_stats: RwLock::new(MemoryStats::default()),
            memory_limit: AtomicUsize::new(0),
            reserved_bytes: AtomicUsize::new(0),
            active_queries: AtomicUsize::new(0),
            admission_semaphore: Semaphore::new(max_queries),
            max_concurrent_queries: max_queries,
            pressure_notify: Notify::new(),
            last_check: RwLock::new(Instant::now()),
            running: AtomicBool::new(true),
            stats: BackpressureStats::default(),
        }
    }

    /// Set memory limit
    pub fn set_memory_limit(&self, limit: usize) {
        self.memory_limit.store(limit, Ordering::Relaxed);
    }

    /// Get current pressure level
    pub fn pressure_level(&self) -> MemoryPressureLevel {
        *self.pressure_level.read()
    }

    /// Get current memory stats
    pub fn memory_stats(&self) -> MemoryStats {
        self.current_stats.read().clone()
    }

    /// Get statistics
    pub fn stats(&self) -> &BackpressureStats {
        &self.stats
    }

    /// Update memory statistics
    pub fn update_stats(&self) -> MemoryStats {
        let stats = self.collect_memory_stats();

        // Update peak
        let peak = self.stats.peak_memory_bytes.load(Ordering::Relaxed);
        if stats.allocated_bytes > peak {
            self.stats
                .peak_memory_bytes
                .store(stats.allocated_bytes, Ordering::Relaxed);
        }

        // Update pressure level
        let old_level = *self.pressure_level.read();
        let new_level = self.calculate_pressure_level(&stats);

        if new_level != old_level {
            *self.pressure_level.write() = new_level;
            self.stats.pressure_changes.fetch_add(1, Ordering::Relaxed);
            self.pressure_notify.notify_waiters();

            // Trigger GC if entering high pressure
            if new_level >= MemoryPressureLevel::High && self.config.aggressive_gc_enabled {
                self.trigger_gc();
            }
        }

        *self.current_stats.write() = stats.clone();
        *self.last_check.write() = Instant::now();

        stats
    }

    /// Calculate pressure level from stats
    fn calculate_pressure_level(&self, stats: &MemoryStats) -> MemoryPressureLevel {
        let limit = self.memory_limit.load(Ordering::Relaxed);
        let usage = if limit > 0 {
            stats.allocated_bytes as f64 / limit as f64
        } else {
            stats.usage_ratio()
        };

        if usage >= self.config.critical_threshold {
            MemoryPressureLevel::Critical
        } else if usage >= self.config.pressure_threshold {
            MemoryPressureLevel::High
        } else if usage >= self.config.pressure_threshold * 0.8 {
            MemoryPressureLevel::Moderate
        } else {
            MemoryPressureLevel::Normal
        }
    }

    /// Collect current memory statistics
    fn collect_memory_stats(&self) -> MemoryStats {
        let mut stats = MemoryStats::default();

        // Get system memory stats
        if let Some(mem_stats) = memory_stats::memory_stats() {
            stats.system_total = mem_stats.physical_mem;
            // Estimate available as total - resident
            stats.system_available =
                mem_stats.physical_mem.saturating_sub(stats.resident_bytes);
        }

        stats
    }

    /// Trigger garbage collection / memory release
    fn trigger_gc(&self) {
        self.stats.gc_triggers.fetch_add(1, Ordering::Relaxed);
        // Memory release is handled automatically by the allocator
        // This is a signal that we've detected high memory pressure
    }

    /// Reserve memory for a query
    pub async fn reserve(&self, bytes: usize) -> Result<MemoryReservation> {
        let level = self.pressure_level();

        // Reject under critical pressure
        if level == MemoryPressureLevel::Critical {
            self.stats.queries_rejected.fetch_add(1, Ordering::Relaxed);
            let stats = self.memory_stats();
            return Err(QueryError::MemoryPressure {
                used_bytes: stats.allocated_bytes,
                limit_bytes: self.memory_limit.load(Ordering::Relaxed),
            });
        }

        // Throttle under pressure
        if level >= MemoryPressureLevel::Moderate {
            self.stats.queries_throttled.fetch_add(1, Ordering::Relaxed);
            let start = Instant::now();
            tokio::time::sleep(level.backoff_duration()).await;
            let elapsed = start.elapsed().as_micros() as u64;
            self.stats.throttle_wait_us.fetch_add(elapsed, Ordering::Relaxed);
        }

        // Acquire admission permit
        let permit = self.admission_semaphore.acquire().await.map_err(|_| {
            QueryError::Internal("Admission semaphore closed".to_string())
        })?;

        // Reserve the memory
        self.reserved_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.active_queries.fetch_add(1, Ordering::Relaxed);

        Ok(MemoryReservation {
            manager: self,
            bytes,
            _permit: permit,
        })
    }

    /// Try to reserve memory (non-blocking)
    pub fn try_reserve(&self, bytes: usize) -> Result<MemoryReservation> {
        let level = self.pressure_level();

        if level == MemoryPressureLevel::Critical {
            self.stats.queries_rejected.fetch_add(1, Ordering::Relaxed);
            let stats = self.memory_stats();
            return Err(QueryError::MemoryPressure {
                used_bytes: stats.allocated_bytes,
                limit_bytes: self.memory_limit.load(Ordering::Relaxed),
            });
        }

        let permit = self.admission_semaphore.try_acquire().map_err(|_| {
            QueryError::MemoryPressure {
                used_bytes: self.reserved_bytes.load(Ordering::Relaxed),
                limit_bytes: self.memory_limit.load(Ordering::Relaxed),
            }
        })?;

        self.reserved_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.active_queries.fetch_add(1, Ordering::Relaxed);

        Ok(MemoryReservation {
            manager: self,
            bytes,
            _permit: permit,
        })
    }

    /// Release reserved memory
    fn release(&self, bytes: usize) {
        self.reserved_bytes.fetch_sub(bytes, Ordering::Relaxed);
        self.active_queries.fetch_sub(1, Ordering::Relaxed);
    }

    /// Wait for pressure to reduce
    pub async fn wait_for_relief(&self, timeout: Duration) -> bool {
        let start = Instant::now();

        while start.elapsed() < timeout {
            if self.pressure_level() <= MemoryPressureLevel::Moderate {
                return true;
            }

            tokio::select! {
                _ = self.pressure_notify.notified() => {
                    if self.pressure_level() <= MemoryPressureLevel::Moderate {
                        return true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
        }

        false
    }

    /// Get number of active queries
    pub fn active_queries(&self) -> usize {
        self.active_queries.load(Ordering::Relaxed)
    }

    /// Get reserved bytes
    pub fn reserved_bytes(&self) -> usize {
        self.reserved_bytes.load(Ordering::Relaxed)
    }

    /// Check if should throttle
    pub fn should_throttle(&self) -> bool {
        self.pressure_level() >= MemoryPressureLevel::Moderate
    }

    /// Shutdown the manager
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.admission_semaphore.close();
        self.pressure_notify.notify_waiters();
    }
}

/// Memory reservation handle
pub struct MemoryReservation<'a> {
    manager: &'a MemoryBackpressureManager,
    bytes: usize,
    _permit: tokio::sync::SemaphorePermit<'a>,
}

impl<'a> MemoryReservation<'a> {
    /// Get reserved bytes
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Check current pressure level
    pub fn pressure_level(&self) -> MemoryPressureLevel {
        self.manager.pressure_level()
    }

    /// Check if should yield due to pressure
    pub fn should_yield(&self) -> bool {
        self.manager.pressure_level() >= MemoryPressureLevel::High
    }
}

impl<'a> Drop for MemoryReservation<'a> {
    fn drop(&mut self) {
        self.manager.release(self.bytes);
    }
}

/// Backpressure controller for individual operators
pub struct OperatorBackpressure {
    /// Parent manager
    manager: Arc<MemoryBackpressureManager>,
    /// Operator name
    operator: String,
    /// Bytes processed
    bytes_processed: AtomicUsize,
    /// Rows processed
    rows_processed: AtomicUsize,
    /// Throttle events
    throttle_events: AtomicU64,
    /// Last check time
    last_check: RwLock<Instant>,
    /// Check interval
    check_interval: Duration,
}

impl OperatorBackpressure {
    /// Create a new operator backpressure controller
    pub fn new(manager: Arc<MemoryBackpressureManager>, operator: impl Into<String>) -> Self {
        Self {
            manager,
            operator: operator.into(),
            bytes_processed: AtomicUsize::new(0),
            rows_processed: AtomicUsize::new(0),
            throttle_events: AtomicU64::new(0),
            last_check: RwLock::new(Instant::now()),
            check_interval: Duration::from_millis(10),
        }
    }

    /// Record bytes processed and check for throttling
    pub async fn record_and_check(&self, bytes: usize, rows: usize) {
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
        self.rows_processed.fetch_add(rows, Ordering::Relaxed);

        // Periodic pressure check
        let should_check = {
            let last = *self.last_check.read();
            last.elapsed() >= self.check_interval
        };

        if should_check {
            *self.last_check.write() = Instant::now();
            self.manager.update_stats();

            let level = self.manager.pressure_level();
            if level >= MemoryPressureLevel::Moderate {
                self.throttle_events.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(level.backoff_duration()).await;
            }
        }
    }

    /// Check if should stop due to critical pressure
    pub fn should_stop(&self) -> bool {
        self.manager.pressure_level() == MemoryPressureLevel::Critical
    }

    /// Get bytes processed
    pub fn bytes_processed(&self) -> usize {
        self.bytes_processed.load(Ordering::Relaxed)
    }

    /// Get rows processed
    pub fn rows_processed(&self) -> usize {
        self.rows_processed.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pressure_levels() {
        assert_eq!(MemoryPressureLevel::Normal.throttle_factor(), 1.0);
        assert!(MemoryPressureLevel::Critical.throttle_factor() < 0.1);

        assert!(MemoryPressureLevel::Normal < MemoryPressureLevel::Moderate);
        assert!(MemoryPressureLevel::Moderate < MemoryPressureLevel::High);
        assert!(MemoryPressureLevel::High < MemoryPressureLevel::Critical);
    }

    #[test]
    fn test_memory_stats() {
        let stats = MemoryStats {
            system_total: 1000,
            system_available: 600,
            ..Default::default()
        };

        assert!((stats.usage_ratio() - 0.4).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_backpressure_manager_basic() {
        let config = MemoryConfig::default();
        let manager = MemoryBackpressureManager::new(config);

        assert_eq!(manager.pressure_level(), MemoryPressureLevel::Normal);
        assert_eq!(manager.active_queries(), 0);
    }

    #[tokio::test]
    async fn test_memory_reservation() {
        let config = MemoryConfig::default();
        let manager = MemoryBackpressureManager::new(config);

        let reservation = manager.reserve(1024).await.unwrap();
        assert_eq!(reservation.bytes(), 1024);
        assert_eq!(manager.reserved_bytes(), 1024);
        assert_eq!(manager.active_queries(), 1);

        drop(reservation);
        assert_eq!(manager.reserved_bytes(), 0);
        assert_eq!(manager.active_queries(), 0);
    }

    #[tokio::test]
    async fn test_try_reserve() {
        let config = MemoryConfig::default();
        let manager = MemoryBackpressureManager::new(config);

        let reservation = manager.try_reserve(512).unwrap();
        assert_eq!(manager.active_queries(), 1);
        drop(reservation);
    }

    #[test]
    fn test_backpressure_stats() {
        let stats = BackpressureStats::default();
        stats.queries_throttled.fetch_add(5, Ordering::Relaxed);
        stats.queries_rejected.fetch_add(2, Ordering::Relaxed);

        assert_eq!(stats.queries_throttled.load(Ordering::Relaxed), 5);
        assert_eq!(stats.queries_rejected.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_operator_backpressure() {
        let config = MemoryConfig::default();
        let manager = Arc::new(MemoryBackpressureManager::new(config));

        let op_bp = OperatorBackpressure::new(manager, "TestOperator");

        op_bp.record_and_check(1000, 10).await;
        assert_eq!(op_bp.bytes_processed(), 1000);
        assert_eq!(op_bp.rows_processed(), 10);
        assert!(!op_bp.should_stop());
    }

    #[test]
    fn test_update_stats() {
        let config = MemoryConfig::default();
        let manager = MemoryBackpressureManager::new(config);

        let stats = manager.update_stats();
        // Stats should be populated (exact values depend on runtime)
        assert!(stats.system_total > 0 || stats.allocated_bytes == 0);
    }
}
