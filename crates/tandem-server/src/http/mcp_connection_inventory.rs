use std::collections::HashMap;

use axum::extract::{Extension, State};
use axum::Json;
use serde_json::{json, Value};
use tandem_runtime::{McpConnection, McpConnectionClass, McpPrincipalRef};
use tandem_types::{TenantContext, VerifiedTenantContext};

use super::{mcp::mcp_namespace_segment, AppState};

pub(super) async fn list_mcp(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> Json<Value> {
    Json(
        public_mcp_inventory_with_connections(
            &state,
            &tenant_context,
            verified_tenant_context
                .as_ref()
                .map(|extension| &extension.0),
        )
        .await,
    )
}

pub(super) async fn public_mcp_inventory_with_connections(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Value {
    let mut connections = state
        .mcp
        .list_connections()
        .await
        .into_values()
        .filter(|connection| {
            mcp_connection_visible_to_request(connection, tenant_context, verified_tenant_context)
        })
        .collect::<Vec<_>>();
    connections.sort_by(|left, right| {
        left.server_id
            .cmp(&right.server_id)
            .then_with(|| left.connection_id.cmp(&right.connection_id))
    });

    let mut connections_by_server: HashMap<String, Vec<Value>> = HashMap::new();
    for connection in connections {
        connections_by_server
            .entry(connection.server_id.clone())
            .or_default()
            .push(public_mcp_connection_view(&connection));
    }

    let mut inventory = serde_json::Map::new();
    for (server_name, server) in state.mcp.list_public().await {
        let mut server_value = json!(server);
        if let Some(object) = server_value.as_object_mut() {
            object.insert(
                "connections".to_string(),
                Value::Array(
                    connections_by_server
                        .remove(&server_name)
                        .unwrap_or_default(),
                ),
            );
        }
        inventory.insert(server_name, server_value);
    }

    Value::Object(inventory)
}

fn mcp_connection_visible_to_request(
    connection: &McpConnection,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> bool {
    if tenant_context.is_local_implicit() {
        return connection.tenant_context.is_local_implicit();
    }
    if !mcp_tenant_scope_matches(&connection.tenant_context, tenant_context) {
        return false;
    }
    if mcp_request_is_tenant_admin(verified_tenant_context) {
        return true;
    }
    match connection.connection_class {
        McpConnectionClass::UserOwned => {
            connection.tenant_context == *tenant_context
                || matches!(
                    &connection.owner,
                    McpPrincipalRef::HumanActor { actor_id }
                        if tenant_context.actor_id.as_deref() == Some(actor_id.as_str())
                )
        }
        McpConnectionClass::ServiceAccount
        | McpConnectionClass::SharedReadOnly
        | McpConnectionClass::SharedReadWrite
        | McpConnectionClass::AdminManaged => true,
    }
}

fn mcp_tenant_scope_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn mcp_request_is_tenant_admin(verified_tenant_context: Option<&VerifiedTenantContext>) -> bool {
    let Some(verified) = verified_tenant_context else {
        return false;
    };
    verified.roles.iter().any(|role| {
        matches!(
            role.as_str(),
            "owner"
                | "admin"
                | "hosted:owner"
                | "hosted:admin"
                | "enterprise:admin"
                | "workspace:admin"
                | "organization:admin"
        )
    }) || verified.capabilities.iter().any(|capability| {
        matches!(
            capability.as_str(),
            "hosted.owner" | "hosted.admin" | "mcp.admin" | "automation.share"
        )
    })
}

fn public_mcp_connection_view(connection: &McpConnection) -> Value {
    let oauth_provider_id = connection
        .oauth
        .as_ref()
        .map(|oauth| oauth.provider_id.as_str());
    let tool_cache = connection
        .tool_cache
        .iter()
        .map(|tool| {
            let namespaced_name = format!(
                "mcp.{}.{}",
                mcp_namespace_segment(&connection.server_id),
                mcp_namespace_segment(&tool.tool_name)
            );
            json!({
                "tool_name": &tool.tool_name,
                "toolName": &tool.tool_name,
                "namespaced_name": namespaced_name,
                "namespacedName": namespaced_name,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "connection_id": &connection.connection_id,
        "connectionId": &connection.connection_id,
        "connection_generation": &connection.connection_generation,
        "connectionGeneration": &connection.connection_generation,
        "server": &connection.server_id,
        "server_id": &connection.server_id,
        "serverId": &connection.server_id,
        "tenant_context": &connection.tenant_context,
        "tenantContext": &connection.tenant_context,
        "owner": &connection.owner,
        "principal": &connection.owner,
        "connection_class": &connection.connection_class,
        "connectionClass": &connection.connection_class,
        "connected": connection.connected,
        "enabled": connection.enabled,
        "last_error": &connection.last_error,
        "lastError": &connection.last_error,
        "last_auth_challenge": &connection.last_auth_challenge,
        "lastAuthChallenge": &connection.last_auth_challenge,
        "tool_count": connection.tool_cache.len(),
        "toolCount": connection.tool_cache.len(),
        "tool_cache": &tool_cache,
        "toolCache": &tool_cache,
        "tools_fetched_at_ms": connection.tools_fetched_at_ms,
        "toolsFetchedAtMs": connection.tools_fetched_at_ms,
        "upstream_account": &connection.upstream_account,
        "upstreamAccount": &connection.upstream_account,
        "oauth_provider_id": oauth_provider_id,
        "oauthProviderId": oauth_provider_id,
        "local_implicit": connection.tenant_context.is_local_implicit(),
        "localImplicit": connection.tenant_context.is_local_implicit(),
        "created_at_ms": connection.created_at_ms,
        "createdAtMs": connection.created_at_ms,
        "updated_at_ms": connection.updated_at_ms,
        "updatedAtMs": connection.updated_at_ms,
    })
}
