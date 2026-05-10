//! Session-internal state types. Cross-module visibility is `pub(super)` so
//! sibling files (network, automine, fishing, etc.) can share them; the
//! façade in `mod.rs` re-exports anything that crosses the crate boundary.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use bson::Document;
use tokio::sync::{mpsc, watch};

use crate::constants::{movement, protocol as ids, timing};
use crate::models::{BotTarget, PlayerPosition, SessionStatus, WorldSnapshot};
use crate::protocol;

#[derive(Debug)]
pub(super) struct ActiveRuntime {
    pub(super) id: u64,
    pub(super) outbound_tx: OutboundHandle,
    pub(super) stop_tx: watch::Sender<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendMode {
    Mergeable,
    ExclusiveBatch,
    ImmediateExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueuePriority {
    BeforeGenerated,
    AfterGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SchedulerPhase {
    Disconnected,
    MenuIdle,
    MenuStBurst,
    WorldIdle,
    WorldMoving,
}

#[derive(Debug)]
pub(super) enum SchedulerCommand {
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

pub(super) type OutboundHandle = mpsc::Sender<SchedulerCommand>;

#[derive(Debug, Clone)]
pub(super) struct MovementTickState {
    pub(super) in_world: bool,
    pub(super) world_x: f64,
    pub(super) world_y: f64,
    pub(super) is_moving: bool,
    pub(super) anim: i32,
    pub(super) direction: i32,
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
pub(super) enum PendingBatch {
    Mergeable {
        docs: Vec<Document>,
        priority: QueuePriority,
    },
    Exclusive(Vec<Document>),
}

pub(super) struct StSyncState {
    pub(super) samples: [i32; timing::ST_SAMPLE_COUNT],
    pub(super) sample_count: usize,
    pub(super) success_counter: i32,
    pub(super) interval_secs: i32,
    pub(super) last_sent_at: Option<std::time::Instant>,
}

impl StSyncState {
    pub(super) fn new() -> Self {
        Self {
            samples: [i32::MAX; timing::ST_SAMPLE_COUNT],
            sample_count: 0,
            success_counter: 0,
            interval_secs: timing::ST_INTERVAL_INIT_SECS,
            last_sent_at: None,
        }
    }

    pub(super) fn record_sample(&mut self, rtt_ms: i32) {
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

pub(super) struct SchedulerState {
    pub(super) phase: SchedulerPhase,
    pub(super) movement: MovementTickState,
    pub(super) st_sync: StSyncState,
    pub(super) pending: VecDeque<PendingBatch>,
    pub(super) immediate: VecDeque<Vec<Document>>,
    pub(super) menu_keepalive_due: bool,
    pub(super) st_due: bool,
}

impl SchedulerState {
    pub(super) fn new() -> Self {
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

    pub(super) fn has_immediate_batch(&self) -> bool {
        !self.immediate.is_empty()
    }

    pub(super) fn is_menu_phase(&self) -> bool {
        matches!(
            self.phase,
            SchedulerPhase::MenuIdle | SchedulerPhase::MenuStBurst
        )
    }

    pub(super) fn enqueue_packets(&mut self, docs: Vec<Document>, mode: SendMode, priority: QueuePriority) {
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

    pub(super) fn update_movement(
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

    pub(super) fn set_phase(&mut self, phase: SchedulerPhase) {
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

    pub(super) fn mark_menu_keepalive_due(&mut self) {
        if self.is_menu_phase() {
            self.menu_keepalive_due = true;
        }
    }

    pub(super) fn mark_st_due(&mut self) {
        self.st_due = true;
    }

    pub(super) fn take_immediate_batch(&mut self) -> Option<Vec<Document>> {
        self.immediate.pop_front()
    }

    pub(super) fn take_slot_batch(&mut self) -> Option<Vec<Document>> {
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
                    false,
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

    pub(super) fn on_batch_sent(&mut self, batch: &[Document]) -> bool {
        let sent_st = batch.iter().any(|doc| packet_id(doc) == ids::PACKET_ID_ST);
        if sent_st {
            self.st_sync.last_sent_at = Some(std::time::Instant::now());
        }
        sent_st
    }

    pub(super) fn handle_st_response(&mut self) -> Option<Duration> {
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

pub(super) fn packet_id(doc: &Document) -> &str {
    doc.get_str("ID").unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FishingPhase {
    Idle,
    WaitingForHook,
    HookPrompted,
    GaugeActive,
    CleanupPending,
    Completed,
}

#[derive(Debug, Clone)]
pub(super) struct FishingAutomationState {
    pub(super) active: bool,
    pub(super) phase: FishingPhase,
    pub(super) target_map_x: Option<i32>,
    pub(super) target_map_y: Option<i32>,
    pub(super) bait_name: Option<String>,
    pub(super) last_result: Option<String>,
    pub(super) fish_block: Option<i32>,
    pub(super) rod_block: Option<i32>,
    pub(super) gauge_entered_at: Option<Instant>,
    pub(super) hook_sent: bool,
    pub(super) land_sent: bool,
    pub(super) cleanup_pending: bool,
    pub(super) sim_last_at: Option<Instant>,
    pub(super) sim_fish_position: f64,
    pub(super) sim_target_position: f64,
    pub(super) sim_progress: f64,
    pub(super) sim_overlap_threshold: f64,
    pub(super) sim_fill_rate: f64,
    pub(super) sim_target_speed: f64,
    pub(super) sim_fish_move_speed: f64,
    pub(super) sim_run_frequency: f64,
    pub(super) sim_pull_strength: f64,
    pub(super) sim_min_land_delay: f64,
    pub(super) sim_phase: f64,
    pub(super) sim_overlap: bool,
    pub(super) sim_ready_since: Option<Instant>,
    pub(super) sim_difficulty_meter: f64,
    pub(super) sim_size_multiplier: f64,
    pub(super) sim_drag_extra: f64,
    pub(super) sim_run_active: bool,
    pub(super) sim_run_until: Option<Instant>,
    pub(super) sim_force_land_after: Option<Instant>,
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
pub(super) struct CollectCooldowns {
    pub(super) cooldowns: HashMap<i32, Instant>,
}

impl CollectCooldowns {
    pub(super) const COOLDOWN: Duration = Duration::from_secs(1); // 1s per item (reduced from 3s)

    pub(super) fn can_collect(&self, id: i32) -> bool {
        match self.cooldowns.get(&id) {
            Some(&last) => last.elapsed() >= Self::COOLDOWN,
            None => true,
        }
    }

    pub(super) fn is_on_cooldown(&self, id: i32) -> bool {
        !self.can_collect(id)
    }

    pub(super) fn mark_collected(&mut self, id: i32) {
        self.cooldowns.insert(id, Instant::now());
        // Cleanup old entries (older than 30s)
        self.cooldowns
            .retain(|_, last| last.elapsed() < Duration::from_secs(30));
    }
}

#[derive(Debug)]
pub(super) struct SessionState {
    pub(super) status: SessionStatus,
    pub(super) device_id: String,
    pub(super) current_host: String,
    pub(super) current_port: u16,
    pub(super) proxy: Option<String>,
    pub(super) current_world: Option<String>,
    pub(super) pending_world: Option<String>,
    pub(super) pending_world_is_instance: bool,
    /// Counter for OoIP "ServerFull" retries. The game server uses the `Amt`
    /// field on the next TTjW to mean "try shard #N instead of the full one".
    /// Reset on successful world entry (GET_WORLD_CONTENT) and when the user
    /// issues a fresh JoinWorld command.
    pub(super) serverfull_retries: u32,
    /// Last "interesting" outbound action description, kept so we can correlate
    /// KErr kicks with the bot action that triggered them. Set on every move /
    /// hit / HAI by the automine loop.
    pub(super) last_action_hint: Option<String>,
    pub(super) last_action_at: Option<Instant>,
    pub(super) username: Option<String>,
    pub(super) user_id: Option<String>,
    pub(super) world: Option<WorldSnapshot>,
    pub(super) world_foreground_tiles: Vec<u16>,
    pub(super) world_background_tiles: Vec<u16>,
    pub(super) world_water_tiles: Vec<u16>,
    pub(super) world_wiring_tiles: Vec<u16>,
    pub(super) current_outbound_tx: Option<OutboundHandle>,
    pub(super) growing_tiles: HashMap<(i32, i32), GrowingTileState>,
    pub(super) player_position: PlayerPosition,
    pub(super) other_players: HashMap<String, PlayerPosition>,
    pub(super) ai_enemies: HashMap<i32, AiEnemyState>,
    pub(super) inventory: Vec<InventoryEntry>,
    pub(super) worn_items: std::collections::HashSet<u16>,
    pub(super) collectables: HashMap<i32, CollectableState>,
    pub(super) current_direction: i32,
    pub(super) last_error: Option<String>,
    pub(super) awaiting_ready: bool,
    pub(super) tutorial_spawn_pod_confirmed: bool,
    pub(super) tutorial_automation_running: bool,
    pub(super) autonether: super::autonether::AutonetherState,
    /// Tiles we've hit and are waiting for a DB (Destroy Block) confirmation.
    /// Maps (x, y) to the Instant of the LAST hit.
    pub(super) pending_hits: HashMap<(i32, i32), Instant>,
    pub(super) tutorial_phase4_acknowledged: bool,
    pub(super) fishing: FishingAutomationState,
    pub(super) ping_ms: Option<u32>,
    pub(super) collect_cooldowns: CollectCooldowns,
    pub(super) rate_limit_until: Option<Instant>,
    pub(super) current_target: Option<BotTarget>,
    pub(super) world_items: Vec<crate::world::DecodedWorldItem>,
    /// Automine speed multiplier. 1.0 = safe default (~350ms/tile),
    /// lower values = slower (safer), higher values = faster (riskier).
    /// Clamped to [0.4, 1.6] in the loop. Default: 1.0.
    pub(super) automine_speed: f32,
}

#[derive(Debug)]
pub(super) enum SessionCommand {
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
    StartAutonether,
    StopAutonether,
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
    SetAutomineSpeed { multiplier: f32 },
    StartAutoClear { world: String },
    StopAutoClear,
}

#[derive(Debug)]
pub(super) enum ControllerEvent {
    Command(SessionCommand),
    Inbound(u64, Document),
    ReadLoopStopped(u64, String),
}

#[derive(Debug, Clone)]
pub(super) struct InventoryEntry {
    pub(super) inventory_key: i32,
    pub(super) block_id: u16,
    pub(super) inventory_type: u16,
    pub(super) amount: u16,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct AiEnemyState {
    pub(super) ai_id: i32,
    pub(super) map_x: i32,
    pub(super) map_y: i32,
    pub(super) last_seen: Instant,
    pub(super) alive: bool,
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
pub(super) struct FishingTarget {
    pub(super) direction: String,
    pub(super) bait_query: String,
    pub(super) map_x: i32,
    pub(super) map_y: i32,
}

#[derive(Debug, Clone)]
pub(super) struct NamedInventoryEntry {
    pub(super) inventory_key: i32,
    pub(super) block_id: u16,
    pub(super) name: String,
}

#[derive(Debug, Clone)]
pub(super) struct GrowingTileState {
    pub(super) block_id: u16,
    pub(super) growth_end_time: i64,
    pub(super) growth_duration_secs: i32,
    pub(super) mixed: bool,
    pub(super) harvest_seeds: i32,
    pub(super) harvest_blocks: i32,
    pub(super) harvest_gems: i32,
    pub(super) harvest_extra_blocks: i32,
}
