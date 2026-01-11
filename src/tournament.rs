//! Tournament runner for Ensi games.
//!
//! Provides a pure function interface: `(seed, programs) -> GameResult`
//!
//! The tournament runner handles:
//! - WASM loading for player programs
//! - Deterministic map generation
//! - Parallel bot execution with rayon
//! - Command collection and sequential application
//! - Game state updates (combat, economy, eliminations)

// Tournament uses intentional casts and Result patterns
#![allow(clippy::cast_possible_truncation, clippy::unnecessary_wraps)]

mod mapgen;

pub use mapgen::{generate_map, MapGenError};

use crate::game::{
    process_attack, Command, GameState, PlayerId, TileType, MAX_COMMANDS_PER_TURN,
};
use crate::wasm::{WasmBot, WasmError, DEFAULT_FUEL_BUDGET};
use crate::TurnResult;
use wasmtime::{Engine, Module};

/// Maximum number of players in a tournament.
pub const MAX_PLAYERS: usize = 8;

/// A player program as WASM bytes.
#[derive(Debug, Clone)]
pub struct PlayerProgram {
    /// Raw WASM bytes for the program.
    pub wasm_bytes: Vec<u8>,
}

impl PlayerProgram {
    /// Create a new player program from WASM bytes.
    #[must_use]
    pub fn new(wasm_bytes: Vec<u8>) -> Self {
        Self { wasm_bytes }
    }

    /// Compile this program into a reusable module.
    ///
    /// # Errors
    ///
    /// Returns an error if WASM compilation fails.
    pub fn compile(&self, engine: &Engine) -> Result<CompiledProgram, WasmError> {
        let module = Module::new(engine, &self.wasm_bytes)?;
        Ok(CompiledProgram { module })
    }
}

/// A pre-compiled WASM program.
///
/// Compiling WASM is expensive (~3% of CPU per-game). Pre-compile programs
/// once at tournament start, then reuse across many games.
#[derive(Clone)]
pub struct CompiledProgram {
    /// The compiled wasmtime module.
    module: Module,
}

impl std::fmt::Debug for CompiledProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledProgram").finish_non_exhaustive()
    }
}

impl CompiledProgram {
    /// Get a reference to the compiled module.
    #[must_use]
    pub fn module(&self) -> &Module {
        &self.module
    }
}

/// Configuration for the tournament.
#[derive(Debug, Clone, Copy)]
pub struct TournamentConfig {
    /// Maximum turns before game ends.
    pub max_turns: u32,
    /// Fuel budget per turn per player.
    pub fuel_budget: u64,
    /// Map width.
    pub map_width: u16,
    /// Map height.
    pub map_height: u16,
}

impl Default for TournamentConfig {
    fn default() -> Self {
        Self {
            max_turns: 1000,
            fuel_budget: DEFAULT_FUEL_BUDGET,
            map_width: 64,
            map_height: 64,
        }
    }
}

/// Statistics for a single player.
#[derive(Debug, Clone, Copy)]
pub struct PlayerStats {
    /// Player identifier.
    pub player_id: PlayerId,
    /// Total fuel consumed across all turns.
    pub total_fuel_consumed: u64,
    /// Number of turns where the bot trapped.
    pub trap_count: u32,
    /// Turn the player was eliminated (None if survived).
    pub eliminated_turn: Option<u32>,
    /// Final score.
    pub final_score: f64,
}

/// Final result of a game.
#[derive(Debug, Clone)]
pub struct GameResult {
    /// The winning player (None if draw or all eliminated).
    pub winner: Option<PlayerId>,
    /// Final scores for all players.
    pub scores: Vec<f64>,
    /// Total turns played.
    pub turns_played: u32,
    /// Per-player statistics.
    pub player_stats: Vec<PlayerStats>,
    /// Elimination order (first eliminated is index 0).
    pub elimination_order: Vec<PlayerId>,
    /// The seed used for this game.
    pub seed: u64,
}

/// Error type for tournament operations.
#[derive(Debug)]
pub enum TournamentError {
    /// Not enough players (minimum 2).
    TooFewPlayers(usize),
    /// Too many players (maximum 8).
    TooManyPlayers(usize),
    /// Failed to load WASM for a player.
    WasmLoadError {
        /// Which player (0-indexed).
        player: usize,
        /// Error details.
        error: WasmError,
    },
    /// Map generation failed.
    MapGenerationError(MapGenError),
    /// WASM engine creation failed.
    EngineError(WasmError),
}

