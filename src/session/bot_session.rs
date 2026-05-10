//! Public BotSession handle and its 90+ method API surface. Entry point
//! used by main.rs, the HTTP layer, and Lua scripts.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use bson::{Document, doc};
use parking_lot::Mutex as PlMutex;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::{mpsc, watch, RwLock};
use tokio::time::{interval, sleep, MissedTickBehavior};

use crate::auth;
use crate::constants::{
    fishing as fishing_consts, movement as movement_consts, network as network_consts,
    protocol as ids, timing, tutorial as tutorial_consts,
};
use crate::logging::{Direction, Logger};
use crate::lua_runtime::{self, LuaScriptHandle};
use crate::models::{
    AiEnemySnapshot, AuthInput, BotTarget, FishingStartRequest, InventoryItem,
    LuaCollectableSnapshot, LuaGrowingTileSnapshot, LuaScriptStartRequest, LuaTileSnapshot,
    LuaWorldObjectsSnapshot, LuaWorldSnapshot, LuaWorldSpawnSnapshot, LuaWorldTilesSnapshot,
    MinimapSnapshot, PlayerPosition, RemotePlayerSnapshot, ServerEvent, SessionSnapshot,
    SessionStatus, TileCount, WorldSnapshot,
};
use crate::net;
use crate::pathfinding::astar;
use crate::protocol;
use crate::world;

use super::automine::{self, automine_loop};
use super::autonether;
use super::fishing::{fishing_loop, stop_fishing_game};
use super::movement::{
    current_facing_direction, drop_target_tile, fallback_straight_line_path,
    is_walkable_map_position, manual_move, manual_place, manual_punch, move_to_map, movement_doc,
    next_manual_step, planned_path, punch_target_from_offset, send_world_chat,
    set_local_map_position, set_local_world_position, spam_loop, walk_predefined_path,
    walk_to_map, walk_to_map_cancellable, wait_for_map_position,
};
use super::network::{
    enqueue_packets, ensure_not_cancelled, read_loop, scheduler_loop, send_doc,
    send_doc_before_generated, send_docs, send_docs_exclusive, send_docs_immediate,
    send_scheduler_cmd, stop_background_worker, update_player_position_from_message,
};
use super::state::{
    ActiveRuntime, AiEnemyState, CollectCooldowns, CollectableState, ControllerEvent,
    FishingAutomationState, FishingPhase, FishingTarget, GrowingTileState, InventoryEntry,
    NamedInventoryEntry, OutboundHandle, PendingBatch, QueuePriority, SchedulerCommand,
    SchedulerPhase, SchedulerState, SendMode, SessionCommand, SessionState, StSyncState,
};
use super::fishing::{
    consume_fishing_bait, find_fishing_map_point, initialize_fishing_gauge, rod_family_name,
    service_fishing_simulation,
};
use super::tutorial::{
    ensure_world, ensure_world_cancellable, run_tutorial_script, wait_for_tutorial_phase4_ack,
    wait_for_tutorial_spawn_pod_confirmation, wait_for_tutorial_world_ready_to_enter,
};
use super::world_data::{
    apply_destroy_block_change, apply_foreground_block_change, block_inventory_type_for,
    block_name_for, block_names, block_type_name_for, block_types,
    collect_all_visible_collectables, collect_all_visible_collectables_cancellable,
    decode_inventory, find_inventory_bait, inventory_key_for, is_tile_ready_to_harvest_at,
    normalize_block_name, publish_state_snapshot, summarize_tile_counts, tile_index,
    tile_snapshot_at, wait_for_collectables, POSITION_PUBLISH_THROTTLE,
};


#[derive(Debug)]
pub struct BotSession {
    id: String,
    auth: AuthInput,
    state: Arc<RwLock<SessionState>>,
    controller_tx: mpsc::Sender<ControllerEvent>,
    logger: Logger,
    last_position_publish_at: PlMutex<Option<Instant>>,
}

impl BotSession {
    pub(crate) fn id_string(&self) -> String {
        self.id.clone()
    }

