#[tokio::test]
async fn memory_delete_missing_memory_writes_not_found_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let del_req = Request::builder()
        .method("DELETE")
        .uri("/memory/missing-delete-memory")
        .body(Body::empty())
        .expect("memory delete request");
    let del_resp = app
        .clone()
        .oneshot(del_req)
        .await
        .expect("memory delete response");
    assert_eq!(del_resp.status(), StatusCode::NOT_FOUND);
    let delete_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.deleted"),
    )
    .await
    .expect("missing memory.deleted event");
    assert_eq!(
        delete_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some("missing-delete-memory")
    );
    assert_eq!(
        delete_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("not_found")
    );
    assert!(delete_event
        .properties
        .get("kind")
        .is_some_and(Value::is_null));
    assert!(delete_event
        .properties
        .get("classification")
        .is_some_and(Value::is_null));
    assert_eq!(
        delete_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert!(delete_event
        .properties
        .get("visibility")
        .is_some_and(Value::is_null));
    assert!(delete_event
        .properties
        .get("tier")
        .is_some_and(Value::is_null));
    assert!(delete_event
        .properties
        .get("partitionKey")
        .is_some_and(Value::is_null));
    assert!(delete_event
        .properties
        .get("demoted")
        .is_some_and(Value::is_null));
    assert!(delete_event
        .properties
        .get("runID")
        .is_some_and(Value::is_null));
    assert!(delete_event.properties.get("linkage").is_none());
    assert!(delete_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("memory not found")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit")
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
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_delete")
                    && row.get("status").and_then(Value::as_str) == Some("not_found")
                    && row.get("memory_id").and_then(Value::as_str) == Some("missing-delete-memory")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("memory not found"))
            })
        })
        .cloned()
        .expect("missing delete audit row");
    assert!(delete_audit_exists
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| {
            !detail.contains("origin_run_id=") && !detail.contains("project_id=")
        }));
    assert_eq!(
        delete_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        delete_audit_exists.get("audit_id").and_then(Value::as_str)
    );
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
    let _put_updated_event = next_event_of_type(&mut rx, "memory.updated").await;

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
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-5")
    );
    assert_eq!(
        demote_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
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
                                && detail.contains("origin_run_id=run-5")
                                && detail.contains("project_id=proj-1")
                                && detail.contains("promote_run_id=")
                        })
            })
        })
        .unwrap_or(false);
    assert!(demote_audit_exists);
}

#[tokio::test]
async fn memory_demote_missing_memory_writes_not_found_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let demote_req = Request::builder()
        .method("POST")
        .uri("/memory/demote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "id": "missing-demote-memory",
                "run_id": "run-5-missing"
            })
            .to_string(),
        ))
        .expect("demote request");
    let demote_resp = app
        .clone()
        .oneshot(demote_req)
        .await
        .expect("demote response");
    assert_eq!(demote_resp.status(), StatusCode::NOT_FOUND);
    let demote_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.updated"),
    )
    .await
    .expect("missing memory.updated event");
    assert_eq!(
        demote_event
            .properties
            .get("memoryID")
            .and_then(Value::as_str),
        Some("missing-demote-memory")
    );
    assert_eq!(
        demote_event.properties.get("runID").and_then(Value::as_str),
        Some("run-5-missing")
    );
    assert_eq!(
        demote_event
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("demote")
    );
    assert_eq!(
        demote_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("not_found")
    );
    assert!(demote_event
        .properties
        .get("kind")
        .is_some_and(Value::is_null));
    assert!(demote_event
        .properties
        .get("classification")
        .is_some_and(Value::is_null));
    assert_eq!(
        demote_event
            .properties
            .get("artifactRefs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert!(demote_event
        .properties
        .get("visibility")
        .is_some_and(Value::is_null));
    assert!(demote_event
        .properties
        .get("tier")
        .is_some_and(Value::is_null));
    assert_eq!(
        demote_event
            .properties
            .get("partitionKey")
            .and_then(Value::as_str),
        Some("demoted")
    );
    assert!(demote_event
        .properties
        .get("demoted")
        .is_some_and(Value::is_null));
    assert!(demote_event.properties.get("linkage").is_none());
    assert!(demote_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("memory not found")));

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-5-missing")
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
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_demote")
                    && row.get("memory_id").and_then(Value::as_str) == Some("missing-demote-memory")
                    && row.get("status").and_then(Value::as_str) == Some("not_found")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("memory not found"))
            })
        })
        .cloned()
        .expect("missing demote audit row");
    assert!(demote_audit_exists
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| {
            !detail.contains("origin_run_id=") && !detail.contains("project_id=")
        }));
    assert_eq!(
        demote_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        demote_audit_exists.get("audit_id").and_then(Value::as_str)
    );
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
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-6")
    );
    assert_eq!(
        search_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
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
                                && detail.contains("origin_run_id=run-6")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .unwrap_or(false);
    assert!(blocked_search_exists);
}

