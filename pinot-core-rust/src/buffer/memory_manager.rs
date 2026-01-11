//! Memory manager for tracking and managing buffer allocations
//!
//! Provides memory tracking, limits, and allocation strategies.

use super::data_buffer::ByteOrderType;
use super::pinot_buffer::PinotBuffer;
use super::{BufferError, BufferResult, BufferType};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Memory manager configuration
#[derive(Debug, Clone)]
pub struct MemoryManagerConfig {
    /// Maximum direct memory allowed (in bytes)
    pub max_direct_memory: u64,
    /// Maximum mmap memory allowed (in bytes)
    pub max_mmap_memory: u64,
    /// Default byte order for allocations
    pub default_byte_order: ByteOrderType,
    /// Enable memory tracking
    pub enable_tracking: bool,
}

impl Default for MemoryManagerConfig {
    fn default() -> Self {
        Self {
            max_direct_memory: 1024 * 1024 * 1024, // 1GB
            max_mmap_memory: 10 * 1024 * 1024 * 1024, // 10GB
            default_byte_order: ByteOrderType::Native,
            enable_tracking: true,
        }
    }
}

/// Buffer allocation context for tracking
#[derive(Debug, Clone)]
pub struct AllocationContext {
    pub buffer_type: BufferType,
    pub size: u64,
    pub description: String,
    pub allocation_time: std::time::Instant,
}

/// Memory manager for buffer allocations
pub struct MemoryManager {
    config: MemoryManagerConfig,
    direct_memory_used: AtomicU64,
    mmap_memory_used: AtomicU64,
    allocation_count: AtomicUsize,
    allocations: Mutex<HashMap<usize, AllocationContext>>,
    next_id: AtomicUsize,
}

impl MemoryManager {
    /// Creates a new memory manager with default configuration
    pub fn new() -> Arc<Self> {
        Self::with_config(MemoryManagerConfig::default())
    }

    /// Creates a new memory manager with custom configuration
    pub fn with_config(config: MemoryManagerConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            direct_memory_used: AtomicU64::new(0),
            mmap_memory_used: AtomicU64::new(0),
            allocation_count: AtomicUsize::new(0),
            allocations: Mutex::new(HashMap::new()),
            next_id: AtomicUsize::new(0),
        })
    }

    /// Allocates a direct buffer
    pub fn allocate_direct(
        self: &Arc<Self>,
        size: u64,
        description: &str,
    ) -> BufferResult<ManagedBuffer> {
        // Check memory limit
        let current = self.direct_memory_used.load(Ordering::SeqCst);
        if current + size > self.config.max_direct_memory {
            return Err(BufferError::AllocationFailed(format!(
                "Direct memory limit exceeded: {} + {} > {}",
                current, size, self.config.max_direct_memory
            )));
        }

        let buffer = PinotBuffer::allocate_direct(
            size,
            self.config.default_byte_order,
            Some(description),
        )?;

        self.direct_memory_used.fetch_add(size, Ordering::SeqCst);

        let id = self.register_allocation(BufferType::Direct, size, description);

        Ok(ManagedBuffer {
            id,
            buffer,
            manager: Arc::clone(self),
        })
    }

    /// Allocates a memory-mapped buffer
    pub fn map_file(
        self: &Arc<Self>,
        path: &std::path::Path,
        offset: u64,
        size: u64,
        read_only: bool,
        description: &str,
    ) -> BufferResult<ManagedBuffer> {
        // Check memory limit
        let current = self.mmap_memory_used.load(Ordering::SeqCst);
        if current + size > self.config.max_mmap_memory {
            return Err(BufferError::AllocationFailed(format!(
                "Mmap memory limit exceeded: {} + {} > {}",
                current, size, self.config.max_mmap_memory
            )));
        }

        let buffer = if read_only {
            PinotBuffer::map_file_read_only(
                path,
                offset,
                size,
                self.config.default_byte_order,
                Some(description),
            )?
        } else {
            PinotBuffer::map_file_read_write(
                path,
                offset,
                size,
                self.config.default_byte_order,
                Some(description),
            )?
        };

        self.mmap_memory_used.fetch_add(size, Ordering::SeqCst);

        let id = self.register_allocation(BufferType::Mmap, size, description);

        Ok(ManagedBuffer {
            id,
            buffer,
            manager: Arc::clone(self),
        })
    }

    fn register_allocation(
        &self,
        buffer_type: BufferType,
        size: u64,
        description: &str,
    ) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.allocation_count.fetch_add(1, Ordering::SeqCst);

        if self.config.enable_tracking {
            let mut allocations = self.allocations.lock();
            allocations.insert(
                id,
                AllocationContext {
                    buffer_type,
                    size,
                    description: description.to_string(),
                    allocation_time: std::time::Instant::now(),
                },
            );
        }

        id
    }

    fn release_allocation(&self, id: usize) {
        if self.config.enable_tracking {
            let mut allocations = self.allocations.lock();
            if let Some(ctx) = allocations.remove(&id) {
                match ctx.buffer_type {
                    BufferType::Direct => {
                        self.direct_memory_used.fetch_sub(ctx.size, Ordering::SeqCst);
                    }
                    BufferType::Mmap => {
                        self.mmap_memory_used.fetch_sub(ctx.size, Ordering::SeqCst);
                    }
                }
            }
        }

        self.allocation_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Returns the current direct memory usage
    pub fn direct_memory_used(&self) -> u64 {
        self.direct_memory_used.load(Ordering::SeqCst)
    }

    /// Returns the current mmap memory usage
    pub fn mmap_memory_used(&self) -> u64 {
        self.mmap_memory_used.load(Ordering::SeqCst)
    }

    /// Returns the total memory usage
    pub fn total_memory_used(&self) -> u64 {
        self.direct_memory_used() + self.mmap_memory_used()
    }

    /// Returns the number of active allocations
    pub fn allocation_count(&self) -> usize {
        self.allocation_count.load(Ordering::SeqCst)
    }

    /// Returns the configuration
    pub fn config(&self) -> &MemoryManagerConfig {
        &self.config
    }

    /// Returns allocation details (for debugging)
    pub fn get_allocation_details(&self) -> Vec<AllocationContext> {
        let allocations = self.allocations.lock();
        allocations.values().cloned().collect()
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self {
            config: MemoryManagerConfig::default(),
            direct_memory_used: AtomicU64::new(0),
            mmap_memory_used: AtomicU64::new(0),
            allocation_count: AtomicUsize::new(0),
            allocations: Mutex::new(HashMap::new()),
            next_id: AtomicUsize::new(0),
        }
    }
}

