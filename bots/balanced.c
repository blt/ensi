/*
 * Balanced/Adaptive Bot for Ensi
 *
 * Strategy phases:
 * - Early game (turn 0-300): Expand aggressively, capture neutral cities
 * - Mid game (turn 300-700): Balance economy and military strength
 * - Late game (turn 700+): Push for victory with accumulated forces
 *
 * Adapts to:
 * - Food balance (avoid starvation)
 * - Enemy proximity (defensive posture when threatened)
 * - Resource availability (convert pop when safe)
 */

#include "ensi.h"

/* Game phase thresholds */
#define EARLY_GAME_END   300
#define MID_GAME_END     700

/* Strategy parameters */
#define MIN_FOOD_RESERVE     50
#define CRITICAL_FOOD_LEVEL  10
#define MAX_ARMY_SCAN_TILES  256

/* Direction offsets for adjacent tiles */
static const int DX[4] = {0, 1, 0, -1};
static const int DY[4] = {-1, 0, 1, 0};

/* Game state */
static uint8_t my_id;
static MapSize map_size;
static uint32_t turn;
static int32_t food;
static uint32_t population;
static uint32_t army;
static Coord capital;

/* Tracked army positions for movement */
typedef struct {
    Coord pos;
    uint16_t count;
    uint8_t valid;
} ArmyUnit;

static ArmyUnit my_armies[MAX_ARMY_SCAN_TILES];
static uint32_t army_count;

/* Tracked cities */
typedef struct {
    Coord pos;
    uint8_t mine;
    uint8_t neutral;
    uint8_t enemy;
} CityInfo;

static CityInfo cities[MAX_ARMY_SCAN_TILES];
static uint32_t city_count;

/* Determine game phase */
static int get_phase(void) {
    if (turn < EARLY_GAME_END) return 0;  /* Early */
    if (turn < MID_GAME_END) return 1;    /* Mid */
    return 2;                              /* Late */
}

/* Check if coordinates are within map bounds */
static int in_bounds(int x, int y) {
    return x >= 0 && x < map_size.width && y >= 0 && y < map_size.height;
}

/* Scan visible area around a point and record armies/cities */
static void scan_area(Coord center, int radius) {
    int cx = center.x;
    int cy = center.y;

    for (int dy = -radius; dy <= radius; dy++) {
        for (int dx = -radius; dx <= radius; dx++) {
            int x = cx + dx;
            int y = cy + dy;

            if (!in_bounds(x, y)) continue;

            TileInfo tile = get_tile((uint16_t)x, (uint16_t)y);

            if (is_fog(tile)) continue;

            /* Record armies we control */
            if (tile.army > 0 && tile.owner == my_id) {
                if (army_count < MAX_ARMY_SCAN_TILES) {
                    my_armies[army_count].pos = coord((uint16_t)x, (uint16_t)y);
                    my_armies[army_count].count = tile.army;
                    my_armies[army_count].valid = 1;
                    army_count++;
                }
            }

            /* Record cities */
            if (is_city(tile)) {
                if (city_count < MAX_ARMY_SCAN_TILES) {
                    cities[city_count].pos = coord((uint16_t)x, (uint16_t)y);
                    cities[city_count].mine = (tile.owner == my_id);
                    cities[city_count].neutral = (tile.owner == OWNER_NEUTRAL);
                    cities[city_count].enemy = is_enemy(tile, my_id);
                    city_count++;
                }
            }
        }
    }
}

/* Find nearest target (neutral or enemy city, or enemy army) */
static int find_nearest_target(Coord from, Coord *target, int prefer_neutral) {
    int best_dist = 9999;
    int found = 0;

    /* First pass: look for preferred targets */
    for (uint32_t i = 0; i < city_count; i++) {
        if (prefer_neutral && cities[i].neutral) {
            int d = distance(from, cities[i].pos);
            if (d < best_dist) {
                best_dist = d;
                *target = cities[i].pos;
                found = 1;
            }
        } else if (!prefer_neutral && cities[i].enemy) {
            int d = distance(from, cities[i].pos);
            if (d < best_dist) {
                best_dist = d;
                *target = cities[i].pos;
                found = 1;
            }
        }
    }

    /* Second pass: any valid target if none found */
    if (!found) {
        for (uint32_t i = 0; i < city_count; i++) {
            if (cities[i].neutral || cities[i].enemy) {
                int d = distance(from, cities[i].pos);
                if (d < best_dist) {
                    best_dist = d;
                    *target = cities[i].pos;
                    found = 1;
                }
            }
        }
    }

    return found;
}

/* Check if there's enemy threat nearby */
static int enemy_nearby(Coord pos, int radius) {
    for (int dy = -radius; dy <= radius; dy++) {
        for (int dx = -radius; dx <= radius; dx++) {
            int x = pos.x + dx;
            int y = pos.y + dy;

            if (!in_bounds(x, y)) continue;

            TileInfo tile = get_tile((uint16_t)x, (uint16_t)y);
            if (!is_fog(tile) && is_enemy(tile, my_id) && tile.army > 0) {
                return 1;
            }
        }
    }
    return 0;
}

