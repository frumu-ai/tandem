use super::*;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tandem_runtime::McpAuthChallenge;
use tandem_types::RequestPrincipal;
use uuid::Uuid;

const BUILTIN_GITHUB_MCP_SERVER_NAME: &str = "github";
const BUILTIN_GITHUB_MCP_TRANSPORT_URL: &str = "https://api.githubcopilot.com/mcp/";
const MCP_OAUTH_SESSION_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct McpOAuthSessionRecord {
    pub session_id: String,
    pub server_name: String,
    pub status: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
    pub authorization_url: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct McpOAuthCallbackInput {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpProtectedResourceMetadata {
    authorization_servers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct McpAuthorizationServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpDynamicClientRegistrationResponse {
    client_id: String,
    client_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpTokenExchangeResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug)]
struct McpOAuthBootstrap {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    resource_metadata_url: String,
}

pub(super) async fn bootstrap_mcp_servers_when_ready(state: AppState) {
    if state.wait_until_ready_or_failed(120, 250).await {
        bootstrap_mcp_servers(&state).await;
    } else {
        tracing::warn!("mcp bootstrap: skipped because runtime startup failed or timed out");
    }
}

pub(super) async fn bootstrap_mcp_servers(state: &AppState) {
    let _ = ensure_builtin_github_mcp_server(state).await;
    let _ = ensure_hosted_kb_mcp_server(state).await;

    let mut enabled_servers = state
        .mcp
        .list()
        .await
        .into_iter()
        .filter_map(|(name, server)| if server.enabled { Some(name) } else { None })
        .collect::<Vec<_>>();
    enabled_servers.sort();

    for name in enabled_servers {
        let connected = state.mcp.connect(&name).await;
        if !connected {
            tracing::warn!("mcp bootstrap: failed to connect server '{}'", name);
            continue;
        }
        let count = sync_mcp_tools_for_server(state, &name).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.connected",
            json!({
                "name": name,
                "status": "connected",
                "source": "startup_bootstrap"
            }),
        ));
        state.event_bus.publish(EngineEvent::new(
            "mcp.tools.updated",
            json!({
                "name": name,
                "count": count,
                "source": "startup_bootstrap"
            }),
        ));
        tracing::info!(
            "mcp bootstrap: connected '{}' with {} tools registered",
            name,
            count
        );
    }
}

async fn ensure_hosted_kb_mcp_server(state: &AppState) -> bool {
    let cfg = state.config.get_effective_value().await;
    let Some(hosted) = cfg.get("hosted").and_then(Value::as_object) else {
        return false;
    };
    let kb_image = hosted
        .get("kb_image")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(kb_image) = kb_image else {
        return false;
    };
    let kb_admin_url = hosted
        .get("kb_admin_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http://tandem-kb-mcp:39736");
    let transport = format!("{}/mcp", kb_admin_url.trim_end_matches('/'));

    tracing::info!(
        "mcp bootstrap: ensuring hosted KB MCP server from {}",
        kb_image
    );
    ensure_remote_mcp_server(state, "kb", &transport, HashMap::new()).await
}

fn github_mcp_headers_from_auth() -> Option<HashMap<String, String>> {
    let token = std::env::var("GITHUB_PERSONAL_ACCESS_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("GITHUB_TOKEN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            tandem_core::load_provider_auth()
                .get("github")
                .cloned()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| {
            tandem_core::load_provider_auth()
                .get("copilot")
                .cloned()
                .filter(|value| !value.trim().is_empty())
        })?;

    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    Some(headers)
}

pub(super) async fn ensure_remote_mcp_server(
    state: &AppState,
    name: &str,
    transport_url: &str,
    headers: HashMap<String, String>,
) -> bool {
    let existing = state.mcp.list().await.get(name).cloned();
    if let Some(server) = existing {
        if !server.enabled {
            return false;
        }
        if server.transport.trim() == transport_url.trim() && !headers.is_empty() {
            let mut effective_headers = server.headers.clone();
            for (key, value) in server.secret_header_values {
                effective_headers.insert(key, value);
            }
            if effective_headers != headers {
                state
                    .mcp
                    .add_or_update(
                        name.to_string(),
                        transport_url.to_string(),
                        headers,
                        server.enabled,
                    )
                    .await;
            }
        }
        let connected = state.mcp.connect(name).await;
        if connected {
            let _ = sync_mcp_tools_for_server(state, name).await;
        }
        return connected;
    }

    state
        .mcp
        .add_or_update(name.to_string(), transport_url.to_string(), headers, true)
        .await;
    let connected = state.mcp.connect(name).await;
    if connected {
        let _ = sync_mcp_tools_for_server(state, name).await;
    }
    connected
}

pub(super) async fn ensure_builtin_github_mcp_server(state: &AppState) -> bool {
    let Some(headers) = github_mcp_headers_from_auth() else {
        let existing = state
            .mcp
            .list()
            .await
            .get(BUILTIN_GITHUB_MCP_SERVER_NAME)
            .cloned();
        if let Some(server) = existing {
            if !server.enabled {
                return false;
            }
            let connected = state.mcp.connect(BUILTIN_GITHUB_MCP_SERVER_NAME).await;
            if connected {
                let _ = sync_mcp_tools_for_server(state, BUILTIN_GITHUB_MCP_SERVER_NAME).await;
            }
            return connected;
        }
        tracing::info!(
            "mcp bootstrap: GitHub PAT not available, skipping builtin GitHub MCP server"
        );
        return false;
    };

    ensure_remote_mcp_server(
        state,
        BUILTIN_GITHUB_MCP_SERVER_NAME,
        BUILTIN_GITHUB_MCP_TRANSPORT_URL,
        headers,
    )
    .await
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct McpAddInput {
    pub name: Option<String>,
    pub transport: Option<String>,
    pub auth_kind: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub secret_headers: Option<HashMap<String, tandem_runtime::McpSecretRef>>,
    pub enabled: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct McpPatchInput {
    pub enabled: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub clear_allowed_tools: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct McpCapabilityRequestInput {
    pub agent_id: String,
    pub mcp_name: String,
    pub rationale: String,
    #[serde(default)]
    pub catalog_slug: Option<String>,
    #[serde(default)]
    pub requested_tools: Option<Vec<String>>,
    #[serde(default)]
    pub context: Option<Value>,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone)]
pub(super) struct McpBridgeTool {
    pub schema: ToolSchema,
    pub mcp: tandem_runtime::McpRegistry,
    pub server_name: String,
    pub tool_name: String,
}

#[async_trait]
impl Tool for McpBridgeTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.mcp
            .call_tool(&self.server_name, &self.tool_name, args)
            .await
            .map_err(anyhow::Error::msg)
    }
}

pub(super) async fn list_mcp(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.mcp.list_public().await))
}

