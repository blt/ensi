//! Tournament runner for Ensi games.
//!
//! Provides a pure function interface: `(seed, programs) -> GameResult`
//!
//! The tournament runner handles:
//! - ELF loading for player programs
//! - Deterministic map generation
//! - Parallel VM execution with rayon
//! - Command collection and sequential application
//! - Game state updates (combat, economy, eliminations)

mod elf;
mod mapgen;

pub use elf::{load_elf, ElfLoadError, STACK_TOP, TEXT_BASE};
pub use mapgen::{generate_map, MapGenError};

use crate::game::{
    process_attack, Command, GameState, GameSyscallHandler, PlayerId, TileType,
};
use crate::vm::{Cpu, Memory};
use crate::{MeteringConfig, TurnResult, Vm};
use rayon::prelude::*;
use std::sync::{Arc, RwLock};

/// A player program as raw bytes.
#[derive(Debug, Clone)]
pub struct PlayerProgram {
    /// Raw ELF bytes for the program.
    pub elf_bytes: Vec<u8>,
}

impl PlayerProgram {
    /// Create a new player program from ELF bytes.
    #[must_use]
    pub fn new(elf_bytes: Vec<u8>) -> Self {
        Self { elf_bytes }
    }
}

/// Configuration for the tournament.
#[derive(Debug, Clone, Copy)]
pub struct TournamentConfig {
    /// Maximum turns before game ends.
    pub max_turns: u32,
    /// Instruction budget per turn per player.
    pub turn_budget: u32,
    /// Memory size for each VM (bytes).
    pub vm_memory_size: u32,
    /// Memory base address.
    pub vm_memory_base: u32,
    /// Map width.
    pub map_width: u16,
    /// Map height.
    pub map_height: u16,
}

impl Default for TournamentConfig {
    fn default() -> Self {
        Self {
            max_turns: 1000,
            turn_budget: 100_000,
            vm_memory_size: 1024 * 1024, // 1 MiB
            vm_memory_base: TEXT_BASE,
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
    /// Total instructions executed across all turns.
    pub total_instructions: u64,
    /// Number of turns where the VM trapped.
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
#[derive(Debug, Clone)]
pub enum TournamentError {
    /// Not enough players (minimum 2).
    TooFewPlayers(usize),
    /// Too many players (maximum 8).
    TooManyPlayers(usize),
    /// Failed to load ELF for a player.
    ElfLoadError {
        /// Which player (0-indexed).
        player: usize,
        /// Error details.
        error: ElfLoadError,
    },
    /// Map generation failed.
    MapGenerationError(MapGenError),
}

impl std::fmt::Display for TournamentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewPlayers(n) => write!(f, "Too few players: {n} (minimum 2)"),
            Self::TooManyPlayers(n) => write!(f, "Too many players: {n} (maximum 8)"),
            Self::ElfLoadError { player, error } => {
                write!(f, "Failed to load ELF for player {player}: {error}")
            }
            Self::MapGenerationError(e) => write!(f, "Map generation failed: {e}"),
        }
    }
}

impl std::error::Error for TournamentError {}

/// State for a single player's VM that persists across turns.
struct PlayerVm {
    /// Player identifier.
    player_id: PlayerId,
    /// CPU state (registers + PC).
    cpu: Cpu,
    /// Memory state.
    memory: Memory,
    /// Whether this player has been eliminated.
    eliminated: bool,
    /// Total instructions executed.
    total_instructions: u64,
    /// Number of traps encountered.
    trap_count: u32,
    /// Turn when eliminated.
    eliminated_turn: Option<u32>,
}

impl PlayerVm {
    /// Create a new `PlayerVm` from an ELF program.
    fn from_elf(
        player_id: PlayerId,
        elf_bytes: &[u8],
        memory_size: u32,
        memory_base: u32,
    ) -> Result<Self, ElfLoadError> {
        let (cpu, memory) = load_elf(elf_bytes, memory_size, memory_base)?;

        Ok(Self {
            player_id,
            cpu,
            memory,
            eliminated: false,
            total_instructions: 0,
            trap_count: 0,
            eliminated_turn: None,
        })
    }
}

