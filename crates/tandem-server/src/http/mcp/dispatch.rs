use serde_json::Value;
use tandem_tools::ToolDispatchSource;
use tandem_types::{TenantContext, ToolResult};

use crate::AppState;

use super::resync_mcp_bridge_tools_for_server;

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
    source: ToolDispatchSource,
) -> anyhow::Result<ToolResult> {
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
    let _ = resync_mcp_bridge_tools_for_server(state, server_name).await;
    let dispatch_name = remote.namespaced_name;
    let context = state.tool_dispatch_context(source, tenant_context, vec![dispatch_name.clone()]);
    let result = state
        .tool_dispatcher
        .dispatch(&dispatch_name, args, context)
        .await?;
    require_registered_dispatch_result(result, &dispatch_name)
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
    use tandem_types::ToolResult;

    use super::require_registered_dispatch_result;

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
