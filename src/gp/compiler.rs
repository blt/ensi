//! Compile genomes to WASM bytecode.
//!
//! This module generates WASM modules from GP genomes using the `wasm-encoder` crate.
//! The generated code iterates over owned tiles and evaluates rules in priority order.

// WASM bytecode generation uses intentional casts for instruction encoding
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]

use crate::gp::genome::{Action, Expr, Genome, TileRef, NUM_CONSTANTS};
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, ImportSection, Instruction,
    MemorySection, MemoryType, Module, TypeSection, ValType,
};

/// Compile a genome to WASM bytecode.
///
/// The generated WASM module:
/// - Imports all `ensi_*` host functions
/// - Exports `run_turn` and `memory`
/// - Implements the rule-based decision logic
///
/// # Errors
///
/// Returns an error if WASM encoding fails.
pub fn compile(genome: &Genome) -> Result<Vec<u8>, CompileError> {
    let mut module = Module::new();

    // Type section: define function signatures
    let mut types = TypeSection::new();

    // Type 0: () -> i32 (simple queries like get_turn, get_player_id)
    types.ty().function([], [ValType::I32]);

    // Type 1: (i32) -> i32 (single param query)
    types.ty().function([ValType::I32], [ValType::I32]);

    // Type 2: (i32, i32) -> i32 (two param query like get_tile)
    types.ty().function([ValType::I32, ValType::I32], [ValType::I32]);

    // Type 3: (i32, i32, i32, i32, i32) -> i32 (move: from_x, from_y, to_x, to_y, count)
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I32, ValType::I32, ValType::I32],
        [ValType::I32],
    );

    // Type 4: (i32, i32, i32) -> i32 (convert: city_x, city_y, count)
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I32],
        [ValType::I32],
    );

    // Type 5: () -> () (yield)
    types.ty().function([], []);

    // Type 6: (i32) -> i32 (run_turn signature: takes fuel_budget, returns status)
    types.ty().function([ValType::I32], [ValType::I32]);

    module.section(&types);

    // Import section: import host functions
    let mut imports = ImportSection::new();

    // Query functions (type 0: () -> i32)
    imports.import("env", "ensi_get_turn", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_player_id", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_my_capital", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_my_food", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_my_population", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_my_army", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_map_width", wasm_encoder::EntityType::Function(0));
    imports.import("env", "ensi_get_map_height", wasm_encoder::EntityType::Function(0));

    // get_tile (type 2: (i32, i32) -> i32)
    imports.import("env", "ensi_get_tile", wasm_encoder::EntityType::Function(2));

    // move (type 3: (i32, i32, i32, i32, i32) -> i32)
    imports.import("env", "ensi_move", wasm_encoder::EntityType::Function(3));

    // convert (type 4: (i32, i32, i32) -> i32)
    imports.import("env", "ensi_convert", wasm_encoder::EntityType::Function(4));

    // move_capital (type 2: (i32, i32) -> i32)
    imports.import("env", "ensi_move_capital", wasm_encoder::EntityType::Function(2));

    // yield (type 5: () -> ())
    imports.import("env", "ensi_yield", wasm_encoder::EntityType::Function(5));

    module.section(&imports);

    // Function section: declare our functions
    let mut functions = FunctionSection::new();
    functions.function(6); // run_turn: type 6 (i32 param, i32 return)
    module.section(&functions);

    // Memory section: 1 page (64KB)
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: Some(16),
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    // Export section
    let mut exports = ExportSection::new();
    exports.export("run_turn", ExportKind::Func, 13); // Function index 13 (after 13 imports)
    exports.export("memory", ExportKind::Memory, 0);
    module.section(&exports);

    // Code section: implement run_turn
    let mut codes = CodeSection::new();
    let mut func = Function::new([
        // Local variables (parameter at index 0, locals start at index 1):
        // 0: fuel_budget (parameter, unused)
        // 1: iter_x
        // 2: iter_y
        // 3: map_width
        // 4: map_height
        // 5: player_id
        // 6: temp
        // 7: temp2 (for nested expressions like Min/Max)
        (1, ValType::I32), // iter_x
        (1, ValType::I32), // iter_y
        (1, ValType::I32), // map_width
        (1, ValType::I32), // map_height
        (1, ValType::I32), // player_id
        (1, ValType::I32), // temp
        (1, ValType::I32), // temp2
    ]);

    // Store constants in memory at address 0
    // Memory layout: [constants: 16 x i32][scratch space]
    for (i, &constant) in genome.constants.iter().enumerate() {
        func.instruction(&Instruction::I32Const(i as i32 * 4)); // Address
        func.instruction(&Instruction::I32Const(constant)); // Value
        func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
    }

    // Get map dimensions and player ID
    func.instruction(&Instruction::Call(6)); // ensi_get_map_width
    func.instruction(&Instruction::LocalSet(3)); // map_width
    func.instruction(&Instruction::Call(7)); // ensi_get_map_height
    func.instruction(&Instruction::LocalSet(4)); // map_height
    func.instruction(&Instruction::Call(1)); // ensi_get_player_id
    func.instruction(&Instruction::LocalSet(5)); // player_id

    // Outer loop: iterate y from 0 to map_height
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2)); // iter_y = 0

    func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty)); // block for break
    func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty)); // outer loop

    // Check if iter_y >= map_height
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1)); // break outer

    // Inner loop: iterate x from 0 to map_width
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(1)); // iter_x = 0

    func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty)); // block for break
    func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty)); // inner loop

    // Check if iter_x >= map_width
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeU);
    func.instruction(&Instruction::BrIf(1)); // break inner

    // Get tile at (iter_x, iter_y)
    func.instruction(&Instruction::LocalGet(1)); // x
    func.instruction(&Instruction::LocalGet(2)); // y
    func.instruction(&Instruction::Call(8)); // ensi_get_tile
    func.instruction(&Instruction::LocalSet(6)); // temp = tile

    // Check if we own this tile: (tile >> 8) & 0xFF == player_id
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32ShrU);
    func.instruction(&Instruction::I32Const(0xFF));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Eq);

    // If we own this tile, evaluate rules
    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));

    // Sort rules by priority and compile each
    let mut sorted_rules: Vec<_> = genome.rules.iter().collect();
    sorted_rules.sort_by_key(|r| r.priority);

    for rule in sorted_rules {
        // Compile condition
        compile_expr(&mut func, &rule.condition, genome);

        // If condition is true, execute action
        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
        compile_action(&mut func, &rule.action, genome);
        func.instruction(&Instruction::End);
    }

    func.instruction(&Instruction::End); // end if (own tile)

    // Increment iter_x
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::Br(0)); // continue inner loop
    func.instruction(&Instruction::End); // end inner loop block
    func.instruction(&Instruction::End); // end inner break block

    // Increment iter_y
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0)); // continue outer loop
    func.instruction(&Instruction::End); // end outer loop block
    func.instruction(&Instruction::End); // end outer break block

    // Return 0
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::End);

    codes.function(&func);
    module.section(&codes);

    Ok(module.finish())
}

