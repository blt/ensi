//! ASCII renderer for terminal viewing with ANSI colors.

use crate::game::{Coord, GameState, PlayerId, TileType};

/// ANSI color codes for players.
const PLAYER_COLORS: [&str; 8] = [
    "\x1b[31m", // Player 1: Red
    "\x1b[34m", // Player 2: Blue
    "\x1b[32m", // Player 3: Green
    "\x1b[33m", // Player 4: Yellow
    "\x1b[35m", // Player 5: Magenta
    "\x1b[36m", // Player 6: Cyan
    "\x1b[91m", // Player 7: Bright Red
    "\x1b[94m", // Player 8: Bright Blue
];

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const WHITE: &str = "\x1b[37m";
const GRAY: &str = "\x1b[90m";

/// Player display names.
const PLAYER_NAMES: [&str; 8] = [
    "Red", "Blue", "Green", "Yellow", "Magenta", "Cyan", "B.Red", "B.Blue",
];

/// Render game state to ASCII with ANSI colors.
///
/// Output format:
/// ```text
/// Turn 42/1000                                    [P1: 523] [P2: 412]
/// ┌────────────────────────────────────────────────────────────────┐
/// │ . . M . C₁ . . . . . . . . . . . . . . . . . . . . . . . . . │
/// │ . . M . . . ²₁ . . . . M . . . . . . . . . . . . . . . . . . │
/// └────────────────────────────────────────────────────────────────┘
///
/// Legend: M=Mountain  C=City  .=Desert  ₁₂=Owner  ²=Army
///
/// Player 1 (Red):   Cities: 3  Pop: 312  Army: 45  Food: +23/turn
/// Player 2 (Blue):  Cities: 2  Pop: 245  Army: 38  Food: +12/turn
/// ```
#[must_use]
pub fn render_ascii(state: &GameState, turn: u32) -> String {
    let mut output = String::new();

    // Header with turn and scores
    render_header(&mut output, state, turn);

    // Map
    render_map(&mut output, state);

    // Legend
    output.push_str("\nLegend: M=Mountain  C=City  .=Desert  Subscript=Owner  Superscript=Army\n\n");

    // Player stats
    render_player_stats(&mut output, state);

    // Controls hint
    output.push_str("\n[<] Back  [>] Forward  [g] Goto turn  [q] Quit\n");

    output
}

/// Render the header line with turn number and scores.
fn render_header(output: &mut String, state: &GameState, turn: u32) {
    let max_turns = state.max_turns;

    // Calculate scores
    let scores: Vec<(PlayerId, f64)> = state
        .players
        .iter()
        .map(|p| (p.id, state.calculate_score(p.id)))
        .collect();

    // Turn info
    output.push_str(&format!("Turn {turn}/{max_turns}"));

    // Pad to align scores on the right
    let padding = 40usize.saturating_sub(format!("Turn {turn}/{max_turns}").len());
    for _ in 0..padding {
        output.push(' ');
    }

    // Scores
    for (pid, score) in &scores {
        let color = get_player_color(*pid);
        output.push_str(&format!("{color}[P{pid}: {score:.0}]{RESET} "));
    }
    output.push('\n');
}

/// Render the map grid.
fn render_map(output: &mut String, state: &GameState) {
    let width = state.map.width();
    let height = state.map.height();

    // Top border
    output.push('┌');
    for _ in 0..(width * 2 + 1) {
        output.push('─');
    }
    output.push_str("┐\n");

    // Map rows
    for y in 0..height {
        output.push_str("│ ");
        for x in 0..width {
            let coord = Coord::new(x, y);
            render_tile(output, state, coord);
            output.push(' ');
        }
        output.push_str("│\n");
    }

    // Bottom border
    output.push('└');
    for _ in 0..(width * 2 + 1) {
        output.push('─');
    }
    output.push_str("┘\n");
}

/// Render a single tile.
fn render_tile(output: &mut String, state: &GameState, coord: Coord) {
    let tile = match state.map.get(coord) {
        Some(t) => t,
        None => {
            output.push('?');
            return;
        }
    };

    let symbol = match tile.tile_type {
        TileType::Mountain => {
            output.push_str(&format!("{WHITE}{BOLD}M{RESET}"));
            return;
        }
        TileType::Desert => {
            if tile.army > 0 {
                // Show army presence
                if let Some(owner) = tile.owner {
                    let color = get_player_color(owner);
                    let army_char = army_to_char(tile.army);
                    output.push_str(&format!("{color}{army_char}{RESET}"));
                    return;
                }
            }
            '.'
        }
        TileType::City => 'C',
    };

    // Apply owner coloring
    if let Some(owner) = tile.owner {
        let color = get_player_color(owner);
        // Show city with subscript owner
        let subscript = owner_subscript(owner);
        output.push_str(&format!("{color}{symbol}{subscript}{RESET}"));
    } else {
        // Neutral
        output.push_str(&format!("{GRAY}{symbol}{RESET}"));
    }
}

