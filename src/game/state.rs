//! Game state management.

use crate::game::{
    apply_economy, calculate_food_balance, resolve_combat, Coord, FoodBalance, Map, Player,
    PlayerId, TileType,
};

/// Maximum number of players in a game.
pub const MAX_PLAYERS: usize = 8;

/// Pre-computed statistics for a player.
///
/// This cache is computed once at the start of each turn and used
/// for all syscalls during that turn. This eliminates O(n) map iterations
/// per syscall, replacing them with O(1) lookups.
#[derive(Debug, Clone, Copy, Default)]
pub struct CachedPlayerStats {
    /// Total population across all cities.
    pub population: u32,
    /// Total army across all tiles.
    pub army: u32,
    /// Total territory (non-mountain tiles owned).
    pub territory: u32,
    /// Food balance for the turn.
    pub food_balance: i32,
}

/// Collection of cached stats for all players.
///
/// Uses a fixed-size array indexed by player_id - 1 to avoid heap allocation.
#[derive(Debug, Clone, Copy)]
pub struct AllPlayerStats {
    /// Stats indexed by player_id - 1.
    stats: [CachedPlayerStats; MAX_PLAYERS],
}

impl Default for AllPlayerStats {
    fn default() -> Self {
        Self {
            stats: [CachedPlayerStats::default(); MAX_PLAYERS],
        }
    }
}

impl AllPlayerStats {
    /// Get stats for a player.
    #[must_use]
    #[inline]
    pub fn get(&self, player_id: PlayerId) -> CachedPlayerStats {
        if player_id == 0 || player_id as usize > MAX_PLAYERS {
            return CachedPlayerStats::default();
        }
        self.stats[(player_id - 1) as usize]
    }
}

/// Scoring weights for final score calculation.
#[derive(Debug, Clone, Copy)]
pub struct ScoringWeights {
    /// Points per population (default: 1.0).
    pub population: f64,
    /// Points per city owned (default: 10.0).
    pub city: f64,
    /// Points per territory tile owned (default: 0.5).
    pub territory: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            population: 1.0,
            city: 10.0,
            territory: 0.5,
        }
    }
}

/// Complete game state.
#[derive(Debug, Clone)]
pub struct GameState {
    /// The game map.
    pub map: Map,
    /// All players in the game.
    pub players: Vec<Player>,
    /// Current turn number (0-indexed).
    pub turn: u32,
    /// Maximum number of turns before game ends.
    pub max_turns: u32,
    /// Scoring weights for final score calculation.
    pub scoring: ScoringWeights,
}

impl GameState {
    /// Create a new game state with the given map and players.
    ///
    /// Players should already have their capitals set and the map should
    /// have the starting cities configured.
    #[must_use]
    pub fn new(map: Map, players: Vec<Player>, max_turns: u32) -> Self {
        Self {
            map,
            players,
            turn: 0,
            max_turns,
            scoring: ScoringWeights::default(),
        }
    }

    /// Get the current turn number.
    #[must_use]
    pub const fn turn(&self) -> u32 {
        self.turn
    }

    /// Check if the game is over.
    #[must_use]
    pub fn is_game_over(&self) -> bool {
        // Game ends if turn limit reached
        if self.turn >= self.max_turns {
            return true;
        }

        // Game ends if only one player (or zero) remains alive
        let alive_count = self.players.iter().filter(|p| p.alive).count();
        alive_count <= 1
    }

    /// Get a player by ID.
    #[must_use]
    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// Get a mutable reference to a player by ID.
    #[must_use]
    pub fn get_player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    /// Calculate the score for a player.
    #[must_use]
    pub fn calculate_score(&self, player_id: PlayerId) -> f64 {
        let population = self.map.total_population(player_id) as f64;
        let cities = self.map.count_cities(player_id) as f64;
        let territory = self.map.count_territory(player_id) as f64;

        population * self.scoring.population
            + cities * self.scoring.city
            + territory * self.scoring.territory
    }

    /// Get the food balance for a player.
    #[must_use]
    pub fn food_balance(&self, player_id: PlayerId) -> FoodBalance {
        calculate_food_balance(&self.map, player_id)
    }

    /// Check if a player can see a tile (fog of war).
    #[must_use]
    pub fn can_see_tile(&self, player_id: PlayerId, coord: Coord) -> bool {
        // Check if player owns this tile
        if let Some(tile) = self.map.get(coord) {
            if tile.owner == Some(player_id) {
                return true;
            }
        }

        // Check if player owns any adjacent tile
        let (adjacent, count) = coord.adjacent(self.map.width(), self.map.height());
        for adj in &adjacent[..count as usize] {
            if let Some(tile) = self.map.get(*adj) {
                if tile.owner == Some(player_id) {
                    return true;
                }
            }
        }

        false
    }

