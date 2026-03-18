use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    evaluate_routine_execution_policy, AppState, AutomationAgentMcpPolicy, AutomationAgentProfile,
    AutomationAgentToolPolicy, AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunStatus,
    AutomationV2Schedule, AutomationV2Spec, AutomationV2Status, RoutineExecutionDecision,
    RoutineHistoryEvent, RoutineMisfirePolicy, RoutineRunArtifact, RoutineRunRecord,
    RoutineRunStatus, RoutineSchedule, RoutineSpec, RoutineStatus, RoutineStoreError,
};
use tandem_types::EngineEvent;

fn routine_run_with_context_links(run: &RoutineRunRecord) -> Value {
    let context_run_id = super::context_runs::routine_context_run_id(&run.run_id);
    let mut payload = serde_json::to_value(run).unwrap_or_else(|_| json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("contextRunID".to_string(), json!(context_run_id));
        obj.insert("linked_context_run_id".to_string(), json!(context_run_id));
    }
    payload
}

fn automation_v2_node_repair_guidance(output: &Value) -> Option<Value> {
    let validator_summary = output.get("validator_summary");
    let artifact_validation = output.get("artifact_validation");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .cloned()
        .unwrap_or_default();
    let validator_reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .cloned()
        .unwrap_or_default();
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if required_next_tool_actions.is_empty()
        && validator_reason.is_none()
        && unmet_requirements.is_empty()
        && blocking_classification.is_none()
    {
        return None;
    }

    Some(json!({
        "status": output.get("status").and_then(Value::as_str),
        "failureKind": output.get("failure_kind").and_then(Value::as_str),
        "reason": validator_reason,
        "unmetRequirements": unmet_requirements,
        "blockingClassification": blocking_classification,
        "requiredNextToolActions": required_next_tool_actions,
        "repairAttempt": artifact_validation
            .and_then(|value| value.get("repair_attempt"))
            .and_then(Value::as_u64),
        "repairAttemptsRemaining": artifact_validation
            .and_then(|value| value.get("repair_attempts_remaining"))
            .and_then(Value::as_u64),
    }))
}

fn automation_v2_run_with_context_links(run: &crate::AutomationV2RunRecord) -> Value {
    let mut normalized_run = run.clone();
    if let Some(automation) = normalized_run.automation_snapshot.clone() {
        for node in &automation.flow.nodes {
            if let Some(output) = normalized_run
                .checkpoint
                .node_outputs
                .get_mut(&node.node_id)
            {
                *output =
                    crate::app::state::enrich_automation_node_output_for_contract(node, output);
            }
        }
    }
    let mut node_repair_guidance = serde_json::Map::new();
    let mut needs_repair_node_ids = Vec::new();
    for (node_id, output) in &normalized_run.checkpoint.node_outputs {
        if output
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("needs_repair"))
        {
            needs_repair_node_ids.push(node_id.clone());
        }
        if let Some(guidance) = automation_v2_node_repair_guidance(output) {
            node_repair_guidance.insert(node_id.clone(), guidance);
        }
    }
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let mut payload = serde_json::to_value(&normalized_run).unwrap_or_else(|_| json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("contextRunID".to_string(), json!(context_run_id.clone()));
        obj.insert("linked_context_run_id".to_string(), json!(context_run_id));
        obj.insert(
            "nodeRepairGuidance".to_string(),
            Value::Object(node_repair_guidance),
        );
        obj.insert(
            "needsRepairNodeIDs".to_string(),
            json!(needs_repair_node_ids),
        );
    }
    payload
}

