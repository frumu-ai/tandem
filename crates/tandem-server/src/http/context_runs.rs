use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures::Stream;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::http::context_types::*;
use crate::AppState;
use base64::Engine;
use tandem_types::EngineEvent;
use tandem_workflows::WorkflowActionRunStatus;

#[derive(Debug, Clone)]
pub(super) struct ContextRunCommitResult {
    pub(super) run: ContextRunState,
    pub(super) event: ContextRunEventRecord,
    pub(super) blackboard: ContextBlackboardState,
    pub(super) patch: Option<ContextBlackboardPatchRecord>,
}

#[derive(Default)]
pub(crate) struct ContextRunEngine {
    locks: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub(crate) fn context_run_engine() -> &'static ContextRunEngine {
    static ENGINE: std::sync::OnceLock<ContextRunEngine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(ContextRunEngine::default)
}

impl ContextRunEngine {
    async fn lock_for(&self, run_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self.locks.lock().await;
        guard
            .entry(run_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

fn parse_context_task_kind(value: Option<&str>) -> Option<ContextTaskKind> {
    match value.map(str::trim).filter(|row| !row.is_empty())? {
        "implementation" => Some(ContextTaskKind::Implementation),
        "inspection" => Some(ContextTaskKind::Inspection),
        "research" => Some(ContextTaskKind::Research),
        "validation" => Some(ContextTaskKind::Validation),
        _ => None,
    }
}

fn context_task_kind_str(kind: &ContextTaskKind) -> &'static str {
    match kind {
        ContextTaskKind::Implementation => "implementation",
        ContextTaskKind::Inspection => "inspection",
        ContextTaskKind::Research => "research",
        ContextTaskKind::Validation => "validation",
    }
}

fn context_task_execution_mode_str(mode: &ContextTaskExecutionMode) -> &'static str {
    match mode {
        ContextTaskExecutionMode::StrictWrite => "strict_write",
        ContextTaskExecutionMode::StrictNonwriting => "strict_nonwriting",
        ContextTaskExecutionMode::BestEffort => "best_effort",
    }
}

fn normalize_context_task_output_target(value: Option<&Value>) -> Option<ContextTaskOutputTarget> {
    let Value::Object(map) = value?.clone() else {
        return None;
    };
    let path = map
        .get("path")
        .or_else(|| map.get("file"))
        .or_else(|| map.get("file_path"))
        .or_else(|| map.get("target"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())?
        .to_string();
    let kind = map
        .get("kind")
        .or_else(|| map.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string);
    let operation = map
        .get("operation")
        .or_else(|| map.get("mode"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string);
    Some(ContextTaskOutputTarget {
        path,
        kind,
        operation,
    })
}

fn normalize_context_task_payload(
    task_type: &str,
    payload: &Value,
) -> Result<Option<(String, Value)>, (String, String)> {
    let mut map = match payload {
        Value::Object(existing) => existing.clone(),
        Value::Null => serde_json::Map::new(),
        _ => {
            return Err((
                "TASK_CONTRACT_INVALID".to_string(),
                "task payload must be a JSON object for strict blackboard tasks".to_string(),
            ));
        }
    };
    let payload_kind = map.get("task_kind").and_then(Value::as_str);
    let inferred_kind =
        parse_context_task_kind(payload_kind).or_else(|| parse_context_task_kind(Some(task_type)));
    let Some(task_kind) = inferred_kind else {
        return Ok(None);
    };
    let execution_mode = match task_kind {
        ContextTaskKind::Implementation => ContextTaskExecutionMode::StrictWrite,
        ContextTaskKind::Inspection | ContextTaskKind::Research | ContextTaskKind::Validation => {
            ContextTaskExecutionMode::StrictNonwriting
        }
    };
    let normalized_task_type = context_task_kind_str(&task_kind).to_string();
    map.insert("task_kind".to_string(), json!(normalized_task_type));
    map.insert(
        "execution_mode".to_string(),
        json!(context_task_execution_mode_str(&execution_mode)),
    );
    match task_kind {
        ContextTaskKind::Implementation => {
            let output_target = normalize_context_task_output_target(map.get("output_target"));
            let Some(output_target) = output_target else {
                return Err((
                    "TASK_OUTPUT_TARGET_REQUIRED".to_string(),
                    "implementation tasks must include payload.output_target.path".to_string(),
                ));
            };
            map.insert(
                "output_target".to_string(),
                serde_json::to_value(output_target).map_err(|_| {
                    (
                        "TASK_CONTRACT_INVALID".to_string(),
                        "invalid output target".to_string(),
                    )
                })?,
            );
        }
        ContextTaskKind::Inspection | ContextTaskKind::Research | ContextTaskKind::Validation => {
            map.remove("write_required");
        }
    }
    Ok(Some((normalized_task_type, Value::Object(map))))
}

pub(super) fn context_runs_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("context_runs"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("context_runs"))
}

pub(super) fn context_run_dir(state: &AppState, run_id: &str) -> PathBuf {
    context_runs_root(state).join(run_id)
}

pub(crate) async fn append_json_artifact_to_context_run(
    state: &AppState,
    run_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    relative_path: &str,
    payload: &Value,
) -> anyhow::Result<()> {
    let path = context_run_dir(state, run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let raw = serde_json::to_string_pretty(payload)?;
    tokio::fs::write(&path, raw).await?;
    let artifact = ContextBlackboardArtifact {
        id: artifact_id.to_string(),
        ts_ms: crate::now_ms(),
        path: path.to_string_lossy().to_string(),
        artifact_type: artifact_type.to_string(),
        step_id: None,
        source_event_id: None,
    };
    context_run_engine()
        .commit_blackboard_patch(
            state,
            run_id,
            ContextBlackboardPatchOp::AddArtifact,
            serde_json::to_value(&artifact)?,
        )
        .await
        .map_err(|status| anyhow::anyhow!("context blackboard patch failed: {status}"))?;
    Ok(())
}

pub(super) fn context_run_state_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("run_state.json")
}

pub(super) fn context_run_events_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("events.jsonl")
}

pub(super) fn context_run_blackboard_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("blackboard.json")
}

pub(super) fn context_run_blackboard_patches_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("blackboard_patches.jsonl")
}

pub(super) fn context_run_checkpoints_dir(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("checkpoints")
}

