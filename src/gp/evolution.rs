//! Main evolution loop for genetic programming.
//!
//! This module orchestrates the evolutionary process: initialization,
//! fitness evaluation, selection, crossover, mutation, and persistence.

// Evolution uses print_stderr for progress output and intentional casts
#![allow(
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

use crate::gp::crossover::{crossover, CrossoverConfig};
use crate::gp::fitness::{compile_population, FitnessConfig};
use crate::gp::genome::Genome;
use crate::gp::mutation::{mutate, MutationConfig};
use crate::gp::persistence::{
    checkpoint_path, load_checkpoint, save_best_wasm, save_checkpoint, save_evolved_bot,
    best_wasm_path, Checkpoint,
};
use crate::gp::selection::{select_parents, SelectionConfig, SelectionStats};
use crate::tournament::CompiledProgram;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use wasmtime::Engine;

/// Configuration for the evolution process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfig {
    /// Population size.
    pub population_size: usize,
    /// Number of generations to run.
    pub generations: usize,
    /// Number of rules in initial random genomes.
    pub initial_rules: usize,
    /// RNG seed for reproducibility.
    pub seed: u64,
    /// Fitness evaluation configuration.
    pub fitness: FitnessConfig,
    /// Selection configuration.
    pub selection: SelectionConfig,
    /// Crossover configuration.
    pub crossover: CrossoverConfig,
    /// Mutation configuration.
    pub mutation: MutationConfig,
    /// Output directory for checkpoints.
    pub output_dir: PathBuf,
    /// How often to save checkpoints (every N generations).
    pub checkpoint_interval: usize,
    /// Whether to print progress to stderr.
    pub verbose: bool,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            population_size: 100,
            generations: 1000,
            initial_rules: 10,
            seed: 42,
            fitness: FitnessConfig::default(),
            selection: SelectionConfig::default(),
            crossover: CrossoverConfig::default(),
            mutation: MutationConfig::default(),
            output_dir: PathBuf::from("evolution"),
            checkpoint_interval: 100,
            verbose: true,
        }
    }
}

/// Statistics for a single generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerationStats {
    /// Generation number.
    pub generation: usize,
    /// Best fitness in this generation.
    pub best_fitness: f64,
    /// Mean fitness.
    pub mean_fitness: f64,
    /// Fitness standard deviation.
    pub fitness_std: f64,
}

/// Overall statistics from an evolution run.
#[derive(Debug, Clone)]
pub struct EvolutionStats {
    /// Statistics per generation.
    pub generations: Vec<GenerationStats>,
    /// Best fitness achieved.
    pub best_fitness: f64,
    /// Generation where best fitness was achieved.
    pub best_generation: usize,
    /// Total time in seconds.
    pub elapsed_seconds: f64,
}

