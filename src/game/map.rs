//! Map and tile types.

use crate::game::PlayerId;

/// A coordinate on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    /// X coordinate (column).
    pub x: u16,
    /// Y coordinate (row).
    pub y: u16,
}

impl Coord {
    /// Create a new coordinate.
    #[must_use]
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    /// Get adjacent coordinates (up, down, left, right).
    ///
    /// Returns a fixed-size array and count to avoid heap allocation.
    /// The array contains valid coordinates in indices 0..count.
    #[must_use]
    #[inline]
    pub fn adjacent(&self, width: u16, height: u16) -> ([Coord; 4], u8) {
        let mut result = [Coord::new(0, 0); 4];
        let mut count = 0u8;

        if self.y > 0 {
            result[count as usize] = Coord::new(self.x, self.y - 1); // up
            count += 1;
        }
        if self.y + 1 < height {
            result[count as usize] = Coord::new(self.x, self.y + 1); // down
            count += 1;
        }
        if self.x > 0 {
            result[count as usize] = Coord::new(self.x - 1, self.y); // left
            count += 1;
        }
        if self.x + 1 < width {
            result[count as usize] = Coord::new(self.x + 1, self.y); // right
            count += 1;
        }

        (result, count)
    }
}

/// Type of terrain on a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TileType {
    /// City tile - has population, produces food.
    City = 0,
    /// Desert tile - can be garrisoned, no food production.
    Desert = 1,
    /// Mountain tile - impassable.
    Mountain = 2,
}

impl TileType {
    /// Check if this tile type can be traversed by armies.
    #[must_use]
    pub const fn is_passable(self) -> bool {
        !matches!(self, TileType::Mountain)
    }

    /// Check if this tile type can have population.
    #[must_use]
    pub const fn can_have_population(self) -> bool {
        matches!(self, TileType::City)
    }
}

/// A single tile on the map.
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    /// Type of terrain.
    pub tile_type: TileType,
    /// Owner of this tile (None = neutral/unowned).
    pub owner: Option<PlayerId>,
    /// Army count stationed on this tile.
    pub army: u32,
    /// Population count (only meaningful for City tiles).
    pub population: u32,
}

impl Tile {
    /// Create a new tile with the given type.
    #[must_use]
    pub const fn new(tile_type: TileType) -> Self {
        Self {
            tile_type,
            owner: None,
            army: 0,
            population: 0,
        }
    }

    /// Create a new city tile with initial population.
    #[must_use]
    pub const fn city(population: u32) -> Self {
        Self {
            tile_type: TileType::City,
            owner: None,
            army: 0,
            population,
        }
    }

    /// Create a new desert tile.
    #[must_use]
    pub const fn desert() -> Self {
        Self::new(TileType::Desert)
    }

    /// Create a new mountain tile.
    #[must_use]
    pub const fn mountain() -> Self {
        Self::new(TileType::Mountain)
    }
}

/// The game map.
#[derive(Debug, Clone)]
pub struct Map {
    /// Width of the map in tiles.
    width: u16,
    /// Height of the map in tiles.
    height: u16,
    /// Tiles stored in row-major order.
    tiles: Vec<Tile>,
}

impl Map {
    /// Create a new map filled with desert tiles.
    ///
    /// # Errors
    ///
    /// Returns `None` if width or height is zero.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let size = usize::from(width) * usize::from(height);
        let tiles = vec![Tile::desert(); size];