fn normalize_catalog_match_key(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn catalog_entry_match_keys(entry: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for field in [
        "slug",
        "name",
        "server_name",
        "server_config_name",
        "pack_id",
    ] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            let normalized = normalize_catalog_match_key(value);
            if !normalized.is_empty() {
                keys.push(normalized);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn inventory_server_match_keys(server: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for field in ["name", "transport"] {
        if let Some(value) = server.get(field).and_then(Value::as_str) {
            let normalized = normalize_catalog_match_key(value);
            if !normalized.is_empty() {
                keys.push(normalized);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn annotate_catalog_entry(entry: &Value, inventory: Option<&Value>) -> Value {
    let mut annotated = entry.clone();
    let (enabled, connected, server_name, last_error, pending_auth_tools, registered_tool_count) =
        if let Some(inventory) = inventory {
            let enabled = inventory
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let connected = inventory
                .get("connected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let server_name = inventory
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let last_error = inventory.get("last_error").cloned().unwrap_or(Value::Null);
            let pending_auth_tools = inventory
                .get("pending_auth_tools")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let registered_tool_count = inventory
                .get("registered_tool_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (
                enabled,
                connected,
                server_name,
                last_error,
                pending_auth_tools,
                registered_tool_count,
            )
        } else {
            (false, false, String::new(), Value::Null, json!([]), 0)
        };
    let connection_status = match (enabled, connected, inventory.is_some()) {
        (true, true, true) => "connected_enabled",
        (false, true, true) => "connected_disabled",
        (false, false, true) => "cataloged_disabled",
        (true, false, true) => "cataloged_not_connected",
        (_, _, false) => "cataloged_not_connected",
    };
    if let Some(obj) = annotated.as_object_mut() {
        obj.insert("enabled".to_string(), Value::Bool(enabled));
        obj.insert("connected".to_string(), Value::Bool(connected));
        obj.insert(
            "connection_status".to_string(),
            Value::String(connection_status.to_string()),
        );
        if server_name.is_empty() {
            obj.insert("matched_server_name".to_string(), Value::Null);
        } else {
            obj.insert(
                "matched_server_name".to_string(),
                Value::String(server_name),
            );
        }
        obj.insert("last_error".to_string(), last_error);
        obj.insert("pending_auth_tools".to_string(), pending_auth_tools);
        obj.insert(
            "registered_tool_count".to_string(),
            Value::Number(serde_json::Number::from(registered_tool_count)),
        );
        obj.insert(
            "execution_available".to_string(),
            Value::Bool(enabled && connected),
        );
    }
    annotated
}

async fn mcp_catalog_overlay_snapshot(state: &AppState) -> Option<Value> {
    let mut catalog = mcp_catalog::index()?.clone();
    let inventory = mcp_inventory_snapshot(state).await;
    let inventory_servers = inventory
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut inventory_by_key: HashMap<String, Value> = HashMap::new();
    let mut matched_inventory_names = std::collections::HashSet::<String>::new();
    for server in &inventory_servers {
        for key in inventory_server_match_keys(server) {
            inventory_by_key.insert(key, server.clone());
        }
    }

    let mut annotated_servers = Vec::new();
    let mut connected_catalog_server_names = Vec::new();
    let mut connected_disabled_server_names = Vec::new();
    let mut cataloged_not_connected_server_names = Vec::new();
    let mut cataloged_disabled_server_names = Vec::new();
    let mut uncataloged_connected_servers = Vec::new();

    if let Some(servers) = catalog.get("servers").and_then(Value::as_array).cloned() {
        for entry in servers {
            let matched_inventory = catalog_entry_match_keys(&entry)
                .into_iter()
                .find_map(|key| inventory_by_key.get(&key).cloned());
            if let Some(ref inventory_row) = matched_inventory {
                if let Some(name) = inventory_row.get("name").and_then(Value::as_str) {
                    matched_inventory_names.insert(normalize_catalog_match_key(name));
                }
            }

            let annotated = annotate_catalog_entry(&entry, matched_inventory.as_ref());
            let catalog_name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match annotated
                .get("connection_status")
                .and_then(Value::as_str)
                .unwrap_or("cataloged_not_connected")
            {
                "connected_enabled" => connected_catalog_server_names.push(catalog_name),
                "connected_disabled" => connected_disabled_server_names.push(catalog_name),
                "cataloged_disabled" => cataloged_disabled_server_names.push(catalog_name),
                _ => cataloged_not_connected_server_names.push(catalog_name),
            }
            annotated_servers.push(annotated);
        }
    }

    for server in &inventory_servers {
        let connected = server
            .get("connected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !connected {
            continue;
        }
        let matched = inventory_server_match_keys(server)
            .iter()
            .any(|key| matched_inventory_names.contains(key));
        if matched {
            continue;
        }
        let mut row = server.clone();
        if let Some(obj) = row.as_object_mut() {
            obj.insert(
                "connection_status".to_string(),
                Value::String("uncataloged_connected".to_string()),
            );
        }
        uncataloged_connected_servers.push(row);
    }

    let status_counts = json!({
        "connected_enabled": connected_catalog_server_names.len(),
        "connected_disabled": connected_disabled_server_names.len(),
        "cataloged_not_connected": cataloged_not_connected_server_names.len(),
        "cataloged_disabled": cataloged_disabled_server_names.len(),
        "uncataloged_connected": uncataloged_connected_servers.len(),
    });

    if let Some(obj) = catalog.as_object_mut() {
        obj.insert("servers".to_string(), Value::Array(annotated_servers));
        obj.insert(
            "overlay".to_string(),
            json!({
                "inventory_version": inventory.get("inventory_version").cloned().unwrap_or_else(|| json!(1)),
                "connected_server_names": inventory.get("connected_server_names").cloned().unwrap_or_else(|| json!([])),
                "enabled_server_names": inventory.get("enabled_server_names").cloned().unwrap_or_else(|| json!([])),
                "connected_catalog_server_names": connected_catalog_server_names,
                "connected_disabled_server_names": connected_disabled_server_names,
                "cataloged_not_connected_server_names": cataloged_not_connected_server_names,
                "cataloged_disabled_server_names": cataloged_disabled_server_names,
                "uncataloged_connected_servers": uncataloged_connected_servers,
                "status_counts": status_counts,
            }),
        );
    }
    Some(catalog)
}

async fn mcp_request_capability_impl(
    state: &AppState,
    requested_by: crate::automation_v2::governance::GovernanceActorRef,
    input: McpCapabilityRequestInput,
) -> anyhow::Result<Value> {
    let agent_id = input.agent_id.trim().to_string();
    if agent_id.is_empty() {
        anyhow::bail!("agent_id is required");
    }
    let mcp_name = input.mcp_name.trim().to_string();
    if mcp_name.is_empty() {
        anyhow::bail!("mcp_name is required");
    }
    let rationale = input.rationale.trim().to_string();
    if rationale.is_empty() {
        anyhow::bail!("rationale is required");
    }
    let catalog = mcp_catalog_overlay_snapshot(state).await;
    let requested_slug = input
        .catalog_slug
        .as_deref()
        .map(normalize_catalog_match_key)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| normalize_catalog_match_key(&mcp_name));
    let catalog_entry = catalog.as_ref().and_then(|catalog| {
        catalog
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|servers| {
                servers.iter().find(|server| {
                    catalog_entry_match_keys(server)
                        .iter()
                        .any(|key| key == &requested_slug)
                })
            })
            .cloned()
    });
    let approval_context = json!({
        "agent_id": agent_id,
        "mcp_name": mcp_name,
        "catalog_slug": input.catalog_slug,
        "requested_tools": input.requested_tools,
        "catalog_match": catalog_entry.is_some(),
        "catalog_entry": catalog_entry,
        "capability_key": format!("mcp:{}", requested_slug),
        "context": input.context,
    });
    let request = state
        .request_approval(
            crate::automation_v2::governance::GovernanceApprovalRequestType::CapabilityRequest,
            requested_by.clone(),
            crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "agent".to_string(),
                id: agent_id,
            },
            rationale,
            approval_context,
            input.expires_at_ms,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let approval = super::governance::approval_request_wire(&request);
    Ok(json!({
        "ok": true,
        "approval": approval,
        "requested_by": requested_by,
    }))
}

pub(super) async fn add_mcp(
    State(state): State<AppState>,
    Json(input): Json<McpAddInput>,
) -> Json<Value> {
    let name = input.name.unwrap_or_else(|| "default".to_string());
    let transport = input.transport.unwrap_or_else(|| "stdio".to_string());
    let auth_kind = normalize_mcp_auth_kind(input.auth_kind.as_deref().unwrap_or_default());
    let audit_transport = transport.clone();
    state
        .mcp
        .add_or_update_with_secret_refs(
            name.clone(),
            transport,
            input.headers.unwrap_or_default(),
            input.secret_headers.unwrap_or_default(),
            input.enabled.unwrap_or(true),
        )
        .await;
    if let Some(allowed_tools) = input.allowed_tools.clone() {
        let _ = state
            .mcp
            .set_allowed_tools(&name, Some(allowed_tools))
            .await;
    }
    if !auth_kind.is_empty() {
        let _ = state.mcp.set_auth_kind(&name, auth_kind.clone()).await;
    }
    state.event_bus.publish(EngineEvent::new(
        "mcp.server.updated",
        json!({
            "name": name,
        }),
    ));
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "mcp.server.updated",
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
                "name": name,
                "transport": audit_transport,
            "enabled": input.enabled.unwrap_or(true),
            "auth_kind": auth_kind,
            "allowed_tools": input.allowed_tools,
        }),
    )
    .await;
    Json(json!({"ok": true}))
}

fn normalize_mcp_auth_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oauth" | "auto" | "bearer" | "x-api-key" | "custom" | "none" => {
            raw.trim().to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn mcp_tool_names_for_server(tool_names: &[String], server_name: &str) -> Vec<String> {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(server_name));
    let mut tools = tool_names
        .iter()
        .filter(|tool_name| tool_name.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    tools.sort();
    tools.dedup();
    tools
}

#[derive(Default)]
struct McpToolScopeFilter {
    wildcard_server_segments: std::collections::HashSet<String>,
    exact_tool_names: std::collections::HashSet<String>,
}

fn parse_mcp_tool_scope_filter(tool_names: &[String]) -> McpToolScopeFilter {
    let mut filter = McpToolScopeFilter::default();
    for raw in tool_names {
        let tool_name = raw.trim();
        if tool_name.is_empty() {
            continue;
        }
        if let Some(rest) = tool_name.strip_prefix("mcp.") {
            if let Some((server_segment, tool_segment)) = rest.split_once('.') {
                if tool_segment == "*" {
                    filter
                        .wildcard_server_segments
                        .insert(server_segment.to_string());
                } else {
                    filter
                        .exact_tool_names
                        .insert(format!("mcp.{server_segment}.{tool_segment}"));
                }
            }
        }
    }
    filter
}

fn filter_mcp_snapshot_by_tool_scope(snapshot: Value, filter: &McpToolScopeFilter) -> Value {
    let mut snapshot = snapshot;
    if filter.wildcard_server_segments.is_empty() && filter.exact_tool_names.is_empty() {
        return snapshot;
    }

    if let Some(root) = snapshot.as_object_mut() {
        if let Some(Value::Array(rows)) = root.get_mut("servers") {
            rows.retain(|row| {
                let server_name = row.get("name").and_then(Value::as_str).unwrap_or("");
                let server_segment = mcp_namespace_segment(server_name);
                if filter.wildcard_server_segments.contains(&server_segment) {
                    return true;
                }
                let exact_tools = row
                    .get("remote_tools")
                    .and_then(Value::as_array)
                    .map(|tools| {
                        tools
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<std::collections::HashSet<_>>()
                    })
                    .unwrap_or_default();
                exact_tools
                    .iter()
                    .any(|tool_name| filter.exact_tool_names.contains(tool_name))
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("connected_server_names") {
            rows.retain(|row| {
                row.as_str().is_some_and(|server| {
                    let segment = mcp_namespace_segment(server);
                    filter.wildcard_server_segments.contains(&segment)
                        || filter
                            .exact_tool_names
                            .iter()
                            .any(|tool| tool.starts_with(&format!("mcp.{segment}.")))
                })
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("enabled_server_names") {
            rows.retain(|row| {
                row.as_str().is_some_and(|server| {
                    let segment = mcp_namespace_segment(server);
                    filter.wildcard_server_segments.contains(&segment)
                        || filter
                            .exact_tool_names
                            .iter()
                            .any(|tool| tool.starts_with(&format!("mcp.{segment}.")))
                })
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("remote_tools") {
            rows.retain(|row| {
                row.get("server_name")
                    .and_then(Value::as_str)
                    .is_some_and(|server| {
                        let segment = mcp_namespace_segment(server);
                        if filter.wildcard_server_segments.contains(&segment) {
                            return true;
                        }
                        row.get("namespaced_name")
                            .and_then(Value::as_str)
                            .is_some_and(|tool_name| filter.exact_tool_names.contains(tool_name))
                    })
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("registered_tools") {
            rows.retain(|row| {
                row.as_str().is_some_and(|tool_name| {
                    tool_name == "mcp_list"
                        || filter.exact_tool_names.contains(tool_name)
                        || filter
                            .wildcard_server_segments
                            .iter()
                            .any(|segment| tool_name.starts_with(&format!("mcp.{segment}.")))
                })
            });
        }
    }

    snapshot
}

pub(crate) async fn mcp_inventory_snapshot(state: &AppState) -> Value {
    let mut server_rows = state.mcp.list().await.into_values().collect::<Vec<_>>();
    server_rows.sort_by(|a, b| a.name.cmp(&b.name));

    let remote_tools = state.mcp.list_tools().await;
    let registered_tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();

    let mut connected_server_names = Vec::new();
    let mut enabled_server_names = Vec::new();
    let mut all_remote_tool_names = Vec::new();
    let mut all_registered_tool_names = Vec::new();
    let mut servers = Vec::new();

    for server in server_rows {
        let mut remote_tool_names = remote_tools
            .iter()
            .filter(|tool| tool.server_name == server.name)
            .map(|tool| tool.namespaced_name.trim().to_string())
            .filter(|tool_name| !tool_name.is_empty())
            .collect::<Vec<_>>();
        remote_tool_names.sort();
        remote_tool_names.dedup();

        let registered_names = mcp_tool_names_for_server(&registered_tool_names, &server.name);

        if server.enabled {
            enabled_server_names.push(server.name.clone());
        }
        if server.connected {
            connected_server_names.push(server.name.clone());
        }
        all_remote_tool_names.extend(remote_tool_names.clone());
        all_registered_tool_names.extend(registered_names.clone());

        let mut pending_auth_tools = server
            .pending_auth_by_tool
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        pending_auth_tools.sort();
        pending_auth_tools.dedup();

        servers.push(json!({
            "name": server.name,
            "transport": server.transport,
            "enabled": server.enabled,
            "connected": server.connected,
            "last_error": server.last_error,
            "last_auth_challenge": server.last_auth_challenge,
            "pending_auth_tools": pending_auth_tools,
            "remote_tool_count": remote_tool_names.len(),
            "registered_tool_count": registered_names.len(),
            "allowed_tool_count": server.allowed_tools.as_ref().map(|tools| tools.len()).unwrap_or(remote_tool_names.len()),
            "allowed_tools": server.allowed_tools.clone(),
            "discovered_tool_count": server.tool_cache.len(),
            "remote_tools": remote_tool_names,
            "registered_tools": registered_names,
        }));
    }

    connected_server_names.sort();
    connected_server_names.dedup();
    enabled_server_names.sort();
    enabled_server_names.dedup();
    all_remote_tool_names.sort();
    all_remote_tool_names.dedup();
    all_registered_tool_names.sort();
    all_registered_tool_names.dedup();

    json!({
        "inventory_version": 1,
        "connected_server_names": connected_server_names,
        "enabled_server_names": enabled_server_names,
        "remote_tools": all_remote_tool_names,
        "registered_tools": all_registered_tool_names,
        "servers": servers,
    })
}

async fn current_mcp_auth_challenge(state: &AppState, name: &str) -> Option<McpAuthChallenge> {
    state
        .mcp
        .list()
        .await
        .get(name)
        .and_then(|server| server.last_auth_challenge.clone())
}

fn effective_mcp_headers(server: &tandem_runtime::McpServer) -> HashMap<String, String> {
    let mut headers = server.headers.clone();
    for (key, value) in &server.secret_header_values {
        headers.insert(key.clone(), value.clone());
    }
    headers
}

fn mcp_uses_oauth(server: &tandem_runtime::McpServer) -> bool {
    server.auth_kind.trim().eq_ignore_ascii_case("oauth")
}

fn mcp_public_base_url(state: &AppState, cfg: &Value) -> String {
    cfg.get("hosted")
        .and_then(Value::as_object)
        .and_then(|hosted| hosted.get("public_url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| state.server_base_url())
}

fn mcp_public_base_url_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(parsed) = reqwest::Url::parse(origin) {
            if let Some(host) = parsed.host_str() {
                let mut out = format!("{}://{}", parsed.scheme(), host);
                if let Some(port) = parsed.port() {
                    out.push(':');
                    out.push_str(&port.to_string());
                }
                return Some(out);
            }
        }
    }

    if let Some(referer) = headers
        .get("referer")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(parsed) = reqwest::Url::parse(referer) {
            if let Some(host) = parsed.host_str() {
                let mut out = format!("{}://{}", parsed.scheme(), host);
                if let Some(port) = parsed.port() {
                    out.push(':');
                    out.push_str(&port.to_string());
                }
                return Some(out);
            }
        }
    }

    let host = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty())?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    Some(format!("{proto}://{host}"))
}

fn mcp_oauth_redirect_uri_for_base(base_url: &str, server_name: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    format!(
        "{base}/api/engine/mcp/{}/auth/callback",
        urlencoding::encode(server_name)
    )
}

fn generate_mcp_oauth_state() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ))
}

fn generate_mcp_pkce_pair() -> (String, String) {
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ));
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

fn mcp_oauth_provider_id(server_name: &str) -> String {
    format!("mcp-oauth::{}", mcp_namespace_segment(server_name))
}

fn www_authenticate_param(header: &str, key: &str) -> Option<String> {
    for part in header.split(',') {
        let trimmed = part.trim();
        let trimmed = trimmed.strip_prefix("Bearer ").unwrap_or(trimmed);
        let (candidate_key, candidate_value) = trimmed.split_once('=')?;
        if candidate_key.trim().eq_ignore_ascii_case(key) {
            let value = candidate_value.trim().trim_matches('"').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn default_mcp_resource_metadata_url(endpoint: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(endpoint).ok()?;
    let host = parsed.host_str()?;
    let mut out = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str("/.well-known/oauth-protected-resource");
    out.push_str(parsed.path());
    Some(out)
}

fn authorization_server_metadata_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.ends_with("/.well-known/oauth-authorization-server") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/.well-known/oauth-authorization-server")
    }
}

fn build_mcp_authorization_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let pairs = [
        ("response_type", "code".to_string()),
        ("client_id", client_id.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_challenge", code_challenge.to_string()),
        ("code_challenge_method", "S256".to_string()),
        ("state", state.to_string()),
    ];
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{}", authorization_endpoint.trim(), query)
}

fn mcp_oauth_callback_html(ok: bool, title: &str, detail: &str) -> axum::response::Html<String> {
    let status = if ok { "Connected" } else { "OAuth failed" };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body style=\"font-family: sans-serif; padding: 32px;\"><h1>{status}</h1><p>{detail}</p><script>setTimeout(function(){{try{{window.close();}}catch(e){{}}}}, 500);</script></body></html>"
    );
    axum::response::Html(body)
}

async fn discover_mcp_oauth_bootstrap(
    endpoint: &str,
    headers: &HashMap<String, String>,
) -> Result<McpOAuthBootstrap, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("tandem/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("failed to build MCP OAuth client: {error}"))?;
    let mut request = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, application/json+rpc, text/event-stream",
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "initialize-oauth-discovery",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {
                    "name": "tandem",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        }));
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("mcp oauth discovery request failed: {error}"))?;
    let status = response.status();
    let www_authenticate = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read MCP OAuth discovery response: {error}"))?;
    if status.is_success() {
        return Err("mcp server did not request oauth authorization".to_string());
    }
    if status != reqwest::StatusCode::UNAUTHORIZED {
        return Err(format!(
            "mcp server returned HTTP {} during oauth discovery: {}",
            status.as_u16(),
            body.chars().take(240).collect::<String>()
        ));
    }

    let resource_metadata_url = www_authenticate
        .as_deref()
        .and_then(|header| www_authenticate_param(header, "resource_metadata"))
        .or_else(|| default_mcp_resource_metadata_url(endpoint))
        .ok_or_else(|| {
            "mcp oauth discovery did not include resource metadata in WWW-Authenticate".to_string()
        })?;

    let protected_resource = client
        .get(&resource_metadata_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("failed to fetch MCP protected resource metadata: {error}"))?;
    let protected_status = protected_resource.status();
    let protected_body = protected_resource
        .text()
        .await
        .map_err(|error| format!("failed to read MCP protected resource metadata: {error}"))?;
    if !protected_status.is_success() {
        return Err(format!(
            "protected resource metadata request failed with HTTP {}: {}",
            protected_status.as_u16(),
            protected_body.chars().take(240).collect::<String>()
        ));
    }
    let protected_metadata: McpProtectedResourceMetadata = serde_json::from_str(&protected_body)
        .map_err(|error| format!("invalid protected resource metadata: {error}"))?;
    let authorization_server = protected_metadata
        .authorization_servers
        .unwrap_or_default()
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "protected resource metadata did not include an authorization server".to_string()
        })?;

    let metadata_url = authorization_server_metadata_url(&authorization_server);
    let auth_server_response = client
        .get(&metadata_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("failed to fetch authorization server metadata: {error}"))?;
    let auth_server_status = auth_server_response.status();
    let auth_server_body = auth_server_response
        .text()
        .await
        .map_err(|error| format!("failed to read authorization server metadata: {error}"))?;
    if !auth_server_status.is_success() {
        return Err(format!(
            "authorization server metadata request failed with HTTP {}: {}",
            auth_server_status.as_u16(),
            auth_server_body.chars().take(240).collect::<String>()
        ));
    }
    let auth_metadata: McpAuthorizationServerMetadata = serde_json::from_str(&auth_server_body)
        .map_err(|error| format!("invalid authorization server metadata: {error}"))?;
    let registration_endpoint = auth_metadata
        .registration_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "authorization server does not support dynamic client registration".to_string()
        })?
        .to_string();

    Ok(McpOAuthBootstrap {
        authorization_endpoint: auth_metadata.authorization_endpoint,
        token_endpoint: auth_metadata.token_endpoint,
        registration_endpoint,
        resource_metadata_url,
    })
}

async fn start_mcp_oauth_session(
    state: &AppState,
    name: &str,
    public_base_url_hint: Option<&str>,
) -> Result<McpAuthChallenge, String> {
    let server = state
        .mcp
        .list()
        .await
        .get(name)
        .cloned()
        .ok_or_else(|| format!("MCP server '{name}' not found"))?;
    if !mcp_uses_oauth(&server) {
        return Err(format!("MCP server '{name}' is not configured for OAuth"));
    }
    let endpoint = server.transport.trim().to_string();
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        return Err("MCP OAuth is only supported for HTTP/S transports".to_string());
    }
    if let Some(existing) = current_mcp_auth_challenge(state, name).await {
        return Ok(existing);
    }

    let bootstrap =
        discover_mcp_oauth_bootstrap(&endpoint, &effective_mcp_headers(&server)).await?;
    let effective_cfg = state.config.get_effective_value().await;
    let public_base_url = public_base_url_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| mcp_public_base_url(state, &effective_cfg));
    let redirect_uri = mcp_oauth_redirect_uri_for_base(&public_base_url, name);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("tandem/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("failed to build MCP OAuth registration client: {error}"))?;
    let registration_response = client
        .post(&bootstrap.registration_endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({
            "client_name": "Tandem MCP Client",
            "client_uri": "https://tandem.ac",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|error| format!("failed to register MCP OAuth client: {error}"))?;
    let registration_status = registration_response.status();
    let registration_body = registration_response
        .text()
        .await
        .map_err(|error| format!("failed to read MCP OAuth registration response: {error}"))?;
    if !registration_status.is_success() {
        return Err(format!(
            "dynamic client registration failed with HTTP {}: {}",
            registration_status.as_u16(),
            registration_body.chars().take(240).collect::<String>()
        ));
    }
    let registration: McpDynamicClientRegistrationResponse =
        serde_json::from_str(&registration_body)
            .map_err(|error| format!("invalid dynamic client registration response: {error}"))?;
    let (code_verifier, code_challenge) = generate_mcp_pkce_pair();
    let state_token = generate_mcp_oauth_state();
    let authorization_url = build_mcp_authorization_url(
        &bootstrap.authorization_endpoint,
        &registration.client_id,
        &redirect_uri,
        &code_challenge,
        &state_token,
    );
    let challenge = McpAuthChallenge {
        challenge_id: format!("mcp-oauth-{}", Uuid::new_v4()),
        tool_name: name.to_string(),
        authorization_url: authorization_url.clone(),
        message: format!(
            "Authorization required. Open the link to connect this MCP server. Discovered OAuth metadata from {}.",
            bootstrap.resource_metadata_url
        ),
        requested_at_ms: crate::now_ms(),
        status: "pending".to_string(),
    };
    let session_id = Uuid::new_v4().to_string();
    let created_at_ms = crate::now_ms();
    state.mcp_oauth_sessions.write().await.insert(
        session_id.clone(),
        McpOAuthSessionRecord {
            session_id,
            server_name: name.to_string(),
            status: "pending".to_string(),
            created_at_ms,
            expires_at_ms: created_at_ms.saturating_add(MCP_OAUTH_SESSION_TTL_MS),
            redirect_uri,
            state: state_token,
            code_verifier,
            authorization_url: authorization_url.clone(),
            token_endpoint: bootstrap.token_endpoint,
            client_id: registration.client_id,
            client_secret: registration.client_secret,
            error: None,
        },
    );
    let _ = state
        .mcp
        .record_server_auth_challenge(name, challenge.clone(), None)
        .await;
    Ok(challenge)
}

async fn find_pending_mcp_oauth_session(
    state: &AppState,
    server_name: &str,
) -> Option<McpOAuthSessionRecord> {
    state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .find(|session| {
            session.server_name == server_name
                && session.status.trim().eq_ignore_ascii_case("pending")
                && session.expires_at_ms > crate::now_ms()
        })
        .cloned()
}

async fn exchange_mcp_oauth_code(
    session: &McpOAuthSessionRecord,
    code: &str,
) -> Result<McpTokenExchangeResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("tandem/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("failed to build MCP OAuth token client: {error}"))?;
    let mut params = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("client_id", session.client_id.clone()),
        ("redirect_uri", session.redirect_uri.clone()),
        ("code_verifier", session.code_verifier.clone()),
    ];
    if let Some(client_secret) = session
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.push(("client_secret", client_secret.to_string()));
    }
    let response = client
        .post(&session.token_endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|error| format!("mcp oauth token exchange failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read MCP OAuth token response: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "mcp oauth token exchange failed with HTTP {}: {}",
            status.as_u16(),
            body.chars().take(240).collect::<String>()
        ));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("invalid mcp oauth token response: {error}"))
}

