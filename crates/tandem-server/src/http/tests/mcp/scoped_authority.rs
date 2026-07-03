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
async fn mcp_phase_tool_authority_enterprise_override_blocks_local_allow() {
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
    state.enterprise.policy_rules.write().await.insert(
        "enterprise-phase-tool-deny".to_string(),
        tandem_enterprise_contract::EnterprisePolicyRule::new(
            "enterprise-phase-tool-deny",
            "enterprise-phase-tool-floor",
            tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
            tandem_enterprise_contract::EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant.clone())
        .with_workflow_phase("research")
        .with_tool_patterns(vec!["mcp.notion.*".to_string()])
        .with_reason(
            "enterprise_phase_tool_floor",
            "enterprise policy denies phase MCP tools",
        ),
    );
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-enterprise-deny",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__phase_tool_authority": {
                "phase": "research",
                "allowed_tools": ["mcp.notion.alice_search"],
                "run_id": "run-phase-enterprise-deny",
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
    .expect_err("enterprise policy must override phase tool allow");

    assert!(err.contains("ToolDenied { reason: PhaseToolAuthority }"));
    assert!(err.contains("enterprise policy denies phase MCP tools"));
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-phase-enterprise-deny", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "enterprise_phase_tool_floor")
        .expect("enterprise phase tool decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);
    drop(server);
}

#[tokio::test]
async fn mcp_bridge_derives_phase_authority_from_dispatch_context() {
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
    assert_eq!(
        crate::http::mcp::sync_mcp_tools_for_server_for_tenant(&state, "notion", &tenant).await,
        1
    );
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-dispatch-phase-tool",
    );
    let context = tandem_tools::ToolDispatchContext::for_tenant("test", tenant.clone())
        .with_source(
            tandem_tools::ToolDispatchSource::new("engine_loop")
                .session("session-dispatch")
                .message("message-dispatch")
                .run("run-dispatch")
                .node("node-dispatch"),
        )
        .with_scope_allowlist(vec!["mcp.notion.alice_search".to_string()])
        .with_verified_tenant_context(verified);

    let result = state
        .tool_dispatcher
        .dispatch(
            "mcp.notion.alice_search",
            json!({
                "query": "roadmap",
                "__phase_tool_authority": {
                    "allowed_tools": ["mcp.notion.spoofed"]
                }
            }),
            context,
        )
        .await
        .expect("dispatcher-injected phase authority should allow matching MCP tool");

    assert_eq!(
        result
            .metadata
            .pointer("/phaseToolAuthorityPreflight/runId")
            .and_then(Value::as_str),
        Some("run-dispatch")
    );
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-dispatch", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| {
            decision.policy_id.as_deref() == Some("workflow_phase_tool_authority")
                && decision.reason_code == "phase_tool_allowed"
        })
        .expect("phase tool decision from dispatch context");
    assert_eq!(
        decision
            .metadata
            .pointer("/phase_tool_authority/allowed_tools/0")
            .and_then(Value::as_str),
        Some("mcp.notion.alice_search")
    );
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
async fn mcp_phase_tool_authority_is_required_for_explicit_tenant_calls() {
    let state = test_state().await;
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-missing",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({ "query": "roadmap" }),
        &tenant,
        Some(&verified),
    )
    .await
    .expect_err("explicit tenant MCP calls must include trusted phase authority");

    assert!(err.contains("ToolDenied { reason: PhaseToolAuthority }"));
    assert!(err.contains("phase tool authority is missing"));
    let decisions = state.list_policy_decisions(&tenant, 50).await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "phase_tool_authority_missing")
        .expect("missing phase authority decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.phase_tool.denied\""));
    assert!(audit.contains("phase_tool_authority_missing"));
}

#[tokio::test]
async fn mcp_phase_tool_authority_empty_allowlist_denies_all_tools() {
    let state = test_state().await;
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-empty",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__phase_tool_authority": {
                "phase": "research",
                "allowed_tools": [],
                "run_id": "run-phase-empty"
            }
        }),
        &tenant,
        Some(&verified),
    )
    .await
    .expect_err("empty phase allowlist must deny all tools");

    assert!(err.contains("ToolDenied { reason: PhaseToolAuthority }"));
    assert!(err.contains("has no allowed tools"));
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-phase-empty", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "phase_tool_authority_empty_allowlist")
        .expect("empty allowlist denial decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);
}

#[tokio::test]
async fn mcp_phase_tool_authority_does_not_match_bare_tool_aliases() {
    let state = test_state().await;
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let verified = verified_mcp_execute_context(
        &tenant,
        tandem_types::PrincipalRef::human_user("alice").with_tenant_actor_id("alice"),
        "assertion-phase-tool-bare",
    );

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_verified_context(
        &state,
        "notion",
        "alice_search",
        json!({
            "query": "roadmap",
            "__phase_tool_authority": {
                "phase": "research",
                "allowed_tools": ["alice_search"],
                "run_id": "run-phase-bare"
            }
        }),
        &tenant,
        Some(&verified),
    )
    .await
    .expect_err("bare tool names must not match cross-server MCP tools");

    assert!(err.contains("ToolDenied { reason: PhaseToolAuthority }"));
    let decisions = state
        .list_policy_decisions_for_run(&tenant, "run-phase-bare", 50)
        .await;
    let decision = decisions
        .iter()
        .find(|decision| decision.reason_code == "phase_tool_not_allowed")
        .expect("bare alias denial decision");
    assert_eq!(decision.decision, tandem_types::PolicyDecisionEffect::Deny);
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
