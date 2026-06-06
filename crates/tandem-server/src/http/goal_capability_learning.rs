use super::*;
use tandem_types::{GoalSpec, TenantContext};

/// Tenant scope key derived from the *authenticated* context, never from the
/// caller's payload. Scopes by both org and workspace, matching how runtime
/// policy decisions are tenant-scoped elsewhere in the server.
fn tenant_scope_key(tenant_context: &TenantContext) -> String {
    format!("{}/{}", tenant_context.org_id, tenant_context.workspace_id)
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
    Json(input): Json<DiscoverGoalCapabilitiesInput>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = tenant_scope_key(&tenant_context);

    let response = state
        .discover_goal_capabilities(input.goal, tenant_id)
        .await;

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
/// Retrieve a discovery decision by ID, scoped to the authenticated tenant.
pub(super) async fn get_discovery_decision(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(decision_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = tenant_scope_key(&tenant_context);
    let decision = state
        .get_discovery_decision(&decision_id)
        .await
        // Fail closed: a decision owned by another tenant is treated as
        // not-found so its existence is not even revealed cross-tenant.
        .filter(|decision| decision.tenant_id == tenant_id)
        .ok_or(StatusCode::NOT_FOUND)?;

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
/// List discovery decisions for the authenticated tenant.
pub(super) async fn list_discovery_decisions(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = tenant_scope_key(&tenant_context);

    let decisions = state.list_discovery_decisions_for_tenant(&tenant_id).await;

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
