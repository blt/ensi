//! Extended fuzzing tests for tournament components.
//!
//! Run with: PROPTEST_CASES=100000 cargo test --release fuzz_tournament

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;

use ensi::tournament::generate_map;
use ensi::game::{Coord, TileType};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50000))]

    /// Fuzz map generation with random seeds - should never panic.
    #[test]
    fn fuzz_map_generation(seed in any::<u64>(), num_players in 2usize..=8) {
        let result = generate_map(seed, 64, 64, num_players);
        prop_assert!(result.is_ok(), "Map generation failed for seed={seed}, players={num_players}");

        let (map, players) = result.unwrap();

        // Verify invariants
        prop_assert_eq!(players.len(), num_players);
        prop_assert_eq!(map.width(), 64);
        prop_assert_eq!(map.height(), 64);

        // All players should have valid capitals
        for player in &players {
            let tile = map.get(player.capital);
            prop_assert!(tile.is_some(), "Player {} capital at invalid coord", player.id);
            let tile = tile.unwrap();
            prop_assert_eq!(tile.tile_type, TileType::City);
            prop_assert_eq!(tile.owner, Some(player.id));
            prop_assert!(tile.population > 0);
        }
    }

    /// Fuzz map generation with various sizes.
    #[test]
    fn fuzz_map_sizes(
        seed in any::<u64>(),
        width in 16u16..=128,
        height in 16u16..=128,
        num_players in 2usize..=8
    ) {
        let result = generate_map(seed, width, height, num_players);
        // May fail if map is too small for players, but should never panic
        if let Ok((map, players)) = result {
            prop_assert_eq!(map.width(), width);
            prop_assert_eq!(map.height(), height);
            prop_assert_eq!(players.len(), num_players);
        }
    }

    /// Fuzz coordinate adjacency calculations - should never panic.
    #[test]
    fn fuzz_coord_adjacent(w in 1u16..100, h in 1u16..100) {
        // Only test coordinates within valid bounds
        for x in 0..w.min(10) {
            for y in 0..h.min(10) {
                let coord = Coord::new(x, y);
                let (adjacent, count) = coord.adjacent(w, h);
                let adj_slice = &adjacent[..count as usize];

                // Should have 0-4 neighbors
                prop_assert!(count <= 4);

                // All neighbors should be within bounds
                for adj in adj_slice {
                    prop_assert!(adj.x < w, "adj.x={} >= w={}", adj.x, w);
                    prop_assert!(adj.y < h, "adj.y={} >= h={}", adj.y, h);
                }

                // Should have correct count based on position
                let expected_count = {
                    let mut c = 0u8;
                    if x > 0 { c += 1; }
                    if y > 0 { c += 1; }
                    if x + 1 < w { c += 1; }
                    if y + 1 < h { c += 1; }
                    c
                };
                prop_assert_eq!(count, expected_count);
            }
        }
    }
}

/// Determinism test - run same seed twice, verify identical results.
#[test]
fn test_map_generation_determinism_extended() {
    for seed in 0..1000 {
        let (map1, players1) = generate_map(seed, 64, 64, 4).unwrap();
        let (map2, players2) = generate_map(seed, 64, 64, 4).unwrap();

        // Verify maps are identical
        for y in 0..64 {
            for x in 0..64 {
                let coord = Coord::new(x, y);
                let t1 = map1.get(coord).unwrap();
                let t2 = map2.get(coord).unwrap();
                assert_eq!(t1.tile_type, t2.tile_type, "seed={seed}, coord=({x},{y})");
                assert_eq!(t1.owner, t2.owner, "seed={seed}, coord=({x},{y})");
                assert_eq!(t1.army, t2.army, "seed={seed}, coord=({x},{y})");
                assert_eq!(t1.population, t2.population, "seed={seed}, coord=({x},{y})");
            }
        }

        // Verify players are identical
        assert_eq!(players1.len(), players2.len());
        for (p1, p2) in players1.iter().zip(players2.iter()) {
            assert_eq!(p1.id, p2.id, "seed={seed}");
            assert_eq!(p1.capital, p2.capital, "seed={seed}");
        }
    }
}
