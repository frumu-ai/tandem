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
use tandem_types::{EngineEvent, TenantContext};
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
        "fix_proposal" | "fix-proposal" => Some(ContextTaskKind::FixProposal),
        _ => None,
    }
}

fn context_task_kind_str(kind: &ContextTaskKind) -> &'static str {
    match kind {
        ContextTaskKind::Implementation => "implementation",
        ContextTaskKind::Inspection => "inspection",
        ContextTaskKind::Research => "research",
        ContextTaskKind::Validation => "validation",
        ContextTaskKind::FixProposal => "fix_proposal",
    }
}

fn tenant_context_event_value(tenant_context: &TenantContext) -> Value {
    serde_json::to_value(tenant_context).unwrap_or_else(|_| json!(tenant_context))
}

fn with_tenant_context(mut properties: Value, tenant_context: &TenantContext) -> Value {
    if let Some(map) = properties.as_object_mut() {
        map.insert(
            "tenantContext".to_string(),
            tenant_context_event_value(tenant_context),
        );
    }
    properties
}

fn load_context_run_tenant_context_sync(state: &AppState, run_id: &str) -> Option<TenantContext> {
    load_context_run_state_sync(state, run_id)
        .ok()
        .map(|run| run.tenant_context)
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
        ContextTaskKind::Inspection
        | ContextTaskKind::Research
        | ContextTaskKind::Validation
        | ContextTaskKind::FixProposal => ContextTaskExecutionMode::StrictNonwriting,
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
        ContextTaskKind::Inspection
        | ContextTaskKind::Research
        | ContextTaskKind::Validation
        | ContextTaskKind::FixProposal => {
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
            payload
                .entry("tenantContext".to_string())
                .or_insert_with(|| tenant_context_event_value(&run.tenant_context));
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
    let mut payload = serde_json::to_value(envelope).unwrap_or_else(|_| json!({}));
    if let Some(tenant_context) = load_context_run_tenant_context_sync(state, &envelope.run_id) {
        if let Some(map) = payload.as_object_mut() {
            map.entry("tenantContext".to_string())
                .or_insert_with(|| tenant_context_event_value(&tenant_context));
        }
    }
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
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<ContextRunCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    context_run_create_impl(state, tenant_context, input).await
}

pub(super) async fn context_run_create_impl(
    state: AppState,
    tenant_context: TenantContext,
    input: ContextRunCreateInput,
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
        tenant_context,
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
