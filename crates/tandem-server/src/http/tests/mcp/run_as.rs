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

    let verified = verified_mcp_execute_context(
        &alice,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-alice-mcp-run-as",
    );
    let result = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &alice,
        Some(&verified),
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
    let reliability_path =
        crate::stateful_runtime::stateful_reliability_path_from_runtime_events_path(
            &state.runtime_events_path,
        );
    let reliability = crate::stateful_runtime::load_stateful_reliability(&reliability_path);
    let receipt = reliability
        .tool_effects
        .iter()
        .find(|effect| {
            effect.provider.as_deref() == Some("notion")
                && effect.tool.as_deref() == Some("mcp.notion.alice_search")
        })
        .expect("mcp tool effect receipt");
    assert_eq!(
        receipt
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.pointer("/run_as/connectionId"))
            .and_then(Value::as_str),
        Some(alice_connection_id.as_str())
    );
    assert_eq!(
        receipt
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.pointer("/run_as/principal/type"))
            .and_then(Value::as_str),
        Some("human_actor")
    );
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_enterprise_policy_override_blocks_context_assertion_allow() {
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
    state.enterprise.policy_rules.write().await.insert(
        "enterprise-mcp-context-deny".to_string(),
        tandem_enterprise_contract::EnterprisePolicyRule::new(
            "enterprise-mcp-context-deny",
            "enterprise-mcp-floor",
            tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
            tandem_enterprise_contract::EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(alice.clone())
        .with_tool_patterns(vec!["mcp.notion.*".to_string()])
        .with_reason(
            "enterprise_mcp_context_floor",
            "enterprise policy denies MCP execution",
        ),
    );
    let verified = verified_mcp_execute_context(
        &alice,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-alice-enterprise-deny",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &alice,
        Some(&verified),
    )
    .await
    .expect_err("enterprise policy must override context assertion allow");

    assert!(err.contains("ToolDenied { reason: ContextAssertion }"));
    assert!(err.contains("enterprise policy denies MCP execution"));
    let decisions = state.list_policy_decisions(&alice, 100).await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "enterprise_mcp_context_floor")
        .expect("enterprise override policy decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);
    drop(server);
}

#[tokio::test]
async fn mcp_run_as_denies_explicit_tenant_without_context_assertion() {
    let state = test_state().await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &alice,
    )
    .await
    .expect_err("explicit tenant MCP calls must require verified context");

    assert!(err.contains("ToolDenied { reason: ContextAssertion }"));
    assert!(err.contains("verified tenant context assertion is required"));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.context_assertion_denied\""));
    assert!(audit.contains("missing_verified_tenant_context"));
    let decisions = state
        .list_policy_decisions_for_run(&alice, "missing-run", 100)
        .await;
    assert!(decisions.is_empty());
    assert!(state
        .list_policy_decisions(&alice, 100)
        .await
        .iter()
        .any(
            |decision| decision.policy_id.as_deref() == Some("mcp_context_assertion_preflight")
                && decision.reason_code == "missing_verified_tenant_context"
        ));
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

    let verified = verified_mcp_execute_context(
        &scheduled_tenant,
        tandem_types::PrincipalRef::new(
            tandem_types::PrincipalKind::ServiceAccount,
            "scheduled-automation",
        ),
        "assertion-scheduled-mcp-run-as",
    );
    let result = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &scheduled_tenant,
        Some(&verified),
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
async fn mcp_run_as_denies_cross_tenant_connection_id() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let tenant_a =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let tenant_b =
        tandem_types::TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "alice");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "tenant-a-alice-token", &tenant_a)
        .await
        .expect("store tenant a token");
    state
        .mcp
        .refresh_for_tenant("notion", &tenant_a)
        .await
        .expect("refresh tenant a tools");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "tenant-b-alice-token", &tenant_b)
        .await
        .expect("store tenant b token");
    state
        .mcp
        .refresh_for_tenant("notion", &tenant_b)
        .await
        .expect("refresh tenant b tools");
    let tenant_b_connection_id = state.mcp.connection_id_for_tenant("notion", &tenant_b);

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__mcp_connection_id": tenant_b_connection_id.clone(),
        }),
        &tenant_a,
    )
    .await
    .expect_err("tenant a must not execute with tenant b's MCP connection");

    assert!(err.contains("ToolDenied { reason: McpRunAsPolicy }"));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.run_as_denied\""));
    assert!(audit.contains(&tenant_b_connection_id));
    assert!(audit.contains("requested connection"));
    assert!(!audit.contains("tenant-b-alice-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_connect_events_are_tenant_tagged_and_content_free() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
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
    let connection_id = state.mcp.connection_id_for_tenant("notion", &alice);

    let app = app_router(state.clone());
    let connect_resp = app
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("connect response");
    assert_eq!(connect_resp.status(), StatusCode::OK);

    let connected = crate::test_support::next_event_of_type(&mut rx, "mcp.server.connected").await;
    let tools = crate::test_support::next_event_of_type(&mut rx, "mcp.tools.updated").await;
    for event in [&connected, &tools] {
        assert_eq!(
            event.properties.get("connectionId").and_then(Value::as_str),
            Some(connection_id.as_str())
        );
        assert_eq!(
            event
                .properties
                .pointer("/tenantContext/actor_id")
                .and_then(Value::as_str),
            Some("alice")
        );
        assert_eq!(
            event
                .properties
                .pointer("/principal/type")
                .and_then(Value::as_str),
            Some("human_actor")
        );
        let properties = serde_json::to_string(&event.properties).expect("event properties json");
        assert!(!properties.contains("alice-union-token"));
    }

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
