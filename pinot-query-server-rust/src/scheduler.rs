//! Query scheduler for multi-stage execution
//!
//! This module provides scheduling and coordination for multi-stage query execution
//! with memory-aware resource management.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use tokio::sync::{mpsc, Notify, Semaphore};
use tokio::task::JoinHandle;

use crate::backpressure::{MemoryBackpressureManager, MemoryPressureLevel};
use crate::block::BlockStats;
use crate::config::QueryServerConfig;
use crate::error::{QueryError, Result};
use crate::operator::OpChain;

/// Query state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryState {
    /// Query is queued
    Queued,
    /// Query is running
    Running,
    /// Query completed successfully
    Completed,
    /// Query failed
    Failed,
    /// Query was cancelled
    Cancelled,
    /// Query timed out
    TimedOut,
}

/// Query priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueryPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Default for QueryPriority {
    fn default() -> Self {
        QueryPriority::Normal
    }
}

/// Query information
pub struct QueryInfo {
    /// Query ID
    pub id: String,
    /// Query state
    pub state: RwLock<QueryState>,
    /// Priority
    pub priority: QueryPriority,
    /// Estimated memory usage
    pub estimated_memory: usize,
    /// Actual memory used (updated during execution)
    pub memory_used: AtomicUsize,
    /// Number of stages
    pub num_stages: usize,
    /// Stages completed
    pub stages_completed: AtomicUsize,
    /// Submit time
    pub submit_time: Instant,
    /// Start time (when execution began)
    pub start_time: RwLock<Option<Instant>>,
    /// End time
    pub end_time: RwLock<Option<Instant>>,
    /// Timeout
    pub timeout: Duration,
    /// Statistics per stage
    pub stage_stats: DashMap<i32, BlockStats>,
    /// Error message if failed
    pub error: RwLock<Option<String>>,
    /// Cancellation notify
    pub cancel_notify: Notify,
}

impl QueryInfo {
    pub fn new(
        id: impl Into<String>,
        priority: QueryPriority,
        estimated_memory: usize,
        num_stages: usize,
        timeout: Duration,
    ) -> Self {
        Self {
            id: id.into(),
            state: RwLock::new(QueryState::Queued),
            priority,
            estimated_memory,
            memory_used: AtomicUsize::new(0),
            num_stages,
            stages_completed: AtomicUsize::new(0),
            submit_time: Instant::now(),
            start_time: RwLock::new(None),
            end_time: RwLock::new(None),
            timeout,
            stage_stats: DashMap::new(),
            error: RwLock::new(None),
            cancel_notify: Notify::new(),
        }
    }

    pub fn start(&self) {
        *self.state.write() = QueryState::Running;
        *self.start_time.write() = Some(Instant::now());
    }

    pub fn complete(&self) {
        *self.state.write() = QueryState::Completed;
        *self.end_time.write() = Some(Instant::now());
    }

    pub fn fail(&self, error: impl Into<String>) {
        *self.state.write() = QueryState::Failed;
        *self.end_time.write() = Some(Instant::now());
        *self.error.write() = Some(error.into());
    }

    pub fn cancel(&self) {
        *self.state.write() = QueryState::Cancelled;
        *self.end_time.write() = Some(Instant::now());
        self.cancel_notify.notify_waiters();
    }

    pub fn timeout_query(&self) {
        *self.state.write() = QueryState::TimedOut;
        *self.end_time.write() = Some(Instant::now());
    }

    pub fn is_done(&self) -> bool {
        matches!(
            *self.state.read(),
            QueryState::Completed | QueryState::Failed | QueryState::Cancelled | QueryState::TimedOut
        )
    }

    pub fn elapsed(&self) -> Duration {
        if let Some(start) = *self.start_time.read() {
            start.elapsed()
        } else {
            self.submit_time.elapsed()
        }
    }

    pub fn is_timed_out(&self) -> bool {
        self.elapsed() > self.timeout
    }

    pub fn record_stage_complete(&self, stage_id: i32, stats: BlockStats) {
        self.stages_completed.fetch_add(1, Ordering::Relaxed);
        self.stage_stats.insert(stage_id, stats);
    }
}

/// Scheduler statistics
#[derive(Debug, Default)]
pub struct SchedulerStats {
    /// Total queries submitted
    pub queries_submitted: AtomicU64,
    /// Queries completed successfully
    pub queries_completed: AtomicU64,
    /// Queries failed
    pub queries_failed: AtomicU64,
    /// Queries cancelled
    pub queries_cancelled: AtomicU64,
    /// Queries timed out
    pub queries_timed_out: AtomicU64,
    /// Currently running queries
    pub queries_running: AtomicUsize,
    /// Currently queued queries
    pub queries_queued: AtomicUsize,
    /// Total execution time (microseconds)
    pub total_execution_us: AtomicU64,
    /// Total queue wait time (microseconds)
    pub total_queue_wait_us: AtomicU64,
}

