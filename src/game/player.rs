//! Player state management.

use std::collections::HashSet;

use crate::game::Coord;

/// Unique identifier for a player.
pub type PlayerId = u8;

/// State for a single player.
#[derive(Debug, Clone)]
pub struct Player {
    /// Unique identifier for this player.
    pub id: PlayerId,
    /// Location of the player's capital city.
    pub capital: Coord,
    /// Whether the player is still alive.
    pub alive: bool,
    /// Set of coordinates this player has discovered (for fog of war).
    pub discovered: HashSet<Coord>,
}

impl Player {
    /// Create a new player with the given ID and capital location.
    #[must_use]
    pub fn new(id: PlayerId, capital: Coord) -> Self {
        let mut discovered = HashSet::new();
        discovered.insert(capital);

        Self {
            id,
            capital,
            alive: true,
            discovered,
        }
    }

    /// Mark a coordinate as discovered by this player.
    pub fn discover(&mut self, coord: Coord) {
        self.discovered.insert(coord);
    }

    /// Check if this player has discovered the given coordinate.
    #[must_use]
    pub fn has_discovered(&self, coord: Coord) -> bool {
        self.discovered.contains(&coord)
    }

    /// Eliminate this player.
    pub fn eliminate(&mut self) {
        self.alive = false;
    }

    /// Move the capital to a new city.
    ///
    /// This should only be called after verifying the new location is a valid city
    /// owned by this player with larger population than the current capital.
    pub fn move_capital(&mut self, new_capital: Coord) {
        self.capital = new_capital;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_creation() {
        let player = Player::new(1, Coord::new(5, 5));
        assert_eq!(player.id, 1);
        assert_eq!(player.capital, Coord::new(5, 5));
        assert!(player.alive);
        assert!(player.has_discovered(Coord::new(5, 5)));
    }

    #[test]
    fn test_player_discover() {
        let mut player = Player::new(1, Coord::new(0, 0));
        assert!(!player.has_discovered(Coord::new(1, 1)));

        player.discover(Coord::new(1, 1));
        assert!(player.has_discovered(Coord::new(1, 1)));
    }

    #[test]
    fn test_player_eliminate() {
        let mut player = Player::new(1, Coord::new(0, 0));
        assert!(player.alive);

        player.eliminate();
        assert!(!player.alive);
    }

    #[test]
    fn test_player_move_capital() {
        let mut player = Player::new(1, Coord::new(0, 0));
        assert_eq!(player.capital, Coord::new(0, 0));

        player.move_capital(Coord::new(5, 5));
        assert_eq!(player.capital, Coord::new(5, 5));
    }
}
