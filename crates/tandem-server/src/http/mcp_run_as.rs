use serde_json::{json, Value};
use tandem_runtime::McpPrincipalRef;
use tandem_types::{TenantContext, ToolResult};

use crate::{now_ms, AppState};

const MCP_CONNECTION_ID_ARG: &str = "__mcp_connection_id";
const MCP_CONNECTION_ID_CAMEL_ARG: &str = "__mcpConnectionId";
const MCP_RUN_AS_ARG: &str = "__mcp_run_as";
const MCP_RUN_AS_CAMEL_ARG: &str = "__mcpRunAs";
const MCP_PRINCIPAL_ARG: &str = "__mcp_principal";
const MCP_PRINCIPAL_CAMEL_ARG: &str = "__mcpPrincipal";

#[derive(Debug, Clone)]
struct McpRunAsRequest {
    connection_id: Option<String>,
    principal: Option<McpPrincipalRef>,
}

#[derive(Debug, Clone)]
struct McpRunAsResolution {
    args: Value,
    requested_tenant_context: TenantContext,
    effective_tenant_context: TenantContext,
    connection_id: String,
    principal: McpPrincipalRef,
    connection_class: Option<String>,
    upstream_account: Option<Value>,
    requested_connection_id: Option<String>,
}

pub(crate) async fn call_mcp_tool_for_tenant_with_audit(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
) -> Result<ToolResult, String> {
    let run_as = resolve_mcp_run_as(state, server_name, tool_name, args, tenant_context).await?;
    let result = state
        .mcp
        .call_tool_for_tenant(
            server_name,
            tool_name,
            run_as.args.clone(),
            &run_as.effective_tenant_context,
        )
        .await;
    if result
        .as_ref()
        .err()
        .is_some_and(|error| mcp_error_is_secret_tenant_mismatch(error))
    {
        append_mcp_secret_tenant_mismatch_audit_event(
            state,
            server_name,
            tool_name,
            &run_as.effective_tenant_context,
        )
        .await;
    }

    append_mcp_tool_execution_audit_event(state, server_name, tool_name, &run_as, &result).await;
    result.map(|mut result| {
        let run_as_payload = run_as.audit_payload();
        if let Some(metadata) = result.metadata.as_object_mut() {
            metadata.insert("mcpRunAs".to_string(), run_as_payload);
        } else {
            result.metadata = json!({ "mcpRunAs": run_as_payload });
        }
        result
    })
}

fn mcp_error_is_secret_tenant_mismatch(error: &str) -> bool {
    error.contains("ToolDenied { reason: TenantScope }")
        && error.contains("store-backed secret header")
        && error.contains("different tenant context")
}

pub(crate) async fn append_mcp_secret_tenant_mismatch_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    tenant_context: &TenantContext,
) {
    let Some(denial) = state
        .mcp
        .secret_tenant_mismatch_audit(server_name, tool_name, tenant_context)
        .await
    else {
        return;
    };
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.secret_tenant_mismatch",
        &denial.tenant_context,
        denial.tenant_context.actor_id.clone(),
        json!({
            "reason": "store_secret_tenant_mismatch",
            "server_name": denial.server_name,
            "tool_name": denial.tool_name,
            "header_names": denial.header_names,
            "tenant_context": denial.tenant_context,
        }),
    )
    .await;
}