async fn finish_mcp_oauth_callback(
    state: AppState,
    name: String,
    input: McpOAuthCallbackInput,
) -> Result<(), String> {
    let state_token = input
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing oauth state".to_string())?;
    let session_id = {
        let sessions = state.mcp_oauth_sessions.read().await;
        sessions.iter().find_map(|(session_id, session)| {
            (session.server_name == name && session.state == state_token)
                .then(|| session_id.clone())
        })
    }
    .ok_or_else(|| "mcp oauth session not found or expired".to_string())?;

    if let Some(error) = input
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let detail = input
            .error_description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        if let Some(session) = state.mcp_oauth_sessions.write().await.get_mut(&session_id) {
            session.status = "error".to_string();
            session.error = Some(detail.clone());
        }
        return Err(detail);
    }

    let code = input
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing authorization code".to_string())?;

    let session = state
        .mcp_oauth_sessions
        .read()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| "mcp oauth session not found".to_string())?;
    if session.expires_at_ms <= crate::now_ms() {
        return Err("mcp oauth session expired before callback completed".to_string());
    }

    let exchanged = exchange_mcp_oauth_code(&session, code).await?;
    let access_token = exchanged
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "mcp oauth token exchange returned no access token".to_string())?;
    let refresh_token = exchanged
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "mcp oauth token exchange returned no refresh token".to_string())?;
    let expires_at_ms =
        crate::now_ms().saturating_add(exchanged.expires_in.unwrap_or(3600).saturating_mul(1000));

    state
        .mcp
        .set_bearer_token(&name, &access_token)
        .await
        .map_err(|error| format!("failed to store mcp oauth token: {error}"))?;
    state
        .mcp
        .set_oauth_refresh_config(
            &name,
            mcp_oauth_provider_id(&name),
            session.token_endpoint.clone(),
            session.client_id.clone(),
            session.client_secret.clone(),
        )
        .await
        .map_err(|error| format!("failed to store mcp oauth refresh metadata: {error}"))?;
    let _ = tandem_core::set_provider_oauth_credential(
        &mcp_oauth_provider_id(&name),
        tandem_core::OAuthProviderCredential {
            provider_id: mcp_oauth_provider_id(&name),
            access_token: access_token.clone(),
            refresh_token,
            expires_at_ms,
            account_id: None,
            email: None,
            display_name: None,
            managed_by: "tandem".to_string(),
            api_key: None,
        },
    );

    match state.mcp.refresh(&name).await {
        Ok(_) => {
            let count = sync_mcp_tools_for_server(&state, &name).await;
            let _ = state.mcp.clear_server_auth_challenge(&name).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.connected",
                json!({
                    "name": name,
                    "status": "connected",
                    "source": "oauth_callback"
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                json!({
                    "name": name,
                    "count": count,
                    "source": "oauth_callback"
                }),
            ));
        }
        Err(error) => {
            if let Some(session_mut) = state.mcp_oauth_sessions.write().await.get_mut(&session_id) {
                session_mut.status = "error".to_string();
                session_mut.error = Some(error.clone());
            }
            return Err(error);
        }
    }

    if let Some(session_mut) = state.mcp_oauth_sessions.write().await.get_mut(&session_id) {
        session_mut.status = "connected".to_string();
        session_mut.error = None;
    }
    Ok(())
}

