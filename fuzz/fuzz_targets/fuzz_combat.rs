#![no_main]

use arbitrary::Arbitrary;
use ensi::game::{check_invariants, process_attack, Coord, GameState, Map, Player, Tile};
use libfuzzer_sys::fuzz_target;

/// Structured input for combat fuzzing.
#[derive(Arbitrary, Debug)]
struct CombatInput {
    /// Attacker army size.
    attacker_army: u32,
    /// Defender army size.
    defender_army: u32,
    /// Attacker population (if city).
    attacker_pop: u32,
    /// Defender population (if city).
    defender_pop: u32,
    /// Whether attacker tile is a city.
    attacker_is_city: bool,
    /// Whether defender tile is a city.
    defender_is_city: bool,
    /// Number of attacking units to send.
    attack_count: u32,
    /// Attacker x coordinate (for boundary testing).
    attacker_x: u8,
    /// Attacker y coordinate (for boundary testing).
    attacker_y: u8,
}

fuzz_target!(|input: CombatInput| {
    // Cap inputs to avoid OOM
    let att_army = input.attacker_army.min(1_000_000);
    let def_army = input.defender_army.min(1_000_000);
    let att_pop = input.attacker_pop.min(100_000);
    let def_pop = input.defender_pop.min(100_000);
    let attack_count = input.attack_count.min(att_army);

    if attack_count == 0 {
        return; // Nothing to attack with
    }

    // Use coordinates within bounds, test boundary conditions
    let att_x = (input.attacker_x % 9) as u16;
    let att_y = (input.attacker_y % 9) as u16;
    let def_x = att_x + 1; // Adjacent tile
    let def_y = att_y;

    // Create map
    let mut map = match Map::new(10, 10) {
        Some(m) => m,
        None => return,
    };

    // Set up attacker tile
    let mut attacker_tile = if input.attacker_is_city {
        Tile::city(att_pop)
    } else {
        Tile::desert()
    };
    attacker_tile.owner = Some(1);
    attacker_tile.army = att_army;
    map.set(Coord::new(att_x, att_y), attacker_tile);

    // Set up defender tile
    let mut defender_tile = if input.defender_is_city {
        Tile::city(def_pop)
    } else {
        Tile::desert()
    };
    defender_tile.owner = Some(2);
    defender_tile.army = def_army;
    map.set(Coord::new(def_x, def_y), defender_tile);

    // Record initial state for conservation checks
    let initial_total_army = att_army.saturating_add(def_army);

    // Create GameState for invariant checking
    let players = vec![
        Player::new(1, Coord::new(att_x, att_y)),
        Player::new(2, Coord::new(def_x, def_y)),
    ];
    let game_state_before = GameState::new(map.clone(), players.clone(), 1000);

    // Check invariants BEFORE combat
    let violations_before = check_invariants(&game_state_before);
    assert!(
        violations_before.is_empty(),
        "Invariants violated before combat: {:?}",
        violations_before
    );

    // Execute attack - must not panic
    let attacker_won = process_attack(
        &mut map,
        Coord::new(att_x, att_y),
        Coord::new(def_x, def_y),
        attack_count,
    );

    // Check map state is consistent after attack
    let att_tile = map.get(Coord::new(att_x, att_y)).unwrap();
    let def_tile = map.get(Coord::new(def_x, def_y)).unwrap();

    // Army conservation: total armies should not increase
    let final_total_army = att_tile.army.saturating_add(def_tile.army);
    assert!(
        final_total_army <= initial_total_army,
        "Army conservation violated: {} -> {}",
        initial_total_army,
        final_total_army
    );

    // Attacker should have fewer or same troops (sent some)
    assert!(
        att_tile.army <= att_army,
        "Attacker army increased after attack"
    );

    // Winner determination must be consistent
    if attacker_won {
        // Defender tile should now be owned by attacker
        assert_eq!(
            def_tile.owner,
            Some(1),
            "Defender tile should be owned by attacker after winning"
        );
        // Remaining army should be attack_count - def_army (if attack won)
        if attack_count > def_army {
            assert_eq!(
                def_tile.army,
                attack_count - def_army,
                "Remaining army should be attack - defense"
            );
        }
    } else {
        // Defender tile should still be owned by defender (or possibly neutral)
        assert_ne!(
            def_tile.owner,
            Some(1),
            "Defender tile should not be owned by attacker after losing"
        );
    }

    // Create GameState for invariant checking after combat
    let game_state_after = GameState::new(map, players, 1000);

    // Check invariants AFTER combat
    let violations_after = check_invariants(&game_state_after);
    assert!(
        violations_after.is_empty(),
        "Invariants violated after combat: {:?}",
        violations_after
    );
});
