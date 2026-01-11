//! Game invariants - sanity checks that detect bugs.
//!
//! With the diminishing returns economy model, these should NEVER trigger
//! in a correctly implemented game. If they do, it indicates a bug.
//!
//! These are NOT gameplay limits - the economic model naturally bounds values.
//! These are sanity checks with very generous bounds.

use crate::game::{GameState, TileType};

/// Sanity bound: population per city should never exceed this.
/// With ~50× territory equilibrium on a 100×100 map (10,000 tiles),
/// theoretical max is ~500,000 total, so 100K per city is very generous.
pub const SANITY_MAX_POP_PER_CITY: u32 = 100_000;

/// Sanity bound: total population should never exceed this.
/// Even a 200×200 map (40,000 tiles) at 50× gives 2M max.
pub const SANITY_MAX_TOTAL_POP: u64 = 10_000_000;

/// Sanity bound: army per tile should never exceed this.
/// Armies come from population, so this is very generous.
pub const SANITY_MAX_ARMY_PER_TILE: u32 = 1_000_000;

/// Sanity bound: score should never exceed this.
pub const SANITY_MAX_SCORE: f64 = 1e15;

/// Invariant violation error.
#[derive(Debug, Clone)]
pub struct InvariantViolation {
    /// Description of the violated invariant.
    pub message: String,
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invariant violation: {}", self.message)
    }
}

impl std::error::Error for InvariantViolation {}

/// Check all game invariants.
///
/// Returns a list of violations found, or empty if all invariants hold.
/// These are bug detectors, not gameplay limits.
#[must_use]
pub fn check_invariants(state: &GameState) -> Vec<InvariantViolation> {
    let mut violations = Vec::new();

    let mut total_population = 0u64;

    for (coord, tile) in state.map.iter() {
        // Population bounds per city
        if tile.tile_type == TileType::City && tile.population > SANITY_MAX_POP_PER_CITY {
            violations.push(InvariantViolation {
                message: format!(
                    "City at {:?} has population {} > sanity max {}",
                    coord, tile.population, SANITY_MAX_POP_PER_CITY
                ),
            });
        }

        if tile.tile_type == TileType::City {
            total_population += u64::from(tile.population);
        }

        // Army bounds per tile
        if tile.army > SANITY_MAX_ARMY_PER_TILE {
            violations.push(InvariantViolation {
                message: format!(
                    "Tile at {:?} has army {} > sanity max {}",
                    coord, tile.army, SANITY_MAX_ARMY_PER_TILE
                ),
            });
        }
    }

    // Total population check
    if total_population > SANITY_MAX_TOTAL_POP {
        violations.push(InvariantViolation {
            message: format!(
                "Total population {total_population} exceeds sanity max {SANITY_MAX_TOTAL_POP}"
            ),
        });
    }

    // Score bounds for alive players
    for player in &state.players {
        if player.alive {
            let score = state.calculate_score(player.id);
            if score > SANITY_MAX_SCORE {
                violations.push(InvariantViolation {
                    message: format!(
                        "Player {} score {} exceeds sanity max {}",
                        player.id, score, SANITY_MAX_SCORE
                    ),
                });
            }
        }
    }

    // Elimination consistency
    for player in &state.players {
        if !player.alive {
            let territory = state
                .map
                .iter()
                .filter(|(_, t)| t.owner == Some(player.id))
                .count();
            if territory > 0 {
                violations.push(InvariantViolation {
                    message: format!("Dead player {} still owns {} tiles", player.id, territory),
                });
            }
        }
    }

    violations
}