#[derive(Debug, Deserialize)]
pub(super) struct RoutineCreateInput {
    pub routine_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: String,
    pub args: Option<Value>,
    pub allowed_tools: Option<Vec<String>>,
    pub output_targets: Option<Vec<String>>,
    pub creator_type: Option<String>,
    pub creator_id: Option<String>,
    pub requires_approval: Option<bool>,
    pub external_integrations_allowed: Option<bool>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationMissionInput {
    pub objective: String,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationToolPolicyInput {
    #[serde(default)]
    pub run_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default)]
    pub orchestrator_only_tool_calls: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationApprovalPolicyInput {
    #[serde(default)]
    pub requires_approval: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationPolicyInput {
    #[serde(default)]
    pub tool: AutomationToolPolicyInput,
    #[serde(default)]
    pub approval: AutomationApprovalPolicyInput,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationCreateInput {
    pub automation_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub mission: AutomationMissionInput,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    pub creator_type: Option<String>,
    pub creator_id: Option<String>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationMissionPatchInput {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub success_criteria: Option<Vec<String>>,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationPatchInput {
    pub name: Option<String>,
    pub status: Option<RoutineStatus>,
    pub schedule: Option<RoutineSchedule>,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub mission: Option<AutomationMissionPatchInput>,
    #[serde(default)]
    pub policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationV2CreateInput {
    pub automation_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<AutomationV2Status>,
    pub schedule: AutomationV2Schedule,
    #[serde(default)]
    pub agents: Vec<AutomationAgentProfile>,
    pub flow: AutomationFlowSpec,
    #[serde(default)]
    pub execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2PatchInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<AutomationV2Status>,
    #[serde(default)]
    pub schedule: Option<AutomationV2Schedule>,
    #[serde(default)]
    pub agents: Option<Vec<AutomationAgentProfile>>,
    #[serde(default)]
    pub flow: Option<AutomationFlowSpec>,
    #[serde(default)]
    pub execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutinePatchInput {
    pub name: Option<String>,
    pub status: Option<RoutineStatus>,
    pub schedule: Option<RoutineSchedule>,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: Option<String>,
    pub args: Option<Value>,
    pub allowed_tools: Option<Vec<String>>,
    pub output_targets: Option<Vec<String>>,
    pub requires_approval: Option<bool>,
    pub external_integrations_allowed: Option<bool>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunNowInput {
    pub run_count: Option<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineHistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunsQuery {
    pub routine_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunDecisionInput {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationV2GateDecisionInput {
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunRepairInput {
    pub node_id: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunTaskActionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2BacklogClaimInput {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub lease_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RoutineRunArtifactInput {
    pub uri: String,
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineEventsQuery {
    pub routine_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationEventsQuery {
    pub automation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutomationV2TaskResetPreview {
    pub run_id: String,
    pub node_id: String,
    pub reset_nodes: Vec<String>,
    pub cleared_outputs: Vec<String>,
    pub preserves_upstream_outputs: bool,
}

pub(super) fn routine_error_response(error: RoutineStoreError) -> (StatusCode, Json<Value>) {
    match error {
        RoutineStoreError::InvalidRoutineId { routine_id } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine id",
                "code": "INVALID_ROUTINE_ID",
                "routineID": routine_id,
            })),
        ),
        RoutineStoreError::InvalidSchedule { detail } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine schedule",
                "code": "INVALID_ROUTINE_SCHEDULE",
                "detail": detail,
            })),
        ),
        RoutineStoreError::PersistFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Routine persistence failed",
                "code": "ROUTINE_PERSIST_FAILED",
                "detail": message,
            })),
        ),
    }
}

pub(super) async fn routines_create(
    State(state): State<AppState>,
    Json(input): Json<RoutineCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = RoutineSpec {
        routine_id: input
            .routine_id
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input.entrypoint,
        args: input.args.unwrap_or_else(|| json!({})),
        allowed_tools: input.allowed_tools.unwrap_or_default(),
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: input.creator_type.unwrap_or_else(|| "user".to_string()),
        creator_id: input.creator_id.unwrap_or_else(|| "unknown".to_string()),
        requires_approval: input.requires_approval.unwrap_or(true),
        external_integrations_allowed: input.external_integrations_allowed.unwrap_or(false),
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    };
    let stored = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "routine.created",
        json!({
            "routineID": stored.routine_id,
            "name": stored.name,
            "entrypoint": stored.entrypoint,
        }),
    ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

pub(super) async fn routines_list(State(state): State<AppState>) -> Json<Value> {
    let routines = state.list_routines().await;
    Json(json!({
        "routines": routines,
        "count": routines.len(),
    }))
}

pub(super) async fn routines_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutinePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        )
    })?;
    if let Some(name) = input.name {
        routine.name = name;
    }
    if let Some(status) = input.status {
        routine.status = status;
    }
    if let Some(schedule) = input.schedule {
        routine.schedule = schedule;
    }
    if let Some(timezone) = input.timezone {
        routine.timezone = timezone;
    }
    if let Some(misfire_policy) = input.misfire_policy {
        routine.misfire_policy = misfire_policy;
    }
    if let Some(entrypoint) = input.entrypoint {
        routine.entrypoint = entrypoint;
    }
    if let Some(args) = input.args {
        routine.args = args;
    }
    if let Some(allowed_tools) = input.allowed_tools {
        routine.allowed_tools = allowed_tools;
    }
    if let Some(output_targets) = input.output_targets {
        routine.output_targets = output_targets;
    }
    if let Some(requires_approval) = input.requires_approval {
        routine.requires_approval = requires_approval;
    }
    if let Some(external_integrations_allowed) = input.external_integrations_allowed {
        routine.external_integrations_allowed = external_integrations_allowed;
    }
    if let Some(next_fire_at_ms) = input.next_fire_at_ms {
        routine.next_fire_at_ms = Some(next_fire_at_ms);
    }

    let stored = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "routine.updated",
        json!({
            "routineID": stored.routine_id,
            "status": stored.status,
            "nextFireAtMs": stored.next_fire_at_ms,
        }),
    ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

pub(super) async fn routines_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine(&id)
        .await
        .map_err(routine_error_response)?;
    if let Some(routine) = deleted {
        state.event_bus.publish(EngineEvent::new(
            "routine.deleted",
            json!({
                "routineID": routine.routine_id,
            }),
        ));
        Ok(Json(json!({
            "deleted": true,
            "routineID": id,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        ))
    }
}

pub(super) async fn routines_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        )
    })?;
    let run_count = input.run_count.unwrap_or(1).clamp(1, 20);
    let now = crate::now_ms();
    let trigger_type = "manual";
    match evaluate_routine_execution_policy(&routine, trigger_type) {
        RoutineExecutionDecision::Allowed => {
            let _ = state.mark_routine_fired(&routine.routine_id, now).await;
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::Queued,
                    input.reason.clone(),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "queued".to_string(),
                    detail: input.reason,
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.fired",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "firedAtMs": now,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Ok(Json(json!({
                "ok": true,
                "status": "queued",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
                "firedAtMs": now,
                "contextRunID": context_run_id,
                "linked_context_run_id": context_run_id,
            })))
        }
        RoutineExecutionDecision::RequiresApproval { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::PendingApproval,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "pending_approval".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.approval_required",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Ok(Json(json!({
                "ok": true,
                "status": "pending_approval",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
                "contextRunID": context_run_id,
                "linked_context_run_id": context_run_id,
            })))
        }
        RoutineExecutionDecision::Blocked { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::BlockedPolicy,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "blocked_policy".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.blocked",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Routine blocked by policy",
                    "code": "ROUTINE_POLICY_BLOCKED",
                    "routineID": id,
                    "runID": run.run_id,
                    "reason": reason,
                    "contextRunID": context_run_id,
                    "linked_context_run_id": context_run_id,
                })),
            ))
        }
    }
}

pub(super) async fn routines_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let events = state.list_routine_history(&id, limit).await;
    Json(json!({
        "routineID": id,
        "events": events,
        "count": events.len(),
    }))
}

pub(super) async fn routines_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let runs = state.list_routine_runs(Some(&id), limit).await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    Json(json!({
        "routineID": id,
        "runs": runs.iter().map(routine_run_with_context_links).collect::<Vec<_>>(),
        "count": runs.len(),
    }))
}

pub(super) async fn routines_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let runs = state
        .list_routine_runs(query.routine_id.as_deref(), limit)
        .await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    Json(json!({
        "runs": runs.iter().map(routine_run_with_context_links).collect::<Vec<_>>(),
        "count": runs.len(),
    }))
}

pub(super) async fn routines_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "run": routine_run_with_context_links(&run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) fn reason_or_default(input: Option<String>, fallback: &str) -> String {
    input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub(super) async fn routines_run_approve(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "approved by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Queued, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.approved",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_deny(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "denied by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Denied, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.denied",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if !matches!(
        current.status,
        RoutineRunStatus::Queued | RoutineRunStatus::Running
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not pausable",
                "code": "ROUTINE_RUN_NOT_PAUSABLE",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let mut cancelled_sessions = Vec::new();
    if current.status == RoutineRunStatus::Running {
        for session_id in &current.active_session_ids {
            if state.cancellations.cancel(session_id).await {
                let _ = state.close_browser_sessions_for_owner(session_id).await;
                cancelled_sessions.push(session_id.clone());
            }
        }
    }
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Paused, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.paused",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "cancelledSessionIDs": cancelled_sessions,
        }),
    ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "cancelledSessionIDs": cancelled_sessions,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not paused",
                "code": "ROUTINE_RUN_NOT_PAUSED",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Queued, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.resumed",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "runID": run_id,
        "artifacts": run.artifacts,
        "count": run.artifacts.len(),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_artifact_add(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if input.uri.trim().is_empty() || input.kind.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"Artifact requires uri and kind",
                "code":"ROUTINE_ARTIFACT_INVALID",
            })),
        ));
    }
    let artifact = RoutineRunArtifact {
        artifact_id: format!("artifact-{}", Uuid::new_v4()),
        uri: input.uri.trim().to_string(),
        kind: input.kind.trim().to_string(),
        label: input
            .label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        created_at_ms: crate::now_ms(),
        metadata: input.metadata,
    };
    let updated = state
        .append_routine_run_artifact(&run_id, artifact.clone())
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Routine run not found",
                    "code":"ROUTINE_RUN_NOT_FOUND",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.artifact_added",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "artifact": artifact,
        }),
    ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "artifact": artifact,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