fn filter_mcp_inventory_snapshot_to_servers(snapshot: Value, allowed_servers: &[String]) -> Value {
    let mut snapshot = snapshot;
    let allowed_servers = allowed_servers
        .iter()
        .map(|server| server.trim().to_string())
        .filter(|server| !server.is_empty())
        .collect::<std::collections::HashSet<_>>();
    if allowed_servers.is_empty() {
        return snapshot;
    }
    let allowed_tool_prefixes = allowed_servers
        .iter()
        .map(|server| format!("mcp.{}.", mcp_namespace_segment(server)))
        .collect::<Vec<_>>();

    let keep_server = |name: &str| allowed_servers.contains(name);

    if let Some(root) = snapshot.as_object_mut() {
        if let Some(Value::Array(rows)) = root.get_mut("servers") {
            rows.retain(|row| {
                row.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(keep_server)
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("connected_server_names") {
            rows.retain(|row| row.as_str().is_some_and(keep_server));
        }
        if let Some(Value::Array(rows)) = root.get_mut("enabled_server_names") {
            rows.retain(|row| row.as_str().is_some_and(keep_server));
        }
        if let Some(Value::Array(rows)) = root.get_mut("remote_tools") {
            rows.retain(|row| {
                row.get("server_name")
                    .and_then(Value::as_str)
                    .is_some_and(keep_server)
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("registered_tools") {
            rows.retain(|row| {
                row.as_str().is_some_and(|tool_name| {
                    tool_name == "mcp_list"
                        || allowed_tool_prefixes
                            .iter()
                            .any(|prefix| tool_name.starts_with(prefix))
                })
            });
        }
    }

    snapshot
}

fn filter_mcp_snapshot_by_exact_and_wildcard_tools(
    snapshot: Value,
    allowed_tools: &[String],
) -> Value {
    let filter = parse_mcp_tool_scope_filter(allowed_tools);
    filter_mcp_snapshot_by_tool_scope(snapshot, &filter)
}

/// Filter MCP inventory by namespace segments (e.g. `["tandem_mcp"]`) derived
/// from `session_allowed_tools` patterns like `mcp.tandem_mcp.*`.  Server names
/// are matched by applying `mcp_namespace_segment` so that `"tandem-mcp"` matches
/// the segment `"tandem_mcp"`.
fn filter_mcp_snapshot_by_namespace_segments(
    snapshot: Value,
    allowed_segments: &[String],
) -> Value {
    let mut snapshot = snapshot;
    let segments_set: std::collections::HashSet<&str> =
        allowed_segments.iter().map(|s| s.as_str()).collect();
    let keep_server = |name: &str| segments_set.contains(mcp_namespace_segment(name).as_str());
    let allowed_tool_prefixes: Vec<String> = allowed_segments
        .iter()
        .map(|seg| format!("mcp.{}.", seg))
        .collect();

    if let Some(root) = snapshot.as_object_mut() {
        if let Some(Value::Array(rows)) = root.get_mut("servers") {
            rows.retain(|row| {
                row.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(keep_server)
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("connected_server_names") {
            rows.retain(|row| row.as_str().is_some_and(keep_server));
        }
        if let Some(Value::Array(rows)) = root.get_mut("enabled_server_names") {
            rows.retain(|row| row.as_str().is_some_and(keep_server));
        }
        if let Some(Value::Array(rows)) = root.get_mut("remote_tools") {
            rows.retain(|row| {
                row.get("server_name")
                    .and_then(Value::as_str)
                    .is_some_and(keep_server)
            });
        }
        if let Some(Value::Array(rows)) = root.get_mut("registered_tools") {
            rows.retain(|row| {
                row.as_str().is_some_and(|tool_name| {
                    tool_name == "mcp_list"
                        || allowed_tool_prefixes
                            .iter()
                            .any(|prefix| tool_name.starts_with(prefix))
                })
            });
        }
    }
    snapshot
}

fn session_mcp_tool_filter(session_tools: &[String]) -> McpToolScopeFilter {
    parse_mcp_tool_scope_filter(session_tools)
}

async fn scoped_mcp_servers_for_session(state: &AppState, session_id: &str) -> Vec<String> {
    state
        .automation_v2_session_mcp_servers
        .read()
        .await
        .get(session_id)
        .cloned()
        .unwrap_or_default()
}

#[derive(Clone)]
pub(crate) struct McpListTool {
    state: AppState,
}

impl McpListTool {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for McpListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp_list",
            "List the currently configured and connected MCP servers and tools",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let mut snapshot = mcp_inventory_snapshot(&self.state).await;
        let session_id = args.get("__session_id").and_then(Value::as_str);
        let allowed_servers = if let Some(sid) = session_id {
            scoped_mcp_servers_for_session(&self.state, sid).await
        } else {
            Vec::new()
        };
        // If no automation-level MCP scoping, check session_allowed_tools
        // (set by per-request tool_allowlist from channel dispatchers).
        let mut session_tool_filter = McpToolScopeFilter::default();
        if let Some(sid) = session_id {
            if let Some(rt) = self.state.runtime.get() {
                let session_tools = rt.engine_loop.get_session_allowed_tools(sid).await;
                session_tool_filter = session_mcp_tool_filter(&session_tools);
            }
        }
        if !allowed_servers.is_empty() {
            snapshot = filter_mcp_inventory_snapshot_to_servers(snapshot, &allowed_servers);
        }
        if !session_tool_filter.wildcard_server_segments.is_empty()
            || !session_tool_filter.exact_tool_names.is_empty()
        {
            snapshot = filter_mcp_snapshot_by_tool_scope(snapshot, &session_tool_filter);
        }
        let output =
            serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
        Ok(ToolResult {
            output,
            metadata: snapshot,
        })
    }
}

#[derive(Clone)]
pub(crate) struct McpListCatalogTool {
    state: AppState,
}

impl McpListCatalogTool {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for McpListCatalogTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp_list_catalog",
            "List the embedded MCP catalog with connection overlay for discovery and gap analysis",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let Some(snapshot) = mcp_catalog_overlay_snapshot(&self.state).await else {
            anyhow::bail!("MCP catalog is unavailable");
        };
        let output =
            serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
        Ok(ToolResult {
            output,
            metadata: snapshot,
        })
    }
}

#[derive(Clone)]
pub(crate) struct McpRequestCapabilityTool {
    state: AppState,
}

impl McpRequestCapabilityTool {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for McpRequestCapabilityTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp_request_capability",
            "Request human approval for an MCP capability gap without attempting to connect or execute it directly",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "mcp_name": { "type": "string" },
                    "catalog_slug": { "type": "string" },
                    "rationale": { "type": "string" },
                    "requested_tools": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "context": { "type": "object" },
                    "expires_at_ms": { "type": "integer" }
                },
                "required": ["agent_id", "mcp_name", "rationale"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input: McpCapabilityRequestInput = serde_json::from_value(args.clone())
            .map_err(|error| anyhow::anyhow!("invalid capability request input: {error}"))?;
        let session_source = args
            .get("__session_id")
            .and_then(Value::as_str)
            .map(|value| format!("tool:{value}"))
            .unwrap_or_else(|| "tool:mcp_request_capability".to_string());
        let requested_by = crate::automation_v2::governance::GovernanceActorRef::agent(
            Some(input.agent_id.clone()),
            session_source,
        );
        let output = mcp_request_capability_impl(&self.state, requested_by, input).await?;
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
}

pub(crate) fn mcp_namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

pub(crate) async fn sync_mcp_tools_for_server(state: &AppState, name: &str) -> usize {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(name));
    state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.server_tools(name).await;
    for tool in &tools {
        let schema = ToolSchema::new(
            tool.namespaced_name.clone(),
            if tool.description.trim().is_empty() {
                format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
            } else {
                tool.description.clone()
            },
            tool.input_schema.clone(),
        );
        state
            .tools
            .register_tool(
                schema.name.clone(),
                Arc::new(McpBridgeTool {
                    schema,
                    mcp: state.mcp.clone(),
                    server_name: tool.server_name.clone(),
                    tool_name: tool.tool_name.clone(),
                }),
            )
            .await;
    }
    tools.len()
}

pub(super) async fn connect_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let ok = state.mcp.connect(&name).await;
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    let auth_challenge = if ok {
        None
    } else {
        let current = current_mcp_auth_challenge(&state, &name).await;
        if current.is_some() {
            current
        } else {
            let server = state.mcp.list().await.get(&name).cloned();
            if server.as_ref().is_some_and(mcp_uses_oauth) {
                start_mcp_oauth_session(&state, &name, public_base_url.as_deref())
                    .await
                    .ok()
            } else {
                None
            }
        }
    };
    if ok {
        let count = sync_mcp_tools_for_server(&state, &name).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.connected",
            json!({
                "name": name,
                "status": "connected",
            }),
        ));
        state.event_bus.publish(EngineEvent::new(
            "mcp.tools.updated",
            json!({
                "name": name,
                "count": count,
            }),
        ));
    } else {
        let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
        let removed = state.tools.unregister_by_prefix(&prefix).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.disconnected",
            json!({
                "name": name,
                "removedToolCount": removed,
                "reason": "connect_failed"
            }),
        ));
    }
    Json(json!({
        "ok": ok,
        "pendingAuth": auth_challenge.is_some(),
        "lastAuthChallenge": auth_challenge,
        "authorizationUrl": auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
    }))
}

pub(super) async fn disconnect_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let ok = state.mcp.disconnect(&name).await;
    if ok {
        let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
        let removed = state.tools.unregister_by_prefix(&prefix).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.disconnected",
            json!({
                "name": name,
                "removedToolCount": removed,
            }),
        ));
    }
    Json(json!({"ok": ok}))
}

