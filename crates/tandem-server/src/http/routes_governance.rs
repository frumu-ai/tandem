// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_types::{AccessPermission, RequestPrincipal, TenantContext, VerifiedTenantContext};
use uuid::Uuid;

use crate::automation_v2::governance::{
    AutomationGrantKind, GovernanceActorKind, GovernanceActorRef, GovernanceApprovalRequestType,
    GovernanceApprovalStatus, GovernanceResourceRef,
};

use super::governance::{
    agent_creation_review_wire, agent_spend_wire, approval_request_wire,
    automation_governance_wire, automation_grant_wire, automation_lifecycle_summary_wire,
    enforce_mutation_or_audit, governance_error_response, premium_governance_required,
    resolve_governance_actor, resolve_governance_provenance,
};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub(super) struct GovernanceApprovalCreateInput {
    pub request_type: GovernanceApprovalRequestType,
    pub target_resource: GovernanceResourceRef,
    pub rationale: String,
    #[serde(default)]
    pub context: Value,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct GovernanceApprovalDecisionInput {
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationGrantCreateInput {
    #[serde(default)]
    pub approval_id: Option<String>,
    pub granted_to_agent_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationGrantRevokeInput {
    #[serde(default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationRetireInput {
    #[serde(default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationExtendInput {
    #[serde(default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct PolicyDecisionListQuery {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

fn governance_route_error(
    status: StatusCode,
    message: impl Into<String>,
    code: &str,
) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": message.into(),
            "code": code,
        })),
    )
}

fn governance_mutation_admin_allowed(
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
        || verified
            .capabilities
            .iter()
            .any(|capability| capability == "governance.admin")
        || verified
            .strict_projection
            .as_ref()
            .is_some_and(|strict| strict.has_permission(AccessPermission::Admin))
}

async fn require_active_automation_owner(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) -> Result<crate::AutomationV2Spec, (StatusCode, Json<Value>)> {
    let automation = state
        .get_automation_v2(automation_id)
        .await
        .ok_or_else(|| {
            governance_route_error(
                StatusCode::NOT_FOUND,
                "Automation not found",
                "AUTOMATION_GOVERNANCE_NOT_FOUND",
            )
        })?;
    if !super::tenant_matches(tenant_context, &automation.tenant_context()) {
        return Err(governance_route_error(
            StatusCode::NOT_FOUND,
            "Automation not found",
            "AUTOMATION_GOVERNANCE_NOT_FOUND",
        ));
    }
    Ok(automation)
}

fn governance_actor_id(actor: &GovernanceActorRef) -> Option<&str> {
    actor
        .actor_id
        .as_deref()
        .map(str::trim)
        .filter(|actor_id| !actor_id.is_empty())
}

async fn require_independent_mutation_approval(
    state: &AppState,
    tenant_context: &TenantContext,
    actor: &GovernanceActorRef,
    automation_id: &str,
    approval_id: Option<&str>,
    action: &str,
    allowed_types: &[GovernanceApprovalRequestType],
) -> Result<(), (StatusCode, Json<Value>)> {
    if super::tenant_is_standalone_local(tenant_context) {
        return Ok(());
    }
    let approval_id = approval_id
        .map(str::trim)
        .filter(|approval_id| !approval_id.is_empty())
        .ok_or_else(|| {
            governance_route_error(
                StatusCode::FORBIDDEN,
                "Hosted governance mutation requires an approved independent review",
                "AUTOMATION_GOVERNANCE_APPROVAL_REQUIRED",
            )
        })?;
    let approval = state
        .get_governance_approval_request_for_tenant(approval_id, tenant_context)
        .await
        .ok_or_else(|| {
            governance_route_error(
                StatusCode::NOT_FOUND,
                "Governance approval not found",
                "AUTOMATION_GOVERNANCE_APPROVAL_NOT_FOUND",
            )
        })?;
    let actor_id = governance_actor_id(actor).ok_or_else(|| {
        governance_route_error(
            StatusCode::FORBIDDEN,
            "Governance mutation requires an identified human actor",
            "AUTOMATION_GOVERNANCE_ACTOR_REQUIRED",
        )
    })?;
    let requested_by = governance_actor_id(&approval.requested_by);
    let reviewed_by = approval.reviewed_by.as_ref().and_then(governance_actor_id);
    let target_matches = matches!(
        approval.target_resource.resource_type.as_str(),
        "automation" | "automation_v2"
    ) && approval.target_resource.id == automation_id;
    let action_matches = approval
        .context
        .get("action")
        .and_then(Value::as_str)
        .is_some_and(|approved_action| approved_action == action);
    if approval.status != GovernanceApprovalStatus::Approved
        || crate::now_ms() >= approval.expires_at_ms
        || !allowed_types.contains(&approval.request_type)
        || !target_matches
        || !action_matches
        || requested_by.is_none_or(|requester| !requester.eq_ignore_ascii_case(actor_id))
        || reviewed_by.is_none()
        || reviewed_by.is_some_and(|reviewer| reviewer.eq_ignore_ascii_case(actor_id))
    {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Governance approval is not bound to this tenant, actor, resource, and action",
            "AUTOMATION_GOVERNANCE_APPROVAL_INVALID",
        ));
    }
    Ok(())
}

