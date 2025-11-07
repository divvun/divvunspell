//! Memory access abstraction for virtual file system.
//!
//! This module provides a trait-based abstraction over different memory access patterns,
//! allowing code to work with either memory-mapped files (zero-copy) or traditional file
//! I/O (syscall-based) through a unified interface.

use std::ptr;

/// Trait for reading primitive values from memory or file storage.
///
/// This abstraction allows data structures to be generic over the memory access pattern,
/// eliminating code duplication between mmap and file-based implementations. It pairs
/// naturally with the `File` trait for opening and mapping files.
pub trait Memory {
    /// Read a little-endian u16 at the given byte offset.
    fn read_u16_at(&self, offset: usize) -> u16;

    /// Read a little-endian u32 at the given byte offset.
    fn read_u32_at(&self, offset: usize) -> u32;

    /// Read a little-endian f32 at the given byte offset.
    fn read_f32_at(&self, offset: usize) -> f32;
}

/// Memory-mapped access implementation.
///
/// Provides zero-copy access to file contents via direct pointer reads.
/// This is the fastest access method and is preferred for production use.
impl Memory for memmap2::Mmap {
    #[inline(always)]
    fn read_u16_at(&self, offset: usize) -> u16 {
        unsafe { ptr::read(self.as_ptr().add(offset) as *const u16) }
    }

    #[inline(always)]
    fn read_u32_at(&self, offset: usize) -> u32 {
        unsafe { ptr::read(self.as_ptr().add(offset) as *const u32) }
    }

    #[inline(always)]
    fn read_f32_at(&self, offset: usize) -> f32 {
        unsafe { ptr::read(self.as_ptr().add(offset) as *const f32) }
    }
}

/// File-based access implementation (Unix only).
///
/// Uses `read_exact_at` syscalls for each access. This is slower than mmap
/// but can be useful when memory mapping is not available or when working
/// with very large files that would exhaust address space.
#[cfg(unix)]
impl Memory for std::fs::File {
    #[inline(always)]
    fn read_u16_at(&self, offset: usize) -> u16 {
        use std::os::unix::fs::FileExt;
        let mut buf = [0u8; 2];
        // Note: unwrap here is intentional - file corruption should panic
        self.read_exact_at(&mut buf, offset as u64).unwrap();
        u16::from_le_bytes(buf)
    }

    #[inline(always)]
    fn read_u32_at(&self, offset: usize) -> u32 {
        use std::os::unix::fs::FileExt;
        let mut buf = [0u8; 4];
        self.read_exact_at(&mut buf, offset as u64).unwrap();
        u32::from_le_bytes(buf)
    }

    #[inline(always)]
    fn read_f32_at(&self, offset: usize) -> f32 {
        use std::os::unix::fs::FileExt;
        let mut buf = [0u8; 4];
        self.read_exact_at(&mut buf, offset as u64).unwrap();
        f32::from_le_bytes(buf)
    }
}