/// Compile an expression to WASM instructions.
///
/// The result is left on the stack.
fn compile_expr(func: &mut Function, expr: &Expr, genome: &Genome) {
    match expr {
        Expr::Const(idx) => {
            // Load constant from memory
            let idx = (*idx as usize).min(NUM_CONSTANTS - 1);
            func.instruction(&Instruction::I32Const(idx as i32 * 4));
            func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        }
        Expr::Turn => {
            func.instruction(&Instruction::Call(0)); // ensi_get_turn
        }
        Expr::MyFood => {
            func.instruction(&Instruction::Call(3)); // ensi_get_my_food
        }
        Expr::MyPop => {
            func.instruction(&Instruction::Call(4)); // ensi_get_my_population
        }
        Expr::MyArmy => {
            func.instruction(&Instruction::Call(5)); // ensi_get_my_army
        }
        Expr::MyTerritory => {
            // Not directly available, use 0 as placeholder
            func.instruction(&Instruction::I32Const(0));
        }
        Expr::MapWidth => {
            func.instruction(&Instruction::LocalGet(3));
        }
        Expr::MapHeight => {
            func.instruction(&Instruction::LocalGet(4));
        }
        Expr::IterX => {
            func.instruction(&Instruction::LocalGet(1));
        }
        Expr::IterY => {
            func.instruction(&Instruction::LocalGet(2));
        }
        Expr::TileType(tile_ref) => {
            compile_tile_ref_coords(func, tile_ref, genome);
            func.instruction(&Instruction::Call(8)); // ensi_get_tile
            func.instruction(&Instruction::I32Const(0xFF));
            func.instruction(&Instruction::I32And);
        }
        Expr::TileOwner(tile_ref) => {
            compile_tile_ref_coords(func, tile_ref, genome);
            func.instruction(&Instruction::Call(8)); // ensi_get_tile
            func.instruction(&Instruction::I32Const(8));
            func.instruction(&Instruction::I32ShrU);
            func.instruction(&Instruction::I32Const(0xFF));
            func.instruction(&Instruction::I32And);
        }
        Expr::TileArmy(tile_ref) => {
            compile_tile_ref_coords(func, tile_ref, genome);
            func.instruction(&Instruction::Call(8)); // ensi_get_tile
            func.instruction(&Instruction::I32Const(16));
            func.instruction(&Instruction::I32ShrU);
        }
        Expr::Add(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32Add);
        }
        Expr::Sub(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32Sub);
        }
        Expr::Mul(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32Mul);
        }
        Expr::Div(a, b) => {
            // Protected division: return 0 if divisor is 0
            // Must save both operands to locals since If block has its own stack
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            // Save both to locals
            func.instruction(&Instruction::LocalSet(7)); // temp2 = b
            func.instruction(&Instruction::LocalSet(6)); // temp = a
            // Check if b is 0
            func.instruction(&Instruction::LocalGet(7));
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            func.instruction(&Instruction::I32Const(0)); // return 0 if b == 0
            func.instruction(&Instruction::Else);
            func.instruction(&Instruction::LocalGet(6)); // load a
            func.instruction(&Instruction::LocalGet(7)); // load b
            func.instruction(&Instruction::I32DivS);
            func.instruction(&Instruction::End);
        }
        Expr::Mod(a, b) => {
            // Protected modulo: return 0 if divisor is 0
            // Must save both operands to locals since If block has its own stack
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            // Save both to locals
            func.instruction(&Instruction::LocalSet(7)); // temp2 = b
            func.instruction(&Instruction::LocalSet(6)); // temp = a
            // Check if b is 0
            func.instruction(&Instruction::LocalGet(7));
            func.instruction(&Instruction::I32Eqz);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            func.instruction(&Instruction::I32Const(0)); // return 0 if b == 0
            func.instruction(&Instruction::Else);
            func.instruction(&Instruction::LocalGet(6)); // load a
            func.instruction(&Instruction::LocalGet(7)); // load b
            func.instruction(&Instruction::I32RemS);
            func.instruction(&Instruction::End);
        }
        Expr::Gt(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32GtS);
        }
        Expr::Lt(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32LtS);
        }
        Expr::Eq(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32Eq);
        }
        Expr::And(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32And);
        }
        Expr::Or(a, b) => {
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            func.instruction(&Instruction::I32Or);
        }
        Expr::Min(a, b) => {
            // min(a, b): if a < b then a else b
            // Compute both operands first, then save to locals
            // This avoids conflicts with nested expressions using temp locals
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            // Now save both values (b on top of stack)
            func.instruction(&Instruction::LocalSet(7)); // save b to temp2
            func.instruction(&Instruction::LocalSet(6)); // save a to temp
            // Compare a < b
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::LocalGet(7));
            func.instruction(&Instruction::I32LtS);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            func.instruction(&Instruction::LocalGet(6)); // return a
            func.instruction(&Instruction::Else);
            func.instruction(&Instruction::LocalGet(7)); // return b
            func.instruction(&Instruction::End);
        }
        Expr::Max(a, b) => {
            // max(a, b): if a > b then a else b
            // Compute both operands first, then save to locals
            compile_expr(func, a, genome);
            compile_expr(func, b, genome);
            // Now save both values (b on top of stack)
            func.instruction(&Instruction::LocalSet(7)); // save b to temp2
            func.instruction(&Instruction::LocalSet(6)); // save a to temp
            // Compare a > b
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::LocalGet(7));
            func.instruction(&Instruction::I32GtS);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            func.instruction(&Instruction::LocalGet(6)); // return a
            func.instruction(&Instruction::Else);
            func.instruction(&Instruction::LocalGet(7)); // return b
            func.instruction(&Instruction::End);
        }
        Expr::Not(a) => {
            compile_expr(func, a, genome);
            func.instruction(&Instruction::I32Eqz);
        }
        Expr::Neg(a) => {
            func.instruction(&Instruction::I32Const(0));
            compile_expr(func, a, genome);
            func.instruction(&Instruction::I32Sub);
        }
        Expr::Abs(a) => {
            compile_expr(func, a, genome);
            func.instruction(&Instruction::LocalSet(6)); // save value to temp
            // Check if value < 0
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32LtS);
            func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I32)));
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::I32Sub);
            func.instruction(&Instruction::Else);
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::End);
        }
    }
}

