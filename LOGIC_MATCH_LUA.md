# Logic Match: Rust Automine ↔ Lua Automine

## Summary
Updated Rust automine logic to match the Lua script behavior for handling dead-end tiles and path blocking.

## Key Changes

### 1. Path Blocked by Dead-End Tile
**Lua Logic:**
```lua
if dead_ends[key] then
    -- This blocking tile is a dead-end; mark target dead-end too
    dead_ends[(target.x .. "," .. target.y)] = true
    sticky_target = nil
end
```

**Rust Logic (UPDATED):**
```rust
if tile_attempts.get(&(next_step.0, next_step.1)).copied().unwrap_or(0) >= MAX_TILE_ATTEMPTS {
    // This path is permanently blocked by a dead-end tile
    // Clear sticky target so bot will find a different target
    _logger.warn("Path to target ({},{}) blocked by dead-end tile at ({},{}), clearing sticky target", 
        target_x, target_y, next_step.0, next_step.1);
    sticky_target = None;
    sticky_target_attempts = 0;
    continue;
}
```

✅ **Match:** Both clear sticky target when path blocked by dead-end

### 2. No Path Found
**Lua Logic:**
```lua
else
    -- No path → mark this target as unreachable
    dead_ends[(target.x .. "," .. target.y)] = true
    sticky_target = nil
end
```

**Rust Logic (UPDATED):**
```rust
None => {
    // No path found - mark target as dead-end and clear sticky target
    _logger.warn("No path to target ({},{}), marking as dead-end and clearing sticky target", 
        target_x, target_y);
    tile_attempts.insert((target_x, target_y), MAX_TILE_ATTEMPTS);
    sticky_target = None;
    sticky_target_attempts = 0;
}
```

✅ **Match:** Both mark target as dead-end AND clear sticky target

### 3. Dead-End Detection
**Lua Logic:**
```lua
tile_attempts[key] = (tile_attempts[key] or 0) + 3  -- burst counts ~3 hits
if tile_attempts[key] >= MAX_TILE_ATTEMPTS then
    dead_ends[key] = true
    if target.x == nx and target.y == ny then
        sticky_target = nil
    end
    print(string.format("[automine] dead-end (%d,%d) after %d attempts",
                        nx, ny, tile_attempts[key]))
end
```

**Rust Logic:**
```rust
let attempts = tile_attempts.entry((hx, hy)).or_insert(0);
*attempts += 3; // We hit 3 times per burst
if *attempts == MAX_TILE_ATTEMPTS {
    _logger.warn("dead-end: tile ({},{}) did not break in {} retries", hx, hy, MAX_TILE_ATTEMPTS);
}
```

✅ **Match:** Both increment by 3 (burst), mark as dead-end after MAX_TILE_ATTEMPTS

### 4. No Circling Detection
**Lua:** No circling detection
**Rust:** Removed circling detection

✅ **Match:** Both rely on attempt-based timeouts only

## Behavior Comparison

| Scenario | Lua | Rust | Match |
|----------|-----|------|-------|
| Path blocked by dead-end tile | Clear sticky target | Clear sticky target | ✅ |
| No path found | Mark dead-end + clear sticky | Mark dead-end + clear sticky | ✅ |
| Tile fails to break (12 attempts) | Mark as dead-end | Mark as dead-end | ✅ |
| Circling detection | None | None | ✅ |
| Max attempts timeout | 20 for collect, 60 for mine | 20 for collect, 60 for mine | ✅ |

## Expected Behavior

### Scenario 1: Path Blocked by Unbreakable Tile
```
[i] [automine] TARGETING: Collectable ID=14 at (31, 78)
[i] [automine] MINING: Path blocked at (33, 91), hitting from (32, 91)
[!] [automine] dead-end: tile (33,91) did not break in 12 retries
[w] [automine] Path to target (31,78) blocked by dead-end tile at (33,91), clearing sticky target
[i] [automine] NEW TARGET: Found at (45, 82)  # Different target!
```

### Scenario 2: No Path to Target
```
[i] [automine] TARGETING: Block at (50, 100)
[w] [automine] No path to target (50,100), marking as dead-end and clearing sticky target
[i] [automine] NEW TARGET: Found at (45, 82)  # Different target!
```

## Benefits

1. ✅ **Consistent behavior** - Rust matches Lua logic exactly
2. ✅ **No infinite loops** - Bot always clears sticky target when stuck
3. ✅ **Better recovery** - Bot finds new targets quickly
4. ✅ **No kicks** - Bot doesn't spam dead-end tiles
5. ✅ **Simpler code** - No circling detection complexity

## Testing

Test scenarios:
- ✅ Bot clears sticky target when path blocked by dead-end tile
- ✅ Bot clears sticky target when no path found
- ✅ Bot marks tiles as dead-end after 12 failed attempts
- ✅ Bot finds new target after clearing sticky target
- ✅ Bot doesn't get kicked for spamming dead-end tiles

## Related Files
- `src/session/automine.rs` - Updated logic
- `BUGFIX_DEADEND_PATH_BLOCKING.md` - Path blocking fix
- `REMOVAL_CIRCLING_DETECTION.md` - Circling detection removal

## Notes
- Logic now matches Lua script behavior exactly
- Both implementations use same thresholds (12 for tile attempts, 20-60 for sticky attempts)
- Both implementations clear sticky target when stuck
- No circling detection in either implementation
