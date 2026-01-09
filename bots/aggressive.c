/*
 * Aggressive Bot - Maximize military and attack enemies
 *
 * Strategy:
 * - Convert most population to army (keep minimal for growth)
 * - Aggressively expand toward enemy territory
 * - Prioritize attacking enemy tiles with army
 * - Scout outward from capital
 */

#include "ensi.h"

/* Direction offsets for adjacent tiles */
static const int dx[4] = { 0, 1, 0, -1 };
static const int dy[4] = { -1, 0, 1, 0 };

/* Minimum population to keep for growth */
#define MIN_POP_KEEP 2

/* Simple absolute value */
static int abs_val(int x) {
    return x < 0 ? -x : x;
}

/* Find and convert population to army in owned cities */
static void build_army(uint8_t my_id, MapSize map) {
    uint32_t pop = get_my_population();

    /* Keep minimal population for growth, convert rest to army */
    if (pop <= MIN_POP_KEEP) {
        return;
    }

    uint32_t to_convert = pop - MIN_POP_KEEP;

    /* Scan map for our cities and convert population */
    for (uint16_t y = 0; y < map.height && to_convert > 0; y++) {
        for (uint16_t x = 0; x < map.width && to_convert > 0; x++) {
            TileInfo tile = get_tile(x, y);
            if (is_city(tile) && is_mine(tile, my_id)) {
                /* Try to convert some population here */
                Coord city = coord(x, y);
                uint32_t amount = to_convert > 10 ? 10 : to_convert;
                if (convert_pop(city, amount) == 0) {
                    to_convert -= amount;
                }
            }
        }
    }
}

/* Find the best move for army on a tile - prioritize enemies, then expansion */
static void move_army_from(Coord from, uint16_t army_count, uint8_t my_id,
                           Coord capital, MapSize map) {
    if (army_count == 0) {
        return;
    }

    int best_priority = -1000;
    Coord best_target = from;
    int found = 0;

    /* Check all 4 adjacent tiles */
    for (int d = 0; d < 4; d++) {
        int nx = (int)from.x + dx[d];
        int ny = (int)from.y + dy[d];

        /* Bounds check */
        if (nx < 0 || nx >= map.width || ny < 0 || ny >= map.height) {
            continue;
        }

        Coord target = coord((uint16_t)nx, (uint16_t)ny);
        TileInfo tile = get_tile(target.x, target.y);

        /* Skip impassable or fog */
        if (!is_passable(tile)) {
            continue;
        }

        int priority = 0;

        if (is_enemy(tile, my_id)) {
            /* High priority: attack enemies! */
            priority = 1000;
            /* Extra priority for enemy cities */
            if (is_city(tile)) {
                priority += 500;
            }
            /* Only attack if we can win (our army > their army) */
            if (army_count <= tile.army) {
                priority = -100; /* Don't suicide */
            }
        } else if (tile.owner == OWNER_NEUTRAL) {
            /* Medium priority: capture neutral territory */
            priority = 100;
            if (is_city(tile)) {
                priority += 200; /* Neutral cities are valuable */
            }
        } else if (is_mine(tile, my_id)) {
            /* Low priority: move within our territory (for scouting) */
            /* Prefer moving away from capital to expand */
            int dist_from_cap = distance(target, capital);
            int current_dist = distance(from, capital);
            if (dist_from_cap > current_dist) {
                priority = 10 + dist_from_cap; /* Move outward */
            } else {
                priority = -10; /* Avoid moving inward */
            }
        }

        if (priority > best_priority) {
            best_priority = priority;
            best_target = target;
            found = 1;
        }
    }

    /* Move all army to best target if found a good one */
    if (found && best_priority > -50) {
        move_army(from, best_target, army_count);
    }
}

/* Main loop - runs once per turn */
int main(void) {
    while (1) {
        uint8_t my_id = get_player_id();
        MapSize map = get_map_size();
        Coord capital = get_my_capital();

        /* Phase 1: Build army from population */
        build_army(my_id, map);

        /* Phase 2: Move all armies - scan from edges toward center for expansion */
        /* This ensures outer units move first, avoiding blocking */
        for (uint16_t y = 0; y < map.height; y++) {
            for (uint16_t x = 0; x < map.width; x++) {
                TileInfo tile = get_tile(x, y);

                /* Only move our armies */
                if (!is_mine(tile, my_id) || tile.army == 0) {
                    continue;
                }

                Coord from = coord(x, y);
                move_army_from(from, tile.army, my_id, capital, map);
            }
        }

        /* End this turn */
        yield_turn();
    }

    return 0;
}
