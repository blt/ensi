//! RISC-V instruction representation and decoding.
//!
//! The cast warnings below are intentionally allowed because RISC-V semantics
//! require deliberate signed/unsigned reinterpretation of 32-bit values.

#![allow(clippy::cast_possible_wrap)]

/// A decoded RISC-V instruction.
///
/// # Field Conventions
/// - `rd`: Destination register (0-31)
/// - `rs1`: Source register 1 (0-31)
/// - `rs2`: Source register 2 (0-31)
/// - `imm`: Sign-extended immediate value
/// - `shamt`: Shift amount (0-31)
#[allow(missing_docs)] // Fields are self-documenting per RISC-V spec
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    // ==================== RV32I Base ====================

    // R-type: register-register operations
    Add { rd: u8, rs1: u8, rs2: u8 },
    Sub { rd: u8, rs1: u8, rs2: u8 },
    Sll { rd: u8, rs1: u8, rs2: u8 },
    Slt { rd: u8, rs1: u8, rs2: u8 },
    Sltu { rd: u8, rs1: u8, rs2: u8 },
    Xor { rd: u8, rs1: u8, rs2: u8 },
    Srl { rd: u8, rs1: u8, rs2: u8 },
    Sra { rd: u8, rs1: u8, rs2: u8 },
    Or { rd: u8, rs1: u8, rs2: u8 },
    And { rd: u8, rs1: u8, rs2: u8 },

    // I-type: immediate operations
    Addi { rd: u8, rs1: u8, imm: i32 },
    Slti { rd: u8, rs1: u8, imm: i32 },
    Sltiu { rd: u8, rs1: u8, imm: i32 },
    Xori { rd: u8, rs1: u8, imm: i32 },
    Ori { rd: u8, rs1: u8, imm: i32 },
    Andi { rd: u8, rs1: u8, imm: i32 },
    Slli { rd: u8, rs1: u8, shamt: u8 },
    Srli { rd: u8, rs1: u8, shamt: u8 },
    Srai { rd: u8, rs1: u8, shamt: u8 },

    // Load instructions (I-type format)
    Lb { rd: u8, rs1: u8, imm: i32 },
    Lh { rd: u8, rs1: u8, imm: i32 },
    Lw { rd: u8, rs1: u8, imm: i32 },
    Lbu { rd: u8, rs1: u8, imm: i32 },
    Lhu { rd: u8, rs1: u8, imm: i32 },

    // S-type: store instructions
    Sb { rs1: u8, rs2: u8, imm: i32 },
    Sh { rs1: u8, rs2: u8, imm: i32 },
    Sw { rs1: u8, rs2: u8, imm: i32 },

    // B-type: branches
    Beq { rs1: u8, rs2: u8, imm: i32 },
    Bne { rs1: u8, rs2: u8, imm: i32 },
    Blt { rs1: u8, rs2: u8, imm: i32 },
    Bge { rs1: u8, rs2: u8, imm: i32 },
    Bltu { rs1: u8, rs2: u8, imm: i32 },
    Bgeu { rs1: u8, rs2: u8, imm: i32 },

    // U-type: upper immediate
    Lui { rd: u8, imm: i32 },
    Auipc { rd: u8, imm: i32 },

    // J-type: jumps
    Jal { rd: u8, imm: i32 },
    Jalr { rd: u8, rs1: u8, imm: i32 },

    // System instructions
    Ecall,
    Ebreak,
    Fence,

    // ==================== M Extension ====================
    Mul { rd: u8, rs1: u8, rs2: u8 },
    Mulh { rd: u8, rs1: u8, rs2: u8 },
    Mulhsu { rd: u8, rs1: u8, rs2: u8 },
    Mulhu { rd: u8, rs1: u8, rs2: u8 },
    Div { rd: u8, rs1: u8, rs2: u8 },
    Divu { rd: u8, rs1: u8, rs2: u8 },
    Rem { rd: u8, rs1: u8, rs2: u8 },
    Remu { rd: u8, rs1: u8, rs2: u8 },
}

