//! Run command implementation.

use super::output::{format_text, JsonGameResult};
use super::{CliError, OutputFormat};
use ensi::replay::{render_llm, Recording};
use ensi::tournament::{run_game, PlayerProgram, TournamentConfig};
use std::fs;
use std::path::PathBuf;

/// Execute the run command.
///
/// # Errors
///
/// Returns an error if the game fails to run.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    bots: Vec<PathBuf>,
    seed: Option<u64>,
    turns: u32,
    format: OutputFormat,
    save: Option<PathBuf>,
    quiet: bool,
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

    // Generate seed if not provided
    let seed = seed.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42)
    });

    // Configure tournament
    let config = TournamentConfig {
        max_turns: turns,
        ..TournamentConfig::default()
    };

    if !quiet {
        println!("Running game with seed {seed}...");
        println!("Players: {}", bot_names.join(", "));
        println!();
    }

    // Run the game
    let result = run_game(seed, &programs, &config)?;

    // Save recording if requested
    if let Some(save_path) = save {
        let recording = Recording::from_programs(seed, &programs, config);
        recording.save(&save_path).map_err(|e| {
            CliError::new(format!("Failed to save recording: {e}"))
        })?;
        if !quiet {
            println!("Recording saved to: {}", save_path.display());
            println!();
        }
    }

    // Output based on format
    match format {
        OutputFormat::Text => {
            print!("{}", format_text(&result, &bot_names));
        }
        OutputFormat::Json => {
            let json_result = JsonGameResult::from_game_result(&result);
            let json = serde_json::to_string_pretty(&json_result)
                .map_err(|e| CliError::new(format!("JSON serialization failed: {e}")))?;
            println!("{json}");
        }
        OutputFormat::Llm => {
            // For LLM mode, we need to replay and show final state
            let recording = Recording::from_programs(seed, &programs, config);
            let engine = ensi::replay::ReplayEngine::new(recording)?;

            // Run to completion
            let mut engine = engine;
            while !engine.is_game_over() {
                if let Err(e) = engine.step_forward() {
                    // Game is over, that's fine
                    if !matches!(e, ensi::replay::ReplayError::GameOver) {
                        return Err(e.into());
                    }
                    break;
                }
            }

            // Show final state in LLM format
            println!("{}", render_llm(engine.state(), engine.turn()));
            println!();
            println!("=== FINAL RESULT ===");
            println!();
            if let Some(winner) = result.winner {
                let name = bot_names.get(winner as usize - 1).map_or("Unknown", String::as_str);
                println!("Winner: Player {winner} ({name})");
            } else {
                println!("Result: Draw");
            }
            println!("Total turns: {}", result.turns_played);
            println!();
            println!("Final scores:");
            for (i, stats) in result.player_stats.iter().enumerate() {
                let name = bot_names.get(i).map_or("Unknown", String::as_str);
                println!("  Player {} ({}): {:.0}", stats.player_id, name, stats.final_score);
            }
        }
    }

    Ok(())
}
