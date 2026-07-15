// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Tenant-scoped runtime APIs for long-running goals (TAN-695): lifecycle,
//! graph/lineage/event read models, governed handoff emission and decisions,
//! external-condition wait inspection/resolution, and an SSE change stream.
//!
//! Every event emitted over SSE comes from the durable stateful store in
//! rowid order — the rowid doubles as the SSE event id, so a reconnect with
//! `Last-Event-ID` (or `?cursor=`) resumes with no missing and no duplicate
//! events, regardless of how the in-memory event bus behaved in between.

use super::*;

use tandem_automation::{
    AutomationV2RunRecord, LongRunningGoal, OrchestrationArtifactRef, OrchestrationNodeKind,
    OrchestrationSpec,
};
use tandem_types::{PrincipalKind, PrincipalRef, RequestPrincipal, VerifiedTenantContext};

use crate::stateful_runtime::{
    list_stateful_waits, GoalPauseOutcome, GoalResumeOutcome, GovernedTransitionRequest,
    GovernedTransitionResult, OrchestrationStateStore, OrchestrationTransitionAuthority,
    StartGoalOutcome, StatefulRuntimeStoragePaths, StatefulWaitQuery, WorkflowCompletionResult,
};

const DEFAULT_GOAL_LIST_LIMIT: usize = 100;
const MAX_GOAL_LIST_LIMIT: usize = 500;
const DEFAULT_GOAL_EVENT_LIMIT: usize = 250;
const MAX_GOAL_EVENT_LIMIT: usize = 1_000;
/// SSE replay page size; the stream keeps paging until it drains.
const SSE_REPLAY_PAGE: usize = 500;

/// File-backed artifact admission: the content path must resolve to a real
/// file inside the workspace root — symlink escapes included, because the
/// check runs on the canonicalized path — and the file must match its
/// declared digest. Runs at every emit surface (HTTP and MCP) before a
/// transition is attempted.
pub(super) fn enforce_artifact_content_policy(
    workspace_root: &str,
    artifact: &OrchestrationArtifactRef,
) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(content_path) = artifact.content_path.as_deref() else {
        return Ok(());
    };
    let root = std::path::Path::new(workspace_root)
        .canonicalize()
        .context("workspace root is unavailable for artifact validation")?;
    let resolved = root.join(content_path).canonicalize().with_context(|| {
        format!("handoff artifact content `{content_path}` does not resolve to a workspace file")
    })?;
    if !resolved.starts_with(&root) {
        anyhow::bail!("handoff artifact content `{content_path}` escapes the workspace root");
    }
    if !resolved.is_file() {
        anyhow::bail!("handoff artifact content `{content_path}` is not a regular file");
    }
    if let Some(digest) = artifact.content_digest.as_deref() {
        let expected = digest.strip_prefix("sha256:").unwrap_or(digest);
        let actual = sha256_file_hex(&resolved)?;
        if !expected.eq_ignore_ascii_case(&actual) {
            anyhow::bail!(
                "handoff artifact content digest mismatch for `{content_path}`: declared {expected}, found {actual}"
            );
        }
    }
    Ok(())
}

fn sha256_file_hex(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn artifact_policy_response(error: &anyhow::Error) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": "artifact_policy_violation",
            "detail": error.to_string(),
        })),
    )
        .into_response()
}

pub(super) fn goal_store(state: &AppState) -> Result<OrchestrationStateStore, Response> {
    OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path).map_err(
        |error| {
            tracing::error!(?error, "failed to open orchestration store");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "orchestration_store_unavailable"})),
            )
                .into_response()
        },
    )
}

pub(super) fn goal_error_response(error: &anyhow::Error) -> Response {
    let message = error.to_string();
    let (status, code) = if message.contains("not found") {
        (StatusCode::NOT_FOUND, "goal_not_found")
    } else if message.contains("tenant scope") {
        // Fail closed: cross-tenant reads are indistinguishable from absence.
        (StatusCode::NOT_FOUND, "goal_not_found")
    } else if message.contains("terminal") {
        (StatusCode::CONFLICT, "goal_terminal")
    } else if message.contains("idempotency key is already bound")
        || message.contains("raced")
        || message.contains("being claimed")
        || message.contains("no longer eligible")
    {
        (StatusCode::CONFLICT, "goal_state_conflict")
    } else if message.contains("not authorized") {
        (StatusCode::FORBIDDEN, "goal_forbidden")
    } else {
        (StatusCode::BAD_REQUEST, "invalid_goal_request")
    };
    (status, Json(json!({"error": code, "detail": message}))).into_response()
}

