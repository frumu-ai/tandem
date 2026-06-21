use super::*;

#[tokio::test]
async fn mcp_run_as_interactive_call_uses_current_actor_connection() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "alice-union-token", &alice)
        .await
        .expect("store alice token");
    state
        .mcp
        .refresh_for_tenant("notion", &alice)
        .await
        .expect("refresh alice tools");
    let alice_connection_id = state.mcp.connection_id_for_tenant("notion", &alice);

    let result = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &alice,
    )
    .await
    .expect("interactive MCP call should use current actor connection");

    let run_as = result
        .metadata
        .get("mcpRunAs")
        .expect("mcp run-as metadata");
    assert_eq!(
        run_as.get("connectionId").and_then(Value::as_str),
        Some(alice_connection_id.as_str())
    );
    assert_eq!(
        run_as.pointer("/principal/type").and_then(Value::as_str),
        Some("human_actor")
    );
    assert_eq!(
        run_as
            .pointer("/effectiveTenantContext/actor_id")
            .and_then(Value::as_str),
        Some("alice")
    );
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.tool.execution\""));
    assert!(audit.contains(&alice_connection_id));
    assert!(!audit.contains("alice-union-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_scheduled_automation_uses_tenant_service_principal_connection() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let scheduled_tenant = tandem_types::TenantContext::explicit("org-a", "workspace-a", None);
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "scheduled-service-token", &scheduled_tenant)
        .await
        .expect("store scheduled automation token");
    state
        .mcp
        .refresh_for_tenant("notion", &scheduled_tenant)
        .await
        .expect("refresh scheduled automation tools");
    let service_connection_id = state
        .mcp
        .connection_id_for_tenant("notion", &scheduled_tenant);

    let result = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &scheduled_tenant,
    )
    .await
    .expect("scheduled automation MCP call should use service-principal connection");

    let run_as = result
        .metadata
        .get("mcpRunAs")
        .expect("mcp run-as metadata");
    assert_eq!(
        run_as.get("connectionId").and_then(Value::as_str),
        Some(service_connection_id.as_str())
    );
    assert_eq!(
        run_as.pointer("/principal/type").and_then(Value::as_str),
        Some("service_principal")
    );
    assert!(run_as.pointer("/effectiveTenantContext/actor_id").is_none());
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.tool.execution\""));
    assert!(audit.contains(&service_connection_id));
    assert!(!audit.contains("scheduled-service-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_denies_cross_actor_connection_id() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let bob =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "bob");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "alice-union-token", &alice)
        .await
        .expect("store alice token");
    state
        .mcp
        .refresh_for_tenant("notion", &alice)
        .await
        .expect("refresh alice tools");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "bob-union-token", &bob)
        .await
        .expect("store bob token");
    state
        .mcp
        .refresh_for_tenant("notion", &bob)
        .await
        .expect("refresh bob tools");
    let bob_connection_id = state.mcp.connection_id_for_tenant("notion", &bob);

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__mcp_connection_id": bob_connection_id,
        }),
        &alice,
    )
    .await
    .expect_err("alice must not execute with bob's MCP connection");

    assert!(err.contains("ToolDenied { reason: McpRunAsPolicy }"));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.run_as_denied\""));
    assert!(audit.contains("requested connection"));
    assert!(!audit.contains("bob-union-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_denies_unsupported_shared_connection_with_audit() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__mcp_principal": {
                "type": "shared_connection",
                "grant_id": "shared-grant-1",
            },
        }),
        &alice,
    )
    .await
    .expect_err("shared connection grants should fail closed until bridge support exists");

    assert!(err.contains("ToolDenied { reason: McpRunAsPolicy }"));
    assert!(err.contains("not executable by the current bridge"));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.run_as_denied\""));
    assert!(audit.contains("not executable by the current bridge"));
    assert!(!audit.contains("shared-grant-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_denies_actor_selected_service_principal_without_trusted_grant() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let service = tandem_types::TenantContext::explicit("org-a", "workspace-a", None);
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "service-principal-token", &service)
        .await
        .expect("store service token");
    state
        .mcp
        .refresh_for_tenant("notion", &service)
        .await
        .expect("refresh service tools");
    let service_connection_id = state.mcp.connection_id_for_tenant("notion", &service);
    let service_principal = state
        .mcp
        .list_connections()
        .await
        .get(&service_connection_id)
        .map(|connection| connection.owner.clone())
        .expect("service connection owner");

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__mcp_run_as": {
                "connection_id": service_connection_id,
                "principal": service_principal,
            },
        }),
        &alice,
    )
    .await
    .expect_err("actor-scoped calls must not self-select tenant service principal");

    assert!(err.contains("ToolDenied { reason: McpRunAsPolicy }"));
    assert!(err.contains("requires a server-side connection grant"));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.run_as_denied\""));
    assert!(audit.contains(&service_connection_id));
    assert!(!audit.contains("service-principal-token"));
    drop(server);
}
