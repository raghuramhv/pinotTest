//! Data buffer trait and implementations
//!
//! Defines the core interface for buffer operations.

use super::{BufferError, BufferResult};
use byteorder::{BigEndian, ByteOrder, LittleEndian, NativeEndian};

/// Byte order for buffer operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrderType {
    BigEndian,
    LittleEndian,
    Native,
}

impl ByteOrderType {
    pub fn is_native(&self) -> bool {
        matches!(self, ByteOrderType::Native)
            || (*self == ByteOrderType::LittleEndian && cfg!(target_endian = "little"))
            || (*self == ByteOrderType::BigEndian && cfg!(target_endian = "big"))
    }
}

/// Trait for data buffer operations
pub trait DataBuffer: Send + Sync {
    /// Returns the size of the buffer in bytes
    fn size(&self) -> u64;

    /// Returns the byte order of the buffer
    fn byte_order(&self) -> ByteOrderType;

    // ========== Primitive Read Operations ==========

    /// Reads a byte at the given offset
    fn get_byte(&self, offset: u64) -> BufferResult<u8>;

    /// Reads a char (2 bytes) at the given offset
    fn get_char(&self, offset: u64) -> BufferResult<char>;

    /// Reads a short (i16) at the given offset
    fn get_short(&self, offset: u64) -> BufferResult<i16>;

    /// Reads an int (i32) at the given offset
    fn get_int(&self, offset: u64) -> BufferResult<i32>;

    /// Reads a long (i64) at the given offset
    fn get_long(&self, offset: u64) -> BufferResult<i64>;

    /// Reads a float (f32) at the given offset
    fn get_float(&self, offset: u64) -> BufferResult<f32>;

    /// Reads a double (f64) at the given offset
    fn get_double(&self, offset: u64) -> BufferResult<f64>;

    // ========== Primitive Write Operations ==========

    /// Writes a byte at the given offset
    fn put_byte(&mut self, offset: u64, value: u8) -> BufferResult<()>;

    /// Writes a char (2 bytes) at the given offset
    fn put_char(&mut self, offset: u64, value: char) -> BufferResult<()>;

    /// Writes a short (i16) at the given offset
    fn put_short(&mut self, offset: u64, value: i16) -> BufferResult<()>;

    /// Writes an int (i32) at the given offset
    fn put_int(&mut self, offset: u64, value: i32) -> BufferResult<()>;

    /// Writes a long (i64) at the given offset
    fn put_long(&mut self, offset: u64, value: i64) -> BufferResult<()>;

    /// Writes a float (f32) at the given offset
    fn put_float(&mut self, offset: u64, value: f32) -> BufferResult<()>;

    /// Writes a double (f64) at the given offset
    fn put_double(&mut self, offset: u64, value: f64) -> BufferResult<()>;

    // ========== Bulk Operations ==========

    /// Copies data from buffer to byte array
    fn copy_to(&self, offset: u64, dest: &mut [u8]) -> BufferResult<()>;

    /// Reads data from byte array into buffer
    fn read_from(&mut self, offset: u64, src: &[u8]) -> BufferResult<()>;

    // ========== View Operations ==========

    /// Creates a view of a portion of this buffer
    fn view(&self, start: u64, end: u64) -> BufferResult<Box<dyn DataBuffer>>;

    /// Returns the raw pointer to the buffer data (unsafe)
    fn as_ptr(&self) -> *const u8;

    /// Returns a mutable raw pointer to the buffer data (unsafe)
    fn as_mut_ptr(&mut self) -> *mut u8;

    // ========== Lifecycle ==========

    /// Flushes any pending writes (for mmap buffers)
    fn flush(&self) -> BufferResult<()>;

    /// Closes the buffer and releases resources
    fn close(&mut self) -> BufferResult<()>;
}

/// In-memory data buffer backed by a Vec<u8>
pub struct VecDataBuffer {
    data: Vec<u8>,
    byte_order: ByteOrderType,
    closed: bool,
}

impl VecDataBuffer {
    pub fn new(size: usize, byte_order: ByteOrderType) -> Self {
        Self {
            data: vec![0u8; size],
            byte_order,
            closed: false,
        }
    }

