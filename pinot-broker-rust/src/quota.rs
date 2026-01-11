//! Query Quota Management Module
//!
//! Implements rate limiting, query quotas, and resource management
//! for fair resource allocation across tenants and users.

use crate::error::{BrokerError, Result};
use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============== Configuration ==============

/// Configuration for quota management
#[derive(Debug, Clone)]
pub struct QuotaConfig {
    /// Maximum queries per second (global)
    pub global_qps_limit: u64,
    /// Maximum concurrent queries (global)
    pub global_concurrent_limit: usize,
    /// Default QPS limit per table
    pub default_table_qps_limit: u64,
    /// Default QPS limit per user
    pub default_user_qps_limit: u64,
    /// Default concurrent query limit per user
    pub default_user_concurrent_limit: usize,
    /// Time window for rate limiting (in seconds)
    pub rate_limit_window_secs: u64,
    /// Enable quota enforcement
    pub enabled: bool,
    /// Maximum query complexity score
    pub max_query_complexity: u64,
    /// Maximum result rows per query
    pub max_result_rows: usize,
    /// Maximum query timeout in seconds
    pub max_query_timeout_secs: u64,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            global_qps_limit: 10000,
            global_concurrent_limit: 1000,
            default_table_qps_limit: 1000,
            default_user_qps_limit: 100,
            default_user_concurrent_limit: 10,
            rate_limit_window_secs: 1,
            enabled: true,
            max_query_complexity: 1000,
            max_result_rows: 1_000_000,
            max_query_timeout_secs: 300,
        }
    }
}

// ============== Rate Limiter ==============

/// Token bucket rate limiter
pub struct TokenBucket {
    /// Maximum tokens (capacity)
    capacity: u64,
    /// Current tokens available
    tokens: AtomicU64,
    /// Tokens added per second
    refill_rate: u64,
    /// Last refill timestamp (nanos since epoch)
    last_refill: AtomicU64,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity,
            tokens: AtomicU64::new(capacity),
            refill_rate,
            last_refill: AtomicU64::new(Self::now_nanos()),
        }
    }

    fn now_nanos() -> u64 {
        Instant::now().elapsed().as_nanos() as u64
    }

    /// Try to acquire tokens, returns true if successful
    pub fn try_acquire(&self, tokens: u64) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < tokens {
                return false;
            }
            if self.tokens.compare_exchange_weak(
                current,
                current - tokens,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                return true;
            }
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&self) {
        let now = Self::now_nanos();
        let last = self.last_refill.load(Ordering::Acquire);
        let elapsed_secs = (now.saturating_sub(last)) as f64 / 1_000_000_000.0;

        if elapsed_secs > 0.0 {
            let new_tokens = (elapsed_secs * self.refill_rate as f64) as u64;
            if new_tokens > 0 {
                if self.last_refill.compare_exchange_weak(
                    last,
                    now,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ).is_ok() {
                    let current = self.tokens.load(Ordering::Acquire);
                    let new_value = std::cmp::min(current + new_tokens, self.capacity);
                    self.tokens.store(new_value, Ordering::Release);
                }
            }
        }
    }

    /// Get current available tokens
    pub fn available(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Acquire)
    }
}

// ============== Sliding Window Counter ==============

/// Sliding window rate counter for more accurate rate limiting
pub struct SlidingWindowCounter {
    /// Window size in seconds
    window_secs: u64,
    /// Counts per second (circular buffer)
    buckets: Vec<AtomicU64>,
    /// Timestamps for each bucket
    timestamps: Vec<AtomicU64>,
    /// Maximum allowed count per window
    limit: u64,
}

impl SlidingWindowCounter {
    pub fn new(window_secs: u64, limit: u64) -> Self {
        let num_buckets = window_secs as usize + 1;
        let buckets: Vec<AtomicU64> = (0..num_buckets).map(|_| AtomicU64::new(0)).collect();
        let timestamps: Vec<AtomicU64> = (0..num_buckets).map(|_| AtomicU64::new(0)).collect();

        Self {
            window_secs,
            buckets,
            timestamps,
            limit,
        }
    }

