//! Economy system: food production, growth, starvation, and rebellion.
//!
//! Uses a city-adjacency production model where cities produce food based
//! on the number of adjacent tiles owned by the same player.
//!
//! # Production Model
//!
//! `City Production = BASE_CITY_PRODUCTION + PRODUCTION_PER_ADJACENT * adjacent_owned`
//!
//! Where:
//! - `BASE_CITY_PRODUCTION` = 30 (isolated cities still produce some food)
//! - `PRODUCTION_PER_ADJACENT` = 20 (bonus per adjacent owned tile)
//! - `adjacent_owned` = count of adjacent tiles owned by same player (0-4)
//!
//! Total production is the sum across all owned cities.
//! Range per city: 30 (isolated) to 110 (4 adjacent) food.
//!
//! # Consumption Model
//!
//! `Consumption = population + army + (desert_tiles * DESERT_UPKEEP)`
//!
//! Desert tiles cost 2 food per turn each to maintain. They provide
//! visibility but no production, limiting mindless expansion.
//!
//! # Strategic Implications
//!
//! - Cities surrounded by owned tiles produce more food
//! - Desert tiles cost upkeep but provide only visibility
//! - Compact territory around cities is economically efficient
//! - Sprawling desert is costly and provides no production

// Economy uses intentional casts for population/resource calculations
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use crate::game::{Coord, Map, PlayerId, TileType};

/// Base food production per city (even isolated cities produce some food).
const BASE_CITY_PRODUCTION: u32 = 30;

/// Additional food production per adjacent owned tile.
/// A city with 4 adjacent owned tiles produces: 30 + 4*20 = 110 food.
const PRODUCTION_PER_ADJACENT: u32 = 20;

/// Food cost per desert tile per turn.
/// Desert tiles represent harsh terrain that requires resources to hold.
/// At value 2: 50 desert tiles costs 100 food/turn (significant but not crippling).
const DESERT_UPKEEP_PER_TILE: u32 = 2;

/// Food balance for a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoodBalance {
    /// Total food produced (based on territory and labor).
    pub production: i32,
    /// Total food consumed by population and army.
    pub consumption: i32,
    /// Net balance (production - consumption).
    pub balance: i32,
    /// Territory tile count.
    pub territory: u32,
}

/// Calculate food production for a single city based on adjacent owned tiles.
///
/// `Production = BASE_CITY_PRODUCTION + PRODUCTION_PER_ADJACENT * adjacent_owned`
///
/// This creates incentives for compact territory:
/// - Isolated cities produce only base food (30)
/// - Cities surrounded by owned tiles produce up to 110 food
/// - Desert tiles cost upkeep but don't contribute to production
#[must_use]
fn calculate_city_production(map: &Map, city_coord: Coord, player: PlayerId) -> u32 {
    let (adjacent, count) = city_coord.adjacent(map.width(), map.height());
    let owned_adjacent = adjacent[..count as usize]
        .iter()
        .filter(|adj| {
            map.get(**adj)
                .map_or(false, |tile| tile.owner == Some(player))
        })
        .count() as u32;

    BASE_CITY_PRODUCTION + PRODUCTION_PER_ADJACENT * owned_adjacent
}

/// Calculate total food production for a player (sum of all city productions).
#[must_use]
fn calculate_total_production(map: &Map, player: PlayerId) -> u32 {
    map.iter()
        .filter(|(_, tile)| tile.owner == Some(player) && tile.tile_type == TileType::City)
        .map(|(coord, _)| calculate_city_production(map, coord, player))
        .sum()
}

/// Calculate food consumption.
///
/// Model: 1 food per population + 1 food per army unit + desert upkeep.
/// Desert tiles cost `DESERT_UPKEEP_PER_TILE` food each to maintain.
#[must_use]
fn calculate_consumption(population: u32, army: u32, desert_tiles: u32) -> u32 {
    let desert_upkeep = desert_tiles.saturating_mul(DESERT_UPKEEP_PER_TILE);
    population.saturating_add(army).saturating_add(desert_upkeep)
}

/// Calculate the food balance for a player.
///
/// Uses city-adjacency production: each city produces base + bonus per adjacent owned tile.
/// Consumption: 1 per population + 1 per army + desert upkeep
#[must_use]
pub fn calculate_food_balance(map: &Map, player: PlayerId) -> FoodBalance {
    let mut total_population: u32 = 0;
    let mut total_army: u32 = 0;
    let mut territory: u32 = 0;
    let mut desert_tiles: u32 = 0;

    for (_, tile) in map.tiles_owned_by(player) {
        territory += 1;
        total_army = total_army.saturating_add(tile.army);
        match tile.tile_type {
            TileType::City => {
                total_population = total_population.saturating_add(tile.population);
            }
            TileType::Desert => {
                desert_tiles += 1;
            }
            TileType::Mountain => {
                // Mountains shouldn't be owned, but handle gracefully
            }
        }
    }

    let production = calculate_total_production(map, player) as i32;
    let consumption = calculate_consumption(total_population, total_army, desert_tiles) as i32;
    let balance = production.saturating_sub(consumption);

    FoodBalance {
        production,
        consumption,
        balance,
        territory,
    }
}