    pub fn from_vec(data: Vec<u8>, byte_order: ByteOrderType) -> Self {
        Self {
            data,
            byte_order,
            closed: false,
        }
    }

    fn check_bounds(&self, offset: u64, size: u64) -> BufferResult<()> {
        if self.closed {
            return Err(BufferError::AlreadyClosed);
        }
        if offset + size > self.data.len() as u64 {
            return Err(BufferError::OutOfBounds {
                offset,
                size,
                buffer_size: self.data.len() as u64,
            });
        }
        Ok(())
    }
}

impl DataBuffer for VecDataBuffer {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }

    fn byte_order(&self) -> ByteOrderType {
        self.byte_order
    }

    fn get_byte(&self, offset: u64) -> BufferResult<u8> {
        self.check_bounds(offset, 1)?;
        Ok(self.data[offset as usize])
    }

    fn get_char(&self, offset: u64) -> BufferResult<char> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.data[offset as usize..offset as usize + 2];
        let code = match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_u16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_u16(bytes),
            ByteOrderType::Native => NativeEndian::read_u16(bytes),
        };
        Ok(char::from_u32(code as u32).unwrap_or('\0'))
    }

    fn get_short(&self, offset: u64) -> BufferResult<i16> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.data[offset as usize..offset as usize + 2];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i16(bytes),
            ByteOrderType::Native => NativeEndian::read_i16(bytes),
        })
    }

    fn get_int(&self, offset: u64) -> BufferResult<i32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.data[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i32(bytes),
            ByteOrderType::Native => NativeEndian::read_i32(bytes),
        })
    }

    fn get_long(&self, offset: u64) -> BufferResult<i64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.data[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i64(bytes),
            ByteOrderType::Native => NativeEndian::read_i64(bytes),
        })
    }

    fn get_float(&self, offset: u64) -> BufferResult<f32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.data[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f32(bytes),
            ByteOrderType::Native => NativeEndian::read_f32(bytes),
        })
    }

    fn get_double(&self, offset: u64) -> BufferResult<f64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.data[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f64(bytes),
            ByteOrderType::Native => NativeEndian::read_f64(bytes),
        })
    }

    fn put_byte(&mut self, offset: u64, value: u8) -> BufferResult<()> {
        self.check_bounds(offset, 1)?;
        self.data[offset as usize] = value;
        Ok(())
    }

    fn put_char(&mut self, offset: u64, value: char) -> BufferResult<()> {
        self.check_bounds(offset, 2)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 2];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_u16(bytes, value as u16),
            ByteOrderType::LittleEndian => LittleEndian::write_u16(bytes, value as u16),
            ByteOrderType::Native => NativeEndian::write_u16(bytes, value as u16),
        }
        Ok(())
    }

    fn put_short(&mut self, offset: u64, value: i16) -> BufferResult<()> {
        self.check_bounds(offset, 2)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 2];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i16(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i16(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i16(bytes, value),
        }
        Ok(())
    }

    fn put_int(&mut self, offset: u64, value: i32) -> BufferResult<()> {
        self.check_bounds(offset, 4)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 4];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i32(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i32(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i32(bytes, value),
        }
        Ok(())
    }

    fn put_long(&mut self, offset: u64, value: i64) -> BufferResult<()> {
        self.check_bounds(offset, 8)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 8];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i64(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i64(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i64(bytes, value),
        }
        Ok(())
    }

    fn put_float(&mut self, offset: u64, value: f32) -> BufferResult<()> {
        self.check_bounds(offset, 4)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 4];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_f32(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_f32(bytes, value),
            ByteOrderType::Native => NativeEndian::write_f32(bytes, value),
        }
        Ok(())
    }

    fn put_double(&mut self, offset: u64, value: f64) -> BufferResult<()> {
        self.check_bounds(offset, 8)?;
        let bytes = &mut self.data[offset as usize..offset as usize + 8];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_f64(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_f64(bytes, value),
            ByteOrderType::Native => NativeEndian::write_f64(bytes, value),
        }
        Ok(())
    }

    fn copy_to(&self, offset: u64, dest: &mut [u8]) -> BufferResult<()> {
        self.check_bounds(offset, dest.len() as u64)?;
        dest.copy_from_slice(&self.data[offset as usize..offset as usize + dest.len()]);
        Ok(())
    }

    fn read_from(&mut self, offset: u64, src: &[u8]) -> BufferResult<()> {
        self.check_bounds(offset, src.len() as u64)?;
        self.data[offset as usize..offset as usize + src.len()].copy_from_slice(src);
        Ok(())
    }

    fn view(&self, start: u64, end: u64) -> BufferResult<Box<dyn DataBuffer>> {
        self.check_bounds(start, end - start)?;
        let view_data = self.data[start as usize..end as usize].to_vec();
        Ok(Box::new(VecDataBuffer::from_vec(view_data, self.byte_order)))
    }

    fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    fn flush(&self) -> BufferResult<()> {
        // No-op for in-memory buffer
        Ok(())
    }

    fn close(&mut self) -> BufferResult<()> {
        self.closed = true;
        Ok(())
    }
}

