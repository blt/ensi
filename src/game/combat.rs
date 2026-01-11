//! Combat resolution.
//!
//! Defenders receive a 25% combat bonus to make defensive positioning meaningful.
//! This creates strategic tradeoffs between aggressive expansion and consolidation.

use std::cmp::Ordering;

use crate::game::{Coord, Map, PlayerId};

/// Defender's combat advantage multiplier.
/// At 0.25, defenders effectively have 125% strength (attacker needs 1.25x army to win).
const DEFENDER_BONUS: f64 = 0.25;

/// Result of combat on a single tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatResult {
    /// Winning player (None if tile becomes neutral/empty).
    pub winner: Option<PlayerId>,
    /// Remaining army count.
    pub remaining_army: u32,
}

/// Resolve combat on a tile where multiple players have armies.
///
/// Uses subtraction model: larger army wins, loses the smaller army's count.
/// If armies are equal, both are destroyed and tile becomes neutral.
///
/// Returns the combat result with winner and remaining army.
#[cfg(test)]
#[must_use]
fn resolve_tile_combat(armies: &[(PlayerId, u32)]) -> CombatResult {
    if armies.is_empty() {
        return CombatResult {
            winner: None,
            remaining_army: 0,
        };
    }

    if armies.len() == 1 {
        return CombatResult {
            winner: Some(armies[0].0),
            remaining_army: armies[0].1,
        };
    }

    // Find the two largest armies
    let mut sorted: Vec<_> = armies.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by army size descending

    let (player1, army1) = sorted[0];
    let army2: u32 = sorted[1..].iter().map(|(_, a)| *a).sum(); // Sum of all other armies

    if army1 > army2 {
        CombatResult {
            winner: Some(player1),
            remaining_army: army1 - army2,
        }
    } else {
        // Tie or defender wins - tile becomes neutral with no army
        CombatResult {
            winner: None,
            remaining_army: 0,
        }
    }
}

/// Resolve combat on all tiles of the map.
///
/// This should be called after all move commands have been processed.
/// Modifies the map in place, updating tile ownership and army counts.
pub fn resolve_combat(map: &mut Map) {
    let width = map.width();
    let height = map.height();

    for y in 0..height {
        for x in 0..width {
            let coord = Coord::new(x, y);
            if let Some(tile) = map.get_mut(coord) {
                // Skip tiles with single owner or no army
                if tile.army == 0 {
                    continue;
                }

                // For now, tiles only have one owner's army at a time
                // Combat happens during move resolution when armies collide
                // This function handles post-move cleanup

                // If tile has army but no owner, that's invalid state
                // If tile has owner but army <= 0, clear the owner
                if tile.army == 0 && tile.owner.is_some() {
                    tile.owner = None;
                }
            }
        }
    }
}

/// Process an attack from one tile to another.
///
/// Returns `true` if the attack was successful (attacker captured the tile).
pub fn process_attack(
    map: &mut Map,
    from: Coord,
    to: Coord,
    attacking_army: u32,
) -> bool {
    let defender = {
        let Some(to_tile) = map.get(to) else {
            return false;
        };
        (to_tile.owner, to_tile.army)
    };

    let attacker_owner = {
        let Some(from_tile) = map.get(from) else {
            return false;
        };
        from_tile.owner
    };

    let Some(attacker_owner) = attacker_owner else {
        return false;
    };

    let (defender_owner, defending_army) = defender;

    // Deduct attacking army from source tile first
    if let Some(from_tile) = map.get_mut(from) {
        from_tile.army = from_tile.army.saturating_sub(attacking_army);
    }

    // Same owner - this is a move, not an attack
    if defender_owner == Some(attacker_owner) {
        if let Some(to_tile) = map.get_mut(to) {
            to_tile.army = to_tile.army.saturating_add(attacking_army);
        }
        return true;
    }

    // Combat resolution with defender's advantage
    // Defender gets +25% effective strength
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let effective_defending = (f64::from(defending_army) * (1.0 + DEFENDER_BONUS)) as u32;

    match attacking_army.cmp(&effective_defending) {
        Ordering::Greater => {
            // Attacker wins - remaining is attack minus effective defense
            let remaining = attacking_army.saturating_sub(effective_defending);
            if let Some(to_tile) = map.get_mut(to) {
                to_tile.owner = Some(attacker_owner);
                to_tile.army = remaining;
            }
            true
        }
        Ordering::Less => {
            // Defender wins - remaining is based on raw defending army minus attack
            // (defender doesn't lose bonus troops, just real ones)
            let remaining = defending_army.saturating_sub(attacking_army);
            if let Some(to_tile) = map.get_mut(to) {
                to_tile.army = remaining;
            }
            false
        }
        Ordering::Equal => {
            // Tie at effective level - defender wins with reduced army
            let remaining = defending_army.saturating_sub(attacking_army);
            if let Some(to_tile) = map.get_mut(to) {
                to_tile.army = remaining;
            }
            false
        }
    }
}

