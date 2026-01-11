//! Fitness evaluation for genetic programming.
//!
//! Fitness is determined by running tournaments between evolved bots and
//! measuring their win rate. The tournament infrastructure from the main
//! crate is reused for efficiency.

// Fitness evaluation uses intentional casts and naming conventions
#![allow(clippy::cast_precision_loss, clippy::wrong_self_convention)]

use crate::gp::compiler::{compile, CompileError};
use crate::gp::genome::Genome;
use crate::tournament::{
    run_game_with_modules, CompiledProgram, PlayerProgram, TournamentConfig,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use wasmtime::Engine;

/// Configuration for fitness evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FitnessConfig {
    /// Number of games to run for each fitness evaluation.
    pub games_per_eval: usize,
    /// Starting seed for game RNG.
    pub base_seed: u64,
    /// Maximum turns per game.
    pub max_turns: u32,
    /// Fuel budget per turn.
    pub fuel_budget: u64,
    /// Map width.
    pub map_width: u16,
    /// Map height.
    pub map_height: u16,
}

impl Default for FitnessConfig {
    fn default() -> Self {
        Self {
            games_per_eval: 20,
            base_seed: 12345,
            max_turns: 500,
            fuel_budget: 50_000,
            map_width: 32,
            map_height: 32,
        }
    }
}

impl FitnessConfig {
    /// Convert to tournament config.
    #[must_use]
    fn to_tournament_config(&self) -> TournamentConfig {
        TournamentConfig {
            max_turns: self.max_turns,
            fuel_budget: self.fuel_budget,
            map_width: self.map_width,
            map_height: self.map_height,
        }
    }
}

/// Result of a fitness evaluation.
#[derive(Debug, Clone, Copy)]
pub struct FitnessResult {
    /// Win rate (0.0 to 1.0).
    pub win_rate: f64,
    /// Average score across all games.
    pub avg_score: f64,
    /// Number of games played.
    pub games_played: usize,
}

/// Error during fitness evaluation.
#[derive(Debug)]
pub enum FitnessError {
    /// Genome failed to compile.
    CompileError(CompileError),
    /// WASM module failed to load.
    WasmError(String),
}

impl std::fmt::Display for FitnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CompileError(e) => write!(f, "compile error: {e}"),
            Self::WasmError(e) => write!(f, "WASM error: {e}"),
        }
    }
}

impl std::error::Error for FitnessError {}

impl From<CompileError> for FitnessError {
    fn from(e: CompileError) -> Self {
        Self::CompileError(e)
    }
}

/// Evaluate fitness of a single genome against a set of opponents.
///
/// # Errors
///
/// Returns an error if the genome fails to compile or load.
pub fn evaluate_fitness(
    genome: &Genome,
    opponents: &[CompiledProgram],
    engine: &Engine,
    config: &FitnessConfig,
) -> Result<FitnessResult, FitnessError> {
    // Compile genome to WASM
    let wasm_bytes = compile(genome)?;

    // Load as a compiled program
    let program = PlayerProgram::new(wasm_bytes);
    let compiled = program
        .compile(engine)
        .map_err(|e| FitnessError::WasmError(e.to_string()))?;

    // Build player list: our bot + opponents
    let mut players: Vec<CompiledProgram> = vec![compiled];
    players.extend(opponents.iter().cloned());

    // Need at least 2 players
    if players.len() < 2 {
        // Add a copy of ourselves as opponent if needed
        let wasm_bytes2 = compile(genome)?;
        let program2 = PlayerProgram::new(wasm_bytes2);
        let compiled2 = program2
            .compile(engine)
            .map_err(|e| FitnessError::WasmError(e.to_string()))?;
        players.push(compiled2);
    }

    let tournament_config = config.to_tournament_config();

    // Run games
    let mut wins = 0;
    let mut total_score = 0.0;

    for game_idx in 0..config.games_per_eval {
        let seed = config.base_seed.wrapping_add(game_idx as u64);

        // Run the game
        let result = run_game_with_modules(seed, engine, &players, &tournament_config);

        if let Ok(result) = result {
            // Check if we won (player ID 1, since IDs are 1-indexed)
            if result.winner == Some(1) {
                wins += 1;
            }

            // Accumulate score
            if let Some(&score) = result.scores.first() {
                total_score += score;
            }
        }
    }

    let games = config.games_per_eval;
    Ok(FitnessResult {
        win_rate: f64::from(wins) / games as f64,
        avg_score: total_score / games as f64,
        games_played: games,
    })
}

/// Evaluate fitness of all genomes in a population in parallel.
///
/// Returns fitness results in the same order as the input population.
/// Genomes that fail to compile are assigned zero fitness.
#[must_use] 
pub fn evaluate_population(
    population: &[Genome],
    opponents: &[CompiledProgram],
    engine: &Engine,
    config: &FitnessConfig,
) -> Vec<f64> {
    population
        .par_iter()
        .map(|genome| {
            match evaluate_fitness(genome, opponents, engine, config) {
                Ok(result) => {
                    // Fitness = win rate (primary) + normalized score (secondary)
                    result.win_rate * 0.8 + (result.avg_score / 10000.0).min(0.2)
                }
                Err(_) => 0.0, // Failed genomes get zero fitness
            }
        })
        .collect()
}

/// Compile a population of genomes for use as opponents.
///
/// Returns compiled programs for genomes that successfully compile.
/// Failed compilations are skipped.
#[must_use] 
pub fn compile_population(
    population: &[Genome],
    engine: &Engine,
) -> Vec<CompiledProgram> {
    population
        .par_iter()
        .filter_map(|genome| {
            let wasm_bytes = compile(genome).ok()?;
            let program = PlayerProgram::new(wasm_bytes);
            program.compile(engine).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    #[test]
    fn test_compile_random_genome() {
        let mut rng = SmallRng::seed_from_u64(42);
        let genome = Genome::random(&mut rng, 5);

        let wasm = compile(&genome).expect("compile");
        assert!(!wasm.is_empty());
    }

    #[test]
    fn test_fitness_config_default() {
        let config = FitnessConfig::default();
        assert!(config.games_per_eval > 0);
        assert!(config.base_seed > 0);
    }
}