pub(super) fn request_actor(principal: &RequestPrincipal) -> PrincipalRef {
    PrincipalRef::new(
        PrincipalKind::HumanUser,
        principal.actor_id.as_deref().unwrap_or("anonymous"),
    )
}

/// The actor a mutation is attributed to: the verified assertion's human
/// actor when present, else the request principal. Audit records must name
/// this identity, not the raw transport principal.
pub(super) fn effective_actor(
    principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> PrincipalRef {
    verified
        .map(|context| context.human_actor.actor_id.trim())
        .filter(|actor| !actor.is_empty())
        .map(|actor| PrincipalRef::new(PrincipalKind::HumanUser, actor))
        .unwrap_or_else(|| request_actor(principal))
}

/// Run-as/authority-chain context recorded into audit events so receipts can
/// distinguish the effective actor from the delegating principal.
pub(super) fn run_as_context(verified: Option<&VerifiedTenantContext>) -> Value {
    verified
        .map(|context| {
            json!({
                "actor_id": context.human_actor.actor_id,
                "issuer": context.issuer,
                "assertion_id": context.assertion_id,
                "roles": context.roles,
                "org_units": context.org_units,
                "initiated_by": context.authority_chain.initiated_by,
                "executed_as": context.authority_chain.executed_as,
            })
        })
        .unwrap_or(Value::Null)
}

pub(super) fn publish_goal_audit_receipt(
    state: &AppState,
    tenant: &TenantContext,
    event_type: &str,
    goal_id: &str,
    actor: &PrincipalRef,
    verified: Option<&VerifiedTenantContext>,
    mut details: serde_json::Map<String, Value>,
) {
    details.insert("goalID".to_string(), json!(goal_id));
    details.insert("effective_actor".to_string(), json!(actor));
    details.insert("run_as".to_string(), run_as_context(verified));
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            event_type,
            tenant,
            Value::Object(details),
        ));
}

pub(super) fn verified_has_admin_authority(verified: Option<&VerifiedTenantContext>) -> bool {
    verified.is_some_and(|context| {
        context.roles.iter().any(|role| {
            matches!(
                role.as_str(),
                "owner" | "admin" | "hosted:owner" | "hosted:admin" | "enterprise:admin"
            )
        }) || context.capabilities.iter().any(|capability| {
            matches!(
                capability.as_str(),
                "hosted.admin" | "orchestration.control" | "orchestration.write"
            )
        })
    })
}

/// Enterprise authority gate for goal mutations. In hosted and enterprise
/// auth modes the ingress middleware already rejects explicit tenants without
/// a verified assertion, so a present `VerifiedTenantContext` is the signal
/// that enterprise enforcement applies: capability-gated surfaces (approval,
/// wait resolution) then require the named capability or an administrative
/// role. Local single-tenant mode (no assertion) keeps its operator UX. Deny
/// wins: nothing in the request body or tenant headers can satisfy this
/// check.
pub(super) fn require_goal_authority(
    _tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    required_capability: Option<&str>,
) -> Result<(), Response> {
    let Some(verified) = verified else {
        // Local single-tenant mode: hosted ingress never reaches here
        // unverified because unsigned tenant headers are denied upstream.
        return Ok(());
    };
    if let Some(capability) = required_capability {
        let authorized = verified
            .capabilities
            .iter()
            .any(|value| value == capability)
            || verified.roles.iter().any(|role| {
                matches!(
                    role.as_str(),
                    "owner" | "admin" | "hosted:owner" | "hosted:admin" | "enterprise:admin"
                )
            });
        if !authorized {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "goal_forbidden",
                    "detail": format!("authenticated principal lacks {capability} authority"),
                })),
            )
                .into_response());
        }
    }
    Ok(())
}

/// Goal mutations belong to the goal's initiating actor unless the caller
/// carries administrative authority. Enforced for verified (hosted) callers;
/// the local single-operator is always the owner.
pub(super) fn require_goal_owner(
    _tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    goal: &LongRunningGoal,
    actor: &PrincipalRef,
) -> Result<(), Response> {
    if verified.is_none() || verified_has_admin_authority(verified) {
        return Ok(());
    }
    let started_by = goal
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("started_by"))
        .and_then(|value| {
            value
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
        });
    if started_by != Some(actor.id.as_str()) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "goal_forbidden",
                "detail": "goal mutation requires its initiating actor or an authorized administrator",
            })),
        )
            .into_response());
    }
    Ok(())
}

