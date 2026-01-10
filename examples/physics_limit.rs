//! Measure wasmtime physics limits

use std::time::Instant;
use wasmtime::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal WASM that just returns
    let wat = r#"
        (module
            (func (export "run_turn") (param i32) (result i32)
                i32.const 0
            )
        )
    "#;

    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let module = Module::new(&engine, wat)?;

    // Measure store + instance creation
    let start = Instant::now();
    let iterations = 10000;
    for _ in 0..iterations {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[])?;
        std::hint::black_box(&instance);
    }
    let instance_ns = start.elapsed().as_nanos() / iterations as u128;
    println!("Store + Instance creation: {}ns ({:.1}us)", instance_ns, instance_ns as f64 / 1000.0);

    // Measure WASM call with fuel
    let mut store = Store::new(&engine, ());
    store.set_fuel(u64::MAX)?;
    let instance = Instance::new(&mut store, &module, &[])?;
    let run_turn = instance.get_typed_func::<i32, i32>(&mut store, "run_turn")?;

    let start = Instant::now();
    let call_iterations = 1_000_000u128;
    for _ in 0..call_iterations {
        store.set_fuel(50000)?;
        let _ = run_turn.call(&mut store, 50000);
    }
    let call_ns = start.elapsed().as_nanos() / call_iterations;
    println!("WASM call (with fuel set): {}ns", call_ns);

    // Pure call without fuel reset
    store.set_fuel(u64::MAX)?;
    let start = Instant::now();
    for _ in 0..call_iterations {
        let _ = run_turn.call(&mut store, 50000);
    }
    let pure_call_ns = start.elapsed().as_nanos() / call_iterations;
    println!("WASM call (no fuel reset): {}ns", pure_call_ns);

    println!("\n=== PHYSICS LIMIT ===\n");

    // Native Rust overhead (from earlier measurement)
    let map_alloc_ns = 310u128;
    let rng_ops_ns = 1600u128;
    let tile_iter_ns = 300u128;

    // Wasmtime overhead
    let per_game_fixed = instance_ns + map_alloc_ns + rng_ops_ns;
    let per_player_turn = call_ns + tile_iter_ns;

    println!("Fixed per-game cost: {}ns ({:.2}us)", per_game_fixed, per_game_fixed as f64 / 1000.0);
    println!("  - Store+Instance: {}ns", instance_ns);
    println!("  - Map allocation: {}ns", map_alloc_ns);
    println!("  - RNG (map gen):  {}ns", rng_ops_ns);
    println!();
    println!("Per player-turn cost: {}ns ({:.2}us)", per_player_turn, per_player_turn as f64 / 1000.0);
    println!("  - WASM call+fuel: {}ns", call_ns);
    println!("  - Tile iteration: {}ns", tile_iter_ns);
    println!();

    println!("=== Maximum Theoretical Throughput ===\n");
    for (turns, players) in [(1, 2), (10, 2), (100, 2), (1000, 2), (1000, 4)] {
        let total_ns = per_game_fixed + (turns as u128 * players as u128 * per_player_turn);
        let games_per_sec = 1_000_000_000u128 / total_ns;
        println!("{:4} turns, {} players: {:>6.2}ms = {:>6} games/sec/core",
                 turns, players, total_ns as f64 / 1_000_000.0, games_per_sec);
    }

    println!("\n=== 16-Core Theoretical Maximum ===\n");
    for (turns, players) in [(1000, 2), (1000, 4)] {
        let total_ns = per_game_fixed + (turns as u128 * players as u128 * per_player_turn);
        let games_per_sec = 16 * 1_000_000_000u128 / total_ns;
        println!("{:4} turns, {} players: {:>6} games/sec (16 cores)", turns, players, games_per_sec);
    }

    Ok(())
}
