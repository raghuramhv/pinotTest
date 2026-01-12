//! Segment-level dictionary readers module
//!
//! Provides abstractions for reading dictionary values from segment files.

mod value_reader;
mod fixed_byte_reader;
mod var_length_reader;
mod segment_dictionary;

pub use value_reader::{ValueReader, ValueReaderError};
pub use fixed_byte_reader::FixedByteValueReader;
pub use var_length_reader::VarLengthValueReader;
pub use segment_dictionary::{SegmentDictionary, SegmentDictionaryConfig};
