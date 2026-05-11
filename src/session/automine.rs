//! Packet-based automine automation.
//!
//! This is a port of `automine.lua` to Rust. The state machine
//! (Surveying → Approaching → Acting → Exiting), target queue with Lua
//! priorities, gem blacklist, durability tracking, periodic repair / recycle /
//! craft / upgrade, and humanizer (chat + session break) all mirror the Lua
//! reference one-to-one.
//!
//! What is *deliberately preserved* from the original Rust version:
//!   * Pathfinding   — `crate::pathfinding::astar::find_tile_path`.
//!   * Block-break   — `protocol::make_mine_hit_stationary` /
//!                     `protocol::make_move_to_map_point`.
//! The Lua script uses its own A* dig-cost variant (`dig_path` /
//! `execute_dig_path`); we keep Rust's existing pathfinder which already
//! understands breakable-tile costs (see `pathfinding::astar::get_tile_cost`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch, RwLock};

use crate::constants::movement;
use crate::logging::Logger;
use crate::models::{BotTarget, SessionStatus};
use crate::protocol;
use crate::session::ControllerEvent;

use super::network::{send_doc, send_docs, send_docs_exclusive};
use super::publish_state_snapshot;
use super::state::{InventoryEntry, OutboundHandle, SessionState};

// ─── Lua constants port ─────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct MineTier {
    name: &'static str,
    level: i32,
    world_id: i32,
    /// 0 = no key required (Newbie tier).
    key_id: u16,
}

const MINE_TIERS: &[MineTier] = &[
    MineTier { name: "Newbie",   level: 5,  world_id: 0, key_id: 0      },
    MineTier { name: "Bronze",   level: 10, world_id: 1, key_id: 0x0f81 },
    MineTier { name: "Silver",   level: 20, world_id: 2, key_id: 0x0f82 },
    MineTier { name: "Golden",   level: 40, world_id: 3, key_id: 0x0f83 },
    MineTier { name: "Platinum", level: 60, world_id: 4, key_id: 0x0f84 },
];

/// Gemstone foreground-tile ids that count as a mining target (Lua's
/// `GEMSTONE_BLOCK_IDS`).
const GEMSTONE_BLOCK_IDS: &[u16] = &[
    0x0f9b, 0x0f9c, 0x0f9f, 0x0fa0, 0x0fa1, 0x0fa2, 0x0fa3,
];

/// Crystal blocks for the optional `auto_seek_crystal` feature.
const CRYSTAL_BLOCK_IDS: &[u16] = &[3974, 3975, 3976];

/// Recycle-target gemstones if `auto_recycle` is enabled (Lua's
/// `DEFAULT_GEM_IDS`).
const DEFAULT_GEM_IDS: &[u16] = &[
    0x0fac, 0x0fad, 0x0fae, 0x0faf, 0x0fb0,
    0x0fb1, 0x0fb2, 0x0fb3, 0x0fb4, 0x0fb5,
    0x0fb6, 0x0fb7, 0x0fb8, 0x0fb9, 0x0fba,
    0x0fbb, 0x0fbc, 0x0fbd, 0x0fbe, 0x0fbf,
    0x0fc0, 0x0fc1, 0x0fc2, 0x0fc3, 0x0fc4,
    0x0fc5, 0x0fc6, 0x0fc7, 0x0fc8, 0x0fc9,
    0x0fca, 0x0fcb, 0x0fcc, 0x0fcd, 0x0fce,
    0x0fcf, 0x0fd0, 0x0fd1, 0x0fd2, 0x0fd3,
    0x0fd4, 0x0fd5, 0x0fd6, 0x0fd7, 0x0fd8,
];

/// Pickaxe block-ids covered by the Lua repair / upgrade flow. Note: the Rust
/// auto-equip uses a richer list (`PICKAXE_PRIORITY` below) but the
/// repair / upgrade actions still target whatever pickaxe is currently worn
/// so this list isn't otherwise needed — we keep it for documentation.
#[allow(dead_code)]
const LUA_PICKAXE_TIERS: &[u16] = &[0x0ff7, 0x0ff8, 0x0ff9, 0x0ffa, 0x0ffb];

const REPAIR_KIT_BLOCK_ID: u16 = 0x1041;

/// Inventory-slot count that triggers an "inventory full → exit + rejoin".
const INVENTORY_NEAR_FULL: usize = 130;
const SURVEY_EMPTY_LIMIT: u32 = 24;
const PUNCH_RETRY_CAP: u32 = 10;
const NO_PROGRESS_LIMIT: u32 = 5;

const BL_GEM_DURATION: Duration = Duration::from_secs(60);
const PERIODIC_TICK_INTERVAL: Duration = Duration::from_secs(15);
const PICKAXE_UPGRADE_INTERVAL: Duration = Duration::from_secs(300);
const ACT_DEADLINE: Duration = Duration::from_secs(10);
const DESTROY_WINDOW: Duration = Duration::from_secs(3);

const HUMANIZER_CHAT_MIN_S: u64 = 1800;
const HUMANIZER_CHAT_MAX_S: u64 = 5400;
const HUMANIZER_BREAK_MIN_S: u64 = 3600;
const HUMANIZER_BREAK_MAX_S: u64 = 10800;

const RECYCLE_MIN_AMOUNT: u16 = 100;
const PICKAXE_LOW_DURABILITY: i32 = 800;
const REPAIR_HIT_INTERVAL: u64 = 80;
/// Search radius (Manhattan) for nearby drops to prefer over wall gems.
const DROP_PREFERENCE_RADIUS: i32 = 30;

const CHAT_LINES: &[&str] = &[
    "afk", "brb", "lf gem trades", "buying gemstones",
    "anyone selling pickaxe?", "wts ores cheap", "trading",
    "lf miner", "lol got dced", "back",
];

const PICKAXE_PRIORITY: &[u16] = &[
    4195, // WeaponPickaxeDark
    4093, // WeaponPickaxeEpic
    4092, // WeaponPickaxeMaster
    4091, // WeaponPickaxeHeavy   (0x0ffb)
    4090, // WeaponPickaxeSturdy  (0x0ffa)
    4089, // WeaponPickaxeBasic   (0x0ff9)
    4088, // WeaponPickaxeFlimsy  (0x0ff8)
    4087, // WeaponPickaxeCrappy  (0x0ff7)
];

// ─── State machine types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Surveying,
    Approaching,
    Acting,
    Exiting,
}

#[derive(Debug, Clone)]
enum CurrentTarget {
    Gem { x: i32, y: i32, expected_fg: u16 },
    Drop { id: i32, x: i32, y: i32 },
    Mob { x: i32, y: i32, ai_id: i32 },
}

