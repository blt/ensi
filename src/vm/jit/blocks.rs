//! Basic block analysis for JIT compilation.
//!
//! A basic block is a sequence of instructions with:
//! - Single entry point (the first instruction)
//! - Single exit point (branch, jump, syscall, or fall-through)
//! - No branches into the middle of the block

use crate::isa::{decode, Instruction};
use crate::vm::Memory;
use crate::MeteringConfig;

/// Maximum instructions per basic block.
const MAX_BLOCK_SIZE: usize = 64;

/// A decoded instruction with its address and cost.
#[derive(Debug, Clone, Copy)]
pub struct DecodedInst {
    /// Program counter of this instruction.
    pub pc: u32,
    /// The decoded instruction.
    pub inst: Instruction,
    /// Cost of this instruction.
    pub cost: u32,
}

/// How a basic block terminates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// Unconditional jump (JAL, JALR).
    Jump,
    /// Conditional branch (BEQ, BNE, etc.).
    Branch,
    /// System call (ECALL).
    Syscall,
    /// Breakpoint (EBREAK).
    Break,
    /// Fall through to next instruction (block size limit reached).
    FallThrough,
    /// Invalid instruction encountered.
    Invalid,
}

/// A basic block of RISC-V instructions.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Starting PC of this block.
    pub start_pc: u32,
    /// Instructions in this block.
    pub instructions: Vec<DecodedInst>,
    /// How this block terminates.
    pub terminator: Terminator,
    /// Total cost of all instructions.
    pub total_cost: u32,
    /// PC after the last instruction (for fall-through).
    pub end_pc: u32,
}

impl BasicBlock {
    /// Get the number of instructions in this block.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Check if this block is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

/// Analyze a basic block starting at the given PC.
///
/// Returns `None` if the block cannot be analyzed (invalid PC, unaligned, etc.).
pub(crate) fn analyze_block(start_pc: u32, memory: &Memory, metering: &MeteringConfig) -> Option<BasicBlock> {
    // Check alignment
    if !start_pc.is_multiple_of(4) {
        return None;
    }

    let mut instructions = Vec::with_capacity(MAX_BLOCK_SIZE);
    let mut total_cost = 0u32;
    let mut pc = start_pc;
    let mut terminator = Terminator::FallThrough;

    for _ in 0..MAX_BLOCK_SIZE {
        // Fetch instruction word
        let word = memory.fetch(pc).ok()?;

        // Decode instruction
        let inst = match decode(word) {
            Ok(i) => i,
            Err(_) => {
                terminator = Terminator::Invalid;
                break;
            }
        };

        // Calculate cost
        let cost = metering.cost(&inst);
        total_cost = total_cost.saturating_add(cost);

        // Add to block
        instructions.push(DecodedInst { pc, inst, cost });

        // Check if this instruction terminates the block
        match inst {
            // Unconditional jumps
            Instruction::Jal { .. } | Instruction::Jalr { .. } => {
                terminator = Terminator::Jump;
                pc = pc.wrapping_add(4);
                break;
            }

            // Conditional branches
            Instruction::Beq { .. }
            | Instruction::Bne { .. }
            | Instruction::Blt { .. }
            | Instruction::Bge { .. }
            | Instruction::Bltu { .. }
            | Instruction::Bgeu { .. } => {
                terminator = Terminator::Branch;
                pc = pc.wrapping_add(4);
                break;
            }

            // System calls
            Instruction::Ecall => {
                terminator = Terminator::Syscall;
                pc = pc.wrapping_add(4);
                break;
            }

            // Breakpoints
            Instruction::Ebreak => {
                terminator = Terminator::Break;
                pc = pc.wrapping_add(4);
                break;
            }

            // Everything else continues the block
            _ => {
                pc = pc.wrapping_add(4);
            }
        }
    }

    if instructions.is_empty() {
        return None;
    }

    Some(BasicBlock {
        start_pc,
        instructions,
        terminator,
        total_cost,
        end_pc: pc,
    })
}

/// Check if an instruction is a block terminator.
#[allow(dead_code)]
fn is_terminator(inst: &Instruction) -> bool {
    matches!(
        inst,
        Instruction::Jal { .. }
            | Instruction::Jalr { .. }
            | Instruction::Beq { .. }
            | Instruction::Bne { .. }
            | Instruction::Blt { .. }
            | Instruction::Bge { .. }
            | Instruction::Bltu { .. }
            | Instruction::Bgeu { .. }
            | Instruction::Ecall
            | Instruction::Ebreak
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_empty_memory() {
        let memory = Memory::new(1024, 0);
        let metering = MeteringConfig::default();

        // Memory is zero-initialized, which decodes as an invalid instruction (opcode 0).
        // When the first instruction is invalid, analyze_block returns None.
        let block = analyze_block(0, &memory, &metering);
        assert!(block.is_none());
    }

    #[test]
    fn test_analyze_simple_block() {
        let mut memory = Memory::new(1024, 0);
        let metering = MeteringConfig::default();

        // addi x1, x0, 42
        memory.store_u32(0, 0x02A00093).ok();
        // addi x2, x0, 10
        memory.store_u32(4, 0x00A00113).ok();
        // beq x1, x2, 8 (terminates block)
        memory.store_u32(8, 0x00208463).ok();

        let block = analyze_block(0, &memory, &metering);
        assert!(block.is_some());
        let block = block.unwrap();

        assert_eq!(block.start_pc, 0);
        assert_eq!(block.instructions.len(), 3);
        assert_eq!(block.terminator, Terminator::Branch);
    }

    #[test]
    fn test_is_terminator() {
        assert!(is_terminator(&Instruction::Jal { rd: 0, imm: 0 }));
        assert!(is_terminator(&Instruction::Beq {
            rs1: 0,
            rs2: 0,
            imm: 0
        }));
        assert!(is_terminator(&Instruction::Ecall));
        assert!(!is_terminator(&Instruction::Add {
            rd: 0,
            rs1: 0,
            rs2: 0
        }));
    }
}
