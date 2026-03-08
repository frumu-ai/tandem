use super::*;

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
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_put")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("write tier not allowed"))
            })
        })
        .unwrap_or(false);
    assert!(blocked_put_exists);
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
        .map(|rows| {
            rows.iter().any(|row| {
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
                        })
            })
        })
        .unwrap_or(false);
    assert!(search_audit_exists);
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
        .map(|events| {
            events.iter().any(|event| {
                event.get("action").and_then(|v| v.as_str()) == Some("memory_promote")
                    && event.get("status").and_then(|v| v.as_str()) == Some("blocked")
            })
        })
        .unwrap_or(false);
    assert!(blocked_promote_exists);
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
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_promote")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row.get("source_memory_id").and_then(Value::as_str)
                        == Some("review-guardrail-memory")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("review approval required"))
            })
        })
        .unwrap_or(false);
    assert!(blocked_promote_exists);
}

#[tokio::test]
async fn memory_promote_rejects_disallowed_target_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let capability = json!({
        "run_id": "run-3-target",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["team"],
            "require_review_for_promote": false,
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
                "run_id": "run-3-target",
                "source_memory_id": "target-guardrail-memory",
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "disallowed target test",
                "review": {
                    "required": false
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
        Some("target-guardrail-memory")
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
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("promotion target not allowed")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-3-target")
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
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_promote")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row.get("source_memory_id").and_then(Value::as_str)
                        == Some("target-guardrail-memory")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("promotion target not allowed"))
            })
        })
        .unwrap_or(false);
    assert!(blocked_promote_exists);
}

