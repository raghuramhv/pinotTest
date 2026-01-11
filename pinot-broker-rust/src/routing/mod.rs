//! Query routing module.
//!
//! This module handles:
//! - Segment selection and pruning
//! - Instance (server) selection
//! - Adaptive server selection based on latency/load
//! - Routing table management

pub mod instance_selector;
pub mod segment_pruner;
pub mod routing_manager;
pub mod adaptive_selector;

pub use instance_selector::{InstanceSelector, BalancedInstanceSelector, SelectionResult};
pub use segment_pruner::{SegmentPruner, TimePruner, PartitionPruner};
pub use routing_manager::{RoutingManager, RoutingManagerBuilder};
pub use adaptive_selector::AdaptiveServerSelector;
