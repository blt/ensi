//! Structured text output for LLM consumption.
//!
//! This format is optimized for machine readability while remaining
//! human-parseable. It provides all relevant game state information
//! in a structured format.

// Allow format! with push_str for readability - the allocation overhead is negligible for text rendering
#![allow(
    clippy::format_push_string,
    clippy::manual_let_else,
    clippy::cast_possible_wrap
)]

use crate::game::{Coord, GameState, PlayerId, TileType};

/// Render game state to structured text format for LLM consumption.
///
/// Output format:
/// ```text
/// === TURN 42 OF 1000 ===
///
/// MAP (64x64):
/// Notable positions:
/// - P1 capital at (12, 8)
/// - P2 capital at (45, 32)
///
/// PLAYER 1 STATUS:
/// - Cities: 3 at [(12,8), (15,12), (8,20)]
/// - Total population: 312
/// - Total army: 45
/// - Food balance: +23/turn
///
/// ...
/// ```
#[must_use]
pub fn render_llm(state: &GameState, turn: u32) -> String {
    let mut output = String::new();

    // Header
    render_header(&mut output, state, turn);

    // Map overview
    render_map_overview(&mut output, state);

    // Player statuses
    for player in &state.players {
        render_player_status(&mut output, state, player.id);
    }

    // Game status
    render_game_status(&mut output, state);

    output
}

/// Render the header.
fn render_header(output: &mut String, state: &GameState, turn: u32) {
    output.push_str(&format!(
        "=== TURN {} OF {} ===\n\n",
        turn,
        state.max_turns
    ));
}

/// Render map overview.
fn render_map_overview(output: &mut String, state: &GameState) {
    let width = state.map.width();
    let height = state.map.height();

    output.push_str(&format!("MAP ({width}x{height}):\n"));

    // Count terrain types
    let mut mountains = 0u32;
    let mut cities = 0u32;
    let mut neutral_cities = 0u32;

    for y in 0..height {
        for x in 0..width {
            let coord = Coord::new(x, y);
            if let Some(tile) = state.map.get(coord) {
                match tile.tile_type {
                    TileType::Mountain => mountains += 1,
                    TileType::City => {
                        cities += 1;
                        if tile.owner.is_none() {
                            neutral_cities += 1;
                        }
                    }
                    TileType::Desert => {}
                }
            }
        }
    }

    output.push_str(&format!("Terrain: {mountains} mountains, {cities} cities ({neutral_cities} neutral)\n"));

    // Notable positions
    output.push_str("Notable positions:\n");
    for player in &state.players {
        if player.alive {
            output.push_str(&format!(
                "- P{} capital at ({}, {})\n",
                player.id, player.capital.x, player.capital.y
            ));
        }
    }

    // Find contested areas (tiles with armies from multiple players nearby)
    let contested = find_contested_areas(state);
    if !contested.is_empty() {
        output.push_str("Contested areas: ");
        let coords: Vec<String> = contested
            .iter()
            .take(5)
            .map(|c| format!("({},{})", c.x, c.y))
            .collect();
        output.push_str(&coords.join(", "));
        if contested.len() > 5 {
            output.push_str(&format!(" and {} more", contested.len() - 5));
        }
        output.push('\n');
    }

    output.push('\n');
}

/// Find tiles that are contested (have enemy armies adjacent).
fn find_contested_areas(state: &GameState) -> Vec<Coord> {
    let mut contested = Vec::new();
    let width = state.map.width();
    let height = state.map.height();

    for y in 0..height {
        for x in 0..width {
            let coord = Coord::new(x, y);
            if let Some(tile) = state.map.get(coord)
                && tile.army > 0
                    && let Some(owner) = tile.owner {
                        // Check if any adjacent tile has a different owner's army
                        let (adjacent, adj_count) = coord.adjacent(width, height);
                        for adj in &adjacent[..adj_count as usize] {
                            if let Some(adj_tile) = state.map.get(*adj)
                                && adj_tile.army > 0 && adj_tile.owner != Some(owner) {
                                    contested.push(coord);
                                    break;
                                }
                        }
                    }
        }
    }

    contested
}

