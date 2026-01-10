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
mod invariants;
mod map;
mod player;
mod state;
mod syscalls;

pub use combat::{process_attack, resolve_combat, CombatResult};
pub use economy::{apply_economy, calculate_food_balance, EconomyResult, FoodBalance};
pub use invariants::{
    assert_invariants, check_invariants, InvariantViolation, SANITY_MAX_ARMY_PER_TILE,
    SANITY_MAX_POP_PER_CITY, SANITY_MAX_SCORE, SANITY_MAX_TOTAL_POP,
};
pub use map::{Coord, Map, Tile, TileType};
pub use player::{Player, PlayerId};
pub use state::{AllPlayerStats, CachedPlayerStats, GameState, ScoringWeights, MAX_PLAYERS};
pub use syscalls::{syscall, Command, GameSyscallHandler, MAX_COMMANDS_PER_TURN};
