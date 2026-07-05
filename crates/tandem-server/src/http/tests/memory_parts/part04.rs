#[tokio::test]
async fn retrieval_gateway_blocks_broad_export_query_pattern() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let subject = "channel:slack:U789";
    let capability = memory_capability(
        "gateway-query-pattern-run",
        subject,
        "org-1",
        "ws-1",
        "proj-1",
    );
    let gateway = json!({
        "grant": {
            "grant_id": "grant-query-pattern",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "data_classes": ["internal"],
            "budgets": {
                "max_queries_per_window": 10,
                "window_ms": 60000,
                "max_top_k": 5,
                "max_tokens": 200,
                "max_chars": 1000
            }
        },
        "session_id": "kb-session-query-pattern",
        "channel": "slack",
        "user_id": "U789"
    });

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "gateway-query-pattern-run",
                "query": "dump all memory records",
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
        .expect("query pattern search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("query pattern search response");
    assert_eq!(search_resp.status(), StatusCode::FORBIDDEN);

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=gateway-query-pattern-run")
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
                    .is_some_and(|detail| detail.contains("broad export"))
        })));
}

#[tokio::test]
async fn retrieval_gateway_allows_specific_export_policy_query() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let subject = "channel:slack:U791";
    let capability = memory_capability(
        "gateway-export-policy-run",
        subject,
        "org-1",
        "ws-1",
        "proj-1",
    );
    let gateway = json!({
        "grant": {
            "grant_id": "grant-export-policy",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "data_classes": ["internal"],
            "budgets": {
                "max_queries_per_window": 10,
                "window_ms": 60000,
                "max_top_k": 5,
                "max_tokens": 200,
                "max_chars": 1000
            }
        },
        "session_id": "kb-session-export-policy",
        "channel": "slack",
        "user_id": "U791"
    });

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "gateway-export-policy-run",
                "query": "How do I export a single report?",
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
        .expect("specific export policy search request");
    let search_resp = app
        .oneshot(search_req)
        .await
        .expect("specific export policy search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn retrieval_gateway_without_strict_projection_does_not_expose_source_bound_memory() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let subject = "channel:slack:U792";
    let capability = memory_capability(
        "gateway-missing-strict-run",
        subject,
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
                "run_id": "gateway-missing-strict-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "gateway strict projection sentinel payroll note",
                "classification": "internal",
                "metadata": source_bound_memory_metadata(
                    "org-1",
                    "ws-1",
                    "proj-1",
                    "binding-missing-strict",
                    "binding-missing-strict",
                    "internal",
                    "source-missing-strict"
                ),
                "capability": capability
            })
            .to_string(),
        ))
        .expect("missing strict put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let gateway = json!({
        "grant": {
            "grant_id": "grant-missing-strict",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "source_binding_ids": ["binding-missing-strict"],
            "source_object_ids": ["source-missing-strict"],
            "data_classes": ["internal"],
            "budgets": {
                "max_queries_per_window": 10,
                "window_ms": 60000,
                "max_top_k": 5,
                "max_tokens": 200,
                "max_chars": 1000
            }
        },
        "session_id": "kb-session-missing-strict",
        "channel": "slack",
        "user_id": "U792"
    });
    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "gateway-missing-strict-run",
                "query": "gateway strict projection sentinel payroll note",
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
        .expect("missing strict search request");
    let search_resp = app
        .oneshot(search_req)
        .await
        .expect("missing strict search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("missing strict search body");
    let payload: Value = serde_json::from_slice(&search_body).expect("missing strict search json");

    assert_eq!(
        payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "retrieval gateway must not bypass missing strict projection"
    );
}