#[tokio::test]
async fn memory_promote_preserves_artifact_refs_and_shared_visibility() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();
    let artifact_refs = vec![Value::from("artifact://run-3/task-1/patch.diff")];

    let capability = json!({
        "run_id": "run-3-ok",
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
                "run_id": "run-3-ok",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "solution_capsule",
                "content": "safe promote memory with artifact provenance",
                "artifact_refs": artifact_refs,
                "classification": "internal",
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
    let put_audit_id = put_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("put audit id")
        .to_string();
    let put_event = next_event_of_type(&mut rx, "memory.put").await;
    assert_eq!(
        put_event.properties.get("memoryID").and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        put_event.properties.get("kind").and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        put_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        put_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        put_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("private")
    );
    let put_updated_event = next_event_of_type(&mut rx, "memory.updated").await;
    assert_eq!(
        put_updated_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("runID")
            .and_then(Value::as_str),
        Some("run-3-ok")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("put")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("private")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("tier")
            .and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(
        put_updated_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        Some(put_audit_id.as_str())
    );

    let private_project_search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-ok",
                "query": "safe promote memory",
                "read_scopes": ["project"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "capability": capability,
                "limit": 5
            })
            .to_string(),
        ))
        .expect("private project search request");
    let private_project_search_resp = app
        .clone()
        .oneshot(private_project_search_req)
        .await
        .expect("private project search response");
    assert_eq!(private_project_search_resp.status(), StatusCode::OK);
    let private_project_search_body = to_bytes(private_project_search_resp.into_body(), usize::MAX)
        .await
        .expect("private project search body");
    let private_project_search_payload: Value =
        serde_json::from_slice(&private_project_search_body).expect("private project search json");
    assert_eq!(
        private_project_search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-ok",
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
        promote_payload.get("promoted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        promote_payload.get("new_memory_id").and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    let promote_audit_id = promote_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("promote audit id")
        .to_string();
    let promote_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        promote_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        promote_event
            .properties
            .get("sourceMemoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        promote_event.properties.get("kind").and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        promote_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        promote_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        promote_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("shared")
    );
    assert_eq!(
        promote_event
            .properties
            .get("toTier")
            .and_then(Value::as_str),
        Some("project")
    );
    assert_eq!(
        promote_event
            .properties
            .get("approvalID")
            .and_then(Value::as_str),
        Some("appr-1")
    );
    let promote_updated_event = next_event_of_type(&mut rx, "memory.updated").await;
    assert_eq!(
        promote_updated_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("runID")
            .and_then(Value::as_str),
        Some("run-3-ok")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("promote")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("solution_capsule")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("shared")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("tier")
            .and_then(Value::as_str),
        Some("project")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("sourceMemoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("approvalID")
            .and_then(Value::as_str),
        Some("appr-1")
    );
    assert_eq!(
        promote_updated_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        Some(promote_audit_id.as_str())
    );

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-ok",
                "query": "safe promote memory",
                "read_scopes": ["project"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "capability": capability,
                "limit": 5
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("search json");
    let promoted_hit = search_payload
        .get("results")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(memory_id.as_str()))
        })
        .cloned()
        .expect("promoted hit");
    assert_eq!(
        promoted_hit.get("tier").and_then(Value::as_str),
        Some("project")
    );
    assert_eq!(
        promoted_hit.get("visibility").and_then(Value::as_str),
        Some("shared")
    );
    assert_eq!(
        promoted_hit.get("artifact_refs").and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("review"))
            .and_then(|row| row.get("approval_id"))
            .and_then(Value::as_str),
        Some("appr-1")
    );
    assert_eq!(
        promoted_hit
            .get("provenance")
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("promote_run_id"))
            .and_then(Value::as_str),
        Some("run-3-ok")
    );
    assert_eq!(
        promoted_hit
            .get("linkage")
            .and_then(|row| row.get("promote_run_id"))
            .and_then(Value::as_str),
        Some("run-3-ok")
    );
    assert_eq!(
        promoted_hit
            .get("linkage")
            .and_then(|row| row.get("approval_id"))
            .and_then(Value::as_str),
        Some("appr-1")
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-3-ok")
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
    let put_audit_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_put")
                    && row.get("audit_id").and_then(Value::as_str) == Some(put_audit_id.as_str())
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("kind=solution_capsule")
                                && detail.contains("classification=internal")
                                && detail.contains("artifact_refs=artifact://run-3/task-1/fix.json")
                                && detail.contains("visibility=private")
                                && detail.contains("tier=session")
                                && detail.contains("partition_key=org-1/ws-1/proj-1/session")
                        })
            })
        })
        .unwrap_or(false);
    assert!(put_audit_exists);
    let promote_audit_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_promote")
                    && row.get("audit_id").and_then(Value::as_str)
                        == Some(promote_audit_id.as_str())
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("kind=solution_capsule")
                                && detail.contains("classification=internal")
                                && detail.contains("artifact_refs=artifact://run-3/task-1/fix.json")
                                && detail.contains("visibility=shared")
                                && detail.contains("tier=project")
                                && detail.contains("partition_key=org-1/ws-1/proj-1/project")
                                && detail.contains("source_memory_id=")
                                && detail.contains("approval_id=appr-1")
                        })
            })
        })
        .unwrap_or(false);
    assert!(promote_audit_exists);
}

