//! Result reduction and merging module.
//!
//! Handles:
//! - Merging DataTables from multiple servers
//! - Aggregation operations (GROUP BY, ORDER BY)
//! - Result limiting and pagination

pub mod reducer;
pub mod aggregator;
pub mod merger;

pub use reducer::{BrokerReduceService, ReduceConfig};
pub use aggregator::{Aggregator, AggregationFunction};
pub use merger::DataTableMerger;
