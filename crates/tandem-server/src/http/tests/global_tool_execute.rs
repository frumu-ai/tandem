use super::*;

struct EchoGlobalExecuteArgsTool;

#[async_trait::async_trait]
impl tandem_tools::Tool for EchoGlobalExecuteArgsTool {
    fn schema(&self) -> tandem_types::ToolSchema {
        tandem_types::ToolSchema::new(
            "echo_global_execute_args",
            "Echo global execute args for tests",
            serde_json::json!({
                "type": "object",
                "additionalProperties": true
            }),
        )
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<tandem_types::ToolResult> {
        Ok(tandem_types::ToolResult {
            output: args.to_string(),
            metadata: serde_json::json!({}),
        })
    }
}

#[tokio::test]
async fn tool_execute_scope_allowlist_is_injected_as_trusted_phase_authority() {
    let state = test_state().await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule(
            "echo_global_execute_args".to_string(),
            "echo_global_execute_args".to_string(),
            tandem_core::PermissionAction::Allow,
        )
        .await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "scopeAllowlist": [
                    " echo_global_execute_args ",
                    "",
                    "mcp.email_demo.email_draft",
                    "mcp.email_demo.email_draft"
                ],
                "args": {
                    "value": 1,
                    "__phase_tool_authority": {
                        "allowed_tools": ["mcp.spoofed.tool"]
                    }
                }
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let echoed: serde_json::Value = serde_json::from_str(
        payload
            .get("output")
            .and_then(serde_json::Value::as_str)
            .expect("output"),
    )
    .expect("echoed args");
    let allowed_tools = echoed
        .pointer("/__phase_tool_authority/allowed_tools")
        .and_then(serde_json::Value::as_array)
        .expect("trusted allowed tools");
    assert_eq!(
        allowed_tools,
        &vec![
            serde_json::json!("echo_global_execute_args"),
            serde_json::json!("mcp.email_demo.email_draft")
        ]
    );
    assert_eq!(
        echoed
            .pointer("/__phase_tool_authority/source")
            .and_then(serde_json::Value::as_str),
        Some("tool_dispatch_context")
    );
}

#[tokio::test]
async fn tool_execute_without_matching_server_policy_is_denied_and_receipted() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "args": { "value": "must-not-run" }
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let reliability_path =
        crate::stateful_runtime::stateful_reliability_path_from_runtime_events_path(
            &state.runtime_events_path,
        );
    let receipts = crate::stateful_runtime::load_stateful_reliability(&reliability_path);
    assert!(receipts.tool_effects.iter().any(|receipt| {
        receipt.tool.as_deref() == Some("echo_global_execute_args")
            && receipt
                .receipt_payload_redacted
                .as_ref()
                .and_then(|payload| payload.get("policy_outcome"))
                .and_then(serde_json::Value::as_str)
                == Some("denied")
    }));
}

#[tokio::test]
async fn enterprise_allow_does_not_bypass_server_ask_permission() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let rule = tandem_enterprise_contract::EnterprisePolicyRule::new(
        "global-http-allow",
        "global-http-policy",
        tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
        tandem_enterprise_contract::EnterprisePolicyEffect::Allow,
    )
    .with_tool_patterns(vec!["echo_global_execute_args".to_string()]);
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(rule.rule_id.clone(), rule);

    let app = app_router(state);
    let request = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "args": { "value": "must-still-wait" }
            })
            .to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn enterprise_approval_is_not_created_without_server_allow_permission() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let rule = tandem_enterprise_contract::EnterprisePolicyRule::new(
        "global-http-dead-approval",
        "global-http-policy",
        tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
        tandem_enterprise_contract::EnterprisePolicyEffect::ApprovalRequired,
    )
    .with_tool_patterns(vec!["echo_global_execute_args".to_string()])
    .with_approval_id("global-http-review");
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(rule.rule_id.clone(), rule);

    let app = app_router(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "args": { "value": "must-not-create-dead-approval" }
            })
            .to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let approvals = state
        .list_approval_requests_for_tenant(
            None,
            None,
            &tandem_types::TenantContext::local_implicit(),
        )
        .await;
    assert!(
        approvals.is_empty(),
        "server Ask permissions must not create approvals that can never authorize execution"
    );
}

#[tokio::test]
async fn tool_execute_client_scope_cannot_grant_server_permission() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "scopeAllowlist": ["echo_global_execute_args"],
                "args": { "value": "must-not-run" }
            })
            .to_string(),
        ))
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tool_execute_scope_denial_returns_structured_forbidden_response() {
    let state = test_state().await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let app = app_router(state);
    let request = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "scopeAllowlist": ["different_tool"],
                "args": { "value": "must-not-run" }
            })
            .to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("JSON response");
    assert_eq!(payload["code"], "TOOL_DISPATCH_DENIED");
    assert_eq!(payload["outcome"], "denied");
    assert!(payload["reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("ScopeAllowlist")));
}

#[tokio::test]
async fn tool_execute_returns_structured_pending_approval_without_running_tool() {
    let state = test_state().await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule(
            "echo_global_execute_args".to_string(),
            "echo_global_execute_args".to_string(),
            tandem_core::PermissionAction::Allow,
        )
        .await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let rule = tandem_enterprise_contract::EnterprisePolicyRule::new(
        "global-http-approval",
        "global-http-policy",
        tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
        tandem_enterprise_contract::EnterprisePolicyEffect::ApprovalRequired,
    )
    .with_tool_patterns(vec!["echo_global_execute_args".to_string()])
    .with_approval_id("global-http-review");
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(rule.rule_id.clone(), rule);

    let app = app_router(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "tool": "echo_global_execute_args",
                "args": { "value": "must-wait" }
            })
            .to_string(),
        ))
        .expect("request");

    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("JSON response");
    assert_eq!(payload["code"], "TOOL_APPROVAL_REQUIRED");
    assert_eq!(payload["outcome"], "approval_required");
    assert_eq!(
        payload["approval_requirement"]["approval_class"],
        "global-http-review"
    );
    assert_eq!(
        payload["approval_requirement"]["rule_id"],
        "global-http-approval"
    );

    let reliability_path =
        crate::stateful_runtime::stateful_reliability_path_from_runtime_events_path(
            &state.runtime_events_path,
        );
    let receipts = crate::stateful_runtime::load_stateful_reliability(&reliability_path);
    assert!(receipts.tool_effects.iter().any(|receipt| {
        receipt.tool.as_deref() == Some("echo_global_execute_args")
            && receipt
                .receipt_payload_redacted
                .as_ref()
                .and_then(|payload| payload.get("policy_outcome"))
                .and_then(serde_json::Value::as_str)
                == Some("approval_required")
    }));
}

#[tokio::test]
async fn trusted_server_scope_cannot_override_explicit_deny() {
    let state = test_state().await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule(
            "echo_global_execute_args",
            "echo_global_execute_args",
            tandem_core::PermissionAction::Deny,
        )
        .await;
    state
        .tools
        .register_tool(
            "echo_global_execute_args".to_string(),
            std::sync::Arc::new(EchoGlobalExecuteArgsTool),
        )
        .await;
    let context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("trusted_test"),
        tandem_types::TenantContext::local_implicit(),
        vec!["echo_global_execute_args".to_string()],
    );

    let error = state
        .tool_dispatcher
        .dispatch(
            "echo_global_execute_args",
            serde_json::json!({"value": "must-not-run"}),
            context,
        )
        .await
        .expect_err("explicit deny must override trusted scope");
    assert!(error.to_string().contains("denied by permission rule"));
}
