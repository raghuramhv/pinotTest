//! Variable-length value reader for segment dictionaries.
//!
//! Handles reading variable-size values (STRING, BYTES) from buffers
//! where each value can have a different length.

use super::value_reader::{ValueReader, ValueReaderError, ValueReaderResult};
use crate::buffer::{DataBuffer, VecDataBuffer, ByteOrderType};
use std::cmp::Ordering;
use std::sync::Arc;

/// Storage format for variable-length values.
#[derive(Debug, Clone, Copy)]
pub enum VarLengthFormat {
    /// Offset table at the start: `[offset0][offset1]...[offsetN][data0][data1]...`
    /// Each offset is 4 bytes pointing to the start of the value
    OffsetTable,
    /// Length-prefixed: `[len0][data0][len1][data1]...`
    /// Each length is 4 bytes followed by the actual data
    LengthPrefixed,
}

/// Reader for variable-length values stored in a buffer.
///
/// Supports two storage formats:
/// 1. Offset table: Array of offsets followed by packed data
/// 2. Length-prefixed: Each value preceded by its length
pub struct VarLengthValueReader {
    /// Underlying buffer containing the data
    buffer: Arc<dyn DataBuffer>,
    /// Number of values in the buffer
    length: usize,
    /// Storage format
    format: VarLengthFormat,
    /// Offset to the data section (for OffsetTable format)
    data_offset: usize,
    /// Cached offsets for faster access (lazily computed)
    offsets: Vec<usize>,
    /// Cached lengths for each value
    lengths: Vec<usize>,
}

impl VarLengthValueReader {
    /// Convert buffer error to value reader error
    #[inline]
    fn convert_err(e: crate::buffer::BufferError) -> ValueReaderError {
        ValueReaderError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }

    /// Create a new variable-length value reader with offset table format.
    ///
    /// # Arguments
    /// * `buffer` - The underlying buffer
    /// * `length` - Number of values
    ///
    /// Buffer format: `[offset0: i32][offset1: i32]...[offsetN: i32][data...]`
    pub fn with_offset_table(buffer: Arc<dyn DataBuffer>, length: usize) -> ValueReaderResult<Self> {
        let offset_table_size = (length + 1) * 4; // N+1 offsets for N values
        let data_offset = offset_table_size;

        // Read all offsets
        let mut offsets = Vec::with_capacity(length + 1);
        for i in 0..=length {
            let offset_pos = (i * 4) as u64;
            if offset_pos + 4 > buffer.size() {
                return Err(ValueReaderError::BufferUnderflow {
                    offset: offset_pos as usize,
                    needed: 4,
                });
            }
            let val = buffer.get_int(offset_pos).map_err(Self::convert_err)?;
            offsets.push(val as usize);
        }

        // Compute lengths from offsets
        let mut lengths = Vec::with_capacity(length);
        for i in 0..length {
            lengths.push(offsets[i + 1] - offsets[i]);
        }

        Ok(Self {
            buffer,
            length,
            format: VarLengthFormat::OffsetTable,
            data_offset,
            offsets,
            lengths,
        })
    }

    /// Create a new variable-length value reader with length-prefixed format.
    ///
    /// Buffer format: `[len0: i32][data0][len1: i32][data1]...`
    pub fn with_length_prefix(buffer: Arc<dyn DataBuffer>, length: usize) -> ValueReaderResult<Self> {
        let mut offsets = Vec::with_capacity(length);
        let mut lengths = Vec::with_capacity(length);

        // Scan through buffer to build offset/length tables
        let mut pos = 0u64;
        for _ in 0..length {
            if pos + 4 > buffer.size() {
                return Err(ValueReaderError::BufferUnderflow {
                    offset: pos as usize,
                    needed: 4,
                });
            }

            let value_len = buffer.get_int(pos).map_err(Self::convert_err)? as usize;
            offsets.push((pos + 4) as usize); // Data starts after length prefix
            lengths.push(value_len);
            pos += 4 + value_len as u64;
        }

        Ok(Self {
            buffer,
            length,
            format: VarLengthFormat::LengthPrefixed,
            data_offset: 0,
            offsets,
            lengths,
        })
    }

    /// Create from raw bytes with offset table format.
    pub fn from_bytes_offset_table(data: Vec<u8>, length: usize) -> ValueReaderResult<Self> {
        let buffer = VecDataBuffer::from_vec(data, ByteOrderType::BigEndian);
        Self::with_offset_table(Arc::new(buffer), length)
    }

    /// Create from raw bytes with length-prefixed format.
    pub fn from_bytes_length_prefix(data: Vec<u8>, length: usize) -> ValueReaderResult<Self> {
        let buffer = VecDataBuffer::from_vec(data, ByteOrderType::BigEndian);
        Self::with_length_prefix(Arc::new(buffer), length)
    }

    /// Get the byte offset for a value.
    #[inline]
    fn value_offset(&self, index: usize) -> u64 {
        match self.format {
            VarLengthFormat::OffsetTable => (self.data_offset + self.offsets[index]) as u64,
            VarLengthFormat::LengthPrefixed => self.offsets[index] as u64,
        }
    }

