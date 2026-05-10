# Automine Debug Guide - Bot Stuck Troubleshooting

## Gejala: Bot Tidak Bergerak

Jika bot stuck dan tidak bergerak, check log messages untuk mendiagnosa masalahnya.

## Log Messages & Diagnosis

### 1. **"FALLING: Waiting for landing at (x, y)"**
**Penyebab**: Bot mendeteksi sedang jatuh (current tile dan below tile keduanya walkable)

**Solusi**:
- Bot akan otomatis resume setelah landing
- Jika stuck terus, kemungkinan tile detection salah
- Check apakah `world_foreground_tiles` sudah loaded dengan benar

**Fix**:
```rust
// Jika falling detection terlalu sensitif, bisa disable sementara:
// Comment out bagian falling detection di automine.rs line ~240
```

### 2. **"NO TARGET: No gemstones or collectables found near (x, y)"**
**Penyebab**: Tidak ada gemstone atau collectable dalam radius 60 tiles

**Solusi**:
- Pastikan bot sudah di MINEWORLD
- Check apakah ada gemstones di sekitar bot
- Gemstone IDs yang dicari: 3995-4003, 4101-4102, 4154-4162
- Check apakah `world_foreground_tiles` sudah loaded

**Fix**:
```rust
// Tambahkan lebih banyak gemstone IDs jika perlu
// Edit is_minegem() function di automine.rs
```

### 3. **"STUCK: A* suggested hitting current tile"**
**Penyebab**: Pathfinding error, A* menyarankan hit tile yang sedang ditempati

**Solusi**:
- Bot akan skip dan cari target lain
- Jika terjadi terus, ada bug di pathfinding
- Check tile_attempts counter

### 4. **"STUCK: Target is player tile"**
**Penyebab**: Target mining adalah tile yang sedang ditempati player

**Solusi**:
- Bot akan skip dan cari target lain
- Kemungkinan gemstone ada di tile player

### 5. **"STUCK: No path to collectable {id}"**
**Penyebab**: Collectable terlihat tapi tidak ada path yang valid

**Solusi**:
- Collectable mungkin di balik wall
- A* tidak bisa menemukan path
- Bot akan cari target lain

### 6. **"dead-end: tile (x,y) did not break in N retries"**
**Penyebab**: Tile sudah di-hit MAX_TILE_ATTEMPTS kali tapi tidak pecah

**Solusi**:
- Tile mungkin bedrock atau non-mineable
- Server tidak mengirim DB (Destroy Block) packet
- Tile akan di-mask sebagai bedrock untuk pathfinding

### 7. **Tidak ada log sama sekali**
**Penyebab**: Bot mungkin stuck di status check atau waiting

**Check**:
- Session status (Connecting, Authenticating, Redirecting, JoiningWorld, LoadingWorld, AwaitingReady)
- World width == 0 (world data belum loaded)
- is_in_mine == false (belum di MINEWORLD)

## Common Issues & Fixes

### Issue 1: Bot Stuck di "Waiting for world data"
**Symptoms**: world_width == 0, bot kirim idle movement packet

**Fix**:
```bash
# Check log untuk:
# - "World data not loaded yet"
# - Session status: LoadingWorld atau AwaitingReady

# Solution: Wait atau restart session
```

### Issue 2: Bot Stuck di Falling Detection
**Symptoms**: Log terus menerus "FALLING: Waiting for landing"

**Root Cause**: Tile detection salah, bot pikir sedang jatuh padahal tidak

**Fix**:
```rust
// Option 1: Disable falling detection sementara
// Di automine.rs, comment out:
if is_falling {
    _logger.info("automine", Some(&_session_id), format!("FALLING: Waiting for landing at ({}, {})", player_x, player_y));
    continue;
}

// Option 2: Perbaiki tile detection
// Check apakah world_foreground_tiles index calculation benar
```

### Issue 3: Bot Tidak Menemukan Target
**Symptoms**: Log "NO TARGET: No gemstones or collectables found"

**Root Cause**: 
- Tidak ada gemstone dalam radius 60 tiles
- Gemstone IDs tidak match
- Foreground tiles belum loaded

**Fix**:
```rust
// Option 1: Expand gemstone IDs
pub fn is_minegem(block_id: u16) -> bool {
    // Add more IDs here
    (block_id >= 3995 && block_id <= 4003) 
    || (block_id >= 4101 && block_id <= 4102) 
    || (block_id >= 4154 && block_id <= 4162)
    || (block_id >= YOUR_NEW_RANGE_START && block_id <= YOUR_NEW_RANGE_END)
}

// Option 2: Increase search radius
let search_radius = 100; // dari 60
```

### Issue 4: Bot Stuck di Auto-Reconnect Loop
**Symptoms**: Log "auto-rejoin: session disconnected, requesting reconnect"

**Root Cause**: Session terus disconnect

