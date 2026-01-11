/**
 * Ensi Bot SDK
 *
 * This header provides the interface for writing bots in C that compile to WASM.
 * Bots interact with the game through host-provided functions.
 *
 * Build command:
 *   clang --target=wasm32-unknown-unknown -O3 -nostdlib \
 *         -Wl,--no-entry -Wl,--export=run_turn \
 *         -o bot.wasm bot.c
 *
 * Or with wasi-sdk:
 *   clang --target=wasm32-wasi -O3 -o bot.wasm bot.c
 */

#ifndef ENSI_H
#define ENSI_H

#include <stdint.h>

/*============================================================================
 * Tile Type Constants
 *============================================================================*/

/** City tile - has population, produces food. */
#define TILE_CITY 0
/** Desert tile - passable, no special properties. */
#define TILE_DESERT 1
/** Mountain tile - impassable. */
#define TILE_MOUNTAIN 2
/** Fog of war - tile not visible. */
#define TILE_FOG 255

/*============================================================================
 * Query Functions (Imports from Host)
 *
 * These functions query the current game state.
 * They consume fuel but are generally cheap.
 *============================================================================*/

/**
 * Get the current turn number (0-indexed).
 * @return Current turn number.
 */
extern int ensi_get_turn(void);

/**
 * Get this bot's player ID (1-8).
 * @return Player ID.
 */
extern int ensi_get_player_id(void);

/**
 * Get this player's capital coordinates.
 * @return Packed coordinates (x << 16 | y), or -1 if no capital.
 */
extern int ensi_get_my_capital(void);

/**
 * Get tile information at coordinates.
 *
 * Returns a packed value:
 *   - bits 0-7:   tile_type (0=city, 1=desert, 2=mountain, 255=fog)
 *   - bits 8-15:  owner player_id (0=unowned, 255=fog)
 *   - bits 16-31: army count
 *
 * @param x X coordinate.
 * @param y Y coordinate.
 * @return Packed tile info.
 */
extern int ensi_get_tile(int x, int y);

/**
 * Get this player's food balance (production - consumption).
 * @return Food balance (can be negative).
 */
extern int ensi_get_my_food(void);

/**
 * Get this player's total population across all cities.
 * @return Total population.
 */
extern int ensi_get_my_population(void);

/**
 * Get this player's total army count across all tiles.
 * @return Total army.
 */
extern int ensi_get_my_army(void);

/**
 * Get the map width.
 * @return Map width in tiles.
 */
extern int ensi_get_map_width(void);

/**
 * Get the map height.
 * @return Map height in tiles.
 */
extern int ensi_get_map_height(void);

/*============================================================================
 * Action Functions (Imports from Host)
 *
 * These functions issue commands to the game engine.
 * Commands are validated and queued; invalid commands return 1.
 *============================================================================*/

/**
 * Move army from one tile to an adjacent tile.
 *
 * The source tile must be owned by this player and have at least `count` army.
 * The destination tile must be adjacent and passable (not a mountain).
 *
 * @param from_x Source X coordinate.
 * @param from_y Source Y coordinate.
 * @param to_x Destination X coordinate.
 * @param to_y Destination Y coordinate.
 * @param count Number of army units to move.
 * @return 0 on success, 1 on failure.
 */
extern int ensi_move(int from_x, int from_y, int to_x, int to_y, int count);

/**
 * Convert population to army in a city.
 *
 * The city must be owned by this player and have at least `count` population.
 *
 * @param city_x City X coordinate.
 * @param city_y City Y coordinate.
 * @param count Number of population to convert to army.
 * @return 0 on success, 1 on failure.
 */
extern int ensi_convert(int city_x, int city_y, int count);

/**
 * Move capital to a new city.
 *
 * The new capital must be owned by this player and have more population
 * than the current capital.
 *
 * @param city_x New capital X coordinate.
 * @param city_y New capital Y coordinate.
 * @return 0 on success, 1 on failure.
 */