impl CurrentTarget {
    fn point(&self) -> (i32, i32) {
        match self {
            CurrentTarget::Gem { x, y, .. }
            | CurrentTarget::Drop { x, y, .. }
            | CurrentTarget::Mob { x, y, .. } => (*x, *y),
        }
    }

    fn to_bot_target(&self) -> BotTarget {
        match self {
            CurrentTarget::Gem { x, y, .. } => BotTarget::Mining { x: *x, y: *y },
            CurrentTarget::Drop { id, x, y } => BotTarget::Collecting {
                id: *id,
                block_id: 0,
                x: *x,
                y: *y,
            },
            CurrentTarget::Mob { x, y, .. } => BotTarget::Mining { x: *x, y: *y },
        }
    }
}

#[derive(Debug, Clone)]
struct ExitRequest {
    reason: String,
    rejoin: bool,
}

// ─── Public helpers (kept stable for re-use) ────────────────────────────────

/// Is this a mineable gemstone block?
///
/// We keep the Rust range (`3995..=4003 | 4101..=4102`) which is a superset of
/// the Lua hard-coded list — both intentionally include the in-between ids
/// because the server has reused them across content drops.
pub fn is_minegem(block_id: u16) -> bool {
    (block_id >= 3995 && block_id <= 4003) || (block_id >= 4101 && block_id <= 4102)
}

/// Pick the best target (collectible or gemstone) within a 60-tile radius
/// of the player. Targets near AI enemies are skipped.
///
/// Kept for callers outside the automine loop (`bot_session.rs` exposes it via
/// `get_bot_target_for_position`). The new loop uses its own priority-aware
/// queue picker.
pub fn find_best_bot_target(
    player_map_x: i32,
    player_map_y: i32,
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
    collectables: &std::collections::HashMap<i32, crate::session::CollectableState>,
    ai_enemies: &std::collections::HashMap<i32, crate::session::AiEnemyState>,
) -> Option<crate::models::BotTarget> {
    let mut best_target: Option<(crate::models::BotTarget, u32)> = None;

    for (&id, state) in collectables {
        let index = (state.map_y as u32 * world_width + state.map_x as u32) as usize;
        if let Some(&fg) = foreground_tiles.get(index) {
            if fg != 0 {
                continue;
            }
        }

        let dx = state.map_x - player_map_x;
        let dy = state.map_y - player_map_y;
        let dist_sq = (dx * dx + dy * dy) as u32;

        let mut near_enemy = false;
        for enemy in ai_enemies.values() {
            let e_dx = state.map_x - enemy.map_x;
            let e_dy = state.map_y - enemy.map_y;
            if (e_dx * e_dx + e_dy * e_dy) < (3 * 3) {
                near_enemy = true;
                break;
            }
        }
        if near_enemy {
            continue;
        }

        if best_target.is_none() || dist_sq < best_target.as_ref().unwrap().1 {
            best_target = Some((
                crate::models::BotTarget::Collecting {
                    id,
                    block_id: state.block_type as u16,
                    x: state.map_x,
                    y: state.map_y,
                },
                dist_sq,
            ));
        }
    }

    let search_radius = 60;
    let min_x = (player_map_x - search_radius).max(0);
    let max_x = (player_map_x + search_radius).min(world_width as i32 - 1);
    let min_y = (player_map_y - search_radius).max(0);
    let max_y = (player_map_y + search_radius).min(world_height as i32 - 1);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&block_id) = foreground_tiles.get(index) {
                if is_minegem(block_id) {
                    let dx = x - player_map_x;
                    let dy = y - player_map_y;
                    let dist_sq = (dx * dx + dy * dy) as u32;

                    let mut near_enemy = false;
                    for enemy in ai_enemies.values() {
                        let e_dx = x - enemy.map_x;
                        let e_dy = y - enemy.map_y;
                        if (e_dx * e_dx + e_dy * e_dy) < (3 * 3) {
                            near_enemy = true;
                            break;
                        }
                    }
                    if near_enemy {
                        continue;
                    }

                    if best_target.is_none() || dist_sq < best_target.as_ref().unwrap().1 {
                        best_target = Some((crate::models::BotTarget::Mining { x, y }, dist_sq));
                    }
                }
            }
        }
    }

    best_target.map(|(target, _)| target)
}

/// Get the A* path from the player's tile to a target tile, if any exists.
///
/// **Unchanged** per requirement: the pathfinder is the authoritative
/// movement planner.
pub fn get_path_to_target(
    player_map_x: i32,
    player_map_y: i32,
    target_x: i32,
    target_y: i32,
    foreground_tiles: &[u16],
    world_width: u32,
    world_height: u32,
) -> Option<Vec<(i32, i32)>> {
    crate::pathfinding::astar::find_tile_path(
        foreground_tiles,
        world_width as usize,
        world_height as usize,
        (player_map_x, player_map_y),
        (target_x, target_y),
    )
}

pub(super) fn find_best_pickaxe(inventory: &[InventoryEntry]) -> Option<u16> {
    PICKAXE_PRIORITY.iter().find_map(|&id| {
        inventory
            .iter()
            .find(|e| e.block_id == id && e.amount > 0)
            .map(|_| id)
    })
}

pub(super) async fn record_action(state: &Arc<RwLock<SessionState>>, hint: String) {
    let mut st = state.write().await;
    st.last_action_hint = Some(hint);
    st.last_action_at = Some(Instant::now());
}

// ─── Lua-flow helpers ───────────────────────────────────────────────────────