fn routines_sse_stream(
    state: AppState,
    routine_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "routines",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("routine.") {
                return None;
            }
            if let Some(routine_id) = routine_id.as_deref() {
                let event_routine_id = event
                    .properties
                    .get("routineID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_routine_id != routine_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn routines_events(
    State(state): State<AppState>,
    Query(query): Query<RoutineEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(routines_sse_stream(state, query.routine_id))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn objective_from_args(args: &Value, routine_id: &str, entrypoint: &str) -> String {
    args.get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!("Execute automation '{routine_id}' with entrypoint '{entrypoint}'.")
        })
}

pub(super) fn success_criteria_from_args(args: &Value) -> Vec<String> {
    args.get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn mode_from_args(args: &Value) -> String {
    args.get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("standalone")
        .to_string()
}

pub(super) fn normalize_automation_mode(raw: Option<&str>) -> Result<String, String> {
    let value = raw.unwrap_or("standalone").trim();
    if value.is_empty() {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("standalone") {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("orchestrated") {
        return Ok("orchestrated".to_string());
    }
    Err("mode must be one of standalone|orchestrated".to_string())
}

pub(super) fn validate_model_spec_object(value: &Value, path: &str) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("{path} must be an object"))?;
    let provider_id = obj
        .get("provider_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.provider_id is required"))?;
    let model_id = obj
        .get("model_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.model_id is required"))?;
    if provider_id.is_empty() || model_id.is_empty() {
        return Err(format!(
            "{path}.provider_id and {path}.model_id are required"
        ));
    }
    Ok(())
}

pub(crate) fn validate_model_policy(value: &Value) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "model_policy must be an object".to_string())?;
    if let Some(default_model) = obj.get("default_model") {
        validate_model_spec_object(default_model, "model_policy.default_model")?;
    }
    if let Some(role_models) = obj.get("role_models") {
        let role_obj = role_models
            .as_object()
            .ok_or_else(|| "model_policy.role_models must be an object".to_string())?;
        for (role, spec) in role_obj {
            validate_model_spec_object(spec, &format!("model_policy.role_models.{role}"))?;
        }
    }
    Ok(())
}

pub(super) fn routine_to_automation_wire(routine: RoutineSpec) -> Value {
    json!({
        "automation_id": routine.routine_id,
        "name": routine.name,
        "status": routine.status,
        "schedule": routine.schedule,
        "timezone": routine.timezone,
        "misfire_policy": routine.misfire_policy,
        "mode": mode_from_args(&routine.args),
        "mission": {
            "objective": objective_from_args(&routine.args, &routine.routine_id, &routine.entrypoint),
            "success_criteria": success_criteria_from_args(&routine.args),
            "briefing": routine.args.get("briefing").cloned(),
            "entrypoint_compat": routine.entrypoint,
        },
        "policy": {
            "tool": {
                "run_allowlist": routine.allowed_tools,
                "external_integrations_allowed": routine.external_integrations_allowed,
                "orchestrator_only_tool_calls": routine
                    .args
                    .get("orchestrator_only_tool_calls")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            },
            "approval": {
                "requires_approval": routine.requires_approval
            }
        },
        "model_policy": routine.args.get("model_policy").cloned(),
        "output_targets": routine.output_targets,
        "creator_type": routine.creator_type,
        "creator_id": routine.creator_id,
        "next_fire_at_ms": routine.next_fire_at_ms,
        "last_fired_at_ms": routine.last_fired_at_ms
    })
}

pub(super) fn routine_run_to_automation_wire(run: RoutineRunRecord) -> Value {
    let context_run_id = super::context_runs::routine_context_run_id(&run.run_id);
    let latest_session_id = run
        .latest_session_id
        .clone()
        .or_else(|| run.active_session_ids.last().cloned());
    let attach_event_stream = latest_session_id
        .as_ref()
        .map(|session_id| format!("/event?sessionID={session_id}&runID={}", run.run_id));
    json!({
        "run_id": run.run_id,
        "automation_id": run.routine_id,
        "trigger_type": run.trigger_type,
        "run_count": run.run_count,
        "status": run.status,
        "created_at_ms": run.created_at_ms,
        "updated_at_ms": run.updated_at_ms,
        "fired_at_ms": run.fired_at_ms,
        "started_at_ms": run.started_at_ms,
        "finished_at_ms": run.finished_at_ms,
        "mode": mode_from_args(&run.args),
        "mission_snapshot": {
            "objective": objective_from_args(&run.args, &run.routine_id, &run.entrypoint),
            "success_criteria": success_criteria_from_args(&run.args),
            "entrypoint_compat": run.entrypoint,
        },
        "policy_snapshot": {
            "tool": {
                "run_allowlist": run.allowed_tools,
            },
            "approval": {
                "requires_approval": run.requires_approval
            }
        },
        "model_policy": run.args.get("model_policy").cloned(),
        "requires_approval": run.requires_approval,
        "approval_reason": run.approval_reason,
        "denial_reason": run.denial_reason,
        "paused_reason": run.paused_reason,
        "detail": run.detail,
        "output_targets": run.output_targets,
        "artifacts": run.artifacts,
        "correlation_id": run.run_id,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
        "active_session_ids": run.active_session_ids,
        "latest_session_id": latest_session_id,
        "attach_event_stream": attach_event_stream,
    })
}

pub(super) fn routine_event_to_run_event(event: &EngineEvent) -> Option<EngineEvent> {
    let mut props = event.properties.clone();
    let event_type = match event.event_type.as_str() {
        "routine.run.created" => "run.started",
        "routine.run.started" => "run.step",
        "routine.run.completed" => "run.completed",
        "routine.run.failed" => "run.failed",
        "routine.approval_required" => "approval.required",
        "routine.run.artifact_added" => "run.step",
        "routine.run.model_selected" => "run.step",
        "routine.blocked" => "run.failed",
        _ => return None,
    };
    if let Some(routine_id) = props
        .get("routineID")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("automationID".to_string(), Value::String(routine_id));
    }
    if event.event_type == "routine.run.started"
        || event.event_type == "routine.run.artifact_added"
        || event.event_type == "routine.run.model_selected"
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("phase".to_string(), Value::String("do".to_string()));
    }
    Some(EngineEvent::new(event_type, props))
}