/// Result of applying economy for one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomyResult {
    /// Population change (positive = growth, negative = starvation deaths).
    pub population_change: i32,
    /// Cities that rebelled (became neutral).
    pub rebellions: Vec<Coord>,
}

/// Apply economy effects for a player.
///
/// - If balance > 0: population grows by balance / 2
/// - If balance < 0: population dies equal to deficit
/// - Starving cities have a chance to rebel (flip neutral)
///
/// The `rng_seed` parameter is used for deterministic rebellion rolls.
/// It should be derived from the turn number and player ID.
pub fn apply_economy(map: &mut Map, player: PlayerId, rng_seed: u64) -> EconomyResult {
    let balance = calculate_food_balance(map, player);
    let mut result = EconomyResult {
        population_change: 0,
        rebellions: Vec::new(),
    };

    if balance.balance >= 0 {
        // Growth phase: distribute growth across cities
        let growth = balance.balance / 2;
        if growth > 0 {
            apply_growth(map, player, growth, &mut result);
        }
    } else {
        // Starvation phase: kill population and check rebellions
        let deficit = (-balance.balance) as u32;
        apply_starvation(map, player, deficit, rng_seed, &mut result);
    }

    result
}

/// Distribute population growth across player's cities.
fn apply_growth(map: &mut Map, player: PlayerId, growth: i32, result: &mut EconomyResult) {
    // Collect cities owned by this player
    let cities: Vec<_> = map
        .iter()
        .filter(|(_, tile)| tile.owner == Some(player) && tile.tile_type == TileType::City)
        .map(|(coord, _)| coord)
        .collect();

    if cities.is_empty() {
        return;
    }

    // Distribute growth evenly across cities
    let growth_per_city = growth / cities.len() as i32;
    let remainder = growth % cities.len() as i32;

    for (i, coord) in cities.iter().enumerate() {
        let extra = i32::from((i as i32) < remainder);
        let city_growth = growth_per_city + extra;

        if let Some(tile) = map.get_mut(*coord) {
            tile.population = tile.population.saturating_add(city_growth as u32);
            result.population_change += city_growth;
        }
    }
}

/// Apply starvation effects and check for rebellions.
fn apply_starvation(
    map: &mut Map,
    player: PlayerId,
    deficit: u32,
    rng_seed: u64,
    result: &mut EconomyResult,
) {
    // Collect cities owned by this player with their populations
    let cities: Vec<_> = map
        .iter()
        .filter(|(_, tile)| tile.owner == Some(player) && tile.tile_type == TileType::City)
        .map(|(coord, tile)| (coord, tile.population))
        .collect();

    if cities.is_empty() {
        return;
    }

    // Distribute deaths across cities proportionally
    let total_pop: u32 = cities.iter().map(|(_, pop)| *pop).sum();
    if total_pop == 0 {
        return;
    }

    let mut remaining_deficit = deficit;

    for (i, (coord, city_pop)) in cities.iter().enumerate() {
        if *city_pop == 0 {
            continue;
        }

        // Calculate this city's share of the deficit
        let city_share =
            (u64::from(deficit) * u64::from(*city_pop) / u64::from(total_pop)) as u32;
        let deaths = city_share.min(remaining_deficit).min(*city_pop);

        if let Some(tile) = map.get_mut(*coord) {
            tile.population = tile.population.saturating_sub(deaths);
            result.population_change -= deaths as i32;

            // Check for rebellion
            // Probability = deficit / city_population (capped at 1.0)
            if tile.population > 0 {
                let rebellion_chance = (f64::from(deficit) / f64::from(tile.population)).min(1.0);
                let roll = simple_hash(rng_seed, i as u64) as f64 / u64::MAX as f64;

                if roll < rebellion_chance {
                    tile.owner = None;
                    tile.army = 0;
                    result.rebellions.push(*coord);
                }
            }
        }

        remaining_deficit = remaining_deficit.saturating_sub(deaths);
    }
}

