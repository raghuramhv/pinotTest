//! Routing manager for query routing.

use crate::config::RoutingConfig;
use crate::routing::adaptive_selector::{AdaptiveServerSelector, SelectionStrategy};
use crate::routing::instance_selector::{BalancedInstanceSelector, InstanceSelector, SelectionResult};
use crate::routing::segment_pruner::{CompositePruner, SegmentPruner, TimePruner, PartitionPruner, EmptySegmentPruner};
use crate::types::{BrokerRequest, RoutingTable, SegmentsToQuery, ServerInstance, TableType};
use crate::{BrokerError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Routing entry for a table.
#[derive(Debug)]
struct RoutingEntry {
    /// Table name
    table_name: String,
    /// Enabled server instances
    enabled_instances: Vec<ServerInstance>,
    /// Segment to instances mapping
    segment_to_instances: HashMap<String, Vec<ServerInstance>>,
    /// Last update time
    last_update: Instant,
}

impl RoutingEntry {
    fn new(table_name: String) -> Self {
        Self {
            table_name,
            enabled_instances: Vec::new(),
            segment_to_instances: HashMap::new(),
            last_update: Instant::now(),
        }
    }
}

/// Routing manager that handles query routing.
pub struct RoutingManager {
    /// Per-table routing entries
    routing_entries: DashMap<String, RoutingEntry>,
    /// Instance selector
    instance_selector: Arc<dyn InstanceSelector>,
    /// Segment pruners
    segment_pruner: Arc<dyn SegmentPruner>,
    /// Adaptive server selector (optional)
    adaptive_selector: Option<Arc<AdaptiveServerSelector>>,
    /// Configuration
    config: RoutingConfig,
    /// Global lock for routing updates
    update_lock: RwLock<()>,
}

impl RoutingManager {
    pub fn new(config: RoutingConfig) -> Self {
        let instance_selector: Arc<dyn InstanceSelector> = Arc::new(BalancedInstanceSelector::new());

        let segment_pruner: Arc<dyn SegmentPruner> = Arc::new(
            CompositePruner::builder()
                .add_pruner(EmptySegmentPruner::new())
                .add_pruner(TimePruner::new())
                .add_pruner(PartitionPruner::new())
                .build(),
        );

        let adaptive_selector = Some(Arc::new(
            AdaptiveServerSelector::new(SelectionStrategy::Hybrid)
                .with_ema_decay(0.3)
                .with_max_stats_age(Duration::from_secs(300)),
        ));

        Self {
            routing_entries: DashMap::new(),
            instance_selector,
            segment_pruner,
            adaptive_selector,
            config,
            update_lock: RwLock::new(()),
        }
    }

    /// Get the routing table for a query.
    pub fn get_routing_table(
        &self,
        request: &BrokerRequest,
        _request_id: u64,
    ) -> Result<RoutingTable> {
        let entry = self
            .routing_entries
            .get(&request.table_name)
            .ok_or_else(|| BrokerError::TableNotFound(request.table_name.clone()))?;

        // Get all segments for the table
        let all_segments: Vec<String> = entry.segment_to_instances.keys().cloned().collect();

        if all_segments.is_empty() {
            return Err(BrokerError::NoServersAvailable(request.table_name.clone()));
        }

        // Prune segments based on query
        let prune_result = self.segment_pruner.prune(request, all_segments)?;
        let segments = prune_result.segments;
        let num_pruned = prune_result.num_pruned;

        if segments.is_empty() {
            return Ok(RoutingTable {
                server_to_segments: HashMap::new(),
                unavailable_segments: Vec::new(),
                num_pruned_segments: num_pruned,
            });
        }

        // Select instances for segments
        let selection = self.instance_selector.select(
            &segments,
            &entry.segment_to_instances,
            _request_id,
        )?;

        // Apply adaptive selection if enabled
        let (server_to_segments, unavailable_segments) = if let Some(ref adaptive) = self.adaptive_selector {
            self.apply_adaptive_selection(selection, adaptive)?
        } else {
            (selection.server_to_segments, selection.unavailable_segments)
        };

        Ok(RoutingTable {
            server_to_segments,
            unavailable_segments,
            num_pruned_segments: num_pruned,
        })
    }

    fn apply_adaptive_selection(
        &self,
        selection: SelectionResult,
        _adaptive: &AdaptiveServerSelector,
    ) -> Result<(HashMap<ServerInstance, SegmentsToQuery>, Vec<String>)> {
        // For now, just use the selection as-is
        // In a full implementation, we would re-select based on adaptive metrics
        // when multiple replicas are available
        Ok((selection.server_to_segments, selection.unavailable_segments))
    }

    /// Register a table for routing.
    pub fn register_table(&self, table_name: String) {
        let _lock = self.update_lock.write();
        self.routing_entries
            .entry(table_name.clone())
            .or_insert_with(|| RoutingEntry::new(table_name));
    }

    /// Unregister a table from routing.
    pub fn unregister_table(&self, table_name: &str) {
        let _lock = self.update_lock.write();
        self.routing_entries.remove(table_name);
    }

    /// Update the segment to instances mapping for a table.
    pub fn update_segment_mapping(
        &self,
        table_name: &str,
        segment: String,
        instances: Vec<ServerInstance>,
    ) {
        let _lock = self.update_lock.write();
        if let Some(mut entry) = self.routing_entries.get_mut(table_name) {
            entry.segment_to_instances.insert(segment, instances);
            entry.last_update = Instant::now();
        }
    }

    /// Remove a segment from routing.
    pub fn remove_segment(&self, table_name: &str, segment: &str) {
        let _lock = self.update_lock.write();
        if let Some(mut entry) = self.routing_entries.get_mut(table_name) {
            entry.segment_to_instances.remove(segment);
            entry.last_update = Instant::now();
        }
    }

    /// Update enabled instances for a table.
    pub fn update_enabled_instances(&self, table_name: &str, instances: Vec<ServerInstance>) {
        let _lock = self.update_lock.write();
        if let Some(mut entry) = self.routing_entries.get_mut(table_name) {
            entry.enabled_instances = instances;
            entry.last_update = Instant::now();
        }
    }

    /// Get the adaptive server selector.
    pub fn adaptive_selector(&self) -> Option<&Arc<AdaptiveServerSelector>> {
        self.adaptive_selector.as_ref()
    }

    /// Get routing statistics.
    pub fn get_stats(&self) -> RoutingStats {
        let num_tables = self.routing_entries.len();
        let mut total_segments = 0;
        let mut total_instances = 0;

        for entry in self.routing_entries.iter() {
            total_segments += entry.segment_to_instances.len();
            total_instances += entry.enabled_instances.len();
        }

        RoutingStats {
            num_tables,
            total_segments,
            total_instances,
        }
    }

    /// Check if a table is registered.
    pub fn has_table(&self, table_name: &str) -> bool {
        self.routing_entries.contains_key(table_name)
    }

    /// Get all registered tables.
    pub fn get_tables(&self) -> Vec<String> {
        self.routing_entries
            .iter()
            .map(|e| e.table_name.clone())
            .collect()
    }
}

/// Routing statistics.
#[derive(Debug, Clone)]
pub struct RoutingStats {
    /// Number of registered tables
    pub num_tables: usize,
    /// Total number of segments across all tables
    pub total_segments: usize,
    /// Total number of enabled instances
    pub total_instances: usize,
}

/// Builder for RoutingManager.
pub struct RoutingManagerBuilder {
    config: RoutingConfig,
    instance_selector: Option<Arc<dyn InstanceSelector>>,
    segment_pruner: Option<Arc<dyn SegmentPruner>>,
    enable_adaptive: bool,
}

impl RoutingManagerBuilder {
    pub fn new() -> Self {
        Self {
            config: RoutingConfig::default(),
            instance_selector: None,
            segment_pruner: None,
            enable_adaptive: true,
        }
    }

    pub fn config(mut self, config: RoutingConfig) -> Self {
        self.config = config;
        self
    }

    pub fn instance_selector<S: InstanceSelector + 'static>(mut self, selector: S) -> Self {
        self.instance_selector = Some(Arc::new(selector));
        self
    }

    pub fn segment_pruner<P: SegmentPruner + 'static>(mut self, pruner: P) -> Self {
        self.segment_pruner = Some(Arc::new(pruner));
        self
    }

    pub fn enable_adaptive(mut self, enable: bool) -> Self {
        self.enable_adaptive = enable;
        self
    }

    pub fn build(self) -> RoutingManager {
        let instance_selector = self
            .instance_selector
            .unwrap_or_else(|| Arc::new(BalancedInstanceSelector::new()));

        let segment_pruner = self.segment_pruner.unwrap_or_else(|| {
            Arc::new(
                CompositePruner::builder()
                    .add_pruner(EmptySegmentPruner::new())
                    .add_pruner(TimePruner::new())
                    .add_pruner(PartitionPruner::new())
                    .build(),
            )
        });

        let adaptive_selector = if self.enable_adaptive {
            Some(Arc::new(
                AdaptiveServerSelector::new(SelectionStrategy::Hybrid)
                    .with_ema_decay(0.3)
                    .with_max_stats_age(Duration::from_secs(300)),
            ))
        } else {
            None
        };

        RoutingManager {
            routing_entries: DashMap::new(),
            instance_selector,
            segment_pruner,
            adaptive_selector,
            config: self.config,
            update_lock: RwLock::new(()),
        }
    }
}

