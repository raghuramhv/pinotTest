//! PinotBuffer - High-level buffer abstraction
//!
//! Provides a unified interface for both direct memory and memory-mapped file buffers.

use super::data_buffer::{ByteOrderType, DataBuffer, VecDataBuffer};
use super::{BufferError, BufferResult, BufferType};
use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Global buffer statistics
static DIRECT_BUFFER_COUNT: AtomicUsize = AtomicUsize::new(0);
static DIRECT_BUFFER_USAGE: AtomicU64 = AtomicU64::new(0);
static MMAP_BUFFER_COUNT: AtomicUsize = AtomicUsize::new(0);
static MMAP_BUFFER_USAGE: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// PinotBuffer - Main buffer abstraction
pub struct PinotBuffer {
    inner: Box<dyn DataBuffer>,
    buffer_type: BufferType,
    size: u64,
    description: Option<String>,
}

impl PinotBuffer {
    /// Allocates a direct (heap) buffer
    pub fn allocate_direct(
        size: u64,
        byte_order: ByteOrderType,
        description: Option<&str>,
    ) -> BufferResult<Self> {
        let buffer = VecDataBuffer::new(size as usize, byte_order);

        DIRECT_BUFFER_COUNT.fetch_add(1, Ordering::SeqCst);
        DIRECT_BUFFER_USAGE.fetch_add(size, Ordering::SeqCst);

        Ok(Self {
            inner: Box::new(buffer),
            buffer_type: BufferType::Direct,
            size,
            description: description.map(String::from),
        })
    }

    /// Memory-maps a file for reading
    pub fn map_file_read_only(
        path: impl AsRef<Path>,
        offset: u64,
        size: u64,
        byte_order: ByteOrderType,
        description: Option<&str>,
    ) -> BufferResult<Self> {
        let file = File::open(path.as_ref()).map_err(BufferError::IoError)?;

        let mmap = unsafe {
            MmapOptions::new()
                .offset(offset)
                .len(size as usize)
                .map(&file)
                .map_err(|e| BufferError::MmapFailed(e.to_string()))?
        };

        MMAP_BUFFER_COUNT.fetch_add(1, Ordering::SeqCst);
        MMAP_BUFFER_USAGE.fetch_add(size, Ordering::SeqCst);

        Ok(Self {
            inner: Box::new(MmapBuffer::read_only(mmap, byte_order)),
            buffer_type: BufferType::Mmap,
            size,
            description: description.map(String::from),
        })
    }

