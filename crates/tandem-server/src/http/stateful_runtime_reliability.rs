use super::*;

use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, list_stateful_compensations,
    list_stateful_dead_letters, list_stateful_outbox, list_stateful_tool_effects,
    mark_compensation_status, mark_dead_letter_disposition, operator_principal,
    stateful_reliability_path_from_runtime_events_path, stateful_run_from_automation_v2,
    stateful_run_from_workflow, StatefulCompensationStatus, StatefulDeadLetterStatus,
    StatefulReliabilityQuery, StatefulRunEventRecord, StatefulWorkflowRunRecord,
    StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use tandem_types::TenantContext;

const DEFAULT_RELIABILITY_API_LIMIT: usize = 250;
const MAX_RELIABILITY_API_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulReliabilityListQuery {
    pub run_id: Option<String>,
    pub status: Option<String>,
    pub source_type: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulResumePlanActionInput {
    pub choice: String,
    pub reason: Option<String>,
    pub actor_id: Option<String>,
    pub dead_letter_id: Option<String>,
    pub compensation_id: Option<String>,
    pub target_effect_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RunRecoveryContext {
    run: Option<StatefulWorkflowRunRecord>,
    completed_nodes: Vec<String>,
    pending_nodes: Vec<String>,
    blocked_nodes: Vec<String>,
    last_failure: Option<Value>,
}

pub(super) async fn list_stateful_reliability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<StatefulReliabilityListQuery>,
) -> Json<Value> {
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let limit = limit(query.limit);
    let query = reliability_query(&query, limit);
    let outbox = list_stateful_outbox(&path, &tenant_context, query);
    let tool_effects = list_stateful_tool_effects(&path, &tenant_context, query);
    let dead_letters = list_stateful_dead_letters(&path, &tenant_context, query);
    let compensations = list_stateful_compensations(&path, &tenant_context, query);

    Json(json!({
        "source": "stateful_runtime_reliability",
        "outbox": outbox,
        "tool_effects": tool_effects,
        "dead_letters": dead_letters,
        "compensations": compensations,
        "counts": {
            "outbox": outbox.len(),
            "tool_effects": tool_effects.len(),
            "dead_letters": dead_letters.len(),
            "compensations": compensations.len(),
        },
        "limit": limit,
    }))
}

pub(super) async fn get_stateful_run_reliability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulReliabilityListQuery>,
) -> Response {
    let Some(context) = find_run_recovery_context(&state, &tenant_context, &run_id).await else {
        return stateful_run_not_found(run_id).into_response();
    };
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let limit = limit(query.limit);
    let query = StatefulReliabilityQuery {
        run_id: Some(&run_id),
        status: query.status.as_deref(),
        source_type: query.source_type.as_deref(),
        limit: Some(limit),
    };
    let outbox = list_stateful_outbox(&path, &tenant_context, query);
    let tool_effects = list_stateful_tool_effects(&path, &tenant_context, query);
    let dead_letters = list_stateful_dead_letters(&path, &tenant_context, query);
    let compensations = list_stateful_compensations(&path, &tenant_context, query);

    Json(json!({
        "run_id": run_id,
        "run": context.run,
        "outbox": outbox,
        "tool_effects": tool_effects,
        "dead_letters": dead_letters,
        "compensations": compensations,
        "counts": {
            "outbox": outbox.len(),
            "tool_effects": tool_effects.len(),
            "dead_letters": dead_letters.len(),
            "compensations": compensations.len(),
        },
        "limit": limit,
    }))
    .into_response()
}

pub(super) async fn get_stateful_run_resume_plan(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulReliabilityListQuery>,
) -> Response {
    let Some(context) = find_run_recovery_context(&state, &tenant_context, &run_id).await else {
        return stateful_run_not_found(run_id).into_response();
    };
    let plan = build_resume_plan(
        &state,
        &tenant_context,
        &run_id,
        context,
        limit(query.limit),
    )
    .await;
    Json(plan).into_response()
}

