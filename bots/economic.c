/*
 * Economic/Defensive Bot for Ensi
 *
 * Strategy:
 * - Maximize population growth by maintaining positive food balance
 * - Keep army small (just enough for defense)
 * - Expand to capture neutral cities
 * - Defend owned territory rather than attack enemies
 */

#include "ensi.h"

/* Configuration constants */
#define MAX_CITIES 64
#define MIN_FOOD_BUFFER 5       /* Minimum food surplus to maintain */
#define DEFENSE_ARMY_PER_CITY 3 /* Base army per owned city */
#define EXPANSION_ARMY 2        /* Army to send for capturing neutral cities */

/* Simple array to track our cities */
static Coord my_cities[MAX_CITIES];
static int num_cities;

/* Find all cities we own */
static void find_my_cities(uint8_t my_id) {
    MapSize map = get_map_size();
    num_cities = 0;

    for (uint16_t y = 0; y < map.height && num_cities < MAX_CITIES; y++) {
        for (uint16_t x = 0; x < map.width && num_cities < MAX_CITIES; x++) {
            TileInfo tile = get_tile(x, y);
            if (is_city(tile) && is_mine(tile, my_id)) {
                my_cities[num_cities++] = coord(x, y);
            }
        }
    }
}

/* Find army on a tile we own */
static uint16_t get_army_at(Coord pos, uint8_t my_id) {
    TileInfo tile = get_tile(pos.x, pos.y);
    if (is_mine(tile, my_id)) {
        return tile.army;
    }
    return 0;
}

/* Check if position is within map bounds */
static int in_bounds(int x, int y, MapSize map) {
    return x >= 0 && x < map.width && y >= 0 && y < map.height;
}

/* Direction offsets: up, right, down, left */
static const int dx[] = {0, 1, 0, -1};
static const int dy[] = {-1, 0, 1, 0};

/* Find a neutral city adjacent to a tile we own and try to capture it */
static void expand_to_neutral_cities(uint8_t my_id) {
    MapSize map = get_map_size();

    /* Scan all visible tiles for our armies */
    for (uint16_t y = 0; y < map.height; y++) {
        for (uint16_t x = 0; x < map.width; x++) {
            TileInfo tile = get_tile(x, y);
            if (!is_mine(tile, my_id) || tile.army < EXPANSION_ARMY) {
                continue;
            }

            /* Check adjacent tiles for neutral cities */
            for (int d = 0; d < 4; d++) {
                int nx = (int)x + dx[d];
                int ny = (int)y + dy[d];

                if (!in_bounds(nx, ny, map)) continue;

                TileInfo adj = get_tile((uint16_t)nx, (uint16_t)ny);

                /* Target neutral cities (owner = 0) */
                if (is_city(adj) && adj.owner == OWNER_NEUTRAL) {
                    /* Send enough army to capture */
                    uint16_t to_send = tile.army > EXPANSION_ARMY ? EXPANSION_ARMY : tile.army;
                    if (to_send > adj.army) {
                        move_army(coord(x, y), coord((uint16_t)nx, (uint16_t)ny), to_send);
                        return; /* One expansion per turn to conserve */
                    }
                }
            }
        }
    }
}

/* Move army to defend a threatened tile */
static void defend_territory(uint8_t my_id) {
    MapSize map = get_map_size();

    /* Find tiles with enemy armies adjacent to our territory */
    for (uint16_t y = 0; y < map.height; y++) {
        for (uint16_t x = 0; x < map.width; x++) {
            TileInfo tile = get_tile(x, y);
            if (!is_mine(tile, my_id)) continue;

            /* Check if any adjacent tile has enemy army */
            for (int d = 0; d < 4; d++) {
                int nx = (int)x + dx[d];
                int ny = (int)y + dy[d];

                if (!in_bounds(nx, ny, map)) continue;

                TileInfo adj = get_tile((uint16_t)nx, (uint16_t)ny);

                if (is_enemy(adj, my_id) && adj.army > 0) {
                    /* Enemy adjacent - gather army to this tile */
                    /* Look for nearby friendly tiles with spare army */
                    for (int d2 = 0; d2 < 4; d2++) {
                        int fx = (int)x + dx[d2];
                        int fy = (int)y + dy[d2];

                        if (!in_bounds(fx, fy, map)) continue;

                        TileInfo friend_tile = get_tile((uint16_t)fx, (uint16_t)fy);
                        if (is_mine(friend_tile, my_id) && friend_tile.army > 1) {
                            /* Move army to threatened tile */
                            move_army(coord((uint16_t)fx, (uint16_t)fy),
                                      coord(x, y),
                                      friend_tile.army - 1);
                        }
                    }
                }
            }
        }
    }
}

