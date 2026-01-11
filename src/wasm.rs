//! WASM runtime for bot execution.
//!
//! This module provides WASM-based bot execution using wasmtime with fuel metering.
//! Bots are compiled from C/Rust to WASM and executed in a sandboxed environment.

// WASM runtime code intentionally uses casts for memory/ABI operations
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless
)]

mod host;

use crate::game::{
    AllPlayerStats, CachedPlayerStats, Command, Coord, GameState, Map, PlayerId, TileType,
    MAX_COMMANDS_PER_TURN,
};
use crate::TurnResult;
use std::path::Path;
use wasmtime::{
    Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

/// Default fuel budget per turn.
pub const DEFAULT_FUEL_BUDGET: u64 = 50_000;

/// WASM memory size limit (1 MiB).
const MEMORY_LIMIT: usize = 1024 * 1024;

/// Error type for WASM operations.
#[derive(Debug)]
pub enum WasmError {
    /// wasmtime error.
    Wasmtime(wasmtime::Error),
    /// Module missing required export.
    MissingExport(&'static str),
    /// Bot exceeded fuel budget.
    OutOfFuel,
    /// Bot trapped.
    Trap(String),
}

impl std::fmt::Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wasmtime(e) => write!(f, "wasmtime error: {e}"),
            Self::MissingExport(name) => write!(f, "missing export: {name}"),
            Self::OutOfFuel => write!(f, "out of fuel"),
            Self::Trap(msg) => write!(f, "trap: {msg}"),
        }
    }
}

impl std::error::Error for WasmError {}

impl From<wasmtime::Error> for WasmError {
    fn from(e: wasmtime::Error) -> Self {
        Self::Wasmtime(e)
    }
}

/// State passed to host functions.
///
/// This holds the game state reference and command buffer during bot execution.
///
/// SAFETY: The `map_ptr` is only valid during a `run_turn` call. It is set
/// before calling the WASM bot and cleared after. The pointer references
/// the `GameState.map` which lives on the stack in `run_game`.
pub struct BotState {
    /// Player ID this bot represents.
    player_id: PlayerId,
    /// Pre-computed player stats for O(1) lookups.
    cached_stats: CachedPlayerStats,
    /// Commands accumulated during this turn.
    commands: [Command; MAX_COMMANDS_PER_TURN],
    /// Number of commands accumulated.
    command_count: u16,
    /// Whether the bot has yielded.
    yielded: bool,
    /// Store limits.
    limits: StoreLimits,
    /// Map width (cached for host functions).
    map_width: u16,
    /// Map height (cached for host functions).
    map_height: u16,
    /// Current turn number.
    turn: u32,
    /// Player's capital coordinate.
    capital: Option<Coord>,
    /// Current capital population (for `move_capital` validation).
    capital_population: u32,
    /// Raw pointer to the map (valid only during `run_turn`).
    /// This avoids cloning the map for each callback.
    map_ptr: *const Map,
}

impl std::fmt::Debug for BotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("player_id", &self.player_id)
            .field("command_count", &self.command_count)
            .field("yielded", &self.yielded)
            .finish_non_exhaustive()
    }
}

/// Tile information returned to bots.
#[derive(Debug, Clone, Copy)]
pub struct TileInfo {
    /// Tile type (0=City, 1=Desert, 2=Mountain).
    pub tile_type: u8,
    /// Owner player ID (0 = unowned).
    pub owner: u8,
    /// Army count on tile.
    pub army: u16,
}

impl TileInfo {
    /// Pack tile info into a u32 for WASM.
    #[must_use]
    pub fn pack(self) -> u32 {
        u32::from(self.tile_type) | (u32::from(self.owner) << 8) | (u32::from(self.army) << 16)
    }

    /// Create fog-of-war tile info.
    #[must_use]
    pub fn fog() -> Self {
        Self {
            tile_type: 255,
            owner: 255,
            army: 0,
        }
    }
}

// SAFETY: BotState contains a raw pointer but we only access it during
// run_turn when the referenced data is valid. The pointer is cleared after.
unsafe impl Send for BotState {}