impl std::fmt::Display for TournamentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewPlayers(n) => write!(f, "Too few players: {n} (minimum 2)"),
            Self::TooManyPlayers(n) => write!(f, "Too many players: {n} (maximum 8)"),
            Self::WasmLoadError { player, error } => {
                write!(f, "Failed to load WASM for player {player}: {error}")
            }
            Self::MapGenerationError(e) => write!(f, "Map generation failed: {e}"),
            Self::EngineError(e) => write!(f, "WASM engine error: {e}"),
        }
    }
}

impl std::error::Error for TournamentError {}

/// State for a single player's WASM bot.
struct PlayerBot {
    /// Player identifier.
    player_id: PlayerId,
    /// WASM bot instance.
    bot: WasmBot,
    /// Whether this player has been eliminated.
    eliminated: bool,
    /// Total fuel consumed.
    total_fuel_consumed: u64,
    /// Number of traps encountered.
    trap_count: u32,
    /// Turn when eliminated.
    eliminated_turn: Option<u32>,
}

/// Collected commands from a single player's turn.
#[derive(Clone, Copy)]
struct TurnCommands {
    /// Player identifier.
    player_id: PlayerId,
    /// Commands issued this turn (fixed array, no heap).
    commands: [Command; MAX_COMMANDS_PER_TURN],
    /// Number of valid commands.
    command_count: u16,
}

impl TurnCommands {
    /// Get the commands as a slice.
    #[inline]
    fn commands(&self) -> &[Command] {
        &self.commands[..self.command_count as usize]
    }
}

/// Run a complete game with the given seed and player programs.
///
/// This is the main entry point - a pure function from inputs to result.
///
/// # Arguments
///
/// * `seed` - Random seed for deterministic map generation
/// * `programs` - WASM programs for each player (2-8 players)
/// * `config` - Tournament configuration
///
/// # Determinism
///
/// Given the same seed and programs, this function always produces
/// the same `GameResult`.
///
/// # Errors
///
/// Returns an error if:
/// - Number of players is outside valid range (2-8)
/// - Any WASM program fails to load
/// - Map generation fails
pub fn run_game(
    seed: u64,
    programs: &[PlayerProgram],
    config: &TournamentConfig,
) -> Result<GameResult, TournamentError> {
    let engine = WasmBot::create_engine().map_err(TournamentError::EngineError)?;
    let runner = GameRunner::new_from_bytes(seed, programs, config, &engine)?;
    runner.run()
}

/// Run a complete game using pre-compiled WASM modules.
///
/// This is faster than `run_game` because it skips WASM compilation.
/// Use this when running many games with the same bot programs.
///
/// # Arguments
///
/// * `seed` - Random seed for deterministic map generation
/// * `engine` - Pre-created wasmtime engine
/// * `modules` - Pre-compiled WASM modules for each player (2-8 players)
/// * `config` - Tournament configuration
///
/// # Errors
///
/// Returns an error if:
/// - Number of players is outside valid range (2-8)
/// - Bot instantiation fails
/// - Map generation fails
pub fn run_game_with_modules(
    seed: u64,
    engine: &Engine,
    modules: &[CompiledProgram],
    config: &TournamentConfig,
) -> Result<GameResult, TournamentError> {
    let runner = GameRunner::new_from_modules(seed, engine, modules, config)?;
    runner.run()
}

/// The main game runner that orchestrates the tournament.
struct GameRunner {
    /// Game state.
    game_state: GameState,
    /// Per-player bot states.
    player_bots: Vec<PlayerBot>,
    /// Configuration.
    config: TournamentConfig,
    /// Original seed.
    seed: u64,
    /// Elimination order tracking.
    elimination_order: Vec<PlayerId>,
    /// Cached player stats from end of previous turn (avoids recomputation).
    cached_stats: Option<crate::game::AllPlayerStats>,
}

impl GameRunner {
    /// Create a new game runner from raw WASM bytes.
    fn new_from_bytes(
        seed: u64,
        programs: &[PlayerProgram],
        config: &TournamentConfig,
        engine: &Engine,
    ) -> Result<Self, TournamentError> {
        let num_players = programs.len();

        if num_players < 2 {
            return Err(TournamentError::TooFewPlayers(num_players));
        }
        if num_players > 8 {
            return Err(TournamentError::TooManyPlayers(num_players));
        }

        // Generate map
        let (map, players) = generate_map(seed, config.map_width, config.map_height, num_players)
            .map_err(TournamentError::MapGenerationError)?;

        // Create game state
        let game_state = GameState::new(map, players, config.max_turns);

        // Load player programs as WASM bots (compiles each module)
        let mut player_bots = Vec::with_capacity(num_players);
        for (i, program) in programs.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let player_id = (i + 1) as PlayerId;

            let bot = WasmBot::from_bytes(
                engine,
                &program.wasm_bytes,
                player_id,
                config.map_width,
                config.map_height,
            )
            .map_err(|e| TournamentError::WasmLoadError { player: i, error: e })?;

            player_bots.push(PlayerBot {
                player_id,
                bot,
                eliminated: false,
                total_fuel_consumed: 0,
                trap_count: 0,
                eliminated_turn: None,
            });
        }

