//! Segment pruning for query optimization.

use crate::types::BrokerRequest;
use crate::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for segment pruning strategies.
pub trait SegmentPruner: Send + Sync {
    /// Prune segments based on the query.
    fn prune(&self, request: &BrokerRequest, segments: Vec<String>) -> Result<PruneResult>;

    /// Get the pruner name.
    fn name(&self) -> &'static str;
}

/// Result of segment pruning.
#[derive(Debug, Clone)]
pub struct PruneResult {
    /// Segments that passed pruning
    pub segments: Vec<String>,
    /// Number of segments pruned
    pub num_pruned: usize,
}

impl PruneResult {
    pub fn new(segments: Vec<String>, num_pruned: usize) -> Self {
        Self {
            segments,
            num_pruned,
        }
    }

    pub fn no_pruning(segments: Vec<String>) -> Self {
        Self {
            segments,
            num_pruned: 0,
        }
    }
}

/// Time-based segment pruner.
pub struct TimePruner {
    /// Segment to time range mapping
    segment_time_ranges: parking_lot::RwLock<HashMap<String, TimeRange>>,
}

/// Time range for a segment.
#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    /// Start time (inclusive)
    pub start_ms: i64,
    /// End time (inclusive)
    pub end_ms: i64,
}

impl TimeRange {
    pub fn new(start_ms: i64, end_ms: i64) -> Self {
        Self { start_ms, end_ms }
    }

    /// Check if this range overlaps with another.
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start_ms <= other.end_ms && other.start_ms <= self.end_ms
    }

    /// Check if a timestamp falls within this range.
    pub fn contains(&self, timestamp_ms: i64) -> bool {
        timestamp_ms >= self.start_ms && timestamp_ms <= self.end_ms
    }
}