fn approval_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-tandem-approval-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn apply(router: axum::Router<AppState>) -> axum::Router<AppState> {
    router
        .route(
            "/governance/policy-decisions",
            axum::routing::get(governance_policy_decisions_list),
        )
        .route(
            "/governance/approvals",
            axum::routing::get(governance_approvals_list),
        )
        .route(
            "/governance/approvals",
            axum::routing::post(governance_approval_create),
        )
        .route(
            "/governance/approvals/{approval_id}/approve",
            axum::routing::post(governance_approval_approve),
        )
        .route(
            "/governance/approvals/{approval_id}/deny",
            axum::routing::post(governance_approval_deny),
        )
        .route(
            "/governance/spend",
            axum::routing::get(governance_spend_list),
        )
        .route(
            "/governance/agents/{agent_id}/spend",
            axum::routing::get(governance_spend_get),
        )
        .route(
            "/governance/reviews",
            axum::routing::get(governance_reviews_list),
        )
        .route(
            "/automations/v2/{id}/governance",
            axum::routing::get(automation_governance_get),
        )
        .route(
            "/automations/v2/{id}/grants",
            axum::routing::get(automation_grants_list).post(automation_grant_create),
        )
        .route(
            "/automations/v2/{id}/grants/{grant_id}",
            axum::routing::delete(automation_grant_revoke),
        )
        .route(
            "/automations/v2/{id}/restore",
            axum::routing::post(automation_restore),
        )
        .route(
            "/automations/v2/{id}/retire",
            axum::routing::post(automation_retire),
        )
        .route(
            "/automations/v2/{id}/extend",
            axum::routing::post(automation_extend),
        )
}

pub(super) async fn governance_policy_decisions_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<PolicyDecisionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = if let Some(run_id) = query.run_id.as_deref() {
        state
            .list_policy_decisions_for_run(&tenant_context, run_id, limit)
            .await
    } else {
        state.list_policy_decisions(&tenant_context, limit).await
    };
    Ok(Json(json!({
        "policy_decisions": rows,
        "count": rows.len(),
    })))
}

pub(super) async fn governance_approvals_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let mut rows = state
        .list_approval_requests_for_tenant(None, None, &tenant_context)
        .await;
    if !super::workflows::workflow_reviewer_is_eligible(&tenant_context, verified.as_deref()) {
        let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
        let caller = governance_actor_id(&actor);
        rows.retain(|approval| {
            caller.is_some_and(|caller| {
                governance_actor_id(&approval.requested_by)
                    .is_some_and(|requester| requester.eq_ignore_ascii_case(caller))
            })
        });
    }
    Ok(Json(json!({
        "approvals": rows.iter().map(approval_request_wire).collect::<Vec<_>>(),
        "count": rows.len(),
    })))
}

pub(super) async fn governance_approval_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<GovernanceApprovalCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let requested_by = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let request = state
        .request_approval(
            input.request_type,
            requested_by,
            input.target_resource,
            input.rationale,
            input.context,
            input.expires_at_ms,
            &tenant_context,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "GOVERNANCE_APPROVAL_CREATE_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({
        "approval": approval_request_wire(&request),
    })))
}

