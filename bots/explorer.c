/*
 * Explorer Bot - Methodical territory exploration and control
 *
 * Strategy:
 * - Systematic spiral exploration from capital outward
 * - Send scouts to explore fog of war
 * - Claim neutral territory methodically
 * - Spread army thin to maximize territory coverage
 * - Maintain steady population-to-army conversion rate
 */

#include "ensi.h"

/* Direction offsets for movement (right, down, left, up) */
static const int DX[4] = { 1, 0, -1, 0 };
static const int DY[4] = { 0, 1, 0, -1 };

/* Get a direction based on turn number for spiral exploration */
static int get_spiral_direction(uint32_t turn, int layer) {
    /* Rotate through directions, changing with layer depth */
    return (turn + layer) % 4;
}

/* Check if coordinate is within map bounds */
static int in_bounds(int x, int y, MapSize map) {
    return x >= 0 && x < map.width && y >= 0 && y < map.height;
}

/* Find the best adjacent tile to move toward (prioritize fog, then neutral, then owned) */
static int find_exploration_target(Coord from, MapSize map, uint8_t my_id, Coord *target) {
    int best_priority = -1;
    int best_dir = -1;

    for (int dir = 0; dir < 4; dir++) {
        int nx = (int)from.x + DX[dir];
        int ny = (int)from.y + DY[dir];

        if (!in_bounds(nx, ny, map)) continue;

        TileInfo tile = get_tile((uint16_t)nx, (uint16_t)ny);

        /* Skip mountains and our own tiles with army */
        if (tile.type == TILE_MOUNTAIN) continue;

        int priority = 0;
        if (tile.type == TILE_FOG) {
            /* Highest priority: unexplored fog */
            priority = 100;
        } else if (tile.owner == OWNER_NEUTRAL) {
            /* High priority: unclaimed territory */
            priority = 80;
            /* Bonus for cities */
            if (tile.type == TILE_CITY) priority += 15;
        } else if (tile.owner != my_id) {
            /* Medium priority: enemy territory (if we can take it) */
            priority = 50;
        } else {
            /* Low priority: our own territory (for passing through) */
            priority = 10;
        }

        if (priority > best_priority) {
            best_priority = priority;
            best_dir = dir;
        }
    }

    if (best_dir >= 0) {
        target->x = (uint16_t)((int)from.x + DX[best_dir]);
        target->y = (uint16_t)((int)from.y + DY[best_dir]);
        return 1;
    }
    return 0;
}

/* Send scouts from tiles with army to explore */
static void explore_from_tile(Coord pos, uint8_t my_id, MapSize map, uint32_t turn) {
    TileInfo tile = get_tile(pos.x, pos.y);

    /* Only process our tiles with army */
    if (!is_mine(tile, my_id) || tile.army < 2) return;

    /* Calculate how many scouts to send (leave 1 behind for territory) */
    uint32_t available = tile.army - 1;
    if (available == 0) return;

    /* Try to find an exploration target */
    Coord target;
    if (find_exploration_target(pos, map, my_id, &target)) {
        TileInfo target_tile = get_tile(target.x, target.y);

        /* Calculate army needed */
        uint32_t needed = 1;
        if (!is_fog(target_tile) && !is_mine(target_tile, my_id)) {
            /* Need to overcome enemy army */
            needed = target_tile.army + 1;
        }

        if (available >= needed) {
            move_army(pos, target, needed);
        } else if (available > 0 && (is_fog(target_tile) || target_tile.owner == OWNER_NEUTRAL)) {
            /* Send what we have for neutral/fog */
            move_army(pos, target, available);
        }
    }
}

