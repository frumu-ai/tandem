use super::*;

use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, list_stateful_compensations,
    list_stateful_dead_letters, list_stateful_outbox, list_stateful_tool_effects,
    load_stateful_reliability, mark_compensation_status, mark_dead_letter_disposition,
    operator_principal, stateful_reliability_path_from_runtime_events_path,
    stateful_run_from_automation_v2, stateful_run_from_workflow, StatefulCompensationStatus,
    StatefulDeadLetterStatus, StatefulReliabilityQuery, StatefulRunEventRecord,
    StatefulWorkflowRunRecord, StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use serde::Serialize;
use tandem_types::TenantContext;

const DEFAULT_RELIABILITY_API_LIMIT: usize = 250;
const MAX_RELIABILITY_API_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulReliabilityListQuery {
    pub run_id: Option<String>,
    pub status: Option<String>,
    pub source_type: Option<String>,
    pub after_id: Option<String>,
    #[serde(
        alias = "after_kind",
        alias = "afterCollection",
        alias = "after_collection"
    )]
    pub after_collection: Option<String>,
    #[serde(
        alias = "before_created_at",
        alias = "beforeCreatedAtMs",
        alias = "beforeCreatedAt"
    )]
    pub before_created_at_ms: Option<u64>,
    #[serde(default, alias = "activeRecoveryOnly", alias = "active_only")]
    pub active_recovery_only: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReliabilityCollection {
    Outbox,
    ToolEffects,
    DeadLetters,
    Compensations,
}

impl ReliabilityCollection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Outbox => "outbox",
            Self::ToolEffects => "tool_effects",
            Self::DeadLetters => "dead_letters",
            Self::Compensations => "compensations",
        }
    }

    fn from_query(value: Option<&str>) -> Option<Self> {
        match value.map(normalize_choice).as_deref() {
            Some("outbox") => Some(Self::Outbox),
            Some("tool_effect" | "tool_effects") => Some(Self::ToolEffects),
            Some("dead_letter" | "dead_letters") => Some(Self::DeadLetters),
            Some("compensation" | "compensations") => Some(Self::Compensations),
            _ => None,
        }
    }
}