        Ok(Self {
            game_state,
            player_bots,
            config: *config,
            seed,
            elimination_order: Vec::new(),
            cached_stats: None,
        })
    }

    /// Create a new game runner from pre-compiled WASM modules.
    ///
    /// This is faster than `new_from_bytes` because it skips WASM compilation.
    fn new_from_modules(
        seed: u64,
        engine: &Engine,
        modules: &[CompiledProgram],
        config: &TournamentConfig,
    ) -> Result<Self, TournamentError> {
        let num_players = modules.len();

        if num_players < 2 {
            return Err(TournamentError::TooFewPlayers(num_players));
        }
        if num_players > 8 {
            return Err(TournamentError::TooManyPlayers(num_players));
        }

        // Generate map
        let (map, players) = generate_map(seed, config.map_width, config.map_height, num_players)
            .map_err(TournamentError::MapGenerationError)?;

        // Create game state
        let game_state = GameState::new(map, players, config.max_turns);

        // Instantiate player bots from pre-compiled modules (no compilation)
        let mut player_bots = Vec::with_capacity(num_players);
        for (i, compiled) in modules.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let player_id = (i + 1) as PlayerId;

            let bot = WasmBot::from_module(
                engine,
                compiled.module(),
                player_id,
                config.map_width,
                config.map_height,
            )
            .map_err(|e| TournamentError::WasmLoadError { player: i, error: e })?;

            player_bots.push(PlayerBot {
                player_id,
                bot,
                eliminated: false,
                total_fuel_consumed: 0,
                trap_count: 0,
                eliminated_turn: None,
            });
        }

        Ok(Self {
            game_state,
            player_bots,
            config: *config,
            seed,
            elimination_order: Vec::new(),
            cached_stats: None,
        })
    }

    /// Run the complete game to completion.
    fn run(mut self) -> Result<GameResult, TournamentError> {
        while !self.game_state.is_game_over() {
            self.execute_turn();
        }

        Ok(self.build_result())
    }

    /// Execute a single turn for all players.
    fn execute_turn(&mut self) {
        let turn = self.game_state.turn();

        // Phase 1: Execute all bots and collect commands
        let turn_commands = self.execute_bots();

        // Phase 2: Sequential command application (deterministic ordering)
        self.apply_commands_sequential(turn_commands);

        // Phase 3: Game state updates
        self.game_state.process_combat();
        self.game_state.process_economy();

        // Compute stats after combat/economy for elimination check
        // Cache these for next turn's execute_bots (avoids duplicate computation)
        let all_stats = self.game_state.compute_all_player_stats();
        self.game_state.check_eliminations(&all_stats);
        self.cached_stats = Some(all_stats);

        // Track eliminations this turn
        self.update_eliminations(turn);

        // Phase 4: Advance turn counter
        self.game_state.advance_turn();
    }

    /// Execute all player bots and collect commands.
    fn execute_bots(&mut self) -> Vec<TurnCommands> {
        let fuel_budget = self.config.fuel_budget;
        let game_state = &self.game_state;

        // Use cached stats from end of previous turn, or compute if first turn
        let all_stats = self.cached_stats.take().unwrap_or_else(|| {
            game_state.compute_all_player_stats()
        });

        // Execute each alive bot and collect results
        let mut turn_commands_vec = Vec::new();

        for player_bot in &mut self.player_bots {
            if player_bot.eliminated {
                continue;
            }

            // Run the bot
            let result = player_bot.bot.run_turn(fuel_budget, game_state, &all_stats);

            if let Ok((turn_result, commands)) = result {
                let trapped = matches!(turn_result, TurnResult::Trap(_));
                let fuel_consumed = match turn_result {
                    TurnResult::BudgetExhausted { remaining } => {
                        fuel_budget.saturating_sub(u64::from(remaining))
                    }
                    TurnResult::Trap(_) => fuel_budget,
                };

                // Copy commands to fixed array
                let mut cmd_array = [Command::Yield; MAX_COMMANDS_PER_TURN];
                let cmd_count = commands.len().min(MAX_COMMANDS_PER_TURN);
                cmd_array[..cmd_count].copy_from_slice(&commands[..cmd_count]);

                turn_commands_vec.push(TurnCommands {
                    player_id: player_bot.player_id,
                    commands: cmd_array,
                    command_count: cmd_count as u16,
                });

                player_bot.total_fuel_consumed += fuel_consumed;
                if trapped {
                    player_bot.trap_count += 1;
                }
            } else {
                // Bot execution failed - treat as trap
                turn_commands_vec.push(TurnCommands {
                    player_id: player_bot.player_id,
                    commands: [Command::Yield; MAX_COMMANDS_PER_TURN],
                    command_count: 0,
                });
                player_bot.trap_count += 1;
                player_bot.total_fuel_consumed += fuel_budget;
            }
        }

        turn_commands_vec
    }

    /// Apply commands from all players in deterministic order.
    fn apply_commands_sequential(&mut self, mut turn_commands: Vec<TurnCommands>) {
        // Sort by player_id for deterministic ordering
        turn_commands.sort_by_key(|tc| tc.player_id);

        for tc in turn_commands {
            for &command in tc.commands() {
                self.apply_command(tc.player_id, command);
            }
        }
    }

    /// Apply a single command to the game state.
    fn apply_command(&mut self, player_id: PlayerId, command: Command) {
        match command {
            Command::Move { from, to, count } => {
                // Verify source tile has enough army to move
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
            Command::Yield => {
                // No state change needed
            }
        }
    }

    /// Update elimination tracking after game state changes.
    fn update_eliminations(&mut self, turn: u32) {
        for bot in &mut self.player_bots {
            if bot.eliminated {
                continue;
            }

            // Check if player is now dead
            let player_alive = self
                .game_state
                .get_player(bot.player_id)
                .is_some_and(|p| p.alive);

            if !player_alive {
                bot.eliminated = true;
                bot.eliminated_turn = Some(turn);
                self.elimination_order.push(bot.player_id);
            }
        }
    }

    /// Build the final game result.
    fn build_result(self) -> GameResult {
        // Calculate final scores
        let scores: Vec<f64> = self
            .player_bots
            .iter()
            .map(|bot| self.game_state.calculate_score(bot.player_id))
            .collect();

        // Build player stats
        let player_stats: Vec<PlayerStats> = self
            .player_bots
            .iter()
            .enumerate()
            .map(|(i, bot)| PlayerStats {
                player_id: bot.player_id,
                total_fuel_consumed: bot.total_fuel_consumed,
                trap_count: bot.trap_count,
                eliminated_turn: bot.eliminated_turn,
                final_score: scores[i],
            })
            .collect();

        // Determine winner (highest score among alive players)
        let winner = self
            .player_bots
            .iter()
            .filter(|bot| !bot.eliminated)
            .max_by(|a, b| {
                let score_a = self.game_state.calculate_score(a.player_id);
                let score_b = self.game_state.calculate_score(b.player_id);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|bot| bot.player_id);

        GameResult {
            winner,
            scores,
            turns_played: self.game_state.turn(),
            player_stats,
            elimination_order: self.elimination_order,
            seed: self.seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tournament_error_display() {
        let err = TournamentError::TooFewPlayers(1);
        assert!(format!("{err}").contains("Too few players"));

        let err = TournamentError::TooManyPlayers(10);
        assert!(format!("{err}").contains("Too many players"));
    }

    #[test]
    fn test_tournament_config_default() {
        let config = TournamentConfig::default();
        assert_eq!(config.max_turns, 1000);
        assert_eq!(config.fuel_budget, DEFAULT_FUEL_BUDGET);
        assert_eq!(config.map_width, 64);
        assert_eq!(config.map_height, 64);
    }

    #[test]
    fn test_run_game_too_few_players() {
        let programs = vec![PlayerProgram::new(vec![])];
        let config = TournamentConfig::default();
        let result = run_game(42, &programs, &config);
        assert!(matches!(result, Err(TournamentError::TooFewPlayers(1))));
    }

    #[test]
    fn test_run_game_too_many_players() {
        let programs: Vec<_> = (0..9).map(|_| PlayerProgram::new(vec![])).collect();
        let config = TournamentConfig::default();
        let result = run_game(42, &programs, &config);
        assert!(matches!(result, Err(TournamentError::TooManyPlayers(9))));
    }
}
