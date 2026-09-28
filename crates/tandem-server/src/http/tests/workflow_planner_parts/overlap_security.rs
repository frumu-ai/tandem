// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn assert_overlap_hidden_after_tenant_change(state: &AppState, app: axum::Router) {
    let mut prior = state
        .get_automation_v2("prior-overlap-automation")
        .await
        .expect("stored prior automation");
    prior.set_tenant_context(&tandem_types::TenantContext::explicit(
        "other-org",
        "other-workspace",
        Some("other-owner".to_string()),
    ));
    state
        .put_automation_v2(prior)
        .await
        .expect("move prior automation to another tenant");
    let response = app
        .oneshot(preview_request(json!({
            "prompt": "Compare two competitor summaries and generate a report",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("cross-tenant preview response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("cross-tenant preview body");
    let payload: Value = serde_json::from_slice(&body).expect("cross-tenant preview json");
    assert!(
        payload
            .get("overlap_analysis")
            .and_then(|row| row.get("matched_plan_id"))
            .is_none_or(Value::is_null),
        "another tenant's plan ID must not appear in overlap analysis"
    );
}
