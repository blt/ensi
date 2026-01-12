# Ensi

A programming game combining resource management (in the tradition of the
classic Hammurabi) with territorial conquest (inspired by generals.io).

## Overview

Players submit WASM programs that compete against each other. Each program acts
as an **ensi** (Sumerian for "city governor"), managing resources and expanding
territory in an ancient Mesopotamian setting.

The core constraint: programs have a **limited number of instructions per
turn**. Victory goes to those who optimize their decision-making within these
computational bounds.

## Game Mechanics

### Map & Tiles

- **64x64 grid** with three tile types:
  - **City** - Has population, produces food, can convert population to army
  - **Desert** - Passable terrain, no special properties
  - **Mountain** - Impassable barrier

### Resources

- **Population** - Grows in cities based on food availability
- **Food** - Produced by cities adjacent to other owned cities (adjacency bonus)
- **Army** - Military units converted from population, used for conquest

### Actions

Each turn, bots can:
- **Move** army between adjacent tiles
- **Convert** population to army in owned cities
- **Move Capital** to a larger city (strategic repositioning)
- **Yield** to end turn early and save compute budget

### Victory Conditions

- **Domination** - Eliminate all other players (capture their capitals)
- **Turn Limit** - After 1000 turns, player with most territory wins

## Installation

```bash
# Clone and build
git clone https://github.com/troutwine/ensi
cd ensi
cargo build --release

# The binary is at ./target/release/ensi
```

## Quick Start

### Run a Game

```bash
# Run a game between two bots
ensi run bot1.wasm bot2.wasm

# Watch a game in the terminal UI
ensi watch bot1.wasm bot2.wasm --speed 200

# Run a tournament (1000 games, parallel)
ensi tournament bot1.wasm bot2.wasm -g 1000 --progress
```

### Evolve Bots

```bash
# Start evolution (runs until Ctrl+C)
ensi evolve -v

# Run for specific generations
ensi evolve -g 100 -v

# Resume from checkpoint
ensi evolve -r evolution/gen_00050.bin -v

# Limit CPU usage
ensi evolve -j 4 -v
```

### Manage Evolved Bots

```bash
# List evolved bots
ensi bots --list

# Examine a bot's genome
ensi bots --examine ~/.ensi/evolved/evolved_20260112.wasm

# Or use the example tool for detailed analysis
cargo run --example examine_genome -- evolution/best.wasm
```

## CLI Reference

### `ensi run`

Run a single game between 2-8 bots.

```bash
ensi run [OPTIONS] <BOTS>...

Options:
  -s, --seed <SEED>     Random seed for reproducibility
  -t, --turns <TURNS>   Maximum turns (default: 1000)
  -f, --format <FMT>    Output: text, json, or llm
      --save <FILE>     Save replay to file
  -q, --quiet           Suppress turn-by-turn output
```

### `ensi watch`

Interactive terminal UI for watching games.

```bash
ensi watch [OPTIONS] <BOTS>...

Options:
  -s, --seed <SEED>      Random seed
  --speed <MS>           Turn delay in milliseconds (default: 500)
  -p, --player <N>       View from player N's perspective (fog of war)
```

### `ensi tournament`

Run mass parallel games for statistical analysis.

```bash
ensi tournament [OPTIONS] <BOTS>...

Options:
  -g, --games <N>        Number of games (default: 1000)
  -j, --threads <N>      Parallel threads (default: all CPUs)
  -f, --format <FMT>     Output: text, json, or csv
  -p, --progress         Show progress bar
```

### `ensi evolve`

Evolve bots using genetic programming.

```bash
ensi evolve [OPTIONS]

Options:
  -o, --output <DIR>        Output directory (default: evolution)
  -p, --population <N>      Population size (default: 100)
  -g, --generations <N>     Generations to run (0 = forever)
  --games <N>               Games per fitness eval (default: 20)
  -s, --seed <SEED>         Random seed
  -r, --resume <FILE>       Resume from checkpoint
  -j, --jobs <N>            Parallel threads
  -v, --verbose             Show progress
```

## Writing Bots

Bots are WASM modules that export a `run_turn` function. The SDK (`sdk/ensi.h`)
provides a C interface, but any language that compiles to WASM32 works.

### C Example

```c
#include "ensi.h"

int run_turn(int fuel_budget) {
    int player_id = ensi_get_player_id();
    int width = ensi_get_map_width();
    int height = ensi_get_map_height();

    // Scan map using push-based visibility (fast!)
    for (int y = 0; y < height; y++) {
        for (int x = 0; x < width; x++) {
            int tile = ensi_tile_map_get(x, y);

            if (!TILE_VISIBLE(tile)) continue;
            if (!TILE_OWNED_BY(tile, player_id)) continue;

            int army = TILE_ARMY(tile);
            if (army > 1) {
                // Move army to adjacent tile
                ensi_move(x, y, x+1, y, army - 1);
            }
        }
    }

    ensi_yield();
    return 0;
}
```