/// Render a single player's status.
fn render_player_status(output: &mut String, state: &GameState, player_id: PlayerId) {
    let player = match state.get_player(player_id) {
        Some(p) => p,
        None => return,
    };

    output.push_str(&format!("PLAYER {player_id} STATUS:\n"));

    if !player.alive {
        output.push_str("- ELIMINATED\n\n");
        return;
    }

    // Gather stats
    let mut cities: Vec<Coord> = Vec::new();
    let mut total_pop = 0u32;
    let mut total_army = 0u32;
    let mut territory = 0u32;
    let mut army_positions: Vec<(Coord, u32)> = Vec::new();

    let width = state.map.width();
    let height = state.map.height();

    for y in 0..height {
        for x in 0..width {
            let coord = Coord::new(x, y);
            if let Some(tile) = state.map.get(coord)
                && tile.owner == Some(player_id) {
                    territory += 1;
                    total_army += tile.army;

                    if tile.army > 0 {
                        army_positions.push((coord, tile.army));
                    }

                    if tile.tile_type == TileType::City {
                        cities.push(coord);
                        total_pop += tile.population;
                    }
                }
        }
    }

    // Cities
    let city_coords: Vec<String> = cities
        .iter()
        .take(5)
        .map(|c| format!("({},{})", c.x, c.y))
        .collect();
    output.push_str(&format!("- Cities: {} at [{}]", cities.len(), city_coords.join(", ")));
    if cities.len() > 5 {
        output.push_str(&format!(" and {} more", cities.len() - 5));
    }
    output.push('\n');

    // Population
    output.push_str(&format!("- Total population: {total_pop}\n"));

    // Army
    output.push_str(&format!("- Total army: {total_army}"));
    if !army_positions.is_empty() {
        // Show top army positions
        army_positions.sort_by(|a, b| b.1.cmp(&a.1));
        let positions: Vec<String> = army_positions
            .iter()
            .take(3)
            .map(|(c, a)| format!("{a} at ({},{})", c.x, c.y))
            .collect();
        output.push_str(&format!(" (distributed: {})", positions.join(", ")));
    }
    output.push('\n');

    // Territory
    output.push_str(&format!("- Territory: {territory} tiles\n"));

    // Food balance
    // Net: population produces +1/turn (2 production - 1 consumption)
    // Army consumes 1/turn
    let food_production = total_pop * 2;
    let food_consumption = total_pop + total_army;
    let food_balance = food_production as i32 - food_consumption as i32;
    let food_str = if food_balance >= 0 {
        format!("+{food_balance}")
    } else {
        format!("{food_balance}")
    };
    output.push_str(&format!(
        "- Food balance: {food_str}/turn (production: {food_production}, consumption: {food_consumption})\n"
    ));

    // Score
    let score = state.calculate_score(player_id);
    output.push_str(&format!("- Current score: {score:.1}\n"));

    output.push('\n');
}

/// Render overall game status.
fn render_game_status(output: &mut String, state: &GameState) {
    output.push_str("GAME STATUS:\n");

    // Alive players
    let alive_count = &state.players.iter().filter(|p| p.alive).count();
    let total_count = &state.players.len();
    output.push_str(&format!("- Players alive: {alive_count}/{total_count}\n"));

    // Score ranking
    let mut scores: Vec<(PlayerId, f64, bool)> = state
        .players
        .iter()
        .map(|p| (p.id, state.calculate_score(p.id), p.alive))
        .collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    output.push_str("- Score ranking: ");
    let ranking: Vec<String> = scores
        .iter()
        .map(|(pid, score, alive)| {
            if *alive {
                format!("P{pid}({score:.0})")
            } else {
                format!("P{pid}({score:.0},ELIM)")
            }
        })
        .collect();
    output.push_str(&ranking.join(" > "));
    output.push('\n');

    // Game over check
    if state.is_game_over() {
        output.push_str("- GAME OVER\n");
        if let Some((winner, _, _)) = scores.first() {
            if scores.iter().filter(|(_, _, alive)| *alive).count() > 0 {
                output.push_str(&format!("- Winner: Player {winner}\n"));
            } else {
                output.push_str("- No winner (all eliminated)\n");
            }
        }
    } else {
        let turns_remaining = state.max_turns.saturating_sub(state.turn());
        output.push_str(&format!("- Turns remaining: {turns_remaining}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Map, Player, Tile};

    fn create_test_state() -> GameState {
        let mut map = Map::new(16, 16).unwrap();

        // Set up some tiles
        let mut city1 = Tile::city(100);
        city1.owner = Some(1);
        city1.army = 10;
        map.set(Coord::new(4, 4), city1);

        let mut city2 = Tile::city(80);
        city2.owner = Some(2);
        city2.army = 8;
        map.set(Coord::new(12, 12), city2);

        // Add a neutral city
        map.set(Coord::new(8, 8), Tile::city(50));

        // Add some mountains
        map.set(Coord::new(6, 6), Tile::mountain());
        map.set(Coord::new(7, 6), Tile::mountain());

        let players = vec![
            Player::new(1, Coord::new(4, 4)),
            Player::new(2, Coord::new(12, 12)),
        ];

        GameState::new(map, players, 1000)
    }

    #[test]
    fn test_render_llm_basic() {
        let state = create_test_state();
        let output = render_llm(&state, 0);

        // Should contain turn info
        assert!(output.contains("TURN 0 OF 1000"));

        // Should contain map info
        assert!(output.contains("MAP (16x16)"));

        // Should contain player status
        assert!(output.contains("PLAYER 1 STATUS"));
        assert!(output.contains("PLAYER 2 STATUS"));

        // Should contain game status
        assert!(output.contains("GAME STATUS"));
        assert!(output.contains("Players alive: 2/2"));
    }

    #[test]
    fn test_render_llm_contains_capitals() {
        let state = create_test_state();
        let output = render_llm(&state, 0);

        // Should mention capitals
        assert!(output.contains("P1 capital at (4, 4)"));
        assert!(output.contains("P2 capital at (12, 12)"));
    }

    #[test]
    fn test_render_llm_contains_scores() {
        let state = create_test_state();
        let output = render_llm(&state, 0);

        // Should contain score ranking
        assert!(output.contains("Score ranking"));
    }

    #[test]
    fn test_render_llm_food_balance() {
        let state = create_test_state();
        let output = render_llm(&state, 0);

        // Should contain food balance info
        assert!(output.contains("Food balance"));
        assert!(output.contains("production"));
        assert!(output.contains("consumption"));
    }
}
