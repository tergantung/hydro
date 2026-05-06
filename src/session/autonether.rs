/// Autonether automation module.
/// Converts the logic from nether.lua to Rust.
/// Warps into NETHERWORLD using a Nether Scroll, collects all Nether Keys on
/// the ground, then finds and activates the exit portal (block 1419).
/// Repeats indefinitely until stopped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::time::{interval, MissedTickBehavior};

use crate::logging::Logger;

// Constants from nether.lua
pub const NETHER_SCROLL_ID: u32 = 5016;
pub const NETHER_KEY_ID: u32 = 1420;
pub const NETHER_EXIT_ID: u32 = 1419;
pub const NETHER_WORLD: &str = "NETHERWORLD";

/// Phases of the autonether automation loop
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutonetherPhase {
    Idle,
    EnteringNether,
    CollectingKeys,
    FindingExit,
    Exiting,
}

impl AutonetherPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutonetherPhase::Idle => "idle",
            AutonetherPhase::EnteringNether => "entering_nether",
            AutonetherPhase::CollectingKeys => "collecting_keys",
            AutonetherPhase::FindingExit => "finding_exit",
            AutonetherPhase::Exiting => "exiting",
        }
    }
}

/// State for the autonether automation
#[derive(Debug)]
pub struct AutonetherState {
    pub active: bool,
    pub phase: AutonetherPhase,
    pub last_action: Instant,
    pub action_cooldown: Duration,
    pub cancel_flag: Arc<AtomicBool>,
}

