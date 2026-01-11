//! Deterministic map generation for tournaments.

// Map generation uses intentional casts for coordinate/RNG operations
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::game::{Coord, Map, Player, PlayerId, Tile, TileType};

/// Deterministic PRNG using xorshift64.
#[derive(Debug, Clone, Copy)]
struct Rng {
    state: u64,
}

impl Rng {
    /// Create a new RNG with the given seed.
    const fn new(seed: u64) -> Self {
        // Ensure non-zero state
        let state = if seed == 0 { 0x5555_5555_5555_5555 } else { seed };
        Self { state }
    }

    /// Generate next random u64.
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Generate random u32 in [0, max).
    fn next_u32(&mut self, max: u32) -> u32 {
        if max == 0 {
            return 0;
        }
        (self.next_u64() % u64::from(max)) as u32
    }

    /// Generate random f64 in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }
}

/// Error type for map generation.
#[derive(Debug, Clone)]
pub struct MapGenError {
    /// Description of the error.
    pub reason: String,
}

impl std::fmt::Display for MapGenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Map generation error: {}", self.reason)
    }
}

impl std::error::Error for MapGenError {}

/// Generate a map and starting positions for all players.
///
/// # Arguments
///
/// * `seed` - Random seed for deterministic generation
/// * `width` - Map width in tiles
/// * `height` - Map height in tiles
/// * `num_players` - Number of players (2-8)
///
/// # Errors
///
/// Returns an error if dimensions are invalid or not enough starting positions.
pub fn generate_map(
    seed: u64,
    width: u16,
    height: u16,
    num_players: usize,
) -> Result<(Map, Vec<Player>), MapGenError> {
    if num_players < 2 {
        return Err(MapGenError {
            reason: format!("Need at least 2 players, got {num_players}"),
        });
    }
    if num_players > 8 {
        return Err(MapGenError {
            reason: format!("Maximum 8 players, got {num_players}"),
        });
    }

    let mut rng = Rng::new(seed);

    // Create base map with terrain
    let mut map = Map::new(width, height).ok_or_else(|| MapGenError {
        reason: "Invalid map dimensions (must be > 0)".to_string(),
    })?;

    // Generate terrain
    generate_terrain(&mut map, &mut rng);

    // Place neutral cities
    place_neutral_cities(&mut map, &mut rng);

    // Find starting positions for players
    let starting_positions = find_starting_positions(&map, num_players, &mut rng)?;

    // Create players and their starting cities
    let players = create_players(&mut map, &starting_positions);

    Ok((map, players))
}

/// Generate terrain (mountains on ~10% of tiles).
fn generate_terrain(map: &mut Map, rng: &mut Rng) {
    let width = map.width();
    let height = map.height();

    for y in 0..height {
        for x in 0..width {
            let coord = Coord::new(x, y);
            let noise = rng.next_f64();

            if noise < 0.10 {
                map.set(coord, Tile::mountain());
            }
            // Otherwise keep as desert (default)
        }
    }
}

/// Place neutral cities (~5% of non-mountain tiles).
fn place_neutral_cities(map: &mut Map, rng: &mut Rng) {
    let width = map.width();
    let height = map.height();

    // Target: ~5% of tiles are cities
    let total_tiles = usize::from(width) * usize::from(height);
    let target_cities = total_tiles / 20;

    // Use grid-based approach for more even distribution
    let grid_size = 8u16;
    let mut placed = 0;

    for grid_y in 0..(height.saturating_add(grid_size - 1) / grid_size) {
        for grid_x in 0..(width.saturating_add(grid_size - 1) / grid_size) {
            if placed >= target_cities {
                break;
            }

            // Pick random position in this grid cell
            let base_x = grid_x.saturating_mul(grid_size);
            let base_y = grid_y.saturating_mul(grid_size);

            let cell_width = grid_size.min(width.saturating_sub(base_x));
            let cell_height = grid_size.min(height.saturating_sub(base_y));

            if cell_width == 0 || cell_height == 0 {
                continue;
            }

            let x = base_x.saturating_add(rng.next_u32(u32::from(cell_width)) as u16);
            let y = base_y.saturating_add(rng.next_u32(u32::from(cell_height)) as u16);
            let coord = Coord::new(x, y);

            if let Some(tile) = map.get(coord)
                && tile.tile_type == TileType::Desert {
                    // Place neutral city with random population
                    let population = 50 + rng.next_u32(100);
                    map.set(coord, Tile::city(population));
                    placed += 1;
                }
        }
    }
}

