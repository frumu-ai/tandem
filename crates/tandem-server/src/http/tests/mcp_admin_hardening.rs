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
