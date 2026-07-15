// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use tandem_enterprise_server::apply_routes;
use tandem_server::build_router_with_extensions;
use tandem_server::test_support::test_state;

#[tokio::test]
async fn enterprise_ingestion_quarantine_review_updates_job_state() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    state.enterprise.ingestion_jobs.write().await.insert(
        "local::local::local::job-review".to_string(),
        tandem_enterprise_contract::IngestionJob {
            job_id: "job-review".to_string(),
            tenant_context: tenant.clone(),
            connector_id: "manual_upload".to_string(),
            binding_id: "review-binding".to_string(),
            state: tandem_enterprise_contract::IngestionJobState::Quarantined,
            source_object_ids: vec!["source-object-review".to_string()],
            started_at_ms: Some(1_000),
            finished_at_ms: Some(2_000),
            quarantine_id: Some("quarantine-review".to_string()),
        },
    );
    state.enterprise.ingestion_quarantines.write().await.insert(
        "local::local::local::quarantine-review".to_string(),
        tandem_enterprise_contract::IngestionQuarantine {
            quarantine_id: "quarantine-review".to_string(),
            tenant_context: tenant,
            connector_id: "manual_upload".to_string(),
            binding_id: "review-binding".to_string(),
            source_object_ids: vec!["source-object-review".to_string()],
            reason: "source binding requires ingestion review".to_string(),
            created_at_ms: 1_500,
            reviewed_by: None,
            reviewed_at_ms: None,
            disposition: None,
        },
    );
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/ingestion-quarantines/quarantine-review/review")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "local_control_panel")
        .body(Body::from(json!({"disposition": "delete"}).to_string()))
        .expect("review request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("quarantines")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("disposition"))
            .and_then(Value::as_str),
        Some("delete")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/ingestion-jobs?binding_id=review-binding")
        .header("x-tandem-request-source", "local_control_panel")
        .body(Body::empty())
        .expect("jobs request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("ingestion_jobs")
            .and_then(Value::as_array)
            .and_then(|jobs| jobs.first())
            .and_then(|job| job.get("state"))
            .and_then(Value::as_str),
        Some("skipped")
    );
}
