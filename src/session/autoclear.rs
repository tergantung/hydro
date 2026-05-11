use std::sync::Arc;
use std::time::{Duration, Instant};

use bson::Document;
use tokio::sync::{mpsc, watch, RwLock};

use crate::constants::movement;
use crate::logging::Logger;
use crate::models::SessionStatus;
use crate::pathfinding::astar;
use crate::protocol;
use crate::session::ControllerEvent;

use super::network::send_docs_exclusive;
use super::publish_state_snapshot;
use super::state::{OutboundHandle, SessionState};

pub(super) async fn record_action(state: &Arc<RwLock<SessionState>>, hint: String) {
    let mut st = state.write().await;
    st.last_action_hint = Some(hint);
    st.last_action_at = Some(Instant::now());
}

struct ClearTarget {
    x: i32,
    y: i32,
    stand_x: i32,
    stand_y: i32,
    hit_background: bool,
    path: Vec<(i32, i32)>,
}

fn tile_at(tiles: &[u16], width: u32, height: u32, x: i32, y: i32) -> Option<u16> {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return None;
    }
    tiles.get((y as u32 * width + x as u32) as usize).copied()
}

fn is_unclearable_foreground(block_id: u16) -> bool {
    matches!(block_id, 3 | 8 | 3987 | 3988 | 3990 | 3993 | 4103)
}

fn is_walkable_clear_position(tiles: &[u16], width: u32, height: u32, x: i32, y: i32) -> bool {
    tile_at(tiles, width, height, x, y)
        .map(astar::is_walkable_tile)
        .unwrap_or(false)
}

fn find_clear_path(
    tiles: &[u16],
    width: u32,
    height: u32,
    start: (i32, i32),
    goal: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    astar::find_path(width as usize, height as usize, start, goal, |x, y| {
        if (x, y) == start || is_walkable_clear_position(tiles, width, height, x, y) {
            Some(1)
        } else {
            None
        }
    })
}

fn find_reachable_clear_target(
    player_x: i32,
    player_y: i32,
    width: u32,
    height: u32,
    foreground: &[u16],
    background: &[u16],
) -> Option<ClearTarget> {
    let max_x = width as i32 - 1;
    let max_y = height as i32 - 1;

    for y in (0..=max_y).rev() {
        for x in 0..=max_x {
            let fg = tile_at(foreground, width, height, x, y).unwrap_or(0);
            let bg = tile_at(background, width, height, x, y).unwrap_or(0);

            if fg == 0 && bg == 0 {
                continue;
            }
            if fg != 0 && is_unclearable_foreground(fg) {
                continue;
            }

            let hit_background = fg == 0 && bg != 0;
            for (stand_x, stand_y) in [(x, y - 1), (x - 1, y), (x + 1, y), (x, y + 1)] {
                if !is_walkable_clear_position(foreground, width, height, stand_x, stand_y) {
                    continue;
                }
                if let Some(path) = find_clear_path(
                    foreground,
                    width,
                    height,
                    (player_x, player_y),
                    (stand_x, stand_y),
                ) {
                    return Some(ClearTarget {
                        x,
                        y,
                        stand_x,
                        stand_y,
                        hit_background,
                        path,
                    });
                }
            }
        }
    }

    None
}

fn make_clear_hit_packets(
    player_x: i32,
    player_y: i32,
    target_x: i32,
    target_y: i32,
    direction: i32,
    hit_background: bool,
) -> Vec<Document> {
    let (world_x, world_y) = protocol::map_to_world(player_x as f64, player_y as f64);
    let hit_doc = if hit_background {
        protocol::make_hit_block_background(target_x, target_y)
    } else {
        protocol::make_hit_block(target_x, target_y)
    };
    vec![
        protocol::make_movement_packet(world_x, world_y, movement::ANIM_HIT, direction, false),
        hit_doc.clone(),
        hit_doc,
        protocol::make_st(),
    ]
}

