// Memory isolation matrix (TAN-608): consolidated regressions for the axes the
// Memory Isolation & Scoping project fixed. Each test names the axis it pins.
// Sibling coverage (do not duplicate here):
// - tenant axis: part04 `tenant_a_cannot_*`, db_tests_a/b partition tests
// - org-unit axis: part07 org-unit tests + governed_read_tests (TAN-605)
// - vector-chunk subject axis: manager_parts/part02 + governed_read_tests (TAN-607)
// - context-tree tenancy: part07 `context_tree_endpoints_are_tenant_scoped` (TAN-602)
// - cross-user demote/delete authz: part04 (TAN-604)

/// Axis: private user × records-store RECALL (TAN-601/TAN-608a). Two users in
/// one tenant; each user's explicitly private /memory/search recall must exclude
/// the other's records, not merely reject unauthorized operations.
#[tokio::test]
async fn matrix_records_recall_is_subject_scoped_within_tenant() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put = |actor: &str, run_id: &str, content: &str| {
        tenant_memory_request(
            "POST",
            "/memory/put",
            "acme",
            "north",
            actor,
            Some(json!({
                "run_id": run_id,
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "note",
                "content": content,
                "classification": "internal",
                "private": true,
                "capability": memory_capability(run_id, actor, "acme", "north", "proj-a")
            })),
        )
    };
    let resp = app
        .clone()
        .oneshot(put(
            "user-a",
            "matrix-recall-a",
            "alpha roadmap milestones for launch readiness",
        ))
        .await
        .expect("put a");
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(put(
            "user-b",
            "matrix-recall-b",
            "bravo budget forecast for launch readiness",
        ))
        .await
        .expect("put b");
    assert_eq!(resp.status(), StatusCode::OK);

    let search = |actor: &str, run_id: &str, query: &str| {
        tenant_memory_request(
            "POST",
            "/memory/search",
            "acme",
            "north",
            actor,
            Some(json!({
                "run_id": run_id,
                "query": query,
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "limit": 10,
                "capability": memory_capability(run_id, actor, "acme", "north", "proj-a")
            })),
        )
    };

    // User A searching a phrase from B's record must not see it, even though
    // both records share the tenant, workspace, project, and tier.
    let resp = app
        .clone()
        .oneshot(search(
            "user-a",
            "matrix-recall-a",
            "launch readiness forecast",
        ))
        .await
        .expect("search as a");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        results
            .iter()
            .all(|row| !row.to_string().contains("bravo budget")),
        "user A recall must exclude user B records: {results:?}"
    );
    assert!(
        results.iter().any(|row| row.to_string().contains("alpha")),
        "user A still recalls their own record"
    );

    // And symmetrically for user B.
    let resp = app
        .oneshot(search(
            "user-b",
            "matrix-recall-b",
            "launch readiness milestones",
        ))
        .await
        .expect("search as b");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(results
        .iter()
        .all(|row| !row.to_string().contains("alpha roadmap")));
}

/// Axis: tenant × ingest partitioning (TAN-606/TAN-608e). A governed write
/// under tenant T lands in T's partition and never in the shared local/local
/// pool.
#[tokio::test]
async fn matrix_governed_ingest_never_lands_in_local_partition() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let resp = app
        .oneshot(tenant_memory_request(
            "POST",
            "/memory/put",
            "acme",
            "north",
            "user-a",
            Some(json!({
                "run_id": "matrix-ingest-1",
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "note",
                "content": "governed ingest partition sentinel",
                "classification": "internal",
                "capability": memory_capability(
                    "matrix-ingest-1",
                    "user-a",
                    "acme",
                    "north",
                    "proj-a"
                )
            })),
        ))
        .await
        .expect("put");
    assert_eq!(resp.status(), StatusCode::OK);

    let db = open_global_memory_db_for_state_test(&state).await;

    // Visible through the tenant-scoped read...
    let tenant_rows = db
        .list_global_memory_for_tenant(
            "acme",
            "north",
            None,
            "user-a",
            Some("partition sentinel"),
            Some("proj-a"),
            None,
            20,
            0,
        )
        .await
        .expect("tenant rows");
    assert_eq!(tenant_rows.len(), 1);

    // ...and absent from the local/local partition under any subject probe.
    let local_rows = db
        .list_global_memory_for_tenant(
            "local",
            "local",
            None,
            "user-a",
            Some("partition sentinel"),
            None,
            None,
            20,
            0,
        )
        .await
        .expect("local rows");
    assert!(
        local_rows.is_empty(),
        "governed ingest must never land in local/local"
    );
}

