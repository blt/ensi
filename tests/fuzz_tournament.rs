//! Extended fuzzing tests for tournament components.
//!
//! Run with: PROPTEST_CASES=100000 cargo test --release fuzz_tournament

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;

use ensi::tournament::generate_map;
use ensi::game::{Coord, TileType};
use ensi::{Vm, NoSyscalls, MeteringConfig};

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

    /// Fuzz VM creation and basic operations - should never panic.
    #[test]
    fn fuzz_vm_creation(
        mem_size in 1024u32..=1024*1024,
        mem_base in any::<u32>()
    ) {
        let vm = Vm::new(mem_size, mem_base, NoSyscalls);
        prop_assert_eq!(vm.cpu.pc, 0);
        prop_assert_eq!(vm.total_executed(), 0);
    }

    /// Fuzz VM memory operations - should handle out-of-bounds gracefully.
    #[test]
    fn fuzz_vm_memory(
        addr in any::<u32>(),
        value in any::<u32>()
    ) {
        let mut vm = Vm::new(65536, 0, NoSyscalls);

        // Store should succeed or fail gracefully
        let store_result = vm.memory.store_u32(addr, value);

        // If store succeeded, load should return same value
        if store_result.is_ok() {
            let load_result = vm.memory.load_u32(addr);
            prop_assert!(load_result.is_ok());
            prop_assert_eq!(load_result.unwrap(), value);
        }
    }

    /// Fuzz VM step with random instructions - should never panic.
    #[test]
    fn fuzz_vm_step(inst in any::<u32>()) {
        let mut vm = Vm::new(65536, 0, NoSyscalls);
        let _ = vm.memory.store_u32(0, inst);

        // Step should return Ok or Trap, never panic
        let result = vm.step();
        prop_assert!(matches!(result, ensi::StepResult::Ok(_) | ensi::StepResult::Trap(_)));
    }

    /// Fuzz VM run_turn with random instructions - should never panic.
    #[test]
    fn fuzz_vm_run_turn(
        instructions in prop::collection::vec(any::<u32>(), 1..100),
        budget in 1u32..10000
    ) {
        let mut vm = Vm::new(65536, 0, NoSyscalls);

        // Fill memory with random instructions
        for (i, &inst) in instructions.iter().enumerate() {
            let addr = (i * 4) as u32;
            if addr < 65536 - 4 {
                let _ = vm.memory.store_u32(addr, inst);
            }
        }

        // Run turn - should return without panicking
        let result = vm.run_turn(budget);
        let is_valid = matches!(
            result,
            ensi::TurnResult::BudgetExhausted { .. } | ensi::TurnResult::Trap(_)
        );
        prop_assert!(is_valid, "Unexpected result from run_turn");
    }

    /// Fuzz metering config - should handle any values.
    #[test]
    fn fuzz_metering_config(
        base in any::<u32>(),
        memory in any::<u32>(),
        branch in any::<u32>(),
        multiply in any::<u32>(),
        divide in any::<u32>(),
        syscall in any::<u32>()
    ) {
        let config = MeteringConfig {
            base,
            memory,
            branch,
            multiply,
            divide,
            syscall,
        };

        let mut vm = Vm::with_metering(65536, 0, NoSyscalls, config);
        let _ = vm.memory.store_u32(0, 0x00000013); // NOP (addi x0, x0, 0)

        // Should work regardless of metering values
        let result = vm.step();
        prop_assert!(matches!(result, ensi::StepResult::Ok(_) | ensi::StepResult::Trap(_)));
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

/// Stress test VM with many instructions.
#[test]
fn test_vm_stress() {
    for _ in 0..100 {
        let mut vm = Vm::new(1024 * 1024, 0, NoSyscalls);

        // Fill with NOPs
        for i in 0..1000 {
            let _ = vm.memory.store_u32(i * 4, 0x00000013);
        }

        // Run a lot of turns
        for _ in 0..10 {
            let result = vm.run_turn(10000);
            match result {
                ensi::TurnResult::BudgetExhausted { .. } => {}
                ensi::TurnResult::Trap(_) => break,
            }
        }
    }
}
