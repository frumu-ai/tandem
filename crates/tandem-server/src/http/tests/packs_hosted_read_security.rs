// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tower::ServiceExt;

fn pack_read_router(
    state: AppState,
    tenant: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
) -> Router {
    let mut router = Router::<AppState>::new()
        .route("/packs", get(packs_list))
        .route("/packs/{selector}", get(packs_get))
        .route("/packs/{selector}/files/{*path}", get(packs_file_get))
        .route("/packs/{selector}/updates", get(packs_updates_get))
        .route("/packs/{selector}/update", post(packs_update_post))
        .layer(Extension(tenant));
    if let Some(verified) = verified {
        router = router.layer(Extension(verified));
    }
    router.with_state(state)
}

fn verified_actor(tenant: TenantContext) -> tandem_types::VerifiedTenantContext {
    use tandem_types::{
        AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
    };

    let now = crate::now_ms();
    let claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 300_000,
        "pack-read-bob",
        tenant,
        HumanActor::tandem_user("bob"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("bob", "tandem-web")),
        vec!["hosted:role:member".to_string()],
    );
    tandem_types::VerifiedTenantContext::from(claims)
}

async fn pack_read_request(app: Router, method: &str, uri: &str) -> (StatusCode, Vec<u8>) {
    let body = if method == "POST" {
        Body::from("{}")
    } else {
        Body::empty()
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .expect("pack request");
    let response = app.oneshot(request).await.expect("pack response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("pack response body");
    (status, bytes.to_vec())
}

async fn seeded_private_pack_state(root: &std::path::Path) -> AppState {
    let mut state = crate::test_support::test_state().await;
    let pack_root = root.join("packs");
    let install_path = pack_root.join("private-workflow").join("0.1.0");
    let bundle_path = install_path.join("workflows/alice/plan-package.json");
    std::fs::create_dir_all(bundle_path.parent().expect("bundle parent"))
        .expect("installed pack directory");
    std::fs::write(
        install_path.join("tandempack.yaml"),
        "name: private-workflow\nversion: 0.1.0\ntype: workflow\npack_id: private-workflow\ncontents:\n  workflows:\n    - id: alice\n      path: workflows/alice/plan-package.json\n      format: workflow_plan_bundle\n",
    )
    .expect("installed pack manifest");
    std::fs::write(&bundle_path, b"alice-private-plan-bundle")
        .expect("installed private workflow bundle");
    let record: crate::pack_manager::PackInstallRecord = serde_json::from_value(json!({
        "pack_id": "private-workflow",
        "name": "private-workflow",
        "version": "0.1.0",
        "pack_type": "workflow",
        "install_path": install_path.to_string_lossy(),
        "sha256": "private-test-digest",
        "solution_content_sha256": null,
        "installed_at_ms": 1,
        "source": {"kind": "workflow_pack_import", "path": "/private/alice.zip"},
        "marker_detected": true,
        "routines_enabled": false,
    }))
    .expect("deserialize private test pack");
    let index = crate::pack_manager::PackIndex {
        packs: vec![record],
    };
    std::fs::write(
        pack_root.join("index.json"),
        serde_json::to_vec(&index).expect("serialize pack index"),
    )
    .expect("pack index");
    state.pack_manager = Arc::new(crate::pack_manager::PackManager::new(pack_root));
    state
}

#[tokio::test]
async fn hosted_actor_cannot_enumerate_or_read_shared_installed_pack() {
    let root = tempfile::tempdir().expect("temporary pack root");
    let state = seeded_private_pack_state(root.path()).await;
    assert_eq!(
        state.pack_manager.list().await.expect("seeded index").len(),
        1
    );
    let hosted_tenant = TenantContext::explicit_user_workspace(
        "org-pack",
        "workspace-pack",
        Some("deployment-pack".to_string()),
        "bob",
    );
    let hosted = pack_read_router(
        state,
        hosted_tenant.clone(),
        Some(verified_actor(hosted_tenant)),
    );
    for (method, uri) in [
        ("GET", "/packs"),
        ("GET", "/packs/private-workflow"),
        (
            "GET",
            "/packs/private-workflow/files/workflows/alice/plan-package.json",
        ),
        ("GET", "/packs/private-workflow/updates"),
        ("POST", "/packs/private-workflow/update"),
    ] {
        let (status, body) = pack_read_request(hosted.clone(), method, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert!(
            !String::from_utf8_lossy(&body).contains("alice-private-plan-bundle"),
            "{method} {uri} disclosed private bundle"
        );
    }
}

#[tokio::test]
async fn local_implicit_actor_keeps_installed_pack_reads() {
    let root = tempfile::tempdir().expect("temporary pack root");
    let state = seeded_private_pack_state(root.path()).await;
    let local = pack_read_router(state, TenantContext::local_implicit(), None);
    for (method, uri) in [
        ("GET", "/packs"),
        ("GET", "/packs/private-workflow"),
        (
            "GET",
            "/packs/private-workflow/files/workflows/alice/plan-package.json",
        ),
        ("GET", "/packs/private-workflow/updates"),
        ("POST", "/packs/private-workflow/update"),
    ] {
        let (status, body) = pack_read_request(local.clone(), method, uri).await;
        assert_eq!(status, StatusCode::OK, "{method} {uri}");
        if uri.ends_with("plan-package.json") {
            assert_eq!(body.as_slice(), b"alice-private-plan-bundle");
        }
    }
    let (status, _) = pack_read_request(local, "GET", "/packs/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn verified_context_cannot_read_shared_packs_even_with_local_tenant_fields() {
    let root = tempfile::tempdir().expect("temporary pack root");
    let state = seeded_private_pack_state(root.path()).await;
    let local_tenant = TenantContext::local_implicit();
    let signed = pack_read_router(
        state,
        local_tenant.clone(),
        Some(verified_actor(local_tenant)),
    );
    let (status, _) = pack_read_request(signed, "GET", "/packs").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