/// Authority for emit/approve surfaces, derived from the verified principal.
fn transition_authority(
    principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
    can_approve: bool,
) -> OrchestrationTransitionAuthority {
    OrchestrationTransitionAuthority {
        actor: effective_actor(principal, verified),
        can_emit: true,
        can_approve,
    }
}

pub(super) fn load_tenant_goal(
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    goal_id: &str,
) -> Result<LongRunningGoal, Response> {
    // Scope is enforced in the store's SQL (org/workspace/deployment); the
    // request tenant's actor_id never affects resource visibility.
    match store.get_goal_for_tenant(tenant, goal_id) {
        Ok(Some(goal)) => Ok(goal),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "goal_not_found"})),
        )
            .into_response()),
        Err(error) => Err(goal_error_response(&error)),
    }
}

fn goal_response(goal: &LongRunningGoal) -> Value {
    json!({
        "goal": goal,
        "goal_id": goal.goal_id,
        "status": goal.status,
        "budgets": goal_budgets(goal),
    })
}

pub(super) fn goal_budgets(goal: &LongRunningGoal) -> Value {
    json!({
        "policy": goal.policy,
        "consumed": {
            "hops": goal.hop_count,
            "total_tokens": goal.total_tokens,
            "total_cost_usd": goal.total_cost_usd,
        },
        "remaining": {
            "hops": goal.policy.max_hops.saturating_sub(goal.hop_count),
            "tokens": goal
                .policy
                .max_total_tokens
                .map(|maximum| maximum.saturating_sub(goal.total_tokens)),
            "cost_usd": goal
                .policy
                .max_total_cost_usd
                .map(|maximum| (maximum - goal.total_cost_usd).max(0.0)),
            "deadline_ms": goal
                .policy
                .deadline_at_ms
                .map(|deadline| deadline.saturating_sub(crate::now_ms())),
        },
    })
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct StartGoalPayload {
    pub orchestration_id: String,
    #[serde(default)]
    pub orchestration_version: Option<u64>,
    pub objective: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub metadata: Option<Value>,
}

pub(super) async fn start_goal(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(payload): Json<StartGoalPayload>,
) -> Response {
    let verified = verified.as_deref();
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    let actor = effective_actor(&principal, verified);
    let mut metadata = match payload.metadata {
        Some(Value::Object(object)) => object,
        Some(other) => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), other);
            object
        }
        None => serde_json::Map::new(),
    };
    // Goal ownership is transport authority, never caller-controlled data.
    metadata.insert("started_by".to_string(), json!(actor));
    let request = crate::app::state::StartGoalRequest {
        orchestration_id: payload.orchestration_id,
        orchestration_version: payload.orchestration_version,
        objective: payload.objective,
        idempotency_key: payload.idempotency_key,
        metadata: Some(Value::Object(metadata)),
        now_ms: crate::now_ms(),
    };
    match state
        .start_long_running_goal(&tenant, &request, &actor)
        .await
    {
        Ok(StartGoalOutcome::Created { goal, root_run }) => (
            StatusCode::CREATED,
            Json(json!({
                "goal": goal,
                "root_run_id": root_run.run_id,
                "replayed": false,
            })),
        )
            .into_response(),
        Ok(StartGoalOutcome::AlreadyStarted { goal, root_run }) => Json(json!({
            "goal": goal,
            "root_run_id": root_run.run_id,
            "replayed": true,
        }))
        .into_response(),
        Err(error) => goal_error_response(&error),
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct GoalListQuery {
    pub status: Option<String>,
    pub orchestration_id: Option<String>,
    pub limit: Option<usize>,
}

pub(super) async fn list_goals(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<GoalListQuery>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_GOAL_LIST_LIMIT)
        .clamp(1, MAX_GOAL_LIST_LIMIT);
    match store.list_goals(
        &tenant,
        query.status.as_deref(),
        query.orchestration_id.as_deref(),
        limit,
    ) {
        Ok(goals) => Json(json!({"goals": goals, "count": goals.len()})).into_response(),
        Err(error) => goal_error_response(&error),
    }
}

