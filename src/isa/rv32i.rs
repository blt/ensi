//! RV32I base instruction execution.
//!
//! The cast warnings below are intentionally allowed because RISC-V semantics
//! require deliberate signed/unsigned reinterpretation of 32-bit values.

#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::bool_to_int_with_if)]
#![allow(clippy::if_not_else)]

use crate::error::{TrapCause, VmResult};
use crate::isa::Instruction;
use crate::vm::cpu::Cpu;
use crate::vm::memory::Memory;

/// Execute an RV32I instruction.
///
/// Returns the next PC value on success, or a trap cause on failure.
/// The caller is responsible for advancing the PC.
///
/// # Errors
///
/// Returns a [`TrapCause`] if the instruction causes a trap (ecall, ebreak,
/// memory fault, or invalid instruction).
#[inline]
#[allow(clippy::too_many_lines)]
pub(crate) fn execute_rv32i(
    inst: Instruction,
    cpu: &mut Cpu,
    memory: &mut Memory,
    pc: u32,
) -> VmResult<u32> {
    let next_pc = pc.wrapping_add(4);

    match inst {
        // ==================== Arithmetic ====================
        Instruction::Add { rd, rs1, rs2 } => {
            let result = cpu.read_reg(rs1).wrapping_add(cpu.read_reg(rs2));
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }
        Instruction::Sub { rd, rs1, rs2 } => {
            let result = cpu.read_reg(rs1).wrapping_sub(cpu.read_reg(rs2));
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }
        Instruction::Addi { rd, rs1, imm } => {
            let result = cpu.read_reg(rs1).wrapping_add(imm as u32);
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // ==================== Logical ====================
        Instruction::And { rd, rs1, rs2 } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) & cpu.read_reg(rs2));
            Ok(next_pc)
        }
        Instruction::Or { rd, rs1, rs2 } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) | cpu.read_reg(rs2));
            Ok(next_pc)
        }
        Instruction::Xor { rd, rs1, rs2 } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) ^ cpu.read_reg(rs2));
            Ok(next_pc)
        }
        Instruction::Andi { rd, rs1, imm } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) & (imm as u32));
            Ok(next_pc)
        }
        Instruction::Ori { rd, rs1, imm } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) | (imm as u32));
            Ok(next_pc)
        }
        Instruction::Xori { rd, rs1, imm } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) ^ (imm as u32));
            Ok(next_pc)
        }

        // ==================== Shifts ====================
        Instruction::Sll { rd, rs1, rs2 } => {
            let shamt = cpu.read_reg(rs2) & 0x1F;
            cpu.write_reg(rd, cpu.read_reg(rs1) << shamt);
            Ok(next_pc)
        }
        Instruction::Srl { rd, rs1, rs2 } => {
            let shamt = cpu.read_reg(rs2) & 0x1F;
            cpu.write_reg(rd, cpu.read_reg(rs1) >> shamt);
            Ok(next_pc)
        }
        Instruction::Sra { rd, rs1, rs2 } => {
            let shamt = cpu.read_reg(rs2) & 0x1F;
            let result = ((cpu.read_reg(rs1) as i32) >> shamt) as u32;
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }
        Instruction::Slli { rd, rs1, shamt } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) << shamt);
            Ok(next_pc)
        }
        Instruction::Srli { rd, rs1, shamt } => {
            cpu.write_reg(rd, cpu.read_reg(rs1) >> shamt);
            Ok(next_pc)
        }
        Instruction::Srai { rd, rs1, shamt } => {
            let result = ((cpu.read_reg(rs1) as i32) >> shamt) as u32;
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // ==================== Comparisons ====================
        Instruction::Slt { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1) as i32;
            let b = cpu.read_reg(rs2) as i32;
            cpu.write_reg(rd, if a < b { 1 } else { 0 });
            Ok(next_pc)
        }
        Instruction::Sltu { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1);
            let b = cpu.read_reg(rs2);
            cpu.write_reg(rd, if a < b { 1 } else { 0 });
            Ok(next_pc)
        }
        Instruction::Slti { rd, rs1, imm } => {
            let a = cpu.read_reg(rs1) as i32;
            cpu.write_reg(rd, if a < imm { 1 } else { 0 });
            Ok(next_pc)
        }
        Instruction::Sltiu { rd, rs1, imm } => {
            let a = cpu.read_reg(rs1);
            cpu.write_reg(rd, if a < (imm as u32) { 1 } else { 0 });
            Ok(next_pc)
        }

        // ==================== Loads ====================
        Instruction::Lw { rd, rs1, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = memory.load_u32(addr)?;
            cpu.write_reg(rd, value);
            Ok(next_pc)
        }
        Instruction::Lh { rd, rs1, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = memory.load_u16(addr)?;
            // Sign-extend from 16 bits
            cpu.write_reg(rd, (value as i16) as i32 as u32);
            Ok(next_pc)
        }
        Instruction::Lhu { rd, rs1, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = memory.load_u16(addr)?;
            // Zero-extend
            cpu.write_reg(rd, u32::from(value));
            Ok(next_pc)
        }
        Instruction::Lb { rd, rs1, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = memory.load_u8(addr)?;
            // Sign-extend from 8 bits
            cpu.write_reg(rd, (value as i8) as i32 as u32);
            Ok(next_pc)
        }
        Instruction::Lbu { rd, rs1, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = memory.load_u8(addr)?;
            // Zero-extend
            cpu.write_reg(rd, u32::from(value));
            Ok(next_pc)
        }

        // ==================== Stores ====================
        Instruction::Sw { rs1, rs2, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = cpu.read_reg(rs2);
            memory.store_u32(addr, value)?;
            Ok(next_pc)
        }
        Instruction::Sh { rs1, rs2, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = cpu.read_reg(rs2) as u16;
            memory.store_u16(addr, value)?;
            Ok(next_pc)
        }
        Instruction::Sb { rs1, rs2, imm } => {
            let addr = cpu.read_reg(rs1).wrapping_add(imm as u32);
            let value = cpu.read_reg(rs2) as u8;
            memory.store_u8(addr, value)?;
            Ok(next_pc)
        }

        // ==================== Branches ====================
        Instruction::Beq { rs1, rs2, imm } => {
            if cpu.read_reg(rs1) == cpu.read_reg(rs2) {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }
        Instruction::Bne { rs1, rs2, imm } => {
            if cpu.read_reg(rs1) != cpu.read_reg(rs2) {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }
        Instruction::Blt { rs1, rs2, imm } => {
            let a = cpu.read_reg(rs1) as i32;
            let b = cpu.read_reg(rs2) as i32;
            if a < b {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }
        Instruction::Bge { rs1, rs2, imm } => {
            let a = cpu.read_reg(rs1) as i32;
            let b = cpu.read_reg(rs2) as i32;
            if a >= b {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }
        Instruction::Bltu { rs1, rs2, imm } => {
            if cpu.read_reg(rs1) < cpu.read_reg(rs2) {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }
        Instruction::Bgeu { rs1, rs2, imm } => {
            if cpu.read_reg(rs1) >= cpu.read_reg(rs2) {
                Ok(pc.wrapping_add(imm as u32))
            } else {
                Ok(next_pc)
            }
        }

        // ==================== Jumps ====================
        Instruction::Jal { rd, imm } => {
            cpu.write_reg(rd, next_pc);
            Ok(pc.wrapping_add(imm as u32))
        }
        Instruction::Jalr { rd, rs1, imm } => {
            let target = cpu.read_reg(rs1).wrapping_add(imm as u32) & !1;
            cpu.write_reg(rd, next_pc);
            Ok(target)
        }

        // ==================== Upper Immediate ====================
        Instruction::Lui { rd, imm } => {
            cpu.write_reg(rd, imm as u32);
            Ok(next_pc)
        }
        Instruction::Auipc { rd, imm } => {
            cpu.write_reg(rd, pc.wrapping_add(imm as u32));
            Ok(next_pc)
        }

        // ==================== System ====================
        Instruction::Ecall => Err(TrapCause::Ecall),
        Instruction::Ebreak => Err(TrapCause::Ebreak),
        Instruction::Fence => Ok(next_pc), // No-op for single-core

        // M extension handled elsewhere
        _ => Err(TrapCause::InvalidInstruction(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cpu() -> Cpu {
        Cpu::new()
    }

    fn make_memory() -> Memory {
        Memory::new(1024, 0)
    }

    #[test]
    fn test_add() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 100);
        cpu.write_reg(2, 42);

        let next = execute_rv32i(
            Instruction::Add {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 142);
        assert_eq!(next, 4);
    }

    #[test]
    fn test_add_overflow() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, u32::MAX);
        cpu.write_reg(2, 1);

        execute_rv32i(
            Instruction::Add {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 0); // Wraps around
    }

    #[test]
    fn test_sub() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 100);
        cpu.write_reg(2, 42);

        execute_rv32i(
            Instruction::Sub {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 58);
    }

    #[test]
    fn test_sra_sign_extension() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 0x8000_0000); // Negative number
        cpu.write_reg(2, 4);

        execute_rv32i(
            Instruction::Sra {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 0xF800_0000); // Sign-extended
    }

    #[test]
    fn test_srl_no_sign_extension() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 0x8000_0000);
        cpu.write_reg(2, 4);

        execute_rv32i(
            Instruction::Srl {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 0x0800_0000); // Zero-extended
    }

    #[test]
    fn test_beq_taken() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 42);

        let next = execute_rv32i(
            Instruction::Beq {
                rs1: 1,
                rs2: 2,
                imm: 100,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(next, 100);
    }

    #[test]
    fn test_beq_not_taken() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 43);

        let next = execute_rv32i(
            Instruction::Beq {
                rs1: 1,
                rs2: 2,
                imm: 100,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(next, 4); // Not taken, advance by 4
    }

    #[test]
    fn test_jal() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();

        let next =
            execute_rv32i(Instruction::Jal { rd: 1, imm: 100 }, &mut cpu, &mut mem, 0).unwrap();

        assert_eq!(cpu.read_reg(1), 4); // Return address
        assert_eq!(next, 100); // Jump target
    }

    #[test]
    fn test_jalr_clears_low_bit() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 101); // Odd address

        let next = execute_rv32i(
            Instruction::Jalr {
                rd: 2,
                rs1: 1,
                imm: 0,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(next, 100); // Low bit cleared
    }

    #[test]
    fn test_lui() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();

        execute_rv32i(
            Instruction::Lui {
                rd: 1,
                imm: 0x12345000_u32 as i32,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(1), 0x12345000);
    }

    #[test]
    fn test_load_store_word() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        cpu.write_reg(1, 0); // Base address
        cpu.write_reg(2, 0xDEAD_BEEF);

        // Store
        execute_rv32i(
            Instruction::Sw {
                rs1: 1,
                rs2: 2,
                imm: 100,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        // Load
        execute_rv32i(
            Instruction::Lw {
                rd: 3,
                rs1: 1,
                imm: 100,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 0xDEAD_BEEF);
    }

    #[test]
    fn test_lb_sign_extension() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        mem.store_u8(0, 0xFF).unwrap(); // -1 as signed byte

        execute_rv32i(
            Instruction::Lb {
                rd: 1,
                rs1: 0,
                imm: 0,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(1), 0xFFFF_FFFF); // Sign-extended to -1
    }

    #[test]
    fn test_lbu_zero_extension() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();
        mem.store_u8(0, 0xFF).unwrap();

        execute_rv32i(
            Instruction::Lbu {
                rd: 1,
                rs1: 0,
                imm: 0,
            },
            &mut cpu,
            &mut mem,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(1), 0xFF); // Zero-extended
    }

    #[test]
    fn test_ecall_traps() {
        let mut cpu = make_cpu();
        let mut mem = make_memory();

        let result = execute_rv32i(Instruction::Ecall, &mut cpu, &mut mem, 0);

        assert!(matches!(result, Err(TrapCause::Ecall)));
    }
}