    /// Check if index is valid.
    #[inline]
    fn check_bounds(&self, index: usize) -> ValueReaderResult<()> {
        if index >= self.length {
            return Err(ValueReaderError::IndexOutOfBounds {
                index,
                length: self.length,
            });
        }
        Ok(())
    }
}

impl ValueReader for VarLengthValueReader {
    fn len(&self) -> usize {
        self.length
    }

    fn bytes_per_value(&self) -> usize {
        0 // Variable length
    }

    fn get_int(&self, index: usize) -> ValueReaderResult<i32> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);

        if self.lengths[index] < 4 {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed: 4,
            });
        }

        self.buffer.get_int(offset).map_err(Self::convert_err)
    }

    fn get_long(&self, index: usize) -> ValueReaderResult<i64> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);

        if self.lengths[index] < 8 {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed: 8,
            });
        }

        self.buffer.get_long(offset).map_err(Self::convert_err)
    }

    fn get_float(&self, index: usize) -> ValueReaderResult<f32> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);

        if self.lengths[index] < 4 {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed: 4,
            });
        }

        self.buffer.get_float(offset).map_err(Self::convert_err)
    }

    fn get_double(&self, index: usize) -> ValueReaderResult<f64> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);

        if self.lengths[index] < 8 {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed: 8,
            });
        }

        self.buffer.get_double(offset).map_err(Self::convert_err)
    }

    fn get_bytes(&self, index: usize) -> ValueReaderResult<Vec<u8>> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);
        let len = self.lengths[index];

        if offset + len as u64 > self.buffer.size() {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed: len,
            });
        }

        let mut result = vec![0u8; len];
        self.buffer.copy_to(offset, &mut result).map_err(Self::convert_err)?;
        Ok(result)
    }

    fn get_string(&self, index: usize) -> ValueReaderResult<String> {
        let bytes = self.get_bytes(index)?;
        // Trim trailing null bytes (common in fixed-width string storage)
        let trimmed = if let Some(pos) = bytes.iter().position(|&b| b == 0) {
            &bytes[..pos]
        } else {
            &bytes[..]
        };
        String::from_utf8(trimmed.to_vec()).map_err(|_| ValueReaderError::InvalidUtf8 { index })
    }

    fn compare_bytes(&self, index1: usize, index2: usize) -> ValueReaderResult<Ordering> {
        self.check_bounds(index1)?;
        self.check_bounds(index2)?;

        let offset1 = self.value_offset(index1);
        let offset2 = self.value_offset(index2);
        let len1 = self.lengths[index1];
        let len2 = self.lengths[index2];

        let min_len = len1.min(len2);

        // Compare byte by byte
        for i in 0..min_len {
            let b1 = self.buffer.get_byte(offset1 + i as u64).map_err(Self::convert_err)?;
            let b2 = self.buffer.get_byte(offset2 + i as u64).map_err(Self::convert_err)?;
            match b1.cmp(&b2) {
                Ordering::Equal => continue,
                other => return Ok(other),
            }
        }

        // If all compared bytes are equal, shorter value is "less"
        Ok(len1.cmp(&len2))
    }

    fn compare_with_bytes(&self, index: usize, value: &[u8]) -> ValueReaderResult<Ordering> {
        self.check_bounds(index)?;
        let offset = self.value_offset(index);
        let len = self.lengths[index];

        let min_len = len.min(value.len());

        for i in 0..min_len {
            let b1 = self.buffer.get_byte(offset + i as u64).map_err(Self::convert_err)?;
            let b2 = value[i];
            match b1.cmp(&b2) {
                Ordering::Equal => continue,
                other => return Ok(other),
            }
        }

        Ok(len.cmp(&value.len()))
    }
}

/// Builder for creating variable-length value buffers.
pub struct VarLengthValueWriter {
    values: Vec<Vec<u8>>,
}

impl VarLengthValueWriter {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, value: &[u8]) {
        self.values.push(value.to_vec());
    }

    pub fn push_string(&mut self, value: &str) {
        self.values.push(value.as_bytes().to_vec());
    }

    /// Build buffer with offset table format.
    pub fn build_offset_table(self) -> Vec<u8> {
        let num_values = self.values.len();
        let offset_table_size = (num_values + 1) * 4;

        // Calculate total data size
        let total_data_size: usize = self.values.iter().map(|v| v.len()).sum();
        let mut buffer = Vec::with_capacity(offset_table_size + total_data_size);

        // Write offset table
        let mut current_offset = 0i32;
        for value in &self.values {
            buffer.extend_from_slice(&current_offset.to_be_bytes());
            current_offset += value.len() as i32;
        }
        // Write final offset
        buffer.extend_from_slice(&current_offset.to_be_bytes());

        // Write data
        for value in &self.values {
            buffer.extend_from_slice(value);
        }

        buffer
    }

    /// Build buffer with length-prefixed format.
    pub fn build_length_prefixed(self) -> Vec<u8> {
        let total_size: usize = self.values.iter().map(|v| 4 + v.len()).sum();
        let mut buffer = Vec::with_capacity(total_size);

        for value in &self.values {
            buffer.extend_from_slice(&(value.len() as i32).to_be_bytes());
            buffer.extend_from_slice(value);
        }

        buffer
    }
}