impl Default for RoutingManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_routing_manager() -> RoutingManager {
        RoutingManagerBuilder::new().build()
    }

    fn create_request(table: &str) -> BrokerRequest {
        BrokerRequest::new(1, format!("SELECT * FROM {}", table), table.to_string())
    }

    #[test]
    fn test_register_table() {
        let manager = create_routing_manager();
        manager.register_table("myTable".to_string());

        assert!(manager.has_table("myTable"));
        assert!(!manager.has_table("otherTable"));
    }

    #[test]
    fn test_unregister_table() {
        let manager = create_routing_manager();
        manager.register_table("myTable".to_string());
        assert!(manager.has_table("myTable"));

        manager.unregister_table("myTable");
        assert!(!manager.has_table("myTable"));
    }

    #[test]
    fn test_update_segment_mapping() {
        let manager = create_routing_manager();
        manager.register_table("myTable".to_string());

        let instances = vec![
            ServerInstance::new("host1".to_string(), 8099, TableType::Offline),
            ServerInstance::new("host2".to_string(), 8099, TableType::Offline),
        ];

        manager.update_segment_mapping("myTable", "segment1".to_string(), instances);

        let request = create_request("myTable");
        let routing = manager.get_routing_table(&request, 1).unwrap();

        assert!(!routing.is_empty());
    }

    #[test]
    fn test_table_not_found() {
        let manager = create_routing_manager();
        let request = create_request("nonExistentTable");

        let result = manager.get_routing_table(&request, 1);
        assert!(matches!(result, Err(BrokerError::TableNotFound(_))));
    }

    #[test]
    fn test_get_stats() {
        let manager = create_routing_manager();
        manager.register_table("table1".to_string());
        manager.register_table("table2".to_string());

        let stats = manager.get_stats();
        assert_eq!(stats.num_tables, 2);
    }

    #[test]
    fn test_get_tables() {
        let manager = create_routing_manager();
        manager.register_table("table1".to_string());
        manager.register_table("table2".to_string());

        let tables = manager.get_tables();
        assert_eq!(tables.len(), 2);
        assert!(tables.contains(&"table1".to_string()));
        assert!(tables.contains(&"table2".to_string()));
    }

    #[test]
    fn test_remove_segment() {
        let manager = create_routing_manager();
        manager.register_table("myTable".to_string());

        let instances = vec![ServerInstance::new(
            "host1".to_string(),
            8099,
            TableType::Offline,
        )];

        manager.update_segment_mapping("myTable", "segment1".to_string(), instances.clone());
        manager.update_segment_mapping("myTable", "segment2".to_string(), instances);

        let request = create_request("myTable");
        let routing1 = manager.get_routing_table(&request, 1).unwrap();
        assert_eq!(routing1.num_segments(), 2);

        manager.remove_segment("myTable", "segment1");
        let routing2 = manager.get_routing_table(&request, 2).unwrap();
        assert_eq!(routing2.num_segments(), 1);
    }

    #[test]
    fn test_builder() {
        let manager = RoutingManagerBuilder::new()
            .enable_adaptive(false)
            .build();

        assert!(manager.adaptive_selector().is_none());

        let manager_with_adaptive = RoutingManagerBuilder::new()
            .enable_adaptive(true)
            .build();

        assert!(manager_with_adaptive.adaptive_selector().is_some());
    }
}