#[tokio::test]
async fn memory_list_and_delete_admin_routes_work() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();
    let artifact_refs = vec![Value::from("artifact://run-4/task-1/admin.json")];

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-4",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "admin memory test",
                "artifact_refs": artifact_refs,
                "classification": "internal",
                "metadata": null
            })
            .to_string(),
        ))
        .expect("memory put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("memory put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("memory put body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("memory put json");
    let memory_id = put_payload
        .get("id")
        .and_then(|v| v.as_str())
        .expect("memory id")
        .to_string();

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?limit=20")
        .body(Body::empty())
        .expect("memory list request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("memory list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("memory list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("memory list json");
    let contains = list_payload
        .get("items")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("id").and_then(|v| v.as_str()) == Some(memory_id.as_str())
                    && row.get("classification").and_then(Value::as_str) == Some("internal")
                    && row.get("tier").and_then(Value::as_str) == Some("session")
                    && row.get("kind").and_then(Value::as_str) == Some("fact")
                    && row.get("artifact_refs").and_then(Value::as_array) == Some(&artifact_refs)
                    && row
                        .get("linkage")
                        .and_then(|v| v.get("origin_run_id"))
                        .and_then(Value::as_str)
                        == Some("run-4")
                    && row
                        .get("linkage")
                        .and_then(|v| v.get("project_id"))
                        .and_then(Value::as_str)
                        == Some("proj-1")
                    && row
                        .get("provenance")
                        .and_then(|v| v.get("origin_run_id"))
                        .and_then(Value::as_str)
                        == Some("run-4")
                    && row
                        .get("provenance")
                        .and_then(|v| v.get("partition"))
                        .and_then(|v| v.get("project_id"))
                        .and_then(Value::as_str)
                        == Some("proj-1")
            })
        })
        .unwrap_or(false);
    assert!(contains);

    let del_req = Request::builder()
        .method("DELETE")
        .uri(format!("/memory/{memory_id}"))
        .body(Body::empty())
        .expect("memory delete request");
    let del_resp = app
        .clone()
        .oneshot(del_req)
        .await
        .expect("memory delete response");
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .expect("memory delete body");
    let del_payload: Value = serde_json::from_slice(&del_body).expect("memory delete json");
    let delete_audit_id = del_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("memory delete audit id")
        .to_string();
    let delete_event = next_event_of_type(&mut rx, "memory.deleted").await;
    assert_eq!(
        delete_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        delete_event.properties.get("runID").and_then(Value::as_str),
        Some("run-4")
    );
    assert_eq!(
        delete_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        Some(delete_audit_id.as_str())
    );
    assert_eq!(
        delete_event.properties.get("kind").and_then(Value::as_str),
        Some("fact")
    );
    assert_eq!(
        delete_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        delete_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&artifact_refs)
    );
    assert_eq!(
        delete_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("private")
    );
    assert_eq!(
        delete_event.properties.get("tier").and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(
        delete_event
            .properties
            .get("demoted")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        delete_event
            .properties
            .get("partitionKey")
            .and_then(Value::as_str),
        Some("org-1/ws-1/proj-1/session")
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-4")
        .body(Body::empty())
        .expect("memory audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("memory audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("memory audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("memory audit json");
    let delete_audit_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_delete")
                    && row.get("status").and_then(Value::as_str) == Some("ok")
                    && row.get("memory_id").and_then(Value::as_str) == Some(memory_id.as_str())
                    && row.get("audit_id").and_then(Value::as_str) == Some(delete_audit_id.as_str())
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("kind=fact")
                                && detail.contains("classification=internal")
                                && detail
                                    .contains("artifact_refs=artifact://run-4/task-1/admin.json")
                                && detail.contains("visibility=private")
                                && detail.contains("tier=session")
                                && detail.contains("partition_key=org-1/ws-1/proj-1/session")
                                && detail.contains("demoted=false")
                        })
            })
        })
        .unwrap_or(false);
    assert!(delete_audit_exists);
}

