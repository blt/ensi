//! Tournament command implementation.

use super::output::{format_tournament_csv, format_tournament_text, JsonTournamentResult, TournamentStats};
use super::{CliError, TournamentFormat};
use ensi::tournament::{run_game_with_modules, CompiledProgram, PlayerProgram, TournamentConfig};
use ensi::wasm::WasmBot;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// Execute the tournament command.
///
/// # Errors
///
/// Returns an error if the tournament fails.
pub(crate) fn execute(
    bots: Vec<PathBuf>,
    games: u64,
    seed: Option<u64>,
    threads: Option<usize>,
    budget: Option<u32>,
    max_turns: Option<u32>,
    format: TournamentFormat,
    progress: bool,
) -> Result<(), CliError> {
    // Create shared wasmtime engine once
    let engine = WasmBot::create_engine()
        .map_err(|e| CliError::new(format!("Failed to create WASM engine: {e}")))?;

    // Load and pre-compile bot programs (once, not per-game)
    let mut compiled_modules: Vec<CompiledProgram> = Vec::with_capacity(bots.len());
    let mut bot_names = Vec::with_capacity(bots.len());

    for bot_path in &bots {
        let wasm_bytes = fs::read(bot_path).map_err(|e| {
            CliError::new(format!("Failed to read {}: {e}", bot_path.display()))
        })?;
        let program = PlayerProgram::new(wasm_bytes);
        let compiled = program.compile(&engine).map_err(|e| {
            CliError::new(format!("Failed to compile {}: {e}", bot_path.display()))
        })?;
        compiled_modules.push(compiled);
        bot_names.push(
            bot_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        );
    }

    // Set thread pool size if specified
    if let Some(num_threads) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global()
            .ok(); // Ignore error if already initialized
    }

    // Base seed
    let base_seed = seed.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42)
    });

    // Config
    let mut config = TournamentConfig::default();
    if let Some(b) = budget {
        config.fuel_budget = u64::from(b);
    }
    if let Some(t) = max_turns {
        config.max_turns = t;
    }

    // Progress bar
    let pb = if progress {
        let pb = ProgressBar::new(games);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} games ({per_sec})")
                .expect("valid template")
                .progress_chars("=>-"),
        );
        Some(pb)
    } else {
        None
    };

    let start = Instant::now();
    let num_players = bots.len();

    // Run games in parallel using lock-free fold/reduce pattern
    // Each thread accumulates into its own TournamentStats, then we merge at the end
    // Progress is tracked via games_played in stats (no atomics in hot path)
    let stats = (0..games)
        .into_par_iter()
        .fold(
            || TournamentStats::new(num_players),
            |mut local_stats, i| {
                let game_seed = base_seed.wrapping_add(i);

                // Use pre-compiled modules (no per-game compilation)
                if let Ok(result) = run_game_with_modules(game_seed, &engine, &compiled_modules, &config) {
                    local_stats.add_result(&result);
                }

                local_stats
            },
        )
        .reduce(
            || TournamentStats::new(num_players),
            |mut a, b| {
                a.merge(&b);
                a
            },
        );

    // Update progress bar after completion (no atomic overhead in hot path)
    if let Some(pb) = pb {
        pb.set_position(stats.games_played);
        pb.finish_with_message("done");
    }

    let duration = start.elapsed();

    // Calculate games per second
    let games_per_sec = if duration.as_secs_f64() > 0.0 {
        stats.games_played as f64 / duration.as_secs_f64()
    } else {
        0.0
    };

    // Output based on format
    match format {
        TournamentFormat::Text => {
            println!();
            print!("{}", format_tournament_text(&stats, &bot_names));
            println!();
            println!("Duration: {:.2}s ({:.0} games/sec)", duration.as_secs_f64(), games_per_sec);
        }
        TournamentFormat::Json => {
            let json_result = JsonTournamentResult::from_stats(&stats, &bot_names);
            let json = serde_json::to_string_pretty(&json_result)
                .map_err(|e| CliError::new(format!("JSON serialization failed: {e}")))?;
            println!("{json}");
        }
        TournamentFormat::Csv => {
            print!("{}", format_tournament_csv(&stats, &bot_names));
        }
    }

    Ok(())
}
