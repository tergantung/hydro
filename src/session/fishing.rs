//! Fishing automation: bait selection, gauge simulation, and the fishing loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bson::Document;
use tokio::sync::{watch, RwLock};
use tokio::time::{interval, sleep, MissedTickBehavior};

use crate::constants::fishing as fishing_consts;
use crate::logging::Logger;
use crate::models::{BotTarget, WorldSnapshot};
use crate::protocol;

use super::movement::{movement_doc, walk_to_map_cancellable};
use super::network::{send_doc, send_docs, send_docs_exclusive};
use super::state::{
    FishingAutomationState, FishingPhase, FishingTarget, NamedInventoryEntry,
    OutboundHandle, SessionState,
};
use super::{block_name_for, find_inventory_bait, normalize_block_name, publish_state_snapshot};

pub(super) fn find_fishing_map_point(
    world: Option<&WorldSnapshot>,
    _water_tiles: &[u16],
    player_x: i32,
    player_y: i32,
    direction: &str,
) -> Result<(i32, i32), String> {
    let world = world.ok_or_else(|| "join a world before starting fishing".to_string())?;
    if world.width == 0 || world.height == 0 {
        return Err("world data is not loaded yet".to_string());
    }

    let width = world.width as i32;
    let height = world.height as i32;
    let target_x = player_x + if direction == "left" { -1 } else { 1 };
    let target_y = player_y - 1;
    if target_x < 0 || target_x >= width || target_y < 0 || target_y >= height {
        return Err(format!(
            "fishing target ({target_x}, {target_y}) is outside world bounds"
        ));
    }
    Ok((target_x, target_y))
}

pub(super) fn rod_family_name(rod_block: Option<i32>) -> &'static str {
    let rod = rod_block.unwrap_or(2406);
    let index = if (2406..=2421).contains(&rod) {
        (rod - 2406) % 4
    } else {
        0
    };
    match index {
        0 => "bamboo",
        1 => "fiberglass",
        2 => "carbon",
        3 => "titanium",
        _ => "bamboo",
    }
}

pub(super) fn initialize_fishing_gauge(fishing: &mut FishingAutomationState, now: Instant) {
    let rod_block = fishing.rod_block.unwrap_or(2406);
    let fish_name = fishing
        .fish_block
        .and_then(|id| block_name_for(id as u16))
        .unwrap_or_default();
    let normalized = normalize_block_name(&fish_name);
    let bucket = fishing_consts::fish_bucket_from_name(&normalized);
    let rod_profile = fishing_consts::rod_profile(rod_block);

    fishing.sim_overlap_threshold = 0.095 + (rod_profile.slider_size * 0.035);
    fishing.sim_fill_rate = rod_profile.fill_multiplier * 0.10;
    fishing.sim_target_speed = rod_profile.slider_speed * 0.20;
    fishing.sim_fish_move_speed = bucket.fish_move_speed;
    fishing.sim_run_frequency = bucket.run_frequency;
    fishing.sim_pull_strength = fishing_consts::pull_strength(bucket, rod_family_name(Some(rod_block)));
    fishing.sim_min_land_delay = bucket.min_land_delay;
    fishing.sim_last_at = Some(now);
    fishing.sim_fish_position = fishing_consts::DEFAULT_FISH_POSITION;
    fishing.sim_target_position = fishing_consts::DEFAULT_TARGET_POSITION;
    fishing.sim_progress = fishing_consts::DEFAULT_PROGRESS;
    fishing.sim_phase = 0.0;
    fishing.sim_overlap = false;
    fishing.sim_ready_since = None;
    fishing.sim_difficulty_meter = 0.0;
    fishing.sim_size_multiplier = 0.0;
    fishing.sim_drag_extra = fishing_consts::DEFAULT_DRAG_EXTRA;
    fishing.sim_run_active = false;
    fishing.sim_run_until = None;
    fishing.sim_force_land_after = Some(
        now + Duration::from_secs_f64(
            (bucket.min_land_delay + fishing_consts::FORCE_LAND_EXTRA_DELAY_SECS)
                .max(fishing_consts::FORCE_LAND_MIN_SECS),
        ),
    );
    fishing.land_sent = false;
}

pub(super) fn current_fishing_land_values(fishing: &FishingAutomationState) -> (i32, i32, f64) {
    let size_multiplier = fishing
        .sim_size_multiplier
        .clamp(0.001, fishing_consts::MAX_SIZE_MULTIPLIER);
    let difficulty_meter = fishing
        .sim_difficulty_meter
        .clamp(0.001, fishing_consts::MAX_DIFFICULTY_METER);
    let vendor_index = (size_multiplier * 1000.0).max(1.0) as i32;
    let index_key = (difficulty_meter * 1000.0).max(1.0) as i32;
    let amount = fishing.sim_fish_position - fishing.sim_drag_extra;
    (vendor_index, index_key, amount)
}

