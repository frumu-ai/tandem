//! Canonical, bounded read/control contract for long-running goals.

use super::*;

use std::collections::{HashMap, HashSet};

use tandem_automation::{
    AutomationRunStatus, AutomationV2RunRecord, GoalRunLink, LongRunningGoal,
    LongRunningGoalStatus, OrchestrationSpec, WorkflowHandoff, WorkflowHandoffStatus,
};
use tandem_types::{RequestPrincipal, VerifiedTenantContext};

use crate::stateful_runtime::{
    stable_definition_snapshot_hash, GoalPauseOutcome, GoalResumeOutcome, OrchestrationStateStore,
    OrchestrationTransitionAuthority, StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus,
};

const DEFAULT_TIMELINE_LIMIT: usize = 100;
const MAX_TIMELINE_LIMIT: usize = 250;
const MAX_PROJECTION_RECORDS: usize = 250;

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub(super) struct GoalProjectionQuery {
    /// When present, project state as of the latest durable event at or before
    /// this store-wide cursor. Omission selects the live projection.
    pub cursor: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GoalActionPayload {
    pub expected_updated_at_ms: u64,
    pub idempotency_key: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
struct DurableGoalProjectionSnapshot {
    goal: LongRunningGoal,
    #[serde(default)]
    links: Vec<GoalRunLink>,
    #[serde(default)]
    runs: Vec<AutomationV2RunRecord>,
    #[serde(default)]
    waits: Vec<StatefulWaitRecord>,
    #[serde(default)]
    handoffs: Vec<WorkflowHandoff>,
}

pub(super) async fn get_goal_projection(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    Path(goal_id): Path<String>,
    Query(query): Query<GoalProjectionQuery>,
) -> Response {
    match build_projection(&state, &tenant, &principal, &goal_id, query).await {
        Ok(projection) => Json(projection).into_response(),
        Err(response) => response,
    }
}

pub(super) async fn dispatch_goal_action(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified_tenant: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((goal_id, action_id)): Path<(String, String)>,
    Json(payload): Json<GoalActionPayload>,
) -> Response {
    if !is_authenticated(&principal) {
        return projection_error(
            StatusCode::UNAUTHORIZED,
            "goal_action_unauthenticated",
            None,
        );
    }
    if payload.idempotency_key.trim().is_empty() || payload.idempotency_key.len() > 256 {
        return projection_error(
            StatusCode::BAD_REQUEST,
            "invalid_goal_action",
            Some("idempotency_key must be non-empty and at most 256 bytes"),
        );
    }

    let store = match super::goals_api::goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let goal = match super::goals_api::load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let operation = format!("goal_action:{goal_id}:{action_id}");
    let request_digest = stable_definition_snapshot_hash(&json!({
        "goal_id": goal_id,
        "action_id": action_id,
        "expected_updated_at_ms": payload.expected_updated_at_ms,
        "reason": payload.reason,
        "decision": payload.decision,
        "payload": payload.payload,
    }));
    match store.completed_orchestration_tool_request(
        &tenant,
        &operation,
        &payload.idempotency_key,
        &request_digest,
    ) {
        Ok(Some(outcome)) => {
            return action_response(&state, &tenant, &principal, &goal_id, &action_id, outcome)
                .await
        }
        Ok(None) => {}
        Err(error) => {
            return projection_error(
                StatusCode::CONFLICT,
                "goal_action_idempotency_conflict",
                Some(&error.to_string()),
            )
        }
    }
    if goal.updated_at_ms != payload.expected_updated_at_ms {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "stale_goal_action",
                "expected_updated_at_ms": payload.expected_updated_at_ms,
                "current_updated_at_ms": goal.updated_at_ms,
                "goal": goal,
            })),
        )
            .into_response();
    }

    let waits = bounded_waits(&state, &tenant, &store, &goal_id);
    let handoffs = store.list_goal_handoffs(&goal_id).unwrap_or_default();
    let run = goal
        .active_run_id
        .as_deref()
        .and_then(|run_id| store.get_automation_run(run_id).ok().flatten());
    let recovery = match run.as_ref() {
        Some(run) => {
            super::stateful_runtime_reliability::stateful_run_resume_plan_value(
                &state,
                &tenant,
                &run.run_id,
                MAX_PROJECTION_RECORDS,
            )
            .await
        }
        None => None,
    };
    let descriptors = action_descriptors(
        &goal,
        &handoffs,
        &waits,
        run.as_ref(),
        recovery.as_ref(),
        true,
        false,
    );
    let descriptor = descriptors.iter().find(|row| row["id"] == action_id);
    if descriptor.is_none_or(|row| !row["enabled"].as_bool().unwrap_or(false)) {
        // Matching handoff decisions are authoritative idempotent reads even
        // though the now-settled descriptor is no longer enabled.
        if let Some(response) = duplicate_handoff_decision_response(
            &store,
            &goal,
            &action_id,
            payload.decision.as_deref(),
        ) {
            return action_response(&state, &tenant, &principal, &goal_id, &action_id, response)
                .await;
        }
        return projection_error(
            StatusCode::CONFLICT,
            "goal_action_not_available",
            descriptor
                .and_then(|row| row["disabled_reason"].as_str())
                .or(Some("action is not valid for the current goal state")),
        );
    }

    match store.begin_orchestration_action_request(
        &tenant,
        &operation,
        &payload.idempotency_key,
        &request_digest,
        crate::now_ms(),
    ) {
        Ok(Some(outcome)) => {
            return action_response(&state, &tenant, &principal, &goal_id, &action_id, outcome)
                .await
        }
        Ok(None) => {}
        Err(error) => {
            return projection_error(
                StatusCode::CONFLICT,
                "goal_action_idempotency_conflict",
                Some(&error.to_string()),
            )
        }
    }

    let outcome = if action_id == "pause" {
        let reason = match required_reason(payload.reason.as_deref()) {
            Ok(reason) => reason,
            Err(response) => return response,
        };
        match state
            .pause_long_running_goal(
                &goal_id,
                &goal.tenant_context,
                reason,
                &super::goals_api::request_actor(&principal),
            )
            .await
        {
            Ok((outcome, goal)) => json!({
                "outcome": match outcome {
                    GoalPauseOutcome::Applied => "paused",
                    GoalPauseOutcome::AlreadyPaused => "already_paused",
                },
                "goal": goal,
            }),
            Err(error) => return super::goals_api::goal_error_response(&error),
        }
    } else if action_id == "resume" {
        match state
            .resume_long_running_goal(
                &goal_id,
                &goal.tenant_context,
                payload.reason.as_deref().unwrap_or("operator resume"),
                &super::goals_api::request_actor(&principal),
            )
            .await
        {
            Ok((outcome, goal)) => json!({
                "outcome": match outcome {
                    GoalResumeOutcome::Applied => "resumed",
                    GoalResumeOutcome::NotPaused => "not_paused",
                },
                "goal": goal,
            }),
            Err(error) => return super::goals_api::goal_error_response(&error),
        }
    } else if action_id == "cancel" {
        let reason = match required_reason(payload.reason.as_deref()) {
            Ok(reason) => reason,
            Err(response) => return response,
        };
        match state
            .cancel_long_running_goal(
                &goal_id,
                &goal.tenant_context,
                reason,
                &super::goals_api::request_actor(&principal),
            )
            .await
        {
            Ok(result) => json!({"outcome": format!("{:?}", result.outcome), "goal": result.goal}),
            Err(error) => return super::goals_api::goal_error_response(&error),
        }
    } else if let Some(handoff_id) = action_id
        .strip_prefix("handoff:")
        .and_then(|value| value.strip_suffix(":decision"))
    {
        let approve = match payload.decision.as_deref() {
            Some("approve") => true,
            Some("reject") => false,
            _ => {
                return projection_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_goal_action",
                    Some("decision must be approve or reject"),
                )
            }
        };
        if !approve && required_reason(payload.reason.as_deref()).is_err() {
            return projection_error(
                StatusCode::BAD_REQUEST,
                "goal_action_reason_required",
                Some("rejecting a handoff requires a reason"),
            );
        }
        let authority = OrchestrationTransitionAuthority {
            actor: super::goals_api::request_actor(&principal),
            can_emit: true,
            can_approve: true,
        };
        match store.decide_pending_handoff(
            handoff_id,
            &goal.tenant_context,
            approve,
            &authority,
            crate::now_ms(),
        ) {
            Ok(handoff) => json!({"outcome": "decided", "handoff": handoff, "goal": goal}),
            Err(error) => return super::goals_api::goal_error_response(&error),
        }
    } else if let Some(run_id) = action_id
        .strip_prefix("approval:")
        .and_then(|value| value.strip_suffix(":decision"))
    {
        let decision = payload.decision.clone().unwrap_or_default();
        let approval_wait = waits.iter().find(|wait| {
            wait.run_id == run_id
                && wait.wait_kind == StatefulWaitKind::Approval
                && wait.status == StatefulWaitStatus::Waiting
        });
        let approval_request_id = approval_wait.and_then(approval_request_id);
        let transition_id = approval_wait
            .and_then(|wait| wait.metadata.as_ref())
            .and_then(|metadata| metadata.pointer("/approval_wait/transition_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let input = super::routines_automations::AutomationV2GateDecisionInput {
            decision,
            reason: payload.reason.clone(),
            approval_request_id,
            transition_id,
        };
        match super::routines_automations::automations_v2_run_gate_decide(
            State(state.clone()),
            Extension(tenant.clone()),
            Extension(principal.clone()),
            verified_tenant.clone(),
            headers.clone(),
            Path(run_id.to_string()),
            Json(input),
        )
        .await
        {
            Ok(Json(value)) => value,
            Err((status, Json(value))) => return (status, Json(value)).into_response(),
        }
    } else if let Some(target) = action_id.strip_prefix("retry:") {
        let Some((run_id, node_id)) = target.split_once(':') else {
            return projection_error(
                StatusCode::BAD_REQUEST,
                "invalid_goal_action",
                Some("retry action is missing its run or node id"),
            );
        };
        let input = super::routines_automations::AutomationV2RunTaskActionInput {
            reason: payload.reason.clone(),
        };
        match super::routines_automations::automations_v2_run_task_retry(
            State(state.clone()),
            Extension(tenant.clone()),
            Path((run_id.to_string(), node_id.to_string())),
            Json(input),
        )
        .await
        {
            Ok(Json(value)) => value,
            Err((status, Json(value))) => return (status, Json(value)).into_response(),
        }
    } else if let Some(run_id) = action_id
        .strip_prefix("resume-plan:")
        .and_then(|value| value.strip_suffix(":apply"))
    {
        let Some(plan) = recovery.as_ref() else {
            return projection_error(
                StatusCode::CONFLICT,
                "goal_recovery_plan_unavailable",
                Some("the active run no longer has an applicable recovery plan"),
            );
        };
        if plan["run_id"].as_str() != Some(run_id) {
            return projection_error(
                StatusCode::CONFLICT,
                "goal_recovery_plan_stale",
                Some("the recovery action does not belong to the active goal run"),
            );
        }
        let choice = payload
            .decision
            .clone()
            .or_else(|| {
                payload
                    .payload
                    .get("choice")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if let Err(detail) = validate_recovery_targets(plan, &payload.payload) {
            return projection_error(
                StatusCode::BAD_REQUEST,
                "goal_recovery_target_invalid",
                Some(detail),
            );
        }
        let input = super::stateful_runtime_reliability::StatefulResumePlanActionInput {
            choice,
            reason: payload.reason.clone(),
            actor_id: principal.actor_id.clone(),
            dead_letter_id: payload_field(&payload.payload, "dead_letter_id"),
            compensation_id: payload_field(&payload.payload, "compensation_id"),
            target_effect_id: payload_field(&payload.payload, "target_effect_id"),
        };
        let response = super::stateful_runtime_reliability::apply_stateful_run_resume_plan_action(
            State(state.clone()),
            Extension(tenant.clone()),
            Path(run_id.to_string()),
            Json(input),
        )
        .await;
        match response_json(response).await {
            Ok(value) => value,
            Err(response) => return response,
        }
    } else if let Some(wait_id) = action_id
        .strip_prefix("wait:")
        .and_then(|value| value.strip_suffix(":resolve"))
    {
        match state
            .resolve_automation_v2_external_wait(
                &goal.tenant_context,
                wait_id,
                &payload.idempotency_key,
                payload.payload,
            )
            .await
        {
            Ok(Some(wait)) => json!({"outcome": "resolved", "wait": wait, "goal": goal}),
            Ok(None) => {
                return projection_error(
                    StatusCode::CONFLICT,
                    "wait_resolution_conflict",
                    Some("wait is no longer eligible for resolution"),
                )
            }
            Err(error) => return super::goals_api::goal_error_response(&error),
        }
    } else {
        return projection_error(
            StatusCode::NOT_FOUND,
            "goal_action_not_found",
            Some("unknown goal action descriptor id"),
        );
    };

    if let Err(error) = store.complete_orchestration_tool_request(
        &tenant,
        &operation,
        &payload.idempotency_key,
        &request_digest,
        &outcome,
        crate::now_ms(),
    ) {
        return projection_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "goal_action_receipt_failed",
            Some(&error.to_string()),
        );
    }

    action_response(&state, &tenant, &principal, &goal_id, &action_id, outcome).await
}

async fn action_response(
    state: &AppState,
    tenant: &TenantContext,
    principal: &RequestPrincipal,
    goal_id: &str,
    action_id: &str,
    outcome: Value,
) -> Response {
    match build_projection(
        state,
        tenant,
        principal,
        goal_id,
        GoalProjectionQuery::default(),
    )
    .await
    {
        Ok(projection) => Json(json!({
            "goal": projection["goal"],
            "action": {"id": action_id, "result": outcome},
            "projection_cursor": projection["cursor"],
            "projection": projection,
        }))
        .into_response(),
        Err(response) => response,
    }
}

async fn build_projection(
    state: &AppState,
    tenant: &TenantContext,
    principal: &RequestPrincipal,
    goal_id: &str,
    query: GoalProjectionQuery,
) -> Result<Value, Response> {
    let store = super::goals_api::goal_store(state)?;
    let live_goal = super::goals_api::load_tenant_goal(&store, tenant, goal_id)?;
    let (retained_from_cursor, live_cursor) = store
        .goal_event_cursor_bounds(goal_id)
        .map_err(|error| super::goals_api::goal_error_response(&error))?
        .unwrap_or((0, 0));
    if query
        .cursor
        .is_some_and(|cursor| live_cursor > 0 && cursor < retained_from_cursor)
    {
        return Err((
            StatusCode::GONE,
            Json(json!({
                "error": "projection_cursor_not_retained",
                "retained_from_cursor": retained_from_cursor,
            })),
        )
            .into_response());
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_TIMELINE_LIMIT)
        .clamp(1, MAX_TIMELINE_LIMIT);
    let timeline = store
        .query_goal_event_window(goal_id, query.cursor, limit)
        .map_err(|error| super::goals_api::goal_error_response(&error))?;
    let snapshot = match timeline.iter().rev().find(|row| {
        row.event.payload.get("projection_snapshot").is_some()
            || row.event.payload.get("projection_snapshot_ref").is_some()
    }) {
        Some(row) => {
            let value = if let Some(value) = row.event.payload.get("projection_snapshot") {
                value.clone()
            } else {
                store
                    .resolve_goal_projection_snapshot(&row.event.payload["projection_snapshot_ref"])
                    .map_err(|error| {
                        projection_error(
                            StatusCode::CONFLICT,
                            "historical_projection_snapshot_invalid",
                            Some(&error.to_string()),
                        )
                    })?
            };
            Some(
                serde_json::from_value::<DurableGoalProjectionSnapshot>(value).map_err(
                    |error| {
                        projection_error(
                            StatusCode::CONFLICT,
                            "historical_projection_snapshot_invalid",
                            Some(&error.to_string()),
                        )
                    },
                )?,
            )
        }
        None => None,
    };
    let replay = query.cursor.is_some();
    if replay && snapshot.is_none() {
        return Err(projection_error(
            StatusCode::CONFLICT,
            "historical_projection_snapshot_unavailable",
            Some("the retained event predates deterministic projection snapshots"),
        ));
    }
    let (goal, links, handoffs, waits, runs, state_source, state_exact) = if replay {
        let snapshot = snapshot.expect("replay snapshot checked above");
        (
            snapshot.goal,
            snapshot.links,
            snapshot.handoffs,
            snapshot.waits,
            snapshot.runs,
            "projection_snapshot",
            true,
        )
    } else {
        let mut links = store.list_goal_run_links(goal_id).unwrap_or_default();
        if links.len() > MAX_PROJECTION_RECORDS {
            links.drain(..links.len() - MAX_PROJECTION_RECORDS);
        }
        let runs = links
            .iter()
            .filter_map(|link| store.get_automation_run(&link.run_id).ok().flatten())
            .collect::<Vec<_>>();
        let mut handoffs = store.list_goal_handoffs(goal_id).unwrap_or_default();
        if handoffs.len() > MAX_PROJECTION_RECORDS {
            handoffs.drain(..handoffs.len() - MAX_PROJECTION_RECORDS);
        }
        let waits = bounded_waits(state, &live_goal.tenant_context, &store, goal_id);
        (
            live_goal.clone(),
            links,
            handoffs,
            waits,
            runs,
            "current_goal",
            true,
        )
    };
    let cursor = timeline
        .last()
        .map(|row| row.cursor)
        .unwrap_or_else(|| query.cursor.unwrap_or(live_cursor).min(live_cursor));
    let (orchestration, orchestration_source) = orchestration_snapshot(&store, tenant, &goal)
        .map_err(|error| super::goals_api::goal_error_response(&error))?;
    let runs = runs
        .into_iter()
        .map(|run| (run.run_id.clone(), run))
        .collect::<HashMap<_, _>>();

    let projected_run = goal
        .active_run_id
        .as_deref()
        .and_then(|run_id| runs.get(run_id).cloned());
    let recovery = if replay {
        None
    } else {
        match projected_run.as_ref() {
            Some(run) => {
                super::stateful_runtime_reliability::stateful_run_resume_plan_value(
                    state,
                    tenant,
                    &run.run_id,
                    MAX_PROJECTION_RECORDS,
                )
                .await
            }
            None => None,
        }
    };
    let graph = semantic_graph(
        &goal,
        orchestration.as_ref(),
        &links,
        &handoffs,
        &waits,
        &runs,
    );
    let workflow = current_workflow(projected_run.as_ref());
    let artifacts = handoffs
        .iter()
        .map(|handoff| {
            json!({
                "artifact": handoff.artifact,
                "handoff_id": handoff.handoff_id,
                "source_run_id": handoff.source_run_id,
                "consumed_by_run_id": handoff.consumed_by_run_id,
            })
        })
        .collect::<Vec<_>>();
    let actions = action_descriptors(
        &live_goal,
        &handoffs,
        &waits,
        projected_run.as_ref(),
        recovery.as_ref(),
        is_authenticated(principal),
        replay,
    );

    Ok(json!({
        "schema_version": 1,
        "goal_id": goal_id,
        "mode": if replay { "replay" } else { "live" },
        "cursor": cursor,
        "live_cursor": live_cursor,
        "retained_from_cursor": retained_from_cursor,
        "goal": goal,
        "historical_state": {"source": state_source, "exact": state_exact},
        "orchestration": orchestration,
        "orchestration_source": orchestration_source,
        "graph": graph,
        "workflow": workflow,
        "waits": waits,
        "handoffs": handoffs,
        "artifacts": artifacts,
        "recovery": recovery,
        "budgets": super::goals_api::goal_budgets(&goal),
        "timeline": {
            "events": timeline.iter().map(|row| json!({"cursor": row.cursor, "event": super::goals_api::goal_event_wire(row.event.clone())})).collect::<Vec<_>>(),
            "count": timeline.len(),
            "limit": limit,
            "truncated": timeline.len() == limit && timeline.first().is_some_and(|row| row.cursor > retained_from_cursor),
        },
        "actions": actions,
    }))
}

fn orchestration_snapshot(
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    goal: &LongRunningGoal,
) -> anyhow::Result<(Option<OrchestrationSpec>, &'static str)> {
    if let Some(snapshot) = goal
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("orchestration_snapshot"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
    {
        return Ok((Some(snapshot), "goal_metadata_snapshot"));
    }
    Ok((
        store.get_orchestration_for_tenant(
            tenant,
            &goal.orchestration_id,
            goal.orchestration_version,
        )?,
        "published_definition_fallback",
    ))
}

fn semantic_graph(
    goal: &LongRunningGoal,
    orchestration: Option<&OrchestrationSpec>,
    links: &[GoalRunLink],
    handoffs: &[WorkflowHandoff],
    waits: &[StatefulWaitRecord],
    runs_by_id: &HashMap<String, AutomationV2RunRecord>,
) -> Value {
    let Some(orchestration) = orchestration else {
        return json!({"nodes": [], "edges": [], "available": false});
    };
    let visited = links
        .iter()
        .map(|link| link.orchestration_node_id.as_str())
        .collect::<HashSet<_>>();
    let nodes = orchestration
        .nodes
        .iter()
        .map(|node| {
            let node_links = links
                .iter()
                .filter(|link| link.orchestration_node_id == node.node_id);
            let runs = node_links
                .clone()
                .map(|link| {
                    let run = runs_by_id.get(&link.run_id);
                    json!({"run_id": link.run_id, "hop_index": link.hop_index,
                    "status": run.map(|run| &run.status)})
                })
                .collect::<Vec<_>>();
            let latest_run = links
                .iter()
                .rev()
                .find(|link| link.orchestration_node_id == node.node_id)
                .and_then(|link| runs_by_id.get(&link.run_id));
            let state = semantic_node_state(
                goal,
                &node.node_id,
                latest_run,
                waits,
                visited.contains(node.node_id.as_str()),
            );
            json!({
                "node_id": node.node_id,
                "name": node.name,
                "kind": node.node,
                "state": state,
                "runs": runs,
            })
        })
        .collect::<Vec<_>>();
    let edges = orchestration
        .edges
        .iter()
        .map(|edge| {
            let handoff = handoffs.iter().rev().find(|handoff| handoff.edge_id == edge.edge_id);
            let state = match handoff.map(|handoff| &handoff.status) {
                Some(WorkflowHandoffStatus::Consumed) => "taken",
                Some(WorkflowHandoffStatus::Rejected | WorkflowHandoffStatus::DeadLettered) => "not_started",
                Some(_) => "claiming",
                None if goal.current_node_id.as_deref() == Some(edge.from_node_id.as_str()) => "eligible",
                None => "not_started",
            };
            json!({"edge": edge, "state": state, "handoff_id": handoff.map(|handoff| &handoff.handoff_id)})
        })
        .collect::<Vec<_>>();
    json!({"nodes": nodes, "edges": edges, "available": true})
}

fn semantic_node_state(
    goal: &LongRunningGoal,
    node_id: &str,
    run: Option<&AutomationV2RunRecord>,
    waits: &[StatefulWaitRecord],
    visited: bool,
) -> &'static str {
    let current = goal.current_node_id.as_deref() == Some(node_id);
    if current && goal.status == LongRunningGoalStatus::Paused {
        return "paused";
    }
    if let Some(run) = run {
        if current {
            if let Some(wait) = waits.iter().find(|wait| {
                wait.run_id == run.run_id
                    && matches!(
                        wait.status,
                        StatefulWaitStatus::Waiting | StatefulWaitStatus::Claimed
                    )
            }) {
                return match wait.wait_kind {
                    StatefulWaitKind::Timer | StatefulWaitKind::RetryBackoff => "timer_wait",
                    StatefulWaitKind::Approval => "approval",
                    StatefulWaitKind::Webhook | StatefulWaitKind::ExternalCondition => {
                        "external_wait"
                    }
                };
            }
        }
        return match run.status {
            AutomationRunStatus::Queued => "claiming",
            AutomationRunStatus::Running | AutomationRunStatus::Pausing => "running",
            AutomationRunStatus::Paused => "paused",
            AutomationRunStatus::AwaitingApproval => "approval",
            AutomationRunStatus::Completed => "completed",
            AutomationRunStatus::Blocked
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Cancelled => "failed",
        };
    }
    if current {
        return match goal.status {
            LongRunningGoalStatus::Completed => "completed",
            LongRunningGoalStatus::Failed
            | LongRunningGoalStatus::Cancelled
            | LongRunningGoalStatus::Expired => "failed",
            LongRunningGoalStatus::Paused => "paused",
            LongRunningGoalStatus::Waiting => "external_wait",
            LongRunningGoalStatus::Queued => "claiming",
            LongRunningGoalStatus::Active => "running",
        };
    }
    if visited {
        "completed"
    } else {
        "not_started"
    }
}

fn current_workflow(run: Option<&AutomationV2RunRecord>) -> Value {
    let Some(run) = run else {
        return Value::Null;
    };
    json!({
        "run_id": run.run_id,
        "automation_id": run.automation_id,
        "status": run.status,
        "stage": run.checkpoint.pending_nodes.first().or(run.checkpoint.completed_nodes.last()),
        "checkpoint": {
            "completed_nodes": run.checkpoint.completed_nodes,
            "pending_nodes": run.checkpoint.pending_nodes,
            "blocked_nodes": run.checkpoint.blocked_nodes,
        },
        "outputs": run.checkpoint.node_outputs,
        "retries": {
            "attempts": run.checkpoint.node_attempts,
            "verdicts": run.checkpoint.node_attempt_verdicts,
            "last_failure": run.checkpoint.last_failure,
        },
    })
}

fn bounded_waits(
    state: &AppState,
    tenant: &TenantContext,
    store: &OrchestrationStateStore,
    goal_id: &str,
) -> Vec<StatefulWaitRecord> {
    let mut waits = super::goals_api::goal_waits(state, tenant, store, goal_id);
    waits.sort_by_key(|wait| (wait.created_at_ms, wait.wait_id.clone()));
    if waits.len() > MAX_PROJECTION_RECORDS {
        waits.drain(..waits.len() - MAX_PROJECTION_RECORDS);
    }
    waits
}

fn action_descriptors(
    goal: &LongRunningGoal,
    handoffs: &[WorkflowHandoff],
    waits: &[StatefulWaitRecord],
    run: Option<&AutomationV2RunRecord>,
    recovery: Option<&Value>,
    authenticated: bool,
    replay: bool,
) -> Vec<Value> {
    let terminal = goal.status.is_terminal();
    let pause_reason = action_disabled_reason(
        authenticated,
        replay,
        if terminal {
            Some("goal_terminal")
        } else if goal.status == LongRunningGoalStatus::Paused {
            Some("goal_already_paused")
        } else {
            None
        },
    );
    let resume_reason = action_disabled_reason(
        authenticated,
        replay,
        if terminal {
            Some("goal_terminal")
        } else if goal.status != LongRunningGoalStatus::Paused {
            Some("goal_not_paused")
        } else {
            None
        },
    );
    let cancel_reason =
        action_disabled_reason(authenticated, replay, terminal.then_some("goal_terminal"));
    let mut actions = vec![
        descriptor(
            "pause",
            "pause",
            "Pause goal",
            pause_reason,
            true,
            true,
            None,
            None,
            "Stops new goal transitions until resumed",
        ),
        descriptor(
            "resume",
            "resume",
            "Resume goal",
            resume_reason,
            false,
            false,
            None,
            None,
            "Allows goal execution to continue",
        ),
        descriptor(
            "cancel",
            "cancel",
            "Cancel goal",
            cancel_reason,
            true,
            true,
            None,
            None,
            "Permanently cancels the goal and active work",
        ),
    ];
    for handoff in handoffs
        .iter()
        .filter(|handoff| handoff.status == WorkflowHandoffStatus::PendingApproval)
    {
        let disabled = action_disabled_reason(authenticated, replay, None);
        actions.push(descriptor(
            &format!("handoff:{}:decision", handoff.handoff_id),
            "handoff",
            "Decide handoff",
            disabled,
            true,
            true,
            Some(json!(["approve", "reject"])),
            Some(&handoff.handoff_id),
            "Approves or rejects the pending workflow transition",
        ));
    }
    for wait in waits.iter().filter(|wait| {
        wait.wait_kind == StatefulWaitKind::ExternalCondition
            && wait.status == StatefulWaitStatus::Waiting
    }) {
        let disabled = action_disabled_reason(authenticated, replay, None);
        let mut action = descriptor(
            &format!("wait:{}:resolve", wait.wait_id),
            "wait",
            "Resolve wait",
            disabled,
            false,
            false,
            None,
            Some(&wait.wait_id),
            "Supplies the external condition payload and resumes eligible work",
        );
        action["payload_fields"] = json!([{
            "name": "value",
            "label": "Condition payload (JSON)",
            "required": false,
            "format": "json"
        }]);
        actions.push(action);
    }
    for wait in waits.iter().filter(|wait| {
        wait.wait_kind == StatefulWaitKind::Approval && wait.status == StatefulWaitStatus::Waiting
    }) {
        let disabled = action_disabled_reason(authenticated, replay, None);
        actions.push(descriptor(
            &format!("approval:{}:decision", wait.run_id),
            "approval",
            "Decide approval",
            disabled,
            true,
            true,
            Some(json!(["approve", "reject"])),
            Some(&wait.wait_id),
            "Approves or rejects the current governed workflow gate",
        ));
    }
    if let Some(run) = run {
        if matches!(
            run.status,
            AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Paused
                | AutomationRunStatus::Cancelled
        ) {
            if let Some(node_id) = run
                .checkpoint
                .last_failure
                .as_ref()
                .map(|failure| failure.node_id.as_str())
            {
                let disabled = action_disabled_reason(authenticated, replay, None);
                actions.push(descriptor(
                    &format!("retry:{}:{}", run.run_id, node_id),
                    "retry",
                    "Retry failed stage",
                    disabled,
                    true,
                    true,
                    None,
                    Some(node_id),
                    "Resets the failed stage and affected subtree before retrying",
                ));
            }
        }
        if let Some(plan) = recovery {
            let choices = plan["operator_choices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|choice| choice["enabled"].as_bool().unwrap_or(false))
                .filter_map(|choice| choice["choice"].as_str())
                .collect::<Vec<_>>();
            if !choices.is_empty() {
                let disabled = action_disabled_reason(authenticated, replay, None);
                let mut action = descriptor(
                    &format!("resume-plan:{}:apply", run.run_id),
                    "recovery",
                    "Apply recovery plan",
                    disabled,
                    true,
                    true,
                    Some(json!(choices)),
                    Some(&run.run_id),
                    "Applies a governed recovery choice to uncertain or interrupted work",
                );
                action["payload_fields"] = recovery_payload_fields(plan);
                actions.push(action);
            }
        }
    }
    actions
}

fn action_disabled_reason(
    authenticated: bool,
    replay: bool,
    state_reason: Option<&'static str>,
) -> Option<&'static str> {
    if !authenticated {
        Some("authentication_required")
    } else if replay {
        Some("historical_projection_read_only")
    } else {
        state_reason
    }
}

#[allow(clippy::too_many_arguments)]
fn descriptor(
    id: &str,
    kind: &str,
    label: &str,
    disabled_reason: Option<&str>,
    destructive: bool,
    reason_required: bool,
    decision_options: Option<Value>,
    target_id: Option<&str>,
    impact: &str,
) -> Value {
    json!({
        "id": id,
        "kind": kind,
        "label": label,
        "enabled": disabled_reason.is_none(),
        "destructive": destructive,
        "reason_required": reason_required,
        "decision_options": decision_options,
        "target_id": target_id,
        "impact": impact,
        "disabled_reason": disabled_reason,
        "payload_fields": [],
    })
}

fn recovery_payload_fields(plan: &Value) -> Value {
    let options = |collection: &str, key: &str| {
        plan[collection]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row[key].as_str())
            .map(|value| json!({"value": value, "label": value}))
            .collect::<Vec<_>>()
    };
    json!([
        {"name": "dead_letter_id", "label": "Dead letter", "required": false, "options": options("dead_letters", "dead_letter_id")},
        {"name": "compensation_id", "label": "Compensation", "required": false, "options": options("pending_compensations", "compensation_id")},
        {"name": "target_effect_id", "label": "External effect", "required": false, "options": options("uncertain_effects", "effect_id")}
    ])
}