pub(super) async fn get_goal(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => Json(goal_response(&goal)).into_response(),
        Err(response) => response,
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct GoalControlPayload {
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) async fn pause_goal(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(goal_id): Path<String>,
    Json(payload): Json<GoalControlPayload>,
) -> Response {
    let verified = verified.as_deref();
    let reason = payload.reason.as_deref().unwrap_or("operator pause");
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let actor = effective_actor(&principal, verified);
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    if let Err(response) = require_goal_owner(&tenant, verified, &stored, &actor) {
        return response;
    }
    match state
        .pause_long_running_goal(&goal_id, &stored.tenant_context, reason, &actor)
        .await
    {
        Ok((outcome, goal)) => {
            let outcome_name = match outcome {
                GoalPauseOutcome::Applied => "paused",
                GoalPauseOutcome::AlreadyPaused => "already_paused",
            };
            publish_goal_audit_receipt(
                &state,
                &tenant,
                "orchestration.goal.action_receipt",
                &goal_id,
                &actor,
                verified,
                serde_json::Map::from_iter([
                    ("action".to_string(), json!("pause")),
                    ("outcome".to_string(), json!(outcome_name)),
                ]),
            );
            Json(json!({"goal": goal, "outcome": outcome_name})).into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

pub(super) async fn resume_goal(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(goal_id): Path<String>,
    Json(payload): Json<GoalControlPayload>,
) -> Response {
    let verified = verified.as_deref();
    let reason = payload.reason.as_deref().unwrap_or("operator resume");
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let actor = effective_actor(&principal, verified);
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    if let Err(response) = require_goal_owner(&tenant, verified, &stored, &actor) {
        return response;
    }
    match state
        .resume_long_running_goal(&goal_id, &stored.tenant_context, reason, &actor)
        .await
    {
        Ok((outcome, goal)) => {
            let outcome_name = match outcome {
                GoalResumeOutcome::Applied => "resumed",
                GoalResumeOutcome::NotPaused => "not_paused",
            };
            publish_goal_audit_receipt(
                &state,
                &tenant,
                "orchestration.goal.action_receipt",
                &goal_id,
                &actor,
                verified,
                serde_json::Map::from_iter([
                    ("action".to_string(), json!("resume")),
                    ("outcome".to_string(), json!(outcome_name)),
                ]),
            );
            Json(json!({"goal": goal, "outcome": outcome_name})).into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

pub(super) async fn cancel_goal(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(goal_id): Path<String>,
    Json(payload): Json<GoalControlPayload>,
) -> Response {
    let verified = verified.as_deref();
    let reason = payload.reason.as_deref().unwrap_or("operator cancel");
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let actor = effective_actor(&principal, verified);
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    if let Err(response) = require_goal_owner(&tenant, verified, &stored, &actor) {
        return response;
    }
    match state
        .cancel_long_running_goal(&goal_id, &stored.tenant_context, reason, &actor)
        .await
    {
        Ok(result) => {
            let outcome = format!("{:?}", result.outcome);
            publish_goal_audit_receipt(
                &state,
                &tenant,
                "orchestration.goal.action_receipt",
                &goal_id,
                &actor,
                verified,
                serde_json::Map::from_iter([
                    ("action".to_string(), json!("cancel")),
                    ("outcome".to_string(), json!(outcome)),
                ]),
            );
            Json(json!({
                "goal": result.goal,
                "outcome": outcome,
                "cancelled_run_id": result.cancelled_run.as_ref().map(|run| &run.run_id),
                "cancelled_wait_ids": result.cancelled_wait_ids,
                "dead_lettered_handoff_ids": result.dead_lettered_handoff_ids,
            }))
            .into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

// ---------------------------------------------------------------------------
// Read models: graph, runs, events, artifacts, budgets
// ---------------------------------------------------------------------------

pub(super) async fn get_goal_graph(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let goal = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let orchestration = match store.get_orchestration_for_tenant(
        &tenant,
        &goal.orchestration_id,
        goal.orchestration_version,
    ) {
        Ok(Some(spec)) => spec,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "orchestration_not_found"})),
            )
                .into_response()
        }
        Err(error) => return goal_error_response(&error),
    };
    let links = store
        .list_goal_run_links_for_tenant(&tenant, &goal_id)
        .unwrap_or_default();
    let mut runs_by_node = std::collections::HashMap::<String, Vec<Value>>::new();
    let mut active_run: Option<AutomationV2RunRecord> = None;
    for link in &links {
        let run = state.get_automation_v2_run(&link.run_id).await;
        if goal.active_run_id.as_deref() == Some(link.run_id.as_str()) {
            active_run = run.clone();
        }
        runs_by_node
            .entry(link.orchestration_node_id.clone())
            .or_default()
            .push(json!({
                "run_id": link.run_id,
                "hop_index": link.hop_index,
                "parent_run_id": link.parent_run_id,
                "triggering_handoff_id": link.triggering_handoff_id,
                "status": run.as_ref().map(|run| json!(run.status)).unwrap_or(Value::Null),
            }));
    }
    let nodes = orchestration
        .nodes
        .iter()
        .map(|node| {
            let node_runs = runs_by_node.remove(&node.node_id).unwrap_or_default();
            let node_state = if goal.current_node_id.as_deref() == Some(node.node_id.as_str()) {
                "current"
            } else if !node_runs.is_empty() {
                "visited"
            } else {
                "not_started"
            };
            json!({
                "node_id": node.node_id,
                "name": node.name,
                "kind": node.node,
                "state": node_state,
                "runs": node_runs,
            })
        })
        .collect::<Vec<_>>();
    // The internal Automation V2 stage of the active run: the first pending
    // node in its checkpoint (or the last completed one when it is settling).
    let current_stage = active_run.as_ref().map(|run| {
        json!({
            "run_id": run.run_id,
            "automation_id": run.automation_id,
            "status": run.status,
            "current_node_id": run
                .checkpoint
                .pending_nodes
                .first()
                .or(run.checkpoint.completed_nodes.last()),
            "completed_nodes": run.checkpoint.completed_nodes.len(),
            "pending_nodes": run.checkpoint.pending_nodes.len(),
        })
    });
    Json(json!({
        "goal_id": goal_id,
        "status": goal.status,
        "orchestration_id": goal.orchestration_id,
        "orchestration_version": goal.orchestration_version,
        "current_node_id": goal.current_node_id,
        "current_workflow": current_stage,
        "nodes": nodes,
        "edges": orchestration.edges,
        "budgets": goal_budgets(&goal),
    }))
    .into_response()
}

pub(super) async fn list_goal_runs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let goal = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let links = store
        .list_goal_run_links_for_tenant(&tenant, &goal_id)
        .unwrap_or_default();
    let mut rows = Vec::new();
    for link in &links {
        let run = state.get_automation_v2_run(&link.run_id).await;
        rows.push(json!({
            "link": link,
            "run": run,
        }));
    }
    Json(json!({
        "goal_id": goal_id,
        "active_run_id": goal.active_run_id,
        "runs": rows,
        "count": rows.len(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct GoalEventsQuery {
    /// Durable store cursor (SSE `Last-Event-ID` compatible).
    pub cursor: Option<i64>,
    pub limit: Option<usize>,
}

pub(super) async fn list_goal_events(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
    Query(query): Query<GoalEventsQuery>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    if let Err(response) = load_tenant_goal(&store, &tenant, &goal_id) {
        return response;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_GOAL_EVENT_LIMIT)
        .clamp(1, MAX_GOAL_EVENT_LIMIT);
    match store.query_goal_events_for_tenant(&tenant, &goal_id, query.cursor, limit) {
        Ok(rows) => {
            let last_cursor = rows.last().map(|row| row.cursor);
            Json(json!({
                "goal_id": goal_id,
                "events": rows
                    .iter()
                    .map(|row| json!({"cursor": row.cursor, "event": goal_event_wire(row.event.clone())}))
                    .collect::<Vec<_>>(),
                "count": rows.len(),
                "last_cursor": last_cursor,
                "event_source": "stateful_runtime",
            }))
            .into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct GoalStreamQuery {
    pub cursor: Option<i64>,
}

/// SSE stream of durable goal events. Replays everything after the cursor
/// (query param or `Last-Event-ID` header), then tails the store, waking on
/// engine-bus activity with a polling floor so no durable write is missed.
pub(super) async fn stream_goal_events(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
    Query(query): Query<GoalStreamQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, Response> {
    let store = goal_store(&state)?;
    load_tenant_goal(&store, &tenant, &goal_id)?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok());
    let mut cursor = last_event_id.or(query.cursor).unwrap_or(0);

    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(256);
    let mut bus = state.event_bus.subscribe();
    let scope_tenant = tenant.clone();
    tokio::spawn(async move {
        let ready = Event::default().event("ready").data(
            json!({
                "goal_id": goal_id,
                "cursor": cursor,
                "timestamp_ms": crate::now_ms(),
            })
            .to_string(),
        );
        if tx.send(ready).await.is_err() {
            return;
        }
        loop {
            // Drain everything the durable log has after the cursor. Rowid
            // order plus the id header gives exact-once reconnect semantics.
            loop {
                let rows = match store.query_goal_events_for_tenant(
                    &scope_tenant,
                    &goal_id,
                    Some(cursor),
                    SSE_REPLAY_PAGE,
                ) {
                    Ok(rows) => rows,
                    Err(error) => {
                        let _ = tx
                            .send(Event::default().event("error").data(
                                json!({"error": "goal_event_read_failed", "detail": error.to_string()})
                                    .to_string(),
                            ))
                            .await;
                        return;
                    }
                };
                let drained = rows.len() < SSE_REPLAY_PAGE;
                for row in rows {
                    cursor = row.cursor;
                    let event = Event::default()
                        .id(row.cursor.to_string())
                        .event(row.event.event_type.clone())
                        .data(
                            serde_json::to_string(&json!({
                                "cursor": row.cursor,
                                "event": goal_event_wire(row.event),
                            }))
                            .unwrap_or_default(),
                        );
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
                if drained {
                    break;
                }
            }
            // Wait for a wake signal: engine-bus activity for this goal, or
            // the polling floor so durable writes from other processes are
            // still picked up promptly.
            tokio::select! {
                received = bus.recv() => {
                    match received {
                        Ok(event) => {
                            let relevant = event.event_type.starts_with("orchestration.goal")
                                && event
                                    .properties
                                    .get("goalID")
                                    .and_then(Value::as_str)
                                    .is_none_or(|id| id == goal_id);
                            if !relevant {
                                continue;
                            }
                        }
                        Err(_) => {
                            // Lagged or closed: fall through to a store poll.
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(750)) => {}
            }
            if tx.is_closed() {
                return;
            }
        }
    });
    Ok(Sse::new(futures::StreamExt::map(
        tokio_stream::wrappers::ReceiverStream::new(rx),
        Ok,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

pub(super) fn goal_event_wire(
    mut event: crate::stateful_runtime::StatefulRunEventRecord,
) -> crate::stateful_runtime::StatefulRunEventRecord {
    if let Some(payload) = event.payload.as_object_mut() {
        payload.remove("projection_snapshot");
        payload.remove("projection_snapshot_ref");
    }
    event
}

pub(super) async fn list_goal_artifacts(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let goal = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let handoffs = store
        .list_goal_handoffs_for_tenant(&tenant, &goal_id)
        .unwrap_or_default();
    let artifacts = handoffs
        .iter()
        .map(|handoff| {
            json!({
                "artifact": handoff.artifact,
                "handoff_id": handoff.handoff_id,
                "transition_key": handoff.transition_key,
                "source_run_id": handoff.source_run_id,
                "consumed_by_run_id": handoff.consumed_by_run_id,
                "created_at_ms": handoff.created_at_ms,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "goal_id": goal_id,
        "artifacts": artifacts,
        "final_artifact": goal.final_artifact,
        "count": artifacts.len(),
    }))
    .into_response()
}

pub(super) async fn get_goal_budgets(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => Json(json!({
            "goal_id": goal_id,
            "status": goal.status,
            "budgets": goal_budgets(&goal),
        }))
        .into_response(),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Handoffs: emit, list, decide; workflow completion settlement
// ---------------------------------------------------------------------------

pub(super) async fn list_goal_handoffs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    if let Err(response) = load_tenant_goal(&store, &tenant, &goal_id) {
        return response;
    }
    match store.list_goal_handoffs_for_tenant(&tenant, &goal_id) {
        Ok(handoffs) => {
            Json(json!({"goal_id": goal_id, "handoffs": handoffs, "count": handoffs.len()}))
                .into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct EmitTransitionPayload {
    pub transition_key: String,
    pub idempotency_key: String,
    pub artifact: OrchestrationArtifactRef,
}

pub(super) async fn emit_goal_transition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(goal_id): Path<String>,
    Json(payload): Json<EmitTransitionPayload>,
) -> Response {
    let verified = verified.as_deref();
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        // Terminal goals reject transition emissions with a stable contract
        // (only governed recovery operations may touch them).
        Ok(goal) if goal.status.is_terminal() => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "goal_terminal",
                    "detail": "terminal goals cannot emit transitions",
                    "status": goal.status,
                })),
            )
                .into_response()
        }
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let actor = effective_actor(&principal, verified);
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    if let Err(response) = require_goal_owner(&tenant, verified, &stored, &actor) {
        return response;
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    if let Err(error) = enforce_artifact_content_policy(&workspace_root, &payload.artifact) {
        return artifact_policy_response(&error);
    }
    let request = GovernedTransitionRequest {
        transition_key: payload.transition_key,
        idempotency_key: payload.idempotency_key,
        artifact: payload.artifact,
        now_ms: crate::now_ms(),
    };
    let authority = transition_authority(&principal, verified, false);
    match state
        .emit_orchestration_transition(&goal_id, &request, &authority)
        .await
    {
        Ok(GovernedTransitionResult::Committed {
            commit,
            handoff,
            downstream_run,
            link,
            goal,
        }) => Json(json!({
            "outcome": "committed",
            "commit": format!("{commit:?}"),
            "handoff": handoff,
            "downstream_run_id": downstream_run.run_id,
            "link": link,
            "goal": goal,
        }))
        .into_response(),
        Ok(GovernedTransitionResult::PendingApproval { handoff, goal }) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "outcome": "pending_approval",
                "handoff": handoff,
                "goal": goal,
            })),
        )
            .into_response(),
        Err(error) => goal_error_response(&error),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct HandoffDecisionPayload {
    /// `approve` or `reject`.
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) async fn decide_goal_handoff(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path((goal_id, handoff_id)): Path<(String, String)>,
    Json(payload): Json<HandoffDecisionPayload>,
) -> Response {
    let verified = verified.as_deref();
    // Approvals move authority between workflows: they require the approval
    // capability (or an administrative role) on explicit tenants.
    if let Err(response) = require_goal_authority(&tenant, verified, Some("orchestration.approve"))
    {
        return response;
    }
    let approve = match payload.decision.as_str() {
        "approve" => true,
        "reject" => false,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_goal_request",
                    "detail": "decision must be approve or reject",
                })),
            )
                .into_response()
        }
    };
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    match store.get_workflow_handoff_for_tenant(&tenant, &handoff_id) {
        Ok(Some(handoff)) if handoff.goal_id == goal_id => {}
        Ok(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "handoff_not_found"})),
            )
                .into_response()
        }
        Err(error) => return goal_error_response(&error),
    }
    let authority = transition_authority(&principal, verified, true);
    match store.decide_pending_handoff(
        &handoff_id,
        &stored.tenant_context,
        approve,
        &authority,
        crate::now_ms(),
    ) {
        Ok(handoff) => {
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.handoff_decided",
                    &tenant,
                    json!({
                        "goalID": goal_id,
                        "handoffID": handoff_id,
                        "approved": approve,
                        "reason": payload.reason,
                        "effective_actor": authority.actor,
                        "run_as": run_as_context(verified),
                    }),
                ));
            Json(json!({"handoff": handoff})).into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CompletionPayload {
    #[serde(default)]
    pub transition_key: Option<String>,
    #[serde(default)]
    pub final_artifact: Option<OrchestrationArtifactRef>,
}

/// Settle the active workflow's completion: either into a terminal node via
/// `transition_key`, or into the awaiting-transition state when omitted.
pub(super) async fn settle_goal_completion(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(goal_id): Path<String>,
    Json(payload): Json<CompletionPayload>,
) -> Response {
    let verified = verified.as_deref();
    if let Err(response) = require_goal_authority(&tenant, verified, None) {
        return response;
    }
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let goal = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) if goal.status.is_terminal() => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "goal_terminal",
                    "detail": "terminal goals cannot settle workflow completion",
                    "status": goal.status,
                })),
            )
                .into_response()
        }
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let actor = effective_actor(&principal, verified);
    if let Err(response) = require_goal_owner(&tenant, verified, &goal, &actor) {
        return response;
    }
    if let Some(final_artifact) = payload.final_artifact.as_ref() {
        let workspace_root = state.workspace_index.snapshot().await.root;
        if let Err(error) = enforce_artifact_content_policy(&workspace_root, final_artifact) {
            return artifact_policy_response(&error);
        }
    }
    let authority = transition_authority(&principal, verified, false);
    match state
        .settle_orchestration_workflow_completion(
            &goal_id,
            payload.transition_key.as_deref(),
            payload.final_artifact,
            &authority,
        )
        .await
    {
        Ok(WorkflowCompletionResult::Waiting { goal }) => {
            Json(json!({"outcome": "awaiting_transition", "goal": goal})).into_response()
        }
        Ok(WorkflowCompletionResult::Terminal { goal }) => {
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.terminal",
                    &tenant,
                    json!({"goalID": goal_id, "status": goal.status}),
                ));
            Json(json!({"outcome": "terminal", "goal": goal})).into_response()
        }
        Err(error) => goal_error_response(&error),
    }
}

