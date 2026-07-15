// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-approval-preview-{}", uuid::Uuid::new_v4()));
    let artifact_dir = workspace_root
        .join(".tandem")
        .join("runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    std::fs::write(
        artifact_dir.join("draft.json"),
        serde_json::json!({
            "schema_version": "1",
            "has_rows_to_write": true,
            "ready_to_write": [{
                "Company": "SponsorCo",
                "Contact name": "Ada Lovelace",
                "Role / Title": "Partnerships Lead",
                "Email": "ada@sponsor.example",
                "Status": "Verified"
            }]
        })
        .to_string(),
    )
    .expect("write preview artifact");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            if let Some(snapshot) = row.automation_snapshot.as_mut() {
                snapshot.workspace_root = Some(workspace_root.to_string_lossy().to_string());
            }
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "publish".to_string(),
                title: "Publish approval".to_string(),
                instructions: Some("approve final publish step".to_string()),
                decisions: vec!["approve".to_string(), "reject".to_string()],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["draft".to_string()],
                metadata: None,
                expiry_policy: None,
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

    let first = approvals
        .iter()
        .find(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("created approval should be listed");
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
    assert_eq!(
        first.get("instructions").and_then(Value::as_str),
        Some("approve final publish step")
    );
    let preview = first
        .get("action_preview_markdown")
        .and_then(Value::as_str)
        .expect("approval preview markdown");
    assert!(
        preview.contains("Proposed contact rows: **1**"),
        "preview should summarize proposed writes: {preview}"
    );
    assert!(
        preview.contains("Ada Lovelace"),
        "preview should include proposed row details: {preview}"
    );
    assert_ne!(preview, "approve final publish step");
    let request_id = first
        .get("request_id")
        .and_then(Value::as_str)
        .expect("request_id");
    assert!(
        request_id.starts_with("automation_v2:"),
        "request_id should be namespaced: {request_id}",
    );
    let expected_wait_id = format!("{request_id}:wait");
    let approval_wait = first.get("approval_wait").expect("approval_wait");
    assert_eq!(
        approval_wait
            .get("approval_request_id")
            .and_then(Value::as_str),
        Some(request_id)
    );
    assert_eq!(
        approval_wait.get("wait_id").and_then(Value::as_str),
        Some(expected_wait_id.as_str())
    );
    let decisions = first
        .get("decisions")
        .and_then(Value::as_array)
        .expect("decisions array");
    assert_eq!(decisions.len(), 3);
    let decision_values = decisions
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(decision_values, vec!["approve", "cancel", "rework"]);

    let surface = first
        .get("surface_payload")
        .expect("surface_payload object");
    assert_eq!(
        surface.get("decide_endpoint").and_then(Value::as_str),
        Some(format!("/automations/v2/runs/{}/gate", run.run_id).as_str())
    );
    assert_eq!(
        surface.get("wait_id").and_then(Value::as_str),
        Some(expected_wait_id.as_str())
    );

    let count = payload.get("count").and_then(Value::as_u64).unwrap_or(0);
    assert!(count >= 1);
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn approvals_pending_endpoint_surfaces_zero_contact_company_status_work() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-zero-contact-approval").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-zero-contact-approval-{}",
        uuid::Uuid::new_v4()
    ));
    let artifact_dir = workspace_root
        .join(".tandem")
        .join("runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    std::fs::write(
        artifact_dir.join("discover-contact-candidates.json"),
        serde_json::json!({
            "schema_version": "1",
            "has_candidates": false,
            "candidates_by_company": [
                {
                    "company": "Pirkka-cola (Kesko)",
                    "domain": "k-ryhma.fi",
                    "domain_resolution_status": "resolved",
                    "hunter_checked": true,
                    "candidate_count": 0,
                    "candidates": []
                },
                {
                    "company": "Coinmotion",
                    "domain": null,
                    "domain_resolution_status": "not_found",
                    "hunter_checked": false,
                    "candidate_count": 0,
                    "candidates": []
                }
            ]
        })
        .to_string(),
    )
    .expect("write discovery artifact");
    std::fs::write(
        artifact_dir.join("enrich-and-verify-contacts.json"),
        serde_json::json!({
            "schema_version": "1",
            "has_rows_to_write": false,
            "ready_to_write": [],
            "duplicates_or_skipped": []
        })
        .to_string(),
    )
    .expect("write enrichment artifact");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            if let Some(snapshot) = row.automation_snapshot.as_mut() {
                snapshot.workspace_root = Some(workspace_root.to_string_lossy().to_string());
            }
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "write-contacts-to-notion".to_string(),
                title: "Write contacts to Notion".to_string(),
                instructions: Some("approve company status writes".to_string()),
                decisions: vec!["approve".to_string(), "cancel".to_string()],
                rework_targets: vec!["enrich-and-verify-contacts".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec![
                    "discover-contact-candidates".to_string(),
                    "enrich-and-verify-contacts".to_string(),
                ],
                metadata: None,
                expiry_policy: None,
            });
        })
        .await
        .expect("updated run");

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
    let approval = approvals
        .iter()
        .find(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("created approval should be listed");
    let preview = approval
        .get("action_preview_markdown")
        .and_then(Value::as_str)
        .expect("approval preview markdown");
    assert!(
        preview.contains("Proposed contact rows: **0**"),
        "preview should show zero contact rows: {preview}"
    );
    assert!(
        preview.contains("Company Research Status updates are still expected"),
        "preview should say company status writes still happen: {preview}"
    );
    assert!(
        preview.contains("Pirkka-cola (Kesko) -> no_hunter_results"),
        "preview should include terminal zero-contact status: {preview}"
    );
    assert!(
        preview.contains("Coinmotion -> no_domain"),
        "preview should include no-domain status: {preview}"
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn approvals_pending_endpoint_reads_sharded_automation_v2_runs() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-approvals-sharded-gate").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.detail = Some("awaiting approval for gate `approval`".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.awaiting_gate = None;
        })
        .await
        .expect("updated run");

    state.automation_v2_runs.write().await.remove(&run.run_id);

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
    let recovered = approvals
        .iter()
        .find(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("sharded approval should be listed");
    assert_eq!(
        recovered.get("node_id").and_then(Value::as_str),
        Some("approval")
    );
}