impl Default for VarLengthValueWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_strings() -> Vec<&'static str> {
        vec!["apple", "banana", "cherry", "date", "elderberry"]
    }

    fn create_offset_table_reader() -> VarLengthValueReader {
        let strings = create_test_strings();
        let mut writer = VarLengthValueWriter::new();
        for s in &strings {
            writer.push_string(s);
        }
        let buffer = writer.build_offset_table();
        VarLengthValueReader::from_bytes_offset_table(buffer, strings.len()).unwrap()
    }

    fn create_length_prefix_reader() -> VarLengthValueReader {
        let strings = create_test_strings();
        let mut writer = VarLengthValueWriter::new();
        for s in &strings {
            writer.push_string(s);
        }
        let buffer = writer.build_length_prefixed();
        VarLengthValueReader::from_bytes_length_prefix(buffer, strings.len()).unwrap()
    }

    #[test]
    fn test_offset_table_get_string() {
        let reader = create_offset_table_reader();
        assert_eq!(reader.get_string(0).unwrap(), "apple");
        assert_eq!(reader.get_string(1).unwrap(), "banana");
        assert_eq!(reader.get_string(2).unwrap(), "cherry");
        assert_eq!(reader.get_string(3).unwrap(), "date");
        assert_eq!(reader.get_string(4).unwrap(), "elderberry");
    }

    #[test]
    fn test_length_prefix_get_string() {
        let reader = create_length_prefix_reader();
        assert_eq!(reader.get_string(0).unwrap(), "apple");
        assert_eq!(reader.get_string(1).unwrap(), "banana");
        assert_eq!(reader.get_string(4).unwrap(), "elderberry");
    }

    #[test]
    fn test_get_bytes() {
        let reader = create_offset_table_reader();
        let bytes = reader.get_bytes(0).unwrap();
        assert_eq!(bytes, b"apple".to_vec());
    }

    #[test]
    fn test_out_of_bounds() {
        let reader = create_offset_table_reader();
        assert!(reader.get_string(5).is_err());
        assert!(reader.get_string(100).is_err());
    }

    #[test]
    fn test_compare_bytes() {
        let reader = create_offset_table_reader();
        // "apple" < "banana"
        assert_eq!(
            reader.compare_bytes(0, 1).unwrap(),
            Ordering::Less
        );
        // "cherry" == "cherry"
        assert_eq!(
            reader.compare_bytes(2, 2).unwrap(),
            Ordering::Equal
        );
        // "elderberry" > "date"
        assert_eq!(
            reader.compare_bytes(4, 3).unwrap(),
            Ordering::Greater
        );
    }

    #[test]
    fn test_compare_with_bytes() {
        let reader = create_offset_table_reader();
        assert_eq!(
            reader.compare_with_bytes(0, b"apple").unwrap(),
            Ordering::Equal
        );
        assert_eq!(
            reader.compare_with_bytes(0, b"banana").unwrap(),
            Ordering::Less
        );
        assert_eq!(
            reader.compare_with_bytes(1, b"apple").unwrap(),
            Ordering::Greater
        );
    }

    #[test]
    fn test_len_and_bytes_per_value() {
        let reader = create_offset_table_reader();
        assert_eq!(reader.len(), 5);
        assert_eq!(reader.bytes_per_value(), 0); // Variable length
        assert!(!reader.is_empty());
    }

    #[test]
    fn test_empty_strings() {
        let mut writer = VarLengthValueWriter::new();
        writer.push_string("");
        writer.push_string("hello");
        writer.push_string("");

        let buffer = writer.build_offset_table();
        let reader = VarLengthValueReader::from_bytes_offset_table(buffer, 3).unwrap();

        assert_eq!(reader.get_string(0).unwrap(), "");
        assert_eq!(reader.get_string(1).unwrap(), "hello");
        assert_eq!(reader.get_string(2).unwrap(), "");
    }

    #[test]
    fn test_binary_data() {
        let mut writer = VarLengthValueWriter::new();
        writer.push(&[0x00, 0xFF, 0x80]);
        writer.push(&[0x01, 0x02, 0x03, 0x04, 0x05]);

        let buffer = writer.build_length_prefixed();
        let reader = VarLengthValueReader::from_bytes_length_prefix(buffer, 2).unwrap();

        assert_eq!(reader.get_bytes(0).unwrap(), vec![0x00, 0xFF, 0x80]);
        assert_eq!(
            reader.get_bytes(1).unwrap(),
            vec![0x01, 0x02, 0x03, 0x04, 0x05]
        );
    }

    #[test]
    fn test_writer_with_capacity() {
        let mut writer = VarLengthValueWriter::with_capacity(100);
        for i in 0..100 {
            writer.push_string(&format!("value_{}", i));
        }
        let buffer = writer.build_offset_table();
        let reader = VarLengthValueReader::from_bytes_offset_table(buffer, 100).unwrap();

        assert_eq!(reader.get_string(0).unwrap(), "value_0");
        assert_eq!(reader.get_string(99).unwrap(), "value_99");
    }
}