/// Collected commands from a single player's turn.
struct TurnCommands {
    /// Player identifier.
    player_id: PlayerId,
    /// Commands issued this turn.
    commands: Vec<Command>,
    /// Whether the player yielded.
    #[allow(dead_code)]
    yielded: bool,
    /// Instructions executed this turn.
    #[allow(dead_code)]
    instructions_executed: u64,
    /// Whether a trap occurred.
    trapped: bool,
}

/// Run a complete game with the given seed and player programs.
///
/// This is the main entry point - a pure function from inputs to result.
///
/// # Arguments
///
/// * `seed` - Random seed for deterministic map generation and any RNG
/// * `programs` - ELF programs for each player (2-8 players)
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
/// - Any ELF program fails to load
/// - Map generation fails
pub fn run_game(
    seed: u64,
    programs: &[PlayerProgram],
    config: &TournamentConfig,
) -> Result<GameResult, TournamentError> {
    let runner = GameRunner::new(seed, programs, config)?;
    runner.run()
}

/// The main game runner that orchestrates the tournament.
struct GameRunner {
    /// Game state.
    game_state: GameState,
    /// Per-player VM states.
    player_vms: Vec<PlayerVm>,
    /// Configuration.
    config: TournamentConfig,
    /// Original seed.
    seed: u64,
    /// Elimination order tracking.
    elimination_order: Vec<PlayerId>,
}

impl GameRunner {
    /// Create a new game runner.
    fn new(
        seed: u64,
        programs: &[PlayerProgram],
        config: &TournamentConfig,
    ) -> Result<Self, TournamentError> {
        let num_players = programs.len();

        if num_players < 2 {
            return Err(TournamentError::TooFewPlayers(num_players));
        }
        if num_players > 8 {
            return Err(TournamentError::TooManyPlayers(num_players));
        }

        // Generate map
        let (map, players) =
            generate_map(seed, config.map_width, config.map_height, num_players)
                .map_err(TournamentError::MapGenerationError)?;

        // Create game state
        let game_state = GameState::new(map, players, config.max_turns);

        // Load player programs
        let mut player_vms = Vec::with_capacity(num_players);
        for (i, program) in programs.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let player_id = (i + 1) as PlayerId;

            let vm = PlayerVm::from_elf(
                player_id,
                &program.elf_bytes,
                config.vm_memory_size,
                config.vm_memory_base,
            )
            .map_err(|e| TournamentError::ElfLoadError { player: i, error: e })?;

            player_vms.push(vm);
        }

