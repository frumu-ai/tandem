use super::*;

#[tokio::test]
async fn memory_import_validates_project_and_session_scope() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let import_root = state.memory_audit_path.parent().unwrap().join("docs");
    tokio::fs::create_dir_all(&import_root)
        .await
        .expect("import root");
    tokio::fs::write(import_root.join("note.md"), "memory import validation")
        .await
        .expect("import file");

    let project_req = Request::builder()
        .method("POST")
        .uri("/memory/import")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": {"kind": "path", "path": import_root.display().to_string()},
                "format": "directory",
                "tier": "project",
                "sync_deletes": false
            })
            .to_string(),
        ))
        .expect("project import request");
    let project_resp = app.clone().oneshot(project_req).await.expect("response");
    assert_eq!(project_resp.status(), StatusCode::BAD_REQUEST);

    let session_req = Request::builder()
        .method("POST")
        .uri("/memory/import")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": {"kind": "path", "path": import_root.display().to_string()},
                "format": "directory",
                "tier": "session",
                "sync_deletes": false
            })
            .to_string(),
        ))
        .expect("session import request");
    let session_resp = app.oneshot(session_req).await.expect("response");
    assert_eq!(session_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn memory_import_rejects_invalid_path_source() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/memory/import")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source": {"kind": "upload", "path": "/tmp/nope"},
                "format": "directory",
                "tier": "global",
                "sync_deletes": false
            })
            .to_string(),
        ))
        .expect("import request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn memory_put_enforces_default_write_scope() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-1",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "kind": "note",
                "content": "should fail without write scope",
                "classification": "internal"
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let blocked_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.put"),
    )
    .await
    .expect("blocked memory.put event");
    assert_eq!(
        blocked_event.properties.get("kind").and_then(Value::as_str),
        Some("note")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert!(blocked_event
        .properties
        .get("visibility")
        .is_some_and(Value::is_null));
    assert_eq!(
        blocked_event.properties.get("tier").and_then(Value::as_str),
        Some("project")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-1")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("write tier not allowed")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-1")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_put_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_put")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("write tier not allowed")
                                && detail.contains("origin_run_id=run-1")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("blocked memory_put audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_put_exists.get("audit_id").and_then(Value::as_str)
    );
    let persisted_audit = tokio::fs::read_to_string(&state.memory_audit_path)
        .await
        .expect("persisted audit file");
    let persisted_audit_id = blocked_put_exists
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("blocked audit id");
    let persisted_exists = persisted_audit.lines().any(|line| {
        serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|row| {
                row.get("audit_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .is_some_and(|audit_id| audit_id == persisted_audit_id)
    });
    assert!(
        persisted_exists,
        "blocked audit event should be written to disk"
    );
}

#[tokio::test]
async fn memory_put_then_search_in_session_scope() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();
    let artifact_refs = vec![
        Value::from("artifact://run-2/task-1/patch.diff"),
        Value::from("artifact://run-2/task-2/validation.json"),
    ];

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-2",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "solution_capsule",
                "content": "retry budget extension pattern",
                "classification": "internal",
                "artifact_refs": artifact_refs
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-2",
                "query": "budget extension",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "limit": 5
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app.clone().oneshot(search_req).await.expect("response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let result_count = payload
        .get("results")
        .and_then(|v| v.as_array())
        .map(|v| v.len())
        .unwrap_or(0);
    assert!(result_count >= 1);
    let first_result = payload
        .get("results")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        first_result.get("classification").and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        first_result.get("tier").and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(
        first_result.get("kind").and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        first_result.get("artifact_refs").and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        first_result
            .get("linkage")
            .and_then(|row| row.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-2")
    );
    assert_eq!(
        first_result
            .get("linkage")
            .and_then(|row| row.get("partition_key"))
            .and_then(Value::as_str),
        Some("org-1/ws-1/proj-1/session")
    );
    assert_eq!(
        first_result
            .get("linkage")
            .and_then(|row| row.get("artifact_refs"))
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        first_result
            .get("provenance")
            .and_then(|row| row.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-2")
    );
    assert_eq!(
        first_result
            .get("provenance")
            .and_then(|row| row.get("partition_key"))
            .and_then(Value::as_str),
        Some("org-1/ws-1/proj-1/session")
    );
    assert_eq!(
        first_result
            .get("provenance")
            .and_then(|row| row.get("artifact_refs"))
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    let search_event = next_event_of_type(&mut rx, "memory.search").await;
    assert_eq!(
        search_event.properties.get("query").and_then(Value::as_str),
        Some("budget extension")
    );
    assert_eq!(
        search_event
            .properties
            .get("resultIDs")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec![first_result
            .get("id")
            .and_then(Value::as_str)
            .expect("first result id")])
    );
    assert_eq!(
        search_event
            .properties
            .get("resultKinds")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["solution_capsule"])
    );
    assert_eq!(
        search_event
            .properties
            .get("requestedScopes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["session"])
    );
    assert_eq!(
        search_event
            .properties
            .get("scopesUsed")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["session"])
    );
    assert_eq!(
        search_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-2")
    );
    assert_eq!(
        search_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-2")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let search_audit_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_search")
                    && row.get("status").and_then(Value::as_str) == Some("ok")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("query=budget extension")
                                && detail.contains("result_count=")
                                && detail.contains("result_kinds=solution_capsule")
                                && detail.contains("requested_scopes=session")
                                && detail.contains("scopes_used=session")
                                && detail.contains("origin_run_id=run-2")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("successful memory_search audit row");
    assert_eq!(
        search_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        search_audit_exists.get("audit_id").and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_put_rejects_expired_capability_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-1-expired",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "note",
                "content": "expired capability should fail",
                "classification": "internal",
                "capability": {
                    "run_id": "run-1-expired",
                    "subject": "expired-user",
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "memory": {
                        "read_tiers": ["session"],
                        "write_tiers": ["session"],
                        "promote_targets": ["project"],
                        "require_review_for_promote": true,
                        "allow_auto_use_tiers": ["curated"]
                    },
                    "expires_at": 1
                }
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let blocked_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.put"),
    )
    .await
    .expect("blocked memory.put event");
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-1-expired")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("capability expired")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-1-expired")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_put_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_put")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("capability expired")
                                && detail.contains("origin_run_id=run-1-expired")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("expired memory_put audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_put_exists.get("audit_id").and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_put_rejects_mismatched_capability_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-1-cap-mismatch",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "note",
                "content": "mismatched capability should fail",
                "classification": "internal",
                "capability": {
                    "run_id": "run-1-cap-mismatch",
                    "subject": "mismatch-user",
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-2",
                    "memory": {
                        "read_tiers": ["session"],
                        "write_tiers": ["session"],
                        "promote_targets": ["project"],
                        "require_review_for_promote": true,
                        "allow_auto_use_tiers": ["curated"]
                    },
                    "expires_at": 9999999999999u64
                }
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let blocked_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.put"),
    )
    .await
    .expect("blocked memory.put event");
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-1-cap-mismatch")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("capability context mismatch")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-1-cap-mismatch")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_put_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_put")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("capability context mismatch")
                                && detail.contains("origin_run_id=run-1-cap-mismatch")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("mismatched memory_put audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_put_exists.get("audit_id").and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_search_preserves_restricted_classification() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-2b",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "note",
                "content": "restricted note without secrets",
                "classification": "restricted"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-2b",
                "query": "restricted note without secrets",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "limit": 5
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app.oneshot(search_req).await.expect("response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let first_result = payload
        .get("results")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        first_result.get("classification").and_then(Value::as_str),
        Some("restricted")
    );
    assert_eq!(
        first_result.get("kind").and_then(Value::as_str),
        Some("note")
    );
}

#[tokio::test]
async fn memory_promote_blocks_sensitive_content_and_emits_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let capability = json!({
        "run_id": "run-3",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": true,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "solution_capsule",
                "content": concat!("-----BEGIN", " PRIVATE KEY-----"),
                "classification": "restricted",
                "capability": capability
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("put body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("put json");
    let memory_id = put_payload
        .get("id")
        .and_then(|v| v.as_str())
        .expect("memory id")
        .to_string();

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3",
                "source_memory_id": memory_id,
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "promote test",
                "review": {
                    "required": true,
                    "reviewer_id": "user-1",
                    "approval_id": "appr-1"
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_body = to_bytes(promote_resp.into_body(), usize::MAX)
        .await
        .expect("promote body");
    let promote_payload: Value = serde_json::from_slice(&promote_body).expect("promote json");
    assert_eq!(
        promote_payload.get("promoted").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        promote_payload
            .get("scrub_report")
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("blocked")
    );
    let blocked_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        blocked_event
            .properties
            .get("sourceMemoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event.properties.get("kind").and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("restricted")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&Vec::<Value>::new())
    );
    assert_eq!(
        blocked_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("private")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("toTier")
            .and_then(Value::as_str),
        Some("project")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("scrubStatus")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-3")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("private key")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-3")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_promote_exists = audit_payload
        .get("events")
        .and_then(|v| v.as_array())
        .and_then(|events| {
            events.iter().find(|event| {
                event.get("action").and_then(|v| v.as_str()) == Some("memory_promote")
                    && event.get("status").and_then(|v| v.as_str()) == Some("blocked")
                    && event
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("private key")
                                && detail.contains("origin_run_id=run-3")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("scrub-blocked memory_promote audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_promote_exists
            .get("audit_id")
            .and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_promote_missing_source_emits_blocked_event_shape() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let capability = json!({
        "run_id": "run-3-missing",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": true,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-missing",
                "source_memory_id": "missing-memory-id",
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "missing source promote test",
                "review": {
                    "required": true,
                    "reviewer_id": "user-1",
                    "approval_id": "appr-missing-1"
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_body = to_bytes(promote_resp.into_body(), usize::MAX)
        .await
        .expect("promote body");
    let promote_payload: Value = serde_json::from_slice(&promote_body).expect("promote json");
    assert_eq!(
        promote_payload.get("promoted").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        promote_payload
            .get("scrub_report")
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("blocked")
    );

    let blocked_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        blocked_event
            .properties
            .get("sourceMemoryID")
            .and_then(Value::as_str),
        Some("missing-memory-id")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-3-missing")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("kind")
        .is_some_and(Value::is_null));
    assert!(blocked_event
        .properties
        .get("classification")
        .is_some_and(Value::is_null));
    assert_eq!(
        blocked_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert!(blocked_event
        .properties
        .get("visibility")
        .is_some_and(Value::is_null));
    assert_eq!(
        blocked_event
            .properties
            .get("scrubStatus")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("source memory missing")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-3-missing")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_promote_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_promote")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row.get("source_memory_id").and_then(Value::as_str)
                        == Some("missing-memory-id")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("source memory missing")
                                && detail.contains("origin_run_id=run-3-missing")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("missing source promote audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_promote_exists
            .get("audit_id")
            .and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_promote_requires_review_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let capability = json!({
        "run_id": "run-3-review",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": true,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-review",
                "source_memory_id": "review-guardrail-memory",
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "review required test",
                "review": {
                    "required": true
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::FORBIDDEN);

    let blocked_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        blocked_event
            .properties
            .get("sourceMemoryID")
            .and_then(Value::as_str),
        Some("review-guardrail-memory")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-3-review")
    );
    assert_eq!(
        blocked_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(blocked_event
        .properties
        .get("kind")
        .is_some_and(Value::is_null));
    assert!(blocked_event
        .properties
        .get("classification")
        .is_some_and(Value::is_null));
    assert_eq!(
        blocked_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert!(blocked_event
        .properties
        .get("visibility")
        .is_some_and(Value::is_null));
    assert!(blocked_event
        .properties
        .get("scrubStatus")
        .is_some_and(Value::is_null));
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("review approval required")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-3-review")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let blocked_promote_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_promote")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row.get("source_memory_id").and_then(Value::as_str)
                        == Some("review-guardrail-memory")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("review approval required")
                                && detail.contains("origin_run_id=run-3-review")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("review-blocked memory_promote audit row");
    assert_eq!(
        blocked_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_promote_exists
            .get("audit_id")
            .and_then(Value::as_str)
    );
}