#[tokio::test]
async fn approvals_pending_endpoint_uses_full_run_when_hot_row_is_stale() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-approvals-stale-hot-row").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.detail = Some("awaiting approval for gate `approval`".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.node_outputs.insert(
                "review".to_string(),
                json!({ "status": "completed", "summary": "ready for approval" }),
            );
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "approval".to_string(),
                title: "Approval".to_string(),
                instructions: Some("Check the review output".to_string()),
                decisions: vec![
                    "approve".to_string(),
                    "rework".to_string(),
                    "cancel".to_string(),
                ],
                rework_targets: vec!["draft".to_string()],
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: vec!["review".to_string()],
                metadata: None,
                expiry_policy: None,
            });
        })
        .await
        .expect("updated run");

    {
        let mut guard = state.automation_v2_runs.write().await;
        let hot = guard.get_mut(&run.run_id).expect("hot run row");
        hot.status = crate::AutomationRunStatus::Queued;
        hot.detail = Some("stale list row".to_string());
        hot.checkpoint.awaiting_gate = None;
        hot.updated_at_ms = hot.updated_at_ms.saturating_add(60_000);
    }

    let hydrated = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("hydrated run");
    assert_eq!(
        hydrated.status,
        crate::AutomationRunStatus::AwaitingApproval
    );
    assert_eq!(
        hydrated
            .checkpoint
            .awaiting_gate
            .as_ref()
            .map(|gate| gate.node_id.as_str()),
        Some("approval")
    );

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
    let approval = approvals
        .iter()
        .find(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("approval should use full run state instead of stale hot row");
    assert_eq!(
        approval.get("source").and_then(Value::as_str),
        Some("automation_v2")
    );
    assert_eq!(
        approval.get("node_id").and_then(Value::as_str),
        Some("approval")
    );
}

#[tokio::test]
async fn approvals_pending_endpoint_recovers_automation_v2_gate_from_pending_node() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-approvals-recovered-gate").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.detail = Some("awaiting approval for gate `approval`".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.awaiting_gate = None;
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
    let recovered = approvals
        .iter()
        .find(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
        })
        .expect("recovered approval should be listed");
    assert_eq!(
        recovered.get("node_id").and_then(Value::as_str),
        Some("approval")
    );
    assert_eq!(
        recovered.get("instructions").and_then(Value::as_str),
        Some("Check the review output")
    );
}