/// GOV-B4: an approval decision (approve or deny) must be made by a verified
/// human who is not the requester. Enforced before delegating to the governance
/// layer so the human-in-the-loop control cannot be satisfied by an agent or by
/// the requester self-reviewing.
async fn ensure_governance_review_authorized(
    state: &AppState,
    approval_id: &str,
    reviewer: &GovernanceActorRef,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    if reviewer.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can review governance approvals",
                "code": "GOVERNANCE_APPROVAL_REQUIRES_HUMAN",
            })),
        ));
    }
    // CT-09: scope the self-review lookup to the reviewer's tenant so a cross-tenant
    // receipt is treated as absent here; decide_approval_request is the hard gate.
    if let Some(existing) = state
        .get_governance_approval_request_for_tenant(approval_id, tenant_context)
        .await
    {
        // Separation of duties applies regardless of whether the requester was a
        // human or an agent. A human requester must not approve their own high-impact
        // governance change merely because the request was filed through a human surface.
        if let (Some(reviewer_id), Some(requester_id)) = (
            reviewer.actor_id.as_deref().map(str::trim),
            existing.requested_by.actor_id.as_deref().map(str::trim),
        ) {
            if !reviewer_id.is_empty() && reviewer_id.eq_ignore_ascii_case(requester_id) {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "Governance requests require an independent reviewer",
                        "code": "GOVERNANCE_APPROVAL_SELF_REVIEW",
                    })),
                ));
            }
        }
    }
    Ok(())
}

pub(super) async fn governance_approval_approve(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(input): Json<GovernanceApprovalDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let reviewer = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    ensure_governance_review_authorized(&state, &approval_id, &reviewer, &tenant_context).await?;
    if !super::workflows::workflow_reviewer_is_eligible(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Governance review requires an eligible tenant reviewer",
            "GOVERNANCE_APPROVAL_REVIEWER_FORBIDDEN",
        ));
    }
    let notes = input.notes.clone();
    let Some(reviewed) = state
        .decide_approval_request(&approval_id, reviewer, true, notes.clone(), &tenant_context)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "GOVERNANCE_APPROVAL_DECISION_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Approval request not found",
                "code": "GOVERNANCE_APPROVAL_NOT_FOUND",
                "approvalID": approval_id,
            })),
        ));
    };
    if reviewed.status == GovernanceApprovalStatus::Approved
        && reviewed.request_type == GovernanceApprovalRequestType::LifecycleReview
    {
        let trigger = reviewed
            .context
            .get("trigger")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reviewer_actor =
            resolve_governance_actor(&headers, &tenant_context, &request_principal);
        match reviewed.target_resource.resource_type.as_str() {
            "agent" if trigger == "creation_quota" => {
                let _ = state
                    .acknowledge_agent_creation_review(
                        &reviewed.target_resource.id,
                        reviewer_actor,
                        notes.clone(),
                    )
                    .await;
            }
            "automation" if trigger == "run_drift" || trigger == "health_drift" => {
                let _ = state
                    .acknowledge_automation_review(
                        &reviewed.target_resource.id,
                        reviewer_actor,
                        notes.clone(),
                    )
                    .await;
            }
            "automation" if trigger == "dependency_revoked" => {
                let _ = state
                    .acknowledge_automation_review(
                        &reviewed.target_resource.id,
                        reviewer_actor,
                        notes.clone(),
                    )
                    .await;
            }
            _ => {}
        }
    }
    Ok(Json(json!({
        "approval": approval_request_wire(&reviewed),
    })))
}

pub(super) async fn governance_approval_deny(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(input): Json<GovernanceApprovalDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let reviewer = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    ensure_governance_review_authorized(&state, &approval_id, &reviewer, &tenant_context).await?;
    if !super::workflows::workflow_reviewer_is_eligible(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Governance review requires an eligible tenant reviewer",
            "GOVERNANCE_APPROVAL_REVIEWER_FORBIDDEN",
        ));
    }
    let Some(reviewed) = state
        .decide_approval_request(&approval_id, reviewer, false, input.notes, &tenant_context)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "GOVERNANCE_APPROVAL_DECISION_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Approval request not found",
                "code": "GOVERNANCE_APPROVAL_NOT_FOUND",
                "approvalID": approval_id,
            })),
        ));
    };
    Ok(Json(json!({
        "approval": approval_request_wire(&reviewed),
    })))
}

