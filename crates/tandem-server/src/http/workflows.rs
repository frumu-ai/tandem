// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::{execute_workflow, simulate_workflow_event};
use tandem_types::{
    AccessPermission, ApprovalSourceKind, ApprovalWaitRef, EngineEvent, TenantContext,
    VerifiedTenantContext,
};

use super::AppState;

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowRunsQuery {
    pub workflow_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowEventsQuery {
    pub workflow_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowRunPath {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowHookPath {
    pub id: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowValidateInput {
    #[serde(default)]
    pub reload: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowHookPatchInput {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowSimulateInput {
    pub event_type: String,
    #[serde(default)]
    pub properties: Value,
}

pub(super) async fn workflows_list(State(state): State<AppState>) -> Json<Value> {
    let workflows = state.list_workflows().await;
    let automation_previews = workflows
        .iter()
        .map(|workflow| {
            (
                workflow.workflow_id.clone(),
                serde_json::to_value(
                    crate::workflows::compile_workflow_spec_to_automation_preview(workflow),
                )
                .unwrap_or(Value::Null),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Json(json!({
        "workflows": workflows,
        "automation_previews": automation_previews,
        "count": automation_previews.len(),
    }))
}

pub(super) async fn workflows_get(
    State(state): State<AppState>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let workflow = state.get_workflow(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let hooks = state.list_workflow_hooks(Some(&id)).await;
    let automation_preview =
        crate::workflows::compile_workflow_spec_to_automation_preview(&workflow);
    Ok(Json(json!({
        "workflow": workflow,
        "hooks": hooks,
        "automation_preview": automation_preview
    })))
}

pub(super) async fn workflows_validate(
    State(state): State<AppState>,
    Json(input): Json<WorkflowValidateInput>,
) -> Result<Json<Value>, StatusCode> {
    let messages = if input.reload.unwrap_or(true) {
        state
            .reload_workflows()
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "messages": messages,
        "registry": state.workflow_registry().await,
    })))
}

pub(super) async fn workflow_hooks_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowRunsQuery>,
) -> Json<Value> {
    let hooks = state
        .list_workflow_hooks(query.workflow_id.as_deref())
        .await;
    Json(json!({ "hooks": hooks, "count": hooks.len() }))
}

pub(super) async fn workflow_hooks_patch(
    State(state): State<AppState>,
    Path(WorkflowHookPath { id }): Path<WorkflowHookPath>,
    Json(input): Json<WorkflowHookPatchInput>,
) -> Result<Json<Value>, StatusCode> {
    let hook = state
        .set_workflow_hook_enabled(&id, input.enabled)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({ "hook": hook })))
}

pub(super) async fn workflows_simulate(
    State(state): State<AppState>,
    Json(input): Json<WorkflowSimulateInput>,
) -> Json<Value> {
    let event = EngineEvent::new(input.event_type, input.properties);
    let result = simulate_workflow_event(&state, &event).await;
    Json(json!({ "simulation": result }))
}

pub(super) async fn workflows_run(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let workflow = state.get_workflow(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let run = Box::pin(execute_workflow(
        &state,
        &workflow,
        tenant_context,
        Some("manual".to_string()),
        None,
        None,
        false,
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "run": run })))
}

fn workflow_run_visible_to_caller(
    run: &tandem_workflows::WorkflowRunRecord,
    tenant_context: &TenantContext,
    request_principal: &tandem_types::RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if !super::tenant_matches(tenant_context, &run.tenant_context) {
        return false;
    }
    if workflow_reviewer_is_eligible(tenant_context, verified) {
        return true;
    }
    match (
        request_principal.actor_id.as_deref().map(str::trim),
        run.tenant_context.actor_id.as_deref().map(str::trim),
    ) {
        (Some(caller), Some(owner)) if !caller.is_empty() => caller.eq_ignore_ascii_case(owner),
        _ => false,
    }
}

pub(super) async fn workflow_runs_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<tandem_types::RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<WorkflowRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let mut runs = state
        .list_workflow_runs(query.workflow_id.as_deref(), limit)
        .await;
    runs.retain(|run| {
        workflow_run_visible_to_caller(
            run,
            &tenant_context,
            &request_principal,
            verified.as_deref(),
        )
    });
    Json(json!({ "runs": runs, "count": runs.len() }))
}

pub(super) async fn workflow_runs_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<tandem_types::RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let run = state
        .get_workflow_run(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    if !workflow_run_visible_to_caller(
        &run,
        &tenant_context,
        &request_principal,
        verified.as_deref(),
    ) {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(json!({ "run": run })))
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowGateDecisionInput {
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) fn workflow_reviewer_is_eligible(
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if verified.is_none() && super::tenant_is_standalone_local(tenant_context) {
        return true;
    }
    let Some(verified) = verified else {
        return false;
    };
    if crate::now_ms() >= verified.expires_at_ms
        || !super::tenant_matches(tenant_context, &verified.tenant_context)
    {
        return false;
    }
    verified
        .roles
        .iter()
        .any(|role| matches!(role.as_str(), "owner" | "admin"))
        || verified.capabilities.iter().any(|capability| {
            matches!(
                capability.as_str(),
                "approval.review" | "governance.review" | "governance.admin"
            )
        })
        || verified.strict_projection.as_ref().is_some_and(|strict| {
            strict.has_permission(AccessPermission::Admin)
                || strict.has_permission(AccessPermission::Delegate)
        })
}

async fn ensure_workflow_gate_digest_current(
    state: &AppState,
    run: &tandem_workflows::WorkflowRunRecord,
    gate: &tandem_workflows::WorkflowPendingGate,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    let current =
        crate::workflows::current_workflow_gate_action_digest(state, run, &gate.action_id)
            .await
            .map_err(|error| {
                (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": format!("workflow gate definition is stale: {error}"),
                        "code": "WORKFLOW_GATE_STALE",
                    })),
                )
            })?;
    if gate.action_digest.is_empty() && super::tenant_is_standalone_local(tenant_context) {
        return Ok(());
    }
    if gate.action_digest != current {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "workflow definition changed after the gate was created",
                "code": "WORKFLOW_GATE_STALE",
            })),
        ));
    }
    Ok(())
}