pub(super) async fn ensure_context_run_dir(
    state: &AppState,
    run_id: &str,
) -> Result<(), StatusCode> {
    let run_dir = context_run_dir(state, run_id);
    tokio::fs::create_dir_all(&run_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

pub(super) async fn load_context_run_state(
    state: &AppState,
    run_id: &str,
) -> Result<ContextRunState, StatusCode> {
    load_and_repair_context_run_state(state, run_id)
}

fn write_string_atomically(path: &FsPath, payload: &str) -> Result<(), StatusCode> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json")
    ));
    std::fs::write(&tmp_path, payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::rename(&tmp_path, path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(super) async fn save_context_run_state(
    state: &AppState,
    run: &ContextRunState,
) -> Result<(), StatusCode> {
    ensure_context_run_dir(state, &run.run_id).await?;
    let path = context_run_state_path(state, &run.run_id);
    let payload =
        serde_json::to_string_pretty(run).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::task::spawn_blocking(move || write_string_atomically(&path, &payload))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub(super) fn load_context_run_events_jsonl(
    path: &FsPath,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextRunEventRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let mut rows: Vec<ContextRunEventRecord> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<ContextRunEventRecord>(line).ok())
        .filter(|row| {
            if let Some(since) = since_seq {
                return row.seq > since;
            }
            true
        })
        .collect();
    rows.sort_by_key(|row| row.seq);
    if let Some(tail_count) = tail {
        if rows.len() > tail_count {
            rows = rows.split_off(rows.len().saturating_sub(tail_count));
        }
    }
    rows
}

pub(super) fn latest_context_run_event_seq(path: &FsPath) -> u64 {
    load_context_run_events_jsonl(path, None, None)
        .last()
        .map(|row| row.seq)
        .unwrap_or(0)
}

pub(super) fn append_jsonl_line(path: &FsPath, value: &Value) -> Result<(), StatusCode> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let line = serde_json::to_string(value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    writeln!(file, "{}", line).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(super) fn parse_context_run_ids_csv(raw: Option<&str>) -> Vec<String> {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn decode_context_stream_cursor(raw: Option<&str>) -> ContextRunsStreamCursor {
    let Some(token) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return ContextRunsStreamCursor::default();
    };
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(token));
    let Ok(bytes) = decoded else {
        return ContextRunsStreamCursor::default();
    };
    serde_json::from_slice::<ContextRunsStreamCursor>(&bytes).unwrap_or_default()
}

pub(super) fn load_context_run_workspace_sync(state: &AppState, run_id: &str) -> Option<String> {
    let path = context_run_state_path(state, run_id);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("canonical_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

pub(super) fn load_context_run_state_sync(
    state: &AppState,
    run_id: &str,
) -> Result<ContextRunState, StatusCode> {
    let path = context_run_state_path(state, run_id);
    let raw = std::fs::read_to_string(path).map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_str::<ContextRunState>(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(super) fn save_context_run_state_sync(
    state: &AppState,
    run: &ContextRunState,
) -> Result<(), StatusCode> {
    let path = context_run_state_path(state, &run.run_id);
    let payload =
        serde_json::to_string_pretty(run).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    write_string_atomically(&path, &payload)
}

fn upsert_context_run_task(run: &mut ContextRunState, task: ContextBlackboardTask) {
    if let Some(existing) = run.tasks.iter_mut().find(|row| row.id == task.id) {
        *existing = task;
    } else {
        run.tasks.push(task);
    }
}

fn next_context_run_event_seq(state: &AppState, run_id: &str, run: &ContextRunState) -> u64 {
    run.last_event_seq
        .max(latest_context_run_event_seq(&context_run_events_path(
            state, run_id,
        )))
        .saturating_add(1)
}

fn append_context_run_event_record_sync(
    state: &AppState,
    run_id: &str,
    record: &ContextRunEventRecord,
) -> Result<(), StatusCode> {
    append_jsonl_line(
        &context_run_events_path(state, run_id),
        &serde_json::to_value(record).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    )?;
    if let Some(workspace) = load_context_run_workspace_sync(state, run_id) {
        if !workspace.trim().is_empty() {
            let envelope = ContextRunsStreamEnvelope {
                kind: "context_run_event".to_string(),
                run_id: run_id.to_string(),
                workspace,
                seq: record.seq,
                ts_ms: record.ts_ms,
                payload: serde_json::to_value(record).unwrap_or_else(|_| json!({})),
            };
            publish_context_run_stream_envelope(state, &envelope);
        }
    }
    Ok(())
}

fn apply_context_run_event_record(run: &mut ContextRunState, event: &ContextRunEventRecord) {
    if let Some(next_run) = event
        .payload
        .get("run")
        .cloned()
        .and_then(|value| serde_json::from_value::<ContextRunState>(value).ok())
    {
        *run = next_run;
        run.run_id = event.run_id.clone();
        run.revision = event.revision.max(run.revision);
        run.last_event_seq = event.seq.max(run.last_event_seq);
        run.updated_at_ms = run.updated_at_ms.max(event.ts_ms);
        return;
    }

    if let Some(task) = event
        .payload
        .get("task")
        .cloned()
        .and_then(|value| serde_json::from_value::<ContextBlackboardTask>(value).ok())
    {
        upsert_context_run_task(run, task);
    } else {
        apply_context_event_transition(
            run,
            &ContextRunEventAppendInput {
                event_type: event.event_type.clone(),
                status: event.status.clone(),
                step_id: event.step_id.clone(),
                payload: event.payload.clone(),
            },
        );
    }
    run.revision = event.revision.max(run.revision);
    run.last_event_seq = event.seq.max(run.last_event_seq);
    run.updated_at_ms = run.updated_at_ms.max(event.ts_ms);
}

fn load_and_repair_context_run_state(
    state: &AppState,
    run_id: &str,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_context_run_state_sync(state, run_id)?;
    let pending = load_context_run_events_jsonl(
        &context_run_events_path(state, run_id),
        Some(run.last_event_seq),
        None,
    );
    if !pending.is_empty() {
        for event in &pending {
            apply_context_run_event_record(&mut run, event);
        }
        save_context_run_state_sync(state, &run)?;
    }
    Ok(run)
}

impl ContextRunEngine {
    pub(super) async fn commit_task_mutation(
        &self,
        state: &AppState,
        run_id: &str,
        mut next_task: ContextBlackboardTask,
        patch_op: ContextBlackboardPatchOp,
        mut patch_payload: Value,
        event_type: String,
        event_status: ContextRunStatus,
        command_id: Option<String>,
        mut event_payload: Value,
    ) -> Result<ContextRunCommitResult, StatusCode> {
        let lock = self.lock_for(run_id).await;
        let _guard = lock.lock().await;
        let mut run = load_and_repair_context_run_state(state, run_id)?;
        let now = crate::now_ms();
        let next_revision = run.revision.saturating_add(1);
        let next_event_seq = next_context_run_event_seq(state, run_id, &run);

        next_task.updated_ts = now;
        upsert_context_run_task(&mut run, next_task.clone());
        run.revision = next_revision;
        run.last_event_seq = next_event_seq;
        run.updated_at_ms = now;

        if let Some(payload) = event_payload.as_object_mut() {
            payload.insert(
                "task".to_string(),
                serde_json::to_value(&next_task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            );
            payload
                .entry("patch_seq".to_string())
                .or_insert_with(|| json!(next_event_seq));
        }

        let event = ContextRunEventRecord {
            event_id: format!("evt-{}", Uuid::new_v4()),
            run_id: run_id.to_string(),
            seq: next_event_seq,
            ts_ms: now,
            event_type,
            status: event_status,
            revision: next_revision,
            step_id: Some(next_task.id.clone()),
            task_id: Some(next_task.id.clone()),
            command_id,
            payload: event_payload,
        };
        append_context_run_event_record_sync(state, run_id, &event)?;
        save_context_run_state_sync(state, &run)?;

        if let Some(payload) = patch_payload.as_object_mut() {
            payload
                .entry("task_rev".to_string())
                .or_insert_with(|| json!(next_task.task_rev));
        }
        let patch = append_context_blackboard_patch_record(
            state,
            run_id,
            next_event_seq,
            patch_op,
            patch_payload,
        )?;
        let mut published_payload = event.payload.clone();
        if let Some(payload) = published_payload.as_object_mut() {
            payload
                .entry("runID".to_string())
                .or_insert_with(|| json!(run_id));
            payload
                .entry("eventID".to_string())
                .or_insert_with(|| json!(event.event_id.clone()));
            payload
                .entry("eventSeq".to_string())
                .or_insert_with(|| json!(event.seq));
            payload
                .entry("tsMs".to_string())
                .or_insert_with(|| json!(event.ts_ms));
            payload
                .entry("taskID".to_string())
                .or_insert_with(|| json!(next_task.id.clone()));
            payload
                .entry("taskStatus".to_string())
                .or_insert_with(|| json!(next_task.status.clone()));
        }
        state.event_bus.publish(EngineEvent::new(
            event.event_type.clone(),
            published_payload,
        ));
        let blackboard = load_projected_context_blackboard(state, run_id);
        Ok(ContextRunCommitResult {
            run,
            event,
            blackboard,
            patch: Some(patch),
        })
    }

    pub(super) async fn commit_run_event(
        &self,
        state: &AppState,
        run_id: &str,
        input: ContextRunEventAppendInput,
        command_id: Option<String>,
    ) -> Result<ContextRunCommitResult, StatusCode> {
        let lock = self.lock_for(run_id).await;
        let _guard = lock.lock().await;
        let mut run = load_and_repair_context_run_state(state, run_id)?;
        let now = crate::now_ms();
        let next_revision = run.revision.saturating_add(1);
        let next_event_seq = next_context_run_event_seq(state, run_id, &run);
        apply_context_event_transition(&mut run, &input);
        run.revision = next_revision;
        run.last_event_seq = next_event_seq;
        run.updated_at_ms = now;

        let event = ContextRunEventRecord {
            event_id: format!("evt-{}", Uuid::new_v4()),
            run_id: run_id.to_string(),
            seq: next_event_seq,
            ts_ms: now,
            event_type: input.event_type.clone(),
            status: input.status.clone(),
            revision: next_revision,
            step_id: input.step_id.clone(),
            task_id: None,
            command_id,
            payload: input.payload.clone(),
        };
        append_context_run_event_record_sync(state, run_id, &event)?;
        save_context_run_state_sync(state, &run)?;
        let blackboard = load_projected_context_blackboard(state, run_id);
        Ok(ContextRunCommitResult {
            run,
            event,
            blackboard,
            patch: None,
        })
    }

    pub(super) async fn commit_snapshot_with_event(
        &self,
        state: &AppState,
        run_id: &str,
        mut run: ContextRunState,
        input: ContextRunEventAppendInput,
        command_id: Option<String>,
    ) -> Result<ContextRunCommitResult, StatusCode> {
        let lock = self.lock_for(run_id).await;
        let _guard = lock.lock().await;
        let current = load_and_repair_context_run_state(state, run_id)?;
        let now = crate::now_ms();
        let next_revision = current.revision.saturating_add(1);
        let next_event_seq = next_context_run_event_seq(state, run_id, &current);
        run.revision = next_revision;
        run.last_event_seq = next_event_seq;
        run.updated_at_ms = now;
        let event = ContextRunEventRecord {
            event_id: format!("evt-{}", Uuid::new_v4()),
            run_id: run_id.to_string(),
            seq: next_event_seq,
            ts_ms: now,
            event_type: input.event_type.clone(),
            status: input.status.clone(),
            revision: next_revision,
            step_id: input.step_id.clone(),
            task_id: None,
            command_id,
            payload: input.payload.clone(),
        };
        append_context_run_event_record_sync(state, run_id, &event)?;
        save_context_run_state_sync(state, &run)?;
        let blackboard = load_projected_context_blackboard(state, run_id);
        Ok(ContextRunCommitResult {
            run,
            event,
            blackboard,
            patch: None,
        })
    }

    pub(super) async fn commit_blackboard_patch(
        &self,
        state: &AppState,
        run_id: &str,
        op: ContextBlackboardPatchOp,
        payload: Value,
    ) -> Result<ContextRunCommitResult, StatusCode> {
        let lock = self.lock_for(run_id).await;
        let _guard = lock.lock().await;
        let mut run = load_and_repair_context_run_state(state, run_id)?;
        let now = crate::now_ms();
        let next_revision = run.revision.saturating_add(1);
        let next_event_seq = next_context_run_event_seq(state, run_id, &run);
        run.revision = next_revision;
        run.last_event_seq = next_event_seq;
        run.updated_at_ms = now;
        let event = ContextRunEventRecord {
            event_id: format!("evt-{}", Uuid::new_v4()),
            run_id: run_id.to_string(),
            seq: next_event_seq,
            ts_ms: now,
            event_type: if matches!(op, ContextBlackboardPatchOp::AddArtifact) {
                "context.artifact.added".to_string()
            } else {
                "context.blackboard.patched".to_string()
            },
            status: run.status.clone(),
            revision: next_revision,
            step_id: None,
            task_id: None,
            command_id: None,
            payload: json!({
                "op": op,
                "payload": payload.clone(),
                "patch_seq": next_event_seq,
            }),
        };
        append_context_run_event_record_sync(state, run_id, &event)?;
        save_context_run_state_sync(state, &run)?;
        let patch =
            append_context_blackboard_patch_record(state, run_id, next_event_seq, op, payload)?;
        let blackboard = load_projected_context_blackboard(state, run_id);
        Ok(ContextRunCommitResult {
            run,
            event,
            blackboard,
            patch: Some(patch),
        })
    }
}

pub(super) fn publish_context_run_stream_envelope(
    state: &AppState,
    envelope: &ContextRunsStreamEnvelope,
) {
    if envelope.run_id.trim().is_empty() {
        return;
    }
    let payload = serde_json::to_value(envelope).unwrap_or_else(|_| json!({}));
    state
        .event_bus
        .publish(EngineEvent::new("context.run.stream", payload));
}

pub(super) async fn list_context_runs_for_workspace(
    state: &AppState,
    workspace: &str,
    limit: usize,
) -> Result<Vec<ContextRunState>, StatusCode> {
    let root = context_runs_root(state);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let normalized_workspace =
        tandem_core::normalize_workspace_path(workspace).ok_or(StatusCode::BAD_REQUEST)?;
    let mut rows = Vec::<ContextRunState>::new();
    let mut dir = tokio::fs::read_dir(root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let run_id = entry.file_name().to_string_lossy().to_string();
        if let Ok(run) = load_context_run_state(state, &run_id).await {
            if run.workspace.canonical_path.trim() == normalized_workspace {
                rows.push(run);
            }
        }
    }
    rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    rows.truncate(limit.clamp(1, 1000));
    Ok(rows)
}

pub(super) fn load_context_blackboard(state: &AppState, run_id: &str) -> ContextBlackboardState {
    let path = context_run_blackboard_path(state, run_id);
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<ContextBlackboardState>(&raw).unwrap_or_default(),
        Err(_) => ContextBlackboardState::default(),
    }
}

pub(super) fn save_context_blackboard(
    state: &AppState,
    run_id: &str,
    blackboard: &ContextBlackboardState,
) -> Result<(), StatusCode> {
    let path = context_run_blackboard_path(state, run_id);
    let mut stored = blackboard.clone();
    stored.tasks.clear();
    let payload =
        serde_json::to_string_pretty(&stored).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    write_string_atomically(&path, &payload)
}

pub(super) fn project_context_blackboard_with_run_tasks(
    state: &AppState,
    run_id: &str,
    mut blackboard: ContextBlackboardState,
) -> ContextBlackboardState {
    if let Ok(run) = load_context_run_state_sync(state, run_id) {
        blackboard.tasks = run.tasks;
    }
    blackboard
}

pub(super) fn load_projected_context_blackboard(
    state: &AppState,
    run_id: &str,
) -> ContextBlackboardState {
    let mut blackboard = load_context_blackboard(state, run_id);
    let latest_patch_seq = load_context_blackboard_patches(state, run_id, None, Some(1))
        .last()
        .map(|patch| patch.seq)
        .unwrap_or(0);
    if blackboard.revision < latest_patch_seq {
        let patches =
            load_context_blackboard_patches(state, run_id, Some(blackboard.revision), None);
        for patch in &patches {
            let _ = apply_context_blackboard_patch(&mut blackboard, patch);
        }
        let _ = save_context_blackboard(state, run_id, &blackboard);
    }
    project_context_blackboard_with_run_tasks(state, run_id, blackboard)
}

pub(super) fn apply_context_blackboard_patch(
    blackboard: &mut ContextBlackboardState,
    patch: &ContextBlackboardPatchRecord,
) -> Result<(), StatusCode> {
    let mut task_idx: HashMap<String, usize> = HashMap::new();
    for (idx, task) in blackboard.tasks.iter().enumerate() {
        task_idx.insert(task.id.clone(), idx);
    }
    match patch.op {
        ContextBlackboardPatchOp::AddFact => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.facts.push(row);
        }
        ContextBlackboardPatchOp::AddDecision => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.decisions.push(row);
        }
        ContextBlackboardPatchOp::AddOpenQuestion => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.open_questions.push(row);
        }
        ContextBlackboardPatchOp::AddArtifact => {
            let row = serde_json::from_value::<ContextBlackboardArtifact>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.artifacts.push(row);
        }
        ContextBlackboardPatchOp::SetRollingSummary => {
            let value = patch
                .payload
                .as_str()
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            blackboard.summaries.rolling = value;
        }
        ContextBlackboardPatchOp::SetLatestContextPack => {
            let value = patch
                .payload
                .as_str()
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            blackboard.summaries.latest_context_pack = value;
        }
        ContextBlackboardPatchOp::AddTask => {
            let task = serde_json::from_value::<ContextBlackboardTask>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            if let Some(idx) = task_idx.get(&task.id).copied() {
                blackboard.tasks[idx] = task;
            } else {
                blackboard.tasks.push(task);
            }
        }
        ContextBlackboardPatchOp::UpdateTaskLease => {
            let task_id = patch
                .payload
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            let Some(idx) = task_idx.get(&task_id).copied() else {
                return Err(StatusCode::NOT_FOUND);
            };
            let task = blackboard.tasks.get_mut(idx).ok_or(StatusCode::NOT_FOUND)?;
            task.lease_owner = patch
                .payload
                .get("lease_owner")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            task.lease_token = patch
                .payload
                .get("lease_token")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            task.lease_expires_at_ms = patch
                .payload
                .get("lease_expires_at_ms")
                .and_then(Value::as_u64);
            if let Some(agent_id) = patch
                .payload
                .get("assigned_agent")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.assigned_agent = Some(agent_id.to_string());
            }
            if let Some(next_rev) = patch.payload.get("task_rev").and_then(Value::as_u64) {
                task.task_rev = next_rev;
            } else {
                task.task_rev = task.task_rev.saturating_add(1);
            }
            task.updated_ts = patch.ts_ms;
        }
        ContextBlackboardPatchOp::UpdateTaskState => {
            let task_id = patch
                .payload
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            let Some(idx) = task_idx.get(&task_id).copied() else {
                return Err(StatusCode::NOT_FOUND);
            };
            let task = blackboard.tasks.get_mut(idx).ok_or(StatusCode::NOT_FOUND)?;
            let status = patch
                .payload
                .get("status")
                .cloned()
                .ok_or(StatusCode::BAD_REQUEST)
                .and_then(|row| {
                    serde_json::from_value::<ContextBlackboardTaskStatus>(row)
                        .map_err(|_| StatusCode::BAD_REQUEST)
                })?;
            task.status = status;
            if let Some(lease_owner) = patch
                .payload
                .get("lease_owner")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.lease_owner = Some(lease_owner.to_string());
            }
            if patch.payload.get("lease_owner").is_some()
                && patch
                    .payload
                    .get("lease_owner")
                    .and_then(Value::as_str)
                    .is_none()
            {
                task.lease_owner = None;
            }
            if let Some(lease_token) = patch
                .payload
                .get("lease_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.lease_token = Some(lease_token.to_string());
            }
            if patch.payload.get("lease_token").is_some()
                && patch
                    .payload
                    .get("lease_token")
                    .and_then(Value::as_str)
                    .is_none()
            {
                task.lease_token = None;
            }
            task.lease_expires_at_ms = patch
                .payload
                .get("lease_expires_at_ms")
                .and_then(Value::as_u64);
            if let Some(agent_id) = patch
                .payload
                .get("assigned_agent")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.assigned_agent = Some(agent_id.to_string());
            }
            if let Some(err_text) = patch
                .payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.last_error = Some(err_text.to_string());
            }
            if let Some(next_retry_at_ms) = patch
                .payload
                .get("next_retry_at_ms")
                .and_then(Value::as_u64)
            {
                task.next_retry_at_ms = Some(next_retry_at_ms);
            }
            if let Some(next_attempt) = patch.payload.get("attempt").and_then(Value::as_u64) {
                task.attempt = next_attempt as u32;
            }
            if let Some(artifact_ids) = patch.payload.get("artifact_ids").and_then(Value::as_array)
            {
                task.artifact_ids = artifact_ids
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect();
            }
            if let Some(next_rev) = patch.payload.get("task_rev").and_then(Value::as_u64) {
                task.task_rev = next_rev;
            } else {
                task.task_rev = task.task_rev.saturating_add(1);
            }
            task.updated_ts = patch.ts_ms;
        }
    }
    blackboard.revision = patch.seq;
    Ok(())
}

pub(super) fn load_context_blackboard_patches(
    state: &AppState,
    run_id: &str,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextBlackboardPatchRecord> {
    let path = context_run_blackboard_patches_path(state, run_id);
    let rows = std::fs::read_to_string(path).unwrap_or_default();
    let mut parsed = rows
        .lines()
        .filter_map(|line| serde_json::from_str::<ContextBlackboardPatchRecord>(line).ok())
        .filter(|row| {
            if let Some(min_seq) = since_seq {
                return row.seq > min_seq;
            }
            true
        })
        .collect::<Vec<_>>();
    parsed.sort_by_key(|row| row.seq);
    if let Some(tail_count) = tail {
        if parsed.len() > tail_count {
            parsed = parsed.split_off(parsed.len().saturating_sub(tail_count));
        }
    }
    parsed
}

pub(super) fn append_context_blackboard_patch_record(
    state: &AppState,
    run_id: &str,
    seq: u64,
    op: ContextBlackboardPatchOp,
    payload: Value,
) -> Result<ContextBlackboardPatchRecord, StatusCode> {
    let patch = ContextBlackboardPatchRecord {
        patch_id: format!("bbp-{}", Uuid::new_v4()),
        run_id: run_id.to_string(),
        seq,
        ts_ms: crate::now_ms(),
        source_event_seq: Some(seq),
        op,
        payload,
    };
    append_jsonl_line(
        &context_run_blackboard_patches_path(state, run_id),
        &serde_json::to_value(&patch).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    )?;
    let mut blackboard = load_projected_context_blackboard(state, run_id);
    apply_context_blackboard_patch(&mut blackboard, &patch)?;
    save_context_blackboard(state, run_id, &blackboard)?;
    if let Some(workspace) = load_context_run_workspace_sync(state, run_id) {
        let envelope = ContextRunsStreamEnvelope {
            kind: "blackboard_patch".to_string(),
            run_id: run_id.to_string(),
            workspace,
            seq: patch.seq,
            ts_ms: patch.ts_ms,
            payload: serde_json::to_value(&patch).unwrap_or_else(|_| json!({})),
        };
        publish_context_run_stream_envelope(state, &envelope);
    }
    Ok(patch)
}

pub(super) fn context_task_status_event_name(status: &ContextBlackboardTaskStatus) -> &'static str {
    match status {
        ContextBlackboardTaskStatus::InProgress => "context.task.started",
        ContextBlackboardTaskStatus::Done => "context.task.completed",
        ContextBlackboardTaskStatus::Failed => "context.task.failed",
        ContextBlackboardTaskStatus::Blocked => "context.task.blocked",
        ContextBlackboardTaskStatus::Runnable | ContextBlackboardTaskStatus::Pending => {
            "context.task.requeued"
        }
    }
}

