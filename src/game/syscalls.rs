//! Syscall handler for game interaction.

use std::sync::{Arc, RwLock};

use crate::error::{TrapCause, VmResult};
use crate::game::{Coord, GameState, PlayerId, TileType};
use crate::vm::{Cpu, Memory};
use crate::SyscallHandler;

/// Syscall numbers for game interaction.
///
/// Programs use these numbers in register a7 (x17) to specify which syscall to invoke.
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

/// Constants for packing tile information.
mod packed {
    pub(super) const FOG_TILE_TYPE: u8 = 255;
    pub(super) const FOG_OWNER: u8 = 255;

    #[must_use]
    pub(super) fn pack_tile(tile_type: u8, owner: u8, army: u16) -> u32 {
        u32::from(tile_type) | (u32::from(owner) << 8) | (u32::from(army) << 16)
    }

    #[must_use]
    pub(super) fn fog() -> u32 {
        pack_tile(FOG_TILE_TYPE, FOG_OWNER, 0)
    }
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

/// Syscall handler for game interaction.
///
/// This handler implements the `SyscallHandler` trait and provides
/// game-specific syscalls for querying state and issuing commands.
pub struct GameSyscallHandler {
    /// ID of the player this handler serves.
    player_id: PlayerId,
    /// Shared game state.
    game_state: Arc<RwLock<GameState>>,
    /// Commands accumulated during this turn.
    commands: Vec<Command>,
    /// Whether the player has yielded.
    yielded: bool,
}

// Manual Debug implementation since Arc<RwLock<T>> doesn't derive nicely
impl std::fmt::Debug for GameSyscallHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameSyscallHandler")
            .field("player_id", &self.player_id)
            .field("commands", &self.commands)
            .field("yielded", &self.yielded)
            .finish_non_exhaustive()
    }
}

impl GameSyscallHandler {
    /// Create a new syscall handler for the given player.
    #[must_use]
    pub fn new(player_id: PlayerId, game_state: Arc<RwLock<GameState>>) -> Self {
        Self {
            player_id,
            game_state,
            commands: Vec::new(),
            yielded: false,
        }
    }

    /// Get the commands accumulated during this turn.
    #[must_use]
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    /// Check if the player has yielded.
    #[must_use]
    pub const fn has_yielded(&self) -> bool {
        self.yielded
    }

    /// Reset state for a new turn.
    pub fn reset(&mut self) {
        self.commands.clear();
        self.yielded = false;
    }
}

impl SyscallHandler for GameSyscallHandler {
    fn handle(&mut self, cpu: &mut Cpu, _memory: &mut Memory) -> VmResult<()> {
        let syscall_num = cpu.read_reg(17); // a7

        match syscall_num {
            syscall::GET_TURN => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                cpu.write_reg(10, game.turn());
                Ok(())
            }

            syscall::GET_PLAYER_ID => {
                cpu.write_reg(10, u32::from(self.player_id));
                Ok(())
            }

            syscall::GET_MY_CAPITAL => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                if let Some(player) = game.get_player(self.player_id) {
                    let packed = pack_coord(player.capital);
                    cpu.write_reg(10, packed);
                } else {
                    cpu.write_reg(10, u32::MAX);
                }
                Ok(())
            }

            syscall::GET_TILE => {
                let x = cpu.read_reg(10) as u16; // a0
                let y = cpu.read_reg(11) as u16; // a1
                let coord = Coord::new(x, y);

                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;

                // Check fog of war
                if !game.can_see_tile(self.player_id, coord) {
                    cpu.write_reg(10, packed::fog());
                    return Ok(());
                }

                // Get tile info
                if let Some(tile) = game.map.get(coord) {
                    let tile_type = match tile.tile_type {
                        TileType::City => 0,
                        TileType::Desert => 1,
                        TileType::Mountain => 2,
                    };
                    let owner = tile.owner.map_or(0, |id| id);
                    let army = tile.army.min(65535) as u16;

                    cpu.write_reg(10, packed::pack_tile(tile_type, owner, army));
                } else {
                    cpu.write_reg(10, packed::fog());
                }
                Ok(())
            }