/// Compile tile reference coordinates onto the stack (x, y).
fn compile_tile_ref_coords(func: &mut Function, tile_ref: &TileRef, _genome: &Genome) {
    match tile_ref {
        TileRef::Absolute(x_idx, y_idx) => {
            // Load x from constant pool
            let x_idx = (*x_idx as usize).min(NUM_CONSTANTS - 1);
            func.instruction(&Instruction::I32Const(x_idx as i32 * 4));
            func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
            // Load y from constant pool
            let y_idx = (*y_idx as usize).min(NUM_CONSTANTS - 1);
            func.instruction(&Instruction::I32Const(y_idx as i32 * 4));
            func.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        }
        TileRef::Relative(dx, dy) => {
            // x = iter_x + dx
            func.instruction(&Instruction::LocalGet(1));
            func.instruction(&Instruction::I32Const(i32::from(*dx)));
            func.instruction(&Instruction::I32Add);
            // y = iter_y + dy
            func.instruction(&Instruction::LocalGet(2));
            func.instruction(&Instruction::I32Const(i32::from(*dy)));
            func.instruction(&Instruction::I32Add);
        }
        TileRef::Capital => {
            // Get capital packed coords and unpack
            func.instruction(&Instruction::Call(2)); // ensi_get_my_capital
            func.instruction(&Instruction::LocalTee(6));
            func.instruction(&Instruction::I32Const(16));
            func.instruction(&Instruction::I32ShrU); // x = capital >> 16
            func.instruction(&Instruction::LocalGet(6));
            func.instruction(&Instruction::I32Const(0xFFFF));
            func.instruction(&Instruction::I32And); // y = capital & 0xFFFF
        }
        TileRef::IterTile => {
            func.instruction(&Instruction::LocalGet(1)); // iter_x
            func.instruction(&Instruction::LocalGet(2)); // iter_y
        }
    }
}