#[tokio::test]
async fn memory_search_hides_source_bound_records_without_strict_grant() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let capability = memory_capability(
        "source-bound-global-run",
        "default",
        "org-1",
        "ws-1",
        "proj-1",
    );

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "source-bound-global-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "payroll restricted governed memory must stay hidden",
                "classification": "restricted",
                "metadata": {
                    "enterprise_source_binding": {
                        "binding_id": "binding-hr-finance",
                        "connector_id": "manual-upload",
                        "resource_ref": {
                            "organization_id": "org-1",
                            "workspace_id": "ws-1",
                            "resource_kind": "document_collection",
                            "resource_id": "hr-payroll"
                        },
                        "data_class": "financial_record",
                        "source_object_id": "source-object-hr-payroll",
                        "native_object_id": "/imports/hr/payroll.md",
                        "content_hash": "hash-hr-payroll"
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("source-bound put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("source-bound put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "source-bound-global-run",
                "query": "payroll restricted governed memory",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "capability": capability,
                "limit": 10
            })
            .to_string(),
        ))
        .expect("source-bound search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("source-bound search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("source-bound search body");
    let search_payload: Value =
        serde_json::from_slice(&search_body).expect("source-bound search json");
    assert_eq!(
        search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "source-bound governed/global memory must be hidden without a strict read grant"
    );
}

#[tokio::test]
async fn memory_list_hides_source_bound_citation_metadata_without_strict_grant() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let capability = memory_capability(
        "source-bound-list-run",
        "default",
        "org-1",
        "ws-1",
        "proj-1",
    );

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "source-bound-list-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "payroll citation metadata must not leak from list",
                "classification": "restricted",
                "metadata": {
                    "enterprise_source_binding": {
                        "binding_id": "binding-hr-finance",
                        "connector_id": "manual-upload",
                        "resource_ref": {
                            "organization_id": "org-1",
                            "workspace_id": "ws-1",
                            "resource_kind": "document_collection",
                            "resource_id": "hr-payroll"
                        },
                        "data_class": "financial_record",
                        "source_object_id": "source-object-hr-payroll",
                        "native_object_id": "/imports/hr/payroll.md",
                        "content_hash": "hash-hr-payroll"
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("source-bound list put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("source-bound list put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?q=payroll&project_id=proj-1")
        .body(Body::empty())
        .expect("source-bound list request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("source-bound list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("source-bound list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("source-bound list json");
    assert_eq!(
        list_payload
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "source-bound list results must not expose citation/source-object metadata without a strict read grant"
    );
    let serialized = serde_json::to_string(&list_payload).expect("list payload string");
    assert!(!serialized.contains("source-object-hr-payroll"));
    assert!(!serialized.contains("/imports/hr/payroll.md"));
    assert!(!serialized.contains("binding-hr-finance"));
}

#[tokio::test]
async fn memory_search_rejects_expired_capability_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-6-expired",
                "query": "expired capability search",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "read_scopes": ["session"],
                "capability": {
                    "run_id": "run-6-expired",
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
                },
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
    assert_eq!(search_resp.status(), StatusCode::UNAUTHORIZED);

    let search_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.search"),
    )
    .await
    .expect("blocked memory.search event");
    assert_eq!(
        search_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        search_event.properties.get("query").and_then(Value::as_str),
        Some("expired capability search")
    );
    assert!(search_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("capability expired")));
    assert_eq!(
        search_event
            .properties
            .get("blockedScopes")
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
        Some("run-6-expired")
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
        .uri("/memory/audit?run_id=run-6-expired")
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
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_search")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("capability expired")
                                && detail.contains("blocked_scopes=session")
                                && detail.contains("origin_run_id=run-6-expired")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("expired memory_search audit row");
    assert_eq!(
        search_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_search_exists
            .get("audit_id")
            .and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_search_rejects_mismatched_capability_and_emits_blocked_audit() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-6-cap-mismatch",
                "query": "mismatched capability search",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "read_scopes": ["session"],
                "capability": {
                    "run_id": "run-6-cap-mismatch",
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
                },
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
    assert_eq!(search_resp.status(), StatusCode::FORBIDDEN);

    let search_event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_event_of_type(&mut rx, "memory.search"),
    )
    .await
    .expect("blocked memory.search event");
    assert_eq!(
        search_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        search_event.properties.get("query").and_then(Value::as_str),
        Some("mismatched capability search")
    );
    assert_eq!(
        search_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("origin_run_id"))
            .and_then(Value::as_str),
        Some("run-6-cap-mismatch")
    );
    assert_eq!(
        search_event
            .properties
            .get("linkage")
            .and_then(|v| v.get("project_id"))
            .and_then(Value::as_str),
        Some("proj-1")
    );
    assert!(search_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("capability context mismatch")));
    assert_eq!(
        search_event
            .properties
            .get("blockedScopes")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["session"])
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=run-6-cap-mismatch")
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
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("action").and_then(Value::as_str) == Some("memory_search")
                    && row.get("status").and_then(Value::as_str) == Some("blocked")
                    && row
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| {
                            detail.contains("capability context mismatch")
                                && detail.contains("blocked_scopes=session")
                                && detail.contains("origin_run_id=run-6-cap-mismatch")
                                && detail.contains("project_id=proj-1")
                        })
            })
        })
        .cloned()
        .expect("mismatched memory_search audit row");
    assert_eq!(
        search_event
            .properties
            .get("auditID")
            .and_then(Value::as_str),
        blocked_search_exists
            .get("audit_id")
            .and_then(Value::as_str)
    );
}

