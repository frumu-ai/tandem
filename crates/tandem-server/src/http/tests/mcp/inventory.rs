// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
#[serial_test::serial(mcp_provider_auth)]
async fn mcp_inventory_redacts_and_filters_connections_by_actor() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_hosted_mcp_oauth_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);

    let app = app_router(state.clone());
    let alice_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("alice connect response");
    assert_eq!(alice_resp.status(), StatusCode::OK);

    let alice_tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let alice_session = state
        .oauth
        .mcp_sessions()
        .read()
        .await
        .values()
        .find(|candidate| candidate.tenant_context == alice_tenant)
        .cloned()
        .expect("alice mcp oauth session");

    let bob_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "bob",
        ))
        .await
        .expect("bob connect response");
    assert_eq!(bob_resp.status(), StatusCode::OK);

    let bob_tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "bob");
    let bob_session = state
        .oauth
        .mcp_sessions()
        .read()
        .await
        .values()
        .find(|candidate| candidate.tenant_context == bob_tenant)
        .cloned()
        .expect("bob mcp oauth session");

    let alice_inventory_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/mcp",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("alice inventory response");
    assert_eq!(alice_inventory_resp.status(), StatusCode::OK);
    let alice_inventory_body = to_bytes(alice_inventory_resp.into_body(), usize::MAX)
        .await
        .expect("alice inventory body");
    let alice_inventory_payload: Value =
        serde_json::from_slice(&alice_inventory_body).expect("alice inventory json");
    let alice_inventory_connections = alice_inventory_payload
        .pointer("/notion/connections")
        .and_then(Value::as_array)
        .expect("alice inventory connections");

    let alice_connection = alice_inventory_connections
        .iter()
        .find(|connection| {
            connection.get("connection_id").and_then(Value::as_str)
                == Some(alice_session.connection_id.as_str())
        })
        .expect("alice should see her own pending MCP connection");
    let connection_generation = alice_connection
        .get("connection_generation")
        .and_then(Value::as_str)
        .expect("public connection generation");
    assert!(!connection_generation.is_empty());
    assert_eq!(
        alice_connection
            .get("connectionGeneration")
            .and_then(Value::as_str),
        Some(connection_generation)
    );
    assert!(
        alice_inventory_connections.iter().all(|connection| {
            connection.get("connection_id").and_then(Value::as_str)
                != Some(bob_session.connection_id.as_str())
        }),
        "alice must not see bob's pending MCP connection"
    );
    assert!(
        !String::from_utf8_lossy(&alice_inventory_body).contains("client_secret"),
        "public MCP inventory must not expose OAuth client secrets"
    );

    let delete_response = app
        .oneshot(tenant_admin_request(
            "DELETE",
            "/mcp/notion/auth",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("Alice auth deletion response");
    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload: Value = serde_json::from_slice(
        &to_bytes(delete_response.into_body(), usize::MAX)
            .await
            .expect("delete auth body"),
    )
    .expect("delete auth json");
    assert_eq!(
        delete_payload
            .get("removedOauthSessionCount")
            .and_then(Value::as_u64),
        Some(1)
    );
    let sessions = state.oauth.mcp_sessions().read().await;
    assert!(sessions
        .values()
        .all(|session| session.tenant_context != alice_tenant));
    assert!(
        sessions
            .values()
            .any(|session| session.tenant_context == bob_tenant),
        "Alice auth deletion must preserve Bob's pending OAuth session"
    );
    drop(sessions);
    let connections = state.mcp.list_connections().await;
    assert!(connections
        .get(&alice_session.connection_id)
        .expect("Alice connection remains inventoried")
        .last_auth_challenge
        .is_none());
    assert!(
        connections
            .get(&bob_session.connection_id)
            .expect("Bob connection remains inventoried")
            .last_auth_challenge
            .is_some(),
        "Alice auth deletion must preserve Bob's scoped connection"
    );

    drop(server);
}