/// Compile an action to WASM instructions.
fn compile_action(func: &mut Function, action: &Action, genome: &Genome) {
    match action {
        Action::Move { from, to, count } => {
            // Push from coords
            compile_tile_ref_coords(func, from, genome);
            // Push to coords
            compile_tile_ref_coords(func, to, genome);
            // Push count and ensure it's positive: max(count, 0)
            compile_expr(func, count, genome);
            func.instruction(&Instruction::LocalTee(6)); // save count to temp
            func.instruction(&Instruction::I32Const(0)); // push 0 (false value for select)
            func.instruction(&Instruction::LocalGet(6)); // get count for comparison
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32GtS); // count > 0?
            func.instruction(&Instruction::Select); // if count > 0 then count else 0
            // Call move
            func.instruction(&Instruction::Call(9)); // ensi_move
            func.instruction(&Instruction::Drop);
        }
        Action::Convert { city, count } => {
            compile_tile_ref_coords(func, city, genome);
            // Push count and ensure it's positive: max(count, 0)
            compile_expr(func, count, genome);
            func.instruction(&Instruction::LocalTee(6)); // save count to temp
            func.instruction(&Instruction::I32Const(0)); // push 0 (false value for select)
            func.instruction(&Instruction::LocalGet(6)); // get count for comparison
            func.instruction(&Instruction::I32Const(0));
            func.instruction(&Instruction::I32GtS); // count > 0?
            func.instruction(&Instruction::Select); // if count > 0 then count else 0
            func.instruction(&Instruction::Call(10)); // ensi_convert
            func.instruction(&Instruction::Drop);
        }
        Action::MoveCapital { city } => {
            compile_tile_ref_coords(func, city, genome);
            func.instruction(&Instruction::Call(11)); // ensi_move_capital
            func.instruction(&Instruction::Drop);
        }
        Action::Skip => {
            // Do nothing
        }
    }
}