impl BotState {
    /// Create a new bot state.
    fn new(player_id: PlayerId, map_width: u16, map_height: u16) -> Self {
        Self {
            player_id,
            cached_stats: CachedPlayerStats::default(),
            commands: [Command::Yield; MAX_COMMANDS_PER_TURN],
            command_count: 0,
            yielded: false,
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT)
                .build(),
            map_width,
            map_height,
            turn: 0,
            capital: None,
            capital_population: 0,
            map_ptr: std::ptr::null(),
        }
    }

    /// Get accumulated commands.
    #[inline]
    fn commands(&self) -> &[Command] {
        &self.commands[..self.command_count as usize]
    }

    /// Push a command. Returns false if buffer is full.
    #[inline]
    fn push_command(&mut self, cmd: Command) -> bool {
        if (self.command_count as usize) < MAX_COMMANDS_PER_TURN {
            self.commands[self.command_count as usize] = cmd;
            self.command_count += 1;
            true
        } else {
            false
        }
    }

    /// Reset for a new turn (but keep `map_ptr`, it's set separately).
    fn reset(&mut self) {
        self.command_count = 0;
        self.yielded = false;
    }

    /// Get a reference to the map.
    ///
    /// # Safety
    ///
    /// This is only safe to call during `run_turn` when `map_ptr` is valid.
    #[inline]
    unsafe fn map(&self) -> &Map {
        debug_assert!(!self.map_ptr.is_null());
        // SAFETY: Caller guarantees map_ptr is valid
        unsafe { &*self.map_ptr }
    }

    /// Check if a tile is visible to this player (fog of war).
    ///
    /// A tile is visible if the player owns it or any adjacent tile.
    #[inline]
    pub(crate) fn can_see_tile(&self, coord: Coord) -> bool {
        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        // Check if player owns this tile
        if let Some(tile) = map.get(coord)
            && tile.owner == Some(self.player_id) {
                return true;
            }

        // Check adjacent tiles
        let (adjacent, count) = coord.adjacent(self.map_width, self.map_height);
        for adj in &adjacent[..count as usize] {
            if let Some(tile) = map.get(*adj)
                && tile.owner == Some(self.player_id) {
                    return true;
                }
        }
        false
    }

    /// Get tile info for a coordinate.
    #[inline]
    pub(crate) fn get_tile_info(&self, coord: Coord) -> Option<TileInfo> {
        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        map.get(coord).map(|tile| TileInfo {
            tile_type: match tile.tile_type {
                TileType::City => 0,
                TileType::Desert => 1,
                TileType::Mountain => 2,
            },
            owner: tile.owner.unwrap_or(0),
            army: tile.army.min(65535) as u16,
        })
    }

    /// Validate a move command.
    #[inline]
    pub(crate) fn validate_move(&self, from: Coord, to: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        let Some(from_tile) = map.get(from) else {
            return false;
        };
        if from_tile.owner != Some(self.player_id) {
            return false;
        }
        if from_tile.army < count {
            return false;
        }

        // Check adjacency
        let (adjacent, adj_count) = from.adjacent(self.map_width, self.map_height);
        let is_adjacent = adjacent[..adj_count as usize].contains(&to);
        if !is_adjacent {
            return false;
        }

        // Check destination is passable
        let Some(to_tile) = map.get(to) else {
            return false;
        };
        to_tile.tile_type.is_passable()
    }

    /// Validate a convert command.
    #[inline]
    pub(crate) fn validate_convert(&self, city: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        let Some(tile) = map.get(city) else {
            return false;
        };
        if tile.tile_type != TileType::City {
            return false;
        }
        if tile.owner != Some(self.player_id) {
            return false;
        }
        tile.population >= count
    }

    /// Validate a move capital command.
    #[inline]
    pub(crate) fn validate_move_capital(&self, new_capital: Coord) -> bool {
        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        let Some(tile) = map.get(new_capital) else {
            return false;
        };
        if tile.tile_type != TileType::City {
            return false;
        }
        if tile.owner != Some(self.player_id) {
            return false;
        }
        tile.population > self.capital_population
    }

    /// Validate an abandon command.
    ///
    /// Rules:
    /// - Player must own the tile
    /// - Cannot abandon the capital
    #[inline]
    pub(crate) fn validate_abandon(&self, coord: Coord) -> bool {
        // Cannot abandon the capital
        if self.capital == Some(coord) {
            return false;
        }

        // SAFETY: Called only during run_turn when map_ptr is valid
        let map = unsafe { self.map() };

        let Some(tile) = map.get(coord) else {
            return false;
        };

        // Must own the tile
        tile.owner == Some(self.player_id)
    }
}

