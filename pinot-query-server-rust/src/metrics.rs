//! Metrics collection and export
//!
//! This module provides Prometheus-compatible metrics for monitoring
//! query server performance.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use prometheus::{
    self, Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts,
    Registry,
};

lazy_static! {
    /// Global metrics registry
    pub static ref REGISTRY: Registry = Registry::new();

    // Query metrics
    pub static ref QUERIES_TOTAL: Counter = Counter::new(
        "pinot_query_server_queries_total",
        "Total number of queries processed"
    ).unwrap();

    pub static ref QUERIES_ACTIVE: Gauge = Gauge::new(
        "pinot_query_server_queries_active",
        "Number of currently active queries"
    ).unwrap();

    pub static ref QUERY_LATENCY: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "pinot_query_server_query_latency_seconds",
            "Query latency in seconds"
        ).buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
    ).unwrap();

    // Exchange metrics
    pub static ref BLOCKS_SENT: CounterVec = CounterVec::new(
        Opts::new("pinot_query_server_blocks_sent_total", "Total blocks sent"),
        &["exchange_type"]
    ).unwrap();

    pub static ref BLOCKS_RECEIVED: CounterVec = CounterVec::new(
        Opts::new("pinot_query_server_blocks_received_total", "Total blocks received"),
        &["stage"]
    ).unwrap();

    pub static ref BYTES_TRANSFERRED: CounterVec = CounterVec::new(
        Opts::new("pinot_query_server_bytes_transferred_total", "Total bytes transferred"),
        &["direction"]
    ).unwrap();

    // Backpressure metrics
    pub static ref BACKPRESSURE_EVENTS: Counter = Counter::new(
        "pinot_query_server_backpressure_events_total",
        "Total number of backpressure events"
    ).unwrap();

    pub static ref BACKPRESSURE_WAIT: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "pinot_query_server_backpressure_wait_seconds",
            "Time spent waiting due to backpressure"
        ).buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0])
    ).unwrap();

    // Memory metrics
    pub static ref MEMORY_USED: Gauge = Gauge::new(
        "pinot_query_server_memory_used_bytes",
        "Current memory usage in bytes"
    ).unwrap();

    pub static ref MEMORY_LIMIT: Gauge = Gauge::new(
        "pinot_query_server_memory_limit_bytes",
        "Memory limit in bytes"
    ).unwrap();

    pub static ref MEMORY_PRESSURE: GaugeVec = GaugeVec::new(
        Opts::new("pinot_query_server_memory_pressure", "Current memory pressure level (0-3)"),
        &["level"]
    ).unwrap();

    // Mailbox metrics
    pub static ref MAILBOX_COUNT: GaugeVec = GaugeVec::new(
        Opts::new("pinot_query_server_mailbox_count", "Number of active mailboxes"),
        &["type"]
    ).unwrap();

    pub static ref MAILBOX_PENDING_BLOCKS: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "pinot_query_server_mailbox_pending_blocks",
            "Number of pending blocks in mailboxes"
        ).buckets(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0])
    ).unwrap();

    // Channel metrics
    pub static ref CHANNELS_ACTIVE: Gauge = Gauge::new(
        "pinot_query_server_channels_active",
        "Number of active gRPC channels"
    ).unwrap();

    pub static ref CHANNEL_ERRORS: Counter = Counter::new(
        "pinot_query_server_channel_errors_total",
        "Total channel errors"
    ).unwrap();

    // Operator metrics
    pub static ref OPERATOR_EXECUTION_TIME: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "pinot_query_server_operator_execution_seconds",
            "Operator execution time in seconds"
        ).buckets(vec![0.0001, 0.001, 0.01, 0.1, 1.0, 10.0]),
        &["operator"]
    ).unwrap();

    pub static ref ROWS_PROCESSED: CounterVec = CounterVec::new(
        Opts::new("pinot_query_server_rows_processed_total", "Total rows processed"),
        &["operator"]
    ).unwrap();
}

/// Initialize all metrics
pub fn init_metrics() {
    let _ = REGISTRY.register(Box::new(QUERIES_TOTAL.clone()));
    let _ = REGISTRY.register(Box::new(QUERIES_ACTIVE.clone()));
    let _ = REGISTRY.register(Box::new(QUERY_LATENCY.clone()));
    let _ = REGISTRY.register(Box::new(BLOCKS_SENT.clone()));
    let _ = REGISTRY.register(Box::new(BLOCKS_RECEIVED.clone()));
    let _ = REGISTRY.register(Box::new(BYTES_TRANSFERRED.clone()));
    let _ = REGISTRY.register(Box::new(BACKPRESSURE_EVENTS.clone()));
    let _ = REGISTRY.register(Box::new(BACKPRESSURE_WAIT.clone()));
    let _ = REGISTRY.register(Box::new(MEMORY_USED.clone()));
    let _ = REGISTRY.register(Box::new(MEMORY_LIMIT.clone()));
    let _ = REGISTRY.register(Box::new(MEMORY_PRESSURE.clone()));
    let _ = REGISTRY.register(Box::new(MAILBOX_COUNT.clone()));
    let _ = REGISTRY.register(Box::new(MAILBOX_PENDING_BLOCKS.clone()));
    let _ = REGISTRY.register(Box::new(CHANNELS_ACTIVE.clone()));
    let _ = REGISTRY.register(Box::new(CHANNEL_ERRORS.clone()));
    let _ = REGISTRY.register(Box::new(OPERATOR_EXECUTION_TIME.clone()));
    let _ = REGISTRY.register(Box::new(ROWS_PROCESSED.clone()));
}