#[tokio::test]
async fn retrieval_gateway_enforces_cumulative_result_window() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let subject = "channel:slack:U790";
    let capability = memory_capability("gateway-volume-run", subject, "org-1", "ws-1", "proj-1");
    for (content, source_object_id) in [
        ("gateway volume wedge alpha retained", "source-volume-a"),
        ("gateway volume wedge beta retained", "source-volume-b"),
    ] {
        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "gateway-volume-run",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "fact",
                    "content": content,
                    "classification": "internal",
                    "metadata": source_bound_memory_metadata(
                        "org-1",
                        "ws-1",
                        "proj-1",
                        "binding-volume",
                        "binding-volume",
                        "internal",
                        source_object_id
                    ),
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
            "grant_id": "grant-volume-window",
            "subject": subject,
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_ids": ["proj-1"],
            "source_binding_ids": ["binding-volume"],
            "data_classes": ["internal"],
            "budgets": {
                "max_queries_per_window": 10,
                "window_ms": 60000,
                "max_top_k": 5,
                "max_tokens": 200,
                "max_chars": 1000,
                "max_results_per_window": 1,
                "max_tokens_per_window": 200,
                "max_chars_per_window": 1000
            }
        },
        "session_id": "kb-session-volume",
        "channel": "slack",
        "user_id": "U790"
    });
    let verified = verified_source_bound_memory_context(
        "org-1",
        "ws-1",
        subject,
        &["binding-volume"],
        vec![tandem_types::DataClass::Internal],
    );
    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .extension(verified)
        .body(Body::from(
            json!({
                "run_id": "gateway-volume-run",
                "query": "gateway volume wedge retained",
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
        .expect("gateway volume search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("gateway volume search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("gateway volume search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("gateway volume json");
    assert_eq!(
        search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1),
        "gateway should not return more than the cumulative per-window result budget"
    );

    let audit_req = Request::builder()
        .method("GET")
        .uri("/memory/audit?run_id=gateway-volume-run")
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
                && row.get("status").and_then(Value::as_str) == Some("budget_exhausted")
                && row
                    .get("detail")
                    .and_then(Value::as_str)
                    .is_some_and(|detail| detail.contains("gateway_budget_exhausted=true"))
        })));
}