/// Batch reader for efficient reading of multiple values
pub struct BatchReader<'a> {
    buffer: &'a dyn DataBuffer,
    offset: u64,
}

impl<'a> BatchReader<'a> {
    pub fn new(buffer: &'a dyn DataBuffer, offset: u64) -> Self {
        Self { buffer, offset }
    }

    /// Reads multiple ints into the output slice
    pub fn read_ints(&mut self, out: &mut [i32]) -> BufferResult<()> {
        for i in 0..out.len() {
            out[i] = self.buffer.get_int(self.offset)?;
            self.offset += 4;
        }
        Ok(())
    }

    /// Reads multiple longs into the output slice
    pub fn read_longs(&mut self, out: &mut [i64]) -> BufferResult<()> {
        for i in 0..out.len() {
            out[i] = self.buffer.get_long(self.offset)?;
            self.offset += 8;
        }
        Ok(())
    }

    /// Reads multiple floats into the output slice
    pub fn read_floats(&mut self, out: &mut [f32]) -> BufferResult<()> {
        for i in 0..out.len() {
            out[i] = self.buffer.get_float(self.offset)?;
            self.offset += 4;
        }
        Ok(())
    }

    /// Reads multiple doubles into the output slice
    pub fn read_doubles(&mut self, out: &mut [f64]) -> BufferResult<()> {
        for i in 0..out.len() {
            out[i] = self.buffer.get_double(self.offset)?;
            self.offset += 8;
        }
        Ok(())
    }

    /// Returns the current offset
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

/// Batch writer for efficient writing of multiple values
pub struct BatchWriter<'a> {
    buffer: &'a mut dyn DataBuffer,
    offset: u64,
}

impl<'a> BatchWriter<'a> {
    pub fn new(buffer: &'a mut dyn DataBuffer, offset: u64) -> Self {
        Self { buffer, offset }
    }

    /// Writes multiple ints from the input slice
    pub fn write_ints(&mut self, values: &[i32]) -> BufferResult<()> {
        for &value in values {
            self.buffer.put_int(self.offset, value)?;
            self.offset += 4;
        }
        Ok(())
    }

    /// Writes multiple longs from the input slice
    pub fn write_longs(&mut self, values: &[i64]) -> BufferResult<()> {
        for &value in values {
            self.buffer.put_long(self.offset, value)?;
            self.offset += 8;
        }
        Ok(())
    }

    /// Writes multiple floats from the input slice
    pub fn write_floats(&mut self, values: &[f32]) -> BufferResult<()> {
        for &value in values {
            self.buffer.put_float(self.offset, value)?;
            self.offset += 4;
        }
        Ok(())
    }

    /// Writes multiple doubles from the input slice
    pub fn write_doubles(&mut self, values: &[f64]) -> BufferResult<()> {
        for &value in values {
            self.buffer.put_double(self.offset, value)?;
            self.offset += 8;
        }
        Ok(())
    }

    /// Returns the current offset
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_buffer_basic() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        assert_eq!(buffer.size(), 100);

        // Write and read byte
        buffer.put_byte(0, 42).unwrap();
        assert_eq!(buffer.get_byte(0).unwrap(), 42);

        // Write and read int
        buffer.put_int(4, 12345).unwrap();
        assert_eq!(buffer.get_int(4).unwrap(), 12345);

        // Write and read long
        buffer.put_long(8, 123456789012345).unwrap();
        assert_eq!(buffer.get_long(8).unwrap(), 123456789012345);

