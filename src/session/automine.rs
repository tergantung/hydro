//! Packet-based automine automation: tile predicates, target selection,
//! pathfinding glue, pickaxe equip helpers, and the main `automine_loop`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use rand::RngExt;

use tokio::sync::{watch, RwLock};

use crate::constants::movement;
use crate::logging::Logger;
use crate::models::{BotTarget, SessionStatus};
use crate::protocol;

use super::network::{send_doc, send_docs, send_docs_exclusive};
use super::publish_state_snapshot;
use super::state::{InventoryEntry, OutboundHandle, SessionState};

// `record_action` writes through to mod.rs's tile-attempt tracker; tile
// helpers stay in mod.rs until Task 8.

/// Is this a mineable gemstone block?
pub fn is_minegem(block_id: u16) -> bool {
    // Only actual Gemstone BLOCKS that can be mined
    (block_id >= 3995 && block_id <= 4003) || (block_id >= 4101 && block_id <= 4102)
}

/// Pick the best target — collectible or gemstone — within a 60-tile radius
/// of the player. Targets near AI enemies are skipped.
///
/// Returns `None` when no eligible target exists in range.
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

    // Collectibles (items on the floor) and gemstones (blocks in walls) are
    // ranked equally — nearest reachable target wins.
    for (&id, state) in collectables {
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

// === Pickaxe selection =============================================

const PICKAXE_PRIORITY: &[u16] = &[
    4195, // WeaponPickaxeDark
    4093, // WeaponPickaxeEpic
    4092, // WeaponPickaxeMaster
    4091, // WeaponPickaxeHeavy
    4090, // WeaponPickaxeSturdy
    4089, // WeaponPickaxeBasic
    4088, // WeaponPickaxeFlimsy
    4087, // WeaponPickaxeCrappy
];

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

pub(super) async fn automine_loop(
    _session_id: &str,
    _logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    // We now use dynamic sleep instead of a fixed interval to handle ping jitter.
    let mut last_tick = Instant::now();

    // Equip a pickaxe once per session and re-equip after losing it. Without
    // a pickaxe in hand the server ignores all HB packets — verified via Seraph
    // capture which logs "pickaxe 0x0ff8 equipped" before any mining attempt.
    let mut equipped_pickaxe: Option<u16> = None;

    // Per-tile HB attempt counter. After MAX_TILE_ATTEMPTS hits without the
    // server confirming destruction (DB packet → foreground tile zeroed),
    // the tile is considered a dead-end and excluded from target search.
    // Increased to 15 to handle high-ping scenarios.
    const MAX_TILE_ATTEMPTS: u32 = 12;
    let mut tile_attempts: HashMap<(i32, i32), u32> = HashMap::new();
    let mut current_world_name: Option<String> = None;
    let mut sticky_target: Option<BotTarget> = None;

    loop {
        let ping = { state.read().await.ping_ms.unwrap_or(0) };
        let base_delay = 850;
        let dynamic_delay = {
            let mut rng = rand::rng();
            let jitter = rng.random_range(0..350);
            let thinking_pause = if rng.random_bool(0.05) { 500 } else { 0 };

            if ping > 150 {
                base_delay + (ping - 100) + jitter + thinking_pause
            } else {
                base_delay + jitter + thinking_pause
            }
        };
        let sleep_duration = (last_tick + Duration::from_millis(dynamic_delay as u64))
            .saturating_duration_since(Instant::now());

        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(sleep_duration) => {
                last_tick = Instant::now();
                let (player_x, player_y, world_width, world_height, foreground, current_world, inventory, session_status, _nearby_enemies, all_enemies) = {
                    let st = state.read().await;
                    let px = st.player_position.map_x.unwrap_or(0.0) as i32;
                    let py = st.player_position.map_y.unwrap_or(0.0) as i32;
                    let inventory = st.inventory.clone();
                    let current_world = st.current_world.clone();
                    let session_status = st.status.clone();

                    let nearby_enemies: Vec<(i32, i32, i32)> = st.ai_enemies.values()
                        .filter(|e| e.alive && (e.map_x - px).abs() <= 3 && (e.map_y - py).abs() <= 3)
                        .map(|e| (e.map_x, e.map_y, e.ai_id))
                        .collect();

                    let all_enemies: Vec<(i32, i32)> = st.ai_enemies.values()
                        .filter(|e| e.alive)
                        .map(|e| (e.map_x, e.map_y))
                        .collect();

                    if let Some(w) = &st.world {
                        (px, py, w.width, w.height, st.world_foreground_tiles.clone(), current_world, inventory, session_status, nearby_enemies, all_enemies)
                    } else {
                        (px, py, 0, 0, vec![], current_world, inventory, session_status, nearby_enemies, all_enemies)
                    }
                };

                // Stop if session was explicitly stopped or errored
                if matches!(session_status, SessionStatus::Idle | SessionStatus::Disconnected | SessionStatus::Error) {
                    return Ok(());
                }

                // If currently transitioning connections (e.g. Redirecting to mine), just wait
                if matches!(session_status, SessionStatus::Connecting | SessionStatus::Authenticating | SessionStatus::Redirecting) {
                    continue;
                }

                let is_in_mine = current_world.as_deref().map(|w| w.to_uppercase() == "MINEWORLD").unwrap_or(false);
                if !is_in_mine {
                    let best_level = 0; 
                    
                    {
                        let mut st = state.write().await;
                        st.status = SessionStatus::JoiningWorld;
                        st.pending_world = Some("MINEWORLD".to_string());
                        st.pending_world_is_instance = true;
                    }

                    // Send wlA and TTjW in the exact same batch just like a normal JoinWorld command!
                    let _ = send_docs(
                        outbound_tx,
                        vec![
                            protocol::make_world_action_mine(best_level),
                            protocol::make_join_world_special("MINEWORLD", 0),
                        ],
                    ).await;
                    
                    // Wait for world transition
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    continue;
                }

                if world_width == 0 {
                    // World data not loaded yet, send a movement packet to stay alive
                    if is_in_mine {

                        let move_pkts = protocol::make_move_to_map_point(player_x, player_y, player_x, player_y, movement::ANIM_IDLE, movement::DIR_LEFT);
                        let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                    }
                    continue;
                }

                // Equip the best available pickaxe before any HB attempts. The server
                // silently drops mining packets from a player without one in hand.
                if equipped_pickaxe.is_none() {
                    if let Some(pickaxe_id) = find_best_pickaxe(&inventory) {
                        let _ = send_doc(outbound_tx, protocol::make_wear_item(pickaxe_id as i32)).await;
                        equipped_pickaxe = Some(pickaxe_id);
                    } else {
                        _logger.warn("automine", Some(_session_id),
                            "no pickaxe in inventory — HB packets will be ignored by the server");
                    }
                }

                // Reset attempt counters and pickaxe state when entering a new world.
                if current_world_name != current_world {
                    tile_attempts.clear();
                    equipped_pickaxe = None;
                    current_world_name = current_world.clone();
                }

                // Drop attempt entries for tiles the server has confirmed destroyed
                // (foreground tile is now 0). Those positions are walkable now.
                tile_attempts.retain(|&(x, y), _| {
                    if x < 0 || y < 0 || (x as u32) >= world_width || (y as u32) >= world_height {
                        return false;
                    }
                    let idx = (y as u32 * world_width + x as u32) as usize;
                    foreground.get(idx).copied().unwrap_or(0) != 0
                });

                // Build a masked view of the foreground where dead-end tiles are
                // replaced with bedrock (3993 — astar's get_tile_cost returns None,
                // making it both unreachable AND not a target candidate).
                let mut masked_foreground = foreground.clone();
                for (&(x, y), &attempts) in &tile_attempts {
                    if attempts >= MAX_TILE_ATTEMPTS
                        && x >= 0 && y >= 0
                        && (x as u32) < world_width
                        && (y as u32) < world_height
                    {
                        let idx = (y as u32 * world_width + x as u32) as usize;
                        if let Some(t) = masked_foreground.get_mut(idx) {
                            *t = 3993;
                        }
                    }
                }
                for (ex, ey) in &all_enemies {
                    let idx = (*ey as u32 * world_width + *ex as u32) as usize;
                    if let Some(t) = masked_foreground.get_mut(idx) {
                        // Mark AI tiles as obsidian (non-destructible dead-end) for pathfinding
                        *t = 3993;
                    }
                }
                // Godmode-by-omission: damage is fully client-side (verified via packet capture —
                // taking damage emits only a [PPA] audio packet, never a damage packet to the
                // server). An external bot that never simulates self-damage is implicitly invincible,
                // so we no longer wear damage/fighting potions.

                // Combat Stance: Single-target priority combat (matches MineBot.cs logic)
                // Select only the single closest enemy to avoid "Machine Gun" kicks.
                // Distance is computed via i64 widening so a corrupted enemy entry
                // with massive negative coords can't wrap into a tiny i32 and slip
                // past the `dist <= 2` gate.
                let mut closest_enemy: Option<(i32, i32, i32)> = None;
                let mut min_dist: i64 = 999;

                {
                    let st = state.read().await;
                    for e in st.ai_enemies.values() {
                        if !(e.alive && e.map_x != 0) {
                            continue;
                        }
                        let dx = (e.map_x as i64) - (player_x as i64);
                        let dy = (e.map_y as i64) - (player_y as i64);
                        let dist = dx.abs() + dy.abs();
                        // Maximum valid melee reach is 2 blocks.
                        if dist <= 2 && dist < min_dist {
                            min_dist = dist;
                            closest_enemy = Some((e.map_x, e.map_y, e.ai_id));
                        }
                    }
                }

                if let Some((ex, ey, ai_id)) = closest_enemy {
                    _logger.info("automine", Some(&_session_id), format!("COMBAT: Hitting single closest AI enemy ID={} at ({},{}) from player pos ({},{})", ai_id, ex, ey, player_x, player_y));
                    let _ = send_doc(outbound_tx, protocol::make_hit_ai_enemy(ex, ey, ai_id)).await;
                    record_action(state, format!("HAI ai_id={ai_id} at=({ex},{ey})")).await;
                }

                // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                // AUTO-COLLECT: spam `C` for every dropped collectable within
                // magnet range (~4 world-tiles). The server validates proximity
                // and quietly drops collect requests for items too far away, so
                // there's no penalty for asking. This catches nuggets/coins/gems
                // that scattered around when we mined adjacent blocks — the old
                // path-walking-to-drop logic was missing them because we'd already
                // moved on by the time the path completed.
                // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                {
                    const COLLECT_RADIUS: i32 = 2; // map tiles
                    let to_collect: Vec<i32> = {
                        let st = state.read().await;
                        st.collectables
                            .values()
                            .filter_map(|c| {
                                if !st.collect_cooldowns.can_collect(c.collectable_id) {
                                    return None;
                                }
                                // CollectableState map_x/map_y are already correctly scaled.
                                let cx = c.map_x;
                                let cy = c.map_y;
                                let dx = (cx - player_x).abs();
                                let dy = (cy - player_y).abs();
                                if dx <= COLLECT_RADIUS && dy <= COLLECT_RADIUS {
                                    Some(c.collectable_id)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    };
                    if !to_collect.is_empty() {
                        _logger.info("automine", Some(&_session_id),
                            format!("AUTO-COLLECT: requesting {} drops within {}-tile radius", to_collect.len(), COLLECT_RADIUS));
                        for cid in to_collect {
                            let _ = send_doc(outbound_tx, protocol::make_collectable_request(cid)).await;
                            // Keep local set in sync using cooldowns instead of optimistic deletion.
                            state.write().await.collect_cooldowns.mark_collected(cid);
                        }
                    }
                }

                // Track tiles we attempt to break this tick so we can bump their counters
                // and log dead-end transitions outside the pathfinding closure.
                let mut hit_this_tick: Option<(i32, i32)> = None;

                // Pathfinding target selection with stickiness
                let mut target: Option<(BotTarget, Vec<(i32, i32)>)> = None;
                
                // 1. Check if our sticky target is still valid
                if let Some(st_target) = sticky_target.clone() {
                    let still_exists = {
                        let st = state.read().await;
                        match st_target {
                            BotTarget::Collecting { id, .. } => st.collectables.contains_key(&id),
                            BotTarget::Mining { x, y } => {
                                let idx = (y as u32 * world_width + x as u32) as usize;
                                foreground.get(idx).copied().unwrap_or(0) != 0
                            }
                            _ => false,
                        }
                    };

                    if still_exists {
                        let (tx, ty) = match st_target {
                            BotTarget::Mining { x, y } => (x, y),
                            BotTarget::Collecting { x, y, .. } => (x, y),
                            _ => (0, 0),
                        };
                        
                        // Check if it's a dead-end
                        if tile_attempts.get(&(tx, ty)).copied().unwrap_or(0) < MAX_TILE_ATTEMPTS {
                            if let Some(path) = get_path_to_target(player_x, player_y, tx, ty, &masked_foreground, world_width, world_height) {
                                target = Some((st_target, path));
                            }
                        }
                    }
                }

                // 2. Perform a fresh scan if no sticky target
                if target.is_none() {
                    let st = state.read().await;
                    let best = find_best_bot_target(
                        player_x, player_y,
                        world_width, world_height,
                        &masked_foreground,
                        &st.collectables,
                        &st.ai_enemies,
                    );
                    
                    if let Some(t) = best {
                        let (tx, ty) = match t {
                            BotTarget::Mining { x, y } => (x, y),
                            BotTarget::Collecting { x, y, .. } => (x, y),
                            _ => (0, 0),
                        };
                        if let Some(path) = get_path_to_target(player_x, player_y, tx, ty, &masked_foreground, world_width, world_height) {
                            target = Some((t, path));
                        }
                    }
                }

                if let Some((t, _)) = target.clone() {
                    sticky_target = Some(t);
                }


                // Sync current targeting state to the UI.
                {
                    let mut st = state.write().await;
                    st.current_target = target.as_ref().map(|(t, _)| t.clone());
                }

                match target {
                    Some((t, path)) => {
                        let (target_x, target_y, is_collectable, opt_cid) = match t {
                            BotTarget::Mining { x, y } => (x, y, false, None),
                            BotTarget::Collecting { id, x, y, .. } => (x, y, true, Some(id)),
                            _ => (0, 0, false, None),
                        };

                        if is_collectable {
                            _logger.info("automine", Some(&_session_id), format!("TARGETING: Collectable ID={} at ({}, {})", opt_cid.unwrap(), target_x, target_y));
                        } else {
                            _logger.info("automine", Some(&_session_id), format!("TARGETING: Block at ({}, {})", target_x, target_y));
                        }
                        
                        let resolved_path = Some(path);
                        match resolved_path {
                            Some(path) => {
                                if path.len() > 1 {
                                    let next_step = path[1];
                                    let next_index = (next_step.1 as u32 * world_width + next_step.0 as u32) as usize;
                                    let next_block = foreground.get(next_index).copied().unwrap_or(0);
                                    let is_last_step = path.len() == 2;
                                    let next_is_solid = !crate::pathfinding::astar::is_walkable_tile(next_block);
                                    
                                    // If the NEXT tile is solid, we definitely can't move into it.
                                    // If the NEXT tile is the TARGET and the TARGET is a block, stay here.
                                    let move_blocked = next_is_solid || (is_last_step && !is_collectable);

                                    // Direction: face toward the target
                                    let dir = if target_x > player_x { movement::DIR_RIGHT } else { movement::DIR_LEFT };

                                    // Animation must match the physics the server can verify from
                                    // before/after positions. Anything else triggers KErr code 1
                                    // ("animation/physics mismatch"). Smaller map_y = higher visually
                                    // (Unity-style top-down map coords).
                                    let is_moving_up = next_step.1 < player_y;
                                    let is_moving_down = next_step.1 > player_y;

                                    if move_blocked {
                                        // Check if we already hit this tile and are waiting for a DB packet
                                        let is_pending = {
                                            let st = state.read().await;
                                            st.pending_hits.get(&(next_step.0, next_step.1))
                                                .map(|last| last.elapsed() < Duration::from_millis(900))
                                                .unwrap_or(false)
                                        };

                                        if is_pending {
                                            // _logger.info("automine", Some(&_session_id), format!("STABILITY: Waiting for DB at ({}, {})", next_step.0, next_step.1));
                                            continue; // Skip this tick to wait for server confirmation
                                        }

                                        // Check if THIS blocking tile is a dead-end
                                        if tile_attempts.get(&(next_step.0, next_step.1)).copied().unwrap_or(0) >= MAX_TILE_ATTEMPTS {
                                            // This path is permanently blocked. Mark the FINAL target as a dead-end too
                                            // so we pick something else.
                                            tile_attempts.insert((target_x, target_y), MAX_TILE_ATTEMPTS);
                                            continue;
                                        }

                                        // Respect the DB (Destroy Block) packet: 
                                        // If the path is blocked, we STAY STILL and hit.
                                        // SAFETY GUARD: Never hit the tile we are standing on.
                                        if next_step.0 == player_x && next_step.1 == player_y {
                                            _logger.warn("automine", Some(&_session_id), "STUCK: A* suggested hitting current tile. Skipping to prevent self-mine kick.");
                                            continue;
                                        }

                                        _logger.info("automine", Some(&_session_id), format!("MINING: Path blocked at ({}, {}), hitting from ({}, {})", next_step.0, next_step.1, player_x, player_y));
                                        let pkts = protocol::make_mine_hit_stationary(
                                            player_x, player_y,
                                            next_step.0, next_step.1,
                                            dir,
                                        );
                                        let _ = send_docs_exclusive(outbound_tx, pkts).await;
                                        record_action(state, format!("mine+move from ({player_x},{player_y}) hit ({},{})", next_step.0, next_step.1)).await;
                                        hit_this_tick = Some((next_step.0, next_step.1));

                                        {
                                            let mut st = state.write().await;
                                            st.pending_hits.insert((next_step.0, next_step.1), Instant::now());
                                        }
                                    } else {
                                        // Pure movement: pick the anim that physically describes this transition.
                                        let anim = if next_step.1 > player_y {
                                            movement::ANIM_FALL // Moving down
                                        } else if next_step.1 < player_y {
                                            movement::ANIM_JUMP // Moving up
                                        } else {
                                            movement::ANIM_WALK // Moving horizontal
                                        };

                                        let move_pkts = protocol::make_move_to_map_point(player_x, player_y, next_step.0, next_step.1, anim, dir);
                                        let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                                        record_action(state, format!("move from ({player_x},{player_y}) to ({},{}) anim={anim}", next_step.0, next_step.1)).await;

                                        {
                                            let mut st = state.write().await;
                                            // Update BOTH map and world so internal state stays
                                            // self-consistent. The previous bug was updating only
                                            // map_x/y while world_x/y stayed stale from the last
                                            // server echo — `make_move_to_map_point` then computed
                                            // outbound coords from drifted optimistic map and the
                                            // server saw a giant teleport (ER=7 SpeedHack kick).
                                            let (wx, wy) = protocol::map_to_world(
                                                next_step.0 as f64,
                                                next_step.1 as f64,
                                            );
                                            st.player_position.map_x = Some(next_step.0 as f64);
                                            st.player_position.map_y = Some(next_step.1 as f64);
                                            st.player_position.world_x = Some(wx);
                                            st.player_position.world_y = Some(wy);
                                        }

                                        if path.len() == 2 {
                                            if is_collectable {
                                                let cid = opt_cid.unwrap();
                                                let _ = send_doc(outbound_tx, protocol::make_collectable_request(cid)).await;
                                                {
                                                    let mut st = state.write().await;
                                                    st.collect_cooldowns.mark_collected(cid);
                                                }
                                                record_action(state, format!("request collectable cid={cid} from ({player_x},{player_y})")).await;
                                            } else {
                                                // SAFETY GUARD: Never hit the tile we are standing on.
                                                if target_x == player_x && target_y == player_y {
                                                    _logger.warn("automine", Some(&_session_id), "STUCK: Target is player tile. Skipping.");
                                                    continue;
                                                }

                                                _logger.info("automine", Some(&_session_id), format!("MINING: Stationary hit at ({}, {})", target_x, target_y));
                                                let hit_pkts = protocol::make_mine_hit_stationary(
                                                    next_step.0, next_step.1,
                                                    target_x, target_y,
                                                    dir,
                                                );
                                                let _ = send_docs_exclusive(outbound_tx, hit_pkts).await;
                                                record_action(state, format!("stationary hit at ({target_x},{target_y})")).await;
                                                hit_this_tick = Some((target_x, target_y));
                                            }
                                        }
                                    }
                                } else {
                                    if is_collectable {
                                        let cid = opt_cid.unwrap();
                                        let _ = send_doc(outbound_tx, protocol::make_collectable_request(cid)).await;
                                        {
                                            let mut st = state.write().await;
                                            st.collect_cooldowns.mark_collected(cid);
                                        }
                                        record_action(state, format!("request collectable cid={cid} on tile")).await;
                                    } else {
                                        // Already on top of target — stationary hit (a=6 Hit) as exclusive batch
                                        let dir = if target_x > player_x { movement::DIR_RIGHT } else { movement::DIR_LEFT };

                                        // SAFETY GUARD: Never hit the tile we are standing on.
                                        if target_x == player_x && target_y == player_y {
                                            _logger.warn("automine", Some(&_session_id), "STUCK: Already on target tile. Skipping hit.");
                                            continue;
                                        }

                                        _logger.info("automine", Some(&_session_id), format!("MINING: On-tile stationary hit at ({}, {})", target_x, target_y));
                                        let hit_pkts = protocol::make_mine_hit_stationary(
                                            player_x, player_y,
                                            target_x, target_y,
                                            dir,
                                        );
                                        let _ = send_docs_exclusive(outbound_tx, hit_pkts).await;
                                        record_action(state, format!("stationary hit at ({target_x},{target_y}) from ({player_x},{player_y})")).await;
                                        hit_this_tick = Some((target_x, target_y));
                                    }
                                }
                            }
                            None => {
                                tile_attempts.insert((target_x, target_y), MAX_TILE_ATTEMPTS);
                            }
                        }
                    }
                    None => {}
                }

                if let Some((hx, hy)) = hit_this_tick {
                    let attempts = tile_attempts.entry((hx, hy)).or_insert(0);
                    *attempts += 3; // We hit 3 times per burst
                    if *attempts == MAX_TILE_ATTEMPTS {
                        _logger.warn("automine", Some(_session_id),
                            format!("dead-end: tile ({},{}) did not break in {} retries", hx, hy, MAX_TILE_ATTEMPTS));
                    }
                }

                // Force UI update AFTER all critical game packets have been sent
                publish_state_snapshot(_logger, _session_id, state).await;
            }
        }
    }
}

