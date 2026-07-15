use std::sync::Arc;

use tandem_core::tool_name_security_descriptor;
use tandem_types::ToolSchema;

use crate::AppState;

use super::{mcp_namespace_segment, McpBridgeTool};

static MCP_BRIDGE_REGISTRY_LOCK: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

fn mcp_bridge_tool_schema(tool: &tandem_runtime::McpRemoteTool) -> ToolSchema {
    ToolSchema::new(
        tool.namespaced_name.clone(),
        if tool.description.trim().is_empty() {
            format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
        } else {
            tool.description.clone()
        },
        tool.input_schema.clone(),
    )
    .with_security(tool_name_security_descriptor(&tool.namespaced_name))
}

async fn mcp_bridge_tool_registration_is_current(
    state: &AppState,
    remote: &tandem_runtime::McpRemoteTool,
) -> bool {
    let expected = mcp_bridge_tool_schema(remote);
    state
        .tools
        .list()
        .await
        .into_iter()
        .any(|schema| schema == expected)
}

/// Keep connector lifecycle mutations out of concurrent governed dispatches.
///
/// The returned read guard must live through dispatch. Normal calls only take
/// the shared guard; a missing or changed bridge schema upgrades through the
/// exclusive resync path and then rechecks before allowing execution.
pub(super) async fn ensure_mcp_bridge_tool_for_dispatch(
    state: &AppState,
    remote: &tandem_runtime::McpRemoteTool,
) -> anyhow::Result<tokio::sync::RwLockReadGuard<'static, ()>> {
    loop {
        let read_guard = MCP_BRIDGE_REGISTRY_LOCK.read().await;
        if mcp_bridge_tool_registration_is_current(state, remote).await {
            return Ok(read_guard);
        }
        drop(read_guard);

        let write_guard = MCP_BRIDGE_REGISTRY_LOCK.write().await;
        if !mcp_bridge_tool_registration_is_current(state, remote).await {
            let _ = resync_mcp_bridge_tools_for_server_locked(state, &remote.server_name).await;
        }
        if !mcp_bridge_tool_registration_is_current(state, remote).await {
            anyhow::bail!(
                "MCP bridge tool `{}` is no longer registered after connector resync",
                remote.namespaced_name
            );
        }
        drop(write_guard);
    }
}

async fn resync_mcp_bridge_tools_for_server_locked(state: &AppState, name: &str) -> (usize, usize) {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(name));
    let removed = state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.bridge_tools_for_server(name).await;
    for tool in &tools {
        let schema = mcp_bridge_tool_schema(tool);
        state
            .tools
            .register_tool(
                schema.name.clone(),
                Arc::new(McpBridgeTool {
                    schema,
                    state: state.clone(),
                    server_name: tool.server_name.clone(),
                    tool_name: tool.tool_name.clone(),
                }),
            )
            .await;
    }
    (removed, tools.len())
}

pub(super) async fn resync_mcp_bridge_tools_for_server(
    state: &AppState,
    name: &str,
) -> (usize, usize) {
    let _write_guard = MCP_BRIDGE_REGISTRY_LOCK.write().await;
    resync_mcp_bridge_tools_for_server_locked(state, name).await
}

pub(super) async fn unregister_mcp_bridge_tools_for_server(state: &AppState, name: &str) -> usize {
    let _write_guard = MCP_BRIDGE_REGISTRY_LOCK.write().await;
    let prefix = format!("mcp.{}.", mcp_namespace_segment(name));
    state.tools.unregister_by_prefix(&prefix).await
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use async_trait::async_trait;
    use serde_json::{json, Value};
    use tandem_runtime::McpRemoteTool;
    use tandem_tools::Tool;
    use tandem_types::{ToolResult, ToolSchema};

    use super::{
        ensure_mcp_bridge_tool_for_dispatch, mcp_bridge_tool_schema,
        resync_mcp_bridge_tools_for_server,
    };

    struct CurrentBridgeTool {
        schema: ToolSchema,
    }

    #[async_trait]
    impl Tool for CurrentBridgeTool {
        fn schema(&self) -> ToolSchema {
            self.schema.clone()
        }

        async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "ok".to_string(),
                metadata: json!({}),
            })
        }
    }

    #[tokio::test]
    async fn current_bridge_dispatches_share_guard_while_lifecycle_resync_waits() {
        let state = crate::test_support::test_state().await;
        let remote = McpRemoteTool {
            server_name: "linear_concurrency_test".to_string(),
            tool_name: "create_issue".to_string(),
            namespaced_name: "mcp.linear_concurrency_test.create_issue".to_string(),
            description: "Create issue".to_string(),
            input_schema: json!({"type": "object"}),
            fetched_at_ms: 1,
            schema_hash: "schema-1".to_string(),
        };
        let schema = mcp_bridge_tool_schema(&remote);
        state
            .tools
            .register_tool(schema.name.clone(), Arc::new(CurrentBridgeTool { schema }))
            .await;

        let first_dispatch_guard = ensure_mcp_bridge_tool_for_dispatch(&state, &remote)
            .await
            .expect("current bridge should be registered");
        let second_dispatch_guard = tokio::time::timeout(
            Duration::from_secs(1),
            ensure_mcp_bridge_tool_for_dispatch(&state, &remote),
        )
        .await
        .expect("current bridge dispatches should share the registry read guard")
        .expect("current bridge should remain registered");
        drop(second_dispatch_guard);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let state_for_resync = state.clone();
        let mut resync = tokio::spawn(async move {
            let _ = started_tx.send(());
            resync_mcp_bridge_tools_for_server(&state_for_resync, "linear_concurrency_test").await
        });
        started_rx.await.expect("resync task should start");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut resync)
                .await
                .is_err(),
            "connector lifecycle resync must wait for active governed dispatches"
        );

        drop(first_dispatch_guard);
        tokio::time::timeout(Duration::from_secs(1), resync)
            .await
            .expect("resync should proceed after dispatch completes")
            .expect("resync task should not panic");
    }

    #[tokio::test]
    async fn stale_remote_fails_after_resync_instead_of_waiting_forever() {
        let state = crate::test_support::test_state().await;
        let remote = McpRemoteTool {
            server_name: "missing_concurrency_test".to_string(),
            tool_name: "create_issue".to_string(),
            namespaced_name: "mcp.missing_concurrency_test.create_issue".to_string(),
            description: "Create issue".to_string(),
            input_schema: json!({"type": "object"}),
            fetched_at_ms: 1,
            schema_hash: "schema-1".to_string(),
        };

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            ensure_mcp_bridge_tool_for_dispatch(&state, &remote),
        )
        .await
        .expect("stale bridge check must terminate");
        let error = match result {
            Ok(_) => panic!("stale bridge must not receive a dispatch guard"),
            Err(error) => error,
        };
        assert!(error
            .to_string()
            .contains("is no longer registered after connector resync"));
    }
}