extern int ensi_move_capital(int city_x, int city_y);

/**
 * Abandon a tile (relinquish ownership).
 *
 * The tile must be owned by this player and cannot be the capital.
 * Useful for reducing desert upkeep costs.
 *
 * @param x Tile X coordinate.
 * @param y Tile Y coordinate.
 * @return 0 on success, 1 on failure.
 */
extern int ensi_abandon(int x, int y);

/**
 * End turn early.
 *
 * Call this to stop execution and let other players take their turn.
 * Useful for saving fuel when no more actions are needed.
 */
extern void ensi_yield(void);

/*============================================================================
 * Push-Based Visibility Map (High Performance)
 *
 * The host pushes a visibility-masked tile map to WASM memory before each turn.
 * Reading from this map is MUCH faster than calling ensi_get_tile (~100x).
 *
 * Use ensi_tile_map_get() for bulk tile queries (e.g., scanning the map).
 * Use ensi_get_tile() for occasional queries or when you need compatibility.
 *============================================================================*/

/** Base address of the pushed tile map in linear memory. */
#define ENSI_TILE_MAP_BASE 0x10000

/** Header: magic "ENSI" (4 bytes) + width (2) + height (2) + turn (4) + player_id (2) + reserved (2) */
#define ENSI_TILE_MAP_HEADER_SIZE 16

/**
 * Get the width from the tile map header.
 * @return Map width in tiles.
 */
static inline int ensi_tile_map_width(void) {
    return *((unsigned short*)(ENSI_TILE_MAP_BASE + 4));
}

/**
 * Get the height from the tile map header.
 * @return Map height in tiles.
 */
static inline int ensi_tile_map_height(void) {
    return *((unsigned short*)(ENSI_TILE_MAP_BASE + 6));
}

/**
 * Get tile information from the pushed visibility map.
 * This is ~100x faster than ensi_get_tile() syscall.
 *
 * @param x X coordinate.
 * @param y Y coordinate.
 * @return Packed tile info (same format as ensi_get_tile).
 */
static inline int ensi_tile_map_get(int x, int y) {
    int width = ensi_tile_map_width();
    unsigned int* tiles = (unsigned int*)(ENSI_TILE_MAP_BASE + ENSI_TILE_MAP_HEADER_SIZE);
    return (int)tiles[y * width + x];
}

/*============================================================================
 * Helper Macros
 *============================================================================*/

/** Unpack X coordinate from packed capital. */
#define CAPITAL_X(packed) ((packed) >> 16)
/** Unpack Y coordinate from packed capital. */
#define CAPITAL_Y(packed) ((packed) & 0xFFFF)

/** Unpack tile type from packed tile info. */
#define TILE_TYPE(packed) ((packed) & 0xFF)
/** Unpack owner from packed tile info. */
#define TILE_OWNER(packed) (((packed) >> 8) & 0xFF)
/** Unpack army count from packed tile info. */
#define TILE_ARMY(packed) ((packed) >> 16)

/** Check if tile is visible (not fog). */
#define TILE_VISIBLE(packed) (TILE_TYPE(packed) != TILE_FOG)
/** Check if tile is owned by a specific player. */
#define TILE_OWNED_BY(packed, player) (TILE_OWNER(packed) == (player))
/** Check if tile is a city. */
#define TILE_IS_CITY(packed) (TILE_TYPE(packed) == TILE_CITY)
/** Check if tile is passable (not mountain). */
#define TILE_PASSABLE(packed) (TILE_TYPE(packed) != TILE_MOUNTAIN)

/*============================================================================
 * Compatibility Types and Functions
 *
 * These provide backwards compatibility with older bot code.
 *============================================================================*/

/** Coordinate type. */
typedef struct {
    unsigned short x;
    unsigned short y;
} Coord;

/** Tile info from unpacked tile. */
typedef struct {
    unsigned char type;
    unsigned char owner;
    unsigned short army;
} TileInfo;

