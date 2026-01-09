//! Game replay and viewing system.
//!
//! Because Ensi games are 100% deterministic, replay requires only:
//! - `seed: u64` - The random seed for map generation
//! - `programs: Vec<Vec<u8>>` - ELF bytes for each player
//!
//! No state deltas needed. To view turn N, re-run the simulation from turn 0 to N.
//!
//! # Time Travel
//!
//! - **Forward**: Continue stepping the simulation
//! - **Backward**: Re-run from turn 0 to (current_turn - 1)
//! - **Jump to turn N**: Re-run from turn 0 to N

mod render;
mod text;

pub use render::render_ascii;
pub use text::render_llm;

use crate::game::{GameState, PlayerId};
use crate::tournament::{
    generate_map, load_elf, ElfLoadError, MapGenError, PlayerProgram, TournamentConfig,
};
use crate::vm::{Cpu, Memory};
use crate::game::{process_attack, Command, GameSyscallHandler, TileType};
use crate::{MeteringConfig, Vm};
use rayon::prelude::*;
use std::sync::{Arc, RwLock};
use std::io::{self, Read as IoRead, Write as IoWrite};
use std::path::Path;
use std::fs::File;

/// Minimal recording - just seed, programs, and config.
///
/// Because the game is deterministic, this is all we need to replay.
#[derive(Debug, Clone)]
pub struct Recording {
    /// Random seed for map generation.
    pub seed: u64,
    /// ELF bytes for each player.
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

    /// Create from PlayerProgram slice (convenience).
    #[must_use]
    pub fn from_programs(seed: u64, programs: &[PlayerProgram], config: TournamentConfig) -> Self {
        Self {
            seed,
            programs: programs.iter().map(|p| p.elf_bytes.clone()).collect(),
            config,
        }
    }

    /// Save recording to a file.
    ///
    /// Format: Simple binary format:
    /// - 8 bytes: seed (little-endian)
    /// - 4 bytes: num_programs (little-endian u32)
    /// - For each program:
    ///   - 4 bytes: program length (little-endian u32)
    ///   - N bytes: program ELF bytes
    /// - TournamentConfig (fixed size, just the raw bytes)
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

        // Write config (each field separately for portability)
        file.write_all(&self.config.max_turns.to_le_bytes())?;
        file.write_all(&self.config.turn_budget.to_le_bytes())?;
        file.write_all(&self.config.vm_memory_size.to_le_bytes())?;
        file.write_all(&self.config.vm_memory_base.to_le_bytes())?;
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
        let mut u16_bytes = [0u8; 2];

        file.read_exact(&mut u32_bytes)?;
        let max_turns = u32::from_le_bytes(u32_bytes);

        file.read_exact(&mut u32_bytes)?;
        let turn_budget = u32::from_le_bytes(u32_bytes);

        file.read_exact(&mut u32_bytes)?;
        let vm_memory_size = u32::from_le_bytes(u32_bytes);

        file.read_exact(&mut u32_bytes)?;
        let vm_memory_base = u32::from_le_bytes(u32_bytes);

        file.read_exact(&mut u16_bytes)?;
        let map_width = u16::from_le_bytes(u16_bytes);

        file.read_exact(&mut u16_bytes)?;
        let map_height = u16::from_le_bytes(u16_bytes);