/// Find starting positions for players (spread out, equidistant).
fn find_starting_positions(
    map: &Map,
    num_players: usize,
    rng: &mut Rng,
) -> Result<Vec<Coord>, MapGenError> {
    let width = map.width();
    let height = map.height();

    // Find all valid starting positions (desert tiles)
    let valid_positions: Vec<Coord> = (0..height)
        .flat_map(|y| (0..width).map(move |x| Coord::new(x, y)))
        .filter(|&coord| {
            map.get(coord)
                .is_some_and(|t| t.tile_type == TileType::Desert)
        })
        .collect();

    if valid_positions.len() < num_players {
        return Err(MapGenError {
            reason: format!(
                "Not enough valid starting positions: need {}, have {}",
                num_players,
                valid_positions.len()
            ),
        });
    }

    // Use angular placement around center for fairness
    let center_x = f64::from(width) / 2.0;
    let center_y = f64::from(height) / 2.0;
    let radius = f64::from(width.min(height)) * 0.35; // 35% of smaller dimension

    let mut positions = Vec::with_capacity(num_players);
    let angle_step = std::f64::consts::TAU / (num_players as f64);
    let angle_offset = rng.next_f64() * std::f64::consts::TAU; // Random rotation

    for i in 0..num_players {
        let angle = angle_offset + (i as f64) * angle_step;
        let target_x = center_x + radius * angle.cos();
        let target_y = center_y + radius * angle.sin();

        // Find nearest valid position to target that isn't already taken
        let best = valid_positions
            .iter()
            .filter(|&&coord| !positions.contains(&coord))
            .min_by_key(|&&coord| {
                let dx = f64::from(coord.x) - target_x;
                let dy = f64::from(coord.y) - target_y;
                #[allow(clippy::cast_possible_truncation)]
                {
                    ((dx * dx + dy * dy) * 1000.0) as u64
                }
            })
            .copied()
            .ok_or_else(|| MapGenError {
                reason: "Failed to find starting position".to_string(),
            })?;

        positions.push(best);
    }

    Ok(positions)
}

