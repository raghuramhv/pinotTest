//! Fixed-length value reader for segment dictionaries.
//!
//! Handles reading fixed-size values (INT, LONG, FLOAT, DOUBLE) from buffers
//! where each value occupies exactly the same number of bytes.

use super::value_reader::{ValueReader, ValueReaderError, ValueReaderResult};
use crate::buffer::{DataBuffer, VecDataBuffer, ByteOrderType};
use std::cmp::Ordering;
use std::sync::Arc;

/// Reader for fixed-length values stored in a contiguous buffer.
///
/// Storage format: `[value0][value1][value2]...`
/// where each value is exactly `num_bytes_per_value` bytes.
///
/// Offset calculation: `offset = index * num_bytes_per_value`
pub struct FixedByteValueReader {
    /// Underlying buffer containing the data
    buffer: Arc<dyn DataBuffer>,
    /// Number of values in the buffer
    length: usize,
    /// Bytes per value
    num_bytes_per_value: usize,
    /// Starting offset in the buffer
    start_offset: usize,
}

impl FixedByteValueReader {
    /// Create a new fixed-byte value reader.
    ///
    /// # Arguments
    /// * `buffer` - The underlying buffer
    /// * `length` - Number of values
    /// * `num_bytes_per_value` - Bytes per value
    pub fn new(buffer: Arc<dyn DataBuffer>, length: usize, num_bytes_per_value: usize) -> Self {
        Self {
            buffer,
            length,
            num_bytes_per_value,
            start_offset: 0,
        }
    }

    /// Create a reader with a custom start offset.
    pub fn with_offset(
        buffer: Arc<dyn DataBuffer>,
        length: usize,
        num_bytes_per_value: usize,
        start_offset: usize,
    ) -> Self {
        Self {
            buffer,
            length,
            num_bytes_per_value,
            start_offset,
        }
    }

    /// Create a reader from raw bytes.
    pub fn from_bytes(data: Vec<u8>, length: usize, num_bytes_per_value: usize) -> Self {
        let buffer = VecDataBuffer::from_vec(data, ByteOrderType::BigEndian);
        Self::new(Arc::new(buffer), length, num_bytes_per_value)
    }

