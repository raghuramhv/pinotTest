//! ValueReader trait for reading values by index from segment buffers.
//!
//! This abstraction allows dictionary implementations to read values
//! without knowing the underlying storage format (fixed-length vs variable-length).

use std::cmp::Ordering;
use thiserror::Error;

/// Errors that can occur during value reading operations.
#[derive(Debug, Error)]
pub enum ValueReaderError {
    #[error("Index out of bounds: {index} >= {length}")]
    IndexOutOfBounds { index: usize, length: usize },

    #[error("Buffer underflow at offset {offset}, needed {needed} bytes")]
    BufferUnderflow { offset: usize, needed: usize },

    #[error("Invalid UTF-8 data at index {index}")]
    InvalidUtf8 { index: usize },

    #[error("Buffer not initialized")]
    UninitializedBuffer,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ValueReaderResult<T> = std::result::Result<T, ValueReaderError>;

/// Trait for reading values by index from a segment buffer.
///
/// Implementations handle different storage formats:
/// - Fixed-length: Each value occupies exactly `num_bytes_per_value` bytes
/// - Variable-length: Values have a length prefix or offset table
pub trait ValueReader: Send + Sync {
    /// Returns the number of values in this reader.
    fn len(&self) -> usize;

    /// Returns true if the reader contains no values.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the number of bytes per value (for fixed-length types).
    /// Returns 0 for variable-length types.
    fn bytes_per_value(&self) -> usize;

    /// Read an i32 value at the given index.
    fn get_int(&self, index: usize) -> ValueReaderResult<i32>;

    /// Read an i64 value at the given index.
    fn get_long(&self, index: usize) -> ValueReaderResult<i64>;

    /// Read an f32 value at the given index.
    fn get_float(&self, index: usize) -> ValueReaderResult<f32>;

    /// Read an f64 value at the given index.
    fn get_double(&self, index: usize) -> ValueReaderResult<f64>;

    /// Read raw bytes at the given index.
    /// For fixed-length types, returns exactly `bytes_per_value()` bytes.
    /// For variable-length types, returns the actual data length.
    fn get_bytes(&self, index: usize) -> ValueReaderResult<Vec<u8>>;

    /// Read a string at the given index.
    fn get_string(&self, index: usize) -> ValueReaderResult<String> {
        let bytes = self.get_bytes(index)?;
        String::from_utf8(bytes).map_err(|_| ValueReaderError::InvalidUtf8 { index })
    }

    /// Compare values at two indices without materializing them.
    /// Returns ordering based on the raw byte representation.
    fn compare_bytes(&self, index1: usize, index2: usize) -> ValueReaderResult<Ordering>;

    /// Compare a value at index with the given bytes.
    fn compare_with_bytes(&self, index: usize, value: &[u8]) -> ValueReaderResult<Ordering>;

    /// Read multiple int values into a slice (batch operation).
    fn get_int_batch(&self, indices: &[usize], output: &mut [i32]) -> ValueReaderResult<()> {
        for (i, &idx) in indices.iter().enumerate() {
            output[i] = self.get_int(idx)?;
        }
        Ok(())
    }

    /// Read multiple long values into a slice (batch operation).
    fn get_long_batch(&self, indices: &[usize], output: &mut [i64]) -> ValueReaderResult<()> {
        for (i, &idx) in indices.iter().enumerate() {
            output[i] = self.get_long(idx)?;
        }
        Ok(())
    }

    /// Read multiple double values into a slice (batch operation).
    fn get_double_batch(&self, indices: &[usize], output: &mut [f64]) -> ValueReaderResult<()> {
        for (i, &idx) in indices.iter().enumerate() {
            output[i] = self.get_double(idx)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ValueReaderError::IndexOutOfBounds {
            index: 10,
            length: 5,
        };
        assert!(err.to_string().contains("10"));
        assert!(err.to_string().contains("5"));
    }
}