/// Convert army count to a display character.
fn army_to_char(army: u32) -> char {
    match army {
        0 => ' ',
        1..=9 => char::from_digit(army, 10).unwrap_or('9'),
        _ => '+', // More than 9
    }
}

/// Get Unicode subscript for owner number.
fn owner_subscript(owner: PlayerId) -> &'static str {
    match owner {
        1 => "₁",
        2 => "₂",
        3 => "₃",
        4 => "₄",
        5 => "₅",
        6 => "₆",
        7 => "₇",
        8 => "₈",
        _ => "?",
    }
}

/// Get ANSI color for a player.
fn get_player_color(player_id: PlayerId) -> &'static str {
    let idx = (player_id as usize).saturating_sub(1);
    PLAYER_COLORS.get(idx).copied().unwrap_or(WHITE)
}

/// Render player statistics.
fn render_player_stats(output: &mut String, state: &GameState) {
    for player in &state.players {
        if !player.alive {
            output.push_str(&format!(
                "{DIM}Player {} ({}): ELIMINATED{RESET}\n",
                player.id,
                get_player_name(player.id)
            ));
            continue;
        }

        let color = get_player_color(player.id);
        let name = get_player_name(player.id);

        // Count cities and total population/army
        let mut cities = 0u32;
        let mut total_pop = 0u32;
        let mut total_army = 0u32;

        for y in 0..state.map.height() {
            for x in 0..state.map.width() {
                let coord = Coord::new(x, y);
                if let Some(tile) = state.map.get(coord) {
                    if tile.owner == Some(player.id) {
                        total_army += tile.army;
                        if tile.tile_type == TileType::City {
                            cities += 1;
                            total_pop += tile.population;
                        }
                    }
                }
            }
        }

        // Calculate food balance
        // Net: population produces +1/turn (2 production - 1 consumption)
        // Army consumes 1/turn
        let food_balance = total_pop as i32 - total_army as i32;
        let food_str = if food_balance >= 0 {
            format!("+{food_balance}")
        } else {
            format!("{food_balance}")
        };

        output.push_str(&format!(
            "{color}Player {} ({name:>7}):{RESET}  Cities: {cities:<2}  Pop: {total_pop:<4}  Army: {total_army:<4}  Food: {food_str}/turn\n",
            player.id
        ));
    }
}

/// Get player display name.
fn get_player_name(player_id: PlayerId) -> &'static str {
    let idx = (player_id as usize).saturating_sub(1);
    PLAYER_NAMES.get(idx).copied().unwrap_or("Unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Map, Player, Tile};

    fn create_test_state() -> GameState {
        let mut map = Map::new(8, 8).unwrap();

        // Set up some tiles
        let mut city1 = Tile::city(100);
        city1.owner = Some(1);
        city1.army = 10;
        map.set(Coord::new(2, 2), city1);

        let mut city2 = Tile::city(80);
        city2.owner = Some(2);
        city2.army = 8;
        map.set(Coord::new(5, 5), city2);

        map.set(Coord::new(3, 3), Tile::mountain());
        map.set(Coord::new(4, 3), Tile::mountain());

        let players = vec![
            Player::new(1, Coord::new(2, 2)),
            Player::new(2, Coord::new(5, 5)),
        ];

        GameState::new(map, players, 1000)
    }

    #[test]
    fn test_render_ascii_basic() {
        let state = create_test_state();
        let output = render_ascii(&state, 0);

        // Should contain turn info
        assert!(output.contains("Turn 0/1000"));

        // Should contain border characters
        assert!(output.contains("┌"));
        assert!(output.contains("┘"));

        // Should contain legend
        assert!(output.contains("Legend"));

        // Should contain player stats
        assert!(output.contains("Player 1"));
        assert!(output.contains("Player 2"));
    }

    #[test]
    fn test_army_to_char() {
        assert_eq!(army_to_char(0), ' ');
        assert_eq!(army_to_char(1), '1');
        assert_eq!(army_to_char(5), '5');
        assert_eq!(army_to_char(9), '9');
        assert_eq!(army_to_char(10), '+');
        assert_eq!(army_to_char(100), '+');
    }

    #[test]
    fn test_owner_subscript() {
        assert_eq!(owner_subscript(1), "₁");
        assert_eq!(owner_subscript(2), "₂");
        assert_eq!(owner_subscript(8), "₈");
    }

    #[test]
    fn test_get_player_color() {
        // Just verify it doesn't panic for all valid players
        for i in 1..=8 {
            let _ = get_player_color(i);
        }
    }
}