async fn resolve_mcp_run_as(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
) -> Result<McpRunAsResolution, String> {
    let request = extract_mcp_run_as_request(&args);
    let effective_tenant_context = match effective_tenant_context_for_run_as(
        tenant_context,
        request.principal.as_ref(),
    ) {
        Ok(context) => context,
        Err(reason) => {
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                tenant_context,
                request.connection_id.as_deref(),
                None,
                &reason,
            )
            .await;
            return Err(format!(
                    "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
                ));
        }
    };
    let expected_connection_id = state
        .mcp
        .connection_id_for_tenant(server_name, &effective_tenant_context);

    if let Some(requested_connection_id) = request.connection_id.as_deref() {
        if requested_connection_id != expected_connection_id {
            let reason = format!(
                "requested connection `{requested_connection_id}` is not owned by the effective tenant/principal"
            );
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                &reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }

    let connections = state.mcp.list_connections().await;
    let connection = connections.get(&expected_connection_id).cloned();
    let expected_principal = McpPrincipalRef::from_tenant_context(&effective_tenant_context);
    if let Some(connection) = connection.as_ref() {
        if connection.tenant_context != effective_tenant_context
            || connection.owner != expected_principal
        {
            let reason = "stored connection identity did not match the effective tenant/principal";
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }
    if let Some(requested_principal) = request.principal.as_ref() {
        let principal_matches = connection
            .as_ref()
            .map(|connection| requested_principal == &connection.owner)
            .unwrap_or_else(|| requested_principal == &expected_principal);
        if !principal_matches {
            let reason = "requested run-as principal did not match the selected connection";
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }

    Ok(McpRunAsResolution {
        args: strip_mcp_run_as_args(args),
        requested_tenant_context: tenant_context.clone(),
        effective_tenant_context,
        connection_id: expected_connection_id,
        principal: expected_principal,
        connection_class: connection.as_ref().and_then(|connection| {
            serde_json::to_value(&connection.connection_class)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
        }),
        upstream_account: connection
            .and_then(|connection| serde_json::to_value(connection.upstream_account).ok())
            .filter(|value| !value.is_null()),
        requested_connection_id: request.connection_id,
    })
}

fn effective_tenant_context_for_run_as(
    tenant_context: &TenantContext,
    principal: Option<&McpPrincipalRef>,
) -> Result<TenantContext, String> {
    let Some(principal) = principal else {
        return Ok(tenant_context.clone());
    };
    match principal {
        McpPrincipalRef::HumanActor { actor_id } => {
            if tenant_context.actor_id.as_deref() == Some(actor_id.as_str()) {
                Ok(tenant_context.clone())
            } else {
                Err(format!(
                    "human actor `{actor_id}` does not match the request tenant actor"
                ))
            }
        }
        McpPrincipalRef::ServicePrincipal { .. } => {
            let mut service_tenant = tenant_context.clone();
            service_tenant.actor_id = None;
            Ok(service_tenant)
        }
        McpPrincipalRef::LocalImplicit => {
            if tenant_context.is_local_implicit() {
                Ok(tenant_context.clone())
            } else {
                Err(
                    "local-implicit MCP connections cannot be selected from explicit tenants"
                        .to_string(),
                )
            }
        }
        McpPrincipalRef::AutomationPrincipal { .. } | McpPrincipalRef::SharedConnection { .. } => {
            Err(
                "the selected delegated MCP principal is not executable by the current bridge"
                    .to_string(),
            )
        }
    }
}

fn extract_mcp_run_as_request(args: &Value) -> McpRunAsRequest {
    let Some(object) = args.as_object() else {
        return McpRunAsRequest {
            connection_id: None,
            principal: None,
        };
    };
    let run_as = object
        .get(MCP_RUN_AS_ARG)
        .or_else(|| object.get(MCP_RUN_AS_CAMEL_ARG));
    let connection_id = object
        .get(MCP_CONNECTION_ID_ARG)
        .or_else(|| object.get(MCP_CONNECTION_ID_CAMEL_ARG))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| run_as.and_then(connection_id_from_run_as_value));
    let principal = object
        .get(MCP_PRINCIPAL_ARG)
        .or_else(|| object.get(MCP_PRINCIPAL_CAMEL_ARG))
        .and_then(parse_mcp_principal_ref)
        .or_else(|| run_as.and_then(principal_from_run_as_value));
    McpRunAsRequest {
        connection_id,
        principal,
    }
}

fn connection_id_from_run_as_value(value: &Value) -> Option<String> {
    value
        .get("connection_id")
        .or_else(|| value.get("connectionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn principal_from_run_as_value(value: &Value) -> Option<McpPrincipalRef> {
    value
        .get("principal")
        .and_then(parse_mcp_principal_ref)
        .or_else(|| parse_mcp_principal_ref(value))
}

fn parse_mcp_principal_ref(value: &Value) -> Option<McpPrincipalRef> {
    serde_json::from_value::<McpPrincipalRef>(value.clone()).ok()
}

fn strip_mcp_run_as_args(args: Value) -> Value {
    let Value::Object(mut object) = args else {
        return args;
    };
    for key in [
        MCP_CONNECTION_ID_ARG,
        MCP_CONNECTION_ID_CAMEL_ARG,
        MCP_RUN_AS_ARG,
        MCP_RUN_AS_CAMEL_ARG,
        MCP_PRINCIPAL_ARG,
        MCP_PRINCIPAL_CAMEL_ARG,
    ] {
        object.remove(key);
    }
    Value::Object(object)
}

async fn append_mcp_run_as_denial_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    requested_tenant_context: &TenantContext,
    effective_tenant_context: &TenantContext,
    requested_connection_id: Option<&str>,
    expected_connection_id: Option<&str>,
    reason: &str,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.run_as_denied",
        effective_tenant_context,
        requested_tenant_context.actor_id.clone(),
        json!({
            "reason": reason,
            "server_name": server_name,
            "tool_name": tool_name,
            "requested_connection_id": requested_connection_id,
            "expected_connection_id": expected_connection_id,
            "requested_tenant_context": requested_tenant_context,
            "effective_tenant_context": effective_tenant_context,
            "created_at_ms": now_ms(),
        }),
    )
    .await;
}

async fn append_mcp_tool_execution_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    run_as: &McpRunAsResolution,
    result: &Result<ToolResult, String>,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.tool.execution",
        &run_as.effective_tenant_context,
        run_as.requested_tenant_context.actor_id.clone(),
        json!({
            "status": if result.is_ok() { "completed" } else { "failed" },
            "server_name": server_name,
            "tool_name": tool_name,
            "connection_id": run_as.connection_id,
            "requested_connection_id": run_as.requested_connection_id,
            "principal": run_as.principal,
            "connection_class": run_as.connection_class,
            "upstream_account": run_as.upstream_account,
            "requested_tenant_context": run_as.requested_tenant_context,
            "effective_tenant_context": run_as.effective_tenant_context,
            "error": result.as_ref().err().map(|error| error.as_str()),
        }),
    )
    .await;
}

impl McpRunAsResolution {
    fn audit_payload(&self) -> Value {
        json!({
            "connectionId": self.connection_id,
            "requestedConnectionId": self.requested_connection_id,
            "principal": self.principal,
            "connectionClass": self.connection_class,
            "upstreamAccount": self.upstream_account,
            "requestedTenantContext": self.requested_tenant_context,
            "effectiveTenantContext": self.effective_tenant_context,
        })
    }
}
