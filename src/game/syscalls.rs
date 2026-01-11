//! Game commands and syscall constants.

use crate::game::{CachedPlayerStats, Coord, GameState, PlayerId, TileType};

/// Maximum commands per turn per player. 256 is generous - typical turns use <50.
pub const MAX_COMMANDS_PER_TURN: usize = 256;

/// Syscall numbers for game interaction.
///
/// Programs use these numbers to specify which operation to invoke.
pub mod syscall {
    /// Get the current turn number.
    pub const GET_TURN: u32 = 1;
    /// Get this player's ID.
    pub const GET_PLAYER_ID: u32 = 2;
    /// Get this player's capital coordinates.
    pub const GET_MY_CAPITAL: u32 = 3;
    /// Get tile information at coordinates (with fog of war).
    pub const GET_TILE: u32 = 4;
    /// Get this player's food balance.
    pub const GET_MY_FOOD: u32 = 5;
    /// Get this player's total population.
    pub const GET_MY_POPULATION: u32 = 6;
    /// Get this player's total army count.
    pub const GET_MY_ARMY: u32 = 7;
    /// Get the map dimensions.
    pub const GET_MAP_SIZE: u32 = 8;
    /// Move army from one tile to an adjacent tile.
    pub const MOVE: u32 = 100;
    /// Convert population to army in a city.
    pub const CONVERT: u32 = 101;
    /// Move capital to a larger city.
    pub const MOVE_CAPITAL: u32 = 102;
    /// End turn early.
    pub const YIELD: u32 = 103;
}

/// A command issued by a player during their turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Move army from one tile to another.
    Move {
        /// Source tile coordinate.
        from: Coord,
        /// Destination tile coordinate.
        to: Coord,
        /// Number of army units to move.
        count: u32,
    },
    /// Convert population to army in a city.
    Convert {
        /// City coordinate.
        city: Coord,
        /// Number of population to convert.
        count: u32,
    },
    /// Move capital to a new city.
    MoveCapital {
        /// New capital coordinate.
        new_capital: Coord,
    },
    /// End turn early.
    Yield,
}

/// Syscall handler data for game interaction.
///
/// This struct holds the data needed to handle game syscalls.
/// Used by the WASM host functions to access game state.
pub struct GameSyscallHandler<'a> {
    /// ID of the player this handler serves.
    player_id: PlayerId,
    /// Immutable reference to game state.
    game_state: &'a GameState,
    /// Pre-computed stats for this player (O(1) lookup).
    cached_stats: CachedPlayerStats,
    /// Commands accumulated during this turn (fixed array, no heap).
    commands: [Command; MAX_COMMANDS_PER_TURN],
    /// Number of commands accumulated.
    command_count: u16,
    /// Whether the player has yielded.
    yielded: bool,
}

impl std::fmt::Debug for GameSyscallHandler<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameSyscallHandler")
            .field("player_id", &self.player_id)
            .field("command_count", &self.command_count)
            .field("yielded", &self.yielded)
            .finish_non_exhaustive()
    }
}

impl<'a> GameSyscallHandler<'a> {
    /// Create a new syscall handler for the given player with pre-computed stats.
    #[must_use]
    pub fn new(
        player_id: PlayerId,
        game_state: &'a GameState,
        cached_stats: CachedPlayerStats,
    ) -> Self {
        Self {
            player_id,
            game_state,
            cached_stats,
            commands: [Command::Yield; MAX_COMMANDS_PER_TURN],
            command_count: 0,
            yielded: false,
        }
    }

    /// Get the commands accumulated during this turn as a slice.
    #[must_use]
    #[inline]
    pub fn commands(&self) -> &[Command] {
        &self.commands[..self.command_count as usize]
    }

    /// Get the number of commands.
    #[must_use]
    #[inline]
    pub const fn command_count(&self) -> u16 {
        self.command_count
    }

    /// Check if the player has yielded.
    #[must_use]
    #[inline]
    pub const fn has_yielded(&self) -> bool {
        self.yielded
    }

    /// Get the player ID.
    #[must_use]
    #[inline]
    pub const fn player_id(&self) -> PlayerId {
        self.player_id
    }

    /// Get the game state.
    #[must_use]
    #[inline]
    pub const fn game_state(&self) -> &GameState {
        self.game_state
    }

    /// Get the cached stats.
    #[must_use]
    #[inline]
    pub const fn cached_stats(&self) -> &CachedPlayerStats {
        &self.cached_stats
    }

