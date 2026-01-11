//! CLI command for managing evolved bots.

#![allow(clippy::ptr_arg)]

use super::CliError;
use ensi::gp::{list_evolved_bots, load_evolved_genome, evolved_bots_dir};
use std::path::PathBuf;

/// Execute the `bots` command.
pub(crate) fn execute(
    list: bool,
    path: Option<PathBuf>,
    examine: Option<PathBuf>,
) -> Result<(), CliError> {
    if list || path.is_none() && examine.is_none() {
        // Default: list all evolved bots
        return list_bots();
    }

    if let Some(bot_path) = examine {
        return examine_bot(&bot_path);
    }

    if let Some(dir_path) = path {
        // Show path to evolved bots directory
        println!("{}", dir_path.display());
    }

    Ok(())
}

/// List all evolved bots in ~/.ensi/evolved/.
fn list_bots() -> Result<(), CliError> {
    let bots = list_evolved_bots()?;

    if bots.is_empty() {
        let dir = evolved_bots_dir().unwrap_or_else(|_| PathBuf::from("~/.ensi/evolved"));
        println!("No evolved bots found in {}", dir.display());
        println!();
        println!("Run 'ensi evolve' to evolve some bots!");
        return Ok(());
    }

    println!("Evolved bots in ~/.ensi/evolved/:");
    println!();
    println!(
        "{:<40} {:>6} {:>8} {:>6}",
        "Path", "Gen", "Fitness", "Rules"
    );
    println!("{:-<40} {:->6} {:->8} {:->6}", "", "", "", "");

    for (path, meta) in &bots {
        let filename = path
            .file_name().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().to_string());

        println!(
            "{:<40} {:>6} {:>7.2}% {:>6}",
            filename,
            meta.generation,
            meta.fitness * 100.0,
            meta.num_rules
        );
    }

    println!();
    println!("Total: {} evolved bot(s)", bots.len());

    // Show the best bot
    if let Some((best_path, best_meta)) = bots.first() {
        println!();
        println!("Best bot: {} (fitness: {:.1}%)", best_path.display(), best_meta.fitness * 100.0);
    }

    Ok(())
}

/// Examine a specific evolved bot's genome.
fn examine_bot(path: &PathBuf) -> Result<(), CliError> {
    let genome = load_evolved_genome(path)
        .map_err(|e| CliError::new(format!("Failed to load genome: {e}")))?;

    println!("Genome from: {}", path.display());
    println!();
    println!("Constants: {:?}", genome.constants);
    println!();
    println!("Rules ({}):", genome.rules.len());

    for (i, rule) in genome.rules.iter().enumerate() {
        println!();
        println!("  Rule {} (priority {}):", i, rule.priority);
        println!("    Condition: {:?}", rule.condition);
        println!("    Action: {:?}", rule.action);
    }

    Ok(())
}
