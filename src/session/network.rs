//! Network plumbing: read loop, scheduler loop, batched senders, and the
//! kick-error / redirect handling that runs inside the read loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use bson::Document;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, watch, RwLock};
use tokio::time::{interval_at, sleep_until, MissedTickBehavior};

use crate::constants::timing;
use crate::logging::{Direction, Logger};
use crate::models::PlayerPosition;
use crate::protocol;

use super::state::{
    ControllerEvent, OutboundHandle, QueuePriority,
    SchedulerCommand, SchedulerPhase, SchedulerState, SendMode, SessionState,
};

pub(super) fn update_player_position_from_message(message: &Document, position: &mut PlayerPosition) -> bool {
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

pub(super) async fn write_logged_batch(
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

pub(super) fn stop_background_worker(stop_tx: &mut Option<watch::Sender<bool>>) {
    if let Some(tx) = stop_tx.take() {
        let _ = tx.send(true);
    }
}

pub(super) fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(AtomicOrdering::Relaxed) {
        Err("lua script stopped".to_string())
    } else {
        Ok(())
    }
}

pub(super) async fn read_loop(
    mut reader: OwnedReadHalf,
    controller_tx: mpsc::Sender<ControllerEvent>,
    logger: Logger,
    session_id: String,
    runtime_id: u64,
) {
    loop {
        match protocol::read_packet(&mut reader).await {
            Ok(packet) => {
                logger.tcp_trace(Direction::Incoming, "tcp", Some(&session_id), || {
                    protocol::log_packet(&packet)
                });
                let messages = protocol::extract_messages(&packet);
                for message in messages {
                    if controller_tx
                        .send(ControllerEvent::Inbound(runtime_id, message))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = controller_tx
                    .send(ControllerEvent::ReadLoopStopped(runtime_id, error))
                    .await;
                return;
            }
        }
    }
}

pub(super) async fn scheduler_loop(
    mut writer: OwnedWriteHalf,
    mut outbound_rx: mpsc::Receiver<SchedulerCommand>,
    mut stop_rx: watch::Receiver<bool>,
    logger: Logger,
    session_id: String,
    state: Arc<RwLock<SessionState>>,
) {
    let mut scheduler = SchedulerState::new();
    let start = tokio::time::Instant::now();
    let mut slot_tick = interval_at(start + timing::send_slot_interval(), timing::send_slot_interval());
    let mut keepalive_tick = interval_at(
        start + timing::menu_keepalive_interval(),
        timing::menu_keepalive_interval(),
    );
    slot_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    keepalive_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let far_future = start + Duration::from_secs(60 * 60 * 24 * 365);
    let mut st_sleep = Box::pin(sleep_until(far_future));

    loop {
        tokio::select! {
            biased;

            _ = async {}, if scheduler.has_immediate_batch() => {
                if let Some(batch) = scheduler.take_immediate_batch() {
                    if write_logged_batch(&mut writer, &logger, &session_id, &batch).await.is_err() {
                        return;
                    }
                    let sent_st = scheduler.on_batch_sent(&batch);
                    if sent_st && scheduler.phase == SchedulerPhase::MenuIdle {
                        scheduler.phase = SchedulerPhase::MenuStBurst;
                    }
                }
            }

            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    return;
                }
            }

            _ = &mut st_sleep => {
                scheduler.mark_st_due();
                st_sleep.as_mut().reset(far_future);
            }

            _ = keepalive_tick.tick() => {
                scheduler.mark_menu_keepalive_due();
            }

            _ = slot_tick.tick() => {
                if let Some(batch) = scheduler.take_slot_batch() {
                    if write_logged_batch(&mut writer, &logger, &session_id, &batch).await.is_err() {
                        return;
                    }
                    let sent_st = scheduler.on_batch_sent(&batch);
                    if sent_st && scheduler.phase == SchedulerPhase::MenuIdle {
                        scheduler.phase = SchedulerPhase::MenuStBurst;
                    }
                }
            }

            Some(cmd) = outbound_rx.recv() => {
                match cmd {
                    SchedulerCommand::EnqueuePackets { docs, mode, priority } => {
                        scheduler.enqueue_packets(docs, mode, priority);
                    }
                    SchedulerCommand::UpdateMovement { world_x, world_y, is_moving, anim, direction } => {
                        scheduler.update_movement(world_x, world_y, is_moving, anim, direction);
                    }
                    SchedulerCommand::SetPhase { phase } => {
                        scheduler.set_phase(phase);
                    }
                    SchedulerCommand::StResponseReceived => {
                        if let Some(rtt_ms) = scheduler.st_sync.last_sent_at.map(|sent_at| sent_at.elapsed().as_millis() as u32) {
                            state.write().await.ping_ms = Some(rtt_ms);
                        }
                        if let Some(next) = scheduler.handle_st_response() {
                            let deadline = tokio::time::Instant::now() + next;
                            st_sleep.as_mut().reset(deadline);
                        }
                    }
                    SchedulerCommand::Shutdown => return,
                }
            }

            else => return,
        }
    }
}

pub(super) async fn enqueue_packets(
    outbound_tx: &OutboundHandle,
    docs: Vec<Document>,
    mode: SendMode,
    priority: QueuePriority,
) -> Result<(), String> {
    if docs.is_empty() {
        return Ok(());
    }
    outbound_tx
        .send(SchedulerCommand::EnqueuePackets {
            docs,
            mode,
            priority,
        })
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn send_doc(outbound_tx: &OutboundHandle, doc: Document) -> Result<(), String> {
    enqueue_packets(
        outbound_tx,
        vec![doc],
        SendMode::Mergeable,
        QueuePriority::AfterGenerated,
    )
    .await
}

pub(super) async fn send_doc_before_generated(
    outbound_tx: &OutboundHandle,
    doc: Document,
) -> Result<(), String> {
    enqueue_packets(
        outbound_tx,
        vec![doc],
        SendMode::Mergeable,
        QueuePriority::BeforeGenerated,
    )
    .await
}

pub(super) async fn send_scheduler_cmd(outbound_tx: &OutboundHandle, cmd: SchedulerCommand) -> Result<(), String> {
    outbound_tx.send(cmd).await.map_err(|error| error.to_string())
}

pub(super) async fn send_docs(outbound_tx: &OutboundHandle, docs: Vec<Document>) -> Result<(), String> {
    enqueue_packets(
        outbound_tx,
        docs,
        SendMode::Mergeable,
        QueuePriority::AfterGenerated,
    )
    .await
}

pub(super) async fn send_docs_exclusive(
    outbound_tx: &OutboundHandle,
    docs: Vec<Document>,
) -> Result<(), String> {
    enqueue_packets(
        outbound_tx,
        docs,
        SendMode::ExclusiveBatch,
        QueuePriority::AfterGenerated,
    )
    .await
}

pub(super) async fn send_docs_immediate(outbound_tx: &OutboundHandle, docs: Vec<Document>) -> Result<(), String> {
    enqueue_packets(
        outbound_tx,
        docs,
        SendMode::ImmediateExclusive,
        QueuePriority::AfterGenerated,
    )
    .await
}

