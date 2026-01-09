//! Game layer for Ensi.
//!
//! Implements the game rules on top of the VM:
//! - Map with tiles (cities, desert, mountains)
//! - Players with capitals and resources
//! - Economy (food production, growth, starvation)
//! - Combat resolution
//! - Syscall handler for player programs

mod combat;
mod economy;
mod map;
mod player;
mod state;
mod syscalls;

pub use combat::{process_attack, resolve_combat, CombatResult};
pub use economy::{apply_economy, calculate_food_balance, EconomyResult, FoodBalance};
pub use map::{Coord, Map, Tile, TileType};
pub use player::{Player, PlayerId};
pub use state::{GameState, ScoringWeights};
pub use syscalls::{syscall, Command, GameSyscallHandler};