pub(super) async fn delete_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
    let removed_tool_count = state.tools.unregister_by_prefix(&prefix).await;
    let ok = state.mcp.remove(&name).await;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.deleted",
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
            }),
        ));
        let _ = crate::audit::append_protected_audit_event(
            &state,
            "mcp.server.deleted",
            &tandem_types::TenantContext::local_implicit(),
            None,
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
            }),
        )
        .await;
    }
    Json(json!({ "ok": ok, "removedToolCount": removed_tool_count }))
}

pub(super) async fn patch_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<McpPatchInput>,
) -> Json<Value> {
    let mut changed = false;
    let mut should_resync = false;
    if input.clear_allowed_tools.unwrap_or(false) || input.allowed_tools.is_some() {
        let next_allowed_tools = if input.clear_allowed_tools.unwrap_or(false) {
            None
        } else {
            input.allowed_tools.clone()
        };
        changed |= state.mcp.set_allowed_tools(&name, next_allowed_tools).await;
        should_resync = true;
    }
    if let Some(enabled) = input.enabled {
        let enabled_changed = state.mcp.set_enabled(&name, enabled).await;
        changed |= enabled_changed;
        if enabled_changed {
            if enabled {
                let _ = state.mcp.connect(&name).await;
            } else {
                let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
                let _ = state.tools.unregister_by_prefix(&prefix).await;
            }
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.updated",
                json!({
                    "name": name,
                    "enabled": enabled,
                }),
            ));
            let _ = crate::audit::append_protected_audit_event(
                &state,
                "mcp.server.updated",
                &tandem_types::TenantContext::local_implicit(),
                None,
                json!({
                    "name": name,
                    "enabled": enabled,
                }),
            )
            .await;
            if enabled {
                should_resync = true;
            }
        }
    }
    if should_resync {
        let server = state.mcp.list().await.get(&name).cloned();
        if server
            .as_ref()
            .is_some_and(|server| server.enabled && server.connected)
        {
            let count = sync_mcp_tools_for_server(&state, &name).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                json!({
                    "name": name,
                    "count": count,
                }),
            ));
        }
    }
    Json(json!({"ok": changed}))
}

