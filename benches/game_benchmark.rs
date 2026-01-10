//! Benchmarks for running complete games.
//!
//! This benchmarks the full tournament game loop - the hot path we're optimizing.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ensi::tournament::{PlayerProgram, TournamentConfig, run_game};

/// Load bot WASM from the bots directory.
fn load_bot(name: &str) -> PlayerProgram {
    let path = format!("bots/{name}");
    let bytes = std::fs::read(&path).expect(&format!("Failed to load {path}"));
    PlayerProgram::new(bytes)
}

fn bench_single_game(c: &mut Criterion) {
    // Load two bots
    let balanced = load_bot("balanced.wasm");
    let economic = load_bot("economic.wasm");
    let programs = vec![balanced, economic];
    let config = TournamentConfig::default();

    c.bench_function("single_game_2p", |b| {
        b.iter(|| {
            let result = run_game(black_box(42), black_box(&programs), black_box(&config));
            black_box(result)
        });
    });
}

fn bench_single_game_4p(c: &mut Criterion) {
    // Load four bots for more realistic tournament scenario
    let balanced = load_bot("balanced.wasm");
    let economic = load_bot("economic.wasm");
    let aggressive = load_bot("aggressive.wasm");
    let explorer = load_bot("explorer.wasm");
    let programs = vec![balanced, economic, aggressive, explorer];
    let config = TournamentConfig::default();

    c.bench_function("single_game_4p", |b| {
        b.iter(|| {
            let result = run_game(black_box(42), black_box(&programs), black_box(&config));
            black_box(result)
        });
    });
}

fn bench_game_batch(c: &mut Criterion) {
    // Benchmark running 10 games sequentially (without parallel overhead)
    let balanced = load_bot("balanced.wasm");
    let economic = load_bot("economic.wasm");
    let programs = vec![balanced, economic];
    let config = TournamentConfig::default();

    c.bench_function("10_games_sequential", |b| {
        b.iter(|| {
            for seed in 0..10u64 {
                let result = run_game(black_box(seed), black_box(&programs), black_box(&config));
                let _ = black_box(result);
            }
        });
    });
}

fn bench_short_game(c: &mut Criterion) {
    // Benchmark with shorter fuel budget to see if WASM is the bottleneck
    let balanced = load_bot("balanced.wasm");
    let economic = load_bot("economic.wasm");
    let programs = vec![balanced, economic];

    // Short game: 100 turns, 10k fuel per turn
    let mut config = TournamentConfig::default();
    config.max_turns = 100;
    config.fuel_budget = 10_000;

    c.bench_function("short_game_2p", |b| {
        b.iter(|| {
            let result = run_game(black_box(42), black_box(&programs), black_box(&config));
            black_box(result)
        });
    });
}

criterion_group!(benches, bench_single_game, bench_single_game_4p, bench_game_batch, bench_short_game);
criterion_main!(benches);