fn duplicate_handoff_decision_response(
    store: &OrchestrationStateStore,
    goal: &LongRunningGoal,
    action_id: &str,
    decision: Option<&str>,
) -> Option<Value> {
    let handoff_id = action_id
        .strip_prefix("handoff:")?
        .strip_suffix(":decision")?;
    let handoff = store.get_workflow_handoff(handoff_id).ok().flatten()?;
    if handoff.goal_id != goal.goal_id {
        return None;
    }
    let matches = matches!(
        (decision, &handoff.status),
        (Some("approve"), WorkflowHandoffStatus::Approved)
            | (Some("reject"), WorkflowHandoffStatus::Rejected)
    );
    matches.then(|| json!({"outcome": "already_decided", "handoff": handoff, "goal": goal}))
}

fn required_reason(reason: Option<&str>) -> Result<&str, Response> {
    reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .ok_or_else(|| {
            projection_error(
                StatusCode::BAD_REQUEST,
                "goal_action_reason_required",
                Some("this destructive action requires a reason"),
            )
        })
}

fn approval_request_id(wait: &StatefulWaitRecord) -> Option<String> {
    wait.metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .pointer("/approval_wait/approval_request_id")
                .or_else(|| metadata.get("approval_request_id"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn payload_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_recovery_targets(plan: &Value, payload: &Value) -> Result<(), &'static str> {
    let targets = [
        ("dead_letter_id", "dead_letters", "dead_letter_id"),
        (
            "compensation_id",
            "pending_compensations",
            "compensation_id",
        ),
        ("target_effect_id", "uncertain_effects", "effect_id"),
    ];
    for (payload_key, collection, record_key) in targets {
        let Some(requested) = payload.get(payload_key).and_then(Value::as_str) else {
            continue;
        };
        let belongs_to_plan = plan[collection].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row[record_key].as_str() == Some(requested))
        });
        if !belongs_to_plan {
            return Err("a selected recovery record is not part of the active run's resume plan");
        }
    }
    Ok(())
}

async fn response_json(response: Response) -> Result<Value, Response> {
    let status = response.status();
    let bytes = match axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(projection_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "goal_action_response_invalid",
                Some(&error.to_string()),
            ))
        }
    };
    let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        json!({"error": "goal_action_response_invalid", "detail": "handler returned a non-JSON response"})
    });
    if status.is_success() {
        Ok(value)
    } else {
        Err((status, Json(value)).into_response())
    }
}

fn is_authenticated(principal: &RequestPrincipal) -> bool {
    principal
        .actor_id
        .as_deref()
        .is_some_and(|actor| !actor.trim().is_empty())
}

fn projection_error(status: StatusCode, code: &str, detail: Option<&str>) -> Response {
    (status, Json(json!({"error": code, "detail": detail}))).into_response()
}