/* Convert population to army conservatively */
static void convert_for_defense(uint8_t my_id) {
    int32_t food = get_my_food();
    uint32_t population = get_my_population();
    uint32_t army = get_my_army();

    /* Calculate desired army size: just enough for basic defense */
    uint32_t desired_army = (uint32_t)num_cities * DEFENSE_ARMY_PER_CITY;

    /* Only convert if:
     * 1. We need more army
     * 2. We have enough food buffer
     * 3. Food balance will stay positive after conversion
     *
     * Food balance = population - army
     * After converting N: balance = (population - N) - (army + N) = population - army - 2N
     */
    if (army >= desired_army) return;

    uint32_t needed = desired_army - army;

    /* How much can we convert while keeping food balance positive? */
    int32_t current_balance = (int32_t)population - (int32_t)army;
    if (current_balance <= MIN_FOOD_BUFFER) return;

    /* After converting N, balance = current_balance - 2N
     * We want: current_balance - 2N >= MIN_FOOD_BUFFER
     * So: N <= (current_balance - MIN_FOOD_BUFFER) / 2
     */
    int32_t max_convert = (current_balance - MIN_FOOD_BUFFER) / 2;
    if (max_convert <= 0) return;

    uint32_t to_convert = needed < (uint32_t)max_convert ? needed : (uint32_t)max_convert;
    if (to_convert == 0) return;

    /* Convert from the capital (usually has most population) */
    Coord capital = get_my_capital();
    convert_pop(capital, to_convert);
}

/* Spread army from capital to other cities for defense */
static void distribute_army(uint8_t my_id) {
    Coord capital = get_my_capital();
    TileInfo cap_tile = get_tile(capital.x, capital.y);

    if (cap_tile.army <= DEFENSE_ARMY_PER_CITY) return;

    MapSize map = get_map_size();
    uint16_t excess = cap_tile.army - DEFENSE_ARMY_PER_CITY;

    /* Move excess army towards cities that need defense */
    for (int d = 0; d < 4 && excess > 0; d++) {
        int nx = (int)capital.x + dx[d];
        int ny = (int)capital.y + dy[d];

        if (!in_bounds(nx, ny, map)) continue;

        TileInfo adj = get_tile((uint16_t)nx, (uint16_t)ny);

        /* Move to passable owned tiles */
        if (is_passable(adj) && (is_mine(adj, my_id) || adj.owner == OWNER_NEUTRAL)) {
            uint16_t to_move = excess > EXPANSION_ARMY ? EXPANSION_ARMY : excess;
            if (move_army(capital, coord((uint16_t)nx, (uint16_t)ny), to_move) == 0) {
                excess -= to_move;
            }
        }
    }
}

int main(void) {
    uint8_t my_id = get_player_id();

    /* Main game loop - runs once per turn */
    while (1) {
        /* Update city list */
        find_my_cities(my_id);

        /* Priority 1: Defend against threats */
        defend_territory(my_id);

        /* Priority 2: Convert population if we need army */
        convert_for_defense(my_id);

        /* Priority 3: Expand to neutral cities */
        expand_to_neutral_cities(my_id);

        /* Priority 4: Distribute army from capital */
        distribute_army(my_id);

        /* End turn */
        yield_turn();
    }

    return 0;
}
