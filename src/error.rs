//! Error types for the RISC-V VM.

use std::fmt;

/// Memory access type for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Read access (load instructions).
    Read,
    /// Write access (store instructions).
    Write,
    /// Execute access (instruction fetch).
    Execute,
}

/// Trap causes that halt or redirect VM execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapCause {
    /// Environment call (ecall instruction).
    Ecall,
    /// Breakpoint (ebreak instruction).
    Ebreak,
    /// Invalid or unimplemented instruction.
    InvalidInstruction(u32),
    /// Memory access violation.
    MemoryFault {
        /// The address that caused the fault.
        addr: u32,
        /// The type of access attempted.
        access: AccessType,
    },
    /// Instruction address misaligned.
    InstructionMisaligned(u32),
}

impl fmt::Display for TrapCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrapCause::Ecall => write!(f, "environment call"),
            TrapCause::Ebreak => write!(f, "breakpoint"),
            TrapCause::InvalidInstruction(word) => {
                write!(f, "invalid instruction: {word:#010x}")
            }
            TrapCause::MemoryFault { addr, access } => {
                write!(f, "memory {access:?} fault at {addr:#010x}")
            }
            TrapCause::InstructionMisaligned(addr) => {
                write!(f, "instruction address misaligned: {addr:#010x}")
            }
        }
    }
}

impl std::error::Error for TrapCause {}

/// Result type for VM execution steps.
pub type VmResult<T> = Result<T, TrapCause>;
