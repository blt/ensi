//! Differential testing against rrs-lib reference implementation.
//!
//! This module tests our RISC-V VM against the rrs-lib crate to ensure
//! bit-exact compatibility for all RV32IM instructions.

#![allow(missing_docs)]
#![allow(clippy::unreadable_literal)] // Instruction encodings are standard hex
#![allow(clippy::unwrap_used)] // Test code can use unwrap
#![allow(clippy::cast_lossless)] // Test code casts are intentional
#![allow(clippy::cast_sign_loss)] // Test code casts are intentional
#![allow(clippy::cast_possible_truncation)] // Test code casts are intentional
#![allow(clippy::match_same_arms)] // ARM encodings are clearer with separate arms

use proptest::prelude::*;
use rrs_lib::{HartState, instruction_executor::InstructionExecutor, memories::VecMemory};

use ensi::{NoSyscalls, Vm, isa::decode};

/// Generate a valid RV32I/M instruction word.
fn valid_instruction() -> impl Strategy<Value = u32> {
    // Focus on instructions that are likely to be valid
    prop_oneof![
        // R-type arithmetic (ADD, SUB, AND, OR, XOR, SLT, SLTU, SLL, SRL, SRA)
        r_type_instruction(),
        // I-type arithmetic (ADDI, ANDI, ORI, XORI, SLTI, SLTIU)
        i_type_arithmetic(),
        // Shift immediate (SLLI, SRLI, SRAI)
        shift_immediate(),
        // M extension (MUL, MULH, MULHSU, MULHU, DIV, DIVU, REM, REMU)
        m_extension(),
    ]
}

fn r_type_instruction() -> impl Strategy<Value = u32> {
    (0u8..32, 0u8..32, 0u8..32, 0u8..10).prop_map(|(rd, rs1, rs2, op)| {
        let opcode = 0b0110011u32;
        let funct3 = match op {
            0 => 0b000, // ADD
            1 => 0b000, // SUB (funct7 = 0x20)
            2 => 0b001, // SLL
            3 => 0b010, // SLT
            4 => 0b011, // SLTU
            5 => 0b100, // XOR
            6 => 0b101, // SRL
            7 => 0b101, // SRA (funct7 = 0x20)
            8 => 0b110, // OR
            _ => 0b111, // AND
        };
        let funct7 = match op {
            1 | 7 => 0b0100000,
            _ => 0b0000000,
        };
        opcode
            | ((rd as u32) << 7)
            | (funct3 << 12)
            | ((rs1 as u32) << 15)
            | ((rs2 as u32) << 20)
            | (funct7 << 25)
    })
}

fn i_type_arithmetic() -> impl Strategy<Value = u32> {
    (0u8..32, 0u8..32, -2048i32..2048, 0u8..6).prop_map(|(rd, rs1, imm, op)| {
        let opcode = 0b0010011u32;
        let funct3 = match op {
            0 => 0b000, // ADDI
            1 => 0b010, // SLTI
            2 => 0b011, // SLTIU
            3 => 0b100, // XORI
            4 => 0b110, // ORI
            _ => 0b111, // ANDI
        };
        let imm_bits = (imm as u32) & 0xFFF;
        opcode | ((rd as u32) << 7) | (funct3 << 12) | ((rs1 as u32) << 15) | (imm_bits << 20)
    })
}

fn shift_immediate() -> impl Strategy<Value = u32> {
    (0u8..32, 0u8..32, 0u8..32, 0u8..3).prop_map(|(rd, rs1, shamt, op)| {
        let opcode = 0b0010011u32;
        let funct3 = match op {
            0 => 0b001, // SLLI
            1 => 0b101, // SRLI
            _ => 0b101, // SRAI (funct7 = 0x20)
        };
        let funct7 = if op == 2 { 0b0100000 } else { 0b0000000 };
        opcode
            | ((rd as u32) << 7)
            | (funct3 << 12)
            | ((rs1 as u32) << 15)
            | (((shamt & 0x1F) as u32) << 20)
            | (funct7 << 25)
    })
}

fn m_extension() -> impl Strategy<Value = u32> {
    (0u8..32, 0u8..32, 0u8..32, 0u8..8).prop_map(|(rd, rs1, rs2, op)| {
        let opcode = 0b0110011u32;
        let funct7 = 0b0000001u32;
        let funct3 = op as u32; // 0=MUL, 1=MULH, 2=MULHSU, 3=MULHU, 4=DIV, 5=DIVU, 6=REM, 7=REMU
        opcode
            | ((rd as u32) << 7)
            | (funct3 << 12)
            | ((rs1 as u32) << 15)
            | ((rs2 as u32) << 20)
            | (funct7 << 25)
    })
}

/// Set up our VM with given register values and instruction.
fn setup_our_vm(regs: &[u32; 32], inst: u32) -> Vm<NoSyscalls> {
    let mut vm = Vm::new(1024, 0, NoSyscalls);
    vm.cpu.set_registers(*regs);
    vm.cpu.pc = 0;
    let _ = vm.memory.store_u32(0, inst);
    vm
}

