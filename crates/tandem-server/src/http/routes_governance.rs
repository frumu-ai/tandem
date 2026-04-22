use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_types::{RequestPrincipal, TenantContext};
use uuid::Uuid;

use crate::automation_v2::governance::{
    AutomationGrantKind, GovernanceActorKind, GovernanceActorRef, GovernanceApprovalRequestType,
    GovernanceApprovalStatus, GovernanceResourceRef,
};

use super::governance::{
    agent_creation_review_wire, agent_spend_wire, approval_request_wire,
    automation_governance_wire, automation_grant_wire, automation_lifecycle_summary_wire,
    governance_error_response, resolve_governance_actor, resolve_governance_provenance,
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
    pub granted_to_agent_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationGrantRevokeInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationRetireInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationExtendInput {
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) fn apply(router: axum::Router<AppState>) -> axum::Router<AppState> {
    router
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

pub(super) async fn governance_approvals_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state.list_approval_requests(None, None).await;
    Json(json!({
        "approvals": rows.iter().map(approval_request_wire).collect::<Vec<_>>(),
        "count": rows.len(),
    }))
}

pub(super) async fn governance_approval_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<GovernanceApprovalCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let requested_by = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let request = state
        .request_approval(
            input.request_type,
            requested_by,
            input.target_resource,
            input.rationale,
            input.context,
            input.expires_at_ms,
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

pub(super) async fn governance_approval_approve(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(input): Json<GovernanceApprovalDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reviewer = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let notes = input.notes.clone();
    let Some(reviewed) = state
        .decide_approval_request(&approval_id, reviewer, true, notes.clone())
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
    headers: HeaderMap,
    Path(approval_id): Path<String>,
    Json(input): Json<GovernanceApprovalDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reviewer = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let Some(reviewed) = state
        .decide_approval_request(&approval_id, reviewer, false, input.notes)
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
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(record) = state.get_automation_governance(&id).await else {
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
    let mut spend = Vec::new();
    for agent_id in spend_agent_ids {
        if let Some(summary) = state.agent_spend_summary(&agent_id).await {
            spend.push(agent_spend_wire(&summary));
        }
    }
    let agent_review = if record.provenance.creator.kind == GovernanceActorKind::Agent {
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

pub(super) async fn governance_reviews_list(State(state): State<AppState>) -> Json<Value> {
    let agent_reviews = state
        .list_agent_creation_review_summaries()
        .await
        .into_iter()
        .filter(|summary| summary.review_required)
        .map(|summary| agent_creation_review_wire(&summary))
        .collect::<Vec<_>>();

    let automations = state.list_automations_v2().await;
    let mut lifecycle_reviews = Vec::new();
    for automation in automations {
        if let Some(record) = state
            .get_automation_governance(&automation.automation_id)
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
        .list_approval_requests(
            None,
            Some(crate::automation_v2::governance::GovernanceApprovalStatus::Pending),
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

    Json(json!({
        "agent_creation_reviews": agent_reviews,
        "automation_lifecycle_reviews": lifecycle_reviews,
        "pending_approvals": pending_approvals,
        "count": agent_reviews.len() + lifecycle_reviews.len() + pending_approvals.len(),
    }))
}

pub(super) async fn governance_spend_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state.list_agent_spend_summaries().await;
    Json(json!({
        "spend": rows.iter().map(agent_spend_wire).collect::<Vec<_>>(),
        "count": rows.len(),
    }))
}

pub(super) async fn governance_spend_get(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(summary) = state.agent_spend_summary(&agent_id).await else {
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
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(record) = state.get_automation_governance(&id).await else {
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
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationGrantCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
    let grant = state
        .grant_automation_modify_access(
            &id,
            GovernanceActorRef::agent(
                Some(input.granted_to_agent_id.clone()),
                "automation_grant_create",
            ),
            granted_by,
            input.reason,
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
    headers: HeaderMap,
    Path((id, grant_id)): Path<(String, String)>,
    Json(input): Json<AutomationGrantRevokeInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let revoked_by = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if revoked_by.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can revoke modify grants",
                "code": "AUTOMATION_GOVERNANCE_GRANT_FORBIDDEN",
            })),
        ));
    }
    let Some(grant) = state
        .revoke_automation_modify_access(&id, &grant_id, revoked_by.clone(), input.reason)
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can restore deleted automations",
                "code": "AUTOMATION_GOVERNANCE_RESTORE_FORBIDDEN",
            })),
        ));
    }
    let Some(restored) = state
        .restore_deleted_automation_v2(&id)
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
    Ok(Json(json!({
        "automation": restored,
    })))
}

pub(super) async fn automation_retire(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationRetireInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can retire automations",
                "code": "AUTOMATION_GOVERNANCE_RETIRE_FORBIDDEN",
            })),
        ));
    }
    let Some(automation) = state
        .retire_automation_v2(&id, actor, input.reason)
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
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationExtendInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = resolve_governance_actor(&headers, &tenant_context, &request_principal);
    if actor.kind != GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only humans can extend automation retirement",
                "code": "AUTOMATION_GOVERNANCE_EXTEND_FORBIDDEN",
            })),
        ));
    }
    let Some(automation) = state
        .extend_automation_v2_retirement(&id, actor, input.expires_at_ms, input.reason)
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