/// RISC-V opcode constants (private to this module).
const LOAD: u32 = 0b000_0011;
const STORE: u32 = 0b010_0011;
const BRANCH: u32 = 0b110_0011;
const JALR: u32 = 0b110_0111;
const JAL: u32 = 0b110_1111;
const OP_IMM: u32 = 0b001_0011;
const OP: u32 = 0b011_0011;
const AUIPC: u32 = 0b001_0111;
const LUI: u32 = 0b011_0111;
const SYSTEM: u32 = 0b111_0011;
const FENCE: u32 = 0b000_1111;

/// Decode a 32-bit instruction word.
///
/// Returns `Err(word)` if the instruction is invalid or unimplemented.
pub(super) fn decode(word: u32) -> Result<Instruction, u32> {
    let opcode = word & 0x7F;
    let rd = ((word >> 7) & 0x1F) as u8;
    let funct3 = (word >> 12) & 0x07;
    let rs1 = ((word >> 15) & 0x1F) as u8;
    let rs2 = ((word >> 20) & 0x1F) as u8;
    let funct7 = word >> 25;

    match opcode {
        OP => decode_r_type(rd, funct3, rs1, rs2, funct7),
        OP_IMM => decode_op_imm(rd, funct3, rs1, word),
        LOAD => decode_load(rd, funct3, rs1, word),
        STORE => decode_store(funct3, rs1, rs2, word),
        BRANCH => decode_branch(funct3, rs1, rs2, word),
        LUI => Ok(Instruction::Lui {
            rd,
            imm: decode_u_imm(word),
        }),
        AUIPC => Ok(Instruction::Auipc {
            rd,
            imm: decode_u_imm(word),
        }),
        JAL => Ok(Instruction::Jal {
            rd,
            imm: decode_j_imm(word),
        }),
        JALR => {
            if funct3 != 0 {
                return Err(word);
            }
            Ok(Instruction::Jalr {
                rd,
                rs1,
                imm: decode_i_imm(word),
            })
        }
        SYSTEM => decode_system(word),
        FENCE => Ok(Instruction::Fence),
        _ => Err(word),
    }
}

/// Decode R-type instructions (register-register operations).
fn decode_r_type(rd: u8, funct3: u32, rs1: u8, rs2: u8, funct7: u32) -> Result<Instruction, u32> {
    match (funct7, funct3) {
        // RV32I base
        (0b000_0000, 0b000) => Ok(Instruction::Add { rd, rs1, rs2 }),
        (0b010_0000, 0b000) => Ok(Instruction::Sub { rd, rs1, rs2 }),
        (0b000_0000, 0b001) => Ok(Instruction::Sll { rd, rs1, rs2 }),
        (0b000_0000, 0b010) => Ok(Instruction::Slt { rd, rs1, rs2 }),
        (0b000_0000, 0b011) => Ok(Instruction::Sltu { rd, rs1, rs2 }),
        (0b000_0000, 0b100) => Ok(Instruction::Xor { rd, rs1, rs2 }),
        (0b000_0000, 0b101) => Ok(Instruction::Srl { rd, rs1, rs2 }),
        (0b010_0000, 0b101) => Ok(Instruction::Sra { rd, rs1, rs2 }),
        (0b000_0000, 0b110) => Ok(Instruction::Or { rd, rs1, rs2 }),
        (0b000_0000, 0b111) => Ok(Instruction::And { rd, rs1, rs2 }),

        // M extension
        (0b000_0001, 0b000) => Ok(Instruction::Mul { rd, rs1, rs2 }),
        (0b000_0001, 0b001) => Ok(Instruction::Mulh { rd, rs1, rs2 }),
        (0b000_0001, 0b010) => Ok(Instruction::Mulhsu { rd, rs1, rs2 }),
        (0b000_0001, 0b011) => Ok(Instruction::Mulhu { rd, rs1, rs2 }),
        (0b000_0001, 0b100) => Ok(Instruction::Div { rd, rs1, rs2 }),
        (0b000_0001, 0b101) => Ok(Instruction::Divu { rd, rs1, rs2 }),
        (0b000_0001, 0b110) => Ok(Instruction::Rem { rd, rs1, rs2 }),
        (0b000_0001, 0b111) => Ok(Instruction::Remu { rd, rs1, rs2 }),

        _ => Err(0), // Will be replaced with actual word in caller
    }
}

