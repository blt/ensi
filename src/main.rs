//! Ensi CLI - Command-line interface for running and viewing Ensi games.

// Allow print in the CLI binary
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod cli;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// Ensi - A deterministic RISC-V game engine
#[derive(Parser, Debug)]
#[command(name = "ensi")]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a single game between bots
    Run {
        /// Bot ELF files (2-8 bots required)
        #[arg(required = true, num_args = 2..=8)]
        bots: Vec<std::path::PathBuf>,

        /// Random seed (default: random)
        #[arg(short, long)]
        seed: Option<u64>,

        /// Maximum turns (default: 1000)
        #[arg(short, long, default_value = "1000")]
        turns: u32,

        /// Output format: text, json, or llm
        #[arg(short, long, default_value = "text")]
        format: cli::OutputFormat,

        /// Save recording to file
        #[arg(long)]
        save: Option<std::path::PathBuf>,

        /// Suppress turn-by-turn output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Interactive TUI to watch a game in real-time
    Watch {
        /// Bot ELF files (2-8 bots required)
        #[arg(required = true, num_args = 2..=8)]
        bots: Vec<std::path::PathBuf>,

        /// Random seed
        #[arg(short, long)]
        seed: Option<u64>,

        /// Maximum turns (default: 1000)
        #[arg(short, long, default_value = "1000")]
        turns: u32,

        /// Turn delay in milliseconds (default: 500)
        #[arg(long, default_value = "500")]
        speed: u64,

        /// View from player N's perspective (1-8, default: all)
        #[arg(short, long)]
        player: Option<u8>,
    },

    /// Replay a recorded game
    Replay {
        /// Recording file (.ensi)
        #[arg(required = true)]
        recording: std::path::PathBuf,

        /// Output format: tui, text, or llm
        #[arg(short, long, default_value = "tui")]
        format: cli::ReplayFormat,

        /// Start at specific turn
        #[arg(short, long)]
        turn: Option<u32>,

        /// View from player N's perspective (1-8, default: all)
        #[arg(short, long)]
        player: Option<u8>,
    },

    /// Run mass parallel games and aggregate statistics
    Tournament {
        /// Bot ELF files (2-8 bots required)
        #[arg(required = true, num_args = 2..=8)]
        bots: Vec<std::path::PathBuf>,

        /// Number of games to run (default: 1000)
        #[arg(short, long, default_value = "1000")]
        games: u64,

        /// Starting seed (increments for each game)
        #[arg(short, long)]
        seed: Option<u64>,

        /// Parallel threads (default: CPU count)
        #[arg(short = 'j', long)]
        threads: Option<usize>,

        /// Instruction budget per turn (default: 100000)
        #[arg(short, long)]
        budget: Option<u32>,

        /// Maximum turns per game (default: 1000)
        #[arg(short = 't', long)]
        max_turns: Option<u32>,

        /// Output format: text, json, or csv
        #[arg(short, long, default_value = "text")]
        format: cli::TournamentFormat,

        /// Show progress bar
        #[arg(short, long)]
        progress: bool,
    },

    /// Validate an ELF file for compatibility
    Validate {
        /// Bot ELF file to validate
        #[arg(required = true)]
        bot: std::path::PathBuf,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();

    let result = match args.command {
        Commands::Run {
            bots,
            seed,
            turns,
            format,
            save,
            quiet,
        } => cli::run::execute(bots, seed, turns, format, save, quiet),

        Commands::Watch {
            bots,
            seed,
            turns,
            speed,
            player,
        } => cli::watch::execute(bots, seed, turns, speed, player),

        Commands::Replay {
            recording,
            format,
            turn,
            player,
        } => cli::replay::execute(recording, format, turn, player),

        Commands::Tournament {
            bots,
            games,
            seed,
            threads,
            budget,
            max_turns,
            format,
            progress,
        } => cli::tournament::execute(bots, games, seed, threads, budget, max_turns, format, progress),

        Commands::Validate { bot } => cli::validate::execute(bot),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}
