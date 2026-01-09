//! Output formatting utilities for CLI.

use ensi::tournament::GameResult;
use serde::Serialize;

/// JSON-serializable game result.
#[derive(Debug, Serialize)]
pub(super) struct JsonGameResult {
    /// Random seed used.
    pub(super) seed: u64,
    /// Winner player ID (null if draw).
    pub(super) winner: Option<u8>,
    /// Total turns played.
    pub(super) turns_played: u32,
    /// Per-player results.
    pub(super) players: Vec<JsonPlayerResult>,
}

/// JSON-serializable player result.
#[derive(Debug, Serialize)]
pub(super) struct JsonPlayerResult {
    /// Player ID (1-8).
    pub(super) id: u8,
    /// Final score.
    pub(super) score: f64,
    /// Total instructions executed.
    pub(super) instructions: u64,
    /// Turn eliminated (null if survived).
    pub(super) eliminated_turn: Option<u32>,
}

impl JsonGameResult {
    /// Create from a GameResult.
    pub(super) fn from_game_result(result: &GameResult) -> Self {
        Self {
            seed: result.seed,
            winner: result.winner,
            turns_played: result.turns_played,
            players: result
                .player_stats
                .iter()
                .map(|ps| JsonPlayerResult {
                    id: ps.player_id,
                    score: ps.final_score,
                    instructions: ps.total_instructions,
                    eliminated_turn: ps.eliminated_turn,
                })
                .collect(),
        }
    }
}

/// Format a game result as human-readable text.
pub(super) fn format_text(result: &GameResult, bot_names: &[String]) -> String {
    let mut output = String::new();

    output.push_str(&format!("Game Result (seed: {})\n", result.seed));
    if let Some(winner) = result.winner {
        let name = bot_names.get(winner as usize - 1).map_or("Unknown", String::as_str);
        output.push_str(&format!("  Winner: Player {winner} ({name})\n"));
    } else {
        output.push_str("  Winner: Draw\n");
    }
    output.push_str(&format!("  Turns: {}\n\n", result.turns_played));

    for (i, stats) in result.player_stats.iter().enumerate() {
        let name = bot_names.get(i).map_or("Unknown", String::as_str);
        output.push_str(&format!(
            "  Player {}: {:.0} points ({})",
            stats.player_id, stats.final_score, name
        ));
        if let Some(turn) = stats.eliminated_turn {
            output.push_str(&format!(" [eliminated turn {turn}]"));
        }
        output.push('\n');
    }

    output
}

/// Tournament statistics for aggregated results.
#[derive(Debug, Default)]
pub(super) struct TournamentStats {
    /// Total games played.
    pub(super) games_played: u64,
    /// Win count per player.
    pub(super) wins: Vec<u64>,
    /// Draw count.
    pub(super) draws: u64,
    /// Total score per player.
    total_scores: Vec<f64>,
    /// Score sum of squares for std dev calculation.
    score_sq_sums: Vec<f64>,
    /// Total turns across all games.
    total_turns: u64,
}

impl TournamentStats {
    /// Create new stats for n players.
    pub(super) fn new(num_players: usize) -> Self {
        Self {
            games_played: 0,
            wins: vec![0; num_players],
            draws: 0,
            total_scores: vec![0.0; num_players],
            score_sq_sums: vec![0.0; num_players],
            total_turns: 0,
        }
    }

    /// Add a game result to the stats.
    pub(super) fn add_result(&mut self, result: &GameResult) {
        self.games_played += 1;
        self.total_turns += u64::from(result.turns_played);

        if let Some(winner) = result.winner {
            let idx = winner as usize - 1;
            if idx < self.wins.len() {
                self.wins[idx] += 1;
            }
        } else {
            self.draws += 1;
        }

        for (i, stats) in result.player_stats.iter().enumerate() {
            if i < self.total_scores.len() {
                self.total_scores[i] += stats.final_score;
                self.score_sq_sums[i] += stats.final_score * stats.final_score;
            }
        }
    }

    /// Get win rate for a player (0.0-1.0).
    pub(super) fn win_rate(&self, player_idx: usize) -> f64 {
        if self.games_played == 0 {
            return 0.0;
        }
        self.wins.get(player_idx).copied().unwrap_or(0) as f64 / self.games_played as f64
    }