#[tokio::test]
async fn gate_decide_recovers_missing_awaiting_gate_from_pending_node() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-gate-decide-recovered").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.detail = Some("awaiting approval for gate `approval`".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.awaiting_gate = None;
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
    assert_eq!(resp.status(), 200);

    let updated = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("updated run");
    assert!(updated.checkpoint.awaiting_gate.is_none());
    let record = updated
        .checkpoint
        .gate_history
        .last()
        .expect("gate decision record");
    let expected_request_id = format!("automation_v2:{}:approval", run.run_id);
    assert_eq!(
        record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("approval_wait"))
            .and_then(|wait| wait.get("approval_request_id"))
            .and_then(Value::as_str),
        Some(expected_request_id.as_str())
    );
    assert!(updated
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node| node == "approval"));
}

#[tokio::test]
async fn recovered_pending_gate_ignores_guard_denial_history() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-guard-denied-recovered").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let expected_request_id = format!("automation_v2:{}:approval", run.run_id);
    let expected_transition_id = format!("{expected_request_id}:decision");

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.detail = Some("awaiting approval for gate `approval`".to_string());
            row.checkpoint.completed_nodes = vec!["draft".to_string(), "review".to_string()];
            row.checkpoint.pending_nodes = vec!["approval".to_string()];
            row.checkpoint.awaiting_gate = None;
            row.checkpoint
                .gate_history
                .push(crate::AutomationGateDecisionRecord {
                    node_id: "approval".to_string(),
                    decision: "guard_denied".to_string(),
                    reason: Some("stale approval request".to_string()),
                    decided_at_ms: crate::now_ms(),
                    decided_by: None,
                    metadata: Some(json!({
                        "transition_guard_denial": {
                            "expected_approval_request_id": expected_request_id.clone(),
                            "expected_transition_id": expected_transition_id.clone(),
                        }
                    })),
                });
        })
        .await
        .expect("updated run");

    let pending_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending")
                .body(Body::empty())
                .expect("pending request"),
        )
        .await
        .expect("pending response");
    assert_eq!(pending_resp.status(), 200);
    let pending_body = to_bytes(pending_resp.into_body(), 1_000_000)
        .await
        .expect("pending body");
    let pending_payload: Value = serde_json::from_slice(&pending_body).expect("pending json");
    let approvals = pending_payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(
        approvals.iter().any(|approval| {
            approval.get("run_id").and_then(Value::as_str) == Some(run.run_id.as_str())
                && approval.get("node_id").and_then(Value::as_str) == Some("approval")
        }),
        "guard_denied must not hide recovered pending approval"
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/gate", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "decision": "approve",
                        "approval_request_id": expected_request_id,
                        "transition_id": expected_transition_id,
                    })
                    .to_string(),
                ))
                .expect("decision request"),
        )
        .await
        .expect("decision response");
    assert_eq!(resp.status(), 200);

    let updated = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("updated run");
    assert_eq!(updated.status, crate::AutomationRunStatus::Queued);
    assert_eq!(
        updated
            .checkpoint
            .gate_history
            .iter()
            .map(|record| record.decision.as_str())
            .collect::<Vec<_>>(),
        vec!["guard_denied", "approve"]
    );
}

