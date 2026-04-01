use std::collections::{BTreeMap, BTreeSet};

use super::*;
use tandem_core::{
    MutationCheckpointRecord, MutationCheckpointRollbackSnapshot, MutationCheckpointSnapshotStatus,
};

#[derive(Debug, Clone, serde::Serialize)]
struct ContextRunMutationCheckpointView {
    seq: u64,
    ts_ms: u64,
    event_id: String,
    record: MutationCheckpointRecord,
    rollback_readiness: MutationCheckpointRollbackReadiness,
    rollback_plan: MutationCheckpointRollbackPlan,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackReadiness {
    candidate_file_count: usize,
    directly_revertible_file_count: usize,
    requires_snapshot_file_count: usize,
    by_action: BTreeMap<String, u64>,
    files: Vec<MutationCheckpointRollbackFile>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackFile {
    path: String,
    action: String,
    directly_revertible: bool,
    requires_snapshot: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackPlan {
    executable: bool,
    executable_operation_count: usize,
    advisory_operation_count: usize,
    operations: Vec<MutationCheckpointRollbackOperation>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackOperation {
    path: String,
    action: String,
    executable: bool,
    operation: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    advisory_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackPreview {
    executable: bool,
    step_count: usize,
    executable_step_count: usize,
    advisory_step_count: usize,
    executable_operation_count: usize,
    advisory_operation_count: usize,
    by_action: BTreeMap<String, u64>,
    steps: Vec<MutationCheckpointRollbackPreviewStep>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackPreviewStep {
    seq: u64,
    event_id: String,
    tool: String,
    executable: bool,
    operation_count: usize,
    operations: Vec<MutationCheckpointRollbackOperation>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct MutationCheckpointRollbackExecuteRequest {
    #[serde(default)]
    event_ids: Vec<String>,
    confirm: String,
    #[serde(default)]
    policy_ack: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct MutationCheckpointRollbackHistoryEntry {
    seq: u64,
    ts_ms: u64,
    event_id: String,
    outcome: String,
    selected_event_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_event_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied_step_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied_operation_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied_by_action: Option<BTreeMap<String, u64>>,
}

pub(super) async fn context_run_mutation_checkpoints(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let events = load_context_run_mutation_checkpoint_source_events(
        &state,
        &run_id,
        query.since_seq,
        query.tail,
    );
    let records = context_run_mutation_checkpoint_records(&events);
    Ok(Json(json!({
        "records": records,
        "summary": context_run_mutation_checkpoint_summary(&records),
        "rollback_readiness": context_run_mutation_checkpoint_rollback_summary(&records),
        "rollback_plan": context_run_mutation_checkpoint_plan_summary(&records),
    })))
}

pub(super) async fn context_run_mutation_checkpoint_rollback_preview(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let events = load_context_run_mutation_checkpoint_source_events(
        &state,
        &run_id,
        query.since_seq,
        query.tail,
    );
    let records = context_run_mutation_checkpoint_records(&events);
    Ok(Json(json!(context_run_mutation_checkpoint_preview(
        &records
    ))))
}

pub(super) async fn context_run_mutation_checkpoint_rollback_history(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let history = context_run_mutation_checkpoint_rollback_history_entries(
        &load_context_run_mutation_checkpoint_source_events(
            &state,
            &run_id,
            query.since_seq,
            query.tail,
        ),
    );
    Ok(Json(json!({
        "entries": history,
        "summary": context_run_mutation_checkpoint_rollback_history_summary(&history),
    })))
}

pub(super) async fn context_run_mutation_checkpoint_rollback_execute(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(request): Json<MutationCheckpointRollbackExecuteRequest>,
) -> Result<Json<Value>, StatusCode> {
    if request.confirm.trim() != "rollback" {
        return Err(StatusCode::BAD_REQUEST);
    }
    if request.event_ids.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let run = super::context_runs::load_context_run_state(&state, &run_id).await?;
    let policy_ack = request.policy_ack.as_deref().unwrap_or_default();
    if policy_ack != "allow_rollback_execution" {
        let blocked_event = build_rollback_execution_blocked_event(
            "rollback execution requires explicit policy acknowledgement",
            &request.event_ids,
            None,
        );
        super::context_runs::append_context_run_event(&state, &run_id, blocked_event).await?;
        return Ok(Json(json!({
            "applied": false,
            "reason": "rollback execution requires explicit policy acknowledgement",
            "selected_event_ids": request.event_ids,
        })));
    }
    if !rollback_execution_allowed_for_status(&run.status) {
        let blocked_event = build_rollback_execution_blocked_event(
            "rollback execution is not allowed for the current run status",
            &request.event_ids,
            None,
        );
        super::context_runs::append_context_run_event(&state, &run_id, blocked_event).await?;
        return Ok(Json(json!({
            "applied": false,
            "reason": "rollback execution is not allowed for the current run status",
            "run_status": serde_json::to_value(&run.status).unwrap_or(Value::Null),
            "selected_event_ids": request.event_ids,
        })));
    }
    let records = context_run_mutation_checkpoint_records(
        &load_context_run_mutation_checkpoint_source_events(&state, &run_id, None, None),
    );
    let preview = context_run_mutation_checkpoint_preview(&records);
    let requested_event_ids = normalize_requested_event_ids(&request.event_ids)?;
    let selected_steps = preview
        .steps
        .iter()
        .filter(|step| requested_event_ids.contains(&step.event_id))
        .cloned()
        .collect::<Vec<_>>();

    if selected_steps.len() != requested_event_ids.len() {
        let known_ids = preview
            .steps
            .iter()
            .map(|step| step.event_id.clone())
            .collect::<BTreeSet<_>>();
        let missing = requested_event_ids
            .iter()
            .filter(|event_id| !known_ids.contains(*event_id))
            .cloned()
            .collect::<Vec<_>>();
        let blocked_event = build_rollback_execution_blocked_event(
            "selected rollback step was not found in current preview",
            &request.event_ids,
            Some(missing.clone()),
        );
        super::context_runs::append_context_run_event(&state, &run_id, blocked_event).await?;
        return Ok(Json(json!({
            "applied": false,
            "reason": "selected rollback step was not found in current preview",
            "selected_event_ids": request.event_ids,
            "missing_event_ids": missing,
        })));
    }
    if selected_steps.iter().any(|step| !step.executable) {
        let blocked_event = build_rollback_execution_blocked_event(
            "selected rollback step is advisory_only",
            &request.event_ids,
            None,
        );
        super::context_runs::append_context_run_event(&state, &run_id, blocked_event).await?;
        return Ok(Json(json!({
            "applied": false,
            "reason": "selected rollback step is advisory_only",
            "selected_event_ids": request.event_ids,
        })));
    }

    let workspace_root = std::path::PathBuf::from(&run.workspace.canonical_path);
    let mut applied_steps = Vec::new();
    let mut applied_operation_count = 0usize;
    let mut applied_by_action = BTreeMap::<String, u64>::new();
    for step in &selected_steps {
        let mut applied_operations = Vec::new();
        for operation in &step.operations {
            apply_rollback_operation(&workspace_root, operation)?;
            applied_operation_count += 1;
            *applied_by_action
                .entry(operation.action.clone())
                .or_default() += 1;
            applied_operations.push(json!({
                "path": operation.path,
                "action": operation.action,
                "kind": operation.operation.get("kind").cloned().unwrap_or(Value::Null),
            }));
        }
        applied_steps.push(json!({
            "seq": step.seq,
            "event_id": step.event_id,
            "tool": step.tool,
            "operation_count": step.operation_count,
            "operations": applied_operations,
        }));
    }

    let event = ContextRunEventAppendInput {
        event_type: "rollback_execution_applied".to_string(),
        status: ContextRunStatus::Running,
        step_id: Some("session-run".to_string()),
        payload: json!({
            "workspace_root": run.workspace.canonical_path,
            "selected_event_ids": request.event_ids,
            "applied_step_count": applied_steps.len(),
            "applied_operation_count": applied_operation_count,
            "applied_by_action": applied_by_action,
            "steps": applied_steps,
        }),
    };
    super::context_runs::append_context_run_event(&state, &run_id, event).await?;

    Ok(Json(json!({
        "applied": true,
        "selected_event_ids": request.event_ids,
        "applied_step_count": applied_steps.len(),
        "applied_operation_count": applied_operation_count,
        "applied_by_action": applied_by_action,
        "steps": applied_steps,
    })))
}

pub(super) fn context_run_mutation_checkpoint_summary_for_run(
    state: &AppState,
    run_id: &str,
) -> Value {
    let records = context_run_mutation_checkpoint_records(
        &load_context_run_mutation_checkpoint_source_events(state, run_id, None, None),
    );
    context_run_mutation_checkpoint_summary(&records)
}

pub(super) fn context_run_mutation_checkpoint_preview_summary_for_run(
    state: &AppState,
    run_id: &str,
) -> Value {
    let records = context_run_mutation_checkpoint_records(
        &load_context_run_mutation_checkpoint_source_events(state, run_id, None, None),
    );
    let preview = context_run_mutation_checkpoint_preview(&records);
    json!({
        "executable": preview.executable,
        "step_count": preview.step_count,
        "executable_step_count": preview.executable_step_count,
        "advisory_step_count": preview.advisory_step_count,
        "executable_operation_count": preview.executable_operation_count,
        "advisory_operation_count": preview.advisory_operation_count,
        "by_action": preview.by_action,
    })
}

pub(super) fn context_run_mutation_checkpoint_rollback_history_summary_for_run(
    state: &AppState,
    run_id: &str,
) -> Value {
    let history = context_run_mutation_checkpoint_rollback_history_entries(
        &load_context_run_mutation_checkpoint_source_events(state, run_id, None, None),
    );
    context_run_mutation_checkpoint_rollback_history_summary(&history)
}

pub(super) fn context_run_mutation_checkpoint_last_rollback_outcome_for_run(
    state: &AppState,
    run_id: &str,
) -> Value {
    let history = context_run_mutation_checkpoint_rollback_history_entries(
        &load_context_run_mutation_checkpoint_source_events(state, run_id, None, None),
    );
    match history.last() {
        Some(entry) => json!({
            "seq": entry.seq,
            "ts_ms": entry.ts_ms,
            "event_id": entry.event_id,
            "outcome": entry.outcome,
            "reason": entry.reason,
            "selected_event_ids": entry.selected_event_ids,
        }),
        None => Value::Null,
    }
}

pub(super) fn context_run_mutation_checkpoint_rollback_policy_summary(
    run: &ContextRunState,
) -> Value {
    json!({
        "eligible": rollback_execution_allowed_for_status(&run.status),
        "run_status": run.status,
        "required_confirm": "rollback",
        "required_policy_ack": "allow_rollback_execution",
    })
}

fn load_context_run_mutation_checkpoint_source_events(
    state: &AppState,
    run_id: &str,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextRunEventRecord> {
    super::context_runs::load_context_run_events_jsonl(
        &super::context_runs::context_run_events_path(state, run_id),
        since_seq,
        tail,
    )
}

fn build_rollback_execution_blocked_event(
    reason: &str,
    selected_event_ids: &[String],
    missing_event_ids: Option<Vec<String>>,
) -> ContextRunEventAppendInput {
    ContextRunEventAppendInput {
        event_type: "rollback_execution_blocked".to_string(),
        status: ContextRunStatus::Running,
        step_id: Some("session-run".to_string()),
        payload: json!({
            "reason": reason,
            "selected_event_ids": selected_event_ids,
            "missing_event_ids": missing_event_ids,
        }),
    }
}

fn rollback_execution_allowed_for_status(status: &ContextRunStatus) -> bool {
    matches!(
        status,
        ContextRunStatus::AwaitingApproval
            | ContextRunStatus::Paused
            | ContextRunStatus::Blocked
            | ContextRunStatus::Failed
            | ContextRunStatus::Completed
            | ContextRunStatus::Cancelled
    )
}

fn apply_rollback_operation(
    workspace_root: &std::path::Path,
    operation: &MutationCheckpointRollbackOperation,
) -> Result<(), StatusCode> {
    if !operation.executable {
        return Err(StatusCode::CONFLICT);
    }
    let target_path = resolve_workspace_relative_path(
        workspace_root,
        operation
            .operation
            .get("path")
            .and_then(Value::as_str)
            .ok_or(StatusCode::BAD_REQUEST)?,
    )?;
    match operation
        .operation
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(StatusCode::BAD_REQUEST)?
    {
        "delete_file" => match std::fs::remove_file(&target_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        },
        "write_file" => {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            let content = operation
                .operation
                .get("content")
                .and_then(Value::as_str)
                .ok_or(StatusCode::BAD_REQUEST)?;
            std::fs::write(&target_path, content).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    Ok(())
}

fn resolve_workspace_relative_path(
    workspace_root: &std::path::Path,
    raw_path: &str,
) -> Result<std::path::PathBuf, StatusCode> {
    let relative = std::path::Path::new(raw_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(workspace_root.join(relative))
}

fn normalize_requested_event_ids(event_ids: &[String]) -> Result<BTreeSet<String>, StatusCode> {
    let normalized = event_ids
        .iter()
        .map(|event_id| event_id.trim())
        .filter(|event_id| !event_id.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if normalized.is_empty() || normalized.len() != event_ids.len() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(normalized)
}

fn context_run_mutation_checkpoint_rollback_history_entries(
    events: &[ContextRunEventRecord],
) -> Vec<MutationCheckpointRollbackHistoryEntry> {
    events
        .iter()
        .filter_map(|event| match event.event_type.as_str() {
            "rollback_execution_applied" => Some(MutationCheckpointRollbackHistoryEntry {
                seq: event.seq,
                ts_ms: event.ts_ms,
                event_id: event.event_id.clone(),
                outcome: "applied".to_string(),
                selected_event_ids: string_array_field(&event.payload, "selected_event_ids"),
                reason: None,
                missing_event_ids: None,
                applied_step_count: event
                    .payload
                    .get("applied_step_count")
                    .and_then(Value::as_u64),
                applied_operation_count: event
                    .payload
                    .get("applied_operation_count")
                    .and_then(Value::as_u64),
                applied_by_action: event
                    .payload
                    .get("applied_by_action")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
            }),
            "rollback_execution_blocked" => Some(MutationCheckpointRollbackHistoryEntry {
                seq: event.seq,
                ts_ms: event.ts_ms,
                event_id: event.event_id.clone(),
                outcome: "blocked".to_string(),
                selected_event_ids: string_array_field(&event.payload, "selected_event_ids"),
                reason: event
                    .payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                missing_event_ids: {
                    let missing = string_array_field(&event.payload, "missing_event_ids");
                    if missing.is_empty() {
                        None
                    } else {
                        Some(missing)
                    }
                },
                applied_step_count: None,
                applied_operation_count: None,
                applied_by_action: None,
            }),
            _ => None,
        })
        .collect()
}

fn context_run_mutation_checkpoint_rollback_history_summary(
    entries: &[MutationCheckpointRollbackHistoryEntry],
) -> Value {
    let mut by_outcome = BTreeMap::<String, u64>::new();
    let mut last_seq = None;
    let mut last_ts_ms = None;
    for entry in entries {
        *by_outcome.entry(entry.outcome.clone()).or_default() += 1;
        last_seq = Some(entry.seq);
        last_ts_ms = Some(entry.ts_ms);
    }
    json!({
        "entry_count": entries.len(),
        "by_outcome": by_outcome,
        "last_seq": last_seq,
        "last_ts_ms": last_ts_ms,
    })
}

fn string_array_field(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn context_run_mutation_checkpoint_records(
    events: &[ContextRunEventRecord],
) -> Vec<ContextRunMutationCheckpointView> {
    events
        .iter()
        .filter_map(|event| {
            if event.event_type != "mutation_checkpoint_recorded" {
                return None;
            }
            let record =
                event.payload.get("record").cloned().and_then(|value| {
                    serde_json::from_value::<MutationCheckpointRecord>(value).ok()
                })?;
            Some(ContextRunMutationCheckpointView {
                seq: event.seq,
                ts_ms: event.ts_ms,
                event_id: event.event_id.clone(),
                rollback_readiness: derive_record_rollback_readiness(&record),
                rollback_plan: derive_record_rollback_plan(&record),
                record,
            })
        })
        .collect()
}

fn context_run_mutation_checkpoint_summary(records: &[ContextRunMutationCheckpointView]) -> Value {
    let mut by_outcome = BTreeMap::<String, u64>::new();
    let mut by_tool = BTreeMap::<String, u64>::new();
    let mut changed_files = 0u64;

    for row in records {
        *by_outcome
            .entry(serde_json::to_string(&row.record.outcome).unwrap_or_default())
            .or_default() += 1;
        *by_tool.entry(row.record.tool.clone()).or_default() += 1;
        changed_files += row.record.changed_file_count as u64;
    }

    json!({
        "record_count": records.len(),
        "changed_file_count": changed_files,
        "by_outcome": normalize_serialized_enum_counts(by_outcome),
        "by_tool": by_tool,
        "last_seq": records.last().map(|record| record.seq),
        "last_ts_ms": records.last().map(|record| record.ts_ms),
    })
}

fn context_run_mutation_checkpoint_rollback_summary(
    records: &[ContextRunMutationCheckpointView],
) -> Value {
    let mut by_action = BTreeMap::<String, u64>::new();
    let mut candidate_file_count = 0u64;
    let mut directly_revertible_file_count = 0u64;
    let mut requires_snapshot_file_count = 0u64;

    for row in records {
        candidate_file_count += row.rollback_readiness.candidate_file_count as u64;
        directly_revertible_file_count +=
            row.rollback_readiness.directly_revertible_file_count as u64;
        requires_snapshot_file_count += row.rollback_readiness.requires_snapshot_file_count as u64;
        for (action, count) in &row.rollback_readiness.by_action {
            *by_action.entry(action.clone()).or_default() += *count;
        }
    }

    json!({
        "record_count": records.len(),
        "candidate_file_count": candidate_file_count,
        "directly_revertible_file_count": directly_revertible_file_count,
        "requires_snapshot_file_count": requires_snapshot_file_count,
        "by_action": by_action,
    })
}

fn context_run_mutation_checkpoint_plan_summary(
    records: &[ContextRunMutationCheckpointView],
) -> Value {
    let mut executable_record_count = 0u64;
    let mut advisory_record_count = 0u64;
    let mut executable_operation_count = 0u64;
    let mut advisory_operation_count = 0u64;
    let mut by_action = BTreeMap::<String, u64>::new();

    for row in records {
        if row.rollback_plan.executable {
            executable_record_count += 1;
        } else {
            advisory_record_count += 1;
        }
        executable_operation_count += row.rollback_plan.executable_operation_count as u64;
        advisory_operation_count += row.rollback_plan.advisory_operation_count as u64;
        for operation in &row.rollback_plan.operations {
            *by_action.entry(operation.action.clone()).or_default() += 1;
        }
    }

    json!({
        "record_count": records.len(),
        "executable_record_count": executable_record_count,
        "advisory_record_count": advisory_record_count,
        "executable_operation_count": executable_operation_count,
        "advisory_operation_count": advisory_operation_count,
        "by_action": by_action,
    })
}

fn context_run_mutation_checkpoint_preview(
    records: &[ContextRunMutationCheckpointView],
) -> MutationCheckpointRollbackPreview {
    let mut steps = records
        .iter()
        .filter(|row| !row.rollback_plan.operations.is_empty())
        .map(|row| MutationCheckpointRollbackPreviewStep {
            seq: row.seq,
            event_id: row.event_id.clone(),
            tool: row.record.tool.clone(),
            executable: row.rollback_plan.executable,
            operation_count: row.rollback_plan.operations.len(),
            operations: row.rollback_plan.operations.clone(),
        })
        .collect::<Vec<_>>();
    steps.sort_by(|left, right| right.seq.cmp(&left.seq));

    let executable_step_count = steps.iter().filter(|step| step.executable).count();
    let advisory_step_count = steps.len().saturating_sub(executable_step_count);
    let executable_operation_count = steps
        .iter()
        .map(|step| {
            step.operations
                .iter()
                .filter(|operation| operation.executable)
                .count()
        })
        .sum::<usize>();
    let advisory_operation_count = steps
        .iter()
        .map(|step| {
            step.operations
                .iter()
                .filter(|operation| !operation.executable)
                .count()
        })
        .sum::<usize>();
    let mut by_action = BTreeMap::<String, u64>::new();
    for step in &steps {
        for operation in &step.operations {
            *by_action.entry(operation.action.clone()).or_default() += 1;
        }
    }

    MutationCheckpointRollbackPreview {
        executable: advisory_step_count == 0 && !steps.is_empty(),
        step_count: steps.len(),
        executable_step_count,
        advisory_step_count,
        executable_operation_count,
        advisory_operation_count,
        by_action,
        steps,
    }
}

fn derive_record_rollback_readiness(
    record: &MutationCheckpointRecord,
) -> MutationCheckpointRollbackReadiness {
    let mut files = Vec::new();
    let mut by_action = BTreeMap::<String, u64>::new();
    let mut directly_revertible_file_count = 0usize;
    let mut requires_snapshot_file_count = 0usize;

    for file in record.files.iter().filter(|file| file.changed) {
        let (action, directly_revertible, requires_snapshot) = derive_file_rollback_action(
            file.existed_before,
            file.existed_after,
            &file.rollback_snapshot,
        );
        if directly_revertible {
            directly_revertible_file_count += 1;
        }
        if requires_snapshot {
            requires_snapshot_file_count += 1;
        }
        *by_action.entry(action.clone()).or_default() += 1;
        files.push(MutationCheckpointRollbackFile {
            path: file.path.clone(),
            action,
            directly_revertible,
            requires_snapshot,
        });
    }

    MutationCheckpointRollbackReadiness {
        candidate_file_count: files.len(),
        directly_revertible_file_count,
        requires_snapshot_file_count,
        by_action,
        files,
    }
}

fn derive_record_rollback_plan(
    record: &MutationCheckpointRecord,
) -> MutationCheckpointRollbackPlan {
    let operations = record
        .files
        .iter()
        .filter(|file| file.changed)
        .map(|file| derive_file_rollback_operation(file))
        .collect::<Vec<_>>();
    let executable_operation_count = operations.iter().filter(|row| row.executable).count();
    let advisory_operation_count = operations.len().saturating_sub(executable_operation_count);

    MutationCheckpointRollbackPlan {
        executable: advisory_operation_count == 0 && !operations.is_empty(),
        executable_operation_count,
        advisory_operation_count,
        operations,
    }
}

fn derive_file_rollback_operation(
    file: &tandem_core::MutationCheckpointFileRecord,
) -> MutationCheckpointRollbackOperation {
    let (action, directly_revertible, _) = derive_file_rollback_action(
        file.existed_before,
        file.existed_after,
        &file.rollback_snapshot,
    );
    let (operation, advisory_reason) = match action.as_str() {
        "delete_created_file" => (
            json!({
                "kind": "delete_file",
                "path": file.path.clone(),
            }),
            None,
        ),
        "restore_previous_contents" | "recreate_deleted_file" => {
            if file.rollback_snapshot.status == MutationCheckpointSnapshotStatus::InlineText {
                (
                    json!({
                        "kind": "write_file",
                        "path": file.path.clone(),
                        "content": file.rollback_snapshot.content.clone(),
                    }),
                    None,
                )
            } else {
                (
                    json!({
                        "kind": "advisory_only",
                        "path": file.path.clone(),
                        "snapshot_status": file.rollback_snapshot.status,
                    }),
                    Some(match file.rollback_snapshot.status {
                        MutationCheckpointSnapshotStatus::TooLarge => {
                            "prior contents were too large to inline".to_string()
                        }
                        MutationCheckpointSnapshotStatus::Binary => {
                            "prior contents were binary and not inlined".to_string()
                        }
                        MutationCheckpointSnapshotStatus::NotNeeded => {
                            "prior contents were not captured".to_string()
                        }
                        MutationCheckpointSnapshotStatus::InlineText => {
                            "prior contents are available inline".to_string()
                        }
                    }),
                )
            }
        }
        _ => (
            json!({
                "kind": "noop",
                "path": file.path.clone(),
            }),
            Some("no rollback action required".to_string()),
        ),
    };

    MutationCheckpointRollbackOperation {
        path: file.path.clone(),
        action,
        executable: directly_revertible,
        operation,
        advisory_reason,
    }
}

fn derive_file_rollback_action(
    existed_before: bool,
    existed_after: bool,
    rollback_snapshot: &MutationCheckpointRollbackSnapshot,
) -> (String, bool, bool) {
    match (existed_before, existed_after) {
        (false, true) => ("delete_created_file".to_string(), true, false),
        (true, false) => {
            let snapshot_available =
                rollback_snapshot.status == MutationCheckpointSnapshotStatus::InlineText;
            (
                "recreate_deleted_file".to_string(),
                snapshot_available,
                !snapshot_available,
            )
        }
        (true, true) => {
            let snapshot_available =
                rollback_snapshot.status == MutationCheckpointSnapshotStatus::InlineText;
            (
                "restore_previous_contents".to_string(),
                snapshot_available,
                !snapshot_available,
            )
        }
        (false, false) => ("no_action".to_string(), false, false),
    }
}

fn normalize_serialized_enum_counts(counts: BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    counts
        .into_iter()
        .map(|(key, value)| (key.trim_matches('"').to_string(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation_event(
        seq: u64,
        tool: &str,
        outcome: &str,
        changed_file_count: u64,
    ) -> ContextRunEventRecord {
        ContextRunEventRecord {
            event_id: format!("event-{seq}"),
            run_id: "run-1".to_string(),
            seq,
            ts_ms: seq * 10,
            event_type: "mutation_checkpoint_recorded".to_string(),
            status: ContextRunStatus::Running,
            revision: seq,
            step_id: Some("session-run".to_string()),
            task_id: None,
            command_id: None,
            payload: json!({
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": tool,
                    "outcome": outcome,
                    "file_count": 1,
                    "changed_file_count": changed_file_count,
                    "files": [{
                        "path": "src/lib.rs",
                        "resolved_path": "/workspace/src/lib.rs",
                        "existed_before": false,
                        "existed_after": true,
                        "changed": true,
                        "rollback_snapshot": {
                            "status": "not_needed"
                        }
                    }]
                }
            }),
        }
    }

    #[test]
    fn mutation_checkpoint_summary_counts_outcomes_and_changed_files() {
        let records = context_run_mutation_checkpoint_records(&[
            mutation_event(1, "write", "succeeded", 1),
            mutation_event(2, "apply_patch", "failed", 0),
        ]);

        let summary = context_run_mutation_checkpoint_summary(&records);
        assert_eq!(summary["record_count"].as_u64(), Some(2));
        assert_eq!(summary["changed_file_count"].as_u64(), Some(1));
        assert_eq!(summary["by_outcome"]["succeeded"].as_u64(), Some(1));
        assert_eq!(summary["by_outcome"]["failed"].as_u64(), Some(1));
        assert_eq!(summary["by_tool"]["write"].as_u64(), Some(1));
    }

    #[test]
    fn mutation_checkpoint_rollback_summary_distinguishes_direct_and_snapshot_paths() {
        let records = context_run_mutation_checkpoint_records(&[
            mutation_event(1, "write", "succeeded", 1),
            ContextRunEventRecord {
                event_id: "event-2".to_string(),
                run_id: "run-1".to_string(),
                seq: 2,
                ts_ms: 20,
                event_type: "mutation_checkpoint_recorded".to_string(),
                status: ContextRunStatus::Running,
                revision: 2,
                step_id: Some("session-run".to_string()),
                task_id: None,
                command_id: None,
                payload: json!({
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-2",
                        "tool": "apply_patch",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": "/workspace/src/lib.rs",
                            "existed_before": true,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "too_large",
                                "byte_count": 64000
                            }
                        }]
                    }
                }),
            },
        ]);

        let summary = context_run_mutation_checkpoint_rollback_summary(&records);
        assert_eq!(summary["candidate_file_count"].as_u64(), Some(2));
        assert_eq!(summary["directly_revertible_file_count"].as_u64(), Some(1));
        assert_eq!(summary["requires_snapshot_file_count"].as_u64(), Some(1));
        assert_eq!(
            summary["by_action"]["delete_created_file"].as_u64(),
            Some(1)
        );
        assert_eq!(
            summary["by_action"]["restore_previous_contents"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn mutation_checkpoint_plan_summary_distinguishes_executable_and_advisory_operations() {
        let records = context_run_mutation_checkpoint_records(&[
            mutation_event(1, "write", "succeeded", 1),
            ContextRunEventRecord {
                event_id: "event-2".to_string(),
                run_id: "run-1".to_string(),
                seq: 2,
                ts_ms: 20,
                event_type: "mutation_checkpoint_recorded".to_string(),
                status: ContextRunStatus::Running,
                revision: 2,
                step_id: Some("session-run".to_string()),
                task_id: None,
                command_id: None,
                payload: json!({
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-2",
                        "tool": "edit",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": "/workspace/src/lib.rs",
                            "existed_before": true,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "inline_text",
                                "content": "before",
                                "byte_count": 6
                            }
                        }]
                    }
                }),
            },
        ]);

        let summary = context_run_mutation_checkpoint_plan_summary(&records);
        assert_eq!(summary["record_count"].as_u64(), Some(2));
        assert_eq!(summary["executable_record_count"].as_u64(), Some(2));
        assert_eq!(summary["advisory_record_count"].as_u64(), Some(0));
        assert_eq!(summary["executable_operation_count"].as_u64(), Some(2));
        assert_eq!(
            summary["by_action"]["delete_created_file"].as_u64(),
            Some(1)
        );
        assert_eq!(
            summary["by_action"]["restore_previous_contents"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn mutation_checkpoint_preview_orders_steps_newest_first_and_tracks_advisory_steps() {
        let records = context_run_mutation_checkpoint_records(&[
            mutation_event(1, "write", "succeeded", 1),
            ContextRunEventRecord {
                event_id: "event-2".to_string(),
                run_id: "run-1".to_string(),
                seq: 2,
                ts_ms: 20,
                event_type: "mutation_checkpoint_recorded".to_string(),
                status: ContextRunStatus::Running,
                revision: 2,
                step_id: Some("session-run".to_string()),
                task_id: None,
                command_id: None,
                payload: json!({
                    "record": {
                        "session_id": "session-1",
                        "message_id": "message-2",
                        "tool": "apply_patch",
                        "outcome": "succeeded",
                        "file_count": 1,
                        "changed_file_count": 1,
                        "files": [{
                            "path": "src/lib.rs",
                            "resolved_path": "/workspace/src/lib.rs",
                            "existed_before": true,
                            "existed_after": true,
                            "changed": true,
                            "rollback_snapshot": {
                                "status": "too_large",
                                "byte_count": 64000
                            }
                        }]
                    }
                }),
            },
        ]);

        let preview = context_run_mutation_checkpoint_preview(&records);
        assert_eq!(preview.step_count, 2);
        assert!(!preview.executable);
        assert_eq!(preview.advisory_step_count, 1);
        assert_eq!(preview.executable_step_count, 1);
        assert_eq!(preview.steps[0].seq, 2);
        assert_eq!(preview.steps[1].seq, 1);
        assert_eq!(preview.steps[0].tool, "apply_patch");
        assert_eq!(preview.steps[1].tool, "write");
    }
}