impl SchedulerStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn average_execution_time(&self) -> Duration {
        let completed = self.queries_completed.load(Ordering::Relaxed);
        if completed == 0 {
            Duration::ZERO
        } else {
            let total_us = self.total_execution_us.load(Ordering::Relaxed);
            Duration::from_micros(total_us / completed)
        }
    }

    pub fn average_queue_wait(&self) -> Duration {
        let submitted = self.queries_submitted.load(Ordering::Relaxed);
        if submitted == 0 {
            Duration::ZERO
        } else {
            let total_us = self.total_queue_wait_us.load(Ordering::Relaxed);
            Duration::from_micros(total_us / submitted)
        }
    }
}

/// Query scheduler
pub struct QueryScheduler {
    /// Configuration
    config: Arc<QueryServerConfig>,
    /// Backpressure manager
    backpressure: Arc<MemoryBackpressureManager>,
    /// Query registry
    queries: DashMap<String, Arc<QueryInfo>>,
    /// Priority queues
    priority_queues: Vec<Mutex<VecDeque<String>>>,
    /// Concurrency semaphore
    concurrency_semaphore: Semaphore,
    /// Maximum concurrent queries
    max_concurrent: usize,
    /// Statistics
    stats: Arc<SchedulerStats>,
    /// Shutdown flag
    shutdown: AtomicBool,
    /// Scheduler notify
    schedule_notify: Notify,
}

impl QueryScheduler {
    /// Create a new scheduler
    pub fn new(config: Arc<QueryServerConfig>, backpressure: Arc<MemoryBackpressureManager>) -> Self {
        let num_cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);
        let max_concurrent = num_cpus * 2;

