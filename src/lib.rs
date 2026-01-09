// Allow unwrap and unreadable literals in tests (test code is not production)
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::unreadable_literal))]
//! Ensi: A deterministic RISC-V VM for programming games.
//!
//! This crate provides a RISC-V RV32IM interpreter designed for:
//! - Bit-exact deterministic execution
//! - Instruction metering for fair resource limits
//! - Pluggable syscall handling via traits
//!
//! # Architecture
//!
//! The VM is intentionally game-agnostic. Game-specific logic is injected
//! through the [`SyscallHandler`] trait.
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │        Your Game Logic              │
//! ├─────────────────────────────────────┤
//! │    SyscallHandler implementation    │
//! ├─────────────────────────────────────┤
//! │         RISC-V VM (this crate)      │
//! └─────────────────────────────────────┘
//! ```

pub mod error;
pub mod game;
pub mod isa;
pub mod vm;

pub use error::{AccessType, TrapCause, VmResult};
pub use isa::Instruction;
pub use vm::{Cpu, Memory};

// Re-export key game types at crate root for convenience
pub use game::{
    Command, Coord, GameState, GameSyscallHandler, Map, Player, PlayerId, Tile, TileType,
};

use isa::{decode, execute_rv32i, execute_rv32m};

/// Handler for system calls (ecall instruction).
///
/// Implement this trait to provide game-specific functionality.
/// The VM calls this handler when executing an `ecall` instruction.
pub trait SyscallHandler {
    /// Handle a system call.
    ///
    /// Read arguments from registers a0-a6 (x10-x16).
    /// Write return value to a0 (x10).
    /// The syscall number is in a7 (x17).
    ///
    /// Return `Ok(())` to continue execution, or `Err(TrapCause)` to halt.
    ///
    /// # Errors
    ///
    /// Returns a [`TrapCause`] if the syscall should halt VM execution.
    fn handle(&mut self, cpu: &mut Cpu, memory: &mut Memory) -> VmResult<()>;
}

/// A no-op syscall handler that returns an error on any ecall.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSyscalls;

impl SyscallHandler for NoSyscalls {
    fn handle(&mut self, _cpu: &mut Cpu, _memory: &mut Memory) -> VmResult<()> {
        Err(TrapCause::Ecall)
    }
}

/// Instruction cost model for metering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeteringConfig {
    /// Cost of basic instructions (ADD, AND, shifts, etc.)
    pub base: u32,
    /// Cost of memory operations (load/store)
    pub memory: u32,
    /// Cost of branch instructions
    pub branch: u32,
    /// Cost of multiply instructions
    pub multiply: u32,
    /// Cost of divide/remainder instructions
    pub divide: u32,
    /// Base cost of syscall (ecall)
    pub syscall: u32,
}

impl Default for MeteringConfig {
    fn default() -> Self {
        Self {
            base: 1,
            memory: 2,
            branch: 1,
            multiply: 3,
            divide: 10,
            syscall: 5,
        }
    }
}

impl MeteringConfig {
    /// Get the cost of an instruction.
    #[must_use]
    pub fn cost(&self, inst: &Instruction) -> u32 {
        match inst {
            // Memory operations
            Instruction::Lb { .. }
            | Instruction::Lh { .. }
            | Instruction::Lw { .. }
            | Instruction::Lbu { .. }
            | Instruction::Lhu { .. }
            | Instruction::Sb { .. }
            | Instruction::Sh { .. }
            | Instruction::Sw { .. } => self.memory,

            // Branches
            Instruction::Beq { .. }
            | Instruction::Bne { .. }
            | Instruction::Blt { .. }
            | Instruction::Bge { .. }
            | Instruction::Bltu { .. }
            | Instruction::Bgeu { .. } => self.branch,

            // Multiplication
            Instruction::Mul { .. }
            | Instruction::Mulh { .. }
            | Instruction::Mulhsu { .. }
            | Instruction::Mulhu { .. } => self.multiply,

            // Division
            Instruction::Div { .. }
            | Instruction::Divu { .. }
            | Instruction::Rem { .. }
            | Instruction::Remu { .. } => self.divide,

            // Syscalls
            Instruction::Ecall => self.syscall,

            // Everything else
            _ => self.base,
        }
    }
}

/// Result of executing a single instruction step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    /// Instruction executed successfully. Contains the cost.
    Ok(u32),
    /// A trap occurred (ecall, ebreak, error).
    Trap(TrapCause),
}

/// Result of running the VM for a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnResult {
    /// Turn completed because budget was exhausted.
    ///
    /// The `remaining` field contains any leftover budget that was insufficient
    /// to execute the next instruction.
    BudgetExhausted {
        /// Instructions remaining (less than cost of next instruction).
        remaining: u32,
    },
    /// Turn completed because of a trap (ecall, ebreak, memory fault, etc.).
    Trap(TrapCause),
}

/// The RISC-V virtual machine.
///
/// Generic over the syscall handler type for zero-cost abstraction.
#[derive(Debug)]
pub struct Vm<H> {
    /// CPU state (registers + PC).
    pub cpu: Cpu,
    /// Memory.
    pub memory: Memory,
    /// Syscall handler.
    handler: H,
    /// Metering configuration.
    metering: MeteringConfig,
    /// Total instructions executed (for statistics).
    total_executed: u64,
}

impl<H: SyscallHandler> Vm<H> {
    /// Create a new VM with the given memory size, base address, and syscall handler.
    #[must_use]
    pub fn new(memory_size: u32, memory_base: u32, handler: H) -> Self {
        Self {
            cpu: Cpu::new(),
            memory: Memory::new(memory_size, memory_base),
            handler,
            metering: MeteringConfig::default(),
            total_executed: 0,
        }
    }

