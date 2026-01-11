//! Query Cancellation and Timeout Handling Module
//!
//! Provides mechanisms for cancelling queries, handling timeouts,
//! and managing query lifecycle.

use crate::error::{BrokerError, Result};
use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::time::timeout;

// ============== Query State ==============

/// State of a query in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryState {
    /// Query is being planned/routed
    Planning,
    /// Query is being executed on servers
    Executing,
    /// Query is being reduced/aggregated
    Reducing,
    /// Query completed successfully
    Completed,
    /// Query was cancelled
    Cancelled,
    /// Query failed
    Failed,
    /// Query timed out
    TimedOut,
}

/// Metadata for a tracked query
#[derive(Debug)]
pub struct QueryMetadata {
    /// Unique query ID
    pub query_id: u64,
    /// Table being queried
    pub table: String,
    /// User who submitted the query
    pub user: String,
    /// Query submission time
    pub submitted_at: Instant,
    /// Query timeout
    pub timeout: Duration,
    /// Current state
    state: AtomicU64, // QueryState encoded as u64
    /// Cancellation token
    cancel_requested: AtomicBool,
    /// Notification for cancellation
    cancel_notify: Notify,
}

impl QueryMetadata {
    pub fn new(
        query_id: u64,
        table: impl Into<String>,
        user: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            query_id,
            table: table.into(),
            user: user.into(),
            submitted_at: Instant::now(),
            timeout,
            state: AtomicU64::new(QueryState::Planning as u64),
            cancel_requested: AtomicBool::new(false),
            cancel_notify: Notify::new(),
        }
    }

    /// Get current state
    pub fn state(&self) -> QueryState {
        match self.state.load(Ordering::Acquire) {
            0 => QueryState::Planning,
            1 => QueryState::Executing,
            2 => QueryState::Reducing,
            3 => QueryState::Completed,
            4 => QueryState::Cancelled,
            5 => QueryState::Failed,
            6 => QueryState::TimedOut,
            _ => QueryState::Failed,
        }
    }

    /// Set state
    pub fn set_state(&self, state: QueryState) {
        self.state.store(state as u64, Ordering::Release);
    }

    /// Check if cancellation was requested
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }

    /// Request cancellation
    pub fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
    }

    /// Wait for cancellation
    pub async fn wait_for_cancel(&self) {
        self.cancel_notify.notified().await;
    }

    /// Get elapsed time since submission
    pub fn elapsed(&self) -> Duration {
        self.submitted_at.elapsed()
    }

    /// Get remaining time until timeout
    pub fn remaining(&self) -> Option<Duration> {
        self.timeout.checked_sub(self.elapsed())
    }

    /// Check if query has timed out
    pub fn is_timed_out(&self) -> bool {
        self.elapsed() >= self.timeout
    }

    /// Check if query is still active
    pub fn is_active(&self) -> bool {
        matches!(
            self.state(),
            QueryState::Planning | QueryState::Executing | QueryState::Reducing
        )
    }
}

// ============== Cancellation Token ==============

