//! Tournament command implementation.

use super::output::{format_tournament_csv, format_tournament_text, JsonTournamentResult, TournamentStats};
use super::{CliError, TournamentFormat};
use ensi::tournament::{run_game, PlayerProgram, TournamentConfig};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
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
    format: TournamentFormat,
    progress: bool,
) -> Result<(), CliError> {
    // Load bot programs
    let mut programs = Vec::with_capacity(bots.len());
    let mut bot_names = Vec::with_capacity(bots.len());

    for bot_path in &bots {
        let elf_bytes = fs::read(bot_path).map_err(|e| {
            CliError::new(format!("Failed to read {}: {e}", bot_path.display()))
        })?;
        programs.push(PlayerProgram::new(elf_bytes));
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
    let config = TournamentConfig::default();

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

    // Run games in parallel
    let stats = Mutex::new(TournamentStats::new(bots.len()));
    let completed = AtomicU64::new(0);

    (0..games).into_par_iter().for_each(|i| {
        let game_seed = base_seed.wrapping_add(i);

        if let Ok(result) = run_game(game_seed, &programs, &config) {
            if let Ok(mut stats) = stats.lock() {
                stats.add_result(&result);
            }
        }

        let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(ref pb) = pb {
            pb.set_position(done);
        }
    });

    if let Some(pb) = pb {
        pb.finish_with_message("done");
    }

    let duration = start.elapsed();
    let stats = stats.into_inner().map_err(|e| CliError::new(format!("Lock error: {e}")))?;

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
