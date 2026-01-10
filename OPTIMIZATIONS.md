# push_visibility_map Optimizations

Current status: `push_visibility_map` uses 65% of CPU in game execution.

## Baseline Performance
- 314-335 games/sec for 1000-turn 4-player games (single-threaded)
- 51% of physics limit

---

## Results Summary

| Optimization | Expected | Actual | Status |
|--------------|----------|--------|--------|
| Option 1: Inline visibility | 40-50% reduction | 144 games/sec (WORSE) | FAILED |
| Option 3: Persistent bitmap + fill | 15-20% reduction | 282 games/sec (WORSE) | FAILED |
| Generation counter | 15-20% reduction | 328 games/sec (WORSE) | FAILED |
| Option 8: Unsafe writes | 20-25% reduction | No improvement | FAILED |
| **Direct tile iteration** | N/A | **484-508 games/sec (+50%)** | **SUCCESS** |

---

## What Worked: Direct Tile Iteration

**Result: 484-508 games/sec (50% improvement)**

The `map.iter()` method was computing coordinates (div + mod) for every tile, even when not needed.

```rust
// Before: coord computed for every tile via div/mod
for (idx, (coord, tile)) in game_state.map.iter().enumerate() {
    // coord often unused
}

// After: iterate tiles directly, compute coord only when needed
let tiles = game_state.map.tiles();
for (idx, tile) in tiles.iter().enumerate() {
    if tile.owner == Some(player_id) {
        // Only compute coord for owned tiles
        let x = (idx % width_usize) as u16;
        let y = (idx / width_usize) as u16;
        let coord = Coord::new(x, y);
        // ...
    }
}
```

Key insight: In pass 1, we only need coords for owned tiles (~20-30% of map). In pass 2, we don't need coords at all.

---

## What Failed and Why

### Option 1: Inline Visibility (144 games/sec - WORSE)
Computing visibility inline requires 4-8 `map.get()` calls per fog tile. Since most tiles are fog, this was far more expensive than the bitmap lookup.

### Option 3: Persistent Bitmap with fill() (282 games/sec - WORSE)
`slice.fill(false)` is actually slower than `vec![false; n]` allocation. Modern allocators (jemalloc) have efficient same-size allocation caching, and `vec![false; n]` may get zeroed pages from the OS.

### Generation Counter (328 games/sec - WORSE)
Using `Vec<u32>` (4 bytes per tile) instead of `Vec<bool>` (1 byte per tile) increased memory bandwidth. The comparison `visibility_gen[idx] == cur_gen` is also slightly slower than `visible[idx]`.

### Option 8: Unsafe Writes (No improvement)
The compiler already optimizes `copy_from_slice` for small fixed-size copies. Explicit `ptr::write_unaligned` didn't help and added unsafe code complexity.

---

## Lessons Learned

1. **Profile before optimizing** - The bottleneck was coord computation in iteration, not allocation
2. **Modern allocators are fast** - `vec![value; n]` is highly optimized, don't try to avoid it
3. **Measure, don't assume** - Expected improvements can be wrong; always benchmark
4. **Simple wins** - Direct slice iteration beat complex allocation schemes

---

## Future Optimization Candidates

If more performance is needed:

1. **Option 5: Incremental visibility** - Only update changed tiles between turns
2. **Option 7: Sparse visibility** - Push only visible tiles, reduces memory bandwidth
3. **SIMD tile packing** - Process 8 tiles at once with AVX2

---

## Results Log

| Date | Optimization | Before | After | Improvement |
|------|--------------|--------|-------|-------------|
| 2026-01-10 | Baseline | 314-335/s | - | - |
| 2026-01-10 | Direct tile iteration | 314-335/s | 484-508/s | +50% |
| 2026-01-10 | Stats caching (eliminate dup compute) | 484-508/s | 621-650/s | +28% |
| 2026-01-10 | Economy merge | 621-650/s | 559-591/s | **-10% REJECTED** |
| 2026-01-10 | Dead resolve_combat removal | 621-650/s | 626-647/s | ~0% (cleaner) |
| 2026-01-10 | Linker caching | 626-647/s | 584-604/s | **-8% REJECTED** |
| 2026-01-10 | tiles() in compute_all_player_stats | 598-634/s | 405-499/s | **-25% REJECTED** |
| 2026-01-10 | Fast adjacency (Manhattan distance) | 598-634/s | 576-630/s | ~0% (neutral) |

---

## Current Status (After All Optimizations)

- **Performance: 610-632 games/sec** (80-83% of physics limit)
- **Total improvement: +90% from baseline** (314-335 → 610-632)

### What Worked (Round 1 + 2):
1. Direct tile iteration: +50% - avoiding div/mod for coord computation
2. Stats caching: +28% - eliminate duplicate `compute_all_player_stats` call per turn
3. Dead code removal: ~0% - removed no-op `resolve_combat` (cleaner, no perf change)

### What Failed (Round 1 + 2 + 3):
1. Inline visibility: -57% - neighbor lookups too expensive for fog tiles
2. Persistent bitmap + fill: -16% - fill() slower than vec! allocation
3. Generation counter: -2% - 4x memory bandwidth vs bool
4. Unsafe writes: 0% - compiler already optimizes copy_from_slice
5. **Economy merge: -10%** - merging iterations somehow regressed perf (cache effects?)
6. **Linker caching: -8%** - sharing Linker across bots caused regression
7. **tiles() instead of iter(): -25%** - counter-intuitive! Iterator with enumerate().map() optimizes better than direct slice iteration for `for (_, tile)` patterns
8. Fast adjacency (Manhattan): ~0% - no improvement, likely not in hot path

### Key Insight from Failures:
"Obvious" optimizations often backfire:
- Reusing buffers can be slower than fresh allocation (allocators are smart)
- Merging loops can hurt branch prediction or cache locality
- Sharing objects across threads/instances has hidden synchronization costs
- **Iterator abstractions can be faster than direct slice access** - LLVM optimizes iterator chains better than expected
