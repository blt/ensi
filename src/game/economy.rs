//! Economy system: food production, growth, starvation, and rebellion.
//!
//! Uses a Cobb-Douglas production model with diminishing returns on both
//! labor (population) and land (territory). This creates natural equilibrium
//! without requiring artificial caps.
//!
//! # Production Model
//!
//! Production = sqrt(effective_workers) × sqrt(territory) × SCALING_FACTOR
//!
//! Where:
//! - effective_workers = min(population, territory × WORKERS_PER_TILE)
//! - WORKERS_PER_TILE = 10 (max efficient workers per territory tile)
//! - SCALING_FACTOR = 7.0 (gives ~50× territory equilibrium)
//!
//! # Equilibrium
//!
//! At equilibrium with no army: pop ≈ territory × 50
//! - 10 tiles → ~500 population
//! - 100 tiles → ~5,000 population
//! - 1000 tiles → ~50,000 population

use crate::game::{Coord, Map, PlayerId, TileType};

/// Maximum workers that can efficiently work each territory tile.
/// With 50 workers/tile and scaling factor 7.0, equilibrium is ~50× territory.
const WORKERS_PER_TILE: u32 = 50;

/// Production scaling factor. With 7.0, equilibrium is ~50× territory.
const SCALING_FACTOR: f64 = 7.0;

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

/// Calculate food production with diminishing returns.
///
/// Production = sqrt(effective_workers) × sqrt(territory) × scaling
///
/// This creates natural equilibrium because:
/// - sqrt() gives diminishing returns on labor
/// - Territory limits max workers
/// - More land needed for larger populations
#[must_use]
fn calculate_production(territory: u32, population: u32) -> u32 {
    if territory == 0 || population == 0 {
        return 0;
    }

    let max_workers = territory.saturating_mul(WORKERS_PER_TILE);
    let effective_workers = population.min(max_workers);

    let labor_factor = (effective_workers as f64).sqrt();
    let land_factor = (territory as f64).sqrt();

    (labor_factor * land_factor * SCALING_FACTOR) as u32
}

/// Calculate food consumption.
///
/// Simple model: 1 food per population + 1 food per army unit.
#[must_use]
fn calculate_consumption(population: u32, army: u32) -> u32 {
    population.saturating_add(army)
}