        let config = TournamentConfig {
            max_turns,
            turn_budget,
            vm_memory_size,
            vm_memory_base,
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
#[derive(Debug, Clone)]
pub enum ReplayError {
    /// Failed to load ELF for a player.
    ElfLoadError {
        /// Which player (0-indexed).
        player: usize,
        /// Error details.
        error: ElfLoadError,
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
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ElfLoadError { player, error } => {
                write!(f, "Failed to load ELF for player {player}: {error}")
            }
            Self::MapGenerationError(e) => write!(f, "Map generation failed: {e}"),
            Self::TurnOutOfBounds { requested, max_turn } => {
                write!(f, "Turn {requested} out of bounds (max: {max_turn})")
            }
            Self::GameOver => write!(f, "Game is already over"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// State for a single player's VM during replay.
#[derive(Debug)]
struct ReplayPlayerVm {
    /// Player identifier.
    player_id: PlayerId,
    /// CPU state (registers + PC).
    cpu: Cpu,
    /// Memory state.
    memory: Memory,
    /// Whether this player has been eliminated.
    eliminated: bool,
}

impl ReplayPlayerVm {
    /// Create a new ReplayPlayerVm from ELF bytes.
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
        })
    }
}

/// Replay engine - steps through game deterministically.
///
/// Since games are deterministic, this engine can:
/// - Step forward by executing one turn
/// - Step backward by replaying from turn 0
/// - Jump to any turn by replaying from turn 0
#[derive(Debug)]
pub struct ReplayEngine {
    /// The recording being replayed.
    recording: Recording,
    /// Current game state.
    game_state: GameState,
    /// Per-player VM states.
    player_vms: Vec<ReplayPlayerVm>,
    /// Current turn number.
    current_turn: u32,
}

impl ReplayEngine {
    /// Create a new replay engine from a recording, starting at turn 0.
    ///
    /// # Errors
    ///
    /// Returns an error if ELF loading or map generation fails.
    pub fn new(recording: Recording) -> Result<Self, ReplayError> {
        Self::new_at_turn(recording, 0)
    }

    /// Create a new replay engine at a specific turn.
    ///
    /// This replays from turn 0 to the target turn.
    ///
    /// # Errors
    ///
    /// Returns an error if ELF loading or map generation fails.
    pub fn new_at_turn(recording: Recording, target_turn: u32) -> Result<Self, ReplayError> {
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

        // Load player programs
        let mut player_vms = Vec::with_capacity(num_players);
        for (i, program) in recording.programs.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let player_id = (i + 1) as PlayerId;

            let vm = ReplayPlayerVm::from_elf(
                player_id,
                program,
                config.vm_memory_size,
                config.vm_memory_base,
            )
            .map_err(|e| ReplayError::ElfLoadError { player: i, error: e })?;

            player_vms.push(vm);
        }

        let mut engine = Self {
            recording,
            game_state,
            player_vms,
            current_turn: 0,
        };

        // Replay to target turn if needed
        for _ in 0..target_turn {
            if engine.game_state.is_game_over() {
                break;
            }
            engine.execute_turn_internal();
        }

        Ok(engine)
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
    /// This replays from turn 0 to (current_turn - 1).
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

        // Re-initialize from scratch
        *self = Self::new_at_turn(self.recording.clone(), target_turn)?;
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
        // Phase 1: Execute all VMs and collect commands
        let turn_commands = self.execute_vms();

        // Phase 2: Apply commands in deterministic order
        self.apply_commands(turn_commands);

        // Phase 3: Game state updates
        self.game_state.process_combat();
        self.game_state.process_economy();
        self.game_state.check_eliminations();

        // Update eliminated status in VMs
        for vm in &mut self.player_vms {
            if !vm.eliminated {
                let alive = self
                    .game_state
                    .get_player(vm.player_id)
                    .map_or(false, |p| p.alive);
                if !alive {
                    vm.eliminated = true;
                }
            }
        }

        // Phase 4: Advance turn
        self.game_state.advance_turn();
        self.current_turn += 1;
    }

    /// Execute all player VMs and collect commands.
    fn execute_vms(&mut self) -> Vec<(PlayerId, Vec<Command>)> {
        let game_state_arc = Arc::new(RwLock::new(self.game_state.clone()));
        let turn_budget = self.recording.config.turn_budget;

        // Collect indices of alive players
        let alive_indices: Vec<usize> = self
            .player_vms
            .iter()
            .enumerate()
            .filter(|(_, vm)| !vm.eliminated)
            .map(|(i, _)| i)
            .collect();

        // Execute VMs in parallel
        let results: Vec<(usize, PlayerId, Vec<Command>, Cpu, Memory)> = alive_indices
            .into_par_iter()
            .map(|idx| {
                let vm = &self.player_vms[idx];
                let player_id = vm.player_id;

                // Create syscall handler
                let handler = GameSyscallHandler::new(player_id, Arc::clone(&game_state_arc));

                // Create temporary VM
                let mut temp_vm = Vm::from_parts(
                    vm.cpu.clone(),
                    vm.memory.clone(),
                    handler,
                    MeteringConfig::default(),
                );

                // Execute turn
                let _ = temp_vm.run_turn(turn_budget);

                // Extract state
                let cpu = temp_vm.cpu.clone();
                let memory = temp_vm.memory.clone();
                let commands = temp_vm.take_handler().take_commands();

                (idx, player_id, commands, cpu, memory)
            })
            .collect();

        // Update VM states and collect commands
        let mut turn_commands = Vec::new();
        for (idx, player_id, commands, cpu, memory) in results {
            let vm = &mut self.player_vms[idx];
            vm.cpu = cpu;
            vm.memory = memory;
            turn_commands.push((player_id, commands));
        }

        turn_commands
    }

    /// Apply commands in deterministic order.
    fn apply_commands(&mut self, mut turn_commands: Vec<(PlayerId, Vec<Command>)>) {
        // Sort by player_id for deterministic ordering
        turn_commands.sort_by_key(|(pid, _)| *pid);

        for (player_id, commands) in turn_commands {
            for command in commands {
                self.apply_command(player_id, command);
            }
        }
    }

    /// Apply a single command.
    fn apply_command(&mut self, player_id: PlayerId, command: Command) {
        match command {
            Command::Move { from, to, count } => {
                let can_move = self
                    .game_state
                    .map
                    .get(from)
                    .map_or(false, |t| t.owner == Some(player_id) && t.army >= count);

                if can_move {
                    if let Some(from_tile) = self.game_state.map.get_mut(from) {
                        from_tile.army -= count;
                    }
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
            Command::Yield => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament::TEXT_BASE;
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
            turn_budget: 50_000,
            vm_memory_size: 512 * 1024,
            vm_memory_base: TEXT_BASE,
            map_width: 32,
            map_height: 32,
        };
        let recording = Recording::new(123456789, programs.clone(), config);

        // Save to temp file
        let temp_file = NamedTempFile::new().unwrap();
        recording.save(temp_file.path()).unwrap();

        // Load back
        let loaded = Recording::load(temp_file.path()).unwrap();

        assert_eq!(loaded.seed, recording.seed);
        assert_eq!(loaded.programs, recording.programs);
        assert_eq!(loaded.config.max_turns, recording.config.max_turns);
        assert_eq!(loaded.config.turn_budget, recording.config.turn_budget);
        assert_eq!(loaded.config.vm_memory_size, recording.config.vm_memory_size);
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
