// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn automations_v2_run_recover_uses_completion_assertion_node_detail() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation =
        create_branched_test_automation_v2(&state, "auto-v2-completion-assertion").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Failed;
            row.detail = Some(
                "automation run missing successful external action receipt for outbound node `publish`"
                    .to_string(),
            );
            row.checkpoint.completed_nodes = vec![
                "research".to_string(),
                "analysis".to_string(),
                "draft".to_string(),
                "publish".to_string(),
            ];
            row.checkpoint.pending_nodes.clear();
            for node_id in ["research", "analysis", "draft", "publish"] {
                row.checkpoint.node_outputs.insert(
                    node_id.to_string(),
                    json!({"status":"completed","summary":node_id}),
                );
                row.checkpoint.node_attempts.insert(node_id.to_string(), 1);
            }
            row.checkpoint.last_failure = None;
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/recover", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "recover completion assertion failure" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let recovered = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after recover");
    assert_eq!(recovered.status, crate::AutomationRunStatus::Queued);
    for node_id in ["research", "analysis", "draft"] {
        assert!(recovered
            .checkpoint
            .completed_nodes
            .iter()
            .any(|completed| completed == node_id));
        assert!(recovered.checkpoint.node_outputs.contains_key(node_id));
    }
    assert!(!recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("publish"));
    assert!(!recovered.checkpoint.node_attempts.contains_key("publish"));
}

#[tokio::test]
async fn automations_v2_run_recover_resets_terminal_node_for_run_level_deliverable_assertion() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation =
        create_branched_test_automation_v2(&state, "auto-v2-run-level-deliverable").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Failed;
            row.detail = Some(
                "automation run deliverable `customer.incident_reported` lacks current-run output evidence"
                    .to_string(),
            );
            row.checkpoint.completed_nodes = vec![
                "research".to_string(),
                "analysis".to_string(),
                "draft".to_string(),
                "publish".to_string(),
            ];
            row.checkpoint.pending_nodes.clear();
            for node_id in ["research", "analysis", "draft", "publish"] {
                row.checkpoint.node_outputs.insert(
                    node_id.to_string(),
                    json!({"status":"completed","summary":node_id}),
                );
                row.checkpoint.node_attempts.insert(node_id.to_string(), 1);
            }
            row.checkpoint.last_failure = None;
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/recover", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "reconcile run-level deliverables" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let recovered = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after recover");
    assert_eq!(recovered.status, crate::AutomationRunStatus::Queued);
    for node_id in ["research", "analysis", "draft"] {
        assert!(recovered
            .checkpoint
            .completed_nodes
            .iter()
            .any(|completed| completed == node_id));
        assert!(recovered.checkpoint.node_outputs.contains_key(node_id));
        assert_eq!(recovered.checkpoint.node_attempts.get(node_id), Some(&1));
    }
    assert!(!recovered
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(recovered
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "publish"));
    assert!(!recovered.checkpoint.node_outputs.contains_key("publish"));
    assert!(!recovered.checkpoint.node_attempts.contains_key("publish"));
    assert!(recovered
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "run_recovered"));
}

#[tokio::test]
async fn global_workspace_is_available_through_authenticated_proxy_routes() {
    let state = test_state().await;
    let expected = state.workspace_index.snapshot().await.root;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/global/workspace")
        .extension(axum::extract::ConnectInfo(SocketAddr::from((
            [127, 0, 0, 1],
            43123,
        ))))
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("workspace_root").and_then(Value::as_str),
        Some(expected.as_str())
    );

    let proxied = Request::builder()
        .method("GET")
        .uri("/global/workspace")
        .header("x-forwarded-for", "198.51.100.9")
        .extension(axum::extract::ConnectInfo(SocketAddr::from((
            [127, 0, 0, 1],
            43123,
        ))))
        .body(Body::empty())
        .expect("request");
    let proxied_resp = app.oneshot(proxied).await.expect("response");
    assert_eq!(proxied_resp.status(), StatusCode::OK);
}
