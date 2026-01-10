//! Memory subsystem with load/store operations.
//!
//! The truncation warnings are allowed because this is a 32-bit VM that
//! enforces memory size limits at construction time.

#![allow(clippy::cast_possible_truncation)]

use crate::error::{AccessType, TrapCause, VmResult};

/// Memory for a VM instance.
///
/// Provides a flat address space with bounds checking.
/// All operations are little-endian per RISC-V specification.
#[derive(Debug, Clone)]
pub struct Memory {
    /// Backing storage.
    data: Vec<u8>,

    /// Base address of this memory region.
    base: u32,
}

impl Memory {
    /// Create a new memory region of the given size.
    ///
    /// Memory is zero-initialized and starts at the given base address.
    #[must_use]
    pub fn new(size: u32, base: u32) -> Self {
        Memory {
            data: vec![0u8; size as usize],
            base,
        }
    }

    /// Get the size of this memory region in bytes.
    #[must_use]
    pub fn size(&self) -> u32 {
        self.data.len() as u32
    }

    /// Get the base address of this memory region.
    #[must_use]
    pub fn base(&self) -> u32 {
        self.base
    }

    /// Check if an address range is valid for the given access type.
    #[inline]
    fn check_bounds(&self, addr: u32, len: u32, access: AccessType) -> VmResult<usize> {
        if addr < self.base {
            return Err(TrapCause::MemoryFault { addr, access });
        }

        let offset = addr.wrapping_sub(self.base);
        let end = offset.saturating_add(len);

        if end > self.data.len() as u32 {
            return Err(TrapCause::MemoryFault { addr, access });
        }

        Ok(offset as usize)
    }

    /// Load a byte (8-bit) from memory.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn load_u8(&self, addr: u32) -> VmResult<u8> {
        let offset = self.check_bounds(addr, 1, AccessType::Read)?;
        Ok(self.data[offset])
    }

    /// Load a halfword (16-bit) from memory, little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn load_u16(&self, addr: u32) -> VmResult<u16> {
        let offset = self.check_bounds(addr, 2, AccessType::Read)?;
        Ok(u16::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
        ]))
    }

    /// Load a word (32-bit) from memory, little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn load_u32(&self, addr: u32) -> VmResult<u32> {
        let offset = self.check_bounds(addr, 4, AccessType::Read)?;
        // Bounds are verified, so indexing is safe. Copy to array for from_le_bytes.
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[offset..offset + 4]);
        Ok(u32::from_le_bytes(bytes))
    }

    /// Store a byte (8-bit) to memory.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn store_u8(&mut self, addr: u32, value: u8) -> VmResult<()> {
        let offset = self.check_bounds(addr, 1, AccessType::Write)?;
        self.data[offset] = value;
        Ok(())
    }

    /// Store a halfword (16-bit) to memory, little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn store_u16(&mut self, addr: u32, value: u16) -> VmResult<()> {
        let offset = self.check_bounds(addr, 2, AccessType::Write)?;
        self.data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Store a word (32-bit) to memory, little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn store_u32(&mut self, addr: u32, value: u32) -> VmResult<()> {
        let offset = self.check_bounds(addr, 4, AccessType::Write)?;
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Fetch an instruction from memory.
    ///
    /// Identical to `load_u32` but uses [`AccessType::Execute`] for error reporting,
    /// which allows distinguishing instruction fetch faults from data load faults.
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address is out of bounds.
    #[inline]
    pub fn fetch(&self, addr: u32) -> VmResult<u32> {
        let offset = self.check_bounds(addr, 4, AccessType::Execute)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[offset..offset + 4]);
        Ok(u32::from_le_bytes(bytes))
    }

    /// Load a slice of bytes from memory (for bulk operations).
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address range is out of bounds.
    #[inline]
    pub fn load_bytes(&self, addr: u32, len: u32) -> VmResult<&[u8]> {
        let offset = self.check_bounds(addr, len, AccessType::Read)?;
        Ok(&self.data[offset..offset + len as usize])
    }

    /// Store a slice of bytes to memory (for bulk operations).
    ///
    /// # Errors
    ///
    /// Returns [`TrapCause::MemoryFault`] if the address range is out of bounds.
    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    pub fn store_bytes(&mut self, addr: u32, bytes: &[u8]) -> VmResult<()> {
        let offset = self.check_bounds(addr, bytes.len() as u32, AccessType::Write)?;
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Get a checksum of memory contents (for determinism testing).
    #[must_use]
    pub fn checksum(&self) -> u64 {
        // Simple FNV-1a hash for quick comparison
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &byte in &self.data {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    /// Reset memory to zero for reuse in pooling.
    ///
    /// This is more efficient than allocating new memory.
    /// LLVM auto-vectorizes the fill operation.
    #[inline]
    pub fn reset(&mut self) {
        self.data.fill(0);
    }

    /// Get raw access to the underlying data for bulk operations.
    ///
    /// Used internally for efficient memory pooling.
    #[inline]
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Get a mutable raw pointer to the underlying data for JIT access.
    ///
    /// # Safety
    ///
    /// The returned pointer is only valid while this Memory is live and not moved.
    #[inline]
    pub fn data_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_store_byte() {
        let mut mem = Memory::new(256, 0);

        mem.store_u8(0, 0x42).unwrap();
        assert_eq!(mem.load_u8(0).unwrap(), 0x42);

        mem.store_u8(255, 0xFF).unwrap();
        assert_eq!(mem.load_u8(255).unwrap(), 0xFF);
    }

    #[test]
    fn test_load_store_word_little_endian() {
        let mut mem = Memory::new(256, 0);

        mem.store_u32(0, 0x1234_5678).unwrap();

        // Check little-endian byte order
        assert_eq!(mem.load_u8(0).unwrap(), 0x78);
        assert_eq!(mem.load_u8(1).unwrap(), 0x56);
        assert_eq!(mem.load_u8(2).unwrap(), 0x34);
        assert_eq!(mem.load_u8(3).unwrap(), 0x12);

        assert_eq!(mem.load_u32(0).unwrap(), 0x1234_5678);
    }

    #[test]
    fn test_base_address() {
        let mut mem = Memory::new(256, 0x1000);

        // Access at base should work
        mem.store_u32(0x1000, 0xDEAD_BEEF).unwrap();
        assert_eq!(mem.load_u32(0x1000).unwrap(), 0xDEAD_BEEF);

        // Access below base should fail
        assert!(mem.load_u8(0x0FFF).is_err());
    }

    #[test]
    fn test_bounds_checking() {
        let mem = Memory::new(256, 0);

        // Valid accesses
        assert!(mem.load_u8(255).is_ok());
        assert!(mem.load_u32(252).is_ok());

        // Out of bounds
        assert!(mem.load_u8(256).is_err());
        assert!(mem.load_u32(253).is_err()); // Would read past end
    }
}
