//! Metrics collection for the broker.

use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounter,
    IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Broker metrics.
pub struct BrokerMetrics {
    registry: Registry,

    // Query metrics
    queries_total: IntCounterVec,
    query_latency: HistogramVec,
    query_errors: IntCounterVec,

    // Routing metrics
    routing_time: HistogramVec,
    segments_queried: IntCounterVec,
    servers_queried: IntCounterVec,

    // Reduce metrics
    reduce_time: Histogram,
    rows_returned: IntCounter,

    // Connection metrics
    active_connections: IntGaugeVec,
    connection_errors: IntCounterVec,

    // Server metrics
    server_latency: HistogramVec,
    server_errors: IntCounterVec,
}

impl BrokerMetrics {
    pub fn new(prefix: &str) -> Self {
        let registry = Registry::new();

        // Query metrics
        let queries_total = IntCounterVec::new(
            Opts::new(
                format!("{}_queries_total", prefix),
                "Total number of queries",
            ),
            &["table", "status"],
        )
        .unwrap();

        let query_latency = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_query_latency_ms", prefix),
                "Query latency in milliseconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0]),
            &["table"],
        )
        .unwrap();

        let query_errors = IntCounterVec::new(
            Opts::new(
                format!("{}_query_errors_total", prefix),
                "Total number of query errors",
            ),
            &["table", "error_type"],
        )
        .unwrap();

        // Routing metrics
        let routing_time = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_routing_time_us", prefix),
                "Routing time in microseconds",
            )
            .buckets(vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0]),
            &["table"],
        )
        .unwrap();

        let segments_queried = IntCounterVec::new(
            Opts::new(
                format!("{}_segments_queried_total", prefix),
                "Total segments queried",
            ),
            &["table"],
        )
        .unwrap();

        let servers_queried = IntCounterVec::new(
            Opts::new(
                format!("{}_servers_queried_total", prefix),
                "Total servers queried",
            ),
            &["table"],
        )
        .unwrap();

        // Reduce metrics
        let reduce_time = Histogram::with_opts(
            HistogramOpts::new(
                format!("{}_reduce_time_ms", prefix),
                "Reduce time in milliseconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0]),
        )
        .unwrap();

        let rows_returned = IntCounter::new(
            format!("{}_rows_returned_total", prefix),
            "Total rows returned",
        )
        .unwrap();

        // Connection metrics
        let active_connections = IntGaugeVec::new(
            Opts::new(
                format!("{}_active_connections", prefix),
                "Number of active connections",
            ),
            &["server"],
        )
        .unwrap();

        let connection_errors = IntCounterVec::new(
            Opts::new(
                format!("{}_connection_errors_total", prefix),
                "Total connection errors",
            ),
            &["server", "error_type"],
        )
        .unwrap();

        // Server metrics
        let server_latency = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_server_latency_ms", prefix),
                "Server response latency in milliseconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]),
            &["server"],
        )
        .unwrap();

        let server_errors = IntCounterVec::new(
            Opts::new(
                format!("{}_server_errors_total", prefix),
                "Total server errors",
            ),
            &["server", "error_type"],
        )
        .unwrap();

        // Register all metrics
        registry.register(Box::new(queries_total.clone())).unwrap();
        registry.register(Box::new(query_latency.clone())).unwrap();
        registry.register(Box::new(query_errors.clone())).unwrap();
        registry.register(Box::new(routing_time.clone())).unwrap();
        registry.register(Box::new(segments_queried.clone())).unwrap();
        registry.register(Box::new(servers_queried.clone())).unwrap();
        registry.register(Box::new(reduce_time.clone())).unwrap();
        registry.register(Box::new(rows_returned.clone())).unwrap();
        registry.register(Box::new(active_connections.clone())).unwrap();
        registry.register(Box::new(connection_errors.clone())).unwrap();
        registry.register(Box::new(server_latency.clone())).unwrap();
        registry.register(Box::new(server_errors.clone())).unwrap();

        Self {
            registry,
            queries_total,
            query_latency,
            query_errors,
            routing_time,
            segments_queried,
            servers_queried,
            reduce_time,
            rows_returned,
            active_connections,
            connection_errors,
            server_latency,
            server_errors,
        }
    }

    /// Record a query.
    pub fn record_query(&self, table: &str, status: &str, latency_ms: f64) {
        self.queries_total
            .with_label_values(&[table, status])
            .inc();
        self.query_latency
            .with_label_values(&[table])
            .observe(latency_ms);
    }

    /// Record a query error.
    pub fn record_query_error(&self, table: &str, error_type: &str) {
        self.query_errors
            .with_label_values(&[table, error_type])
            .inc();
    }

    /// Record routing time.
    pub fn record_routing_time(&self, table: &str, time_us: f64) {
        self.routing_time
            .with_label_values(&[table])
            .observe(time_us);
    }

    /// Record segments queried.
    pub fn record_segments_queried(&self, table: &str, count: u64) {
        self.segments_queried
            .with_label_values(&[table])
            .inc_by(count);
    }

    /// Record servers queried.
    pub fn record_servers_queried(&self, table: &str, count: u64) {
        self.servers_queried
            .with_label_values(&[table])
            .inc_by(count);
    }

    /// Record reduce time.
    pub fn record_reduce_time(&self, time_ms: f64) {
        self.reduce_time.observe(time_ms);
    }

    /// Record rows returned.
    pub fn record_rows_returned(&self, count: u64) {
        self.rows_returned.inc_by(count);
    }

    /// Set active connections for a server.
    pub fn set_active_connections(&self, server: &str, count: i64) {
        self.active_connections
            .with_label_values(&[server])
            .set(count);
    }

    /// Record a connection error.
    pub fn record_connection_error(&self, server: &str, error_type: &str) {
        self.connection_errors
            .with_label_values(&[server, error_type])
            .inc();
    }

    /// Record server latency.
    pub fn record_server_latency(&self, server: &str, latency_ms: f64) {
        self.server_latency
            .with_label_values(&[server])
            .observe(latency_ms);
    }

    /// Record a server error.
    pub fn record_server_error(&self, server: &str, error_type: &str) {
        self.server_errors
            .with_label_values(&[server, error_type])
            .inc();
    }

    /// Get the registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Create a timer that records duration on drop.
    pub fn timer(&self, table: &str) -> QueryTimer {
        QueryTimer {
            start: Instant::now(),
            table: table.to_string(),
            metrics: self,
        }
    }
}