#[tokio::test]
async fn memory_demote_hides_item_from_search_results() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-5",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "demote me from search",
                "classification": "internal"
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

    let demote_req = Request::builder()
        .method("POST")
        .uri("/memory/demote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "id": memory_id,
                "run_id": "run-5"
            })
            .to_string(),
        ))
        .expect("demote request");
    let demote_resp = app
        .clone()
        .oneshot(demote_req)
        .await
        .expect("demote response");
    assert_eq!(demote_resp.status(), StatusCode::OK);
    let demote_body = to_bytes(demote_resp.into_body(), usize::MAX)
        .await
        .expect("demote body");
    let demote_payload: Value = serde_json::from_slice(&demote_body).expect("demote json");
    let demote_audit_id = demote_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("demote audit id")
        .to_string();
    assert!(demote_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .is_some());
    let demote_event = next_event_of_type(&mut rx, "memory.updated").await;
    assert_eq!(
        demote_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some(memory_id.as_str())
    );
    assert_eq!(
        demote_event.properties.get("runID").and_then(Value::as_str),
        Some("run-5")
    );
    assert_eq!(
        demote_event
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("demote")
    );
    assert_eq!(
        demote_event.properties.get("kind").and_then(Value::as_str),
        Some("fact")
    );
    assert_eq!(
        demote_event
            .properties
            .get("classification")
            .and_then(Value::as_str),
        Some("internal")
    );
    assert_eq!(
        demote_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array),
        Some(&Vec::<Value>::new())
    );
    assert_eq!(
        demote_event
            .properties
            .get("visibility")
            .and_then(Value::as_str),
        Some("private")
    );
    assert_eq!(
        demote_event.properties.get("tier").and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(
        demote_event
            .properties
            .get("demoted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        demote_event
            .properties
            .get("partitionKey")
            .and_then(Value::as_str),
        Some("org-1/ws-1/proj-1/session")
    );
    assert_eq!(
        demote_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        Some(demote_audit_id.as_str())
    );

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-5",
                "query": "demote me from search",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "read_scopes": ["session"],
                "limit": 10
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("search json");
    let count = search_payload
        .get("results")
        .and_then(|v| v.as_array())
        .map(|rows| rows.len())
        .unwrap_or_default();
    assert_eq!(count, 0);

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?limit=20")
        .body(Body::empty())
        .expect("memory list request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("memory list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("memory list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("memory list json");
    let demoted_row = list_payload
        .get("items")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(memory_id.as_str()))
        })
        .cloned()
        .expect("demoted memory row");
    assert_eq!(
        demoted_row.get("demoted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        demoted_row.get("visibility").and_then(Value::as_str),
        Some("private")
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-5")
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
    let demote_audit_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_demote")
                    && row.get("memory_id").and_then(Value::as_str) == Some(memory_id.as_str())
                    && row.get("status").and_then(Value::as_str) == Some("ok")
                    && row.get("audit_id").and_then(Value::as_str) == Some(demote_audit_id.as_str())
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("kind=fact")
                                && detail.contains("classification=internal")
                                && detail.contains("artifact_refs=")
                                && detail.contains("visibility=private")
                                && detail.contains("tier=session")
                                && detail.contains("partition_key=org-1/ws-1/proj-1/session")
                                && detail.contains("demoted=true")
                        })
            })
        })
        .unwrap_or(false);
    assert!(demote_audit_exists);
}

#[tokio::test]
async fn memory_search_returns_empty_when_all_requested_scopes_are_blocked() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-6",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "blocked scopes should return no results",
                "classification": "internal"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let capability = json!({
        "run_id": "run-6",
        "subject": "default",
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

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-6",
                "query": "blocked scopes should return no results",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "read_scopes": ["team"],
                "capability": capability,
                "limit": 10
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("search json");
    assert_eq!(
        search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        search_payload
            .get("scopes_used")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        search_payload
            .get("blocked_scopes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["team"])
    );
    let search_audit_id = search_payload
        .get("audit_id")
        .and_then(Value::as_str)
        .expect("search audit id");
    let search_event = next_event_of_type(&mut rx, "memory.search").await;
    assert_eq!(
        search_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        search_event.properties.get("query").and_then(Value::as_str),
        Some("blocked scopes should return no results")
    );
    assert_eq!(
        search_event
            .properties
            .get("resultIDs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        search_event
            .properties
            .get("resultKinds")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        search_event
            .properties
            .get("requestedScopes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["team"])
    );
    assert_eq!(
        search_event
            .properties
            .get("scopesUsed")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        search_event
            .properties
            .get("blockedScopes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["team"])
    );
    assert_eq!(
        search_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        Some(search_audit_id)
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-6")
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
    let blocked_search_exists = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_search")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row.get("audit_id").and_then(Value::as_str) == Some(search_audit_id)
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("query=blocked scopes should return no results")
                                && detail.contains("result_count=0")
                                && detail.contains("result_kinds=")
                                && detail.contains("scopes_used=")
                                && detail.contains("blocked_scopes=team")
                        })
            })
        })
        .unwrap_or(false);
    assert!(blocked_search_exists);
}
