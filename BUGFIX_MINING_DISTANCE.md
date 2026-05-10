# Bugfix: Improved Sticky Target Management

## Problem
Bot circling atau stuck pada target yang tidak bisa dijangkau, menyebabkan:
- Bot berputar-putar di posisi yang sama
- Bot terus mencoba target yang sama berulang kali
- Recovery lambat dari situasi stuck
- Efisiensi mining rendah

## Root Cause
Bot tidak memiliki mekanisme untuk clear sticky target ketika stuck, sehingga terus mencoba target yang sama meskipun tidak bisa dijangkau.

## Solution

Menambahkan logika untuk **clear sticky target** (tanpa mark sebagai dead-end) agar bot bisa mencoba target lain atau retry target yang sama nanti ketika:

1. **Circling detection** - Clear target ketika bot circling
2. **Max attempts reached** - Clear target setelah terlalu banyak attempts
3. **No path timeout** - Clear target jika tidak ada path setelah banyak attempts

**Penting:** Tidak mark sebagai dead-end agar bot bisa retry target tersebut nanti jika pathfinding membaik setelah mining block lain.

### Clear Logic (Without Dead-End Marking)
```rust
// Just clear sticky target, don't mark as dead-end
// This allows bot to try the target again later if pathfinding improves
sticky_target = None;
sticky_target_attempts = 0;
```

## Changes Made

### File: `src/session/automine.rs`

#### 1. Increased MAX_STICKY_ATTEMPTS (line ~206)

**Before:**
```rust
const MAX_STICKY_ATTEMPTS: u32 = 8;
```

**After:**
```rust
const MAX_STICKY_ATTEMPTS: u32 = 20; // Increased to allow more attempts before giving up
```

#### 2. Circling Detection (line ~270)

**Behavior:**
- Detects when bot circles between 3 or fewer positions in last 10 moves
- Clears sticky target WITHOUT marking as dead-end
- Allows bot to try the target again later

```rust
if unique_positions.len() <= 3 {
    _logger.warn("CIRCLING DETECTED: clearing sticky target at ({},{})", tx, ty);
    sticky_target = None;  // Just clear, don't mark as dead-end
    sticky_target_attempts = 0;
    recent_positions.clear();
}
```

#### 3. Max Attempts Timeout (line ~570 & ~590)

**For Collectables (20 attempts):**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS {
    _logger.warn("STUCK: Collectable not collected after {} attempts, clearing", ...);
    sticky_target = None;  // Just clear, don't mark as dead-end
}
```

**For Mining (60 attempts = 3x collectables):**
```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS * 3 {
    _logger.warn("STUCK: Mining target not reached after {} attempts, clearing", ...);
    sticky_target = None;  // Just clear, don't mark as dead-end
}
```

#### 4. No Path Timeout (line ~650)

**Behavior:**
- Waits for 20 attempts before clearing
- Does NOT mark as dead-end
- Allows retry later when pathfinding improves

```rust
if sticky_target_attempts > MAX_STICKY_ATTEMPTS {
    _logger.warn("No path to sticky target after {} attempts, clearing", ...);
    sticky_target = None;  // Just clear, don't mark as dead-end
}
```

## Benefits

1. **More persistent** - Bot tries harder before giving up (20-60 attempts instead of 4-16)
2. **Better for vertical mining** - Bot doesn't give up too early when mining up/down
3. **Allows retry** - Targets are not marked as dead-end, so bot can try again later
4. **Handles complex paths** - More attempts means bot can handle longer mining sequences
5. **Clearer logging** - Shows when and why sticky target is cleared

## Impact

### Before (Aggressive Dead-End Marking)
- Bot gives up too quickly (4-16 attempts) ❌
- Targets marked as dead-end permanently ❌
- Bot can't retry targets even after mining nearby blocks ❌
- Problems with vertical mining (up/down) ❌

### After (Patient Retry Logic)
- Bot tries harder before giving up (20-60 attempts) ✅
- Targets NOT marked as dead-end ✅
- Bot can retry targets after mining nearby blocks ✅
- Better handling of vertical mining ✅
- More natural mining behavior ✅

## Thresholds

| Scenario | Attempts Before Clear | Notes |
|----------|----------------------|-------|
| Circling Detection | Immediate | When only 3 positions in 10 moves |
| Collectable Timeout | 20 attempts | MAX_STICKY_ATTEMPTS |
| Mining Timeout | 60 attempts | MAX_STICKY_ATTEMPTS * 3 |
| No Path Timeout | 20 attempts | MAX_STICKY_ATTEMPTS |

## Logging Examples

### Circling Detected
```
[w] [automine] CIRCLING DETECTED: Only 2 unique positions in last 10 moves, clearing sticky target at (45,67)
[i] [automine] NEW TARGET: Found at (52,71)
```

### Max Attempts Reached (Collectable)
```
[w] [automine] STUCK: Collectable ID=12345 at (45,67) not collected after 20 attempts, clearing sticky target
[i] [automine] NEW TARGET: Found at (52,71)
```

### Max Attempts Reached (Mining)
```
[w] [automine] STUCK: Mining target at (45,67) not reached after 60 attempts, clearing sticky target
[i] [automine] NEW TARGET: Found at (52,71)
```

### No Path Timeout
```
[w] [automine] No path to sticky target at (45,67) after 20 attempts, clearing
[i] [automine] NEW TARGET: Found at (52,71)
```

## Testing

Test scenarios:
- ✅ Bot tries 20 attempts before clearing collectable target
- ✅ Bot tries 60 attempts before clearing mining target
- ✅ Bot clears target when circling detected
- ✅ Bot can retry same target later (not marked as dead-end)
- ✅ Bot handles vertical mining (up/down) better
- ✅ Bot finds new target after clearing old target

## Performance

- **Memory**: No change (no dead-end marking)
- **CPU**: Negligible
- **Mining Speed**: Improved (more persistent on difficult targets)
- **Recovery Time**: Balanced (not too fast, not too slow)

## Related Files
- `src/session/automine.rs` - Main changes
- `BUGFIX_CIRCLING.md` - Original circling detection doc

## Notes
- Targets are NOT marked as dead-end (can be retried later)
- Bot will still attempt to mine blocks regardless of distance (pathfinding handles movement)
- Circling detection threshold: 3 or fewer unique positions in last 10 moves
- Max attempts: 20 for collectables, 60 for mining, 20 for no-path timeout
- Dead-end marking only happens when tile fails to break after 12 HB attempts (MAX_TILE_ATTEMPTS)