pub(super) async fn list_stateful_reliability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<StatefulReliabilityListQuery>,
) -> Json<Value> {
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let limit = limit(query.limit);
    let cursor_collection = reliability_cursor_collection(&path, &tenant_context, &query, None);
    let stale_cursor = reliability_cursor_is_stale(&query, cursor_collection);
    let outbox = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_outbox(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                None,
                limit,
                cursor_collection,
                ReliabilityCollection::Outbox,
            ),
        )
    };
    let tool_effects = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_tool_effects(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                None,
                limit,
                cursor_collection,
                ReliabilityCollection::ToolEffects,
            ),
        )
    };
    let dead_letters = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_dead_letters(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                None,
                limit,
                cursor_collection,
                ReliabilityCollection::DeadLetters,
            ),
        )
    };
    let compensations = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_compensations(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                None,
                limit,
                cursor_collection,
                ReliabilityCollection::Compensations,
            ),
        )
    };
    let pagination = reliability_pagination(
        query.after_id.as_deref(),
        cursor_collection,
        query.before_created_at_ms,
        limit,
        &outbox,
        &tool_effects,
        &dead_letters,
        &compensations,
    );

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
        "pagination": pagination,
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
    let cursor_collection =
        reliability_cursor_collection(&path, &tenant_context, &query, Some(run_id.as_str()));
    let stale_cursor = reliability_cursor_is_stale(&query, cursor_collection);
    let outbox = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_outbox(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                Some(run_id.as_str()),
                limit,
                cursor_collection,
                ReliabilityCollection::Outbox,
            ),
        )
    };
    let tool_effects = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_tool_effects(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                Some(run_id.as_str()),
                limit,
                cursor_collection,
                ReliabilityCollection::ToolEffects,
            ),
        )
    };
    let dead_letters = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_dead_letters(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                Some(run_id.as_str()),
                limit,
                cursor_collection,
                ReliabilityCollection::DeadLetters,
            ),
        )
    };
    let compensations = if stale_cursor {
        Vec::new()
    } else {
        list_stateful_compensations(
            &path,
            &tenant_context,
            reliability_query(
                &query,
                Some(run_id.as_str()),
                limit,
                cursor_collection,
                ReliabilityCollection::Compensations,
            ),
        )
    };
    let pagination = reliability_pagination(
        query.after_id.as_deref(),
        cursor_collection,
        query.before_created_at_ms,
        limit,
        &outbox,
        &tool_effects,
        &dead_letters,
        &compensations,
    );

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
        "pagination": pagination,
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
    let execution_mode = recovery_choice_execution_mode(&choice);
    let automatic_dispatch = recovery_choice_automatic_dispatch(&choice);

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
            "execution_mode": execution_mode,
            "automatic_dispatch": automatic_dispatch,
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

    // TAN-564: for a retry choice, actually re-execute the failed effect now by
    // re-driving the owning run through its governed dispatch path, rather than
    // only recording intent. The dispatcher also runs on every executor tick, so
    // this is a latency optimization, not the sole trigger.
    let dispatched = if automatic_dispatch && input.dead_letter_id.is_some() {
        state.dispatch_ready_stateful_dead_letter_retries().await
    } else {
        0
    };

    Json(json!({
        "run_id": run_id,
        "choice": choice,
        "execution_mode": execution_mode,
        "automatic_dispatch": automatic_dispatch,
        "dispatched": dispatched,
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
        after_id: None,
        before_created_at_ms: None,
        active_recovery_only: true,
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
    let mut choices = vec![operator_choice("abandon_with_audit", true)];
    if !safe_resume_points.is_empty() {
        choices.push(operator_choice("resume_from_checkpoint", true));
    }
    if !uncertain_effects.is_empty() || !dead_letters.is_empty() {
        choices.push(operator_choice("retry_failed_effect", true));
        choices.push(operator_choice("reconcile_external_effect", true));
    }
    if !compensations.is_empty() {
        choices.push(operator_choice("compensate_pending_effects", true));
    }
    choices
}

fn operator_choice(choice: &str, enabled: bool) -> Value {
    json!({
        "choice": choice,
        "enabled": enabled,
        "execution_mode": recovery_choice_execution_mode(choice),
        "automatic_dispatch": recovery_choice_automatic_dispatch(choice),
    })
}

fn reliability_query<'a>(
    query: &'a StatefulReliabilityListQuery,
    run_id: Option<&'a str>,
    limit: usize,
    cursor_collection: Option<ReliabilityCollection>,
    collection: ReliabilityCollection,
) -> StatefulReliabilityQuery<'a> {
    StatefulReliabilityQuery {
        run_id: run_id.or(query.run_id.as_deref()),
        status: query.status.as_deref(),
        source_type: query.source_type.as_deref(),
        after_id: (cursor_collection == Some(collection))
            .then(|| query.after_id.as_deref())
            .flatten(),
        before_created_at_ms: query.before_created_at_ms,
        active_recovery_only: query.active_recovery_only,
        limit: Some(limit),
    }
}

fn reliability_cursor_collection(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    query: &StatefulReliabilityListQuery,
    run_id: Option<&str>,
) -> Option<ReliabilityCollection> {
    let after_id = query.after_id.as_deref()?.trim();
    if after_id.is_empty() {
        return None;
    }
    ReliabilityCollection::from_query(query.after_collection.as_deref()).or_else(|| {
        infer_reliability_cursor_collection(path, tenant_context, query, run_id, after_id)
    })
}

fn reliability_cursor_is_stale(
    query: &StatefulReliabilityListQuery,
    cursor_collection: Option<ReliabilityCollection>,
) -> bool {
    query
        .after_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|after_id| !after_id.is_empty())
        && cursor_collection.is_none()
}

fn infer_reliability_cursor_collection(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    query: &StatefulReliabilityListQuery,
    run_id: Option<&str>,
    after_id: &str,
) -> Option<ReliabilityCollection> {
    let store = load_stateful_reliability(path);
    let run_id = run_id.or(query.run_id.as_deref());
    let mut matches = Vec::new();
    if store.outbox.iter().any(|row| {
        row.visible_to_tenant(tenant_context)
            && reliability_option_filter_matches(run_id, row.run_id.as_deref())
            && reliability_status_matches(query.status.as_deref(), &row.status)
            && row.outbox_id == after_id
    }) {
        matches.push(ReliabilityCollection::Outbox);
    }
    if store.tool_effects.iter().any(|row| {
        row.visible_to_tenant(tenant_context)
            && reliability_option_filter_matches(run_id, row.run_id.as_deref())
            && reliability_status_matches(query.status.as_deref(), &row.status)
            && reliability_option_filter_matches(
                query.source_type.as_deref(),
                row.source_kind.as_deref(),
            )
            && row.effect_id == after_id
    }) {
        matches.push(ReliabilityCollection::ToolEffects);
    }
    if store.dead_letters.iter().any(|row| {
        row.visible_to_tenant(tenant_context)
            && reliability_option_filter_matches(run_id, row.run_id.as_deref())
            && reliability_status_matches(query.status.as_deref(), &row.status)
            && reliability_option_filter_matches(
                query.source_type.as_deref(),
                Some(row.source_type.as_str()),
            )
            && row.dead_letter_id == after_id
    }) {
        matches.push(ReliabilityCollection::DeadLetters);
    }
    if store.compensations.iter().any(|row| {
        row.visible_to_tenant(tenant_context)
            && reliability_option_filter_matches(run_id, row.run_id.as_deref())
            && reliability_status_matches(query.status.as_deref(), &row.status)
            && row.compensation_id == after_id
    }) {
        matches.push(ReliabilityCollection::Compensations);
    }
    (matches.len() == 1)
        .then(|| matches.first().copied())
        .flatten()
}

