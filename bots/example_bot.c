/**
 * Example Ensi Bot
 *
 * A simple bot that demonstrates the SDK API.
 * Strategy: Convert population to army, then expand aggressively.
 *
 * Build:
 *   clang --target=wasm32-unknown-unknown -O3 -nostdlib \
 *         -Wl,--no-entry -Wl,--export=run_turn \
 *         -o example_bot.wasm example_bot.c
 */

#include "ensi.h"

/**
 * Simple pseudo-random number generator.
 * Uses a linear congruential generator with state persisted across turns.
 */
static unsigned int rng_state = 12345;

static unsigned int rand_next(void) {
    rng_state = rng_state * 1103515245 + 12345;
    return (rng_state >> 16) & 0x7FFF;
}

/**
 * Find and convert population to army in our cities.
 * Uses push-based visibility map for fast tile queries.
 */
static void convert_armies(int player_id, int map_width, int map_height) {
    for (int y = 0; y < map_height; y++) {
        for (int x = 0; x < map_width; x++) {
            // Use push-based map read (no syscall!)
            int tile = ensi_tile_map_get(x, y);

            // Skip non-visible tiles
            if (!TILE_VISIBLE(tile)) continue;

            // Skip tiles we don't own
            if (!TILE_OWNED_BY(tile, player_id)) continue;

            // Skip non-cities
            if (!TILE_IS_CITY(tile)) continue;

            // Convert up to 10% of population to army per turn
            // This balances growth vs military
            int pop = ensi_get_my_population();
            int to_convert = pop / 10;
            if (to_convert > 0) {
                ensi_convert(x, y, to_convert > 50 ? 50 : to_convert);
            }
        }
    }
}

/**
 * Move armies to expand territory or attack enemies.
 * Uses push-based visibility map for fast tile queries.
 */
static void expand_and_attack(int player_id, int map_width, int map_height) {
    for (int y = 0; y < map_height; y++) {
        for (int x = 0; x < map_width; x++) {
            // Use push-based map read (no syscall!)
            int tile = ensi_tile_map_get(x, y);

            // Skip non-visible tiles
            if (!TILE_VISIBLE(tile)) continue;

            // Skip tiles we don't own
            if (!TILE_OWNED_BY(tile, player_id)) continue;

            // Get army count on this tile
            int army = TILE_ARMY(tile);
            if (army < 2) continue;  // Keep at least 1 for defense

            // Check adjacent tiles for expansion opportunities
            // Try each direction: up, down, left, right
            int dx[] = {0, 0, -1, 1};
            int dy[] = {-1, 1, 0, 0};

            for (int d = 0; d < 4; d++) {
                int nx = x + dx[d];
                int ny = y + dy[d];

                // Skip out of bounds
                if (nx < 0 || nx >= map_width || ny < 0 || ny >= map_height) continue;

                // Use push-based map read (no syscall!)
                int neighbor = ensi_tile_map_get(nx, ny);

                // Skip impassable terrain
                if (!TILE_PASSABLE(neighbor)) continue;

                // Prioritize unowned or enemy tiles
                int owner = TILE_OWNER(neighbor);
                if (owner == player_id) continue;  // Already ours

                // Calculate attack force
                int enemy_army = TILE_ARMY(neighbor);
                int attack_force = army - 1;  // Keep 1 for defense

                // Only attack if we have numerical advantage
                if (attack_force > enemy_army) {
                    // Move most of our army
                    ensi_move(x, y, nx, ny, attack_force);
                    break;  // Only one move per tile per turn
                }
            }
        }
    }
}

/**
 * Main bot entry point.
 */
int run_turn(int fuel_budget) {
    (void)fuel_budget;  // We don't track fuel manually

    int player_id = ensi_get_player_id();
    int map_width = ensi_get_map_width();
    int map_height = ensi_get_map_height();

    // Seed RNG with turn number for variety
    rng_state = (unsigned int)(ensi_get_turn() + player_id * 1000);

    // Phase 1: Build armies
    convert_armies(player_id, map_width, map_height);

    // Phase 2: Expand and attack
    expand_and_attack(player_id, map_width, map_height);

    // End turn
    ensi_yield();

    return 0;
}