pub(super) async fn automation_governance_get(
    State(state): State<AppState>,
    Extension(caller_tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let automation = require_active_automation_owner(&state, &id, &caller_tenant).await?;
    let Some(record) = state
        .get_automation_governance_for_tenant(&id, &caller_tenant)
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Automation governance not found",
                "code": "AUTOMATION_GOVERNANCE_NOT_FOUND",
                "automationID": id,
            })),
        ));
    };
    let spend_agent_ids = record.agent_lineage_ids();
    let tenant_context = automation.tenant_context();
    let mut spend = Vec::new();
    for agent_id in spend_agent_ids {
        if let Some(summary) = state
            .tenant_agent_spend_summary(&tenant_context, &agent_id)
            .await
        {
            spend.push(agent_spend_wire(&summary));
        }
    }
    let agent_review = if super::tenant_is_standalone_local(&caller_tenant)
        && record.provenance.creator.kind == GovernanceActorKind::Agent
    {
        if let Some(agent_id) = record.provenance.creator.actor_id.as_deref() {
            state
                .agent_creation_review_summary(agent_id)
                .await
                .map(|summary| agent_creation_review_wire(&summary))
        } else {
            None
        }
    } else {
        None
    };
    let limits = state.automation_governance.read().await.limits.clone();
    Ok(Json(json!({
        "governance": automation_governance_wire(&record),
        "agent_review": agent_review,
        "lifecycle": automation_lifecycle_summary_wire(&record),
        "spend": {
            "weekly_spend_cap_usd": limits.weekly_spend_cap_usd,
            "warning_threshold_ratio": limits.spend_warning_threshold_ratio,
            "agents": spend,
        },
    })))
}

pub(super) async fn governance_reviews_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let agent_reviews = if super::tenant_is_standalone_local(&tenant_context) {
        state
            .list_agent_creation_review_summaries()
            .await
            .into_iter()
            .filter(|summary| summary.review_required)
            .map(|summary| agent_creation_review_wire(&summary))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let automations = state.list_automations_v2().await;
    let mut lifecycle_reviews = Vec::new();
    for automation in automations {
        if !super::tenant_matches(&tenant_context, &automation.tenant_context()) {
            continue;
        }
        if let Some(record) = state
            .get_automation_governance_for_tenant(&automation.automation_id, &tenant_context)
            .await
        {
            let review_required = record.review_required
                || !record.health_findings.is_empty()
                || record.expired_at_ms.is_some()
                || record.retired_at_ms.is_some();
            if review_required {
                lifecycle_reviews.push(json!({
                    "automation_id": record.automation_id,
                    "creator_id": record.provenance.creator.actor_id.clone(),
                    "review": automation_lifecycle_summary_wire(&record),
                }));
            }
        }
    }
    lifecycle_reviews.sort_by(|a, b| {
        b.get("review")
            .and_then(|value| value.get("review_requested_at_ms"))
            .and_then(Value::as_u64)
            .cmp(
                &a.get("review")
                    .and_then(|value| value.get("review_requested_at_ms"))
                    .and_then(Value::as_u64),
            )
    });

    let pending_approvals = state
        .list_approval_requests_for_tenant(
            None,
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
            &tenant_context,
        )
        .await
        .into_iter()
        .filter(|request| {
            matches!(
                request.request_type,
                GovernanceApprovalRequestType::LifecycleReview
                    | GovernanceApprovalRequestType::RetirementAction
            )
        })
        .map(|request| approval_request_wire(&request))
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "agent_creation_reviews": agent_reviews,
        "automation_lifecycle_reviews": lifecycle_reviews,
        "pending_approvals": pending_approvals,
        "count": agent_reviews.len() + lifecycle_reviews.len() + pending_approvals.len(),
    })))
}

pub(super) async fn governance_spend_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let mut rows = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for automation in state.list_automations_v2().await {
        if !super::tenant_matches(&tenant_context, &automation.tenant_context()) {
            continue;
        }
        let Some(record) = state
            .get_automation_governance_for_tenant(&automation.automation_id, &tenant_context)
            .await
        else {
            continue;
        };
        for agent_id in record.agent_lineage_ids() {
            if seen.insert(agent_id.clone()) {
                if let Some(summary) = state
                    .tenant_agent_spend_summary(&tenant_context, &agent_id)
                    .await
                {
                    rows.push(summary);
                }
            }
        }
    }
    Ok(Json(json!({
        "spend": rows.iter().map(agent_spend_wire).collect::<Vec<_>>(),
        "count": rows.len(),
    })))
}

pub(super) async fn governance_spend_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(agent_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let Some(summary) = state
        .tenant_agent_spend_summary(&tenant_context, &agent_id)
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Agent spend record not found",
                "code": "AGENT_SPEND_NOT_FOUND",
                "agentID": agent_id,
            })),
        ));
    };
    Ok(Json(json!({
        "spend": agent_spend_wire(&summary),
    })))
}