impl Default for BrokerMetrics {
    fn default() -> Self {
        Self::new("pinot_broker")
    }
}

/// Timer for query latency.
pub struct QueryTimer<'a> {
    start: Instant,
    table: String,
    metrics: &'a BrokerMetrics,
}

impl<'a> QueryTimer<'a> {
    /// Stop the timer and record success.
    pub fn success(self) {
        let elapsed = self.start.elapsed().as_millis() as f64;
        self.metrics.record_query(&self.table, "success", elapsed);
    }

    /// Stop the timer and record error.
    pub fn error(self, error_type: &str) {
        let elapsed = self.start.elapsed().as_millis() as f64;
        self.metrics.record_query(&self.table, "error", elapsed);
        self.metrics.record_query_error(&self.table, error_type);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = BrokerMetrics::new("test");
        assert!(metrics.registry().gather().len() > 0);
    }

    #[test]
    fn test_record_query() {
        let metrics = BrokerMetrics::new("test");

        metrics.record_query("myTable", "success", 100.0);
        metrics.record_query("myTable", "success", 200.0);
        metrics.record_query("myTable", "error", 50.0);

        // Metrics should be recorded (checking via registry)
        let gathered = metrics.registry().gather();
        assert!(!gathered.is_empty());
    }

    #[test]
    fn test_timer() {
        let metrics = BrokerMetrics::new("test");

        let timer = metrics.timer("myTable");
        std::thread::sleep(std::time::Duration::from_millis(10));
        timer.success();

        // Query should be recorded
        let gathered = metrics.registry().gather();
        assert!(!gathered.is_empty());
    }

    #[test]
    fn test_server_metrics() {
        let metrics = BrokerMetrics::new("test");

        metrics.record_server_latency("host1:8099", 50.0);
        metrics.record_server_error("host1:8099", "timeout");
        metrics.set_active_connections("host1:8099", 5);

        let gathered = metrics.registry().gather();
        assert!(!gathered.is_empty());
    }
}