    pub(super) async fn new(id: String, auth: AuthInput, logger: Logger, proxy: Option<String>) -> Arc<Self> {
        let (controller_tx, controller_rx) = mpsc::channel(512);
        let device_id = auth.device_id();
        let state = Arc::new(RwLock::new(SessionState {
            status: SessionStatus::Idle,
            device_id: if device_id.is_empty() {
                network_consts::DEFAULT_DEVICE_ID.to_string()
            } else {
                device_id
            },
            current_host: net::default_host(),
            current_port: net::default_port(),
            proxy,
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
            worn_items: std::collections::HashSet::new(),
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
            automine_speed: 1.0,
        }));

        let session = Arc::new(Self {
            id,
            auth,
            state,
            controller_tx,
            logger,
            last_position_publish_at: PlMutex::new(None),
        });

        let cloned = session.clone();
        tokio::spawn(async move {
            cloned.run_controller(controller_rx).await;
        });

        session
    }

    pub async fn snapshot(&self) -> SessionSnapshot {
        let state = self.state.read().await;
        SessionSnapshot {
            id: self.id.clone(),
            status: state.status.clone(),
            device_id: state.device_id.clone(),
            current_host: state.current_host.clone(),
            current_port: state.current_port,
            proxy: state.proxy.clone(),
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
    }

    pub fn clone_arc(self: &Arc<Self>) -> Arc<Self> {
        self.clone()
    }

    pub async fn queue_start_autonether(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StartAutonether).await?;
        Ok("Nether automation starting...".to_string())
    }