    /// Create a new VM with custom metering configuration.
    #[must_use]
    pub fn with_metering(
        memory_size: u32,
        memory_base: u32,
        handler: H,
        metering: MeteringConfig,
    ) -> Self {
        Self {
            cpu: Cpu::new(),
            memory: Memory::new(memory_size, memory_base),
            handler,
            metering,
            total_executed: 0,
        }
    }

    /// Get total instructions executed.
    #[must_use]
    pub fn total_executed(&self) -> u64 {
        self.total_executed
    }

    /// Execute a single instruction step.
    ///
    /// Returns [`StepResult::Ok`] with the instruction cost on success, advancing
    /// the PC to the next instruction. Returns [`StepResult::Trap`] on any trap
    /// condition (ecall, ebreak, memory fault, etc.) without advancing the PC.
    pub fn step(&mut self) -> StepResult {
        // Check PC alignment
        if !self.cpu.pc.is_multiple_of(4) {
            return StepResult::Trap(TrapCause::InstructionMisaligned(self.cpu.pc));
        }

        // Fetch
        let word = match self.memory.fetch(self.cpu.pc) {
            Ok(w) => w,
            Err(e) => return StepResult::Trap(e),
        };

        // Decode
        let inst = match decode(word) {
            Ok(i) => i,
            Err(w) => return StepResult::Trap(TrapCause::InvalidInstruction(w)),
        };

        // Get cost
        let cost = self.metering.cost(&inst);

        // Execute
        let next_pc = match self.execute_instruction(inst) {
            Ok(pc) => pc,
            Err(cause) => return StepResult::Trap(cause),
        };

        // Update state
        self.cpu.pc = next_pc;
        self.total_executed += 1;

        StepResult::Ok(cost)
    }

    /// Execute an instruction, returning the next PC.
    #[inline]
    fn execute_instruction(&mut self, inst: Instruction) -> VmResult<u32> {
        let pc = self.cpu.pc;

        // Handle ecall specially - dispatch to handler
        if matches!(inst, Instruction::Ecall) {
            self.handler.handle(&mut self.cpu, &mut self.memory)?;
            return Ok(pc.wrapping_add(4));
        }

        // Try RV32I first (most common)
        match execute_rv32i(inst, &mut self.cpu, &mut self.memory, pc) {
            Ok(next_pc) => Ok(next_pc),
            Err(TrapCause::InvalidInstruction(_)) => {
                // Try M extension
                execute_rv32m(inst, &mut self.cpu, pc)
            }
            Err(e) => Err(e),
        }
    }

    /// Run the VM for a turn with the given instruction budget.
    pub fn run_turn(&mut self, budget: u32) -> TurnResult {
        let mut remaining = budget;

        loop {
            // Check PC alignment
            if !self.cpu.pc.is_multiple_of(4) {
                return TurnResult::Trap(TrapCause::InstructionMisaligned(self.cpu.pc));
            }

            // Fetch
            let word = match self.memory.fetch(self.cpu.pc) {
                Ok(w) => w,
                Err(e) => return TurnResult::Trap(e),
            };

            // Decode
            let inst = match decode(word) {
                Ok(i) => i,
                Err(w) => return TurnResult::Trap(TrapCause::InvalidInstruction(w)),
            };

            // Check budget before executing
            let cost = self.metering.cost(&inst);
            if remaining < cost {
                return TurnResult::BudgetExhausted { remaining };
            }

            // Execute
            let next_pc = match self.execute_instruction(inst) {
                Ok(pc) => pc,
                Err(cause) => return TurnResult::Trap(cause),
            };

            // Update state
            self.cpu.pc = next_pc;
            self.total_executed += 1;
            remaining -= cost;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_step_add() {
        let mut vm = Vm::new(1024, 0, NoSyscalls);

        // Store: addi x1, x0, 42
        vm.memory.store_u32(0, 0x02A00093).expect("store failed");
        // Store: addi x2, x0, 10
        vm.memory.store_u32(4, 0x00A00113).expect("store failed");
        // Store: add x3, x1, x2
        vm.memory.store_u32(8, 0x002081B3).expect("store failed");

        // Execute three instructions
        assert!(matches!(vm.step(), StepResult::Ok(_)));
        assert_eq!(vm.cpu.read_reg(1), 42);

        assert!(matches!(vm.step(), StepResult::Ok(_)));
        assert_eq!(vm.cpu.read_reg(2), 10);

        assert!(matches!(vm.step(), StepResult::Ok(_)));
        assert_eq!(vm.cpu.read_reg(3), 52);
    }

    #[test]
    fn test_vm_metering() {
        let mut vm = Vm::new(1024, 0, NoSyscalls);

        // Store: addi x1, x0, 42 (cost: 1)
        vm.memory.store_u32(0, 0x02A00093).expect("store failed");

        let result = vm.step();
        assert!(matches!(result, StepResult::Ok(1)));
    }

    #[test]
    fn test_vm_budget_exhaustion() {
        let mut vm = Vm::new(1024, 0, NoSyscalls);

        // Store a bunch of addi instructions
        for i in 0..100 {
            vm.memory
                .store_u32(i * 4, 0x02A00093)
                .expect("store failed");
        }

        // Run with budget of 5
        let result = vm.run_turn(5);
        assert!(matches!(
            result,
            TurnResult::BudgetExhausted { remaining: 0 }
        ));
        assert_eq!(vm.total_executed(), 5);
    }
}