/// Run the evolution process.
///
/// Runs continuous tournaments where bots in the population compete against
/// each other. Winners reproduce, losers are replaced. Continues until the
/// generation limit is reached (or forever if generations = 0).
///
/// # Errors
///
/// Returns an error if file I/O fails or WASM engine creation fails.
pub fn evolve(config: &EvolutionConfig) -> Result<(Genome, EvolutionStats), EvolutionError> {
    let start_time = std::time::Instant::now();

    // Create output directory
    std::fs::create_dir_all(&config.output_dir)?;

    // Create WASM engine
    let engine = create_engine()?;

    // Initialize RNG
    let mut rng = SmallRng::seed_from_u64(config.seed);

    // Initialize population
    let mut population: Vec<Genome> = (0..config.population_size)
        .map(|_| Genome::random(&mut rng, config.initial_rules))
        .collect();

    // Track best genome
    let mut best_genome = population[0].clone();
    let mut best_fitness = 0.0;
    let mut best_generation = 0;

    // Generation statistics
    let mut gen_stats = Vec::new();

    // Main evolution loop - runs forever if generations == 0
    let mut generation = 0usize;
    loop {
        if config.generations > 0 && generation >= config.generations {
            break;
        }

        // Include best genome in population for fair comparison
        // This ensures we compare against the same opponents each generation
        let mut eval_population = population.clone();
        let best_genome_idx = if generation > 0 {
            eval_population.push(best_genome.clone());
            Some(eval_population.len() - 1)
        } else {
            None
        };

        // Compile entire population for tournament play
        let compiled_population = compile_population(&eval_population, &engine);

        // Skip if we couldn't compile enough bots
        if compiled_population.len() < 2 {
            eprintln!("Warning: Not enough bots compiled, regenerating population");
            population = (0..config.population_size)
                .map(|_| Genome::random(&mut rng, config.initial_rules))
                .collect();
            continue;
        }

        // Run tournament: each bot plays against all others
        let fitness = run_tournament(&compiled_population, &engine, &config.fitness);

        // Get fitness values for original population (excluding best_genome if added)
        let pop_fitness: Vec<f64> = fitness.iter().take(population.len()).copied().collect();

        // Calculate statistics (on original population only)
        let stats = SelectionStats::from_fitness(&pop_fitness);
        let gen_stat = GenerationStats {
            generation,
            best_fitness: stats.best_fitness,
            mean_fitness: stats.mean_fitness,
            fitness_std: stats.fitness_std,
        };
        gen_stats.push(gen_stat);

        // Update best genome: compare against same-generation fitness
        // Find best from current generation's population
        if let Some((idx, &fit)) = pop_fitness
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        {
            // Get previous best's fitness in this same tournament (fair comparison)
            let prev_best_fit = best_genome_idx
                .and_then(|i| fitness.get(i).copied())
                .unwrap_or(0.0);

            if fit > prev_best_fit {
                best_fitness = fit;
                best_genome = population[idx].clone();
                best_generation = generation;

                // Save to permanent storage (~/.ensi/evolved/)
                match save_evolved_bot(&best_genome, generation as u32, fit) {
                    Ok(path) => {
                        if config.verbose {
                            eprintln!("  Saved new best to: {}", path.display());
                        }
                    }
                    Err(e) => {
                        eprintln!("  Warning: failed to save evolved bot: {e}");
                    }
                }
            }
        }

        // Log progress
        if config.verbose {
            eprintln!(
                "Gen {:>5}: best={:.4} mean={:.4} std={:.4}",
                generation, stats.best_fitness, stats.mean_fitness, stats.fitness_std
            );
        }

        // Save checkpoint
        if generation.is_multiple_of(config.checkpoint_interval) {
            let checkpoint = Checkpoint {
                generation: generation as u32,
                population: population.clone(),
                fitness: pop_fitness.clone(),
                best_fitness,
                rng_seed: config.seed,
            };
            let path = checkpoint_path(&config.output_dir, generation as u32);
            if let Err(e) = save_checkpoint(&checkpoint, &path) {
                eprintln!("Warning: failed to save checkpoint: {e}");
            }

            // Also save best WASM
            let wasm_path = best_wasm_path(&config.output_dir);
            if let Err(e) = save_best_wasm(&best_genome, &wasm_path) {
                eprintln!("Warning: failed to save best WASM: {e}");
            }
        }

        // Selection (use pop_fitness which excludes the re-evaluated best genome)
        let selection_result = select_parents(
            &pop_fitness,
            &config.selection,
            config.population_size,
            &mut rng,
        );

        // Create next generation
        let mut next_population = Vec::with_capacity(config.population_size);

        // Preserve elite
        for &idx in &selection_result.elite_indices {
            next_population.push(population[idx].clone());
        }

        // Create offspring through crossover and mutation
        for (p1_idx, p2_idx) in &selection_result.parent_pairs {
            let p1 = &population[*p1_idx];
            let p2 = &population[*p2_idx];

            // Create two children from each pair
            let mut child1 = crossover(p1, p2, &config.crossover, &mut rng);
            mutate(&mut child1, &config.mutation, &mut rng);
            next_population.push(child1);

            if next_population.len() < config.population_size {
                let mut child2 = crossover(p2, p1, &config.crossover, &mut rng);
                mutate(&mut child2, &config.mutation, &mut rng);
                next_population.push(child2);
            }
        }

        // Ensure population size is exactly right
        next_population.truncate(config.population_size);
        while next_population.len() < config.population_size {
            next_population.push(Genome::random(&mut rng, config.initial_rules));
        }

        population = next_population;
        generation += 1;
    }

    // Final checkpoint
    let checkpoint = Checkpoint {
        generation: generation as u32,
        population: population.clone(),
        fitness: Vec::new(),
        best_fitness,
        rng_seed: config.seed,
    };
    let path = checkpoint_path(&config.output_dir, generation as u32);
    save_checkpoint(&checkpoint, &path)?;

    // Save final best WASM
    let wasm_path = best_wasm_path(&config.output_dir);
    save_best_wasm(&best_genome, &wasm_path)?;

    let elapsed = start_time.elapsed().as_secs_f64();

    Ok((
        best_genome,
        EvolutionStats {
            generations: gen_stats,
            best_fitness,
            best_generation,
            elapsed_seconds: elapsed,
        },
    ))
}