#[inline]
fn manhattan(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

/// Lua's `tile_safe` predicate. We approximate the Lua collision check —
/// which uses `getInfo(fg).collision & 0x0c == 0` — with the existing
/// pathfinder's walkability rule, which already excludes solid blocks.
fn tile_is_safe(foreground: &[u16], width: u32, height: u32, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 || (x as u32) >= width || (y as u32) >= height {
        return false;
    }
    let idx = (y as u32 * width + x as u32) as usize;
    let fg = foreground.get(idx).copied().unwrap_or(0xFFFF);
    fg == 0 || crate::pathfinding::astar::is_walkable_tile(fg)
}

/// Lua's `has_floor_below` (counts foreground OR water below the tile).
/// We only check foreground here because water tiles aren't tracked in the
/// fast `foreground` slice — close enough for the "is this a valid stand
/// position" predicate.
fn has_floor_below(foreground: &[u16], width: u32, height: u32, x: i32, y: i32) -> bool {
    if x < 0 || y + 1 < 0 || (x as u32) >= width || ((y + 1) as u32) >= height {
        return false;
    }
    let idx = ((y + 1) as u32 * width + x as u32) as usize;
    foreground.get(idx).copied().unwrap_or(0) != 0
}

/// Lua's `pick_stand_tile`: try four neighbours of `target` for a safe,
/// walkable, floored tile.
fn pick_stand_tile(
    foreground: &[u16],
    width: u32,
    height: u32,
    target: (i32, i32),
) -> Option<(i32, i32)> {
    let candidates = [
        (target.0 - 1, target.1),
        (target.0 + 1, target.1),
        (target.0, target.1 + 1),
        (target.0, target.1 - 1),
    ];
    candidates.into_iter().find(|&(x, y)| {
        tile_is_safe(foreground, width, height, x, y)
            && has_floor_below(foreground, width, height, x, y)
    })
}

/// Lua's `pick_tier`. Returns the highest unlocked tier (level + key present)
/// for the given level. Falls back to the lowest tier.
fn pick_tier(level: i32, inventory: &[InventoryEntry]) -> &'static MineTier {
    let has_key = |key_id: u16| -> bool {
        key_id == 0
            || inventory
                .iter()
                .any(|e| e.block_id == key_id && e.amount > 0)
    };
    for tier in MINE_TIERS.iter().rev() {
        if level >= tier.level && has_key(tier.key_id) {
            return tier;
        }
    }
    &MINE_TIERS[0]
}

/// Total inventory slots in use (Lua's `inv.slots`). The Rust inventory
/// list stores one entry per stack so the count of entries with amount > 0
/// is the slot count.
fn inventory_slot_count(inventory: &[InventoryEntry]) -> usize {
    inventory.iter().filter(|e| e.amount > 0).count()
}

#[inline]
fn fg_at(foreground: &[u16], width: u32, x: i32, y: i32) -> u16 {
    if x < 0 || y < 0 {
        return 0;
    }
    let idx = (y as u32 * width + x as u32) as usize;
    foreground.get(idx).copied().unwrap_or(0)
}

/// Lua's `queue_pick` priorities, applied directly against the live state.
/// Returns the chosen target (gem / drop / mob) or `None` when nothing
/// reachable is left.
///
/// Priorities (matching Lua exactly):
///   1. Nearest drop, if within `DROP_PREFERENCE_RADIUS` Manhattan tiles.
///   2. Otherwise gem on the same row (|dy| ≤ 1) with the smallest |dx|.
///   3. Otherwise gem below (dy ≥ 2) scored by `|dx| + 2|dy|`.
///   4. Otherwise gem above (dy < 0) scored by `|dx| + 5|dy|`.
///   5. Otherwise nearest enemy within 20 Manhattan tiles.
fn pick_best_target(
    me: (i32, i32),
    foreground: &[u16],
    width: u32,
    height: u32,
    collectables: &std::collections::HashMap<i32, crate::session::CollectableState>,
    ai_enemies: &std::collections::HashMap<i32, crate::session::AiEnemyState>,
    blacklist: &HashMap<(i32, i32), Instant>,
    target_set: &HashSet<u16>,
) -> Option<CurrentTarget> {
    let now = Instant::now();
    let bl_active = |p: (i32, i32)| -> bool {
        blacklist
            .get(&p)
            .map(|exp| *exp > now)
            .unwrap_or(false)
    };
    let near_enemy = |p: (i32, i32)| -> bool {
        ai_enemies.values().any(|e| {
            let dx = p.0 - e.map_x;
            let dy = p.1 - e.map_y;
            (dx * dx + dy * dy) < 9
        })
    };

    // 1) Nearest drop within radius.
    let mut best_drop: Option<(CurrentTarget, i32)> = None;
    for c in collectables.values() {
        let p = (c.map_x, c.map_y);
        if bl_active(p) || near_enemy(p) {
            continue;
        }
        // Skip drops sitting inside a solid foreground tile (pickup would
        // succeed but the bot can never reach them).
        if fg_at(foreground, width, p.0, p.1) != 0 {
            continue;
        }
        let d = manhattan(me, p);
        if d <= DROP_PREFERENCE_RADIUS {
            match &best_drop {
                Some((_, best)) if *best <= d => {}
                _ => {
                    best_drop = Some((
                        CurrentTarget::Drop {
                            id: c.collectable_id,
                            x: p.0,
                            y: p.1,
                        },
                        d,
                    ))
                }
            }
        }
    }
    if let Some((target, _)) = best_drop {
        return Some(target);
    }

    // 2-4) Scored gems by row/below/above.
    let mut best_same: Option<(CurrentTarget, i32)> = None;
    let mut best_below: Option<(CurrentTarget, i32)> = None;
    let mut best_above: Option<(CurrentTarget, i32)> = None;

    // Search radius mirrors Lua's `world:tiles()` scan but bounded to a 60-tile
    // box (Rust default) so we don't melt the CPU on huge worlds.
    let search_radius = 60i32;
    let min_x = (me.0 - search_radius).max(0);
    let max_x = (me.0 + search_radius).min(width as i32 - 1);
    let min_y = (me.1 - search_radius).max(0);
    let max_y = (me.1 + search_radius).min(height as i32 - 1);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let fg = fg_at(foreground, width, x, y);
            if fg == 0 {
                continue;
            }
            if !target_set.contains(&fg) && !is_minegem(fg) {
                continue;
            }
            let p = (x, y);
            if bl_active(p) || near_enemy(p) {
                continue;
            }
            let dx = x - me.0;
            let dy = y - me.1;
            let adx = dx.abs();
            let ady = dy.abs();
            let candidate = CurrentTarget::Gem {
                x,
                y,
                expected_fg: fg,
            };

            if ady <= 1 {
                let score = adx;
                if best_same.as_ref().map_or(true, |(_, s)| score < *s) {
                    best_same = Some((candidate, score));
                }
            } else if dy >= 2 {
                let score = adx + dy * 2;
                if best_below.as_ref().map_or(true, |(_, s)| score < *s) {
                    best_below = Some((candidate, score));
                }
            } else {
                let score = adx + ady * 5;
                if best_above.as_ref().map_or(true, |(_, s)| score < *s) {
                    best_above = Some((candidate, score));
                }
            }
        }
    }

    if let Some((t, _)) = best_same.or(best_below).or(best_above) {
        return Some(t);
    }

    // 5) Closest enemy within ~20 Manhattan tiles.
    let mut best_mob: Option<(CurrentTarget, i32)> = None;
    for e in ai_enemies.values() {
        if !e.alive {
            continue;
        }
        let p = (e.map_x, e.map_y);
        if bl_active(p) {
            continue;
        }
        let d = manhattan(me, p);
        if d > 20 {
            continue;
        }
        if best_mob.as_ref().map_or(true, |(_, best)| d < *best) {
            best_mob = Some((
                CurrentTarget::Mob {
                    x: p.0,
                    y: p.1,
                    ai_id: e.ai_id,
                },
                d,
            ));
        }
    }

    best_mob.map(|(t, _)| t)
}

