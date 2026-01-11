# Ensi

A programming game combining resource management (in the tradition of the
classic Hammurabi) with territorial conquest (inspired by generals.io).

## Concept

Players submit programs that compete against each other. Each program acts as
an **ensi** (Sumerian for "city governor"), managing resources and expanding
territory in an ancient Mesopotamian setting.

The core constraint: programs have a **limited number of instructions per
turn**. Victory goes to those who optimize their decision-making within these
computational bounds.

## Status

Early development.

## Performance

The game engine achieves **610-632 games/sec per core** for 1000-turn, 4-player
games on a 64x64 map. This is approximately **80% of the theoretical physics
limit** (763 games/sec), where the physics limit represents the minimum time
needed to process game state updates without any bot execution overhead.

Tournament mode scales linearly across cores, achieving ~2800 games/sec on 16
cores for parallel game execution.

## License

MIT License. See [LICENSE](LICENSE) for details.