    /// Memory-maps a file for reading and writing
    pub fn map_file_read_write(
        path: impl AsRef<Path>,
        offset: u64,
        size: u64,
        byte_order: ByteOrderType,
        description: Option<&str>,
    ) -> BufferResult<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.as_ref())
            .map_err(BufferError::IoError)?;

        // Extend file if necessary
        let file_len = file.metadata().map_err(BufferError::IoError)?.len();
        if offset + size > file_len {
            file.set_len(offset + size).map_err(BufferError::IoError)?;
        }

        let mmap = unsafe {
            MmapOptions::new()
                .offset(offset)
                .len(size as usize)
                .map_mut(&file)
                .map_err(|e| BufferError::MmapFailed(e.to_string()))?
        };

        MMAP_BUFFER_COUNT.fetch_add(1, Ordering::SeqCst);
        MMAP_BUFFER_USAGE.fetch_add(size, Ordering::SeqCst);

        Ok(Self {
            inner: Box::new(MmapMutBuffer::new(mmap, byte_order)),
            buffer_type: BufferType::Mmap,
            size,
            description: description.map(String::from),
        })
    }

    /// Loads a file into a direct buffer
    pub fn load_file(
        path: impl AsRef<Path>,
        offset: u64,
        size: u64,
        byte_order: ByteOrderType,
        description: Option<&str>,
    ) -> BufferResult<Self> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = File::open(path.as_ref()).map_err(BufferError::IoError)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(BufferError::IoError)?;

        let mut data = vec![0u8; size as usize];
        file.read_exact(&mut data).map_err(BufferError::IoError)?;

        let buffer = VecDataBuffer::from_vec(data, byte_order);

        DIRECT_BUFFER_COUNT.fetch_add(1, Ordering::SeqCst);
        DIRECT_BUFFER_USAGE.fetch_add(size, Ordering::SeqCst);

        Ok(Self {
            inner: Box::new(buffer),
            buffer_type: BufferType::Direct,
            size,
            description: description.map(String::from),
        })
    }

    /// Returns the buffer type
    pub fn buffer_type(&self) -> BufferType {
        self.buffer_type
    }

    /// Returns the buffer size
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the description
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the inner buffer
    pub fn inner(&self) -> &dyn DataBuffer {
        self.inner.as_ref()
    }

    /// Returns a mutable reference to the inner buffer
    pub fn inner_mut(&mut self) -> &mut dyn DataBuffer {
        self.inner.as_mut()
    }

    // ========== Static Methods for Statistics ==========

    pub fn direct_buffer_count() -> usize {
        DIRECT_BUFFER_COUNT.load(Ordering::SeqCst)
    }

    pub fn direct_buffer_usage() -> u64 {
        DIRECT_BUFFER_USAGE.load(Ordering::SeqCst)
    }

    pub fn mmap_buffer_count() -> usize {
        MMAP_BUFFER_COUNT.load(Ordering::SeqCst)
    }

    pub fn mmap_buffer_usage() -> u64 {
        MMAP_BUFFER_USAGE.load(Ordering::SeqCst)
    }

    pub fn allocation_failure_count() -> usize {
        ALLOCATION_FAILURE_COUNT.load(Ordering::SeqCst)
    }
}

impl Drop for PinotBuffer {
    fn drop(&mut self) {
        match self.buffer_type {
            BufferType::Direct => {
                DIRECT_BUFFER_COUNT.fetch_sub(1, Ordering::SeqCst);
                DIRECT_BUFFER_USAGE.fetch_sub(self.size, Ordering::SeqCst);
            }
            BufferType::Mmap => {
                MMAP_BUFFER_COUNT.fetch_sub(1, Ordering::SeqCst);
                MMAP_BUFFER_USAGE.fetch_sub(self.size, Ordering::SeqCst);
            }
        }
    }
}

// ========== Memory-mapped buffer implementations ==========

use byteorder::{BigEndian, ByteOrder, LittleEndian, NativeEndian};

/// Read-only memory-mapped buffer
struct MmapBuffer {
    mmap: memmap2::Mmap,
    byte_order: ByteOrderType,
}

impl MmapBuffer {
    fn read_only(mmap: memmap2::Mmap, byte_order: ByteOrderType) -> Self {
        Self { mmap, byte_order }
    }

    fn check_bounds(&self, offset: u64, size: u64) -> BufferResult<()> {
        if offset + size > self.mmap.len() as u64 {
            return Err(BufferError::OutOfBounds {
                offset,
                size,
                buffer_size: self.mmap.len() as u64,
            });
        }
        Ok(())
    }
}

impl DataBuffer for MmapBuffer {
    fn size(&self) -> u64 {
        self.mmap.len() as u64
    }

    fn byte_order(&self) -> ByteOrderType {
        self.byte_order
    }

    fn get_byte(&self, offset: u64) -> BufferResult<u8> {
        self.check_bounds(offset, 1)?;
        Ok(self.mmap[offset as usize])
    }

