#![no_main]

use arbitrary::Arbitrary;
use ensi::game::{
    apply_economy, calculate_food_balance, check_invariants, Coord, GameState, Map, Player, Tile,
};
use libfuzzer_sys::fuzz_target;

/// Structured input for economy fuzzing.
#[derive(Arbitrary, Debug)]
struct EconomyInput {
    /// Population in the main city (capped to avoid OOM).
    population: u32,
    /// Army on the main city tile (capped to avoid OOM).
    army: u32,
    /// Number of additional territory tiles.
    extra_territory: u8,
    /// Number of additional cities (for multi-city testing).
    extra_cities: u8,
    /// Population for extra cities.
    extra_city_pop: u32,
    /// RNG seed for rebellion rolls.
    rng_seed: u64,
}

fuzz_target!(|input: EconomyInput| {
    // Cap inputs to reasonable values to avoid OOM
    let pop = input.population.min(10_000_000);
    let army = input.army.min(10_000_000);
    let territory = (input.extra_territory as u32).min(100);
    let extra_cities = (input.extra_cities as u32).min(10);
    let extra_city_pop = input.extra_city_pop.min(1_000_000);

    // Create map with city and territory
    let mut map = match Map::new(20, 20) {
        Some(m) => m,
        None => return,
    };

    // Place main city with population
    let mut city = Tile::city(pop);
    city.owner = Some(1);
    city.army = army;
    map.set(Coord::new(5, 5), city);

    // Add extra cities for multi-city testing
    for i in 0..extra_cities {
        let x = 6 + (i % 10) as u16;
        let y = 6 + (i / 10) as u16;
        if x < 20 && y < 20 {
            let mut extra_city = Tile::city(extra_city_pop);
            extra_city.owner = Some(1);
            map.set(Coord::new(x, y), extra_city);
        }
    }

    // Add extra territory
    for i in 0..territory {
        let x = ((i + 1) % 20) as u16;
        let y = ((i + 1) / 20) as u16;
        if (x, y) != (5, 5) && map.get(Coord::new(x, y)).map_or(true, |t| t.owner.is_none()) {
            let mut desert = Tile::desert();
            desert.owner = Some(1);
            map.set(Coord::new(x, y), desert);
        }
    }

    // Test calculate_food_balance - must not panic
    let balance = calculate_food_balance(&map, 1);

    // Sanity checks on balance
    assert!(balance.production >= 0, "Production should be non-negative");
    assert!(balance.consumption >= 0, "Consumption should be non-negative");
    assert!(
        balance.territory <= 400,
        "Territory should be bounded by map size"
    );

    // Create a minimal GameState for invariant checking
    let players = vec![Player::new(1, Coord::new(5, 5))];
    let game_state = GameState::new(map.clone(), players.clone(), 1000);

    // Check invariants BEFORE economy
    let violations_before = check_invariants(&game_state);
    assert!(
        violations_before.is_empty(),
        "Invariants violated before economy: {:?}",
        violations_before
    );

    // Test apply_economy - must not panic
    let result = apply_economy(&mut map, 1, input.rng_seed);

    // Sanity checks on result
    let max_change = (pop as i64 + 1_000_000).min(i32::MAX as i64) as i32;
    assert!(
        result.population_change.abs() <= max_change,
        "Population change {} seems unreasonable",
        result.population_change
    );

    // Create GameState with updated map for invariant checking
    let game_state_after = GameState::new(map, players, 1000);

    // Check invariants AFTER economy - critical for catching bugs
    let violations_after = check_invariants(&game_state_after);
    assert!(
        violations_after.is_empty(),
        "Invariants violated after economy: {:?}",
        violations_after
    );
});