**Fix**:
```bash
# Check:
# - Network connection
# - Server status
# - Auth credentials
# - Rate limiting

# Temporary fix: Increase backoff timer
# Di automine.rs, change:
Duration::from_secs(15) -> Duration::from_secs(30)
```

### Issue 5: Bot Terlalu Lambat
**Symptoms**: Bot bergerak tapi sangat lambat

**Root Cause**: Timing terlalu konservatif

**Fix**:
```rust
// Di automine.rs, adjust timing:
let base_delay = 280; // Decrease untuk lebih cepat (min ~200ms)
let jitter = rng.random_range(0..80); // Decrease untuk lebih konsisten
```

### Issue 6: Bot Kena Speed-Hack Kick (KErr ER=7)
**Symptoms**: Bot disconnect dengan error ER=7

**Root Cause**: Timing terlalu cepat

**Fix**:
```rust
// Di automine.rs, increase timing:
let base_delay = 350; // Increase dari 280ms
let jitter = rng.random_range(0..100); // Increase jitter
```

## Debug Checklist

Saat bot stuck, check dalam urutan ini:

1. ✅ **Session Status**
   ```
   Check: session_status == SessionStatus::Connected
   Location: Log atau dashboard
   ```

2. ✅ **World Loaded**
   ```
   Check: world_width > 0
   Check: current_world == "MINEWORLD"
   Location: Log "World data not loaded yet"
   ```

3. ✅ **Player Position**
   ```
   Check: player_x, player_y valid
   Check: Not falling (is_falling == false)
   Location: Log "FALLING: Waiting for landing"
   ```

4. ✅ **Target Found**
   ```
   Check: target != None
   Location: Log "NO TARGET" atau "TARGETING"
   ```

5. ✅ **Pathfinding**
   ```
   Check: path exists to target
   Check: No dead-end tiles blocking
   Location: Log "STUCK: No path"
   ```

6. ✅ **Pickaxe Equipped**
   ```
   Check: equipped_pickaxe != None
   Location: Log "no pickaxe in inventory"
   ```

7. ✅ **No Rate Limiting**
   ```
   Check: rate_limit_until == None atau expired
   Location: Check state
   ```

## Enable Verbose Logging

Untuk debug lebih detail, tambahkan logging di key points:

```rust
// Di automine_loop, tambahkan:

// After session status check:
_logger.info("automine", Some(&_session_id), 
    format!("STATUS: {:?}, world_width: {}, in_mine: {}", 
    session_status, world_width, is_in_mine));

// After target selection:
_logger.info("automine", Some(&_session_id), 
    format!("TARGET: {:?}, sticky: {:?}", target, sticky_target));

// After pathfinding:
if let Some((_, path)) = &target {
    _logger.info("automine", Some(&_session_id), 
        format!("PATH: len={}, next={:?}", path.len(), path.get(1)));
}
```

## Performance Tuning

Jika bot berjalan tapi perlu optimization:

### Faster Mining
```rust
let base_delay = 250; // Faster (risk: speed-hack kick)
let jitter = rng.random_range(0..50); // Less variance
```

### Safer Mining
```rust
let base_delay = 350; // Slower but safer
let jitter = rng.random_range(0..100); // More human-like
```

### High Ping Adaptation
```rust
// Already implemented:
if ping > 150 {
    base_delay + (ping - 100) + jitter
}

// For very high ping (>300ms):
if ping > 300 {
    base_delay + (ping - 50) + jitter // More aggressive compensation
}
```

## Testing Commands

```bash
# Build with debug info
cargo build

# Run with logging
RUST_LOG=debug cargo run

# Check specific session logs
tail -f logs/session_XXXXX.log | grep "automine"

# Monitor for stuck patterns
tail -f logs/session_XXXXX.log | grep -E "(STUCK|FALLING|NO TARGET)"
```

## Emergency Fixes

### Quick Disable Falling Detection
```rust
// Line ~240 in automine.rs
// if is_falling {
//     _logger.info("automine", Some(&_session_id), format!("FALLING: Waiting for landing at ({}, {})", player_x, player_y));
//     continue;
// }
```

### Quick Disable Auto-Reconnect
```rust
// Line ~285 in automine.rs
// if matches!(session_status, SessionStatus::Disconnected | SessionStatus::Error) {
//     ...
//     continue;
// }
```

### Force Target Search Every Tick
```rust
// Line ~530 in automine.rs
// Comment out sticky target logic:
// if let Some(st_target) = sticky_target.clone() {
//     ...
// }
```

## Contact & Support

Jika masalah persist setelah troubleshooting:
1. Collect logs (last 100 lines sebelum stuck)
2. Note session status, world name, player position
3. Check if reproducible
4. Report dengan detail lengkap

---

**Remember**: Automine improvements membuat bot 3x lebih cepat, tapi juga lebih sensitif terhadap network issues dan server behavior. Adjust timing sesuai kondisi network Anda.