/// Query metrics tracking
pub struct QueryMetrics {
    /// Query ID
    query_id: String,
    /// Start time
    start_time: Instant,
    /// Rows processed
    rows_processed: AtomicU64,
    /// Bytes processed
    bytes_processed: AtomicU64,
    /// Stages completed
    stages_completed: AtomicU64,
}

impl QueryMetrics {
    /// Create new query metrics
    pub fn new(query_id: impl Into<String>) -> Self {
        QUERIES_TOTAL.inc();
        QUERIES_ACTIVE.inc();

        Self {
            query_id: query_id.into(),
            start_time: Instant::now(),
            rows_processed: AtomicU64::new(0),
            bytes_processed: AtomicU64::new(0),
            stages_completed: AtomicU64::new(0),
        }
    }

    /// Get query ID
    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    /// Record rows processed
    pub fn record_rows(&self, rows: u64) {
        self.rows_processed.fetch_add(rows, Ordering::Relaxed);
    }

    /// Record bytes processed
    pub fn record_bytes(&self, bytes: u64) {
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record stage completion
    pub fn record_stage_complete(&self) {
        self.stages_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get rows processed
    pub fn rows_processed(&self) -> u64 {
        self.rows_processed.load(Ordering::Relaxed)
    }

    /// Get bytes processed
    pub fn bytes_processed(&self) -> u64 {
        self.bytes_processed.load(Ordering::Relaxed)
    }

    /// Finish tracking (records latency)
    pub fn finish(&self) {
        let latency = self.elapsed().as_secs_f64();
        QUERY_LATENCY.observe(latency);
        QUERIES_ACTIVE.dec();
    }
}

impl Drop for QueryMetrics {
    fn drop(&mut self) {
        QUERIES_ACTIVE.dec();
    }
}

/// Timer for measuring operation duration
pub struct Timer {
    name: String,
    start: Instant,
}

impl Timer {
    /// Start a new timer
    pub fn start(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: Instant::now(),
        }
    }

    /// Stop timer and record to histogram
    pub fn stop(self) {
        let duration = self.start.elapsed().as_secs_f64();
        OPERATOR_EXECUTION_TIME
            .with_label_values(&[&self.name])
            .observe(duration);
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Record a backpressure event
pub fn record_backpressure(wait_duration: Duration) {
    BACKPRESSURE_EVENTS.inc();
    BACKPRESSURE_WAIT.observe(wait_duration.as_secs_f64());
}

/// Record memory usage
pub fn record_memory(used: usize, limit: usize) {
    MEMORY_USED.set(used as f64);
    MEMORY_LIMIT.set(limit as f64);
}

/// Record memory pressure level
pub fn record_pressure_level(level: u8) {
    let level_name = match level {
        0 => "normal",
        1 => "moderate",
        2 => "high",
        3 => "critical",
        _ => "unknown",
    };
    MEMORY_PRESSURE.with_label_values(&[level_name]).set(level as f64);
}

/// Record blocks sent
pub fn record_blocks_sent(exchange_type: &str, count: u64) {
    BLOCKS_SENT
        .with_label_values(&[exchange_type])
        .inc_by(count as f64);
}

/// Record blocks received
pub fn record_blocks_received(stage: &str, count: u64) {
    BLOCKS_RECEIVED
        .with_label_values(&[stage])
        .inc_by(count as f64);
}

/// Record bytes transferred
pub fn record_bytes_transferred(direction: &str, bytes: u64) {
    BYTES_TRANSFERRED
        .with_label_values(&[direction])
        .inc_by(bytes as f64);
}

/// Record mailbox counts
pub fn record_mailbox_count(mailbox_type: &str, count: usize) {
    MAILBOX_COUNT
        .with_label_values(&[mailbox_type])
        .set(count as f64);
}

/// Record pending blocks in mailbox
pub fn record_pending_blocks(pending: usize) {
    MAILBOX_PENDING_BLOCKS.observe(pending as f64);
}

/// Record active channels
pub fn record_active_channels(count: usize) {
    CHANNELS_ACTIVE.set(count as f64);
}

/// Record channel error
pub fn record_channel_error() {
    CHANNEL_ERRORS.inc();
}

/// Record rows processed by operator
pub fn record_operator_rows(operator: &str, rows: u64) {
    ROWS_PROCESSED
        .with_label_values(&[operator])
        .inc_by(rows as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_metrics() {
        let metrics = QueryMetrics::new("test-query-1");
        assert_eq!(metrics.query_id(), "test-query-1");

        metrics.record_rows(100);
        metrics.record_bytes(1024);
        metrics.record_stage_complete();

        assert_eq!(metrics.rows_processed(), 100);
        assert_eq!(metrics.bytes_processed(), 1024);
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start("test_operation");
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed() >= Duration::from_millis(10));
        timer.stop();
    }

    #[test]
    fn test_record_functions() {
        // These should not panic
        record_backpressure(Duration::from_millis(100));
        record_memory(1000000, 10000000);
        record_pressure_level(1);
        record_blocks_sent("HASH", 10);
        record_blocks_received("stage_1", 5);
        record_bytes_transferred("sent", 1024);
        record_mailbox_count("receiving", 3);
        record_pending_blocks(2);
        record_active_channels(5);
        record_channel_error();
        record_operator_rows("HashJoin", 1000);
    }
}