    /// Calculate the byte offset for a given index.
    #[inline]
    fn offset_for(&self, index: usize) -> u64 {
        (self.start_offset + index * self.num_bytes_per_value) as u64
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

    /// Check if buffer has enough bytes at offset.
    #[inline]
    fn check_buffer(&self, offset: u64, needed: usize) -> ValueReaderResult<()> {
        if offset + needed as u64 > self.buffer.size() {
            return Err(ValueReaderError::BufferUnderflow {
                offset: offset as usize,
                needed,
            });
        }
        Ok(())
    }

    /// Convert buffer error to value reader error
    #[inline]
    fn convert_err(e: crate::buffer::BufferError) -> ValueReaderError {
        ValueReaderError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

impl ValueReader for FixedByteValueReader {
    fn len(&self) -> usize {
        self.length
    }

    fn bytes_per_value(&self) -> usize {
        self.num_bytes_per_value
    }

    fn get_int(&self, index: usize) -> ValueReaderResult<i32> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, 4)?;
        self.buffer.get_int(offset).map_err(Self::convert_err)
    }

    fn get_long(&self, index: usize) -> ValueReaderResult<i64> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, 8)?;
        self.buffer.get_long(offset).map_err(Self::convert_err)
    }

    fn get_float(&self, index: usize) -> ValueReaderResult<f32> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, 4)?;
        self.buffer.get_float(offset).map_err(Self::convert_err)
    }

    fn get_double(&self, index: usize) -> ValueReaderResult<f64> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, 8)?;
        self.buffer.get_double(offset).map_err(Self::convert_err)
    }

    fn get_bytes(&self, index: usize) -> ValueReaderResult<Vec<u8>> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, self.num_bytes_per_value)?;

        let mut result = vec![0u8; self.num_bytes_per_value];
        self.buffer.copy_to(offset, &mut result).map_err(Self::convert_err)?;
        Ok(result)
    }

    fn compare_bytes(&self, index1: usize, index2: usize) -> ValueReaderResult<Ordering> {
        self.check_bounds(index1)?;
        self.check_bounds(index2)?;

        let offset1 = self.offset_for(index1);
        let offset2 = self.offset_for(index2);

        self.check_buffer(offset1, self.num_bytes_per_value)?;
        self.check_buffer(offset2, self.num_bytes_per_value)?;

        // Compare byte by byte
        for i in 0..self.num_bytes_per_value {
            let b1 = self.buffer.get_byte(offset1 + i as u64).map_err(Self::convert_err)?;
            let b2 = self.buffer.get_byte(offset2 + i as u64).map_err(Self::convert_err)?;
            match b1.cmp(&b2) {
                Ordering::Equal => continue,
                other => return Ok(other),
            }
        }
        Ok(Ordering::Equal)
    }

    fn compare_with_bytes(&self, index: usize, value: &[u8]) -> ValueReaderResult<Ordering> {
        self.check_bounds(index)?;
        let offset = self.offset_for(index);
        self.check_buffer(offset, self.num_bytes_per_value)?;

        let len = self.num_bytes_per_value.min(value.len());
        for i in 0..len {
            let b1 = self.buffer.get_byte(offset + i as u64).map_err(Self::convert_err)?;
            let b2 = value[i];
            match b1.cmp(&b2) {
                Ordering::Equal => continue,
                other => return Ok(other),
            }
        }

        // If all compared bytes are equal, compare lengths
        Ok(self.num_bytes_per_value.cmp(&value.len()))
    }

    fn get_int_batch(&self, indices: &[usize], output: &mut [i32]) -> ValueReaderResult<()> {
        // Optimized batch read with bounds check once
        if let Some(&max_idx) = indices.iter().max() {
            self.check_bounds(max_idx)?;
        }

        for (i, &idx) in indices.iter().enumerate() {
            let offset = self.offset_for(idx);
            output[i] = self.buffer.get_int(offset).map_err(Self::convert_err)?;
        }
        Ok(())
    }

    fn get_long_batch(&self, indices: &[usize], output: &mut [i64]) -> ValueReaderResult<()> {
        if let Some(&max_idx) = indices.iter().max() {
            self.check_bounds(max_idx)?;
        }

        for (i, &idx) in indices.iter().enumerate() {
            let offset = self.offset_for(idx);
            output[i] = self.buffer.get_long(offset).map_err(Self::convert_err)?;
        }
        Ok(())
    }

    fn get_double_batch(&self, indices: &[usize], output: &mut [f64]) -> ValueReaderResult<()> {
        if let Some(&max_idx) = indices.iter().max() {
            self.check_bounds(max_idx)?;
        }

        for (i, &idx) in indices.iter().enumerate() {
            let offset = self.offset_for(idx);
            output[i] = self.buffer.get_double(offset).map_err(Self::convert_err)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_int_buffer() -> FixedByteValueReader {
        // Create buffer with 5 i32 values: 10, 20, 30, 40, 50
        let mut data = Vec::with_capacity(20);
        for &val in &[10i32, 20, 30, 40, 50] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        FixedByteValueReader::from_bytes(data, 5, 4)
    }

    fn create_long_buffer() -> FixedByteValueReader {
        // Create buffer with 3 i64 values
        let mut data = Vec::with_capacity(24);
        for &val in &[100i64, 200, 300] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        FixedByteValueReader::from_bytes(data, 3, 8)
    }

    #[test]
    fn test_get_int() {
        let reader = create_int_buffer();
        assert_eq!(reader.get_int(0).unwrap(), 10);
        assert_eq!(reader.get_int(2).unwrap(), 30);
        assert_eq!(reader.get_int(4).unwrap(), 50);
    }

    #[test]
    fn test_get_int_out_of_bounds() {
        let reader = create_int_buffer();
        assert!(reader.get_int(5).is_err());
        assert!(reader.get_int(100).is_err());
    }

    #[test]
    fn test_get_long() {
        let reader = create_long_buffer();
        assert_eq!(reader.get_long(0).unwrap(), 100);
        assert_eq!(reader.get_long(1).unwrap(), 200);
        assert_eq!(reader.get_long(2).unwrap(), 300);
    }

    #[test]
    fn test_get_bytes() {
        let reader = create_int_buffer();
        let bytes = reader.get_bytes(0).unwrap();
        assert_eq!(bytes.len(), 4);
        assert_eq!(bytes, 10i32.to_be_bytes().to_vec());
    }

    #[test]
    fn test_compare_bytes() {
        let reader = create_int_buffer();
        // 10 < 20
        assert_eq!(
            reader.compare_bytes(0, 1).unwrap(),
            Ordering::Less
        );
        // 30 == 30
        assert_eq!(
            reader.compare_bytes(2, 2).unwrap(),
            Ordering::Equal
        );
        // 50 > 40
        assert_eq!(
            reader.compare_bytes(4, 3).unwrap(),
            Ordering::Greater
        );
    }

    #[test]
    fn test_batch_read() {
        let reader = create_int_buffer();
        let indices = [0, 2, 4];
        let mut output = [0i32; 3];
        reader.get_int_batch(&indices, &mut output).unwrap();
        assert_eq!(output, [10, 30, 50]);
    }

    #[test]
    fn test_len_and_bytes_per_value() {
        let reader = create_int_buffer();
        assert_eq!(reader.len(), 5);
        assert_eq!(reader.bytes_per_value(), 4);
        assert!(!reader.is_empty());
    }

    #[test]
    fn test_double_buffer() {
        let mut data = Vec::new();
        for &val in &[1.5f64, 2.5, 3.5] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 3, 8);

        assert_eq!(reader.get_double(0).unwrap(), 1.5);
        assert_eq!(reader.get_double(1).unwrap(), 2.5);
        assert_eq!(reader.get_double(2).unwrap(), 3.5);
    }

    #[test]
    fn test_float_buffer() {
        let mut data = Vec::new();
        for &val in &[1.5f32, 2.5, 3.5] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 3, 4);

        assert_eq!(reader.get_float(0).unwrap(), 1.5);
        assert_eq!(reader.get_float(1).unwrap(), 2.5);
        assert_eq!(reader.get_float(2).unwrap(), 3.5);
    }

    #[test]
    fn test_with_offset() {
        // Create buffer with header (4 bytes) + 3 ints
        let mut data = vec![0xFF, 0xFF, 0xFF, 0xFF]; // Header
        for &val in &[100i32, 200, 300] {
            data.extend_from_slice(&val.to_be_bytes());
        }

        let buffer = VecDataBuffer::from_vec(data, ByteOrderType::BigEndian);
        let reader = FixedByteValueReader::with_offset(
            Arc::new(buffer),
            3,
            4,
            4, // Start after header
        );

        assert_eq!(reader.get_int(0).unwrap(), 100);
        assert_eq!(reader.get_int(1).unwrap(), 200);
        assert_eq!(reader.get_int(2).unwrap(), 300);
    }
}
