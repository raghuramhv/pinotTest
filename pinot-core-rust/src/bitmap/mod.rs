//! Bitmap-based filtering and DocIdSet operations
//!
//! This module provides high-performance bitmap operations for query filtering,
//! including AND, OR, NOT operations and various DocIdSet implementations.

mod doc_id_set;
mod doc_id_iterator;
mod filter_operators;
mod scan_operator;

pub use doc_id_set::*;
pub use doc_id_iterator::*;
pub use filter_operators::*;
pub use scan_operator::*;

use thiserror::Error;

/// Errors that can occur during bitmap operations
#[derive(Debug, Error)]
pub enum BitmapError {
    #[error("Invalid document ID: {0}")]
    InvalidDocId(i32),

    #[error("Iterator exhausted")]
    IteratorExhausted,

    #[error("Capacity exceeded: {current} >= {max}")]
    CapacityExceeded { current: usize, max: usize },

    #[error("Empty bitmap operation")]
    EmptyBitmap,
}

pub type BitmapResult<T> = std::result::Result<T, BitmapError>;

/// End of file/iterator marker
pub const EOF: i32 = i32::MAX;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eof_constant() {
        assert_eq!(EOF, i32::MAX);
    }
}