/// Base address in WASM linear memory for the tile map.
/// The host writes visible tiles here before calling `run_turn`.
/// This eliminates syscall overhead for tile queries.
pub const TILE_MAP_BASE_ADDR: usize = 0x10000;

/// Header size in bytes (magic + version + width + height + turn + `player_id`).
pub const TILE_MAP_HEADER_SIZE: usize = 16;

/// A WASM-based bot.
pub struct WasmBot {
    /// wasmtime store (owns the instance data).
    store: Store<BotState>,
    /// `run_turn` function handle.
    run_turn: TypedFunc<i32, i32>,
    /// Exported memory for push-based visibility.
    memory: wasmtime::Memory,
}

impl std::fmt::Debug for WasmBot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmBot")
            .field("player_id", &self.store.data().player_id)
            .finish_non_exhaustive()
    }
}

impl WasmBot {
    /// Create a new WASM engine with fuel metering enabled and optimized settings.
    ///
    /// # Errors
    ///
    /// Returns an error if engine creation fails.
    pub fn create_engine() -> Result<Engine, WasmError> {
        let mut config = Config::new();

        // Deterministic execution via fuel metering
        config.consume_fuel(true);

        // Optimize compiled code for speed
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);

        // Use multiple threads for module compilation
        config.parallel_compilation(true);

        // Eliminate bounds checks via virtual memory guards (4 GiB reservation)
        // This provides ~20-45% faster memory operations
        config.memory_guard_size(1 << 32);

        // Enable bulk memory operations for efficient memcpy
        config.wasm_bulk_memory(true);

