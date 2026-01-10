//! Cranelift code generation for RISC-V basic blocks.
//!
//! Translates RISC-V instructions to Cranelift IR and compiles to native code.

use super::blocks::{BasicBlock, Terminator};
use crate::isa::Instruction;
use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;

/// Error during JIT compilation.
#[derive(Debug, Clone)]
pub struct CodegenError {
    /// Description of the error.
    pub reason: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JIT codegen error: {}", self.reason)
    }
}

impl std::error::Error for CodegenError {}

/// Packed result from compiled block.
/// High 32 bits: tag (0=Continue, 1=Syscall, 2=Trap, 3=BudgetExhausted)
/// Low 32 bits: data (next_pc, trap code, remaining budget, etc.)
#[allow(dead_code)]
mod result_tags {
    pub(super) const CONTINUE: u64 = 0;
    pub(super) const SYSCALL: u64 = 1;
    pub(super) const TRAP: u64 = 2;
    pub(super) const BUDGET_EXHAUSTED: u64 = 3;
}

/// JIT code generator.
pub struct JitCodegen {
    /// Function builder context (reused across compilations).
    builder_ctx: FunctionBuilderContext,
    /// Codegen context.
    ctx: Context,
    /// Counter for unique function names.
    func_counter: u64,
    /// Cache of compiled function IDs by start PC.
    #[allow(dead_code)]
    func_ids: HashMap<u32, FuncId>,
}

impl std::fmt::Debug for JitCodegen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitCodegen")
            .field("func_counter", &self.func_counter)
            .finish()
    }
}

impl JitCodegen {
    /// Create a new code generator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            builder_ctx: FunctionBuilderContext::new(),
            ctx: Context::new(),
            func_counter: 0,
            func_ids: HashMap::new(),
        }
    }

    /// Compile a basic block to native code.
    ///
    /// Returns a pointer to the compiled function.
    pub fn compile_block(
        &mut self,
        block: &BasicBlock,
        module: &mut JITModule,
    ) -> Result<*const u8, CodegenError> {
        let ptr_type = module.target_config().pointer_type();

        // Function signature: fn(cpu: *mut Cpu, memory_data: *mut u8, memory_base: u32, memory_size: u32) -> u64
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(ptr_type)); // cpu
        sig.params.push(AbiParam::new(ptr_type)); // memory data pointer
        sig.params.push(AbiParam::new(types::I32)); // memory base
        sig.params.push(AbiParam::new(types::I32)); // memory size
        sig.returns.push(AbiParam::new(types::I64)); // packed result

        let func_name = format!("block_{:08x}_{}", block.start_pc, self.func_counter);
        self.func_counter += 1;

        let func_id = module
            .declare_function(&func_name, Linkage::Local, &sig)
            .map_err(|e| CodegenError {
                reason: format!("failed to declare function: {e}"),
            })?;

        self.ctx.func = Function::with_name_signature(
            UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // Build function body
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
            generate_block_body(&mut builder, block, ptr_type)?;
            builder.finalize();
        }

        // Compile
        module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| CodegenError {
                reason: format!("failed to define function: {e}"),
            })?;

        module.clear_context(&mut self.ctx);
        module.finalize_definitions().map_err(|e| CodegenError {
            reason: format!("failed to finalize: {e}"),
        })?;

        Ok(module.get_finalized_function(func_id))
    }
}

impl Default for JitCodegen {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate the function body for a basic block.
fn generate_block_body(
    builder: &mut FunctionBuilder,
    block: &BasicBlock,
    ptr_type: types::Type,
) -> Result<(), CodegenError> {
    // Create entry block
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);

    // Get parameters
    let cpu_ptr = builder.block_params(entry_block)[0];
    let mem_data = builder.block_params(entry_block)[1];
    let mem_base = builder.block_params(entry_block)[2];
    let mem_size = builder.block_params(entry_block)[3];

    // Create variable for tracking next PC
    let next_pc_var = Variable::from_u32(0);
    builder.declare_var(next_pc_var, types::I32);
    let initial_pc = builder.ins().iconst(types::I32, i64::from(block.start_pc));
    builder.def_var(next_pc_var, initial_pc);

    // Generate code for each instruction (except the terminator which needs special handling)
    let non_terminator_count = if block.instructions.is_empty() {
        0
    } else {
        block.instructions.len() - 1
    };

    for decoded in &block.instructions[..non_terminator_count] {
        translate_instruction(
            builder,
            &decoded.inst,
            decoded.pc,
            cpu_ptr,
            mem_data,
            mem_base,
            mem_size,
            ptr_type,
            next_pc_var,
        )?;
    }

