//! Walk/move/punch/place helpers + chat. Operates on session state and
//! emits movement packets via the network senders.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use bson::Document;
use tokio::sync::{watch, RwLock};
use tokio::time::{interval, sleep, MissedTickBehavior};

use crate::constants::{movement, tutorial};
use crate::logging::Logger;
use crate::models::{InventoryItem, LuaTileSnapshot, SessionSnapshot, SessionStatus};
use crate::pathfinding::astar;
use crate::protocol;

use super::network::{
    ensure_not_cancelled, send_doc, send_doc_before_generated, send_docs_exclusive,
    send_docs_immediate, send_scheduler_cmd,
};
use super::state::{OutboundHandle, SchedulerCommand, SchedulerPhase, SessionState};

// tile helpers still live in mod.rs until Task 8 — pull them in via super.
use super::{tile_index, tile_snapshot_at};

pub(super) async fn spam_loop(
    session_id: &str,
    logger: &Logger,
    outbound_tx: &OutboundHandle,
    mut stop_rx: watch::Receiver<bool>,
    message: String,
    delay_ms: u64,
) -> Result<(), String> {
    send_world_chat(session_id, logger, outbound_tx, &message).await?;

    let mut tick = interval(Duration::from_millis(delay_ms));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return Ok(());
                }
            }
            _ = tick.tick() => {
                send_world_chat(session_id, logger, outbound_tx, &message).await?;
            }
        }
    }
}

pub(super) async fn send_world_chat(
    _session_id: &str,
    _logger: &Logger,
    outbound_tx: &OutboundHandle,
    message: &str,
) -> Result<(), String> {
    send_docs_exclusive(
        outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_world_chat(message),
            protocol::make_progress_signal(0),
        ],
    )
    .await
}

pub(super) async fn walk_to_map(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    target_map_x: i32,
    target_map_y: i32,
) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    walk_to_map_cancellable(state, outbound_tx, target_map_x, target_map_y, &cancel).await
}

pub(super) async fn walk_to_map_cancellable(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    target_map_x: i32,
    target_map_y: i32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    ensure_not_cancelled(cancel)?;
    let (start_x, start_y) = {
        let state = state.read().await;
        let x = state
            .player_position
            .map_x
            .unwrap_or(tutorial::PORTAL_APPROACH_X as f64)
            .round() as i32;
        let y = state
            .player_position
            .map_y
            .unwrap_or(tutorial::PORTAL_APPROACH_Y as f64)
            .round() as i32;
        (x, y)
    };

    let path = planned_path(state, (start_x, start_y), (target_map_x, target_map_y)).await;
    let steps = path.unwrap_or_else(|| {
        fallback_straight_line_path((start_x, start_y), (target_map_x, target_map_y))
    });
    let mut last_direction = {
        let state = state.read().await;
        current_facing_direction(&state)
    };

    for window in steps.windows(2) {
        ensure_not_cancelled(cancel)?;
        let [previous, current] = window else {
            continue;
        };

        let direction = if current.0 < previous.0 {
            movement::DIR_LEFT
        } else {
            movement::DIR_RIGHT
        };
        last_direction = direction;

        move_to_map(
            state,
            outbound_tx,
            current.0,
            current.1,
            direction,
            movement::ANIM_WALK,
        )
        .await?;
        sleep(tutorial::walk_step_pause()).await;
    }

    ensure_not_cancelled(cancel)?;
    let (world_x, world_y) = {
        let s = state.read().await;
        (
            s.player_position.world_x.unwrap_or_default(),
            s.player_position.world_y.unwrap_or_default(),
        )
    };
    send_scheduler_cmd(
        outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x,
            world_y,
            is_moving: false,
            anim: movement::ANIM_IDLE,
            direction: last_direction,
        },
    )
    .await?;
    Ok(())
}