pub(super) async fn refresh_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let result = state.mcp.refresh(&name).await;
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    match result {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server(&state, &name).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                json!({
                    "name": name,
                    "count": count,
                }),
            ));
            Json(json!({
                "ok": true,
                "count": tools.len(),
            }))
        }
        Err(error) => {
            let mut auth_challenge = current_mcp_auth_challenge(&state, &name).await;
            if auth_challenge.is_none() {
                let server = state.mcp.list().await.get(&name).cloned();
                if server.as_ref().is_some_and(mcp_uses_oauth) {
                    auth_challenge =
                        start_mcp_oauth_session(&state, &name, public_base_url.as_deref())
                            .await
                            .ok();
                }
            }
            let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
            let removed = state.tools.unregister_by_prefix(&prefix).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.disconnected",
                json!({
                    "name": name,
                    "removedToolCount": removed,
                    "reason": "refresh_failed"
                }),
            ));
            Json(json!({
                "ok": false,
                "error": error,
                "pendingAuth": auth_challenge.is_some(),
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
                "removedToolCount": removed
            }))
        }
    }
}

pub(super) async fn auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    if let Some(auth_challenge) = current_mcp_auth_challenge(&state, &name).await {
        return Json(json!({
            "ok": true,
            "pending": true,
            "lastAuthChallenge": auth_challenge,
            "authorizationUrl": auth_challenge.authorization_url,
        }));
    }
    Json(json!({
        "ok": false,
        "pending": false,
        "name": name,
        "message": "No MCP auth challenge recorded yet.",
    }))
}