pub(super) async fn automation_grants_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    require_active_automation_owner(&state, &id, &tenant_context).await?;
    let Some(record) = state
        .get_automation_governance_for_tenant(&id, &tenant_context)
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Automation governance not found",
                "code": "AUTOMATION_GOVERNANCE_NOT_FOUND",
                "automationID": id,
            })),
        ));
    };
    Ok(Json(json!({
        "automationID": id,
        "modify_grants": record.modify_grants.iter().map(automation_grant_wire).collect::<Vec<_>>(),
        "capability_grants": record.capability_grants.iter().map(automation_grant_wire).collect::<Vec<_>>(),
        "count": record.modify_grants.len() + record.capability_grants.len(),
    })))
}

pub(super) async fn automation_grant_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationGrantCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let granted_by = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if granted_by.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can create modify grants",
                "code": "AUTOMATION_GOVERNANCE_GRANT_FORBIDDEN",
            })),
        ));
    }
    if !governance_mutation_admin_allowed(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Modify grants require tenant governance administration authority",
            "AUTOMATION_GOVERNANCE_GRANT_FORBIDDEN",
        ));
    }
    require_active_automation_owner(&state, &id, &tenant_context).await?;
    let mutation = state
        .can_mutate_automation(&id, &granted_by, false, &tenant_context)
        .await;
    enforce_mutation_or_audit(&state, &tenant_context, &id, &granted_by, mutation).await?;
    require_independent_mutation_approval(
        &state,
        &tenant_context,
        &granted_by,
        &id,
        input.approval_id.as_deref(),
        "grant_modify_access",
        &[
            GovernanceApprovalRequestType::CapabilityRequest,
            GovernanceApprovalRequestType::ElevatedCapability,
            GovernanceApprovalRequestType::LifecycleReview,
        ],
    )
    .await?;
    let grant = state
        .grant_automation_modify_access(
            &id,
            GovernanceActorRef::agent(
                Some(input.granted_to_agent_id.clone()),
                "automation_grant_create",
            ),
            granted_by,
            input.reason,
            &tenant_context,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_GRANT_CREATE_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({
        "grant": automation_grant_wire(&grant),
    })))
}

pub(super) async fn automation_grant_revoke(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, grant_id)): Path<(String, String)>,
    Json(input): Json<AutomationGrantRevokeInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let revoked_by = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if revoked_by.kind != GovernanceActorKind::Human {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Only humans can revoke modify grants",
            "AUTOMATION_GOVERNANCE_GRANT_FORBIDDEN",
        ));
    }
    if !governance_mutation_admin_allowed(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Modify grant revocation requires tenant governance administration authority",
            "AUTOMATION_GOVERNANCE_GRANT_FORBIDDEN",
        ));
    }
    require_active_automation_owner(&state, &id, &tenant_context).await?;
    let mutation = state
        .can_mutate_automation(&id, &revoked_by, true, &tenant_context)
        .await;
    enforce_mutation_or_audit(&state, &tenant_context, &id, &revoked_by, mutation).await?;
    require_independent_mutation_approval(
        &state,
        &tenant_context,
        &revoked_by,
        &id,
        input.approval_id.as_deref(),
        "revoke_modify_access",
        &[
            GovernanceApprovalRequestType::CapabilityRequest,
            GovernanceApprovalRequestType::ElevatedCapability,
            GovernanceApprovalRequestType::LifecycleReview,
        ],
    )
    .await?;
    let Some(grant) = state
        .revoke_automation_modify_access(
            &id,
            &grant_id,
            revoked_by.clone(),
            input.reason,
            &tenant_context,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_GRANT_REVOKE_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Grant not found",
                "code": "AUTOMATION_GOVERNANCE_GRANT_NOT_FOUND",
                "automationID": id,
                "grantID": grant_id,
            })),
        ));
    };
    let dependency_reason = grant
        .revoke_reason
        .clone()
        .unwrap_or_else(|| "modify grant revoked".to_string());
    state
        .pause_automation_for_dependency_revocation(
            &id,
            dependency_reason,
            json!({
                "trigger": "grant_revoked",
                "grantID": grant_id,
                "grant": automation_grant_wire(&grant),
                "revokedBy": revoked_by,
            }),
            &tenant_context,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_DEPENDENCY_PAUSE_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({
        "grant": automation_grant_wire(&grant),
    })))
}

