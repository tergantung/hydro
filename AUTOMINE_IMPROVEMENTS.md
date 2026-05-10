# Automine Improvements - Ported from someonecheat

## Perubahan yang Diterapkan

### 1. **Timing yang Lebih Cepat dan Responsif**
- **Base delay**: Dikurangi dari 850ms menjadi 280ms per tick
- **Jitter**: Dikurangi dari 0-350ms menjadi 0-80ms
- **Thinking pause**: Dihapus (sebelumnya 5% chance untuk pause 500ms)
- **Falling physics**: ❌ TIDAK DIGUNAKAN (reverted ke versi awal PAATCHMEDEV)
- **Ping adaptation**: Tetap ada, menambahkan (ping - 100)ms jika ping > 150ms

**Dampak**: Bot bergerak ~3x lebih cepat, lebih mirip kecepatan player asli (~250ms/tile)

**Note**: Falling detection dari someonecheat dihapus karena menyebabkan bot stuck

### 2. **Auto-Reconnect Logic**
- Bot sekarang otomatis reconnect saat disconnect/error
- Backoff timer 15 detik antara reconnect attempts
- Tetap exit jika status Idle (user stop manual)
- Menambahkan parameter `controller_tx` ke automine_loop untuk mengirim command reconnect

**Dampak**: Bot tidak perlu di-restart manual setelah disconnect

### 3. **Falling Physics Detection**
- ❌ **REMOVED** - Falling detection dari someonecheat dihapus
- Menyebabkan bot stuck tidak bergerak
- Kembali ke behavior awal PAATCHMEDEV (no falling detection)

**Dampak**: Bot tidak stuck di falling state

### 4. **Gemstone Range yang Lebih Luas**
- Menambahkan gemstone IDs: 4154-4162
- Sebelumnya hanya: 3995-4003, 4101-4102
- Sekarang: 3995-4003, 4101-4102, 4154-4162

**Dampak**: Bot bisa mine lebih banyak jenis gemstone

### 5. **Cooldown System untuk Collectables**
- `find_best_bot_target` sekarang menerima parameter `cooldowns`
- Skip collectables yang sedang on cooldown
- Mencegah spam collect request untuk item yang sama

**Dampak**: Mengurangi packet spam, lebih efisien

### 6. **Safety Grab Logic**
- Saat collectable berjarak 1 tile (adjacent), kirim grab request lebih awal
- Check cooldown sebelum safety grab
- Mencegah miss collect saat bot bergerak cepat

**Dampak**: Lebih sedikit item yang terlewat

### 7. **Auto-Collect dengan st() Packet**
- Setiap collect request sekarang diikuti dengan `make_st()` packet
- Dikirim sebagai batch exclusive: `[make_collectable_request, make_st]`
- Matches behavior dari client asli

**Dampak**: Lebih reliable collection, mengurangi chance server ignore request

### 8. **Portal Sandwich Logic (Prepared)**
- Menambahkan portal position detection (tile 110 atau 1419)
- Variable `portal_pos` sudah tersedia untuk future use
- Saat ini hanya logged dengan `let _ = portal_pos;`

**Dampak**: Infrastructure ready untuk sync anchor logic jika diperlukan

### 9. **Gemstone Priority Logic**
- Gemstones hanya dicari jika TIDAK ada collectable
- Collectables selalu prioritas pertama
- Mencegah bot ignore drops saat mining

**Dampak**: Lebih efisien dalam mengumpulkan drops

### 10. **Additional Session Status Checks**
- Menambahkan check untuk: JoiningWorld, LoadingWorld, AwaitingReady
- Skip mining saat status ini untuk mencegah packet loss
- Server silently drops HB packets saat loading

**Dampak**: Mengurangi false dead-end tiles, lebih reliable pathfinding

### 11. **User ID and World Coords Logging**
- Menambahkan `user_id` fetch sebelum mining (prepared for future use)
- Menambahkan world coords calculation dengan `map_to_world`
- Saat ini hanya logged dengan `let _ = wx; let _ = wy;`

**Dampak**: Infrastructure ready untuk advanced packet logic

### 12. **Collectable Distance Validation**
- Saat path.len() == 2, validate distance sebelum collect
- Hanya collect jika dx <= 2 && dy <= 2
- Log warning jika stuck (no path to collectable)

**Dampak**: Mengurangi invalid collect attempts

## File yang Dimodifikasi

1. **src/session/automine.rs**
   - Function signature: `automine_loop` → tambah parameter `controller_tx`
   - Function signature: `find_best_bot_target` → tambah parameter `cooldowns`
   - Timing logic: base_delay, jitter, falling detection
   - Auto-reconnect logic
   - Safety grab logic
   - Portal detection
   - Gemstone range expansion
   - Auto-collect dengan st() packet

2. **src/session/bot_session.rs**
   - Pemanggilan `automine_loop`: tambah parameter `controller_tx`

## Testing Recommendations

1. **Speed Test**: Verify bot tidak kena speed-hack kick (KErr ER=7)
2. **Fall Test**: Drop bot dari ketinggian, pastikan tidak desync
3. **Reconnect Test**: Disconnect bot, verify auto-reconnect works
4. **Collection Test**: Verify semua drops terkumpul dengan baik
5. **Gemstone Test**: Verify bot bisa mine gemstone IDs baru (4154-4162)
6. **Long Run Test**: Run bot 1+ jam, verify stability

## Known Issues / Future Work

- Portal sandwich logic belum fully implemented (infrastructure ready)
- User ID dan world coords belum digunakan (infrastructure ready)
- Perlu testing untuk optimal timing di berbagai ping levels

## Compatibility

- Backward compatible dengan PAATCHMEDEV existing code
- Requires `controller_tx` parameter di bot_session.rs
- Semua perubahan non-breaking kecuali function signatures

## Performance Impact

- **CPU**: Minimal increase (falling detection check)
- **Network**: Slight increase (st() packets, safety grabs)
- **Memory**: Negligible (portal_pos variable)
- **Speed**: ~3x faster mining/movement

## Credits

Ported from someonecheat implementation with improvements and optimizations.