fn context_task_status_transition_allowed(
    current: &ContextBlackboardTaskStatus,
    next: &ContextBlackboardTaskStatus,
) -> bool {
    use ContextBlackboardTaskStatus as Status;

    if current == next {
        return true;
    }

    matches!(
        (current, next),
        (Status::Pending, Status::Runnable)
            | (Status::Pending, Status::Blocked)
            | (Status::Runnable, Status::Blocked)
            | (Status::InProgress, Status::Blocked)
            | (Status::InProgress, Status::Done)
            | (Status::InProgress, Status::Failed)
            | (Status::InProgress, Status::Runnable)
            | (Status::Blocked, Status::Runnable)
            | (Status::Blocked, Status::Failed)
            | (Status::Failed, Status::Runnable)
    )
}

fn context_task_transition_requires_valid_lease(
    current: &ContextBlackboardTask,
    action: &str,
    next_status: Option<&ContextBlackboardTaskStatus>,
) -> bool {
    if current.lease_token.is_none() {
        return false;
    }

    if action == "heartbeat" || action == "complete" || action == "fail" || action == "release" {
        return true;
    }

    action == "status"
        && matches!(
            next_status,
            Some(ContextBlackboardTaskStatus::InProgress)
                | Some(ContextBlackboardTaskStatus::Done)
                | Some(ContextBlackboardTaskStatus::Failed)
                | Some(ContextBlackboardTaskStatus::Blocked)
        )
}

fn context_task_retry_window_open(task: &ContextBlackboardTask, now: u64) -> bool {
    task.next_retry_at_ms
        .map(|retry_at| retry_at <= now)
        .unwrap_or(true)
}

fn context_task_dependencies_satisfied(
    run: &ContextRunState,
    task: &ContextBlackboardTask,
) -> bool {
    task.depends_on_task_ids.iter().all(|dep| {
        run.tasks
            .iter()
            .find(|row| row.id == *dep)
            .map(|row| matches!(row.status, ContextBlackboardTaskStatus::Done))
            .unwrap_or(false)
    })
}

fn context_task_matches_filters(
    task: &ContextBlackboardTask,
    task_type: Option<&str>,
    workflow_id: Option<&str>,
) -> bool {
    if let Some(task_type) = task_type {
        if task.task_type != task_type {
            return false;
        }
    }
    if let Some(workflow_id) = workflow_id {
        if task.workflow_id.as_deref() != Some(workflow_id) {
            return false;
        }
    }
    true
}

fn context_task_is_claimable(
    run: &ContextRunState,
    task: &ContextBlackboardTask,
    now: u64,
    task_type: Option<&str>,
    workflow_id: Option<&str>,
) -> bool {
    context_task_matches_filters(task, task_type, workflow_id)
        && matches!(
            task.status,
            ContextBlackboardTaskStatus::Runnable | ContextBlackboardTaskStatus::Pending
        )
        && context_task_dependencies_satisfied(run, task)
        && context_task_retry_window_open(task, now)
}

fn context_task_has_stale_lease(task: &ContextBlackboardTask, now: u64) -> bool {
    matches!(task.status, ContextBlackboardTaskStatus::InProgress)
        && task
            .lease_expires_at_ms
            .map(|lease_expires_at_ms| lease_expires_at_ms <= now)
            .unwrap_or(false)
}

async fn requeue_stale_context_tasks_locked(
    state: &AppState,
    run_id: &str,
    run_status: ContextRunStatus,
    run: &ContextRunState,
) -> Result<Vec<ContextBlackboardTask>, StatusCode> {
    let now = crate::now_ms();
    let stale_tasks = run
        .tasks
        .iter()
        .filter(|task| context_task_has_stale_lease(task, now))
        .cloned()
        .collect::<Vec<_>>();
    if stale_tasks.is_empty() {
        return Ok(Vec::new());
    }

    let mut requeued = Vec::with_capacity(stale_tasks.len());
    for current in stale_tasks {
        let next_rev = current.task_rev.saturating_add(1);
        let lease_owner = current
            .lease_owner
            .clone()
            .or(current.assigned_agent.clone())
            .unwrap_or_else(|| "unknown-agent".to_string());
        let detail = format!("task lease expired while assigned to {lease_owner}");
        let requeued_task = ContextBlackboardTask {
            status: ContextBlackboardTaskStatus::Runnable,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            last_error: Some(detail.clone()),
            next_retry_at_ms: Some(now),
            task_rev: next_rev,
            updated_ts: now,
            ..current.clone()
        };
        context_run_engine()
            .commit_task_mutation(
                state,
                run_id,
                requeued_task.clone(),
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": current.id,
                    "status": ContextBlackboardTaskStatus::Runnable,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "task_rev": next_rev,
                    "error": detail,
                    "attempt": current.attempt,
                    "next_retry_at_ms": now,
                    "stale_requeue": true,
                }),
                "context.task.stale_requeued".to_string(),
                run_status.clone(),
                None,
                json!({
                    "task_id": current.id,
                    "workflow_id": current.workflow_id,
                    "lease_owner": lease_owner,
                    "stale_requeue": true,
                }),
            )
            .await?;
        requeued.push(requeued_task);
    }

    Ok(requeued)
}

async fn commit_context_task_claim(
    state: &AppState,
    run_id: &str,
    run_status: ContextRunStatus,
    selected: &ContextBlackboardTask,
    agent_id: &str,
    lease_ms: Option<u64>,
    command_id: Option<String>,
) -> Result<ContextBlackboardTask, StatusCode> {
    let now = crate::now_ms();
    let lease_ms = lease_ms.unwrap_or(30_000).clamp(5_000, 300_000);
    let lease_token = format!("lease-{}", Uuid::new_v4());
    let next_rev = selected.task_rev.saturating_add(1);
    let mut payload = json!({
        "task_id": selected.id,
        "status": ContextBlackboardTaskStatus::InProgress,
        "assigned_agent": agent_id.trim(),
        "lease_owner": agent_id.trim(),
        "lease_token": lease_token,
        "lease_expires_at_ms": now.saturating_add(lease_ms),
        "task_rev": next_rev,
    });
    let claimed_task = ContextBlackboardTask {
        status: ContextBlackboardTaskStatus::InProgress,
        assigned_agent: Some(agent_id.trim().to_string()),
        lease_owner: Some(agent_id.trim().to_string()),
        lease_token: Some(lease_token.clone()),
        lease_expires_at_ms: Some(now.saturating_add(lease_ms)),
        task_rev: next_rev,
        updated_ts: now,
        ..selected.clone()
    };
    if let Some(command_id) = command_id.clone() {
        payload["command_id"] = json!(command_id);
    }
    context_run_engine()
        .commit_task_mutation(
            state,
            run_id,
            claimed_task.clone(),
            ContextBlackboardPatchOp::UpdateTaskState,
            payload,
            "context.task.claimed".to_string(),
            run_status,
            command_id,
            json!({
                "task_id": selected.id,
                "agent_id": agent_id.trim(),
                "task_rev": next_rev,
                "workflow_id": selected.workflow_id,
            }),
        )
        .await?;
    Ok(claimed_task)
}

pub(super) async fn context_run_lock_for(run_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<
        tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    let map = LOCKS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = map.lock().await;
    guard
        .entry(run_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

pub(super) async fn context_run_create(
    State(state): State<AppState>,
    Json(input): Json<ContextRunCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let run_id = input
        .run_id
        .unwrap_or_else(|| format!("run-{}", Uuid::new_v4()));
    ensure_context_run_dir(&state, &run_id).await?;
    let run_path = context_run_state_path(&state, &run_id);
    if run_path.exists() {
        return Ok(Json(json!({
            "error": "run already exists",
            "code": "CONTEXT_RUN_EXISTS",
            "run_id": run_id
        })));
    }
    let now = crate::now_ms();
    let run = ContextRunState {
        run_id: run_id.clone(),
        run_type: input.run_type.unwrap_or_else(|| "interactive".to_string()),
        source_client: input
            .source_client
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        model_provider: input
            .model_provider
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        model_id: input
            .model_id
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        mcp_servers: input
            .mcp_servers
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>(),
        status: ContextRunStatus::Queued,
        objective: input.objective,
        workspace: input.workspace.unwrap_or_default(),
        steps: Vec::new(),
        tasks: Vec::new(),
        why_next_step: None,
        revision: 1,
        last_event_seq: 0,
        created_at_ms: now,
        started_at_ms: None,
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(&state, &run).await?;
    Ok(Json(json!({ "ok": true, "run": run })))
}

pub(super) async fn context_run_list(
    State(state): State<AppState>,
    Query(query): Query<ContextRunListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let root = context_runs_root(&state);
    if !root.exists() {
        return Ok(Json(json!({ "runs": [] })));
    }
    let workspace_filter = query
        .workspace
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path);
    let run_type_filter = query
        .run_type
        .as_ref()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let mut rows = Vec::<ContextRunState>::new();
    let mut dir = tokio::fs::read_dir(root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let run_id = entry.file_name().to_string_lossy().to_string();
        if let Ok(run) = load_context_run_state(&state, &run_id).await {
            if let Some(workspace) = workspace_filter.as_deref() {
                if run.workspace.canonical_path.trim() != workspace {
                    continue;
                }
            }
            if let Some(run_type) = run_type_filter.as_deref() {
                if run.run_type.trim().to_ascii_lowercase() != run_type {
                    continue;
                }
            }
            rows.push(run);
        }
    }
    rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    rows.truncate(limit);
    Ok(Json(json!({ "runs": rows })))
}

pub(super) async fn context_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let run = load_context_run_state(&state, &run_id).await?;
    let ledger_summary =
        super::context_run_ledger::context_run_ledger_summary_for_run(&state, &run_id);
    let mutation_checkpoint_summary =
        super::context_run_mutation_checkpoints::context_run_mutation_checkpoint_summary_for_run(
            &state, &run_id,
        );
    let rollback_preview_summary =
        super::context_run_mutation_checkpoints::context_run_mutation_checkpoint_preview_summary_for_run(
            &state, &run_id,
        );
    let rollback_history_summary =
        super::context_run_mutation_checkpoints::context_run_mutation_checkpoint_rollback_history_summary_for_run(
            &state, &run_id,
        );
    let last_rollback_outcome =
        super::context_run_mutation_checkpoints::context_run_mutation_checkpoint_last_rollback_outcome_for_run(
            &state, &run_id,
        );
    let rollback_policy =
        super::context_run_mutation_checkpoints::context_run_mutation_checkpoint_rollback_policy_summary(
            &run,
        );
    Ok(Json(json!({
        "run": run,
        "ledger_summary": ledger_summary,
        "mutation_checkpoint_summary": mutation_checkpoint_summary,
        "rollback_preview_summary": rollback_preview_summary,
        "rollback_history_summary": rollback_history_summary,
        "last_rollback_outcome": last_rollback_outcome,
        "rollback_policy": rollback_policy,
    })))
}

pub(super) async fn context_run_put(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(run): Json<ContextRunState>,
) -> Result<Json<Value>, StatusCode> {
    if run.run_id != run_id {
        return Err(StatusCode::BAD_REQUEST);
    }
    let status = run.status.clone();
    let outcome = context_run_engine()
        .commit_snapshot_with_event(
            &state,
            &run_id,
            run.clone(),
            ContextRunEventAppendInput {
                event_type: "context.run.replaced".to_string(),
                status,
                step_id: None,
                payload: json!({ "run": run }),
            },
            None,
        )
        .await?;
    Ok(Json(json!({ "ok": true, "run": outcome.run })))
}

pub(super) fn context_run_is_terminal(status: &ContextRunStatus) -> bool {
    matches!(
        status,
        ContextRunStatus::Failed | ContextRunStatus::Completed | ContextRunStatus::Cancelled
    )
}

pub(super) fn context_step_status_from_todo(status: Option<&str>) -> ContextStepStatus {
    let normalized = status
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "pending".to_string());
    match normalized.as_str() {
        "in_progress" | "in-progress" | "working" | "doing" | "active" => {
            ContextStepStatus::InProgress
        }
        "runnable" | "ready" => ContextStepStatus::Runnable,
        "done" | "completed" | "complete" => ContextStepStatus::Done,
        "blocked" => ContextStepStatus::Blocked,
        "failed" | "error" => ContextStepStatus::Failed,
        _ => ContextStepStatus::Pending,
    }
}

pub(super) fn normalize_context_todo_step_id(raw: Option<&str>, idx: usize) -> String {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("todo-{}", idx.saturating_add(1)))
}

pub(super) async fn context_run_todos_sync(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTodoSyncInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    let replace = input.replace.unwrap_or(true);

    let mapped_steps = input
        .todos
        .iter()
        .enumerate()
        .map(|(idx, todo)| ContextRunStep {
            step_id: normalize_context_todo_step_id(todo.id.as_deref(), idx),
            title: todo.content.trim().to_string(),
            status: context_step_status_from_todo(todo.status.as_deref()),
        })
        .filter(|step| !step.title.is_empty())
        .collect::<Vec<_>>();

    if replace {
        run.steps = mapped_steps;
    } else {
        for step in mapped_steps {
            if let Some(existing) = run.steps.iter_mut().find(|row| row.step_id == step.step_id) {
                existing.title = step.title;
                existing.status = step.status;
            } else {
                run.steps.push(step);
            }
        }
    }

    if context_run_is_terminal(&run.status) {
        // keep terminal status unchanged
    } else if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::InProgress))
    {
        run.status = ContextRunStatus::Running;
    } else if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::Runnable))
    {
        run.status = ContextRunStatus::Planning;
    } else if run
        .steps
        .iter()
        .all(|step| matches!(step.status, ContextStepStatus::Done))
        && !run.steps.is_empty()
    {
        run.status = ContextRunStatus::Completed;
    } else if !run.steps.is_empty() {
        run.status = ContextRunStatus::Planning;
    }

    run.why_next_step = run
        .steps
        .iter()
        .find(|step| {
            matches!(
                step.status,
                ContextStepStatus::InProgress
                    | ContextStepStatus::Runnable
                    | ContextStepStatus::Pending
            )
        })
        .map(|step| format!("continue task `{}` from synced todo list", step.step_id));

    let event_status = run.status.clone();
    let event_payload = json!({
        "count": run.steps.len(),
        "replace": replace,
        "source_session_id": input.source_session_id,
        "source_run_id": input.source_run_id,
        "todos": input.todos,
        "why_next_step": run.why_next_step.clone(),
        "steps": run.steps,
        "run": run,
    });
    let outcome = context_run_engine()
        .commit_snapshot_with_event(
            &state,
            &run_id,
            serde_json::from_value(event_payload.get("run").cloned().unwrap_or(Value::Null))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            ContextRunEventAppendInput {
                event_type: "todo_synced".to_string(),
                status: event_status,
                step_id: None,
                payload: event_payload,
            },
            None,
        )
        .await?;

    Ok(Json(json!({
        "ok": true,
        "run": outcome.run
    })))
}