    fn current_second() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// Increment counter, returns true if within limit
    pub fn increment(&self) -> bool {
        let now = Self::current_second();
        let bucket_idx = (now % (self.window_secs + 1)) as usize;

        // Reset bucket if it's from an old window
        let bucket_ts = self.timestamps[bucket_idx].load(Ordering::Acquire);
        if bucket_ts != now {
            if self.timestamps[bucket_idx].compare_exchange_weak(
                bucket_ts,
                now,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                self.buckets[bucket_idx].store(0, Ordering::Release);
            }
        }

        // Count total in window
        let total = self.count_in_window(now);

        if total >= self.limit {
            return false;
        }

        // Increment current bucket
        self.buckets[bucket_idx].fetch_add(1, Ordering::AcqRel);
        true
    }

    /// Count requests in the current window
    pub fn count_in_window(&self, now: u64) -> u64 {
        let mut total = 0;
        for i in 0..=self.window_secs {
            let bucket_idx = ((now.saturating_sub(i)) % (self.window_secs + 1)) as usize;
            let bucket_ts = self.timestamps[bucket_idx].load(Ordering::Acquire);
            if now.saturating_sub(bucket_ts) <= self.window_secs {
                total += self.buckets[bucket_idx].load(Ordering::Acquire);
            }
        }
        total
    }

    /// Get current count
    pub fn count(&self) -> u64 {
        self.count_in_window(Self::current_second())
    }
}

// ============== Concurrent Query Tracker ==============

/// Tracks concurrent queries per user/table
pub struct ConcurrentQueryTracker {
    /// User -> concurrent count
    user_counts: RwLock<AHashMap<String, AtomicUsize>>,
    /// Table -> concurrent count
    table_counts: RwLock<AHashMap<String, AtomicUsize>>,
    /// Global concurrent count
    global_count: AtomicUsize,
    /// User limits
    user_limits: RwLock<AHashMap<String, usize>>,
    /// Default user limit
    default_user_limit: usize,
    /// Global limit
    global_limit: usize,
}

impl ConcurrentQueryTracker {
    pub fn new(global_limit: usize, default_user_limit: usize) -> Self {
        Self {
            user_counts: RwLock::new(AHashMap::new()),
            table_counts: RwLock::new(AHashMap::new()),
            global_count: AtomicUsize::new(0),
            user_limits: RwLock::new(AHashMap::new()),
            default_user_limit,
            global_limit,
        }
    }

    /// Set custom limit for a user
    pub fn set_user_limit(&self, user: impl Into<String>, limit: usize) {
        let mut limits = self.user_limits.write();
        limits.insert(user.into(), limit);
    }

    /// Try to start a query, returns a guard if successful
    pub fn try_start(self: &Arc<Self>, user: &str, table: &str) -> Option<QueryGuard> {
        // Check global limit
        let current_global = self.global_count.load(Ordering::Acquire);
        if current_global >= self.global_limit {
            return None;
        }

        // Check user limit
        let user_limit = {
            let limits = self.user_limits.read();
            limits.get(user).copied().unwrap_or(self.default_user_limit)
        };

        {
            let counts = self.user_counts.read();
            if let Some(count) = counts.get(user) {
                if count.load(Ordering::Acquire) >= user_limit {
                    return None;
                }
            }
        }

        // Increment counts
        self.global_count.fetch_add(1, Ordering::AcqRel);

        {
            let mut counts = self.user_counts.write();
            counts
                .entry(user.to_string())
                .or_insert_with(|| AtomicUsize::new(0))
                .fetch_add(1, Ordering::AcqRel);
        }

        {
            let mut counts = self.table_counts.write();
            counts
                .entry(table.to_string())
                .or_insert_with(|| AtomicUsize::new(0))
                .fetch_add(1, Ordering::AcqRel);
        }

        Some(QueryGuard {
            user: user.to_string(),
            table: table.to_string(),
            tracker: Arc::clone(self),
        })
    }

