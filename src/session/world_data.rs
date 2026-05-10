//! Tile lookups, block-type metadata, inventory decoding, and the
//! UI snapshot publisher.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bson::Document;
use serde_json::Value;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::constants::tutorial as tutorial_consts;
use crate::logging::Logger;
use crate::models::{
    AiEnemySnapshot, InventoryItem, LuaCollectableSnapshot, LuaTileSnapshot, MinimapSnapshot,
    RemotePlayerSnapshot, ServerEvent, SessionSnapshot, TileCount, WorldSnapshot,
};
use crate::protocol;

use super::movement::walk_to_map_cancellable;
use super::network::{ensure_not_cancelled, send_doc, send_docs};
use super::state::{
    InventoryEntry, NamedInventoryEntry, OutboundHandle, SessionState,
};

pub(super) const POSITION_PUBLISH_THROTTLE: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct BlockTypeInfo {
    pub(super) id: u16,
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) inventory_type: u16,
    #[serde(rename = "typeName")]
    pub(super) type_name: String,
}

pub(super) static BLOCK_TYPES: OnceLock<HashMap<u16, BlockTypeInfo>> = OnceLock::new();

pub(super) fn apply_foreground_block_change(
    state: &mut SessionState,
    map_x: i32,
    map_y: i32,
    block_id: u16,
) -> bool {
    let Some(world) = state.world.as_ref() else {
        return false;
    };
    let Some(index) = tile_index(world, map_x, map_y) else {
        return false;
    };
    let Some(tile) = state.world_foreground_tiles.get_mut(index) else {
        return false;
    };
    if *tile == block_id {
        return false;
    }

    *tile = block_id;
    let Some(world) = state.world.as_mut() else {
        return false;
    };
    world.tile_counts = summarize_tile_counts(&state.world_foreground_tiles);
    true
}

pub(super) fn apply_destroy_block_change(state: &mut SessionState, map_x: i32, map_y: i32) -> bool {
    let Some(world) = state.world.as_ref() else {
        return false;
    };
    let Some(index) = tile_index(world, map_x, map_y) else {
        return false;
    };

    if let Some(tile) = state.world_foreground_tiles.get_mut(index) {
        if *tile != 0 {
            *tile = 0;
            if let Some(world) = state.world.as_mut() {
                world.tile_counts = summarize_tile_counts(&state.world_foreground_tiles);
            }
            return true;
        }
    }

    if let Some(tile) = state.world_background_tiles.get_mut(index) {
        if *tile != 0 {
            *tile = 0;
            return true;
        }
    }

    false
}

pub(super) fn tile_index(world: &WorldSnapshot, map_x: i32, map_y: i32) -> Option<usize> {
    if map_x < 0 || map_y < 0 {
        return None;
    }

    let width = world.width as usize;
    let height = world.height as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let map_x = map_x as usize;
    let map_y = map_y as usize;
    if map_x >= width || map_y >= height {
        return None;
    }

    Some(map_y * width + map_x)
}

pub(super) fn is_tile_ready_to_harvest_at(
    state: &SessionState,
    map_x: i32,
    map_y: i32,
    now_ticks: i64,
) -> Result<bool, String> {
    if state.world.is_none() {
        return Err("no world loaded yet".to_string());
    }

    let Some(growth) = state.growing_tiles.get(&(map_x, map_y)) else {
        return Ok(false);
    };

    Ok(now_ticks >= growth.growth_end_time)
}