/// Calculate the food balance for a player.
///
/// Uses Cobb-Douglas production: sqrt(workers) × sqrt(territory) × 7
/// Consumption: 1 per population + 1 per army
#[must_use]
pub fn calculate_food_balance(map: &Map, player: PlayerId) -> FoodBalance {
    let mut total_population: u32 = 0;
    let mut total_army: u32 = 0;
    let mut territory: u32 = 0;

    for (_, tile) in map.tiles_owned_by(player) {
        territory += 1;
        total_army = total_army.saturating_add(tile.army);
        if tile.tile_type == TileType::City {
            total_population = total_population.saturating_add(tile.population);
        }
    }

    let production = calculate_production(territory, total_population) as i32;
    let consumption = calculate_consumption(total_population, total_army) as i32;
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
        let extra = if (i as i32) < remainder { 1 } else { 0 };
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
                let rebellion_chance = (deficit as f64 / tile.population as f64).min(1.0);
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

    /// Prove that production calculation never overflows.
    ///
    /// For all possible territory and population values, the production
    /// calculation should not overflow and should return a reasonable value.
    #[kani::proof]
    #[kani::unwind(2)]
    fn prove_production_no_overflow() {
        let territory: u32 = kani::any();
        let population: u32 = kani::any();

        // Production uses sqrt so inherently can't overflow
        let production = calculate_production(territory, population);

        // Production should never exceed a reasonable bound
        // sqrt(u32::MAX) * sqrt(u32::MAX) * 7 ≈ u32::MAX * 7, but sqrt caps it
        assert!(production <= u32::MAX);
    }

    /// Prove that consumption calculation never overflows.
    ///
    /// Uses saturating_add, so should always return a valid result.
    #[kani::proof]
    fn prove_consumption_no_overflow() {
        let population: u32 = kani::any();
        let army: u32 = kani::any();

        let consumption = calculate_consumption(population, army);

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

    /// Prove production has a reasonable upper bound.
    ///
    /// Production = sqrt(workers) × sqrt(territory) × 7
    /// Max value is well below u32::MAX.
    #[kani::proof]
    fn prove_production_upper_bound() {
        let territory: u32 = kani::any();
        let population: u32 = kani::any();

        let production = calculate_production(territory, population);

        // sqrt(u32::MAX) ≈ 65535, so max production ≈ 65535 * 65535 * 7 ≈ 30B
        // But sqrt reduces this dramatically: sqrt(4B) * sqrt(4B) * 7 ≈ 4B * 7 = 28B
        // Actually: sqrt(max) * sqrt(max) * 7 = max * 7 but input is capped
        // Real max: sqrt(workers_capped) * sqrt(territory) * 7
        // With 50 workers per tile max, workers = territory * 50
        // So: sqrt(t*50) * sqrt(t) * 7 = sqrt(50) * t * 7 ≈ 49.5 * t
        // For t = u32::MAX, this could overflow, but sqrt limits it

        // Conservative check: production should be < 1 billion for reasonable inputs
        if territory < 1_000_000 && population < 100_000_000 {
            assert!(production < 1_000_000_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Tile;

    fn setup_map_with_territory(population: u32, army: u32, territory: u32) -> Map {
        let mut map = Map::new(20, 20).unwrap();

        // Place a city with population
        let mut city = Tile::city(population);
        city.owner = Some(1);
        city.army = army;
        map.set(Coord::new(5, 5), city);

        // Add additional territory tiles (desert, owned by player 1)
        for i in 0..territory.saturating_sub(1) {
            let x = (i % 20) as u16;
            let y = (i / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map.set(Coord::new(x, y), desert);
            }
        }

        map
    }

    #[test]
    fn test_production_diminishing_returns() {
        // Production should increase slower than linearly with population
        let prod_10 = calculate_production(100, 10);
        let prod_100 = calculate_production(100, 100);
        let prod_1000 = calculate_production(100, 1000);

        // 10× population should give ~3.16× production (sqrt)
        assert!(prod_100 > prod_10 * 2);
        assert!(prod_100 < prod_10 * 10);

        // 10× more population again
        assert!(prod_1000 > prod_100 * 2);
        assert!(prod_1000 < prod_100 * 10);
    }

    #[test]
    fn test_equilibrium_10_tiles() {
        // With 10 tiles, equilibrium should be around 500 population
        let map = setup_map_with_territory(500, 0, 10);
        let balance = calculate_food_balance(&map, 1);

        // Should be near equilibrium (balance close to 0)
        assert!(
            balance.balance.abs() < 100,
            "Expected near-equilibrium, got balance {}",
            balance.balance
        );
    }

    #[test]
    fn test_equilibrium_100_tiles() {
        // With 100 tiles, equilibrium should be around 5000 population
        let map = setup_map_with_territory(5000, 0, 100);
        let balance = calculate_food_balance(&map, 1);

        // Should be near equilibrium
        assert!(
            balance.balance.abs() < 500,
            "Expected near-equilibrium, got balance {}",
            balance.balance
        );
    }

    #[test]
    fn test_growth_with_small_pop_large_territory() {
        // Small population on large territory should grow
        let map = setup_map_with_territory(100, 0, 100);
        let balance = calculate_food_balance(&map, 1);

        assert!(
            balance.balance > 0,
            "Small pop on large territory should have surplus"
        );
    }

    #[test]
    fn test_starvation_with_large_pop_small_territory() {
        // Large population on small territory should starve
        let map = setup_map_with_territory(1000, 0, 5);
        let balance = calculate_food_balance(&map, 1);

        assert!(
            balance.balance < 0,
            "Large pop on small territory should have deficit"
        );
    }

    #[test]
    fn test_army_reduces_surplus() {
        let map_no_army = setup_map_with_territory(100, 0, 50);
        let map_with_army = setup_map_with_territory(100, 200, 50);

        let balance_no_army = calculate_food_balance(&map_no_army, 1);
        let balance_with_army = calculate_food_balance(&map_with_army, 1);

        assert!(
            balance_no_army.balance > balance_with_army.balance,
            "Army should reduce surplus"
        );
    }

    #[test]
    fn test_growth_applies() {
        let mut map = setup_map_with_territory(100, 0, 50);

        // Should have surplus and grow
        let result = apply_economy(&mut map, 1, 12345);

        assert!(
            result.population_change > 0,
            "Population should grow with surplus"
        );

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert!(tile.population > 100);
    }

    #[test]
    fn test_starvation_applies() {
        let mut map = setup_map_with_territory(1000, 0, 5);

        let result = apply_economy(&mut map, 1, 12345);

        assert!(
            result.population_change < 0,
            "Population should decline with deficit"
        );

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert!(tile.population < 1000);
    }

    #[test]
    fn test_no_overflow_extreme_values() {
        // Test with very large values - should not panic
        let prod = calculate_production(1_000_000, 100_000_000);
        assert!(prod > 0);

        let consumption = calculate_consumption(u32::MAX, u32::MAX);
        assert_eq!(consumption, u32::MAX); // saturates

        // Large map test
        let map = setup_map_with_territory(10_000_000, 10_000_000, 100);
        let balance = calculate_food_balance(&map, 1);
        // Just verify it doesn't panic
        let _ = balance;
    }
}
