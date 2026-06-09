use super::*;
use crate::app::state::governance::UnavailableGovernanceEngine;

#[tokio::test]
async fn context_run_ledger_endpoint_returns_records_and_summary() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/context/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "ctx-run-ledger-1",
                "objective": "inspect ledger",
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    for payload in [
        json!({
            "type": "tool_effect_recorded",
            "status": "running",
            "payload": {
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": "read",
                    "phase": "invocation",
                    "status": "started",
                    "args_summary": {"type":"object","field_count":1,"keys":["path"]},
                }
            }
        }),
        json!({
            "type": "tool_effect_recorded",
            "status": "running",
            "payload": {
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": "write",
                    "phase": "outcome",
                    "status": "succeeded",
                    "args_summary": {"type":"object","field_count":1,"keys":["path"]},
                }
            }
        }),
    ] {
        let req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-ledger-1/events")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("event request");
        let resp = app.clone().oneshot(req).await.expect("event response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let ledger_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-ledger-1/ledger")
        .body(Body::empty())
        .expect("ledger request");
    let ledger_resp = app
        .clone()
        .oneshot(ledger_req)
        .await
        .expect("ledger response");
    assert_eq!(ledger_resp.status(), StatusCode::OK);
    let ledger_body = to_bytes(ledger_resp.into_body(), usize::MAX)
        .await
        .expect("ledger body");
    let ledger_payload: Value = serde_json::from_slice(&ledger_body).expect("ledger json");

    assert_eq!(
        ledger_payload
            .get("records")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        ledger_payload
            .get("summary")
            .and_then(|value| value.get("by_tool"))
            .and_then(|value| value.get("read"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        ledger_payload
            .get("summary")
            .and_then(|value| value.get("by_status"))
            .and_then(|value| value.get("succeeded"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[tokio::test]
async fn governance_evidence_export_requires_premium_governance() {
    let mut state = test_state().await;
    state.governance_engine = Arc::new(UnavailableGovernanceEngine);

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/context/runs/ctx-run-ledger-1/governance-evidence")
                .body(Body::empty())
                .expect("governance evidence request"),
        )
        .await
        .expect("governance evidence response");

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("governance evidence body");
    let payload: Value = serde_json::from_slice(&body).expect("governance evidence json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("PREMIUM_FEATURE_REQUIRED")
    );
}
