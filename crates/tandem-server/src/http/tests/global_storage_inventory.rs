// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{extract::Extension, http::StatusCode, routing::get, Router};

use super::legacy_routine_authority::{hosted_state, tenant, verified};

const ABSENT_PATH: &str = "/global/storage/files?path=__tandem_inventory_absent__";

fn locality(proxied: bool) -> super::super::host_authority::RequestLocality {
    let mut headers = axum::http::HeaderMap::new();
    if proxied {
        headers.insert("forwarded", "for=127.0.0.1".parse().unwrap());
    }
    super::super::host_authority::RequestLocality::from_peer_and_headers(
        Some(std::net::SocketAddr::from(([127, 0, 0, 1], 43123))),
        &headers,
    )
}

fn inventory_router() -> Router<AppState> {
    Router::new().route(
        "/global/storage/files",
        get(crate::http::global::global_storage_files),
    )
}

fn hosted_app(state: &AppState, actor: &str, role: &str) -> Router {
    let mut identity = verified(actor, role);
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted actor");
    inventory_router()
        .layer(Extension(tenant(actor)))
        .layer(Extension(locality(false)))
        .layer(Extension(identity))
        .with_state(state.clone())
}

async fn status(app: Router) -> StatusCode {
    app.oneshot(
        Request::builder()
            .uri(ABSENT_PATH)
            .body(Body::empty())
            .expect("inventory request"),
    )
    .await
    .expect("inventory response")
    .status()
}

#[tokio::test]
async fn storage_inventory_requires_current_hosted_admin_before_path_lookup() {
    let (state, policy_dir) = hosted_state().await;
    assert_eq!(
        status(hosted_app(&state, "alice", "member")).await,
        StatusCode::FORBIDDEN
    );

    let admin_app = hosted_app(&state, "admin", "admin");
    assert_eq!(status(admin_app.clone()).await, StatusCode::NOT_FOUND);

    let policy_path = policy_dir.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("policy file"))
            .expect("policy JSON");
    policy["policy_version"] = json!(2);
    let admin = policy["users"]
        .as_array_mut()
        .expect("policy users")
        .iter_mut()
        .find(|user| user["id"] == "admin")
        .expect("admin user");
    admin["is_active"] = json!(false);
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&policy).expect("updated policy JSON"),
    )
    .expect("update test policy");
    state
        .reload_hosted_policy()
        .await
        .expect("reload revoked policy");
    assert_eq!(status(admin_app).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn storage_inventory_local_access_requires_direct_unproxied_loopback() {
    let state = test_state().await;
    let direct = inventory_router()
        .layer(Extension(TenantContext::local_implicit()))
        .layer(Extension(locality(false)))
        .with_state(state.clone());
    assert_eq!(status(direct).await, StatusCode::NOT_FOUND);

    let proxied = inventory_router()
        .layer(Extension(TenantContext::local_implicit()))
        .layer(Extension(locality(true)))
        .with_state(state);
    assert_eq!(status(proxied).await, StatusCode::FORBIDDEN);
}
