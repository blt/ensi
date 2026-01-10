//! CLI command implementations for Ensi.

pub(crate) mod replay;
pub(crate) mod run;
pub(crate) mod tournament;
pub(crate) mod validate;
pub(crate) mod watch;

mod output;

use clap::ValueEnum;
use std::error::Error;
use std::fmt;

/// Output format for the `run` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Human-readable text output.
    Text,
    /// Machine-readable JSON output.
    Json,
    /// Structured text for LLM consumption.
    Llm,
}

/// Output format for the `replay` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ReplayFormat {
    /// Interactive TUI.
    Tui,
    /// Plain text output.
    Text,
    /// Structured text for LLM consumption.
    Llm,
}

/// Output format for the `tournament` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum TournamentFormat {
    /// Human-readable text output.
    Text,
    /// Machine-readable JSON output.
    Json,
    /// CSV format.
    Csv,
}

/// CLI error type.
#[derive(Debug)]
pub(crate) struct CliError {
    message: String,
}

impl CliError {
    /// Create a new CLI error.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::new(e.to_string())
    }
}

impl From<ensi::tournament::TournamentError> for CliError {
    fn from(e: ensi::tournament::TournamentError) -> Self {
        Self::new(e.to_string())
    }
}

impl From<ensi::replay::ReplayError> for CliError {
    fn from(e: ensi::replay::ReplayError) -> Self {
        Self::new(e.to_string())
    }
}
