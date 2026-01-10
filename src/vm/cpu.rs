//! CPU state: registers and program counter.

/// RISC-V CPU state.
///
/// Contains 32 general-purpose 32-bit registers and the program counter.
/// Register x0 is hardwired to zero per the RISC-V specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    /// General-purpose registers x0-x31.
    /// x0 is hardwired to zero (writes ignored, reads return 0).
    x: [u32; 32],

    /// Program counter - address of current instruction.
    pub pc: u32,
}

impl Cpu {
    /// Create a new CPU with all registers zeroed.
    #[must_use]
    pub fn new() -> Self {
        Cpu {
            x: [0u32; 32],
            pc: 0,
        }
    }

    /// Create a new CPU with a specific entry point.
    #[must_use]
    pub fn with_pc(pc: u32) -> Self {
        Cpu { x: [0u32; 32], pc }
    }

    /// Read a register value. x0 always returns 0.
    #[inline]
    #[must_use]
    pub fn read_reg(&self, reg: u8) -> u32 {
        if reg == 0 { 0 } else { self.x[reg as usize] }
    }

    /// Write a register value. Writes to x0 are ignored.
    #[inline]
    pub fn write_reg(&mut self, reg: u8, value: u32) {
        if reg != 0 {
            self.x[reg as usize] = value;
        }
    }

    /// Get a reference to the register file (for testing/debugging).
    #[must_use]
    pub fn registers(&self) -> &[u32; 32] {
        &self.x
    }

    /// Set the entire register file (for testing/differential comparison).
    pub fn set_registers(&mut self, regs: [u32; 32]) {
        self.x = regs;
        // Enforce x0 = 0 invariant
        self.x[0] = 0;
    }

    /// Get a mutable pointer to the register file for JIT access.
    ///
    /// # Safety
    ///
    /// The returned pointer is only valid while this CPU is live and not moved.
    /// The JIT is responsible for ensuring x0 remains zero.
    #[inline]
    pub fn regs_mut_ptr(&mut self) -> *mut u32 {
        self.x.as_mut_ptr()
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x0_hardwired_zero() {
        let mut cpu = Cpu::new();

        // Writes to x0 should be ignored
        cpu.write_reg(0, 0xDEAD_BEEF);
        assert_eq!(cpu.read_reg(0), 0);

        // Other registers should work normally
        cpu.write_reg(1, 42);
        assert_eq!(cpu.read_reg(1), 42);
    }

    #[test]
    fn test_all_registers() {
        let mut cpu = Cpu::new();

        for i in 1..32u8 {
            cpu.write_reg(i, u32::from(i) * 100);
        }

        assert_eq!(cpu.read_reg(0), 0); // x0 still zero
        for i in 1..32u8 {
            assert_eq!(cpu.read_reg(i), u32::from(i) * 100);
        }
    }

    #[test]
    fn test_set_registers_enforces_x0() {
        let mut cpu = Cpu::new();
        let mut regs = [0xFFFF_FFFFu32; 32];
        regs[0] = 0xDEAD_BEEF; // Try to set x0

        cpu.set_registers(regs);

        // x0 should still be 0
        assert_eq!(cpu.read_reg(0), 0);
        // Other registers should have their values
        assert_eq!(cpu.read_reg(1), 0xFFFF_FFFF);
    }
}
