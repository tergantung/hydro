# Changelog: Dead-End Marking Enhancement

## Version: 2024-01-XX

### Summary
Enhanced automine logic to automatically mark unreachable targets as dead-end, forcing bot to find new targets instead of circling or getting stuck.

### Changes

#### 1. Circling Detection Enhancement
**Before:**
```rust
if unique_positions.len() <= 3 {
    sticky_target = None;  // Just clear, bot might pick same target again
}
```

**After:**
```rust
if unique_positions.len() <= 3 {
    // Mark current target as dead-end
    tile_attempts.insert((tx, ty), MAX_TILE_ATTEMPTS);
    sticky_target = None;  // Bot will find NEW target
}
```

#### 2. Max Attempts Enhancement
**Before:**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS {
    sticky_target = None;  // Just clear
}
```

**After:**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS {
    tile_attempts.insert((x, y), MAX_TILE_ATTEMPTS);  // Mark as dead-end
    sticky_target = None;  // Force new target
}
```

#### 3. No Path Timeout Enhancement
**Before:**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS / 2 {
    sticky_target = None;  // Just clear
}
```

**After:**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS / 2 {
    tile_attempts.insert((tx, ty), MAX_TILE_ATTEMPTS);  // Mark as dead-end
    sticky_target = None;  // Force new target
}
```

#### 4. Distance Validation Enhancement
**New:**
```rust
if mine_dist > 2 {
    tile_attempts.insert((target_x, target_y), MAX_TILE_ATTEMPTS);  // Mark as dead-end
    continue;  // Skip this target
}
```

### Impact

#### Before
- Bot clears sticky target but might pick the same target again
- Bot circles between same positions repeatedly
- Bot wastes time on unreachable targets
- Slow recovery from stuck situations

#### After
- Bot marks unreachable targets as dead-end
- Bot automatically finds NEW targets when stuck
- No more circling on same target
- Fast recovery from stuck situations
- Better mining efficiency

### Logging Examples

#### Circling Detected
```
[w] [automine] CIRCLING DETECTED: Only 2 unique positions in last 10 moves, marking target (45,67) as dead-end and finding new target
[i] [automine] NEW TARGET: Found at (52,71)
```

#### Max Attempts Reached
```
[w] [automine] STUCK: Collectable ID=12345 at (45,67) not collected after 8 attempts, marking as dead-end
[i] [automine] NEW TARGET: Found at (52,71)
```

#### No Path Timeout
```
[w] [automine] No path to sticky target at (45,67) after 4 attempts, marking as dead-end
[i] [automine] NEW TARGET: Found at (52,71)
```

#### Distance Too Far
```
[w] [automine] SKIP MINING: Target (45,67) too far from player (42,64), distance=6
[i] [automine] NEW TARGET: Found at (43,65)
```

### Performance

- **Memory**: Minimal increase (one HashMap entry per dead-end tile)
- **CPU**: Negligible (one HashMap insert per stuck situation)
- **Mining Speed**: Improved (no time wasted on unreachable targets)
- **Recovery Time**: Faster (immediate target switch instead of repeated attempts)

### Testing Checklist

- [x] Bot marks target as dead-end when circling detected
- [x] Bot marks target as dead-end after max attempts
- [x] Bot marks target as dead-end when no path found repeatedly
- [x] Bot marks target as dead-end when distance too far
- [x] Bot finds new target after marking old target as dead-end
- [x] Bot doesn't retry dead-end targets
- [x] Dead-end tiles are cleared when entering new world
- [x] Logging shows dead-end marking and new target selection

### Related Files
- `src/session/automine.rs` - Main changes
- `BUGFIX_MINING_DISTANCE.md` - Full documentation
- `BUGFIX_CIRCLING.md` - Original circling detection doc

### Notes
- Dead-end marking is world-specific (cleared on world change)
- Dead-end tiles are also cleared when server confirms destruction (DB packet)
- Shortcut mining doesn't mark as dead-end (normal path might still work)
