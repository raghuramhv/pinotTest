//! Async query response handling.

use crate::types::{DataTable, ServerInstance, ServerResponse};
use crate::{BrokerError, Result};
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Notify};

/// Status of an async query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryStatus {
    /// Query is in progress
    InProgress,
    /// Query completed successfully
    Completed,
    /// Query timed out
    TimedOut,
    /// Query failed
    Failed,
    /// Query was cancelled
    Cancelled,
}

/// Async query response that collects responses from multiple servers.
pub struct AsyncQueryResponse {
    /// Request ID
    request_id: u64,
    /// Expected number of responses
    expected_responses: usize,
    /// Collected responses
    responses: DashMap<ServerInstance, ServerResponse>,
    /// Number of responses received
    responses_received: AtomicUsize,
    /// Query status
    status: parking_lot::RwLock<QueryStatus>,
    /// Notify when all responses are received
    completion_notify: Notify,
    /// Start time
    start_time: Instant,
    /// Timeout duration
    timeout: Duration,
}

impl AsyncQueryResponse {
    pub fn new(request_id: u64, expected_responses: usize, timeout: Duration) -> Self {
        Self {
            request_id,
            expected_responses,
            responses: DashMap::new(),
            responses_received: AtomicUsize::new(0),
            status: parking_lot::RwLock::new(QueryStatus::InProgress),
            completion_notify: Notify::new(),
            start_time: Instant::now(),
            timeout,
        }
    }

    /// Record a response from a server.
    pub fn receive_response(&self, response: ServerResponse) {
        let server = response.server.clone();
        self.responses.insert(server, response);

        let count = self.responses_received.fetch_add(1, Ordering::SeqCst) + 1;

        if count >= self.expected_responses {
            *self.status.write() = QueryStatus::Completed;
            self.completion_notify.notify_waiters();
        }
    }

    /// Record an error for a server.
    pub fn receive_error(&self, server: ServerInstance, error: BrokerError) {
        self.responses.insert(server.clone(), ServerResponse::error(server, error));

        let count = self.responses_received.fetch_add(1, Ordering::SeqCst) + 1;

        if count >= self.expected_responses {
            // Check if all responses are errors
            let all_errors = self
                .responses
                .iter()
                .all(|r| r.value().error.is_some());

            if all_errors {
                *self.status.write() = QueryStatus::Failed;
            } else {
                *self.status.write() = QueryStatus::Completed;
            }
            self.completion_notify.notify_waiters();
        }
    }

    /// Wait for all responses with timeout.
    pub async fn wait_for_responses(&self) -> Result<()> {
        let remaining = self.remaining_timeout();

        if remaining.is_zero() {
            *self.status.write() = QueryStatus::TimedOut;
            return Err(BrokerError::Timeout(self.timeout.as_millis() as u64));
        }

        // Wait for completion or timeout
        let result = tokio::time::timeout(remaining, async {
            loop {
                if self.responses_received.load(Ordering::SeqCst) >= self.expected_responses {
                    return Ok::<(), ()>(());
                }
                self.completion_notify.notified().await;
            }
        })
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(_) => {
                *self.status.write() = QueryStatus::TimedOut;
                Err(BrokerError::Timeout(self.timeout.as_millis() as u64))
            }
        }
    }

    /// Get remaining timeout duration.
    pub fn remaining_timeout(&self) -> Duration {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.timeout {
            Duration::ZERO
        } else {
            self.timeout - elapsed
        }
    }

    /// Get current status.
    pub fn status(&self) -> QueryStatus {
        *self.status.read()
    }

    /// Cancel the query.
    pub fn cancel(&self) {
        *self.status.write() = QueryStatus::Cancelled;
        self.completion_notify.notify_waiters();
    }

    /// Get final responses (consuming the response object).
    pub fn get_final_responses(self) -> Vec<(ServerInstance, ServerResponse)> {
        self.responses
            .into_iter()
            .collect()
    }

    /// Get responses (non-consuming).
    pub fn get_responses(&self) -> Vec<(ServerInstance, ServerResponse)> {
        self.responses
            .iter()
            .map(|r| (r.key().clone(), ServerResponse {
                server: r.value().server.clone(),
                data_table: r.value().data_table.clone(),
                response_size: r.value().response_size,
                response_delay_ms: r.value().response_delay_ms,
                deserialization_time_ms: r.value().deserialization_time_ms,
                error: r.value().error.as_ref().map(|e| BrokerError::Internal(e.to_string())),
            }))
            .collect()
    }

    /// Get successful responses only.
    pub fn get_successful_responses(&self) -> Vec<(ServerInstance, DataTable)> {
        self.responses
            .iter()
            .filter_map(|r| {
                r.value()
                    .data_table
                    .clone()
                    .map(|dt| (r.key().clone(), dt))
            })
            .collect()
    }

    /// Get failed servers.
    pub fn get_failed_servers(&self) -> Vec<ServerInstance> {
        self.responses
            .iter()
            .filter(|r| r.value().error.is_some())
            .map(|r| r.key().clone())
            .collect()
    }

    /// Get number of responses received.
    pub fn num_responses(&self) -> usize {
        self.responses_received.load(Ordering::Relaxed)
    }

    /// Get number of successful responses.
    pub fn num_successful(&self) -> usize {
        self.responses
            .iter()
            .filter(|r| r.value().is_success())
            .count()
    }

    /// Get number of failed responses.
    pub fn num_failed(&self) -> usize {
        self.responses
            .iter()
            .filter(|r| r.value().error.is_some())
            .count()
    }

    /// Get request ID.
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Get elapsed time.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Check if query is complete (all responses received or timed out).
    pub fn is_complete(&self) -> bool {
        let status = *self.status.read();
        matches!(
            status,
            QueryStatus::Completed
                | QueryStatus::TimedOut
                | QueryStatus::Failed
                | QueryStatus::Cancelled
        )
    }

    /// Check if query is still in progress.
    pub fn is_in_progress(&self) -> bool {
        *self.status.read() == QueryStatus::InProgress
    }
}