pub(super) fn context_driver_select_next_step(
    run: &ContextRunState,
) -> (Option<usize>, String, ContextRunStatus) {
    if context_run_is_terminal(&run.status) {
        return (
            None,
            format!(
                "run is terminal (`{}`); no next step can be selected",
                serde_json::to_string(&run.status).unwrap_or_else(|_| "\"terminal\"".to_string())
            ),
            run.status.clone(),
        );
    }
    if let Some(step) = run
        .steps
        .iter()
        .find(|step| matches!(step.status, ContextStepStatus::InProgress))
    {
        return (
            None,
            format!(
                "step `{}` is already in_progress; keep current execution focus",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Runnable))
    {
        return (
            Some(idx),
            format!(
                "selected runnable step `{}` as next execution target",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Pending))
    {
        return (
            Some(idx),
            format!(
                "no runnable step available; promoted pending step `{}` for execution",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if !run.steps.is_empty()
        && run
            .steps
            .iter()
            .all(|step| matches!(step.status, ContextStepStatus::Done))
    {
        return (
            None,
            "all steps are done; marking run completed".to_string(),
            ContextRunStatus::Completed,
        );
    }
    if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::Failed))
    {
        return (
            None,
            "one or more steps failed and no runnable work remains; run is blocked".to_string(),
            ContextRunStatus::Blocked,
        );
    }
    (
        None,
        "no actionable steps found; run remains blocked".to_string(),
        ContextRunStatus::Blocked,
    )
}

pub(super) async fn context_run_driver_next(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextDriverNextInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    let dry_run = input.dry_run.unwrap_or(false);
    let (selected_idx, why_next_step, target_status) = context_driver_select_next_step(&run);

    let selected_step_id = selected_idx.map(|idx| run.steps[idx].step_id.clone());
    let selected_step_status = selected_idx.map(|idx| {
        if matches!(run.steps[idx].status, ContextStepStatus::Pending) {
            "pending"
        } else {
            "runnable"
        }
    });

    if !dry_run {
        if let Some(idx) = selected_idx {
            if matches!(run.steps[idx].status, ContextStepStatus::Pending) {
                run.steps[idx].status = ContextStepStatus::Runnable;
            }
            run.steps[idx].status = ContextStepStatus::InProgress;
        }
        run.status = target_status.clone();
        run.why_next_step = Some(why_next_step.clone());
        let event_payload = json!({
            "why_next_step": why_next_step,
            "selected_step_id": selected_step_id,
            "selected_step_previous_status": selected_step_status,
            "driver": "context_driver_v1",
            "steps": run.steps,
            "run": run,
        });
        run = context_run_engine()
            .commit_snapshot_with_event(
                &state,
                &run_id,
                serde_json::from_value(event_payload.get("run").cloned().unwrap_or(Value::Null))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                ContextRunEventAppendInput {
                    event_type: "meta_next_step_selected".to_string(),
                    status: target_status.clone(),
                    step_id: selected_step_id.clone(),
                    payload: event_payload,
                },
                None,
            )
            .await?
            .run;
    }

    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "run_id": run_id,
        "selected_step_id": selected_step_id,
        "target_status": target_status,
        "why_next_step": why_next_step,
        "run": if dry_run { serde_json::to_value(&run).unwrap_or_else(|_| json!(null)) } else { serde_json::to_value(&run).unwrap_or_else(|_| json!(null)) }
    })))
}

pub(super) fn context_event_payload_text(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

pub(super) fn context_event_step_status(payload: &Value) -> Option<ContextStepStatus> {
    let text = context_event_payload_text(payload, "step_status")
        .or_else(|| context_event_payload_text(payload, "stepStatus"))
        .or_else(|| context_event_payload_text(payload, "step-state"))?;
    parse_context_step_status_text(&text)
}

pub(super) fn materialize_plan_steps_from_objective(objective: &str) -> Vec<ContextRunStep> {
    let mut steps = objective
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let normalized = line
                .trim_start_matches(|c: char| {
                    matches!(c, '-' | '*' | '#' | '0'..='9' | '.' | ')' | '[' | ']')
                })
                .trim();
            if normalized.len() < 8 {
                None
            } else {
                Some(normalized.to_string())
            }
        })
        .take(6)
        .enumerate()
        .map(|(idx, title)| ContextRunStep {
            step_id: format!("step-{}", idx.saturating_add(1)),
            title,
            status: ContextStepStatus::Pending,
        })
        .collect::<Vec<_>>();

    if steps.is_empty() {
        steps = vec![
            ContextRunStep {
                step_id: "step-1".to_string(),
                title: "Plan implementation approach".to_string(),
                status: ContextStepStatus::Pending,
            },
            ContextRunStep {
                step_id: "step-2".to_string(),
                title: "Execute implementation and produce artifacts".to_string(),
                status: ContextStepStatus::Pending,
            },
            ContextRunStep {
                step_id: "step-3".to_string(),
                title: "Validate and finalize output".to_string(),
                status: ContextStepStatus::Pending,
            },
        ];
    }
    steps
}

pub(super) fn apply_context_event_transition(
    run: &mut ContextRunState,
    input: &ContextRunEventAppendInput,
) {
    let event_type = input.event_type.trim().to_ascii_lowercase();
    let event_step_id = input
        .step_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    match event_type.as_str() {
        "session_run_started" => {
            run.status = ContextRunStatus::Running;
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
            let target_step_id = event_step_id
                .clone()
                .unwrap_or_else(|| "session-run".to_string());
            if let Some(step) = run
                .steps
                .iter_mut()
                .find(|step| step.step_id == target_step_id)
            {
                step.status = ContextStepStatus::InProgress;
            }
            run.why_next_step = context_event_payload_text(&input.payload, "why_next_step")
                .or_else(|| Some("session run in progress".to_string()));
        }
        "session_run_finished" => {
            run.status = input.status.clone();
            let target_step_id = event_step_id
                .clone()
                .unwrap_or_else(|| "session-run".to_string());
            if let Some(step) = run
                .steps
                .iter_mut()
                .find(|step| step.step_id == target_step_id)
            {
                step.status = match &input.status {
                    ContextRunStatus::Completed => ContextStepStatus::Done,
                    ContextRunStatus::Cancelled
                    | ContextRunStatus::Blocked
                    | ContextRunStatus::Paused => ContextStepStatus::Blocked,
                    ContextRunStatus::Failed => ContextStepStatus::Failed,
                    _ => ContextStepStatus::Done,
                };
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
            run.why_next_step = context_event_payload_text(&input.payload, "why_next_step")
                .or_else(|| Some("session run finished".to_string()));
        }
        "planning_started" => {
            if run.steps.is_empty() {
                run.steps = materialize_plan_steps_from_objective(&run.objective);
            }
            run.status = ContextRunStatus::AwaitingApproval;
            run.why_next_step = Some("plan generated and awaiting approval".to_string());
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
        }
        "plan_approved" => {
            run.status = ContextRunStatus::Running;
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
            if let Some(step) = run.steps.iter_mut().find(|step| {
                matches!(
                    step.status,
                    ContextStepStatus::Pending | ContextStepStatus::Runnable
                )
            }) {
                step.status = ContextStepStatus::InProgress;
                run.why_next_step =
                    Some(format!("executing step `{}` after approval", step.step_id));
            }
        }
        "run_paused" => {
            run.status = ContextRunStatus::Paused;
            run.why_next_step = Some("run paused by operator".to_string());
        }
        "run_resumed" => {
            run.status = ContextRunStatus::Running;
            run.why_next_step = Some("run resumed by operator".to_string());
            run.ended_at_ms = None;
        }
        "run_cancelled" => {
            run.status = ContextRunStatus::Cancelled;
            run.why_next_step = Some("run cancelled by operator".to_string());
            run.ended_at_ms = Some(crate::now_ms());
        }
        "task_retry_requested" => {
            if let Some(step_id) = input
                .step_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = ContextStepStatus::Runnable;
                }
                run.why_next_step = Some(format!("retry requested for step `{step_id}`"));
            }
            run.status = ContextRunStatus::Running;
            run.ended_at_ms = None;
            run.last_error = None;
        }
        "revision_requested" => {
            run.status = ContextRunStatus::Blocked;
            let feedback =
                context_event_payload_text(&input.payload, "feedback").unwrap_or_default();
            run.why_next_step = Some(if feedback.is_empty() {
                "revision requested by operator".to_string()
            } else {
                format!("revision requested: {feedback}")
            });
        }
        "step_started" | "task_started" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::InProgress);
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        "step_completed" | "task_completed" | "step_done" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::Done);
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        "step_failed" | "task_failed" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::Failed);
                }
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        _ => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step_status) = context_event_step_status(&input.payload) {
                    if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                        step.status = step_status;
                    }
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
        }
    }

    if matches!(
        run.status,
        ContextRunStatus::Completed | ContextRunStatus::Failed | ContextRunStatus::Cancelled
    ) {
        run.ended_at_ms = Some(crate::now_ms());
    }
}

pub(super) async fn context_run_event_append(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextRunEventAppendInput>,
) -> Result<Json<Value>, StatusCode> {
    if input.event_type.trim().starts_with("context.task.") {
        return Ok(Json(json!({
            "ok": false,
            "error": "task events must go through the run task endpoints",
            "code": "TASK_EVENT_APPEND_DISABLED"
        })));
    }
    let outcome = context_run_engine()
        .commit_run_event(&state, &run_id, input, None)
        .await?;
    Ok(Json(json!({ "ok": true, "event": outcome.event })))
}

pub(crate) async fn append_context_run_event(
    state: &AppState,
    run_id: &str,
    input: ContextRunEventAppendInput,
) -> Result<(), StatusCode> {
    let _ = context_run_engine()
        .commit_run_event(state, run_id, input, None)
        .await?;
    Ok(())
}

pub(super) async fn context_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let rows = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        query.since_seq,
        query.tail,
    );
    Ok(Json(json!({ "events": rows })))
}

pub(super) fn context_run_events_sse_stream(
    state: AppState,
    run_id: String,
    query: super::RunEventsQuery,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    tokio::spawn(async move {
        let events_path = context_run_events_path(&state, &run_id);
        let ready = serde_json::to_string(&json!({
            "status":"ready",
            "stream":"context_run_events",
            "runID": run_id,
            "timestamp_ms": crate::now_ms(),
            "path": events_path.to_string_lossy().to_string(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let initial = load_context_run_events_jsonl(&events_path, query.since_seq, query.tail);
        let mut last_seq = query.since_seq.unwrap_or(0);
        for row in initial {
            last_seq = last_seq.max(row.seq);
            let payload = serde_json::to_string(&json!({
                "type":"run.event",
                "properties": row
            }))
            .unwrap_or_default();
            if tx.send(payload).await.is_err() {
                return;
            }
        }

        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let updates = load_context_run_events_jsonl(&events_path, Some(last_seq), None);
            for row in updates {
                last_seq = last_seq.max(row.seq);
                let payload = serde_json::to_string(&json!({
                    "type":"run.event",
                    "properties": row
                }))
                .unwrap_or_default();
                if tx.send(payload).await.is_err() {
                    return;
                }
            }
        }
    });
    ReceiverStream::new(rx).map(|payload| Ok(Event::default().data(payload)))
}

pub(super) async fn context_run_events_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(context_run_events_sse_stream(state, run_id, query))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn context_runs_events_multiplex_sse_stream(
    state: AppState,
    workspace: String,
    subscribed_run_ids: Vec<String>,
    cursor: ContextRunsStreamCursor,
    tail: Option<usize>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(512);
    tokio::spawn(async move {
        let subscribed_set: HashSet<String> = subscribed_run_ids.iter().cloned().collect();
        let ready = serde_json::to_string(&json!({
            "kind":"ready",
            "workspace": workspace,
            "subscribed_run_ids": subscribed_run_ids,
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let mut replay = Vec::<ContextRunsStreamEnvelope>::new();
        for run_id in &subscribed_set {
            let run_events = load_context_run_events_jsonl(
                &context_run_events_path(&state, run_id),
                cursor.events.get(run_id).copied(),
                if cursor.events.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for row in run_events {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "context_run_event".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: row.seq,
                    ts_ms: row.ts_ms,
                    payload: serde_json::to_value(row).unwrap_or_else(|_| json!({})),
                });
            }
            let run_patches = load_context_blackboard_patches(
                &state,
                run_id,
                cursor.patches.get(run_id).copied(),
                if cursor.patches.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for patch in run_patches {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "blackboard_patch".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: patch.seq,
                    ts_ms: patch.ts_ms,
                    payload: serde_json::to_value(patch).unwrap_or_else(|_| json!({})),
                });
            }
        }
        replay.sort_by(|a, b| {
            a.ts_ms
                .cmp(&b.ts_ms)
                .then_with(|| a.run_id.cmp(&b.run_id))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.seq.cmp(&b.seq))
        });
        for row in replay {
            let payload = serde_json::to_string(&row).unwrap_or_default();
            if tx.send(payload).await.is_err() {
                return;
            }
        }

        let mut live = state.event_bus.subscribe();
        loop {
            match live.recv().await {
                Ok(event) => {
                    if event.event_type != "context.run.stream" {
                        continue;
                    }
                    let run_id = event
                        .properties
                        .get("run_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if run_id.is_empty() || !subscribed_set.contains(run_id) {
                        continue;
                    }
                    let event_workspace = event
                        .properties
                        .get("workspace")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if event_workspace != workspace {
                        continue;
                    }
                    let payload = serde_json::to_string(&event.properties).unwrap_or_default();
                    if tx.send(payload).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    ReceiverStream::new(rx).map(|payload| Ok(Event::default().data(payload)))
}

pub(super) async fn context_runs_events_stream(
    State(state): State<AppState>,
    Query(query): Query<ContextRunsEventsStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    let workspace = query
        .workspace
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let requested = parse_context_run_ids_csv(query.run_ids.as_deref());
    let subscribed_run_ids = if requested.is_empty() {
        list_context_runs_for_workspace(&state, &workspace, 1000)
            .await?
            .into_iter()
            .map(|run| run.run_id)
            .collect::<Vec<_>>()
    } else {
        let mut accepted = Vec::<String>::new();
        for run_id in requested {
            let run = load_context_run_state(&state, &run_id)
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            if run.workspace.canonical_path.trim() != workspace {
                return Err(StatusCode::BAD_REQUEST);
            }
            accepted.push(run_id);
        }
        accepted.sort();
        accepted.dedup();
        accepted
    };
    let cursor = decode_context_stream_cursor(query.cursor.as_deref());
    let tail = query.tail.map(|value| value.clamp(1, 2000));
    Ok(Sse::new(context_runs_events_multiplex_sse_stream(
        state,
        workspace,
        subscribed_run_ids,
        cursor,
        tail,
    ))
    .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10))))
}

pub(super) async fn context_run_lease_validate(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextLeaseValidateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    if run.workspace.canonical_path.trim().is_empty() {
        run.workspace.canonical_path = input.current_path.clone();
        if run.workspace.lease_epoch == 0 {
            run.workspace.lease_epoch = 1;
        }
        let _ = context_run_engine()
            .commit_snapshot_with_event(
                &state,
                &run_id,
                run.clone(),
                ContextRunEventAppendInput {
                    event_type: "workspace_validated".to_string(),
                    status: run.status.clone(),
                    step_id: input.step_id.clone(),
                    payload: json!({
                        "phase": input.phase,
                        "expected_path": run.workspace.canonical_path,
                        "actual_path": input.current_path,
                        "run": run,
                    }),
                },
                None,
            )
            .await?;
        return Ok(Json(json!({ "ok": true, "mismatch": false })));
    }

    if run.workspace.canonical_path != input.current_path {
        run.status = ContextRunStatus::Paused;
        let _ = context_run_engine()
            .commit_snapshot_with_event(
                &state,
                &run_id,
                run.clone(),
                ContextRunEventAppendInput {
                    event_type: "workspace_mismatch".to_string(),
                    status: ContextRunStatus::Paused,
                    step_id: input.step_id.clone(),
                    payload: json!({
                        "phase": input.phase,
                        "expected_path": run.workspace.canonical_path,
                        "actual_path": input.current_path,
                        "run": run,
                    }),
                },
                None,
            )
            .await?;
        return Ok(Json(json!({
            "ok": false,
            "mismatch": true,
            "status": "paused"
        })));
    }

    Ok(Json(json!({ "ok": true, "mismatch": false })))
}

pub(super) async fn context_run_blackboard_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let blackboard = load_projected_context_blackboard(&state, &run_id);
    Ok(Json(json!({ "blackboard": blackboard })))
}

pub(super) fn context_run_events_have_command_id(
    state: &AppState,
    run_id: &str,
    command_id: &str,
) -> bool {
    if command_id.trim().is_empty() {
        return false;
    }
    load_context_run_events_jsonl(&context_run_events_path(state, run_id), None, Some(500))
        .iter()
        .any(|event| event.command_id.as_deref() == Some(command_id))
}

pub(super) async fn context_run_blackboard_patches_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ContextBlackboardPatchesQuery>,
) -> Result<Json<Value>, StatusCode> {
    let patches = load_context_blackboard_patches(&state, &run_id, query.since_seq, query.tail);
    Ok(Json(json!({ "patches": patches })))
}

pub(super) async fn context_run_blackboard_patch(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextBlackboardPatchInput>,
) -> Result<Json<Value>, StatusCode> {
    if matches!(
        input.op,
        ContextBlackboardPatchOp::AddTask
            | ContextBlackboardPatchOp::UpdateTaskLease
            | ContextBlackboardPatchOp::UpdateTaskState
    ) {
        return Ok(Json(json!({
            "ok": false,
            "error": "task mutations must go through the run task endpoints",
            "code": "TASK_PATCH_OP_DISABLED"
        })));
    }
    let outcome = context_run_engine()
        .commit_blackboard_patch(&state, &run_id, input.op, input.payload)
        .await?;
    Ok(Json(json!({
        "ok": true,
        "patch": outcome.patch,
        "blackboard": outcome.blackboard
    })))
}

pub(super) async fn context_run_tasks_create(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTaskCreateBatchInput>,
) -> Result<Json<Value>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    if input.tasks.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let run_status = load_context_run_state(&state, &run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Planning);
    let mut created = Vec::<ContextBlackboardTask>::new();
    let mut patches = Vec::<ContextBlackboardPatchRecord>::new();

    for row in input.tasks {
        let command_id = row.command_id.clone();
        if row.task_type.trim().is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if let Some(command_id) = command_id.as_deref() {
            if context_run_events_have_command_id(&state, &run_id, command_id) {
                continue;
            }
        }
        let now = crate::now_ms();
        let normalized_contract = match normalize_context_task_payload(&row.task_type, &row.payload)
        {
            Ok(value) => value,
            Err((code, error)) => {
                return Ok(Json(json!({
                    "ok": false,
                    "code": code,
                    "error": error,
                    "task_id": row.id.clone().unwrap_or_default(),
                })));
            }
        };
        let (task_type, payload) = normalized_contract
            .unwrap_or_else(|| (row.task_type.trim().to_string(), row.payload.clone()));
        let task = ContextBlackboardTask {
            id: row
                .id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("task-{}", Uuid::new_v4())),
            task_type,
            payload,
            status: row.status.unwrap_or(ContextBlackboardTaskStatus::Pending),
            workflow_id: row
                .workflow_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            workflow_node_id: row
                .workflow_node_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            parent_task_id: row
                .parent_task_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            depends_on_task_ids: row
                .depends_on_task_ids
                .into_iter()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>(),
            decision_ids: row.decision_ids,
            artifact_ids: row.artifact_ids,
            assigned_agent: None,
            priority: row.priority.unwrap_or(0),
            attempt: 0,
            max_attempts: row.max_attempts.unwrap_or(3),
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        };
        let mut payload =
            serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(command_id) = command_id.clone() {
            payload["command_id"] = json!(command_id);
        }
        let event_payload = json!({
            "task_id": task.id,
            "task_type": task.task_type,
            "task_rev": task.task_rev,
            "workflow_id": task.workflow_id,
        });
        let outcome = context_run_engine()
            .commit_task_mutation(
                &state,
                &run_id,
                task.clone(),
                ContextBlackboardPatchOp::AddTask,
                payload,
                "context.task.created".to_string(),
                run_status.clone(),
                command_id,
                event_payload,
            )
            .await?;
        if let Some(patch) = outcome.patch {
            patches.push(patch);
        }
        created.push(task);
    }

    let blackboard = load_projected_context_blackboard(&state, &run_id);
    Ok(Json(
        json!({ "ok": true, "tasks": created, "patches": patches, "blackboard": blackboard }),
    ))
}

pub(super) async fn context_run_tasks_claim(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTaskClaimInput>,
) -> Result<Json<Value>, StatusCode> {
    let task = claim_next_context_task(
        &state,
        &run_id,
        &input.agent_id,
        input.task_type.as_deref(),
        input.workflow_id.as_deref(),
        input.lease_ms,
        input.command_id.clone(),
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "blackboard": load_projected_context_blackboard(&state, &run_id),
    })))
}