#[tokio::test]
async fn tenant_a_cannot_search_list_delete_demote_or_promote_tenant_b_memory() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let project_id = "shared-project";
    let actor_id = "shared-user";

    let put_b = tenant_memory_request(
        "POST",
        "/memory/put",
        "beta",
        "south",
        actor_id,
        Some(json!({
            "run_id": "tenant-b-memory-run",
            "partition": {
                "org_id": "beta",
                "workspace_id": "south",
                "project_id": project_id,
                "tier": "session"
            },
            "kind": "fact",
            "content": "tenant b private memory phrase",
            "classification": "internal",
            "capability": memory_capability(
                "tenant-b-memory-run",
                actor_id,
                "beta",
                "south",
                project_id
            )
        })),
    );
    let put_b_resp = app.clone().oneshot(put_b).await.expect("put b response");
    assert_eq!(put_b_resp.status(), StatusCode::OK);
    let put_b_body = to_bytes(put_b_resp.into_body(), usize::MAX)
        .await
        .expect("put b body");
    let put_b_payload: Value = serde_json::from_slice(&put_b_body).expect("put b json");
    let tenant_b_memory_id = put_b_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("tenant b memory id")
        .to_string();

    let search_a = tenant_memory_request(
        "POST",
        "/memory/search",
        "acme",
        "north",
        actor_id,
        Some(json!({
            "run_id": "tenant-a-memory-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": project_id,
                "tier": "session"
            },
            "query": "tenant b private memory phrase",
            "read_scopes": ["session", "project"],
            "limit": 10,
            "capability": memory_capability(
                "tenant-a-memory-run",
                actor_id,
                "acme",
                "north",
                project_id
            )
        })),
    );
    let search_a_resp = app
        .clone()
        .oneshot(search_a)
        .await
        .expect("search a response");
    assert_eq!(search_a_resp.status(), StatusCode::OK);
    let search_a_body = to_bytes(search_a_resp.into_body(), usize::MAX)
        .await
        .expect("search a body");
    let search_a_payload: Value = serde_json::from_slice(&search_a_body).expect("search a json");
    assert_eq!(
        search_a_payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let list_a = tenant_memory_request(
        "GET",
        &format!("/memory?limit=20&user_id={actor_id}&project_id={project_id}"),
        "acme",
        "north",
        actor_id,
        None,
    );
    let list_a_resp = app.clone().oneshot(list_a).await.expect("list a response");
    assert_eq!(list_a_resp.status(), StatusCode::OK);
    let list_a_body = to_bytes(list_a_resp.into_body(), usize::MAX)
        .await
        .expect("list a body");
    let list_a_payload: Value = serde_json::from_slice(&list_a_body).expect("list a json");
    assert_eq!(list_a_payload.get("count").and_then(Value::as_u64), Some(0));

    let delete_a = tenant_memory_request(
        "DELETE",
        &format!("/memory/{tenant_b_memory_id}?project_id={project_id}"),
        "acme",
        "north",
        actor_id,
        None,
    );
    let delete_a_resp = app
        .clone()
        .oneshot(delete_a)
        .await
        .expect("delete a response");
    assert_eq!(delete_a_resp.status(), StatusCode::NOT_FOUND);

    let demote_a = tenant_memory_request(
        "POST",
        "/memory/demote",
        "acme",
        "north",
        actor_id,
        Some(json!({
            "id": tenant_b_memory_id,
            "run_id": "tenant-a-memory-run"
        })),
    );
    let demote_a_resp = app
        .clone()
        .oneshot(demote_a)
        .await
        .expect("demote a response");
    assert_eq!(demote_a_resp.status(), StatusCode::NOT_FOUND);

    let promote_a = tenant_memory_request(
        "POST",
        "/memory/promote",
        "acme",
        "north",
        actor_id,
        Some(json!({
            "run_id": "tenant-a-memory-run",
            "source_memory_id": tenant_b_memory_id,
            "from_tier": "session",
            "to_tier": "project",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": project_id,
                "tier": "project"
            },
            "reason": "cross tenant promote attempt",
            "review": {
                "required": false,
                "reviewer_id": actor_id,
                "approval_id": "approval-a"
            },
            "capability": memory_capability(
                "tenant-a-memory-run",
                actor_id,
                "acme",
                "north",
                project_id
            )
        })),
    );
    let promote_a_resp = app
        .clone()
        .oneshot(promote_a)
        .await
        .expect("promote a response");
    assert_eq!(promote_a_resp.status(), StatusCode::OK);
    let promote_a_body = to_bytes(promote_a_resp.into_body(), usize::MAX)
        .await
        .expect("promote a body");
    let promote_a_payload: Value = serde_json::from_slice(&promote_a_body).expect("promote a json");
    assert_eq!(
        promote_a_payload.get("promoted").and_then(Value::as_bool),
        Some(false)
    );

    let list_b = tenant_memory_request(
        "GET",
        &format!("/memory?limit=20&user_id={actor_id}&project_id={project_id}"),
        "beta",
        "south",
        actor_id,
        None,
    );
    let list_b_resp = app.clone().oneshot(list_b).await.expect("list b response");
    assert_eq!(list_b_resp.status(), StatusCode::OK);
    let list_b_body = to_bytes(list_b_resp.into_body(), usize::MAX)
        .await
        .expect("list b body");
    let list_b_payload: Value = serde_json::from_slice(&list_b_body).expect("list b json");
    assert_eq!(list_b_payload.get("count").and_then(Value::as_u64), Some(1));
}