    /// Update visibility for a player (mark tiles as discovered).
    pub fn update_visibility(&mut self, player_id: PlayerId) {
        let width = self.map.width();
        let height = self.map.height();

        // Collect visible tiles
        let visible: Vec<Coord> = (0..height)
            .flat_map(|y| (0..width).map(move |x| Coord::new(x, y)))
            .filter(|&coord| self.can_see_tile(player_id, coord))
            .collect();

        // Mark as discovered
        if let Some(player) = self.get_player_mut(player_id) {
            for coord in visible {
                player.discover(coord);
            }
        }
    }

    /// Process economy phase for all players.
    pub fn process_economy(&mut self) {
        let rng_base = u64::from(self.turn) * 1_000_000;

        for player in &self.players {
            if !player.alive {
                continue;
            }

            let player_id = player.id;
            let rng_seed = rng_base + u64::from(player_id);
            let _result = apply_economy(&mut self.map, player_id, rng_seed);
        }
    }

    /// Check for eliminated players (capital captured or zero population).
    ///
    /// Uses pre-computed stats to avoid re-iterating the map.
    pub fn check_eliminations(&mut self, all_stats: &AllPlayerStats) {
        for player in &mut self.players {
            if !player.alive {
                continue;
            }

            // Check if capital is still owned
            let capital_owned = self
                .map
                .get(player.capital)
                .map_or(false, |tile| tile.owner == Some(player.id));

            // Use pre-computed stats instead of iterating map
            let total_pop = all_stats.get(player.id).population;

            if !capital_owned || total_pop == 0 {
                player.eliminate();
            }
        }
    }

    /// Process combat on all tiles.
    pub fn process_combat(&mut self) {
        resolve_combat(&mut self.map);
    }

    /// Advance to the next turn.
    pub fn advance_turn(&mut self) {
        self.turn += 1;
    }

    /// Get all alive players.
    pub fn alive_players(&self) -> impl Iterator<Item = &Player> {
        self.players.iter().filter(|p| p.alive)
    }

    /// Compute cached stats for all players in a single pass over the map.
    ///
    /// This is O(n) where n is the number of tiles, but only needs to be
    /// called once per turn. The returned stats can then be used for O(1)
    /// lookups during syscall handling.
    #[must_use]
    pub fn compute_all_player_stats(&self) -> AllPlayerStats {
        let mut result = AllPlayerStats::default();

        // Single pass over all tiles
        for (_, tile) in self.map.iter() {
            if let Some(owner) = tile.owner {
                if owner == 0 || owner as usize > MAX_PLAYERS {
                    continue;
                }
                let idx = (owner - 1) as usize;

                // Count territory (non-mountain)
                if tile.tile_type != TileType::Mountain {
                    result.stats[idx].territory += 1;
                }

                // Sum army
                result.stats[idx].army = result.stats[idx].army.saturating_add(tile.army);

                // Sum population (cities only)
                if tile.tile_type == TileType::City {
                    result.stats[idx].population =
                        result.stats[idx].population.saturating_add(tile.population);
                }
            }
        }

        // Compute food balance for each player using the collected stats
        for stats in &mut result.stats {
            if stats.territory > 0 {
                // Use the same formula as calculate_food_balance
                let production = Self::calculate_production(stats.territory, stats.population);
                let consumption = stats.population.saturating_add(stats.army);
                stats.food_balance = (production as i32).saturating_sub(consumption as i32);
            }
        }

        result
    }

    /// Calculate production using the Cobb-Douglas formula.
    ///
    /// This is the same formula used in economy.rs.
    fn calculate_production(territory: u32, population: u32) -> u32 {
        const WORKERS_PER_TILE: u32 = 50;
        const SCALING_FACTOR: f64 = 7.0;

        if territory == 0 || population == 0 {
            return 0;
        }

        let max_workers = territory.saturating_mul(WORKERS_PER_TILE);
        let effective_workers = population.min(max_workers);

        let labor_factor = (effective_workers as f64).sqrt();
        let land_factor = (territory as f64).sqrt();

        (labor_factor * land_factor * SCALING_FACTOR) as u32
    }