pub(super) fn automation_create_to_routine(
    input: AutomationCreateInput,
) -> Result<RoutineSpec, String> {
    if input.mission.objective.trim().is_empty() {
        return Err("mission.objective is required".to_string());
    }
    let mode = normalize_automation_mode(input.mode.as_deref())?;
    let mut args = json!({
        "prompt": input.mission.objective.trim(),
        "success_criteria": input.mission.success_criteria,
        "mode": mode,
    });
    if let Some(briefing) = input.mission.briefing {
        if let Some(obj) = args.as_object_mut() {
            obj.insert("briefing".to_string(), Value::String(briefing));
        }
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(value) = policy.tool.orchestrator_only_tool_calls {
            if let Some(obj) = args.as_object_mut() {
                obj.insert(
                    "orchestrator_only_tool_calls".to_string(),
                    Value::Bool(value),
                );
            }
        }
    }
    if let Some(model_policy) = input.model_policy {
        validate_model_policy(&model_policy)?;
        if let Some(obj) = args.as_object_mut() {
            obj.insert("model_policy".to_string(), model_policy);
        }
    }
    let (allowed_tools, external_integrations_allowed, requires_approval) =
        if let Some(policy) = input.policy {
            (
                policy.tool.run_allowlist.unwrap_or_default(),
                policy.tool.external_integrations_allowed.unwrap_or(false),
                policy.approval.requires_approval.unwrap_or(true),
            )
        } else {
            (Vec::new(), false, true)
        };
    Ok(RoutineSpec {
        routine_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-{}", uuid::Uuid::new_v4().simple())),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input
            .mission
            .entrypoint_compat
            .unwrap_or_else(|| "mission.default".to_string()),
        args: Value::Object(args.as_object().cloned().unwrap_or_default()),
        allowed_tools,
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: input.creator_type.unwrap_or_else(|| "user".to_string()),
        creator_id: input.creator_id.unwrap_or_else(|| "desktop".to_string()),
        requires_approval,
        external_integrations_allowed,
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    })
}

pub(super) async fn automations_create(
    State(state): State<AppState>,
    Json(input): Json<AutomationCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = automation_create_to_routine(input).map_err(|detail| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid automation definition",
                "code": "AUTOMATION_INVALID",
                "detail": detail,
            })),
        )
    })?;
    let saved = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "automation.updated",
        json!({
            "automationID": saved.routine_id,
        }),
    ));
    Ok(Json(json!({
        "automation": routine_to_automation_wire(saved)
    })))
}

pub(super) async fn automations_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state
        .list_routines()
        .await
        .into_iter()
        .map(routine_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationPatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_NOT_FOUND",
                "automationID": id,
            })),
        )
    })?;
    if let Some(name) = input.name.as_ref() {
        routine.name = name.clone();
    }
    if let Some(status) = input.status.as_ref() {
        routine.status = status.clone();
    }
    if let Some(schedule) = input.schedule.as_ref() {
        routine.schedule = schedule.clone();
    }
    if let Some(timezone) = input.timezone.as_ref() {
        routine.timezone = timezone.clone();
    }
    if let Some(misfire_policy) = input.misfire_policy.as_ref() {
        routine.misfire_policy = misfire_policy.clone();
    }
    if let Some(next_fire_at_ms) = input.next_fire_at_ms {
        routine.next_fire_at_ms = Some(next_fire_at_ms);
    }
    if let Some(output_targets) = input.output_targets.as_ref() {
        routine.output_targets = output_targets.clone();
    }
    if let Some(model_policy) = input.model_policy.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if model_policy
            .as_object()
            .map(|obj| obj.is_empty())
            .unwrap_or(false)
        {
            args.remove("model_policy");
        } else if model_policy.is_object() {
            validate_model_policy(model_policy).map_err(|detail| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid automation patch",
                        "code": "AUTOMATION_INVALID",
                        "detail": detail,
                    })),
                )
            })?;
            args.insert("model_policy".to_string(), model_policy.clone());
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": "model_policy must be an object (use {} to clear)",
                })),
            ));
        }
        routine.args = Value::Object(args);
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(allowed) = policy.tool.run_allowlist.as_ref() {
            routine.allowed_tools = allowed.clone();
        }
        if let Some(external_allowed) = policy.tool.external_integrations_allowed {
            routine.external_integrations_allowed = external_allowed;
        }
        if let Some(requires_approval) = policy.approval.requires_approval {
            routine.requires_approval = requires_approval;
        }
        if let Some(orchestrator_only) = policy.tool.orchestrator_only_tool_calls {
            let mut args = routine.args.as_object().cloned().unwrap_or_default();
            args.insert(
                "orchestrator_only_tool_calls".to_string(),
                Value::Bool(orchestrator_only),
            );
            routine.args = Value::Object(args);
        }
    }
    if let Some(mode) = input.mode.as_ref() {
        let normalized_mode = normalize_automation_mode(Some(mode.as_str())).map_err(|detail| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": detail,
                })),
            )
        })?;
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        args.insert("mode".to_string(), Value::String(normalized_mode));
        routine.args = Value::Object(args);
    }
    if let Some(mission) = input.mission.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if let Some(objective) = mission.objective.as_ref() {
            args.insert("prompt".to_string(), Value::String(objective.clone()));
        }
        if let Some(success_criteria) = mission.success_criteria.as_ref() {
            args.insert("success_criteria".to_string(), json!(success_criteria));
        }
        if let Some(briefing) = mission.briefing.as_ref() {
            args.insert("briefing".to_string(), Value::String(briefing.clone()));
        }
        if let Some(entrypoint) = mission.entrypoint_compat.as_ref() {
            routine.entrypoint = entrypoint.clone();
        }
        routine.args = Value::Object(args);
    }
    let updated = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    Ok(Json(json!({
        "automation": routine_to_automation_wire(updated)
    })))
}

pub(super) async fn automations_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine(&id)
        .await
        .map_err(routine_error_response)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Automation not found",
                    "code":"AUTOMATION_NOT_FOUND",
                    "automationID": id,
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "automation": routine_to_automation_wire(deleted)
    })))
}

pub(super) async fn automations_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_now(State(state.clone()), Path(id), Json(input)).await?;
    let payload = response.0;
    let run_id = payload
        .get("runID")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Run ID missing", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
            )
        })?;
    let run = state.get_routine_run(run_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Run lookup failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
        )
    })?;
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "status": payload.get("status").cloned().unwrap_or(Value::String("queued".to_string())),
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Json<Value> {
    let response = routines_history(State(state), Path(id.clone()), Query(query)).await;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationID".to_string(), Value::String(id));
        object.remove("routineID");
    }
    Json(payload)
}

