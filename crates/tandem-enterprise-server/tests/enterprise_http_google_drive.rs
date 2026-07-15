// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#![allow(unused_imports, dead_code)]
// EAA-10 (TAN-35): enterprise HTTP integration tests, owned by the crate that
// owns the enterprise routes. The router under test is built from tandem-server's
// base router with `tandem_enterprise_server::apply_routes` mounted as a route
// extension — the same wiring `serve_with_route_extensions` uses in production.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use tandem_enterprise_server::apply_routes;
use tandem_memory::response_cache::ResponseCacheScope;
use tandem_memory::types::{
    MemoryChunk, MemoryTenantScope, MemoryTier, SourceObjectLifecycleRecord,
    SourceObjectLifecycleState, DEFAULT_EMBEDDING_DIMENSION,
};
use tandem_server::build_router_with_extensions;
use tandem_server::test_support::{next_event_of_type, test_state};

mod common;
use common::connector_body;

#[tokio::test]
async fn enterprise_google_drive_rejects_write_credentials() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "credential_id": "drive-write",
                "credential_class": "read_write",
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "google_kms",
                    "secret_id": "kms://finance/write",
                    "name": "Finance Drive write secret"
                },
                "source_bound_resource": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-drive"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_GOOGLE_DRIVE_READ_ONLY_CREDENTIAL_REQUIRED")
    );
}

#[tokio::test]
#[serial_test::serial]
async fn enterprise_google_drive_preflight_lists_bound_folder_without_exposing_token() {
    let drive_base_url = spawn_enterprise_google_drive_fixture().await;
    std::env::set_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL", &drive_base_url);
    std::env::set_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN", "route-drive-token");

    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "credential_id": "drive-readonly",
                "credential_class": "read_only",
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "env",
                    "secret_id": "env://TANDEM_TEST_ROUTE_DRIVE_TOKEN",
                    "name": "Local Drive token"
                },
                "source_bound_resource": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-drive"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "binding_id": "finance-drive",
                "connector_id": "google_drive",
                "source_type": "google_drive",
                "native_source_id": "drive-folder-123",
                "source_root_label": "Finance Drive",
                "credential_ref_id": "drive-readonly",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-drive"
                },
                "data_class": "financial_record"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings/finance-drive/google-drive/preflight")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/preflight/binding_id")
            .and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        payload
            .pointer("/preflight/file_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    let encoded = serde_json::to_string(&payload).expect("encoded response");
    assert!(!encoded.contains("route-drive-token"));
    assert!(!encoded.contains("TANDEM_TEST_ROUTE_DRIVE_TOKEN"));

    std::env::remove_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL");
    std::env::remove_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN");
}

#[tokio::test]
#[ignore = "TAN-35: requires the local embedding stack (MemoryManager::new -> EmbeddingService::new) which is unavailable without the local-embeddings feature; run with --features full"]
#[serial_test::serial]
async fn enterprise_google_drive_import_records_quarantined_source_objects() {
    let drive_base_url = spawn_enterprise_google_drive_fixture().await;
    std::env::set_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL", &drive_base_url);
    std::env::set_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN", "route-drive-token");

    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let binding_id = "finance-drive-quarantine-import";

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "credential_id": "drive-readonly",
                "credential_class": "read_only",
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "env",
                    "secret_id": "env://TANDEM_TEST_ROUTE_DRIVE_TOKEN",
                    "name": "Local Drive token"
                },
                "source_bound_resource": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": binding_id
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "binding_id": binding_id,
                "connector_id": "google_drive",
                "source_type": "google_drive",
                "native_source_id": "drive-folder-123",
                "source_root_label": "Finance Drive",
                "credential_ref_id": "drive-readonly",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": binding_id
                },
                "data_class": "financial_record",
                "ingestion_policy": {
                    "allow_indexing": true,
                    "allow_prompt_context": true,
                    "require_review": true
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/google-drive/import"
        ))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(json!({"tier": "global"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/ingestion_job/state")
            .and_then(Value::as_str),
        Some("quarantined")
    );
    assert_eq!(
        payload.get("drive_files_fetched").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        payload
            .pointer("/stats/indexed_files")
            .and_then(Value::as_u64),
        Some(1)
    );
    let quarantine_id = payload
        .pointer("/ingestion_job/quarantine_id")
        .and_then(Value::as_str)
        .expect("quarantine id")
        .to_string();
    let encoded = serde_json::to_string(&payload).expect("encoded response");
    assert!(!encoded.contains("route-drive-token"));
    assert!(!encoded.contains("TANDEM_TEST_ROUTE_DRIVE_TOKEN"));

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let source_object = payload
        .get("source_objects")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .expect("source object");
    assert_eq!(
        source_object.get("state").and_then(Value::as_str),
        Some("quarantined")
    );
    assert_eq!(
        source_object.get("connector_id").and_then(Value::as_str),
        Some("google_drive")
    );
    assert_eq!(
        source_object.get("data_class").and_then(Value::as_str),
        Some("financial_record")
    );
    assert_eq!(
        source_object
            .get("resource_ref")
            .and_then(|resource| resource.get("resource_id"))
            .and_then(Value::as_str),
        Some(binding_id)
    );

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/enterprise/ingestion-jobs?binding_id={binding_id}"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("ingestion_jobs")
        .and_then(Value::as_array)
        .is_some_and(|jobs| jobs
            .iter()
            .any(|job| job.get("state").and_then(Value::as_str) == Some("quarantined"))));

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/enterprise/ingestion-quarantines?binding_id={binding_id}"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("quarantines")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("quarantine_id"))
            .and_then(Value::as_str),
        Some(quarantine_id.as_str())
    );

    std::env::remove_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL");
    std::env::remove_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN");
}

