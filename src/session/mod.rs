use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use std::io::Cursor;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex as PlMutex;


use bson::{Document, doc};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{RwLock, mpsc, watch};
use tokio::time::{MissedTickBehavior, interval, interval_at, sleep, sleep_until};

use crate::auth;
use crate::constants::{fishing as fishing_consts, movement as movement_consts, network as network_consts, protocol as ids, timing, tutorial as tutorial_consts};
use crate::logging::{Direction, Logger};
use crate::lua_runtime::{self, LuaScriptHandle};
use crate::models::{
    AuthInput, BotTarget, InventoryItem, LuaCollectableSnapshot, LuaGrowingTileSnapshot,
    LuaScriptStatusSnapshot, LuaTileSnapshot, LuaWorldObjectsSnapshot, LuaWorldSnapshot,
    LuaWorldSpawnSnapshot, LuaWorldTilesSnapshot, MinimapSnapshot, PlayerPosition,
    RemotePlayerSnapshot, SessionSnapshot, SessionStatus, TileCount, WorldSnapshot,
};
use crate::net;
use crate::pathfinding::astar;
use crate::protocol;
use crate::world;

pub mod automine;
pub mod autonether;
mod bot_session;
mod fishing;
mod manager;
mod movement;
mod network;
mod state;
mod tutorial;
mod world_data;
pub use bot_session::BotSession;
pub use manager::SessionManager;
use fishing::*;
use movement::*;
use network::*;
use state::*;
use tutorial::*;
use world_data::*;

pub(crate) static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static RUNTIME_COUNTER: AtomicU64 = AtomicU64::new(1);









<<<<<<< HEAD
=======
    pub async fn lua_script_status(
        &self,
        session_id: &str,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        self.get_session(session_id)
            .await
            .ok_or_else(|| "session not found".to_string())?;
        let status = if let Some(entry) = self.lua_scripts.get(session_id) {
            Some(entry.status.clone())
        } else {
            None
        };
        match status {
            Some(status) => Ok(status.read().await.clone()),
            None => Ok(lua_runtime::idle_status()),
        }
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        // Stop any running lua script first
        let _ = self.stop_lua_script(session_id).await;
        // Remove the session
        if self.sessions.remove(session_id).is_some() {
            Ok(())
        } else {
            Err("session not found".to_string())
        }
    }
}
>>>>>>> jeli/fix/sidebar-ui-redesign