    pub async fn queue_stop_autonether(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopAutonether).await?;
        Ok("Nether automation stopping...".to_string())
    }

    pub async fn autonether_status(&self) -> Result<Option<autonether::AutonetherStatusSnapshot>, String> {
        let state = self.state.read().await;
        Ok(Some(state.autonether.snapshot()))
    }

    pub async fn connect(&self) -> Result<(), String> {
        self.send_command(SessionCommand::Connect).await
    }

    pub async fn join_world(&self, world: String, instance: bool) -> Result<(), String> {
        self.send_command(SessionCommand::JoinWorld { world, instance }).await
    }

    pub async fn leave_world(&self) -> Result<(), String> {
        self.send_command(SessionCommand::LeaveWorld).await
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        self.send_command(SessionCommand::Disconnect).await
    }

    pub async fn reconnect(&self) -> Result<(), String> {
        self.send_command(SessionCommand::Disconnect).await?;
        self.send_command(SessionCommand::Connect).await
    }

    pub async fn minimap_snapshot(&self) -> Result<MinimapSnapshot, String> {
        let state = self.state.read().await;
        let world = state
            .world
            .clone()
            .ok_or_else(|| "no world loaded yet".to_string())?;
        if state.world_foreground_tiles.is_empty() {
            return Err("no world tiles loaded yet".to_string());
        }
        Ok(MinimapSnapshot {
            width: world.width,
            height: world.height,
            foreground_tiles: state.world_foreground_tiles.clone(),
            background_tiles: state.world_background_tiles.clone(),
            water_tiles: state.world_water_tiles.clone(),
            wiring_tiles: state.world_wiring_tiles.clone(),
            player_position: state.player_position.clone(),
            other_players: state
                .other_players
                .iter()
                .map(|(user_id, position)| RemotePlayerSnapshot {
                    user_id: user_id.clone(),
                    position: position.clone(),
                })
                .collect(),
            ai_enemies: state
                .ai_enemies
                .values()
                .map(|e| crate::models::AiEnemySnapshot {
                    ai_id: e.ai_id,
                    map_x: e.map_x,
                    map_y: e.map_y,
                    alive: e.alive,
                })
                .collect(),
        })
    }

    pub async fn automate_tutorial(&self) -> Result<String, String> {
        self.send_command(SessionCommand::AutomateTutorial).await?;
        Ok("Tutorial automation queued.".to_string())
    }

    pub async fn is_tile_ready_to_harvest(&self, map_x: i32, map_y: i32) -> Result<bool, String> {
        let state = self.state.read().await;
        is_tile_ready_to_harvest_at(&state, map_x, map_y, protocol::csharp_ticks())
    }

    pub async fn queue_drop_item(
        &self,
        block_id: i32,
        amount: i32,
    ) -> Result<String, String> {
        if amount <= 0 {
            return Err("amount must be greater than 0".to_string());
        }
        self.send_command(SessionCommand::DropItem {
            block_id,
            amount,
        })
        .await?;
        let item_name = block_name_for(block_id as u16)
            .unwrap_or_else(|| format!("item {block_id}"));
        Ok(format!("drop queued: {amount}x {item_name}"))
    }

    pub async fn queue_wear_item(&self, block_id: i32, equip: bool) -> Result<String, String> {
        self.send_command(SessionCommand::WearItem { block_id, equip })
            .await?;
        let action = if equip { "equip" } else { "unequip" };
        Ok(format!("{action} queued for block {block_id}"))
    }

    pub async fn queue_punch(&self, offset_x: i32, offset_y: i32) -> Result<String, String> {
        self.send_command(SessionCommand::Punch { offset_x, offset_y })
            .await?;
        Ok(format!("punch queued at offset ({offset_x}, {offset_y})"))
    }

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

    pub async fn queue_start_autoclear(&self, world: String) -> Result<String, String> {
        self.send_command(SessionCommand::StartAutoClear { world }).await?;
        Ok("autoclear start queued".to_string())
    }

    pub async fn queue_stop_autoclear(&self) -> Result<String, String> {
        self.send_command(SessionCommand::StopAutoClear).await?;
        Ok("autoclear stop queued".to_string())
    }

    pub async fn queue_set_automine_speed(&self, multiplier: f32) -> Result<String, String> {
        let clamped = multiplier.clamp(0.4, 1.6);
        self.send_command(SessionCommand::SetAutomineSpeed { multiplier: clamped }).await?;
        Ok(format!("automine speed set to {clamped:.2}x"))
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
        super::tutorial::ensure_world_cancellable(
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

    pub(crate) async fn inventory(&self) -> Vec<(u16, u32)> {
        self.state
            .read()
            .await
            .inventory
            .iter()
            .map(|entry| (entry.block_id, entry.amount as u32))
            .collect()
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
        let mut autoclear_stop_tx: Option<watch::Sender<bool>> = None;
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
                        stop_background_worker(&mut autoclear_stop_tx);
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
                        stop_background_worker(&mut autoclear_stop_tx);
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
                            let result = super::tutorial::run_tutorial_script(
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
                        let controller_tx = self.controller_tx.clone();
                        tokio::spawn(async move {
                            if let Err(error) = automine::automine_loop(
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                stop_rx,
                                controller_tx,
                            ).await {
                                logger.error("automine", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopAutomine => {
                        stop_background_worker(&mut automine_stop_tx);
                        self.state.write().await.current_target = None;
                    }
                    SessionCommand::StartAutoClear { world } => {
                        stop_background_worker(&mut autoclear_stop_tx);
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before starting autoclear".to_string()).await;
                            continue;
                        };
                        let (stop_tx, stop_rx) = watch::channel(false);
                        autoclear_stop_tx = Some(stop_tx);
                        let outbound_tx = active.outbound_tx.clone();
                        let state = self.state.clone();
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        let controller_tx = self.controller_tx.clone();
                        tokio::spawn(async move {
                            if let Err(error) = crate::session::autoclear::autoclear_loop(
                                world,
                                &session_id,
                                &logger,
                                &state,
                                &outbound_tx,
                                stop_rx,
                                controller_tx,
                            ).await {
                                logger.error("autoclear", Some(&session_id), error);
                            }
                        });
                    }
                    SessionCommand::StopAutoClear => {
                        stop_background_worker(&mut autoclear_stop_tx);
                        self.state.write().await.current_target = None;
                    }
                    SessionCommand::SetAutomineSpeed { multiplier } => {
                        let clamped = multiplier.clamp(0.4, 1.6);
                        self.state.write().await.automine_speed = clamped;
                        self.logger.info("automine", Some(&self.id), format!("speed multiplier updated to {clamped:.2}x"));
                    }
                    SessionCommand::StartAutonether => {
                        stop_background_worker(&mut autonether_stop_tx);
                        let Some(active) = &runtime else {
                            self.set_error("connect the session before starting autonether".to_string()).await;
                            continue;
                        };
                        let (stop_tx, stop_rx) = watch::channel(false);
                        autonether_stop_tx = Some(stop_tx);
                        let logger = self.logger.clone();
                        let session_id = self.id.clone();
                        let session = self.clone_arc();
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
        let proxy = self.state.read().await.proxy.clone();
        let resolved =
            auth::resolve_auth(self.auth.clone(), self.logger.clone(), self.id.clone(), proxy).await?;
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

        let runtime_id = super::RUNTIME_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
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
            ids::PACKET_ID_WEAR_ITEM => {
                self.apply_wear_update(&message, true).await;
            }
            ids::PACKET_ID_UNWEAR_ITEM => {
                self.apply_wear_update(&message, false).await;
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
                        let (world, is_instance, retry_count) = {
                            let mut state = self.state.write().await;
                            state.serverfull_retries = state.serverfull_retries.saturating_add(1);
                            let world = state.pending_world.clone().or_else(|| state.current_world.clone());
                            (world, state.pending_world_is_instance, state.serverfull_retries)
                        };

                        const SERVERFULL_RETRY_LIMIT: u32 = 60;
                        if retry_count <= SERVERFULL_RETRY_LIMIT {
                            if let Some(world) = world {
                                self.logger.info("session", Some(&self.id), 
                                    format!("JR: 8 (Server Full) detected. Manually retrying shard #{retry_count} for {world} (is_instance={is_instance})"));
                                let _ = send_doc(
                                    &runtime.outbound_tx,
                                    protocol::make_join_world_retry(&world, retry_count as i32, is_instance),
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
                        && world_name.as_deref() == Some(tutorial_consts::TUTORIAL_WORLD)
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
                        && state.current_world.as_deref() == Some(tutorial_consts::TUTORIAL_WORLD)
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
                const SERVERFULL_RETRY_LIMIT: u32 = 60;

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
                            format!("OoIP ServerFull retry #{retry_count} for {world} (is_instance={is_instance})"));
                        let _ = send_doc(
                            &runtime.outbound_tx,
                            protocol::make_join_world_retry(&world, retry_count as i32, is_instance),
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

        if picked != tutorial_consts::POST_CHARACTER_POD_CONFIRMATION {
            return;
        }

        let should_reflect = {
            let state = self.state.read().await;
            state.current_world.as_deref() == Some(tutorial_consts::TUTORIAL_WORLD)
                || state.pending_world.as_deref() == Some(tutorial_consts::TUTORIAL_WORLD)
        };
        if !should_reflect {
            return;
        }

        self.logger.state(
            Some(&self.id),
            format!(
                "server confirmed tutorial pod APu={picked:?}, bot will walk to map=({}, {})",
                tutorial_consts::SPAWN_POT_MAP_X,
                tutorial_consts::SPAWN_POT_MAP_Y,
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
            // Remove from worn_items if the item was equipped (e.g. pickaxe broke)
            if state.worn_items.remove(&block_id) {
                self.logger.warn("session", Some(&self.id), 
                    format!("Item {} broke/removed — auto-cleared from worn_items", block_id));
            }
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

    async fn apply_wear_update(&self, message: &Document, is_wearing: bool) {
        let (my_uid, my_username) = {
            let st = self.state.read().await;
            (st.user_id.clone(), st.username.clone())
        };

        let target_uid = message.get_str("U").ok().map(|s| s.to_string());
        let target_name = message.get_str("UN").ok().map(|s| s.to_string());

        let is_me = (my_uid.is_some() && my_uid == target_uid) 
                 || (my_username.is_some() && my_username == target_name);

        if is_me {
            if let Ok(block_id) = message.get_i32("hBlock") {
                let mut st = self.state.write().await;
                if is_wearing {
                    st.worn_items.insert(block_id as u16);
                } else {
                    st.worn_items.remove(&(block_id as u16));
                }
                self.logger.info("session", Some(&self.id), 
                    format!("Worn state updated: {} block_id={}", if is_wearing { "EQUIPPED" } else { "REMOVED" }, block_id));
            }
        }
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
        // Collectables in nCo/nWC packets are already in map-tile units.
        // Do NOT call world_to_map here.
        let (mx, my) = (pos_x, pos_y);

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