pub(super) fn tile_snapshot_at(
    state: &SessionState,
    map_x: i32,
    map_y: i32,
) -> Result<LuaTileSnapshot, String> {
    let world = state
        .world
        .as_ref()
        .ok_or_else(|| "no world loaded yet".to_string())?;
    let index = tile_index(world, map_x, map_y)
        .ok_or_else(|| format!("tile ({map_x}, {map_y}) is out of bounds"))?;
    Ok(LuaTileSnapshot {
        foreground: state
            .world_foreground_tiles
            .get(index)
            .copied()
            .unwrap_or_default(),
        background: state
            .world_background_tiles
            .get(index)
            .copied()
            .unwrap_or_default(),
        water: state
            .world_water_tiles
            .get(index)
            .copied()
            .unwrap_or_default(),
        wiring: state
            .world_wiring_tiles
            .get(index)
            .copied()
            .unwrap_or_default(),
        ready_to_harvest: is_tile_ready_to_harvest_at(
            state,
            map_x,
            map_y,
            protocol::csharp_ticks(),
        )?,
    })
}

pub(super) fn summarize_tile_counts(tiles: &[u16]) -> Vec<TileCount> {
    let mut counts = BTreeMap::<u16, u32>::new();
    for &tile_id in tiles {
        *counts.entry(tile_id).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(tile_id, count)| TileCount { tile_id, count })
        .collect()
}

pub(super) fn block_types() -> &'static HashMap<u16, BlockTypeInfo> {
    BLOCK_TYPES.get_or_init(|| {
        serde_json::from_str::<Vec<BlockTypeInfo>>(include_str!("../../block_types.json"))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.id, entry))
            .collect()
    })
}

pub(super) fn block_names() -> HashMap<u16, String> {
    block_types()
        .iter()
        .map(|(id, entry)| (*id, entry.name.clone()))
        .collect()
}

pub(super) fn block_inventory_type_for(block_id: u16) -> Option<u16> {
    block_types().get(&block_id).map(|entry| entry.inventory_type)
}

pub(super) fn block_name_for(block_id: u16) -> Option<String> {
    block_types().get(&block_id).map(|entry| entry.name.clone())
}

pub(super) fn block_type_name_for(block_id: u16) -> Option<String> {
    block_types().get(&block_id).map(|entry| entry.type_name.clone())
}

