use super::*;
use tandem_types::GoalSpec;

/// Request to discover capabilities for a goal.
#[derive(Debug, Deserialize)]
pub(super) struct DiscoverGoalCapabilitiesInput {
    pub goal: GoalSpec,
    #[serde(default)]
    pub tenant_id: Option<String>,
}

/// POST /goal-capability-learning/discover
/// Discover capabilities for a goal and record the decision.
pub(super) async fn discover_goal_capabilities(
    State(state): State<AppState>,
    Json(input): Json<DiscoverGoalCapabilitiesInput>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = input.tenant_id.unwrap_or_else(|| "default".to_string());

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

/// GET /goal-capability-learning/decisions/:decision_id
/// Retrieve a discovery decision by ID.
pub(super) async fn get_discovery_decision(
    State(state): State<AppState>,
    Path(decision_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let decision = state
        .get_discovery_decision(&decision_id)
        .await
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
/// List discovery decisions for a tenant.
#[derive(Debug, Deserialize)]
pub(super) struct ListDecisionsQuery {
    #[serde(default)]
    pub tenant_id: Option<String>,
}

pub(super) async fn list_discovery_decisions(
    State(state): State<AppState>,
    Query(params): Query<ListDecisionsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let tenant_id = params.tenant_id.unwrap_or_else(|| "default".to_string());

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
