use super::*;
use tandem_memory::types::{
    MemoryTenantScope, MemoryTier, SourceObjectLifecycleRecord, SourceObjectLifecycleState,
};

#[tokio::test]
async fn enterprise_status_returns_public_safe_summary() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/status")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("mode").and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("absent")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("source"))
            .and_then(Value::as_str),
        Some("local_implicit")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("local")
    );
    assert!(payload
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|caps| !caps.is_empty()));
}

#[tokio::test]
async fn enterprise_org_units_storage_threads_request_tenant() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "admin-user")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("ok"));
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("storage_backed")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("clinic-co")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("workspace_id"))
            .and_then(Value::as_str),
        Some("care-delivery")
    );
    assert_eq!(
        payload
            .get("request_principal")
            .and_then(|row| row.get("actor_id"))
            .and_then(Value::as_str),
        Some("admin-user")
    );
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn enterprise_org_units_create_persists_under_request_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise_org_units_path.clone();
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "unit_id": "hr",
                "taxonomy_id": "department",
                "display_name": "Human Resources",
                "kind": "department",
                "labels": ["people", "benefits"]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .and_then(|units| units.first())
            .and_then(|unit| unit.get("tenant_context"))
            .and_then(|tenant| tenant.get("org_id"))
            .and_then(Value::as_str),
        Some("clinic-co")
    );
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .and_then(|units| units.first())
            .and_then(|unit| unit.get("taxonomy_id"))
            .and_then(Value::as_str),
        Some("department")
    );
}

#[tokio::test]
async fn enterprise_org_units_do_not_cross_tenant_boundaries() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::from(
            json!({
                "unit_id": "executive",
                "display_name": "Executive",
                "kind": "executive_group"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn enterprise_source_bindings_storage_threads_request_tenant() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("ok"));
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("storage_backed")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );
    assert_eq!(
        payload
            .get("source_bindings")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

fn source_binding_body(binding_id: &str, org_id: &str, workspace_id: &str) -> String {
    json!({
        "binding_id": binding_id,
        "connector_id": "google_drive",
        "source_type": "google_drive",
        "native_source_id": "drive-root-123",
        "source_root_label": "Finance Drive",
        "resource_ref": {
            "organization_id": org_id,
            "workspace_id": workspace_id,
            "resource_kind": "document_collection",
            "resource_id": "finance-drive"
        },
        "data_class": "financial_record",
        "ingestion_policy": {
            "allow_indexing": true,
            "allow_prompt_context": true,
            "require_review": false
        }
    })
    .to_string()
}

#[tokio::test]
async fn enterprise_source_bindings_create_and_update_persist_under_request_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise_source_bindings_path.clone();
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());
    let create_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        create_event
            .properties
            .get("binding_id")
            .and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        create_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_binding_created")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "state": "disabled",
                "ingestion_policy": {
                    "allow_indexing": false,
                    "allow_prompt_context": false,
                    "require_review": true
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let update_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        update_event
            .properties
            .get("binding_id")
            .and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        update_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_binding_updated")
    );
    assert_eq!(
        update_event
            .properties
            .get("cache_scope")
            .and_then(|scope| scope.get("tenant_org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    let binding = payload
        .get("source_bindings")
        .and_then(Value::as_array)
        .and_then(|bindings| bindings.first())
        .expect("binding");
    assert_eq!(
        binding.get("binding_id").and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        binding.get("state").and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        binding
            .get("ingestion_policy")
            .and_then(|policy| policy.get("allow_indexing"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn enterprise_source_bindings_reject_cross_tenant_resource_ref() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(source_binding_body(
            "wrong-tenant",
            "other-co",
            "finance",
        )))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH")
    );
}

#[tokio::test]
async fn enterprise_source_bindings_do_not_cross_tenant_boundaries() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(json!({"state": "disabled"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enterprise_source_object_lifecycle_actions_are_admin_gated_and_tenant_scoped() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;

    let tenant_scope = MemoryTenantScope {
        org_id: "acme".to_string(),
        workspace_id: "finance".to_string(),
        deployment_id: None,
    };
    let paths = tandem_core::resolve_shared_paths().expect("shared paths");
    let db = tandem_memory::db::MemoryDatabase::new(&paths.memory_db_path)
        .await
        .expect("memory db");
    db.upsert_source_object_active_for_tenant(&SourceObjectLifecycleRecord {
        source_object_id: "source-object-finance-note".to_string(),
        tenant_scope: tenant_scope.clone(),
        source_binding_id: "finance-drive".to_string(),
        connector_id: "manual-upload".to_string(),
        state: SourceObjectLifecycleState::Active,
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        import_namespace: "import-test".to_string(),
        indexed_path: "import-test/note.md".to_string(),
        native_object_id: "import-test/note.md".to_string(),
        resource_ref: json!({
            "organization_id": "acme",
            "workspace_id": "finance",
            "resource_kind": "document_collection",
            "resource_id": "finance-drive"
        }),
        data_class: "financial_record".to_string(),
        content_hash: Some("content-1".to_string()),
        source_hash: Some("source-1".to_string()),
        first_seen_at_ms: 1_000,
        last_seen_at_ms: 1_000,
        tombstoned_at_ms: None,
        metadata: None,
    })
    .await
    .expect("seed lifecycle");

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings/finance-drive/source-objects")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));

    let req = Request::builder()
        .method("DELETE")
        .uri("/enterprise/source-bindings/finance-drive/source-objects/source-object-finance-note")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings/finance-drive/source-objects/source-object-finance-note/reindex")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("reindex_requested")
    );
    let reindex_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        reindex_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_object_reindex_requested")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive/source-objects/source-object-finance-note/scope")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(
            json!({
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-archive"
                },
                "data_class": "internal"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("rescoped")
    );
    assert_eq!(
        payload
            .get("source_object")
            .and_then(|object| object.get("state"))
            .and_then(Value::as_str),
        Some("rescoped")
    );
    assert_eq!(
        payload
            .get("source_object")
            .and_then(|object| object.get("data_class"))
            .and_then(Value::as_str),
        Some("internal")
    );
    let rescope_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        rescope_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_object_rescoped")
    );

    let req = Request::builder()
        .method("DELETE")
        .uri("/enterprise/source-bindings/finance-drive/source-objects/source-object-finance-note")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("deleted")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings/finance-drive/source-objects")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}
