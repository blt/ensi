//! Game replay and viewing system.
//!
//! Because Ensi games are 100% deterministic, replay requires only:
//! - `seed: u64` - The random seed for map generation
//! - `programs: Vec<Vec<u8>>` - WASM bytes for each player
//!
//! No state deltas needed. To view turn N, re-run the simulation from turn 0 to N.
//!
//! # Time Travel
//!
//! - **Forward**: Continue stepping the simulation
//! - **Backward**: Re-run from turn 0 to (`current_turn` - 1)
//! - **Jump to turn N**: Re-run from turn 0 to N

mod render;
mod text;

pub use render::render_ascii;
pub use text::render_llm;

use crate::game::{process_attack, Command, GameState, PlayerId, TileType};
use crate::tournament::{generate_map, MapGenError, PlayerProgram, TournamentConfig};
use crate::wasm::{WasmBot, WasmError};
use std::fs::File;
use std::io::{self, Read as IoRead, Write as IoWrite};
use std::path::Path;

/// Minimal recording - just seed, programs, and config.
///
/// Because the game is deterministic, this is all we need to replay.
#[derive(Debug, Clone)]
pub struct Recording {
    /// Random seed for map generation.
    pub seed: u64,
    /// WASM bytes for each player.
    pub programs: Vec<Vec<u8>>,
    /// Tournament configuration.
    pub config: TournamentConfig,
}

impl Recording {
    /// Create a new recording from tournament inputs.
    #[must_use]
    pub fn new(seed: u64, programs: Vec<Vec<u8>>, config: TournamentConfig) -> Self {
        Self {
            seed,
            programs,
            config,
        }
    }

    /// Create from `PlayerProgram` slice (convenience).
    #[must_use]
    pub fn from_programs(seed: u64, programs: &[PlayerProgram], config: TournamentConfig) -> Self {
        Self {
            seed,
            programs: programs.iter().map(|p| p.wasm_bytes.clone()).collect(),
            config,
        }
    }

    /// Save recording to a file.
    ///
    /// Format: Simple binary format:
    /// - 8 bytes: seed (little-endian)
    /// - 4 bytes: `num_programs` (little-endian u32)
    /// - For each program:
    ///   - 4 bytes: program length (little-endian u32)
    ///   - N bytes: program WASM bytes
    /// - `TournamentConfig` fields
    ///
    /// # Errors
    ///
    /// Returns an error if file operations fail.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut file = File::create(path)?;

        // Write seed
        file.write_all(&self.seed.to_le_bytes())?;

        // Write number of programs
        #[allow(clippy::cast_possible_truncation)]
        let num_programs = self.programs.len() as u32;
        file.write_all(&num_programs.to_le_bytes())?;

        // Write each program
        for program in &self.programs {
            #[allow(clippy::cast_possible_truncation)]
            let len = program.len() as u32;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(program)?;
        }

        // Write config
        file.write_all(&self.config.max_turns.to_le_bytes())?;
        file.write_all(&self.config.fuel_budget.to_le_bytes())?;
        file.write_all(&self.config.map_width.to_le_bytes())?;
        file.write_all(&self.config.map_height.to_le_bytes())?;

        Ok(())
    }

    /// Load recording from a file.
    ///
    /// # Errors
    ///
    /// Returns an error if file operations fail or format is invalid.
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;

        // Read seed
        let mut seed_bytes = [0u8; 8];
        file.read_exact(&mut seed_bytes)?;
        let seed = u64::from_le_bytes(seed_bytes);

        // Read number of programs
        let mut num_programs_bytes = [0u8; 4];
        file.read_exact(&mut num_programs_bytes)?;
        let num_programs = u32::from_le_bytes(num_programs_bytes) as usize;

        // Read each program
        let mut programs = Vec::with_capacity(num_programs);
        for _ in 0..num_programs {
            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes)?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut program = vec![0u8; len];
            file.read_exact(&mut program)?;
            programs.push(program);
        }

        // Read config
        let mut u32_bytes = [0u8; 4];
        let mut u64_bytes = [0u8; 8];
        let mut u16_bytes = [0u8; 2];

        file.read_exact(&mut u32_bytes)?;
        let max_turns = u32::from_le_bytes(u32_bytes);

        file.read_exact(&mut u64_bytes)?;
        let fuel_budget = u64::from_le_bytes(u64_bytes);

        file.read_exact(&mut u16_bytes)?;
        let map_width = u16::from_le_bytes(u16_bytes);

        file.read_exact(&mut u16_bytes)?;
        let map_height = u16::from_le_bytes(u16_bytes);

        let config = TournamentConfig {
            max_turns,
            fuel_budget,
            map_width,
            map_height,
        };

        Ok(Self {
            seed,
            programs,
            config,
        })
    }
}