        Ok(Self {
            game_state,
            player_vms,
            config: *config,
            seed,
            elimination_order: Vec::new(),
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

        // Phase 1: Execute all VMs (parallel or sequential based on alive count)
        let turn_commands = self.execute_vms();

        // Phase 2: Sequential command application (deterministic ordering)
        self.apply_commands_sequential(turn_commands);

        // Phase 3: Game state updates
        self.game_state.process_combat();
        self.game_state.process_economy();
        self.game_state.check_eliminations();

        // Track eliminations this turn
        self.update_eliminations(turn);

        // Phase 4: Advance turn counter
        self.game_state.advance_turn();
    }

    /// Execute all player VMs and collect commands.
    fn execute_vms(&mut self) -> Vec<TurnCommands> {
        let game_state_arc = Arc::new(RwLock::new(self.game_state.clone()));
        let turn_budget = self.config.turn_budget;

        // Collect indices of alive players
        let alive_indices: Vec<usize> = self
            .player_vms
            .iter()
            .enumerate()
            .filter(|(_, vm)| !vm.eliminated)
            .map(|(i, _)| i)
            .collect();

        // Execute VMs in parallel
        let results: Vec<(usize, TurnCommands, Cpu, Memory, u64)> = alive_indices
            .into_par_iter()
            .map(|idx| {
                let vm = &self.player_vms[idx];
                let player_id = vm.player_id;

                // Create syscall handler with shared game state
                let handler =
                    GameSyscallHandler::new(player_id, Arc::clone(&game_state_arc));

                // Create temporary VM
                let mut temp_vm = Vm::from_parts(
                    vm.cpu.clone(),
                    vm.memory.clone(),
                    handler,
                    MeteringConfig::default(),
                );

                // Execute turn
                let turn_result = temp_vm.run_turn(turn_budget);

                // Extract state before taking handler (which consumes the VM)
                let trapped = matches!(turn_result, TurnResult::Trap(_));
                let instructions = temp_vm.total_executed();
                let cpu = temp_vm.cpu.clone();
                let memory = temp_vm.memory.clone();
                let yielded = temp_vm.handler_ref().has_yielded();

                // Now take the handler and extract commands
                let commands = temp_vm.take_handler().take_commands();

                let turn_commands = TurnCommands {
                    player_id,
                    commands,
                    yielded,
                    instructions_executed: instructions,
                    trapped,
                };

                // Return data needed to update PlayerVm
                (idx, turn_commands, cpu, memory, instructions)
            })
            .collect();

        // Update player VM states (sequential)
        let mut turn_commands_vec = Vec::with_capacity(results.len());
        for (idx, turn_commands, cpu, memory, instructions) in results {
            let vm = &mut self.player_vms[idx];
            vm.cpu = cpu;
            vm.memory = memory;
            vm.total_instructions += instructions;
            if turn_commands.trapped {
                vm.trap_count += 1;
            }
            turn_commands_vec.push(turn_commands);
        }

        turn_commands_vec
    }

    /// Apply commands from all players in deterministic order.
    fn apply_commands_sequential(&mut self, mut turn_commands: Vec<TurnCommands>) {
        // Sort by player_id for deterministic ordering
        turn_commands.sort_by_key(|tc| tc.player_id);

        for tc in turn_commands {
            for command in tc.commands {
                self.apply_command(tc.player_id, command);
            }
        }
    }

    /// Apply a single command to the game state.
    fn apply_command(&mut self, player_id: PlayerId, command: Command) {
        match command {
            Command::Move { from, to, count } => {
                // Verify source tile and remove army
                let can_move = self
                    .game_state
                    .map
                    .get(from)
                    .map_or(false, |t| t.owner == Some(player_id) && t.army >= count);

                if can_move {
                    if let Some(from_tile) = self.game_state.map.get_mut(from) {
                        from_tile.army -= count;
                    }
                    // Process attack/move to destination
                    process_attack(&mut self.game_state.map, from, to, count);
                }
            }
            Command::Convert { city, count } => {
                if let Some(tile) = self.game_state.map.get_mut(city) {
                    if tile.owner == Some(player_id)
                        && tile.tile_type == TileType::City
                        && tile.population >= count
                    {
                        tile.population -= count;
                        tile.army += count;
                    }
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
        for vm in &mut self.player_vms {
            if vm.eliminated {
                continue;
            }

            // Check if player is now dead
            let player_alive = self
                .game_state
                .get_player(vm.player_id)
                .map_or(false, |p| p.alive);

            if !player_alive {
                vm.eliminated = true;
                vm.eliminated_turn = Some(turn);
                self.elimination_order.push(vm.player_id);
            }
        }
    }

    /// Build the final game result.
    fn build_result(self) -> GameResult {
        // Calculate final scores
        let scores: Vec<f64> = self
            .player_vms
            .iter()
            .map(|vm| self.game_state.calculate_score(vm.player_id))
            .collect();

        // Build player stats
        let player_stats: Vec<PlayerStats> = self
            .player_vms
            .iter()
            .enumerate()
            .map(|(i, vm)| PlayerStats {
                player_id: vm.player_id,
                total_instructions: vm.total_instructions,
                trap_count: vm.trap_count,
                eliminated_turn: vm.eliminated_turn,
                final_score: scores[i],
            })
            .collect();

        // Determine winner (highest score among alive players, or highest score overall)
        let winner = self
            .player_vms
            .iter()
            .filter(|vm| !vm.eliminated)
            .max_by(|a, b| {
                let score_a = self.game_state.calculate_score(a.player_id);
                let score_b = self.game_state.calculate_score(b.player_id);
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|vm| vm.player_id);

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

    // A minimal RISC-V program that just yields immediately
    // This is a hand-crafted ELF that we can use for testing
    // For now, we'll test with invalid ELF to verify error handling

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
        assert_eq!(config.turn_budget, 100_000);
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

    #[test]
    fn test_run_game_invalid_elf() {
        let programs = vec![
            PlayerProgram::new(vec![0, 1, 2, 3]),
            PlayerProgram::new(vec![0, 1, 2, 3]),
        ];
        let config = TournamentConfig::default();
        let result = run_game(42, &programs, &config);
        assert!(matches!(
            result,
            Err(TournamentError::ElfLoadError { player: 0, .. })
        ));
    }
}