/// Assert all game invariants hold, panicking if any are violated.
///
/// Only active in debug builds. No-op in release builds.
///
/// # Panics
///
/// Panics with detailed message if any invariant is violated.
#[cfg(debug_assertions)]
pub fn assert_invariants(state: &GameState) {
    let violations = check_invariants(state);
    if !violations.is_empty() {
        let messages: Vec<_> = violations.iter().map(|v| v.message.as_str()).collect();
        panic!("Game invariant violations:\n  - {}", messages.join("\n  - "));
    }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
pub fn assert_invariants(_state: &GameState) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Coord, Map, Player, Tile};

    fn create_valid_game() -> GameState {
        let mut map = Map::new(10, 10).unwrap();

        let mut city = Tile::city(100);
        city.owner = Some(1);
        city.army = 10;
        map.set(Coord::new(5, 5), city);

        let players = vec![Player::new(1, Coord::new(5, 5))];
        GameState::new(map, players, 1000)
    }

    #[test]
    fn test_valid_game_passes() {
        let game = create_valid_game();
        let violations = check_invariants(&game);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_excessive_population_detected() {
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.population = SANITY_MAX_POP_PER_CITY + 1;
        }

        let violations = check_invariants(&game);
        assert!(!violations.is_empty());
        assert!(violations[0].message.contains("population"));
    }

    #[test]
    fn test_excessive_army_detected() {
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.army = SANITY_MAX_ARMY_PER_TILE + 1;
        }

        let violations = check_invariants(&game);
        assert!(!violations.is_empty());
        assert!(violations[0].message.contains("army"));
    }

    #[test]
    fn test_dead_player_with_territory_detected() {
        let mut game = create_valid_game();
        // Kill the player but leave them owning territory
        game.players[0].eliminate();
        // Note: The tile still has owner = Some(1)

        let violations = check_invariants(&game);
        assert!(!violations.is_empty());
        assert!(violations[0].message.contains("Dead player"));
    }

    // ==================== BOUNDARY TESTS ====================
    // These test exact boundary conditions to ensure invariant checks
    // are precise and don't have off-by-one errors.

    #[test]
    fn test_population_exactly_at_max_passes() {
        // Population exactly at the max should pass (not >)
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.population = SANITY_MAX_POP_PER_CITY;
        }

        let violations = check_invariants(&game);
        assert!(
            violations.is_empty(),
            "Population at exactly max should pass: {:?}",
            violations
        );
    }

    #[test]
    fn test_population_one_above_max_fails() {
        // Population one above max should fail
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.population = SANITY_MAX_POP_PER_CITY + 1;
        }

        let violations = check_invariants(&game);
        assert_eq!(
            violations.len(),
            1,
            "Expected exactly one violation for population"
        );
        assert!(violations[0].message.contains("population"));
    }

    #[test]
    fn test_army_exactly_at_max_passes() {
        // Army exactly at max should pass
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.army = SANITY_MAX_ARMY_PER_TILE;
        }

        let violations = check_invariants(&game);
        assert!(
            violations.is_empty(),
            "Army at exactly max should pass: {:?}",
            violations
        );
    }

    #[test]
    fn test_army_one_above_max_fails() {
        // Army one above max should fail
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.army = SANITY_MAX_ARMY_PER_TILE + 1;
        }

        let violations = check_invariants(&game);
        assert_eq!(violations.len(), 1, "Expected exactly one violation for army");
        assert!(violations[0].message.contains("army"));
    }

    #[test]
    fn test_total_population_exactly_at_max_passes() {
        // Total population exactly at max should pass
        let mut map = Map::new(20, 20).unwrap();

        // Spread population across multiple cities to reach total max
        // SANITY_MAX_TOTAL_POP = 10,000,000
        // SANITY_MAX_POP_PER_CITY = 100,000
        // So we need 100 cities at max population
        let cities_needed = (SANITY_MAX_TOTAL_POP / u64::from(SANITY_MAX_POP_PER_CITY)) as usize;

        for i in 0..cities_needed.min(100) {
            let x = (i % 20) as u16;
            let y = (i / 20) as u16;
            let mut city = Tile::city(SANITY_MAX_POP_PER_CITY);
            city.owner = Some(1);
            map.set(Coord::new(x, y), city);
        }

        let players = vec![Player::new(1, Coord::new(0, 0))];
        let game = GameState::new(map, players, 1000);

        let violations = check_invariants(&game);
        assert!(
            violations.is_empty(),
            "Total pop at exactly max should pass: {:?}",
            violations
        );
    }

    #[test]
    fn test_total_population_one_above_max_fails() {
        // Total population one above max should fail
        let mut map = Map::new(20, 20).unwrap();

        // Put slightly more than max total population
        let cities_needed = (SANITY_MAX_TOTAL_POP / u64::from(SANITY_MAX_POP_PER_CITY)) as usize;

        for i in 0..cities_needed.min(100) {
            let x = (i % 20) as u16;
            let y = (i / 20) as u16;
            let mut city = Tile::city(SANITY_MAX_POP_PER_CITY);
            city.owner = Some(1);
            map.set(Coord::new(x, y), city);
        }

        // Add one more city with 1 population to exceed limit
        let mut extra = Tile::city(1);
        extra.owner = Some(1);
        map.set(Coord::new(10, 10), extra);

        let players = vec![Player::new(1, Coord::new(0, 0))];
        let game = GameState::new(map, players, 1000);

        let violations = check_invariants(&game);
        assert!(
            !violations.is_empty(),
            "Total pop above max should fail"
        );
        let total_pop_violation = violations
            .iter()
            .any(|v| v.message.contains("Total population"));
        assert!(
            total_pop_violation,
            "Should have total population violation: {:?}",
            violations
        );
    }

    #[test]
    fn test_multiple_violations_all_reported() {
        // Test that multiple violations are all captured
        let mut game = create_valid_game();

        // Add excessive population
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.population = SANITY_MAX_POP_PER_CITY + 1;
            tile.army = SANITY_MAX_ARMY_PER_TILE + 1;
        }

        let violations = check_invariants(&game);
        assert!(
            violations.len() >= 2,
            "Should have at least 2 violations: {:?}",
            violations
        );

        let has_pop = violations.iter().any(|v| v.message.contains("population"));
        let has_army = violations.iter().any(|v| v.message.contains("army"));
        assert!(has_pop, "Should detect population violation");
        assert!(has_army, "Should detect army violation");
    }

    #[test]
    fn test_zero_values_pass() {
        // Edge case: zero population and army should be valid
        let mut game = create_valid_game();
        if let Some(tile) = game.map.get_mut(Coord::new(5, 5)) {
            tile.population = 0;
            tile.army = 0;
        }

        let violations = check_invariants(&game);
        assert!(
            violations.is_empty(),
            "Zero values should be valid: {:?}",
            violations
        );
    }

    #[test]
    fn test_empty_map_passes() {
        // Edge case: empty map with no cities
        let map = Map::new(10, 10).unwrap();
        let players = vec![Player::new(1, Coord::new(5, 5))];
        // Note: player's capital doesn't exist on map - this is an unusual state

        let game = GameState::new(map, players, 1000);
        let violations = check_invariants(&game);

        // This should pass because there's nothing to violate
        // (though the game would be in a weird state)
        assert!(
            violations.is_empty(),
            "Empty map should pass invariants: {:?}",
            violations
        );
    }

    #[test]
    fn test_neutral_tiles_not_checked() {
        // Neutral tiles (no owner) shouldn't trigger player-related checks
        let mut map = Map::new(10, 10).unwrap();

        // Create a neutral city with high population
        let city = Tile::city(SANITY_MAX_POP_PER_CITY + 1);
        // city.owner is None by default (neutral)
        map.set(Coord::new(5, 5), city);

        // Player has a different capital
        let mut capital = Tile::city(100);
        capital.owner = Some(1);
        map.set(Coord::new(2, 2), capital);

        let players = vec![Player::new(1, Coord::new(2, 2))];
        let game = GameState::new(map, players, 1000);

        let violations = check_invariants(&game);

        // The neutral city should still trigger population check
        // because we check ALL cities, not just owned ones
        assert!(
            !violations.is_empty(),
            "Neutral city with high pop should fail: {:?}",
            violations
        );
    }

    #[test]
    fn test_dead_player_with_no_territory_passes() {
        // Dead player with no remaining territory should pass
        let mut map = Map::new(10, 10).unwrap();

        // Create a city owned by player 2 (the one who will survive)
        let mut city = Tile::city(100);
        city.owner = Some(2);
        map.set(Coord::new(5, 5), city);

        // Player 1 is eliminated and owns nothing
        let mut p1 = Player::new(1, Coord::new(0, 0));
        p1.eliminate();

        let players = vec![p1, Player::new(2, Coord::new(5, 5))];
        let game = GameState::new(map, players, 1000);

        let violations = check_invariants(&game);
        assert!(
            violations.is_empty(),
            "Dead player with no territory should pass: {:?}",
            violations
        );
    }
}
