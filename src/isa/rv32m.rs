//! RV32M extension: multiply and divide instructions.
//!
//! The cast warnings below are intentionally allowed because RISC-V semantics
//! require deliberate signed/unsigned reinterpretation of 32-bit values.

#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]

use crate::error::{TrapCause, VmResult};
use crate::isa::Instruction;
use crate::vm::cpu::Cpu;

/// Execute an RV32M instruction.
///
/// Returns the next PC value on success.
/// Division by zero and overflow are handled per RISC-V specification.
///
/// # Errors
///
/// Returns [`TrapCause::InvalidInstruction`] if the instruction is not an M extension opcode.
#[inline]
pub fn execute_rv32m(inst: Instruction, cpu: &mut Cpu, pc: u32) -> VmResult<u32> {
    let next_pc = pc.wrapping_add(4);

    match inst {
        // ==================== Multiplication ====================

        // MUL: Lower 32 bits of signed x signed
        Instruction::Mul { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1);
            let b = cpu.read_reg(rs2);
            let result = a.wrapping_mul(b);
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // MULH: Upper 32 bits of signed x signed
        Instruction::Mulh { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1) as i32 as i64;
            let b = cpu.read_reg(rs2) as i32 as i64;
            let result = ((a * b) >> 32) as u32;
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // MULHU: Upper 32 bits of unsigned x unsigned
        Instruction::Mulhu { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1) as u64;
            let b = cpu.read_reg(rs2) as u64;
            let result = ((a * b) >> 32) as u32;
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // MULHSU: Upper 32 bits of signed x unsigned
        Instruction::Mulhsu { rd, rs1, rs2 } => {
            let a = cpu.read_reg(rs1) as i32 as i64;
            let b = cpu.read_reg(rs2) as u64 as i64;
            let result = ((a * b) >> 32) as u32;
            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // ==================== Division ====================
        // Per RISC-V spec:
        // - Division by zero: quotient = all 1s (-1/MAX), remainder = dividend
        // - Overflow (MIN / -1): quotient = MIN, remainder = 0

        // DIV: Signed division
        Instruction::Div { rd, rs1, rs2 } => {
            let dividend = cpu.read_reg(rs1) as i32;
            let divisor = cpu.read_reg(rs2) as i32;

            let result = if divisor == 0 {
                // Division by zero: return -1 (all 1s)
                u32::MAX
            } else if dividend == i32::MIN && divisor == -1 {
                // Overflow: return dividend (MIN)
                dividend as u32
            } else {
                (dividend / divisor) as u32
            };

            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // DIVU: Unsigned division
        Instruction::Divu { rd, rs1, rs2 } => {
            let dividend = cpu.read_reg(rs1);
            let divisor = cpu.read_reg(rs2);

            let result = if divisor == 0 {
                // Division by zero: return MAX (all 1s)
                u32::MAX
            } else {
                dividend / divisor
            };

            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // REM: Signed remainder
        Instruction::Rem { rd, rs1, rs2 } => {
            let dividend = cpu.read_reg(rs1) as i32;
            let divisor = cpu.read_reg(rs2) as i32;

            let result = if divisor == 0 {
                // Division by zero: return dividend
                dividend as u32
            } else if dividend == i32::MIN && divisor == -1 {
                // Overflow: return 0
                0
            } else {
                (dividend % divisor) as u32
            };

            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        // REMU: Unsigned remainder
        Instruction::Remu { rd, rs1, rs2 } => {
            let dividend = cpu.read_reg(rs1);
            let divisor = cpu.read_reg(rs2);

            let result = if divisor == 0 {
                // Division by zero: return dividend
                dividend
            } else {
                dividend % divisor
            };

            cpu.write_reg(rd, result);
            Ok(next_pc)
        }

        _ => Err(TrapCause::InvalidInstruction(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cpu() -> Cpu {
        Cpu::new()
    }

    #[test]
    fn test_mul() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 7);
        cpu.write_reg(2, 6);

        execute_rv32m(
            Instruction::Mul {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 42);
    }

    #[test]
    fn test_mul_overflow() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 0x8000_0000);
        cpu.write_reg(2, 2);

        execute_rv32m(
            Instruction::Mul {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Lower 32 bits of 0x100000000
        assert_eq!(cpu.read_reg(3), 0);
    }

    #[test]
    fn test_mulh_positive() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 0x7FFF_FFFF); // Max positive
        cpu.write_reg(2, 2);

        execute_rv32m(
            Instruction::Mulh {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Upper 32 bits of 0x7FFFFFFF * 2 = 0xFFFFFFFE
        assert_eq!(cpu.read_reg(3), 0);
    }

    #[test]
    fn test_mulh_negative() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, (-1i32) as u32); // -1
        cpu.write_reg(2, (-1i32) as u32); // -1

        execute_rv32m(
            Instruction::Mulh {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // -1 * -1 = 1, upper bits = 0
        assert_eq!(cpu.read_reg(3), 0);
    }

    #[test]
    fn test_mulhu() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 0xFFFF_FFFF);
        cpu.write_reg(2, 0xFFFF_FFFF);

        execute_rv32m(
            Instruction::Mulhu {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // 0xFFFFFFFF * 0xFFFFFFFF = 0xFFFFFFFE00000001
        // Upper 32 bits = 0xFFFFFFFE
        assert_eq!(cpu.read_reg(3), 0xFFFF_FFFE);
    }

    #[test]
    fn test_div() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 7);

        execute_rv32m(
            Instruction::Div {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 6);
    }

    #[test]
    fn test_div_negative() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, (-42i32) as u32);
        cpu.write_reg(2, 7);

        execute_rv32m(
            Instruction::Div {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3) as i32, -6);
    }

    #[test]
    fn test_div_by_zero() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 0);

        execute_rv32m(
            Instruction::Div {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Division by zero returns -1 (all 1s)
        assert_eq!(cpu.read_reg(3), u32::MAX);
    }

    #[test]
    fn test_divu_by_zero() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 0);

        execute_rv32m(
            Instruction::Divu {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Division by zero returns MAX
        assert_eq!(cpu.read_reg(3), u32::MAX);
    }

    #[test]
    fn test_div_overflow() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 0x8000_0000); // INT_MIN
        cpu.write_reg(2, (-1i32) as u32); // -1

        execute_rv32m(
            Instruction::Div {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Overflow: return dividend (INT_MIN)
        assert_eq!(cpu.read_reg(3), 0x8000_0000);
    }

    #[test]
    fn test_rem() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 43);
        cpu.write_reg(2, 7);

        execute_rv32m(
            Instruction::Rem {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        assert_eq!(cpu.read_reg(3), 1); // 43 % 7 = 1
    }

    #[test]
    fn test_rem_by_zero() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 0);

        execute_rv32m(
            Instruction::Rem {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Division by zero: remainder = dividend
        assert_eq!(cpu.read_reg(3), 42);
    }

    #[test]
    fn test_rem_overflow() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 0x8000_0000); // INT_MIN
        cpu.write_reg(2, (-1i32) as u32); // -1

        execute_rv32m(
            Instruction::Rem {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Overflow: remainder = 0
        assert_eq!(cpu.read_reg(3), 0);
    }

    #[test]
    fn test_remu_by_zero() {
        let mut cpu = make_cpu();
        cpu.write_reg(1, 42);
        cpu.write_reg(2, 0);

        execute_rv32m(
            Instruction::Remu {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // Division by zero: remainder = dividend
        assert_eq!(cpu.read_reg(3), 42);
    }

    #[test]
    fn test_div_rem_identity() {
        // For any a, b where b != 0: (a / b) * b + (a % b) == a
        let mut cpu = make_cpu();
        let a = 12345u32;
        let b = 67u32;

        cpu.write_reg(1, a);
        cpu.write_reg(2, b);

        // quotient
        execute_rv32m(
            Instruction::Divu {
                rd: 3,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        // remainder
        execute_rv32m(
            Instruction::Remu {
                rd: 4,
                rs1: 1,
                rs2: 2,
            },
            &mut cpu,
            0,
        )
        .unwrap();

        let q = cpu.read_reg(3);
        let r = cpu.read_reg(4);

        assert_eq!(q.wrapping_mul(b).wrapping_add(r), a);
    }
}