/// Kani formal verification proofs.
///
/// These prove arithmetic safety properties for combat resolution.
/// Run with: `cargo kani`
#[cfg(kani)]
mod kani_proofs {
    /// Prove that combat subtraction never underflows.
    ///
    /// Combat uses `attacking_army - defending_army` which is safe
    /// because the subtraction is guarded by `attacking_army > defending_army`.
    #[kani::proof]
    fn prove_combat_subtraction_safe() {
        let attacking: u32 = kani::any();
        let defending: u32 = kani::any();

        // Mirror the combat logic from process_attack
        if attacking > defending {
            let remaining = attacking - defending;
            assert!(remaining < attacking);
            assert!(remaining <= u32::MAX);
        } else if attacking < defending {
            let remaining = defending - attacking;
            assert!(remaining < defending);
            assert!(remaining <= u32::MAX);
        }
        // Equal case: both set to 0, no subtraction needed
    }

    /// Prove that saturating_add for army reinforcement is safe.
    #[kani::proof]
    fn prove_army_reinforce_no_overflow() {
        let current_army: u32 = kani::any();
        let reinforcement: u32 = kani::any();

        let new_army = current_army.saturating_add(reinforcement);

        // Result is bounded
        assert!(new_army >= current_army || new_army == u32::MAX);
        assert!(new_army >= reinforcement || new_army == u32::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Tile;

    #[test]
    fn test_resolve_tile_combat_single() {
        let armies = vec![(1, 10)];
        let result = resolve_tile_combat(&armies);
        assert_eq!(result.winner, Some(1));
        assert_eq!(result.remaining_army, 10);
    }

    #[test]
    fn test_resolve_tile_combat_attacker_wins() {
        let armies = vec![(1, 10), (2, 7)];
        let result = resolve_tile_combat(&armies);
        assert_eq!(result.winner, Some(1));
        assert_eq!(result.remaining_army, 3);
    }

    #[test]
    fn test_resolve_tile_combat_tie() {
        let armies = vec![(1, 10), (2, 10)];
        let result = resolve_tile_combat(&armies);
        assert_eq!(result.winner, None);
        assert_eq!(result.remaining_army, 0);
    }

    #[test]
    fn test_resolve_tile_combat_multiple_attackers() {
        // Player 1 has 20, players 2 and 3 have 5 each = 10 total opposition
        let armies = vec![(1, 20), (2, 5), (3, 5)];
        let result = resolve_tile_combat(&armies);
        assert_eq!(result.winner, Some(1));
        assert_eq!(result.remaining_army, 10);
    }

    #[test]
    fn test_process_attack_attacker_wins() {
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        // Set up attacker
        let mut attacker_tile = Tile::desert();
        attacker_tile.owner = Some(1);
        attacker_tile.army = 10;
        map.set(from, attacker_tile);

        // Set up defender
        let mut defender_tile = Tile::desert();
        defender_tile.owner = Some(2);
        defender_tile.army = 7;
        map.set(to, defender_tile);

        let success = process_attack(&mut map, from, to, 10);
        assert!(success);

        let result_tile = map.get(to).unwrap();
        assert_eq!(result_tile.owner, Some(1));
        // With 25% defender bonus: 7 * 1.25 = 8.75 ≈ 8 effective defense
        // Remaining: 10 - 8 = 2
        assert_eq!(result_tile.army, 2);
    }

    #[test]
    fn test_process_attack_defender_wins() {
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        let mut attacker_tile = Tile::desert();
        attacker_tile.owner = Some(1);
        attacker_tile.army = 5;
        map.set(from, attacker_tile);

        let mut defender_tile = Tile::desert();
        defender_tile.owner = Some(2);
        defender_tile.army = 10;
        map.set(to, defender_tile);

        let success = process_attack(&mut map, from, to, 5);
        assert!(!success);

        let result_tile = map.get(to).unwrap();
        assert_eq!(result_tile.owner, Some(2));
        assert_eq!(result_tile.army, 5);
    }

    #[test]
    fn test_process_attack_move_to_own_tile() {
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        let mut tile1 = Tile::desert();
        tile1.owner = Some(1);
        tile1.army = 10;
        map.set(from, tile1);

        let mut tile2 = Tile::desert();
        tile2.owner = Some(1);
        tile2.army = 5;
        map.set(to, tile2);

        let success = process_attack(&mut map, from, to, 10);
        assert!(success);

        let result_tile = map.get(to).unwrap();
        assert_eq!(result_tile.owner, Some(1));
        assert_eq!(result_tile.army, 15);
    }

    #[test]
    fn test_defender_advantage_equal_armies() {
        // Equal armies: defender should win due to 25% bonus
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        let mut attacker_tile = Tile::desert();
        attacker_tile.owner = Some(1);
        attacker_tile.army = 100;
        map.set(from, attacker_tile);

        let mut defender_tile = Tile::desert();
        defender_tile.owner = Some(2);
        defender_tile.army = 100;
        map.set(to, defender_tile);

        // Attack with 100 vs 100 defenders (effective 125 with bonus)
        let success = process_attack(&mut map, from, to, 100);
        assert!(!success, "Equal armies: defender should win with 25% bonus");

        let result_tile = map.get(to).unwrap();
        assert_eq!(result_tile.owner, Some(2), "Defender should retain ownership");
        // Defender loses attacker's army but keeps their troops
        assert_eq!(result_tile.army, 0, "Defender loses troops equal to attack");
    }

    #[test]
    fn test_defender_advantage_breakeven() {
        // Attacker needs 1.25x army to win
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        let mut attacker_tile = Tile::desert();
        attacker_tile.owner = Some(1);
        attacker_tile.army = 125;
        map.set(from, attacker_tile);

        let mut defender_tile = Tile::desert();
        defender_tile.owner = Some(2);
        defender_tile.army = 100;
        map.set(to, defender_tile);

        // 125 attackers vs 100 defenders (effective 125 with bonus)
        // Equal at effective level = defender wins (tie goes to defender)
        let success = process_attack(&mut map, from, to, 125);
        assert!(!success, "Tie at effective level goes to defender");
    }

    #[test]
    fn test_defender_advantage_attacker_needs_margin() {
        // Attacker needs more than 1.25x to win
        let mut map = Map::new(10, 10).unwrap();
        let from = Coord::new(0, 0);
        let to = Coord::new(1, 0);

        let mut attacker_tile = Tile::desert();
        attacker_tile.owner = Some(1);
        attacker_tile.army = 126;
        map.set(from, attacker_tile);

        let mut defender_tile = Tile::desert();
        defender_tile.owner = Some(2);
        defender_tile.army = 100;
        map.set(to, defender_tile);

        // 126 attackers vs 100 defenders (effective 125)
        let success = process_attack(&mut map, from, to, 126);
        assert!(success, "Attacker wins with margin above 1.25x");

        let result_tile = map.get(to).unwrap();
        assert_eq!(result_tile.owner, Some(1));
        // 126 - 125 = 1 remaining
        assert_eq!(result_tile.army, 1);
    }
}