// ---------------------------------------------------------------------------
// Waits: list, inspect, resolve
// ---------------------------------------------------------------------------

pub(super) fn goal_waits(
    state: &AppState,
    tenant: &TenantContext,
    store: &OrchestrationStateStore,
    goal_id: &str,
) -> Vec<crate::stateful_runtime::StatefulWaitRecord> {
    let run_ids = store
        .list_goal_run_links_for_tenant(tenant, goal_id)
        .unwrap_or_default()
        .into_iter()
        .map(|link| link.run_id)
        .collect::<std::collections::HashSet<_>>();
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    list_stateful_waits(
        &paths.waits_path,
        tenant,
        StatefulWaitQuery {
            run_id: None,
            wait_kind: None,
            status: None,
            limit: None,
        },
    )
    .into_iter()
    .filter(|wait| run_ids.contains(&wait.run_id))
    .collect()
}

pub(super) async fn list_goal_waits(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(goal_id): Path<String>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    let waits = goal_waits(&state, &stored.tenant_context, &store, &goal_id);
    Json(json!({"goal_id": goal_id, "waits": waits, "count": waits.len()})).into_response()
}

pub(super) async fn get_goal_wait(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((goal_id, wait_id)): Path<(String, String)>,
) -> Response {
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    match goal_waits(&state, &stored.tenant_context, &store, &goal_id)
        .into_iter()
        .find(|wait| wait.wait_id == wait_id)
    {
        Some(wait) => Json(json!({"goal_id": goal_id, "wait": wait})).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "wait_not_found"})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct WaitResolutionPayload {
    pub idempotency_key: String,
    #[serde(default)]
    pub payload: Value,
}

