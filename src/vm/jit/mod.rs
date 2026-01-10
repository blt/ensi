//! JIT compiler for RISC-V to native code using Cranelift.
//!
//! The JIT compiles basic blocks of RISC-V instructions to native machine code
//! for faster execution. Hot blocks are detected and compiled on demand.

mod blocks;
mod codegen;

use crate::vm::{Cpu, Memory};
use crate::{MeteringConfig, SyscallHandler, TrapCause, TurnResult};
use cranelift_codegen::settings::Configurable;
use cranelift_jit::{JITBuilder, JITModule};
use std::collections::HashMap;

pub use blocks::BasicBlock;
pub use codegen::JitCodegen;

/// Maximum number of compiled blocks to cache.
const MAX_CACHED_BLOCKS: usize = 1024;

/// Execution count threshold before JIT compilation.
const JIT_THRESHOLD: u32 = 10;

/// Result of executing a JIT-compiled block.
#[derive(Debug, Clone, Copy)]
pub enum BlockResult {
    /// Block executed successfully, continue at next PC.
    Continue {
        /// Next PC to execute.
        next_pc: u32,
        /// Instructions executed in this block.
        instructions: u32,
        /// Cost consumed by this block.
        cost: u32,
    },
    /// Block ended with a syscall.
    Syscall {
        /// PC of the ecall instruction.
        pc: u32,
        /// Instructions executed before syscall.
        instructions: u32,
        /// Cost consumed before syscall.
        cost: u32,
    },
    /// Block ended with a trap.
    Trap(TrapCause),
    /// Budget exhausted mid-block.
    BudgetExhausted {
        /// Remaining budget.
        remaining: u32,
    },
}

/// Compiled block metadata.
#[allow(dead_code)]
struct CompiledBlock {
    /// Function pointer to compiled code.
    /// Signature: fn(cpu: *mut Cpu, memory: *mut Memory, budget: u32) -> BlockResult
    func_ptr: *const u8,
    /// Total cost of all instructions in block.
    total_cost: u32,
    /// Number of instructions in block.
    instruction_count: u32,
}

/// Execution statistics for a PC address.
#[derive(Default)]
struct BlockStats {
    /// Number of times this PC has been executed.
    execution_count: u32,
    /// Whether this block has been compiled.
    compiled: bool,
}

/// JIT compilation engine.
pub struct JitEngine {
    /// Cranelift JIT module.
    module: JITModule,
    /// Code generator.
    codegen: JitCodegen,
    /// Cache of compiled blocks by start PC.
    compiled_blocks: HashMap<u32, CompiledBlock>,
    /// Execution statistics per PC.
    block_stats: HashMap<u32, BlockStats>,
    /// Metering configuration.
    metering: MeteringConfig,
}

impl std::fmt::Debug for JitEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitEngine")
            .field("compiled_blocks", &self.compiled_blocks.len())
            .field("block_stats", &self.block_stats.len())
            .finish()
    }
}

impl JitEngine {
    /// Create a new JIT engine.
    ///
    /// # Panics
    ///
    /// Panics if the native target is not supported by Cranelift.
    #[must_use]
    pub fn new(metering: MeteringConfig) -> Self {
        let mut flag_builder = cranelift_codegen::settings::builder();
        // Enable speed optimizations
        flag_builder.set("opt_level", "speed").ok();

        let isa_builder = cranelift_native::builder()
            .unwrap_or_else(|msg| panic!("host machine not supported: {msg}"));

        let isa = isa_builder
            .finish(cranelift_codegen::settings::Flags::new(flag_builder))
            .unwrap_or_else(|e| panic!("failed to build ISA: {e}"));

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);
        let codegen = JitCodegen::new();