pub(super) async fn claim_next_context_task(
    state: &AppState,
    run_id: &str,
    agent_id: &str,
    task_type: Option<&str>,
    workflow_id: Option<&str>,
    lease_ms: Option<u64>,
    command_id: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    if agent_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let _ = requeue_stale_context_tasks_locked(state, run_id, run_status.clone(), &run).await?;
    let run = load_context_run_state(state, run_id).await?;
    let now = crate::now_ms();
    let mut task_idx = run
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| context_task_is_claimable(&run, task, now, task_type, workflow_id))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    task_idx.sort_by(|a, b| {
        let left = &run.tasks[*a];
        let right = &run.tasks[*b];
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_ts.cmp(&right.created_ts))
    });
    let Some(selected_idx) = task_idx.first().copied() else {
        return Ok(None);
    };
    let selected = run.tasks[selected_idx].clone();
    let claimed_task = commit_context_task_claim(
        state, run_id, run_status, &selected, agent_id, lease_ms, command_id,
    )
    .await?;
    Ok(Some(claimed_task))
}

pub(super) async fn claim_context_task_by_id(
    state: &AppState,
    run_id: &str,
    task_id: &str,
    agent_id: &str,
    lease_ms: Option<u64>,
    command_id: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(run_id).await;
    let _guard = lock.lock().await;
    if agent_id.trim().is_empty() || task_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let _ = requeue_stale_context_tasks_locked(state, run_id, run_status.clone(), &run).await?;
    let run = load_context_run_state(state, run_id).await?;
    let now = crate::now_ms();
    let Some(selected) = run.tasks.iter().find(|task| task.id == task_id).cloned() else {
        return Ok(None);
    };
    if !context_task_is_claimable(&run, &selected, now, None, None) {
        return Ok(None);
    }
    let claimed_task = commit_context_task_claim(
        state, run_id, run_status, &selected, agent_id, lease_ms, command_id,
    )
    .await?;
    Ok(Some(claimed_task))
}

pub(super) async fn requeue_context_task_by_id(
    state: &AppState,
    run_id: &str,
    task_id: &str,
    command_id: Option<String>,
    detail: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(run_id).await;
    let _guard = lock.lock().await;
    if task_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let Some(current) = run.tasks.iter().find(|task| task.id == task_id).cloned() else {
        return Ok(None);
    };
    if matches!(current.status, ContextBlackboardTaskStatus::Done) {
        return Ok(None);
    }

    let now = crate::now_ms();
    let next_rev = current.task_rev.saturating_add(1);
    let next_task = ContextBlackboardTask {
        status: ContextBlackboardTaskStatus::Runnable,
        lease_owner: None,
        lease_token: None,
        lease_expires_at_ms: None,
        next_retry_at_ms: Some(now),
        task_rev: next_rev,
        updated_ts: now,
        last_error: detail.clone().or(current.last_error.clone()),
        ..current.clone()
    };
    let mut payload = json!({
        "task_id": current.id,
        "status": ContextBlackboardTaskStatus::Runnable,
        "lease_owner": Value::Null,
        "lease_token": Value::Null,
        "lease_expires_at_ms": Value::Null,
        "task_rev": next_rev,
        "attempt": current.attempt,
        "next_retry_at_ms": now,
    });
    if let Some(detail) = detail.clone() {
        payload["error"] = json!(detail);
    }
    if let Some(command_id) = command_id.clone() {
        payload["command_id"] = json!(command_id);
    }
    context_run_engine()
        .commit_task_mutation(
            state,
            run_id,
            next_task.clone(),
            ContextBlackboardPatchOp::UpdateTaskState,
            payload,
            "context.task.requeued".to_string(),
            run_status,
            command_id,
            json!({
                "task_id": current.id,
                "workflow_id": current.workflow_id,
                "manual_requeue": true,
            }),
        )
        .await?;
    Ok(Some(next_task))
}

