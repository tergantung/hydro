/// Packet-based automine automation: tile predicates, target selection,
/// and pathfinding glue. The actual loop driving these helpers lives in
/// `mod.rs::automine_loop` (will be merged into this file by Task 5).

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