/// Error type for replay operations.
#[derive(Debug)]
pub enum ReplayError {
    /// Failed to load WASM for a player.
    WasmLoadError {
        /// Which player (0-indexed).
        player: usize,
        /// Error details.
        error: WasmError,
    },
    /// Map generation failed.
    MapGenerationError(MapGenError),
    /// Turn number out of bounds.
    TurnOutOfBounds {
        /// Requested turn.
        requested: u32,
        /// Maximum turn (exclusive).
        max_turn: u32,
    },
    /// Game is already over.
    GameOver,
    /// WASM engine error.
    EngineError(WasmError),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WasmLoadError { player, error } => {
                write!(f, "Failed to load WASM for player {player}: {error}")
            }
            Self::MapGenerationError(e) => write!(f, "Map generation failed: {e}"),
            Self::TurnOutOfBounds { requested, max_turn } => {
                write!(f, "Turn {requested} out of bounds (max: {max_turn})")
            }
            Self::GameOver => write!(f, "Game is already over"),
            Self::EngineError(e) => write!(f, "WASM engine error: {e}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// State for a single player's WASM bot during replay.
struct ReplayPlayerBot {
    /// Player identifier.
    player_id: PlayerId,
    /// WASM bot instance.
    bot: WasmBot,
    /// Whether this player has been eliminated.
    eliminated: bool,
}

/// Replay engine - steps through game deterministically.
///
/// Since games are deterministic, this engine can:
/// - Step forward by executing one turn
/// - Step backward by replaying from turn 0
/// - Jump to any turn by replaying from turn 0
pub struct ReplayEngine {
    /// The recording being replayed.
    recording: Recording,
    /// Current game state.
    game_state: GameState,
    /// Per-player bot states.
    player_bots: Vec<ReplayPlayerBot>,
    /// Current turn number.
    current_turn: u32,
}

impl std::fmt::Debug for ReplayEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayEngine")
            .field("current_turn", &self.current_turn)
            .field("is_game_over", &self.game_state.is_game_over())
            .finish_non_exhaustive()
    }
}

impl ReplayEngine {
    /// Create a new replay engine from a recording, starting at turn 0.
    ///
    /// # Errors
    ///
    /// Returns an error if WASM loading or map generation fails.
    pub fn new(recording: Recording) -> Result<Self, ReplayError> {
        Self::new_at_turn(recording, 0)
    }

    /// Create a new replay engine at a specific turn.
    ///
    /// This replays from turn 0 to the target turn.
    ///
    /// # Errors
    ///
    /// Returns an error if WASM loading or map generation fails.
    pub fn new_at_turn(recording: Recording, target_turn: u32) -> Result<Self, ReplayError> {
        let engine = WasmBot::create_engine().map_err(ReplayError::EngineError)?;
        let num_players = recording.programs.len();
        let config = &recording.config;

        // Generate map
        let (map, players) = generate_map(
            recording.seed,
            config.map_width,
            config.map_height,
            num_players,
        )
        .map_err(ReplayError::MapGenerationError)?;

        // Create game state
        let game_state = GameState::new(map, players, config.max_turns);

        // Load player programs as WASM bots
        let mut player_bots = Vec::with_capacity(num_players);
        for (i, program) in recording.programs.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let player_id = (i + 1) as PlayerId;

            let bot = WasmBot::from_bytes(
                &engine,
                program,
                player_id,
                config.map_width,
                config.map_height,
            )
            .map_err(|e| ReplayError::WasmLoadError { player: i, error: e })?;

            player_bots.push(ReplayPlayerBot {
                player_id,
                bot,
                eliminated: false,
            });
        }

        let mut replay_engine = Self {
            recording,
            game_state,
            player_bots,
            current_turn: 0,
        };

        // Replay to target turn if needed
        for _ in 0..target_turn {
            if replay_engine.game_state.is_game_over() {
                break;
            }
            replay_engine.execute_turn_internal();
        }

