//! Property-based tests for game mechanics.
//!
//! These tests verify properties of economy and combat systems.
//! Run with: cargo test --release prop_game

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;

use ensi::game::{apply_economy, calculate_food_balance, process_attack, Coord, Map, Tile};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))]

    /// Food balance calculation never panics for any valid inputs.
    #[test]
    fn prop_food_balance_no_panic(
        pop in 0u32..10_000_000,
        army in 0u32..10_000_000,
        territory in 1u32..200
    ) {
        let mut map = Map::new(20, 20).unwrap();

        // Place city with population
        let mut city = Tile::city(pop);
        city.owner = Some(1);
        city.army = army;
        map.set(Coord::new(5, 5), city);

        // Add territory
        for i in 0..territory.min(100) {
            let x = ((i + 1) % 20) as u16;
            let y = ((i + 1) / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map.set(Coord::new(x, y), desert);
            }
        }

        let balance = calculate_food_balance(&map, 1);

        // Production and consumption should be non-negative
        prop_assert!(balance.production >= 0);
        prop_assert!(balance.consumption >= 0);
        prop_assert!(balance.territory > 0);
    }

    /// Apply economy never panics and produces bounded results.
    #[test]
    fn prop_economy_bounded(
        pop in 1u32..1_000_000,
        army in 0u32..100_000,
        territory in 1u32..50,
        seed in any::<u64>()
    ) {
        let mut map = Map::new(20, 20).unwrap();

        let mut city = Tile::city(pop);
        city.owner = Some(1);
        city.army = army;
        map.set(Coord::new(5, 5), city);

        for i in 0..territory.min(20) {
            let x = ((i + 1) % 20) as u16;
            let y = ((i + 1) / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map.set(Coord::new(x, y), desert);
            }
        }

        let result = apply_economy(&mut map, 1, seed);

        // Population change should be bounded
        let max_change = (pop as i64 + 100_000).min(i32::MAX as i64) as i32;
        prop_assert!(
            result.population_change.abs() <= max_change,
            "Population change {} seems unreasonable",
            result.population_change
        );

        // Check tile after economy
        if let Some(tile) = map.get(Coord::new(5, 5)) {
            prop_assert!(
                tile.population < 100_000_000,
                "Population {} exceeds sanity bound",
                tile.population
            );
        }
    }

    /// Combat resolution is deterministic (same inputs produce same outputs).
    #[test]
    fn prop_combat_deterministic(
        att_army in 1u32..100_000,
        def_army in 1u32..100_000
    ) {
        // Run combat twice with identical setup
        let run_combat = |attacker: u32, defender: u32| -> (bool, u32, Option<u8>) {
            let mut map = Map::new(10, 10).unwrap();

            let mut att_tile = Tile::desert();
            att_tile.owner = Some(1);
            att_tile.army = attacker;
            map.set(Coord::new(2, 2), att_tile);

            let mut def_tile = Tile::desert();
            def_tile.owner = Some(2);
            def_tile.army = defender;
            map.set(Coord::new(3, 2), def_tile);

            let won = process_attack(&mut map, Coord::new(2, 2), Coord::new(3, 2), attacker);
            let result_tile = map.get(Coord::new(3, 2)).unwrap();
            (won, result_tile.army, result_tile.owner)
        };

        let (won1, army1, owner1) = run_combat(att_army, def_army);
        let (won2, army2, owner2) = run_combat(att_army, def_army);

        prop_assert_eq!(won1, won2, "Combat outcome should be deterministic");
        prop_assert_eq!(army1, army2, "Remaining army should be deterministic");
        prop_assert_eq!(owner1, owner2, "Tile ownership should be deterministic");
    }

    /// Combat produces valid results (winner is correct, army is bounded).
    /// Defender gets 25% combat advantage.
    #[test]
    fn prop_combat_valid_results(
        att_army in 1u32..1_000_000,
        def_army in 1u32..1_000_000
    ) {
        let mut map = Map::new(10, 10).unwrap();

        let mut att_tile = Tile::desert();
        att_tile.owner = Some(1);
        att_tile.army = att_army;
        map.set(Coord::new(2, 2), att_tile);

        let mut def_tile = Tile::desert();
        def_tile.owner = Some(2);
        def_tile.army = def_army;
        map.set(Coord::new(3, 2), def_tile);

        let attacker_won = process_attack(&mut map, Coord::new(2, 2), Coord::new(3, 2), att_army);
        let result_tile = map.get(Coord::new(3, 2)).unwrap();

        // Remaining army should be bounded by the larger initial army
        prop_assert!(
            result_tile.army <= att_army.max(def_army),
            "Remaining army {} exceeds max input {}",
            result_tile.army,
            att_army.max(def_army)
        );

        // Defender's 25% combat advantage
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let effective_defense = (f64::from(def_army) * 1.25) as u32;

        // Verify winner is correct (attacker must overcome effective defense)
        if att_army > effective_defense {
            prop_assert!(attacker_won, "Attacker should win when stronger than effective defense");
            prop_assert_eq!(result_tile.owner, Some(1));
            prop_assert_eq!(result_tile.army, att_army.saturating_sub(effective_defense));
        } else if att_army < effective_defense {
            prop_assert!(!attacker_won, "Defender should win with effective bonus");
            prop_assert_eq!(result_tile.owner, Some(2));
            // Defender loses raw attacker troops, not effective
            prop_assert_eq!(result_tile.army, def_army.saturating_sub(att_army));
        } else {
            // Tie at effective level - defender wins
            prop_assert!(!attacker_won, "Tie goes to defender");
            prop_assert_eq!(result_tile.owner, Some(2));
            prop_assert_eq!(result_tile.army, def_army.saturating_sub(att_army));
        }
    }

    /// Economy equilibrium test: with enough territory, population should stabilize.
    /// This tests the diminishing returns property.
    #[test]
    fn prop_economy_equilibrium_direction(territory in 10u32..100) {
        // With 50× territory equilibrium:
        // - Small pop on large territory should grow (positive balance)
        // - Large pop on small territory should shrink (negative balance)

        let equilibrium_pop = territory * 50;

        // Test underpopulated scenario
        {
            let mut map = Map::new(20, 20).unwrap();
            let small_pop = equilibrium_pop / 10;

            let mut city = Tile::city(small_pop);
            city.owner = Some(1);
            map.set(Coord::new(5, 5), city);

            for i in 0..territory.min(100) {
                let x = ((i + 1) % 20) as u16;
                let y = ((i + 1) / 20) as u16;
                if (x, y) != (5, 5) {
                    let mut desert = Tile::desert();
                    desert.owner = Some(1);
                    map.set(Coord::new(x, y), desert);
                }
            }

            let balance = calculate_food_balance(&map, 1);

            // Small population on large territory should have surplus
            prop_assert!(
                balance.balance > 0,
                "Small pop {} on {} tiles should have surplus, got {}",
                small_pop, territory, balance.balance
            );
        }

        // Test overpopulated scenario
        {
            let mut map = Map::new(20, 20).unwrap();
            let large_pop = equilibrium_pop * 5;

            let mut city = Tile::city(large_pop);
            city.owner = Some(1);
            map.set(Coord::new(5, 5), city);

            for i in 0..territory.min(100) {
                let x = ((i + 1) % 20) as u16;
                let y = ((i + 1) / 20) as u16;
                if (x, y) != (5, 5) {
                    let mut desert = Tile::desert();
                    desert.owner = Some(1);
                    map.set(Coord::new(x, y), desert);
                }
            }

            let balance = calculate_food_balance(&map, 1);

            // Large population on territory should have deficit
            prop_assert!(
                balance.balance < 0,
                "Large pop {} on {} tiles should have deficit, got {}",
                large_pop, territory, balance.balance
            );
        }
    }

    /// Production is monotonic in population (more workers = more production).
    #[test]
    fn prop_production_monotonic_in_population(
        territory in 10u32..100,
        pop1 in 1u32..100_000,
        pop_delta in 1u32..10_000
    ) {
        let pop2 = pop1.saturating_add(pop_delta);

        let mut map1 = Map::new(20, 20).unwrap();
        let mut map2 = Map::new(20, 20).unwrap();

        // Set up identical territory
        for i in 0..territory.min(100) {
            let x = (i % 20) as u16;
            let y = (i / 20) as u16;
            let mut desert1 = Tile::desert();
            desert1.owner = Some(1);
            map1.set(Coord::new(x, y), desert1);
            let mut desert2 = Tile::desert();
            desert2.owner = Some(1);
            map2.set(Coord::new(x, y), desert2);
        }

        // Add cities with different populations
        let mut city1 = Tile::city(pop1);
        city1.owner = Some(1);
        map1.set(Coord::new(5, 5), city1);

        let mut city2 = Tile::city(pop2);
        city2.owner = Some(1);
        map2.set(Coord::new(5, 5), city2);

        let balance1 = calculate_food_balance(&map1, 1);
        let balance2 = calculate_food_balance(&map2, 1);

        // More population should produce at least as much (monotonic)
        prop_assert!(
            balance2.production >= balance1.production,
            "Production should be monotonic: pop {} -> {}, prod {} -> {}",
            pop1, pop2, balance1.production, balance2.production
        );
    }

    /// Production is monotonic in territory (more land = more production).
    #[test]
    fn prop_production_monotonic_in_territory(
        pop in 100u32..10_000,
        territory1 in 5u32..50,
        territory_delta in 1u32..20
    ) {
        let territory2 = territory1.saturating_add(territory_delta).min(100);

        let mut map1 = Map::new(20, 20).unwrap();
        let mut map2 = Map::new(20, 20).unwrap();

        // City with same population on both
        let mut city1 = Tile::city(pop);
        city1.owner = Some(1);
        map1.set(Coord::new(5, 5), city1);

        let mut city2 = Tile::city(pop);
        city2.owner = Some(1);
        map2.set(Coord::new(5, 5), city2);

        // Different territory sizes
        for i in 0..territory1 {
            let x = ((i + 1) % 20) as u16;
            let y = ((i + 1) / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map1.set(Coord::new(x, y), desert);
            }
        }

        for i in 0..territory2 {
            let x = ((i + 1) % 20) as u16;
            let y = ((i + 1) / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map2.set(Coord::new(x, y), desert);
            }
        }

        let balance1 = calculate_food_balance(&map1, 1);
        let balance2 = calculate_food_balance(&map2, 1);

        // More territory should produce at least as much
        prop_assert!(
            balance2.production >= balance1.production,
            "Production should be monotonic in territory: {} -> {}, prod {} -> {}",
            territory1, territory2, balance1.production, balance2.production
        );
    }

    /// Consumption is monotonic (more pop/army = more consumption).
    #[test]
    fn prop_consumption_monotonic(
        pop in 1u32..100_000,
        army in 0u32..100_000,
        pop_delta in 0u32..1000,
        army_delta in 0u32..1000
    ) {
        let pop2 = pop.saturating_add(pop_delta);
        let army2 = army.saturating_add(army_delta);

        let mut map1 = Map::new(10, 10).unwrap();
        let mut map2 = Map::new(10, 10).unwrap();

        let mut city1 = Tile::city(pop);
        city1.owner = Some(1);
        city1.army = army;
        map1.set(Coord::new(5, 5), city1);

        let mut city2 = Tile::city(pop2);
        city2.owner = Some(1);
        city2.army = army2;
        map2.set(Coord::new(5, 5), city2);

        let balance1 = calculate_food_balance(&map1, 1);
        let balance2 = calculate_food_balance(&map2, 1);

        // More pop+army should consume at least as much
        prop_assert!(
            balance2.consumption >= balance1.consumption,
            "Consumption should be monotonic: ({}, {}) -> ({}, {}), cons {} -> {}",
            pop, army, pop2, army2, balance1.consumption, balance2.consumption
        );
    }

    /// Army conservation in combat: total armies never increase.
    #[test]
    fn prop_combat_army_conservation(
        att_army in 1u32..100_000,
        def_army in 1u32..100_000
    ) {
        let mut map = Map::new(10, 10).unwrap();

        let mut att_tile = Tile::desert();
        att_tile.owner = Some(1);
        att_tile.army = att_army;
        map.set(Coord::new(2, 2), att_tile);

        let mut def_tile = Tile::desert();
        def_tile.owner = Some(2);
        def_tile.army = def_army;
        map.set(Coord::new(3, 2), def_tile);

        let initial_total = att_army.saturating_add(def_army);

        let _ = process_attack(&mut map, Coord::new(2, 2), Coord::new(3, 2), att_army);

        let att_after = map.get(Coord::new(2, 2)).unwrap();
        let def_after = map.get(Coord::new(3, 2)).unwrap();
        let final_total = att_after.army.saturating_add(def_after.army);

        // Army conservation: total should never increase
        prop_assert!(
            final_total <= initial_total,
            "Army conservation violated: {} -> {}",
            initial_total, final_total
        );
    }

    /// Multi-turn economy convergence: population approaches equilibrium.
    #[test]
    fn prop_economy_converges(
        territory in 20u32..80,
        initial_pop in 10u32..1000,
        seed in any::<u64>()
    ) {
        let mut map = Map::new(20, 20).unwrap();

        let mut city = Tile::city(initial_pop);
        city.owner = Some(1);
        map.set(Coord::new(5, 5), city);

        for i in 0..territory.min(100) {
            let x = ((i + 1) % 20) as u16;
            let y = ((i + 1) / 20) as u16;
            if (x, y) != (5, 5) {
                let mut desert = Tile::desert();
                desert.owner = Some(1);
                map.set(Coord::new(x, y), desert);
            }
        }

        // Run economy for 50 turns
        let mut populations = vec![initial_pop];
        for turn in 0..50u64 {
            let _ = apply_economy(&mut map, 1, seed.wrapping_add(turn));
            if let Some(tile) = map.get(Coord::new(5, 5)) {
                populations.push(tile.population);
            }
        }

        // Verify convergence: variance should decrease over time
        let first_half: Vec<_> = populations.iter().take(25).collect();
        let second_half: Vec<_> = populations.iter().skip(25).collect();

        let variance = |pops: &[&u32]| -> f64 {
            if pops.is_empty() { return 0.0; }
            let mean = pops.iter().map(|&&x| x as f64).sum::<f64>() / pops.len() as f64;
            pops.iter().map(|&&x| (x as f64 - mean).powi(2)).sum::<f64>() / pops.len() as f64
        };

        let var1 = variance(&first_half);
        let var2 = variance(&second_half);

        // Second half should have lower or similar variance (converging)
        // Allow some tolerance for oscillation around equilibrium
        prop_assert!(
            var2 <= var1 * 2.0 + 1000.0,
            "Economy should converge: var1={}, var2={}, pops={:?}",
            var1, var2, populations.iter().take(10).collect::<Vec<_>>()
        );
    }
}
