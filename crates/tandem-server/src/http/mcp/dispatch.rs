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
    state
        .tool_dispatcher
        .dispatch(&dispatch_name, args, context)
        .await
}
