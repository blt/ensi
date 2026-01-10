# Ensi Bots

This directory contains WASM bot implementations for the Ensi game.

## Building

Requires clang with WASM target:

```bash
# Build a bot
clang --target=wasm32-unknown-unknown -O3 -nostdlib \
      -Wl,--no-entry -Wl,--export=run_turn \
      -o mybot.wasm mybot.c

# Or use the SDK makefile
cd ../sdk && make
```

## Example Bot

`example_bot.c` demonstrates the SDK API - converts population to army
and expands aggressively.

## SDK

The `ensi.h` header provides the game interface.

### Query Functions
- `ensi_get_turn()` - Current turn number (0-indexed)
- `ensi_get_player_id()` - Your player ID (1-8)
- `ensi_get_my_capital()` - Your capital coordinates (packed)
- `ensi_get_tile(x, y)` - Tile info (respects fog of war)
- `ensi_get_my_food()` - Food balance (can be negative)
- `ensi_get_my_population()` - Total population
- `ensi_get_my_army()` - Total army
- `ensi_get_map_width/height()` - Map dimensions

### High-Performance Tile Access
- `ensi_tile_map_get(x, y)` - Read tile from push-based visibility map (100x faster)

### Command Functions
- `ensi_move(fx, fy, tx, ty, count)` - Move army to adjacent tile
- `ensi_convert(cx, cy, count)` - Convert population to army
- `ensi_move_capital(cx, cy)` - Move capital to larger city
- `ensi_yield()` - End turn early

### Compatibility Layer
For simpler code, use the compatibility wrapper functions which provide
struct-based interfaces:
- `get_tile(x, y)` returns `TileInfo` struct (uses push-based map)
- `move_army(from, to, count)` takes `Coord` structs
- `convert_pop(city, count)` takes `Coord` struct
- etc.

## Writing Your Own Bot

```c
#include "ensi.h"

int run_turn(int fuel_budget) {
    int my_id = ensi_get_player_id();
    int width = ensi_get_map_width();
    int height = ensi_get_map_height();

    // Scan map using push-based visibility (no syscall overhead)
    for (int y = 0; y < height; y++) {
        for (int x = 0; x < width; x++) {
            int tile = ensi_tile_map_get(x, y);
            if (TILE_OWNED_BY(tile, my_id) && TILE_IS_CITY(tile)) {
                // Your strategy here
            }
        }
    }

    ensi_yield();
    return 0;
}
```

Key constraints:
- Entry point must be `int run_turn(int fuel_budget)`
- No libc (use -nostdlib)
- Per-turn fuel budget
- Call `ensi_yield()` to end turn early