<<<<<<< HEAD
=======
    async fn new(id: String, auth: AuthInput, logger: Logger) -> Arc<Self> {
        let (controller_tx, controller_rx) = mpsc::channel(512);
        let device_id = auth.device_id();
        let state = Arc::new(RwLock::new(SessionState {
            status: SessionStatus::Idle,
            device_id: if device_id.is_empty() {
                network::DEFAULT_DEVICE_ID.to_string()
            } else {
                device_id
            },
            current_host: net::default_host(),
            current_port: net::default_port(),
            current_world: None,
            pending_world: None,
            pending_world_is_instance: false,
            serverfull_retries: 0,
            last_action_hint: None,
            last_action_at: None,
            username: None,
            user_id: None,
            world: None,
            world_foreground_tiles: Vec::new(),
            world_background_tiles: Vec::new(),
            world_water_tiles: Vec::new(),
            world_wiring_tiles: Vec::new(),
            current_outbound_tx: None,
            growing_tiles: HashMap::new(),
            player_position: PlayerPosition {
                map_x: None,
                map_y: None,
                world_x: None,
                world_y: None,
            },
            current_direction: movement::DIR_RIGHT,
            other_players: HashMap::new(),
            ai_enemies: HashMap::new(),
            inventory: Vec::new(),
            collectables: HashMap::new(),
            world_items: Vec::new(),
            last_error: None,
            awaiting_ready: false,
            tutorial_spawn_pod_confirmed: false,
            tutorial_automation_running: false,
            pending_hits: HashMap::new(),
            tutorial_phase4_acknowledged: false,
            fishing: FishingAutomationState::default(),
            ping_ms: None,
            collect_cooldowns: CollectCooldowns::default(),
            rate_limit_until: None,
            current_target: None,
            autonether: autonether::AutonetherState::new(),
        }));
>>>>>>> jeli/fix/sidebar-ui-redesign
















<<<<<<< HEAD
=======
    pub async fn queue_place(
        &self,
        offset_x: i32,
        offset_y: i32,
        block_id: i32,
    ) -> Result<String, String> {
        self.send_command(SessionCommand::Place {
            offset_x,
            offset_y,
            block_id,
        })
        .await?;
        Ok(format!(
            "place queued for block {block_id} at offset ({offset_x}, {offset_y})"
        ))
    }

    pub async fn queue_move_direction(&self, direction: &str) -> Result<String, String> {
        let normalized = direction.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "left" | "right" | "up" | "down") {
            return Err("direction must be left, right, up, or down".to_string());
        }
        self.send_command(SessionCommand::ManualMove {
            direction: normalized.clone(),
        })
        .await?;
        Ok(format!("queued 1 step {normalized}"))
    }

    pub async fn queue_start_fishing(&self, direction: &str, bait: &str) -> Result<String, String> {
        let normalized = direction.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "left" | "right") {
            return Err("fishing direction must be left or right".to_string());
        }
        let bait = bait.trim();
        if bait.is_empty() {
            return Err("bait is required".to_string());
        }
        self.send_command(SessionCommand::StartFishing {
            direction: normalized.clone(),
            bait: bait.to_string(),
        })
        .await?;
        Ok(format!(
            "auto-fishing queued to the {normalized} using {bait}"
        ))
    }

    pub async fn queue_stop_fishing(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopFishing).await?;
        Ok("fishing stop queued".to_string())
    }

    pub async fn queue_talk(&self, message: &str) -> Result<String, String> {
        let message = message.trim();
        if message.is_empty() {
            return Err("message is required".to_string());
        }
        self.send_command(SessionCommand::Talk {
            message: message.to_string(),
        })
        .await?;
        Ok("chat message queued".to_string())
    }

    pub async fn queue_start_spam(&self, message: &str, delay_ms: u64) -> Result<String, String> {
        let message = message.trim();
        if message.is_empty() {
            return Err("message is required".to_string());
        }
        if delay_ms < 250 {
            return Err("spam delay must be at least 250ms".to_string());
        }
        if delay_ms > 3_600_000 {
            return Err("spam delay must be at most 3600000ms".to_string());
        }
        self.send_command(SessionCommand::StartSpam {
            message: message.to_string(),
            delay_ms,
        })
        .await?;
        Ok(format!("spam loop queued at {delay_ms}ms"))
    }

    pub async fn queue_stop_spam(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopSpam).await?;
        Ok("spam stop queued".to_string())
    }

    pub async fn queue_start_automine(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StartAutomine).await?;
        Ok("automine start queued".to_string())
    }

    pub async fn queue_stop_automine(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopAutomine).await?;
        Ok("automine stop queued".to_string())
    }

    pub async fn queue_start_autonether(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StartAutonether).await?;
        Ok("autonether start queued".to_string())
    }

    pub async fn queue_stop_autonether(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopAutonether).await?;
        Ok("autonether stop queued".to_string())
    }

    pub async fn autonether_status(&self) -> Result<crate::session::autonether::AutonetherStatusSnapshot, String> {
        let state = self.state.read().await;
        Ok(state.autonether.snapshot())
    }

    pub(crate) async fn walk(
        &self,
        offset_x: i32,
        offset_y: i32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let (target_x, target_y) = {
            let state = self.state.read().await;
            (
                state
                    .player_position
                    .map_x
                    .ok_or_else(|| "player map x is not known yet".to_string())?
                    .round() as i32
                    + offset_x,
                state
                    .player_position
                    .map_y
                    .ok_or_else(|| "player map y is not known yet".to_string())?
                    .round() as i32
                    + offset_y,
            )
        };
        self.walk_to(target_x, target_y, cancel).await
    }

    pub(crate) async fn walk_to(
        &self,
        map_x: i32,
        map_y: i32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before walking".to_string())?;
        walk_to_map_cancellable(&self.state, &outbound_tx, map_x, map_y, cancel).await
    }

    pub(crate) async fn find_path(
        &self,
        map_x: i32,
        map_y: i32,
    ) -> Result<Vec<(i32, i32)>, String> {
        let (start_x, start_y) = {
            let state = self.state.read().await;
            (
                state
                    .player_position
                    .map_x
                    .ok_or_else(|| "player map x is not known yet".to_string())?
                    .round() as i32,
                state
                    .player_position
                    .map_y
                    .ok_or_else(|| "player map y is not known yet".to_string())?
                    .round() as i32,
            )
        };

        Ok(
            planned_path(&self.state, (start_x, start_y), (map_x, map_y))
                .await
                .unwrap_or_else(|| fallback_straight_line_path((start_x, start_y), (map_x, map_y))),
        )
    }

    pub(crate) async fn punch(
        &self,
        offset_x: i32,
        offset_y: i32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before punching".to_string())?;
        manual_punch(
            &self.id,
            &self.logger,
            &self.state,
            &outbound_tx,
            offset_x,
            offset_y,
        )
        .await
    }

    pub(crate) async fn place(
        &self,
        offset_x: i32,
        offset_y: i32,
        block_id: i32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before placing blocks".to_string())?;
        manual_place(
            &self.id,
            &self.logger,
            &self.state,
            &outbound_tx,
            offset_x,
            offset_y,
            block_id,
        )
        .await
    }

    pub(crate) async fn wear(
        &self,
        block_id: i32,
        equip: bool,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before wearing items".to_string())?;
        let packet = if equip {
            protocol::make_wear_item(block_id)
        } else {
            protocol::make_unwear_item(block_id)
        };
        send_doc(&outbound_tx, packet).await
    }

    pub(crate) async fn talk(&self, message: &str, cancel: &AtomicBool) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before sending chat".to_string())?;
        send_world_chat(&self.id, &self.logger, &outbound_tx, message).await
    }

    pub(crate) async fn collect(&self, cancel: &AtomicBool) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before collecting".to_string())?;
        collect_all_visible_collectables_cancellable(&self.state, &outbound_tx, cancel).await
    }

    pub(crate) async fn warp(&self, world: &str, cancel: &AtomicBool) -> Result<(), String> {
        self.warp_inner(world, false, cancel).await
    }

    pub(crate) async fn warp_instance(
        &self,
        world: &str,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        self.warp_inner(world, true, cancel).await
    }

    async fn warp_inner(
        &self,
        world: &str,
        instance: bool,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let world = world.trim().to_uppercase();
        if world.is_empty() {
            return Err("world is required".to_string());
        }
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before warping".to_string())?;
        ensure_world_cancellable(
            &self.id,
            &self.logger,
            &self.state,
            &self.controller_tx,
            &outbound_tx,
            &world,
            instance,
            cancel,
        )
        .await
    }

    pub(crate) async fn send_packet(
        &self,
        packet: Document,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        ensure_not_cancelled(cancel)?;
        let outbound_tx = self
            .state
            .read()
            .await
            .current_outbound_tx
            .clone()
            .ok_or_else(|| "connect the session before sending packets".to_string())?;
        send_doc(&outbound_tx, packet).await
    }

    pub(crate) async fn position(&self) -> PlayerPosition {
        self.state.read().await.player_position.clone()
    }

    pub(crate) async fn current_world(&self) -> Option<String> {
        self.state.read().await.current_world.clone()
    }

    pub(crate) async fn status(&self) -> SessionStatus {
        self.state.read().await.status.clone()
    }

    pub(crate) async fn is_in_world(&self) -> bool {
        self.state.read().await.status == SessionStatus::InWorld
    }

    pub(crate) async fn inventory_count(&self, block_id: u16) -> u32 {
        self.state
            .read()
            .await
            .inventory
            .iter()
                    .filter(|entry| entry.block_id == block_id)
            .map(|entry| entry.amount as u32)
            .sum()
    }

    pub(crate) async fn collectables(&self) -> Vec<LuaCollectableSnapshot> {
        let mut collectables = self
            .state
            .read()
            .await
            .collectables
            .values()
            .cloned()
            .map(|item| LuaCollectableSnapshot {
                id: item.collectable_id,
                block_type: item.block_type,
                amount: item.amount,
                inventory_type: item.inventory_type,
                pos_x: item.pos_x,
                pos_y: item.pos_y,
                is_gem: item.is_gem,
            })
            .collect::<Vec<_>>();
        collectables.sort_by_key(|item| item.id);
        collectables
    }

    pub(crate) async fn world(&self) -> Result<LuaWorldSnapshot, String> {
        let state = self.state.read().await;
        let world = state
            .world
            .as_ref()
            .ok_or_else(|| "no world loaded yet".to_string())?;
        let mut growing_tiles = state
            .growing_tiles
            .iter()
            .map(|(&(x, y), item)| LuaGrowingTileSnapshot {
                x,
                y,
                block_id: item.block_id,
                growth_end_time: item.growth_end_time,
                growth_duration_secs: item.growth_duration_secs,
                mixed: item.mixed,
                harvest_seeds: item.harvest_seeds,
                harvest_blocks: item.harvest_blocks,
                harvest_gems: item.harvest_gems,
                harvest_extra_blocks: item.harvest_extra_blocks,
            })
            .collect::<Vec<_>>();
        growing_tiles.sort_by_key(|item| (item.y, item.x));

        let mut collectables = state
            .collectables
            .values()
            .cloned()
            .map(|item| LuaCollectableSnapshot {
                id: item.collectable_id,
                block_type: item.block_type,
                amount: item.amount,
                inventory_type: item.inventory_type,
                pos_x: item.pos_x,
                pos_y: item.pos_y,
                is_gem: item.is_gem,
            })
            .collect::<Vec<_>>();
        collectables.sort_by_key(|item| item.id);

        Ok(LuaWorldSnapshot {
            name: world.world_name.clone(),
            width: world.width,
            height: world.height,
            spawn: LuaWorldSpawnSnapshot {
                map_x: world.spawn_map_x,
                map_y: world.spawn_map_y,
                world_x: world.spawn_world_x,
                world_y: world.spawn_world_y,
            },
            tiles: LuaWorldTilesSnapshot {
                foreground: state.world_foreground_tiles.clone(),
                background: state.world_background_tiles.clone(),
                water: state.world_water_tiles.clone(),
                wiring: state.world_wiring_tiles.clone(),
            },
            objects: LuaWorldObjectsSnapshot {
                collectables,
                growing_tiles,
            },
        })
    }

    pub(crate) async fn tile(&self, map_x: i32, map_y: i32) -> Result<LuaTileSnapshot, String> {
        let state = self.state.read().await;
        tile_snapshot_at(&state, map_x, map_y)
    }

    async fn send_command(&self, command: SessionCommand) -> Result<(), String> {
        self.controller_tx
            .send(ControllerEvent::Command(command))
            .await
            .map_err(|error| error.to_string())
    }

    async fn resolve_fishing_target(
        &self,
        direction: &str,
        bait_query: &str,
    ) -> Result<FishingTarget, String> {
        let state = self.state.read().await;
        let player_x = state
            .player_position
            .map_x
            .ok_or_else(|| "player position is unknown; join a world before fishing".to_string())?
            .round() as i32;
        let player_y = state
            .player_position
            .map_y
            .ok_or_else(|| "player position is unknown; join a world before fishing".to_string())?
            .round() as i32;
        let target = find_fishing_map_point(
            state.world.as_ref(),
            &state.world_water_tiles,
            player_x,
            player_y,
            direction,
        )?;
        find_inventory_bait(&state.inventory, bait_query)?;
        Ok(FishingTarget {
            direction: direction.to_string(),
            bait_query: bait_query.trim().to_string(),
            map_x: target.0,
            map_y: target.1,
        })
    }

    async fn clear_fishing_state(&self, status: Option<FishingPhase>) {
        let mut state = self.state.write().await;
        state.fishing.active = false;
        state.fishing.phase = status.unwrap_or(FishingPhase::Idle);
        state.fishing.target_map_x = None;
        state.fishing.target_map_y = None;
        state.fishing.bait_name = None;
        state.fishing.last_result = None;
    }

    async fn run_controller(self: Arc<Self>, mut rx: mpsc::Receiver<ControllerEvent>) {
        let mut runtime: Option<ActiveRuntime> = None;
        let mut spam_stop_tx: Option<watch::Sender<bool>> = None;
        let mut fishing_stop_tx: Option<watch::Sender<bool>> = None;
        let mut automine_stop_tx: Option<watch::Sender<bool>> = None;
        let mut autonether_stop_tx: Option<watch::Sender<bool>> = None;

        while let Some(event) = rx.recv().await {
            match event {
                ControllerEvent::Command(command) => match command {
                    SessionCommand::Connect => {
                        if runtime.is_some() {
                            continue;
                        }
                        match self.establish_connection(None).await {
                            Ok(active) => runtime = Some(active),
                            Err(error) => self.set_error(error).await,
                        }
                    }
                    SessionCommand::JoinWorld { world, instance } => {
                        {
                            let mut state = self.state.write().await;
                            state.pending_world = Some(world.to_uppercase());
                            state.pending_world_is_instance = instance;
                            state.status = SessionStatus::JoiningWorld;
                            state.other_players.clear();
                            state.ai_enemies.clear();
                        }
                        self.publish_snapshot().await;
                        if let Some(active) = &runtime {
                            if instance {
                                let _ = send_docs_exclusive(
                                    &active.outbound_tx,
                                    vec![
                                        protocol::make_world_load_args(&[0]),
                                        protocol::make_join_world_special(&world, 0),
                                    ],
                                )
                                .await;
                            } else {
                                let _ =
                                    send_doc(&active.outbound_tx, protocol::make_join_world(&world))
                                        .await;
                            }
                        }
                    }
                    SessionCommand::LeaveWorld => {
                        stop_background_worker(&mut spam_stop_tx);
                        stop_background_worker(&mut fishing_stop_tx);
                        stop_background_worker(&mut automine_stop_tx);
                        if let Some(active) = &runtime {
                            let _ =
                                send_doc(&active.outbound_tx, protocol::make_leave_world()).await;
                            let _ = send_scheduler_cmd(
                                &active.outbound_tx,
                                SchedulerCommand::SetPhase {
                                    phase: SchedulerPhase::MenuIdle,
                                },
                            )
                            .await;
                        }
                        self.reset_world_state(SessionStatus::MenuReady).await;
                    }
                    SessionCommand::Disconnect => {
                        stop_background_worker(&mut spam_stop_tx);
                        stop_background_worker(&mut fishing_stop_tx);
                        stop_background_worker(&mut automine_stop_tx);
                        if let Some(active) = runtime.take() {
                            let _ = active.stop_tx.send(true);
                        }
                        self.reset_world_state(SessionStatus::Disconnected).await;
                    }
                    SessionCommand::AutomateTutorial => {
                        let already_running = {
                            let state = self.state.read().await;
                            state.tutorial_automation_running
                        };
                        if already_running {
                            self.logger.state(
                                Some(&self.id),
                                "tutorial automation is already running for this session",
                            );
                            continue;
                        }
                        {
                            let mut state = self.state.write().await;
                            state.tutorial_automation_running = true;
                        }
                        let Some(active) = &runtime else {
                            {
                                let mut state = self.state.write().await;
                                state.tutorial_automation_running = false;
                            }
                            self.set_error(
                                "connect the session before starting tutorial automation"
                                    .to_string(),
                            )
                            .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let controller_tx = self.controller_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            let result = run_tutorial_script(
                                session_id.clone(),
                                logger.clone(),
                                state.clone(),
                                controller_tx,
                                outbound_tx,
                            )
                            .await;
                            state.write().await.tutorial_automation_running = false;
                            if let Err(error) = result {
                                logger.error("tutorial", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::ManualMove { direction } => {
                        let Some(active) = &runtime else {
                            self.set_error(
                                "connect the session before sending manual movement".to_string(),
                            )
                            .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) =
                                manual_move(&session_id, &logger, &state, &outbound_tx, &direction)
                                    .await
                            {
                                logger.error("movement", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::WearItem { block_id, equip } => {
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before wearing items".to_string())
                                .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let packet = if equip {
                            protocol::make_wear_item(block_id)
                        } else {
                            protocol::make_unwear_item(block_id)
                        };
                        let _ = send_doc(&outbound_tx, packet).await;
                    }
                    SessionCommand::Punch { offset_x, offset_y } => {
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before punching".to_string())
                                .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = manual_punch(
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                offset_x,
                                offset_y,
                            )
                            .await
                            {
                                logger.error("punch", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::Place {
                        offset_x,
                        offset_y,
                        block_id,
                    } => {
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before placing blocks".to_string())
                                .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = manual_place(
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                offset_x,
                                offset_y,
                                block_id,
                            )
                            .await
                            {
                                logger.error("place", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StartFishing { direction, bait } => {
                        stop_background_worker(&mut fishing_stop_tx);
                        let Some(active) = &runtime else {
                            self.set_error(
                                "connect the session before starting fishing".to_string(),
                            )
                            .await;
                            continue;
                        };
                        let target = match self.resolve_fishing_target(&direction, &bait).await {
                            Ok(target) => target,
                            Err(error) => {
                                self.set_error(error).await;
                                continue;
                            }
                        };
                        let (stop_tx, stop_rx) = watch::channel(false);
                        fishing_stop_tx = Some(stop_tx);
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = fishing_loop(
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                stop_rx,
                                target,
                            )
                            .await
                            {
                                logger.error("fishing", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopFishing => {
                        stop_background_worker(&mut fishing_stop_tx);
                        self.state.write().await.current_target = None;
                        if let Some(active) = &runtime {
                            let _ = send_doc(
                                &active.outbound_tx,
                                protocol::make_stop_fishing_game(false),
                            )
                            .await;
                            let _ = send_doc(
                                &active.outbound_tx,
                                protocol::make_stop_fishing_game(true),
                            )
                            .await;
                        }
                        self.clear_fishing_state(None).await;
                    }
                    SessionCommand::Talk { message } => {
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before sending chat".to_string())
                                .await;
                            continue;
                        };
                        if let Err(error) =
                            send_world_chat(&self.id, &self.logger, &active.outbound_tx, &message)
                                .await
                        {
                            self.set_error(error).await;
                        }
                    }
                    SessionCommand::StartSpam { message, delay_ms } => {
                        stop_background_worker(&mut spam_stop_tx);
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before starting spam".to_string())
                                .await;
                            continue;
                        };
                        let (stop_tx, stop_rx) = watch::channel(false);
                        spam_stop_tx = Some(stop_tx);
                        let outbound_tx = active.outbound_tx.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = spam_loop(
                                &session_id,
                                &logger,
                                &outbound_tx,
                                stop_rx,
                                message,
                                delay_ms,
                            )
                            .await
                            {
                                logger.error("spam", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopSpam => {
                        stop_background_worker(&mut spam_stop_tx);
                    }
                    SessionCommand::StartAutomine => {
                        stop_background_worker(&mut automine_stop_tx);
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before starting automine".to_string()).await;
                            continue;
                        };
                        let (stop_tx, stop_rx) = watch::channel(false);
                        automine_stop_tx = Some(stop_tx);
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = automine_loop(
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                stop_rx,
                            ).await {
                                logger.error("automine", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopAutomine => {
                        stop_background_worker(&mut automine_stop_tx);
                        self.state.write().await.current_target = None;
                    }
                    SessionCommand::StartAutonether => {
                        stop_background_worker(&mut autonether_stop_tx);
                        let Some(_active) = &runtime else {
                            self.set_error("connect the session before starting autonether".to_string()).await;
                            continue;
                        };
                        {
                            let mut state = self.state.write().await;
                            state.autonether.start();
                        }
                        let (stop_tx, stop_rx) = watch::channel(false);
                        autonether_stop_tx = Some(stop_tx);
                        let session = Arc::clone(&self);
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        tokio::spawn(async move {
                            if let Err(error) = autonether::autonether_loop(
                                &session_id,
                                &logger,
                                session,
                                stop_rx,
                            ).await {
                                logger.error("autonether", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopAutonether => {
                        stop_background_worker(&mut autonether_stop_tx);
                        self.state.write().await.autonether.stop();
                    }
                    SessionCommand::DropItem {
                        block_id,
                        amount,
                    } => {
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before dropping items".to_string())
                                .await;
                            continue;
                        };
                        let outbound_tx = active.outbound_tx.clone();
                        let drop_request = {
                            let state = self.state.read().await;
                            let inventory_type = block_inventory_type_for(block_id as u16)
                                .map(i32::from)
                                .or_else(|| {
                                    state
                                        .inventory
                                        .iter()
                                        .find(|entry| {
                                            entry.block_id == block_id as u16 && entry.amount > 0
                                        })
                                        .map(|entry| entry.inventory_type as i32)
                                })
                                .unwrap_or_default();
                            drop_target_tile(&state)
                                .map(|(drop_x, drop_y)| (drop_x, drop_y, inventory_type))
                        };
                        let (tile_x, tile_y, inventory_type) = match drop_request {
                            Ok(request) => request,
                            Err(error) => {
                                self.set_error(error).await;
                                continue;
                            }
                        };
                        let _ = send_docs_immediate(
                            &outbound_tx,
                            vec![
                                protocol::make_empty_movement(),
                                protocol::make_drop_item(
                                    tile_x,
                                    tile_y,
                                    block_id,
                                    inventory_type,
                                    amount,
                                ),
                                protocol::make_progress_signal(0),
                            ],
                        )
                        .await;
                    }
                },
                ControllerEvent::Inbound(runtime_id, message) => {
                    if let Some(active) = runtime.as_mut() {
                        if active.id != runtime_id {
                            continue;
                        }
                        if let Err(error) = self.handle_inbound(active, message).await {
                            stop_background_worker(&mut spam_stop_tx);
                            stop_background_worker(&mut fishing_stop_tx);
                            self.set_error(error).await;
                        }
                    }
                }
                ControllerEvent::ReadLoopStopped(runtime_id, reason) => {
                    let Some(active) = runtime.as_ref() else {
                        continue;
                    };
                    if active.id != runtime_id {
                        continue;
                    }
                    stop_background_worker(&mut spam_stop_tx);
                    stop_background_worker(&mut fishing_stop_tx);
                    runtime = None;
                    self.state.write().await.current_outbound_tx = None;
                    self.set_error(reason).await;
                }
            }
        }
    }

    async fn establish_connection(
        &self,
        host_override: Option<String>,
    ) -> Result<ActiveRuntime, String> {
        self.update_status(SessionStatus::Connecting, None).await;
        let resolved =
            auth::resolve_auth(self.auth.clone(), self.logger.clone(), self.id.clone()).await?;
        {
            let mut state = self.state.write().await;
            state.device_id = resolved.device_id.clone();
            if let Some(host) = host_override {
                state.current_host = host;
            }
            state.current_port = net::default_port();
            state.last_error = None;
        }
        self.publish_snapshot().await;

        self.update_status(SessionStatus::Authenticating, None)
            .await;
        let host = self.state.read().await.current_host.clone();
        self.logger.state(
            Some(&self.id),
            format!("connecting to {host}:{}", net::default_port()),
        );

        let mut stream = net::connect_tcp(&host, net::default_port()).await?;
        self.send_and_expect(
            &mut stream,
            &[protocol::make_vchk(&resolved.device_id)],
            ids::PACKET_ID_VCHK,
        )
        .await?;
        let gpd = self
            .send_and_expect(
                &mut stream,
                &[protocol::make_gpd(&resolved.jwt)],
                ids::PACKET_ID_GPD,
            )
            .await?;
        self.apply_profile(&gpd).await;

        let runtime_id = RUNTIME_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (read_half, write_half) = stream.into_split();
        let (outbound_tx, outbound_rx) = mpsc::channel(512);
        let (stop_tx, stop_rx) = watch::channel(false);
        let controller_tx = self.controller_tx.clone();
        let session_id = self.id.clone();
        let logger = self.logger.clone();
        tokio::spawn(async move {
            read_loop(read_half, controller_tx, logger, session_id, runtime_id).await;
        });
        let session_id = self.id.clone();
        let logger = self.logger.clone();
        let state_for_scheduler = self.state.clone();
        tokio::spawn(async move {
            scheduler_loop(write_half, outbound_rx, stop_rx, logger, session_id, state_for_scheduler).await;
        });

        self.state.write().await.current_outbound_tx = Some(outbound_tx.clone());
        self.update_status(SessionStatus::MenuReady, None).await;
        Ok(ActiveRuntime {
            id: runtime_id,
            outbound_tx,
            stop_tx,
        })
    }

    async fn send_and_expect(
        &self,
        stream: &mut tokio::net::TcpStream,
        messages: &[Document],
        expected_id: &str,
    ) -> Result<Document, String> {
        self.logger
            .tcp_trace(Direction::Outgoing, "tcp", Some(&self.id), || {
                protocol::summarize_messages(messages)
            });
        protocol::write_batch(stream, messages).await?;

        let mut received_batches = Vec::new();
        for _ in 0..16 {
            let response = protocol::read_packet(stream).await?;
            let extracted = protocol::extract_messages(&response);
            self.logger
                .tcp_trace(Direction::Incoming, "tcp", Some(&self.id), || {
                    protocol::summarize_messages(&extracted)
                });

            if extracted.is_empty() {
                received_batches.push("empty response batch".to_string());
                continue;
            }

            for message in &extracted {
                if message.get_str("ID").unwrap_or_default() == ids::PACKET_ID_GPD {
                    self.apply_profile(message).await;
                }
            }

            if let Some(found) = extracted
                .iter()
                .find(|message| message.get_str("ID").unwrap_or_default() == expected_id)
            {
                return Ok(found.clone());
            }

            received_batches.push(protocol::summarize_messages(&extracted));
        }

        Err(format!(
            "expected {expected_id}, got {}",
            received_batches.join(" -> ")
        ))
    }

    async fn send_and_receive(
        &self,
        stream: &mut tokio::net::TcpStream,
        messages: &[Document],
    ) -> Result<Document, String> {
        self.logger
            .tcp_trace(Direction::Outgoing, "tcp", Some(&self.id), || {
                protocol::summarize_messages(messages)
            });
        protocol::write_batch(stream, messages).await?;
        let response = protocol::read_packet(stream).await?;
        self.logger
            .tcp_trace(Direction::Incoming, "tcp", Some(&self.id), || {
                protocol::summarize_messages(&protocol::extract_messages(&response))
            });
        Ok(response)
    }

    async fn apply_profile(&self, profile: &Document) {
        let mut state = self.state.write().await;
        state.username = profile.get_str("UN").ok().map(ToOwned::to_owned);
        state.user_id = profile.get_str("U").ok().map(ToOwned::to_owned);
        state.inventory = decode_inventory(profile);
        state.last_error = None;
        drop(state);
        self.publish_snapshot().await;
    }

    async fn handle_inbound(
        &self,
        runtime: &mut ActiveRuntime,
        message: Document,
    ) -> Result<(), String> {
        let id = message.get_str("ID").unwrap_or_default();
        match id {
            ids::PACKET_ID_ST => {
                let _ = send_scheduler_cmd(
                    &runtime.outbound_tx,
                    SchedulerCommand::StResponseReceived,
                )
                .await;
            }
            ids::PACKET_ID_KEEPALIVE
            | ids::PACKET_ID_VCHK
            | ids::PACKET_ID_WREU
            | ids::PACKET_ID_BCSU
            | ids::PACKET_ID_DAILY_BONUS
            | ids::PACKET_ID_GET_LSI => {}
            ids::PACKET_ID_GPD => self.apply_profile(&message).await,
            ids::PACKET_ID_MOVEMENT | "U" | "AnP" => {
                self.maybe_update_player_positions(&message).await;
            }
            ids::PACKET_ID_PLAYER_LEAVE => {
                self.remove_other_player(&message).await;
            }
            "A" => {
                self.maybe_apply_spawn_pot_selection(&message).await;
            }
            ids::PACKET_ID_SET_BLOCK => {
                self.apply_set_block_message(&message).await;
            }
            ids::PACKET_ID_SEED_BLOCK => {
                self.apply_seed_growth_message(&message).await;
            }
            ids::PACKET_ID_DESTROY_BLOCK => {
                self.apply_destroy_block_message(&message).await;
            }
            ids::PACKET_ID_NEW_COLLECTABLE | ids::PACKET_ID_COLLECTABLE_REQUEST => {
                self.track_collectable(&message, false).await;
            }
            ids::PACKET_ID_NEW_WORLD_COLLECTABLE => {
                self.track_collectable(&message, true).await;
            }
            ids::PACKET_ID_AI_HIT_DAMAGE => {
                self.track_ai_enemy(&message).await;
            }
            ids::PACKET_ID_AI_SPAWN => {
                self.track_ai_spawn(&message).await;
            }
            ids::PACKET_ID_COLLECTABLE_REMOVE => {
                self.remove_collectable(&message).await;
            }
            ids::PACKET_ID_INVENTORY_UPDATE => {
                self.apply_inventory_update(&message).await;
            }
            ids::PACKET_ID_FISHING_GAME_ACTION => {
                self.apply_fishing_message(&message, &runtime.outbound_tx).await;
            }
            ids::PACKET_ID_FISHING_RESULT => {
                let result = message
                    .get_i32("IK")
                    .map(|item| format!("fishing reward inventory_key={item}"))
                    .unwrap_or_else(|_| "fishing reward received".to_string());
                {
                    let mut state = self.state.write().await;
                    state.fishing.active = false;
                    state.fishing.phase = FishingPhase::CleanupPending;
                    state.fishing.cleanup_pending = true;
                    state.fishing.last_result = Some(result.clone());
                }
                let _ = send_doc(
                    &runtime.outbound_tx,
                    protocol::make_fishing_cleanup_action(),
                )
                .await;
                self.logger.state(Some(&self.id), result);
            }
            ids::PACKET_ID_STOP_MINIGAME => {
                let mut state = self.state.write().await;
                if state.fishing.cleanup_pending {
                    state.fishing = FishingAutomationState::default();
                    state.fishing.phase = FishingPhase::Completed;
                } else if state.fishing.active {
                    state.fishing = FishingAutomationState::default();
                }
            }
            ids::PACKET_ID_JOIN_WORLD => {
                let jr = message.get_i32("JR").unwrap_or_default();
                if jr != 0 {
                    let mut retry_triggered = false;
                    if jr == 8 {
                        // JR: 8 is "Server Full". Trigger a shard-hopping retry.
                        let (world, retry_count) = {
                            let mut state = self.state.write().await;
                            state.serverfull_retries = state.serverfull_retries.saturating_add(1);
                            (state.pending_world.clone(), state.serverfull_retries)
                        };

                        const SERVERFULL_RETRY_LIMIT: u32 = 30;
                        if retry_count <= SERVERFULL_RETRY_LIMIT {
                            if let Some(world) = world {
                                self.logger.info("session", Some(&self.id), 
                                    format!("JR: 8 (Server Full) detected. Manually retrying shard #{retry_count} for {world}"));
                                let _ = send_doc(
                                    &runtime.outbound_tx,
                                    protocol::make_join_world_retry(&world, retry_count as i32),
                                ).await;
                                retry_triggered = true;
                            }
                        }
                    }

                    if !retry_triggered {
                        let err = message
                            .get_str("E")
                            .or_else(|_| message.get_str("Err"))
                            .unwrap_or_else(|_| if jr == 8 { "server full" } else { "join denied" });
                        self.logger
                            .warn("session", Some(&self.id), format!("TTjW denied (JR={jr}): {err}"));
                        self.set_error(format!("TTjW denied: {err}")).await;
                    }
                } else {
                    let world = message
                        .get_str("WN")
                        .ok()
                        .map(ToOwned::to_owned)
                        .or_else(|| {
                            self.state
                                .try_read()
                                .ok()
                                .and_then(|state| state.pending_world.clone())
                        });
                    {
                        let mut state = self.state.write().await;
                        state.current_world = world.clone();
                        state.status = SessionStatus::LoadingWorld;
                        state.other_players.clear();
                            state.ai_enemies.clear();
                    }
                    self.publish_snapshot().await;
                    if let Some(world) = world {
                        let _ = send_docs_exclusive(
                            &runtime.outbound_tx,
                            protocol::make_enter_world(&world),
                        )
                        .await;
                    }
                }
            }
            ids::PACKET_ID_GET_WORLD_CONTENT => {
                let raw = protocol::binary_bytes(message.get("W")).unwrap_or_default();
                let world_name = self.state.read().await.current_world.clone();
                let decoded_world = world::decode_gwc(world_name.clone(), &raw)?;
                let tutorial_phase_owned = {
                    let state = self.state.read().await;
                    state.tutorial_automation_running
                        && world_name.as_deref() == Some(tutorial::TUTORIAL_WORLD)
                };
                {
                    let mut state = self.state.write().await;
                    state.world = Some(decoded_world.snapshot.clone());
                    state.serverfull_retries = 0;
                    state.world_foreground_tiles = decoded_world.foreground_tiles;
                    state.world_background_tiles = decoded_world.background_tiles;
                    state.world_water_tiles = decoded_world.water_tiles;
                    state.world_wiring_tiles = decoded_world.wiring_tiles;
                    state.growing_tiles.clear();
                    state.collectables.clear();
                    
                    // Dump to world_objects.txt for analysis
                    {
                        use std::fs::OpenOptions;
                        use std::io::Write;
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("world_objects.txt") {
                            let _ = writeln!(file, "\n--- WORLD: {} ---", world_name.as_deref().unwrap_or("unknown"));
                            
                            for c in &decoded_world.collectables {
                                let (mx, my) = protocol::world_to_map(c.pos_x, c.pos_y);
                                let name = block_types().get(&(c.block_type as u16)).map(|info| info.name.as_str()).unwrap_or("Unknown");
                                let _ = writeln!(file, "[Collectable] ID={} Type={} ({}) at ({}, {}) map=({}, {})", 
                                    c.collectable_id, c.block_type, name, c.pos_x, c.pos_y, mx.floor() as i32, my.floor() as i32);
                                
                                state.collectables.insert(c.collectable_id, CollectableState {
                                    collectable_id: c.collectable_id,
                                    block_type: c.block_type,
                                    amount: c.amount,
                                    inventory_type: c.inventory_type,
                                    pos_x: c.pos_x,
                                    pos_y: c.pos_y,
                                    map_x: mx.floor() as i32,
                                    map_y: my.floor() as i32,
                                    is_gem: c.is_gem,
                                    gem_type: c.gem_type,
                                    is_nwc: false,
                                });
                            }

                            for i in &decoded_world.world_items {
                                let name = block_types().get(&i.item_id).map(|info| info.name.as_str()).unwrap_or("Unknown");
                                let _ = writeln!(file, "[WorldItem] ID={} ({}) at ({}, {}) state={}", 
                                    i.item_id, name, i.map_x, i.map_y, i.state);
                            }
                        }
                    }

                    state.world_items = decoded_world.world_items;
                    state.other_players.clear();
                    state.ai_enemies.clear();
                    state.player_position = PlayerPosition {
                        map_x: decoded_world.snapshot.spawn_map_x,
                        map_y: decoded_world.snapshot.spawn_map_y,
                        world_x: decoded_world.snapshot.spawn_world_x,
                        world_y: decoded_world.snapshot.spawn_world_y,
                    };
                    state.status = SessionStatus::AwaitingReady;
                    state.awaiting_ready = true;
                    state.tutorial_phase4_acknowledged = false;
                }
                self.publish_snapshot().await;

                if !tutorial_phase_owned {
                    if let Some(world) = world_name {
                        let _ = send_docs_exclusive(
                            &runtime.outbound_tx,
                            protocol::make_spawn_location_sync(&world),
                        )
                        .await;
                        let _ =
                            send_docs_exclusive(&runtime.outbound_tx, protocol::make_spawn_setup())
                                .await;
                    }
                }
            }
            ids::PACKET_ID_R_OP => {
                self.update_status(SessionStatus::AwaitingReady, None).await;
            }
            ids::PACKET_ID_R_AI => {
                let ai_id = message.get_i32("AId").ok(); // Is it an i32 or a blob?
                self.logger.info("session", Some(&self.id), format!("rAI packet: ai_id={:?} full={}", ai_id, protocol::log_message(&message)));
                
                if let Some(id) = ai_id {
                    let mut state = self.state.write().await;
                    state.ai_enemies.remove(&id);
                }

                let (should_ready, tutorial_phase_owned) = {
                    let mut state = self.state.write().await;
                    let tutorial_phase_owned = state.tutorial_automation_running
                        && state.current_world.as_deref() == Some(tutorial::TUTORIAL_WORLD)
                        && state.awaiting_ready;
                    if tutorial_phase_owned {
                        state.tutorial_phase4_acknowledged = true;
                    }
                    (state.awaiting_ready, tutorial_phase_owned)
                };
                if tutorial_phase_owned {
                    return Ok(());
                }
                if should_ready {
                    let _ =
                        send_docs_exclusive(&runtime.outbound_tx, protocol::make_ready_to_play())
                            .await;

                    sleep(Duration::from_millis(1000)).await;

                    if let Some(world) = self.state.read().await.world.clone() {
                        if let (Some(map_x), Some(map_y), Some(world_x), Some(world_y)) = (
                            world.spawn_map_x,
                            world.spawn_map_y,
                            world.spawn_world_x,
                            world.spawn_world_y,
                        ) {
                            let _ = send_docs_exclusive(
                                &runtime.outbound_tx,
                                protocol::make_spawn_packets(
                                map_x.round() as i32,
                                map_y.round() as i32,
                                world_x,
                                world_y,
                                ),
                            )
                            .await;
                        }
                    }

                    {
                        let mut state = self.state.write().await;
                        state.awaiting_ready = false;
                        state.status = SessionStatus::InWorld;
                    }
                    let _ = send_scheduler_cmd(
                        &runtime.outbound_tx,
                        SchedulerCommand::SetPhase {
                            phase: SchedulerPhase::WorldIdle,
                        },
                    )
                    .await;
                    self.publish_snapshot().await;
                }
            }
            ids::PACKET_ID_REDIRECT => {
                const SERVERFULL_RETRY_LIMIT: u32 = 30;

                let redirect_host = message.get_str("IP").unwrap_or_default().to_string();
                let er = message.get_str("ER").ok().map(ToOwned::to_owned);
                let is_serverfull = er.as_deref() == Some("ServerFull");

                // Snapshot state and bump retry counter atomically.
                let (fallback, is_instance, retry_count) = {
                    let mut state = self.state.write().await;
                    if is_serverfull {
                        state.serverfull_retries = state.serverfull_retries.saturating_add(1);
                    } else {
                        // Normal redirect — reset the counter, this isn't a queue retry.
                        state.serverfull_retries = 0;
                    }
                    let world = state
                        .pending_world
                        .clone()
                        .or_else(|| state.current_world.clone());
                    (world, state.pending_world_is_instance, state.serverfull_retries)
                };

                if is_serverfull && retry_count > SERVERFULL_RETRY_LIMIT {
                    self.set_error(format!(
                        "OoIP ServerFull: still full after {SERVERFULL_RETRY_LIMIT} retries"
                    ))
                    .await;
                    return Ok(());
                }

                let world = message
                    .get_str("WN")
                    .ok()
                    .map(ToOwned::to_owned)
                    .or(fallback);

                let _ = runtime.stop_tx.send(true);
                self.update_status(SessionStatus::Redirecting, None).await;
                let new_runtime = self.establish_connection(Some(redirect_host)).await?;
                *runtime = new_runtime;

                if let Some(world) = world {
                    {
                        let mut state = self.state.write().await;
                        state.pending_world = Some(world.clone());
                    }
                    if is_serverfull {
                        // Re-issue TTjW with Amt=retry_count to ask the matchmaker
                        // for a different shard. establish_connection already replayed
                        // the VChk + GPd handshake on the new socket.
                        self.logger.info("session", Some(&self.id),
                            format!("OoIP ServerFull retry #{retry_count} for {world}"));
                        let _ = send_doc(
                            &runtime.outbound_tx,
                            protocol::make_join_world_retry(&world, retry_count as i32),
                        )
                        .await;
                    } else if is_instance {
                        let _ = send_docs_exclusive(
                            &runtime.outbound_tx,
                            vec![
                                protocol::make_world_load_args(&[0]),
                                protocol::make_join_world_special(&world, 0),
                            ],
                        )
                        .await;
                    } else if world.to_uppercase() == "MINEWORLD" {
                        let _ = send_docs_exclusive(
                            &runtime.outbound_tx,
                            vec![
                                protocol::make_world_action_mine(0),
                                protocol::make_join_world_special(&world, 0),
                            ],
                        )
                        .await;
                    } else {
                        let _ = send_doc(&runtime.outbound_tx, protocol::make_join_world(&world)).await;
                    }
                }
            }
            ids::PACKET_ID_ALREADY_CONNECTED => {
                self.set_error("server reported Already Connected".to_string())
                    .await;
            }
            ids::PACKET_ID_KICK_ERROR => {
                let error_code = message.get_i32("ER").unwrap_or_default();
                let body = protocol::log_message(&message);

                // Suspected code → reason mapping. Filled in over time as we
                // observe (action that triggered) → (ER code) pairs. The game
                // DLL has anti-cheat tags like "SpeedHackDetected" but no
                // exposed numeric enum, so these are educated guesses.
                let reason_hint = match error_code {
                    1 => "anim/physics mismatch (likely a=FALL on flat-y or position outside reachable delta)",
                    2 => "rate limit / flood (too many actions per second)",
                    3 => "invalid wear (equipping a block id you don't own)",
                    4 => "invalid HB target (mining out of range or non-mineable)",
                    7 => "speed-hack / movement violation",
                    _ => "unknown — collect more samples to map",
                };

                let raw_hex = message.to_vec().ok().map(|b| hex::encode(b)).unwrap_or_else(|| "ERR_BSON_SERIALIZE".to_string());
                self.logger.error("session", Some(&self.id), 
                    format!("KICKED: code={} hint={} raw_bson={}", error_code, reason_hint, raw_hex));

                let (last_action, action_age, pos) = {
                    let st = self.state.read().await;
                    let age = st.last_action_at.map(|t| t.elapsed());
                    (
                        st.last_action_hint.clone(),
                        age,
                        st.player_position.clone(),
                    )
                };

                let action_str = match (last_action.as_deref(), action_age) {
                    (Some(a), Some(age)) => format!("{a} ({}ms ago)", age.as_millis()),
                    _ => "no recent action recorded".to_string(),
                };

                self.set_error(format!(
                    "kicked by server: code={error_code} hint=\"{reason_hint}\" \
                     last_action={action_str} pos=({:?},{:?}) world=({:?},{:?}) \
                     full_packet={body}",
                    pos.map_x, pos.map_y, pos.world_x, pos.world_y,
                ))
                .await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn update_status(&self, status: SessionStatus, last_error: Option<String>) {
        {
            let mut state = self.state.write().await;
            state.status = status;
            state.last_error = last_error;
        }
        self.publish_snapshot().await;
    }

    async fn set_error(&self, error: String) {
        self.logger.error("session", Some(&self.id), &error);
        {
            let mut state = self.state.write().await;
            state.status = SessionStatus::Error;
            state.last_error = Some(error);
        }
        self.publish_snapshot().await;
    }

    async fn reset_world_state(&self, status: SessionStatus) {
        {
            let mut state = self.state.write().await;
            state.status = status;
            state.current_world = None;
            state.pending_world = None;
            state.pending_world_is_instance = false;
            state.world = None;
            state.world_foreground_tiles.clear();
            state.world_background_tiles.clear();
            state.world_water_tiles.clear();
            state.world_wiring_tiles.clear();
            state.current_outbound_tx = None;
            state.growing_tiles.clear();
            state.collectables.clear();
            state.other_players.clear();
                            state.ai_enemies.clear();
            state.player_position = PlayerPosition {
                map_x: None,
                map_y: None,
                world_x: None,
                world_y: None,
            };
            state.awaiting_ready = false;
            state.tutorial_spawn_pod_confirmed = false;
            state.tutorial_automation_running = false;
            state.tutorial_phase4_acknowledged = false;
            state.fishing = FishingAutomationState::default();
            state.last_error = None;
        }
        self.publish_snapshot().await;
    }

    async fn publish_snapshot(&self) {
        let snapshot = self.snapshot().await;
        *self.last_position_publish_at.lock() = Some(Instant::now());
        self.logger.session_snapshot(snapshot);
    }

    async fn publish_snapshot_position_throttled(&self) {
        let now = Instant::now();
        let should_publish = {
            let mut last = self.last_position_publish_at.lock();
            match *last {
                Some(prev) if now.duration_since(prev) < POSITION_PUBLISH_THROTTLE => false,
                _ => {
                    *last = Some(now);
                    true
                }
            }
        };
        if should_publish {
            let snapshot = self.snapshot().await;
            self.logger.session_snapshot(snapshot);
        }
    }

    async fn maybe_update_player_positions(&self, message: &Document) {
        let packet_uid = message.get_str("U").ok();
        let local_uid = self.state.read().await.user_id.clone();
        if let Some(uid) = packet_uid {
            let mut state = self.state.write().await;
            if local_uid.as_deref() == Some(uid) {
                let changed = update_player_position_from_message(message, &mut state.player_position);
                if let Ok(direction) = message.get_i32("d") {
                    state.current_direction = direction;
                }
                drop(state);
                if changed {
                    self.publish_snapshot_position_throttled().await;
                }
                return;
            }

            let remote = state
                .other_players
                .entry(uid.to_string())
                .or_insert(PlayerPosition {
                    map_x: None,
                    map_y: None,
                    world_x: None,
                    world_y: None,
                });
            let changed = update_player_position_from_message(message, remote);
            drop(state);
            if changed {
                self.publish_snapshot_position_throttled().await;
            }
            return;
        }

        if let Ok(ai_id) = message.get_i32("AIid") {
            let mut state = self.state.write().await;
            if let Some(ai) = state.ai_enemies.get_mut(&ai_id) {
                let mut dummy_pos = PlayerPosition {
                    map_x: Some(ai.map_x as f64),
                    map_y: Some(ai.map_y as f64),
                    world_x: None,
                    world_y: None,
                };
                if update_player_position_from_message(message, &mut dummy_pos) {
                    if let Some(x) = dummy_pos.map_x { ai.map_x = x as i32; }
                    if let Some(y) = dummy_pos.map_y { ai.map_y = y as i32; }
                    drop(state);
                    self.publish_snapshot_position_throttled().await;
                }
            }
            return;
        }
    }

    async fn remove_other_player(&self, message: &Document) {
        let Ok(user_id) = message.get_str("U") else {
            return;
        };
        let local_uid = self.state.read().await.user_id.clone();
        if local_uid.as_deref() == Some(user_id) {
            return;
        }

        let removed = self.state.write().await.other_players.remove(user_id).is_some();
        if removed {
            self.publish_snapshot().await;
        }
    }

    async fn apply_set_block_message(&self, message: &Document) {
        let Ok(map_x) = message.get_i32("x") else {
            return;
        };
        let Ok(map_y) = message.get_i32("y") else {
            return;
        };
        let block_id = match message.get_i32("BlockType") {
            Ok(value) if value >= 0 => value as u16,
            _ => return,
        };

        let changed = {
            let mut state = self.state.write().await;
            apply_foreground_block_change(&mut state, map_x, map_y, block_id)
        };
        if changed {
            self.publish_snapshot().await;
        }
    }

    async fn apply_seed_growth_message(&self, message: &Document) {
        let Ok(map_x) = message.get_i32("x") else {
            return;
        };
        let Ok(map_y) = message.get_i32("y") else {
            return;
        };
        let Ok(growth_end_time) = message.get_i64("GrowthEndTime") else {
            return;
        };
        let Ok(block_id) = message.get_i32("BlockType") else {
            return;
        };

        let growth = GrowingTileState {
            block_id: block_id.max(0) as u16,
            growth_end_time,
            growth_duration_secs: message.get_i32("GrowthDuration").unwrap_or_default().max(0),
            mixed: message.get_bool("Mixed").unwrap_or(false),
            harvest_seeds: message.get_i32("HarvestSeeds").unwrap_or_default().max(0),
            harvest_blocks: message.get_i32("HarvestBlocks").unwrap_or_default().max(0),
            harvest_gems: message.get_i32("HarvestGems").unwrap_or_default().max(0),
            harvest_extra_blocks: message
                .get_i32("HarvestExtraBlocks")
                .unwrap_or_default()
                .max(0),
        };

        self.state
            .write()
            .await
            .growing_tiles
            .insert((map_x, map_y), growth);
    }

    async fn apply_destroy_block_message(&self, message: &Document) {
        let Ok(map_x) = message.get_i32("x") else {
            return;
        };
        let Ok(map_y) = message.get_i32("y") else {
            return;
        };

        let changed = {
            let mut state = self.state.write().await;
            state.growing_tiles.remove(&(map_x, map_y));
            state.pending_hits.remove(&(map_x, map_y));
            apply_destroy_block_change(&mut state, map_x, map_y)
        };
        if changed {
            self.publish_snapshot().await;
        }
    }

    async fn maybe_apply_spawn_pot_selection(&self, message: &Document) {
        let Ok(values) = message.get_array("APu") else {
            return;
        };

        let picked = values
            .iter()
            .filter_map(|value| match value {
                bson::Bson::Int32(value) => Some(*value),
                bson::Bson::Int64(value) => i32::try_from(*value).ok(),
                _ => None,
            })
            .collect::<Vec<_>>();

        if picked != tutorial::POST_CHARACTER_POD_CONFIRMATION {
            return;
        }

        let should_reflect = {
            let state = self.state.read().await;
            state.current_world.as_deref() == Some(tutorial::TUTORIAL_WORLD)
                || state.pending_world.as_deref() == Some(tutorial::TUTORIAL_WORLD)
        };
        if !should_reflect {
            return;
        }

        self.logger.state(
            Some(&self.id),
            format!(
                "server confirmed tutorial pod APu={picked:?}, bot will walk to map=({}, {})",
                tutorial::SPAWN_POT_MAP_X,
                tutorial::SPAWN_POT_MAP_Y,
            ),
        );
        let mut state = self.state.write().await;
        state.tutorial_spawn_pod_confirmed = true;
    }

    async fn apply_inventory_update(&self, message: &Document) {
        let get_u16 = |key: &str| -> u16 {
            message.get(key)
                .and_then(|v| v.as_i64().map(|i| i as u16)
                .or_else(|| v.as_f64().map(|f| f as u16)))
                .unwrap_or_default()
        };
        let get_i32 = |key: &str| -> Option<i32> {
            message.get(key)
                .and_then(|v| v.as_i64().map(|i| i as i32)
                .or_else(|| v.as_f64().map(|f| f as i32)))
        };

        let Some(inventory_key) = get_i32("Bi") else { 
            self.logger.warn("session", Some(&self.id), format!("InventoryUpdate missing Bi: {}", protocol::log_message(message)));
            return; 
        };
        let amount = get_u16("Amt");
        let block_id = get_u16("BT");
        let inventory_type = get_u16("IT");

        self.logger.info("session", Some(&self.id), 
            format!("InventoryUpdate: key={} block_id={} amount={} type={}", inventory_key, block_id, amount, inventory_type));

        let mut state = self.state.write().await;
        if amount == 0 {
            state.inventory.retain(|e| e.inventory_key != inventory_key);
        } else {
            if let Some(entry) = state.inventory.iter_mut().find(|e| e.inventory_key == inventory_key) {
                entry.amount = amount;
                entry.block_id = block_id;
                entry.inventory_type = inventory_type;
                self.logger.info("session", Some(&self.id), format!("Updated existing inventory entry for key={}", inventory_key));
            } else {
                state.inventory.push(InventoryEntry {
                    inventory_key,
                    block_id,
                    inventory_type,
                    amount,
                });
                self.logger.info("session", Some(&self.id), format!("Created new inventory entry for key={}", inventory_key));
            }
        }
        drop(state);
        self.publish_snapshot().await;
    }

    async fn track_collectable(&self, message: &Document, is_nwc: bool) {
        self.logger.info("session", Some(&self.id), format!("COLLECTABLE PACKET: {}", protocol::log_message(message)));

        let Some(collectable_id) = message.get_i32("CollectableID").ok()
            .or_else(|| message.get_i32("id").ok())
            .or_else(|| message.get_i32("cid").ok()) else {
            return;
        };

        let pos_x = message.get_f64("PosX").ok().or_else(|| message.get_i32("PosX").ok().map(|v| v as f64)).unwrap_or_default();
        let pos_y = message.get_f64("PosY").ok().or_else(|| message.get_i32("PosY").ok().map(|v| v as f64)).unwrap_or_default();
        let (mx, my) = protocol::world_to_map(pos_x, pos_y);

        let collectable = CollectableState {
            collectable_id,
            block_type: message.get_i32("BlockType").unwrap_or_default(),
            amount: message.get_i32("Amount").unwrap_or_default(),
            inventory_type: message.get_i32("InventoryType").unwrap_or_default(),
            pos_x,
            pos_y,
            map_x: mx.floor() as i32,
            map_y: my.floor() as i32,
            is_gem: message.get_bool("IsGem").unwrap_or(false),
            gem_type: message.get_i32("GemType").unwrap_or_default(),
            is_nwc,
        };

        self.state
            .write()
            .await
            .collectables
            .insert(collectable_id, collectable);
    }

    async fn remove_collectable(&self, message: &Document) {
        let Some(collectable_id) = message.get_i32("CollectableID").ok() else {
            return;
        };
        self.state
            .write()
            .await
            .collectables
            .remove(&collectable_id);
    }

    /// Record an AI enemy's last known position from an AIHD packet.
    /// AIHD fields: BDmg, IC, AIid, HBv, x, y. IC=true (or HBv<=0) means the enemy died.
    async fn track_ai_enemy(&self, message: &Document) {
        let Ok(ai_id) = message.get_i32("AIid") else {
            return;
        };
        let _ = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("ai_debug.log")
            .await
            .map(|mut f| {
                use tokio::io::AsyncWriteExt;
                let _ = f.write_all(format!("AIHD: {}\n", message).as_bytes());
            });

        let x_val = message.get_f64("x").ok().or_else(|| message.get_i32("x").ok().map(|v| v as f64)).unwrap_or_default();
        let y_val = message.get_f64("y").ok().or_else(|| message.get_i32("y").ok().map(|v| v as f64)).unwrap_or_default();
        
        let map_x = (x_val / 32.0) as i32;
        let map_y = (y_val / 32.0) as i32;
        let killed = message.get_bool("IC").unwrap_or(false)
            || message.get_i32("HBv").map(|hp| hp <= 0).unwrap_or(false);

        let mut state = self.state.write().await;
        if killed {
            state.ai_enemies.remove(&ai_id);
        } else {
            state.ai_enemies.insert(
                ai_id,
                AiEnemyState {
                    ai_id,
                    map_x,
                    map_y,
                    last_seen: Instant::now(),
                    alive: true,
                },
            );
        }
    }

    /// Server announces every AI enemy in the loaded world via a stream of
    /// `AI` packets, each carrying a 37-byte binary `AId` blob (observed in
    /// Seraph capture: `AI {ID="AI" AId=<binary:37B>} (x28)` for 28 enemies).
    ///
    /// The exact byte layout isn't decoded yet — we log the raw hex so we can
    /// reverse the format from a real capture. The first 4 LE bytes are
    /// almost certainly the i32 ai_id; the next 8 bytes are likely (x, y) as
    /// i32+i32 by analogy with the `mp` (map-point) packet which is 8 bytes
    /// of (x, y) i32 pair. The remaining 25 bytes likely encode enemy type,
    /// HP, and animation state.
    async fn track_ai_spawn(&self, message: &Document) {
        let Some(blob) = protocol::binary_bytes(message.get("AId")) else {
            self.logger.warn("session", Some(&self.id),
                "AI packet missing AId binary field".to_string());
            return;
        };

        if blob.len() != 37 {
            // True AI spawns are exactly 37 bytes long (as observed in captures).
            // Any other length means this is a different AI event type.
            return;
        }

        // Byte 12: Event Type.
        // Observed types: 4=Spawn, 1=Move/Update?
        let event_type = blob[12];
        if event_type != 4 {
            // Event types: 1=Move, 2=Update/Hit, 4=Spawn, 6=Death/Removal?
            if blob.len() >= 26 && (event_type == 1 || event_type == 2) {
                let ai_id = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
                let map_x = i32::from_le_bytes([blob[18], blob[19], blob[20], blob[21]]);
                let map_y = i32::from_le_bytes([blob[22], blob[23], blob[24], blob[25]]);
                
                if (0..=1024).contains(&map_x) && (0..=1024).contains(&map_y) {
                    let mut state = self.state.write().await;
                    if let Some(enemy) = state.ai_enemies.get_mut(&ai_id) {
                        enemy.map_x = map_x;
                        enemy.map_y = map_y;
                        enemy.last_seen = Instant::now();
                    }
                    return;
                }
            }

            if event_type == 6 {
                let ai_id = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
                let mut state = self.state.write().await;
                state.ai_enemies.remove(&ai_id);
                return;
            }

            // Log other event types for analysis
            self.logger.info("session", Some(&self.id), 
                format!("AI packet event_type={} len={} hex={}", event_type, blob.len(), hex::encode(blob)));
            return;
        }

        // Byte 14: Entity sub-type. 0x1c (28) is the static spawn point format
        // where bytes 18-25 are the (x, y) i32 pair. Other values (e.g. 0x09)
        // are different entity kinds whose payload uses different field offsets.
        // Reading bytes 18-25 on those gives garbage coords like (-1811939316,
        // -369098673) which then poison combat targeting via i32 wraparound.
        if blob[14] != 0x1c {
            return;
        }

        // Confirmed offsets from packet analysis:
        // Byte 8-11:  ai_id (i32 LE)
        // Byte 18-21: map_x (i32 LE)
        // Byte 22-25: map_y (i32 LE)
        let ai_id = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
        let map_x = i32::from_le_bytes([blob[18], blob[19], blob[20], blob[21]]);
        let map_y = i32::from_le_bytes([blob[22], blob[23], blob[24], blob[25]]);

        // Sanity bound: PixelWorlds maps top out around 200x200. Anything outside
        // [0, 1024] is corrupt — refuse to register it. This is belt-and-braces
        // alongside the byte 14 filter above.
        if !(0..=1024).contains(&map_x) || !(0..=1024).contains(&map_y) {
            self.logger.warn("session", Some(&self.id),
                format!("rejected AI spawn ID={ai_id} with out-of-range coords ({map_x},{map_y})"));
            return;
        }

        // self.logger.info("session", Some(&self.id),
        //     format!("AI spawn ID={ai_id} at ({map_x},{map_y})"));

        let mut state = self.state.write().await;
        state.ai_enemies.insert(
            ai_id,
            AiEnemyState {
                ai_id,
                map_x,
                map_y,
                last_seen: Instant::now(),
                alive: true,
            },
        );
        drop(state);
        self.publish_snapshot_position_throttled().await;
    }

    async fn apply_fishing_message(
        &self,
        message: &Document,
        outbound_tx: &OutboundHandle,
    ) {
        let minigame_type = message.get_i32("MGT").unwrap_or_default();
        if minigame_type != 2 {
            return;
        }

        let mut state = self.state.write().await;
        if !state.fishing.active {
            return;
        }

        let mgd = message
            .get_i64("MGD")
            .ok()
            .or_else(|| message.get_i32("MGD").ok().map(i64::from))
            .unwrap_or_default();

        match mgd {
            2 => {
                state.fishing.phase = FishingPhase::HookPrompted;
                if !state.fishing.hook_sent {
                    state.fishing.hook_sent = true;
                    let _ = send_doc(outbound_tx, protocol::make_fishing_hook_action()).await;
                }
            }
            3 => {
                state.fishing.phase = FishingPhase::GaugeActive;
                state.fishing.fish_block = message.get_i32("BT").ok();
                state.fishing.rod_block = message.get_i32("WBT").ok();
                let now = Instant::now();
                state.fishing.gauge_entered_at = Some(now);
                initialize_fishing_gauge(&mut state.fishing, now);
            }
            1 | 5 => {
                state.fishing = FishingAutomationState::default();
            }
            _ => {}
        }
    }
}

fn update_player_position_from_message(message: &Document, position: &mut PlayerPosition) -> bool {
    let previous = position.clone();

    // PW packets can send coordinates as either f64 or i32 depending on the entity type.
    // We must try both to ensure we don't miss updates.
    if let Some(x) = message.get_f64("x").ok().or_else(|| message.get_i32("x").ok().map(|v| v as f64)) {
        position.world_x = Some(x);
        let (map_x, _) = protocol::world_to_map(x, position.world_y.unwrap_or_default());
        position.map_x = Some(map_x);
    }
    if let Some(y) = message.get_f64("y").ok().or_else(|| message.get_i32("y").ok().map(|v| v as f64)) {
        position.world_y = Some(y);
        let (_, map_y) = protocol::world_to_map(position.world_x.unwrap_or_default(), y);
        position.map_y = Some(map_y);
    }

    position.map_x != previous.map_x
        || position.map_y != previous.map_y
        || position.world_x != previous.world_x
        || position.world_y != previous.world_y
}

#[derive(Debug)]
struct ActiveRuntime {
    id: u64,
    outbound_tx: OutboundHandle,
    stop_tx: watch::Sender<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendMode {
    Mergeable,
    ExclusiveBatch,
    ImmediateExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueuePriority {
    BeforeGenerated,
    AfterGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerPhase {
    Disconnected,
    MenuIdle,
    MenuStBurst,
    WorldIdle,
    WorldMoving,
}

#[derive(Debug)]
enum SchedulerCommand {
    EnqueuePackets {
        docs: Vec<Document>,
        mode: SendMode,
        priority: QueuePriority,
    },
    UpdateMovement {
        world_x: f64,
        world_y: f64,
        is_moving: bool,
        anim: i32,
        direction: i32,
    },
    SetPhase {
        phase: SchedulerPhase,
    },
    StResponseReceived,
    Shutdown,
}

type OutboundHandle = mpsc::Sender<SchedulerCommand>;

#[derive(Debug, Clone)]
struct MovementTickState {
    in_world: bool,
    world_x: f64,
    world_y: f64,
    is_moving: bool,
    anim: i32,
    direction: i32,
}

impl Default for MovementTickState {
    fn default() -> Self {
        Self {
            in_world: false,
            world_x: 0.0,
            world_y: 0.0,
            is_moving: false,
            anim: movement::ANIM_IDLE,
            direction: movement::DIR_RIGHT,
        }
    }
}

#[derive(Debug)]
enum PendingBatch {
    Mergeable {
        docs: Vec<Document>,
        priority: QueuePriority,
    },
    Exclusive(Vec<Document>),
}

struct StSyncState {
    samples: [i32; timing::ST_SAMPLE_COUNT],
    sample_count: usize,
    success_counter: i32,
    interval_secs: i32,
    last_sent_at: Option<std::time::Instant>,
}

impl StSyncState {
    fn new() -> Self {
        Self {
            samples: [i32::MAX; timing::ST_SAMPLE_COUNT],
            sample_count: 0,
            success_counter: 0,
            interval_secs: timing::ST_INTERVAL_INIT_SECS,
            last_sent_at: None,
        }
    }

    fn record_sample(&mut self, rtt_ms: i32) {
        let idx = self.sample_count % timing::ST_SAMPLE_COUNT;
        self.samples[idx] = rtt_ms;
        self.sample_count += 1;

        let valid_count = self.sample_count.min(timing::ST_SAMPLE_COUNT);
        let mut sorted = self.samples;
        sorted[..valid_count].sort_unstable();
        let median = sorted[valid_count / 2];

        let deviation = (rtt_ms - median).abs();

        if deviation >= self.interval_secs * 1000 {
            self.interval_secs =
                (self.interval_secs - timing::ST_INTERVAL_STEP_SECS).max(timing::ST_INTERVAL_MIN_SECS);
            self.success_counter = 0;
        } else {
            self.success_counter += 1;
            if self.success_counter > timing::ST_SUCCESS_THRESHOLD {
                self.success_counter = 0;
                self.interval_secs =
                    (self.interval_secs + timing::ST_INTERVAL_STEP_SECS).min(timing::ST_INTERVAL_MAX_SECS);
            }
        }
    }
}

struct SchedulerState {
    phase: SchedulerPhase,
    movement: MovementTickState,
    st_sync: StSyncState,
    pending: VecDeque<PendingBatch>,
    immediate: VecDeque<Vec<Document>>,
    menu_keepalive_due: bool,
    st_due: bool,
}

impl SchedulerState {
    fn new() -> Self {
        Self {
            phase: SchedulerPhase::MenuStBurst,
            movement: MovementTickState::default(),
            st_sync: StSyncState::new(),
            pending: VecDeque::new(),
            immediate: VecDeque::new(),
            menu_keepalive_due: false,
            st_due: true,
        }
    }

    fn has_immediate_batch(&self) -> bool {
        !self.immediate.is_empty()
    }

    fn is_menu_phase(&self) -> bool {
        matches!(
            self.phase,
            SchedulerPhase::MenuIdle | SchedulerPhase::MenuStBurst
        )
    }

    fn enqueue_packets(&mut self, docs: Vec<Document>, mode: SendMode, priority: QueuePriority) {
        if docs.is_empty() {
            return;
        }
        match mode {
            SendMode::ImmediateExclusive => self.immediate.push_back(docs),
            SendMode::ExclusiveBatch => self.pending.push_back(PendingBatch::Exclusive(docs)),
            SendMode::Mergeable => match self.pending.back_mut() {
                Some(PendingBatch::Mergeable {
                    docs: queued,
                    priority: queued_priority,
                }) if *queued_priority == priority => queued.extend(docs),
                _ => self
                    .pending
                    .push_back(PendingBatch::Mergeable { docs, priority }),
            },
        }
    }

    fn update_movement(
        &mut self,
        world_x: f64,
        world_y: f64,
        is_moving: bool,
        anim: i32,
        direction: i32,
    ) {
        self.movement.world_x = world_x;
        self.movement.world_y = world_y;
        self.movement.is_moving = is_moving;
        self.movement.anim = anim;
        self.movement.direction = direction;
        if matches!(
            self.phase,
            SchedulerPhase::WorldIdle | SchedulerPhase::WorldMoving
        ) {
            self.phase = if is_moving {
                SchedulerPhase::WorldMoving
            } else {
                SchedulerPhase::WorldIdle
            };
        }
    }

    fn set_phase(&mut self, phase: SchedulerPhase) {
        self.phase = match phase {
            SchedulerPhase::WorldIdle | SchedulerPhase::WorldMoving => {
                self.movement.in_world = true;
                if self.movement.is_moving {
                    SchedulerPhase::WorldMoving
                } else {
                    SchedulerPhase::WorldIdle
                }
            }
            SchedulerPhase::MenuIdle | SchedulerPhase::MenuStBurst => {
                self.movement.in_world = false;
                self.movement.is_moving = false;
                self.menu_keepalive_due = false;
                phase
            }
            SchedulerPhase::Disconnected => {
                self.movement.in_world = false;
                self.movement.is_moving = false;
                SchedulerPhase::Disconnected
            }
        };
    }

    fn mark_menu_keepalive_due(&mut self) {
        if self.is_menu_phase() {
            self.menu_keepalive_due = true;
        }
    }

    fn mark_st_due(&mut self) {
        self.st_due = true;
    }

    fn take_immediate_batch(&mut self) -> Option<Vec<Document>> {
        self.immediate.pop_front()
    }

    fn take_slot_batch(&mut self) -> Option<Vec<Document>> {
        if let Some(PendingBatch::Exclusive(_)) = self.pending.front() {
            if let Some(PendingBatch::Exclusive(docs)) = self.pending.pop_front() {
                return Some(docs);
            }
        }

        let mut before_generated = Vec::new();
        let mut after_generated = Vec::new();
        while let Some(PendingBatch::Mergeable { .. }) = self.pending.front() {
            if let Some(PendingBatch::Mergeable { docs, priority }) = self.pending.pop_front() {
                match priority {
                    QueuePriority::BeforeGenerated => before_generated.extend(docs),
                    QueuePriority::AfterGenerated => after_generated.extend(docs),
                }
            }
        }

        let mut batch = before_generated;
        match self.phase {
            SchedulerPhase::Disconnected => {
                if batch.is_empty() && after_generated.is_empty() {
                    return None;
                }
            }
            SchedulerPhase::MenuIdle => {
                batch.extend(after_generated);
                if self.menu_keepalive_due {
                    batch.push(protocol::make_keepalive());
                    self.menu_keepalive_due = false;
                }
                if self.st_due {
                    batch.push(protocol::make_st());
                    self.st_due = false;
                }
                if batch.is_empty() {
                    return Some(Vec::new());
                }
            }
            SchedulerPhase::MenuStBurst => {
                batch.extend(after_generated);
                if self.menu_keepalive_due {
                    batch.push(protocol::make_keepalive());
                    self.menu_keepalive_due = false;
                }
                if self.st_due {
                    batch.push(protocol::make_st());
                    self.st_due = false;
                }
                if batch.is_empty() {
                    return None;
                }
            }
            SchedulerPhase::WorldIdle => {
                batch.extend(after_generated);
                if self.st_due {
                    batch.push(protocol::make_st());
                    self.st_due = false;
                }
                if batch.is_empty() {
                    return None;
                }
            }
            SchedulerPhase::WorldMoving => {
                batch.push(protocol::make_movement_packet(
                    self.movement.world_x,
                    self.movement.world_y,
                    self.movement.anim,
                    self.movement.direction,
                    false
        ));
                batch.extend(after_generated);
                if self.st_due {
                    batch.push(protocol::make_st());
                    self.st_due = false;
                }
            }
        }

        Some(batch)
    }

    fn on_batch_sent(&mut self, batch: &[Document]) -> bool {
        let sent_st = batch.iter().any(|doc| packet_id(doc) == ids::PACKET_ID_ST);
        if sent_st {
            self.st_sync.last_sent_at = Some(std::time::Instant::now());
        }
        sent_st
    }

    fn handle_st_response(&mut self) -> Option<Duration> {
        let sent_at = self.st_sync.last_sent_at.take()?;
        let rtt_ms = sent_at.elapsed().as_millis() as i32;
        self.st_sync.record_sample(rtt_ms);
        if self.st_sync.sample_count < timing::ST_SAMPLE_COUNT {
            self.st_due = true;
            if self.phase == SchedulerPhase::MenuIdle {
                self.phase = SchedulerPhase::MenuStBurst;
            }
            None
        } else {
            if self.phase == SchedulerPhase::MenuStBurst {
                self.phase = SchedulerPhase::MenuIdle;
            }
            Some(Duration::from_secs(self.st_sync.interval_secs as u64))
        }
    }
}

fn packet_id(doc: &Document) -> &str {
    doc.get_str("ID").unwrap_or_default()
}

async fn write_logged_batch(
    writer: &mut OwnedWriteHalf,
    logger: &Logger,
    session_id: &str,
    batch: &[Document],
) -> Result<(), String> {
    logger.tcp_trace(Direction::Outgoing, "tcp", Some(session_id), || {
        protocol::log_batch(batch)
    });
    protocol::write_batch(writer, batch).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FishingPhase {
    Idle,
    WaitingForHook,
    HookPrompted,
    GaugeActive,
    CleanupPending,
    Completed,
}

#[derive(Debug, Clone)]
struct FishingAutomationState {
    active: bool,
    phase: FishingPhase,
    target_map_x: Option<i32>,
    target_map_y: Option<i32>,
    bait_name: Option<String>,
    last_result: Option<String>,
    fish_block: Option<i32>,
    rod_block: Option<i32>,
    gauge_entered_at: Option<Instant>,
    hook_sent: bool,
    land_sent: bool,
    cleanup_pending: bool,
    sim_last_at: Option<Instant>,
    sim_fish_position: f64,
    sim_target_position: f64,
    sim_progress: f64,
    sim_overlap_threshold: f64,
    sim_fill_rate: f64,
    sim_target_speed: f64,
    sim_fish_move_speed: f64,
    sim_run_frequency: f64,
    sim_pull_strength: f64,
    sim_min_land_delay: f64,
    sim_phase: f64,
    sim_overlap: bool,
    sim_ready_since: Option<Instant>,
    sim_difficulty_meter: f64,
    sim_size_multiplier: f64,
    sim_drag_extra: f64,
    sim_run_active: bool,
    sim_run_until: Option<Instant>,
    sim_force_land_after: Option<Instant>,
}

impl Default for FishingAutomationState {
    fn default() -> Self {
        Self {
            active: false,
            phase: FishingPhase::Idle,
            target_map_x: None,
            target_map_y: None,
            bait_name: None,
            last_result: None,
            fish_block: None,
            rod_block: None,
            gauge_entered_at: None,
            hook_sent: false,
            land_sent: false,
            cleanup_pending: false,
            sim_last_at: None,
            sim_fish_position: 0.5,
            sim_target_position: 0.5,
            sim_progress: 0.5,
            sim_overlap_threshold: 0.13,
            sim_fill_rate: 0.12,
            sim_target_speed: 0.4,
            sim_fish_move_speed: 0.8,
            sim_run_frequency: 0.04,
            sim_pull_strength: 3.4,
            sim_min_land_delay: 4.8,
            sim_phase: 0.0,
            sim_overlap: false,
            sim_ready_since: None,
            sim_difficulty_meter: 0.0,
            sim_size_multiplier: 0.0,
            sim_drag_extra: 1.0,
            sim_run_active: false,
            sim_run_until: None,
            sim_force_land_after: None,
        }
    }
}

#[derive(Debug, Default)]
struct CollectCooldowns {
    cooldowns: HashMap<i32, Instant>,
}

impl CollectCooldowns {
    const COOLDOWN: Duration = Duration::from_secs(3); // 3s per item minimum

    fn can_collect(&self, id: i32) -> bool {
        match self.cooldowns.get(&id) {
            Some(&last) => last.elapsed() >= Self::COOLDOWN,
            None => true,
        }
    }

    fn mark_collected(&mut self, id: i32) {
        self.cooldowns.insert(id, Instant::now());
        // Cleanup old entries (older than 30s)
        self.cooldowns.retain(|_, last| last.elapsed() < Duration::from_secs(30));
    }
}
#[derive(Debug)]
struct SessionState {
    status: SessionStatus,
    device_id: String,
    current_host: String,
    current_port: u16,
    current_world: Option<String>,
    pending_world: Option<String>,
    pending_world_is_instance: bool,
    /// Counter for OoIP "ServerFull" retries. The game server uses the `Amt`
    /// field on the next TTjW to mean "try shard #N instead of the full one".
    /// Reset on successful world entry (GET_WORLD_CONTENT) and when the user
    /// issues a fresh JoinWorld command.
    serverfull_retries: u32,
    /// Last "interesting" outbound action description, kept so we can correlate
    /// KErr kicks with the bot action that triggered them. Set on every move /
    /// hit / HAI by the automine loop.
    last_action_hint: Option<String>,
    last_action_at: Option<Instant>,
    username: Option<String>,
    user_id: Option<String>,
    world: Option<WorldSnapshot>,
    world_foreground_tiles: Vec<u16>,
    world_background_tiles: Vec<u16>,
    world_water_tiles: Vec<u16>,
    world_wiring_tiles: Vec<u16>,
    current_outbound_tx: Option<OutboundHandle>,
    growing_tiles: HashMap<(i32, i32), GrowingTileState>,
    player_position: PlayerPosition,
    other_players: HashMap<String, PlayerPosition>,
    ai_enemies: HashMap<i32, AiEnemyState>,
    inventory: Vec<InventoryEntry>,
    collectables: HashMap<i32, CollectableState>,
    current_direction: i32,
    last_error: Option<String>,
    awaiting_ready: bool,
    tutorial_spawn_pod_confirmed: bool,
    tutorial_automation_running: bool,
    /// Tiles we've hit and are waiting for a DB (Destroy Block) confirmation.
    /// Maps (x, y) to the Instant of the LAST hit.
    pending_hits: HashMap<(i32, i32), Instant>,
    tutorial_phase4_acknowledged: bool,
    fishing: FishingAutomationState,
    ping_ms: Option<u32>,
    collect_cooldowns: CollectCooldowns,
    rate_limit_until: Option<Instant>,
    current_target: Option<BotTarget>,
    world_items: Vec<crate::world::DecodedWorldItem>,
    autonether: autonether::AutonetherState,
}

#[derive(Debug)]
enum SessionCommand {
    Connect,
    JoinWorld { world: String, instance: bool },
    LeaveWorld,
    Disconnect,
    AutomateTutorial,
    ManualMove {
        direction: String,
    },
    WearItem {
        block_id: i32,
        equip: bool,
    },
    Punch {
        offset_x: i32,
        offset_y: i32,
    },
    Place {
        offset_x: i32,
        offset_y: i32,
        block_id: i32,
    },
    StartFishing {
        direction: String,
        bait: String,
    },
    StopFishing,
    Talk {
        message: String,
    },
    StartSpam {
        message: String,
        delay_ms: u64,
    },
    StopSpam,
    DropItem {
        block_id: i32,
        amount: i32,
    },
    StartAutomine,
    StopAutomine,
    StartAutonether,
    StopAutonether,
}

#[derive(Debug)]
enum ControllerEvent {
    Command(SessionCommand),
    Inbound(u64, Document),
    ReadLoopStopped(u64, String),
}

#[derive(Debug, Clone)]
struct InventoryEntry {
    inventory_key: i32,
    block_id: u16,
    inventory_type: u16,
    amount: u16,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AiEnemyState {
    ai_id: i32,
    map_x: i32,
    map_y: i32,
    last_seen: Instant,
    alive: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CollectableState {
    pub(crate) collectable_id: i32,
    pub(crate) block_type: i32,
    pub(crate) amount: i32,
    pub(crate) inventory_type: i32,
    pub(crate) pos_x: f64,
    pub(crate) pos_y: f64,
    pub(crate) map_x: i32,
    pub(crate) map_y: i32,
    pub(crate) is_gem: bool,
    pub(crate) gem_type: i32,
    pub(crate) is_nwc: bool,
}

#[derive(Debug, Clone)]
struct FishingTarget {
    direction: String,
    bait_query: String,
    map_x: i32,
    map_y: i32,
}

#[derive(Debug, Clone)]
struct NamedInventoryEntry {
    inventory_key: i32,
    block_id: u16,
    name: String,
}

#[derive(Debug, Clone)]
struct GrowingTileState {
    block_id: u16,
    growth_end_time: i64,
    growth_duration_secs: i32,
    mixed: bool,
    harvest_seeds: i32,
    harvest_blocks: i32,
    harvest_gems: i32,
    harvest_extra_blocks: i32,
}

fn stop_background_worker(stop_tx: &mut Option<watch::Sender<bool>>) {
    if let Some(tx) = stop_tx.take() {
        let _ = tx.send(true);
    }
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(AtomicOrdering::Relaxed) {
        Err("lua script stopped".to_string())
    } else {
        Ok(())
    }
}

fn apply_foreground_block_change(
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

fn apply_destroy_block_change(state: &mut SessionState, map_x: i32, map_y: i32) -> bool {
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

fn tile_index(world: &WorldSnapshot, map_x: i32, map_y: i32) -> Option<usize> {
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

fn is_tile_ready_to_harvest_at(
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

fn tile_snapshot_at(
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

fn summarize_tile_counts(tiles: &[u16]) -> Vec<TileCount> {
    let mut counts = BTreeMap::<u16, u32>::new();
    for &tile_id in tiles {
        *counts.entry(tile_id).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(tile_id, count)| TileCount { tile_id, count })
        .collect()
}

fn block_types() -> &'static HashMap<u16, BlockTypeInfo> {
    BLOCK_TYPES.get_or_init(|| {
        serde_json::from_str::<Vec<BlockTypeInfo>>(include_str!("../../block_types.json"))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.id, entry))
            .collect()
    })
}

fn block_names() -> HashMap<u16, String> {
    block_types()
        .iter()
        .map(|(id, entry)| (*id, entry.name.clone()))
        .collect()
}

fn block_inventory_type_for(block_id: u16) -> Option<u16> {
    block_types().get(&block_id).map(|entry| entry.inventory_type)
}

fn block_name_for(block_id: u16) -> Option<String> {
    block_types().get(&block_id).map(|entry| entry.name.clone())
}

fn block_type_name_for(block_id: u16) -> Option<String> {
    block_types().get(&block_id).map(|entry| entry.type_name.clone())
}

fn normalize_block_name(name: &str) -> String {
    name.chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_inventory_bait(
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

fn find_fishing_map_point(
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

fn rod_family_name(rod_block: Option<i32>) -> &'static str {
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

fn initialize_fishing_gauge(fishing: &mut FishingAutomationState, now: Instant) {
    let rod_block = fishing.rod_block.unwrap_or(2406);
    let fish_name = fishing
        .fish_block
        .and_then(|id| block_name_for(id as u16))
        .unwrap_or_default();
    let normalized = normalize_block_name(&fish_name);
    let bucket = fishing::fish_bucket_from_name(&normalized);
    let rod_profile = fishing::rod_profile(rod_block);

    fishing.sim_overlap_threshold = 0.095 + (rod_profile.slider_size * 0.035);
    fishing.sim_fill_rate = rod_profile.fill_multiplier * 0.10;
    fishing.sim_target_speed = rod_profile.slider_speed * 0.20;
    fishing.sim_fish_move_speed = bucket.fish_move_speed;
    fishing.sim_run_frequency = bucket.run_frequency;
    fishing.sim_pull_strength = fishing::pull_strength(bucket, rod_family_name(Some(rod_block)));
    fishing.sim_min_land_delay = bucket.min_land_delay;
    fishing.sim_last_at = Some(now);
    fishing.sim_fish_position = fishing::DEFAULT_FISH_POSITION;
    fishing.sim_target_position = fishing::DEFAULT_TARGET_POSITION;
    fishing.sim_progress = fishing::DEFAULT_PROGRESS;
    fishing.sim_phase = 0.0;
    fishing.sim_overlap = false;
    fishing.sim_ready_since = None;
    fishing.sim_difficulty_meter = 0.0;
    fishing.sim_size_multiplier = 0.0;
    fishing.sim_drag_extra = fishing::DEFAULT_DRAG_EXTRA;
    fishing.sim_run_active = false;
    fishing.sim_run_until = None;
    fishing.sim_force_land_after = Some(
        now + Duration::from_secs_f64(
            (bucket.min_land_delay + fishing::FORCE_LAND_EXTRA_DELAY_SECS)
                .max(fishing::FORCE_LAND_MIN_SECS),
        ),
    );
    fishing.land_sent = false;
}

fn current_fishing_land_values(fishing: &FishingAutomationState) -> (i32, i32, f64) {
    let size_multiplier = fishing
        .sim_size_multiplier
        .clamp(0.001, fishing::MAX_SIZE_MULTIPLIER);
    let difficulty_meter = fishing
        .sim_difficulty_meter
        .clamp(0.001, fishing::MAX_DIFFICULTY_METER);
    let vendor_index = (size_multiplier * 1000.0).max(1.0) as i32;
    let index_key = (difficulty_meter * 1000.0).max(1.0) as i32;
    let amount = fishing.sim_fish_position - fishing.sim_drag_extra;
    (vendor_index, index_key, amount)
}

fn service_fishing_simulation(
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
            .map(|entered| (now - entered).as_secs_f64() >= fishing::RUN_START_AFTER_SECS)
            .unwrap_or(false)
        && (fishing.sim_phase * (0.75 + move_speed)).sin() > (0.985 - run_frequency)
    {
        fishing.sim_run_active = true;
        fishing.sim_run_until = Some(now + Duration::from_millis(fishing::RUN_DURATION_MS));
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
                    (now - ready_since).as_secs_f64() >= fishing::READY_TO_LAND_DELAY_SECS
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

async fn spam_loop(
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

async fn send_world_chat(
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
>>>>>>> jeli/fix/sidebar-ui-redesign

/// Pickaxe block IDs from block_types.json, ordered best → worst.
/// Without one of these equipped, the server silently ignores HB packets,
/// which is why an un-equipped bot looks like it's "doing nothing".


/// Stamp the most recent bot action onto session state so the KErr handler can
/// later say "you were just doing X when you got kicked".
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bson::{Document, doc};

    use super::{
        BotSession, FishingAutomationState, GrowingTileState, QueuePriority, SchedulerPhase,
        SchedulerState, SendMode, SessionState, drop_target_tile,
        apply_destroy_block_change, apply_foreground_block_change, is_tile_ready_to_harvest_at,
        update_player_position_from_message,
    };
    use crate::{
        constants::{movement, protocol as ids, timing},
        logging::{EventHub, Logger},
        models::{PlayerPosition, SessionStatus, WorldSnapshot},
        protocol,
    };
    use std::{sync::Arc, time::Duration};

    fn test_state(
        width: u32,
        height: u32,
        foreground_tiles: Vec<u16>,
        background_tiles: Vec<u16>,
    ) -> SessionState {
        SessionState {
            status: SessionStatus::InWorld,
            device_id: String::new(),
            current_host: String::new(),
            current_port: 0,
            current_world: Some("TEST".to_string()),
            pending_world: None,
            pending_world_is_instance: false,
            serverfull_retries: 0,
            last_action_hint: None,
            last_action_at: None,
            username: None,
            user_id: None,
            world: Some(WorldSnapshot {
                world_name: Some("TEST".to_string()),
                width,
                height,
                spawn_map_x: None,
                spawn_map_y: None,
                spawn_world_x: None,
                spawn_world_y: None,
                collectables_count: 0,
                world_items_count: 0,
                tile_counts: Vec::new(),
            }),
            world_foreground_tiles: foreground_tiles,
            world_background_tiles: background_tiles,
            world_water_tiles: Vec::new(),
            world_wiring_tiles: Vec::new(),
            pending_hits: HashMap::new(),
            current_outbound_tx: None,
            growing_tiles: HashMap::new(),
            player_position: PlayerPosition {
                map_x: None,
                map_y: None,
                world_x: None,
                world_y: None,
            },
            current_direction: movement_consts::DIR_RIGHT,
            other_players: HashMap::new(),
            ai_enemies: HashMap::new(),
            inventory: Vec::new(),
            collectables: HashMap::new(),
            last_error: None,
            awaiting_ready: false,
            tutorial_spawn_pod_confirmed: false,
            tutorial_automation_running: false,
            tutorial_phase4_acknowledged: false,
            fishing: FishingAutomationState::default(),
            ping_ms: None,
        }
    }

    fn batch_ids(batch: &[Document]) -> Vec<String> {
        batch.iter()
            .map(|doc| doc.get_str("ID").unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn applies_block_placement_to_foreground_tiles() {
        let mut state = test_state(3, 2, vec![0, 0, 0, 0, 0, 0], vec![0, 0, 0, 0, 0, 0]);

        let changed = apply_foreground_block_change(&mut state, 1, 1, 2735);

        assert!(changed);
        assert_eq!(state.world_foreground_tiles[4], 2735);
        let world = state.world.unwrap();
        assert_eq!(
            world
                .tile_counts
                .iter()
                .find(|entry| entry.tile_id == 2735)
                .unwrap()
                .count,
            1
        );
        assert_eq!(
            world
                .tile_counts
                .iter()
                .find(|entry| entry.tile_id == 0)
                .unwrap()
                .count,
            5
        );
    }

    #[test]
    fn applies_block_removal_to_foreground_tiles() {
        let mut state = test_state(2, 2, vec![9, 0, 0, 0], vec![0, 0, 0, 0]);

        let changed = apply_foreground_block_change(&mut state, 0, 0, 0);

        assert!(changed);
        assert_eq!(state.world_foreground_tiles[0], 0);
        let world = state.world.unwrap();
        assert_eq!(world.tile_counts.len(), 1);
        assert_eq!(world.tile_counts[0].tile_id, 0);
        assert_eq!(world.tile_counts[0].count, 4);
    }

    #[test]
    fn destroy_clears_background_when_foreground_is_already_empty() {
        let mut state = test_state(2, 2, vec![0, 0, 0, 0], vec![7, 0, 0, 0]);

        let changed = apply_destroy_block_change(&mut state, 0, 0);

        assert!(changed);
        assert_eq!(state.world_foreground_tiles[0], 0);
        assert_eq!(state.world_background_tiles[0], 0);
    }

    #[test]
    fn destroy_prefers_foreground_before_background() {
        let mut state = test_state(2, 2, vec![9, 0, 0, 0], vec![7, 0, 0, 0]);

        let changed = apply_destroy_block_change(&mut state, 0, 0);

        assert!(changed);
        assert_eq!(state.world_foreground_tiles[0], 0);
        assert_eq!(state.world_background_tiles[0], 7);
        let world = state.world.unwrap();
        assert_eq!(world.tile_counts.len(), 1);
        assert_eq!(world.tile_counts[0].tile_id, 0);
        assert_eq!(world.tile_counts[0].count, 4);
    }

    #[test]
    fn growing_tile_reports_ready_only_after_growth_end_time() {
        let mut state = test_state(2, 2, vec![0, 0, 0, 0], vec![0, 0, 0, 0]);
        state.growing_tiles.insert(
            (1, 1),
            GrowingTileState {
                block_id: 2,
                growth_end_time: 1_000,
                growth_duration_secs: 31,
                mixed: false,
                harvest_seeds: 0,
                harvest_blocks: 5,
                harvest_gems: 0,
                harvest_extra_blocks: 0,
            },
        );

        assert!(!is_tile_ready_to_harvest_at(&state, 1, 1, 999).unwrap());
        assert!(is_tile_ready_to_harvest_at(&state, 1, 1, 1_000).unwrap());
        assert!(is_tile_ready_to_harvest_at(&state, 1, 1, 1_001).unwrap());
    }

    #[test]
    fn growing_tile_query_is_false_when_tile_is_not_tracked() {
        let state = test_state(2, 2, vec![0, 0, 0, 0], vec![0, 0, 0, 0]);

        assert!(!is_tile_ready_to_harvest_at(&state, 1, 1, 1_000).unwrap());
    }

    #[test]
    fn update_player_position_from_message_sets_world_and_map_coordinates() {
        let mut position = PlayerPosition {
            map_x: None,
            map_y: None,
            world_x: None,
            world_y: None,
        };

        let changed = update_player_position_from_message(&doc! { "x": 20.8, "y": 14.88 }, &mut position);
        let (expected_map_x, expected_map_y) = protocol::world_to_map(20.8, 14.88);

        assert!(changed);
        assert_eq!(position.world_x, Some(20.8));
        assert_eq!(position.world_y, Some(14.88));
        assert_eq!(position.map_x, Some(expected_map_x));
        assert_eq!(position.map_y, Some(expected_map_y));
    }

    #[test]
    fn update_player_position_from_message_reports_no_change_for_same_coordinates() {
        let (map_x, map_y) = protocol::world_to_map(20.8, 14.88);
        let mut position = PlayerPosition {
            map_x: Some(map_x),
            map_y: Some(map_y),
            world_x: Some(20.8),
            world_y: Some(14.88),
        };

        let changed = update_player_position_from_message(&doc! { "x": 20.8, "y": 14.88 }, &mut position);

        assert!(!changed);
    }

    #[tokio::test]
    async fn player_leave_removes_tracked_remote_player() {
        let session = BotSession::new(
            "test-session".to_string(),
            crate::models::AuthInput::AndroidDevice {
                device_id: Some("device".to_string()),
            },
            Logger::new(Arc::new(EventHub::new(16))),
        )
        .await;

        {
            let mut state = session.state.write().await;
            state.user_id = Some("local-user".to_string());
            state.other_players.insert(
                "remote-user".to_string(),
                PlayerPosition {
                    map_x: Some(1.0),
                    map_y: Some(2.0),
                    world_x: Some(10.0),
                    world_y: Some(20.0),
                },
            );
        }

        session
            .remove_other_player(&doc! { "U": "remote-user" })
            .await;

        assert!(!session
            .state
            .read()
            .await
            .other_players
            .contains_key("remote-user"));
    }

    #[test]
    fn scheduler_menu_idle_emits_empty_batch_when_idle() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuIdle);
        scheduler.st_due = false;

        let batch = scheduler.take_slot_batch().unwrap();

        assert!(batch.is_empty());
    }

    #[test]
    fn scheduler_mergeable_packets_replace_menu_empty_batch() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuIdle);
        scheduler.st_due = false;
        scheduler.enqueue_packets(
            vec![doc! { "ID": "HB" }],
            SendMode::Mergeable,
            QueuePriority::AfterGenerated,
        );

        let batch = scheduler.take_slot_batch().unwrap();

        assert_eq!(batch_ids(&batch), vec!["HB".to_string()]);
    }

    #[test]
    fn scheduler_world_batch_keeps_pre_and_post_generated_order() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::WorldIdle);
        scheduler.st_due = false;
        scheduler.update_movement(12.8, 9.44, true, movement_consts::ANIM_WALK, movement_consts::DIR_RIGHT);
        scheduler.enqueue_packets(
            vec![protocol::make_map_point(41, 30)],
            SendMode::Mergeable,
            QueuePriority::BeforeGenerated,
        );
        scheduler.enqueue_packets(
            vec![doc! { "ID": "HB" }],
            SendMode::Mergeable,
            QueuePriority::AfterGenerated,
        );

        let batch = scheduler.take_slot_batch().unwrap();

        assert_eq!(
            batch_ids(&batch),
            vec![
                ids::PACKET_ID_MAP_POINT.to_string(),
                ids::PACKET_ID_MOVEMENT.to_string(),
                "HB".to_string(),
            ]
        );
    }

    #[test]
    fn scheduler_world_idle_emits_empty_movement_packet() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::WorldIdle);
        scheduler.st_due = false;

        let batch = scheduler.take_slot_batch().unwrap();

        assert_eq!(batch_ids(&batch), vec![ids::PACKET_ID_MOVEMENT.to_string()]);
    }

    #[test]
    fn scheduler_exclusive_batch_stays_isolated() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuIdle);
        scheduler.st_due = false;
        scheduler.enqueue_packets(
            vec![doc! { "ID": "A" }],
            SendMode::Mergeable,
            QueuePriority::AfterGenerated,
        );
        scheduler.enqueue_packets(
            vec![doc! { "ID": "X" }, doc! { "ID": "Y" }],
            SendMode::ExclusiveBatch,
            QueuePriority::AfterGenerated,
        );

        let first = scheduler.take_slot_batch().unwrap();
        let second = scheduler.take_slot_batch().unwrap();

        assert_eq!(batch_ids(&first), vec!["A".to_string()]);
        assert_eq!(batch_ids(&second), vec!["X".to_string(), "Y".to_string()]);
    }

    #[test]
    fn scheduler_immediate_exclusive_preempts_slot_batches() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuIdle);
        scheduler.st_due = false;
        scheduler.enqueue_packets(
            vec![doc! { "ID": "queued" }],
            SendMode::Mergeable,
            QueuePriority::AfterGenerated,
        );
        scheduler.enqueue_packets(
            vec![doc! { "ID": "now" }],
            SendMode::ImmediateExclusive,
            QueuePriority::AfterGenerated,
        );

        let immediate = scheduler.take_immediate_batch().unwrap();
        let later = scheduler.take_slot_batch().unwrap();

        assert_eq!(batch_ids(&immediate), vec!["now".to_string()]);
        assert_eq!(batch_ids(&later), vec!["queued".to_string()]);
    }

    #[test]
    fn scheduler_menu_keepalive_and_st_share_the_same_slot() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuIdle);
        scheduler.menu_keepalive_due = true;
        scheduler.st_due = true;

        let batch = scheduler.take_slot_batch().unwrap();

        assert_eq!(
            batch_ids(&batch),
            vec![
                ids::PACKET_ID_KEEPALIVE.to_string(),
                ids::PACKET_ID_ST.to_string(),
            ]
        );
    }

    #[test]
    fn scheduler_st_response_keeps_burst_active_until_sample_window_is_full() {
        let mut scheduler = SchedulerState::new();
        scheduler.set_phase(SchedulerPhase::MenuStBurst);
        scheduler.st_sync.sample_count = timing::ST_SAMPLE_COUNT - 1;
        scheduler.st_sync.last_sent_at = Some(std::time::Instant::now() - Duration::from_millis(25));
        scheduler.st_due = false;

        let next = scheduler.handle_st_response();

        assert!(next.is_some());
        assert_eq!(scheduler.phase, SchedulerPhase::MenuIdle);
        assert!(!scheduler.st_due);
    }

    #[test]
    fn drop_target_uses_facing_direction_instead_of_y_plus_one() {
        let mut state = test_state(10, 10, vec![0; 100], vec![0; 100]);
        state.player_position.map_x = Some(40.0);
        state.player_position.map_y = Some(30.0);
        state.current_direction = movement_consts::DIR_RIGHT;

        let target = drop_target_tile(&state).unwrap();

        assert_eq!(target, (41, 30));
    }
}