impl TimePruner {
    pub fn new() -> Self {
        Self {
            segment_time_ranges: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Register a segment's time range.
    pub fn register_segment(&self, segment: String, time_range: TimeRange) {
        self.segment_time_ranges
            .write()
            .insert(segment, time_range);
    }

    /// Remove a segment's time range.
    pub fn unregister_segment(&self, segment: &str) {
        self.segment_time_ranges.write().remove(segment);
    }

    /// Extract time filter from query (simplified).
    fn extract_time_filter(&self, request: &BrokerRequest) -> Option<TimeRange> {
        // In a real implementation, this would parse the SQL WHERE clause
        // For now, we return None (no time filter detected)
        None
    }
}

impl Default for TimePruner {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentPruner for TimePruner {
    fn prune(&self, request: &BrokerRequest, segments: Vec<String>) -> Result<PruneResult> {
        let time_filter = match self.extract_time_filter(request) {
            Some(filter) => filter,
            None => return Ok(PruneResult::no_pruning(segments)),
        };

        let time_ranges = self.segment_time_ranges.read();
        let original_count = segments.len();

        let remaining: Vec<String> = segments
            .into_iter()
            .filter(|segment| {
                if let Some(range) = time_ranges.get(segment) {
                    range.overlaps(&time_filter)
                } else {
                    // Keep segments without time info
                    true
                }
            })
            .collect();

        let num_pruned = original_count - remaining.len();
        Ok(PruneResult::new(remaining, num_pruned))
    }

    fn name(&self) -> &'static str {
        "TimePruner"
    }
}

/// Partition-based segment pruner.
pub struct PartitionPruner {
    /// Segment to partition mapping
    segment_partitions: parking_lot::RwLock<HashMap<String, PartitionInfo>>,
}

/// Partition information for a segment.
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    /// Partition column
    pub column: String,
    /// Partition function (murmur, hashcode, etc.)
    pub function: String,
    /// Number of partitions
    pub num_partitions: usize,
    /// Partition ID for this segment
    pub partition_id: usize,
}

impl PartitionPruner {
    pub fn new() -> Self {
        Self {
            segment_partitions: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Register a segment's partition info.
    pub fn register_segment(&self, segment: String, info: PartitionInfo) {
        self.segment_partitions.write().insert(segment, info);
    }

    /// Remove a segment's partition info.
    pub fn unregister_segment(&self, segment: &str) {
        self.segment_partitions.write().remove(segment);
    }

    /// Extract partition filter from query (simplified).
    fn extract_partition_filter(&self, request: &BrokerRequest) -> Option<(String, Vec<usize>)> {
        // In a real implementation, this would parse the SQL WHERE clause
        // and determine which partitions are needed
        None
    }
}

impl Default for PartitionPruner {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentPruner for PartitionPruner {
    fn prune(&self, request: &BrokerRequest, segments: Vec<String>) -> Result<PruneResult> {
        let partition_filter = match self.extract_partition_filter(request) {
            Some(filter) => filter,
            None => return Ok(PruneResult::no_pruning(segments)),
        };

        let (column, partition_ids) = partition_filter;
        let partitions = self.segment_partitions.read();
        let original_count = segments.len();

        let remaining: Vec<String> = segments
            .into_iter()
            .filter(|segment| {
                if let Some(info) = partitions.get(segment) {
                    if info.column == column {
                        partition_ids.contains(&info.partition_id)
                    } else {
                        true // Different partition column, keep
                    }
                } else {
                    true // No partition info, keep
                }
            })
            .collect();

        let num_pruned = original_count - remaining.len();
        Ok(PruneResult::new(remaining, num_pruned))
    }

    fn name(&self) -> &'static str {
        "PartitionPruner"
    }
}

/// Empty segment pruner that filters out empty segments.
pub struct EmptySegmentPruner {
    /// Known empty segments
    empty_segments: parking_lot::RwLock<std::collections::HashSet<String>>,
}

impl EmptySegmentPruner {
    pub fn new() -> Self {
        Self {
            empty_segments: parking_lot::RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// Mark a segment as empty.
    pub fn mark_empty(&self, segment: String) {
        self.empty_segments.write().insert(segment);
    }

    /// Unmark a segment as empty.
    pub fn unmark_empty(&self, segment: &str) {
        self.empty_segments.write().remove(segment);
    }
}

impl Default for EmptySegmentPruner {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentPruner for EmptySegmentPruner {
    fn prune(&self, _request: &BrokerRequest, segments: Vec<String>) -> Result<PruneResult> {
        let empty = self.empty_segments.read();
        let original_count = segments.len();

        let remaining: Vec<String> = segments
            .into_iter()
            .filter(|s| !empty.contains(s))
            .collect();

        let num_pruned = original_count - remaining.len();
        Ok(PruneResult::new(remaining, num_pruned))
    }

    fn name(&self) -> &'static str {
        "EmptySegmentPruner"
    }
}

/// Composite pruner that applies multiple pruners in sequence.
pub struct CompositePruner {
    pruners: Vec<Arc<dyn SegmentPruner>>,
}

impl CompositePruner {
    pub fn new(pruners: Vec<Arc<dyn SegmentPruner>>) -> Self {
        Self { pruners }
    }

    pub fn builder() -> CompositePrunerBuilder {
        CompositePrunerBuilder::new()
    }
}

impl SegmentPruner for CompositePruner {
    fn prune(&self, request: &BrokerRequest, segments: Vec<String>) -> Result<PruneResult> {
        let mut current_segments = segments;
        let mut total_pruned = 0;

        for pruner in &self.pruners {
            let result = pruner.prune(request, current_segments)?;
            total_pruned += result.num_pruned;
            current_segments = result.segments;
        }

        Ok(PruneResult::new(current_segments, total_pruned))
    }

    fn name(&self) -> &'static str {
        "CompositePruner"
    }
}

/// Builder for CompositePruner.
pub struct CompositePrunerBuilder {
    pruners: Vec<Arc<dyn SegmentPruner>>,
}

impl CompositePrunerBuilder {
    pub fn new() -> Self {
        Self {
            pruners: Vec::new(),
        }
    }

    pub fn add_pruner<P: SegmentPruner + 'static>(mut self, pruner: P) -> Self {
        self.pruners.push(Arc::new(pruner));
        self
    }

    pub fn build(self) -> CompositePruner {
        CompositePruner::new(self.pruners)
    }
}

impl Default for CompositePrunerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_request() -> BrokerRequest {
        BrokerRequest::new(
            1,
            "SELECT * FROM myTable".to_string(),
            "myTable".to_string(),
        )
    }

    #[test]
    fn test_time_range_overlap() {
        let r1 = TimeRange::new(100, 200);
        let r2 = TimeRange::new(150, 250);
        let r3 = TimeRange::new(300, 400);

        assert!(r1.overlaps(&r2));
        assert!(r2.overlaps(&r1));
        assert!(!r1.overlaps(&r3));
        assert!(!r3.overlaps(&r1));
    }

    #[test]
    fn test_time_range_contains() {
        let range = TimeRange::new(100, 200);
        assert!(range.contains(100));
        assert!(range.contains(150));
        assert!(range.contains(200));
        assert!(!range.contains(50));
        assert!(!range.contains(250));
    }

    #[test]
    fn test_time_pruner_no_filter() {
        let pruner = TimePruner::new();
        let request = create_request();
        let segments = vec!["s1".to_string(), "s2".to_string()];

        let result = pruner.prune(&request, segments).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.num_pruned, 0);
    }

    #[test]
    fn test_empty_segment_pruner() {
        let pruner = EmptySegmentPruner::new();
        pruner.mark_empty("s2".to_string());

        let request = create_request();
        let segments = vec!["s1".to_string(), "s2".to_string(), "s3".to_string()];

        let result = pruner.prune(&request, segments).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.num_pruned, 1);
        assert!(!result.segments.contains(&"s2".to_string()));
    }

    #[test]
    fn test_composite_pruner() {
        let empty_pruner = EmptySegmentPruner::new();
        empty_pruner.mark_empty("s2".to_string());

        let pruner = CompositePruner::builder()
            .add_pruner(empty_pruner)
            .add_pruner(TimePruner::new())
            .build();

        let request = create_request();
        let segments = vec!["s1".to_string(), "s2".to_string(), "s3".to_string()];

        let result = pruner.prune(&request, segments).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.num_pruned, 1);
    }

    #[test]
    fn test_partition_pruner_no_filter() {
        let pruner = PartitionPruner::new();
        let request = create_request();
        let segments = vec!["s1".to_string(), "s2".to_string()];

        let result = pruner.prune(&request, segments).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.num_pruned, 0);
    }
}