#[tokio::test]
async fn memory_audit_filters_by_tenant_context() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-a")
        .body(Body::from(
            json!({
                "run_id": "tenant-a-run",
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "tenant scoped audit record",
                "classification": "internal"
            })
            .to_string(),
        ))
        .expect("tenant put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("tenant put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=tenant-a-run")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-a")
        .body(Body::empty())
        .expect("tenant audit request");
    let audit_resp = app
        .clone()
        .oneshot(audit_req)
        .await
        .expect("tenant audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    let events = audit_payload
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(!events.is_empty(), "expected tenant-a audit events");
    assert!(events.iter().all(|row| {
        row.get("tenant_context")
            .and_then(|tenant| tenant.get("org_id"))
            .and_then(Value::as_str)
            == Some("acme")
    }));

    let foreign_audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=tenant-a-run")
        .header("x-tandem-org-id", "beta")
        .header("x-tandem-workspace-id", "south")
        .header("x-tandem-actor-id", "user-b")
        .body(Body::empty())
        .expect("foreign audit request");
    let foreign_audit_resp = app
        .clone()
        .oneshot(foreign_audit_req)
        .await
        .expect("foreign audit response");
    assert_eq!(foreign_audit_resp.status(), StatusCode::OK);
    let foreign_audit_body = to_bytes(foreign_audit_resp.into_body(), usize::MAX)
        .await
        .expect("foreign audit body");
    let foreign_audit_payload: Value =
        serde_json::from_slice(&foreign_audit_body).expect("foreign audit json");
    assert_eq!(
        foreign_audit_payload.get("count").and_then(Value::as_u64),
        Some(0)
    );
}

fn tenant_memory_request(
    method: &str,
    uri: &str,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .expect("tenant memory request")
}

fn memory_capability(
    run_id: &str,
    subject: &str,
    org_id: &str,
    workspace_id: &str,
    project_id: &str,
) -> Value {
    json!({
        "run_id": run_id,
        "subject": subject,
        "org_id": org_id,
        "workspace_id": workspace_id,
        "project_id": project_id,
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": false,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    })
}

fn verified_delegate_context(
    tenant_context: tandem_types::TenantContext,
    delegate_id: &str,
) -> tandem_types::VerifiedTenantContext {
    let principal = tandem_types::RequestPrincipal::authenticated_user("user-a", "tandem-web");
    let authority_chain = tandem_types::AuthorityChain::from_request(principal);
    let strict_projection = tandem_types::StrictTenantContext::new(
        tenant_context.clone(),
        tandem_types::PrincipalRef::new(tandem_types::PrincipalKind::ExternalDelegate, delegate_id),
        authority_chain.clone(),
        tandem_types::ResourceScope::root(tandem_types::ResourceRef::new(
            "acme",
            "north",
            tandem_types::ResourceKind::Project,
            "proj-a",
        )),
        tandem_types::AssertionMetadata::new(
            "tandem-web",
            "tandem-runtime",
            1_000,
            9_999_999_999_999,
            "delegate-assertion",
        ),
    )
    .with_data_boundary(tandem_types::DataBoundary::allow(vec![
        tandem_types::DataClass::Internal,
    ]));
    tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user("user-a"),
        authority_chain,
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: Some(strict_projection),
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: "delegate-assertion".to_string(),
    }
}

#[tokio::test]
async fn channel_memory_search_requires_retrieval_gateway() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "channel-gateway-required-run",
                "query": "broad channel search",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "capability": memory_capability(
                    "channel-gateway-required-run",
                    "channel:slack:U123",
                    "org-1",
                    "ws-1",
                    "proj-1"
                )
            })
            .to_string(),
        ))
        .expect("channel search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("channel search response");
    assert_eq!(search_resp.status(), StatusCode::FORBIDDEN);

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=channel-gateway-required-run")
        .body(Body::empty())
        .expect("audit request");
    let audit_resp = app.oneshot(audit_req).await.expect("audit response");
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
        .await
        .expect("audit body");
    let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
    assert!(audit_payload
        .get("events")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("action").and_then(Value::as_str) == Some("memory_search")
                && row.get("status").and_then(Value::as_str) == Some("blocked")
                && row
                    .get("detail")
                    .and_then(Value::as_str)
                    .is_some_and(|detail| detail.contains("requires retrieval gateway"))
        })));
}