    /// Try to move a player's capital to a new city.
    ///
    /// Returns `true` if successful, `false` if the move is invalid.
    pub fn try_move_capital(&mut self, player_id: PlayerId, new_capital: Coord) -> bool {
        // Get current capital population
        let current_pop = {
            let Some(player) = self.get_player(player_id) else {
                return false;
            };
            let Some(tile) = self.map.get(player.capital) else {
                return false;
            };
            tile.population
        };

        // Check new capital is valid
        let Some(new_tile) = self.map.get(new_capital) else {
            return false;
        };

        if new_tile.tile_type != TileType::City {
            return false;
        }

        if new_tile.owner != Some(player_id) {
            return false;
        }

        if new_tile.population <= current_pop {
            return false;
        }

        // Move capital
        if let Some(player) = self.get_player_mut(player_id) {
            player.move_capital(new_capital);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Tile;

    fn create_test_game() -> GameState {
        let mut map = Map::new(10, 10).unwrap();

        // Set up player 1's city at (2, 2)
        let mut city1 = Tile::city(100);
        city1.owner = Some(1);
        city1.army = 10;
        map.set(Coord::new(2, 2), city1);

        // Set up player 2's city at (7, 7)
        let mut city2 = Tile::city(100);
        city2.owner = Some(2);
        city2.army = 10;
        map.set(Coord::new(7, 7), city2);

        let players = vec![
            Player::new(1, Coord::new(2, 2)),
            Player::new(2, Coord::new(7, 7)),
        ];

        GameState::new(map, players, 1000)
    }

    #[test]
    fn test_game_state_creation() {
        let game = create_test_game();
        assert_eq!(game.turn(), 0);
        assert_eq!(game.max_turns, 1000);
        assert!(!game.is_game_over());
    }

    #[test]
    fn test_score_calculation() {
        let game = create_test_game();

        // Player 1: 100 pop * 1 + 1 city * 10 + 1 territory * 0.5 = 110.5
        let score = game.calculate_score(1);
        assert!((score - 110.5).abs() < 0.001);
    }

    #[test]
    fn test_fog_of_war() {
        let game = create_test_game();

        // Player 1 can see their city
        assert!(game.can_see_tile(1, Coord::new(2, 2)));

        // Player 1 can see adjacent tiles
        assert!(game.can_see_tile(1, Coord::new(2, 3)));
        assert!(game.can_see_tile(1, Coord::new(3, 2)));

        // Player 1 cannot see far away
        assert!(!game.can_see_tile(1, Coord::new(7, 7)));

        // Player 2 can see their own area
        assert!(game.can_see_tile(2, Coord::new(7, 7)));
        assert!(!game.can_see_tile(2, Coord::new(2, 2)));
    }

    #[test]
    fn test_elimination_no_population() {
        let mut game = create_test_game();

        // Set player 1's population to 0
        if let Some(tile) = game.map.get_mut(Coord::new(2, 2)) {
            tile.population = 0;
        }

        let all_stats = game.compute_all_player_stats();
        game.check_eliminations(&all_stats);

        assert!(!game.get_player(1).unwrap().alive);
        assert!(game.get_player(2).unwrap().alive);
    }

    #[test]
    fn test_elimination_capital_captured() {
        let mut game = create_test_game();

        // Change ownership of player 1's capital
        if let Some(tile) = game.map.get_mut(Coord::new(2, 2)) {
            tile.owner = Some(2);
        }

        let all_stats = game.compute_all_player_stats();
        game.check_eliminations(&all_stats);

        assert!(!game.get_player(1).unwrap().alive);
        assert!(game.get_player(2).unwrap().alive);
    }

    #[test]
    fn test_game_over_turn_limit() {
        let mut game = create_test_game();
        game.turn = 1000;

        assert!(game.is_game_over());
    }

    #[test]
    fn test_game_over_one_player() {
        let mut game = create_test_game();
        game.players[1].eliminate();

        assert!(game.is_game_over());
    }

    #[test]
    fn test_move_capital() {
        let mut game = create_test_game();

        // Add a larger city for player 1
        let mut big_city = Tile::city(200);
        big_city.owner = Some(1);
        game.map.set(Coord::new(3, 3), big_city);

        // Should succeed - new city has more population
        assert!(game.try_move_capital(1, Coord::new(3, 3)));
        assert_eq!(game.get_player(1).unwrap().capital, Coord::new(3, 3));
    }

    #[test]
    fn test_move_capital_fails_smaller() {
        let mut game = create_test_game();

        // Add a smaller city for player 1
        let mut small_city = Tile::city(50);
        small_city.owner = Some(1);
        game.map.set(Coord::new(3, 3), small_city);

        // Should fail - new city has less population
        assert!(!game.try_move_capital(1, Coord::new(3, 3)));
        assert_eq!(game.get_player(1).unwrap().capital, Coord::new(2, 2));
    }
}
