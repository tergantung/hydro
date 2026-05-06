//! Tutorial scripted flow: phase-by-phase progression through the
//! TUTORIAL2 world plus the world-join helpers used at session start.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bson::Document;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;

use crate::constants::movement as movement_consts;
use crate::constants::tutorial as tutorial_consts;
use crate::logging::Logger;
use crate::models::{PlayerPosition, SessionStatus};
use crate::protocol;

use super::movement::{
    movement_doc, set_local_map_position, set_local_world_position, walk_predefined_path,
    walk_to_map_cancellable,
};
use super::network::{
    ensure_not_cancelled, send_doc, send_docs, send_docs_exclusive, send_scheduler_cmd,
};
use super::publish_state_snapshot;
use super::state::{
    ControllerEvent, OutboundHandle, SchedulerCommand, SchedulerPhase, SessionCommand, SessionState,
};

pub(super) async fn run_tutorial_script(
    session_id: String,
    logger: Logger,
    state: Arc<RwLock<SessionState>>,
    _controller_tx: mpsc::Sender<ControllerEvent>,
    outbound_tx: OutboundHandle,
) -> Result<(), String> {
    logger.state(
        Some(&session_id),
        "tutorial automation: replaying phase 3/4 world-join flow",
    );

    {
        let mut state = state.write().await;
        state.tutorial_phase4_acknowledged = false;
    }

    let should_send_phase3 = {
        let state = state.read().await;
        state.current_world.as_deref() != Some(tutorial_consts::TUTORIAL_WORLD)
            || state.status == SessionStatus::MenuReady
    };

    if should_send_phase3 {
        {
            let mut state = state.write().await;
            state.current_world = Some(tutorial_consts::TUTORIAL_WORLD.to_string());
            state.pending_world = Some(tutorial_consts::TUTORIAL_WORLD.to_string());
            state.status = SessionStatus::LoadingWorld;
            state.awaiting_ready = false;
            state.world = None;
            state.world_foreground_tiles.clear();
            state.world_background_tiles.clear();
            state.world_water_tiles.clear();
            state.world_wiring_tiles.clear();
            state.collectables.clear();
            state.other_players.clear();
                            state.ai_enemies.clear();
        }
        send_docs_exclusive(
            &outbound_tx,
            {
                let mut docs = protocol::make_enter_world_eid(tutorial_consts::TUTORIAL_WORLD, "Start");
                docs.push(protocol::make_st());
                docs
            },
        )
        .await?;
    }

    wait_for_tutorial_world_ready_to_enter(&state).await?;
    sleep(Duration::from_millis(20)).await;

    send_docs_exclusive(
        &outbound_tx,
        protocol::make_world_enter_ready(tutorial_consts::TUTORIAL_WORLD, 0.40),
    )
    .await?;

    wait_for_tutorial_phase4_ack(&state).await?;

    send_docs_exclusive(&outbound_tx, protocol::make_ready_to_play_with_st()).await?;

    sleep(tutorial_consts::initial_spawn_pause()).await;

    let (spawn_world_x, spawn_world_y) = protocol::map_to_world(
        tutorial_consts::TUTORIAL_SPAWN_MAP_X as f64,
        tutorial_consts::TUTORIAL_SPAWN_MAP_Y as f64,
    );
    let mut spawn_batch = protocol::make_spawn_packets(
        tutorial_consts::TUTORIAL_SPAWN_MAP_X,
        tutorial_consts::TUTORIAL_SPAWN_MAP_Y,
        spawn_world_x,
        spawn_world_y,
    );
    spawn_batch.push(protocol::make_st());
    send_docs_exclusive(&outbound_tx, spawn_batch).await?;

    {
        let mut state = state.write().await;
        state.player_position = PlayerPosition {
            map_x: Some(tutorial_consts::TUTORIAL_SPAWN_MAP_X as f64),
            map_y: Some(tutorial_consts::TUTORIAL_SPAWN_MAP_Y as f64),
            world_x: Some(spawn_world_x),
            world_y: Some(spawn_world_y),
        };
        state.current_direction = movement_consts::DIR_RIGHT;
        state.awaiting_ready = false;
        state.status = SessionStatus::InWorld;
    }
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: spawn_world_x,
            world_y: spawn_world_y,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::SetPhase {
            phase: SchedulerPhase::WorldIdle,
        },
    )
    .await?;

    sleep(tutorial_consts::post_spawn_tstate_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement(), protocol::make_tstate(4)],
    )
    .await?;

    sleep(tutorial_consts::pre_charc_friends_list_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement(), protocol::make_gfli()],
    )
    .await?;

    sleep(tutorial_consts::pre_charc_st_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement(), protocol::make_st()],
    )
    .await?;

    sleep(tutorial_consts::pre_charc_create_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_character_create(
                tutorial_consts::TUTORIAL_GENDER,
                tutorial_consts::TUTORIAL_COUNTRY,
                tutorial_consts::TUTORIAL_SKIN_COLOR,
            ),
            protocol::make_wear_item(tutorial_consts::STARTER_FACE_BLOCK),
            protocol::make_wear_item(tutorial_consts::STARTER_HAIR_BLOCK),
        ],
    )
    .await?;

    wait_for_tutorial_spawn_pod_confirmation(&state).await?;

    sleep(tutorial_consts::post_apu_first_step_pause()).await;
    for (index, (map_x, map_y)) in tutorial_consts::SPAWN_POD_CONFIRM_PATH.iter().enumerate() {
        send_docs_exclusive(
            &outbound_tx,
            vec![protocol::make_map_point(*map_x, *map_y), protocol::make_empty_movement()],
        )
        .await?;

        let (world_x, world_y) = protocol::map_to_world(*map_x as f64, *map_y as f64);
        set_local_map_position(&logger, &session_id, &state, *map_x, *map_y).await;
        {
            let mut state = state.write().await;
            state.current_direction = movement_consts::DIR_RIGHT;
        }
        send_scheduler_cmd(
            &outbound_tx,
            SchedulerCommand::UpdateMovement {
                world_x,
                world_y,
                is_moving: false,
                anim: movement_consts::ANIM_IDLE,
                direction: movement_consts::DIR_RIGHT,
            },
        )
        .await?;

        match index {
            0 => sleep(tutorial_consts::post_apu_second_step_pause()).await,
            1 => sleep(tutorial_consts::post_apu_third_step_pause()).await,
            _ => {}
        }
    }

    sleep(tutorial_consts::post_apu_tstate5_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement(), protocol::make_tstate(5)],
    )
    .await?;

    sleep(tutorial_consts::portal_walk_start_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(43, 44),
            protocol::make_movement_packet(13.80, 13.92, movement_consts::ANIM_WALK, movement_consts::DIR_RIGHT, false),
        ],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 13.80, 13.92).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 13.80,
            world_y: 13.92,
            is_moving: true,
            anim: movement_consts::ANIM_WALK,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_walk_step_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(44, 44),
            protocol::make_movement_packet(14.16, 13.92, movement_consts::ANIM_WALK, movement_consts::DIR_RIGHT, false),
        ],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.16, 13.92).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.16,
            world_y: 13.92,
            is_moving: true,
            anim: movement_consts::ANIM_WALK,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_walk_idle_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_movement_packet(
            14.16,
            13.92,
            movement_consts::ANIM_IDLE,
            movement_consts::DIR_RIGHT,
            false
        )],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.16, 13.92).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.16,
            world_y: 13.92,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_jump_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(44, 45),
            protocol::make_movement_packet(14.19, 14.40, 3, movement_consts::DIR_RIGHT, false),
            protocol::make_audio_player_action(20, 1950),
        ],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.19, 14.40).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.19,
            world_y: 14.40,
            is_moving: true,
            anim: 3,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_land_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(45, 45),
            protocol::make_movement_packet(14.46, 14.38, 4, movement_consts::DIR_RIGHT, false),
        ],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.46, 14.38).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.46,
            world_y: 14.38,
            is_moving: true,
            anim: 4,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_land_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(46, 45),
            protocol::make_movement_packet(14.63, 14.24, movement_consts::ANIM_IDLE, movement_consts::DIR_RIGHT, false),
        ],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.63, 14.24).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.63,
            world_y: 14.24,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_land_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_movement_packet(
            14.63,
            14.24,
            movement_consts::ANIM_IDLE,
            movement_consts::DIR_RIGHT,
            false
        )],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.63, 14.24).await;

    sleep(tutorial_consts::portal_settle_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_movement_packet(
            14.71,
            14.24,
            movement_consts::ANIM_IDLE,
            movement_consts::DIR_RIGHT,
            false
        )],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.71, 14.24).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.71,
            world_y: 14.24,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_walk_step_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_movement_packet(
            14.75,
            14.24,
            movement_consts::ANIM_IDLE,
            movement_consts::DIR_RIGHT,
            false
        )],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.75, 14.24).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: 14.75,
            world_y: 14.24,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    )
    .await?;

    sleep(tutorial_consts::portal_land_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_movement_packet(
            14.75,
            14.24,
            movement_consts::ANIM_IDLE,
            movement_consts::DIR_RIGHT,
            false
        )],
    )
    .await?;
    set_local_world_position(&logger, &session_id, &state, 14.75, 14.24).await;

    sleep(tutorial_consts::portal_ready_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_tstate(6),
            protocol::make_activate_out_portal(46, 45),
        ],
    )
    .await?;

    logger.state(
        Some(&session_id),
        "tutorial automation: phase 10 portal activated — waiting for server redirect",
    );

    // The portal at (46,45) triggers a server REDIRECT packet.
    // The current TCP connection will be closed by the server.
    // The bot's redirect handler will reconnect and rejoin TUTORIAL2.
    // Phase 12+ will be triggered by `run_tutorial_phase2` after the world reloads.
    //
    // We intentionally END the script here. The redirect handler in
    // `handle_incoming_message` will detect we are still in TUTORIAL2 and
    // schedule `run_tutorial_phase2` once the world is loaded.

    // ── Phase 11: Wait for portal transition ──────────────────────────────
    sleep(tutorial_consts::portal_transition_timeout()).await;

    // Server teleports us from left room to right room within TUTORIAL2.
    // Update local position to the landing coordinates.
    let (landing_wx, landing_wy) = protocol::map_to_world(
        tutorial_consts::TUTORIAL_LANDING_X as f64,
        tutorial_consts::TUTORIAL_LANDING_Y as f64,
    );
    set_local_map_position(
        &logger, &session_id, &state,
        tutorial_consts::TUTORIAL_LANDING_X, tutorial_consts::TUTORIAL_LANDING_Y,
    ).await;
    set_local_world_position(&logger, &session_id, &state, landing_wx, landing_wy).await;
    send_scheduler_cmd(
        &outbound_tx,
        SchedulerCommand::UpdateMovement {
            world_x: landing_wx,
            world_y: landing_wy,
            is_moving: false,
            anim: movement_consts::ANIM_IDLE,
            direction: movement_consts::DIR_RIGHT,
        },
    ).await?;

    logger.state(Some(&session_id), "tutorial automation: phase 12 walking to landing");

    // ── Phase 12: Place soil blocks at BUILD_TARGETS ──────────────────────
    sleep(tutorial_consts::medium_pause()).await;
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(tutorial_consts::TUTORIAL_LANDING_X, tutorial_consts::TUTORIAL_LANDING_Y),
            protocol::make_movement_packet(
                landing_wx, landing_wy,
                movement_consts::ANIM_IDLE, movement_consts::DIR_RIGHT, false,
            ),
        ],
    ).await?;

    sleep(tutorial_consts::short_pause()).await;

    for (target_x, target_y) in tutorial_consts::BUILD_TARGETS.iter() {
        logger.state(
            Some(&session_id),
            format!("tutorial automation: phase 12 placing soil at ({}, {})", target_x, target_y),
        );
        send_docs_exclusive(
            &outbound_tx,
            vec![
                protocol::make_place_block(*target_x, *target_y, tutorial_consts::SOIL_BLOCK_ID),
                protocol::make_empty_movement(),
            ],
        ).await?;
        sleep(tutorial_consts::walk_step_pause()).await;
    }

    // ── Phase 13: Mine soil to get seeds ──────────────────────────────────
    sleep(tutorial_consts::short_pause()).await;
    for (target_x, target_y) in tutorial_consts::BUILD_TARGETS.iter() {
        logger.state(
            Some(&session_id),
            format!("tutorial automation: phase 13 mining soil at ({}, {})", target_x, target_y),
        );
        send_docs_exclusive(
            &outbound_tx,
            vec![
                protocol::make_hit_block(*target_x, *target_y),
                protocol::make_empty_movement(),
            ],
        ).await?;
        sleep(tutorial_consts::short_pause()).await;
    }

    // ── Phase 14: Plant seed, fertilize, harvest ──────────────────────────
    sleep(tutorial_consts::short_pause()).await;

    // Select the seed from inventory
    logger.state(Some(&session_id), "tutorial automation: phase 14 selecting seed");
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement()],
    ).await?;
    sleep(tutorial_consts::walk_step_pause()).await;

    // Plant seed at farm target
    logger.state(
        Some(&session_id),
        format!("tutorial automation: phase 14 planting seed at ({}, {})",
            tutorial_consts::FARM_TARGET_X, tutorial_consts::FARM_TARGET_Y),
    );
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_seed_block(
                tutorial_consts::FARM_TARGET_X,
                tutorial_consts::FARM_TARGET_Y,
                tutorial_consts::SOIL_BLOCK_ID,
            ),
            protocol::make_empty_movement(),
        ],
    ).await?;
    sleep(tutorial_consts::short_pause()).await;

    // Select fertilizer
    logger.state(Some(&session_id), "tutorial automation: phase 14 selecting fertilizer");
    send_docs_exclusive(
        &outbound_tx,
        vec![protocol::make_empty_movement()],
    ).await?;
    sleep(tutorial_consts::walk_step_pause()).await;

    // Fertilize the seed
    logger.state(
        Some(&session_id),
        format!("tutorial automation: phase 14 fertilizing seed at ({}, {})",
            tutorial_consts::FARM_TARGET_X, tutorial_consts::FARM_TARGET_Y),
    );
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_seed_block(
                tutorial_consts::FARM_TARGET_X,
                tutorial_consts::FARM_TARGET_Y,
                tutorial_consts::FERTILIZER_BLOCK_ID,
            ),
            protocol::make_empty_movement(),
        ],
    ).await?;

    // Wait for the crop to grow (fertilizer makes it instant, but give it a moment)
    sleep(Duration::from_secs(3)).await;

    // Harvest the crop
    logger.state(
        Some(&session_id),
        format!("tutorial automation: phase 14 harvesting crop at ({}, {})",
            tutorial_consts::FARM_TARGET_X, tutorial_consts::FARM_TARGET_Y),
    );
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_hit_block(tutorial_consts::FARM_TARGET_X, tutorial_consts::FARM_TARGET_Y),
            protocol::make_empty_movement(),
        ],
    ).await?;
    sleep(tutorial_consts::medium_pause()).await;

    // ── Phase 15: Collect all drops ───────────────────────────────────────
    logger.state(Some(&session_id), "tutorial automation: phase 15 walking collect route");
    {
        let state_read = state.read().await;
        let collectables: Vec<i32> = state_read.collectables.keys().copied().collect();
        drop(state_read);
        for cid in collectables {
            send_docs_exclusive(
                &outbound_tx,
                vec![protocol::make_collectable_request(cid)],
            ).await?;
            sleep(Duration::from_millis(100)).await;
        }
    }
    logger.state(Some(&session_id), "tutorial automation: phase 15 complete; scheduling shop");
    sleep(tutorial_consts::medium_pause()).await;

    // ── Phase 16: Open shop ───────────────────────────────────────────────
    logger.state(Some(&session_id), "tutorial automation: phase 16 opening shop");
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_tstate(7),
        ],
    ).await?;
    sleep(tutorial_consts::medium_pause()).await;

    // ── Phase 17: Buy clothes pack ────────────────────────────────────────
    logger.state(
        Some(&session_id),
        format!("tutorial automation: phase 17 buying {}", tutorial_consts::CLOTHES_PACK_ID),
    );
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_buy_item_pack(tutorial_consts::CLOTHES_PACK_ID),
            protocol::make_empty_movement(),
        ],
    ).await?;
    sleep(tutorial_consts::medium_pause()).await;

    logger.state(Some(&session_id), "tutorial automation: phase 17 acknowledging purchase");
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_action_event(tutorial_consts::CLOTHES_PACK_AE),
            protocol::make_empty_movement(),
        ],
    ).await?;
    sleep(tutorial_consts::short_pause()).await;

    // ── Phase 18: Return to tutorial world flow ───────────────────────────
    logger.state(Some(&session_id), "tutorial automation: phase 18 returning to tutorial world");
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_tstate(8),
        ],
    ).await?;
    sleep(tutorial_consts::medium_pause()).await;
    logger.state(Some(&session_id), "tutorial automation: phase 18 complete; scheduling clothes equip");

    // ── Phase 19: Equip purchased pack items ──────────────────────────────
    for block_id in tutorial_consts::EQUIP_BLOCKS.iter() {
        logger.state(
            Some(&session_id),
            format!("tutorial automation: phase 19 equipping pack item {}", block_id),
        );
        send_docs_exclusive(
            &outbound_tx,
            vec![
                protocol::make_wear_item(*block_id),
                protocol::make_empty_movement(),
            ],
        ).await?;
        sleep(tutorial_consts::walk_step_pause()).await;
    }
    sleep(tutorial_consts::short_pause()).await;

    // ── Phase 20: Complete tutorial and leave world ────────────────────────
    logger.state(Some(&session_id), "tutorial automation: phase 20 completing and leaving world");

    // Walk to exit portal
    let (exit_wx, exit_wy) = protocol::map_to_world(
        tutorial_consts::PORTAL_ENTRY_X as f64,
        tutorial_consts::PORTAL_ENTRY_Y as f64,
    );
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_map_point(tutorial_consts::PORTAL_ENTRY_X, tutorial_consts::PORTAL_ENTRY_Y),
            protocol::make_movement_packet(
                exit_wx, exit_wy,
                movement_consts::ANIM_WALK, movement_consts::DIR_RIGHT, false,
            ),
        ],
    ).await?;
    sleep(tutorial_consts::medium_pause()).await;

    // Activate the exit portal
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_movement_packet(
                exit_wx, exit_wy,
                movement_consts::ANIM_IDLE, movement_consts::DIR_RIGHT, false,
            ),
            protocol::make_tstate(9),
            protocol::make_activate_out_portal(
                tutorial_consts::PORTAL_ENTRY_X,
                tutorial_consts::PORTAL_ENTRY_Y,
            ),
        ],
    ).await?;
    set_local_world_position(&logger, &session_id, &state, exit_wx, exit_wy).await;

    // Wait for leave confirmation
    sleep(tutorial_consts::portal_transition_timeout()).await;
    logger.state(Some(&session_id), "tutorial automation: phase 20 complete; scheduling PIXELSTATION");

    // Update state to menu
    {
        let mut state = state.write().await;
        state.status = SessionStatus::MenuReady;
        state.current_world = None;
        state.world = None;
    }

    // ── Phase 21: Join PIXELSTATION ───────────────────────────────────────
    sleep(tutorial_consts::medium_pause()).await;
    logger.state(
        Some(&session_id),
        format!("tutorial automation: phase 21 joining {}", tutorial_consts::POST_TUTORIAL_WORLD),
    );

    {
        let mut state = state.write().await;
        state.current_world = Some(tutorial_consts::POST_TUTORIAL_WORLD.to_string());
        state.pending_world = Some(tutorial_consts::POST_TUTORIAL_WORLD.to_string());
        state.status = SessionStatus::LoadingWorld;
        state.awaiting_ready = false;
        state.world = None;
        state.world_foreground_tiles.clear();
        state.world_background_tiles.clear();
        state.world_water_tiles.clear();
        state.world_wiring_tiles.clear();
        state.collectables.clear();
        state.other_players.clear();
                            state.ai_enemies.clear();
    }

    send_docs_exclusive(
        &outbound_tx,
        {
            let mut docs = protocol::make_enter_world_eid(tutorial_consts::POST_TUTORIAL_WORLD, "Start");
            docs.push(protocol::make_st());
            docs
        },
    ).await?;

    // Wait for PIXELSTATION to load
    sleep(Duration::from_secs(10)).await;

    // ── Phase 22: Leave PIXELSTATION ──────────────────────────────────────
    logger.state(Some(&session_id), "tutorial automation: phase 22 leaving PIXELSTATION");
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_tstate(10),
        ],
    ).await?;

    sleep(tutorial_consts::portal_transition_timeout()).await;
    logger.state(Some(&session_id), "tutorial automation: phase 22 complete; scheduling menu reward");

    {
        let mut state = state.write().await;
        state.status = SessionStatus::MenuReady;
        state.current_world = None;
        state.world = None;
    }

    // ── Phase 23: Claim menu reward ───────────────────────────────────────
    sleep(tutorial_consts::medium_pause()).await;
    logger.state(Some(&session_id), "tutorial automation: phase 23 requesting menu reward");
    send_docs_exclusive(
        &outbound_tx,
        vec![
            protocol::make_empty_movement(),
            protocol::make_tstate(11),
        ],
    ).await?;

    sleep(tutorial_consts::medium_pause()).await;
    logger.state(Some(&session_id), "tutorial automation: phase 23 reward confirmed");

    logger.state(
        Some(&session_id),
        "tutorial automation: COMPLETE — account graduated, ready for any world",
    );
    Ok(())
}

