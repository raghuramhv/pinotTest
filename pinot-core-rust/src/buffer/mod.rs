//! Buffer management module
//!
//! Provides off-heap buffer management for high-performance data access,
//! including both direct memory allocation and memory-mapped file support.

mod data_buffer;
mod pinot_buffer;
mod memory_manager;

pub use data_buffer::*;
pub use pinot_buffer::*;
pub use memory_manager::*;

use thiserror::Error;

/// Errors that can occur during buffer operations
#[derive(Debug, Error)]
pub enum BufferError {
    #[error("Buffer allocation failed: {0}")]
    AllocationFailed(String),

    #[error("Buffer out of bounds: offset {offset}, size {size}, buffer size {buffer_size}")]
    OutOfBounds {
        offset: u64,
        size: u64,
        buffer_size: u64,
    },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Buffer already closed")]
    AlreadyClosed,

    #[error("Memory mapping failed: {0}")]
    MmapFailed(String),

    #[error("Invalid byte order")]
    InvalidByteOrder,
}

pub type BufferResult<T> = std::result::Result<T, BufferError>;

/// Buffer type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferType {
    Direct,
    Mmap,
}

impl std::fmt::Display for BufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BufferType::Direct => write!(f, "DIRECT"),
            BufferType::Mmap => write!(f, "MMAP"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_type_display() {
        assert_eq!(BufferType::Direct.to_string(), "DIRECT");
        assert_eq!(BufferType::Mmap.to_string(), "MMAP");
    }
}
