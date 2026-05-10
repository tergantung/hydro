# Quick Start - PAATCHMEDEV Automine v2.0

## ✅ Perubahan Selesai

Automine PAATCHMEDEV sudah berhasil di-update dengan fitur-fitur terbaik dari someonecheat!

## 🚀 Apa yang Berubah?

### Speed
- **3x lebih cepat**: 280ms per tick (dari 850ms)
- **Lebih konsisten**: Jitter 0-80ms (dari 0-350ms)
- **No pause**: Thinking pause dihapus

### Features
- ✅ Auto-reconnect saat disconnect
- ✅ Safety grab untuk collectables
- ✅ Cooldown system (3s per item)
- ✅ Lebih banyak gemstone IDs (19 vs 10)
- ✅ st() packet untuk reliability
- ✅ Better logging untuk debug

### Removed
- ❌ Falling detection (menyebabkan stuck)
- ❌ Thinking pause (tidak perlu)

## 🎮 Cara Menggunakan

### 1. Build
```bash
cd /home/jeli/mycheat/PAATCHMEDEV
cargo build --release
```

### 2. Run
```bash
./target/release/Hydro
```

### 3. Start Automine
- Buka dashboard
- Connect session
- Klik "Start Automine"
- Bot akan otomatis ke MINEWORLD dan mulai mining

## ⚙️ Tuning (Optional)

### Jika Bot Terlalu Cepat (Kena Kick ER=7)
Edit `src/session/automine.rs` line ~200:
```rust
let base_delay = 350; // Increase dari 280
let jitter = rng.random_range(0..100); // Increase dari 80
```

### Jika Bot Terlalu Lambat
Edit `src/session/automine.rs` line ~200:
```rust
let base_delay = 250; // Decrease dari 280
let jitter = rng.random_range(0..50); // Decrease dari 80
```

### Jika High Ping (>200ms)
Timing sudah auto-adjust, tapi bisa tweak:
```rust
if ping > 150 {
    base_delay + (ping - 50) + jitter // Lebih aggressive
}
```

## 🐛 Troubleshooting

### Bot Stuck Tidak Bergerak
Check log untuk:
- `NO TARGET`: Tidak ada gemstone/collectable → Move ke area lain
- `STUCK`: Pathfinding issue → Restart automine
- `Waiting for DB`: Normal, tunggu server response

### Bot Disconnect Terus
- Check network connection
- Verify credentials
- Check rate limiting
- Auto-reconnect akan retry setiap 15s

### Bot Kena Kick
- **ER=7 (Speed-hack)**: Increase base_delay
- **ER=1 (Teleport)**: Kemungkinan network issue
- **Other**: Check logs untuk detail

## 📊 Performance

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Tick Speed | 850ms | 280ms | 3x faster |
| Gemstones | 10 IDs | 19 IDs | 90% more |
| Reliability | Manual reconnect | Auto-reconnect | Much better |
| Collection | Miss items | Safety grab | Better |

## 📝 Files Changed

1. `src/session/automine.rs` - Main logic
2. `src/session/bot_session.rs` - Controller integration
3. `src/session/state.rs` - Cooldown method

## 📚 Documentation

- `AUTOMINE_IMPROVEMENTS.md` - Detailed feature list
- `MIGRATION_SUMMARY.md` - Complete migration notes
- `AUTOMINE_DEBUG_GUIDE.md` - Troubleshooting guide
- `CHANGELOG.md` - Version history

## ⚠️ Known Issues

1. **No Falling Detection**: Dihapus karena menyebabkan stuck. Bot mungkin lebih rentan saat jatuh dari ketinggian.

2. **First Run Slower**: Bot perlu learn dead-end tiles, akan lebih cepat setelah beberapa menit.

3. **High Ping**: Jika ping >300ms, bot mungkin perlu tuning manual.

## 🎯 Best Practices

1. **Start di MINEWORLD**: Bot akan auto-join, tapi lebih cepat jika sudah di sana
2. **Equip Pickaxe**: Bot akan auto-equip, tapi pastikan ada di inventory
3. **Monitor First 5 Minutes**: Check logs untuk ensure tidak ada kick
4. **Adjust Timing**: Setiap server/network berbeda, tune sesuai kebutuhan

## 🔥 Tips

- **Faster Mining**: Decrease base_delay (risk: kick)
- **Safer Mining**: Increase base_delay (slower tapi aman)
- **Better Collection**: Bot sudah optimal dengan safety grab
- **Long Sessions**: Auto-reconnect handle disconnect otomatis

## 📞 Support

Jika ada masalah:
1. Check `AUTOMINE_DEBUG_GUIDE.md`
2. Review logs (last 100 lines)
3. Note: session status, world, position
4. Try adjust timing
5. Report dengan detail lengkap

---

**Status**: ✅ Ready to Use  
**Version**: 2.0.0  
**Performance**: ~3x faster  
**Stability**: Good (needs testing)

**Selamat mining! 🎉**
