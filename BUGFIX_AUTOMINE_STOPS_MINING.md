# Bugfix: Bot Berhenti Mining Setelah Berjalan Lama

## Masalah
Bot automine berhenti break block setelah berjalan beberapa waktu, meskipun masih ada gemstone/collectable di sekitar.

## Root Cause Analysis

### 1. **Tile Attempts Counter Terlalu Agresif**
- Setiap hit increment counter `+3` (karena burst mode)
- `MAX_TILE_ATTEMPTS = 12` berarti hanya 4 burst sebelum tile dianggap dead-end
- Dengan ping tinggi/lag, server bisa lambat kirim DB packet
- Bot sudah mark tile sebagai dead-end padahal tile sebenarnya bisa di-mine
- Tile yang di-mark dead-end diganti dengan bedrock (3993) di `masked_foreground`
- Setelah banyak tile jadi dead-end, bot tidak menemukan target lagi

### 2. **Pending Hits Memory Leak**
- `pending_hits` HashMap track tile yang sedang di-hit
- Tidak ada cleanup untuk entries yang sudah lama (>2 detik)
- Seiring waktu, HashMap membesar dan banyak tile dianggap "pending" padahal sudah lama
- Bot skip tile yang dianggap pending, semakin sedikit tile yang bisa di-hit

### 3. **Pickaxe Tidak Re-equip**
- Bot hanya equip pickaxe sekali di awal
- Kalau pickaxe unequipped karena alasan tertentu (bug, lag, dll), bot tidak re-equip
- Server ignore semua HB packet tanpa pickaxe equipped
- Bot terus jalan tapi tidak ada mining yang terjadi

### 4. **Bot Mencoba Mining Bedrock Layer (y = 2)** ⚠️ NEW
- Bedrock berada di `y = 2` dan tidak bisa dihancurkan
- Bot tidak punya filter untuk skip bedrock layer
- Bot terus mencoba hit bedrock sampai `MAX_TILE_ATTEMPTS` tercapai
- Tile di sekitar bedrock juga ikut ter-mark sebagai dead-end
- Setelah banyak tile di bedrock layer ter-mark, bot kehabisan target

## Solusi Implemented

### 1. Tile Attempts Counter Lebih Forgiving
```rust
// BEFORE: +3 per hit (4 burst = dead-end)
*attempts += 3;

// AFTER: +1 per hit (12 hit = dead-end)
*attempts += 1;
```
Ini memberikan lebih banyak kesempatan untuk tile dengan ping tinggi.

### 2. Cleanup Pending Hits
```rust
// Clean up old pending_hits entries (older than 2 seconds)
{
    let mut st = state.write().await;
    st.pending_hits.retain(|_, last_hit| {
        last_hit.elapsed() < Duration::from_secs(2)
    });
}
```
Cleanup dilakukan setiap tick untuk mencegah memory leak.

### 3. Periodic Pickaxe Re-equip
```rust
// Re-check every 30 seconds
let mut last_pickaxe_check = Instant::now();

if equipped_pickaxe.is_none() || last_pickaxe_check.elapsed() > Duration::from_secs(30) {
    if let Some(pickaxe_id) = find_best_pickaxe(&inventory) {
        let _ = send_doc(outbound_tx, protocol::make_wear_item(pickaxe_id as i32)).await;
        equipped_pickaxe = Some(pickaxe_id);
        last_pickaxe_check = Instant::now();
    }
}
```
Bot sekarang re-check dan re-equip pickaxe setiap 30 detik.

### 4. Skip Bedrock Layer dan Indestructible Blocks ⭐ NEW
```rust
/// Is this an indestructible block (bedrock, lava, boundaries)?
pub fn is_indestructible(block_id: u16) -> bool {
    matches!(block_id, 3993 | 4103 | 3 | 3987 | 3988 | 3990)
}
```

**Target Selection:**
- Skip `y <= 3` (bedrock layer + safety margin) saat scan gemstone
- Skip indestructible blocks dari target selection

**Mining Safety Guards:**
- Sebelum hit tile, check apakah `y <= 3` → skip dan mark as dead-end
- Sebelum hit tile, check apakah indestructible block → skip dan mark as dead-end
- Berlaku untuk semua mining modes: stationary hit, path blocking, shortcut mining

## Testing Recommendations

1. **Test dengan ping tinggi** (>200ms) untuk memastikan tile attempts tidak terlalu cepat habis
2. **Test jangka panjang** (>30 menit) untuk memastikan tidak ada memory leak
3. **Test di dekat bedrock layer** (y = 2-5) untuk memastikan bot tidak stuck mining bedrock
4. **Monitor log** untuk melihat:
   - Berapa banyak tile yang di-mark dead-end
   - Apakah ada warning "STUCK: Target at bedrock layer"
   - Apakah pickaxe di-re-equip dengan benar
   - Apakah pending_hits cleanup berjalan

## Expected Behavior After Fix

- Bot bisa mining lebih lama tanpa kehabisan target
- Lebih toleran terhadap lag/ping tinggi
- Tidak ada memory leak dari pending_hits
- Pickaxe selalu equipped meskipun ada bug yang unequip
- **Bot tidak pernah mencoba mining bedrock atau indestructible blocks**
- **Bot tidak stuck di dekat bedrock layer**

## Files Modified

- `src/session/automine.rs`:
  - Line ~20: Added `is_indestructible()` helper function
  - Line ~95: Skip `y <= 3` in gemstone target selection
  - Line ~98: Skip indestructible blocks in target selection
  - Line ~385: Added pending_hits cleanup
  - Line ~320: Added last_pickaxe_check tracking
  - Line ~340: Added periodic pickaxe re-equip
  - Line ~900: Added bedrock/indestructible safety guards for path blocking mining
  - Line ~1020: Added bedrock/indestructible safety guards for stationary hit #1
  - Line ~1080: Added bedrock/indestructible safety guards for stationary hit #2
  - Line ~745: Added bedrock/indestructible skip in shortcut mining
  - Line ~1044: Changed tile_attempts increment from +3 to +1

## Date
2026-05-09

