use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch, RwLock};

use crate::constants::movement;
use crate::logging::Logger;
use crate::models::{SessionStatus};
use crate::protocol;
use crate::session::ControllerEvent;

use super::network::{send_docs_exclusive, send_doc};
use super::publish_state_snapshot;
use super::state::{OutboundHandle, SessionState};

pub(super) async fn record_action(state: &Arc<RwLock<SessionState>>, hint: String) {
    let mut st = state.write().await;
    st.last_action_hint = Some(hint);
    st.last_action_at = Some(Instant::now());
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

                    if let Some(w) = &st.world {
                        (px, py, w.width, w.height, st.world_foreground_tiles.clone(), st.world_background_tiles.clone(), current_world, session_status)
                    } else {
                        (px, py, 0, 0, vec![], vec![], current_world, session_status)
                    }
                };

                if matches!(session_status, SessionStatus::Idle | SessionStatus::Disconnected | SessionStatus::Error) {
                    return Ok(());
                }

                if matches!(session_status, SessionStatus::Connecting | SessionStatus::Authenticating | SessionStatus::Redirecting | SessionStatus::JoiningWorld | SessionStatus::LoadingWorld) {
                    continue;
                }

                let is_in_target = current_world.as_deref().map(|w| w.to_uppercase() == target_world.to_uppercase()).unwrap_or(false);
                if !is_in_target {
                    if !joined_target {
                        joined_target = true;
                        let _ = controller_tx.send(ControllerEvent::Command(crate::session::state::SessionCommand::JoinWorld { 
                            world: target_world.clone(), 
                            instance: false 
                        })).await;
                    }
                    continue;
                }

                if world_width == 0 {
                    continue;
                }

                // Check pending hits, if recent, wait.
                let is_pending = last_hit.map(|last| last.elapsed() < Duration::from_millis(200)).unwrap_or(false);
                if is_pending {
                    continue;
                }

                // Scan for the next tile
                // from y=58 down to 3
                // from x=0 to 79
                let mut target_tile: Option<(i32, i32)> = None;

                for y in (3..=58).rev() {
                    for x in 0..80 {
                        if x >= world_width as i32 || y >= world_height as i32 {
                            continue;
                        }
                        let idx = (y as u32 * world_width + x as u32) as usize;
                        let fg = foreground.get(idx).copied().unwrap_or(0);
                        let bg = background.get(idx).copied().unwrap_or(0);
                        
                        // Ignore bedrock etc. Bedrock is usually 8.
                        if fg == 8 {
                            continue;
                        }

                        if fg != 0 || bg != 0 {
                            // Check if path to (x, y+1) exists
                            let stand_x = x;
                            let stand_y = y + 1;
                            
                            if stand_y >= world_height as i32 {
                                continue;
                            }
                            
                            // To be able to hit, we must pathfind to stand_x, stand_y
                            if let Some(_) = crate::pathfinding::astar::find_tile_path(
                                &foreground,
                                world_width as usize,
                                world_height as usize,
                                (player_x, player_y),
                                (stand_x, stand_y)
                            ) {
                                target_tile = Some((x, y));
                                break;
                            }
                        }
                    }
                    if target_tile.is_some() {
                        break;
                    }
                }

                match target_tile {
                    Some((tx, ty)) => {
                        let stand_x = tx;
                        let stand_y = ty + 1;

                        if player_x == stand_x && player_y == stand_y {
                            // We are at the right position, hit it!
                            let dir = if tx > player_x { movement::DIR_RIGHT } else if tx < player_x { movement::DIR_LEFT } else { movement::DIR_LEFT };
                            
                            logger.info("autoclear", Some(&session_id), format!("CLEARING: Hit at ({}, {})", tx, ty));
                            let hit_pkts = protocol::make_mine_hit_stationary(
                                player_x, player_y,
                                tx, ty,
                                dir,
                            );
                            let _ = send_docs_exclusive(outbound_tx, hit_pkts).await;
                            record_action(state, format!("autoclear hit ({tx},{ty})")).await;
                            last_hit = Some(Instant::now());
                        } else {
                            // Pathfind to stand_x, stand_y
                            if let Some(path) = crate::pathfinding::astar::find_tile_path(
                                &foreground,
                                world_width as usize,
                                world_height as usize,
                                (player_x, player_y),
                                (stand_x, stand_y)
                            ) {
                                if path.len() > 1 {
                                    let next_step = path[1];
                                    let dir = if next_step.0 > player_x { movement::DIR_RIGHT } else { movement::DIR_LEFT };
                                    let anim = if next_step.1 > player_y { movement::ANIM_FALL } else if next_step.1 < player_y { movement::ANIM_JUMP } else { movement::ANIM_WALK };

                                    let move_pkts = protocol::make_move_to_map_point(player_x, player_y, next_step.0, next_step.1, anim, dir);
                                    let _ = send_docs_exclusive(outbound_tx, move_pkts).await;
                                    record_action(state, format!("autoclear move to ({},{})", next_step.0, next_step.1)).await;
                                    
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
                                }
                            }
                        }
                    }
                    None => {
                        // World is clear, or no pathable blocks down to Y=3.
                        logger.info("autoclear", Some(&session_id), "bot cleared the world!");
                        let _ = controller_tx.send(ControllerEvent::Command(crate::session::state::SessionCommand::Disconnect)).await;
                        return Ok(());
                    }
                }

                publish_state_snapshot(logger, session_id, state).await;
            }
        }
    }
}
