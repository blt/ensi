/*
 * Ensi Game SDK - RISC-V Bot Interface
 *
 * This header provides the syscall interface for bots to interact with
 * the Ensi game engine. Bots are compiled to RV32IM and executed in a
 * sandboxed VM with a per-turn instruction budget.
 *
 * Game mechanics:
 * - Population lives in cities, produces food, can be converted to army
 * - Army moves on the map, fights, captures territory
 * - Food balance = (population * 2) - (population + army)
 *   = population - army (net +1 per pop, -1 per army)
 * - Combat: larger army wins, loses the smaller's count
 * - Fog of war: only see owned tiles and their neighbors
 */

#ifndef ENSI_H
#define ENSI_H

#include <stdint.h>

/* Syscall numbers */
#define SYS_GET_TURN        1
#define SYS_GET_PLAYER_ID   2
#define SYS_GET_MY_CAPITAL  3
#define SYS_GET_TILE        4
#define SYS_GET_MY_FOOD     5
#define SYS_GET_MY_POPULATION 6
#define SYS_GET_MY_ARMY     7
#define SYS_GET_MAP_SIZE    8
#define SYS_MOVE            100
#define SYS_CONVERT         101
#define SYS_MOVE_CAPITAL    102
#define SYS_YIELD           103

/* Tile types from GET_TILE */
#define TILE_CITY     0
#define TILE_DESERT   1
#define TILE_MOUNTAIN 2
#define TILE_FOG      255

/* Owner values from GET_TILE */
#define OWNER_NEUTRAL 0
#define OWNER_FOG     255

/* Coordinate type */
typedef struct {
    uint16_t x;
    uint16_t y;
} Coord;

/* Tile info from GET_TILE (unpacked) */
typedef struct {
    uint8_t type;    /* TILE_CITY, TILE_DESERT, TILE_MOUNTAIN, or TILE_FOG */
    uint8_t owner;   /* 0=neutral, 1-8=player, 255=fog */
    uint16_t army;   /* Army count on tile (0 if fog) */
} TileInfo;

/* Map dimensions */
typedef struct {
    uint16_t width;
    uint16_t height;
} MapSize;

/*
 * Raw syscall - inline assembly for ecall
 * Returns value in a0 after syscall
 */
static inline uint32_t syscall0(uint32_t num) {
    register uint32_t a7 __asm__("a7") = num;
    register uint32_t a0 __asm__("a0");
    __asm__ volatile("ecall" : "=r"(a0) : "r"(a7) : "memory");
    return a0;
}

static inline uint32_t syscall1(uint32_t num, uint32_t arg0) {
    register uint32_t a7 __asm__("a7") = num;
    register uint32_t a0 __asm__("a0") = arg0;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
    return a0;
}

static inline uint32_t syscall2(uint32_t num, uint32_t arg0, uint32_t arg1) {
    register uint32_t a7 __asm__("a7") = num;
    register uint32_t a0 __asm__("a0") = arg0;
    register uint32_t a1 __asm__("a1") = arg1;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a7), "r"(a1) : "memory");
    return a0;
}

static inline uint32_t syscall3(uint32_t num, uint32_t arg0, uint32_t arg1, uint32_t arg2) {
    register uint32_t a7 __asm__("a7") = num;
    register uint32_t a0 __asm__("a0") = arg0;
    register uint32_t a1 __asm__("a1") = arg1;
    register uint32_t a2 __asm__("a2") = arg2;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a7), "r"(a1), "r"(a2) : "memory");
    return a0;
}

static inline uint32_t syscall5(uint32_t num, uint32_t arg0, uint32_t arg1,
                                 uint32_t arg2, uint32_t arg3, uint32_t arg4) {
    register uint32_t a7 __asm__("a7") = num;
    register uint32_t a0 __asm__("a0") = arg0;
    register uint32_t a1 __asm__("a1") = arg1;
    register uint32_t a2 __asm__("a2") = arg2;
    register uint32_t a3 __asm__("a3") = arg3;
    register uint32_t a4 __asm__("a4") = arg4;
    __asm__ volatile("ecall" : "+r"(a0) : "r"(a7), "r"(a1), "r"(a2), "r"(a3), "r"(a4) : "memory");
    return a0;
}

/*
 * High-level API functions
 */