        Ok(replay_engine)
    }

    /// Get the recording.
    #[must_use]
    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    /// Get current turn number.
    #[must_use]
    pub fn turn(&self) -> u32 {
        self.current_turn
    }

    /// Get current game state.
    #[must_use]
    pub fn state(&self) -> &GameState {
        &self.game_state
    }

    /// Check if the game is over.
    #[must_use]
    pub fn is_game_over(&self) -> bool {
        self.game_state.is_game_over()
    }

    /// Step forward one turn.
    ///
    /// # Errors
    ///
    /// Returns an error if the game is already over.
    pub fn step_forward(&mut self) -> Result<(), ReplayError> {
        if self.game_state.is_game_over() {
            return Err(ReplayError::GameOver);
        }

        self.execute_turn_internal();
        Ok(())
    }

    /// Step backward one turn.
    ///
    /// This replays from turn 0 to (`current_turn` - 1).
    ///
    /// # Errors
    ///
    /// Returns an error if already at turn 0 or initialization fails.
    pub fn step_backward(&mut self) -> Result<(), ReplayError> {
        if self.current_turn == 0 {
            return Err(ReplayError::TurnOutOfBounds {
                requested: 0,
                max_turn: 0,
            });
        }

        let target = self.current_turn - 1;
        self.goto_turn(target)
    }

    /// Jump to a specific turn.
    ///
    /// This replays from turn 0 to the target turn.
    ///
    /// # Errors
    ///
    /// Returns an error if the turn is out of bounds or initialization fails.
    pub fn goto_turn(&mut self, target_turn: u32) -> Result<(), ReplayError> {
        if target_turn > self.recording.config.max_turns {
            return Err(ReplayError::TurnOutOfBounds {
                requested: target_turn,
                max_turn: self.recording.config.max_turns,
            });
        }

        // Take the recording and create fresh state
        let recording = self.recording.clone();
        *self = Self::new_at_turn(recording, target_turn)?;
        Ok(())
    }

    /// Render current state to ASCII for terminal viewing.
    #[must_use]
    pub fn render_ascii(&self) -> String {
        render_ascii(&self.game_state, self.current_turn)
    }

    /// Render current state to structured text for LLM consumption.
    #[must_use]
    pub fn render_llm(&self) -> String {
        render_llm(&self.game_state, self.current_turn)
    }

    /// Execute a single turn (internal, no error handling).
    fn execute_turn_internal(&mut self) {
        let fuel_budget = self.recording.config.fuel_budget;
        let all_stats = self.game_state.compute_all_player_stats();

        // Collect commands from all alive bots
        let mut turn_commands: Vec<(PlayerId, Vec<Command>)> = Vec::new();

        for bot in &mut self.player_bots {
            if bot.eliminated {
                continue;
            }

            match bot.bot.run_turn(fuel_budget, &self.game_state, &all_stats) {
                Ok((_, commands)) => {
                    turn_commands.push((bot.player_id, commands));
                }
                Err(_) => {
                    // Bot failed - no commands
                    turn_commands.push((bot.player_id, Vec::new()));
                }
            }
        }

        // Apply commands in deterministic order
        turn_commands.sort_by_key(|(pid, _)| *pid);
        for (player_id, commands) in turn_commands {
            for command in commands {
                self.apply_command(player_id, command);
            }
        }

        // Game state updates
        self.game_state.process_combat();
        self.game_state.process_economy();
        let all_stats = self.game_state.compute_all_player_stats();
        self.game_state.check_eliminations(&all_stats);

        // Update eliminated status
        for bot in &mut self.player_bots {
            if !bot.eliminated {
                let alive = self
                    .game_state
                    .get_player(bot.player_id)
                    .is_some_and(|p| p.alive);
                if !alive {
                    bot.eliminated = true;
                }
            }
        }

        // Advance turn
        self.game_state.advance_turn();
        self.current_turn += 1;
    }

    /// Apply a single command.
    fn apply_command(&mut self, player_id: PlayerId, command: Command) {
        match command {
            Command::Move { from, to, count } => {
                let can_move = self
                    .game_state
                    .map
                    .get(from)
                    .is_some_and(|t| t.owner == Some(player_id) && t.army >= count);

                if can_move {
                    process_attack(&mut self.game_state.map, from, to, count);
                }
            }
            Command::Convert { city, count } => {
                if let Some(tile) = self.game_state.map.get_mut(city)
                    && tile.owner == Some(player_id)
                        && tile.tile_type == TileType::City
                        && tile.population >= count
                    {
                        tile.population -= count;
                        tile.army += count;
                    }
            }
            Command::MoveCapital { new_capital } => {
                self.game_state.try_move_capital(player_id, new_capital);
            }
            Command::Yield => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_recording_new() {
        let programs = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let config = TournamentConfig::default();
        let recording = Recording::new(42, programs.clone(), config);

        assert_eq!(recording.seed, 42);
        assert_eq!(recording.programs.len(), 2);
        assert_eq!(recording.programs[0], vec![1, 2, 3]);
    }

    #[test]
    fn test_recording_save_load_roundtrip() {
        let programs = vec![vec![1, 2, 3, 4, 5], vec![10, 20, 30]];
        let config = TournamentConfig {
            max_turns: 500,
            fuel_budget: 50_000,
            map_width: 32,
            map_height: 32,
        };
        let recording = Recording::new(123456789, programs.clone(), config);

        // Save to temp file
        let temp_file = NamedTempFile::new().expect("create temp file");
        recording.save(temp_file.path()).expect("save recording");

        // Load back
        let loaded = Recording::load(temp_file.path()).expect("load recording");

        assert_eq!(loaded.seed, recording.seed);
        assert_eq!(loaded.programs, recording.programs);
        assert_eq!(loaded.config.max_turns, recording.config.max_turns);
        assert_eq!(loaded.config.fuel_budget, recording.config.fuel_budget);
        assert_eq!(loaded.config.map_width, recording.config.map_width);
        assert_eq!(loaded.config.map_height, recording.config.map_height);
    }

    #[test]
    fn test_replay_error_display() {
        let err = ReplayError::TurnOutOfBounds {
            requested: 1500,
            max_turn: 1000,
        };
        assert!(format!("{err}").contains("1500"));
        assert!(format!("{err}").contains("1000"));

        let err = ReplayError::GameOver;
        assert!(format!("{err}").contains("over"));
    }
}