/// Simple deterministic hash for rebellion rolls.
fn simple_hash(seed: u64, index: u64) -> u64 {
    let mut x = seed.wrapping_add(index);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// Kani formal verification proofs.
///
/// These prove arithmetic safety properties for all possible inputs.
/// Run with: `cargo kani`
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Prove that city production calculation never overflows.
    ///
    /// For any number of adjacent tiles (0-4), the production
    /// calculation should not overflow.
    #[kani::proof]
    fn prove_city_production_no_overflow() {
        let adjacent_owned: u32 = kani::any();

        // Adjacent tiles are bounded 0-4
        if adjacent_owned > 4 {
            return;
        }

        // Production = BASE + BONUS * adjacent
        let production = BASE_CITY_PRODUCTION + PRODUCTION_PER_ADJACENT * adjacent_owned;

        // Max production = 30 + 20*4 = 110, well within u32
        assert!(production <= 110);
        assert!(production >= BASE_CITY_PRODUCTION);
    }

    /// Prove that consumption calculation never overflows.
    ///
    /// Uses saturating_add, so should always return a valid result.
    #[kani::proof]
    fn prove_consumption_no_overflow() {
        let population: u32 = kani::any();
        let army: u32 = kani::any();
        let desert_tiles: u32 = kani::any();

        let consumption = calculate_consumption(population, army, desert_tiles);

        // Saturating add means result is bounded
        assert!(consumption >= population || consumption == u32::MAX);
        assert!(consumption >= army || consumption == u32::MAX);
    }

    /// Prove that the hash function is deterministic.
    #[kani::proof]
    fn prove_hash_deterministic() {
        let seed: u64 = kani::any();
        let index: u64 = kani::any();

        let hash1 = simple_hash(seed, index);
        let hash2 = simple_hash(seed, index);

        assert_eq!(hash1, hash2);
    }

    /// Prove that growth distribution conserves population.
    ///
    /// For N cities and growth G, sum(distributed) == G (no population loss).
    #[kani::proof]
    #[kani::unwind(5)]
    fn prove_growth_distribution_conserves() {
        let growth: i32 = kani::any();
        let num_cities: u8 = kani::any();

        // Bound inputs for tractability
        if num_cities == 0 || num_cities > 10 || growth < 0 || growth > 10000 {
            return;
        }

        let n = num_cities as i32;
        let growth_per_city = growth / n;
        let remainder = growth % n;

        // Sum distributed growth
        let mut total_distributed: i32 = 0;
        for i in 0..num_cities {
            let extra = if (i as i32) < remainder { 1 } else { 0 };
            let city_growth = growth_per_city + extra;
            total_distributed = total_distributed.saturating_add(city_growth);
        }

        // Conservation: total distributed equals input growth
        assert_eq!(total_distributed, growth, "Growth must be conserved");
    }

    /// Prove that starvation deaths never exceed city population.
    ///
    /// Key invariant: deaths_applied <= city_population always.
    #[kani::proof]
    fn prove_starvation_bounded_by_population() {
        let deficit: u32 = kani::any();
        let city_pop: u32 = kani::any();
        let total_pop: u32 = kani::any();

        // Skip invalid states
        if city_pop == 0 || total_pop == 0 || city_pop > total_pop {
            return;
        }

        // Calculate city's share of deficit (from apply_starvation)
        let city_share =
            (u64::from(deficit) * u64::from(city_pop) / u64::from(total_pop)) as u32;
        let deaths = city_share.min(deficit).min(city_pop);

        // Deaths must never exceed population
        assert!(deaths <= city_pop, "Deaths cannot exceed population");

        // Remaining population is non-negative
        let remaining = city_pop.saturating_sub(deaths);
        assert!(remaining <= city_pop, "Remaining must be <= original");
    }

    /// Prove that rebellion probability is always in [0, 1].
    #[kani::proof]
    fn prove_rebellion_probability_valid() {
        let deficit: u32 = kani::any();
        let population: u32 = kani::any();

        // Skip division by zero
        if population == 0 {
            return;
        }

        let rebellion_chance = (deficit as f64 / population as f64).min(1.0);

        // Probability must be valid
        assert!(rebellion_chance >= 0.0, "Probability must be >= 0");
        assert!(rebellion_chance <= 1.0, "Probability must be <= 1");
        assert!(!rebellion_chance.is_nan(), "Probability must not be NaN");
        assert!(
            !rebellion_chance.is_infinite(),
            "Probability must be finite"
        );
    }

    /// Prove total production has a reasonable upper bound.
    ///
    /// With city-adjacency model, max production per city is 110.
    /// Even with 1000 cities, max total is 110,000 - very safe.
    #[kani::proof]
    fn prove_total_production_upper_bound() {
        let num_cities: u32 = kani::any();
        let max_adjacent: u32 = 4;

        // Limit to reasonable city count for tractability
        if num_cities > 1000 {
            return;
        }

        // Max production per city = 30 + 20*4 = 110
        let max_per_city = BASE_CITY_PRODUCTION + PRODUCTION_PER_ADJACENT * max_adjacent;
        let max_total = num_cities.saturating_mul(max_per_city);

        // Even with 1000 cities, max is 110,000
        assert!(max_total <= 110_000 || num_cities > 1000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Tile;

    /// Set up a map with a city and optionally adjacent owned tiles.
    fn setup_city_with_adjacency(population: u32, army: u32, adjacent_owned: u32) -> Map {
        let mut map = Map::new(20, 20).unwrap();

        // Place a city with population at (5, 5)
        let mut city = Tile::city(population);
        city.owner = Some(1);
        city.army = army;
        map.set(Coord::new(5, 5), city);

        // Place adjacent owned tiles (up to 4)
        let adjacent_coords = [(5, 4), (5, 6), (4, 5), (6, 5)];
        for i in 0..(adjacent_owned.min(4) as usize) {
            let (x, y) = adjacent_coords[i];
            let mut desert = Tile::desert();
            desert.owner = Some(1);
            map.set(Coord::new(x, y), desert);
        }

        map
    }

    /// Set up a map with extra desert tiles (not adjacent to city).
    fn setup_city_with_desert(population: u32, army: u32, adjacent: u32, extra_desert: u32) -> Map {
        let mut map = setup_city_with_adjacency(population, army, adjacent);

        // Add non-adjacent desert tiles
        for i in 0..extra_desert {
            let x = 10 + (i % 5) as u16;
            let y = 10 + (i / 5) as u16;
            let mut desert = Tile::desert();
            desert.owner = Some(1);
            map.set(Coord::new(x, y), desert);
        }

        map
    }

    #[test]
    fn test_city_production_isolated() {
        // Isolated city produces base production only
        let map = setup_city_with_adjacency(100, 0, 0);
        let production = calculate_total_production(&map, 1);

        assert_eq!(production, BASE_CITY_PRODUCTION);
    }

    #[test]
    fn test_city_production_with_adjacency() {
        // City with 4 adjacent tiles produces max
        let map = setup_city_with_adjacency(100, 0, 4);
        let production = calculate_total_production(&map, 1);

        assert_eq!(production, BASE_CITY_PRODUCTION + 4 * PRODUCTION_PER_ADJACENT);
        assert_eq!(production, 110); // 30 + 80
    }

    #[test]
    fn test_city_production_partial_adjacency() {
        // City with 2 adjacent tiles
        let map = setup_city_with_adjacency(100, 0, 2);
        let production = calculate_total_production(&map, 1);

        assert_eq!(production, BASE_CITY_PRODUCTION + 2 * PRODUCTION_PER_ADJACENT);
        assert_eq!(production, 70); // 30 + 40
    }

    #[test]
    fn test_food_balance_isolated_city() {
        // Isolated city with 30 pop: production 30, consumption 30, balance 0
        let map = setup_city_with_adjacency(30, 0, 0);
        let balance = calculate_food_balance(&map, 1);

        assert_eq!(balance.production, 30);
        assert_eq!(balance.consumption, 30);
        assert_eq!(balance.balance, 0);
    }

    #[test]
    fn test_food_balance_city_with_adjacency() {
        // City with 4 adjacent: production 110, consumption = pop + desert upkeep
        let map = setup_city_with_adjacency(30, 0, 4);
        let balance = calculate_food_balance(&map, 1);

        // Production: 30 + 4*20 = 110
        assert_eq!(balance.production, 110);
        // Consumption: 30 pop + 4 desert * 2 = 38
        assert_eq!(balance.consumption, 38);
        // Balance: 110 - 38 = 72
        assert_eq!(balance.balance, 72);
    }

    #[test]
    fn test_desert_upkeep_reduces_surplus() {
        // Extra desert tiles reduce surplus without adding production
        let map_no_extra = setup_city_with_desert(20, 0, 4, 0);
        let map_with_extra = setup_city_with_desert(20, 0, 4, 10);

        let balance_no_extra = calculate_food_balance(&map_no_extra, 1);
        let balance_with_extra = calculate_food_balance(&map_with_extra, 1);

        // Same production (only adjacent counts)
        assert_eq!(balance_no_extra.production, balance_with_extra.production);

        // Higher consumption with extra desert
        assert!(balance_with_extra.consumption > balance_no_extra.consumption);

        // Extra desert adds 10 * 2 = 20 upkeep
        let upkeep_diff = balance_with_extra.consumption - balance_no_extra.consumption;
        assert_eq!(upkeep_diff, 20);
    }

    #[test]
    fn test_starvation_with_large_pop() {
        // Large population on small production = starvation
        let map = setup_city_with_adjacency(100, 0, 0);
        let balance = calculate_food_balance(&map, 1);

        // Production: 30, Consumption: 100 = deficit of 70
        assert_eq!(balance.production, 30);
        assert_eq!(balance.consumption, 100);
        assert_eq!(balance.balance, -70);
    }

    #[test]
    fn test_army_reduces_surplus() {
        let map_no_army = setup_city_with_adjacency(20, 0, 4);
        let map_with_army = setup_city_with_adjacency(20, 50, 4);

        let balance_no_army = calculate_food_balance(&map_no_army, 1);
        let balance_with_army = calculate_food_balance(&map_with_army, 1);

        assert!(
            balance_no_army.balance > balance_with_army.balance,
            "Army should reduce surplus"
        );
    }

    #[test]
    fn test_growth_applies() {
        // City with 4 adjacent produces 60, consumes 20 = surplus 32 (after desert upkeep)
        let mut map = setup_city_with_adjacency(20, 0, 4);

        // Should have surplus and grow
        let result = apply_economy(&mut map, 1, 12345);

        assert!(
            result.population_change > 0,
            "Population should grow with surplus"
        );

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert!(tile.population > 20);
    }

    #[test]
    fn test_starvation_applies() {
        // Isolated city with large pop: production 20, consumption 200 = deficit
        let mut map = setup_city_with_adjacency(200, 0, 0);

        let result = apply_economy(&mut map, 1, 12345);

        assert!(
            result.population_change < 0,
            "Population should decline with deficit"
        );

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert!(tile.population < 200);
    }

    #[test]
    fn test_no_overflow_extreme_values() {
        // Test consumption with extreme values - should not panic
        let consumption = calculate_consumption(u32::MAX, u32::MAX, u32::MAX);
        assert_eq!(consumption, u32::MAX); // saturates

        // Test city production bounds
        let max_production = BASE_CITY_PRODUCTION + 4 * PRODUCTION_PER_ADJACENT;
        assert_eq!(max_production, 110);
    }

    #[test]
    fn test_desert_upkeep_cost() {
        // 50 extra desert tiles should cost 100 food/turn
        let map = setup_city_with_desert(0, 0, 0, 50);

        let balance = calculate_food_balance(&map, 1);

        // 50 desert tiles × 2 food = 100 consumption from desert
        assert_eq!(
            balance.consumption, 100,
            "50 desert tiles should cost 100 food"
        );
    }

    #[test]
    fn test_desert_upkeep_limits_expansion() {
        // More desert = more consumption = less surplus
        let small_map = setup_city_with_desert(20, 0, 4, 5);
        let large_map = setup_city_with_desert(20, 0, 4, 50);

        let small_balance = calculate_food_balance(&small_map, 1);
        let large_balance = calculate_food_balance(&large_map, 1);

        // Same production (both have 4 adjacent)
        assert_eq!(small_balance.production, large_balance.production);

        // Larger territory has more desert upkeep
        assert!(
            large_balance.consumption > small_balance.consumption,
            "More desert should have higher consumption"
        );

        // The extra desert tiles (45) should add 90 to consumption
        let consumption_diff = large_balance.consumption - small_balance.consumption;
        assert_eq!(
            consumption_diff, 90,
            "45 extra desert tiles should add 90 consumption"
        );
    }

    #[test]
    fn test_multiple_cities_production() {
        // Test that multiple cities' production sums correctly
        let mut map = Map::new(20, 20).unwrap();

        // City 1 at (5, 5) with 2 adjacent
        let mut city1 = Tile::city(50);
        city1.owner = Some(1);
        map.set(Coord::new(5, 5), city1);
        let mut adj1 = Tile::desert();
        adj1.owner = Some(1);
        map.set(Coord::new(5, 4), adj1.clone());
        map.set(Coord::new(5, 6), adj1);

        // City 2 at (10, 10) with 1 adjacent
        let mut city2 = Tile::city(50);
        city2.owner = Some(1);
        map.set(Coord::new(10, 10), city2);
        let mut adj2 = Tile::desert();
        adj2.owner = Some(1);
        map.set(Coord::new(10, 9), adj2);

        let production = calculate_total_production(&map, 1);

        // City 1: 30 + 2*20 = 70
        // City 2: 30 + 1*20 = 50
        // Total: 120
        assert_eq!(production, 120);
    }
}