    /// Get average score for a player.
    pub(super) fn avg_score(&self, player_idx: usize) -> f64 {
        if self.games_played == 0 {
            return 0.0;
        }
        self.total_scores.get(player_idx).copied().unwrap_or(0.0) / self.games_played as f64
    }

    /// Get score standard deviation for a player.
    pub(super) fn score_std_dev(&self, player_idx: usize) -> f64 {
        if self.games_played == 0 {
            return 0.0;
        }
        let n = self.games_played as f64;
        let mean = self.avg_score(player_idx);
        let sq_sum = self.score_sq_sums.get(player_idx).copied().unwrap_or(0.0);
        let variance = (sq_sum / n) - (mean * mean);
        if variance < 0.0 {
            0.0
        } else {
            variance.sqrt()
        }
    }

    /// Get average game length.
    pub(super) fn avg_turns(&self) -> f64 {
        if self.games_played == 0 {
            return 0.0;
        }
        self.total_turns as f64 / self.games_played as f64
    }
}

/// JSON-serializable tournament result.
#[derive(Debug, Serialize)]
pub(super) struct JsonTournamentResult {
    /// Total games played.
    games_played: u64,
    /// Per-player statistics.
    players: Vec<JsonTournamentPlayer>,
    /// Number of draws.
    draws: u64,
    /// Average game length in turns.
    avg_turns: f64,
}

/// JSON-serializable per-player tournament stats.
#[derive(Debug, Serialize)]
pub(super) struct JsonTournamentPlayer {
    /// Player index (0-based).
    player: usize,
    /// Bot filename.
    bot: String,
    /// Number of wins.
    wins: u64,
    /// Win rate (0.0-1.0).
    win_rate: f64,
    /// Average score.
    avg_score: f64,
    /// Score standard deviation.
    score_std_dev: f64,
}

impl JsonTournamentResult {
    /// Create from stats and bot names.
    pub(super) fn from_stats(stats: &TournamentStats, bot_names: &[String]) -> Self {
        let players = (0..bot_names.len())
            .map(|i| JsonTournamentPlayer {
                player: i + 1,
                bot: bot_names.get(i).cloned().unwrap_or_default(),
                wins: stats.wins.get(i).copied().unwrap_or(0),
                win_rate: stats.win_rate(i),
                avg_score: stats.avg_score(i),
                score_std_dev: stats.score_std_dev(i),
            })
            .collect();

        Self {
            games_played: stats.games_played,
            players,
            draws: stats.draws,
            avg_turns: stats.avg_turns(),
        }
    }
}

/// Format tournament stats as human-readable text.
pub(super) fn format_tournament_text(stats: &TournamentStats, bot_names: &[String]) -> String {
    let mut output = String::new();

    output.push_str(&format!("Tournament Results ({} games)\n", stats.games_played));
    output.push_str("========================================\n\n");

    output.push_str("Win Rates:\n");
    for (i, name) in bot_names.iter().enumerate() {
        let wins = stats.wins.get(i).copied().unwrap_or(0);
        let rate = stats.win_rate(i) * 100.0;
        output.push_str(&format!(
            "  Player {} ({}): {:.1}% ({} wins)\n",
            i + 1, name, rate, wins
        ));
    }
    output.push_str(&format!("  Draws: {} ({:.1}%)\n\n",
        stats.draws,
        (stats.draws as f64 / stats.games_played as f64) * 100.0
    ));

    output.push_str("Average Scores:\n");
    for (i, name) in bot_names.iter().enumerate() {
        let avg = stats.avg_score(i);
        let std = stats.score_std_dev(i);
        output.push_str(&format!(
            "  Player {} ({}): {:.1} (+/- {:.1})\n",
            i + 1, name, avg, std
        ));
    }

    output.push_str(&format!("\nAverage Game Length: {:.0} turns\n", stats.avg_turns()));

    output
}

/// Format tournament stats as CSV.
pub(super) fn format_tournament_csv(stats: &TournamentStats, bot_names: &[String]) -> String {
    let mut output = String::new();

    // Header
    output.push_str("player,bot,wins,win_rate,avg_score,score_std_dev\n");

    // Data rows
    for (i, name) in bot_names.iter().enumerate() {
        output.push_str(&format!(
            "{},{},{},{:.4},{:.2},{:.2}\n",
            i + 1,
            name,
            stats.wins.get(i).copied().unwrap_or(0),
            stats.win_rate(i),
            stats.avg_score(i),
            stats.score_std_dev(i)
        ));
    }

    output
}