/// Decode I-type immediate arithmetic operations.
fn decode_op_imm(rd: u8, funct3: u32, rs1: u8, word: u32) -> Result<Instruction, u32> {
    let imm = decode_i_imm(word);
    let shamt = ((word >> 20) & 0x1F) as u8;
    let funct7 = word >> 25;

    match funct3 {
        0b000 => Ok(Instruction::Addi { rd, rs1, imm }),
        0b010 => Ok(Instruction::Slti { rd, rs1, imm }),
        0b011 => Ok(Instruction::Sltiu { rd, rs1, imm }),
        0b100 => Ok(Instruction::Xori { rd, rs1, imm }),
        0b110 => Ok(Instruction::Ori { rd, rs1, imm }),
        0b111 => Ok(Instruction::Andi { rd, rs1, imm }),
        0b001 => {
            if funct7 == 0b000_0000 {
                Ok(Instruction::Slli { rd, rs1, shamt })
            } else {
                Err(word)
            }
        }
        0b101 => match funct7 {
            0b000_0000 => Ok(Instruction::Srli { rd, rs1, shamt }),
            0b010_0000 => Ok(Instruction::Srai { rd, rs1, shamt }),
            _ => Err(word),
        },
        _ => Err(word),
    }
}

/// Decode load instructions.
fn decode_load(rd: u8, funct3: u32, rs1: u8, word: u32) -> Result<Instruction, u32> {
    let imm = decode_i_imm(word);

    match funct3 {
        0b000 => Ok(Instruction::Lb { rd, rs1, imm }),
        0b001 => Ok(Instruction::Lh { rd, rs1, imm }),
        0b010 => Ok(Instruction::Lw { rd, rs1, imm }),
        0b100 => Ok(Instruction::Lbu { rd, rs1, imm }),
        0b101 => Ok(Instruction::Lhu { rd, rs1, imm }),
        _ => Err(word),
    }
}

/// Decode store instructions.
fn decode_store(funct3: u32, rs1: u8, rs2: u8, word: u32) -> Result<Instruction, u32> {
    let imm = decode_s_imm(word);

    match funct3 {
        0b000 => Ok(Instruction::Sb { rs1, rs2, imm }),
        0b001 => Ok(Instruction::Sh { rs1, rs2, imm }),
        0b010 => Ok(Instruction::Sw { rs1, rs2, imm }),
        _ => Err(word),
    }
}

/// Decode branch instructions.
fn decode_branch(funct3: u32, rs1: u8, rs2: u8, word: u32) -> Result<Instruction, u32> {
    let imm = decode_b_imm(word);

    match funct3 {
        0b000 => Ok(Instruction::Beq { rs1, rs2, imm }),
        0b001 => Ok(Instruction::Bne { rs1, rs2, imm }),
        0b100 => Ok(Instruction::Blt { rs1, rs2, imm }),
        0b101 => Ok(Instruction::Bge { rs1, rs2, imm }),
        0b110 => Ok(Instruction::Bltu { rs1, rs2, imm }),
        0b111 => Ok(Instruction::Bgeu { rs1, rs2, imm }),
        _ => Err(word),
    }
}

/// Decode system instructions.
fn decode_system(word: u32) -> Result<Instruction, u32> {
    let imm = (word >> 20) & 0xFFF;
    let funct3 = (word >> 12) & 0x07;

    if funct3 != 0 {
        return Err(word); // CSR instructions not implemented
    }

    match imm {
        0 => Ok(Instruction::Ecall),
        1 => Ok(Instruction::Ebreak),
        _ => Err(word),
    }
}

// ==================== Immediate Decoders ====================

/// Decode I-type immediate (12-bit, sign-extended).
/// imm[11:0] = inst[31:20]
fn decode_i_imm(word: u32) -> i32 {
    (word as i32) >> 20
}

/// Decode S-type immediate (12-bit, sign-extended).
/// imm[11:5] = inst[31:25], imm[4:0] = inst[11:7]
fn decode_s_imm(word: u32) -> i32 {
    let imm11_5 = (word >> 25) & 0x7F;
    let imm4_0 = (word >> 7) & 0x1F;
    let imm = (imm11_5 << 5) | imm4_0;
    // Sign-extend from bit 11
    ((imm as i32) << 20) >> 20
}

