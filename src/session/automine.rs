/// Packet-based automine automation loop.
/// Selects gemstone/ore targets from world snapshot, pathfinds, and mines.
use std::time::{Duration, Instant};
use std::sync::OnceLock;
use std::collections::HashSet;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AutomineState {
    pub active: bool,
    pub current_target: Option<(i32, i32)>,
    pub last_target_scan: Instant,
    pub last_action: Instant,
    pub scan_cooldown: Duration,
    pub action_cooldown: Duration,
}

impl AutomineState {
    pub fn new() -> Self {
        Self {
            active: false,
            current_target: None,
            last_target_scan: Instant::now(),
            last_action: Instant::now(),
            scan_cooldown: Duration::from_millis(500),
            action_cooldown: Duration::from_millis(300),
        }
    }

    pub fn start(&mut self) {
        self.active = true;
        self.current_target = None;
        self.last_target_scan = Instant::now() - self.scan_cooldown;
        self.last_action = Instant::now() - self.action_cooldown;
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.current_target = None;
    }

    pub fn is_ready_to_scan(&self) -> bool {
        self.active && self.last_target_scan.elapsed() >= self.scan_cooldown
    }

    pub fn is_ready_to_act(&self) -> bool {
        self.active && self.last_action.elapsed() >= self.action_cooldown
    }

    pub fn mark_scan(&mut self) {
        self.last_target_scan = Instant::now();
    }

    pub fn mark_action(&mut self) {
        self.last_action = Instant::now();
    }
}

/// Gemstone/ore block IDs typically mined in PixelWorlds.
/// This is a starter set; expand as needed.
/// Legacy mineable detection (kept for reference). New behavior: prefer collectibles.
/// Mine decoration blocks that look like targets but are indestructible.
/// From Seraph GWC decode: stalactites, bats, spiders, torches.
pub fn is_decoration(block_id: u16) -> bool {
    matches!(
        block_id,
        4143 | // MiningStalactitesTopData
        4144 | // MiningStalactitesBottomData
        4151 | // MiningBatData
        4152 | // MiningSpiderData
        4153   // MiningTorchData
    )
}

pub fn is_mineable(block_id: u16) -> bool {
    // Never target indestructible decorations
    if is_decoration(block_id) {
        return false;
    }
    matches!(
        block_id,
        // Legacy main world mineables
        1..=32 |
        // MineWorld Crystals (3974-3979)
        3974..=3979 |
        // MineWorld Soils/Rocks (3980-3986, 3989, 3991-3992, 3994)
        3980..=3986 | 3989 | 3991 | 3992 | 3994 |
        // MineWorld GemStones (3995-4003)
        3995..=4003
    )
}

/// Is this a MineGem (higher priority targets)?
pub fn is_minegem(block_id: u16) -> bool {
    matches!(
        block_id, 
        // Legacy and MineWorld Gemstones
        20..=32 | 3995..=4003
    )
}

pub fn is_common_terrain(block_id: u16) -> bool {
    matches!(
        block_id, 
        3980..=3984 | // Soils
        3985 | 3986 | 3989 | 3991 | 3992 | 3994 // Standard Rocks/Wood
    )
}

pub fn is_soil(block_id: u16) -> bool {
    matches!(block_id, 3980..=3984)
}

pub fn is_mineable_target(block_id: u16) -> bool {
    is_minegem(block_id) || is_common_terrain(block_id)
}

/// Collectible detection: build a cached set of block IDs that are "collectable".
/// Uses block_types.json (same source as the rest of the project) and marks an ID
/// as collectible if the name contains "collect"/"collectable" or the typeName is "Consumable".
fn collectible_ids() -> &'static HashSet<u16> {
    static COLLECT_IDS: OnceLock<HashSet<u16>> = OnceLock::new();
    COLLECT_IDS.get_or_init(|| {
        let raw = include_str!("../../block_types.json");
        let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(raw) else {
            return HashSet::new();
        };

        entries
            .into_iter()
            .filter_map(|entry| {
                let id = entry.get("id")?.as_u64()? as u16;
                let name = entry.get("name")?.as_str().unwrap_or("").to_lowercase();
                let type_name = entry.get("typeName")?.as_str().unwrap_or("").to_lowercase();

                if name.contains("collect") || name.contains("collectable") || type_name == "consumable" 
                   || matches!(id, 4154..=4162 | 4012..=4056) {
                    Some(id)
                } else {
                    None
                }
            })
            .collect()
    })
}

/// Returns true when the given tile id is considered a collectible target.
pub fn is_collectible(block_id: u16) -> bool {
    collectible_ids().contains(&block_id)
}

/// Mining-specific IDs (from block_types.json)
const PORTAL_MINE_EXIT_ID: u16 = 3966;
const MINING_GEM_START: u16 = 3995;
const MINING_GEM_END: u16 = 4003; // inclusive

/// Heuristic: is this world a mining map? We detect presence of mining gemstones or portal tiles.
pub fn is_mine_map(foreground_tiles: &[u16]) -> bool {
    foreground_tiles.iter().any(|&id| {
        id == PORTAL_MINE_EXIT_ID || (id >= MINING_GEM_START && id <= MINING_GEM_END)
    })
}