pub(super) fn normalize_block_name(name: &str) -> String {
    name.chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn find_inventory_bait(
    inventory: &[InventoryEntry],
    bait_query: &str,
) -> Result<NamedInventoryEntry, String> {
    let bait_query = bait_query.trim();
    if bait_query.is_empty() {
        return Err("bait is required".to_string());
    }

    if let Ok(block_id) = bait_query.parse::<u16>() {
        if let Some(item) = inventory
            .iter()
            .find(|item| item.block_id == block_id && item.amount > 0)
        {
            return Ok(NamedInventoryEntry {
                inventory_key: item.inventory_key,
                block_id: item.block_id,
                name: block_name_for(item.block_id).unwrap_or_else(|| format!("#{}", item.block_id)),
            });
        }
    }

    let normalized_query = normalize_block_name(bait_query);
    inventory
        .iter()
        .filter(|item| item.amount > 0)
        .find_map(|item| {
            let name = block_name_for(item.block_id)?;
            (normalize_block_name(&name) == normalized_query).then_some(NamedInventoryEntry {
                inventory_key: item.inventory_key,
                block_id: item.block_id,
                name,
            })
        })
        .ok_or_else(|| format!("bait '{bait_query}' was not found in inventory"))
}

pub(super) fn decode_inventory(profile: &Document) -> Vec<InventoryEntry> {
    let Some(raw_pd) = protocol::binary_bytes(profile.get("pD")) else {
        return Vec::new();
    };
    let Ok(pd) = Document::from_reader(Cursor::new(raw_pd)) else {
        return Vec::new();
    };
    let Some(inv_blob) = protocol::binary_bytes(pd.get("inv")) else {
        return Vec::new();
    };
    if inv_blob.len() % 6 != 0 {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for chunk in inv_blob.chunks_exact(6) {
        let inventory_key = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as i32;
        let amount = u16::from_le_bytes([chunk[4], chunk[5]]);
        if amount == 0 {
            continue;
        }
        entries.push(InventoryEntry {
            inventory_key,
            block_id: (inventory_key as u32 & 0xFFFF) as u16,
            inventory_type: ((inventory_key as u32 >> 16) & 0xFFFF) as u16,
            amount,
        });
    }
    entries
}

pub(super) async fn inventory_key_for(
    state: &Arc<RwLock<SessionState>>,
    block_id: u16,
    inventory_type: Option<u16>,
    fallback: i32,
) -> i32 {
    let state = state.read().await;
    state
        .inventory
        .iter()
        .find(|entry| {
            entry.block_id == block_id
                && inventory_type
                    .map(|expected| entry.inventory_type == expected)
                    .unwrap_or(true)
                && entry.amount > 0
        })
        .map(|entry| entry.inventory_key)
        .unwrap_or(fallback)
}

pub(super) async fn publish_state_snapshot(
    logger: &Logger,
    session_id: &str,
    state: &Arc<RwLock<SessionState>>,
) {
    let snapshot = {
        let state = state.read().await;
        SessionSnapshot {
            id: session_id.to_string(),
            status: state.status.clone(),
            device_id: state.device_id.clone(),
            current_host: state.current_host.clone(),
            current_port: state.current_port,
            proxy: state.proxy.clone(),
            current_world: state.current_world.clone(),
            pending_world: state.pending_world.clone(),
            username: state.username.clone(),
            user_id: state.user_id.clone(),
            world: state.world.clone(),
            player_position: state.player_position.clone(),
            inventory: state
                .inventory
                .iter()
                .map(|e| InventoryItem {
                    block_id: e.block_id,
                    inventory_type: e.inventory_type,
                    amount: e.amount,
                })
                .collect(),
            ai_enemies: state.ai_enemies.values().map(|e| crate::models::AiEnemySnapshot {
                ai_id: e.ai_id,
                map_x: e.map_x,
                map_y: e.map_y,
                alive: e.alive,
            }).collect(),
            other_players: state.other_players.iter().map(|(id, pos)| crate::models::RemotePlayerSnapshot {
                user_id: id.clone(),
                position: pos.clone(),
            }).collect(),
            last_error: state.last_error.clone(),
            ping_ms: state.ping_ms,
            current_target: state.current_target.clone(),
            collectables: state.collectables.values().map(|c| crate::models::LuaCollectableSnapshot {
                id: c.collectable_id,
                block_type: c.block_type,
                amount: c.amount,
                inventory_type: c.inventory_type,
                pos_x: c.pos_x,
                pos_y: c.pos_y,
                is_gem: c.is_gem,
            }).collect(),
        }
    };
    logger.session_snapshot(snapshot);
}

pub(super) async fn wait_for_collectables(state: &Arc<RwLock<SessionState>>) -> Result<(), String> {
    let deadline = Instant::now() + tutorial_consts::collectable_timeout();
    loop {
        if !state.read().await.collectables.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for tutorial collectables".to_string());
        }
        sleep(Duration::from_millis(200)).await;
    }
}

pub(super) async fn collect_all_visible_collectables(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    collect_all_visible_collectables_cancellable(state, outbound_tx, &cancel).await
}

pub(super) async fn collect_all_visible_collectables_cancellable(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    cancel: &AtomicBool,
) -> Result<(), String> {
    ensure_not_cancelled(cancel)?;
    let collectables = {
        let mut items = state
            .read()
            .await
            .collectables
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.pos_x
                .partial_cmp(&right.pos_x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items
    };

    for collectable in collectables {
        ensure_not_cancelled(cancel)?;
        let (mx, my) = protocol::world_to_map(collectable.pos_x, collectable.pos_y);
        walk_to_map_cancellable(
            state,
            outbound_tx,
            mx.floor() as i32,
            my.floor() as i32,
            cancel,
        )
        .await?;
        send_docs(
            outbound_tx,
            vec![protocol::make_collectable_request(
                collectable.collectable_id,
            )],
        )
        .await?;
        sleep(Duration::from_millis(250)).await;
    }

    Ok(())
}