pub(super) async fn automations_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let runs = state.list_routine_runs(Some(&id), limit).await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    let rows = runs
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let runs = state
        .list_routine_runs(query.routine_id.as_deref(), limit)
        .await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    let rows = runs
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let run = state.get_routine_run(&run_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation run not found",
                "code":"AUTOMATION_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        )
    })?;
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_approve(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_approve(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_deny(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_deny(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_pause(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_resume(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifacts(State(state), Path(run_id.clone())).await?;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationRunID".to_string(), Value::String(run_id));
        object.remove("runID");
    }
    Ok(Json(payload))
}

pub(super) async fn automations_run_artifact_add(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifact_add(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    let artifact = response
        .0
        .get("artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "artifact": artifact,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

fn automations_sse_stream(
    state: AppState,
    automation_id: Option<String>,
    run_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            let mapped = routine_event_to_run_event(&event)?;
            if let Some(automation_id) = automation_id.as_deref() {
                let event_automation_id = mapped
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_automation_id != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = run_id.as_deref() {
                let event_run_id = mapped
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_run_id != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&mapped).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn automations_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(automations_sse_stream(
        state,
        query.automation_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn normalize_automation_v2_agent(
    mut agent: AutomationAgentProfile,
) -> AutomationAgentProfile {
    if agent.display_name.trim().is_empty() {
        agent.display_name = agent.agent_id.clone();
    }
    if agent.tool_policy.allowlist.is_empty() {
        agent.tool_policy = AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        };
    }
    if agent.mcp_policy.allowed_servers.is_empty() {
        agent.mcp_policy = AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        };
    }
    agent
}

pub(super) async fn automations_v2_create(
    State(state): State<AppState>,
    Json(input): Json<AutomationV2CreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let workspace_root = input
        .workspace_root
        .as_deref()
        .map(crate::normalize_absolute_workspace_root)
        .transpose()
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "AUTOMATION_V2_CREATE_FAILED",
                })),
            )
        })?;
    let automation = AutomationV2Spec {
        automation_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-v2-{}", Uuid::new_v4())),
        name: input.name,
        description: input.description,
        status: input.status.unwrap_or(AutomationV2Status::Draft),
        schedule: input.schedule,
        agents: input
            .agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect(),
        flow: input.flow,
        execution: input.execution.unwrap_or(AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        }),
        output_targets: input.output_targets.unwrap_or_default(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: input.creator_id.unwrap_or_else(|| "unknown".to_string()),
        workspace_root,
        metadata: input.metadata,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_CREATE_FAILED",
            })),
        )
    })?;
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state.list_automations_v2().await;
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_v2_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    Ok(Json(json!({ "automation": automation })))
}

pub(super) async fn automations_v2_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2PatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    if let Some(name) = input.name {
        automation.name = name;
    }
    if let Some(description) = input.description {
        automation.description = Some(description);
    }
    if let Some(status) = input.status {
        automation.status = status;
    }
    if let Some(schedule) = input.schedule {
        automation.schedule = schedule;
    }
    if let Some(agents) = input.agents {
        automation.agents = agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect();
    }
    if let Some(flow) = input.flow {
        automation.flow = flow;
    }
    if let Some(execution) = input.execution {
        automation.execution = execution;
    }
    if let Some(output_targets) = input.output_targets {
        automation.output_targets = output_targets;
    }
    if let Some(workspace_root) = input.workspace_root {
        let normalized =
            crate::normalize_absolute_workspace_root(&workspace_root).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": error,
                        "code": "AUTOMATION_V2_UPDATE_FAILED",
                    })),
                )
            })?;
        automation.workspace_root = Some(normalized);
    }
    if let Some(metadata) = input.metadata {
        automation.metadata = Some(metadata);
    }
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_UPDATE_FAILED",
            })),
        )
    })?;
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state.delete_automation_v2(&id).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_DELETE_FAILED",
            })),
        )
    })?;
    if deleted.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    }
    Ok(Json(
        json!({ "ok": true, "deleted": true, "automationID": id }),
    ))
}

pub(super) async fn automations_v2_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_V2_RUN_CREATE_FAILED",
                })),
            )
        })?;
    let _ = super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &run).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_pause(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    automation.status = AutomationV2Status::Paused;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let reason = reason_or_default(input.reason, "paused by operator");
    let runs = state.list_automation_v2_runs(Some(&id), 100).await;
    for run in runs {
        if run.status == AutomationRunStatus::Running {
            let session_ids = run.active_session_ids.clone();
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Pausing;
                    row.pause_reason = Some(reason.clone());
                })
                .await;
            for session_id in run.active_session_ids {
                let _ = state.cancellations.cancel(&session_id).await;
            }
            for instance_id in run.active_instance_ids {
                let _ = state
                    .agent_teams
                    .cancel_instance(&state, &instance_id, "paused by operator")
                    .await;
            }
            state.forget_automation_v2_sessions(&session_ids).await;
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Paused;
                    row.active_session_ids.clear();
                    row.active_instance_ids.clear();
                    crate::record_automation_lifecycle_event(
                        row,
                        "run_paused",
                        row.pause_reason.clone(),
                        None,
                    );
                })
                .await;
        }
    }
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

pub(super) async fn automations_v2_resume(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    automation.status = AutomationV2Status::Active;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

pub(super) async fn automations_v2_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let rows = state.list_automation_v2_runs(Some(&id), limit).await;
    if let Some(automation) = state.get_automation_v2(&id).await {
        for run in &rows {
            let _ =
                super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run)
                    .await;
        }
    }
    let runs = rows
        .iter()
        .map(automation_v2_run_with_context_links)
        .collect::<Vec<_>>();
    Json(json!({ "automationID": id, "runs": runs, "count": rows.len() }))
}

pub(super) async fn automations_v2_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = state.list_automation_v2_runs(None, limit).await;
    for run in &rows {
        if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
            let _ =
                super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run)
                    .await;
        }
    }
    let runs = rows
        .iter()
        .map(automation_v2_run_with_context_links)
        .collect::<Vec<_>>();
    Json(json!({ "runs": runs, "count": rows.len() }))
}