        Ok(Engine::new(&config)?)
    }

    /// Create a pre-configured linker with all host functions registered.
    ///
    /// Linkers are expensive to create due to host function registration.
    /// Creating one linker and sharing it across all bot instantiations saves
    /// significant overhead in tournaments (3-5% speedup).
    ///
    /// # Errors
    ///
    /// Returns an error if host function registration fails.
    pub fn create_linker(engine: &Engine) -> Result<Linker<BotState>, WasmError> {
        let mut linker = Linker::new(engine);
        host::register_host_functions(&mut linker)?;
        Ok(linker)
    }

    /// Load a bot from a WASM file.
    ///
    /// # Errors
    ///
    /// Returns an error if loading or instantiation fails.
    pub fn from_file(
        engine: &Engine,
        path: impl AsRef<Path>,
        player_id: PlayerId,
        map_width: u16,
        map_height: u16,
    ) -> Result<Self, WasmError> {
        let module = Module::from_file(engine, path)?;
        Self::from_module(engine, &module, player_id, map_width, map_height)
    }

    /// Load a bot from WASM bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if loading or instantiation fails.
    pub fn from_bytes(
        engine: &Engine,
        bytes: &[u8],
        player_id: PlayerId,
        map_width: u16,
        map_height: u16,
    ) -> Result<Self, WasmError> {
        let module = Module::new(engine, bytes)?;
        Self::from_module(engine, &module, player_id, map_width, map_height)
    }

    /// Load a bot from a compiled module.
    ///
    /// # Errors
    ///
    /// Returns an error if instantiation fails.
    pub fn from_module(
        engine: &Engine,
        module: &Module,
        player_id: PlayerId,
        map_width: u16,
        map_height: u16,
    ) -> Result<Self, WasmError> {
        // Create linker with host functions (per-bot for now)
        let mut linker = Linker::new(engine);
        host::register_host_functions(&mut linker)?;
        Self::from_module_with_linker(&linker, module, player_id, map_width, map_height)
    }

    /// Create a bot from a pre-compiled module using a shared linker.
    ///
    /// This is more efficient than `from_module` when creating multiple bots,
    /// as the linker is created once and reused.
    ///
    /// # Errors
    ///
    /// Returns an error if instantiation fails.
    pub fn from_module_with_linker(
        linker: &Linker<BotState>,
        module: &Module,
        player_id: PlayerId,
        map_width: u16,
        map_height: u16,
    ) -> Result<Self, WasmError> {
        let state = BotState::new(player_id, map_width, map_height);
        let engine = linker.engine();
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);

        // Instantiate using shared linker (no registration needed)
        let instance = linker.instantiate(&mut store, module)?;

        // Get run_turn export
        let run_turn = instance
            .get_typed_func::<i32, i32>(&mut store, "run_turn")
            .map_err(|_| WasmError::MissingExport("run_turn"))?;

        // Get memory export for push-based visibility
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or(WasmError::MissingExport("memory"))?;

        Ok(Self {
            store,
            run_turn,
            memory,
        })
    }

    /// Push the visibility map to WASM linear memory.
    ///
    /// This pre-computes fog of war and writes all tiles to a known memory address,
    /// allowing bots to read tile data directly from memory instead of making syscalls.
    ///
    /// Memory layout at `TILE_MAP_BASE_ADDR`:
    /// - Header (16 bytes): magic(4), width(2), height(2), turn(4), `player_id(2)`, reserved(2)
    /// - Tiles: width * height * 4 bytes, each tile packed as (type | owner << 8 | army << 16)
    fn push_visibility_map(&mut self, game_state: &GameState) -> Result<(), WasmError> {
        let player_id = self.store.data().player_id;
        let width = game_state.map.width();
        let height = game_state.map.height();
        let num_tiles = usize::from(width) * usize::from(height);
        let total_size = TILE_MAP_HEADER_SIZE + num_tiles * 4;

        // Ensure memory is large enough
        let mem_size = self.memory.data_size(&self.store);
        if mem_size < TILE_MAP_BASE_ADDR + total_size {
            // Try to grow memory
            let pages_needed =
                ((TILE_MAP_BASE_ADDR + total_size - mem_size) / 65536) as u64 + 1;
            self.memory
                .grow(&mut self.store, pages_needed)
                .map_err(WasmError::Wasmtime)?;
        }

        // Get direct access to memory for bulk writes
        let mem_data = self.memory.data_mut(&mut self.store);

        // Write header: magic "ENSI" (0x49534E45), width, height, turn, player_id
        let header_offset = TILE_MAP_BASE_ADDR;
        mem_data[header_offset..header_offset + 4].copy_from_slice(&0x4953_4E45_u32.to_le_bytes());
        mem_data[header_offset + 4..header_offset + 6].copy_from_slice(&width.to_le_bytes());
        mem_data[header_offset + 6..header_offset + 8].copy_from_slice(&height.to_le_bytes());
        mem_data[header_offset + 8..header_offset + 12]
            .copy_from_slice(&game_state.turn().to_le_bytes());
        mem_data[header_offset + 12..header_offset + 14]
            .copy_from_slice(&u16::from(player_id).to_le_bytes());
        mem_data[header_offset + 14..header_offset + 16].copy_from_slice(&0u16.to_le_bytes());

        // Pre-compute visibility: owned tiles and their neighbors
        // Optimized: iterate tiles directly, compute coord only for owned tiles
        let mut visible = vec![false; num_tiles];
        let tiles = game_state.map.tiles();
        let width_usize = usize::from(width);
        for (idx, tile) in tiles.iter().enumerate() {
            if tile.owner == Some(player_id) {
                // Mark this tile as visible
                visible[idx] = true;
                // Compute coord only for owned tiles (lazy)
                let x = (idx % width_usize) as u16;
                let y = (idx / width_usize) as u16;
                let coord = Coord::new(x, y);
                let (adjacent, count) = coord.adjacent(width, height);
                for adj in &adjacent[..count as usize] {
                    let adj_idx = usize::from(adj.y) * width_usize + usize::from(adj.x);
                    visible[adj_idx] = true;
                }
            }
        }

        // Write tiles - direct slice access, no coord computation
        let tiles_offset = TILE_MAP_BASE_ADDR + TILE_MAP_HEADER_SIZE;
        for (idx, tile) in tiles.iter().enumerate() {
            let packed: u32 = if visible[idx] {
                let tile_type = match tile.tile_type {
                    TileType::City => 0u8,
                    TileType::Desert => 1u8,
                    TileType::Mountain => 2u8,
                };
                let owner = tile.owner.unwrap_or(0);
                let army = tile.army.min(65535) as u16;
                u32::from(tile_type) | (u32::from(owner) << 8) | (u32::from(army) << 16)
            } else {
                // Fog: type=255, owner=255, army=0
                0x0000_FFFF
            };

            let offset = tiles_offset + idx * 4;
            mem_data[offset..offset + 4].copy_from_slice(&packed.to_le_bytes());
        }

        Ok(())
    }

    /// Run a turn with the given fuel budget and game state.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    ///
    /// # Safety Note
    ///
    /// This function temporarily stores a raw pointer to `game_state.map` in `BotState`.
    /// The pointer is only valid during this function call and is cleared afterward.
    pub fn run_turn(
        &mut self,
        fuel_budget: u64,
        game_state: &GameState,
        all_stats: &AllPlayerStats,
    ) -> Result<(TurnResult, Vec<Command>), WasmError> {
        let player_id = self.store.data().player_id;

        // Set up state for this turn - NO MAP CLONING
        {
            let state = self.store.data_mut();
            state.reset();
            state.cached_stats = all_stats.get(player_id);
            state.turn = game_state.turn();

            // Store raw pointer to map (avoids cloning)
            state.map_ptr = &raw const game_state.map;

            // Cache capital info (small data, worth copying)
            if let Some(player) = game_state.get_player(player_id) {
                state.capital = Some(player.capital);
                state.capital_population = game_state
                    .map
                    .get(player.capital)
                    .map_or(0, |t| t.population);
            } else {
                state.capital = None;
                state.capital_population = 0;
            }
        }

        // Push visibility map to WASM memory (eliminates syscall overhead)
        self.push_visibility_map(game_state)?;

        // Set fuel
        self.store.set_fuel(fuel_budget)?;

        // Call run_turn
        let result = self
            .run_turn
            .call(&mut self.store, fuel_budget.min(i32::MAX as u64) as i32);

        // Clear map pointer (no longer valid after this function returns)
        self.store.data_mut().map_ptr = std::ptr::null();

        // Check result
        let turn_result = match result {
            Ok(_) => {
                if self.store.data().yielded {
                    TurnResult::BudgetExhausted { remaining: 0 }
                } else {
                    let remaining = self.store.get_fuel().unwrap_or(0);
                    TurnResult::BudgetExhausted {
                        remaining: remaining.min(u64::from(u32::MAX)) as u32,
                    }
                }
            }
            Err(e) => {
                // Check if out of fuel
                let remaining = self.store.get_fuel().unwrap_or(0);
                if remaining == 0 {
                    TurnResult::BudgetExhausted { remaining: 0 }
                } else {
                    return Err(WasmError::Trap(e.to_string()));
                }
            }
        };

        // Extract commands
        let commands = self.store.data().commands().to_vec();

        Ok((turn_result, commands))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_info_pack() {
        let tile = TileInfo {
            tile_type: 1,
            owner: 2,
            army: 100,
        };
        let packed = tile.pack();
        assert_eq!(packed & 0xFF, 1);
        assert_eq!((packed >> 8) & 0xFF, 2);
        assert_eq!(packed >> 16, 100);
    }

    #[test]
    fn test_fog_pack() {
        let fog = TileInfo::fog();
        let packed = fog.pack();
        assert_eq!(packed & 0xFF, 255);
        assert_eq!((packed >> 8) & 0xFF, 255);
        assert_eq!(packed >> 16, 0);
    }
}
