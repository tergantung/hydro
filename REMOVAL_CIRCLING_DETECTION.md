# Removal: Circling Detection

## Change Summary
Removed circling detection logic from automine to allow bot to be more persistent in reaching targets.

## Reason for Removal
Circling detection was too aggressive and caused issues:
- Bot gave up on valid targets too early
- Interfered with vertical mining (up/down)
- Caused bot to abandon targets that were actually reachable
- Created confusion when bot needed to navigate complex paths

## What Was Removed

### 1. Position History Tracking
```rust
// REMOVED:
let mut recent_positions: Vec<(i32, i32)> = Vec::new();
const MAX_POSITION_HISTORY: usize = 10;
```

### 2. Circling Detection Logic
```rust
// REMOVED:
recent_positions.push((player_x, player_y));
if recent_positions.len() > MAX_POSITION_HISTORY {
    recent_positions.remove(0);
}

if recent_positions.len() >= MAX_POSITION_HISTORY {
    let unique_positions: std::collections::HashSet<_> = recent_positions.iter().collect();
    if unique_positions.len() <= 3 {
        // Clear sticky target
        sticky_target = None;
        sticky_target_attempts = 0;
        recent_positions.clear();
    }
}
```

## Current Behavior

Bot now relies on:
1. **MAX_STICKY_ATTEMPTS** (20 attempts) - For collectables
2. **MAX_STICKY_ATTEMPTS * 3** (60 attempts) - For mining targets
3. **No path timeout** (20 attempts) - When no path found

These thresholds are more appropriate and don't interfere with normal mining operations.

## Benefits

1. ✅ Bot is more persistent on difficult targets
2. ✅ Better handling of vertical mining (up/down)
3. ✅ No false positives from complex pathfinding
4. ✅ Simpler code, easier to maintain
5. ✅ Bot will break blocks as needed to reach target

## Remaining Safety Mechanisms

Bot still has protection against getting stuck:
- **Max attempts timeout** - Clears target after 20-60 attempts
- **No path timeout** - Clears target if no path found after 20 attempts
- **Dead-end detection** - Tiles that fail to break after 12 HB attempts are marked as dead-end
- **Pathfinding** - A* algorithm ensures bot takes optimal path

## Testing

Test scenarios:
- ✅ Bot persists on targets that require complex paths
- ✅ Bot handles vertical mining without giving up
- ✅ Bot still clears targets after max attempts (20-60)
- ✅ Bot breaks blocks as needed to reach target
- ✅ Bot doesn't get permanently stuck

## Related Files
- `src/session/automine.rs` - Circling detection removed
- `BUGFIX_MINING_DISTANCE.md` - Current behavior documentation

## Notes
- Circling detection was removed completely
- Bot now relies on attempt-based timeouts only
- This allows bot to be more aggressive in breaking blocks to reach targets
- Bot will always try to break blocks that block the path to target