fn reliability_option_filter_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    let Some(expected) = reliability_filter(expected) else {
        return true;
    };
    actual
        .map(normalize_choice)
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

fn reliability_status_matches<T: Serialize>(expected: Option<&str>, actual: &T) -> bool {
    let Some(expected) = reliability_filter(expected) else {
        return true;
    };
    serde_json::to_value(actual)
        .ok()
        .and_then(|value| value.as_str().map(normalize_choice))
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

fn reliability_filter(value: Option<&str>) -> Option<String> {
    let value = normalize_choice(value.unwrap_or_default());
    (!value.is_empty() && value != "all").then_some(value)
}

fn reliability_pagination(
    after_id: Option<&str>,
    cursor_collection: Option<ReliabilityCollection>,
    before_created_at_ms: Option<u64>,
    limit: usize,
    outbox: &[crate::stateful_runtime::StatefulOutboxRecord],
    tool_effects: &[crate::stateful_runtime::StatefulToolEffectRecord],
    dead_letters: &[crate::stateful_runtime::StatefulDeadLetterRecord],
    compensations: &[crate::stateful_runtime::StatefulCompensationRecord],
) -> Value {
    json!({
        "after_id": after_id,
        "after_collection": cursor_collection.map(ReliabilityCollection::as_str),
        "before_created_at_ms": before_created_at_ms,
        "next": {
            "outbox": reliability_cursor(outbox, limit, before_created_at_ms, ReliabilityCollection::Outbox, |row| &row.outbox_id),
            "tool_effects": reliability_cursor(tool_effects, limit, before_created_at_ms, ReliabilityCollection::ToolEffects, |row| &row.effect_id),
            "dead_letters": reliability_cursor(dead_letters, limit, before_created_at_ms, ReliabilityCollection::DeadLetters, |row| &row.dead_letter_id),
            "compensations": reliability_cursor(compensations, limit, before_created_at_ms, ReliabilityCollection::Compensations, |row| &row.compensation_id),
        },
    })
}

fn reliability_cursor<T, IdFn>(
    rows: &[T],
    limit: usize,
    before_created_at_ms: Option<u64>,
    collection: ReliabilityCollection,
    id: IdFn,
) -> Option<Value>
where
    IdFn: Fn(&T) -> &String,
{
    if rows.len() < limit {
        return None;
    }
    rows.last().map(|row| {
        let mut cursor = json!({
            "after_id": id(row),
            "after_collection": collection.as_str(),
        });
        if let Some(before_created_at_ms) = before_created_at_ms {
            cursor["before_created_at_ms"] = json!(before_created_at_ms);
        }
        cursor
    })
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
        // Only the explicit retry choices flip a dead letter to `RetryRequested`
        // (which the dispatcher consumes). `resume_from_checkpoint` is a
        // record-only run resume, not a dead-letter retry, so it must NOT be
        // re-driven by the dispatcher — it falls through to the reviewed default.
        "retry_failed_effect" | "retry_dead_letter" => {
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

fn recovery_choice_execution_mode(choice: &str) -> &'static str {
    match choice {
        "compensate_pending_effects" | "compensate" => "operator_runbook_record_only",
        "abandon_with_audit" | "ignore_dead_letter" => "audit_disposition_only",
        "retry_failed_effect" | "retry_dead_letter" => "automatic_retry_dispatch",
        "resume_from_checkpoint" | "reconcile_external_effect" => "operator_request_record_only",
        _ => "operator_review_record_only",
    }
}

/// Whether recording this choice also kicks off automatic re-execution (TAN-564).
/// Retry choices re-drive the owning run through its governed dispatch path via
/// the dead-letter retry dispatcher; all other choices remain record-only.
fn recovery_choice_automatic_dispatch(choice: &str) -> bool {
    matches!(choice, "retry_failed_effect" | "retry_dead_letter")
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
