async fn stored_memory_record(
    state: &AppState,
    id: &str,
) -> tandem_memory::types::GlobalMemoryRecord {
    tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory db")
        .get_global_memory_for_tenant(id, "local", "local", None)
        .await
        .expect("read memory")
        .expect("stored memory")
}

async fn make_memory_audit_unwritable(state: &AppState) {
    tokio::fs::remove_file(&state.memory_audit_path)
        .await
        .expect("remove memory audit file");
    tokio::fs::create_dir_all(&state.memory_audit_path)
        .await
        .expect("replace memory audit file with directory");
}

async fn put_atomicity_test_memory(
    app: &Router,
    run_id: &str,
    content: &str,
    capability: Option<&Value>,
) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory/put")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "run_id": run_id,
                        "partition": {
                            "org_id": "org-1",
                            "workspace_id": "ws-1",
                            "project_id": "proj-1",
                            "tier": "session"
                        },
                        "kind": "fact",
                        "content": content,
                        "classification": "internal",
                        "capability": capability
                    })
                    .to_string(),
                ))
                .expect("memory put request"),
        )
        .await
        .expect("memory put response");
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice::<Value>(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("memory put body"),
    )
    .expect("memory put json")
    .get("id")
    .and_then(Value::as_str)
    .expect("memory id")
    .to_string()
}

#[tokio::test]
async fn memory_delete_audit_failure_leaves_record_unchanged() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let memory_id = put_atomicity_test_memory(
        &app,
        "delete-audit-failure-run",
        "delete audit failure atomicity fact",
        None,
    )
    .await;
    let before = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory before delete");
    make_memory_audit_unwritable(&state).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/memory/{memory_id}"))
                .body(Body::empty())
                .expect("memory delete request"),
        )
        .await
        .expect("memory delete response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let after = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory after delete");
    assert_eq!(after, before);
}

#[tokio::test]
async fn memory_promote_audit_failure_leaves_record_unchanged() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let run_id = "promote-audit-failure-run";
    let capability = memory_capability(run_id, "user-1", "org-1", "ws-1", "proj-1");
    let memory_id = put_atomicity_test_memory(
        &app,
        run_id,
        "promotion audit failure atomicity fact",
        Some(&capability),
    )
    .await;
    let before = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory before promotion");
    make_memory_audit_unwritable(&state).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory/promote")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "run_id": run_id,
                        "source_memory_id": memory_id,
                        "from_tier": "session",
                        "to_tier": "project",
                        "partition": {
                            "org_id": "org-1",
                            "workspace_id": "ws-1",
                            "project_id": "proj-1",
                            "tier": "session"
                        },
                        "reason": "verify audit failure atomicity",
                        "review": {
                            "required": false,
                            "reviewer_id": "user-1",
                            "approval_id": "audit-failure-approval"
                        },
                        "source_outcome": {
                            "status": "approved",
                            "approved": true,
                            "source_run_id": run_id,
                            "approval_id": "audit-failure-approval"
                        },
                        "capability": capability
                    })
                    .to_string(),
                ))
                .expect("memory promote request"),
        )
        .await
        .expect("memory promote response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let after = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory after promotion");
    assert_eq!(after, before);
}

#[tokio::test]
async fn memory_demote_audit_failure_leaves_record_unchanged() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let memory_id = put_atomicity_test_memory(
        &app,
        "demote-audit-failure-run",
        "demotion audit failure atomicity fact",
        None,
    )
    .await;
    let before = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory before demotion");
    make_memory_audit_unwritable(&state).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory/demote")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "id": memory_id,
                        "run_id": "demote-audit-failure-run"
                    })
                    .to_string(),
                ))
                .expect("memory demote request"),
        )
        .await
        .expect("memory demote response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let after = serde_json::to_value(stored_memory_record(&state, &memory_id).await)
        .expect("serialize memory after demotion");
    assert_eq!(after, before);
}
