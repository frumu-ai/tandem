#[derive(Clone)]
struct McpToolDispatchBinding {
    registry: McpRegistry,
    server_name: String,
    tool_name: String,
    server_policy: Value,
    tenant: TenantContext,
    connection_generation: Option<String>,
    request_authority: Option<crate::McpRequestAuthority>,
}

fn server_dispatch_policy(server: &McpServer) -> Value {
    json!({
        "transport": server.transport, "enabled": server.enabled,
        "allowed_tools": server.allowed_tools, "auth_kind": server.auth_kind,
        "headers": server.headers, "secret_headers": server.secret_headers,
        "oauth": server.oauth,
    })
}

impl McpToolDispatchBinding {
    fn revalidate(&self) -> Result<(), String> {
        let servers = self
            .registry
            .servers
            .try_read()
            .map_err(|_| "MCP connector authority is changing; retry required".to_string())?;
        let connections = self
            .registry
            .connections
            .try_read()
            .map_err(|_| "MCP connection authority is changing; retry required".to_string())?;
        let server = servers
            .get(&self.server_name)
            .ok_or_else(|| "MCP connector was removed before dispatch".to_string())?;
        if !server.enabled
            || !mcp_tool_is_allowed(server, &self.tool_name)
            || server_dispatch_policy(server) != self.server_policy
        {
            return Err("MCP connector authority changed before dispatch".into());
        }
        // Both registries stay stable for this check, without waiting on a
        // mutation or holding either lock across network I/O.
        if let Some(expected) = &self.connection_generation {
            let owner = McpPrincipalRef::from_tenant_context(&self.tenant);
            let connection_id = mcp_connection_id(&self.server_name, &self.tenant, &owner);
            let connection = connections.get(&connection_id);
            if !connection.is_some_and(|connection| {
                connection.enabled && connection.connection_generation == *expected
            }) {
                return Err("MCP connection was revoked or replaced before dispatch".into());
            }
        }
        if let Some(authority) = &self.request_authority {
            authority.revalidate()?;
        }
        Ok(())
    }
}