/// Create players and their starting cities.
///
/// Each player gets:
/// - Capital city with 100 population and 10 army
/// - 4 adjacent territory tiles (for stable starting economy)
///
/// With 5 territory tiles, starting food balance is positive:
/// - production = sqrt(100) × sqrt(5) × 7 ≈ 156
/// - consumption = 100 + 10 = 110
/// - balance = +46 (sustainable growth)
fn create_players(map: &mut Map, starting_positions: &[Coord]) -> Vec<Player> {
    let mut players = Vec::with_capacity(starting_positions.len());
    let width = map.width();
    let height = map.height();

    for (i, &coord) in starting_positions.iter().enumerate() {
        // Player IDs are 1-indexed
        #[allow(clippy::cast_possible_truncation)]
        let player_id: PlayerId = (i + 1) as PlayerId;

        // Create starting city (replaces whatever tile was there)
        let mut capital = Tile::city(100); // Standard starting population
        capital.owner = Some(player_id);
        capital.army = 10; // Small starting army
        map.set(coord, capital);

        // Give player starting territory (4 adjacent tiles)
        // This ensures positive food balance from turn 0
        let (adjacent, adj_count) = coord.adjacent(width, height);
        for adj in &adjacent[..adj_count as usize] {
            if let Some(tile) = map.get(*adj) {
                // Only claim passable tiles that aren't already cities
                if tile.tile_type.is_passable() && tile.tile_type != TileType::City {
                    let mut territory = Tile::desert();
                    territory.owner = Some(player_id);
                    map.set(*adj, territory);
                }
            }
        }

        // Create player
        players.push(Player::new(player_id, coord));
    }

    players
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_determinism() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(12345);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(54321);

        // Very unlikely to be equal with different seeds
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_map_generation_determinism() {
        let (map1, players1) = generate_map(42, 32, 32, 2).unwrap();
        let (map2, players2) = generate_map(42, 32, 32, 2).unwrap();

        // Same seed should produce same map
        assert_eq!(map1.width(), map2.width());
        assert_eq!(map1.height(), map2.height());

        // Check all tiles are identical
        for y in 0..map1.height() {
            for x in 0..map1.width() {
                let coord = Coord::new(x, y);
                let t1 = map1.get(coord).unwrap();
                let t2 = map2.get(coord).unwrap();
                assert_eq!(t1.tile_type, t2.tile_type);
                assert_eq!(t1.owner, t2.owner);
                assert_eq!(t1.army, t2.army);
                assert_eq!(t1.population, t2.population);
            }
        }

        // Same players
        assert_eq!(players1.len(), players2.len());
        for (p1, p2) in players1.iter().zip(players2.iter()) {
            assert_eq!(p1.id, p2.id);
            assert_eq!(p1.capital, p2.capital);
        }
    }

    #[test]
    fn test_map_generation_different_seeds() {
        let (map1, _) = generate_map(42, 32, 32, 2).unwrap();
        let (map2, _) = generate_map(43, 32, 32, 2).unwrap();

        // Different seeds should produce different maps
        // Check that at least some tiles differ
        let mut differences = 0;
        for y in 0..map1.height() {
            for x in 0..map1.width() {
                let coord = Coord::new(x, y);
                let t1 = map1.get(coord).unwrap();
                let t2 = map2.get(coord).unwrap();
                if t1.tile_type != t2.tile_type {
                    differences += 1;
                }
            }
        }
        assert!(differences > 0);
    }

    #[test]
    fn test_map_generation_player_count() {
        // Test various player counts
        for num_players in 2..=8 {
            let result = generate_map(42, 64, 64, num_players);
            assert!(result.is_ok());
            let (_, players) = result.unwrap();
            assert_eq!(players.len(), num_players);
        }
    }

    #[test]
    fn test_map_generation_too_few_players() {
        let result = generate_map(42, 32, 32, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_map_generation_too_many_players() {
        let result = generate_map(42, 32, 32, 9);
        assert!(result.is_err());
    }

    #[test]
    fn test_starting_cities_owned() {
        let (map, players) = generate_map(42, 32, 32, 4).unwrap();

        for player in &players {
            let tile = map.get(player.capital).unwrap();
            assert_eq!(tile.tile_type, TileType::City);
            assert_eq!(tile.owner, Some(player.id));
            assert_eq!(tile.population, 100);
            assert_eq!(tile.army, 10);
        }
    }

    #[test]
    fn test_starting_territory() {
        let (map, players) = generate_map(42, 64, 64, 2).unwrap();

        for player in &players {
            // Count owned tiles
            let owned_count = map
                .iter()
                .filter(|(_, tile)| tile.owner == Some(player.id))
                .count();

            // Should have at least 3 tiles (capital + 2 adjacent minimum)
            // Maximum is 5 tiles (capital + 4 adjacent)
            assert!(
                owned_count >= 3,
                "Player {} should have at least 3 territory tiles, got {}",
                player.id,
                owned_count
            );
            assert!(
                owned_count <= 5,
                "Player {} should have at most 5 territory tiles, got {}",
                player.id,
                owned_count
            );

            // Verify adjacent tiles are owned (not blocked by mountains)
            let mut adjacent_owned = 0;
            let (adjacent, adj_count) = player.capital.adjacent(map.width(), map.height());
            for adj in &adjacent[..adj_count as usize] {
                if let Some(tile) = map.get(*adj) {
                    if tile.owner == Some(player.id) {
                        adjacent_owned += 1;
                    }
                }
            }
            assert!(
                adjacent_owned >= 2,
                "Player {} should have at least 2 adjacent owned tiles, got {}",
                player.id,
                adjacent_owned
            );
        }
    }

    #[test]
    fn test_starting_economy_sustainable() {
        use crate::game::calculate_food_balance;

        let (map, players) = generate_map(42, 64, 64, 2).unwrap();

        for player in &players {
            let balance = calculate_food_balance(&map, player.id);

            // Starting balance should be positive (sustainable)
            assert!(
                balance.balance >= 0,
                "Player {} should have non-negative food balance, got {}",
                player.id,
                balance.balance
            );

            // Should have at least 3 territory tiles
            assert!(
                balance.territory >= 3,
                "Player {} should have at least 3 territory, got {}",
                player.id,
                balance.territory
            );
        }
    }
}
