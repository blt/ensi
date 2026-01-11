//! Host functions for WASM bots.
//!
//! These functions are imported by WASM bots and provide game interaction.

// WASM host functions intentionally use casts for ABI compatibility
// needless_pass_by_value is required by wasmtime's Caller API
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::needless_pass_by_value,
    clippy::bool_to_int_with_if
)]

use super::{BotState, TileInfo, WasmError};
use crate::game::{Command, Coord};
use wasmtime::{Caller, Linker};

/// Register all host functions with the linker.
///
/// # Errors
///
/// Returns an error if registration fails.
pub(super) fn register_host_functions(linker: &mut Linker<BotState>) -> Result<(), WasmError> {
    // Query functions
    linker.func_wrap("env", "ensi_get_turn", ensi_get_turn)?;
    linker.func_wrap("env", "ensi_get_player_id", ensi_get_player_id)?;
    linker.func_wrap("env", "ensi_get_my_capital", ensi_get_my_capital)?;
    linker.func_wrap("env", "ensi_get_tile", ensi_get_tile)?;
    linker.func_wrap("env", "ensi_get_my_food", ensi_get_my_food)?;
    linker.func_wrap("env", "ensi_get_my_population", ensi_get_my_population)?;
    linker.func_wrap("env", "ensi_get_my_army", ensi_get_my_army)?;
    linker.func_wrap("env", "ensi_get_map_width", ensi_get_map_width)?;
    linker.func_wrap("env", "ensi_get_map_height", ensi_get_map_height)?;

    // Action functions
    linker.func_wrap("env", "ensi_move", ensi_move)?;
    linker.func_wrap("env", "ensi_convert", ensi_convert)?;
    linker.func_wrap("env", "ensi_move_capital", ensi_move_capital)?;
    linker.func_wrap("env", "ensi_abandon", ensi_abandon)?;
    linker.func_wrap("env", "ensi_yield", ensi_yield)?;

    Ok(())
}

/// Get the current turn number.
fn ensi_get_turn(caller: Caller<'_, BotState>) -> i32 {
    caller.data().turn as i32
}

/// Get this bot's player ID.
fn ensi_get_player_id(caller: Caller<'_, BotState>) -> i32 {
    i32::from(caller.data().player_id)
}

/// Get this player's capital coordinates.
/// Returns packed (x << 16 | y), or -1 if no capital.
fn ensi_get_my_capital(caller: Caller<'_, BotState>) -> i32 {
    caller
        .data()
        .capital
        .map_or(-1, |coord| {
            ((i32::from(coord.x)) << 16) | i32::from(coord.y)
        })
}

/// Get tile information at coordinates.
/// Returns packed (`tile_type` | owner << 8 | army << 16).
fn ensi_get_tile(caller: Caller<'_, BotState>, x: i32, y: i32) -> i32 {
    let state = caller.data();
    let coord = Coord::new(x as u16, y as u16);

    // Check fog of war
    if !state.can_see_tile(coord) {
        return TileInfo::fog().pack() as i32;
    }

    // Get tile info
    state
        .get_tile_info(coord)
        .map_or_else(|| TileInfo::fog().pack() as i32, |t| t.pack() as i32)
}

/// Get this player's food balance.
fn ensi_get_my_food(caller: Caller<'_, BotState>) -> i32 {
    caller.data().cached_stats.food_balance
}

/// Get this player's total population.
fn ensi_get_my_population(caller: Caller<'_, BotState>) -> i32 {
    caller.data().cached_stats.population as i32
}

/// Get this player's total army count.
fn ensi_get_my_army(caller: Caller<'_, BotState>) -> i32 {
    caller.data().cached_stats.army as i32
}

/// Get the map width.
fn ensi_get_map_width(caller: Caller<'_, BotState>) -> i32 {
    i32::from(caller.data().map_width)
}

/// Get the map height.
fn ensi_get_map_height(caller: Caller<'_, BotState>) -> i32 {
    i32::from(caller.data().map_height)
}

/// Move army from one tile to another.
/// Returns 0 on success, 1 on failure.
fn ensi_move(
    mut caller: Caller<'_, BotState>,
    from_x: i32,
    from_y: i32,
    to_x: i32,
    to_y: i32,
    count: i32,
) -> i32 {
    let from = Coord::new(from_x as u16, from_y as u16);
    let to = Coord::new(to_x as u16, to_y as u16);
    let count = count as u32;

    if caller.data().validate_move(from, to, count) {
        let cmd = Command::Move { from, to, count };
        if caller.data_mut().push_command(cmd) {
            0
        } else {
            1 // Buffer full
        }
    } else {
        1 // Invalid
    }
}

/// Convert population to army in a city.
/// Returns 0 on success, 1 on failure.
fn ensi_convert(mut caller: Caller<'_, BotState>, city_x: i32, city_y: i32, count: i32) -> i32 {
    let city = Coord::new(city_x as u16, city_y as u16);
    let count = count as u32;

    if caller.data().validate_convert(city, count) {
        let cmd = Command::Convert { city, count };
        if caller.data_mut().push_command(cmd) {
            0
        } else {
            1 // Buffer full
        }
    } else {
        1 // Invalid
    }
}

/// Move capital to a new city.
/// Returns 0 on success, 1 on failure.
fn ensi_move_capital(mut caller: Caller<'_, BotState>, city_x: i32, city_y: i32) -> i32 {
    let new_capital = Coord::new(city_x as u16, city_y as u16);

    if caller.data().validate_move_capital(new_capital) {
        let cmd = Command::MoveCapital { new_capital };
        if caller.data_mut().push_command(cmd) {
            0
        } else {
            1 // Buffer full
        }
    } else {
        1 // Invalid
    }
}

/// Abandon a tile (relinquish ownership).
/// Returns 0 on success, 1 on failure.
fn ensi_abandon(mut caller: Caller<'_, BotState>, x: i32, y: i32) -> i32 {
    let coord = Coord::new(x as u16, y as u16);

    if caller.data().validate_abandon(coord) {
        let cmd = Command::Abandon { coord };
        if caller.data_mut().push_command(cmd) {
            0
        } else {
            1 // Buffer full
        }
    } else {
        1 // Invalid (not owned or is capital)
    }
}

/// End turn early.
fn ensi_yield(mut caller: Caller<'_, BotState>) {
    caller.data_mut().yielded = true;
}
