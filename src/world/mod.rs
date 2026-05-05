use std::collections::BTreeMap;
use std::io::Cursor;

use bson::{Bson, Document};

use crate::constants::tutorial;
use crate::models::{TileCount, WorldSnapshot};
use crate::protocol;

/// One pre-scattered collectable from the world snapshot's `Collectables` document.
/// Field meanings match the `nCo` (NewCollectable) packet that the server uses
/// for live drops, so this struct is interchangeable with on-the-fly drops.
#[derive(Debug, Clone)]
pub struct DecodedCollectable {
    pub collectable_id: i32,
    pub block_type: i32,
    pub amount: i32,
    pub inventory_type: i32,
    pub pos_x: f64,
    pub pos_y: f64,
    pub is_gem: bool,
    pub gem_type: i32,
}

#[derive(Debug, Clone)]
pub struct DecodedWorldItem {
    pub item_id: u16,
    pub map_x: i32,
    pub map_y: i32,
    pub state: i32, // e.g. 0=Closed, 1=Open for Portals
}

#[derive(Debug, Clone)]
pub struct DecodedWorld {
    pub snapshot: WorldSnapshot,
    pub foreground_tiles: Vec<u16>,
    pub background_tiles: Vec<u16>,
    pub water_tiles: Vec<u16>,
    pub wiring_tiles: Vec<u16>,
    /// Pre-scattered collectables that exist at world load. MINEWORLD seeds
    /// nuggets, coins, and small gems this way — the server does NOT replay
    /// them via `nCo` packets, so without parsing this list the bot is blind
    /// to every drop it didn't trigger itself.
    pub collectables: Vec<DecodedCollectable>,
    /// Metadata for special tiles (Spawners, Portals, Signs, Chests).
    /// Seraph uses this to "see" hidden spawners and portals.
    pub world_items: Vec<DecodedWorldItem>,
}

pub fn decode_gwc(world_name: Option<String>, raw: &[u8]) -> Result<DecodedWorld, String> {
    let decompressed =
        zstd::stream::decode_all(Cursor::new(raw)).map_err(|error| error.to_string())?;
    let document =
        Document::from_reader(Cursor::new(decompressed)).map_err(|error| error.to_string())?;
    parse_world_document(world_name, &document)
}

pub fn parse_world_document(
    world_name: Option<String>,
    world_bson: &Document,
) -> Result<DecodedWorld, String> {
    let size = world_bson
        .get_document("WorldSizeSettingsType")
        .cloned()
        .unwrap_or_default();
    let width = size.get_i32("WorldSizeX").unwrap_or_default().max(0) as u32;
    let height = size.get_i32("WorldSizeY").unwrap_or_default().max(0) as u32;

    // For TUTORIAL2 the GWC always ships WorldStartPoint = (40, 30) — that is the
    // generic observer spawn.  New accounts must spawn at the sleeping pod instead:
    // map (39, 44) = world (12.48, 13.92), confirmed from packets.bin rec 32.
    let (spawn_map_x, spawn_map_y) = if world_name.as_deref() == Some(tutorial::TUTORIAL_WORLD) {
        (
            Some(tutorial::TUTORIAL_SPAWN_MAP_X as f64),
            Some(tutorial::TUTORIAL_SPAWN_MAP_Y as f64),
        )
    } else {
        let spawn = world_bson
            .get_document("WorldStartPoint")
            .cloned()
            .unwrap_or_default();
        (spawn_value(&spawn, "x"), spawn_value(&spawn, "y"))
    };
    let (spawn_world_x, spawn_world_y) = match (spawn_map_x, spawn_map_y) {
        (Some(x), Some(y)) => {
            let (wx, wy) = protocol::map_to_world(x, y);
            (Some(wx), Some(wy))
        }
        _ => (None, None),
    };

    let block_layer = protocol::binary_bytes(world_bson.get("BlockLayer")).unwrap_or_default();
    let background_layer =
        protocol::binary_bytes(world_bson.get("BackgroundLayer")).unwrap_or_default();
    let water_layer = protocol::binary_bytes(world_bson.get("WaterLayer")).unwrap_or_default();
    let wiring_layer = protocol::binary_bytes(world_bson.get("WiringLayer")).unwrap_or_default();
    let mut tile_map = BTreeMap::<u16, u32>::new();
    let mut foreground_tiles = Vec::with_capacity(block_layer.len() / 2);
    for chunk in block_layer.chunks_exact(2) {
        let tile_id = u16::from_le_bytes([chunk[0], chunk[1]]);
        foreground_tiles.push(tile_id);
        *tile_map.entry(tile_id).or_insert(0) += 1;
    }
    let tile_counts = tile_map
        .into_iter()
        .map(|(tile_id, count)| TileCount { tile_id, count })
        .collect::<Vec<_>>();

    let collectables = decode_collectables(world_bson.get("Collectables"));
    let collectables_count = collectables.len();
    let world_items = decode_world_items(world_bson.get("WorldItems"));
    let world_items_count = world_items.len();

    Ok(DecodedWorld {
        snapshot: WorldSnapshot {
            world_name,
            width,
            height,
            spawn_map_x,
            spawn_map_y,
            spawn_world_x,
            spawn_world_y,
            collectables_count,
            world_items_count,
            tile_counts,
        },
        foreground_tiles,
        background_tiles: decode_layer(&background_layer),
        water_tiles: decode_layer(&water_layer),
        wiring_tiles: decode_layer(&wiring_layer),
        collectables,
        world_items,
    })
}