#[tokio::test]
async fn approvals_pending_endpoint_scopes_results_to_request_tenant() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-approvals-a",
        "workspace-a",
        None,
        "actor-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-approvals-b",
        "workspace-b",
        None,
        "actor-b",
    );

    let mut automation_a = create_test_automation_v2(&state, "auto-v2-approvals-tenant-a").await;
    automation_a.set_tenant_context(&tenant_a);
    state
        .put_automation_v2(automation_a.clone())
        .await
        .expect("store tenant a automation");
    let run_a = state
        .create_automation_v2_run(&automation_a, "manual")
        .await
        .expect("tenant a run");

    let mut automation_b = create_test_automation_v2(&state, "auto-v2-approvals-tenant-b").await;
    automation_b.set_tenant_context(&tenant_b);
    state
        .put_automation_v2(automation_b.clone())
        .await
        .expect("store tenant b automation");
    let run_b = state
        .create_automation_v2_run(&automation_b, "manual")
        .await
        .expect("tenant b run");

    for (run_id, node_id) in [(&run_a.run_id, "approval-a"), (&run_b.run_id, "approval-b")] {
        state
            .update_automation_v2_run(run_id, |row| {
                row.status = crate::AutomationRunStatus::AwaitingApproval;
                row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                    node_id: node_id.to_string(),
                    title: format!("Approval {node_id}"),
                    instructions: None,
                    decisions: vec!["approve".to_string()],
                    rework_targets: vec![],
                    requested_at_ms: crate::now_ms(),
                    upstream_node_ids: vec![],
                    metadata: None,
                    expiry_policy: None,
                });
            })
            .await
            .expect("mark run awaiting approval");
    }

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending")
                .header("x-tandem-org-id", tenant_b.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_b.workspace_id.as_str())
                .header("x-tandem-actor-id", "actor-b")
                .body(Body::empty())
                .expect("tenant b request"),
        )
        .await
        .expect("tenant b response");
    assert_eq!(resp.status(), 200);
    let body = to_bytes(resp.into_body(), 1_000_000)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    let approvals = payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(
        approvals
            .iter()
            .any(|approval| approval.get("run_id").and_then(Value::as_str)
                == Some(run_b.run_id.as_str())),
        "tenant B should see its own approval"
    );
    assert!(
        approvals
            .iter()
            .all(|approval| approval.get("run_id").and_then(Value::as_str)
                != Some(run_a.run_id.as_str())),
        "tenant B must not see tenant A approvals: {payload}"
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending?org_id=org-approvals-a&workspace_id=workspace-a")
                .header("x-tandem-org-id", tenant_b.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_b.workspace_id.as_str())
                .header("x-tandem-actor-id", "actor-b")
                .body(Body::empty())
                .expect("tenant b narrowed request"),
        )
        .await
        .expect("tenant b narrowed response");
    assert_eq!(resp.status(), 200);
    let body = to_bytes(resp.into_body(), 1_000_000)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn approvals_pending_endpoint_applies_tenant_scope_before_run_cap() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-approvals-cap-a",
        "workspace-a",
        None,
        "actor-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-approvals-cap-b",
        "workspace-b",
        None,
        "actor-b",
    );

    let mut automation_b = create_test_automation_v2(&state, "auto-v2-approvals-cap-b").await;
    automation_b.set_tenant_context(&tenant_b);
    state
        .put_automation_v2(automation_b.clone())
        .await
        .expect("store tenant b automation");
    let run_b = state
        .create_automation_v2_run(&automation_b, "manual")
        .await
        .expect("tenant b run");
    {
        let mut runs = state.automation_v2_runs.write().await;
        let row = runs.get_mut(&run_b.run_id).expect("tenant b row");
        row.created_at_ms = 1;
        row.updated_at_ms = 1;
        row.status = crate::AutomationRunStatus::AwaitingApproval;
        row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
            node_id: "approval-b".to_string(),
            title: "Tenant B approval".to_string(),
            instructions: None,
            decisions: vec!["approve".to_string()],
            rework_targets: vec![],
            requested_at_ms: 1,
            upstream_node_ids: vec![],
            metadata: None,
            expiry_policy: None,
        });
    }

    let mut automation_a = create_test_automation_v2(&state, "auto-v2-approvals-cap-a").await;
    automation_a.set_tenant_context(&tenant_a);
    state
        .put_automation_v2(automation_a.clone())
        .await
        .expect("store tenant a automation");
    let run_a_template = state
        .create_automation_v2_run(&automation_a, "manual")
        .await
        .expect("tenant a run template");
    {
        let mut runs = state.automation_v2_runs.write().await;
        let template = runs
            .remove(&run_a_template.run_id)
            .expect("tenant a template row");
        for index in 0..500_u64 {
            let mut row = template.clone();
            row.run_id = format!("tenant-a-newer-run-{index}");
            row.created_at_ms = 10_000 + index;
            row.updated_at_ms = 10_000 + index;
            runs.insert(row.run_id.clone(), row);
        }
    }

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/approvals/pending")
                .header("x-tandem-org-id", tenant_b.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_b.workspace_id.as_str())
                .header("x-tandem-actor-id", "actor-b")
                .body(Body::empty())
                .expect("tenant b request"),
        )
        .await
        .expect("tenant b response");
    assert_eq!(resp.status(), 200);
    let body = to_bytes(resp.into_body(), 1_000_000)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    let approvals = payload
        .get("approvals")
        .and_then(Value::as_array)
        .expect("approvals array");
    assert!(
        approvals
            .iter()
            .any(|approval| approval.get("run_id").and_then(Value::as_str)
                == Some(run_b.run_id.as_str())),
        "tenant B approval must not be hidden behind another tenant's newest runs: {payload}"
    );
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
                    decided_by: None,
                    metadata: None,
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
                metadata: None,
                expiry_policy: None,
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
                metadata: None,
                expiry_policy: None,
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