async fn open_global_memory_db_for_state_test(
    state: &AppState,
) -> tandem_memory::db::MemoryDatabase {
    tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory db")
}

/// Axis: tenant × distillation (TAN-608f). Distillation has no private opt-in,
/// so its output is shared within the writing tenant and excluded from others.
#[tokio::test]
async fn matrix_distillation_output_scoped_to_writing_tenant() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let provider_app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "choices": [
                    {
                        "message": {
                            "content": r#"[{"category":"fact","content":"Tenant A prefers metric dashboards refreshed hourly.","importance":0.9,"follow_up_needed":false}]"#
                        }
                    }
                ]
            }))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, provider_app)
            .await
            .expect("serve test provider");
    });

    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "default_provider": "openai",
            "providers": {
                "openai": {
                    "url": format!("http://{addr}/v1")
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .auth
        .write()
        .await
        .insert("openai".to_string(), "test-key".to_string());
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state.clone());
    let resp = app
        .oneshot(tenant_memory_request(
            "POST",
            "/memory/context/distill",
            "acme",
            "north",
            "user-a",
            Some(json!({
                "session_id": "matrix-distill-session",
                "conversation": [
                    "user: our operations team needs the metric dashboards to stay fresh because stale numbers keep causing bad rollout decisions during weekly planning reviews",
                    "assistant: understood — I will refresh the metric dashboards hourly, annotate each panel with its last-updated timestamp, and flag any feed that lags behind the hourly cadence",
                    "user: also make sure the refresh preference is remembered for future reporting sessions so we never have to repeat this setup conversation again",
                    "assistant: noted as a durable preference: tenant A prefers metric dashboards refreshed hourly with visible freshness timestamps on every reporting surface"
                ],
                "project_id": "proj-a"
            })),
        ))
        .await
        .expect("distill response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(
        payload.get("stored_count").and_then(Value::as_u64) >= Some(1),
        "distillation must store at least one fact: {payload}"
    );

    let db = open_global_memory_db_for_state_test(&state).await;
    // The writing subject in the writing tenant sees the distilled fact.
    let own = db
        .search_global_memory_for_tenant(
            "acme",
            "north",
            None,
            "user-a",
            "metric dashboards",
            10,
            None,
            None,
            None,
        )
        .await
        .expect("own search");
    assert!(
        own.iter()
            .any(|hit| hit.record.content.contains("metric dashboards")),
        "writer recalls their distilled fact"
    );
    // With no private opt-in, another subject in the same tenant also sees it.
    let sibling = db
        .search_global_memory_for_tenant(
            "acme",
            "north",
            None,
            "user-b",
            "metric dashboards",
            10,
            None,
            None,
            None,
        )
        .await
        .expect("sibling search");
    assert!(
        sibling
            .iter()
            .any(|hit| hit.record.content.contains("metric dashboards")),
        "non-private distilled facts are tenant-shared"
    );
    // Another tenant does not.
    let foreign = db
        .search_global_memory_for_tenant(
            "globex",
            "south",
            None,
            "user-a",
            "metric dashboards",
            10,
            None,
            None,
            None,
        )
        .await
        .expect("foreign search");
    assert!(foreign.is_empty(), "distilled facts must not cross tenants");

    server.abort();
}