    fn get_char(&self, offset: u64) -> BufferResult<char> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 2];
        let code = match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_u16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_u16(bytes),
            ByteOrderType::Native => NativeEndian::read_u16(bytes),
        };
        Ok(char::from_u32(code as u32).unwrap_or('\0'))
    }

    fn get_short(&self, offset: u64) -> BufferResult<i16> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 2];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i16(bytes),
            ByteOrderType::Native => NativeEndian::read_i16(bytes),
        })
    }

    fn get_int(&self, offset: u64) -> BufferResult<i32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i32(bytes),
            ByteOrderType::Native => NativeEndian::read_i32(bytes),
        })
    }

    fn get_long(&self, offset: u64) -> BufferResult<i64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i64(bytes),
            ByteOrderType::Native => NativeEndian::read_i64(bytes),
        })
    }

    fn get_float(&self, offset: u64) -> BufferResult<f32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f32(bytes),
            ByteOrderType::Native => NativeEndian::read_f32(bytes),
        })
    }

    fn get_double(&self, offset: u64) -> BufferResult<f64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f64(bytes),
            ByteOrderType::Native => NativeEndian::read_f64(bytes),
        })
    }

    // Write operations not supported for read-only mmap
    fn put_byte(&mut self, _offset: u64, _value: u8) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_char(&mut self, _offset: u64, _value: char) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_short(&mut self, _offset: u64, _value: i16) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_int(&mut self, _offset: u64, _value: i32) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_long(&mut self, _offset: u64, _value: i64) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_float(&mut self, _offset: u64, _value: f32) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn put_double(&mut self, _offset: u64, _value: f64) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn copy_to(&self, offset: u64, dest: &mut [u8]) -> BufferResult<()> {
        self.check_bounds(offset, dest.len() as u64)?;
        dest.copy_from_slice(&self.mmap[offset as usize..offset as usize + dest.len()]);
        Ok(())
    }

    fn read_from(&mut self, _offset: u64, _src: &[u8]) -> BufferResult<()> {
        Err(BufferError::AllocationFailed(
            "Read-only buffer".to_string(),
        ))
    }

    fn view(&self, start: u64, end: u64) -> BufferResult<Box<dyn DataBuffer>> {
        self.check_bounds(start, end - start)?;
        let view_data = self.mmap[start as usize..end as usize].to_vec();
        Ok(Box::new(VecDataBuffer::from_vec(view_data, self.byte_order)))
    }

    fn as_ptr(&self) -> *const u8 {
        self.mmap.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        // Cannot get mutable pointer from read-only mmap
        std::ptr::null_mut()
    }

    fn flush(&self) -> BufferResult<()> {
        Ok(())
    }

    fn close(&mut self) -> BufferResult<()> {
        Ok(())
    }
}

/// Read-write memory-mapped buffer
struct MmapMutBuffer {
    mmap: MmapMut,
    byte_order: ByteOrderType,
}

impl MmapMutBuffer {
    fn new(mmap: MmapMut, byte_order: ByteOrderType) -> Self {
        Self { mmap, byte_order }
    }

    fn check_bounds(&self, offset: u64, size: u64) -> BufferResult<()> {
        if offset + size > self.mmap.len() as u64 {
            return Err(BufferError::OutOfBounds {
                offset,
                size,
                buffer_size: self.mmap.len() as u64,
            });
        }
        Ok(())
    }
}

impl DataBuffer for MmapMutBuffer {
    fn size(&self) -> u64 {
        self.mmap.len() as u64
    }

    fn byte_order(&self) -> ByteOrderType {
        self.byte_order
    }

    fn get_byte(&self, offset: u64) -> BufferResult<u8> {
        self.check_bounds(offset, 1)?;
        Ok(self.mmap[offset as usize])
    }