pub(super) async fn walk_predefined_path(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    steps: &[(i32, i32)],
) -> Result<(), String> {
    let mut previous = {
        let state = state.read().await;
        (
            state
                .player_position
                .map_x
                .unwrap_or(tutorial::INTRO_PORTAL_WALK_PATH[0].0 as f64)
                .round() as i32,
            state
                .player_position
                .map_y
                .unwrap_or(tutorial::INTRO_PORTAL_WALK_PATH[0].1 as f64)
                .round() as i32,
        )
    };

    for &(map_x, map_y) in steps {
        let direction = if map_x < previous.0 {
            movement::DIR_LEFT
        } else {
            movement::DIR_RIGHT
        };
        move_to_map(
            state,
            outbound_tx,
            map_x,
            map_y,
            direction,
            movement::ANIM_WALK,
        )
        .await?;
        previous = (map_x, map_y);
        sleep(tutorial::walk_step_pause()).await;
    }

    let (world_x, world_y) = {
        let s = state.read().await;
        (
            s.player_position.world_x.unwrap_or_default(),
            s.player_position.world_y.unwrap_or_default(),
        )
    };
    let last_direction = if steps.last().copied().unwrap_or_default().0
        < steps.get(steps.len().saturating_sub(2)).copied().unwrap_or_default().0
    {
        movement::DIR_LEFT
    } else {
        movement::DIR_RIGHT
    };
    send_scheduler_cmd(
        outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x,
            world_y,
            is_moving: false,
            anim: movement::ANIM_IDLE,
            direction: last_direction,
        },
    )
    .await?;
    Ok(())
}