/// Run a round-robin tournament among the population.
///
/// Each bot plays multiple games against randomly selected opponents from the population.
/// Returns fitness scores (win rate) for each bot.
fn run_tournament(
    compiled_bots: &[CompiledProgram],
    engine: &Engine,
    config: &FitnessConfig,
) -> Vec<f64> {
    use crate::tournament::{run_game_with_modules, TournamentConfig};
    use rayon::prelude::*;

    let tournament_config = TournamentConfig {
        max_turns: config.max_turns,
        fuel_budget: config.fuel_budget,
        map_width: config.map_width,
        map_height: config.map_height,
    };

    let num_bots = compiled_bots.len();
    if num_bots < 2 {
        return vec![0.0; num_bots];
    }

    // Each bot plays games_per_eval games against random opponents
    let results: Vec<f64> = (0..num_bots)
        .into_par_iter()
        .map(|bot_idx| {
            let mut wins = 0usize;
            let mut games_played = 0usize;

            for game_idx in 0..config.games_per_eval {
                // Select random opponents (excluding self)
                let mut opponents: Vec<usize> = (0..num_bots)
                    .filter(|&i| i != bot_idx)
                    .collect();

                // Pick 1-3 opponents for this game
                let num_opponents = opponents.len().clamp(1, 3);
                let seed = config.base_seed
                    .wrapping_add(bot_idx as u64 * 1000)
                    .wrapping_add(game_idx as u64);

                // Simple deterministic shuffle using seed
                for i in 0..opponents.len() {
                    let j = ((seed.wrapping_mul(i as u64 + 1)) % opponents.len() as u64) as usize;
                    opponents.swap(i, j);
                }
                opponents.truncate(num_opponents);

                // Build player list: this bot first, then opponents
                let players: Vec<CompiledProgram> = std::iter::once(compiled_bots[bot_idx].clone())
                    .chain(opponents.iter().map(|&i| compiled_bots[i].clone()))
                    .collect();

                // Run the game
                if let Ok(result) = run_game_with_modules(seed, engine, &players, &tournament_config) {
                    games_played += 1;
                    // Player 1 is our bot (1-indexed)
                    if result.winner == Some(1) {
                        wins += 1;
                    }
                }
            }

            if games_played > 0 {
                wins as f64 / games_played as f64
            } else {
                0.0
            }
        })
        .collect();

    results
}