/// Extract every `DecodedCollectable` from the world's `Collectables` document.
/// The BSON layout is `{ "Count": N, "0": {...}, "1": {...}, ... }`.
fn decode_collectables(value: Option<&Bson>) -> Vec<DecodedCollectable> {
    let Some(Bson::Document(document)) = value else {
        return Vec::new();
    };
    document
        .iter()
        .filter_map(|(key, value)| {
            if key.as_str() == "Count" {
                return None;
            }
            let entry = match value {
                Bson::Document(doc) => doc,
                _ => return None,
            };
            // Field names mirror the `nCo` packet exactly. Some entries omit
            // optional fields (GemType, IsGem) — defaults are safe for those.
            let collectable_id = entry.get_i32("CollectableID").ok()?;
            let map_x = entry.get_f64("PosX").unwrap_or_default();
            let map_y = entry.get_f64("PosY").unwrap_or_default();
            let (pos_x, pos_y) = protocol::map_to_world(map_x, map_y);
            
            Some(DecodedCollectable {
                collectable_id,
                block_type: entry.get_i32("BlockType").unwrap_or_default(),
                amount: entry.get_i32("Amount").unwrap_or_default(),
                inventory_type: entry.get_i32("InventoryType").unwrap_or_default(),
                pos_x,
                pos_y,
                is_gem: entry.get_bool("IsGem").unwrap_or(false),
                gem_type: entry.get_i32("GemType").unwrap_or_default(),
            })
        })
        .collect()
}

/// Extract every `DecodedWorldItem` from the world's `WorldItems` document.
/// The BSON layout is `{ "Count": N, "0": { "x": 5, "y": 93, "id": 4103, "s": 1 }, ... }`.
fn decode_world_items(value: Option<&Bson>) -> Vec<DecodedWorldItem> {
    let Some(Bson::Document(document)) = value else {
        return Vec::new();
    };
    document
        .iter()
        .filter_map(|(key, value)| {
            if key.as_str() == "Count" {
                return None;
            }
            let entry = match value {
                Bson::Document(doc) => doc,
                _ => return None,
            };
            
            let item_id = entry.get_i32("id").ok()? as u16;
            let map_x = entry.get_i32("x").ok()?;
            let map_y = entry.get_i32("y").ok()?;
            let state = entry.get_i32("s").unwrap_or_default();
            
            Some(DecodedWorldItem {
                item_id,
                map_x,
                map_y,
                state,
            })
        })
        .collect()
}

fn decode_layer(bytes: &[u8]) -> Vec<u16> {
    let mut tiles = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        tiles.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    tiles
}

fn spawn_value(document: &Document, key: &str) -> Option<f64> {
    match document.get(key) {
        Some(Bson::Double(value)) => Some(*value),
        Some(Bson::Int32(value)) => Some(*value as f64),
        Some(Bson::Int64(value)) => Some(*value as f64),
        _ => None,
    }
}

fn document_len(value: Option<&Bson>) -> usize {
    match value {
        Some(Bson::Document(document)) => document
            .iter()
            .filter(|(key, value)| key.as_str() != "Count" && matches!(value, Bson::Document(_)))
            .count(),
        _ => 0,
    }
}
