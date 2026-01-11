//! Instance (server) selection for query routing.

use crate::types::{SegmentsToQuery, ServerInstance, TableType};
use crate::Result;
use ahash::AHashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Result of instance selection.
#[derive(Debug, Clone)]
pub struct SelectionResult {
    /// Server to segments mapping
    pub server_to_segments: HashMap<ServerInstance, SegmentsToQuery>,
    /// Segments that are unavailable
    pub unavailable_segments: Vec<String>,
}

impl SelectionResult {
    pub fn new() -> Self {
        Self {
            server_to_segments: HashMap::new(),
            unavailable_segments: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.server_to_segments.is_empty()
    }

    pub fn num_servers(&self) -> usize {
        self.server_to_segments.len()
    }
}

impl Default for SelectionResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for instance selection strategies.
pub trait InstanceSelector: Send + Sync {
    /// Select server instances for the given segments.
    fn select(
        &self,
        segments: &[String],
        segment_to_instances: &HashMap<String, Vec<ServerInstance>>,
        request_id: u64,
    ) -> Result<SelectionResult>;

    /// Get the selector name.
    fn name(&self) -> &'static str;
}

/// Balanced round-robin instance selector.
pub struct BalancedInstanceSelector {
    /// Round-robin counter per segment
    segment_counters: RwLock<AHashMap<String, AtomicUsize>>,
    /// Excluded instances (unhealthy servers)
    excluded_instances: RwLock<Vec<ServerInstance>>,
}

impl BalancedInstanceSelector {
    pub fn new() -> Self {
        Self {
            segment_counters: RwLock::new(AHashMap::new()),
            excluded_instances: RwLock::new(Vec::new()),
        }
    }

    /// Mark an instance as excluded (unhealthy).
    pub fn exclude_instance(&self, instance: &ServerInstance) {
        let mut excluded = self.excluded_instances.write();
        if !excluded.contains(instance) {
            excluded.push(instance.clone());
        }
    }

    /// Remove an instance from the excluded list.
    pub fn include_instance(&self, instance: &ServerInstance) {
        let mut excluded = self.excluded_instances.write();
        excluded.retain(|i| i != instance);
    }

    /// Check if an instance is excluded.
    pub fn is_excluded(&self, instance: &ServerInstance) -> bool {
        self.excluded_instances.read().contains(instance)
    }

    fn get_counter(&self, segment: &str) -> usize {
        let counters = self.segment_counters.read();
        if let Some(counter) = counters.get(segment) {
            counter.fetch_add(1, Ordering::Relaxed)
        } else {
            drop(counters);
            let mut counters = self.segment_counters.write();
            counters
                .entry(segment.to_string())
                .or_insert_with(|| AtomicUsize::new(0))
                .fetch_add(1, Ordering::Relaxed)
        }
    }
}

impl Default for BalancedInstanceSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceSelector for BalancedInstanceSelector {
    fn select(
        &self,
        segments: &[String],
        segment_to_instances: &HashMap<String, Vec<ServerInstance>>,
        _request_id: u64,
    ) -> Result<SelectionResult> {
        let mut result = SelectionResult::new();
        let excluded = self.excluded_instances.read();

        for segment in segments {
            if let Some(instances) = segment_to_instances.get(segment) {
                // Filter out excluded instances
                let available: Vec<_> = instances
                    .iter()
                    .filter(|i| !excluded.contains(i))
                    .collect();

                if available.is_empty() {
                    // All instances excluded, use any available
                    if !instances.is_empty() {
                        let counter = self.get_counter(segment);
                        let idx = counter % instances.len();
                        let server = instances[idx].clone();
                        result
                            .server_to_segments
                            .entry(server)
                            .or_insert_with(|| SegmentsToQuery::new(Vec::new()))
                            .segments
                            .push(segment.clone());
                    } else {
                        result.unavailable_segments.push(segment.clone());
                    }
                } else {
                    // Round-robin among available instances
                    let counter = self.get_counter(segment);
                    let idx = counter % available.len();
                    let server = available[idx].clone();
                    result
                        .server_to_segments
                        .entry(server)
                        .or_insert_with(|| SegmentsToQuery::new(Vec::new()))
                        .segments
                        .push(segment.clone());
                }
            } else {
                result.unavailable_segments.push(segment.clone());
            }
        }

        Ok(result)
    }

    fn name(&self) -> &'static str {
        "Balanced"
    }
}

/// Replica group based instance selector.
pub struct ReplicaGroupInstanceSelector {
    /// Replica group to use (hash-based)
    num_replica_groups: usize,
    /// Excluded instances
    excluded_instances: RwLock<Vec<ServerInstance>>,
}

impl ReplicaGroupInstanceSelector {
    pub fn new(num_replica_groups: usize) -> Self {
        Self {
            num_replica_groups: num_replica_groups.max(1),
            excluded_instances: RwLock::new(Vec::new()),
        }
    }

    fn get_replica_group(&self, table_name: &str) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        table_name.hash(&mut hasher);
        (hasher.finish() as usize) % self.num_replica_groups
    }
}

