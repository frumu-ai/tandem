use super::*;

#[tokio::test]
async fn mcp_phase_tool_authority_allows_same_scope_and_records_policy() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "alice-union-token", &tenant)
        .await
        .expect("store tenant token");
    state
        .mcp
        .refresh_for_tenant("notion", &tenant)
        .await
        .expect("refresh tenant tools");
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-allow",
    );

    let result = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__phase_tool_authority": {
                "phase": "research",
                "allowed_tools": ["mcp.notion.alice_search"],
                "run_id": "run-phase-allow",
                "automation_id": "automation-phase",
                "node_id": "node-research",
                "session_id": "session-phase",
                "message_id": "message-phase"
            }
        }),
        &tenant,
        Some(&verified),
    )
    .await
    .expect("allowed phase MCP call");

    assert_eq!(
        result
            .metadata
            .pointer("/phaseToolAuthorityPreflight/phase")
            .and_then(Value::as_str),
        Some("research")
    );
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-phase-allow", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| {
            decision.policy_id.as_deref() == Some("workflow_phase_tool_authority")
                && decision.reason_code == "phase_tool_allowed"
        })
        .expect("phase tool allow policy decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Allow);
    assert_eq!(
        decision
            .metadata
            .pointer("/phase_tool_authority/phase")
            .and_then(Value::as_str),
        Some("research")
    );

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.tool.execution\""));
    assert!(audit.contains("workflow_phase_tool_authority"));
    assert!(!audit.contains("alice-union-token"));
    drop(server);
}

#[tokio::test]
async fn mcp_phase_tool_authority_denies_wrong_phase_tool_with_audit() {
    let state = test_state().await;
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-deny",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__phase_tool_authority": {
                "phase": "publish",
                "allowed_tools": ["mcp.notion.create_page"],
                "run_id": "run-phase-deny",
                "automation_id": "automation-phase",
                "node_id": "node-publish",
                "session_id": "session-phase",
                "message_id": "message-phase"
            }
        }),
        &tenant,
        Some(&verified),
    )
    .await
    .expect_err("wrong phase tool must be denied before remote execution");

    assert!(err.contains("ToolDenied { reason: PhaseToolAuthority }"));
    assert!(err.contains("not allowed during workflow phase `publish`"));
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-phase-deny", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "phase_tool_not_allowed")
        .expect("phase tool denial decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.phase_tool.denied\""));
    assert!(audit.contains("phase_tool_not_allowed"));
}

#[tokio::test]
async fn mcp_secret_tenant_mismatch_records_scope_policy_and_redacts_secret_material() {
    let state = test_state().await;
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("deployment-a".to_string()),
        "user-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-b",
        "workspace-b",
        Some("deployment-b".to_string()),
        "user-b",
    );
    state
        .mcp
        .add_or_update_with_secret_refs(
            "tenant-server".to_string(),
            "http://127.0.0.1:9/mcp".to_string(),
            HashMap::new(),
            HashMap::from([(
                "Authorization".to_string(),
                tandem_runtime::McpSecretRef::Store {
                    secret_id: "super-secret-canary".to_string(),
                    tenant_context: tenant_a,
                },
            )]),
            &tenant_b,
            true,
        )
        .await;
    let verified = verified_mcp_execute_context(
        &tenant_b,
        tandem_types::PrincipalRef::human_user("user-b").with_tenant_actor_id("user-b"),
        "assertion-secret-scope",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "tenant-server",
        "get_me",
        json!({
            "__phase_tool_authority": {
                "phase": "credential_use",
                "allowed_tools": ["mcp.tenant_server.get_me"],
                "run_id": "run-secret-scope",
                "automation_id": "automation-secret",
                "node_id": "node-secret"
            }
        }),
        &tenant_b,
        Some(&verified),
    )
    .await
    .expect_err("tenant B cannot use tenant A's store-backed secret");

    assert!(err.contains("ToolDenied { reason: TenantScope }"));
    let events = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant_b).await;
    let event = events
        .iter()
        .find(|event| event.event_type == "mcp.secret_tenant_mismatch")
        .expect("mcp secret tenant mismatch audit event");
    assert_eq!(
        event.payload["policy_id"].as_str(),
        Some("mcp_secret_scope")
    );
    assert_eq!(
        event
            .payload
            .pointer("/phase_tool_authority/phase")
            .and_then(Value::as_str),
        Some("credential_use")
    );
    assert_eq!(
        event.payload["secret_material_redacted"].as_bool(),
        Some(true)
    );
    assert!(event.payload["run_as"].is_object());

    let protected_audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(
        !protected_audit.contains("super-secret-canary"),
        "secret identifiers must not be written to protected audit payloads"
    );
    let decisions = state
        .list_policy_decisions_for_run(&tenant_b, "run-secret-scope", 50)
        .await;
    let secret_decision = decisions
        .iter()
        .find(|decision| decision.policy_id.as_deref() == Some("mcp_secret_scope"))
        .expect("secret scope policy decision");
    assert_eq!(
        secret_decision.decision,
        tandem_types::PolicyDecisionEffect::Deny
    );
    assert!(secret_decision
        .data_classes
        .contains(&tandem_types::DataClass::Credential));
    assert_eq!(
        secret_decision
            .metadata
            .get("secret_material_redacted")
            .and_then(Value::as_bool),
        Some(true)
    );
}