/// Token for cancelling a query
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Check if cancellation was requested
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Cancel the operation
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// Wait for cancellation
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    /// Create a child token that's cancelled when either parent or child is cancelled
    pub fn child_token(&self) -> CancellationToken {
        let child = CancellationToken::new();
        if self.is_cancelled() {
            child.cancel();
        }
        child
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

// ============== Query Manager ==============

/// Manages active queries and their lifecycle
pub struct QueryManager {
    /// Active queries indexed by query ID
    queries: RwLock<AHashMap<u64, Arc<QueryMetadata>>>,
    /// Next query ID
    next_query_id: AtomicU64,
    /// Default timeout
    default_timeout: Duration,
    /// Maximum queries to track
    max_tracked_queries: usize,
}

impl QueryManager {
    pub fn new(default_timeout: Duration, max_tracked_queries: usize) -> Self {
        Self {
            queries: RwLock::new(AHashMap::new()),
            next_query_id: AtomicU64::new(1),
            default_timeout,
            max_tracked_queries,
        }
    }

    /// Register a new query
    pub fn register_query(
        &self,
        table: impl Into<String>,
        user: impl Into<String>,
        timeout: Option<Duration>,
    ) -> Arc<QueryMetadata> {
        let query_id = self.next_query_id.fetch_add(1, Ordering::AcqRel);
        let timeout = timeout.unwrap_or(self.default_timeout);

        let metadata = Arc::new(QueryMetadata::new(query_id, table, user, timeout));

        // Clean up old queries if we're at capacity
        {
            let queries = self.queries.read();
            if queries.len() >= self.max_tracked_queries {
                drop(queries);
                self.cleanup_completed_queries();
            }
        }

        {
            let mut queries = self.queries.write();
            queries.insert(query_id, metadata.clone());
        }

        metadata
    }

    /// Get query metadata by ID
    pub fn get_query(&self, query_id: u64) -> Option<Arc<QueryMetadata>> {
        let queries = self.queries.read();
        queries.get(&query_id).cloned()
    }

    /// Cancel a query by ID
    pub fn cancel_query(&self, query_id: u64) -> Result<()> {
        let queries = self.queries.read();
        match queries.get(&query_id) {
            Some(query) => {
                if !query.is_active() {
                    return Err(BrokerError::QueryAlreadyCancelled { query_id });
                }
                query.request_cancel();
                query.set_state(QueryState::Cancelled);
                Ok(())
            }
            None => Err(BrokerError::InvalidQuery(format!(
                "Query {} not found",
                query_id
            ))),
        }
    }

    /// Mark query as completed
    pub fn complete_query(&self, query_id: u64, success: bool) {
        let queries = self.queries.read();
        if let Some(query) = queries.get(&query_id) {
            query.set_state(if success {
                QueryState::Completed
            } else {
                QueryState::Failed
            });
        }
    }

    /// Update query state
    pub fn update_state(&self, query_id: u64, state: QueryState) {
        let queries = self.queries.read();
        if let Some(query) = queries.get(&query_id) {
            query.set_state(state);
        }
    }

    /// Check for timed out queries
    pub fn check_timeouts(&self) -> Vec<u64> {
        let queries = self.queries.read();
        let mut timed_out = Vec::new();

        for (query_id, metadata) in queries.iter() {
            if metadata.is_active() && metadata.is_timed_out() {
                metadata.set_state(QueryState::TimedOut);
                metadata.request_cancel();
                timed_out.push(*query_id);
            }
        }

        timed_out
    }

    /// Get all active queries
    pub fn active_queries(&self) -> Vec<Arc<QueryMetadata>> {
        let queries = self.queries.read();
        queries
            .values()
            .filter(|q| q.is_active())
            .cloned()
            .collect()
    }

    /// Get queries for a user
    pub fn user_queries(&self, user: &str) -> Vec<Arc<QueryMetadata>> {
        let queries = self.queries.read();
        queries
            .values()
            .filter(|q| q.user == user)
            .cloned()
            .collect()
    }

    /// Get queries for a table
    pub fn table_queries(&self, table: &str) -> Vec<Arc<QueryMetadata>> {
        let queries = self.queries.read();
        queries
            .values()
            .filter(|q| q.table == table)
            .cloned()
            .collect()
    }

    /// Cleanup completed/cancelled/failed queries
    pub fn cleanup_completed_queries(&self) {
        let mut queries = self.queries.write();
        queries.retain(|_, q| q.is_active());
    }

    /// Remove query from tracking
    pub fn remove_query(&self, query_id: u64) {
        let mut queries = self.queries.write();
        queries.remove(&query_id);
    }

    /// Get number of active queries
    pub fn active_count(&self) -> usize {
        let queries = self.queries.read();
        queries.values().filter(|q| q.is_active()).count()
    }

    /// Get total tracked queries
    pub fn total_count(&self) -> usize {
        let queries = self.queries.read();
        queries.len()
    }
}

impl Default for QueryManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(60), 10000)
    }
}

// ============== Timeout Utilities ==============

/// Execute a future with timeout and cancellation support
pub async fn with_timeout_and_cancel<F, T>(
    future: F,
    timeout_duration: Duration,
    cancel_token: &CancellationToken,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::select! {
        result = timeout(timeout_duration, future) => {
            match result {
                Ok(inner) => inner,
                Err(_) => Err(BrokerError::Timeout(timeout_duration.as_millis() as u64)),
            }
        }
        _ = cancel_token.cancelled() => {
            Err(BrokerError::Cancelled(0))
        }
    }
}

