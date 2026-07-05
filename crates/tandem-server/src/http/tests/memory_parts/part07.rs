#[tokio::test]
async fn context_tree_endpoints_are_tenant_scoped() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // Seed a context node + L2 layer for tenant org-a directly through the
    // memory manager (nodes have no HTTP creation surface).
    if let Some(parent) = state.memory_db_path.parent() {
        tokio::fs::create_dir_all(parent).await.expect("memory dir");
    }
    let manager = tandem_memory::MemoryManager::new(&state.memory_db_path)
        .await
        .expect("memory manager");
    let scope_a = tandem_memory::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let node_id = manager
        .store_content_with_layers(
            "tandem://resources/proj-a/secret.md",
            "tenant A secret context",
            None,
            &scope_a,
        )
        .await
        .expect("seed node");

    let resolve_request = |org: &str, workspace: &str| {
        Request::builder()
            .method("POST")
            .uri("/memory/context/resolve")
            .header("content-type", "application/json")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", "actor-a")
            .body(Body::from(
                json!({ "uri": "tandem://resources/proj-a/secret.md" }).to_string(),
            ))
            .expect("request")
    };

    // The owning tenant resolves the node.
    let resp = app
        .clone()
        .oneshot(resolve_request("org-a", "workspace-a"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.pointer("/node/id").and_then(Value::as_str),
        Some(node_id.as_str())
    );

    // A different tenant gets the same shape as a nonexistent URI: node null.
    let resp = app
        .clone()
        .oneshot(resolve_request("org-b", "workspace-b"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.get("node").is_some_and(Value::is_null));

    // Tree traversal: owner sees the child; foreign tenant sees an empty tree.
    let tree_request = |org: &str, workspace: &str| {
        Request::builder()
            .method("GET")
            .uri("/memory/context/tree?uri=tandem%3A%2F%2Fresources%2Fproj-a&max_depth=3")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", "actor-a")
            .body(Body::empty())
            .expect("request")
    };
    let resp = app
        .clone()
        .oneshot(tree_request("org-a", "workspace-a"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/tree")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let resp = app
        .clone()
        .oneshot(tree_request("org-b", "workspace-b"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/tree")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    // Layer generation on a foreign node id behaves exactly like a missing
    // node id: 200 no-op, and the owner's layer set is untouched.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory/context/layers/generate")
                .header("content-type", "application/json")
                .header("x-tandem-org-id", "org-b")
                .header("x-tandem-workspace-id", "workspace-b")
                .header("x-tandem-actor-id", "actor-a")
                .body(Body::from(json!({ "node_id": node_id }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let scope_b = tandem_memory::types::MemoryTenantScope {
        org_id: "org-b".to_string(),
        workspace_id: "workspace-b".to_string(),
        deployment_id: None,
    };
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L0, &scope_b)
        .await
        .expect("layer lookup")
        .is_none());
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L0, &scope_a)
        .await
        .expect("layer lookup")
        .is_none());
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L2, &scope_a)
        .await
        .expect("layer lookup")
        .is_some());
}