pub(super) async fn automations_v2_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
        let _ =
            super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &run).await;
    }
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "run": automation_v2_run_with_context_links(&run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if !matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not pausable", "code":"AUTOMATION_V2_RUN_NOT_PAUSABLE", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let session_ids = current.active_session_ids.clone();
    let _ = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Pausing;
            run.pause_reason = Some(reason.clone());
            crate::record_automation_lifecycle_event(
                run,
                "run_pause_requested",
                Some(reason.clone()),
                None,
            );
        })
        .await;
    let latest = state.get_automation_v2_run(&run_id).await;
    if let Some(run) = latest {
        for session_id in run.active_session_ids {
            let _ = state.cancellations.cancel(&session_id).await;
        }
        for instance_id in run.active_instance_ids {
            let _ = state
                .agent_teams
                .cancel_instance(&state, &instance_id, "paused by operator")
                .await;
        }
    }
    state.forget_automation_v2_sessions(&session_ids).await;
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Paused;
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_paused",
                run.pause_reason.clone(),
                None,
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if current.status != AutomationRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not paused", "code":"AUTOMATION_V2_RUN_NOT_PAUSED", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            crate::record_automation_lifecycle_event(
                run,
                "run_resumed",
                Some(reason.clone()),
                None,
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_cancel(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Cancelled
            | AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run already terminal", "code":"AUTOMATION_V2_RUN_TERMINAL", "runID": run_id}),
            ),
        ));
    }
    let session_ids = current.active_session_ids.clone();
    for session_id in current.active_session_ids.clone() {
        let _ = state.cancellations.cancel(&session_id).await;
    }
    for instance_id in current.active_instance_ids.clone() {
        let _ = state
            .agent_teams
            .cancel_instance(&state, &instance_id, "cancelled by operator")
            .await;
    }
    state.forget_automation_v2_sessions(&session_ids).await;
    let reason = reason_or_default(input.reason, "cancelled by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Cancelled;
            run.detail = Some(reason.clone());
            run.stop_kind = Some(crate::AutomationStopKind::OperatorStopped);
            run.stop_reason = Some(reason.clone());
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_stopped",
                Some(reason.clone()),
                Some(crate::AutomationStopKind::OperatorStopped),
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_gate_decide(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2GateDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if current.status != AutomationRunStatus::AwaitingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not awaiting approval", "code":"AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL", "runID": run_id}),
            ),
        ));
    }
    let Some(gate) = current.checkpoint.awaiting_gate.clone() else {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run has no pending gate", "code":"AUTOMATION_V2_RUN_GATE_MISSING", "runID": run_id}),
            ),
        ));
    };
    let decision = input.decision.trim().to_ascii_lowercase();
    if !["approve", "rework", "cancel"].contains(&decision.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"decision must be approve, rework, or cancel", "code":"AUTOMATION_V2_GATE_INVALID_DECISION"}),
            ),
        ));
    }
    let Some(automation) = state.get_automation_v2(&current.automation_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let Some(node) = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == gate.node_id)
        .cloned()
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Gate node not found", "code":"AUTOMATION_V2_GATE_NODE_NOT_FOUND", "nodeID": gate.node_id}),
            ),
        ));
    };
    let reason = input
        .reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.checkpoint
                .gate_history
                .push(crate::AutomationGateDecisionRecord {
                    node_id: gate.node_id.clone(),
                    decision: decision.clone(),
                    reason: reason.clone(),
                    decided_at_ms: crate::now_ms(),
                });
            run.checkpoint.awaiting_gate = None;
            match decision.as_str() {
                "approve" => {
                    run.status = AutomationRunStatus::Queued;
                    run.detail = Some(format!("gate `{}` approved", gate.node_id));
                    run.stop_kind = None;
                    run.stop_reason = None;
                    run.checkpoint
                        .pending_nodes
                        .retain(|node_id| node_id != &gate.node_id);
                    if !run
                        .checkpoint
                        .completed_nodes
                        .iter()
                        .any(|node_id| node_id == &gate.node_id)
                    {
                        run.checkpoint.completed_nodes.push(gate.node_id.clone());
                    }
                    run.checkpoint.node_outputs.insert(
                        gate.node_id.clone(),
                        json!({
                            "contract_kind": "approval_gate",
                            "summary": format!("Gate `{}` approved.", gate.node_id),
                            "content": {
                                "decision": "approve",
                                "reason": reason,
                            },
                            "created_at_ms": crate::now_ms(),
                            "node_id": gate.node_id.clone(),
                        }),
                    );
                }
                "rework" => {
                    run.status = AutomationRunStatus::Queued;
                    run.detail = Some(format!("gate `{}` sent work back for rework", gate.node_id));
                    run.stop_kind = None;
                    run.stop_reason = None;
                    let mut roots = gate
                        .rework_targets
                        .iter()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>();
                    if roots.is_empty() {
                        roots.extend(gate.upstream_node_ids.iter().cloned());
                    }
                    roots.insert(gate.node_id.clone());
                    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
                    for node_id in &reset_nodes {
                        run.checkpoint.node_outputs.remove(node_id);
                        run.checkpoint.node_attempts.remove(node_id);
                    }
                    run.checkpoint
                        .completed_nodes
                        .retain(|node_id| !reset_nodes.contains(node_id));
                    let mut pending = run.checkpoint.pending_nodes.clone();
                    for node_id in reset_nodes {
                        if !pending.iter().any(|existing| existing == &node_id) {
                            pending.push(node_id);
                        }
                    }
                    pending.sort();
                    pending.dedup();
                    run.checkpoint.pending_nodes = pending;
                }
                "cancel" => {
                    run.status = AutomationRunStatus::Cancelled;
                    let stop_reason = reason
                        .clone()
                        .unwrap_or_else(|| format!("gate `{}` cancelled the run", gate.node_id));
                    run.detail = Some(stop_reason.clone());
                    run.stop_kind = Some(crate::AutomationStopKind::Cancelled);
                    run.stop_reason = Some(stop_reason.clone());
                    crate::record_automation_lifecycle_event(
                        run,
                        "run_cancelled",
                        Some(stop_reason),
                        Some(crate::AutomationStopKind::Cancelled),
                    );
                }
                _ => {}
            }
            if decision != "cancel" {
                run.resume_reason = Some(format!("gate `{}` decision: {}", gate.node_id, decision));
                crate::refresh_automation_runtime_state(&automation, run);
            }
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let _ = node;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_recover(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if !matches!(
        current.status,
        AutomationRunStatus::Failed | AutomationRunStatus::Paused
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not recoverable", "code":"AUTOMATION_V2_RUN_NOT_RECOVERABLE", "runID": run_id}),
            ),
        ));
    }
    let Some(automation) = state.get_automation_v2(&current.automation_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let reset_nodes = if current.status == AutomationRunStatus::Failed {
        let Some(failure) = current.checkpoint.last_failure.clone() else {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    json!({"error":"Run has no recoverable failed node", "code":"AUTOMATION_V2_RUN_FAILURE_CONTEXT_MISSING", "runID": run_id}),
                ),
            ));
        };
        let roots =
            std::iter::once(failure.node_id.clone()).collect::<std::collections::HashSet<_>>();
        crate::collect_automation_descendants(&automation, &roots)
    } else {
        std::collections::HashSet::new()
    };
    let reason = if current.status == AutomationRunStatus::Paused {
        reason_or_default(input.reason, "recovered from paused state by operator")
    } else {
        reason_or_default(input.reason, "recovered by operator")
    };
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.checkpoint.awaiting_gate = None;
            if !reset_nodes.is_empty() {
                for node_id in &reset_nodes {
                    run.checkpoint.node_outputs.remove(node_id);
                    run.checkpoint.node_attempts.remove(node_id);
                }
                run.checkpoint
                    .completed_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                let mut pending = run.checkpoint.pending_nodes.clone();
                for node_id in &reset_nodes {
                    if !pending.iter().any(|existing| existing == node_id) {
                        pending.push(node_id.clone());
                    }
                }
                pending.sort();
                pending.dedup();
                run.checkpoint.pending_nodes = pending;
                run.checkpoint.last_failure = None;
            }
            crate::record_automation_lifecycle_event(
                run,
                if reset_nodes.is_empty() {
                    "run_recovered_from_pause"
                } else {
                    "run_recovered"
                },
                Some(reason.clone()),
                None,
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_repair(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2RunRepairInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = input.node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"node_id is required", "code":"AUTOMATION_V2_REPAIR_NODE_REQUIRED"}),
            ),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run must be paused, failed, awaiting approval, or cancelled before repair", "code":"AUTOMATION_V2_RUN_NOT_REPAIRABLE", "runID": run_id}),
            ),
        ));
    }
    let Some(mut automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let Some(node) = automation
        .flow
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Node not found", "code":"AUTOMATION_V2_REPAIR_NODE_NOT_FOUND", "nodeID": node_id}),
            ),
        ));
    };
    let agent_id = node.agent_id.clone();
    let previous_prompt = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("prompt"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let prompt = input
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let template_id = input
        .template_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model_policy = input.model_policy.clone();
    if let Some(prompt_value) = prompt.as_ref() {
        let metadata = node.metadata.get_or_insert_with(|| json!({}));
        let builder = metadata
            .as_object_mut()
            .and_then(|root| root.entry("builder").or_insert_with(|| json!({})).as_object_mut())
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"Node metadata is not repairable", "code":"AUTOMATION_V2_REPAIR_METADATA_INVALID"})),
                )
            })?;
        builder.insert("prompt".to_string(), Value::String(prompt_value.clone()));
    }
    let previous_agent = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
    if template_id.is_some() || model_policy.is_some() {
        let Some(agent) = automation
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(
                    json!({"error":"Node agent not found", "code":"AUTOMATION_V2_REPAIR_AGENT_NOT_FOUND", "agentID": agent_id}),
                ),
            ));
        };
        if let Some(template_value) = template_id.clone() {
            agent.template_id = Some(template_value);
        }
        if let Some(model_policy_value) = model_policy.clone() {
            agent.model_policy = Some(model_policy_value);
        }
    }
    automation.updated_at_ms = crate::now_ms();
    let stored_automation = state.put_automation_v2(automation.clone()).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_PERSIST_FAILED"})),
        )
    })?;
    let roots = std::iter::once(node_id.clone()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&stored_automation, &roots);
    let cleared_outputs = crate::clear_automation_subtree_outputs(&state, &stored_automation, &reset_nodes)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_OUTPUT_RESET_FAILED"})),
            )
        })?;
    let reason = reason_or_default(
        input.reason,
        &format!("repaired node `{}` and reset affected subtree", node_id),
    );
    let updated_agent = stored_automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(stored_automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                "run_step_repaired",
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes.iter().cloned().collect::<Vec<_>>(),
                    "prompt_updated": prompt.is_some(),
                    "template_updated": template_id.is_some(),
                    "model_policy_updated": model_policy.is_some(),
                    "reset_only": prompt.is_none() && template_id.is_none() && model_policy.is_none(),
                    "cleared_outputs": cleared_outputs,
                    "previous_prompt": previous_prompt,
                    "new_prompt": prompt,
                    "previous_template_id": previous_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "new_template_id": updated_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "previous_model_policy": previous_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                    "new_model_policy": updated_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                })),
            );
            crate::refresh_automation_runtime_state(&stored_automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ = super::context_runs::sync_automation_v2_run_blackboard(
        &state,
        &stored_automation,
        &updated,
    )
    .await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "automation": stored_automation,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

async fn automation_v2_reset_task_subtree(
    state: &AppState,
    run_id: &str,
    node_id: &str,
    reason: String,
    lifecycle_event: &str,
) -> Result<
    (
        AutomationV2Spec,
        crate::AutomationV2RunRecord,
        Vec<String>,
        Vec<String>,
    ),
    (StatusCode, Json<Value>),
> {
    let Some(current) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Run must be paused, blocked, failed, awaiting approval, completed, or cancelled before task reset",
                "code":"AUTOMATION_V2_RUN_TASK_NOT_MUTABLE",
                "runID": run_id
            })),
        ));
    }
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let roots = std::iter::once(node_id.to_string()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
    let cleared_outputs = crate::clear_automation_subtree_outputs(state, &automation, &reset_nodes)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code":"AUTOMATION_V2_TASK_RESET_OUTPUT_CLEAR_FAILED"
                })),
            )
        })?;
    let mut reset_nodes_list = reset_nodes.iter().cloned().collect::<Vec<_>>();
    reset_nodes_list.sort();
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                lifecycle_event,
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes_list.clone(),
                    "cleared_outputs": cleared_outputs.clone(),
                })),
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Run update failed",
                    "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"
                })),
            )
        })?;
    Ok((automation, updated, cleared_outputs, reset_nodes_list))
}