    /// End a query (called by QueryGuard on drop)
    fn end_query(&self, user: &str, table: &str) {
        self.global_count.fetch_sub(1, Ordering::AcqRel);

        {
            let counts = self.user_counts.read();
            if let Some(count) = counts.get(user) {
                count.fetch_sub(1, Ordering::AcqRel);
            }
        }

        {
            let counts = self.table_counts.read();
            if let Some(count) = counts.get(table) {
                count.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    /// Get current global count
    pub fn global_count(&self) -> usize {
        self.global_count.load(Ordering::Acquire)
    }

    /// Get current user count
    pub fn user_count(&self, user: &str) -> usize {
        let counts = self.user_counts.read();
        counts
            .get(user)
            .map(|c| c.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    /// Get current table count
    pub fn table_count(&self, table: &str) -> usize {
        let counts = self.table_counts.read();
        counts
            .get(table)
            .map(|c| c.load(Ordering::Acquire))
            .unwrap_or(0)
    }
}

/// RAII guard for tracking query lifecycle
pub struct QueryGuard {
    user: String,
    table: String,
    tracker: Arc<ConcurrentQueryTracker>,
}

impl Drop for QueryGuard {
    fn drop(&mut self) {
        self.tracker.end_query(&self.user, &self.table);
    }
}

// ============== Quota Manager ==============

/// Main quota management service
pub struct QuotaManager {
    config: QuotaConfig,
    /// Global rate limiter
    global_rate_limiter: TokenBucket,
    /// Per-user rate limiters
    user_rate_limiters: RwLock<AHashMap<String, Arc<SlidingWindowCounter>>>,
    /// Per-table rate limiters
    table_rate_limiters: RwLock<AHashMap<String, Arc<SlidingWindowCounter>>>,
    /// Concurrent query tracker
    concurrent_tracker: Arc<ConcurrentQueryTracker>,
    /// Custom user QPS limits
    user_qps_limits: RwLock<AHashMap<String, u64>>,
    /// Custom table QPS limits
    table_qps_limits: RwLock<AHashMap<String, u64>>,
}

impl QuotaManager {
    pub fn new(config: QuotaConfig) -> Self {
        let global_rate_limiter = TokenBucket::new(
            config.global_qps_limit,
            config.global_qps_limit,
        );

        let concurrent_tracker = Arc::new(ConcurrentQueryTracker::new(
            config.global_concurrent_limit,
            config.default_user_concurrent_limit,
        ));

        Self {
            config,
            global_rate_limiter,
            user_rate_limiters: RwLock::new(AHashMap::new()),
            table_rate_limiters: RwLock::new(AHashMap::new()),
            concurrent_tracker,
            user_qps_limits: RwLock::new(AHashMap::new()),
            table_qps_limits: RwLock::new(AHashMap::new()),
        }
    }

    /// Set custom QPS limit for a user
    pub fn set_user_qps_limit(&self, user: impl Into<String>, qps: u64) {
        let mut limits = self.user_qps_limits.write();
        limits.insert(user.into(), qps);
    }

    /// Set custom QPS limit for a table
    pub fn set_table_qps_limit(&self, table: impl Into<String>, qps: u64) {
        let mut limits = self.table_qps_limits.write();
        limits.insert(table.into(), qps);
    }

    /// Set custom concurrent limit for a user
    pub fn set_user_concurrent_limit(&self, user: impl Into<String>, limit: usize) {
        self.concurrent_tracker.set_user_limit(user, limit);
    }

    /// Get or create user rate limiter
    fn get_user_rate_limiter(&self, user: &str) -> Arc<SlidingWindowCounter> {
        {
            let limiters = self.user_rate_limiters.read();
            if let Some(limiter) = limiters.get(user) {
                return limiter.clone();
            }
        }

        let qps_limit = {
            let limits = self.user_qps_limits.read();
            limits.get(user).copied().unwrap_or(self.config.default_user_qps_limit)
        };

        let limiter = Arc::new(SlidingWindowCounter::new(
            self.config.rate_limit_window_secs,
            qps_limit,
        ));

        let mut limiters = self.user_rate_limiters.write();
        limiters.insert(user.to_string(), limiter.clone());
        limiter
    }

    /// Get or create table rate limiter
    fn get_table_rate_limiter(&self, table: &str) -> Arc<SlidingWindowCounter> {
        {
            let limiters = self.table_rate_limiters.read();
            if let Some(limiter) = limiters.get(table) {
                return limiter.clone();
            }
        }

        let qps_limit = {
            let limits = self.table_qps_limits.read();
            limits.get(table).copied().unwrap_or(self.config.default_table_qps_limit)
        };

        let limiter = Arc::new(SlidingWindowCounter::new(
            self.config.rate_limit_window_secs,
            qps_limit,
        ));

        let mut limiters = self.table_rate_limiters.write();
        limiters.insert(table.to_string(), limiter.clone());
        limiter
    }

    /// Check if a query can proceed based on quotas
    pub fn check_quota(&self, user: &str, table: &str) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Check global rate limit
        if !self.global_rate_limiter.try_acquire(1) {
            return Err(BrokerError::QuotaExceeded {
                limit_type: "global_qps".to_string(),
                limit: self.config.global_qps_limit,
                current: self.config.global_qps_limit,
            });
        }

        // Check user rate limit
        let user_limiter = self.get_user_rate_limiter(user);
        if !user_limiter.increment() {
            return Err(BrokerError::QuotaExceeded {
                limit_type: "user_qps".to_string(),
                limit: self.config.default_user_qps_limit,
                current: user_limiter.count(),
            });
        }

        // Check table rate limit
        let table_limiter = self.get_table_rate_limiter(table);
        if !table_limiter.increment() {
            return Err(BrokerError::QuotaExceeded {
                limit_type: "table_qps".to_string(),
                limit: self.config.default_table_qps_limit,
                current: table_limiter.count(),
            });
        }

        Ok(())
    }

    /// Try to acquire a query slot (for concurrent limit)
    pub fn try_acquire_query_slot(&self, user: &str, table: &str) -> Result<QueryGuard> {
        if !self.config.enabled {
            // When disabled, still return a guard that tracks properly
            return Ok(QueryGuard {
                user: user.to_string(),
                table: table.to_string(),
                tracker: Arc::clone(&self.concurrent_tracker),
            });
        }

        self.concurrent_tracker
            .try_start(user, table)
            .ok_or_else(|| BrokerError::QuotaExceeded {
                limit_type: "concurrent_queries".to_string(),
                limit: self.config.global_concurrent_limit as u64,
                current: self.concurrent_tracker.global_count() as u64,
            })
    }

    /// Validate query complexity
    pub fn check_query_complexity(&self, complexity_score: u64) -> Result<()> {
        if complexity_score > self.config.max_query_complexity {
            return Err(BrokerError::QueryTooComplex {
                complexity: complexity_score,
                max_allowed: self.config.max_query_complexity,
            });
        }
        Ok(())
    }

    /// Validate result row limit
    pub fn check_result_limit(&self, requested_rows: usize) -> Result<usize> {
        Ok(std::cmp::min(requested_rows, self.config.max_result_rows))
    }

    /// Validate query timeout
    pub fn check_timeout(&self, requested_timeout_secs: u64) -> Result<Duration> {
        let effective_timeout = std::cmp::min(
            requested_timeout_secs,
            self.config.max_query_timeout_secs,
        );
        Ok(Duration::from_secs(effective_timeout))
    }

    /// Get quota stats
    pub fn get_stats(&self) -> QuotaStats {
        QuotaStats {
            global_qps_available: self.global_rate_limiter.available(),
            global_concurrent: self.concurrent_tracker.global_count(),
            global_concurrent_limit: self.config.global_concurrent_limit,
        }
    }
}

/// Quota statistics
#[derive(Debug, Clone)]
pub struct QuotaStats {
    pub global_qps_available: u64,
    pub global_concurrent: usize,
    pub global_concurrent_limit: usize,
}

impl Default for QuotaManager {
    fn default() -> Self {
        Self::new(QuotaConfig::default())
    }
}

// ============== Tests ==============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket() {
        let bucket = TokenBucket::new(10, 10);

        // Should have initial tokens
        assert!(bucket.try_acquire(5));
        assert!(bucket.try_acquire(5));

        // Should be empty now
        assert!(!bucket.try_acquire(1));
    }

    #[test]
    fn test_sliding_window_counter() {
        let counter = SlidingWindowCounter::new(1, 5);

        // Should allow up to limit
        for _ in 0..5 {
            assert!(counter.increment());
        }

        // Should reject over limit
        assert!(!counter.increment());

        assert_eq!(counter.count(), 5);
    }

    #[test]
    fn test_concurrent_tracker() {
        let tracker = Arc::new(ConcurrentQueryTracker::new(10, 3));

        // Start queries
        let guard1 = tracker.try_start("user1", "table1");
        let guard2 = tracker.try_start("user1", "table1");
        let guard3 = tracker.try_start("user1", "table1");

        assert!(guard1.is_some());
        assert!(guard2.is_some());
        assert!(guard3.is_some());

        // Should hit user limit
        let guard4 = tracker.try_start("user1", "table1");
        assert!(guard4.is_none());

        // Different user should work
        let guard5 = tracker.try_start("user2", "table1");
        assert!(guard5.is_some());

        assert_eq!(tracker.global_count(), 4);
        assert_eq!(tracker.user_count("user1"), 3);
    }

    #[test]
    fn test_quota_manager_disabled() {
        let config = QuotaConfig {
            enabled: false,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        // Should always succeed when disabled
        assert!(manager.check_quota("user", "table").is_ok());
        assert!(manager.try_acquire_query_slot("user", "table").is_ok());
    }

    #[test]
    fn test_quota_manager_qps_limit() {
        let config = QuotaConfig {
            enabled: true,
            default_user_qps_limit: 3,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        // Should allow up to limit
        assert!(manager.check_quota("user1", "table1").is_ok());
        assert!(manager.check_quota("user1", "table1").is_ok());
        assert!(manager.check_quota("user1", "table1").is_ok());

        // Should reject over limit
        assert!(manager.check_quota("user1", "table1").is_err());

        // Different user should work
        assert!(manager.check_quota("user2", "table1").is_ok());
    }

    #[test]
    fn test_custom_limits() {
        let config = QuotaConfig {
            enabled: true,
            default_user_qps_limit: 3,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        // Set higher limit for specific user
        manager.set_user_qps_limit("vip_user", 100);

        // VIP user should have higher limit
        for _ in 0..10 {
            assert!(manager.check_quota("vip_user", "table1").is_ok());
        }
    }

    #[test]
    fn test_query_complexity() {
        let config = QuotaConfig {
            max_query_complexity: 100,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        assert!(manager.check_query_complexity(50).is_ok());
        assert!(manager.check_query_complexity(100).is_ok());
        assert!(manager.check_query_complexity(101).is_err());
    }

    #[test]
    fn test_result_limit() {
        let config = QuotaConfig {
            max_result_rows: 1000,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        assert_eq!(manager.check_result_limit(500).unwrap(), 500);
        assert_eq!(manager.check_result_limit(5000).unwrap(), 1000);
    }

    #[test]
    fn test_timeout_limit() {
        let config = QuotaConfig {
            max_query_timeout_secs: 60,
            ..Default::default()
        };
        let manager = QuotaManager::new(config);

        assert_eq!(
            manager.check_timeout(30).unwrap(),
            Duration::from_secs(30)
        );
        assert_eq!(
            manager.check_timeout(300).unwrap(),
            Duration::from_secs(60)
        );
    }
}