    fn get_char(&self, offset: u64) -> BufferResult<char> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 2];
        let code = match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_u16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_u16(bytes),
            ByteOrderType::Native => NativeEndian::read_u16(bytes),
        };
        Ok(char::from_u32(code as u32).unwrap_or('\0'))
    }

    fn get_short(&self, offset: u64) -> BufferResult<i16> {
        self.check_bounds(offset, 2)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 2];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i16(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i16(bytes),
            ByteOrderType::Native => NativeEndian::read_i16(bytes),
        })
    }

    fn get_int(&self, offset: u64) -> BufferResult<i32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i32(bytes),
            ByteOrderType::Native => NativeEndian::read_i32(bytes),
        })
    }

    fn get_long(&self, offset: u64) -> BufferResult<i64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_i64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_i64(bytes),
            ByteOrderType::Native => NativeEndian::read_i64(bytes),
        })
    }

    fn get_float(&self, offset: u64) -> BufferResult<f32> {
        self.check_bounds(offset, 4)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 4];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f32(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f32(bytes),
            ByteOrderType::Native => NativeEndian::read_f32(bytes),
        })
    }

    fn get_double(&self, offset: u64) -> BufferResult<f64> {
        self.check_bounds(offset, 8)?;
        let bytes = &self.mmap[offset as usize..offset as usize + 8];
        Ok(match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::read_f64(bytes),
            ByteOrderType::LittleEndian => LittleEndian::read_f64(bytes),
            ByteOrderType::Native => NativeEndian::read_f64(bytes),
        })
    }

    fn put_byte(&mut self, offset: u64, value: u8) -> BufferResult<()> {
        self.check_bounds(offset, 1)?;
        self.mmap[offset as usize] = value;
        Ok(())
    }

    fn put_char(&mut self, offset: u64, value: char) -> BufferResult<()> {
        self.check_bounds(offset, 2)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 2];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_u16(bytes, value as u16),
            ByteOrderType::LittleEndian => LittleEndian::write_u16(bytes, value as u16),
            ByteOrderType::Native => NativeEndian::write_u16(bytes, value as u16),
        }
        Ok(())
    }

    fn put_short(&mut self, offset: u64, value: i16) -> BufferResult<()> {
        self.check_bounds(offset, 2)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 2];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i16(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i16(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i16(bytes, value),
        }
        Ok(())
    }

    fn put_int(&mut self, offset: u64, value: i32) -> BufferResult<()> {
        self.check_bounds(offset, 4)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 4];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i32(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i32(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i32(bytes, value),
        }
        Ok(())
    }

    fn put_long(&mut self, offset: u64, value: i64) -> BufferResult<()> {
        self.check_bounds(offset, 8)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 8];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_i64(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_i64(bytes, value),
            ByteOrderType::Native => NativeEndian::write_i64(bytes, value),
        }
        Ok(())
    }

    fn put_float(&mut self, offset: u64, value: f32) -> BufferResult<()> {
        self.check_bounds(offset, 4)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 4];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_f32(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_f32(bytes, value),
            ByteOrderType::Native => NativeEndian::write_f32(bytes, value),
        }
        Ok(())
    }

    fn put_double(&mut self, offset: u64, value: f64) -> BufferResult<()> {
        self.check_bounds(offset, 8)?;
        let bytes = &mut self.mmap[offset as usize..offset as usize + 8];
        match self.byte_order {
            ByteOrderType::BigEndian => BigEndian::write_f64(bytes, value),
            ByteOrderType::LittleEndian => LittleEndian::write_f64(bytes, value),
            ByteOrderType::Native => NativeEndian::write_f64(bytes, value),
        }
        Ok(())
    }

    fn copy_to(&self, offset: u64, dest: &mut [u8]) -> BufferResult<()> {
        self.check_bounds(offset, dest.len() as u64)?;
        dest.copy_from_slice(&self.mmap[offset as usize..offset as usize + dest.len()]);
        Ok(())
    }

    fn read_from(&mut self, offset: u64, src: &[u8]) -> BufferResult<()> {
        self.check_bounds(offset, src.len() as u64)?;
        self.mmap[offset as usize..offset as usize + src.len()].copy_from_slice(src);
        Ok(())
    }

    fn view(&self, start: u64, end: u64) -> BufferResult<Box<dyn DataBuffer>> {
        self.check_bounds(start, end - start)?;
        let view_data = self.mmap[start as usize..end as usize].to_vec();
        Ok(Box::new(VecDataBuffer::from_vec(view_data, self.byte_order)))
    }

    fn as_ptr(&self) -> *const u8 {
        self.mmap.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.mmap.as_mut_ptr()
    }

    fn flush(&self) -> BufferResult<()> {
        self.mmap.flush().map_err(BufferError::IoError)
    }

    fn close(&mut self) -> BufferResult<()> {
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_direct_buffer_allocation() {
        let initial_count = PinotBuffer::direct_buffer_count();
        let initial_usage = PinotBuffer::direct_buffer_usage();

        let buffer =
            PinotBuffer::allocate_direct(1024, ByteOrderType::Native, Some("test")).unwrap();

        assert_eq!(buffer.buffer_type(), BufferType::Direct);
        assert_eq!(buffer.size(), 1024);
        assert_eq!(buffer.description(), Some("test"));

        assert_eq!(PinotBuffer::direct_buffer_count(), initial_count + 1);
        assert_eq!(PinotBuffer::direct_buffer_usage(), initial_usage + 1024);

        drop(buffer);

        assert_eq!(PinotBuffer::direct_buffer_count(), initial_count);
        assert_eq!(PinotBuffer::direct_buffer_usage(), initial_usage);
    }

    #[test]
    fn test_direct_buffer_operations() {
        let mut buffer =
            PinotBuffer::allocate_direct(100, ByteOrderType::Native, None).unwrap();

        buffer.inner_mut().put_int(0, 42).unwrap();
        assert_eq!(buffer.inner().get_int(0).unwrap(), 42);

        buffer.inner_mut().put_long(8, 123456789).unwrap();
        assert_eq!(buffer.inner().get_long(8).unwrap(), 123456789);
    }

    #[test]
    fn test_mmap_buffer() {
        let temp_file = NamedTempFile::new().unwrap();

        // Create read-write mmap buffer
        {
            let mut buffer = PinotBuffer::map_file_read_write(
                temp_file.path(),
                0,
                1024,
                ByteOrderType::Native,
                Some("test mmap"),
            )
            .unwrap();

            assert_eq!(buffer.buffer_type(), BufferType::Mmap);
            assert_eq!(buffer.size(), 1024);

            buffer.inner_mut().put_int(0, 12345).unwrap();
            buffer.inner_mut().put_double(8, 3.14159).unwrap();
            buffer.inner().flush().unwrap();
        }

        // Read back with read-only mmap
        {
            let buffer = PinotBuffer::map_file_read_only(
                temp_file.path(),
                0,
                1024,
                ByteOrderType::Native,
                None,
            )
            .unwrap();

            assert_eq!(buffer.inner().get_int(0).unwrap(), 12345);
            assert!((buffer.inner().get_double(8).unwrap() - 3.14159).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_load_file() {
        let temp_file = NamedTempFile::new().unwrap();

        // Write some data
        {
            let mut buffer = PinotBuffer::map_file_read_write(
                temp_file.path(),
                0,
                100,
                ByteOrderType::Native,
                None,
            )
            .unwrap();

            buffer.inner_mut().put_int(0, 999).unwrap();
            buffer.inner().flush().unwrap();
        }

        // Load file into direct buffer
        let buffer =
            PinotBuffer::load_file(temp_file.path(), 0, 100, ByteOrderType::Native, None)
                .unwrap();

        assert_eq!(buffer.buffer_type(), BufferType::Direct);
        assert_eq!(buffer.inner().get_int(0).unwrap(), 999);
    }

    #[test]
    fn test_buffer_statistics() {
        let initial_direct = PinotBuffer::direct_buffer_count();
        let initial_mmap = PinotBuffer::mmap_buffer_count();

        let b1 = PinotBuffer::allocate_direct(100, ByteOrderType::Native, None).unwrap();
        let b2 = PinotBuffer::allocate_direct(200, ByteOrderType::Native, None).unwrap();

        assert_eq!(PinotBuffer::direct_buffer_count(), initial_direct + 2);

        drop(b1);
        assert_eq!(PinotBuffer::direct_buffer_count(), initial_direct + 1);

        drop(b2);
        assert_eq!(PinotBuffer::direct_buffer_count(), initial_direct);
    }
}