/* Scan owned tiles in a spiral pattern from capital */
static void spiral_explore(Coord capital, uint8_t my_id, MapSize map, uint32_t turn) {
    /* Determine scan radius based on turn (expand over time) */
    int max_radius = (int)(turn / 4) + 3;
    if (max_radius > 30) max_radius = 30;

    /* Spiral outward from capital */
    for (int radius = 0; radius <= max_radius; radius++) {
        /* Scan the perimeter at this radius */
        for (int offset = -radius; offset <= radius; offset++) {
            /* Top edge */
            int x = (int)capital.x + offset;
            int y = (int)capital.y - radius;
            if (in_bounds(x, y, map)) {
                explore_from_tile(coord((uint16_t)x, (uint16_t)y), my_id, map, turn);
            }

            /* Bottom edge */
            y = (int)capital.y + radius;
            if (in_bounds(x, y, map)) {
                explore_from_tile(coord((uint16_t)x, (uint16_t)y), my_id, map, turn);
            }

            /* Left edge (skip corners already done) */
            if (offset != -radius && offset != radius) {
                x = (int)capital.x - radius;
                y = (int)capital.y + offset;
                if (in_bounds(x, y, map)) {
                    explore_from_tile(coord((uint16_t)x, (uint16_t)y), my_id, map, turn);
                }

                /* Right edge */
                x = (int)capital.x + radius;
                if (in_bounds(x, y, map)) {
                    explore_from_tile(coord((uint16_t)x, (uint16_t)y), my_id, map, turn);
                }
            }
        }
    }
}

/* Convert population to army at a sustainable rate */
static void manage_conversion(Coord capital, uint8_t my_id, MapSize map) {
    int32_t food = get_my_food();
    uint32_t population = get_my_population();
    uint32_t army = get_my_army();

    /* Calculate sustainable conversion: food balance = pop - army
     * We want to keep food positive, so convert when pop > army + buffer */
    int32_t buffer = 5;
    int32_t convertible = (int32_t)population - (int32_t)army - buffer;

    if (convertible <= 0) return;

    /* Convert at capital first */
    TileInfo cap_tile = get_tile(capital.x, capital.y);
    if (is_city(cap_tile) && is_mine(cap_tile, my_id)) {
        /* Convert up to half of surplus, minimum 1 */
        uint32_t to_convert = (uint32_t)(convertible / 2);
        if (to_convert < 1) to_convert = 1;
        if (to_convert > 10) to_convert = 10; /* Cap per turn */

        convert_pop(capital, to_convert);
    }

    /* Also convert at other owned cities */
    MapSize map_size = get_map_size();
    for (int y = 0; y < map_size.height; y++) {
        for (int x = 0; x < map_size.width; x++) {
            TileInfo tile = get_tile((uint16_t)x, (uint16_t)y);
            if (is_city(tile) && is_mine(tile, my_id)) {
                Coord city = coord((uint16_t)x, (uint16_t)y);
                if (city.x != capital.x || city.y != capital.y) {
                    /* Convert a small amount at non-capital cities */
                    convert_pop(city, 1);
                }
            }
        }
    }
}

/* Spread army thin across territory for maximum coverage */
static void spread_army(uint8_t my_id, MapSize map, uint32_t turn) {
    /* Direction based on turn for varied spreading */
    int dir_offset = (int)(turn % 4);

    for (int y = 0; y < map.height; y++) {
        for (int x = 0; x < map.width; x++) {
            TileInfo tile = get_tile((uint16_t)x, (uint16_t)y);

            /* Only spread from tiles with excess army */
            if (!is_mine(tile, my_id) || tile.army < 3) continue;

            Coord pos = coord((uint16_t)x, (uint16_t)y);
            uint32_t excess = tile.army - 1;

            /* Try each direction, starting from rotated offset */
            for (int d = 0; d < 4; d++) {
                int dir = (d + dir_offset) % 4;
                int nx = x + DX[dir];
                int ny = y + DY[dir];

                if (!in_bounds(nx, ny, map)) continue;

                TileInfo adj = get_tile((uint16_t)nx, (uint16_t)ny);

                /* Spread to our own tiles that need reinforcement */
                if (is_mine(adj, my_id) && adj.army == 0) {
                    Coord target = coord((uint16_t)nx, (uint16_t)ny);
                    uint32_t send = excess / 2;
                    if (send < 1) send = 1;
                    move_army(pos, target, send);
                    break; /* One spread per tile per turn */
                }
            }
        }
    }
}

int main(void) {
    /* Main game loop - runs once per turn */
    while (1) {
        uint32_t turn = get_turn();
        uint8_t my_id = get_player_id();
        Coord capital = get_my_capital();
        MapSize map = get_map_size();

        /* Phase 1: Convert population to army (sustainable rate) */
        manage_conversion(capital, my_id, map);

        /* Phase 2: Spiral exploration from capital outward */
        spiral_explore(capital, my_id, map, turn);

        /* Phase 3: Spread army for territory coverage */
        spread_army(my_id, map, turn);

        /* End turn */
        yield_turn();
    }

    return 0;
}