pub(super) fn service_fishing_simulation(
    fishing: &mut FishingAutomationState,
    now: Instant,
) -> Option<Document> {
    if fishing.phase != FishingPhase::GaugeActive || fishing.cleanup_pending {
        return None;
    }

    let Some(last_at) = fishing.sim_last_at else {
        fishing.sim_last_at = Some(now);
        return None;
    };
    let dt = (now - last_at).as_secs_f64().clamp(0.0, 0.25);
    fishing.sim_last_at = Some(now);
    if dt <= 0.0 {
        return None;
    }

    fishing.sim_phase += dt;
    let prev_fish = fishing.sim_fish_position;
    let prev_target = fishing.sim_target_position;
    let move_speed = fishing.sim_fish_move_speed;
    let run_frequency = fishing.sim_run_frequency;
    let base_wave = 0.18 + (move_speed * 0.05);
    let burst_wave = 0.08 + (run_frequency * 1.1);

    if !fishing.sim_run_active
        && fishing
            .gauge_entered_at
            .map(|entered| (now - entered).as_secs_f64() >= fishing_consts::RUN_START_AFTER_SECS)
            .unwrap_or(false)
        && (fishing.sim_phase * (0.75 + move_speed)).sin() > (0.985 - run_frequency)
    {
        fishing.sim_run_active = true;
        fishing.sim_run_until = Some(now + Duration::from_millis(fishing_consts::RUN_DURATION_MS));
    }

    let run_boost = if fishing.sim_run_active { 0.22 } else { 0.0 };
    let center =
        0.5 + (base_wave + run_boost) * (fishing.sim_phase * (0.9 + move_speed * 0.55)).sin();
    let burst = burst_wave * (fishing.sim_phase * (2.3 + move_speed * 1.1)).sin();
    let fish = (center + burst).clamp(0.0, 1.0);
    if fishing.sim_run_active
        && fishing
            .sim_run_until
            .map(|until| now >= until)
            .unwrap_or(false)
    {
        fishing.sim_run_active = false;
        fishing.sim_run_until = None;
    }

    let distance = (fish - prev_target).abs();
    let should_overlap = distance <= (fishing.sim_overlap_threshold * 1.35);
    let force_finish = fishing
        .sim_force_land_after
        .map(|deadline| now >= deadline)
        .unwrap_or(false);

    let mut target = prev_target;
    if force_finish {
        target = fish;
    } else if should_overlap {
        let step = fishing.sim_target_speed * dt;
        target = if fish > target {
            (target + step).min(fish)
        } else {
            (target - step).max(fish)
        };
    } else {
        let step = (fishing.sim_target_speed * 0.35) * dt;
        target = if fish > target {
            (target + step).min(fish)
        } else {
            (target - step).max(fish)
        };
    }
    target = target.clamp(0.0, 1.0);

    fishing.sim_fish_position = fish;
    fishing.sim_target_position = target;
    fishing.sim_difficulty_meter += (target - prev_target).abs();
    fishing.sim_size_multiplier += (fish - prev_fish).abs();
    fishing.sim_drag_extra = fish + 0.5;

    let off_distance = (fish - target).abs();
    let is_overlapping = off_distance <= fishing.sim_overlap_threshold;
    if is_overlapping != fishing.sim_overlap {
        fishing.sim_overlap = is_overlapping;
        return Some(if is_overlapping {
            protocol::make_fish_on_area()
        } else {
            protocol::make_fish_off_area(off_distance)
        });
    }

    if force_finish {
        fishing.sim_progress = (fishing.sim_progress.max(0.985)
            + (fishing.sim_fill_rate * 2.5).max(0.22) * dt)
            .clamp(0.0, 1.0);
    } else if is_overlapping {
        fishing.sim_progress = (fishing.sim_progress + fishing.sim_fill_rate * dt).clamp(0.0, 1.0);
    } else {
        let drain_rate = ((((off_distance * 2.5) + 0.5) * fishing.sim_pull_strength) * 0.05) * dt;
        fishing.sim_progress = (fishing.sim_progress - drain_rate).clamp(0.0, 1.0);
    }

    let can_land = fishing.sim_progress >= 0.999
        && is_overlapping
        && fishing
            .gauge_entered_at
            .map(|entered| (now - entered).as_secs_f64() >= fishing.sim_min_land_delay)
            .unwrap_or(false);

    if can_land {
        if fishing.sim_ready_since.is_none() {
            fishing.sim_ready_since = Some(now);
        } else if !fishing.land_sent
            && fishing
                .sim_ready_since
                .map(|ready_since| {
                    (now - ready_since).as_secs_f64() >= fishing_consts::READY_TO_LAND_DELAY_SECS
                })
                .unwrap_or(false)
        {
            let (vendor_index, index_key, amount) = current_fishing_land_values(fishing);
            fishing.land_sent = true;
            return Some(protocol::make_fishing_land_action(
                vendor_index,
                index_key,
                amount,
            ));
        }
    } else {
        fishing.sim_ready_since = None;
    }

    None
}

