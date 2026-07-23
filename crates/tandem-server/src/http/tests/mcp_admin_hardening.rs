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
    assert!(state
        .mcp
        .standalone_private_endpoint_access_enabled_for_tests());
    assert!(crate::http::mcp::allow_private_mcp_oauth_endpoint(
        &state, &tenant
    ));

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