/// Resume evolution from a checkpoint.
///
/// # Errors
///
/// Returns an error if the checkpoint cannot be loaded.
#[allow(dead_code)]
pub fn resume(
    checkpoint_path: &std::path::Path,
    config: &EvolutionConfig,
) -> Result<(Genome, EvolutionStats), EvolutionError> {
    let checkpoint = load_checkpoint(checkpoint_path)?;

    // Create a modified config starting from the checkpoint generation
    let mut resume_config = config.clone();
    resume_config.seed = checkpoint.rng_seed;

    // Start evolution with the loaded population
    let start_time = std::time::Instant::now();

    // Create output directory
    std::fs::create_dir_all(&resume_config.output_dir)?;

    // Create WASM engine
    let engine = create_engine()?;

    // Initialize RNG
    let mut rng = SmallRng::seed_from_u64(resume_config.seed);
    // Advance RNG state to match checkpoint generation
    for _ in 0..checkpoint.generation {
        let _: u64 = rng.r#gen();
    }

    let mut population = checkpoint.population;
    let mut best_genome = population[0].clone();
    let mut best_fitness = checkpoint.best_fitness;
    let mut best_generation = checkpoint.generation as usize;
    let mut gen_stats = Vec::new();

    // Continue evolution from checkpoint
    for generation in (checkpoint.generation as usize)..resume_config.generations {
        // Include best genome in population for fair comparison
        let mut eval_population = population.clone();
        let best_genome_idx = eval_population.len();
        eval_population.push(best_genome.clone());

        // Compile entire population for tournament play
        let compiled_population = compile_population(&eval_population, &engine);

        // Skip if we couldn't compile enough bots
        if compiled_population.len() < 2 {
            continue;
        }

        // Run tournament
        let fitness = run_tournament(&compiled_population, &engine, &resume_config.fitness);

        // Get fitness values for original population only
        let pop_fitness: Vec<f64> = fitness.iter().take(population.len()).copied().collect();
        let stats = SelectionStats::from_fitness(&pop_fitness);

        gen_stats.push(GenerationStats {
            generation,
            best_fitness: stats.best_fitness,
            mean_fitness: stats.mean_fitness,
            fitness_std: stats.fitness_std,
        });

        // Update best genome with fair comparison
        if let Some((idx, &fit)) = pop_fitness
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        {
            let prev_best_fit = fitness.get(best_genome_idx).copied().unwrap_or(0.0);
            if fit > prev_best_fit {
                best_fitness = fit;
                best_genome = population[idx].clone();
                best_generation = generation;
            }
        }

        if resume_config.verbose {
            eprintln!(
                "Gen {:>5}: best={:.4} mean={:.4} std={:.4}",
                generation, stats.best_fitness, stats.mean_fitness, stats.fitness_std
            );
        }

        if generation % resume_config.checkpoint_interval == 0 {
            let ckpt = Checkpoint {
                generation: generation as u32,
                population: population.clone(),
                fitness: pop_fitness.clone(),
                best_fitness,
                rng_seed: resume_config.seed,
            };
            let path = crate::gp::persistence::checkpoint_path(&resume_config.output_dir, generation as u32);
            let _ = save_checkpoint(&ckpt, &path);
            let _ = save_best_wasm(&best_genome, &best_wasm_path(&resume_config.output_dir));
        }

        let selection_result = select_parents(
            &pop_fitness,
            &resume_config.selection,
            resume_config.population_size,
            &mut rng,
        );

        let mut next_population = Vec::with_capacity(resume_config.population_size);
        for &idx in &selection_result.elite_indices {
            next_population.push(population[idx].clone());
        }
        for (p1_idx, p2_idx) in &selection_result.parent_pairs {
            let mut child1 = crossover(&population[*p1_idx], &population[*p2_idx], &resume_config.crossover, &mut rng);
            mutate(&mut child1, &resume_config.mutation, &mut rng);
            next_population.push(child1);

            if next_population.len() < resume_config.population_size {
                let mut child2 = crossover(&population[*p2_idx], &population[*p1_idx], &resume_config.crossover, &mut rng);
                mutate(&mut child2, &resume_config.mutation, &mut rng);
                next_population.push(child2);
            }
        }
        next_population.truncate(resume_config.population_size);
        while next_population.len() < resume_config.population_size {
            next_population.push(Genome::random(&mut rng, resume_config.initial_rules));
        }
        population = next_population;
    }

    let elapsed = start_time.elapsed().as_secs_f64();

    Ok((
        best_genome,
        EvolutionStats {
            generations: gen_stats,
            best_fitness,
            best_generation,
            elapsed_seconds: elapsed,
        },
    ))
}

/// Create a WASM engine configured for bot execution.
fn create_engine() -> Result<Engine, EvolutionError> {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    config.parallel_compilation(true);

    Engine::new(&config).map_err(|e| EvolutionError::EngineError(e.to_string()))
}

/// Error during evolution.
#[derive(Debug)]
pub enum EvolutionError {
    /// File I/O error.
    IoError(std::io::Error),
    /// WASM engine creation failed.
    EngineError(String),
}

impl std::fmt::Display for EvolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::EngineError(e) => write!(f, "engine error: {e}"),
        }
    }
}

impl std::error::Error for EvolutionError {}

impl From<std::io::Error> for EvolutionError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evolution_config_default() {
        let config = EvolutionConfig::default();
        assert!(config.population_size > 0);
        assert!(config.generations > 0);
    }

    #[test]
    fn test_create_engine() {
        let engine = create_engine();
        assert!(engine.is_ok());
    }
}