    /// Reset state for a new turn.
    #[inline]
    pub fn reset(&mut self) {
        self.command_count = 0;
        self.yielded = false;
    }

    /// Add a command. Returns false if buffer is full.
    #[inline]
    pub fn push_command(&mut self, cmd: Command) -> bool {
        if (self.command_count as usize) < MAX_COMMANDS_PER_TURN {
            self.commands[self.command_count as usize] = cmd;
            self.command_count += 1;
            true
        } else {
            false
        }
    }

    /// Set yielded flag.
    #[inline]
    pub fn set_yielded(&mut self, yielded: bool) {
        self.yielded = yielded;
    }

    /// Validate a move command.
    #[must_use]
    pub fn validate_move(&self, from: Coord, to: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        let Some(from_tile) = self.game_state.map.get(from) else {
            return false;
        };

        if from_tile.owner != Some(self.player_id) {
            return false;
        }

        if from_tile.army < count {
            return false;
        }

        let (adjacent, adj_count) = from.adjacent(self.game_state.map.width(), self.game_state.map.height());
        let is_adjacent = adjacent[..adj_count as usize].contains(&to);
        if !is_adjacent {
            return false;
        }

        let Some(to_tile) = self.game_state.map.get(to) else {
            return false;
        };

        to_tile.tile_type.is_passable()
    }

    /// Validate a convert command.
    #[must_use]
    pub fn validate_convert(&self, city: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        let Some(tile) = self.game_state.map.get(city) else {
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

    /// Validate a capital move command.
    #[must_use]
    pub fn validate_move_capital(&self, new_capital: Coord) -> bool {
        let Some(player) = self.game_state.get_player(self.player_id) else {
            return false;
        };

        let Some(current_tile) = self.game_state.map.get(player.capital) else {
            return false;
        };
        let current_pop = current_tile.population;

        let Some(new_tile) = self.game_state.map.get(new_capital) else {
            return false;
        };

        if new_tile.tile_type != TileType::City {
            return false;
        }

        if new_tile.owner != Some(self.player_id) {
            return false;
        }

        new_tile.population > current_pop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Map, Player, Tile};

    fn create_test_state() -> GameState {
        let mut map = Map::new(10, 10).expect("create map");

        let mut city1 = Tile::city(100);
        city1.owner = Some(1);
        city1.army = 50;
        map.set(Coord::new(2, 2), city1);

        let mut desert = Tile::desert();
        desert.owner = Some(1);
        desert.army = 10;
        map.set(Coord::new(3, 2), desert);

        let players = vec![Player::new(1, Coord::new(2, 2))];
        GameState::new(map, players, 1000)
    }

    fn create_handler(state: &GameState, player_id: PlayerId) -> GameSyscallHandler<'_> {
        let all_stats = state.compute_all_player_stats();
        GameSyscallHandler::new(player_id, state, all_stats.get(player_id))
    }

    #[test]
    fn test_validate_move_valid() {
        let state = create_test_state();
        let handler = create_handler(&state, 1);
        assert!(handler.validate_move(Coord::new(3, 2), Coord::new(4, 2), 5));
    }

    #[test]
    fn test_validate_move_not_adjacent() {
        let state = create_test_state();
        let handler = create_handler(&state, 1);
        assert!(!handler.validate_move(Coord::new(2, 2), Coord::new(5, 5), 5));
    }

    #[test]
    fn test_validate_move_not_owned() {
        let state = create_test_state();
        let handler = create_handler(&state, 1);
        assert!(!handler.validate_move(Coord::new(0, 0), Coord::new(0, 1), 5));
    }

    #[test]
    fn test_validate_convert() {
        let state = create_test_state();
        let handler = create_handler(&state, 1);
        assert!(handler.validate_convert(Coord::new(2, 2), 50));
    }

    #[test]
    fn test_validate_convert_not_enough_pop() {
        let state = create_test_state();
        let handler = create_handler(&state, 1);
        assert!(!handler.validate_convert(Coord::new(2, 2), 200));
    }

    #[test]
    fn test_push_command() {
        let state = create_test_state();
        let mut handler = create_handler(&state, 1);

        assert_eq!(handler.command_count(), 0);
        assert!(handler.push_command(Command::Yield));
        assert_eq!(handler.command_count(), 1);
    }

    #[test]
    fn test_yielded() {
        let state = create_test_state();
        let mut handler = create_handler(&state, 1);

        assert!(!handler.has_yielded());
        handler.set_yielded(true);
        assert!(handler.has_yielded());
    }
}
