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

/// Is this a gemstone (higher priority targets)?
pub fn is_gemstone(block_id: u16) -> bool {
    matches!(
        block_id, 
        // Legacy main world gemstones
        20..=32 |
        // MineWorld GemStones (3995-4003)
        3995..=4003
    )
}

/// Is this a nugget (highest priority)?
pub fn is_nugget(block_id: u16) -> bool {
    matches!(block_id, 4154..=4157 | 4162)
}

/// Is this a crystal (lowest priority mineable)?
pub fn is_crystal(block_id: u16) -> bool {
    matches!(block_id, 3974..=3979)
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

                if name.contains("collect") || name.contains("collectable") || type_name == "consumable" {
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
        id == PORTAL_MINE_EXIT_ID || is_collectible(id) || (id >= MINING_GEM_START && id <= MINING_GEM_END)
    })
}

/// Return true when there are no more collectible/mining gem tiles in the provided foreground.
pub fn all_collectibles_cleared(foreground_tiles: &[u16]) -> bool {
    !foreground_tiles.iter().any(|&id| is_collectible(id) || (id >= MINING_GEM_START && id <= MINING_GEM_END))
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
pub fn find_best_target(
    player_map_x: i32,
    player_map_y: i32,
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
) -> Option<(i32, i32)> {
    let mut best_target: Option<(i32, i32, u16, i32)> = None; // (x, y, block_id, distance_sq)

    let search_radius = 255; // Search the entire world
    let min_x = 0;
    let max_x = world_width as i32 - 1;
    let min_y = 0;
    let max_y = world_height as i32 - 1;

    // Prefer collectibles first (user requested). Fall back to legacy mineable IDs.
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            let index = (y as u32 * world_width + x as u32) as usize;
            if let Some(&block_id) = foreground_tiles.get(index) {
                if block_id != 0 && (is_collectible(block_id) || is_mineable(block_id)) {
                    let dx = (x - player_map_x) as i32;
                    let dy = (y - player_map_y) as i32;
                    let dist_sq = dx * dx + dy * dy;

                    let is_better = match best_target {
                        None => true,
                        Some((_, _, prev_id, prev_dist)) => {
                            // Priority Ranking: 
                            // 1. Nuggets (4)
                            // 2. Other Collectibles (3)
                            // 3. Gemstones (2)
                            // 4. Crystals (1)
                            // 5. Others (0)
                            
                            let rank = |id: u16| -> i32 {
                                if is_nugget(id) { 4 }
                                else if is_collectible(id) { 3 }
                                else if is_gemstone(id) { 2 }
                                else if is_crystal(id) { 1 }
                                else { 0 }
                            };

                            let current_rank = rank(block_id);
                            let prev_rank = rank(prev_id);

                            if current_rank > prev_rank {
                                true
                            } else if current_rank < prev_rank {
                                false
                            } else {
                                dist_sq < prev_dist
                            }
                        }
                    };

                    if is_better {
                        best_target = Some((x, y, block_id, dist_sq));
                    }
                }
            }
        }
    }

    best_target.map(|(x, y, _, _)| (x, y))
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

