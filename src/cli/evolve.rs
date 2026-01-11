//! CLI command for genetic programming evolution.

#![allow(clippy::needless_pass_by_value, clippy::ptr_arg)]

use crate::cli::CliError;
use ensi::gp::{evolve, EvolutionConfig, FitnessConfig, MutationConfig, CrossoverConfig, SelectionConfig};
use std::path::PathBuf;

/// Execute the evolve command.
pub(crate) fn execute(
    output: PathBuf,
    population: usize,
    generations: usize,
    games: usize,
    seed: Option<u64>,
    resume: Option<PathBuf>,
    verbose: bool,
) -> Result<(), CliError> {
    // Build configuration
    let config = EvolutionConfig {
        population_size: population,
        generations,
        initial_rules: 10,
        seed: seed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(42)
        }),
        fitness: FitnessConfig {
            games_per_eval: games,
            base_seed: 12345,
            max_turns: 500,
            fuel_budget: 50_000,
            map_width: 32,
            map_height: 32,
        },
        selection: SelectionConfig::default(),
        crossover: CrossoverConfig::default(),
        mutation: MutationConfig::default(),
        output_dir: output.clone(),
        checkpoint_interval: 100,
        verbose,
    };

    // Run evolution
    if let Some(checkpoint_path) = resume {
        println!("Resuming evolution from checkpoint: {}", checkpoint_path.display());
        let (best, stats) = ensi::gp::resume(&checkpoint_path, &config)
            .map_err(|e| CliError::new(e.to_string()))?;

        print_results(&output, &best, &stats);
    } else {
        println!("Starting evolution:");
        println!("  Population: {population}");
        if generations == 0 {
            println!("  Generations: infinite (Ctrl+C to stop)");
        } else {
            println!("  Generations: {generations}");
        }
        println!("  Games per eval: {games}");
        println!("  Output: {}", output.display());
        println!();

        let (best, stats) = evolve(&config)
            .map_err(|e| CliError::new(e.to_string()))?;

        print_results(&output, &best, &stats);
    }

    Ok(())
}

fn print_results(output: &PathBuf, _best: &ensi::gp::Genome, stats: &ensi::gp::EvolutionStats) {
    println!();
    println!("Evolution complete!");
    println!("  Best fitness: {:.4}", stats.best_fitness);
    println!("  Best generation: {}", stats.best_generation);
    println!("  Elapsed time: {:.1}s", stats.elapsed_seconds);
    println!();
    println!("Output files:");
    println!("  Best WASM: {}/best.wasm", output.display());
    println!("  Checkpoints: {}/gen_*.bin", output.display());
}