pub(super) async fn fishing_loop(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
    mut stop_rx: watch::Receiver<bool>,
    target: FishingTarget,
) -> Result<(), String> {
    loop {
        if *stop_rx.borrow() {
            return stop_fishing_game(state, outbound_tx).await;
        }

        let bait = match consume_fishing_bait(state, &target.bait_query).await {
            Ok(bait) => bait,
            Err(_) => {
                {
                    let mut session = state.write().await;
                    session.fishing = FishingAutomationState::default();
                }
                publish_state_snapshot(logger, session_id, state).await;
                logger.state(
                    Some(session_id),
                    format!(
                        "auto-fishing stopped: no more '{}' found in inventory",
                        target.bait_query
                    ),
                );
                return Ok(());
            }
        };

        {
            let mut session = state.write().await;
            session.fishing = FishingAutomationState::default();
            session.fishing.active = true;
            session.fishing.phase = FishingPhase::WaitingForHook;
            session.fishing.target_map_x = Some(target.map_x);
            session.fishing.target_map_y = Some(target.map_y);
            session.fishing.bait_name = Some(bait.name.clone());
            session.fishing.last_result = None;
            session.current_target = Some(BotTarget::Fishing {
                x: target.map_x,
                y: target.map_y,
            });
        }
        publish_state_snapshot(logger, session_id, state).await;

        logger.state(
            Some(session_id),
            format!(
                "starting fishing at map=({}, {}) dir={} bait={}",
                target.map_x, target.map_y, target.direction, bait.name
            ),
        );

        send_docs_exclusive(
            outbound_tx,
            vec![
                protocol::make_select_belt_item(bait.inventory_key),
                protocol::make_try_to_fish_from_map_point(
                    target.map_x,
                    target.map_y,
                    bait.block_id as i32,
                ),
                protocol::make_start_fishing_game(
                    target.map_x,
                    target.map_y,
                    bait.block_id as i32,
                ),
            ],
        )
        .await?;

        loop {
            if *stop_rx.borrow() {
                return stop_fishing_game(state, outbound_tx).await;
            }

            let fishing = state.read().await.fishing.clone();
            let phase = fishing.phase;
            if phase == FishingPhase::CleanupPending || phase == FishingPhase::Completed {
                break;
            }
            if !fishing.active {
                return Err("fishing was reset before hook prompt".to_string());
            }
            if phase == FishingPhase::HookPrompted || phase == FishingPhase::GaugeActive {
                break;
            }
            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        return stop_fishing_game(state, outbound_tx).await;
                    }
                }
                _ = sleep(Duration::from_millis(100)) => {}
            }
        }

        let hook_sent = state.read().await.fishing.hook_sent;
        if !hook_sent {
            {
                let mut session = state.write().await;
                session.fishing.hook_sent = true;
            }
            send_docs_exclusive(outbound_tx, vec![protocol::make_fishing_hook_action()]).await?;
        }

        let mut gauge_tick = interval(Duration::from_millis(50));
        gauge_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            if *stop_rx.borrow() {
                return stop_fishing_game(state, outbound_tx).await;
            }

            let fishing = state.read().await.fishing.clone();
            let phase = fishing.phase;
            if phase == FishingPhase::CleanupPending {
                continue;
            }
            if phase == FishingPhase::Completed {
                break;
            }
            if !fishing.active {
                return Err("fishing was reset before reward".to_string());
            }

            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        return stop_fishing_game(state, outbound_tx).await;
                    }
                }
                _ = gauge_tick.tick() => {
                    let packet = {
                        let mut session = state.write().await;
                        service_fishing_simulation(&mut session.fishing, Instant::now())
                    };
                    if let Some(packet) = packet {
                        send_docs(outbound_tx, vec![packet]).await?;
                    }
                }
            }
        }

        // 4. Rate limiting: match or exceed the game client's walking speed
        // to avoid ER=7 (SpeedHack) kicks. 180-200ms is standard.
        sleep(Duration::from_millis(200)).await;
    }
}

pub(super) async fn consume_fishing_bait(
    state: &Arc<RwLock<SessionState>>,
    bait_query: &str,
) -> Result<NamedInventoryEntry, String> {
    let mut session = state.write().await;
    let bait = find_inventory_bait(&session.inventory, bait_query)?;
    let item = session
        .inventory
        .iter_mut()
        .find(|item| item.inventory_key == bait.inventory_key)
        .ok_or_else(|| format!("bait '{bait_query}' was not found in inventory"))?;
    if item.amount == 0 {
        return Err(format!("bait '{bait_query}' was not found in inventory"));
    }
    item.amount -= 1;
    session.inventory.retain(|item| item.amount > 0);
    Ok(bait)
}

pub(super) async fn stop_fishing_game(
    state: &Arc<RwLock<SessionState>>,
    outbound_tx: &OutboundHandle,
) -> Result<(), String> {
    {
        let mut session = state.write().await;
        session.fishing = FishingAutomationState::default();
    }
    send_docs_exclusive(
        outbound_tx,
        vec![
            protocol::make_fishing_cleanup_action(),
            protocol::make_stop_fishing_game(false),
            protocol::make_stop_fishing_game(true),
        ],
    )
    .await
}

