# Ensi Bots

This directory contains RISC-V bot implementations for the Ensi game.

## Building

Requires a RISC-V toolchain:

```bash
# Arch Linux
sudo pacman -S riscv64-elf-gcc riscv64-elf-binutils

# Ubuntu/Debian
sudo apt install gcc-riscv64-linux-gnu

# Then build
make
```

## Bots

| Bot | Strategy | Description |
|-----|----------|-------------|
| `aggressive.c` | Military | Maximizes army, attacks enemies aggressively |
| `economic.c` | Defensive | Focuses on population growth, minimal army |
| `balanced.c` | Adaptive | Changes strategy based on game phase |
| `explorer.c` | Territory | Systematic exploration and expansion |

## SDK

The `ensi.h` header provides the game interface:

### Query Functions
- `get_turn()` - Current turn number (0-indexed)
- `get_player_id()` - Your player ID (1-8)
- `get_my_capital()` - Your capital coordinates
- `get_tile(x, y)` - Tile info (respects fog of war)
- `get_my_food()` - Food balance (can be negative)
- `get_my_population()` - Total population
- `get_my_army()` - Total army
- `get_map_size()` - Map dimensions

### Command Functions
- `move_army(from, to, count)` - Move army to adjacent tile
- `convert_pop(city, count)` - Convert population to army
- `move_capital(city)` - Move capital to larger city
- `yield_turn()` - End turn early

### Game Mechanics
- **Food**: Each population produces +2, consumes -1 (net +1)
- **Army**: Each army consumes -1 food (no production)
- **Combat**: Larger army wins, loses the smaller's count
- **Cities**: Produce food, house population
- **Fog**: Only see owned tiles and neighbors

## Writing Your Own Bot

```c
#include "ensi.h"

int main(void) {
    while (1) {
        uint8_t my_id = get_player_id();
        MapSize map = get_map_size();

        // Your strategy here

        yield_turn();
    }
    return 0;
}
```

Key constraints:
- No floating point (RV32IM only)
- No stdlib (bare metal)
- Per-turn instruction budget
- Must call `yield_turn()` or run out of budget