async fn automation_v2_task_reset_preview(
    state: &AppState,
    run_id: &str,
    node_id: &str,
) -> Result<AutomationV2TaskResetPreview, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let roots = std::iter::once(node_id.to_string()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
    let mut reset_nodes_list = reset_nodes.iter().cloned().collect::<Vec<_>>();
    reset_nodes_list.sort();
    let mut cleared_outputs = automation
        .flow
        .nodes
        .iter()
        .filter(|node| reset_nodes.contains(&node.node_id))
        .filter_map(crate::automation_node_required_output_path)
        .collect::<Vec<_>>();
    cleared_outputs.sort();
    cleared_outputs.dedup();
    Ok(AutomationV2TaskResetPreview {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        reset_nodes: reset_nodes_list,
        cleared_outputs,
        preserves_upstream_outputs: true,
    })
}

async fn load_automation_v2_backlog_task(
    state: &AppState,
    run_id: &str,
    task_id: &str,
) -> Result<crate::http::context_types::ContextBlackboardTask, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let blackboard = super::context_runs::load_projected_context_blackboard(state, &context_run_id);
    let Some(task) = blackboard.tasks.into_iter().find(|task| task.id == task_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Backlog task not found",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_FOUND",
                "taskID": task_id
            })),
        ));
    };
    if task.task_type != "automation_backlog_item" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Task is not a projected backlog item",
                "code":"AUTOMATION_V2_BACKLOG_TASK_INVALID_TYPE",
                "taskID": task_id
            })),
        ));
    }
    Ok(task)
}