#[tokio::test]
#[ignore = "TAN-35: requires the local embedding stack (MemoryManager::new -> EmbeddingService::new) which is unavailable without the local-embeddings feature; run with --features full"]
#[serial_test::serial]
async fn enterprise_google_drive_reindex_refetches_existing_binding_without_exposing_token() {
    let drive_base_url = spawn_enterprise_google_drive_fixture().await;
    std::env::set_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL", &drive_base_url);
    std::env::set_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN", "route-drive-token");

    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "credential_id": "drive-readonly",
                "credential_class": "read_only",
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "env",
                    "secret_id": "env://TANDEM_TEST_ROUTE_DRIVE_TOKEN",
                    "name": "Local Drive token"
                },
                "source_bound_resource": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-drive"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "binding_id": "finance-drive",
                "connector_id": "google_drive",
                "source_type": "google_drive",
                "native_source_id": "drive-folder-123",
                "source_root_label": "Finance Drive",
                "credential_ref_id": "drive-readonly",
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
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
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings/finance-drive/google-drive/import")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(json!({"tier": "global"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings/finance-drive/google-drive/reindex")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({"tier": "global", "sync_deletes": true}).to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/ingestion_job/state")
            .and_then(Value::as_str),
        Some("completed")
    );
    assert!(payload
        .pointer("/ingestion_job/job_id")
        .and_then(Value::as_str)
        .is_some_and(|job_id| job_id.starts_with("google-drive-reindex-")));
    assert_eq!(
        payload.get("drive_files_fetched").and_then(Value::as_u64),
        Some(1)
    );
    let encoded = serde_json::to_string(&payload).expect("encoded response");
    assert!(!encoded.contains("route-drive-token"));
    assert!(!encoded.contains("TANDEM_TEST_ROUTE_DRIVE_TOKEN"));

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/ingestion-jobs?binding_id=finance-drive")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("ingestion_jobs")
        .and_then(Value::as_array)
        .is_some_and(|jobs| jobs.iter().any(|job| job
            .get("job_id")
            .and_then(Value::as_str)
            .is_some_and(|job_id| job_id.starts_with("google-drive-reindex-")))));

    std::env::remove_var("TANDEM_GOOGLE_DRIVE_API_BASE_URL");
    std::env::remove_var("TANDEM_TEST_ROUTE_DRIVE_TOKEN");
}

async fn spawn_enterprise_google_drive_fixture() -> String {
    let app = axum::Router::new()
        .route(
            "/drive/v3/files",
            axum::routing::get(enterprise_google_drive_list_fixture),
        )
        .route(
            "/drive/v3/files/{file_id}",
            axum::routing::get(enterprise_google_drive_download_fixture),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind drive fixture");
    let addr = listener.local_addr().expect("drive fixture addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("drive fixture server");
    });
    format!("http://{addr}")
}

async fn enterprise_google_drive_list_fixture(
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::Json<Value> {
    assert_eq!(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer route-drive-token")
    );
    assert_eq!(
        query.get("q").map(String::as_str),
        Some("'drive-folder-123' in parents and trashed = false")
    );
    if query.get("pageToken").is_some() {
        return axum::Json(json!({
            "files": []
        }));
    }
    axum::Json(json!({
        "nextPageToken": "next-token",
        "files": [{
            "id": "drive-file-1",
            "name": "Board packet.txt",
            "mimeType": "text/plain"
        }]
    }))
}

async fn enterprise_google_drive_download_fixture(
    headers: axum::http::HeaderMap,
    axum::extract::Path(file_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    assert_eq!(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer route-drive-token")
    );
    assert_eq!(file_id, "drive-file-1");
    assert_eq!(query.get("alt").map(String::as_str), Some("media"));
    axum::response::IntoResponse::into_response(
        "Quarterly finance source-bound drive packet for import.",
    )
}