pub(super) async fn resolve_goal_wait(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path((goal_id, wait_id)): Path<(String, String)>,
    Json(payload): Json<WaitResolutionPayload>,
) -> Response {
    let verified = verified.as_deref();
    // Wait resolution injects external state into a paused run: it requires
    // the resolve capability (or an administrative role) on explicit tenants.
    if let Err(response) =
        require_goal_authority(&tenant, verified, Some("orchestration.resolve_wait"))
    {
        return response;
    }
    let store = match goal_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let stored = match load_tenant_goal(&store, &tenant, &goal_id) {
        Ok(goal) => goal,
        Err(response) => return response,
    };
    if !goal_waits(&state, &stored.tenant_context, &store, &goal_id)
        .iter()
        .any(|wait| wait.wait_id == wait_id)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "wait_not_found"})),
        )
            .into_response();
    }
    match state
        .resolve_automation_v2_external_wait(
            &stored.tenant_context,
            &wait_id,
            &payload.idempotency_key,
            payload.payload,
        )
        .await
    {
        Ok(Some(wait)) => {
            let actor = effective_actor(&principal, verified);
            publish_goal_audit_receipt(
                &state,
                &tenant,
                "orchestration.goal.wait_resolution_receipt",
                &goal_id,
                &actor,
                verified,
                serde_json::Map::from_iter([("waitID".to_string(), json!(wait_id))]),
            );
            Json(json!({"goal_id": goal_id, "wait": wait})).into_response()
        }
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "wait_resolution_conflict",
                "detail": "wait is not eligible for this resolution (already woken with a different idempotency key, or not claimable)",
            })),
        )
            .into_response(),
        Err(error) => goal_error_response(&error),
    }
}