/// Return true when there are no more collectible/mining gem tiles in the provided foreground.
pub fn all_collectibles_cleared(foreground_tiles: &[u16]) -> bool {
    !foreground_tiles.iter().any(|&id| id >= MINING_GEM_START && id <= MINING_GEM_END)
}

/// Find the exit gate (portal) coordinates in the foreground tiles, if present.
pub fn find_exit_gate(
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
) -> Option<(i32, i32)> {
    for y in 0..world_height as i32 {
        for x in 0..world_width as i32 {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&id) = foreground_tiles.get(index) {
                if id == PORTAL_MINE_EXIT_ID {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

/// Scan the world snapshot for nearby mineable targets.
/// Prefer gemstones closest to the player.
///
/// `allow_crystals` controls whether crystal blocks (3974..=3979) are eligible.
/// Crystals aren't valuable enough to target every tick — the caller throttles
/// them by passing `false` until enough non-crystal tiles have been broken
/// (default cadence: 1 crystal per 5 successful breaks).
pub fn find_all_targets(
    player_map_x: i32,
    player_map_y: i32,
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
) -> Vec<(i32, i32)> {
    let mut targets = Vec::new();
    let search_radius = 50; // reasonable radius for unified scanning

    let min_x = (player_map_x - search_radius).max(0);
    let max_x = (player_map_x + search_radius).min(world_width as i32 - 1);
    let min_y = (player_map_y - search_radius).max(0);
    let max_y = (player_map_y + search_radius).min(world_height as i32 - 1);

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&block_id) = foreground_tiles.get(index) {
                if block_id != 0 {
                    let is_mineable = is_mineable_target(block_id);
                    
                    if is_mineable {
                        targets.push((x, y));
                    }
                }
            }
        }
    }
    targets
}

pub fn find_best_bot_target(
    player_map_x: i32,
    player_map_y: i32,
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
    collectables: &std::collections::HashMap<i32, crate::session::CollectableState>,
    ai_enemies: &std::collections::HashMap<i32, crate::session::AiEnemyState>,
) -> Option<crate::models::BotTarget> {
    // 1. Priority: Collectibles (Floor items)
    let mut best_collectible: Option<(i32, u32)> = None; 
    for (&id, state) in collectables {
        let dx = state.map_x - player_map_x;
        let dy = state.map_y - player_map_y;
        let dist_sq = (dx * dx + dy * dy) as u32;

        // Enemy proximity check: Avoid items too close to enemies
        let mut near_enemy = false;
        for enemy in ai_enemies.values() {
            let e_dx = state.map_x - enemy.map_x;
            let e_dy = state.map_y - enemy.map_y;
            if (e_dx * e_dx + e_dy * e_dy) < (3 * 3) {
                near_enemy = true;
                break;
            }
        }
        if near_enemy { continue; }
        
        if best_collectible.is_none() || dist_sq < best_collectible.unwrap().1 {
            best_collectible = Some((id, dist_sq));
        }
    }
    
    if let Some((id, _)) = best_collectible {
        let state = collectables.get(&id).unwrap();
        return Some(crate::models::BotTarget::Collecting {
            id,
            block_id: state.block_type as u16,
            x: state.map_x,
            y: state.map_y,
        });
    }

    // 2. Priority: High-Value Gemstones
    let mut best_gem: Option<(i32, i32, u32)> = None;
    for y in 0..world_height as i32 {
        for x in 0..world_width as i32 {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&block_id) = foreground_tiles.get(index) {
                if is_minegem(block_id) {
                    let dx = x - player_map_x;
                    let dy = y - player_map_y;
                    let dist_sq = (dx * dx + dy * dy) as u32;
                    if best_gem.is_none() || dist_sq < best_gem.unwrap().2 {
                        best_gem = Some((x, y, dist_sq));
                    }
                }
            }
        }
    }

    if let Some((x, y, _)) = best_gem {
        return Some(crate::models::BotTarget::Mining { x, y });
    }

    // 3. Priority: Common Terrain (Rocks/Soil) to clear path
    let mut best_terrain: Option<(i32, i32, u32)> = None;
    for y in 0..world_height as i32 {
        for x in 0..world_width as i32 {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&block_id) = foreground_tiles.get(index) {
                if is_mineable(block_id) {
                    let dx = x - player_map_x;
                    let dy = y - player_map_y;
                    let dist_sq = (dx * dx + dy * dy) as u32;
                    if best_terrain.is_none() || dist_sq < best_terrain.unwrap().2 {
                        best_terrain = Some((x, y, dist_sq));
                    }
                }
            }
        }
    }

    if let Some((x, y, _)) = best_terrain {
        return Some(crate::models::BotTarget::Mining { x, y });
    }

    None
}


/// Check if we can reach a target via pathfinding.
pub fn can_reach_target(
    player_map_x: i32,
    player_map_y: i32,
    target_x: i32,
    target_y: i32,
    foreground_tiles: &[u16],
    world_width: u32,
    world_height: u32,
) -> bool {
    let width = world_width as usize;
    let height = world_height as usize;

    match crate::pathfinding::astar::find_tile_path(
        foreground_tiles,
        width,
        height,
        (player_map_x, player_map_y),
        (target_x, target_y),
    ) {
        Some(path) => !path.is_empty() && path.len() < 50,
        None => false,
    }
}

/// Get the path to a target, used to step-by-step navigate and mine obstacles.
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

