//! Multi-turn integration tests for game mechanics.
//!
//! These tests verify that games run correctly over many turns without panicking
//! and that population naturally equilibrates (no runaway growth).
//!
//! Run with: cargo test --release game_integration

#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use ensi::tournament::{run_game, PlayerProgram, TournamentConfig};

/// Load a bot ELF from the bots directory.
fn load_bot(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/bots/{}.elf",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to load {}: {}", path, e))
}

#[test]
fn test_100_turn_game_no_panic() {
    let config = TournamentConfig {
        max_turns: 100,
        ..Default::default()
    };

    // Should complete without panic
    let programs = vec![
        PlayerProgram::new(load_bot("balanced")),
        PlayerProgram::new(load_bot("balanced")),
    ];

    let result = run_game(42, &programs, &config).expect("Game should complete");
    assert!(result.turns_played <= 100);
}

#[test]
fn test_500_turn_game_no_panic() {
    let config = TournamentConfig {
        max_turns: 500,
        ..Default::default()
    };

    let programs = vec![
        PlayerProgram::new(load_bot("economic")),
        PlayerProgram::new(load_bot("aggressive")),
    ];

    let result = run_game(12345, &programs, &config).expect("Game should complete");
    assert!(result.turns_played <= 500);
}

#[test]
fn test_multiple_seeds_no_panic() {
    let config = TournamentConfig {
        max_turns: 200,
        ..Default::default()
    };

    let programs = vec![
        PlayerProgram::new(load_bot("balanced")),
        PlayerProgram::new(load_bot("economic")),
    ];

    // Run games with different seeds
    for seed in 0..50 {
        let result = run_game(seed, &programs, &config);
        assert!(
            result.is_ok(),
            "Seed {} caused error: {:?}",
            seed,
            result.err()
        );
    }
}

#[test]
fn test_four_player_game() {
    let config = TournamentConfig {
        max_turns: 300,
        ..Default::default()
    };

    let programs = vec![
        PlayerProgram::new(load_bot("balanced")),
        PlayerProgram::new(load_bot("economic")),
        PlayerProgram::new(load_bot("aggressive")),
        PlayerProgram::new(load_bot("explorer")),
    ];

    let result = run_game(9999, &programs, &config).expect("4-player game should complete");
    assert!(result.turns_played <= 300);
}

#[test]
fn test_population_bounded_long_game() {
    // Run a long game and verify scores stay reasonable
    // With the Cobb-Douglas model, populations should naturally equilibrate
    let config = TournamentConfig {
        max_turns: 1000,
        ..Default::default()
    };

    let programs = vec![
        PlayerProgram::new(load_bot("economic")),
        PlayerProgram::new(load_bot("economic")),
    ];

    let result = run_game(42424, &programs, &config).expect("Long game should complete");

    // Final scores should be bounded
    // With 64×64 map (4096 tiles) and ~50× equilibrium, max pop ~ 200K
    // Score = pop * 1.0 + cities * 10 + territory * 0.5
    // Conservative bound: 500K score
    for (i, &score) in result.scores.iter().enumerate() {
        assert!(
            score < 500_000.0,
            "Player {} has unreasonably high score {} after {} turns",
            i + 1,
            score,
            result.turns_played
        );
    }
}

#[test]
fn test_different_map_sizes() {
    let seeds = [111, 222, 333];
    let sizes: [(u16, u16); 3] = [(32, 32), (64, 64), (100, 100)];

    for (seed, (width, height)) in seeds.into_iter().zip(sizes.into_iter()) {
        let config = TournamentConfig {
            max_turns: 100,
            map_width: width,
            map_height: height,
            ..Default::default()
        };

        let programs = vec![
            PlayerProgram::new(load_bot("balanced")),
            PlayerProgram::new(load_bot("balanced")),
        ];

        let result = run_game(seed, &programs, &config);
        assert!(
            result.is_ok(),
            "Map size {}x{} failed: {:?}",
            width,
            height,
            result.err()
        );
    }
}

#[test]
fn test_determinism() {
    let config = TournamentConfig {
        max_turns: 100,
        ..Default::default()
    };

    let programs1 = vec![
        PlayerProgram::new(load_bot("balanced")),
        PlayerProgram::new(load_bot("economic")),
    ];

    let programs2 = vec![
        PlayerProgram::new(load_bot("balanced")),
        PlayerProgram::new(load_bot("economic")),
    ];

    let result1 = run_game(7777, &programs1, &config).unwrap();
    let result2 = run_game(7777, &programs2, &config).unwrap();

    // Same seed should produce identical results
    assert_eq!(
        result1.turns_played, result2.turns_played,
        "Turn count should be deterministic"
    );
    assert_eq!(result1.winner, result2.winner, "Winner should be deterministic");
    assert_eq!(result1.scores, result2.scores, "Scores should be deterministic");
}

#[test]
fn test_game_terminates() {
    // Games should always terminate within max_turns
    let config = TournamentConfig {
        max_turns: 50,
        ..Default::default()
    };

    for seed in 0..20 {
        let programs = vec![
            PlayerProgram::new(load_bot("balanced")),
            PlayerProgram::new(load_bot("aggressive")),
        ];

        let result = run_game(seed, &programs, &config).unwrap();
        assert!(
            result.turns_played <= config.max_turns,
            "Game exceeded max_turns for seed {}",
            seed
        );
    }
}
