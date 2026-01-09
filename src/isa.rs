//! RISC-V instruction set definitions.

mod instruction;
mod rv32i;
mod rv32m;

pub use instruction::Instruction;
pub(crate) use rv32i::execute_rv32i;
pub(crate) use rv32m::execute_rv32m;

/// Decode a 32-bit instruction word into an Instruction.
///
/// # Errors
///
/// Returns the original instruction word if decoding fails (invalid opcode or encoding).
pub fn decode(word: u32) -> Result<Instruction, u32> {
    instruction::decode(word)
}