        Self {
            config,
            backpressure,
            queries: DashMap::new(),
            priority_queues: (0..4).map(|_| Mutex::new(VecDeque::new())).collect(),
            concurrency_semaphore: Semaphore::new(max_concurrent),
            max_concurrent,
            stats: Arc::new(SchedulerStats::new()),
            shutdown: AtomicBool::new(false),
            schedule_notify: Notify::new(),
        }
    }

    /// Submit a query for execution
    pub fn submit(&self, query: Arc<QueryInfo>) -> Result<()> {
        if self.shutdown.load(Ordering::Relaxed) {
            return Err(QueryError::Internal("Scheduler is shutdown".to_string()));
        }

        // Check memory pressure
        let pressure = self.backpressure.pressure_level();
        if pressure == MemoryPressureLevel::Critical && query.priority != QueryPriority::Critical {
            return Err(QueryError::MemoryPressure {
                used_bytes: self.backpressure.reserved_bytes(),
                limit_bytes: self.config.memory.max_memory_bytes,
            });
        }

        let query_id = query.id.clone();
        let priority = query.priority as usize;

        self.queries.insert(query_id.clone(), query);
        self.priority_queues[priority].lock().push_back(query_id);

        self.stats.queries_submitted.fetch_add(1, Ordering::Relaxed);
        self.stats.queries_queued.fetch_add(1, Ordering::Relaxed);

        self.schedule_notify.notify_one();

        Ok(())
    }

    /// Get next query to execute (priority-based)
    pub async fn next_query(&self) -> Option<Arc<QueryInfo>> {
        if self.shutdown.load(Ordering::Relaxed) {
            return None;
        }

        // Try to acquire semaphore
        let permit = self.concurrency_semaphore.try_acquire().ok()?;

        // Find highest priority non-empty queue
        for priority in (0..4).rev() {
            let mut queue = self.priority_queues[priority].lock();
            while let Some(query_id) = queue.pop_front() {
                if let Some(query) = self.queries.get(&query_id) {
                    if !query.is_done() {
                        self.stats.queries_queued.fetch_sub(1, Ordering::Relaxed);
                        self.stats.queries_running.fetch_add(1, Ordering::Relaxed);

                        let wait_time = query.submit_time.elapsed();
                        self.stats
                            .total_queue_wait_us
                            .fetch_add(wait_time.as_micros() as u64, Ordering::Relaxed);

                        query.start();
                        std::mem::forget(permit); // Keep permit until query completes
                        return Some(query.clone());
                    }
                }
            }
        }

        // No query available, release permit
        drop(permit);
        None
    }

    /// Wait for next query
    pub async fn wait_for_query(&self) -> Option<Arc<QueryInfo>> {
        loop {
            if let Some(query) = self.next_query().await {
                return Some(query);
            }

            if self.shutdown.load(Ordering::Relaxed) {
                return None;
            }

            self.schedule_notify.notified().await;
        }
    }

    /// Mark query as completed
    pub fn complete_query(&self, query_id: &str, success: bool, error: Option<String>) {
        if let Some(query) = self.queries.get(query_id) {
            if success {
                query.complete();
                self.stats.queries_completed.fetch_add(1, Ordering::Relaxed);
            } else {
                query.fail(error.unwrap_or_default());
                self.stats.queries_failed.fetch_add(1, Ordering::Relaxed);
            }

            let execution_time = query.elapsed();
            self.stats
                .total_execution_us
                .fetch_add(execution_time.as_micros() as u64, Ordering::Relaxed);
        }

        self.stats.queries_running.fetch_sub(1, Ordering::Relaxed);
        self.concurrency_semaphore.add_permits(1);
        self.schedule_notify.notify_one();
    }

    /// Cancel a query
    pub fn cancel_query(&self, query_id: &str) -> bool {
        if let Some(query) = self.queries.get(query_id) {
            if !query.is_done() {
                query.cancel();
                self.stats.queries_cancelled.fetch_add(1, Ordering::Relaxed);
                if *query.state.read() == QueryState::Running {
                    self.stats.queries_running.fetch_sub(1, Ordering::Relaxed);
                    self.concurrency_semaphore.add_permits(1);
                } else {
                    self.stats.queries_queued.fetch_sub(1, Ordering::Relaxed);
                }
                return true;
            }
        }
        false
    }

    /// Get query info
    pub fn get_query(&self, query_id: &str) -> Option<Arc<QueryInfo>> {
        self.queries.get(query_id).map(|q| q.clone())
    }

    /// Get all running queries
    pub fn running_queries(&self) -> Vec<Arc<QueryInfo>> {
        self.queries
            .iter()
            .filter(|q| *q.state.read() == QueryState::Running)
            .map(|q| q.clone())
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Check for timed out queries
    pub fn check_timeouts(&self) {
        for query in self.queries.iter() {
            if !query.is_done() && query.is_timed_out() {
                query.timeout_query();
                self.stats.queries_timed_out.fetch_add(1, Ordering::Relaxed);
                if *query.state.read() == QueryState::Running {
                    self.stats.queries_running.fetch_sub(1, Ordering::Relaxed);
                    self.concurrency_semaphore.add_permits(1);
                }
            }
        }
    }

    /// Clean up completed queries older than duration
    pub fn cleanup(&self, max_age: Duration) {
        self.queries.retain(|_, query| {
            if let Some(end_time) = *query.end_time.read() {
                end_time.elapsed() < max_age
            } else {
                true
            }
        });
    }

    /// Get number of running queries
    pub fn running_count(&self) -> usize {
        self.stats.queries_running.load(Ordering::Relaxed)
    }

    /// Get number of queued queries
    pub fn queued_count(&self) -> usize {
        self.stats.queries_queued.load(Ordering::Relaxed)
    }

    /// Shutdown the scheduler
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);

        // Cancel all running/queued queries
        for query in self.queries.iter() {
            if !query.is_done() {
                query.cancel();
            }
        }

        self.schedule_notify.notify_waiters();
    }

    /// Check if scheduler is shutdown
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}

/// Stage executor for running operator chains
pub struct StageExecutor {
    /// Query info
    query: Arc<QueryInfo>,
    /// Stage ID
    stage_id: i32,
    /// Operator chain
    op_chain: OpChain,
    /// Backpressure manager
    backpressure: Arc<MemoryBackpressureManager>,
}

impl StageExecutor {
    pub fn new(
        query: Arc<QueryInfo>,
        stage_id: i32,
        op_chain: OpChain,
        backpressure: Arc<MemoryBackpressureManager>,
    ) -> Self {
        Self {
            query,
            stage_id,
            op_chain,
            backpressure,
        }
    }