/// Decide a pending workflow approval gate (TAN-73). Mirrors the automation
/// v2 gate semantics: human-only decider, durable decision record, protected
/// audit event, and the dispatcher resumes from the checkpoint on approval.
pub(super) async fn workflow_run_gate_decide(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<tandem_types::RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: axum::http::HeaderMap,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
    Json(input): Json<WorkflowGateDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let run = state.get_workflow_run(&id).await.ok_or((
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "workflow run not found", "code": "WORKFLOW_RUN_NOT_FOUND" })),
    ))?;
    super::ensure_same_tenant(&tenant_context, &run.tenant_context).map_err(|status| {
        (
            status,
            Json(json!({ "error": "workflow run not found", "code": "WORKFLOW_RUN_NOT_FOUND" })),
        )
    })?;

    // GOV: deciding a gate is a privileged human action; agents cannot
    // approve their own work (same rule as automation v2 gates).
    let decider =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if decider.kind != crate::automation_v2::governance::GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "workflow approval gates must be decided by a human operator",
                "code": "WORKFLOW_GATE_REQUIRES_HUMAN",
            })),
        ));
    }

    if run.status != tandem_workflows::WorkflowRunStatus::AwaitingApproval {
        // Race UX parity with automation gates: surface the winning decision.
        let winner = run.gate_history.last().cloned();
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "workflow run is not awaiting approval",
                "code": "WORKFLOW_RUN_NOT_AWAITING_APPROVAL",
                "runID": id,
                "currentStatus": run.status,
                "decidedGate": winner,
            })),
        ));
    }
    let Some(gate) = run.awaiting_gate.clone() else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "workflow run has no pending gate",
                "code": "WORKFLOW_RUN_GATE_MISSING",
            })),
        ));
    };

    if !workflow_reviewer_is_eligible(
        &tenant_context,
        verified_tenant_context
            .as_ref()
            .map(|Extension(verified)| verified),
    ) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "workflow approval requires an eligible tenant reviewer",
                "code": "WORKFLOW_GATE_REVIEWER_FORBIDDEN",
            })),
        ));
    }
    if let (Some(reviewer_id), Some(requester_id)) = (
        decider.actor_id.as_deref().map(str::trim),
        gate.requested_by.as_deref().map(str::trim),
    ) {
        if !reviewer_id.is_empty() && reviewer_id.eq_ignore_ascii_case(requester_id) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "workflow gates require an independent reviewer",
                    "code": "WORKFLOW_GATE_SELF_APPROVAL",
                })),
            ));
        }
    }
    if gate.expires_at_ms > 0 && crate::now_ms() >= gate.expires_at_ms {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "workflow approval gate has expired",
                "code": "WORKFLOW_GATE_EXPIRED",
            })),
        ));
    }
    if !super::tenant_is_standalone_local(&tenant_context)
        && (gate.expires_at_ms == 0
            || gate.action_digest.is_empty()
            || gate.nonce.is_empty()
            || gate.requested_by.as_deref().is_none_or(str::is_empty))
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "legacy unbound workflow gate is quarantined",
                "code": "WORKFLOW_GATE_UNBOUND",
            })),
        ));
    }

    let decision = input.decision.trim().to_ascii_lowercase();
    if !gate.decisions.iter().any(|allowed| allowed == &decision) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("decision must be one of {:?}", gate.decisions),
                "code": "WORKFLOW_GATE_INVALID_DECISION",
            })),
        ));
    }
    if decision == "rework" && gate.rework_targets.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "gate has no rework_targets configured",
                "code": "WORKFLOW_GATE_NO_REWORK_TARGETS",
            })),
        ));
    }

    ensure_workflow_gate_digest_current(&state, &run, &gate, &tenant_context).await?;

    let record = tandem_workflows::WorkflowGateDecisionRecord {
        action_id: gate.action_id.clone(),
        decision: decision.clone(),
        approval_wait: Some(ApprovalWaitRef::for_gate(
            ApprovalSourceKind::Workflow,
            &run.run_id,
            &gate.action_id,
        )),
        reason: input.reason.clone(),
        decided_at_ms: crate::now_ms(),
        action_digest: gate.action_digest.clone(),
        nonce: gate.nonce.clone(),
        decided_by: serde_json::to_value(&decider).ok(),
    };
    // Protected audit is the commit barrier: a ledger failure must leave the run
    // paused and the one-time nonce unused.
    crate::audit::append_protected_audit_event(
        &state,
        "workflow.governance.gate_decided",
        &tenant_context,
        decider.actor_id.clone().or_else(|| decider.source.clone()),
        json!({
            "runID": id,
            "workflowID": run.workflow_id,
            "actionID": gate.action_id,
            "actionDigest": gate.action_digest,
            "nonce": gate.nonce,
            "decision": decision,
            "reason": input.reason,
            "decidedBy": decider,
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
    ensure_workflow_gate_digest_current(&state, &run, &gate, &tenant_context).await?;
    let gate_action_id = gate.action_id.clone();
    let rework_targets = gate.rework_targets.clone();
    let decision_for_update = decision.clone();
    let record_for_update = record.clone();
    let result = state
        .update_workflow_run_persisted(&id, |row| {
            if row.status != tandem_workflows::WorkflowRunStatus::AwaitingApproval
                || row
                    .awaiting_gate
                    .as_ref()
                    .is_none_or(|current| current.nonce != gate.nonce)
            {
                return false;
            }
            row.awaiting_gate = None;
            row.gate_history.push(record_for_update.clone());
            match decision_for_update.as_str() {
                "approve" => {
                    row.status = tandem_workflows::WorkflowRunStatus::Running;
                    if let Some(action) = row
                        .actions
                        .iter_mut()
                        .find(|action| action.action_id == gate_action_id)
                    {
                        action.status = tandem_workflows::WorkflowActionRunStatus::Completed;
                        action.detail = Some("gate approved".to_string());
                        action.output = Some(json!({
                            "decision": "approve",
                            "reason": record_for_update.reason,
                        }));
                        action.updated_at_ms = crate::now_ms();
                    }
                }
                "rework" => {
                    row.status = tandem_workflows::WorkflowRunStatus::Running;
                    for action in row.actions.iter_mut() {
                        if rework_targets
                            .iter()
                            .any(|target| target == &action.action_id)
                        {
                            action.status = tandem_workflows::WorkflowActionRunStatus::Pending;
                            action.detail = Some("re-queued by gate rework".to_string());
                            action.output = None;
                            action.updated_at_ms = crate::now_ms();
                        }
                    }
                }
                _ => {
                    row.status = tandem_workflows::WorkflowRunStatus::Cancelled;
                    row.finished_at_ms = Some(crate::now_ms());
                    if let Some(action) = row
                        .actions
                        .iter_mut()
                        .find(|action| action.action_id == gate_action_id)
                    {
                        action.status = tandem_workflows::WorkflowActionRunStatus::Skipped;
                        action.detail = Some("gate cancelled".to_string());
                        action.updated_at_ms = crate::now_ms();
                    }
                }
            }
            true
        })
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("workflow gate decision could not be persisted: {error}"),
                    "code": "WORKFLOW_GATE_PERSISTENCE_FAILED",
                })),
            )
        })?;
    let Some((updated, applied)) = result else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "workflow run not found", "code": "WORKFLOW_RUN_NOT_FOUND" })),
        ));
    };
    if !applied {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "workflow gate was already decided or replaced",
                "code": "WORKFLOW_GATE_REPLAYED",
            })),
        ));
    }

    if decision == "approve" || decision == "cancel" {
        crate::workflows::sync_workflow_gate_decision_to_mirror(
            &state,
            &updated,
            &gate.action_id,
            &decision,
            &json!({ "decision": decision, "reason": input.reason }),
        )
        .await;
    }

    state.event_bus.publish(EngineEvent::new(
        "approval.decision.recorded",
        json!({
            "run_id": id,
            "workflow_id": updated.workflow_id,
            "node_id": gate.action_id,
            "decision": decision,
            "reason": input.reason,
            "executed_as": "workflow_gate",
            "decided_by": decider,
            "tenantContext": tenant_context,
        }),
    ));

    if matches!(updated.status, tandem_workflows::WorkflowRunStatus::Running) {
        let resume_state = state.clone();
        let resume_run_id = id.clone();
        tokio::spawn(async move {
            if let Err(error) = crate::resume_workflow_run(&resume_state, &resume_run_id).await {
                tracing::warn!(
                    "workflow run `{resume_run_id}` failed to resume after gate decision: {error:#}"
                );
                let _ = resume_state
                    .update_workflow_run(&resume_run_id, |row| {
                        row.status = tandem_workflows::WorkflowRunStatus::Failed;
                        row.finished_at_ms = Some(crate::now_ms());
                    })
                    .await;
            }
        });
    }

    Ok(Json(json!({ "ok": true, "run": updated })))
}

fn workflow_event_tenant_context(event: &EngineEvent) -> TenantContext {
    event
        .properties
        .get("tenantContext")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_else(TenantContext::local_implicit)
}

pub(super) fn workflow_events_stream(
    state: AppState,
    tenant_context: TenantContext,
    workflow_id: Option<String>,
    run_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "workflows",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("workflow.") {
                return None;
            }
            if !super::tenant_matches(&tenant_context, &workflow_event_tenant_context(&event)) {
                return None;
            }
            if let Some(expected) = workflow_id.as_deref() {
                let actual = event
                    .properties
                    .get("workflowID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if actual != expected {
                    return None;
                }
            }
            if let Some(expected) = run_id.as_deref() {
                let actual = event
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if actual != expected {
                    return None;
                }
            }
            Some(Ok(
                Event::default().data(serde_json::to_string(&event).unwrap_or_default())
            ))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn workflow_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<WorkflowEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(workflow_events_stream(
        state,
        tenant_context,
        query.workflow_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