pub(super) async fn callback_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    authenticate_mcp(State(state), Path(name), headers).await
}

pub(super) async fn callback_mcp_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(input): Query<McpOAuthCallbackInput>,
) -> impl IntoResponse {
    match finish_mcp_oauth_callback(state, name, input).await {
        Ok(()) => mcp_oauth_callback_html(
            true,
            "Tandem MCP Connected",
            "The MCP OAuth sign-in completed successfully. You can close this window.",
        )
        .into_response(),
        Err(error) => {
            mcp_oauth_callback_html(false, "Tandem MCP OAuth Failed", &error).into_response()
        }
    }
}

pub(super) async fn authenticate_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Json<Value> {
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    if let Some(session) = find_pending_mcp_oauth_session(&state, &name).await {
        let last_auth_challenge = current_mcp_auth_challenge(&state, &name).await;
        return Json(json!({
            "ok": true,
            "authenticated": false,
            "connected": false,
            "pendingAuth": true,
            "lastAuthChallenge": last_auth_challenge,
            "authorizationUrl": last_auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()).unwrap_or(session.authorization_url),
        }));
    }

    let refresh = state.mcp.refresh(&name).await;
    let current = state.mcp.list().await.get(&name).cloned();
    let last_auth_challenge = current
        .as_ref()
        .and_then(|server| server.last_auth_challenge.clone());
    match refresh {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server(&state, &name).await;
            let _ = state.mcp.clear_server_auth_challenge(&name).await;
            Json(json!({
                "ok": true,
                "authenticated": true,
                "connected": true,
                "pendingAuth": false,
                "lastAuthChallenge": Value::Null,
                "authorizationUrl": Value::Null,
                "count": count.max(tools.len()),
            }))
        }
        Err(error) => {
            let mut auth_challenge = last_auth_challenge;
            if auth_challenge.is_none() {
                let server = state.mcp.list().await.get(&name).cloned();
                if server.as_ref().is_some_and(mcp_uses_oauth) {
                    auth_challenge =
                        start_mcp_oauth_session(&state, &name, public_base_url.as_deref())
                            .await
                            .ok();
                }
            }
            Json(json!({
                "ok": false,
                "authenticated": false,
                "connected": current.as_ref().map(|server| server.connected).unwrap_or(false),
                "pendingAuth": auth_challenge.is_some(),
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
                "error": error,
            }))
        }
    }
}