pub(super) async fn manual_move(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    direction: &str,
) -> Result<(), String> {
    let (target_map_x, target_map_y, facing_direction) = next_manual_step(state, direction).await?;

    let is_blocked = {
        let state = state.read().await;
        if let Some(world) = &state.world {
            if let Some(index) = tile_index(world, target_map_x, target_map_y) {
                if let Some(&tile_id) = state.world_foreground_tiles.get(index) {
                    if !astar::is_walkable_tile(tile_id) {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    };

    if is_blocked {
        return Err(format!(
            "cannot move {direction} - tile ({target_map_x}, {target_map_y}) is solid"
        ));
    }

    let (world_x, world_y) = protocol::map_to_world(target_map_x as f64, target_map_y as f64);
    logger.info(
        "movement",
        Some(session_id),
        format!(
            "manual move {direction} -> map=({target_map_x}, {target_map_y}) world=({world_x:.2}, {world_y:.2})"
        ),
    );
    set_local_map_position(logger, session_id, state, target_map_x, target_map_y).await;
    let (world_x, world_y) = protocol::map_to_world(target_map_x as f64, target_map_y as f64);
    send_doc_before_generated(outbound_tx, protocol::make_map_point(target_map_x, target_map_y)).await?;
    send_scheduler_cmd(
        outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x,
            world_y,
            is_moving: true,
            anim: movement::ANIM_WALK,
            direction: facing_direction,
        },
    )
    .await?;
    sleep(tutorial::walk_step_pause()).await;
    send_scheduler_cmd(
        outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x,
            world_y,
            is_moving: false,
            anim: movement::ANIM_IDLE,
            direction: facing_direction,
        },
    )
    .await?;
    sleep(Duration::from_millis(120)).await;

    Ok(())
}

pub(super) async fn manual_punch(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    offset_x: i32,
    offset_y: i32,
) -> Result<(), String> {
    let (target_map_x, target_map_y, facing_direction) =
        punch_target_from_offset(state, offset_x, offset_y).await?;
    let (fg, water, bg) = {
        let state = state.read().await;
        let snapshot = tile_snapshot_at(&state, target_map_x, target_map_y)
            .unwrap_or(LuaTileSnapshot {
                foreground: 0,
                background: 0,
                water: 0,
                wiring: 0,
                ready_to_harvest: false,
            });
        (snapshot.foreground, snapshot.water, snapshot.background)
    };
    let (layer, hit_doc) = if fg != 0 {
        ("fg", protocol::make_hit_block(target_map_x, target_map_y))
    } else if water != 0 {
        (
            "water",
            protocol::make_hit_block_water(target_map_x, target_map_y),
        )
    } else if bg != 0 {
        (
            "bg",
            protocol::make_hit_block_background(target_map_x, target_map_y),
        )
    } else {
        ("fg", protocol::make_hit_block(target_map_x, target_map_y))
    };
    logger.info(
        "punch",
        Some(session_id),
        format!(
            "manual punch offset=({offset_x}, {offset_y}) -> target=({target_map_x}, {target_map_y}) layer={layer}"
        ),
    );
    send_docs_exclusive(
        outbound_tx,
        vec![
            movement_doc(state, movement::ANIM_PUNCH, facing_direction).await,
            hit_doc,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn manual_place(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    offset_x: i32,
    offset_y: i32,
    block_id: i32,
) -> Result<(), String> {
    let (target_map_x, target_map_y, facing_direction) =
        punch_target_from_offset(state, offset_x, offset_y).await?;
    logger.info(
        "place",
        Some(session_id),
        format!(
            "manual place block={block_id} offset=({offset_x}, {offset_y}) -> target=({target_map_x}, {target_map_y})"
        ),
    );
    send_docs_exclusive(
        outbound_tx,
        vec![
            movement_doc(state, movement::ANIM_IDLE, facing_direction).await,
            protocol::make_place_block(target_map_x, target_map_y, block_id),
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn next_manual_step(
    state: &Arc<RwLock<SessionState>>,
    direction: &str,
) -> Result<(i32, i32, i32), String> {
    let state = state.read().await;
    let current_map_x = state
        .player_position
        .map_x
        .ok_or_else(|| "player map x is not known yet".to_string())?
        .round() as i32;
    let current_map_y = state
        .player_position
        .map_y
        .ok_or_else(|| "player map y is not known yet".to_string())?
        .round() as i32;

    let (dx, dy, facing_direction) = match direction {
        "left" => (-1, 0, movement::DIR_LEFT),
        "right" => (1, 0, movement::DIR_RIGHT),
        "up" => (0, 1, current_facing_direction(&state)),
        "down" => (0, -1, current_facing_direction(&state)),
        _ => return Err(format!("unsupported movement direction: {direction}")),
    };

    Ok((current_map_x + dx, current_map_y + dy, facing_direction))
}

pub(super) async fn punch_target_from_offset(
    state: &Arc<RwLock<SessionState>>,
    offset_x: i32,
    offset_y: i32,
) -> Result<(i32, i32, i32), String> {
    let state = state.read().await;
    let current_map_x = state
        .player_position
        .map_x
        .ok_or_else(|| "player map x is not known yet".to_string())?
        .round() as i32;
    let current_map_y = state
        .player_position
        .map_y
        .ok_or_else(|| "player map y is not known yet".to_string())?
        .round() as i32;
    let facing_direction = if offset_x < 0 {
        movement::DIR_LEFT
    } else if offset_x > 0 {
        movement::DIR_RIGHT
    } else {
        current_facing_direction(&state)
    };

    Ok((
        current_map_x + offset_x,
        current_map_y + offset_y,
        facing_direction,
    ))
}

pub(super) fn current_facing_direction(state: &SessionState) -> i32 {
    match state.current_direction {
        movement::DIR_LEFT => movement::DIR_LEFT,
        _ => movement::DIR_RIGHT,
    }
}

pub(super) fn drop_target_tile(state: &SessionState) -> Result<(i32, i32), String> {
    let current_map_x = state
        .player_position
        .map_x
        .ok_or_else(|| "player map x is not known yet".to_string())?
        .round() as i32;
    let current_map_y = state
        .player_position
        .map_y
        .ok_or_else(|| "player map y is not known yet".to_string())?
        .round() as i32;
    let dx = if current_facing_direction(state) == movement::DIR_LEFT {
        -1
    } else {
        1
    };
    Ok((current_map_x + dx, current_map_y))
}

pub(super) async fn planned_path(
    state: &Arc<RwLock<SessionState>>,
    start: (i32, i32),
    goal: (i32, i32),
) -> Option<Vec<(i32, i32)>> {
    let state = state.read().await;
    let world = state.world.as_ref()?;
    let width = world.width as usize;
    let height = world.height as usize;
    let tiles = &state.world_foreground_tiles;

    astar::find_path(width, height, start, goal, |x, y| {
        if is_walkable_map_position(tiles, width, height, start, goal, x, y) {
            Some(1)
        } else {
            None
        }
    })
}

pub(super) fn is_walkable_map_position(
    tiles: &[u16],
    width: usize,
    height: usize,
    start: (i32, i32),
    goal: (i32, i32),
    x: i32,
    y: i32,
) -> bool {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return false;
    }
    if (x, y) == start || (x, y) == goal {
        return true;
    }

    let index = y as usize * width + x as usize;
    matches!(tiles.get(index), Some(0))
}

pub(super) fn fallback_straight_line_path(start: (i32, i32), goal: (i32, i32)) -> Vec<(i32, i32)> {
    let mut path = vec![start];
    let mut current_x = start.0;
    let mut current_y = start.1;

    while current_x != goal.0 || current_y != goal.1 {
        if current_x < goal.0 {
            current_x += 1;
        } else if current_x > goal.0 {
            current_x -= 1;
        } else if current_y < goal.1 {
            current_y += 1;
        } else if current_y > goal.1 {
            current_y -= 1;
        }
        path.push((current_x, current_y));
    }

    path
}

pub(super) async fn move_to_map(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    map_x: i32,
    map_y: i32,
    direction: i32,
    anim: i32,
) -> Result<(), String> {
    let (world_x, world_y) = {
        let mut state = state.write().await;
        let (world_x, world_y) = protocol::map_to_world(map_x as f64, map_y as f64);
        state.player_position.map_x = Some(map_x as f64);
        state.player_position.map_y = Some(map_y as f64);
        state.player_position.world_x = Some(world_x);
        state.player_position.world_y = Some(world_y);
        state.current_direction = direction;
        (world_x, world_y)
    };
    // Declare the step to the server (once per step).
    send_doc_before_generated(outbound_tx, protocol::make_map_point(map_x, map_y)).await?;
    // Hand position to the scheduler so movement updates continue while walking.
    send_scheduler_cmd(
        outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x,
            world_y,
            is_moving: anim != movement::ANIM_IDLE,
            anim,
            direction,
        },
    )
    .await
}

pub(super) async fn wait_for_map_position(
    state: &Arc<RwLock<SessionState>>,
    target_map_x: i32,
    target_map_y: i32,
    tolerance: f64,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let state = state.read().await;
            if let (Some(map_x), Some(map_y)) =
                (state.player_position.map_x, state.player_position.map_y)
            {
                let dx = (map_x - target_map_x as f64).abs();
                let dy = (map_y - target_map_y as f64).abs();
                if dx <= tolerance && dy <= tolerance {
                    return Ok(());
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting to reach map position ({target_map_x}, {target_map_y})"
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

pub(super) async fn movement_doc(state: &Arc<RwLock<SessionState>>, anim: i32, direction: i32) -> Document {
    let state = state.read().await;
    let world_x = state.player_position.world_x.unwrap_or_else(|| {
        protocol::map_to_world(
            tutorial::TUTORIAL_LANDING_X as f64,
            tutorial::TUTORIAL_LANDING_Y as f64,
        )
        .0
    });
    let world_y = state.player_position.world_y.unwrap_or_else(|| {
        protocol::map_to_world(
            tutorial::TUTORIAL_LANDING_X as f64,
            tutorial::TUTORIAL_LANDING_Y as f64,
        )
        .1
    });
    protocol::make_movement_packet(world_x, world_y, anim, direction, false)
}

pub(super) async fn set_local_map_position(
    logger: &Logger,
    session_id: &str,
    state: &Arc<RwLock<SessionState>>,
    map_x: i32,
    map_y: i32,
) {
    let (world_x, world_y) = protocol::map_to_world(map_x as f64, map_y as f64);
    {
        let mut state = state.write().await;
        state.player_position.map_x = Some(map_x as f64);
        state.player_position.map_y = Some(map_y as f64);
        state.player_position.world_x = Some(world_x);
        state.player_position.world_y = Some(world_y);
    }
    logger.state(
        Some(session_id),
        format!(
            "reflected spawn pot choice at map=({map_x}, {map_y}) world=({world_x:.2}, {world_y:.2})"
        ),
    );
    let snapshot = {
        let state = state.read().await;
        SessionSnapshot {
            id: session_id.to_string(),
            status: state.status.clone(),
            device_id: state.device_id.clone(),
            current_host: state.current_host.clone(),
            current_port: state.current_port,
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

pub(super) async fn set_local_world_position(
    logger: &Logger,
    session_id: &str,
    state: &Arc<RwLock<SessionState>>,
    world_x: f64,
    world_y: f64,
) {
    let (map_x, map_y) = protocol::world_to_map(world_x, world_y);
    {
        let mut state = state.write().await;
        state.player_position.map_x = Some(map_x);
        state.player_position.map_y = Some(map_y);
        state.player_position.world_x = Some(world_x);
        state.player_position.world_y = Some(world_y);
    }
    logger.state(
        Some(session_id),
        format!(
            "reflected scripted world move at map=({map_x:.2}, {map_y:.2}) world=({world_x:.2}, {world_y:.2})"
        ),
    );
    let snapshot = {
        let state = state.read().await;
        SessionSnapshot {
            id: session_id.to_string(),
            status: state.status.clone(),
            device_id: state.device_id.clone(),
            current_host: state.current_host.clone(),
            current_port: state.current_port,
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

