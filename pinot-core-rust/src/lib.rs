//! # Pinot Core Rust
//!
//! High-performance Rust implementations of Apache Pinot core components.
//!
//! This crate provides Rust implementations of performance-critical components
//! from Apache Pinot, designed to be called from Java via JNI.
//!
//! ## Modules
//!
//! - `dictionary`: Dictionary encoding/decoding for columnar data
//! - `aggregation`: Aggregation functions (SUM, COUNT, MIN, MAX, AVG)
//! - `bitmap`: Bitmap-based filtering and DocIdSet operations
//! - `buffer`: Off-heap buffer management
//!
//! ## JNI Integration
//!
//! Each module exposes JNI-compatible functions that can be called from Java.

pub mod dictionary;
pub mod aggregation;
pub mod bitmap;
pub mod buffer;
pub mod jni;

// Re-export commonly used types
pub use dictionary::{Dictionary, MutableDictionary, DataType};
pub use aggregation::{AggregationFunction, AggregationType, ResultHolder};
pub use bitmap::{DocIdSet, DocIdIterator, BitmapDocIdSet};
pub use buffer::{PinotBuffer, BufferType};

/// Error types for the crate
#[derive(Debug, thiserror::Error)]
pub enum PinotError {
    #[error("Dictionary error: {0}")]
    Dictionary(#[from] dictionary::DictionaryError),

    #[error("Aggregation error: {0}")]
    Aggregation(#[from] aggregation::AggregationError),

    #[error("Bitmap error: {0}")]
    Bitmap(#[from] bitmap::BitmapError),

    #[error("Buffer error: {0}")]
    Buffer(#[from] buffer::BufferError),

    #[error("JNI error: {0}")]
    Jni(String),
}

pub type Result<T> = std::result::Result<T, PinotError>;

/// Constants used across the crate
pub mod constants {
    /// Sentinel value for "not found" in dictionary lookups
    pub const NULL_VALUE_INDEX: i32 = -1;

    /// End of file/iterator marker
    pub const EOF: i32 = i32::MAX;

    /// Invalid group key marker
    pub const INVALID_GROUP_KEY: i32 = -1;

    /// Threshold for switching between byte-by-byte and bulk operations
    pub const BULK_BYTES_PROCESSING_THRESHOLD: usize = 10;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(constants::NULL_VALUE_INDEX, -1);
        assert_eq!(constants::EOF, i32::MAX);
    }
}