pub(super) async fn delete_auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    disconnect_mcp(State(state), Path(name)).await
}

pub(super) async fn mcp_catalog_index(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(index) = mcp_catalog_overlay_snapshot(&state).await {
        return Ok(Json(index));
    }
    Err(StatusCode::SERVICE_UNAVAILABLE)
}

pub(super) async fn mcp_request_capability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<McpCapabilityRequestInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let requested_by =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let response = mcp_request_capability_impl(&state, requested_by, input)
        .await
        .map_err(|error| {
            let message = error.to_string();
            let status = if message.contains("is required") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(json!({
                    "error": message,
                    "code": "MCP_CAPABILITY_REQUEST_FAILED",
                })),
            )
        })?;
    Ok(Json(response))
}

pub(super) async fn mcp_catalog_toml(Path(slug): Path<String>) -> Result<Response, StatusCode> {
    if let Some(toml) = mcp_catalog::toml_for_slug(&slug) {
        return Ok((
            [(header::CONTENT_TYPE, "application/toml; charset=utf-8")],
            toml,
        )
            .into_response());
    }
    Err(StatusCode::NOT_FOUND)
}

pub(super) async fn mcp_tools(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.mcp.list_tools().await))
}

pub(super) async fn mcp_resources(State(state): State<AppState>) -> Json<Value> {
    let resources = state
        .mcp
        .list()
        .await
        .into_values()
        .filter(|server| server.connected)
        .map(|server| {
            json!({
                "server": server.name,
                "resources": [
                    {"uri": format!("mcp://{}/tools", server.name), "name":"tools"},
                    {"uri": format!("mcp://{}/prompts", server.name), "name":"prompts"}
                ]
            })
        })
        .collect::<Vec<_>>();
    Json(json!(resources))
}