pub(super) async fn wait_for_tutorial_world_ready_to_enter(
    state: &Arc<RwLock<SessionState>>,
) -> Result<(), String> {
    let deadline = Instant::now() + tutorial_consts::world_join_timeout();
    loop {
        {
            let state = state.read().await;
            if state.current_world.as_deref() == Some(tutorial_consts::TUTORIAL_WORLD)
                && state.world.is_some()
                && state.status == SessionStatus::AwaitingReady
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for tutorial GWC phase to finish".to_string());
        }
        sleep(Duration::from_millis(20)).await;
    }
}

pub(super) async fn wait_for_tutorial_phase4_ack(
    state: &Arc<RwLock<SessionState>>,
) -> Result<(), String> {
    let deadline = Instant::now() + tutorial_consts::world_join_timeout();
    loop {
        {
            let state = state.read().await;
            if state.tutorial_phase4_acknowledged {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for tutorial rAI acknowledgement".to_string());
        }
        sleep(Duration::from_millis(20)).await;
    }
}

pub(super) async fn ensure_world(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    controller_tx: &mpsc::Sender<ControllerEvent>,
    outbound_tx: &OutboundHandle,
    world: &str,
) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    ensure_world_cancellable(
        session_id,
        logger,
        state,
        controller_tx,
        outbound_tx,
        world,
        false,
        &cancel,
    )
    .await
}