### Build Command

```bash
clang --target=wasm32-unknown-unknown -O3 -nostdlib \
      -Wl,--no-entry -Wl,--export=run_turn \
      -o bot.wasm bot.c
```

### SDK Functions

**Query Functions** (read game state):
- `ensi_get_turn()` - Current turn number
- `ensi_get_player_id()` - Your player ID (1-8)
- `ensi_get_tile(x, y)` - Tile info (slow syscall)
- `ensi_tile_map_get(x, y)` - Tile info (fast, ~100x faster)
- `ensi_get_my_food()` - Food balance
- `ensi_get_my_population()` - Total population
- `ensi_get_my_army()` - Total army
- `ensi_get_my_capital()` - Capital coordinates

**Action Functions** (issue commands):
- `ensi_move(fx, fy, tx, ty, count)` - Move army
- `ensi_convert(x, y, count)` - Convert population to army
- `ensi_move_capital(x, y)` - Relocate capital
- `ensi_yield()` - End turn early

### Tile Info Encoding

Tile data is packed into a 32-bit integer:
- Bits 0-7: Tile type (0=city, 1=desert, 2=mountain, 255=fog)
- Bits 8-15: Owner player ID (0=neutral, 255=fog)
- Bits 16-31: Army count

Use macros to unpack: `TILE_TYPE(t)`, `TILE_OWNER(t)`, `TILE_ARMY(t)`

## Genetic Programming

The `ensi evolve` command uses genetic programming to evolve bot strategies.
Evolved bots use a rule-based genome that compiles to WASM.

### Genome Structure

Each genome contains:
- **16 constants** - Evolved integer values
- **8 registers** - In-turn persistent state
- **Rules** - Condition-action pairs evaluated per tile

### Rule System

```
Rule: IF <condition> THEN <action>

Conditions use expressions:
  - Constants, registers, game state queries
  - Arithmetic: +, -, *, /, %
  - Comparison: >, <, ==
  - Logic: &&, ||, !

Actions:
  - Move(from, to, count)
  - Convert(city, count)
  - MoveCapital(city)
  - Store(reg, value)     # Write to register
  - Repeat(count, action) # Bounded loop (max 16 iterations)
```

### Turing Completeness

The genome supports:
- **Registers** - 8 persistent i32 values per turn
- **Bounded loops** - Repeat actions up to 16 times
- **Conditionals** - Rule conditions control execution

This enables evolution of planning algorithms, not just reactive strategies.

### Evolution Output

```
evolution/
├── gen_00000.bin    # Population checkpoints
├── gen_00010.bin
├── ...
└── best.wasm        # Best bot compiled to WASM

~/.ensi/evolved/
├── evolved_20260112_151124.wasm  # Timestamped copies
└── ...
```

## Performance

The game engine achieves **610-632 games/sec per core** for 1000-turn, 4-player
games on a 64x64 map. This is approximately **80% of the theoretical physics
limit** (763 games/sec), where the physics limit represents the minimum time
needed to process game state updates without any bot execution overhead.

Tournament mode scales linearly across cores, achieving ~2800 games/sec on 16
cores for parallel game execution.

### Optimization Tips for Bots

1. **Use push-based tile map** - `ensi_tile_map_get()` is ~100x faster than `ensi_get_tile()`
2. **Yield early** - Call `ensi_yield()` when done to save fuel
3. **Batch operations** - Process all tiles in one pass rather than random access
4. **Minimize syscalls** - Cache values like `player_id` and `map_width`

## Project Structure

```
ensi/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── game/            # Game mechanics & physics
│   ├── wasm/            # WASM runtime (wasmtime)
│   ├── tournament/      # Parallel game execution
│   ├── replay/          # Game recording/playback
│   └── gp/              # Genetic programming
│       ├── genome.rs    # Bot representation
│       ├── compiler.rs  # Genome → WASM compiler
│       ├── mutation.rs  # Mutation operators
│       ├── crossover.rs # Crossover operators
│       ├── selection.rs # Tournament selection
│       ├── fitness.rs   # Fitness evaluation
│       └── evolution.rs # Main evolution loop
├── sdk/
│   ├── ensi.h           # C SDK header
│   └── example_bot.c    # Example bot implementation
├── examples/
│   └── examine_genome.rs # Genome analysis tool
└── tests/               # Integration tests
```

## License

MIT License. See [LICENSE](LICENSE) for details.