        Some(Self {
            width,
            height,
            tiles,
        })
    }

    /// Get the width of the map.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Get the height of the map.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Get a reference to the raw tiles slice for efficient iteration.
    ///
    /// Use this when you don't need coordinates, just the tiles in row-major order.
    #[must_use]
    #[inline]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// Get a mutable reference to the raw tiles slice for efficient iteration.
    ///
    /// Use this when you need direct index-based mutation without coord overhead.
    #[must_use]
    #[inline]
    pub fn tiles_mut(&mut self) -> &mut [Tile] {
        &mut self.tiles
    }

    /// Check if a coordinate is within the map bounds.
    #[must_use]
    pub const fn in_bounds(&self, coord: Coord) -> bool {
        coord.x < self.width && coord.y < self.height
    }

    /// Convert a coordinate to an index into the tiles array.
    #[must_use]
    fn coord_to_index(&self, coord: Coord) -> Option<usize> {
        if self.in_bounds(coord) {
            Some(usize::from(coord.y) * usize::from(self.width) + usize::from(coord.x))
        } else {
            None
        }
    }

    /// Get a reference to the tile at the given coordinate.
    #[must_use]
    pub fn get(&self, coord: Coord) -> Option<&Tile> {
        self.coord_to_index(coord).map(|idx| &self.tiles[idx])
    }

    /// Get a mutable reference to the tile at the given coordinate.
    #[must_use]
    pub fn get_mut(&mut self, coord: Coord) -> Option<&mut Tile> {
        self.coord_to_index(coord).map(|idx| &mut self.tiles[idx])
    }

    /// Set the tile at the given coordinate.
    ///
    /// Returns `false` if the coordinate is out of bounds.
    pub fn set(&mut self, coord: Coord, tile: Tile) -> bool {
        if let Some(idx) = self.coord_to_index(coord) {
            self.tiles[idx] = tile;
            true
        } else {
            false
        }
    }

    /// Iterate over all coordinates and tiles.
    pub fn iter(&self) -> impl Iterator<Item = (Coord, &Tile)> {
        self.tiles.iter().enumerate().map(|(idx, tile)| {
            let x = (idx % usize::from(self.width)) as u16;
            let y = (idx / usize::from(self.width)) as u16;
            (Coord::new(x, y), tile)
        })
    }

    /// Iterate over all coordinates and mutable tiles.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Coord, &mut Tile)> {
        let width = self.width;
        self.tiles.iter_mut().enumerate().map(move |(idx, tile)| {
            let x = (idx % usize::from(width)) as u16;
            let y = (idx / usize::from(width)) as u16;
            (Coord::new(x, y), tile)
        })
    }

    /// Get all tiles owned by a specific player.
    pub fn tiles_owned_by(&self, player: PlayerId) -> impl Iterator<Item = (Coord, &Tile)> {
        self.iter().filter(move |(_, tile)| tile.owner == Some(player))
    }

    /// Count cities owned by a player.
    #[must_use]
    pub fn count_cities(&self, player: PlayerId) -> u32 {
        self.tiles_owned_by(player)
            .filter(|(_, tile)| tile.tile_type == TileType::City)
            .count() as u32
    }

    /// Count total territory (non-mountain tiles) owned by a player.
    #[must_use]
    pub fn count_territory(&self, player: PlayerId) -> u32 {
        self.tiles_owned_by(player)
            .filter(|(_, tile)| tile.tile_type != TileType::Mountain)
            .count() as u32
    }

    /// Sum total population in all cities owned by a player.
    #[must_use]
    pub fn total_population(&self, player: PlayerId) -> u32 {
        self.tiles_owned_by(player)
            .filter(|(_, tile)| tile.tile_type == TileType::City)
            .map(|(_, tile)| tile.population)
            .sum()
    }

    /// Sum total army across all tiles owned by a player.
    #[must_use]
    pub fn total_army(&self, player: PlayerId) -> u32 {
        self.tiles_owned_by(player)
            .map(|(_, tile)| tile.army)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coord_adjacent() {
        let coord = Coord::new(5, 5);
        let (adj, count) = coord.adjacent(10, 10);
        let adj_slice = &adj[..count as usize];
        assert_eq!(count, 4);
        assert!(adj_slice.contains(&Coord::new(5, 4))); // up
        assert!(adj_slice.contains(&Coord::new(5, 6))); // down
        assert!(adj_slice.contains(&Coord::new(4, 5))); // left
        assert!(adj_slice.contains(&Coord::new(6, 5))); // right
    }

    #[test]
    fn test_coord_adjacent_corner() {
        let coord = Coord::new(0, 0);
        let (adj, count) = coord.adjacent(10, 10);
        let adj_slice = &adj[..count as usize];
        assert_eq!(count, 2);
        assert!(adj_slice.contains(&Coord::new(0, 1))); // down
        assert!(adj_slice.contains(&Coord::new(1, 0))); // right
    }

    #[test]
    fn test_map_creation() {
        let map = Map::new(10, 10).unwrap();
        assert_eq!(map.width(), 10);
        assert_eq!(map.height(), 10);
    }

    #[test]
    fn test_map_zero_size() {
        assert!(Map::new(0, 10).is_none());
        assert!(Map::new(10, 0).is_none());
    }

    #[test]
    fn test_map_get_set() {
        let mut map = Map::new(10, 10).unwrap();
        let coord = Coord::new(5, 5);

        // Initially desert
        assert_eq!(map.get(coord).unwrap().tile_type, TileType::Desert);

        // Set to city
        map.set(coord, Tile::city(100));
        let tile = map.get(coord).unwrap();
        assert_eq!(tile.tile_type, TileType::City);
        assert_eq!(tile.population, 100);
    }

    #[test]
    fn test_map_bounds() {
        let map = Map::new(10, 10).unwrap();
        assert!(map.in_bounds(Coord::new(0, 0)));
        assert!(map.in_bounds(Coord::new(9, 9)));
        assert!(!map.in_bounds(Coord::new(10, 0)));
        assert!(!map.in_bounds(Coord::new(0, 10)));
    }

    #[test]
    fn test_tile_type_passable() {
        assert!(TileType::City.is_passable());
        assert!(TileType::Desert.is_passable());
        assert!(!TileType::Mountain.is_passable());
    }
}