pub(super) async fn apply_stateful_run_resume_plan_action(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Json(input): Json<StatefulResumePlanActionInput>,
) -> Response {
    let Some(context) = find_run_recovery_context(&state, &tenant_context, &run_id).await else {
        return stateful_run_not_found(run_id).into_response();
    };
    let Some(run) = context.run.clone() else {
        return stateful_run_not_found(run_id).into_response();
    };
    let choice = normalize_choice(&input.choice);
    if choice.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "stateful_recovery_choice_required",
            })),
        )
            .into_response();
    }

    let now = crate::now_ms();
    let actor = operator_principal(
        input
            .actor_id
            .as_deref()
            .or(tenant_context.actor_id.as_deref()),
    );
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let mut disposition = Value::Null;

    if let Some(dead_letter_id) = input.dead_letter_id.as_deref() {
        let (status, label) = dead_letter_status_for_choice(&choice);
        match mark_dead_letter_disposition(
            &path,
            &tenant_context,
            dead_letter_id,
            status,
            label,
            input.reason.clone(),
            actor.clone(),
            now,
        )
        .await
        {
            Ok(Some(row)) => disposition = json!({ "dead_letter": row }),
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "stateful_dead_letter_not_found",
                        "dead_letter_id": dead_letter_id,
                    })),
                )
                    .into_response()
            }
            Err(error) => return reliability_error("dead_letter_update_failed", error),
        }
    }

    if let Some(compensation_id) = input.compensation_id.as_deref() {
        let status = compensation_status_for_choice(&choice);
        match mark_compensation_status(&path, &tenant_context, compensation_id, status, now).await {
            Ok(Some(row)) => {
                disposition = json!({
                    "previous": disposition,
                    "compensation": row,
                });
            }
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "stateful_compensation_not_found",
                        "compensation_id": compensation_id,
                    })),
                )
                    .into_response()
            }
            Err(error) => return reliability_error("compensation_update_failed", error),
        }
    }

    let event_path =
        crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
            &state.runtime_events_path,
        )
        .run_events_path;
    let event = StatefulRunEventRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        event_id: format!("recovery-choice-{run_id}-{now}"),
        run_id: run_id.clone(),
        seq: 0,
        event_type: "runtime.recovery_choice.recorded".to_string(),
        occurred_at_ms: now,
        scope: run.scope.clone(),
        actor: Some(actor),
        phase_id: run.current_phase_id.clone(),
        phase_transition: None,
        wait_kind: run.active_wait_kind.clone(),
        causation_id: input
            .dead_letter_id
            .clone()
            .or(input.compensation_id.clone()),
        correlation_id: input.target_effect_id.clone(),
        payload: json!({
            "choice": choice,
            "reason": input.reason,
            "dead_letter_id": input.dead_letter_id,
            "compensation_id": input.compensation_id,
            "target_effect_id": input.target_effect_id,
            "disposition": disposition,
        }),
    };
    let (recorded, seq) =
        match append_stateful_run_event_once_with_next_seq(&event_path, &tenant_context, &event)
            .await
        {
            Ok(result) => result,
            Err(error) => return reliability_error("recovery_choice_event_append_failed", error),
        };

    Json(json!({
        "run_id": run_id,
        "choice": choice,
        "recorded": recorded,
        "event_seq": seq,
        "disposition": disposition,
    }))
    .into_response()
}

async fn build_resume_plan(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
    context: RunRecoveryContext,
    limit: usize,
) -> Value {
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let query = StatefulReliabilityQuery {
        run_id: Some(run_id),
        status: None,
        source_type: None,
        limit: Some(limit),
    };
    let effects = list_stateful_tool_effects(&path, tenant_context, query);
    let dead_letters = list_stateful_dead_letters(&path, tenant_context, query);
    let compensations = list_stateful_compensations(&path, tenant_context, query);
    let completed_effects = effects
        .iter()
        .filter(|effect| {
            effect.status == crate::stateful_runtime::StatefulToolEffectStatus::Succeeded
        })
        .cloned()
        .collect::<Vec<_>>();
    let uncertain_effects = effects
        .iter()
        .filter(|effect| {
            effect.status != crate::stateful_runtime::StatefulToolEffectStatus::Succeeded
        })
        .cloned()
        .collect::<Vec<_>>();
    let safe_resume_points = safe_resume_points(&context);
    let operator_choices = operator_choices(
        &safe_resume_points,
        &uncertain_effects,
        &dead_letters,
        &compensations,
    );
    let run_status = context.run.as_ref().map(|run| &run.status);

    json!({
        "plan_id": format!("resume-plan-{run_id}"),
        "run_id": run_id,
        "generated_at_ms": crate::now_ms(),
        "run": context.run,
        "run_status": run_status,
        "completed_nodes": context.completed_nodes,
        "pending_nodes": context.pending_nodes,
        "blocked_nodes": context.blocked_nodes,
        "last_failure": context.last_failure,
        "completed_effects": completed_effects,
        "uncertain_effects": uncertain_effects,
        "pending_compensations": compensations,
        "dead_letters": dead_letters,
        "safe_resume_points": safe_resume_points,
        "operator_choices": operator_choices,
        "audit_summary": {
            "completed_effect_count": completed_effects.len(),
            "uncertain_effect_count": uncertain_effects.len(),
            "dead_letter_count": dead_letters.len(),
            "pending_compensation_count": compensations.len(),
            "requires_operator_review": !uncertain_effects.is_empty() || !dead_letters.is_empty() || !compensations.is_empty(),
        },
    })
}

