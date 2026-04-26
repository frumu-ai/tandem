use super::global::create_test_automation_v2;
use super::*;

use axum::body::{to_bytes, Body};
use axum::http::Request;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn approvals_pending_endpoint_surfaces_automation_v2_awaiting_gate() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-approvals-aggregator").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "publish".to_string(),
                title: "Publish approval".to_string(),
                instructions: Some("approve final publish step".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["draft".to_string()],
            });
        })
        .await
        .expect("updated run");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let body = to_bytes(resp.into_body(), 1_000_000)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");

    let approvals = payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(!approvals.is_empty(), "expected at least one approval");

    let first = &approvals[0];
    assert_eq!(
        first.get("source").and_then(Value::as_str),
        Some("automation_v2")
    );
    assert_eq!(
        first.get("run_id").and_then(Value::as_str),
        Some(run.run_id.as_str())
    );
    assert_eq!(
        first.get("node_id").and_then(Value::as_str),
        Some("publish")
    );
    let request_id = first
        .get("request_id")
        .and_then(Value::as_str)
        .expect("request_id");
    assert!(
        request_id.starts_with("automation_v2:"),
        "request_id should be namespaced: {request_id}",
    );
    let decisions = first
        .get("decisions")
        .and_then(Value::as_array)
        .expect("decisions array");
    assert_eq!(decisions.len(), 3);

    let surface = first
        .get("surface_payload")
        .expect("surface_payload object");
    assert_eq!(
        surface.get("decide_endpoint").and_then(Value::as_str),
        Some(format!("/automations/v2/runs/{}/gate_decide", run.run_id).as_str())
    );

    let count = payload.get("count").and_then(Value::as_u64).unwrap_or(0);
    assert!(count >= 1);
}

#[tokio::test]
async fn approvals_pending_endpoint_returns_empty_when_no_gates_pending() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 200);

    let body = to_bytes(resp.into_body(), 1_000_000)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    let approvals = payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(approvals.is_empty());
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn gate_decide_409_includes_winning_decision_in_body() {
    // Race UX (W2.6): when two surfaces try to decide the same gate
    // concurrently, the loser's 409 response should include the winner's
    // decision so the loser's UI can render "already decided by ..." instead
    // of a raw error.
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-race-ux").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    // Simulate the winner already having decided: append the gate_history
    // entry and move the run out of AwaitingApproval (this is the post-winner
    // state the loser observes).
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Queued;
            row.checkpoint.awaiting_gate = None;
            row.checkpoint
                .gate_history
                .push(crate::AutomationGateDecisionRecord {
                    node_id: "approval".to_string(),
                    decision: "approve".to_string(),
                    reason: Some("looks good".to_string()),
                    decided_at_ms: crate::now_ms(),
                });
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "decision": "approve" }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), 409);

    let body = to_bytes(resp.into_body(), 1_000_000).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL")
    );
    let winner = payload
        .get("winningDecision")
        .expect("winningDecision present in 409 body");
    assert_eq!(
        winner.get("decision").and_then(Value::as_str),
        Some("approve")
    );
    assert_eq!(
        winner.get("node_id").and_then(Value::as_str),
        Some("approval")
    );
    assert_eq!(
        winner.get("reason").and_then(Value::as_str),
        Some("looks good")
    );
    assert!(winner
        .get("decided_at_ms")
        .and_then(Value::as_u64)
        .is_some());
}

/// W5.5 — true concurrent race regression.
///
/// W2.6 added a single-threaded test that simulated the post-race state by
/// pre-mutating gate_history. This test fires two HTTP gate-decide requests
/// in parallel via tokio::spawn, against the *same* run with a real pending
/// gate, and asserts:
///
/// 1. Exactly one wins (200 OK).
/// 2. The other gets 409 with `winningDecision` populated from the winner's
///    `gate_history` entry.
///
/// Without this test, a regression that swapped per-run mutation
/// serialization for a non-atomic check-then-write would silently allow
/// double-decide and the audit trail would record one decision while the
/// runtime processed two. Mandatory before any rollout per the W5 plan.
#[tokio::test]
async fn gate_decide_concurrent_race_yields_exactly_one_winner() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-concurrent-race").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "approval".to_string(),
                title: "Concurrent test".to_string(),
                instructions: None,
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec![],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec![],
            });
        })
        .await
        .expect("updated run");

    // Fire both decisions in parallel against the same run. tokio::spawn
    // lets them race the per-run mutation lock.
    let app_a = app.clone();
    let app_b = app.clone();
    let run_id_a = run.run_id.clone();
    let run_id_b = run.run_id.clone();

    let task_a = tokio::spawn(async move {
        app_a
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/automations/v2/runs/{}/gate", run_id_a))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "decision": "approve",
                            "reason": "looks good"
                        })
                        .to_string(),
                    ))
                    .expect("request a"),
            )
            .await
            .expect("response a")
    });

    let task_b = tokio::spawn(async move {
        app_b
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/automations/v2/runs/{}/gate", run_id_b))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "decision": "cancel",
                            "reason": "scope drifted"
                        })
                        .to_string(),
                    ))
                    .expect("request b"),
            )
            .await
            .expect("response b")
    });

    let resp_a = task_a.await.expect("join a");
    let resp_b = task_b.await.expect("join b");

    let status_a = resp_a.status().as_u16();
    let status_b = resp_b.status().as_u16();
    let outcomes = [status_a, status_b];

    // Exactly one 200 + exactly one 409.
    assert!(
        outcomes.contains(&200) && outcomes.contains(&409),
        "concurrent decisions must produce one 200 and one 409, got {outcomes:?}"
    );

    // Identify which response was the loser and verify it carries
    // winningDecision.
    let loser_resp = if status_a == 409 { resp_a } else { resp_b };
    let body = to_bytes(loser_resp.into_body(), 1_000_000)
        .await
        .expect("loser body");
    let payload: Value = serde_json::from_slice(&body).expect("loser json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL")
    );
    let winner = payload
        .get("winningDecision")
        .expect("loser response must include winningDecision");
    let winning_decision = winner
        .get("decision")
        .and_then(Value::as_str)
        .expect("winningDecision.decision present");
    assert!(
        winning_decision == "approve" || winning_decision == "cancel",
        "winningDecision.decision should be one of the two contenders, got {winning_decision}"
    );
    assert_eq!(
        winner.get("node_id").and_then(Value::as_str),
        Some("approval")
    );
    assert!(winner
        .get("decided_at_ms")
        .and_then(Value::as_u64)
        .is_some());

    // Final run state has exactly one gate_history entry — the winner's.
    let final_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("final run");
    assert_eq!(
        final_run.checkpoint.gate_history.len(),
        1,
        "exactly one decision must have been recorded; concurrent calls must serialize"
    );
    assert!(final_run.checkpoint.awaiting_gate.is_none());
}

#[tokio::test]
async fn approvals_pending_endpoint_filters_by_source_unknown_returns_empty() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-approvals-source-filter").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "publish".to_string(),
                title: "Publish approval".to_string(),
                instructions: None,
                decisions: vec!["approve".to_string()],
                rework_targets: vec![],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec![],
            });
        })
        .await
        .expect("updated run");

    // Filter by `coder` — automation_v2 records should be excluded.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending?source=coder")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = to_bytes(resp.into_body(), 1_000_000).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let approvals = payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(approvals.is_empty());
}
