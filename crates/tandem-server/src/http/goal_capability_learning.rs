// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_types::{GoalSpec, TenantContext};

/// Tenant scope key derived from the *authenticated* context, never from the
/// caller's payload. Scopes by both org and workspace, matching how runtime
/// policy decisions are tenant-scoped elsewhere in the server.
fn tenant_scope_key(tenant_context: &TenantContext) -> String {
    format!("{}/{}", tenant_context.org_id, tenant_context.workspace_id)
}

fn actor_id(tenant_context: &TenantContext) -> Option<&str> {
    tenant_context
        .actor_id
        .as_deref()
        .map(str::trim)
        .filter(|actor| !actor.is_empty())
}

fn decision_visible_to_actor(
    tenant_context: &TenantContext,
    decision: &crate::goal_capability_learning::DiscoveryDecision,
) -> bool {
    decision.tenant_id == tenant_scope_key(tenant_context)
        && (tenant_context.is_local_implicit()
            || matches!(
                (actor_id(tenant_context), decision.owner_actor_id.as_deref()),
                (Some(actor), Some(owner)) if actor == owner
            ))
}

/// Request to discover capabilities for a goal.
///
/// Note: there is intentionally no `tenant_id` field — the tenant is taken from
/// the authenticated `TenantContext`, not the request body, so a caller cannot
/// record (or later read) discovery under another tenant's id.
#[derive(Debug, Deserialize)]
pub(super) struct DiscoverGoalCapabilitiesInput {
    pub goal: GoalSpec,
}

/// POST /goal-capability-learning/discover
/// Discover capabilities for a goal and record the decision.
pub(super) async fn discover_goal_capabilities(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<DiscoverGoalCapabilitiesInput>,
) -> Result<Json<Value>, StatusCode> {
    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedUse,
    )?;
    let tenant_id = tenant_scope_key(&tenant_context);
    let owner_actor_id = actor_id(&tenant_context).map(ToString::to_string);
    if !tenant_context.is_local_implicit() && owner_actor_id.is_none() {
        return Err(StatusCode::FORBIDDEN);
    }

    let response = state
        .discover_goal_capabilities(input.goal, tenant_id, owner_actor_id, || {
            super::require_current_hosted_permission(
                &state,
                &tenant_context,
                verified.as_deref(),
                tandem_types::AccessPermission::HostedUse,
            )
            .is_ok()
        })
        .await
        .ok_or(StatusCode::FORBIDDEN)?;

    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedUse,
    )?;

    state.event_bus.publish(EngineEvent::new(
        "goal_capability_learning.discovered",
        json!({
            "request_id": response.request_id,
            "goal_id": response.report.goal_id,
            "confidence": response.report.overall_confidence_score,
            "paths_found": response.report.composition_candidates.len(),
        }),
    ));

    Ok(Json(json!(response)))
}

/// GET /goal-capability-learning/decisions/{decision_id}
/// Retrieve a discovery decision by ID, scoped to the authenticated actor.
pub(super) async fn get_discovery_decision(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(decision_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedAutomationRead,
    )?;
    let decision = state
        .get_discovery_decision(&decision_id)
        .await
        .filter(|decision| decision_visible_to_actor(&tenant_context, decision))
        .ok_or(StatusCode::NOT_FOUND)?;

    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedAutomationRead,
    )?;

    Ok(Json(json!({
        "decision_id": decision.decision_id,
        "goal_id": decision.goal.goal_id,
        "goal_title": decision.goal.title,
        "tenant_id": decision.tenant_id,
        "created_at_ms": decision.created_at_ms,
        "report": json!(decision.report),
    })))
}

/// GET /goal-capability-learning/decisions
/// List discovery decisions visible to the authenticated actor.
pub(super) async fn list_discovery_decisions(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = tenant_scope_key(&tenant_context);
    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedAutomationRead,
    )?;

    let decisions = state
        .list_discovery_decisions_for_tenant(&tenant_id)
        .await
        .into_iter()
        .filter(|decision| decision_visible_to_actor(&tenant_context, decision))
        .collect::<Vec<_>>();

    super::require_current_hosted_permission(
        &state,
        &tenant_context,
        verified.as_deref(),
        tandem_types::AccessPermission::HostedAutomationRead,
    )?;

    let summary: Vec<Value> = decisions
        .iter()
        .map(|d| {
            json!({
                "decision_id": d.decision_id,
                "goal_id": d.goal.goal_id,
                "goal_title": d.goal.title,
                "created_at_ms": d.created_at_ms,
                "confidence": d.report.overall_confidence_score,
                "paths_found": d.report.composition_candidates.len(),
            })
        })
        .collect();

    Ok(Json(json!({
        "tenant_id": tenant_id,
        "total": decisions.len(),
        "decisions": summary,
    })))
}
