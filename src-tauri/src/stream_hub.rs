use crate::error::Result;
use crate::memory::types::{MemoryTier, StoreMessageRequest};
use crate::sidecar::{SidecarManager, StreamEvent};
use futures::StreamExt;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventSource {
    Sidecar,
    Memory,
    System,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamHealthStatus {
    Healthy,
    Degraded,
    Recovering,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamEventEnvelopeV2 {
    pub event_id: String,
    pub correlation_id: String,
    pub ts_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub source: StreamEventSource,
    pub payload: StreamEvent,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamRuntimeSnapshot {
    pub running: bool,
    pub health: StreamHealthStatus,
    pub health_reason: Option<String>,
    pub sequence: u64,
    pub last_event_ts_ms: Option<u64>,
    pub last_health_change_ts_ms: u64,
}

#[derive(Debug, Clone)]
struct StreamRuntimeState {
    health: StreamHealthStatus,
    health_reason: Option<String>,
    sequence: u64,
    last_event_ts_ms: Option<u64>,
    last_health_change_ts_ms: u64,
}

struct StreamHubState {
    running: bool,
    stop_tx: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct PendingToolState {
    tool: String,
    message_id: String,
    started: Instant,
    correlation_id: String,
}

pub struct StreamHub {
    state: Mutex<StreamHubState>,
    tx: broadcast::Sender<StreamEventEnvelopeV2>,
    runtime: Arc<tokio::sync::RwLock<StreamRuntimeState>>,
}

impl StreamHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(2048);
        let now = crate::logs::now_ms();
        Self {
            state: Mutex::new(StreamHubState {
                running: false,
                stop_tx: None,
                task: None,
            }),
            tx,
            runtime: Arc::new(tokio::sync::RwLock::new(StreamRuntimeState {
                health: StreamHealthStatus::Recovering,
                health_reason: Some("startup".to_string()),
                sequence: 0,
                last_event_ts_ms: None,
                last_health_change_ts_ms: now,
            })),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEventEnvelopeV2> {
        self.tx.subscribe()
    }

    pub async fn runtime_snapshot(&self) -> StreamRuntimeSnapshot {
        let state = self.state.lock().await;
        let runtime = self.runtime.read().await;
        StreamRuntimeSnapshot {
            running: state.running,
            health: runtime.health.clone(),
            health_reason: runtime.health_reason.clone(),
            sequence: runtime.sequence,
            last_event_ts_ms: runtime.last_event_ts_ms,
            last_health_change_ts_ms: runtime.last_health_change_ts_ms,
        }
    }

    pub async fn start(&self, app: AppHandle, sidecar: Arc<SidecarManager>) -> Result<()> {
        let mut state = self.state.lock().await;
        if state.running {
            return Ok(());
        }

        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let tx = self.tx.clone();
        let runtime = self.runtime.clone();

        let task = tokio::spawn(async move {
            let mut health = StreamHealthStatus::Recovering;
            let mut pending_tools: HashMap<(String, String), PendingToolState> = HashMap::new();
            let mut assistant_content_by_message: HashMap<(String, String), String> =
                HashMap::new();
            let mut assistant_last_message_by_session: HashMap<String, String> = HashMap::new();
            let mut assistant_memory_stored: HashSet<(String, String)> = HashSet::new();
            let mut active_sessions: HashSet<String> = HashSet::new();
            let mut last_progress = Instant::now();
            let idle_timeout = Duration::from_secs(10 * 60);
            let no_event_watchdog = Duration::from_secs(45);
            let mut subscription_generation: u64 = 0;

            emit_stream_health(
                StreamHealthStatus::Recovering,
                Some("stream_hub_started".to_string()),
                &app,
                &tx,
                &runtime,
            )
            .await;

            'outer: loop {
                let stream_res = sidecar.subscribe_events().await;
                let stream = match stream_res {
                    Ok(s) => {
                        subscription_generation = subscription_generation.saturating_add(1);
                        emit_event(
                            tracing::Level::INFO,
                            ProcessKind::Desktop,
                            ObservabilityEvent {
                                event: "stream.subscribe.ok",
                                component: "stream_hub",
                                correlation_id: None,
                                session_id: None,
                                run_id: None,
                                message_id: None,
                                provider_id: None,
                                model_id: None,
                                status: Some("ok"),
                                error_code: None,
                                detail: Some("event stream subscription established"),
                            },
                        );
                        if subscription_generation > 1 {
                            let restart_event = StreamEvent::Raw {
                                event_type: "system.engine_restart_detected".to_string(),
                                data: serde_json::json!({
                                    "subscription_generation": subscription_generation,
                                    "reason": "stream_resubscribed"
                                }),
                            };
                            let restart_env = StreamEventEnvelopeV2 {
                                event_id: Uuid::new_v4().to_string(),
                                correlation_id: format!("engine-restart-{}", Uuid::new_v4()),
                                ts_ms: crate::logs::now_ms(),
                                session_id: None,
                                source: StreamEventSource::System,
                                payload: restart_event.clone(),
                            };
                            let _ = app.emit("sidecar_event", &restart_event);
                            let _ = app.emit("sidecar_event_v2", &restart_env);
                            let _ = tx.send(restart_env);
                            emit_event(
                                tracing::Level::WARN,
                                ProcessKind::Desktop,
                                ObservabilityEvent {
                                    event: "engine.restart.detected",
                                    component: "stream_hub",
                                    correlation_id: None,
                                    session_id: None,
                                    run_id: None,
                                    message_id: None,
                                    provider_id: None,
                                    model_id: None,
                                    status: Some("detected"),
                                    error_code: None,
                                    detail: Some("stream subscription generation advanced"),
                                },
                            );
                            emit_event(
                                tracing::Level::INFO,
                                ProcessKind::Desktop,
                                ObservabilityEvent {
                                    event: "tool.reconcile.start",
                                    component: "stream_hub",
                                    correlation_id: None,
                                    session_id: None,
                                    run_id: None,
                                    message_id: None,
                                    provider_id: None,
                                    model_id: None,
                                    status: Some("running"),
                                    error_code: None,
                                    detail: Some("reconciling running tools on stream resubscribe"),
                                },
                            );
                            match crate::tool_history::mark_running_tools_terminal(
                                &app,
                                None,
                                0,
                                "interrupted: stream reconnected",
                            ) {
                                Ok(reconciled) => {
                                    if reconciled > 0 {
                                        emit_event(
                                            tracing::Level::INFO,
                                            ProcessKind::Desktop,
                                            ObservabilityEvent {
                                                event: "tool.reconcile.end",
                                                component: "stream_hub",
                                                correlation_id: None,
                                                session_id: None,
                                                run_id: None,
                                                message_id: None,
                                                provider_id: None,
                                                model_id: None,
                                                status: Some("ok"),
                                                error_code: None,
                                                detail: Some(
                                                    "reconciled stale running tools on resubscribe",
                                                ),
                                            },
                                        );
                                    }
                                }
                                Err(_) => emit_event(
                                    tracing::Level::WARN,
                                    ProcessKind::Desktop,
                                    ObservabilityEvent {
                                        event: "tool.reconcile.end",
                                        component: "stream_hub",
                                        correlation_id: None,
                                        session_id: None,
                                        run_id: None,
                                        message_id: None,
                                        provider_id: None,
                                        model_id: None,
                                        status: Some("failed"),
                                        error_code: Some("TOOL_RECONCILE_FAILED"),
                                        detail: Some("failed to reconcile tools on resubscribe"),
                                    },
                                ),
                            }
                        }
                        if !matches!(health, StreamHealthStatus::Healthy) {
                            health = StreamHealthStatus::Healthy;
                            emit_stream_health(
                                StreamHealthStatus::Healthy,
                                Some("subscription_established".to_string()),
                                &app,
                                &tx,
                                &runtime,
                            )
                            .await;
                        }
                        s
                    }
                    Err(e) => {
                        tracing::warn!("StreamHub failed to subscribe to sidecar events: {}", e);
                        emit_event(
                            tracing::Level::ERROR,
                            ProcessKind::Desktop,
                            ObservabilityEvent {
                                event: "stream.subscribe.error",
                                component: "stream_hub",
                                correlation_id: None,
                                session_id: None,
                                run_id: None,
                                message_id: None,
                                provider_id: None,
                                model_id: None,
                                status: Some("failed"),
                                error_code: Some("STREAM_SUBSCRIBE_FAILED"),
                                detail: Some("failed to subscribe to /event"),
                            },
                        );
                        if !matches!(health, StreamHealthStatus::Degraded) {
                            health = StreamHealthStatus::Degraded;
                            emit_stream_health(
                                StreamHealthStatus::Degraded,
                                Some("subscribe_failed".to_string()),
                                &app,
                                &tx,
                                &runtime,
                            )
                            .await;
                        }
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(800)) => {},
                            _ = &mut stop_rx => break 'outer,
                        }
                        continue;
                    }
                };

                futures::pin_mut!(stream);
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            if let Some(((session_id, part_id), pending)) = pending_tools
                                .iter()
                                .find(|(_, pending)| {
                                    pending.started.elapsed() > tool_timeout_for(&pending.tool)
                                })
                            {
                                let tool_timeout = tool_timeout_for(&pending.tool);
                                let timeout_event = StreamEvent::SessionError {
                                    session_id: session_id.clone(),
                                    error: format!(
                                        "Tool '{}' (part {}) exceeded timeout of {:?}",
                                        pending.tool,
                                        part_id,
                                        tool_timeout
                                    ),
                                };
                                let timeout_env = StreamEventEnvelopeV2 {
                                    event_id: Uuid::new_v4().to_string(),
                                    correlation_id: format!("tool-timeout-{}", Uuid::new_v4()),
                                    ts_ms: crate::logs::now_ms(),
                                    session_id: Some(session_id.clone()),
                                    source: StreamEventSource::System,
                                    payload: timeout_event.clone(),
                                };
                                let _ = app.emit("sidecar_event", &timeout_event);
                                let _ = app.emit("sidecar_event_v2", &timeout_env);
                                let _ = tx.send(timeout_env);

                                let synthetic_end = StreamEvent::ToolEnd {
                                    session_id: session_id.clone(),
                                    message_id: pending.message_id.clone(),
                                    part_id: part_id.clone(),
                                    tool: pending.tool.clone(),
                                    result: None,
                                    error: Some("failed_timeout".to_string()),
                                };
                                let _ = crate::tool_history::record_stream_event(&app, &synthetic_end);
                                let synthetic_env = StreamEventEnvelopeV2 {
                                    event_id: Uuid::new_v4().to_string(),
                                    correlation_id: pending.correlation_id.clone(),
                                    ts_ms: crate::logs::now_ms(),
                                    session_id: Some(session_id.clone()),
                                    source: StreamEventSource::System,
                                    payload: synthetic_end.clone(),
                                };
                                let _ = app.emit("sidecar_event", &synthetic_end);
                                let _ = app.emit("sidecar_event_v2", &synthetic_env);
                                let _ = tx.send(synthetic_env);
                                emit_event(
                                    tracing::Level::WARN,
                                    ProcessKind::Desktop,
                                    ObservabilityEvent {
                                        event: "tool.synthetic_terminal_emitted",
                                        component: "stream_hub",
                                        correlation_id: Some(&pending.correlation_id),
                                        session_id: Some(session_id),
                                        run_id: None,
                                        message_id: Some(&pending.message_id),
                                        provider_id: None,
                                        model_id: None,
                                        status: Some("ok"),
                                        error_code: Some("TOOL_TIMEOUT"),
                                        detail: Some("synthetic tool terminal emitted after timeout"),
                                    },
                                );

                                pending_tools.remove(&(session_id.clone(), part_id.clone()));
                            }

                            if pending_tools.is_empty() && last_progress.elapsed() > idle_timeout {
                                let idle_raw = StreamEvent::Raw {
                                    event_type: "system.stream_idle_timeout".to_string(),
                                    data: serde_json::json!({
                                        "timeout_ms": idle_timeout.as_millis(),
                                    }),
                                };
                                let idle_env = StreamEventEnvelopeV2 {
                                    event_id: Uuid::new_v4().to_string(),
                                    correlation_id: format!("idle-timeout-{}", Uuid::new_v4()),
                                    ts_ms: crate::logs::now_ms(),
                                    session_id: None,
                                    source: StreamEventSource::System,
                                    payload: idle_raw,
                                };
                                let _ = app.emit("sidecar_event_v2", &idle_env);
                                let _ = tx.send(idle_env);
                            }

                            let has_active_sessions =
                                !active_sessions.is_empty() || !pending_tools.is_empty();
                            if last_progress.elapsed() > no_event_watchdog
                                && has_active_sessions
                                && !matches!(health, StreamHealthStatus::Degraded)
                            {
                                emit_event(
                                    tracing::Level::WARN,
                                    ProcessKind::Desktop,
                                    ObservabilityEvent {
                                        event: "stream.watchdog.no_events",
                                        component: "stream_hub",
                                        correlation_id: None,
                                        session_id: None,
                                        run_id: None,
                                        message_id: None,
                                        provider_id: None,
                                        model_id: None,
                                        status: Some("degraded"),
                                        error_code: Some("STREAM_DISCONNECT"),
                                        detail: Some("no events watchdog triggered"),
                                    },
                                );
                                health = StreamHealthStatus::Degraded;
                                emit_stream_health(
                                    StreamHealthStatus::Degraded,
                                    Some("no_events_watchdog".to_string()),
                                    &app,
                                    &tx,
                                    &runtime,
                                )
                                .await;
                            }
                        }
                        _ = &mut stop_rx => {
                            break 'outer;
                        }
                        maybe = stream.next() => {
                            let Some(next_item) = maybe else {
                                tracing::info!("StreamHub stream ended; attempting resubscribe");
                                emit_event(
                                    tracing::Level::WARN,
                                    ProcessKind::Desktop,
                                    ObservabilityEvent {
                                        event: "stream.disconnected",
                                        component: "stream_hub",
                                        correlation_id: None,
                                        session_id: None,
                                        run_id: None,
                                        message_id: None,
                                        provider_id: None,
                                        model_id: None,
                                        status: Some("recovering"),
                                        error_code: Some("STREAM_DISCONNECT"),
                                        detail: Some("sidecar event stream ended"),
                                    },
                                );
                                if !matches!(health, StreamHealthStatus::Recovering) {
                                    health = StreamHealthStatus::Recovering;
                                    emit_stream_health(
                                        StreamHealthStatus::Recovering,
                                        Some("stream_ended".to_string()),
                                        &app,
                                        &tx,
                                        &runtime,
                                    )
                                    .await;
                                }
                                break;
                            };

                            match next_item {
                                Ok(mut event) => {
                                    last_progress = Instant::now();
                                    {
                                        let mut rt = runtime.write().await;
                                        rt.last_event_ts_ms = Some(crate::logs::now_ms());
                                        rt.sequence = rt.sequence.saturating_add(1);
                                    }
                                    if !matches!(health, StreamHealthStatus::Healthy) {
                                        health = StreamHealthStatus::Healthy;
                                        emit_stream_health(
                                            StreamHealthStatus::Healthy,
                                            Some("events_resumed".to_string()),
                                            &app,
                                            &tx,
                                            &runtime,
                                        )
                                        .await;
                                    }
                                    if let StreamEvent::ToolEnd {
                                        session_id,
                                        message_id,
                                        part_id,
                                        tool,
                                        ..
                                    } = &mut event
                                    {
                                        if !pending_tools
                                            .contains_key(&(session_id.clone(), part_id.clone()))
                                        {
                                            if let Some((candidate_part, _)) = pending_tools
                                                .iter()
                                                .find(|((sid, _), pending)| {
                                                    sid == session_id
                                                        && pending.message_id == *message_id
                                                        && pending.tool.eq_ignore_ascii_case(tool)
                                                })
                                                .map(|((_, candidate_part), pending)| {
                                                    (candidate_part.clone(), pending.clone())
                                                })
                                            {
                                                let incoming_part_id = part_id.clone();
                                                *part_id = candidate_part.clone();
                                                tracing::warn!(
                                                    "tool.lifecycle.end remapped mismatched part_id session_id={} message_id={} tool={} incoming_part_id={} resolved_part_id={}",
                                                    session_id,
                                                    message_id,
                                                    tool,
                                                    incoming_part_id,
                                                    candidate_part
                                                );
                                            }
                                        }
                                    }
                                    if let Err(e) = crate::tool_history::record_stream_event(&app, &event) {
                                        tracing::warn!("Failed to persist tool history event: {}", e);
                                        if let StreamEvent::ToolEnd {
                                            session_id,
                                            message_id,
                                            part_id,
                                            tool,
                                            ..
                                        } = &event
                                        {
                                            // Emit an explicit synthetic terminal event so UIs can close
                                            // pending indicators even when persistence is degraded.
                                            let synthetic = StreamEvent::ToolEnd {
                                                session_id: session_id.clone(),
                                                message_id: message_id.clone(),
                                                part_id: part_id.clone(),
                                                tool: tool.clone(),
                                                result: None,
                                                error: Some("interrupted".to_string()),
                                            };
                                            let synthetic_env = StreamEventEnvelopeV2 {
                                                event_id: Uuid::new_v4().to_string(),
                                                correlation_id: format!("{}:{}", session_id, part_id),
                                                ts_ms: crate::logs::now_ms(),
                                                session_id: Some(session_id.clone()),
                                                source: StreamEventSource::System,
                                                payload: synthetic.clone(),
                                            };
                                            let _ = app.emit("sidecar_event", &synthetic);
                                            let _ = app.emit("sidecar_event_v2", &synthetic_env);
                                            let _ = tx.send(synthetic_env);
                                            emit_event(
                                                tracing::Level::WARN,
                                                ProcessKind::Desktop,
                                                ObservabilityEvent {
                                                    event: "tool.synthetic_terminal_emitted",
                                                    component: "stream_hub",
                                                    correlation_id: None,
                                                    session_id: Some(session_id),
                                                    run_id: None,
                                                    message_id: Some(message_id),
                                                    provider_id: None,
                                                    model_id: None,
                                                    status: Some("ok"),
                                                    error_code: Some("TOOL_PERSISTENCE_FAILED"),
                                                    detail: Some(
                                                        "synthetic tool terminal emitted after tool_history write failure",
                                                    ),
                                                },
                                            );
                                        }
                                    }
                                    match &event {
                                        StreamEvent::Content {
                                            session_id,
                                            message_id,
                                            content,
                                            delta,
                                        } => {
                                            let key = (session_id.clone(), message_id.clone());
                                            if !content.trim().is_empty() {
                                                assistant_content_by_message
                                                    .insert(key.clone(), content.clone());
                                            } else if let Some(delta_text) = delta {
                                                if !delta_text.is_empty() {
                                                    assistant_content_by_message
                                                        .entry(key.clone())
                                                        .or_default()
                                                        .push_str(delta_text);
                                                }
                                            }
                                            assistant_last_message_by_session
                                                .insert(session_id.clone(), message_id.clone());
                                            active_sessions.insert(session_id.clone());
                                        }
                                        StreamEvent::ToolStart {
                                            session_id,
                                            message_id,
                                            part_id,
                                            tool,
                                            ..
                                        } => {
                                            let correlation_id =
                                                format!("{}:{}:{}", session_id, message_id, part_id);
                                            tracing::info!(
                                                "tool.lifecycle.start session_id={} message_id={} part_id={} correlation_id={} tool={}",
                                                session_id,
                                                message_id,
                                                part_id,
                                                correlation_id,
                                                tool
                                            );
                                            pending_tools.insert(
                                                (session_id.clone(), part_id.clone()),
                                                PendingToolState {
                                                    tool: tool.clone(),
                                                    message_id: message_id.clone(),
                                                    started: Instant::now(),
                                                    correlation_id,
                                                },
                                            );
                                            active_sessions.insert(session_id.clone());
                                        }
                                        StreamEvent::ToolEnd {
                                            session_id,
                                            message_id,
                                            part_id,
                                            tool,
                                            ..
                                        } => {
                                            tracing::info!(
                                                "tool.lifecycle.end session_id={} message_id={} part_id={} correlation_id={}:{} tool={}",
                                                session_id,
                                                message_id,
                                                part_id,
                                                session_id,
                                                part_id,
                                                tool
                                            );
                                            pending_tools
                                                .remove(&(session_id.clone(), part_id.clone()));
                                            active_sessions.insert(session_id.clone());
                                        }
                                        StreamEvent::SessionIdle { session_id }
                                        | StreamEvent::SessionError { session_id, .. }
                                        | StreamEvent::RunFinished { session_id, .. } => {
                                            active_sessions.remove(session_id);
                                            if let Some(last_message_id) =
                                                assistant_last_message_by_session
                                                    .get(session_id)
                                                    .cloned()
                                            {
                                                let key =
                                                    (session_id.clone(), last_message_id.clone());
                                                if !assistant_memory_stored.contains(&key) {
                                                    if let Some(content) =
                                                        assistant_content_by_message.get(&key)
                                                    {
                                                        if !content.trim().is_empty() {
                                                            assistant_memory_stored
                                                                .insert(key.clone());
                                                            let app_clone = app.clone();
                                                            let session_id_clone = session_id.clone();
                                                            let message_id_clone =
                                                                last_message_id.clone();
                                                            let content_clone = content.clone();
                                                            tokio::spawn(async move {
                                                                persist_assistant_message_memory(
                                                                    &app_clone,
                                                                    &session_id_clone,
                                                                    &message_id_clone,
                                                                    &content_clone,
                                                                )
                                                                .await;
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                            emit_event(
                                                tracing::Level::INFO,
                                                ProcessKind::Desktop,
                                                ObservabilityEvent {
                                                    event: "tool.reconcile.start",
                                                    component: "stream_hub",
                                                    correlation_id: None,
                                                    session_id: Some(session_id),
                                                    run_id: None,
                                                    message_id: None,
                                                    provider_id: None,
                                                    model_id: None,
                                                    status: Some("running"),
                                                    error_code: None,
                                                    detail: Some(
                                                        "reconciling running tools on session terminal event",
                                                    ),
                                                },
                                            );
                                            let dangling: Vec<((String, String), PendingToolState)> =
                                                pending_tools
                                                    .iter()
                                                    .filter(|((sid, _), _)| sid == session_id)
                                                    .map(|(key, pending)| (key.clone(), pending.clone()))
                                                    .collect();
                                            for ((pending_session, pending_part_id), pending) in &dangling {
                                                let synthetic = StreamEvent::ToolEnd {
                                                    session_id: pending_session.clone(),
                                                    message_id: pending.message_id.clone(),
                                                    part_id: pending_part_id.clone(),
                                                    tool: pending.tool.clone(),
                                                    result: None,
                                                    error: Some("interrupted".to_string()),
                                                };
                                                let _ = crate::tool_history::record_stream_event(&app, &synthetic);
                                                let synthetic_env = StreamEventEnvelopeV2 {
                                                    event_id: Uuid::new_v4().to_string(),
                                                    correlation_id: pending.correlation_id.clone(),
                                                    ts_ms: crate::logs::now_ms(),
                                                    session_id: Some(pending_session.clone()),
                                                    source: StreamEventSource::System,
                                                    payload: synthetic.clone(),
                                                };
                                                let _ = app.emit("sidecar_event", &synthetic);
                                                let _ = app.emit("sidecar_event_v2", &synthetic_env);
                                                let _ = tx.send(synthetic_env);
                                            }
                                            for ((pending_session, pending_part_id), _) in dangling {
                                                pending_tools.remove(&(pending_session, pending_part_id));
                                            }
                                            match crate::tool_history::mark_running_tools_terminal(
                                                &app,
                                                Some(session_id),
                                                0,
                                                "interrupted",
                                            ) {
                                                Ok(reconciled) => {
                                                    if reconciled > 0 {
                                                        emit_event(
                                                            tracing::Level::INFO,
                                                            ProcessKind::Desktop,
                                                            ObservabilityEvent {
                                                                event: "tool.reconcile.end",
                                                                component: "stream_hub",
                                                                correlation_id: None,
                                                                session_id: Some(session_id),
                                                                run_id: None,
                                                                message_id: None,
                                                                provider_id: None,
                                                                model_id: None,
                                                                status: Some("ok"),
                                                                error_code: None,
                                                                detail: Some(
                                                                    "reconciled running tools on session terminal event",
                                                                ),
                                                            },
                                                        );
                                                    }
                                                }
                                                Err(_) => emit_event(
                                                    tracing::Level::WARN,
                                                    ProcessKind::Desktop,
                                                    ObservabilityEvent {
                                                        event: "tool.reconcile.end",
                                                        component: "stream_hub",
                                                        correlation_id: None,
                                                        session_id: Some(session_id),
                                                        run_id: None,
                                                        message_id: None,
                                                        provider_id: None,
                                                        model_id: None,
                                                        status: Some("failed"),
                                                        error_code: Some("TOOL_RECONCILE_FAILED"),
                                                        detail: Some(
                                                            "failed to reconcile tools on session terminal event",
                                                        ),
                                                    },
                                                ),
                                            }
                                        }
                                        StreamEvent::PermissionAsked {
                                            session_id,
                                            request_id,
                                            tool,
                                            args,
                                            query,
                                            ..
                                        } => {
                                            active_sessions.insert(session_id.clone());
                                            if let Some(app_state) =
                                                app.try_state::<crate::state::AppState>()
                                            {
                                                if let Some(args_value) = args.clone() {
                                                    app_state
                                                        .permission_args_cache
                                                        .lock()
                                                        .await
                                                        .insert(request_id.clone(), args_value);
                                                }
                                                if tool
                                                    .as_deref()
                                                    .map(normalize_tool_name)
                                                    .as_deref()
                                                    == Some("websearch")
                                                {
                                                    if let Some(query_text) = query.clone().or_else(
                                                        || extract_websearch_query(args.as_ref()),
                                                    ) {
                                                        app_state
                                                            .session_websearch_intent
                                                            .lock()
                                                            .await
                                                            .insert(
                                                                session_id.clone(),
                                                                query_text,
                                                            );
                                                    }
                                                }
                                            }
                                        }
                                        StreamEvent::QuestionAsked { session_id, .. } => {
                                            active_sessions.insert(session_id.clone());
                                        }
                                        StreamEvent::SessionStatus { session_id, status } => {
                                            let terminal = [
                                                "idle",
                                                "completed",
                                                "failed",
                                                "error",
                                                "cancelled",
                                                "timeout",
                                            ]
                                            .contains(&status.as_str());
                                            let running =
                                                ["running", "in_progress", "executing"]
                                                    .contains(&status.as_str());
                                            if terminal {
                                                active_sessions.remove(session_id);
                                            } else if running {
                                                active_sessions.insert(session_id.clone());
                                            }
                                        }
                                        StreamEvent::RunStarted { session_id, .. }
                                        | StreamEvent::RunConflict { session_id, .. } => {
                                            active_sessions.insert(session_id.clone());
                                        }
                                        _ => {}
                                    }

                                    let env = StreamEventEnvelopeV2 {
                                        event_id: Uuid::new_v4().to_string(),
                                        correlation_id: derive_correlation_id(&event),
                                        ts_ms: crate::logs::now_ms(),
                                        session_id: extract_session_id(&event),
                                        source: derive_source(&event),
                                        payload: event.clone(),
                                    };

                                    let _ = app.emit("sidecar_event", &event);
                                    let _ = app.emit("sidecar_event_v2", &env);
                                    let _ = tx.send(env);
                                }
                                Err(e) => {
                                    tracing::warn!("StreamHub stream error: {}", e);
                                    if !matches!(health, StreamHealthStatus::Degraded) {
                                        health = StreamHealthStatus::Degraded;
                                        emit_stream_health(
                                            StreamHealthStatus::Degraded,
                                            Some("stream_error".to_string()),
                                            &app,
                                            &tx,
                                            &runtime,
                                        )
                                        .await;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            tracing::info!("StreamHub task stopped");
        });

        state.running = true;
        state.stop_tx = Some(stop_tx);
        state.task = Some(task);
        Ok(())
    }

    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        if let Some(stop_tx) = state.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(task) = state.task.take() {
            let _ = task.await;
        }
        state.running = false;
    }
}

async fn emit_stream_health(
    status: StreamHealthStatus,
    reason: Option<String>,
    app: &AppHandle,
    tx: &broadcast::Sender<StreamEventEnvelopeV2>,
    runtime: &tokio::sync::RwLock<StreamRuntimeState>,
) {
    let raw = StreamEvent::Raw {
        event_type: "system.stream_health".to_string(),
        data: serde_json::json!({
            "status": status,
            "reason": reason,
        }),
    };
    let env = StreamEventEnvelopeV2 {
        event_id: Uuid::new_v4().to_string(),
        correlation_id: format!("health-{}", Uuid::new_v4()),
        ts_ms: crate::logs::now_ms(),
        session_id: None,
        source: StreamEventSource::System,
        payload: raw,
    };
    let _ = app.emit("sidecar_event_v2", &env);
    let _ = tx.send(env);
    let mut rt = runtime.write().await;
    rt.health = status;
    rt.health_reason = reason;
    rt.last_health_change_ts_ms = crate::logs::now_ms();
    rt.sequence = rt.sequence.saturating_add(1);
}

fn extract_session_id(event: &StreamEvent) -> Option<String> {
    match event {
        StreamEvent::Content { session_id, .. }
        | StreamEvent::ToolStart { session_id, .. }
        | StreamEvent::ToolEnd { session_id, .. }
        | StreamEvent::SessionStatus { session_id, .. }
        | StreamEvent::RunStarted { session_id, .. }
        | StreamEvent::RunFinished { session_id, .. }
        | StreamEvent::RunConflict { session_id, .. }
        | StreamEvent::SessionIdle { session_id }
        | StreamEvent::SessionError { session_id, .. }
        | StreamEvent::PermissionAsked { session_id, .. }
        | StreamEvent::QuestionAsked { session_id, .. }
        | StreamEvent::TodoUpdated { session_id, .. }
        | StreamEvent::FileEdited { session_id, .. }
        | StreamEvent::MemoryRetrieval { session_id, .. }
        | StreamEvent::MemoryStorage { session_id, .. } => Some(session_id.clone()),
        StreamEvent::Raw { .. } => None,
    }
}

fn derive_source(event: &StreamEvent) -> StreamEventSource {
    match event {
        StreamEvent::MemoryRetrieval { .. } | StreamEvent::MemoryStorage { .. } => {
            StreamEventSource::Memory
        }
        StreamEvent::Raw { event_type, .. } if event_type.starts_with("system.") => {
            StreamEventSource::System
        }
        _ => StreamEventSource::Sidecar,
    }
}

fn derive_correlation_id(event: &StreamEvent) -> String {
    match event {
        StreamEvent::ToolStart {
            session_id,
            part_id,
            ..
        }
        | StreamEvent::ToolEnd {
            session_id,
            part_id,
            ..
        } => format!("{}:{}", session_id, part_id),
        StreamEvent::Content {
            session_id,
            message_id,
            ..
        } => format!("{}:{}", session_id, message_id),
        StreamEvent::PermissionAsked {
            session_id,
            request_id,
            ..
        }
        | StreamEvent::QuestionAsked {
            session_id,
            request_id,
            ..
        } => format!("{}:{}", session_id, request_id),
        StreamEvent::SessionStatus { session_id, status } => format!("{}:{}", session_id, status),
        StreamEvent::RunStarted {
            session_id, run_id, ..
        }
        | StreamEvent::RunFinished {
            session_id, run_id, ..
        }
        | StreamEvent::RunConflict {
            session_id, run_id, ..
        } => format!("{}:{}", session_id, run_id),
        StreamEvent::SessionIdle { session_id }
        | StreamEvent::SessionError { session_id, .. }
        | StreamEvent::TodoUpdated { session_id, .. }
        | StreamEvent::FileEdited { session_id, .. }
        | StreamEvent::MemoryRetrieval { session_id, .. }
        | StreamEvent::MemoryStorage { session_id, .. } => session_id.clone(),
        StreamEvent::Raw { .. } => Uuid::new_v4().to_string(),
    }
}

fn tool_timeout_for(tool: &str) -> Duration {
    match tool.trim().to_ascii_lowercase().as_str() {
        // Workspace-wide file enumeration can be slow on large repos (especially Windows).
        "glob" => Duration::from_secs(10 * 60),
        "grep" | "search" | "codesearch" => Duration::from_secs(5 * 60),
        _ => Duration::from_secs(120),
    }
}

fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

fn extract_websearch_query(args: Option<&serde_json::Value>) -> Option<String> {
    let args = args?;
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    for key in QUERY_KEYS {
        if let Some(q) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = q.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

async fn persist_assistant_message_memory(
    app: &AppHandle,
    session_id: &str,
    message_id: &str,
    content: &str,
) {
    let Some(app_state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let Some(manager) = app_state.memory_manager.clone() else {
        return;
    };

    let project_id = match app_state.sidecar.get_session(session_id).await {
        Ok(session) => session.project_id,
        Err(_) => app_state.active_project_id.read().unwrap().clone(),
    };

    let metadata = serde_json::json!({
        "role": "assistant",
        "source_kind": "chat_turn",
        "message_id": message_id
    });

    let session_req = StoreMessageRequest {
        content: content.to_string(),
        tier: MemoryTier::Session,
        session_id: Some(session_id.to_string()),
        project_id: project_id.clone(),
        source: "assistant_response".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        metadata: Some(metadata.clone()),
    };
    let mut session_chunks_stored = 0usize;
    let mut project_chunks_stored = 0usize;
    let mut storage_error: Option<String> = None;
    match manager.store_message(session_req).await {
        Ok(ids) => {
            session_chunks_stored = ids.len();
        }
        Err(err) => {
            tracing::warn!(
                target: "tandem.memory",
                "Failed to store assistant session memory chunk: session_id={} message_id={} error={}",
                session_id,
                message_id,
                err
            );
            storage_error.get_or_insert_with(|| err.to_string());
        }
    }

    if let Some(pid) = project_id {
        let project_req = StoreMessageRequest {
            content: content.to_string(),
            tier: MemoryTier::Project,
            session_id: Some(session_id.to_string()),
            project_id: Some(pid.clone()),
            source: "assistant_response".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            metadata: Some(metadata),
        };
        match manager.store_message(project_req).await {
            Ok(ids) => {
                project_chunks_stored = ids.len();
            }
            Err(err) => {
                tracing::warn!(
                    target: "tandem.memory",
                    "Failed to store assistant project memory chunk: session_id={} project_id={} message_id={} error={}",
                    session_id,
                    pid,
                    message_id,
                    err
                );
                storage_error.get_or_insert_with(|| err.to_string());
            }
        }
    }

    let event = StreamEvent::MemoryStorage {
        session_id: session_id.to_string(),
        message_id: Some(message_id.to_string()),
        role: "assistant".to_string(),
        session_chunks_stored,
        project_chunks_stored,
        status: Some(if storage_error.is_some() {
            "error".to_string()
        } else {
            "ok".to_string()
        }),
        error: storage_error,
    };
    if let Err(err) = crate::tool_history::record_stream_event(app, &event) {
        tracing::warn!("Failed to persist assistant memory storage event: {}", err);
    }
    let _ = app.emit("sidecar_event", &event);
    let envelope = StreamEventEnvelopeV2 {
        event_id: Uuid::new_v4().to_string(),
        correlation_id: format!("{}:memory-store:assistant:{}", session_id, Uuid::new_v4()),
        ts_ms: crate::logs::now_ms(),
        session_id: Some(session_id.to_string()),
        source: StreamEventSource::Memory,
        payload: event,
    };
    let _ = app.emit("sidecar_event_v2", envelope);
}