/* Get current turn number (0-indexed) */
static inline uint32_t get_turn(void) {
    return syscall0(SYS_GET_TURN);
}

/* Get this player's ID (1-8) */
static inline uint8_t get_player_id(void) {
    return (uint8_t)syscall0(SYS_GET_PLAYER_ID);
}

/* Get this player's capital location */
static inline Coord get_my_capital(void) {
    uint32_t packed = syscall0(SYS_GET_MY_CAPITAL);
    Coord c = { (uint16_t)(packed >> 16), (uint16_t)(packed & 0xFFFF) };
    return c;
}

/* Get tile information at coordinates (respects fog of war) */
static inline TileInfo get_tile(uint16_t x, uint16_t y) {
    uint32_t packed = syscall2(SYS_GET_TILE, x, y);
    TileInfo t = {
        (uint8_t)(packed & 0xFF),
        (uint8_t)((packed >> 8) & 0xFF),
        (uint16_t)(packed >> 16)
    };
    return t;
}

/* Get food balance (can be negative) */
static inline int32_t get_my_food(void) {
    return (int32_t)syscall0(SYS_GET_MY_FOOD);
}

/* Get total population across all cities */
static inline uint32_t get_my_population(void) {
    return syscall0(SYS_GET_MY_POPULATION);
}

/* Get total army count across all tiles */
static inline uint32_t get_my_army(void) {
    return syscall0(SYS_GET_MY_ARMY);
}

/* Get map dimensions */
static inline MapSize get_map_size(void) {
    uint32_t packed = syscall0(SYS_GET_MAP_SIZE);
    MapSize m = { (uint16_t)(packed >> 16), (uint16_t)(packed & 0xFFFF) };
    return m;
}

/*
 * Move army from one tile to an adjacent tile.
 * Returns 0 on success, non-zero on failure.
 *
 * Fails if:
 * - Source tile not owned by you
 * - Not enough army at source
 * - Destination not adjacent
 * - Destination is impassable (mountain)
 */
static inline int move_army(Coord from, Coord to, uint32_t count) {
    return (int)syscall5(SYS_MOVE, from.x, from.y, to.x, to.y, count);
}

/*
 * Convert population to army in a city.
 * Returns 0 on success, non-zero on failure.
 *
 * Fails if:
 * - Tile is not a city
 * - City not owned by you
 * - Not enough population
 */
static inline int convert_pop(Coord city, uint32_t count) {
    return (int)syscall3(SYS_CONVERT, city.x, city.y, count);
}

/*
 * Move capital to a city with larger population.
 * Returns 0 on success, non-zero on failure.
 *
 * Fails if:
 * - Tile is not a city
 * - City not owned by you
 * - New city doesn't have more population than current capital
 */
static inline int move_capital(Coord new_capital) {
    return (int)syscall2(SYS_MOVE_CAPITAL, new_capital.x, new_capital.y);
}

/* End turn early (saves instruction budget for next turn... just kidding, it doesn't) */
static inline void yield_turn(void) {
    syscall0(SYS_YIELD);
}

/*
 * Utility functions
 */

static inline Coord coord(uint16_t x, uint16_t y) {
    Coord c = { x, y };
    return c;
}

static inline int is_fog(TileInfo t) {
    return t.type == TILE_FOG;
}

static inline int is_passable(TileInfo t) {
    return t.type != TILE_MOUNTAIN && t.type != TILE_FOG;
}

static inline int is_city(TileInfo t) {
    return t.type == TILE_CITY;
}

static inline int is_mine(TileInfo t, uint8_t my_id) {
    return t.owner == my_id;
}

static inline int is_enemy(TileInfo t, uint8_t my_id) {
    return t.owner != 0 && t.owner != my_id && t.owner != OWNER_FOG;
}

/* Distance between two coordinates (Manhattan) */
static inline int distance(Coord a, Coord b) {
    int dx = (int)a.x - (int)b.x;
    int dy = (int)a.y - (int)b.y;
    if (dx < 0) dx = -dx;
    if (dy < 0) dy = -dy;
    return dx + dy;
}

/* Check if two coordinates are adjacent */
static inline int is_adjacent(Coord a, Coord b) {
    return distance(a, b) == 1;
}

#endif /* ENSI_H */
