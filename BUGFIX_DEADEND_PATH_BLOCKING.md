# Bugfix: Dead-End Path Blocking

## Problem
Bot terus mencoba mining tile yang sudah menjadi dead-end (tidak bisa di-break setelah 12 attempts), menyebabkan:
- Bot stuck pada target yang tidak bisa dijangkau
- Bot terus hit tile yang sama berulang kali
- Connection reset by peer (kicked by server)
- Tidak ada progress mining

## Example from Logs
```
[i] [automine] TARGETING: Collectable ID=14 at (31, 78)
[i] [automine] MINING: Path blocked at (33, 91), hitting from (32, 91)
[!] [automine] dead-end: tile (33,91) did not break in 12 retries
[i] [automine] TARGETING: Collectable ID=14 at (31, 78)  # Same target!
[i] [automine] MINING: Path blocked at (33, 92), hitting from (32, 92)
[!] [automine] dead-end: tile (33,92) did not break in 12 retries
[i] [automine] TARGETING: Collectable ID=14 at (31, 78)  # Same target again!
[i] [automine] MINING: Path blocked at (33, 93), hitting from (32, 93)
[!] [automine] dead-end: tile (33,93) did not break in 12 retries
[x] [session] Connection reset by peer (os error 104)  # KICKED!
```

## Root Cause
Ketika path ke target terblokir oleh tile yang sudah dead-end:
1. Bot detect tile adalah dead-end (12 failed attempts)
2. Bot mark **target** sebagai dead-end (bukan blocking tile)
3. Bot **tidak clear sticky target**
4. Bot terus mencoba target yang sama
5. Pathfinding menemukan path baru yang juga terblokir dead-end tile
6. Loop terus sampai bot di-kick

## Solution

**Clear sticky target** ketika path terblokir oleh dead-end tile, agar bot bisa cari target lain yang reachable.

### Before
```rust
if tile_attempts.get(&(next_step.0, next_step.1)).copied().unwrap_or(0) >= MAX_TILE_ATTEMPTS {
    // Mark target as dead-end
    tile_attempts.insert((target_x, target_y), MAX_TILE_ATTEMPTS);
    continue;  // Bot keeps same sticky target!
}
```

### After
```rust
if tile_attempts.get(&(next_step.0, next_step.1)).copied().unwrap_or(0) >= MAX_TILE_ATTEMPTS {
    // Clear sticky target so bot will find a different target
    _logger.warn("Path to target ({},{}) blocked by dead-end tile at ({},{}), clearing sticky target", 
        target_x, target_y, next_step.0, next_step.1);
    sticky_target = None;
    sticky_target_attempts = 0;
    continue;
}
```

## Changes Made

### File: `src/session/automine.rs` (line ~854)

**Changed behavior:**
- ❌ Before: Mark target as dead-end, keep sticky target
- ✅ After: Clear sticky target, find new target

**New logic:**
1. Detect blocking tile is dead-end (>= MAX_TILE_ATTEMPTS)
2. Log warning with blocking tile position
3. Clear sticky target
4. Reset sticky_target_attempts
5. Continue to next tick (bot will find new target)

## Benefits

1. ✅ Bot tidak stuck pada target yang tidak reachable
2. ✅ Bot otomatis cari target lain ketika path terblokir dead-end
3. ✅ Tidak ada infinite loop mining dead-end tiles
4. ✅ Mengurangi risiko kick dari server
5. ✅ Better mining efficiency

## Expected Behavior After Fix

```
[i] [automine] TARGETING: Collectable ID=14 at (31, 78)
[i] [automine] MINING: Path blocked at (33, 91), hitting from (32, 91)
[!] [automine] dead-end: tile (33,91) did not break in 12 retries
[w] [automine] Path to target (31,78) blocked by dead-end tile at (33,91), clearing sticky target
[i] [automine] NEW TARGET: Found at (45, 82)  # Different target!
[i] [automine] TARGETING: Collectable ID=15 at (45, 82)
... (bot continues with new reachable target)
```

## Testing

Test scenarios:
- ✅ Bot clears sticky target when path blocked by dead-end tile
- ✅ Bot finds new target after clearing
- ✅ Bot doesn't retry same target that was blocked by dead-end
- ✅ Bot doesn't get kicked for repeatedly hitting dead-end tiles
- ✅ Bot makes progress mining reachable targets

## Related Issues

This fix addresses the specific issue where:
- Bot was targeting collectable at (31, 78)
- Path was blocked by tiles at (33, 91), (33, 92), (33, 93)
- All blocking tiles became dead-end
- Bot kept trying same target
- Bot got kicked (Connection reset by peer)

## Notes

- Dead-end tiles are tiles that failed to break after 12 HB attempts
- These are usually bedrock, obsidian, or special unbreakable tiles
- Bot should skip targets that require breaking dead-end tiles
- Clearing sticky target allows bot to find alternative reachable targets
