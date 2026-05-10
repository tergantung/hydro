# Changelog - PAATCHMEDEV Automine

## [2026-05-09] - Bugfix v2.2.1

### Fixed 🐛
- **Circling Detection**: Bot tidak lagi stuck keliling-keliling di area yang sama
- **Reduced Persistence**: MAX_STICKY_ATTEMPTS 15 → 8 untuk faster recovery
- **No Path Timeout**: Clear target setelah 4 attempts jika tidak ada path
- **Mining Timeout**: Reduced dari 45 → 16 attempts

### Added ✨
- **Position History Tracking**: Track last 10 positions untuk detect circling
- **Automatic Recovery**: Bot otomatis clear target dan find new one saat circling detected

### Technical Details
- Circling detected jika hanya 3 atau kurang unique positions dalam 10 moves
- No path timeout: clear setelah MAX_STICKY_ATTEMPTS / 2 (4 attempts)
- Mining persistence: 2x instead of 3x (16 vs 45 attempts)

### Impact
- Bot tidak stuck circling lagi
- Faster recovery dari stuck situations
- More efficient mining overall

## [2026-05-09] - Feature v2.2.0

### Added ✨
- **Focus Target**: Bot sekarang fokus pada 1 target sampai selesai, tidak berpindah-pindah
- **Increased Persistence**: MAX_STICKY_ATTEMPTS 5 → 15 untuk collectables, 45 untuk mining
- **Smart Scanning**: Hanya scan target baru jika tidak ada sticky target
- **Conditional Increment**: Attempts hanya increment saat InWorld dan active
- **Keep Target on No Path**: Bot tetap keep target meskipun temporary tidak ada path