pub(super) async fn context_run_task_transition(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<ContextTaskTransitionInput>,
) -> Result<Json<Value>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    let command_id = input.command_id.clone();
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(&state, &run_id, command_id) {
            let run = load_context_run_state(&state, &run_id).await.ok();
            let blackboard = load_projected_context_blackboard(&state, &run_id);
            return Ok(Json(json!({
                "ok": true,
                "deduped": true,
                "task": run
                    .as_ref()
                    .map(|row| row.tasks.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .find(|row| row.id == task_id),
                "blackboard": blackboard,
            })));
        }
    }
    let run_status = load_context_run_state(&state, &run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(&state, &run_id).await?;
    let Some(current) = run.tasks.iter().find(|row| row.id == task_id).cloned() else {
        return Err(StatusCode::NOT_FOUND);
    };
    if let Some(expected_rev) = input.expected_task_rev {
        if expected_rev != current.task_rev {
            return Ok(Json(json!({
                "ok": false,
                "error": "task revision mismatch",
                "code": "TASK_REV_MISMATCH",
                "task_rev": current.task_rev
            })));
        }
    }
    let action = input.action.trim().to_ascii_lowercase();
    let requested_status = input.status.as_ref();
    if context_task_transition_requires_valid_lease(&current, &action, requested_status) {
        let Some(token) = input.lease_token.as_deref() else {
            return Ok(Json(json!({
                "ok": false,
                "error": "lease token required",
                "code": "TASK_LEASE_REQUIRED"
            })));
        };
        if current.lease_token.as_deref() != Some(token) {
            return Ok(Json(json!({
                "ok": false,
                "error": "invalid lease token",
                "code": "TASK_LEASE_INVALID"
            })));
        }
    }
    match action.as_str() {
        "heartbeat" if current.status != ContextBlackboardTaskStatus::InProgress => {
            return Ok(Json(json!({
                "ok": false,
                "error": "heartbeat requires an in_progress task",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "status" => {
            let Some(next_status) = requested_status else {
                return Err(StatusCode::BAD_REQUEST);
            };
            if !context_task_status_transition_allowed(&current.status, next_status) {
                return Ok(Json(json!({
                    "ok": false,
                    "error": "invalid task status transition",
                    "code": "TASK_TRANSITION_INVALID",
                    "current_status": current.status.clone(),
                    "requested_status": next_status.clone(),
                })));
            }
        }
        "complete" if current.status != ContextBlackboardTaskStatus::InProgress => {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be in_progress before completion",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "fail"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::InProgress | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be active before failure is recorded",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "release"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::InProgress | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be active before release",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "retry"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::Failed | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be failed or blocked before retry",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        _ => {}
    }
    let next_rev = current.task_rev.saturating_add(1);
    let now = crate::now_ms();
    let (op, mut payload, next_task) = match action.as_str() {
        "heartbeat" => {
            let lease_expires_at_ms =
                now.saturating_add(input.lease_ms.unwrap_or(30_000).clamp(5_000, 300_000));
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            let lease_owner = input.agent_id.clone().or(current.lease_owner.clone());
            let lease_token = input.lease_token.clone().or(current.lease_token.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskLease,
                json!({
                    "task_id": task_id,
                    "lease_owner": lease_owner.clone(),
                    "assigned_agent": assigned_agent.clone(),
                    "lease_token": lease_token.clone(),
                    "lease_expires_at_ms": lease_expires_at_ms,
                    "task_rev": next_rev,
                }),
                ContextBlackboardTask {
                    assigned_agent,
                    lease_owner,
                    lease_token,
                    lease_expires_at_ms: Some(lease_expires_at_ms),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "status" => {
            let status = input.status.clone().ok_or(StatusCode::BAD_REQUEST)?;
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": status.clone(),
                    "lease_owner": current.lease_owner.clone(),
                    "lease_token": current.lease_token.clone(),
                    "lease_expires_at_ms": current.lease_expires_at_ms,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": input.error.clone(),
                    "attempt": current.attempt,
                }),
                ContextBlackboardTask {
                    status,
                    assigned_agent,
                    last_error: input.error.clone().or(current.last_error.clone()),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "complete" => {
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Done,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "attempt": current.attempt,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Done,
                    assigned_agent,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "fail" => {
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            let last_error = input.error.clone().or(current.last_error.clone());
            let attempt = current.attempt.saturating_add(1);
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Failed,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": last_error.clone(),
                    "attempt": attempt,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Failed,
                    assigned_agent,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    last_error,
                    attempt,
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "release" => (
            ContextBlackboardPatchOp::UpdateTaskState,
            json!({
                "task_id": task_id,
                "status": ContextBlackboardTaskStatus::Runnable,
                "lease_owner": Value::Null,
                "lease_token": Value::Null,
                "lease_expires_at_ms": Value::Null,
                "assigned_agent": current.assigned_agent.clone(),
                "task_rev": next_rev,
                "attempt": current.attempt,
            }),
            ContextBlackboardTask {
                status: ContextBlackboardTaskStatus::Runnable,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: next_rev,
                updated_ts: now,
                ..current.clone()
            },
        ),
        "retry" => {
            let attempt = current.attempt.saturating_add(1);
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Runnable,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": current.assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": Value::Null,
                    "attempt": attempt,
                    "next_retry_at_ms": now,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Runnable,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    last_error: None,
                    attempt,
                    next_retry_at_ms: Some(now),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if let Some(command_id) = command_id.clone() {
        payload["command_id"] = json!(command_id);
    }
    let task = Some(next_task.clone());
    let (event_type, event_payload) = if action == "heartbeat" {
        (
            "context.task.heartbeat".to_string(),
            json!({
                "task_id": task_id,
                "task_rev": task.as_ref().map(|row| row.task_rev),
            }),
        )
    } else {
        let status = task
            .as_ref()
            .map(|row| row.status.clone())
            .unwrap_or(ContextBlackboardTaskStatus::Blocked);
        (
            context_task_status_event_name(&status).to_string(),
            json!({
                "task_id": task_id,
                "task_rev": task.as_ref().map(|row| row.task_rev),
                "status": status,
                "error": input.error,
            }),
        )
    };
    let outcome = context_run_engine()
        .commit_task_mutation(
            &state,
            &run_id,
            next_task.clone(),
            op,
            payload,
            event_type,
            run_status,
            command_id,
            event_payload,
        )
        .await?;
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "patch": outcome.patch,
        "blackboard": outcome.blackboard
    })))
}

pub(super) async fn context_run_checkpoint_create(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextCheckpointCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let run_state = load_context_run_state(&state, &run_id).await?;
    let blackboard = load_projected_context_blackboard(&state, &run_id);
    let events_path = context_run_events_path(&state, &run_id);
    let seq = latest_context_run_event_seq(&events_path);
    let checkpoint = ContextCheckpointRecord {
        checkpoint_id: format!("cp-{}", Uuid::new_v4()),
        run_id: run_id.clone(),
        seq,
        ts_ms: crate::now_ms(),
        reason: input.reason,
        run_state,
        blackboard,
    };
    let dir = context_run_checkpoints_dir(&state, &run_id);
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = dir.join(format!("{:020}.json", seq));
    let payload =
        serde_json::to_string_pretty(&checkpoint).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(path, payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true, "checkpoint": checkpoint })))
}

pub(super) async fn context_run_checkpoint_latest(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let dir = context_run_checkpoints_dir(&state, &run_id);
    if !dir.exists() {
        return Ok(Json(json!({ "checkpoint": null })));
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let Some(file_name) = latest_name else {
        return Ok(Json(json!({ "checkpoint": null })));
    };
    let raw = std::fs::read_to_string(dir.join(file_name))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let checkpoint = serde_json::from_str::<ContextCheckpointRecord>(&raw)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "checkpoint": checkpoint })))
}

pub(super) fn parse_context_step_status_text(value: &str) -> Option<ContextStepStatus> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => Some(ContextStepStatus::Pending),
        "runnable" => Some(ContextStepStatus::Runnable),
        "in_progress" | "in-progress" | "working" | "active" => Some(ContextStepStatus::InProgress),
        "blocked" => Some(ContextStepStatus::Blocked),
        "done" | "completed" | "complete" => Some(ContextStepStatus::Done),
        "failed" | "error" => Some(ContextStepStatus::Failed),
        _ => None,
    }
}

pub(super) fn replay_step_status_from_event(
    event: &ContextRunEventRecord,
) -> Option<ContextStepStatus> {
    if let Some(status_text) = event.payload.get("step_status").and_then(Value::as_str) {
        if let Some(status) = parse_context_step_status_text(status_text) {
            return Some(status);
        }
    }
    let ty = event.event_type.to_ascii_lowercase();
    if ty.contains("step_started") || ty.contains("step_running") {
        return Some(ContextStepStatus::InProgress);
    }
    if ty.contains("step_blocked") {
        return Some(ContextStepStatus::Blocked);
    }
    if ty.contains("step_failed") {
        return Some(ContextStepStatus::Failed);
    }
    if ty.contains("step_done") || ty.contains("step_completed") {
        return Some(ContextStepStatus::Done);
    }
    None
}

pub(super) fn latest_context_checkpoint_record(
    state: &AppState,
    run_id: &str,
) -> Option<ContextCheckpointRecord> {
    let dir = context_run_checkpoints_dir(state, run_id);
    if !dir.exists() {
        return None;
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).ok()?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .ok()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let file_name = latest_name?;
    let raw = std::fs::read_to_string(dir.join(file_name)).ok()?;
    serde_json::from_str::<ContextCheckpointRecord>(&raw).ok()
}

pub(super) fn context_run_replay_materialize(
    run_id: &str,
    persisted: &ContextRunState,
    checkpoint: Option<&ContextCheckpointRecord>,
    events: &[ContextRunEventRecord],
) -> ContextRunState {
    let mut replay = if let Some(cp) = checkpoint {
        cp.run_state.clone()
    } else {
        let mut base = persisted.clone();
        base.status = ContextRunStatus::Queued;
        base.why_next_step = None;
        base.revision = 1;
        base.updated_at_ms = base.created_at_ms;
        for step in &mut base.steps {
            step.status = ContextStepStatus::Pending;
        }
        base
    };
    replay.run_id = run_id.to_string();
    let mut step_index: HashMap<String, usize> = replay
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| (step.step_id.clone(), idx))
        .collect();

    for event in events {
        replay.status = event.status.clone();
        replay.updated_at_ms = replay.updated_at_ms.max(event.ts_ms);
        if let Some(why) = event.payload.get("why_next_step").and_then(Value::as_str) {
            if !why.trim().is_empty() {
                replay.why_next_step = Some(why.to_string());
            }
        }
        if let Some(items) = event.payload.get("steps").and_then(Value::as_array) {
            for item in items {
                let Some(step_id) = item.get("step_id").and_then(Value::as_str) else {
                    continue;
                };
                let step_title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(step_id)
                    .to_string();
                let step_status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .and_then(parse_context_step_status_text)
                    .unwrap_or(ContextStepStatus::Pending);
                let index = if let Some(existing) = step_index.get(step_id).copied() {
                    existing
                } else {
                    replay.steps.push(ContextRunStep {
                        step_id: step_id.to_string(),
                        title: step_title.clone(),
                        status: ContextStepStatus::Pending,
                    });
                    let idx = replay.steps.len().saturating_sub(1);
                    step_index.insert(step_id.to_string(), idx);
                    idx
                };
                replay.steps[index].title = step_title;
                replay.steps[index].status = step_status;
            }
        }
        if let Some(step_id) = &event.step_id {
            let index = if let Some(existing) = step_index.get(step_id).copied() {
                existing
            } else {
                replay.steps.push(ContextRunStep {
                    step_id: step_id.clone(),
                    title: event
                        .payload
                        .get("step_title")
                        .and_then(Value::as_str)
                        .unwrap_or(step_id.as_str())
                        .to_string(),
                    status: ContextStepStatus::Pending,
                });
                let idx = replay.steps.len().saturating_sub(1);
                step_index.insert(step_id.clone(), idx);
                idx
            };
            if let Some(step_status) = replay_step_status_from_event(event) {
                replay.steps[index].status = step_status;
            }
        }
    }

    let checkpoint_revision = checkpoint.map(|cp| cp.run_state.revision).unwrap_or(1);
    replay.revision = checkpoint_revision.saturating_add(events.len() as u64);
    replay
}

pub(super) fn context_blackboard_replay_materialize(
    persisted: &ContextBlackboardState,
    checkpoint: Option<&ContextCheckpointRecord>,
    patches: &[ContextBlackboardPatchRecord],
) -> ContextBlackboardState {
    let mut replay = checkpoint
        .map(|cp| cp.blackboard.clone())
        .unwrap_or_else(ContextBlackboardState::default);
    if checkpoint.is_none() && replay.revision == 0 && persisted.revision > 0 {
        replay.revision = 0;
    }
    for patch in patches {
        let _ = apply_context_blackboard_patch(&mut replay, patch);
    }
    replay.tasks = persisted.tasks.clone();
    replay
}

pub(super) async fn context_run_replay(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ContextRunReplayQuery>,
) -> Result<Json<Value>, StatusCode> {
    let persisted = load_context_run_state(&state, &run_id).await?;
    let from_checkpoint = query.from_checkpoint.unwrap_or(true);
    let checkpoint = if from_checkpoint {
        latest_context_checkpoint_record(&state, &run_id)
    } else {
        None
    };
    let start_seq = checkpoint.as_ref().map(|cp| cp.seq).unwrap_or(0);
    let mut events = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        Some(start_seq),
        None,
    );
    if let Some(upto_seq) = query.upto_seq {
        events.retain(|row| row.seq <= upto_seq);
    }
    let replay = context_run_replay_materialize(&run_id, &persisted, checkpoint.as_ref(), &events);
    let persisted_blackboard = load_projected_context_blackboard(&state, &run_id);
    let mut patches = load_context_blackboard_patches(&state, &run_id, None, None);
    if let Some(cp) = checkpoint.as_ref() {
        patches.retain(|patch| patch.ts_ms > cp.ts_ms);
    }
    let replay_blackboard =
        context_blackboard_replay_materialize(&persisted_blackboard, checkpoint.as_ref(), &patches);
    let blackboard_revision_mismatch = replay_blackboard.revision != persisted_blackboard.revision;
    let blackboard_task_count_mismatch =
        replay_blackboard.tasks.len() != persisted_blackboard.tasks.len();
    let mut replay_task_status = HashMap::<String, ContextBlackboardTaskStatus>::new();
    for task in &replay_blackboard.tasks {
        replay_task_status.insert(task.id.clone(), task.status.clone());
    }
    let blackboard_task_status_mismatch = persisted_blackboard.tasks.iter().any(|task| {
        replay_task_status
            .get(&task.id)
            .map(|status| status != &task.status)
            .unwrap_or(true)
    });
    let status_mismatch = replay.status != persisted.status;
    let why_next_step_mismatch =
        persisted.why_next_step.is_some() && replay.why_next_step != persisted.why_next_step;
    let step_count_mismatch =
        !persisted.steps.is_empty() && replay.steps.len() != persisted.steps.len();
    let drift = ContextReplayDrift {
        mismatch: status_mismatch
            || why_next_step_mismatch
            || step_count_mismatch
            || blackboard_revision_mismatch
            || blackboard_task_count_mismatch
            || blackboard_task_status_mismatch,
        status_mismatch,
        why_next_step_mismatch,
        step_count_mismatch,
        blackboard_revision_mismatch,
        blackboard_task_count_mismatch,
        blackboard_task_status_mismatch,
    };
    Ok(Json(json!({
        "ok": true,
        "run_id": run_id,
        "from_checkpoint": checkpoint.is_some(),
        "checkpoint_seq": checkpoint.as_ref().map(|cp| cp.seq),
        "events_applied": events.len(),
        "blackboard_patches_applied": patches.len(),
        "replay": replay,
        "replay_blackboard": replay_blackboard,
        "persisted": persisted,
        "persisted_blackboard": persisted_blackboard,
        "drift": drift
    })))
}

pub(crate) async fn sync_automation_v2_run_blackboard(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run: &crate::AutomationV2RunRecord,
) -> Result<(), StatusCode> {
    let run_id = automation_v2_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "automation_v2".to_string(),
            source_client: Some("automation_v2_scheduler".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: automation_run_status_to_context(&run.status),
            objective: automation
                .description
                .clone()
                .unwrap_or_else(|| automation.name.clone()),
            workspace: ContextWorkspaceLease::default(),
            steps: automation
                .flow
                .nodes
                .iter()
                .map(|node| ContextRunStep {
                    step_id: node.node_id.clone(),
                    title: node.objective.clone(),
                    status: ContextStepStatus::Pending,
                })
                .collect(),
            tasks: Vec::new(),
            why_next_step: Some("Track automation v2 flow via blackboard tasks".to_string()),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: now,
            started_at_ms: Some(now),
            ended_at_ms: if matches!(
                run.status,
                crate::AutomationRunStatus::Completed
                    | crate::AutomationRunStatus::Failed
                    | crate::AutomationRunStatus::Cancelled
            ) {
                Some(now)
            } else {
                None
            },
            last_error: run.detail.clone(),
            updated_at_ms: now,
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();

    for node in &automation.flow.nodes {
        let task_id = format!("node-{}", node.node_id);
        let depends_on = node
            .depends_on
            .iter()
            .map(|dep| format!("node-{dep}"))
            .collect::<Vec<_>>();

        let status = automation_node_task_status(run, &node.node_id, &depends_on);

        let output = run.checkpoint.node_outputs.get(&node.node_id);
        let payload = automation_node_task_payload(node, output);
        let attempt = run
            .checkpoint
            .node_attempts
            .get(&node.node_id)
            .copied()
            .unwrap_or(0);
        let max_attempts = crate::app::state::automation_node_max_attempts(node);
        let last_error = output
            .and_then(|value| {
                value
                    .get("blocked_reason")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        value
                            .get("validator_summary")
                            .and_then(|summary| summary.get("reason"))
                            .and_then(Value::as_str)
                    })
                    .or_else(|| {
                        value
                            .get("artifact_validation")
                            .and_then(|validation| validation.get("semantic_block_reason"))
                            .and_then(Value::as_str)
                    })
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let existing = run_state.tasks.iter().find(|t| t.id == task_id).cloned();
        if let Some(task) = existing {
            if task.status != status
                || task.payload != payload
                || task.attempt != attempt
                || task.max_attempts != max_attempts
                || task.last_error != last_error
            {
                let next_task = ContextBlackboardTask {
                    payload: payload.clone(),
                    status: status.clone(),
                    attempt,
                    max_attempts,
                    last_error: last_error.clone(),
                    task_rev: task.task_rev.saturating_add(1),
                    updated_ts: now,
                    ..task.clone()
                };
                let event_type = if task.status != status {
                    context_task_status_event_name(&status).to_string()
                } else {
                    "context.task.updated".to_string()
                };
                let _ = context_run_engine()
                    .commit_task_mutation(
                        state,
                        &run_id,
                        next_task.clone(),
                        ContextBlackboardPatchOp::UpdateTaskState,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "payload": payload,
                            "attempt": attempt,
                            "max_attempts": max_attempts,
                            "last_error": last_error,
                            "task_rev": next_task.task_rev,
                        }),
                        event_type,
                        automation_run_status_to_context(&run.status),
                        None,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "payload": next_task.payload,
                            "attempt": next_task.attempt,
                            "max_attempts": next_task.max_attempts,
                            "last_error": next_task.last_error,
                            "task_rev": next_task.task_rev,
                            "source": "automation_v2",
                            "automation_id": automation.automation_id,
                        }),
                    )
                    .await?;
                if let Some(existing_task) =
                    run_state.tasks.iter_mut().find(|row| row.id == task_id)
                {
                    *existing_task = next_task.clone();
                }
            }
        } else {
            let task = ContextBlackboardTask {
                id: task_id.clone(),
                task_type: "automation_node".to_string(),
                payload,
                status,
                workflow_id: Some(automation.automation_id.clone()),
                workflow_node_id: Some(node.node_id.clone()),
                parent_task_id: None,
                depends_on_task_ids: depends_on,
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                assigned_agent: Some(node.agent_id.clone()),
                priority: 0,
                attempt,
                max_attempts,
                last_error,
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: now,
                updated_ts: now,
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    "context.task.created".to_string(),
                    automation_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": task.id,
                        "task_type": task.task_type,
                        "task_rev": task.task_rev,
                        "source": "automation_v2",
                        "automation_id": automation.automation_id,
                    }),
                )
                .await?;
            run_state.tasks.push(task.clone());
        }
    }

    for node in &automation.flow.nodes {
        for projected_task in parse_backlog_projection_tasks(automation, node, run, now) {
            let existing = run_state
                .tasks
                .iter()
                .find(|task| task.id == projected_task.id)
                .cloned();
            let needs_update = match existing.as_ref() {
                Some(task) => {
                    serde_json::to_value(task).ok() != serde_json::to_value(&projected_task).ok()
                }
                None => true,
            };
            if !needs_update {
                continue;
            }
            let payload = serde_json::to_value(&projected_task)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let event_name = if existing.is_some() {
                "context.task.projected".to_string()
            } else {
                "context.task.created".to_string()
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    projected_task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    payload,
                    event_name,
                    automation_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": projected_task.id,
                        "task_type": projected_task.task_type,
                        "task_rev": projected_task.task_rev,
                        "source": "automation_v2_backlog_projection",
                        "automation_id": automation.automation_id,
                        "workflow_node_id": node.node_id,
                    }),
                )
                .await?;
            if let Some(existing_task) = run_state
                .tasks
                .iter_mut()
                .find(|task| task.id == projected_task.id)
            {
                *existing_task = projected_task.clone();
            } else {
                run_state.tasks.push(projected_task.clone());
            }
        }
    }

    Ok(())
}