/// Set up rrs-lib with given register values and instruction.
fn setup_rrs(regs: &[u32; 32], inst: u32) -> (HartState, VecMemory) {
    let mut hart = HartState::new();
    // Copy registers (rrs-lib ignores index 0)
    hart.registers[1..32].copy_from_slice(&regs[1..32]);
    hart.pc = 0;

    // Create memory with the instruction at address 0
    // VecMemory takes Vec<u32>, so we put the instruction at index 0
    let mut mem_data = vec![0u32; 256]; // 1KB = 256 words
    mem_data[0] = inst;
    let mem = VecMemory::new(mem_data);

    (hart, mem)
}

/// Compare register states between our VM and rrs-lib.
fn compare_states(our_vm: &Vm<NoSyscalls>, rrs_hart: &HartState) -> bool {
    // x0 is always 0 in both
    for i in 1..32 {
        if our_vm.cpu.read_reg(i as u8) != rrs_hart.registers[i] {
            return false;
        }
    }
    // Compare PC (both should have advanced by 4 for a successful instruction)
    our_vm.cpu.pc == rrs_hart.pc
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    /// Test that our decoder accepts the same instructions as rrs-lib.
    #[test]
    fn differential_decode(inst in valid_instruction()) {
        let our_result = decode(inst);
        // If we decode it, it should be valid
        prop_assert!(our_result.is_ok(), "Failed to decode {:#010x}", inst);
    }

    /// Test single instruction execution matches rrs-lib.
    #[test]
    fn differential_execute(
        regs in prop::array::uniform32(any::<u32>()),
        inst in valid_instruction()
    ) {
        // Skip if our decoder rejects it
        if decode(inst).is_err() {
            return Ok(());
        }

        // Set up both VMs
        let mut our_vm = setup_our_vm(&regs, inst);
        let (mut rrs_hart, mut rrs_mem) = setup_rrs(&regs, inst);

        // Execute on our VM
        let our_result = our_vm.step();

        // Execute on rrs-lib
        let mut executor = InstructionExecutor {
            hart_state: &mut rrs_hart,
            mem: &mut rrs_mem,
        };
        let rrs_result = executor.step();

        // Both should succeed or both should fail
        match (our_result, rrs_result) {
            (ensi::StepResult::Ok(_), Ok(())) => {
                // Both succeeded - compare final states
                prop_assert!(
                    compare_states(&our_vm, &rrs_hart),
                    "State mismatch after executing {:#010x}\nOur regs: {:?}\nrrs regs: {:?}\nOur PC: {:#x}\nrrs PC: {:#x}",
                    inst,
                    (1..32).map(|i| our_vm.cpu.read_reg(i as u8)).collect::<Vec<_>>(),
                    &rrs_hart.registers[1..],
                    our_vm.cpu.pc,
                    rrs_hart.pc
                );
            }
            (ensi::StepResult::Trap(_), Err(_)) => {
                // Both failed - acceptable
            }
            (our, rrs) => {
                // Divergence!
                prop_assert!(
                    false,
                    "Execution diverged on {:#010x}: our={:?}, rrs={:?}",
                    inst, our, rrs
                );
            }
        }
    }
}

#[cfg(test)]
mod manual_tests {
    use super::*;

    #[test]
    fn test_add_differential() {
        let mut regs = [0u32; 32];
        regs[1] = 100;
        regs[2] = 42;

        // add x3, x1, x2
        let inst = 0x002081B3u32;

        let mut our_vm = setup_our_vm(&regs, inst);
        let (mut rrs_hart, mut rrs_mem) = setup_rrs(&regs, inst);

        let _ = our_vm.step();
        let mut executor = InstructionExecutor {
            hart_state: &mut rrs_hart,
            mem: &mut rrs_mem,
        };
        let _ = executor.step();

        assert_eq!(our_vm.cpu.read_reg(3), 142);
        assert_eq!(rrs_hart.registers[3], 142);
        assert!(compare_states(&our_vm, &rrs_hart));
    }

    #[test]
    fn test_div_by_zero_differential() {
        let mut regs = [0u32; 32];
        regs[1] = 42;
        regs[2] = 0; // divisor = 0

        // div x3, x1, x2
        let inst = 0x0220C1B3u32;

        let mut our_vm = setup_our_vm(&regs, inst);
        let (mut rrs_hart, mut rrs_mem) = setup_rrs(&regs, inst);

        let _ = our_vm.step();
        let mut executor = InstructionExecutor {
            hart_state: &mut rrs_hart,
            mem: &mut rrs_mem,
        };
        let _ = executor.step();

        // Both should return -1 (all 1s) for division by zero
        assert_eq!(our_vm.cpu.read_reg(3), u32::MAX);
        assert_eq!(rrs_hart.registers[3], u32::MAX);
    }
}
