use crate::error::Result;
use crate::sidecar::{SidecarManager, StreamEvent};
use futures::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
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

struct StreamHubState {
    running: bool,
    stop_tx: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

pub struct StreamHub {
    state: Mutex<StreamHubState>,
    tx: broadcast::Sender<StreamEventEnvelopeV2>,
}

impl StreamHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(2048);
        Self {
            state: Mutex::new(StreamHubState {
                running: false,
                stop_tx: None,
                task: None,
            }),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEventEnvelopeV2> {
        self.tx.subscribe()
    }

    pub async fn start(&self, app: AppHandle, sidecar: Arc<SidecarManager>) -> Result<()> {
        let mut state = self.state.lock().await;
        if state.running {
            return Ok(());
        }

        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let tx = self.tx.clone();

        let task = tokio::spawn(async move {
            let mut health = StreamHealthStatus::Recovering;
            let mut pending_tools: HashMap<(String, String), (String, Instant)> = HashMap::new();
            let mut last_progress = Instant::now();
            let tool_timeout = Duration::from_secs(120);
            let idle_timeout = Duration::from_secs(10 * 60);

            let emit_health = |status: StreamHealthStatus, app: &AppHandle, tx: &broadcast::Sender<StreamEventEnvelopeV2>| {
                let raw = StreamEvent::Raw {
                    event_type: "system.stream_health".to_string(),
                    data: serde_json::json!({
                        "status": status,
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
            };

            emit_health(StreamHealthStatus::Recovering, &app, &tx);

            'outer: loop {
                let stream_res = sidecar.subscribe_events().await;
                let stream = match stream_res {
                    Ok(s) => {
                        if !matches!(health, StreamHealthStatus::Healthy) {
                            health = StreamHealthStatus::Healthy;
                            emit_health(StreamHealthStatus::Healthy, &app, &tx);
                        }
                        s
                    }
                    Err(e) => {
                        tracing::warn!("StreamHub failed to subscribe to sidecar events: {}", e);
                        if !matches!(health, StreamHealthStatus::Degraded) {
                            health = StreamHealthStatus::Degraded;
                            emit_health(StreamHealthStatus::Degraded, &app, &tx);
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
                            if let Some(((session_id, part_id), (tool, _started))) = pending_tools
                                .iter()
                                .find(|(_, (_, started))| started.elapsed() > tool_timeout)
                            {
                                let timeout_event = StreamEvent::SessionError {
                                    session_id: session_id.clone(),
                                    error: format!(
                                        "Tool '{}' (part {}) exceeded timeout of {:?}",
                                        tool,
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
                        }
                        _ = &mut stop_rx => {
                            break 'outer;
                        }
                        maybe = stream.next() => {
                            let Some(next_item) = maybe else {
                                tracing::info!("StreamHub stream ended; attempting resubscribe");
                                if !matches!(health, StreamHealthStatus::Recovering) {
                                    health = StreamHealthStatus::Recovering;
                                    emit_health(StreamHealthStatus::Recovering, &app, &tx);
                                }
                                break;
                            };

                            match next_item {
                                Ok(event) => {
                                    last_progress = Instant::now();
                                    match &event {
                                        StreamEvent::ToolStart { session_id, part_id, tool, .. } => {
                                            pending_tools.insert((session_id.clone(), part_id.clone()), (tool.clone(), Instant::now()));
                                        }
                                        StreamEvent::ToolEnd { session_id, part_id, .. } => {
                                            pending_tools.remove(&(session_id.clone(), part_id.clone()));
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
                                        emit_health(StreamHealthStatus::Degraded, &app, &tx);
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

fn extract_session_id(event: &StreamEvent) -> Option<String> {
    match event {
        StreamEvent::Content { session_id, .. }
        | StreamEvent::ToolStart { session_id, .. }
        | StreamEvent::ToolEnd { session_id, .. }
        | StreamEvent::SessionStatus { session_id, .. }
        | StreamEvent::SessionIdle { session_id }
        | StreamEvent::SessionError { session_id, .. }
        | StreamEvent::PermissionAsked { session_id, .. }
        | StreamEvent::QuestionAsked { session_id, .. }
        | StreamEvent::TodoUpdated { session_id, .. }
        | StreamEvent::FileEdited { session_id, .. }
        | StreamEvent::MemoryRetrieval { session_id, .. } => Some(session_id.clone()),
        StreamEvent::Raw { .. } => None,
    }
}

fn derive_source(event: &StreamEvent) -> StreamEventSource {
    match event {
        StreamEvent::MemoryRetrieval { .. } => StreamEventSource::Memory,
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
        StreamEvent::SessionIdle { session_id }
        | StreamEvent::SessionError { session_id, .. }
        | StreamEvent::TodoUpdated { session_id, .. }
        | StreamEvent::FileEdited { session_id, .. }
        | StreamEvent::MemoryRetrieval { session_id, .. } => session_id.clone(),
        StreamEvent::Raw { .. } => Uuid::new_v4().to_string(),
    }
}