            syscall::GET_MY_FOOD => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                let balance = game.food_balance(self.player_id);
                // Return as signed i32 cast to u32
                cpu.write_reg(10, balance.balance as u32);
                Ok(())
            }

            syscall::GET_MY_POPULATION => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                let pop = game.map.total_population(self.player_id);
                cpu.write_reg(10, pop);
                Ok(())
            }

            syscall::GET_MY_ARMY => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                let army = game.map.total_army(self.player_id);
                cpu.write_reg(10, army);
                Ok(())
            }

            syscall::GET_MAP_SIZE => {
                let game = self.game_state.read().map_err(|_| TrapCause::Ecall)?;
                let packed = (u32::from(game.map.width()) << 16) | u32::from(game.map.height());
                cpu.write_reg(10, packed);
                Ok(())
            }

            syscall::MOVE => {
                let from_x = cpu.read_reg(10) as u16;
                let from_y = cpu.read_reg(11) as u16;
                let to_x = cpu.read_reg(12) as u16;
                let to_y = cpu.read_reg(13) as u16;
                let count = cpu.read_reg(14);

                let from = Coord::new(from_x, from_y);
                let to = Coord::new(to_x, to_y);

                // Validate the move
                let result = self.validate_move(from, to, count);
                cpu.write_reg(10, u32::from(!result));

                if result {
                    self.commands.push(Command::Move { from, to, count });
                }
                Ok(())
            }

            syscall::CONVERT => {
                let city_x = cpu.read_reg(10) as u16;
                let city_y = cpu.read_reg(11) as u16;
                let count = cpu.read_reg(12);

                let city = Coord::new(city_x, city_y);

                // Validate the conversion
                let result = self.validate_convert(city, count);
                cpu.write_reg(10, u32::from(!result));

                if result {
                    self.commands.push(Command::Convert { city, count });
                }
                Ok(())
            }

            syscall::MOVE_CAPITAL => {
                let city_x = cpu.read_reg(10) as u16;
                let city_y = cpu.read_reg(11) as u16;
                let new_capital = Coord::new(city_x, city_y);

                // Validate capital move
                let result = self.validate_move_capital(new_capital);
                cpu.write_reg(10, u32::from(!result));

                if result {
                    self.commands.push(Command::MoveCapital { new_capital });
                }
                Ok(())
            }

            syscall::YIELD => {
                self.yielded = true;
                cpu.write_reg(10, 0);
                Ok(())
            }

            _ => {
                // Unknown syscall
                cpu.write_reg(10, u32::MAX);
                Err(TrapCause::Ecall)
            }
        }
    }
}

impl GameSyscallHandler {
    /// Validate a move command.
    fn validate_move(&self, from: Coord, to: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        let game = match self.game_state.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        // Check source tile
        let Some(from_tile) = game.map.get(from) else {
            return false;
        };

        // Must own the source tile
        if from_tile.owner != Some(self.player_id) {
            return false;
        }

        // Must have enough army
        if from_tile.army < count {
            return false;
        }

        // Check destination is adjacent
        let adjacent = from.adjacent(game.map.width(), game.map.height());
        if !adjacent.contains(&to) {
            return false;
        }

        // Check destination is passable
        let Some(to_tile) = game.map.get(to) else {
            return false;
        };

        to_tile.tile_type.is_passable()
    }

    /// Validate a convert command.
    fn validate_convert(&self, city: Coord, count: u32) -> bool {
        if count == 0 {
            return false;
        }

        let game = match self.game_state.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        let Some(tile) = game.map.get(city) else {
            return false;
        };

        // Must be a city
        if tile.tile_type != TileType::City {
            return false;
        }

        // Must own the city
        if tile.owner != Some(self.player_id) {
            return false;
        }

        // Must have enough population
        tile.population >= count
    }

    /// Validate a capital move command.
    fn validate_move_capital(&self, new_capital: Coord) -> bool {
        let game = match self.game_state.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        let Some(player) = game.get_player(self.player_id) else {
            return false;
        };

        // Get current capital population
        let Some(current_tile) = game.map.get(player.capital) else {
            return false;
        };
        let current_pop = current_tile.population;

        // Check new capital
        let Some(new_tile) = game.map.get(new_capital) else {
            return false;
        };

        // Must be a city
        if new_tile.tile_type != TileType::City {
            return false;
        }

        // Must own it
        if new_tile.owner != Some(self.player_id) {
            return false;
        }

        // Must have more population
        new_tile.population > current_pop
    }
}

