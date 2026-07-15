// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::Value;
use tandem_tools::{ToolDispatchContext, ToolDispatchSource};
use tandem_types::{TenantContext, ToolResult, VerifiedTenantContext};

use crate::AppState;

use super::bridge_registry::ensure_mcp_bridge_tool_for_dispatch;

/// Execute a discovered MCP tool through the server's central dispatch path.
///
/// System-initiated services use this entry point instead of calling the raw
/// MCP registry so policy, outbox, and dispatch receipts cannot be skipped.
pub(crate) async fn dispatch_mcp_tool_for_tenant(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: TenantContext,
    verified_tenant_context: Option<VerifiedTenantContext>,
    source: ToolDispatchSource,
) -> anyhow::Result<ToolResult> {
    state
        .mcp
        .ensure_ready_for_tenant(
            server_name,
            &tenant_context,
            tandem_runtime::mcp_ready::EnsureReadyPolicy::default(),
        )
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "MCP server `{server_name}` is not ready for governed dispatch: {error}"
            )
        })?;
    let remote = state
        .mcp
        .server_tools_for_tenant(server_name, &tenant_context)
        .await
        .into_iter()
        .find(|tool| tool.tool_name == tool_name || tool.namespaced_name == tool_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "MCP tool `{tool_name}` is not available for server `{server_name}` in this tenant"
            )
        })?;
    let _bridge_guard = ensure_mcp_bridge_tool_for_dispatch(state, &remote).await?;
    let dispatch_name = remote.namespaced_name;
    let context = attach_verified_tenant_context(
        state.tool_dispatch_context(source, tenant_context, vec![dispatch_name.clone()]),
        verified_tenant_context,
    );
    let result = state
        .tool_dispatcher
        .dispatch(&dispatch_name, args, context)
        .await?;
    require_registered_dispatch_result(result, &dispatch_name)
}

fn attach_verified_tenant_context(
    context: ToolDispatchContext,
    verified_tenant_context: Option<VerifiedTenantContext>,
) -> ToolDispatchContext {
    match verified_tenant_context {
        Some(verified_tenant_context) => {
            context.with_verified_tenant_context(verified_tenant_context)
        }
        None => context,
    }
}

fn require_registered_dispatch_result(
    result: ToolResult,
    dispatch_name: &str,
) -> anyhow::Result<ToolResult> {
    if result.output == format!("Unknown tool: {dispatch_name}") {
        anyhow::bail!(
            "MCP tool `{dispatch_name}` became unavailable before governed dispatch completed"
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_tools::ToolDispatchContext;
    use tandem_types::{AuthorityChain, HumanActor, RequestPrincipal, TenantContext, ToolResult};

    use super::{attach_verified_tenant_context, require_registered_dispatch_result};

    #[test]
    fn request_principal_survives_governed_mcp_context_building() {
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
        let verified = tandem_types::VerifiedTenantContext {
            tenant_context: tenant_context.clone(),
            human_actor: HumanActor::tandem_user("actor-a"),
            authority_chain: AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "actor-a",
                "tandem-web",
            )),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_999_999_999_999,
            assertion_id: "assertion-mcp-request".to_string(),
            assertion_key_id: None,
        };

        let context = attach_verified_tenant_context(
            ToolDispatchContext::for_tenant("coder_request", tenant_context),
            Some(verified.clone()),
        );

        assert_eq!(context.verified_tenant_context, Some(verified));
    }

    #[test]
    fn stale_bridge_unknown_tool_result_fails_closed() {
        let error = require_registered_dispatch_result(
            ToolResult {
                output: "Unknown tool: mcp.linear.create_issue".to_string(),
                metadata: json!({}),
            },
            "mcp.linear.create_issue",
        )
        .expect_err("stale bridge dispatch must not be reported as a successful delivery");

        assert!(error
            .to_string()
            .contains("became unavailable before governed dispatch completed"));
    }

    #[test]
    fn ordinary_mcp_result_remains_successful() {
        let result = ToolResult {
            output: "created issue TAN-123".to_string(),
            metadata: json!({"id": "TAN-123"}),
        };

        let returned = require_registered_dispatch_result(result, "mcp.linear.create_issue")
            .expect("registered tool result should pass through");
        assert_eq!(returned.output, "created issue TAN-123");
        assert_eq!(returned.metadata, json!({"id": "TAN-123"}));
    }
}