impl InstanceSelector for ReplicaGroupInstanceSelector {
    fn select(
        &self,
        segments: &[String],
        segment_to_instances: &HashMap<String, Vec<ServerInstance>>,
        _request_id: u64,
    ) -> Result<SelectionResult> {
        let mut result = SelectionResult::new();
        let excluded = self.excluded_instances.read();

        for segment in segments {
            if let Some(instances) = segment_to_instances.get(segment) {
                // Filter out excluded instances
                let available: Vec<_> = instances
                    .iter()
                    .filter(|i| !excluded.contains(i))
                    .collect();

                if !available.is_empty() {
                    // Select based on replica group
                    let replica_group = self.get_replica_group(segment);
                    let idx = replica_group % available.len();
                    let server = available[idx].clone();
                    result
                        .server_to_segments
                        .entry(server)
                        .or_insert_with(|| SegmentsToQuery::new(Vec::new()))
                        .segments
                        .push(segment.clone());
                } else if !instances.is_empty() {
                    // Fallback to first instance
                    let server = instances[0].clone();
                    result
                        .server_to_segments
                        .entry(server)
                        .or_insert_with(|| SegmentsToQuery::new(Vec::new()))
                        .segments
                        .push(segment.clone());
                } else {
                    result.unavailable_segments.push(segment.clone());
                }
            } else {
                result.unavailable_segments.push(segment.clone());
            }
        }

        Ok(result)
    }

    fn name(&self) -> &'static str {
        "ReplicaGroup"
    }
}

/// Hash-based instance selector for consistent routing.
pub struct HashBasedInstanceSelector {
    /// Excluded instances
    excluded_instances: RwLock<Vec<ServerInstance>>,
}

impl HashBasedInstanceSelector {
    pub fn new() -> Self {
        Self {
            excluded_instances: RwLock::new(Vec::new()),
        }
    }

    fn hash_segment(&self, segment: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        segment.hash(&mut hasher);
        hasher.finish()
    }
}

impl Default for HashBasedInstanceSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceSelector for HashBasedInstanceSelector {
    fn select(
        &self,
        segments: &[String],
        segment_to_instances: &HashMap<String, Vec<ServerInstance>>,
        _request_id: u64,
    ) -> Result<SelectionResult> {
        let mut result = SelectionResult::new();
        let excluded = self.excluded_instances.read();

        for segment in segments {
            if let Some(instances) = segment_to_instances.get(segment) {
                let available: Vec<_> = instances
                    .iter()
                    .filter(|i| !excluded.contains(i))
                    .collect();

                if !available.is_empty() {
                    let hash = self.hash_segment(segment);
                    let idx = (hash as usize) % available.len();
                    let server = available[idx].clone();
                    result
                        .server_to_segments
                        .entry(server)
                        .or_insert_with(|| SegmentsToQuery::new(Vec::new()))
                        .segments
                        .push(segment.clone());
                } else {
                    result.unavailable_segments.push(segment.clone());
                }
            } else {
                result.unavailable_segments.push(segment.clone());
            }
        }

        Ok(result)
    }

    fn name(&self) -> &'static str {
        "HashBased"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_instances() -> HashMap<String, Vec<ServerInstance>> {
        let mut map = HashMap::new();
        map.insert(
            "segment1".to_string(),
            vec![
                ServerInstance::new("host1".to_string(), 8099, TableType::Offline),
                ServerInstance::new("host2".to_string(), 8099, TableType::Offline),
            ],
        );
        map.insert(
            "segment2".to_string(),
            vec![
                ServerInstance::new("host2".to_string(), 8099, TableType::Offline),
                ServerInstance::new("host3".to_string(), 8099, TableType::Offline),
            ],
        );
        map
    }

    #[test]
    fn test_balanced_selector() {
        let selector = BalancedInstanceSelector::new();
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string(), "segment2".to_string()];

        let result = selector.select(&segments, &instances, 1).unwrap();
        assert!(!result.is_empty());
        assert!(result.unavailable_segments.is_empty());
    }

    #[test]
    fn test_balanced_selector_round_robin() {
        let selector = BalancedInstanceSelector::new();
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string()];

        // Multiple selections should rotate
        let result1 = selector.select(&segments, &instances, 1).unwrap();
        let result2 = selector.select(&segments, &instances, 2).unwrap();

        // Should get different servers
        let server1: Vec<_> = result1.server_to_segments.keys().collect();
        let server2: Vec<_> = result2.server_to_segments.keys().collect();
        assert_ne!(server1, server2);
    }

    #[test]
    fn test_exclude_instance() {
        let selector = BalancedInstanceSelector::new();
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string()];

        let excluded = ServerInstance::new("host1".to_string(), 8099, TableType::Offline);
        selector.exclude_instance(&excluded);

        // Should always select host2 now
        for _ in 0..5 {
            let result = selector.select(&segments, &instances, 1).unwrap();
            for (server, _) in &result.server_to_segments {
                assert_eq!(server.hostname, "host2");
            }
        }
    }

    #[test]
    fn test_unavailable_segments() {
        let selector = BalancedInstanceSelector::new();
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string(), "missing_segment".to_string()];

        let result = selector.select(&segments, &instances, 1).unwrap();
        assert_eq!(result.unavailable_segments.len(), 1);
        assert_eq!(result.unavailable_segments[0], "missing_segment");
    }

    #[test]
    fn test_hash_based_selector_consistency() {
        let selector = HashBasedInstanceSelector::new();
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string()];

        // Multiple selections should return same server (consistent hashing)
        let result1 = selector.select(&segments, &instances, 1).unwrap();
        let result2 = selector.select(&segments, &instances, 2).unwrap();

        let server1: Vec<_> = result1.server_to_segments.keys().collect();
        let server2: Vec<_> = result2.server_to_segments.keys().collect();
        assert_eq!(server1, server2);
    }

    #[test]
    fn test_replica_group_selector() {
        let selector = ReplicaGroupInstanceSelector::new(3);
        let instances = create_test_instances();
        let segments = vec!["segment1".to_string(), "segment2".to_string()];

        let result = selector.select(&segments, &instances, 1).unwrap();
        assert!(!result.is_empty());
    }
}