fn automation_v2_backlog_claim_agent(
    task: &crate::http::context_types::ContextBlackboardTask,
    requested_agent_id: Option<String>,
) -> String {
    requested_agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            task.payload
                .get("task_owner")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| task.assigned_agent.clone())
        .unwrap_or_else(|| "backlog-worker".to_string())
}

pub(super) async fn automations_v2_run_task_reset_preview(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let preview = automation_v2_task_reset_preview(&state, &run_id, &node_id).await?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "preview": preview,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_task_continue(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Run must be blocked, paused, failed, awaiting approval, completed, or cancelled before continue",
                "code":"AUTOMATION_V2_RUN_TASK_NOT_CONTINUEABLE",
                "runID": run_id
            })),
        ));
    }
    let is_blocked = current
        .checkpoint
        .blocked_nodes
        .iter()
        .any(|blocked| blocked == &node_id)
        || current
            .checkpoint
            .node_outputs
            .get(&node_id)
            .and_then(|value| value.get("status").and_then(Value::as_str))
            .map(|value| value.eq_ignore_ascii_case("blocked"))
            .unwrap_or(false);
    if !is_blocked {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Task is not blocked",
                "code":"AUTOMATION_V2_TASK_NOT_BLOCKED",
                "nodeID": node_id
            })),
        ));
    }
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let reset_nodes = std::iter::once(node_id.clone()).collect::<std::collections::HashSet<_>>();
    let cleared_outputs =
        crate::clear_automation_subtree_outputs(&state, &automation, &reset_nodes)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code":"AUTOMATION_V2_TASK_CONTINUE_OUTPUT_CLEAR_FAILED"
                    })),
                )
            })?;
    let reason = reason_or_default(
        input.reason,
        &format!("continued blocked task `{}` with minimal reset", node_id),
    );
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            run.checkpoint.node_outputs.remove(&node_id);
            run.checkpoint.node_attempts.remove(&node_id);
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| completed_id != &node_id);
            if !run
                .checkpoint
                .pending_nodes
                .iter()
                .any(|pending| pending == &node_id)
            {
                run.checkpoint.pending_nodes.push(node_id.clone());
            }
            run.checkpoint.pending_nodes.sort();
            run.checkpoint.pending_nodes.dedup();
            if run
                .checkpoint
                .last_failure
                .as_ref()
                .map(|failure| failure.node_id == node_id)
                .unwrap_or(false)
            {
                run.checkpoint.last_failure = None;
            }
            run.automation_snapshot = Some(automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                "run_task_continued",
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": vec![node_id.clone()],
                    "cleared_outputs": cleared_outputs,
                    "mode": "minimal_reset",
                })),
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Run update failed",
                    "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"
                })),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "node_id": node_id,
        "reset_nodes": vec![node_id],
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_task_retry(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let reason = reason_or_default(
        input.reason,
        &format!("retried task `{}` and reset affected subtree", node_id),
    );
    let (automation, updated, cleared_outputs, reset_nodes) =
        automation_v2_reset_task_subtree(&state, &run_id, &node_id, reason, "run_task_retried")
            .await?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "node_id": node_id,
        "reset_nodes": reset_nodes,
        "cleared_outputs": cleared_outputs,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_task_requeue(
    State(state): State<AppState>,
    Path((run_id, node_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"node_id is required",
                "code":"AUTOMATION_V2_TASK_NODE_REQUIRED"
            })),
        ));
    }
    let reason = reason_or_default(
        input.reason,
        &format!("requeued task `{}` and reset affected subtree", node_id),
    );
    let (automation, updated, cleared_outputs, reset_nodes) =
        automation_v2_reset_task_subtree(&state, &run_id, &node_id, reason, "run_task_requeued")
            .await?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "ok": true,
        "run": automation_v2_run_with_context_links(&updated),
        "node_id": node_id,
        "reset_nodes": reset_nodes,
        "cleared_outputs": cleared_outputs,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_backlog_task_claim(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2BacklogClaimInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"task_id is required",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUIRED"
            })),
        ));
    }
    let task = load_automation_v2_backlog_task(&state, &run_id, &task_id).await?;
    let agent_id = automation_v2_backlog_claim_agent(&task, input.agent_id);
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    let command_id = Some(format!(
        "automation-v2-backlog-claim:{run_id}:{task_id}:{agent_id}"
    ));
    let claimed = super::context_runs::claim_context_task_by_id(
        &state,
        &context_run_id,
        &task_id,
        &agent_id,
        input.lease_ms,
        command_id,
    )
    .await
    .map_err(|status| {
        (
            status,
            Json(json!({
                "error":"Backlog claim failed",
                "code":"AUTOMATION_V2_BACKLOG_TASK_CLAIM_FAILED",
                "taskID": task_id
            })),
        )
    })?;
    let Some(task) = claimed else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Backlog task is not claimable",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_CLAIMABLE",
                "taskID": task_id
            })),
        ));
    };
    let blackboard =
        super::context_runs::load_projected_context_blackboard(&state, &context_run_id);
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "agent_id": agent_id,
        "reason": reason_or_default(
            input.reason,
            &format!("claimed backlog task `{}`", task_id),
        ),
        "blackboard": blackboard,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_backlog_task_requeue(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<AutomationV2RunTaskActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let task_id = task_id.trim().to_string();
    if task_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"task_id is required",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUIRED"
            })),
        ));
    }
    let task = load_automation_v2_backlog_task(&state, &run_id, &task_id).await?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    let reason = reason_or_default(
        input.reason,
        &format!("requeued backlog task `{}`", task_id),
    );
    let requeued = super::context_runs::requeue_context_task_by_id(
        &state,
        &context_run_id,
        &task_id,
        Some(format!("automation-v2-backlog-requeue:{run_id}:{task_id}")),
        Some(reason.clone()),
    )
    .await
    .map_err(|status| {
        (
            status,
            Json(json!({
                "error":"Backlog requeue failed",
                "code":"AUTOMATION_V2_BACKLOG_TASK_REQUEUE_FAILED",
                "taskID": task_id
            })),
        )
    })?;
    let Some(task) = requeued else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Backlog task is not requeueable",
                "code":"AUTOMATION_V2_BACKLOG_TASK_NOT_REQUEUEABLE",
                "taskID": task_id,
                "status": task.status,
            })),
        ));
    };
    let blackboard =
        super::context_runs::load_projected_context_blackboard(&state, &context_run_id);
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "reason": reason,
        "blackboard": blackboard,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations_v2",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("automation.v2.") {
                return None;
            }
            if let Some(automation_id) = query.automation_id.as_deref() {
                let value = event
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = query.run_id.as_deref() {
                let value = event
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    Sse::new(ready.chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