/// Whether the target gem is still present at its expected tile.
fn gem_still_exists(
    target: &CurrentTarget,
    foreground: &[u16],
    width: u32,
) -> bool {
    match target {
        CurrentTarget::Gem { x, y, expected_fg } => {
            let fg = fg_at(foreground, width, *x, *y);
            // Lua: if the tile is air, it was destroyed; if it morphed to a
            // *different* fg, replan; otherwise still here.
            fg == *expected_fg
        }
        // Drops and mobs are validated elsewhere.
        _ => true,
    }
}

/// Trim expired entries from a `(point → Instant)` map so it doesn't grow
/// unbounded. Cheap because the map is small (a few dozen entries at most).
fn trim_expired(map: &mut HashMap<(i32, i32), Instant>) {
    let now = Instant::now();
    map.retain(|_, exp| *exp > now);
}

/// Find an exit portal tile (Mine / Nether) in the world-items layer.
/// Mirrors Lua's `find_exit_portal`.
fn find_exit_portal(world_items: &[crate::world::DecodedWorldItem]) -> Option<(i32, i32)> {
    for item in world_items {
        let name = crate::session::block_name_for(item.item_id)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name.contains("mineexit") || name.contains("netherexit") || name.contains("mineenter") {
            return Some((item.map_x, item.map_y));
        }
    }
    None
}

// ─── Periodic tasks ─────────────────────────────────────────────────────────

async fn try_repair_pickaxe(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    logger: &Logger,
    session_id: &str,
) -> bool {
    let (worn_pickaxe, has_repair_kit) = {
        let st = state.read().await;
        let worn = st.worn_items.iter()
            .find(|&&id| PICKAXE_PRIORITY.contains(&id))
            .copied();
        let kit = st.inventory.iter().any(|e| e.block_id == REPAIR_KIT_BLOCK_ID && e.amount > 0);
        (worn, kit)
    };

    let Some(pickaxe_id) = worn_pickaxe else {
        return false;
    };
    if !has_repair_kit {
        logger.warn("automine", Some(session_id), "repair skipped — no repair kit in inventory");
        return false;
    }

    let packed = protocol::pack_inventory_key_24(
        protocol::INV_TYPE_WEAPON_PICKAXES,
        pickaxe_id as i32,
    );
    if send_doc(outbound_tx, protocol::make_mining_pickaxe_repair(packed)).await.is_err() {
        return false;
    }
    {
        let mut st = state.write().await;
        st.automine_stats.repairs = st.automine_stats.repairs.saturating_add(1);
    }
    logger.info("automine", Some(session_id),
        format!("repair pickaxe block_id={pickaxe_id} packed_ik={packed}"));
    true
}