pub(super) async fn automation_restore(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Only humans can restore deleted automations",
            "AUTOMATION_GOVERNANCE_RESTORE_FORBIDDEN",
        ));
    }
    if !governance_mutation_admin_allowed(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Automation restore requires tenant governance administration authority",
            "AUTOMATION_GOVERNANCE_RESTORE_FORBIDDEN",
        ));
    }
    let deleted = state
        .get_deleted_automation_v2(&id)
        .await
        .filter(|automation| super::tenant_matches(&tenant_context, &automation.tenant_context()))
        .ok_or_else(|| {
            governance_route_error(
                StatusCode::NOT_FOUND,
                "Deleted automation not found",
                "AUTOMATION_GOVERNANCE_RESTORE_NOT_FOUND",
            )
        })?;
    let mutation = state
        .can_mutate_automation(&id, &actor, true, &tenant_context)
        .await;
    enforce_mutation_or_audit(&state, &tenant_context, &id, &actor, mutation).await?;
    let approval_id = approval_id_from_headers(&headers);
    require_independent_mutation_approval(
        &state,
        &tenant_context,
        &actor,
        &id,
        approval_id,
        "restore_automation",
        &[
            GovernanceApprovalRequestType::RetirementAction,
            GovernanceApprovalRequestType::LifecycleReview,
        ],
    )
    .await?;
    let Some(restored) = state
        .restore_deleted_automation_v2(&id, actor, approval_id.map(str::to_string), &tenant_context)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_RESTORE_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Deleted automation not found",
                "code": "AUTOMATION_GOVERNANCE_RESTORE_NOT_FOUND",
                "automationID": id,
            })),
        ));
    };
    debug_assert_eq!(deleted.automation_id, restored.automation_id);
    Ok(Json(json!({
        "automation": restored,
    })))
}

pub(super) async fn automation_retire(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationRetireInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Only humans can retire automations",
            "AUTOMATION_GOVERNANCE_RETIRE_FORBIDDEN",
        ));
    }
    if !governance_mutation_admin_allowed(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Automation retirement requires tenant governance administration authority",
            "AUTOMATION_GOVERNANCE_RETIRE_FORBIDDEN",
        ));
    }
    require_active_automation_owner(&state, &id, &tenant_context).await?;
    let mutation = state
        .can_mutate_automation(&id, &actor, true, &tenant_context)
        .await;
    enforce_mutation_or_audit(&state, &tenant_context, &id, &actor, mutation).await?;
    require_independent_mutation_approval(
        &state,
        &tenant_context,
        &actor,
        &id,
        input.approval_id.as_deref(),
        "retire_automation",
        &[
            GovernanceApprovalRequestType::RetirementAction,
            GovernanceApprovalRequestType::LifecycleReview,
        ],
    )
    .await?;
    let Some(automation) = state
        .retire_automation_v2(&id, actor, input.reason, input.approval_id, &tenant_context)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_RETIRE_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Automation not found",
                "code": "AUTOMATION_GOVERNANCE_NOT_FOUND",
                "automationID": id,
            })),
        ));
    };
    Ok(Json(json!({
        "automation": automation,
    })))
}

pub(super) async fn automation_extend(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationExtendInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    premium_governance_required(&state)?;
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Only humans can extend automation retirement",
            "AUTOMATION_GOVERNANCE_EXTEND_FORBIDDEN",
        ));
    }
    if !governance_mutation_admin_allowed(&tenant_context, verified.as_deref()) {
        return Err(governance_route_error(
            StatusCode::FORBIDDEN,
            "Automation retirement extension requires tenant governance administration authority",
            "AUTOMATION_GOVERNANCE_EXTEND_FORBIDDEN",
        ));
    }
    require_active_automation_owner(&state, &id, &tenant_context).await?;
    let mutation = state
        .can_mutate_automation(&id, &actor, false, &tenant_context)
        .await;
    enforce_mutation_or_audit(&state, &tenant_context, &id, &actor, mutation).await?;
    require_independent_mutation_approval(
        &state,
        &tenant_context,
        &actor,
        &id,
        input.approval_id.as_deref(),
        "extend_automation_retirement",
        &[
            GovernanceApprovalRequestType::RetirementAction,
            GovernanceApprovalRequestType::LifecycleReview,
        ],
    )
    .await?;
    let Some(automation) = state
        .extend_automation_v2_retirement(
            &id,
            actor,
            input.expires_at_ms,
            input.reason,
            input.approval_id,
            &tenant_context,
        )
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_GOVERNANCE_EXTEND_FAILED",
                })),
            )
        })?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Automation not found",
                "code": "AUTOMATION_GOVERNANCE_NOT_FOUND",
                "automationID": id,
            })),
        ));
    };
    Ok(Json(json!({
        "automation": automation,
    })))
}