#[tokio::test]
async fn retrieval_gateway_filters_source_classification_and_query_budget() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let subject = "channel:slack:U456";
    let capability = memory_capability("gateway-filter-run", subject, "org-1", "ws-1", "proj-1");
    for (content, binding_id, data_class, source_object_id) in [
        (
            "gateway wedge needle allowed internal product plan",
            "binding-product",
            "internal",
            "source-product-plan",
        ),
        (
            "gateway wedge needle restricted finance plan",
            "binding-product",
            "financial_record",
            "source-finance-plan",
        ),
        (
            "gateway wedge needle internal support plan",
            "binding-support",
            "internal",
            "source-support-plan",
        ),
    ] {
        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "gateway-filter-run",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "fact",
                    "content": content,
                    "classification": "internal",
                    "metadata": {
                        "enterprise_source_binding": {
                            "binding_id": binding_id,
                            "connector_id": "manual-upload",
                            "resource_ref": {
                                "organization_id": "org-1",
                                "workspace_id": "ws-1",
                                "resource_kind": "document_collection",
                                "resource_id": binding_id
                            },
                            "data_class": data_class,
                            "source_object_id": source_object_id,
                            "native_object_id": format!("/imports/{source_object_id}.md"),
                            "content_hash": format!("hash-{source_object_id}")
                        }
                    },
                    "capability": capability
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);
    }

    let gateway = json!({
        "grant": {
            "grant_id": "grant-product-internal",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "source_binding_ids": ["binding-product"],
            "data_classes": ["internal"],
            "budgets": {
                "max_queries_per_window": 1,
                "window_ms": 60000,
                "max_top_k": 5,
                "max_tokens": 50,
                "max_chars": 500
            }
        },
        "session_id": "kb-session-1",
        "channel": "slack",
        "user_id": "U456"
    });

    let search_body = json!({
        "run_id": "gateway-filter-run",
        "query": "gateway wedge needle",
        "read_scopes": ["session"],
        "partition": {
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_id": "proj-1",
            "tier": "session"
        },
        "capability": capability,
        "retrieval_gateway": gateway,
        "limit": 10
    });
    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(search_body.to_string()))
        .expect("gateway search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("gateway search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("gateway search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("gateway search json");
    let results = search_payload
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results.len(), 1);
    let serialized_results = serde_json::to_string(results).expect("results string");
    assert!(serialized_results.contains("allowed internal product plan"));
    assert!(!serialized_results.contains("restricted finance plan"));
    assert!(!serialized_results.contains("internal support plan"));
    assert!(serialized_results.contains("enterprise_source_binding"));
    assert!(serialized_results.contains("binding-product"));

    let second_search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "gateway-filter-run",
                "query": "gateway wedge needle",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "capability": capability,
                "retrieval_gateway": gateway,
                "limit": 10
            })
            .to_string(),
        ))
        .expect("second gateway search request");
    let second_search_resp = app
        .clone()
        .oneshot(second_search_req)
        .await
        .expect("second gateway search response");
    assert_eq!(second_search_resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=gateway-filter-run")
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
    assert!(audit_payload
        .get("events")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("action").and_then(Value::as_str) == Some("memory_search")
                && row.get("status").and_then(Value::as_str) == Some("blocked")
                && row
                    .get("detail")
                    .and_then(Value::as_str)
                    .is_some_and(|detail| detail.contains("query budget exhausted"))
        })));

    let revoked_gateway = json!({
        "grant": {
            "grant_id": "grant-product-revoked",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "source_binding_ids": ["binding-product"],
            "data_classes": ["internal"],
            "revoked": true
        },
        "session_id": "kb-session-2",
        "channel": "slack",
        "user_id": "U456"
    });
    let revoked_search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "gateway-filter-run",
                "query": "gateway wedge needle",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "capability": capability,
                "retrieval_gateway": revoked_gateway,
                "limit": 10
            })
            .to_string(),
        ))
        .expect("revoked gateway search request");
    let revoked_search_resp = app
        .oneshot(revoked_search_req)
        .await
        .expect("revoked gateway search response");
    assert_eq!(revoked_search_resp.status(), StatusCode::FORBIDDEN);
}