/// Builder for AsyncQueryResponse.
pub struct AsyncQueryResponseBuilder {
    request_id: u64,
    expected_responses: usize,
    timeout: Duration,
}

impl AsyncQueryResponseBuilder {
    pub fn new(request_id: u64) -> Self {
        Self {
            request_id,
            expected_responses: 0,
            timeout: Duration::from_secs(60),
        }
    }

    pub fn expected_responses(mut self, count: usize) -> Self {
        self.expected_responses = count;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> AsyncQueryResponse {
        AsyncQueryResponse::new(self.request_id, self.expected_responses, self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDataType, DataSchema, TableType};

    fn create_server(hostname: &str) -> ServerInstance {
        ServerInstance::new(hostname.to_string(), 8099, TableType::Offline)
    }

    fn create_data_table() -> DataTable {
        DataTable::new(DataSchema::new(
            vec!["col1".to_string()],
            vec![ColumnDataType::Int],
        ))
    }

    #[test]
    fn test_async_response_new() {
        let response = AsyncQueryResponse::new(1, 3, Duration::from_secs(60));
        assert_eq!(response.request_id(), 1);
        assert_eq!(response.status(), QueryStatus::InProgress);
        assert_eq!(response.num_responses(), 0);
    }

    #[test]
    fn test_receive_response() {
        let response = AsyncQueryResponse::new(1, 2, Duration::from_secs(60));

        let server1 = create_server("host1");
        let server2 = create_server("host2");

        response.receive_response(ServerResponse::success(
            server1,
            create_data_table(),
            100,
        ));

        assert_eq!(response.num_responses(), 1);
        assert_eq!(response.status(), QueryStatus::InProgress);

        response.receive_response(ServerResponse::success(
            server2,
            create_data_table(),
            100,
        ));

        assert_eq!(response.num_responses(), 2);
        assert_eq!(response.status(), QueryStatus::Completed);
    }

    #[test]
    fn test_receive_error() {
        let response = AsyncQueryResponse::new(1, 2, Duration::from_secs(60));

        let server1 = create_server("host1");
        let server2 = create_server("host2");

        response.receive_response(ServerResponse::success(
            server1,
            create_data_table(),
            100,
        ));

        response.receive_error(
            server2,
            BrokerError::Connection {
                server: "host2:8099".to_string(),
                details: "refused".to_string(),
            },
        );

        assert_eq!(response.num_responses(), 2);
        assert_eq!(response.num_successful(), 1);
        assert_eq!(response.num_failed(), 1);
    }

    #[test]
    fn test_all_errors() {
        let response = AsyncQueryResponse::new(1, 2, Duration::from_secs(60));

        let server1 = create_server("host1");
        let server2 = create_server("host2");

        response.receive_error(
            server1,
            BrokerError::Timeout(1000),
        );
        response.receive_error(
            server2,
            BrokerError::Timeout(1000),
        );

        assert_eq!(response.status(), QueryStatus::Failed);
    }

    #[test]
    fn test_cancel() {
        let response = AsyncQueryResponse::new(1, 3, Duration::from_secs(60));
        response.cancel();

        assert_eq!(response.status(), QueryStatus::Cancelled);
        assert!(response.is_complete());
    }

    #[test]
    fn test_get_failed_servers() {
        let response = AsyncQueryResponse::new(1, 2, Duration::from_secs(60));

        let server1 = create_server("host1");
        let server2 = create_server("host2");

        response.receive_response(ServerResponse::success(
            server1,
            create_data_table(),
            100,
        ));

        response.receive_error(
            server2.clone(),
            BrokerError::Timeout(1000),
        );

        let failed = response.get_failed_servers();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].hostname, "host2");
    }

    #[test]
    fn test_builder() {
        let response = AsyncQueryResponseBuilder::new(42)
            .expected_responses(5)
            .timeout(Duration::from_secs(30))
            .build();

        assert_eq!(response.request_id(), 42);
        assert!(response.remaining_timeout() <= Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_wait_for_responses() {
        let response = Arc::new(AsyncQueryResponse::new(1, 1, Duration::from_secs(5)));

        let response_clone = response.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            response_clone.receive_response(ServerResponse::success(
                create_server("host1"),
                create_data_table(),
                100,
            ));
        });

        let result = response.wait_for_responses().await;
        assert!(result.is_ok());
        assert_eq!(response.status(), QueryStatus::Completed);
    }

    #[tokio::test]
    async fn test_wait_timeout() {
        let response = AsyncQueryResponse::new(1, 1, Duration::from_millis(50));

        let result = response.wait_for_responses().await;
        assert!(result.is_err());
        assert_eq!(response.status(), QueryStatus::TimedOut);
    }
}
