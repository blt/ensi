//! Economy system: food production, growth, starvation, and rebellion.

use crate::game::{Map, PlayerId, TileType};

/// Food balance for a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoodBalance {
    /// Total food produced by population.
    pub production: i32,
    /// Total food consumed by population and army.
    pub consumption: i32,
    /// Net balance (production - consumption).
    pub balance: i32,
}

/// Calculate the food balance for a player.
///
/// - Each population produces 2 food and consumes 1 food (net +1)
/// - Each army unit consumes 1 food (net -1)
#[must_use]
pub fn calculate_food_balance(map: &Map, player: PlayerId) -> FoodBalance {
    let mut total_population: i32 = 0;
    let mut total_army: i32 = 0;

    for (_, tile) in map.tiles_owned_by(player) {
        if tile.tile_type == TileType::City {
            total_population = total_population.saturating_add(tile.population as i32);
        }
        total_army = total_army.saturating_add(tile.army as i32);
    }

    // Each population produces 2 food
    let production = total_population.saturating_mul(2);
    // Each population and army unit consumes 1 food
    let consumption = total_population.saturating_add(total_army);
    let balance = production - consumption;

    FoodBalance {
        production,
        consumption,
        balance,
    }
}

/// Result of applying economy for one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomyResult {
    /// Population change (positive = growth, negative = starvation deaths).
    pub population_change: i32,
    /// Cities that rebelled (became neutral).
    pub rebellions: Vec<crate::game::Coord>,
}

/// Apply economy effects for a player.
///
/// - If balance > 0: population grows by balance / 2
/// - If balance < 0: population dies equal to deficit
/// - Starving cities have a chance to rebel (flip neutral)
///
/// The `rng_seed` parameter is used for deterministic rebellion rolls.
/// It should be derived from the turn number and player ID.
pub fn apply_economy(
    map: &mut Map,
    player: PlayerId,
    rng_seed: u64,
) -> EconomyResult {
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
        let city_share = (u64::from(deficit) * u64::from(*city_pop) / u64::from(total_pop)) as u32;
        let deaths = city_share.min(remaining_deficit).min(*city_pop);

        if let Some(tile) = map.get_mut(*coord) {
            tile.population = tile.population.saturating_sub(deaths);
            result.population_change -= deaths as i32;

            // Check for rebellion
            // Probability = deficit / city_population
            if tile.population > 0 {
                let rebellion_chance = deficit as f64 / tile.population as f64;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Coord, Tile};

    fn setup_map_with_city(population: u32, army: u32) -> Map {
        let mut map = Map::new(10, 10).unwrap();
        let mut tile = Tile::city(population);
        tile.owner = Some(1);
        tile.army = army;
        map.set(Coord::new(5, 5), tile);
        map
    }

    #[test]
    fn test_food_balance_surplus() {
        // 100 population produces 200 food, consumes 100 food
        // 10 army consumes 10 food
        // Net: 200 - 100 - 10 = 90 surplus
        let map = setup_map_with_city(100, 10);
        let balance = calculate_food_balance(&map, 1);

        assert_eq!(balance.production, 200);
        assert_eq!(balance.consumption, 110);
        assert_eq!(balance.balance, 90);
    }

    #[test]
    fn test_food_balance_deficit() {
        // 10 population produces 20 food, consumes 10 food
        // 50 army consumes 50 food
        // Net: 20 - 10 - 50 = -40 deficit
        let map = setup_map_with_city(10, 50);
        let balance = calculate_food_balance(&map, 1);

        assert_eq!(balance.production, 20);
        assert_eq!(balance.consumption, 60);
        assert_eq!(balance.balance, -40);
    }

    #[test]
    fn test_growth() {
        let mut map = setup_map_with_city(100, 0);

        // Balance should be +100 (200 production - 100 consumption)
        // Growth should be 100 / 2 = 50
        let result = apply_economy(&mut map, 1, 12345);

        assert_eq!(result.population_change, 50);
        assert!(result.rebellions.is_empty());

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert_eq!(tile.population, 150);
    }

    #[test]
    fn test_starvation() {
        let mut map = setup_map_with_city(100, 300);

        // Balance: 200 - 100 - 300 = -200 deficit
        // 200 population should die (capped at 100)
        let result = apply_economy(&mut map, 1, 12345);

        assert!(result.population_change < 0);

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert!(tile.population < 100);
    }

    #[test]
    fn test_no_change_at_equilibrium() {
        // 100 population + 100 army
        // Production: 200, Consumption: 200
        // Balance: 0
        let mut map = setup_map_with_city(100, 100);
        let result = apply_economy(&mut map, 1, 12345);

        assert_eq!(result.population_change, 0);
        assert!(result.rebellions.is_empty());

        let tile = map.get(Coord::new(5, 5)).unwrap();
        assert_eq!(tile.population, 100);
    }
}