/// Execute with query metadata tracking
pub async fn execute_with_tracking<F, T>(
    query: &Arc<QueryMetadata>,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let timeout_duration = query.remaining().unwrap_or(Duration::ZERO);

    if timeout_duration.is_zero() {
        query.set_state(QueryState::TimedOut);
        return Err(BrokerError::Timeout(query.timeout.as_millis() as u64));
    }

    if query.is_cancel_requested() {
        return Err(BrokerError::Cancelled(query.query_id));
    }

    tokio::select! {
        result = timeout(timeout_duration, future) => {
            match result {
                Ok(inner) => inner,
                Err(_) => {
                    query.set_state(QueryState::TimedOut);
                    Err(BrokerError::Timeout(query.timeout.as_millis() as u64))
                }
            }
        }
        _ = query.wait_for_cancel() => {
            Err(BrokerError::Cancelled(query.query_id))
        }
    }
}

// ============== Tests ==============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_metadata() {
        let meta = QueryMetadata::new(1, "test_table", "user1", Duration::from_secs(60));

        assert_eq!(meta.query_id, 1);
        assert_eq!(meta.table, "test_table");
        assert_eq!(meta.user, "user1");
        assert_eq!(meta.state(), QueryState::Planning);
        assert!(!meta.is_cancel_requested());
        assert!(meta.is_active());
    }

    #[test]
    fn test_query_state_transitions() {
        let meta = QueryMetadata::new(1, "table", "user", Duration::from_secs(60));

        assert_eq!(meta.state(), QueryState::Planning);

        meta.set_state(QueryState::Executing);
        assert_eq!(meta.state(), QueryState::Executing);
        assert!(meta.is_active());

        meta.set_state(QueryState::Completed);
        assert_eq!(meta.state(), QueryState::Completed);
        assert!(!meta.is_active());
    }

    #[test]
    fn test_cancellation_token() {
        let token = CancellationToken::new();

        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_query_manager_register() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        let query1 = manager.register_query("table1", "user1", None);
        let query2 = manager.register_query("table2", "user2", None);

        assert_eq!(manager.total_count(), 2);
        assert_eq!(manager.active_count(), 2);

        assert!(query1.query_id < query2.query_id);
    }

    #[test]
    fn test_query_manager_cancel() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        let query = manager.register_query("table", "user", None);
        let query_id = query.query_id;

        assert!(manager.cancel_query(query_id).is_ok());
        assert_eq!(query.state(), QueryState::Cancelled);

        // Second cancel should fail
        assert!(manager.cancel_query(query_id).is_err());
    }

    #[test]
    fn test_query_manager_complete() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        let query = manager.register_query("table", "user", None);
        let query_id = query.query_id;

        manager.complete_query(query_id, true);
        assert_eq!(query.state(), QueryState::Completed);
        assert!(!query.is_active());
    }

    #[test]
    fn test_timeout_detection() {
        let manager = QueryManager::new(Duration::from_millis(1), 100);

        let query = manager.register_query("table", "user", Some(Duration::from_millis(1)));

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(5));

        assert!(query.is_timed_out());

        let timed_out = manager.check_timeouts();
        assert_eq!(timed_out.len(), 1);
        assert_eq!(query.state(), QueryState::TimedOut);
    }

    #[test]
    fn test_user_queries() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        manager.register_query("t1", "user1", None);
        manager.register_query("t2", "user1", None);
        manager.register_query("t3", "user2", None);

        let user1_queries = manager.user_queries("user1");
        assert_eq!(user1_queries.len(), 2);

        let user2_queries = manager.user_queries("user2");
        assert_eq!(user2_queries.len(), 1);
    }

    #[test]
    fn test_table_queries() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        manager.register_query("table1", "u1", None);
        manager.register_query("table1", "u2", None);
        manager.register_query("table2", "u1", None);

        let t1_queries = manager.table_queries("table1");
        assert_eq!(t1_queries.len(), 2);
    }

    #[test]
    fn test_cleanup() {
        let manager = QueryManager::new(Duration::from_secs(60), 100);

        let q1 = manager.register_query("t1", "u1", None);
        let q2 = manager.register_query("t2", "u2", None);

        manager.complete_query(q1.query_id, true);
        manager.cleanup_completed_queries();

        assert_eq!(manager.total_count(), 1);
        assert!(manager.get_query(q2.query_id).is_some());
        assert!(manager.get_query(q1.query_id).is_none());
    }

    #[tokio::test]
    async fn test_cancellation_token_async() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = token_clone.cancelled() => {
                    "cancelled"
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    "timeout"
                }
            }
        });

        // Cancel after a short delay
        tokio::time::sleep(Duration::from_millis(10)).await;
        token.cancel();

        let result = handle.await.unwrap();
        assert_eq!(result, "cancelled");
    }
}