async fn find_run_recovery_context(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
) -> Option<RunRecoveryContext> {
    let automation_runs = state.automation_v2_runs.read().await;
    if let Some(run) = automation_runs.get(run_id) {
        let stateful = stateful_run_from_automation_v2(run);
        if stateful.scope.visible_to_tenant(tenant_context) {
            return Some(RunRecoveryContext {
                run: Some(stateful),
                completed_nodes: run.checkpoint.completed_nodes.clone(),
                pending_nodes: run.checkpoint.pending_nodes.clone(),
                blocked_nodes: run.checkpoint.blocked_nodes.clone(),
                last_failure: run
                    .checkpoint
                    .last_failure
                    .as_ref()
                    .and_then(|failure| serde_json::to_value(failure).ok()),
            });
        }
    }
    drop(automation_runs);

    let workflow_runs = state.workflow_runs.read().await;
    if let Some(run) = workflow_runs.get(run_id) {
        let stateful = stateful_run_from_workflow(run);
        if stateful.scope.visible_to_tenant(tenant_context) {
            return Some(RunRecoveryContext {
                run: Some(stateful),
                ..Default::default()
            });
        }
    }
    None
}

fn safe_resume_points(context: &RunRecoveryContext) -> Vec<Value> {
    let mut points = Vec::new();
    for node_id in &context.completed_nodes {
        points.push(json!({
            "kind": "completed_node_boundary",
            "node_id": node_id,
            "resume_after": true,
        }));
    }
    for node_id in &context.pending_nodes {
        points.push(json!({
            "kind": "pending_node",
            "node_id": node_id,
            "retry_safe": true,
        }));
    }
    for node_id in &context.blocked_nodes {
        points.push(json!({
            "kind": "blocked_node",
            "node_id": node_id,
            "requires_operator_review": true,
        }));
    }
    if points.is_empty()
        && context
            .run
            .as_ref()
            .is_some_and(|run| recovery_status_needs_plan(&run.status))
    {
        points.push(json!({
            "kind": "run_boundary",
            "requires_operator_review": true,
        }));
    }
    points
}

fn operator_choices(
    safe_resume_points: &[Value],
    uncertain_effects: &[crate::stateful_runtime::StatefulToolEffectRecord],
    dead_letters: &[crate::stateful_runtime::StatefulDeadLetterRecord],
    compensations: &[crate::stateful_runtime::StatefulCompensationRecord],
) -> Vec<Value> {
    let mut choices = vec![json!({
        "choice": "abandon_with_audit",
        "enabled": true,
    })];
    if !safe_resume_points.is_empty() {
        choices.push(json!({
            "choice": "resume_from_checkpoint",
            "enabled": true,
        }));
    }
    if !uncertain_effects.is_empty() || !dead_letters.is_empty() {
        choices.push(json!({
            "choice": "retry_failed_effect",
            "enabled": true,
        }));
        choices.push(json!({
            "choice": "reconcile_external_effect",
            "enabled": true,
        }));
    }
    if !compensations.is_empty() {
        choices.push(json!({
            "choice": "compensate_pending_effects",
            "enabled": true,
        }));
    }
    choices
}

fn reliability_query<'a>(
    query: &'a StatefulReliabilityListQuery,
    limit: usize,
) -> StatefulReliabilityQuery<'a> {
    StatefulReliabilityQuery {
        run_id: query.run_id.as_deref(),
        status: query.status.as_deref(),
        source_type: query.source_type.as_deref(),
        limit: Some(limit),
    }
}

fn limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_RELIABILITY_API_LIMIT)
        .clamp(1, MAX_RELIABILITY_API_LIMIT)
}

fn recovery_status_needs_plan(status: &StatefulWorkflowRunStatus) -> bool {
    matches!(
        status,
        StatefulWorkflowRunStatus::Paused
            | StatefulWorkflowRunStatus::Blocked
            | StatefulWorkflowRunStatus::Failed
            | StatefulWorkflowRunStatus::DeadLettered
            | StatefulWorkflowRunStatus::Retrying
    )
}

fn dead_letter_status_for_choice(choice: &str) -> (StatefulDeadLetterStatus, &'static str) {
    match choice {
        "retry_failed_effect" | "retry_dead_letter" | "resume_from_checkpoint" => {
            (StatefulDeadLetterStatus::RetryRequested, "retry_requested")
        }
        "compensate_pending_effects" | "compensate" => (
            StatefulDeadLetterStatus::LinkedToCompensation,
            "linked_to_compensation",
        ),
        "abandon_with_audit" | "ignore_dead_letter" => {
            (StatefulDeadLetterStatus::Ignored, "ignored")
        }
        _ => (StatefulDeadLetterStatus::Open, "reviewed"),
    }
}

fn compensation_status_for_choice(choice: &str) -> StatefulCompensationStatus {
    match choice {
        "compensate_pending_effects" | "compensate" => StatefulCompensationStatus::AwaitingApproval,
        "abandon_with_audit" => StatefulCompensationStatus::Cancelled,
        _ => StatefulCompensationStatus::Proposed,
    }
}

fn normalize_choice(choice: &str) -> String {
    choice.trim().replace('-', "_").to_ascii_lowercase()
}

fn stateful_run_not_found(run_id: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "stateful_run_not_found",
            "run_id": run_id,
        })),
    )
}

fn reliability_error(code: &str, error: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": code,
            "message": error.to_string(),
        })),
    )
        .into_response()
}