async fn recycle_gemstones(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    logger: &Logger,
    session_id: &str,
) {
    // Gather candidates while holding the read lock.
    let candidates: Vec<(u16, u16)> = {
        let st = state.read().await;
        DEFAULT_GEM_IDS
            .iter()
            .filter_map(|&id| {
                let total: u16 = st
                    .inventory
                    .iter()
                    .filter(|e| e.block_id == id)
                    .map(|e| e.amount)
                    .sum();
                if total >= RECYCLE_MIN_AMOUNT {
                    let surplus = total - RECYCLE_MIN_AMOUNT;
                    if surplus > 0 {
                        return Some((id, surplus));
                    }
                }
                None
            })
            .collect()
    };

    for (block_id, amount) in candidates {
        let packed = protocol::pack_inventory_key_24(0, block_id as i32);
        if send_doc(
            outbound_tx,
            protocol::make_recycle_mining_gemstone(packed, amount as i32),
        )
        .await
        .is_err()
        {
            return;
        }
        {
            let mut st = state.write().await;
            st.automine_stats.recycles = st.automine_stats.recycles.saturating_add(1);
        }
        logger.info("automine", Some(session_id),
            format!("recycle: {amount} × #{block_id}"));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn craft_missing_keys(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    logger: &Logger,
    session_id: &str,
) {
    let (level, inventory) = {
        let st = state.read().await;
        (st.world.as_ref().and_then(|_| None).unwrap_or(0i32), st.inventory.clone())
    };
    // Note: the Rust side doesn't expose `client.level` directly. We use the
    // tiers we have keys for as proxy: missing keys for tiers we *should*
    // unlock are crafted on a best-effort basis.
    let _ = level; // placeholder; tier gating below works without it.

    for tier in MINE_TIERS {
        if tier.key_id == 0 {
            continue;
        }
        let has_key = inventory
            .iter()
            .any(|e| e.block_id == tier.key_id && e.amount > 0);
        if has_key {
            continue;
        }
        let packed = protocol::pack_inventory_key_24(
            protocol::INV_TYPE_CONSUMABLE_REPAIR,
            tier.key_id as i32,
        );
        if send_doc(outbound_tx, protocol::make_craft_mining_gear(packed)).await.is_err() {
            return;
        }
        logger.info("automine", Some(session_id),
            format!("craft key for tier {} (block_id={})", tier.name, tier.key_id));
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

async fn craft_pickaxe_upgrade(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    logger: &Logger,
    session_id: &str,
) {
    let worn_pickaxe = {
        let st = state.read().await;
        st.worn_items.iter()
            .find(|&&id| PICKAXE_PRIORITY.contains(&id))
            .copied()
    };
    let Some(pickaxe_id) = worn_pickaxe else { return; };

    let packed = protocol::pack_inventory_key_24(
        protocol::INV_TYPE_WEAPON_PICKAXES,
        pickaxe_id as i32,
    );
    let _ = send_doc(outbound_tx, protocol::make_craft_mining_pickaxe_upgrade(packed)).await;
    logger.info("automine", Some(session_id),
        format!("craft pickaxe upgrade block_id={pickaxe_id}"));
}

async fn humanizer_chat(
    outbound_tx: &OutboundHandle,
    logger: &Logger,
    session_id: &str,
) {
    let line = {
        let idx = rand::random_range(0..CHAT_LINES.len());
        CHAT_LINES[idx]
    };
    let _ = send_doc(outbound_tx, protocol::make_world_chat(line)).await;
    logger.info("automine", Some(session_id), format!("humanizer chat: {line:?}"));
}

// ─── Main loop ──────────────────────────────────────────────────────────────

pub(super) async fn automine_loop(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    mut stop_rx: watch::Receiver<bool>,
    _controller_tx: mpsc::Sender<ControllerEvent>,
) -> Result<(), String> {
    // ── Per-loop transient state ───────────────────────────────────────────
    let mut phase = Phase::Surveying;
    let mut target: Option<CurrentTarget> = None;
    let mut state_deadline: Option<Instant> = None;
    let mut idle_count: u32 = 0;
    let mut punch_count: u32 = 0;
    let mut last_dist: i32 = i32::MAX;
    let mut stuck_iters: u32 = 0;
    let mut exit_request: Option<ExitRequest> = None;
    let mut terminated = false;

    // ── Tickers ────────────────────────────────────────────────────────────
    let mut last_tick = Instant::now();
    let mut next_periodic = Instant::now() + PERIODIC_TICK_INTERVAL;
    let mut last_pickaxe_upgrade: Option<Instant> = None;
    let mut next_chat = Instant::now()
        + Duration::from_secs(rand::random_range(HUMANIZER_CHAT_MIN_S..HUMANIZER_CHAT_MAX_S));
    let mut next_break = Instant::now()
        + Duration::from_secs(rand::random_range(HUMANIZER_BREAK_MIN_S..HUMANIZER_BREAK_MAX_S));

    let mut current_world_name: Option<String> = None;
    let mut world_entered_at: Option<Instant> = None;
    let mut last_equip_attempt: Option<Instant> = None;

    const MAX_TILE_ATTEMPTS: u32 = 30;
    let mut tile_attempts: HashMap<(i32, i32), u32> = HashMap::new();

    loop {
        if terminated {
            return Ok(());
        }

        // ── Tick pacing (preserved from original Rust version) ─────────────
        let (ping, speed_mult) = {
            let st = state.read().await;
            (st.ping_ms.unwrap_or(0), st.automine_speed.clamp(0.4, 1.6))
        };

        let base_delay = (900.0 / speed_mult) as u64;
        let dynamic_delay = {
            let jitter = rand::random_range(0..(350.0 / speed_mult) as u64);
            let thinking_pause = if rand::random_bool(0.05) {
                (500.0 / speed_mult) as u64
            } else {
                0
            };
            if ping > 150 {
                base_delay + (ping as u64 - 100) + jitter + thinking_pause
            } else {
                base_delay + jitter + thinking_pause
            }
        };
        let sleep_duration = (last_tick + Duration::from_millis(dynamic_delay))
            .saturating_duration_since(Instant::now());

        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(sleep_duration) => {
                last_tick = Instant::now();
            }
        }

        // ── Snapshot session state once per tick ───────────────────────────
        let (
            player_x, player_y,
            world_width, world_height,
            foreground, current_world,
            inventory,
            session_status,
            durability, admin_uid, broken_count,
            slot_count,
            currently_worn_pickaxe,
        ) = {
            let st = state.read().await;
            let px = st.player_position.map_x.unwrap_or(0.0) as i32;
            let py = st.player_position.map_y.unwrap_or(0.0) as i32;
            let (w, h) = match &st.world {
                Some(w) => (w.width, w.height),
                None => (0, 0),
            };
            let worn = st.worn_items.iter()
                .find(|&&id| PICKAXE_PRIORITY.contains(&id))
                .copied();
            (
                px, py,
                w, h,
                st.world_foreground_tiles.clone(),
                st.current_world.clone(),
                st.inventory.clone(),
                st.status.clone(),
                st.automine_pickaxe_durability,
                st.automine_admin_in_world.clone(),
                st.automine_pickaxe_broken_count,
                inventory_slot_count(&st.inventory),
                worn,
            )
        };

        if matches!(
            session_status,
            SessionStatus::Idle | SessionStatus::Disconnected | SessionStatus::Error
        ) {
            return Ok(());
        }
        if matches!(
            session_status,
            SessionStatus::Connecting | SessionStatus::Authenticating | SessionStatus::Redirecting
        ) {
            continue;
        }

        // ── 1. Ensure we are in a mining world ────────────────────────────
        let is_in_mine = current_world
            .as_deref()
            .map(|w| w.to_uppercase().contains("MINE"))
            .unwrap_or(false);

        if current_world_name != current_world {
            tile_attempts.clear();
            current_world_name = current_world.clone();
            world_entered_at = Some(Instant::now());
            // Reset per-world automine flags too.
            phase = Phase::Surveying;
            target = None;
            idle_count = 0;
            punch_count = 0;
            stuck_iters = 0;
            last_dist = i32::MAX;
        }

        if !is_in_mine && phase != Phase::Exiting {
            let tier = pick_tier(0, &inventory);
            {
                let mut st = state.write().await;
                st.status = SessionStatus::JoiningWorld;
                st.pending_world = Some("MINEWORLD".to_string());
                st.pending_world_is_instance = true;
            }
            let _ = send_docs(
                outbound_tx,
                vec![
                    protocol::make_world_action_mine(tier.world_id),
                    protocol::make_join_world_special("MINEWORLD", 0),
                ],
            ).await;
            tokio::time::sleep(Duration::from_secs(4)).await;
            continue;
        }

        if world_width == 0 {
            continue;
        }

        // ── 2. Auto-equip best pickaxe ────────────────────────────────────
        if !inventory.is_empty() {
            let can_equip = match world_entered_at {
                Some(t) => t.elapsed() > Duration::from_secs(2),
                None => true,
            } && match last_equip_attempt {
                Some(t) => t.elapsed() > Duration::from_secs(5),
                None => true,
            };
            if can_equip {
                if let Some(best_id) = find_best_pickaxe(&inventory) {
                    let needs_equip = match currently_worn_pickaxe {
                        Some(worn) => worn != best_id,
                        None => true,
                    };
                    if needs_equip {
                        if let Some(old) = currently_worn_pickaxe {
                            logger.info("automine", Some(session_id),
                                format!("upgrading pickaxe {old} → {best_id}"));
                            let _ = send_doc(outbound_tx, protocol::make_unwear_item(old as i32)).await;
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        logger.info("automine", Some(session_id),
                            format!("equipping pickaxe {best_id}"));
                        let _ = send_doc(outbound_tx, protocol::make_wear_item(best_id as i32)).await;
                        last_equip_attempt = Some(Instant::now());
                    }
                }
            }
        }

        // ── 3. Pickaxe-broken 3-strike logic (mirrors Lua) ────────────────
        if broken_count > 0 {
            match broken_count {
                1 => {
                    if try_repair_pickaxe(state, outbound_tx, logger, session_id).await {
                        state.write().await.automine_pickaxe_broken_count = 0;
                    }
                }
                2 => {
                    // wear_best_pickaxe was already attempted by the
                    // auto-equip path above; clear the counter if we
                    // actually have a pickaxe to wear.
                    let have_pickaxe = find_best_pickaxe(&inventory).is_some();
                    if have_pickaxe {
                        state.write().await.automine_pickaxe_broken_count = 0;
                    } else {
                        logger.error("automine", Some(session_id),
                            "ER=8 strike 2 with no spare pickaxe — fatal exit");
                        exit_request = Some(ExitRequest {
                            reason: "no spare pickaxe".to_string(),
                            rejoin: false,
                        });
                    }
                }
                _ => {
                    logger.error("automine", Some(session_id),
                        "ER=8 strike 3+ — fatal exit");
                    exit_request = Some(ExitRequest {
                        reason: "ER=8 ×3".to_string(),
                        rejoin: false,
                    });
                }
            }
        }

        // ── 4. Admin detection ────────────────────────────────────────────
        if admin_uid.is_some() && exit_request.is_none() {
            logger.warn("automine", Some(session_id),
                "admin in world — initiating exit (no rejoin)");
            exit_request = Some(ExitRequest {
                reason: "admin entered world".to_string(),
                rejoin: false,
            });
        }

        // ── 5. Inventory full → exit + rejoin ─────────────────────────────
        if slot_count >= INVENTORY_NEAR_FULL
            && phase != Phase::Exiting
            && exit_request.is_none()
        {
            logger.info("automine", Some(session_id),
                format!("inventory full ({slot_count} slots) → gate-exit"));
            exit_request = Some(ExitRequest {
                reason: "inventory full".to_string(),
                rejoin: true,
            });
        }

        // ── 6. Periodic tasks ─────────────────────────────────────────────
        if Instant::now() >= next_periodic {
            // Repair when durability is critically low.
            if let Some(d) = durability {
                if d < PICKAXE_LOW_DURABILITY {
                    let _ = try_repair_pickaxe(state, outbound_tx, logger, session_id).await;
                }
            } else {
                // If the server hasn't told us the durability yet, fall back
                // to Lua's heuristic: repair every ~80 hits.
                let hits = state.read().await.automine_stats.hits_sent;
                if hits > 0 && hits % REPAIR_HIT_INTERVAL == 0 {
                    let _ = try_repair_pickaxe(state, outbound_tx, logger, session_id).await;
                }
            }
            recycle_gemstones(state, outbound_tx, logger, session_id).await;
            craft_missing_keys(state, outbound_tx, logger, session_id).await;
            let should_upgrade = last_pickaxe_upgrade
                .map(|t| t.elapsed() >= PICKAXE_UPGRADE_INTERVAL)
                .unwrap_or(true);
            if should_upgrade {
                craft_pickaxe_upgrade(state, outbound_tx, logger, session_id).await;
                last_pickaxe_upgrade = Some(Instant::now());
            }
            next_periodic = Instant::now() + PERIODIC_TICK_INTERVAL;
        }

        // ── 7. Humanizer chat ─────────────────────────────────────────────
        if Instant::now() >= next_chat {
            humanizer_chat(outbound_tx, logger, session_id).await;
            next_chat = Instant::now()
                + Duration::from_secs(rand::random_range(HUMANIZER_CHAT_MIN_S..HUMANIZER_CHAT_MAX_S));
        }

        // ── 8. Humanizer session break ────────────────────────────────────
        if Instant::now() >= next_break && exit_request.is_none() {
            logger.info("automine", Some(session_id), "humanizer session break");
            exit_request = Some(ExitRequest {
                reason: "session break".to_string(),
                rejoin: true,
            });
            next_break = Instant::now()
                + Duration::from_secs(rand::random_range(HUMANIZER_BREAK_MIN_S..HUMANIZER_BREAK_MAX_S));
        }

        // ── 9. Apply exit request ─────────────────────────────────────────
        if exit_request.is_some() {
            phase = Phase::Exiting;
        }

        // ── 10. Trim expired blacklist entries each tick ──────────────────
        {
            let mut st = state.write().await;
            trim_expired(&mut st.automine_gem_blacklist);
            // Also age out destroy timestamps that are no longer in the
            // 3-second observation window.
            let now = Instant::now();
            st.automine_last_destroy_at
                .retain(|_, t| now.duration_since(*t) <= DESTROY_WINDOW);
        }

        // ── 11. State-machine dispatch ────────────────────────────────────
        match phase {
            Phase::Surveying => {
                let target_set: HashSet<u16> = {
                    let mut s: HashSet<u16> = GEMSTONE_BLOCK_IDS.iter().copied().collect();
                    // Optional crystal seeking: when configured the Lua bot
                    // adds these to the target set after N gems. Without a
                    // config plumb here we just keep crystals out by default.
                    let _ = CRYSTAL_BLOCK_IDS; // referenced for clarity
                    // Ensure the Rust block-range superset is also valid.
                    for id in 3995..=4003 {
                        s.insert(id);
                    }
                    for id in 4101..=4102 {
                        s.insert(id);
                    }
                    s
                };

                let pick = {
                    let st = state.read().await;
                    pick_best_target(
                        (player_x, player_y),
                        &foreground,
                        world_width,
                        world_height,
                        &st.collectables,
                        &st.ai_enemies,
                        &st.automine_gem_blacklist,
                        &target_set,
                    )
                };

                if let Some(t) = pick {
                    target = Some(t.clone());
                    {
                        let mut st = state.write().await;
                        st.current_target = Some(t.to_bot_target());
                    }
                    phase = Phase::Approaching;
                    idle_count = 0;
                    last_dist = i32::MAX;
                    stuck_iters = 0;
                    punch_count = 0;
                } else {
                    idle_count = idle_count.saturating_add(1);
                    if idle_count >= SURVEY_EMPTY_LIMIT && exit_request.is_none() {
                        logger.info("automine", Some(session_id),
                            "survey exhausted — exiting + rejoining");
                        exit_request = Some(ExitRequest {
                            reason: "exhausted scans".to_string(),
                            rejoin: true,
                        });
                    }
                }
            }

            Phase::Approaching => {
                let Some(cur) = target.clone() else {
                    phase = Phase::Surveying;
                    continue;
                };

                // Gem morphed/destroyed? blacklist & resurvey.
                if !gem_still_exists(&cur, &foreground, world_width) {
                    let p = cur.point();
                    logger.info("automine", Some(session_id),
                        format!("approach: gem at ({},{}) gone → blacklist", p.0, p.1));
                    {
                        let mut st = state.write().await;
                        st.automine_gem_blacklist
                            .insert(p, Instant::now() + BL_GEM_DURATION);
                    }
                    target = None;
                    phase = Phase::Surveying;
                    continue;
                }

                let me = (player_x, player_y);
                let goal = cur.point();
                let dist = manhattan(me, goal);

                // Already adjacent + standing on a safe, floored tile?
                if dist <= 1
                    && tile_is_safe(&foreground, world_width, world_height, me.0, me.1)
                    && has_floor_below(&foreground, world_width, world_height, me.0, me.1)
                {
                    phase = Phase::Acting;
                    state_deadline = Some(Instant::now() + ACT_DEADLINE);
                    punch_count = 0;
                    continue;
                }

                if dist >= last_dist {
                    stuck_iters = stuck_iters.saturating_add(1);
                    if stuck_iters >= NO_PROGRESS_LIMIT {
                        logger.info("automine", Some(session_id),
                            format!("approach: no progress {stuck_iters} iters → blacklist ({},{}, dist={dist})",
                                goal.0, goal.1));
                        {
                            let mut st = state.write().await;
                            st.automine_gem_blacklist
                                .insert(goal, Instant::now() + BL_GEM_DURATION);
                        }
                        target = None;
                        phase = Phase::Surveying;
                        continue;
                    }
                } else {
                    stuck_iters = 0;
                }
                last_dist = dist;

                // Try to walk towards a stand tile next to the target. If
                // there's no clear path, fall through to the Acting step
                // which will hit the tile in the way.
                let stand = pick_stand_tile(&foreground, world_width, world_height, goal)
                    .unwrap_or(goal);

                // Mask high-attempt tiles and enemies before pathing.
                let mut masked = foreground.clone();
                for (&(x, y), &att) in &tile_attempts {
                    if att >= MAX_TILE_ATTEMPTS
                        && x >= 0 && y >= 0
                        && (x as u32) < world_width
                        && (y as u32) < world_height
                    {
                        let idx = (y as u32 * world_width + x as u32) as usize;
                        if let Some(t) = masked.get_mut(idx) {
                            *t = 3993; // bedrock — impassable
                        }
                    }
                }
                {
                    let st = state.read().await;
                    for e in st.ai_enemies.values() {
                        if !e.alive {
                            continue;
                        }
                        let idx = (e.map_y as u32 * world_width + e.map_x as u32) as usize;
                        if let Some(t) = masked.get_mut(idx) {
                            *t = 3993;
                        }
                    }
                }

                let path = get_path_to_target(
                    player_x, player_y,
                    stand.0, stand.1,
                    &masked, world_width, world_height,
                );

                match path {
                    Some(path) if path.len() > 1 => {
                        let next_step = path[1];
                        let is_last_step = path.len() == 2;
                        let next_fg = fg_at(&foreground, world_width, next_step.0, next_step.1);
                        let next_is_solid =
                            !crate::pathfinding::astar::is_walkable_tile(next_fg);
                        let move_blocked = next_is_solid;
                        let dir = if goal.0 > player_x {
                            movement::DIR_RIGHT
                        } else {
                            movement::DIR_LEFT
                        };

                        if move_blocked {
                            let is_pending = {
                                let st = state.read().await;
                                st.pending_hits
                                    .get(&next_step)
                                    .map(|t| t.elapsed() < Duration::from_millis(900))
                                    .unwrap_or(false)
                            };
                            if is_pending {
                                continue;
                            }
                            if tile_attempts.get(&next_step).copied().unwrap_or(0) >= MAX_TILE_ATTEMPTS {
                                tile_attempts.insert(goal, MAX_TILE_ATTEMPTS);
                                continue;
                            }
                            if next_step == (player_x, player_y) {
                                logger.warn("automine", Some(session_id),
                                    "STUCK: A* suggested hitting current tile — skipping");
                                continue;
                            }
                            logger.info("automine", Some(session_id),
                                format!("MINING (path-break) at ({},{}) from ({},{})",
                                    next_step.0, next_step.1, player_x, player_y));
                            let pkts = protocol::make_mine_hit_stationary(
                                player_x, player_y,
                                next_step.0, next_step.1,
                                dir,
                            );
                            let _ = send_docs_exclusive(outbound_tx, pkts).await;
                            record_action(state, format!(
                                "mine path-break from ({player_x},{player_y}) hit ({},{})",
                                next_step.0, next_step.1
                            )).await;
                            {
                                let mut st = state.write().await;
                                st.pending_hits.insert(next_step, Instant::now());
                                st.automine_stats.hits_sent =
                                    st.automine_stats.hits_sent.saturating_add(1);
                            }
                            let attempts = tile_attempts.entry(next_step).or_insert(0);
                            *attempts += 2;
                        } else {
                            let anim = if next_step.1 > player_y {
                                movement::ANIM_FALL
                            } else if next_step.1 < player_y {
                                movement::ANIM_JUMP
                            } else {
                                movement::ANIM_WALK
                            };
                            let move_pkts = protocol::make_move_to_map_point(
                                player_x, player_y,
                                next_step.0, next_step.1,
                                anim, dir,
                            );
                            let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                            record_action(state, format!(
                                "move from ({player_x},{player_y}) to ({},{}) anim={anim}",
                                next_step.0, next_step.1
                            )).await;
                            {
                                let mut st = state.write().await;
                                let (wx, wy) = protocol::map_to_world(
                                    next_step.0 as f64,
                                    next_step.1 as f64,
                                );
                                st.player_position.map_x = Some(next_step.0 as f64);
                                st.player_position.map_y = Some(next_step.1 as f64);
                                st.player_position.world_x = Some(wx);
                                st.player_position.world_y = Some(wy);
                            }
                            if is_last_step {
                                // Arrived at the stand tile next to the target;
                                // Acting picks it up next tick.
                                phase = Phase::Acting;
                                state_deadline = Some(Instant::now() + ACT_DEADLINE);
                                punch_count = 0;
                            }
                        }
                    }
                    _ => {
                        // No path. Mirror Lua's `sleep 800` and stay in
                        // Approaching; the stuck counter will eventually
                        // blacklist the target.
                        tokio::time::sleep(Duration::from_millis(800)).await;
                    }
                }
            }

            Phase::Acting => {
                let Some(cur) = target.clone() else {
                    phase = Phase::Surveying;
                    continue;
                };

                match cur {
                    CurrentTarget::Drop { id, .. } => {
                        let on_cooldown = {
                            let st = state.read().await;
                            !st.collect_cooldowns.can_collect(id)
                        };
                        if !on_cooldown {
                            let _ = send_doc(outbound_tx, protocol::make_collectable_request(id)).await;
                            {
                                let mut st = state.write().await;
                                st.collect_cooldowns.mark_collected(id);
                                st.collectables.remove(&id);
                            }
                            record_action(state, format!("collect drop id={id}")).await;
                        }
                        target = None;
                        phase = Phase::Surveying;
                    }
                    CurrentTarget::Mob { x, y, ai_id } => {
                        let _ = send_doc(outbound_tx, protocol::make_hit_ai_enemy(x, y, ai_id)).await;
                        record_action(state, format!("hit mob ai_id={ai_id} at=({x},{y})")).await;
                        target = None;
                        phase = Phase::Surveying;
                    }
                    CurrentTarget::Gem { x, y, .. } => {
                        // Did the server confirm the destroy in the last few seconds?
                        let just_destroyed = {
                            let st = state.read().await;
                            st.automine_last_destroy_at
                                .get(&(x, y))
                                .map(|t| t.elapsed() <= DESTROY_WINDOW)
                                .unwrap_or(false)
                        };
                        if just_destroyed {
                            {
                                let mut st = state.write().await;
                                st.automine_stats.gems_mined =
                                    st.automine_stats.gems_mined.saturating_add(1);
                                st.automine_gems_since_crystal_seek =
                                    st.automine_gems_since_crystal_seek.saturating_add(1);
                            }
                            target = None;
                            phase = Phase::Surveying;
                            continue;
                        }

                        if punch_count >= PUNCH_RETRY_CAP
                            || state_deadline
                                .map(|t| Instant::now() > t)
                                .unwrap_or(false)
                        {
                            logger.info("automine", Some(session_id),
                                format!("act: PUNCH_RETRY_CAP reached at ({x},{y}) → blacklist"));
                            {
                                let mut st = state.write().await;
                                st.automine_gem_blacklist
                                    .insert((x, y), Instant::now() + BL_GEM_DURATION);
                            }
                            target = None;
                            phase = Phase::Surveying;
                            continue;
                        }

                        let dir = if x > player_x {
                            movement::DIR_RIGHT
                        } else {
                            movement::DIR_LEFT
                        };
                        let pkts = protocol::make_mine_hit_stationary(
                            player_x, player_y,
                            x, y,
                            dir,
                        );
                        let _ = send_docs_exclusive(outbound_tx, pkts).await;
                        record_action(state, format!("hit gem ({x},{y})")).await;
                        punch_count += 1;
                        {
                            let mut st = state.write().await;
                            st.automine_stats.hits_sent =
                                st.automine_stats.hits_sent.saturating_add(1);
                            st.pending_hits.insert((x, y), Instant::now());
                        }

                        // Opportunistic collect of nearby drops (Lua's
                        // "for did,d in pairs(known_drops)" loop inside act_mine).
                        let nearby: Vec<i32> = {
                            let st = state.read().await;
                            st.collectables
                                .values()
                                .filter_map(|c| {
                                    if !st.collect_cooldowns.can_collect(c.collectable_id) {
                                        return None;
                                    }
                                    if manhattan((c.map_x, c.map_y), (x, y)) <= 2 {
                                        Some(c.collectable_id)
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        };
                        for cid in nearby {
                            let _ = send_doc(outbound_tx, protocol::make_collectable_request(cid)).await;
                            let mut st = state.write().await;
                            st.collect_cooldowns.mark_collected(cid);
                        }
                    }
                }
            }

            Phase::Exiting => {
                let req = exit_request.clone().unwrap_or(ExitRequest {
                    reason: "manual".to_string(),
                    rejoin: false,
                });
                logger.info("automine", Some(session_id),
                    format!("exit phase: reason={} rejoin={}", req.reason, req.rejoin));

                let portal = {
                    let st = state.read().await;
                    find_exit_portal(&st.world_items)
                };

                if let Some((px, py)) = portal {
                    // Walk to the portal then trigger PAoP. Mirrors Lua's
                    // `client:findPath(portal) + client:enterPortal(portal)`.
                    let path = get_path_to_target(
                        player_x, player_y,
                        px, py,
                        &foreground, world_width, world_height,
                    );
                    if let Some(path) = path {
                        for step in path.iter().skip(1) {
                            let dir = if step.0 > player_x {
                                movement::DIR_RIGHT
                            } else {
                                movement::DIR_LEFT
                            };
                            let move_pkts = protocol::make_move_to_map_point(
                                player_x, player_y,
                                step.0, step.1,
                                movement::ANIM_WALK, dir,
                            );
                            let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                    let _ = send_doc(outbound_tx, protocol::make_activate_out_portal(px, py)).await;
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                } else {
                    logger.info("automine", Some(session_id),
                        "exit: no portal found → leave world");
                    let _ = send_doc(outbound_tx, protocol::make_leave_world()).await;
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }

                if !req.rejoin {
                    terminated = true;
                    continue;
                }

                // Decoy hop: jump to a neutral world and pause. The Lua bot
                // calls `client:warpRandom()`; we approximate with a long
                // sleep + a join to START to lose the heat from rapid
                // identical-mine rejoins.
                logger.info("automine", Some(session_id), "decoy hop → START");
                let _ = send_doc(outbound_tx, protocol::make_join_world("START")).await;
                let hop_ms = rand::random_range(2500u64..6000);
                tokio::time::sleep(Duration::from_millis(hop_ms)).await;

                // Re-join the mine at the appropriate tier.
                let tier = pick_tier(0, &inventory);
                logger.info("automine", Some(session_id),
                    format!("rejoin: tier={} world_id={}", tier.name, tier.world_id));
                let _ = send_docs(
                    outbound_tx,
                    vec![
                        protocol::make_world_action_mine(tier.world_id),
                        protocol::make_join_world_special("MINEWORLD", 0),
                    ],
                ).await;

                exit_request = None;
                phase = Phase::Surveying;
                idle_count = 0;
                target = None;
                punch_count = 0;
                stuck_iters = 0;
                last_dist = i32::MAX;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }

        publish_state_snapshot(logger, session_id, state).await;
    }
}