/// Error during compilation.
#[derive(Debug)]
pub enum CompileError {
    /// WASM encoding error.
    Encoding(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encoding(msg) => write!(f, "WASM encoding error: {msg}"),
        }
    }
}

impl std::error::Error for CompileError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament::PlayerProgram;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use wasmtime::Engine;

    fn create_test_engine() -> Engine {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        Engine::new(&config).expect("create engine")
    }

    #[test]
    fn test_compile_empty_genome() {
        let genome = Genome::default();
        let wasm = compile(&genome).unwrap();
        assert!(!wasm.is_empty());
        // Verify WASM magic number
        assert_eq!(&wasm[0..4], b"\0asm");
    }

    #[test]
    fn test_compile_random_genome() {
        let mut rng = SmallRng::seed_from_u64(42);
        let genome = Genome::random(&mut rng, 5);
        let wasm = compile(&genome).unwrap();
        assert!(!wasm.is_empty());
        assert_eq!(&wasm[0..4], b"\0asm");
    }

    #[test]
    fn test_compile_deterministic() {
        let mut rng = SmallRng::seed_from_u64(123);
        let genome = Genome::random(&mut rng, 3);

        let wasm1 = compile(&genome).unwrap();
        let wasm2 = compile(&genome).unwrap();

        assert_eq!(wasm1, wasm2);
    }

    #[test]
    fn test_compile_and_load_empty() {
        let engine = create_test_engine();
        let genome = Genome::default();
        let wasm_bytes = compile(&genome).expect("compile should succeed");
        let program = PlayerProgram::new(wasm_bytes);
        program.compile(&engine).expect("empty genome should load");
    }

    #[test]
    fn test_compile_and_load() {
        let engine = create_test_engine();
        let mut rng = SmallRng::seed_from_u64(42);

        for i in 0..10 {
            let genome = Genome::random(&mut rng, 5);
            let wasm_bytes = compile(&genome).expect("compile should succeed");

            // Dump failing WASM for debugging
            let program = PlayerProgram::new(wasm_bytes.clone());
            if let Err(e) = program.compile(&engine) {
                // Write to temp file for analysis
                let path = format!("/tmp/failing_genome_{i}.wasm");
                std::fs::write(&path, &wasm_bytes).ok();
                panic!("genome {i} failed to load: {e}\nWASM written to {path}\nGenome: {genome:?}");
            }
        }
    }
}
