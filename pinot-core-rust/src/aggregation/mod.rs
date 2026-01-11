//! Aggregation functions module
//!
//! This module provides high-performance aggregation function implementations
//! compatible with Apache Pinot's query execution engine.
//!
//! # Supported Aggregations
//!
//! - SUM: Sum of values
//! - COUNT: Count of values
//! - MIN: Minimum value
//! - MAX: Maximum value
//! - AVG: Average value
//!
//! # GroupBy Support
//!
//! All aggregations support both:
//! - Non-grouped aggregation (single result)
//! - GroupBy aggregation (result per group)

mod functions;
mod result_holders;
mod executor;

pub use functions::*;
pub use result_holders::*;
pub use executor::*;

use thiserror::Error;

/// Errors that can occur during aggregation
#[derive(Debug, Error)]
pub enum AggregationError {
    #[error("Invalid group key: {0}")]
    InvalidGroupKey(i32),

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("Result holder not initialized")]
    NotInitialized,

    #[error("Capacity exceeded: {current} >= {max}")]
    CapacityExceeded { current: usize, max: usize },

    #[error("Null handling error: {0}")]
    NullHandling(String),
}

pub type AggregationResult<T> = std::result::Result<T, AggregationError>;

/// Aggregation function types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregationType {
    Sum,
    Count,
    Min,
    Max,
    Avg,
    DistinctCount,
}

impl std::fmt::Display for AggregationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AggregationType::Sum => write!(f, "SUM"),
            AggregationType::Count => write!(f, "COUNT"),
            AggregationType::Min => write!(f, "MIN"),
            AggregationType::Max => write!(f, "MAX"),
            AggregationType::Avg => write!(f, "AVG"),
            AggregationType::DistinctCount => write!(f, "DISTINCTCOUNT"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregation_type_display() {
        assert_eq!(AggregationType::Sum.to_string(), "SUM");
        assert_eq!(AggregationType::Count.to_string(), "COUNT");
        assert_eq!(AggregationType::Min.to_string(), "MIN");
        assert_eq!(AggregationType::Max.to_string(), "MAX");
        assert_eq!(AggregationType::Avg.to_string(), "AVG");
    }
}