/// A buffer managed by a MemoryManager
pub struct ManagedBuffer {
    id: usize,
    buffer: PinotBuffer,
    manager: Arc<MemoryManager>,
}

impl ManagedBuffer {
    /// Returns the underlying buffer
    pub fn buffer(&self) -> &PinotBuffer {
        &self.buffer
    }

    /// Returns a mutable reference to the underlying buffer
    pub fn buffer_mut(&mut self) -> &mut PinotBuffer {
        &mut self.buffer
    }

    /// Returns the buffer size
    pub fn size(&self) -> u64 {
        self.buffer.size()
    }

    /// Returns the buffer type
    pub fn buffer_type(&self) -> BufferType {
        self.buffer.buffer_type()
    }
}

impl Drop for ManagedBuffer {
    fn drop(&mut self) {
        self.manager.release_allocation(self.id);
    }
}

impl std::ops::Deref for ManagedBuffer {
    type Target = PinotBuffer;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl std::ops::DerefMut for ManagedBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_manager_allocation() {
        let manager = MemoryManager::new();

        let buf1 = manager.allocate_direct(1024, "test1").unwrap();
        assert_eq!(buf1.size(), 1024);
        assert_eq!(manager.direct_memory_used(), 1024);
        assert_eq!(manager.allocation_count(), 1);

        let buf2 = manager.allocate_direct(2048, "test2").unwrap();
        assert_eq!(manager.direct_memory_used(), 3072);
        assert_eq!(manager.allocation_count(), 2);

        drop(buf1);
        assert_eq!(manager.direct_memory_used(), 2048);
        assert_eq!(manager.allocation_count(), 1);

        drop(buf2);
        assert_eq!(manager.direct_memory_used(), 0);
        assert_eq!(manager.allocation_count(), 0);
    }

    #[test]
    fn test_memory_limit() {
        let config = MemoryManagerConfig {
            max_direct_memory: 1000,
            ..Default::default()
        };
        let manager = MemoryManager::with_config(config);

        // Should succeed
        let _buf1 = manager.allocate_direct(500, "test1").unwrap();

        // Should fail - exceeds limit
        let result = manager.allocate_direct(600, "test2");
        assert!(result.is_err());
    }

    #[test]
    fn test_allocation_tracking() {
        let manager = MemoryManager::new();

        let _buf1 = manager.allocate_direct(100, "buffer1").unwrap();
        let _buf2 = manager.allocate_direct(200, "buffer2").unwrap();

        let details = manager.get_allocation_details();
        assert_eq!(details.len(), 2);

        let descriptions: Vec<&str> = details.iter().map(|d| d.description.as_str()).collect();
        assert!(descriptions.contains(&"buffer1"));
        assert!(descriptions.contains(&"buffer2"));
    }

    #[test]
    fn test_managed_buffer_deref() {
        let manager = MemoryManager::new();
        let mut buf = manager.allocate_direct(100, "test").unwrap();

        // Test Deref
        assert_eq!(buf.size(), 100);

        // Test DerefMut
        buf.inner_mut().put_int(0, 42).unwrap();
        assert_eq!(buf.inner().get_int(0).unwrap(), 42);
    }

    #[test]
    fn test_total_memory() {
        let manager = MemoryManager::new();

        let _buf1 = manager.allocate_direct(100, "direct").unwrap();
        assert_eq!(manager.total_memory_used(), 100);
        assert_eq!(manager.direct_memory_used(), 100);
        assert_eq!(manager.mmap_memory_used(), 0);
    }
}
