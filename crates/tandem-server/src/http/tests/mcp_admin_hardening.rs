// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn direct_loopback_request() -> axum::http::request::Builder {
    Request::builder().extension(axum::extract::ConnectInfo(
        "127.0.0.1:39731"
            .parse::<std::net::SocketAddr>()
            .expect("loopback socket address"),
    ))
}

#[tokio::test]
async fn private_mcp_oauth_requires_standalone_listener_posture() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    state.mcp.set_strict_tenant_enforcement(false);
    assert!(state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());
    assert!(crate::http::mcp::allow_private_mcp_oauth_endpoint(
        &state, &tenant
    ));

    state.mcp.set_strict_tenant_enforcement(true);
    assert!(
        !crate::http::mcp::allow_private_mcp_oauth_endpoint(&state, &tenant),
        "strict hosted mode must deny private OAuth even on a bound loopback listener"
    );
    state.mcp.set_strict_tenant_enforcement(false);

    state.set_host_operations_loopback_only(false);
    state.set_server_base_url("https://tandem.example".to_string());
    state
        .trust_test_tenant_headers
        .store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(!state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());
    assert!(
        !crate::http::mcp::allow_private_mcp_oauth_endpoint(&state, &tenant),
        "local implicit identity alone must not authorize private OAuth egress"
    );
}

#[tokio::test]
async fn stopped_listener_posture_blocks_actual_private_mcp_request() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind canary");
    let addr = listener.local_addr().expect("canary address");
    let hits_for_handler = hits.clone();
    let canary = axum::Router::new().fallback(axum::routing::any(move || {
        let hits = hits_for_handler.clone();
        async move {
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": "canary",
                "result": {}
            }))
        }
    }));
    let canary_task = tokio::spawn(async move {
        axum::serve(listener, canary).await.expect("serve canary");
    });

    let state = test_state().await;
    state.mcp.set_strict_tenant_enforcement(false);
    state.mcp.deny_private_endpoints_for_tests();
    state.set_http_listener_bound_loopback_only(false);
    assert!(!state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());

    let listener_posture = super::super::HttpListenerPostureGuard::activate(&state, addr);
    assert!(state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());
    state
        .mcp
        .add(
            "private-posture-canary".to_string(),
            format!("http://{addr}/mcp"),
        )
        .await;
    let _ = state.mcp.refresh("private-posture-canary").await;
    assert!(
        hits.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "production listener posture must be the sole authorization for the positive request"
    );

    hits.store(0, std::sync::atomic::Ordering::SeqCst);
    drop(listener_posture);
    assert!(!state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());
    let result = state.mcp.refresh("private-posture-canary").await;
    assert!(result.is_err());
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    canary_task.abort();
}

#[tokio::test]
async fn mcp_registration_audit_records_header_names_without_values() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let secret = "mcp-admin-audit-secret-canary";

    let req = direct_loopback_request()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "audit-redaction",
                "transport": "https://example.com/mcp",
                "headers": {
                    "Authorization": format!("Bearer {secret}"),
                    "X-Custom-Secret": secret
                },
                "enabled": false
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("mcp_server_manage"));
    assert!(audit.contains("mcp.server.update_authorized"));
    assert!(audit.contains("Authorization"));
    assert!(audit.contains("X-Custom-Secret"));
    assert!(
        !audit.contains(secret),
        "MCP administrative audit must contain header names, never values"
    );
}
