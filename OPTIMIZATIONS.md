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