#[tokio::test]
async fn same_tenant_user_cannot_delete_or_demote_another_users_memory() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let project_id = "shared-project";

    // User A stores a private memory record in tenant acme/north.
    let put_a = tenant_memory_request(
        "POST",
        "/memory/put",
        "acme",
        "north",
        "user-a",
        Some(json!({
            "run_id": "user-a-memory-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": project_id,
                "tier": "session"
            },
            "kind": "fact",
            "content": "user a private memory phrase",
            "classification": "internal",
            "capability": memory_capability(
                "user-a-memory-run",
                "user-a",
                "acme",
                "north",
                project_id
            )
        })),
    );
    let put_a_resp = app.clone().oneshot(put_a).await.expect("put a response");
    assert_eq!(put_a_resp.status(), StatusCode::OK);
    let put_a_body = to_bytes(put_a_resp.into_body(), usize::MAX)
        .await
        .expect("put a body");
    let put_a_payload: Value = serde_json::from_slice(&put_a_body).expect("put a json");
    let memory_id = put_a_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("user a memory id")
        .to_string();

    // User B shares the tenant (acme/north) so the record is visible, but is a
    // different actor. Deleting user A's record must be forbidden, not silently
    // allowed (TAN-604).
    let delete_b = tenant_memory_request(
        "DELETE",
        &format!("/memory/{memory_id}?project_id={project_id}"),
        "acme",
        "north",
        "user-b",
        None,
    );
    let delete_b_resp = app
        .clone()
        .oneshot(delete_b)
        .await
        .expect("delete b response");
    assert_eq!(delete_b_resp.status(), StatusCode::FORBIDDEN);

    // Demote by another user in the same tenant is likewise forbidden.
    let demote_b = tenant_memory_request(
        "POST",
        "/memory/demote",
        "acme",
        "north",
        "user-b",
        Some(json!({
            "id": memory_id,
            "run_id": "user-b-memory-run"
        })),
    );
    let demote_b_resp = app
        .clone()
        .oneshot(demote_b)
        .await
        .expect("demote b response");
    assert_eq!(demote_b_resp.status(), StatusCode::FORBIDDEN);

    // Positive control: the owning user can still delete their own record.
    let delete_a = tenant_memory_request(
        "DELETE",
        &format!("/memory/{memory_id}?project_id={project_id}"),
        "acme",
        "north",
        "user-a",
        None,
    );
    let delete_a_resp = app
        .clone()
        .oneshot(delete_a)
        .await
        .expect("delete a response");
    assert_eq!(delete_a_resp.status(), StatusCode::OK);
}

#[test]
fn ingested_run_memory_owner_matches_mutation_guard_subject() {
    use super::super::skills_memory::ingested_memory_owner_subject;

    // Local-unrestricted: the ownership guard is bypassed, so run memory stays
    // owned by the run client id (matching the prompt-injection reader).
    let local = tandem_types::TenantContext::local_implicit();
    assert_eq!(
        ingested_memory_owner_subject(&local, None, "cli-client-7"),
        "cli-client-7"
    );

    // Governed (tenant actor present): own by the resolved actor subject, not the
    // run client id, so the same actor's delete/demote guard resolves to the same
    // value and they can manage their own ingested memory (TAN-604 follow-up).
    let governed = tandem_types::TenantContext::explicit_user_workspace(
        "acme",
        "north",
        Some("dep-a".to_string()),
        "user-a",
    );
    assert_eq!(
        ingested_memory_owner_subject(&governed, None, "cli-client-7"),
        "user-a"
    );
}