/* Get the best direction to move toward a target */
static int get_best_direction(Coord from, Coord target, Coord *next) {
    int best_dist = 9999;
    int best_dir = -1;

    for (int dir = 0; dir < 4; dir++) {
        int nx = from.x + DX[dir];
        int ny = from.y + DY[dir];

        if (!in_bounds(nx, ny)) continue;

        TileInfo tile = get_tile((uint16_t)nx, (uint16_t)ny);
        if (!is_passable(tile)) continue;

        Coord next_pos = coord((uint16_t)nx, (uint16_t)ny);
        int d = distance(next_pos, target);

        if (d < best_dist) {
            best_dist = d;
            best_dir = dir;
            *next = next_pos;
        }
    }

    return best_dir >= 0;
}

/* Move army toward a target */
static void move_toward(Coord from, Coord target, uint16_t count) {
    Coord next;
    if (get_best_direction(from, target, &next)) {
        move_army(from, next, count);
    }
}

/* Convert population based on game phase and food situation */
static void manage_conversion(void) {
    int phase = get_phase();

    /* Don't convert if food is critical */
    if (food < CRITICAL_FOOD_LEVEL) return;

    /* Calculate safe conversion amount based on food */
    uint32_t safe_amount = 0;
    if (food > MIN_FOOD_RESERVE) {
        safe_amount = (uint32_t)(food - MIN_FOOD_RESERVE);
    }

    /* Adjust conversion rate by phase */
    uint32_t convert_rate;
    switch (phase) {
        case 0:  /* Early: moderate conversion for expansion */
            convert_rate = population / 4;
            break;
        case 1:  /* Mid: balanced conversion */
            convert_rate = population / 3;
            break;
        case 2:  /* Late: aggressive conversion for push */
            convert_rate = population / 2;
            break;
        default:
            convert_rate = population / 4;
    }

    /* Cap by safe amount */
    if (convert_rate > safe_amount) {
        convert_rate = safe_amount;
    }

    /* Convert in owned cities */
    for (uint32_t i = 0; i < city_count && convert_rate > 0; i++) {
        if (cities[i].mine) {
            uint32_t to_convert = convert_rate > 5 ? 5 : convert_rate;
            if (convert_pop(cities[i].pos, to_convert) == 0) {
                convert_rate -= to_convert;
            }
        }
    }
}

/* Move armies based on game phase */
static void manage_armies(void) {
    int phase = get_phase();
    int threat_detected = enemy_nearby(capital, 5);

    for (uint32_t i = 0; i < army_count; i++) {
        if (!my_armies[i].valid || my_armies[i].count == 0) continue;

        Coord pos = my_armies[i].pos;
        uint16_t count = my_armies[i].count;
        Coord target;

        /* If threat near capital, rally defense */
        if (threat_detected && distance(pos, capital) > 3) {
            move_toward(pos, capital, count);
            continue;
        }

        /* Find target based on phase */
        int prefer_neutral = (phase == 0);  /* Early game prefers neutral cities */

        if (find_nearest_target(pos, &target, prefer_neutral)) {
            /* Check if target is adjacent - attack with force */
            TileInfo target_tile = get_tile(target.x, target.y);

            if (is_adjacent(pos, target)) {
                /* Attack if we have advantage or it's neutral */
                if (target_tile.owner == OWNER_NEUTRAL || count > target_tile.army) {
                    move_army(pos, target, count);
                } else if (count > target_tile.army / 2) {
                    /* Probe attack if reasonably strong */
                    move_army(pos, target, count);
                }
            } else {
                /* Move toward target */
                move_toward(pos, target, count);
            }
        } else {
            /* No target found - explore outward from capital */
            int dx = pos.x - capital.x;
            int dy = pos.y - capital.y;

            /* Move away from capital to explore */
            Coord explore_target;
            if (dx == 0 && dy == 0) {
                explore_target = coord(capital.x + 5, capital.y);
            } else {
                int tx = capital.x + dx * 2;
                int ty = capital.y + dy * 2;
                if (tx < 0) tx = 0;
                if (ty < 0) ty = 0;
                if (tx >= map_size.width) tx = map_size.width - 1;
                if (ty >= map_size.height) ty = map_size.height - 1;
                explore_target = coord((uint16_t)tx, (uint16_t)ty);
            }
            move_toward(pos, explore_target, count);
        }
    }
}

/* Emergency food management */
static void handle_starvation(void) {
    if (food >= 0) return;

    /* When starving, be more defensive and don't convert */
    /* Armies will die off naturally, reducing drain */
    /* Just try to hold cities and wait for recovery */
}

/* Main bot logic - runs once per turn */
static void run_turn(void) {
    /* Refresh game state */
    turn = get_turn();
    my_id = get_player_id();
    map_size = get_map_size();
    capital = get_my_capital();
    food = get_my_food();
    population = get_my_population();
    army = get_my_army();

    /* Reset tracking arrays */
    army_count = 0;
    city_count = 0;

    /* Scan visible area (starts from capital, expands as we own more) */
    int scan_radius = 10 + (turn / 100);  /* Gradually increase scan range */
    if (scan_radius > 30) scan_radius = 30;

    scan_area(capital, scan_radius);

    /* Handle starvation if occurring */
    handle_starvation();

    /* Manage population conversion */
    manage_conversion();

    /* Move armies */
    manage_armies();
}

/* Entry point - infinite loop, one iteration per turn */
int main(void) {
    while (1) {
        run_turn();
        yield_turn();
    }
    return 0;
}