impl AutonetherState {
    pub fn new() -> Self {
        Self {
            active: false,
            phase: AutonetherPhase::Idle,
            last_action: Instant::now(),
            action_cooldown: Duration::from_millis(200),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self) {
        self.active = true;
        self.phase = AutonetherPhase::Idle;
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.last_action = Instant::now() - self.action_cooldown;
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.phase = AutonetherPhase::Idle;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn snapshot(&self) -> AutonetherStatusSnapshot {
        AutonetherStatusSnapshot {
            active: self.active,
            phase: self.phase.as_str().to_string(),
        }
    }
}

impl Default for AutonetherState {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot type for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonetherStatusSnapshot {
    pub active: bool,
    pub phase: String,
}

// ─── Pure helper functions (no session I/O) ──────────────────────────────────

/// Check whether the given world name is NETHERWORLD (case-insensitive).
pub fn is_nether_world(world_name: &str) -> bool {
    world_name.eq_ignore_ascii_case(NETHER_WORLD)
}

/// Count Nether Keys among the collectables snapshot.
pub fn count_nether_keys(collectables: &[crate::models::LuaCollectableSnapshot]) -> usize {
    collectables
        .iter()
        .filter(|c| c.block_type == NETHER_KEY_ID as i32)
        .count()
}

/// Find the exit portal tile in the foreground tile layer.
/// Returns `Some((x, y))` for the first matching tile, or `None`.
pub fn find_exit_portal(
    world_width: u32,
    world_height: u32,
    foreground_tiles: &[u16],
) -> Option<(i32, i32)> {
    for y in 0..world_height as i32 {
        for x in 0..world_width as i32 {
            let idx = (y as u32 * world_width + x as u32) as usize;
            if foreground_tiles.get(idx).copied().unwrap_or(0) == NETHER_EXIT_ID as u16 {
                return Some((x, y));
            }
        }
    }
    None
}

// ─── Async automation loop ────────────────────────────────────────────────────

/// Main autonether loop.
///
/// Mirrors the logic from `nether.lua`:
/// - If in NETHERWORLD: collect all Nether Keys, then find and activate the
///   exit portal (block 1419).
/// - If NOT in NETHERWORLD: check inventory for a Nether Scroll (item 5016),
///   then `warp_instance("NETHERWORLD")`.
///
/// The loop runs until `stop_rx` signals `true` or the session is cancelled.
/// It uses `BotSession`'s public API so it can live in this module without
/// needing direct access to the private `SessionState`.
pub async fn autonether_loop(
    session_id: &str,
    logger: &Logger,
    session: Arc<super::BotSession>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));

    let mut tick = interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    logger.info("autonether", Some(session_id), "Nether automation started.");

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    logger.info("autonether", Some(session_id), "Autonether stopped.");
                    cancel.store(true, Ordering::Relaxed);
                    return Ok(());
                }
            }
            _ = tick.tick() => {
                // Skip if not connected / not in any world
                if !session.is_in_world().await {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }

                let current_world = session.current_world().await;
                let in_nether = current_world
                    .as_deref()
                    .map(is_nether_world)
                    .unwrap_or(false);

                if in_nether {
                    // ── Inside NETHERWORLD ──────────────────────────────────
                    let collectables = session.collectables().await;
                    let nether_keys: Vec<_> = collectables
                        .iter()
                        .filter(|c| c.block_type == NETHER_KEY_ID as i32)
                        .cloned()
                        .collect();

                    if !nether_keys.is_empty() {
                        // Collect all Nether Keys on the ground
                        logger.info(
                            "autonether",
                            Some(session_id),
                            format!("Collecting {} Nether Key(s)...", nether_keys.len()),
                        );
                        for key in &nether_keys {
                            if cancel.load(Ordering::Relaxed) {
                                return Ok(());
                            }
                            // Walk to the key position
                            let key_x = key.pos_x as i32;
                            let key_y = key.pos_y as i32;
                            if let Err(e) = session.walk_to(key_x, key_y, &cancel).await {
                                logger.warn(
                                    "autonether",
                                    Some(session_id),
                                    format!("Failed to walk to key at ({key_x}, {key_y}): {e}"),
                                );
                                continue;
                            }
                            // Collect all visible collectables (including this key)
                            let _ = session.collect(&cancel).await;
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    } else {
                        // No keys left — find and activate the exit portal
                        logger.info(
                            "autonether",
                            Some(session_id),
                            "No keys on ground. Looking for exit portal...",
                        );

                        // Read world tiles to find the exit portal
                        let world_snapshot = session.world().await;
                        match world_snapshot {
                            Ok(world) => {
                                let exit = find_exit_portal(
                                    world.width,
                                    world.height,
                                    &world.tiles.foreground,
                                );
                                match exit {
                                    Some((ex, ey)) => {
                                        logger.info(
                                            "autonether",
                                            Some(session_id),
                                            format!("Found exit portal at ({ex}, {ey}), walking there..."),
                                        );
                                        // Walk to the exit portal
                                        if let Err(e) = session.walk_to(ex, ey, &cancel).await {
                                            logger.warn(
                                                "autonether",
                                                Some(session_id),
                                                format!("walkTo exit failed: {e}"),
                                            );
                                            tokio::time::sleep(Duration::from_millis(2000)).await;
                                            continue;
                                        }
                                        // Send NWE packet to activate the portal
                                        let nwe_packet = bson::doc! {
                                            "ID": "NWE",
                                            "x": ex,
                                            "y": ey,
                                        };
                                        logger.info(
                                            "autonether",
                                            Some(session_id),
                                            format!("Activating nether exit at ({ex}, {ey})"),
                                        );
                                        let _ = session.send_packet(nwe_packet, &cancel).await;
                                        tokio::time::sleep(Duration::from_millis(2000)).await;
                                    }
                                    None => {
                                        logger.warn(
                                            "autonether",
                                            Some(session_id),
                                            "No exit portal found in world tiles.",
                                        );
                                        tokio::time::sleep(Duration::from_millis(2000)).await;
                                    }
                                }
                            }
                            Err(e) => {
                                logger.warn(
                                    "autonether",
                                    Some(session_id),
                                    format!("Could not read world snapshot: {e}"),
                                );
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                            }
                        }
                    }
                } else {
                    // ── Outside NETHERWORLD ─────────────────────────────────
                    let world_name = current_world.as_deref().unwrap_or("");
                    if world_name.is_empty() {
                        // Not in any world yet — wait
                        logger.info(
                            "autonether",
                            Some(session_id),
                            "Not in a world. Waiting...",
                        );
                        tokio::time::sleep(Duration::from_millis(3000)).await;
                        continue;
                    }

                    // Check inventory for a Nether Scroll
                    let scroll_count = session.inventory_count(NETHER_SCROLL_ID as u16).await;
                    if scroll_count > 0 {
                        logger.info(
                            "autonether",
                            Some(session_id),
                            format!("Entering NETHERWORLD from {world_name}..."),
                        );
                        if let Err(e) = session.warp_instance(NETHER_WORLD, &cancel).await {
                            logger.warn(
                                "autonether",
                                Some(session_id),
                                format!("warpInstance failed: {e}"),
                            );
                            tokio::time::sleep(Duration::from_millis(5000)).await;
                        } else {
                            tokio::time::sleep(Duration::from_millis(3000)).await;
                        }
                    } else {
                        logger.info(
                            "autonether",
                            Some(session_id),
                            "No Nether Scroll in inventory. Waiting...",
                        );
                        tokio::time::sleep(Duration::from_millis(5000)).await;
                    }
                }
            }
        }
    }
}