#[tokio::test]
async fn explicit_tenant_memory_put_rejects_partition_tenant_switch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let put_req = tenant_memory_request(
        "POST",
        "/memory/put",
        "acme",
        "north",
        "user-a",
        Some(json!({
            "run_id": "tenant-switch-memory-run",
            "partition": {
                "org_id": "beta",
                "workspace_id": "south",
                "project_id": "shared-project",
                "tier": "session"
            },
            "kind": "fact",
            "content": "attempted tenant switch",
            "classification": "internal",
            "capability": memory_capability(
                "tenant-switch-memory-run",
                "user-a",
                "beta",
                "south",
                "shared-project"
            )
        })),
    );
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn memory_list_uses_capability_subject_and_rejects_mismatched_user() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let capability = json!({
        "run_id": "tenant-list-run",
        "subject": "user-a",
        "org_id": "acme",
        "workspace_id": "north",
        "project_id": "proj-a",
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
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-a")
        .body(Body::from(
            json!({
                "run_id": "tenant-list-run",
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "tenant list record",
                "classification": "internal",
                "capability": capability
            })
            .to_string(),
        ))
        .expect("tenant list put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("tenant list put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?limit=20&user_id=user-a")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-a")
        .body(Body::empty())
        .expect("tenant list request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("tenant list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("tenant list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("tenant list json");
    let found = list_payload
        .get("items")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("run_id").and_then(Value::as_str) == Some("tenant-list-run")
                    && row.get("user_id").and_then(Value::as_str) == Some("user-a")
            })
        });
    assert!(found, "expected tenant-scoped memory item to be listed");

    let forbidden_req = Request::builder()
        .method("GET")
        .uri("/memory?limit=20&user_id=user-a")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-b")
        .body(Body::empty())
        .expect("forbidden list request");
    let forbidden_resp = app
        .clone()
        .oneshot(forbidden_req)
        .await
        .expect("forbidden list response");
    assert_eq!(forbidden_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn memory_put_without_capability_ignores_client_id_and_defaults_to_connected_actor() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "north")
        .header("x-tandem-actor-id", "user-a")
        .header("x-tandem-client-id", "forged-client")
        .body(Body::from(
            json!({
                "run_id": "actor-default-memory-run",
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "actor default memory should belong to user a",
                "classification": "internal"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let list_req = tenant_memory_request(
        "GET",
        "/memory?limit=20&user_id=user-a&project_id=proj-a",
        "acme",
        "north",
        "user-a",
        None,
    );
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    let found = list_payload
        .get("items")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("run_id").and_then(Value::as_str) == Some("actor-default-memory-run")
                    && row.get("user_id").and_then(Value::as_str) == Some("user-a")
            })
        });
    assert!(found, "default memory capability should use request actor");

    let forged_list_req = tenant_memory_request(
        "GET",
        "/memory?limit=20&user_id=forged-client&project_id=proj-a",
        "acme",
        "north",
        "forged-client",
        None,
    );
    let forged_list_resp = app
        .clone()
        .oneshot(forged_list_req)
        .await
        .expect("forged client list response");
    assert_eq!(forged_list_resp.status(), StatusCode::OK);
    let forged_list_body = to_bytes(forged_list_resp.into_body(), usize::MAX)
        .await
        .expect("forged list body");
    let forged_list_payload: Value =
        serde_json::from_slice(&forged_list_body).expect("forged list json");
    let forged_serialized =
        serde_json::to_string(&forged_list_payload).expect("forged list payload");
    assert!(
        !forged_serialized.contains("actor default memory should belong to user a"),
        "client id should not become the memory subject"
    );
}

#[tokio::test]
async fn memory_put_rejects_capability_subject_actor_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = tenant_memory_request(
        "POST",
        "/memory/put",
        "acme",
        "north",
        "user-a",
        Some(json!({
            "run_id": "forged-subject-put-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": "proj-a",
                "tier": "session"
            },
            "kind": "fact",
            "content": "forged subject write should be blocked",
            "classification": "internal",
            "capability": memory_capability(
                "forged-subject-put-run",
                "user-b",
                "acme",
                "north",
                "proj-a"
            )
        })),
    );
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);

    let list_req = tenant_memory_request(
        "GET",
        "/memory?limit=20&user_id=user-b&project_id=proj-a",
        "acme",
        "north",
        "user-b",
        None,
    );
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    let serialized = serde_json::to_string(&list_payload).expect("list payload");
    assert!(!serialized.contains("forged subject write should be blocked"));
}