        // Write and read float
        buffer.put_float(16, 3.14).unwrap();
        assert!((buffer.get_float(16).unwrap() - 3.14).abs() < 0.001);

        // Write and read double
        buffer.put_double(20, 3.14159265358979).unwrap();
        assert!((buffer.get_double(20).unwrap() - 3.14159265358979).abs() < f64::EPSILON);
    }

    #[test]
    fn test_vec_buffer_bounds_check() {
        let buffer = VecDataBuffer::new(10, ByteOrderType::Native);

        // Should succeed
        assert!(buffer.get_byte(9).is_ok());

        // Should fail
        assert!(buffer.get_byte(10).is_err());
        assert!(buffer.get_int(8).is_err()); // 8 + 4 > 10
    }

    #[test]
    fn test_vec_buffer_bulk_operations() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        let src = [1u8, 2, 3, 4, 5];
        buffer.read_from(10, &src).unwrap();

        let mut dest = [0u8; 5];
        buffer.copy_to(10, &mut dest).unwrap();

        assert_eq!(dest, src);
    }

    #[test]
    fn test_vec_buffer_view() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        buffer.put_int(10, 42).unwrap();
        buffer.put_int(14, 43).unwrap();

        let view = buffer.view(10, 20).unwrap();
        assert_eq!(view.size(), 10);
        assert_eq!(view.get_int(0).unwrap(), 42);
        assert_eq!(view.get_int(4).unwrap(), 43);
    }

    #[test]
    fn test_byte_order_big_endian() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::BigEndian);

        buffer.put_int(0, 0x01020304).unwrap();

        // Big endian: most significant byte first
        assert_eq!(buffer.get_byte(0).unwrap(), 0x01);
        assert_eq!(buffer.get_byte(1).unwrap(), 0x02);
        assert_eq!(buffer.get_byte(2).unwrap(), 0x03);
        assert_eq!(buffer.get_byte(3).unwrap(), 0x04);
    }

    #[test]
    fn test_byte_order_little_endian() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::LittleEndian);

        buffer.put_int(0, 0x01020304).unwrap();

        // Little endian: least significant byte first
        assert_eq!(buffer.get_byte(0).unwrap(), 0x04);
        assert_eq!(buffer.get_byte(1).unwrap(), 0x03);
        assert_eq!(buffer.get_byte(2).unwrap(), 0x02);
        assert_eq!(buffer.get_byte(3).unwrap(), 0x01);
    }

    #[test]
    fn test_batch_reader() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        // Write some ints
        buffer.put_int(0, 1).unwrap();
        buffer.put_int(4, 2).unwrap();
        buffer.put_int(8, 3).unwrap();

        let mut reader = BatchReader::new(&buffer, 0);
        let mut ints = [0i32; 3];
        reader.read_ints(&mut ints).unwrap();

        assert_eq!(ints, [1, 2, 3]);
        assert_eq!(reader.offset(), 12);
    }

    #[test]
    fn test_batch_writer() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        {
            let mut writer = BatchWriter::new(&mut buffer, 0);
            writer.write_ints(&[10, 20, 30]).unwrap();
            assert_eq!(writer.offset(), 12);
        }

        assert_eq!(buffer.get_int(0).unwrap(), 10);
        assert_eq!(buffer.get_int(4).unwrap(), 20);
        assert_eq!(buffer.get_int(8).unwrap(), 30);
    }

    #[test]
    fn test_buffer_close() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        buffer.put_int(0, 42).unwrap();
        buffer.close().unwrap();

        // Operations should fail after close
        assert!(buffer.get_int(0).is_err());
        assert!(buffer.put_int(0, 43).is_err());
    }

    #[test]
    fn test_short_operations() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        buffer.put_short(0, 12345).unwrap();
        assert_eq!(buffer.get_short(0).unwrap(), 12345);

        buffer.put_short(2, -12345).unwrap();
        assert_eq!(buffer.get_short(2).unwrap(), -12345);
    }

    #[test]
    fn test_char_operations() {
        let mut buffer = VecDataBuffer::new(100, ByteOrderType::Native);

        buffer.put_char(0, 'A').unwrap();
        assert_eq!(buffer.get_char(0).unwrap(), 'A');
    }
}