pub(super) fn automation_v2_context_run_id(run_id: &str) -> String {
    format!("automation-v2-{run_id}")
}

pub(crate) fn routine_context_run_id(run_id: &str) -> String {
    format!("routine-{run_id}")
}

pub(crate) fn session_context_run_id(session_id: &str) -> String {
    format!("session-{session_id}")
}

pub(crate) fn session_run_status_to_context(status: &str) -> ContextRunStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" => ContextRunStatus::Completed,
        "cancelled" => ContextRunStatus::Cancelled,
        "timeout" | "error" | "failed" => ContextRunStatus::Failed,
        _ => ContextRunStatus::Running,
    }
}

pub(crate) async fn ensure_session_context_run(
    state: &AppState,
    session: &tandem_types::Session,
) -> Result<String, StatusCode> {
    let run_id = session_context_run_id(&session.id);
    if load_context_run_state(state, &run_id).await.is_ok() {
        return Ok(run_id);
    }
    let now = crate::now_ms();
    let workspace = session
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|path| ContextWorkspaceLease {
            workspace_id: session
                .project_id
                .clone()
                .unwrap_or_else(|| session.id.clone()),
            canonical_path: path.to_string(),
            lease_epoch: 0,
        })
        .unwrap_or_default();
    let run = ContextRunState {
        run_id: run_id.clone(),
        run_type: "session".to_string(),
        source_client: Some("session_api".to_string()),
        model_provider: session.provider.clone(),
        model_id: session.model.as_ref().map(|model| model.model_id.clone()),
        mcp_servers: Vec::new(),
        status: ContextRunStatus::Queued,
        objective: {
            let title = session.title.trim();
            if title.is_empty() {
                format!("Interactive session {}", session.id)
            } else {
                format!("Interactive session: {title}")
            }
        },
        workspace,
        steps: vec![ContextRunStep {
            step_id: "session-run".to_string(),
            title: "Execute interactive session work".to_string(),
            status: ContextStepStatus::Pending,
        }],
        tasks: Vec::new(),
        why_next_step: Some("waiting for session run activity".to_string()),
        revision: 1,
        last_event_seq: 0,
        created_at_ms: now,
        started_at_ms: None,
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(state, &run).await?;
    Ok(run_id)
}

pub(crate) fn workflow_context_run_id(run_id: &str) -> String {
    format!("workflow-{run_id}")
}

pub(super) fn automation_run_status_to_context(
    status: &crate::AutomationRunStatus,
) -> ContextRunStatus {
    match status {
        crate::AutomationRunStatus::Queued => ContextRunStatus::Queued,
        crate::AutomationRunStatus::Running
        | crate::AutomationRunStatus::Pausing
        | crate::AutomationRunStatus::Paused
        | crate::AutomationRunStatus::AwaitingApproval => ContextRunStatus::Running,
        crate::AutomationRunStatus::Completed => ContextRunStatus::Completed,
        crate::AutomationRunStatus::Blocked => ContextRunStatus::Blocked,
        crate::AutomationRunStatus::Failed => ContextRunStatus::Failed,
        crate::AutomationRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}

fn routine_run_status_to_context(status: &crate::RoutineRunStatus) -> ContextRunStatus {
    match status {
        crate::RoutineRunStatus::Queued => ContextRunStatus::Queued,
        crate::RoutineRunStatus::PendingApproval
        | crate::RoutineRunStatus::Running
        | crate::RoutineRunStatus::Paused => ContextRunStatus::Running,
        crate::RoutineRunStatus::BlockedPolicy => ContextRunStatus::Blocked,
        crate::RoutineRunStatus::Denied => ContextRunStatus::Cancelled,
        crate::RoutineRunStatus::Completed => ContextRunStatus::Completed,
        crate::RoutineRunStatus::Failed => ContextRunStatus::Failed,
        crate::RoutineRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}

fn routine_run_status_to_step(status: &crate::RoutineRunStatus) -> ContextStepStatus {
    match status {
        crate::RoutineRunStatus::Queued => ContextStepStatus::Pending,
        crate::RoutineRunStatus::PendingApproval
        | crate::RoutineRunStatus::Paused
        | crate::RoutineRunStatus::BlockedPolicy
        | crate::RoutineRunStatus::Denied
        | crate::RoutineRunStatus::Cancelled => ContextStepStatus::Blocked,
        crate::RoutineRunStatus::Running => ContextStepStatus::InProgress,
        crate::RoutineRunStatus::Completed => ContextStepStatus::Done,
        crate::RoutineRunStatus::Failed => ContextStepStatus::Failed,
    }
}

pub(crate) async fn sync_routine_run_blackboard(
    state: &AppState,
    run: &crate::RoutineRunRecord,
) -> Result<String, StatusCode> {
    let run_id = routine_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "routine".to_string(),
            source_client: Some("routine_runtime".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: routine_run_status_to_context(&run.status),
            objective: format!("Routine {} ({})", run.routine_id, run.entrypoint),
            workspace: ContextWorkspaceLease::default(),
            steps: vec![ContextRunStep {
                step_id: "routine-run".to_string(),
                title: format!("Execute routine {}", run.entrypoint),
                status: routine_run_status_to_step(&run.status),
            }],
            tasks: Vec::new(),
            why_next_step: Some("Track routine run lifecycle and output artifacts".to_string()),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: run.created_at_ms.max(now),
            started_at_ms: run.started_at_ms.or(run.fired_at_ms),
            ended_at_ms: run.finished_at_ms,
            last_error: run.detail.clone(),
            updated_at_ms: run.updated_at_ms.max(now),
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();
    run_state.status = routine_run_status_to_context(&run.status);
    run_state.objective = format!("Routine {} ({})", run.routine_id, run.entrypoint);
    run_state.updated_at_ms = run.updated_at_ms.max(now);
    run_state.started_at_ms = run_state
        .started_at_ms
        .or(run.started_at_ms)
        .or(run.fired_at_ms);
    run_state.ended_at_ms = run.finished_at_ms;
    run_state.last_error = run.detail.clone();
    run_state.steps = vec![ContextRunStep {
        step_id: "routine-run".to_string(),
        title: format!("Execute routine {}", run.entrypoint),
        status: routine_run_status_to_step(&run.status),
    }];
    save_context_run_state(state, &run_state).await?;

    let mut blackboard = load_context_blackboard(state, &run_id);
    for artifact_row in &run.artifacts {
        let artifact_id = format!("routine-artifact-{}", artifact_row.artifact_id);
        if blackboard.artifacts.iter().any(|row| row.id == artifact_id) {
            continue;
        }
        let artifact = ContextBlackboardArtifact {
            id: artifact_id,
            ts_ms: artifact_row.created_at_ms,
            path: artifact_row.uri.clone(),
            artifact_type: artifact_row.kind.clone(),
            step_id: Some("routine-run".to_string()),
            source_event_id: None,
        };
        let _ = context_run_engine()
            .commit_blackboard_patch(
                state,
                &run_id,
                ContextBlackboardPatchOp::AddArtifact,
                serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            )
            .await?;
        blackboard.artifacts.push(artifact);
    }

    Ok(run_id)
}

fn workflow_run_status_to_context(status: &crate::WorkflowRunStatus) -> ContextRunStatus {
    match status {
        crate::WorkflowRunStatus::Queued => ContextRunStatus::Queued,
        crate::WorkflowRunStatus::Running => ContextRunStatus::Running,
        crate::WorkflowRunStatus::Completed | crate::WorkflowRunStatus::DryRun => {
            ContextRunStatus::Completed
        }
        crate::WorkflowRunStatus::Failed => ContextRunStatus::Failed,
    }
}

fn automation_node_builder_string(node: &crate::AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_builder_bool(node: &crate::AutomationFlowNode, key: &str) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_task_payload(node: &crate::AutomationFlowNode, output: Option<&Value>) -> Value {
    let mut payload = json!({
        "node_id": node.node_id,
        "title": automation_node_builder_string(node, "title").unwrap_or_else(|| node.objective.clone()),
        "name": node.objective,
        "description": node.objective,
        "agent_id": node.agent_id,
        "task_kind": automation_node_builder_string(node, "task_kind"),
        "backlog_task_id": automation_node_builder_string(node, "task_id"),
        "repo_root": automation_node_builder_string(node, "repo_root"),
        "write_scope": automation_node_builder_string(node, "write_scope"),
        "acceptance_criteria": automation_node_builder_string(node, "acceptance_criteria"),
        "task_dependencies": automation_node_builder_string(node, "task_dependencies"),
        "verification_state": automation_node_builder_string(node, "verification_state"),
        "task_owner": automation_node_builder_string(node, "task_owner"),
        "verification_command": automation_node_builder_string(node, "verification_command"),
        "output_path": automation_node_builder_string(node, "output_path"),
        "projects_backlog_tasks": automation_node_builder_bool(node, "project_backlog_tasks"),
    });
    if let Some(object) = payload.as_object_mut() {
        if let Some(output) = output {
            if let Some(status) = output.get("status").and_then(Value::as_str) {
                object.insert("node_status".to_string(), json!(status));
            }
            if let Some(failure_kind) = output.get("failure_kind").and_then(Value::as_str) {
                object.insert("failure_kind".to_string(), json!(failure_kind));
            }
            if let Some(reason) = output
                .get("validator_summary")
                .and_then(|value| value.get("reason"))
                .and_then(Value::as_str)
                .or_else(|| output.get("blocked_reason").and_then(Value::as_str))
            {
                let reason = reason.trim();
                if !reason.is_empty() {
                    object.insert("validator_reason".to_string(), json!(reason));
                }
            }
            if let Some(unmet) = output
                .get("validator_summary")
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .filter(|value| !value.is_empty())
            {
                object.insert(
                    "unmet_requirements".to_string(),
                    Value::Array(unmet.clone()),
                );
            }
            if let Some(actions) = output
                .get("artifact_validation")
                .and_then(|value| value.get("required_next_tool_actions"))
                .and_then(Value::as_array)
                .filter(|value| !value.is_empty())
            {
                object.insert(
                    "required_next_tool_actions".to_string(),
                    Value::Array(actions.clone()),
                );
            }
            if let Some(classification) = output
                .get("artifact_validation")
                .and_then(|value| value.get("blocking_classification"))
                .and_then(Value::as_str)
            {
                let classification = classification.trim();
                if !classification.is_empty() {
                    object.insert("blocking_classification".to_string(), json!(classification));
                }
            }
            if let Some(validation_basis) = output
                .get("artifact_validation")
                .and_then(|value| value.get("validation_basis"))
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("validation_basis".to_string(), validation_basis);
            }
            if let Some(knowledge_preflight) = output
                .get("knowledge_preflight")
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("knowledge_preflight".to_string(), knowledge_preflight);
            }
            if let Some(knowledge_preflight) =
                output.get("knowledge_preflight").and_then(Value::as_object)
            {
                if let Some(reuse_reason) = knowledge_preflight
                    .get("reuse_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert("knowledge_reuse_reason".to_string(), json!(reuse_reason));
                }
                if let Some(skip_reason) = knowledge_preflight
                    .get("skip_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert("knowledge_skip_reason".to_string(), json!(skip_reason));
                }
                if let Some(freshness_reason) = knowledge_preflight
                    .get("freshness_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    object.insert(
                        "knowledge_freshness_reason".to_string(),
                        json!(freshness_reason),
                    );
                }
            }
            if let Some(blocker_category) = output
                .get("blocker_category")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                object.insert("blocker_category".to_string(), json!(blocker_category));
            }
            if let Some(receipt_timeline) = output
                .get("receipt_timeline")
                .or_else(|| {
                    output
                        .get("attempt_evidence")
                        .and_then(|value| value.get("receipt_timeline"))
                })
                .cloned()
                .filter(|value| !value.is_null())
            {
                object.insert("receipt_timeline".to_string(), receipt_timeline);
            }
            if let Some(repair_attempt) = output
                .get("artifact_validation")
                .and_then(|value| value.get("repair_attempt"))
                .and_then(Value::as_u64)
            {
                object.insert("repair_attempt".to_string(), json!(repair_attempt));
            }
            if let Some(repair_attempts_remaining) = output
                .get("artifact_validation")
                .and_then(|value| value.get("repair_attempts_remaining"))
                .and_then(Value::as_u64)
            {
                object.insert(
                    "repair_attempts_remaining".to_string(),
                    json!(repair_attempts_remaining),
                );
            }
        }
    }
    payload
}

fn extract_markdown_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = text;
    while let Some(start) = remainder.find("```") {
        remainder = &remainder[start + 3..];
        let Some(line_end) = remainder.find('\n') else {
            break;
        };
        let lang = remainder[..line_end].trim().to_ascii_lowercase();
        remainder = &remainder[line_end + 1..];
        let Some(end) = remainder.find("```") else {
            break;
        };
        let block = remainder[..end].trim();
        if !block.is_empty() && (lang.is_empty() || lang == "json" || lang == "javascript") {
            blocks.push(block.to_string());
        }
        remainder = &remainder[end + 3..];
    }
    blocks
}