pub(super) async fn ensure_world_cancellable(
    session_id: &str,
    logger: &Logger,
    state: &Arc<RwLock<SessionState>>,
    controller_tx: &mpsc::Sender<ControllerEvent>,
    outbound_tx: &OutboundHandle,
    world: &str,
    instance: bool,
    cancel: &AtomicBool,
) -> Result<(), String> {
    ensure_not_cancelled(cancel)?;
    let current = state.read().await.current_world.clone();
    let status = state.read().await.status.clone();
    if current.as_deref() == Some(world) && status == SessionStatus::InWorld {
        return Ok(());
    }

    let should_bootstrap_tutorial = world == tutorial_consts::TUTORIAL_WORLD
        && current.is_none()
        && status == SessionStatus::MenuReady;

    if should_bootstrap_tutorial {
        logger.state(
            Some(session_id),
            format!("bootstrapping {world} directly with Gw/GWC flow from packets.bin"),
        );
        {
            let mut state = state.write().await;
            state.current_world = Some(world.to_string());
            state.pending_world = Some(world.to_string());
            state.status = SessionStatus::LoadingWorld;
            state.world = None;
            state.world_foreground_tiles.clear();
            state.world_background_tiles.clear();
            state.world_water_tiles.clear();
            state.world_wiring_tiles.clear();
            state.collectables.clear();
            state.other_players.clear();
                            state.ai_enemies.clear();
        }
        let eid = if world == tutorial_consts::TUTORIAL_WORLD {
            "Start"
        } else {
            ""
        };
        send_docs_exclusive(outbound_tx, protocol::make_enter_world_eid(world, eid)).await?;
    } else {
        logger.state(
            Some(session_id),
            format!("joining {world}{}",
                if instance { " (instance)" } else { "" }),
        );
        controller_tx
            .send(ControllerEvent::Command(SessionCommand::JoinWorld {
                world: world.to_string(),
                instance,
            }))
            .await
            .map_err(|error| error.to_string())?;
    }

    let deadline = Instant::now() + tutorial_consts::world_join_timeout();
    loop {
        ensure_not_cancelled(cancel)?;
        {
            let state = state.read().await;
            if state.current_world.as_deref() == Some(world)
                && state.status == SessionStatus::InWorld
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting to enter {world}"));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

pub(super) async fn wait_for_tutorial_spawn_pod_confirmation(
    state: &Arc<RwLock<SessionState>>,
) -> Result<(), String> {
    let deadline = Instant::now() + tutorial_consts::spawn_pod_confirm_timeout();
    loop {
        {
            let mut state = state.write().await;
            if state.tutorial_spawn_pod_confirmed {
                state.tutorial_spawn_pod_confirmed = false;
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err("timed out waiting for tutorial spawn pod confirmation".to_string());
        }

        sleep(Duration::from_millis(50)).await;
    }
}