/** Map dimensions. */
typedef struct {
    unsigned short width;
    unsigned short height;
} MapSize;

/** Owner constants */
#define OWNER_NEUTRAL 0
#define OWNER_FOG 255

/** Create a coordinate. */
static inline Coord coord(unsigned short x, unsigned short y) {
    Coord c = { x, y };
    return c;
}

/** Get map size. */
static inline MapSize get_map_size(void) {
    MapSize m = { (unsigned short)ensi_get_map_width(), (unsigned short)ensi_get_map_height() };
    return m;
}

/** Get tile info (unpacked) - for backwards compatibility, uses push-based map. */
static inline TileInfo get_tile(unsigned short x, unsigned short y) {
    int packed = ensi_tile_map_get(x, y);
    TileInfo t = {
        (unsigned char)TILE_TYPE(packed),
        (unsigned char)TILE_OWNER(packed),
        (unsigned short)TILE_ARMY(packed)
    };
    return t;
}

/** Get player's capital. */
static inline Coord get_my_capital(void) {
    int packed = ensi_get_my_capital();
    Coord c = { (unsigned short)CAPITAL_X(packed), (unsigned short)CAPITAL_Y(packed) };
    return c;
}

/** Get player's population. */
static inline unsigned int get_my_population(void) {
    return (unsigned int)ensi_get_my_population();
}

/** Get player's army count. */
static inline unsigned int get_my_army(void) {
    return (unsigned int)ensi_get_my_army();
}

/** Get player ID. */
static inline unsigned char get_player_id(void) {
    return (unsigned char)ensi_get_player_id();
}

/** Get current turn. */
static inline unsigned int get_turn(void) {
    return (unsigned int)ensi_get_turn();
}

/** Tile type checks. */
static inline int is_fog(TileInfo t) { return t.type == TILE_FOG; }
static inline int is_passable(TileInfo t) { return t.type != TILE_MOUNTAIN && t.type != TILE_FOG; }
static inline int is_city(TileInfo t) { return t.type == TILE_CITY; }
static inline int is_mine(TileInfo t, unsigned char my_id) { return t.owner == my_id; }
static inline int is_enemy(TileInfo t, unsigned char my_id) {
    return t.owner != 0 && t.owner != my_id && t.owner != OWNER_FOG;
}

/** Move army. */
static inline int move_army(Coord from, Coord to, unsigned int count) {
    return ensi_move(from.x, from.y, to.x, to.y, (int)count);
}

/** Convert population to army. */
static inline int convert_pop(Coord city, unsigned int count) {
    return ensi_convert(city.x, city.y, (int)count);
}

/** Move capital. */
static inline int move_capital(Coord new_capital) {
    return ensi_move_capital(new_capital.x, new_capital.y);
}

/** Abandon a tile. */
static inline int abandon_tile(Coord tile) {
    return ensi_abandon(tile.x, tile.y);
}

/** End turn. */
static inline void yield_turn(void) {
    ensi_yield();
}

/** Manhattan distance. */
static inline int distance(Coord a, Coord b) {
    int dx = (int)a.x - (int)b.x;
    int dy = (int)a.y - (int)b.y;
    if (dx < 0) dx = -dx;
    if (dy < 0) dy = -dy;
    return dx + dy;
}

/*============================================================================
 * Bot Entry Point
 *
 * Bots must implement this function. It is called once per turn.
 *============================================================================*/

/**
 * Run one turn of the bot.
 *
 * This function is called by the game engine at the start of each turn.
 * The bot should query the game state and issue commands.
 *
 * The fuel_budget parameter indicates how many "fuel units" the bot has
 * for this turn. Each instruction and host call consumes fuel.
 * If fuel runs out, the turn ends automatically.
 *
 * @param fuel_budget Fuel available for this turn.
 * @return 0 on success (ignored by engine).
 */
int run_turn(int fuel_budget);

#endif /* ENSI_H */