fn extract_backlog_task_values(candidate: &Value) -> Vec<Value> {
    match candidate {
        Value::Array(items) => items.clone(),
        Value::Object(map) => {
            if let Some(items) = map.get("backlog_tasks").and_then(Value::as_array) {
                return items.clone();
            }
            if let Some(items) = map.get("tasks").and_then(Value::as_array) {
                return items.clone();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn normalize_backlog_task_identifier(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            normalized.push(ch);
        } else if !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    normalized.trim_matches('-').to_string()
}

fn parse_backlog_dependencies(value: Option<&Value>) -> Vec<String> {
    if let Some(items) = value.and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    value
        .and_then(Value::as_str)
        .map(|text| {
            text.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn backlog_task_status_from_value(value: &Value) -> ContextBlackboardTaskStatus {
    let status = value
        .get("status")
        .or_else(|| value.get("verification_state"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("runnable")
        .to_ascii_lowercase();
    match status.as_str() {
        "blocked" => ContextBlackboardTaskStatus::Blocked,
        "failed" | "verify_failed" => ContextBlackboardTaskStatus::Failed,
        "done" | "completed" | "verified" => ContextBlackboardTaskStatus::Done,
        "in_progress" | "running" | "claimed" => ContextBlackboardTaskStatus::InProgress,
        "ready" | "runnable" | "todo" | "queued" => ContextBlackboardTaskStatus::Runnable,
        _ => ContextBlackboardTaskStatus::Pending,
    }
}

fn parse_backlog_projection_tasks(
    automation: &crate::AutomationV2Spec,
    node: &crate::AutomationFlowNode,
    run: &crate::AutomationV2RunRecord,
    now: u64,
) -> Vec<ContextBlackboardTask> {
    let projects_backlog_tasks = automation_node_builder_bool(node, "project_backlog_tasks")
        || automation_node_builder_string(node, "task_kind")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("repo_plan"));
    if !projects_backlog_tasks {
        return Vec::new();
    }
    let Some(output) = run.checkpoint.node_outputs.get(&node.node_id) else {
        return Vec::new();
    };
    let text_candidates = [
        output
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str),
        output
            .get("content")
            .and_then(|content| content.get("raw_text"))
            .and_then(Value::as_str),
    ];
    let mut parsed_items = Vec::new();
    for text in text_candidates.into_iter().flatten() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            parsed_items.extend(extract_backlog_task_values(&value));
        }
        for block in extract_markdown_json_blocks(trimmed) {
            if let Ok(value) = serde_json::from_str::<Value>(&block) {
                parsed_items.extend(extract_backlog_task_values(&value));
            }
        }
    }
    parsed_items
        .into_iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let title = object
                .get("title")
                .or_else(|| object.get("objective"))
                .or_else(|| object.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?
                .to_string();
            let raw_task_id = object
                .get("task_id")
                .or_else(|| object.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| title.to_ascii_lowercase().replace(' ', "-"));
            let normalized_task_id = normalize_backlog_task_identifier(&raw_task_id);
            if normalized_task_id.is_empty() {
                return None;
            }
            let task_dependencies =
                parse_backlog_dependencies(object.get("task_dependencies").or_else(|| object.get("dependencies")));
            let task_owner = object
                .get("task_owner")
                .or_else(|| object.get("owner"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| automation_node_builder_string(node, "task_owner"));
            Some(ContextBlackboardTask {
                id: format!("backlog-{}-{}", node.node_id, normalized_task_id),
                task_type: "automation_backlog_item".to_string(),
                payload: json!({
                    "title": title,
                    "description": object.get("description").or_else(|| object.get("summary")).and_then(Value::as_str).map(str::trim).unwrap_or_default(),
                    "task_id": raw_task_id,
                    "backlog_task_id": raw_task_id,
                    "task_kind": object.get("task_kind").and_then(Value::as_str).unwrap_or("code_change"),
                    "repo_root": object.get("repo_root").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "repo_root")),
                    "write_scope": object.get("write_scope").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "write_scope")),
                    "acceptance_criteria": object.get("acceptance_criteria").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "acceptance_criteria")),
                    "task_dependencies": task_dependencies,
                    "verification_state": object.get("verification_state").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "verification_state")),
                    "task_owner": task_owner,
                    "verification_command": object.get("verification_command").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or_else(|| automation_node_builder_string(node, "verification_command")),
                    "source_node_id": node.node_id,
                    "projects_backlog_tasks": true,
                }),
                status: backlog_task_status_from_value(&item),
                workflow_id: Some(automation.automation_id.clone()),
                workflow_node_id: Some(node.node_id.clone()),
                parent_task_id: Some(format!("node-{}", node.node_id)),
                depends_on_task_ids: task_dependencies
                    .iter()
                    .map(|dep| format!("backlog-{}-{}", node.node_id, normalize_backlog_task_identifier(dep)))
                    .collect(),
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                assigned_agent: task_owner,
                priority: object
                    .get("priority")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                attempt: 0,
                max_attempts: 3,
                last_error: None,
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: now,
                updated_ts: now,
            })
        })
        .collect()
}

fn workflow_action_status_to_context(
    status: &WorkflowActionRunStatus,
) -> ContextBlackboardTaskStatus {
    match status {
        WorkflowActionRunStatus::Pending => ContextBlackboardTaskStatus::Pending,
        WorkflowActionRunStatus::Running => ContextBlackboardTaskStatus::InProgress,
        WorkflowActionRunStatus::Completed => ContextBlackboardTaskStatus::Done,
        WorkflowActionRunStatus::Failed => ContextBlackboardTaskStatus::Failed,
        WorkflowActionRunStatus::Skipped => ContextBlackboardTaskStatus::Blocked,
    }
}

pub(crate) async fn sync_workflow_run_blackboard(
    state: &AppState,
    run: &crate::WorkflowRunRecord,
) -> Result<String, StatusCode> {
    let run_id = workflow_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let workspace_root = state.workspace_index.snapshot().await.root;
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "workflow".to_string(),
            source_client: Some("workflow_runtime".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: workflow_run_status_to_context(&run.status),
            objective: format!("Workflow {}", run.workflow_id),
            workspace: ContextWorkspaceLease {
                workspace_id: run.workflow_id.clone(),
                canonical_path: workspace_root,
                lease_epoch: 0,
            },
            steps: run
                .actions
                .iter()
                .map(|action| ContextRunStep {
                    step_id: action.action_id.clone(),
                    title: action.action.clone(),
                    status: match workflow_action_status_to_context(&action.status) {
                        ContextBlackboardTaskStatus::Pending => ContextStepStatus::Pending,
                        ContextBlackboardTaskStatus::Runnable => ContextStepStatus::Runnable,
                        ContextBlackboardTaskStatus::InProgress => ContextStepStatus::InProgress,
                        ContextBlackboardTaskStatus::Done => ContextStepStatus::Done,
                        ContextBlackboardTaskStatus::Failed => ContextStepStatus::Failed,
                        ContextBlackboardTaskStatus::Blocked => ContextStepStatus::Blocked,
                    },
                })
                .collect(),
            tasks: Vec::new(),
            why_next_step: Some(
                "Track workflow hook actions via blackboard tasks and artifacts".to_string(),
            ),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: run.created_at_ms.max(now),
            started_at_ms: Some(run.created_at_ms.max(now)),
            ended_at_ms: run.finished_at_ms,
            last_error: run.actions.iter().find_map(|action| action.detail.clone()),
            updated_at_ms: run.updated_at_ms.max(now),
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();
    run_state.status = workflow_run_status_to_context(&run.status);
    run_state.objective = format!("Workflow {}", run.workflow_id);
    run_state.updated_at_ms = run.updated_at_ms.max(now);
    run_state.ended_at_ms = run.finished_at_ms;
    run_state.last_error = run.actions.iter().find_map(|action| action.detail.clone());
    run_state.steps = run
        .actions
        .iter()
        .map(|action| ContextRunStep {
            step_id: action.action_id.clone(),
            title: action.action.clone(),
            status: match workflow_action_status_to_context(&action.status) {
                ContextBlackboardTaskStatus::Pending => ContextStepStatus::Pending,
                ContextBlackboardTaskStatus::Runnable => ContextStepStatus::Runnable,
                ContextBlackboardTaskStatus::InProgress => ContextStepStatus::InProgress,
                ContextBlackboardTaskStatus::Done => ContextStepStatus::Done,
                ContextBlackboardTaskStatus::Failed => ContextStepStatus::Failed,
                ContextBlackboardTaskStatus::Blocked => ContextStepStatus::Blocked,
            },
        })
        .collect();
    save_context_run_state(state, &run_state).await?;

    let mut blackboard = load_context_blackboard(state, &run_id);
    for action in &run.actions {
        let task_id = format!("workflow-action-{}", action.action_id);
        let artifact_id = format!("workflow-artifact-{}", action.action_id);
        let artifact_ids = if action.output.is_some() {
            vec![artifact_id.clone()]
        } else {
            Vec::new()
        };
        let status = workflow_action_status_to_context(&action.status);
        let existing = run_state
            .tasks
            .iter()
            .find(|row| row.id == task_id)
            .cloned();
        if let Some(task) = existing {
            if task.status != status
                || task.last_error != action.detail
                || task.artifact_ids != artifact_ids
            {
                let next_task = ContextBlackboardTask {
                    status: status.clone(),
                    last_error: action.detail.clone(),
                    artifact_ids: artifact_ids.clone(),
                    task_rev: task.task_rev.saturating_add(1),
                    updated_ts: action.updated_at_ms.max(now),
                    ..task.clone()
                };
                let _ = context_run_engine()
                    .commit_task_mutation(
                        state,
                        &run_id,
                        next_task.clone(),
                        ContextBlackboardPatchOp::UpdateTaskState,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "error": action.detail,
                            "artifact_ids": artifact_ids,
                            "task_rev": next_task.task_rev,
                        }),
                        context_task_status_event_name(&status).to_string(),
                        workflow_run_status_to_context(&run.status),
                        None,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "error": action.detail,
                            "artifact_ids": artifact_ids,
                            "task_rev": next_task.task_rev,
                            "source": "workflow_runtime",
                        }),
                    )
                    .await?;
                if let Some(existing_task) =
                    run_state.tasks.iter_mut().find(|row| row.id == task_id)
                {
                    *existing_task = next_task.clone();
                }
            }
        } else {
            let task = ContextBlackboardTask {
                id: task_id.clone(),
                task_type: "workflow_action".to_string(),
                payload: json!({
                    "action": action.action,
                    "action_id": action.action_id,
                    "workflow_run_id": run.run_id,
                    "workflow_id": run.workflow_id,
                    "task_id": action.task_id.clone().or(run.task_id.clone()),
                }),
                status: status.clone(),
                workflow_id: Some(run.workflow_id.clone()),
                workflow_node_id: Some(action.action_id.clone()),
                parent_task_id: run.task_id.clone(),
                depends_on_task_ids: Vec::new(),
                decision_ids: Vec::new(),
                artifact_ids: artifact_ids.clone(),
                assigned_agent: None,
                priority: 0,
                attempt: 0,
                max_attempts: 1,
                last_error: action.detail.clone(),
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: run.created_at_ms.max(now),
                updated_ts: action.updated_at_ms.max(now),
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    "context.task.created".to_string(),
                    workflow_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": task.id,
                        "task_type": task.task_type,
                        "task_rev": task.task_rev,
                        "source": "workflow_runtime",
                    }),
                )
                .await?;
            run_state.tasks.push(task.clone());
        }

        if let Some(output) = action.output.clone() {
            let artifact_exists = blackboard.artifacts.iter().any(|row| row.id == artifact_id);
            if !artifact_exists {
                let artifact = ContextBlackboardArtifact {
                    id: artifact_id.clone(),
                    ts_ms: action.updated_at_ms.max(now),
                    path: format!(
                        "workflow://{}/{}/{}",
                        run.workflow_id, run.run_id, action.action_id
                    ),
                    artifact_type: "workflow_action_output".to_string(),
                    step_id: Some(task_id.clone()),
                    source_event_id: run.source_event_id.clone(),
                };
                let _ = context_run_engine()
                    .commit_blackboard_patch(
                        state,
                        &run_id,
                        ContextBlackboardPatchOp::AddArtifact,
                        serde_json::to_value(&artifact)
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    )
                    .await?;
                blackboard.artifacts.push(artifact);
                let _ = output;
            }
        }
    }

    Ok(run_id)
}

pub(super) fn automation_node_task_status(
    run: &crate::AutomationV2RunRecord,
    node_id: &str,
    depends_on: &[String],
) -> ContextBlackboardTaskStatus {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|row| row == node_id);
    if completed {
        return ContextBlackboardTaskStatus::Done;
    }
    if matches!(
        run.status,
        crate::AutomationRunStatus::Cancelled | crate::AutomationRunStatus::Failed
    ) {
        return ContextBlackboardTaskStatus::Failed;
    }
    let deps_done = depends_on.iter().all(|dep_task_id| {
        let dep_node_id = dep_task_id.strip_prefix("node-").unwrap_or(dep_task_id);
        run.checkpoint
            .completed_nodes
            .iter()
            .any(|row| row == dep_node_id)
    });
    if !deps_done {
        return ContextBlackboardTaskStatus::Blocked;
    }
    if matches!(
        run.status,
        crate::AutomationRunStatus::Paused | crate::AutomationRunStatus::Pausing
    ) {
        return ContextBlackboardTaskStatus::Blocked;
    }
    if run
        .checkpoint
        .pending_nodes
        .iter()
        .any(|row| row == node_id)
    {
        return ContextBlackboardTaskStatus::Runnable;
    }
    ContextBlackboardTaskStatus::Pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn automation_node_task_payload_includes_repair_guidance_from_output() {
        let node = crate::AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "title": "Research Brief",
                    "output_path": "marketing-brief.md"
                }
            })),
        };
        let output = json!({
            "status": "needs_repair",
            "failure_kind": "research_missing_reads",
            "validator_summary": {
                "reason": "research brief did not read concrete workspace files, so source-backed validation is incomplete",
                "unmet_requirements": ["no_concrete_reads"]
            },
            "artifact_validation": {
                "blocking_classification": "tool_available_but_not_used",
                "required_next_tool_actions": [
                    "Use `read` on concrete workspace files before finalizing the brief."
                ],
                "repair_attempt": 1,
                "repair_attempts_remaining": 2
            }
        });

        let payload = automation_node_task_payload(&node, Some(&output));

        assert_eq!(
            payload.get("node_status").and_then(Value::as_str),
            Some("needs_repair")
        );
        assert_eq!(
            payload.get("failure_kind").and_then(Value::as_str),
            Some("research_missing_reads")
        );
        assert_eq!(
            payload.get("validator_reason").and_then(Value::as_str),
            Some(
                "research brief did not read concrete workspace files, so source-backed validation is incomplete"
            )
        );
        assert_eq!(
            payload
                .get("blocking_classification")
                .and_then(Value::as_str),
            Some("tool_available_but_not_used")
        );
        assert_eq!(
            payload
                .get("required_next_tool_actions")
                .and_then(Value::as_array)
                .and_then(|rows| rows.first())
                .and_then(Value::as_str),
            Some("Use `read` on concrete workspace files before finalizing the brief.")
        );
        assert_eq!(
            payload.get("repair_attempt").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            payload
                .get("repair_attempts_remaining")
                .and_then(Value::as_u64),
            Some(2)
        );
    }

    #[test]
    fn automation_node_task_payload_includes_knowledge_preflight_reasons() {
        let node = crate::AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research-brief".to_string(),
            agent_id: "research".to_string(),
            objective: "Write marketing-brief.md".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "title": "Research Brief",
                    "output_path": "marketing-brief.md"
                }
            })),
        };
        let output = json!({
            "status": "needs_repair",
            "knowledge_preflight": {
                "decision": "reuse_promoted",
                "coverage_key": "project::marketing::research::brief",
                "reuse_reason": "reusing 2 promoted knowledge item(s) from 1 space(s)",
                "skip_reason": null,
                "freshness_reason": null,
                "items": []
            }
        });

        let payload = automation_node_task_payload(&node, Some(&output));

        assert_eq!(
            payload
                .get("knowledge_preflight")
                .and_then(|value| value.get("coverage_key"))
                .and_then(Value::as_str),
            Some("project::marketing::research::brief")
        );
        assert_eq!(
            payload
                .get("knowledge_reuse_reason")
                .and_then(Value::as_str),
            Some("reusing 2 promoted knowledge item(s) from 1 space(s)")
        );
        assert_eq!(
            payload.get("knowledge_skip_reason").and_then(Value::as_str),
            None
        );
        assert_eq!(
            payload
                .get("knowledge_freshness_reason")
                .and_then(Value::as_str),
            None
        );
    }
}
