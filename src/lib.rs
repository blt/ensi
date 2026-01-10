// Allow unwrap and unreadable literals in tests (test code is not production)
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::unreadable_literal))]
//! Ensi: A deterministic WASM-based game engine for programming competitions.
//!
//! This crate provides a WASM-based game engine designed for:
//! - Bit-exact deterministic execution
//! - Fuel metering for fair resource limits
//! - Sandboxed bot execution
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │        Tournament Runner            │
//! ├─────────────────────────────────────┤
//! │         Game Logic                  │
//! ├─────────────────────────────────────┤
//! │    WASM Runtime (wasmtime)          │
//! └─────────────────────────────────────┘
//! ```

pub mod error;
pub mod game;
pub mod replay;
pub mod tournament;
pub mod wasm;

pub use error::{AccessType, TrapCause, VmResult};

// Re-export key game types at crate root for convenience
pub use game::{
    Command, Coord, GameState, GameSyscallHandler, Map, Player, PlayerId, Tile, TileType,
};

/// Result of running a WASM bot for a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnResult {
    /// Turn completed because fuel budget was exhausted.
    ///
    /// The `remaining` field contains any leftover fuel.
    BudgetExhausted {
        /// Fuel remaining when turn ended.
        remaining: u32,
    },
    /// Turn completed because of a trap.
    Trap(TrapCause),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_result_debug() {
        let result = TurnResult::BudgetExhausted { remaining: 100 };
        let debug = format!("{result:?}");
        assert!(debug.contains("BudgetExhausted"));
        assert!(debug.contains("100"));
    }
}