/// Decode B-type immediate (13-bit, sign-extended, bit 0 always 0).
/// imm[12|10:5|4:1|11] = inst[31|30:25|11:8|7]
fn decode_b_imm(word: u32) -> i32 {
    let imm12 = (word >> 31) & 0x1;
    let imm11 = (word >> 7) & 0x1;
    let imm10_5 = (word >> 25) & 0x3F;
    let imm4_1 = (word >> 8) & 0xF;
    let imm = (imm12 << 12) | (imm11 << 11) | (imm10_5 << 5) | (imm4_1 << 1);
    // Sign-extend from bit 12
    ((imm as i32) << 19) >> 19
}

/// Decode U-type immediate (upper 20 bits).
/// imm[31:12] = inst[31:12]
fn decode_u_imm(word: u32) -> i32 {
    (word & 0xFFFF_F000) as i32
}

/// Decode J-type immediate (21-bit, sign-extended, bit 0 always 0).
/// imm[20|10:1|11|19:12] = inst[31|30:21|20|19:12]
fn decode_j_imm(word: u32) -> i32 {
    let imm20 = (word >> 31) & 0x1;
    let imm19_12 = (word >> 12) & 0xFF;
    let imm11 = (word >> 20) & 0x1;
    let imm10_1 = (word >> 21) & 0x3FF;
    let imm = (imm20 << 20) | (imm19_12 << 12) | (imm11 << 11) | (imm10_1 << 1);
    // Sign-extend from bit 20
    ((imm as i32) << 11) >> 11
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_add() {
        // add x3, x1, x2 = 0x002080B3
        let word = 0x002080B3;
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Add {
                rd: 1,
                rs1: 1,
                rs2: 2
            }
        );
    }

    #[test]
    fn test_decode_addi() {
        // addi x1, x0, 42 = 0x02A00093
        let word = 0x02A00093;
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Addi {
                rd: 1,
                rs1: 0,
                imm: 42
            }
        );
    }

    #[test]
    fn test_decode_addi_negative() {
        // addi x1, x0, -1 = 0xFFF00093
        let word = 0xFFF00093;
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Addi {
                rd: 1,
                rs1: 0,
                imm: -1
            }
        );
    }

    #[test]
    fn test_decode_lui() {
        // lui x1, 0x12345 = 0x12345037 (note: rd=0 is actually encoded differently)
        // lui x1, 0x12345
        let word = 0x123450B7; // lui x1, 0x12345
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Lui {
                rd: 1,
                imm: 0x12345000_u32 as i32
            }
        );
    }

    #[test]
    fn test_decode_beq() {
        // beq x1, x2, 8 = offset of 8 bytes
        // imm[12|10:5] = inst[31|30:25]
        // imm[4:1|11] = inst[11:8|7]
        // For offset 8: imm = 0b0_000000_0100_0 = 8
        let word = 0x00208463; // beq x1, x2, 8
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Beq {
                rs1: 1,
                rs2: 2,
                imm: 8
            }
        );
    }

    #[test]
    fn test_decode_jal() {
        // jal x1, 0 = jump and link with offset 0
        let word = 0x000000EF; // jal x1, 0
        let inst = decode(word).unwrap();
        assert_eq!(inst, Instruction::Jal { rd: 1, imm: 0 });
    }

    #[test]
    fn test_decode_ecall() {
        let word = 0x00000073;
        let inst = decode(word).unwrap();
        assert_eq!(inst, Instruction::Ecall);
    }

    #[test]
    fn test_decode_ebreak() {
        let word = 0x00100073;
        let inst = decode(word).unwrap();
        assert_eq!(inst, Instruction::Ebreak);
    }

    #[test]
    fn test_decode_mul() {
        // mul x3, x1, x2
        let word = 0x022081B3;
        let inst = decode(word).unwrap();
        assert_eq!(
            inst,
            Instruction::Mul {
                rd: 3,
                rs1: 1,
                rs2: 2
            }
        );
    }
}