#[tokio::test]
async fn memory_put_rejects_channel_capability_subject_actor_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = tenant_memory_request(
        "POST",
        "/memory/put",
        "acme",
        "north",
        "user-a",
        Some(json!({
            "run_id": "forged-channel-subject-put-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": "proj-a",
                "tier": "session"
            },
            "kind": "fact",
            "content": "forged channel subject write should be blocked",
            "classification": "internal",
            "capability": memory_capability(
                "forged-channel-subject-put-run",
                "channel:slack:U999",
                "acme",
                "north",
                "proj-a"
            )
        })),
    );
    let put_resp = app.oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn verified_delegate_memory_put_accepts_delegate_subject() {
    let state = test_state().await;
    let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
        "acme",
        "north",
        Some("dep-a".to_string()),
        "user-a",
    );
    let verified = verified_delegate_context(tenant_context.clone(), "a2a-worker-1");
    let partition = tandem_memory::MemoryPartition {
        org_id: "acme".to_string(),
        workspace_id: "north".to_string(),
        project_id: "proj-a".to_string(),
        tier: tandem_memory::GovernedMemoryTier::Session,
    };
    let capability = tandem_memory::MemoryCapabilityToken {
        run_id: "delegate-memory-put-run".to_string(),
        subject: "a2a-worker-1".to_string(),
        org_id: "acme".to_string(),
        workspace_id: "north".to_string(),
        project_id: "proj-a".to_string(),
        memory: tandem_memory::MemoryCapabilities::default(),
        expires_at: 9_999_999_999_999,
    };

    let response = super::super::skills_memory::memory_put_impl_with_verified(
        &state,
        &tenant_context,
        Some(&verified),
        tandem_memory::MemoryPutRequest {
            run_id: "delegate-memory-put-run".to_string(),
            partition: partition.clone(),
            kind: tandem_memory::MemoryContentKind::Fact,
            content: "delegate scoped memory should store".to_string(),
            artifact_refs: Vec::new(),
            classification: tandem_memory::MemoryClassification::Internal,
            authority_job_context: None,
            metadata: None,
        },
        Some(capability),
    )
    .await
    .expect("verified delegate subject should be accepted");

    assert!(response.stored);
    let db = super::super::skills_memory::open_global_memory_db_for_state(&state)
        .await
        .expect("memory db");
    let rows = db
        .list_global_memory_for_tenant(
            "acme",
            "north",
            Some("dep-a"),
            "a2a-worker-1",
            Some("delegate scoped memory"),
            Some("proj-a"),
            None,
            20,
            0,
        )
        .await
        .expect("list delegate memory");
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn memory_search_rejects_capability_subject_actor_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_b = tenant_memory_request(
        "POST",
        "/memory/put",
        "acme",
        "north",
        "user-b",
        Some(json!({
            "run_id": "subject-b-memory-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": "proj-a",
                "tier": "session"
            },
            "kind": "fact",
            "content": "forged subject private memory phrase",
            "classification": "internal",
            "capability": memory_capability(
                "subject-b-memory-run",
                "user-b",
                "acme",
                "north",
                "proj-a"
            )
        })),
    );
    let put_b_resp = app.clone().oneshot(put_b).await.expect("put b response");
    assert_eq!(put_b_resp.status(), StatusCode::OK);

    let forged_search = tenant_memory_request(
        "POST",
        "/memory/search",
        "acme",
        "north",
        "user-a",
        Some(json!({
            "run_id": "forged-subject-search-run",
            "partition": {
                "org_id": "acme",
                "workspace_id": "north",
                "project_id": "proj-a",
                "tier": "session"
            },
            "query": "forged subject private memory phrase",
            "read_scopes": ["session", "project"],
            "limit": 10,
            "capability": memory_capability(
                "forged-subject-search-run",
                "user-b",
                "acme",
                "north",
                "proj-a"
            )
        })),
    );
    let search_resp = app
        .clone()
        .oneshot(forged_search)
        .await
        .expect("forged search response");
    assert_eq!(search_resp.status(), StatusCode::FORBIDDEN);

    let audit_req = tenant_memory_request(
        "GET",
        "/memory/audit?run_id=forged-subject-search-run",
        "acme",
        "north",
        "user-a",
        None,
    );
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
                    .is_some_and(|detail| detail.contains("capability subject actor mismatch"))
        })));
}