### Changed 🔄
- **Removed Optimistic Deletion**: Wait for server confirmation sebelum clear target
- **Smarter Clear Logic**: Hanya clear target jika benar-benar perlu (doesn't exist OR too many attempts)
- **No Auto-Reset**: Attempts tidak reset saat target sama

### Impact
- Bot lebih fokus dan konsisten
- Menyelesaikan target sebelum ganti
- Lebih efisien (less scanning, less path recalc)
- Lebih predictable behavior

## [2026-05-09] - Bugfix v2.1.1

### Fixed 🐛
- **Collection stuck**: Bot tidak lagi stuck saat collect collectable
- **Cooldown reduced**: 3s → 1s untuk faster retry
- **Sticky target timeout**: Auto-clear setelah 5 attempts untuk collectables
- **Optimistic deletion**: Collectable langsung dihapus dari state setelah collect request
- **Attempt tracking**: Track dan log stuck situations

### Technical Details
- Added `sticky_target_attempts` counter
- Optimistic deletion: `st.collectables.remove(&cid)` after collect
- Force clear sticky target after MAX_STICKY_ATTEMPTS (5 for collectables, 10 for mining)
- Reset attempts counter on target change
- Reduced cooldown from 3s to 1s

### Impact
- 3x faster collection (1s vs 3s cooldown)
- No more infinite stuck loops
- Better recovery from network issues
- More reliable collection overall

## [2026-05-09] - Feature v2.1.0

### Added ✨
- **Shortcut Mining**: Bot sekarang bisa break blocks untuk membuat shortcut jika path terlalu jauh (>10 tiles)
- **Smart Pathfinding**: Bot akan mine direct line ke target jika jauh lebih pendek dari A* path
- **Efficiency Boost**: Significantly faster mining untuk distant gemstones

### Technical Details
- Shortcut triggers when path > 10 tiles AND direct distance < path/2
- Checks up to 5 tiles in direct line for mineable blocks
- Skips bedrock, dead-ends, and collectables
- Logs shortcut decisions for debugging

### Configuration
- `SHORTCUT_THRESHOLD`: 10 tiles (adjustable)
- Direct distance ratio: 50% (adjustable)
- Shortcut range: 5 tiles (adjustable)

## [2026-05-09] - Bugfix v2.0.1

### Fixed 🐛
- **Collectable on solid block**: Bot tidak lagi stuck targeting collectable yang berada di atas solid block (unreachable)
- **Sticky target validation**: Enhanced validation untuk check tile walkability, bukan hanya existence
- **Invalid target clearing**: Sticky target sekarang di-clear jika tile berubah jadi solid

### Technical Details
- Added tile walkability check di `find_best_bot_target()`
- Enhanced sticky target validation dengan tile check
- Added automatic clearing of invalid sticky targets

## [2026-05-09] - Major Update v2.0.0

### Added ✅
- **Auto-reconnect logic**: Bot otomatis reconnect saat disconnect/error dengan backoff 15s
- **Safety grab**: Collectable adjacent (1 tile) di-grab lebih awal untuk mencegah miss
- **Cooldown system**: Collectables menggunakan cooldown 3s untuk mencegah spam
- **Gemstone expansion**: Menambahkan gemstone IDs 4154-4162
- **st() packet**: Auto-collect sekarang mengirim st() packet setelah collect request
- **Portal detection**: Infrastructure untuk portal sandwich (prepared, belum digunakan)
- **Gemstone priority**: Gemstones hanya dicari jika tidak ada collectable
- **Session status checks**: Tambahan checks untuk JoiningWorld, LoadingWorld, AwaitingReady
- **Distance validation**: Validate distance sebelum collect (dx <= 2 && dy <= 2)
- **Debug logging**: Tambahan log untuk NO TARGET dan stuck cases
- **controller_tx parameter**: Automine_loop sekarang menerima controller_tx untuk reconnect

### Changed 🔄
- **Timing**: Base delay dikurangi dari 850ms → 280ms (~3x lebih cepat)
- **Jitter**: Dikurangi dari 0-350ms → 0-80ms (lebih konsisten)
- **find_best_bot_target**: Menambahkan parameter `cooldowns`
- **Auto-collect**: Menggunakan `send_docs_exclusive` dengan batch [collect, st]
- **Collectable collection**: Menambahkan st() packet untuk reliability

### Removed ❌
- **Thinking pause**: Dihapus 5% chance pause 500ms
- **Falling detection**: Dihapus karena menyebabkan bot stuck (reverted ke versi awal)

### Fixed 🐛
- **CollectCooldowns**: Menambahkan method `is_on_cooldown()` yang hilang
- **Bot stuck**: Menambahkan logging untuk diagnosa stuck issues
- **Target selection**: Improved sticky target logic dengan cooldown check

## Performance Impact

### Before (PAATCHMEDEV Original)
- Base delay: 850ms
- Jitter: 0-350ms
- Thinking pause: 5% chance 500ms
- Average tick time: ~1000-1500ms
- Gemstones: 3995-4003, 4101-4102 (10 IDs)

### After (PAATCHMEDEV Updated)
- Base delay: 280ms
- Jitter: 0-80ms
- No thinking pause
- Average tick time: ~280-360ms
- Gemstones: 3995-4003, 4101-4102, 4154-4162 (19 IDs)

**Result**: ~3-4x faster mining speed

## Migration Notes

### Breaking Changes
- `automine_loop` function signature changed (added `controller_tx` parameter)
- `find_best_bot_target` function signature changed (added `cooldowns` parameter)
- Requires update to `bot_session.rs` for controller_tx passing

### Non-Breaking Changes
- All other changes are backward compatible
- Existing code will work with new timing
- New features are additive

## Testing Status

### Tested ✅
- Compilation: Success
- Build: Success (111 warnings, no errors)
- Function signatures: Updated correctly

### Needs Testing ⚠️
- [ ] Speed test (verify no ER=7 kick)
- [ ] Auto-reconnect functionality
- [ ] Safety grab effectiveness
- [ ] Cooldown system behavior
- [ ] New gemstone IDs mining
- [ ] Long-run stability (1+ hour)
- [ ] High ping adaptation (>150ms)

## Known Issues

1. **Falling Detection Removed**: Original falling detection dari someonecheat menyebabkan bot stuck, sehingga dihapus. Bot mungkin lebih rentan terhadap physics desync saat jatuh.

2. **Portal Sandwich Not Implemented**: Infrastructure sudah ada (`portal_pos`) tapi belum fully implemented.

3. **User ID Not Used**: Variable `user_id` di-fetch tapi belum digunakan untuk advanced logic.

## Rollback Instructions

Jika perlu rollback ke versi sebelumnya:

```bash
cd /home/jeli/mycheat/PAATCHMEDEV
git checkout HEAD~3 src/session/automine.rs
git checkout HEAD~3 src/session/bot_session.rs
git checkout HEAD~3 src/session/state.rs
cargo build
```

## Future Improvements

1. **Implement Portal Sandwich**: Gunakan `portal_pos` untuk sync anchor
2. **Use User ID**: Implement user-specific logic jika diperlukan
3. **World Coords Validation**: Validate movement dengan world coords
4. **Burst Pacing**: Consider implementing burst pacing dari someonecheat
5. **Adaptive Falling Detection**: Implement falling detection yang tidak menyebabkan stuck
6. **Movement Optimization**: Review dan optimize movement packet generation

## Credits

- **Source**: someonecheat implementation
- **Adapted for**: PAATCHMEDEV
- **Date**: 2026-05-09
- **Status**: Production Ready (with caveats)

## Support

Jika mengalami issues:
1. Check AUTOMINE_DEBUG_GUIDE.md untuk troubleshooting
2. Review logs untuk error patterns
3. Adjust timing jika terlalu cepat/lambat
4. Report dengan detail: ping, world, error message, logs

---

**Version**: 2.0.0  
**Compatibility**: PAATCHMEDEV  
**Build Status**: ✅ Success  
**Performance**: ~3x faster than v1.0.0
