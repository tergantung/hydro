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
mod bot_session;
mod fishing;
mod movement;
mod network;
mod state;
mod tutorial;
mod world_data;
pub use bot_session::BotSession;
use fishing::*;
use movement::*;
use network::*;
use state::*;
use tutorial::*;
use world_data::*;

pub(crate) static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static RUNTIME_COUNTER: AtomicU64 = AtomicU64::new(1);


#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Arc<BotSession>>>,
    lua_scripts: Arc<DashMap<String, LuaScriptHandle>>,
    logger: Logger,
}

impl SessionManager {
    pub fn new(logger: Logger) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            lua_scripts: Arc::new(DashMap::new()),
            logger,
        }
    }

    pub async fn create_session(&self, auth: AuthInput) -> Arc<BotSession> {
        let id = format!(
            "session-{}",
            SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let session = BotSession::new(id.clone(), auth, self.logger.clone()).await;
        self.sessions.insert(id, session.clone());
        session
    }

    pub async fn get_session(&self, id: &str) -> Option<Arc<BotSession>> {
        self.sessions.get(id).map(|entry| entry.clone())
    }

    pub async fn list_snapshots(&self) -> Vec<SessionSnapshot> {
        let sessions: Vec<Arc<BotSession>> = self
            .sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let mut items = Vec::with_capacity(sessions.len());
        for session in sessions {
            items.push(session.snapshot().await);
        }
        items
    }

    pub async fn start_lua_script(
        &self,
        session_id: &str,
        source: String,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        let session = self
            .get_session(session_id)
            .await
            .ok_or_else(|| "session not found".to_string())?;
        self.stop_lua_script(session_id).await?;
        let handle = lua_runtime::spawn_script(session, source, self.logger.clone());
        let status = handle.status.read().await.clone();
        self.lua_scripts.insert(session_id.to_string(), handle);
        Ok(status)
    }

    pub async fn stop_lua_script(
        &self,
        session_id: &str,
    ) -> Result<LuaScriptStatusSnapshot, String> {
        if let Some((_, handle)) = self.lua_scripts.remove(session_id) {
            handle.cancel.store(true, AtomicOrdering::Relaxed);
            handle.task.abort();
            Ok(handle.status.read().await.clone())
        } else {
            Ok(lua_runtime::idle_status())
        }
    }

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
}



























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







