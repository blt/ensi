#![no_main]

//! Full game turn fuzzer.
//!
//! This fuzz target exercises the complete game turn sequence:
//! 1. Parse and validate commands
//! 2. Apply commands (moves, attacks, conversions)
//! 3. Process combat
//! 4. Process economy
//! 5. Check eliminations
//!
//! This catches integration bugs that individual component fuzzers miss.

use arbitrary::Arbitrary;
use ensi::game::{
    apply_economy, check_invariants, process_attack, resolve_combat, Coord, GameState, Map,
    Player, Tile, TileType,
};
use libfuzzer_sys::fuzz_target;

/// A fuzzer-generated command.
#[derive(Arbitrary, Debug, Clone)]
enum FuzzCommand {
    /// Move army from one tile to adjacent tile.
    Move {
        from_x: u8,
        from_y: u8,
        to_x: u8,
        to_y: u8,
        count: u16,
    },
    /// Convert population to army.
    Convert { city_x: u8, city_y: u8, count: u16 },
}

/// Structured input for full game turn fuzzing.
#[derive(Arbitrary, Debug)]
struct GameTurnInput {
    /// Starting population for each player's capital.
    starting_pop: [u16; 2],
    /// Starting army for each player's capital.
    starting_army: [u16; 2],
    /// Commands for player 1.
    p1_commands: Vec<FuzzCommand>,
    /// Commands for player 2.
    p2_commands: Vec<FuzzCommand>,
    /// RNG seed for economy.
    rng_seed: u64,
    /// Number of turns to simulate.
    num_turns: u8,
}

fuzz_target!(|input: GameTurnInput| {
    // Cap values to avoid excessive runtime
    let num_turns = (input.num_turns % 10).max(1);
    let p1_commands: Vec<_> = input.p1_commands.into_iter().take(10).collect();
    let p2_commands: Vec<_> = input.p2_commands.into_iter().take(10).collect();

    // Create a small map
    let mut map = match Map::new(16, 16) {
        Some(m) => m,
        None => return,
    };

    // Place player 1's capital at (3, 3)
    let pop1 = (input.starting_pop[0] as u32).min(10000).max(10);
    let army1 = (input.starting_army[0] as u32).min(1000);
    let mut city1 = Tile::city(pop1);
    city1.owner = Some(1);
    city1.army = army1;
    map.set(Coord::new(3, 3), city1);

    // Give player 1 starting territory
    for (dx, dy) in &[(0, 1), (1, 0), (0, -1i16 as u16), (1, 1)] {
        let x = 3u16.wrapping_add(*dx);
        let y = 3u16.wrapping_add(*dy);
        if x < 16 && y < 16 {
            let mut t = Tile::desert();
            t.owner = Some(1);
            map.set(Coord::new(x, y), t);
        }
    }

    // Place player 2's capital at (12, 12)
    let pop2 = (input.starting_pop[1] as u32).min(10000).max(10);
    let army2 = (input.starting_army[1] as u32).min(1000);
    let mut city2 = Tile::city(pop2);
    city2.owner = Some(2);
    city2.army = army2;
    map.set(Coord::new(12, 12), city2);

    // Give player 2 starting territory
    for (dx, dy) in &[(0, 1), (1, 0), (0, -1i16 as u16), (1, 1)] {
        let x = 12u16.wrapping_add(*dx);
        let y = 12u16.wrapping_add(*dy);
        if x < 16 && y < 16 {
            let mut t = Tile::desert();
            t.owner = Some(2);
            map.set(Coord::new(x, y), t);
        }
    }

    // Create players
    let players = vec![
        Player::new(1, Coord::new(3, 3)),
        Player::new(2, Coord::new(12, 12)),
    ];

    let mut game_state = GameState::new(map, players, 1000);

    // Check invariants at start
    let violations = check_invariants(&game_state);
    assert!(
        violations.is_empty(),
        "Invariants violated at start: {:?}",
        violations
    );

    // Run multiple turns
    for turn in 0..num_turns {
        // Phase 1: Apply player 1 commands
        for cmd in &p1_commands {
            apply_command(&mut game_state, 1, cmd);
        }

        // Phase 2: Apply player 2 commands
        for cmd in &p2_commands {
            apply_command(&mut game_state, 2, cmd);
        }

        // Phase 3: Combat resolution
        resolve_combat(&mut game_state.map);

        // Phase 4: Economy
        let rng = input.rng_seed.wrapping_add(turn as u64);
        apply_economy(&mut game_state.map, 1, rng);
        apply_economy(&mut game_state.map, 2, rng.wrapping_add(1));

        // Phase 5: Check eliminations
        let all_stats = game_state.compute_all_player_stats();
        game_state.check_eliminations(&all_stats);

        // Verify invariants after each turn
        let violations = check_invariants(&game_state);
        assert!(
            violations.is_empty(),
            "Invariants violated after turn {}: {:?}",
            turn,
            violations
        );

        // Stop if game is over
        if game_state.is_game_over() {
            break;
        }

        game_state.advance_turn();
    }

    // Final invariant check
    let violations = check_invariants(&game_state);
    assert!(
        violations.is_empty(),
        "Invariants violated at end: {:?}",
        violations
    );

    // Verify some basic properties
    // Total army should be reasonable
    let total_army: u32 = game_state
        .map
        .iter()
        .map(|(_, t)| t.army)
        .sum();
    assert!(
        total_army < 100_000_000,
        "Total army {} is unreasonably large",
        total_army
    );
});

/// Apply a fuzzer-generated command to the game state.
fn apply_command(game_state: &mut GameState, player_id: u8, cmd: &FuzzCommand) {
    match cmd {
        FuzzCommand::Move {
            from_x,
            from_y,
            to_x,
            to_y,
            count,
        } => {
            let from = Coord::new(*from_x as u16 % 16, *from_y as u16 % 16);
            let to = Coord::new(*to_x as u16 % 16, *to_y as u16 % 16);

            // Validate move
            let can_move = game_state
                .map
                .get(from)
                .map_or(false, |t| {
                    t.owner == Some(player_id) && t.army >= *count as u32
                });

            // Check adjacency
            let (adjacent, adj_count) = from.adjacent(16, 16);
            let is_adjacent = adjacent[..adj_count as usize].contains(&to);

            // Check destination is passable
            let passable = game_state
                .map
                .get(to)
                .map_or(false, |t| t.tile_type.is_passable());

            if can_move && is_adjacent && passable && *count > 0 {
                process_attack(&mut game_state.map, from, to, *count as u32);
            }
        }
        FuzzCommand::Convert { city_x, city_y, count } => {
            let city = Coord::new(*city_x as u16 % 16, *city_y as u16 % 16);

            // Validate conversion
            if let Some(tile) = game_state.map.get_mut(city) {
                if tile.owner == Some(player_id)
                    && tile.tile_type == TileType::City
                    && tile.population >= *count as u32
                    && *count > 0
                {
                    tile.population -= *count as u32;
                    tile.army += *count as u32;
                }
            }
        }
    }
}
