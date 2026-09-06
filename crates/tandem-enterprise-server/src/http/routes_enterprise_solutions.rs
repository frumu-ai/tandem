use axum::{
    extract::{Extension, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_enterprise_contract::VerifiedTenantContext;
use tandem_server::{
    solution_installation::{SolutionConfigurationRequest, SolutionStagingRequest},
    stateful_runtime::orchestration_store::CustomerConfigVersion,
    AppState,
};

use super::routes_enterprise::EnterpriseResult;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/enterprise/onboarding-plans/solutions/preview",
            post(preview),
        )
        .route(
            "/enterprise/onboarding-plans/solutions/configuration",
            post(save),
        )
        .route("/enterprise/onboarding-plans/solutions/stage", post(stage))
}

fn identity(
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<VerifiedTenantContext, (StatusCode, Json<Value>)> {
    verified.map(|Extension(context)| context).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": "VERIFIED_IDENTITY_REQUIRED"})),
        )
    })
}

fn rejected() -> (StatusCode, Json<Value>) {
    // Do not expose storage paths, source names or cross-tenant lookup errors.
    (
        StatusCode::CONFLICT,
        Json(json!({"code": "SOLUTION_REVIEW_REQUIRED",
        "message": "Current authorization, host prerequisites, configuration or native state could not be validated. Refresh the installation review."})),
    )
}

async fn preview(
    State(state): State<AppState>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<SolutionConfigurationRequest>,
) -> EnterpriseResult<Value> {
    let verified = identity(verified)?;
    let result = state
        .preview_solution_configuration(&verified, &request)
        .await
        .map_err(|_| rejected())?;
    Ok(Json(json!(result)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    request: SolutionConfigurationRequest,
    expected_version: Option<CustomerConfigVersion>,
}

async fn save(
    State(state): State<AppState>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<SaveRequest>,
) -> EnterpriseResult<Value> {
    let verified = identity(verified)?;
    let result = state
        .save_solution_configuration(&verified, request.request, request.expected_version)
        .await
        .map_err(|_| rejected())?;
    Ok(Json(json!(result)))
}

async fn stage(
    State(state): State<AppState>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(request): Json<SolutionStagingRequest>,
) -> EnterpriseResult<Value> {
    let verified = identity(verified)?;
    let result = state
        .stage_solution_installation(&verified, request)
        .await
        .map_err(|_| rejected())?;
    Ok(Json(
        json!({"installation": result, "activation_required": true, "solution_ready": false}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn solution_preview_requires_verified_identity_before_pack_lookup() {
        let state = tandem_server::test_support::test_state().await;
        let request = json!({"pack_selector": "private-pack", "configuration": {
            "schema_version": "1", "scope": {"org_id": "org-a", "workspace_id": "dep-a",
                "deployment_id": "dep-a", "instance_id": "brain"},
            "profile_ref": "profile-ref:org-a", "timezone": "UTC", "locale": "en",
            "constraints": {"allowed_providers": [], "allow_network_egress": false,
                "max_tokens_per_run": 1, "max_concurrent_runs": 1, "max_daily_cost_microusd": 0}
        }});
        let response = apply(Router::new())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/onboarding-plans/solutions/preview")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
