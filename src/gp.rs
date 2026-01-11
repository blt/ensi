//! Genetic Programming module for evolving game bots.
//!
//! This module provides a complete evolutionary framework for evolving
//! game-playing strategies using genetic programming. Bots are represented
//! as rule-based programs that compile to WASM for execution.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │         Evolution Loop              │
//! ├─────────────────────────────────────┤
//! │  Selection │ Crossover │ Mutation   │
//! ├─────────────────────────────────────┤
//! │         Fitness Evaluation          │
//! ├─────────────────────────────────────┤
//! │    Genome → WASM Compiler           │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use ensi::gp::{EvolutionConfig, evolve};
//!
//! let config = EvolutionConfig::default();
//! let best = evolve(&config)?;
//! let wasm_bytes = best.compile()?;
//! ```

mod compiler;
mod crossover;
mod evolution;
mod fitness;
mod genome;
mod mutation;
mod persistence;
mod selection;

pub use compiler::{compile, CompileError};
pub use crossover::{crossover, CrossoverConfig};
pub use evolution::{evolve, resume, EvolutionConfig, EvolutionError, EvolutionStats};
pub use fitness::{compile_population, evaluate_fitness, evaluate_population, FitnessConfig, FitnessError, FitnessResult};
pub use genome::{Action, Expr, Genome, Rule, TileRef, MAX_RULES, NUM_CONSTANTS};
pub use mutation::{mutate, MutationConfig};
pub use persistence::{
    best_wasm_path, checkpoint_path, ensi_data_dir, evolved_bots_dir, list_evolved_bots,
    load_checkpoint, load_evolved_genome, load_population, save_best_wasm, save_checkpoint,
    save_evolved_bot, save_population, Checkpoint, EvolvedBotMeta,
};
pub use selection::{select_parents, SelectionConfig, SelectionResult, SelectionStats};