/// Pack a coordinate into a single u32.
fn pack_coord(coord: Coord) -> u32 {
    (u32::from(coord.x) << 16) | u32::from(coord.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Map, Player, Tile};

    fn create_test_handler() -> (GameSyscallHandler, Arc<RwLock<GameState>>) {
        let mut map = Map::new(10, 10).unwrap();

        // Player 1's city at (2, 2)
        let mut city1 = Tile::city(100);
        city1.owner = Some(1);
        city1.army = 50;
        map.set(Coord::new(2, 2), city1);

        // Player 1 also owns adjacent tile
        let mut desert = Tile::desert();
        desert.owner = Some(1);
        desert.army = 10;
        map.set(Coord::new(3, 2), desert);

        let players = vec![Player::new(1, Coord::new(2, 2))];
        let state = GameState::new(map, players, 1000);
        let state = Arc::new(RwLock::new(state));

        let handler = GameSyscallHandler::new(1, Arc::clone(&state));
        (handler, state)
    }

    #[test]
    fn test_get_turn() {
        let (mut handler, _) = create_test_handler();
        let mut cpu = Cpu::new();
        let mut memory = Memory::new(1024, 0);

        cpu.write_reg(17, syscall::GET_TURN);
        handler.handle(&mut cpu, &mut memory).unwrap();

        assert_eq!(cpu.read_reg(10), 0);
    }

    #[test]
    fn test_get_player_id() {
        let (mut handler, _) = create_test_handler();
        let mut cpu = Cpu::new();
        let mut memory = Memory::new(1024, 0);

        cpu.write_reg(17, syscall::GET_PLAYER_ID);
        handler.handle(&mut cpu, &mut memory).unwrap();

        assert_eq!(cpu.read_reg(10), 1);
    }

    #[test]
    fn test_get_tile_visible() {
        let (mut handler, _) = create_test_handler();
        let mut cpu = Cpu::new();
        let mut memory = Memory::new(1024, 0);

        cpu.write_reg(17, syscall::GET_TILE);
        cpu.write_reg(10, 2); // x
        cpu.write_reg(11, 2); // y
        handler.handle(&mut cpu, &mut memory).unwrap();

        let result = cpu.read_reg(10);
        let tile_type = (result & 0xFF) as u8;
        let owner = ((result >> 8) & 0xFF) as u8;
        let army = (result >> 16) as u16;

        assert_eq!(tile_type, 0); // City
        assert_eq!(owner, 1); // Player 1
        assert_eq!(army, 50);
    }

    #[test]
    fn test_get_tile_fog() {
        let (mut handler, _) = create_test_handler();
        let mut cpu = Cpu::new();
        let mut memory = Memory::new(1024, 0);

        cpu.write_reg(17, syscall::GET_TILE);
        cpu.write_reg(10, 9); // x - far away
        cpu.write_reg(11, 9); // y
        handler.handle(&mut cpu, &mut memory).unwrap();

        assert_eq!(cpu.read_reg(10), packed::fog());
    }

    #[test]
    fn test_validate_move_valid() {
        let (handler, _) = create_test_handler();

        // Move from (3, 2) to (4, 2) - adjacent, owned, has army
        assert!(handler.validate_move(Coord::new(3, 2), Coord::new(4, 2), 5));
    }

    #[test]
    fn test_validate_move_not_adjacent() {
        let (handler, _) = create_test_handler();

        // Try to move to non-adjacent tile
        assert!(!handler.validate_move(Coord::new(2, 2), Coord::new(5, 5), 5));
    }

    #[test]
    fn test_validate_move_not_owned() {
        let (handler, _) = create_test_handler();

        // Try to move from unowned tile
        assert!(!handler.validate_move(Coord::new(0, 0), Coord::new(0, 1), 5));
    }

    #[test]
    fn test_validate_convert() {
        let (handler, _) = create_test_handler();

        // Convert 50 population to army in owned city
        assert!(handler.validate_convert(Coord::new(2, 2), 50));
    }

    #[test]
    fn test_validate_convert_not_enough_pop() {
        let (handler, _) = create_test_handler();

        // Try to convert more than available
        assert!(!handler.validate_convert(Coord::new(2, 2), 200));
    }

    #[test]
    fn test_yield() {
        let (mut handler, _) = create_test_handler();
        let mut cpu = Cpu::new();
        let mut memory = Memory::new(1024, 0);

        assert!(!handler.has_yielded());

        cpu.write_reg(17, syscall::YIELD);
        handler.handle(&mut cpu, &mut memory).unwrap();

        assert!(handler.has_yielded());
    }
}