        Self {
            module,
            codegen,
            compiled_blocks: HashMap::new(),
            block_stats: HashMap::new(),
            metering,
        }
    }

    /// Check if a block at the given PC should be compiled.
    fn should_compile(&mut self, pc: u32) -> bool {
        let stats = self.block_stats.entry(pc).or_default();

        if stats.compiled {
            return false;
        }

        stats.execution_count += 1;

        if stats.execution_count >= JIT_THRESHOLD {
            stats.compiled = true;
            return true;
        }

        false
    }

    /// Get a compiled block if available.
    #[allow(dead_code)]
    fn get_compiled(&self, pc: u32) -> Option<&CompiledBlock> {
        self.compiled_blocks.get(&pc)
    }

    /// Compile a basic block starting at the given PC.
    ///
    /// Returns true if compilation succeeded.
    #[allow(dead_code)]
    pub fn compile_block(&mut self, pc: u32, memory: &Memory) -> bool {
        // Analyze the basic block
        let block = match blocks::analyze_block(pc, memory, &self.metering) {
            Some(b) => b,
            None => return false,
        };

        // Generate Cranelift IR and compile
        match self.codegen.compile_block(&block, &mut self.module) {
            Ok(func_ptr) => {
                // Evict old blocks if cache is full
                if self.compiled_blocks.len() >= MAX_CACHED_BLOCKS {
                    // Simple eviction: remove a random block
                    // A real implementation would use LRU
                    if let Some(&key) = self.compiled_blocks.keys().next() {
                        self.compiled_blocks.remove(&key);
                    }
                }

                self.compiled_blocks.insert(
                    pc,
                    CompiledBlock {
                        func_ptr,
                        total_cost: block.total_cost,
                        instruction_count: block.instructions.len() as u32,
                    },
                );
                true
            }
            Err(_) => false,
        }
    }

    /// Run a turn using hybrid interpreter/JIT execution.
    ///
    /// Hot blocks are compiled and executed natively, cold blocks use interpreter.
    pub fn run_turn<H: SyscallHandler>(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut Memory,
        handler: &mut H,
        budget: u32,
    ) -> TurnResult {
        let mut remaining = budget;
        #[allow(unused_variables)]
        let mut total_executed: u64 = 0;

        loop {
            // Check PC alignment
            if !cpu.pc.is_multiple_of(4) {
                return TurnResult::Trap(TrapCause::InstructionMisaligned(cpu.pc));
            }

            // Check if we should compile this block
            if self.should_compile(cpu.pc) {
                self.compile_block(cpu.pc, memory);
            }

            // Try to execute compiled block if available and budget allows
            if let Some(block) = self.compiled_blocks.get(&cpu.pc) {
                if block.total_cost <= remaining {
                    // Execute the compiled block
                    // Safety: func_ptr is valid and was compiled with the correct signature
                    let packed_result = unsafe {
                        let func: fn(*mut u32, *mut u8, u32, u32) -> u64 =
                            std::mem::transmute(block.func_ptr);
                        func(
                            cpu.regs_mut_ptr(),
                            memory.data_mut_ptr(),
                            memory.base(),
                            memory.size(),
                        )
                    };

                    // Unpack result: high 32 bits = tag, low 32 bits = data
                    let tag = packed_result >> 32;
                    let data = packed_result as u32;

                    match tag {
                        0 => {
                            // Continue: data = next_pc
                            cpu.pc = data;
                            remaining -= block.total_cost;
                            total_executed += u64::from(block.instruction_count);
                            continue;
                        }
                        1 => {
                            // Syscall: data = pc of ecall
                            // Advance PC past the ecall
                            cpu.pc = data.wrapping_add(4);
                            // Handle the syscall
                            if let Err(cause) = handler.handle(cpu, memory) {
                                return TurnResult::Trap(cause);
                            }
                            remaining -= block.total_cost;
                            total_executed += u64::from(block.instruction_count);
                            continue;
                        }
                        2 => {
                            // Trap: data = trap code
                            return TurnResult::Trap(TrapCause::InvalidInstruction(data));
                        }
                        3 => {
                            // Budget exhausted: data = remaining
                            return TurnResult::BudgetExhausted { remaining: data };
                        }
                        _ => {
                            // Should not happen - fall back to interpreter
                        }
                    }
                }
            }

            // Fall back to interpreter
            let result = self.interpret_one(cpu, memory, handler, remaining);

            match result {
                BlockResult::Continue {
                    next_pc,
                    instructions,
                    cost,
                } => {
                    cpu.pc = next_pc;
                    total_executed += u64::from(instructions);
                    if cost > remaining {
                        return TurnResult::BudgetExhausted { remaining };
                    }
                    remaining -= cost;
                }
                BlockResult::Syscall {
                    pc: _,
                    instructions,
                    cost,
                } => {
                    total_executed += u64::from(instructions);
                    remaining = remaining.saturating_sub(cost);
                    // Syscall was already handled in interpret_one
                }
                BlockResult::Trap(cause) => {
                    return TurnResult::Trap(cause);
                }
                BlockResult::BudgetExhausted { remaining: r } => {
                    return TurnResult::BudgetExhausted { remaining: r };
                }
            }
        }
    }

    /// Interpret a single instruction (fallback path).
    fn interpret_one<H: SyscallHandler>(
        &self,
        cpu: &mut Cpu,
        memory: &mut Memory,
        handler: &mut H,
        budget: u32,
    ) -> BlockResult {
        use crate::isa::{decode, execute_rv32i, execute_rv32m};

        // Fetch
        let word = match memory.fetch(cpu.pc) {
            Ok(w) => w,
            Err(e) => return BlockResult::Trap(e),
        };

        // Decode
        let inst = match decode(word) {
            Ok(i) => i,
            Err(w) => return BlockResult::Trap(TrapCause::InvalidInstruction(w)),
        };

        // Check budget
        let cost = self.metering.cost(&inst);
        if budget < cost {
            return BlockResult::BudgetExhausted { remaining: budget };
        }

        // Handle ecall specially
        if matches!(inst, crate::Instruction::Ecall) {
            if let Err(cause) = handler.handle(cpu, memory) {
                return BlockResult::Trap(cause);
            }
            return BlockResult::Syscall {
                pc: cpu.pc,
                instructions: 1,
                cost,
            };
        }

        // Execute
        let next_pc = match execute_rv32i(inst, cpu, memory, cpu.pc) {
            Ok(pc) => pc,
            Err(TrapCause::InvalidInstruction(_)) => {
                match execute_rv32m(inst, cpu, cpu.pc) {
                    Ok(pc) => pc,
                    Err(e) => return BlockResult::Trap(e),
                }
            }
            Err(e) => return BlockResult::Trap(e),
        };

        BlockResult::Continue {
            next_pc,
            instructions: 1,
            cost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Instruction encoding helpers for tests
    // ADDI rd, rs1, imm: I-type, opcode=0x13, funct3=0
    fn encode_addi(rd: u32, rs1: u32, imm: i32) -> u32 {
        let imm_u = (imm as u32) & 0xFFF;
        (imm_u << 20) | (rs1 << 15) | (0 << 12) | (rd << 7) | 0x13
    }

    // ADD rd, rs1, rs2: R-type, opcode=0x33, funct3=0, funct7=0
    fn encode_add(rd: u32, rs1: u32, rs2: u32) -> u32 {
        (0 << 25) | (rs2 << 20) | (rs1 << 15) | (0 << 12) | (rd << 7) | 0x33
    }

    // BEQ rs1, rs2, imm: B-type, opcode=0x63, funct3=0
    fn encode_beq(rs1: u32, rs2: u32, imm: i32) -> u32 {
        let imm_u = imm as u32;
        let imm_12 = (imm_u >> 12) & 1;
        let imm_10_5 = (imm_u >> 5) & 0x3F;
        let imm_4_1 = (imm_u >> 1) & 0xF;
        let imm_11 = (imm_u >> 11) & 1;
        (imm_12 << 31) | (imm_10_5 << 25) | (rs2 << 20) | (rs1 << 15) | (0 << 12) | (imm_4_1 << 8) | (imm_11 << 7) | 0x63
    }

    // JAL rd, imm: J-type, opcode=0x6F
    fn encode_jal(rd: u32, imm: i32) -> u32 {
        let imm_u = imm as u32;
        let imm_20 = (imm_u >> 20) & 1;
        let imm_10_1 = (imm_u >> 1) & 0x3FF;
        let imm_11 = (imm_u >> 11) & 1;
        let imm_19_12 = (imm_u >> 12) & 0xFF;
        (imm_20 << 31) | (imm_10_1 << 21) | (imm_11 << 20) | (imm_19_12 << 12) | (rd << 7) | 0x6F
    }

    // MUL rd, rs1, rs2: R-type, opcode=0x33, funct3=0, funct7=1
    fn encode_mul(rd: u32, rs1: u32, rs2: u32) -> u32 {
        (1 << 25) | (rs2 << 20) | (rs1 << 15) | (0 << 12) | (rd << 7) | 0x33
    }

    #[test]
    fn test_jit_engine_creation() {
        let engine = JitEngine::new(MeteringConfig::default());
        assert_eq!(engine.compiled_blocks.len(), 0);
    }

    #[test]
    fn test_jit_compile_simple_block() {
        let mut engine = JitEngine::new(MeteringConfig::default());
        let mut memory = Memory::new(1024, 0);

        // addi x1, x0, 42  (x1 = 42)
        memory.store_u32(0, encode_addi(1, 0, 42)).ok();
        // addi x2, x0, 10  (x2 = 10)
        memory.store_u32(4, encode_addi(2, 0, 10)).ok();
        // add x3, x1, x2   (x3 = x1 + x2 = 52)
        memory.store_u32(8, encode_add(3, 1, 2)).ok();
        // beq x0, x0, 0    (unconditional branch to self - terminates block)
        memory.store_u32(12, encode_beq(0, 0, 0)).ok();

        // Force compilation by calling compile_block directly
        let compiled = engine.compile_block(0, &memory);
        assert!(compiled, "Block should compile successfully");
        assert_eq!(engine.compiled_blocks.len(), 1);
    }

    #[test]
    fn test_jit_execute_arithmetic() {
        let mut engine = JitEngine::new(MeteringConfig::default());
        let mut memory = Memory::new(1024, 0);
        let mut cpu = Cpu::new();

        // Program:
        // addi x1, x0, 100   (x1 = 100)
        // addi x2, x0, 23    (x2 = 23)
        // add  x3, x1, x2    (x3 = 123)
        // jal  x0, 0         (jump to self, terminates block)
        memory.store_u32(0, encode_addi(1, 0, 100)).ok();
        memory.store_u32(4, encode_addi(2, 0, 23)).ok();
        memory.store_u32(8, encode_add(3, 1, 2)).ok();
        memory.store_u32(12, encode_jal(0, 0)).ok();

        // Force compilation
        assert!(engine.compile_block(0, &memory));

        // Execute the compiled block directly
        let block = engine.compiled_blocks.get(&0).expect("block exists");

        // Safety: func_ptr was compiled with correct signature
        let packed_result = unsafe {
            let func: fn(*mut u32, *mut u8, u32, u32) -> u64 =
                std::mem::transmute(block.func_ptr);
            func(
                cpu.regs_mut_ptr(),
                memory.data_mut_ptr(),
                memory.base(),
                memory.size(),
            )
        };

        // Check result: should be Continue with pc=12 (jal x0, 0 jumps to 12+0=12)
        let tag = packed_result >> 32;
        let data = packed_result as u32;
        assert_eq!(tag, 0, "Should be Continue result");
        assert_eq!(data, 12, "Next PC should be 12");

        // Check registers were updated correctly
        assert_eq!(cpu.read_reg(0), 0, "x0 should always be 0");
        assert_eq!(cpu.read_reg(1), 100, "x1 should be 100");
        assert_eq!(cpu.read_reg(2), 23, "x2 should be 23");
        assert_eq!(cpu.read_reg(3), 123, "x3 should be 123 (100 + 23)");
    }

    #[test]
    fn test_jit_branch_taken() {
        let mut engine = JitEngine::new(MeteringConfig::default());
        let mut memory = Memory::new(1024, 0);
        let mut cpu = Cpu::new();

        // Program:
        // addi x1, x0, 5     (x1 = 5)
        // beq  x1, x1, 100   (branch to pc+100 if x1 == x1, always true)
        memory.store_u32(0, encode_addi(1, 0, 5)).ok();
        memory.store_u32(4, encode_beq(1, 1, 100)).ok();

        // Force compilation
        assert!(engine.compile_block(0, &memory));

        let block = engine.compiled_blocks.get(&0).expect("block exists");
        let packed_result = unsafe {
            let func: fn(*mut u32, *mut u8, u32, u32) -> u64 =
                std::mem::transmute(block.func_ptr);
            func(
                cpu.regs_mut_ptr(),
                memory.data_mut_ptr(),
                memory.base(),
                memory.size(),
            )
        };

        let tag = packed_result >> 32;
        let data = packed_result as u32;
        assert_eq!(tag, 0, "Should be Continue result");
        assert_eq!(data, 4 + 100, "Branch should be taken, next PC = 4 + 100 = 104");
        assert_eq!(cpu.read_reg(1), 5, "x1 should be 5");
    }

    #[test]
    fn test_jit_branch_not_taken() {
        let mut engine = JitEngine::new(MeteringConfig::default());
        let mut memory = Memory::new(1024, 0);
        let mut cpu = Cpu::new();

        // Program:
        // addi x1, x0, 5     (x1 = 5)
        // addi x2, x0, 10    (x2 = 10)
        // beq  x1, x2, 100   (branch to pc+100 if x1 == x2, false since 5 != 10)
        memory.store_u32(0, encode_addi(1, 0, 5)).ok();
        memory.store_u32(4, encode_addi(2, 0, 10)).ok();
        memory.store_u32(8, encode_beq(1, 2, 100)).ok();

        assert!(engine.compile_block(0, &memory));

        let block = engine.compiled_blocks.get(&0).expect("block exists");
        let packed_result = unsafe {
            let func: fn(*mut u32, *mut u8, u32, u32) -> u64 =
                std::mem::transmute(block.func_ptr);
            func(
                cpu.regs_mut_ptr(),
                memory.data_mut_ptr(),
                memory.base(),
                memory.size(),
            )
        };

        let tag = packed_result >> 32;
        let data = packed_result as u32;
        assert_eq!(tag, 0, "Should be Continue result");
        assert_eq!(data, 12, "Branch not taken, next PC = 8 + 4 = 12");
    }

    #[test]
    fn test_jit_multiply() {
        let mut engine = JitEngine::new(MeteringConfig::default());
        let mut memory = Memory::new(1024, 0);
        let mut cpu = Cpu::new();

        // Program:
        // addi x1, x0, 7     (x1 = 7)
        // addi x2, x0, 6     (x2 = 6)
        // mul  x3, x1, x2    (x3 = 7 * 6 = 42)
        // jal  x0, 0         (terminates block)
        memory.store_u32(0, encode_addi(1, 0, 7)).ok();
        memory.store_u32(4, encode_addi(2, 0, 6)).ok();
        memory.store_u32(8, encode_mul(3, 1, 2)).ok();
        memory.store_u32(12, encode_jal(0, 0)).ok();

        assert!(engine.compile_block(0, &memory));

        let block = engine.compiled_blocks.get(&0).expect("block exists");
        let packed_result = unsafe {
            let func: fn(*mut u32, *mut u8, u32, u32) -> u64 =
                std::mem::transmute(block.func_ptr);
            func(
                cpu.regs_mut_ptr(),
                memory.data_mut_ptr(),
                memory.base(),
                memory.size(),
            )
        };

        let tag = packed_result >> 32;
        assert_eq!(tag, 0, "Should be Continue result");
        assert_eq!(cpu.read_reg(3), 42, "x3 should be 42 (7 * 6)");
    }
}