    // Handle terminator instruction
    if let Some(last) = block.instructions.last() {
        let result = translate_terminator(
            builder,
            &last.inst,
            last.pc,
            cpu_ptr,
            mem_data,
            mem_base,
            mem_size,
            ptr_type,
            next_pc_var,
            &block.terminator,
        )?;
        builder.ins().return_(&[result]);
    } else {
        // Empty block - return continue with same PC
        let result = pack_continue(builder, block.start_pc);
        builder.ins().return_(&[result]);
    }

    Ok(())
}

/// Translate a non-terminator instruction.
#[allow(clippy::too_many_arguments)]
fn translate_instruction(
    builder: &mut FunctionBuilder,
    inst: &Instruction,
    pc: u32,
    cpu_ptr: cranelift_codegen::ir::Value,
    _mem_data: cranelift_codegen::ir::Value,
    _mem_base: cranelift_codegen::ir::Value,
    _mem_size: cranelift_codegen::ir::Value,
    ptr_type: types::Type,
    next_pc_var: Variable,
) -> Result<(), CodegenError> {
    let next_pc = pc.wrapping_add(4);

    match inst {
        // R-type arithmetic
        Instruction::Add { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().iadd(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Sub { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().isub(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::And { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().band(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Or { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().bor(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Xor { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().bxor(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Sll { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                // Mask shift amount to 5 bits (0-31)
                let mask = builder.ins().iconst(types::I32, 0x1F);
                let shamt = builder.ins().band(v2, mask);
                let result = builder.ins().ishl(v1, shamt);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Srl { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let mask = builder.ins().iconst(types::I32, 0x1F);
                let shamt = builder.ins().band(v2, mask);
                let result = builder.ins().ushr(v1, shamt);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Sra { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let mask = builder.ins().iconst(types::I32, 0x1F);
                let shamt = builder.ins().band(v2, mask);
                let result = builder.ins().sshr(v1, shamt);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Slt { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let cmp = builder.ins().icmp(IntCC::SignedLessThan, v1, v2);
                let result = builder.ins().uextend(types::I32, cmp);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Sltu { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let cmp = builder.ins().icmp(IntCC::UnsignedLessThan, v1, v2);
                let result = builder.ins().uextend(types::I32, cmp);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // I-type immediate
        Instruction::Addi { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let result = builder.ins().iadd(v1, imm_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Andi { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let result = builder.ins().band(v1, imm_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Ori { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let result = builder.ins().bor(v1, imm_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Xori { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let result = builder.ins().bxor(v1, imm_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Slti { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let cmp = builder.ins().icmp(IntCC::SignedLessThan, v1, imm_val);
                let result = builder.ins().uextend(types::I32, cmp);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Sltiu { rd, rs1, imm } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let cmp = builder.ins().icmp(IntCC::UnsignedLessThan, v1, imm_val);
                let result = builder.ins().uextend(types::I32, cmp);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Slli { rd, rs1, shamt } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let shamt_val = builder.ins().iconst(types::I32, i64::from(*shamt));
                let result = builder.ins().ishl(v1, shamt_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Srli { rd, rs1, shamt } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let shamt_val = builder.ins().iconst(types::I32, i64::from(*shamt));
                let result = builder.ins().ushr(v1, shamt_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Srai { rd, rs1, shamt } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let shamt_val = builder.ins().iconst(types::I32, i64::from(*shamt));
                let result = builder.ins().sshr(v1, shamt_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Upper immediate
        Instruction::Lui { rd, imm } => {
            if *rd != 0 {
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                store_reg(builder, cpu_ptr, *rd, imm_val, ptr_type);
            }
        }
        Instruction::Auipc { rd, imm } => {
            if *rd != 0 {
                let pc_val = builder.ins().iconst(types::I32, i64::from(pc));
                let imm_val = builder.ins().iconst(types::I32, i64::from(*imm));
                let result = builder.ins().iadd(pc_val, imm_val);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // M extension - multiply
        Instruction::Mul { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().imul(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Mulh { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                // Sign extend to 64-bit, multiply, take upper 32 bits
                let v1_ext = builder.ins().sextend(types::I64, v1);
                let v2_ext = builder.ins().sextend(types::I64, v2);
                let product = builder.ins().imul(v1_ext, v2_ext);
                let shift = builder.ins().iconst(types::I64, 32);
                let high = builder.ins().sshr(product, shift);
                let result = builder.ins().ireduce(types::I32, high);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Mulhu { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                // Zero extend to 64-bit, multiply, take upper 32 bits
                let v1_ext = builder.ins().uextend(types::I64, v1);
                let v2_ext = builder.ins().uextend(types::I64, v2);
                let product = builder.ins().imul(v1_ext, v2_ext);
                let shift = builder.ins().iconst(types::I64, 32);
                let high = builder.ins().ushr(product, shift);
                let result = builder.ins().ireduce(types::I32, high);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Mulhsu { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                // rs1 signed, rs2 unsigned
                let v1_ext = builder.ins().sextend(types::I64, v1);
                let v2_ext = builder.ins().uextend(types::I64, v2);
                let product = builder.ins().imul(v1_ext, v2_ext);
                let shift = builder.ins().iconst(types::I64, 32);
                let high = builder.ins().sshr(product, shift);
                let result = builder.ins().ireduce(types::I32, high);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // M extension - divide
        Instruction::Div { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().sdiv(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Divu { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().udiv(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Rem { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().srem(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }
        Instruction::Remu { rd, rs1, rs2 } => {
            if *rd != 0 {
                let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                let result = builder.ins().urem(v1, v2);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Fence is a no-op for single-threaded execution
        Instruction::Fence => {}

        // Load byte (sign extend)
        Instruction::Lb { rd, rs1, imm } => {
            if *rd != 0 {
                let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
                let offset = compute_offset(builder, addr, _mem_base);
                let offset_ext = builder.ins().uextend(ptr_type, offset);
                let host_addr = builder.ins().iadd(_mem_data, offset_ext);
                let byte = builder.ins().load(types::I8, MemFlags::trusted(), host_addr, 0);
                let result = builder.ins().sextend(types::I32, byte);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Load halfword (sign extend)
        Instruction::Lh { rd, rs1, imm } => {
            if *rd != 0 {
                let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
                let offset = compute_offset(builder, addr, _mem_base);
                let offset_ext = builder.ins().uextend(ptr_type, offset);
                let host_addr = builder.ins().iadd(_mem_data, offset_ext);
                let half = builder.ins().load(types::I16, MemFlags::trusted(), host_addr, 0);
                let result = builder.ins().sextend(types::I32, half);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Load word
        Instruction::Lw { rd, rs1, imm } => {
            if *rd != 0 {
                let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
                let offset = compute_offset(builder, addr, _mem_base);
                let offset_ext = builder.ins().uextend(ptr_type, offset);
                let host_addr = builder.ins().iadd(_mem_data, offset_ext);
                let result = builder.ins().load(types::I32, MemFlags::trusted(), host_addr, 0);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Load byte unsigned
        Instruction::Lbu { rd, rs1, imm } => {
            if *rd != 0 {
                let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
                let offset = compute_offset(builder, addr, _mem_base);
                let offset_ext = builder.ins().uextend(ptr_type, offset);
                let host_addr = builder.ins().iadd(_mem_data, offset_ext);
                let byte = builder.ins().load(types::I8, MemFlags::trusted(), host_addr, 0);
                let result = builder.ins().uextend(types::I32, byte);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Load halfword unsigned
        Instruction::Lhu { rd, rs1, imm } => {
            if *rd != 0 {
                let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
                let offset = compute_offset(builder, addr, _mem_base);
                let offset_ext = builder.ins().uextend(ptr_type, offset);
                let host_addr = builder.ins().iadd(_mem_data, offset_ext);
                let half = builder.ins().load(types::I16, MemFlags::trusted(), host_addr, 0);
                let result = builder.ins().uextend(types::I32, half);
                store_reg(builder, cpu_ptr, *rd, result, ptr_type);
            }
        }

        // Store byte
        Instruction::Sb { rs1, rs2, imm } => {
            let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
            let offset = compute_offset(builder, addr, _mem_base);
            let offset_ext = builder.ins().uextend(ptr_type, offset);
            let host_addr = builder.ins().iadd(_mem_data, offset_ext);
            let value = load_reg(builder, cpu_ptr, *rs2, ptr_type);
            let byte = builder.ins().ireduce(types::I8, value);
            builder.ins().store(MemFlags::trusted(), byte, host_addr, 0);
        }

        // Store halfword
        Instruction::Sh { rs1, rs2, imm } => {
            let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
            let offset = compute_offset(builder, addr, _mem_base);
            let offset_ext = builder.ins().uextend(ptr_type, offset);
            let host_addr = builder.ins().iadd(_mem_data, offset_ext);
            let value = load_reg(builder, cpu_ptr, *rs2, ptr_type);
            let half = builder.ins().ireduce(types::I16, value);
            builder.ins().store(MemFlags::trusted(), half, host_addr, 0);
        }

        // Store word
        Instruction::Sw { rs1, rs2, imm } => {
            let addr = compute_addr(builder, cpu_ptr, *rs1, *imm, ptr_type);
            let offset = compute_offset(builder, addr, _mem_base);
            let offset_ext = builder.ins().uextend(ptr_type, offset);
            let host_addr = builder.ins().iadd(_mem_data, offset_ext);
            let value = load_reg(builder, cpu_ptr, *rs2, ptr_type);
            builder.ins().store(MemFlags::trusted(), value, host_addr, 0);
        }

        // These are terminators, should not reach here
        Instruction::Beq { .. }
        | Instruction::Bne { .. }
        | Instruction::Blt { .. }
        | Instruction::Bge { .. }
        | Instruction::Bltu { .. }
        | Instruction::Bgeu { .. }
        | Instruction::Jal { .. }
        | Instruction::Jalr { .. }
        | Instruction::Ecall
        | Instruction::Ebreak => {
            return Err(CodegenError {
                reason: format!("terminator in non-terminator position: {inst:?}"),
            });
        }
    }

    // Update next_pc variable
    let new_pc = builder.ins().iconst(types::I32, i64::from(next_pc));
    builder.def_var(next_pc_var, new_pc);

    Ok(())
}

/// Translate a terminator instruction and return the packed result.
#[allow(clippy::too_many_arguments)]
fn translate_terminator(
    builder: &mut FunctionBuilder,
    inst: &Instruction,
    pc: u32,
    cpu_ptr: cranelift_codegen::ir::Value,
    _mem_data: cranelift_codegen::ir::Value,
    _mem_base: cranelift_codegen::ir::Value,
    _mem_size: cranelift_codegen::ir::Value,
    ptr_type: types::Type,
    _next_pc_var: Variable,
    terminator: &Terminator,
) -> Result<cranelift_codegen::ir::Value, CodegenError> {
    match terminator {
        Terminator::Jump => {
            match inst {
                Instruction::Jal { rd, imm } => {
                    // rd = pc + 4, pc = pc + imm
                    if *rd != 0 {
                        let link = builder.ins().iconst(types::I32, i64::from(pc.wrapping_add(4)));
                        store_reg(builder, cpu_ptr, *rd, link, ptr_type);
                    }
                    let target = pc.wrapping_add(*imm as u32);
                    Ok(pack_continue(builder, target))
                }
                Instruction::Jalr { rd, rs1, imm } => {
                    // rd = pc + 4, pc = (rs1 + imm) & ~1
                    if *rd != 0 {
                        let link = builder.ins().iconst(types::I32, i64::from(pc.wrapping_add(4)));
                        store_reg(builder, cpu_ptr, *rd, link, ptr_type);
                    }
                    let base = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let offset = builder.ins().iconst(types::I32, i64::from(*imm));
                    let sum = builder.ins().iadd(base, offset);
                    let mask = builder.ins().iconst(types::I32, !1i64);
                    let target = builder.ins().band(sum, mask);
                    Ok(pack_continue_dynamic(builder, target))
                }
                _ => Err(CodegenError {
                    reason: format!("unexpected instruction for Jump terminator: {inst:?}"),
                }),
            }
        }

        Terminator::Branch => {
            // For branches, we need to compute both possible targets
            // and return the appropriate one
            let next_pc = pc.wrapping_add(4);

            match inst {
                Instruction::Beq { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::Equal, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                Instruction::Bne { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::NotEqual, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                Instruction::Blt { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::SignedLessThan, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                Instruction::Bge { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                Instruction::Bltu { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::UnsignedLessThan, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                Instruction::Bgeu { rs1, rs2, imm } => {
                    let v1 = load_reg(builder, cpu_ptr, *rs1, ptr_type);
                    let v2 = load_reg(builder, cpu_ptr, *rs2, ptr_type);
                    let cond = builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, v1, v2);
                    let taken = pc.wrapping_add(*imm as u32);
                    Ok(pack_branch_result(builder, cond, taken, next_pc))
                }
                _ => Err(CodegenError {
                    reason: format!("unexpected instruction for Branch terminator: {inst:?}"),
                }),
            }
        }

        Terminator::Syscall => {
            // Return syscall marker - caller will handle the actual syscall
            Ok(pack_syscall(builder, pc))
        }

        Terminator::Break => {
            // Return trap marker
            Ok(pack_trap(builder, 1)) // EBREAK trap code
        }

        Terminator::FallThrough => {
            // Block ended due to size limit - continue at next PC
            let next_pc = pc.wrapping_add(4);
            Ok(pack_continue(builder, next_pc))
        }

        Terminator::Invalid => {
            // Invalid instruction - return trap
            Ok(pack_trap(builder, 2)) // Invalid instruction trap code
        }
    }
}

/// Pack a Continue result with a static PC.
fn pack_continue(builder: &mut FunctionBuilder, next_pc: u32) -> cranelift_codegen::ir::Value {
    // tag=0 (Continue), data=next_pc
    builder.ins().iconst(types::I64, i64::from(next_pc))
}

/// Pack a Continue result with a dynamic PC.
fn pack_continue_dynamic(
    builder: &mut FunctionBuilder,
    next_pc: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    // tag=0 (Continue), data=next_pc
    builder.ins().uextend(types::I64, next_pc)
}

/// Pack a branch result - select between two PCs based on condition.
fn pack_branch_result(
    builder: &mut FunctionBuilder,
    cond: cranelift_codegen::ir::Value,
    taken_pc: u32,
    not_taken_pc: u32,
) -> cranelift_codegen::ir::Value {
    let taken_val = builder.ins().iconst(types::I64, i64::from(taken_pc));
    let not_taken_val = builder.ins().iconst(types::I64, i64::from(not_taken_pc));
    builder.ins().select(cond, taken_val, not_taken_val)
}

/// Pack a Syscall result.
fn pack_syscall(builder: &mut FunctionBuilder, pc: u32) -> cranelift_codegen::ir::Value {
    // tag=1 (Syscall) in high bits, pc in low bits
    let tag = result_tags::SYSCALL << 32;
    let result = tag | u64::from(pc);
    builder.ins().iconst(types::I64, result as i64)
}

/// Pack a Trap result.
fn pack_trap(builder: &mut FunctionBuilder, trap_code: u32) -> cranelift_codegen::ir::Value {
    // tag=2 (Trap) in high bits, trap_code in low bits
    let tag = result_tags::TRAP << 32;
    let result = tag | u64::from(trap_code);
    builder.ins().iconst(types::I64, result as i64)
}

/// Load a register value from the CPU struct.
fn load_reg(
    builder: &mut FunctionBuilder,
    cpu_ptr: cranelift_codegen::ir::Value,
    reg: u8,
    ptr_type: types::Type,
) -> cranelift_codegen::ir::Value {
    if reg == 0 {
        return builder.ins().iconst(types::I32, 0);
    }

    let offset = i32::from(reg) * 4;
    let offset_val = builder.ins().iconst(ptr_type, i64::from(offset));
    let addr = builder.ins().iadd(cpu_ptr, offset_val);
    builder.ins().load(types::I32, MemFlags::trusted(), addr, 0)
}

/// Store a value to a register in the CPU struct.
fn store_reg(
    builder: &mut FunctionBuilder,
    cpu_ptr: cranelift_codegen::ir::Value,
    reg: u8,
    value: cranelift_codegen::ir::Value,
    ptr_type: types::Type,
) {
    if reg == 0 {
        return;
    }

    let offset = i32::from(reg) * 4;
    let offset_val = builder.ins().iconst(ptr_type, i64::from(offset));
    let addr = builder.ins().iadd(cpu_ptr, offset_val);
    builder.ins().store(MemFlags::trusted(), value, addr, 0);
}

/// Compute effective address for load/store: rs1 + imm
fn compute_addr(
    builder: &mut FunctionBuilder,
    cpu_ptr: cranelift_codegen::ir::Value,
    rs1: u8,
    imm: i32,
    ptr_type: types::Type,
) -> cranelift_codegen::ir::Value {
    let base = load_reg(builder, cpu_ptr, rs1, ptr_type);
    let offset = builder.ins().iconst(types::I32, i64::from(imm));
    builder.ins().iadd(base, offset)
}

/// Compute offset into memory data: addr - mem_base
fn compute_offset(
    builder: &mut FunctionBuilder,
    addr: cranelift_codegen::ir::Value,
    mem_base: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    builder.ins().isub(addr, mem_base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegen_creation() {
        let codegen = JitCodegen::new();
        assert_eq!(codegen.func_counter, 0);
    }
}
