//! Dictionary encoding/decoding module
//!
//! This module provides dictionary-based encoding for columnar data,
//! supporting both immutable (offline) and mutable (realtime) dictionaries.
//!
//! # Dictionary Types
//!
//! - Immutable dictionaries: Pre-built, sorted, used for offline segments
//! - Mutable dictionaries: Dynamic, unsorted, used for realtime segments
//!
//! # Supported Data Types
//!
//! - INT (i32)
//! - LONG (i64)
//! - FLOAT (f32)
//! - DOUBLE (f64)
//! - STRING
//! - BYTES

mod immutable;
mod mutable;
mod traits;
mod types;
pub mod readers;

#[cfg(test)]
mod proptest_tests;

pub use immutable::*;
pub use mutable::*;
pub use traits::*;
pub use types::*;
pub use readers::*;

use thiserror::Error;

/// Errors that can occur during dictionary operations
#[derive(Debug, Error)]
pub enum DictionaryError {
    #[error("Value not found in dictionary")]
    ValueNotFound,

    #[error("Dictionary is full (capacity: {capacity})")]
    DictionaryFull { capacity: usize },

    #[error("Invalid dictionary ID: {dict_id}")]
    InvalidDictId { dict_id: i32 },

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("Index out of bounds: {index} >= {length}")]
    IndexOutOfBounds { index: usize, length: usize },

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Concurrent modification detected")]
    ConcurrentModification,
}

pub type DictionaryResult<T> = std::result::Result<T, DictionaryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = DictionaryError::ValueNotFound;
        assert_eq!(err.to_string(), "Value not found in dictionary");
    }
}