pub(super) async fn autoclear_loop(
    target_world: String,
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    mut stop_rx: watch::Receiver<bool>,
    controller_tx: mpsc::Sender<ControllerEvent>,
) -> Result<(), String> {
    let mut last_tick = Instant::now();
    let mut joined_target = false;

    // We store the last hit to avoid hitting too fast.
    let mut last_hit: Option<Instant> = None;

    loop {
        // dynamic delay ~350ms to be safe
        let base_delay = 350;
        let sleep_duration = (last_tick + Duration::from_millis(base_delay))
            .saturating_duration_since(Instant::now());

        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(sleep_duration) => {
                last_tick = Instant::now();

                let (player_x, player_y, world_width, world_height, foreground, background, current_world, session_status) = {
                    let st = state.read().await;
                    let px = st.player_position.map_x.unwrap_or(0.0) as i32;
                    let py = st.player_position.map_y.unwrap_or(0.0) as i32;
                    let current_world = st.current_world.clone();
                    let session_status = st.status.clone();

                    if let Some(world) = &st.world {
                        (
                            px,
                            py,
                            world.width,
                            world.height,
                            st.world_foreground_tiles.clone(),
                            st.world_background_tiles.clone(),
                            current_world,
                            session_status,
                        )
                    } else {
                        (px, py, 0, 0, vec![], vec![], current_world, session_status)
                    }
                };

                if matches!(
                    session_status,
                    SessionStatus::Idle | SessionStatus::Disconnected | SessionStatus::Error
                ) {
                    return Ok(());
                }

                if matches!(
                    session_status,
                    SessionStatus::Connecting
                        | SessionStatus::Authenticating
                        | SessionStatus::Redirecting
                        | SessionStatus::JoiningWorld
                        | SessionStatus::LoadingWorld
                ) {
                    continue;
                }

                let is_in_target = current_world
                    .as_deref()
                    .map(|world| world.eq_ignore_ascii_case(&target_world))
                    .unwrap_or(false);
                if !is_in_target {
                    if !joined_target {
                        joined_target = true;
                        let _ = controller_tx
                            .send(ControllerEvent::Command(
                                crate::session::state::SessionCommand::JoinWorld {
                                    world: target_world.clone(),
                                    instance: false,
                                },
                            ))
                            .await;
                    }
                    continue;
                }

                if world_width == 0 {
                    continue;
                }

                // Check pending hits, if recent, wait.
                let is_pending = last_hit
                    .map(|last| last.elapsed() < Duration::from_millis(200))
                    .unwrap_or(false);
                if is_pending {
                    continue;
                }

                // Scan top-down for the next reachable foreground/background tile.
                let target_tile = find_reachable_clear_target(
                    player_x,
                    player_y,
                    world_width,
                    world_height,
                    &foreground,
                    &background,
                );

                match target_tile {
                    Some(target) => {
                        if player_x == target.stand_x && player_y == target.stand_y {
                            // We are at the right position, hit it!
                            let dir = if target.x > player_x {
                                movement::DIR_RIGHT
                            } else if target.x < player_x {
                                movement::DIR_LEFT
                            } else {
                                movement::DIR_LEFT
                            };

                            logger.info(
                                "autoclear",
                                Some(&session_id),
                                format!("CLEARING: Hit at ({}, {})", target.x, target.y),
                            );
                            let hit_pkts = make_clear_hit_packets(
                                player_x,
                                player_y,
                                target.x,
                                target.y,
                                dir,
                                target.hit_background,
                            );
                            let _ = send_docs_exclusive(outbound_tx, hit_pkts).await;
                            record_action(
                                state,
                                format!("autoclear hit ({},{})", target.x, target.y),
                            )
                            .await;
                            last_hit = Some(Instant::now());
                        } else {
                            // Pathfind to stand_x, stand_y
                            if target.path.len() > 1 {
                                let next_step = target.path[1];
                                let dir = if next_step.0 > player_x {
                                    movement::DIR_RIGHT
                                } else if next_step.0 < player_x {
                                    movement::DIR_LEFT
                                } else if target.x >= player_x {
                                    movement::DIR_RIGHT
                                } else {
                                    movement::DIR_LEFT
                                };
                                let anim = if next_step.1 > player_y {
                                    movement::ANIM_FALL
                                } else if next_step.1 < player_y {
                                    movement::ANIM_JUMP
                                } else {
                                    movement::ANIM_WALK
                                };

                                let move_pkts = protocol::make_move_to_map_point(
                                    player_x,
                                    player_y,
                                    next_step.0,
                                    next_step.1,
                                    anim,
                                    dir,
                                );
                                let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                                record_action(
                                    state,
                                    format!("autoclear move to ({},{})", next_step.0, next_step.1),
                                )
                                .await;

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
                                    st.current_direction = dir;
                                }
                            }
                        }
                    }
                    None => {
                        // World is clear, or no reachable clearable blocks remain.
                        logger.info("autoclear", Some(&session_id), "bot cleared the world!");
                        let _ = controller_tx
                            .send(ControllerEvent::Command(
                                crate::session::state::SessionCommand::Disconnect,
                            ))
                            .await;
                        return Ok(());
                    }
                }

                publish_state_snapshot(logger, session_id, state).await;
            }
        }
    }
}