    /// Execute the stage
    pub async fn execute(&self) -> Result<BlockStats> {
        // Check for cancellation
        if self.query.is_done() {
            return Err(QueryError::QueryCancelled {
                query_id: self.query.id.clone(),
                reason: "Query already terminated".to_string(),
            });
        }

        // Check timeout
        if self.query.is_timed_out() {
            return Err(QueryError::QueryTimeout {
                query_id: self.query.id.clone(),
                timeout_ms: self.query.timeout.as_millis() as u64,
            });
        }

        // Reserve memory
        let reservation = self
            .backpressure
            .reserve(self.query.estimated_memory)
            .await?;

        // Execute operator chain
        let results = tokio::select! {
            result = self.op_chain.execute() => result,
            _ = self.query.cancel_notify.notified() => {
                Err(QueryError::QueryCancelled {
                    query_id: self.query.id.clone(),
                    reason: "Cancelled during execution".to_string(),
                })
            }
        };

        drop(reservation);

        let stats = self.op_chain.stats();
        self.query.record_stage_complete(self.stage_id, stats.clone());

        results.map(|_| stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MemoryConfig;

    fn create_test_scheduler() -> (QueryScheduler, Arc<MemoryBackpressureManager>) {
        let config = Arc::new(QueryServerConfig::default());
        let backpressure = Arc::new(MemoryBackpressureManager::new(MemoryConfig::default()));
        let scheduler = QueryScheduler::new(config, backpressure.clone());
        (scheduler, backpressure)
    }

    #[test]
    fn test_query_state() {
        let query = QueryInfo::new("q1", QueryPriority::Normal, 1024, 3, Duration::from_secs(30));

        assert_eq!(*query.state.read(), QueryState::Queued);
        assert!(!query.is_done());

        query.start();
        assert_eq!(*query.state.read(), QueryState::Running);

        query.complete();
        assert_eq!(*query.state.read(), QueryState::Completed);
        assert!(query.is_done());
    }

    #[test]
    fn test_query_priority_ordering() {
        assert!(QueryPriority::Low < QueryPriority::Normal);
        assert!(QueryPriority::Normal < QueryPriority::High);
        assert!(QueryPriority::High < QueryPriority::Critical);
    }

    #[tokio::test]
    async fn test_scheduler_submit() {
        let (scheduler, _) = create_test_scheduler();

        let query = Arc::new(QueryInfo::new(
            "q1",
            QueryPriority::Normal,
            1024,
            1,
            Duration::from_secs(30),
        ));

        scheduler.submit(query).unwrap();

        assert_eq!(scheduler.queued_count(), 1);
        assert_eq!(scheduler.stats.queries_submitted.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_scheduler_priority() {
        let (scheduler, _) = create_test_scheduler();

        // Submit low priority
        let q1 = Arc::new(QueryInfo::new(
            "q1",
            QueryPriority::Low,
            1024,
            1,
            Duration::from_secs(30),
        ));
        scheduler.submit(q1).unwrap();

        // Submit high priority
        let q2 = Arc::new(QueryInfo::new(
            "q2",
            QueryPriority::High,
            1024,
            1,
            Duration::from_secs(30),
        ));
        scheduler.submit(q2).unwrap();

        // High priority should come first
        let next = scheduler.next_query().await.unwrap();
        assert_eq!(next.id, "q2");
    }

    #[tokio::test]
    async fn test_scheduler_complete() {
        let (scheduler, _) = create_test_scheduler();

        let query = Arc::new(QueryInfo::new(
            "q1",
            QueryPriority::Normal,
            1024,
            1,
            Duration::from_secs(30),
        ));

        scheduler.submit(query.clone()).unwrap();

        let next = scheduler.next_query().await.unwrap();
        assert_eq!(scheduler.running_count(), 1);

        scheduler.complete_query("q1", true, None);
        assert_eq!(scheduler.running_count(), 0);
        assert_eq!(scheduler.stats.queries_completed.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_scheduler_cancel() {
        let (scheduler, _) = create_test_scheduler();

        let query = Arc::new(QueryInfo::new(
            "q1",
            QueryPriority::Normal,
            1024,
            1,
            Duration::from_secs(30),
        ));

        scheduler.submit(query).unwrap();
        assert!(scheduler.cancel_query("q1"));
        assert_eq!(scheduler.stats.queries_cancelled.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_scheduler_stats() {
        let stats = SchedulerStats::new();

        stats.queries_completed.fetch_add(10, Ordering::Relaxed);
        stats.total_execution_us.fetch_add(1000000, Ordering::Relaxed);

        let avg = stats.average_execution_time();
        assert_eq!(avg, Duration::from_micros(100000));
    }

    #[test]
    fn test_query_timeout() {
        let query = QueryInfo::new("q1", QueryPriority::Normal, 1024, 1, Duration::from_millis(1));

        std::thread::sleep(Duration::from_millis(10));
        assert!(query.is_timed_out());
    }
}
