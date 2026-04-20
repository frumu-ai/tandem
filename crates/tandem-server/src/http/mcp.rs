use super::*;
use tandem_runtime::McpAuthChallenge;

const BUILTIN_GITHUB_MCP_SERVER_NAME: &str = "github";
const BUILTIN_GITHUB_MCP_TRANSPORT_URL: &str = "https://api.githubcopilot.com/mcp/";

pub(super) async fn bootstrap_mcp_servers_when_ready(state: AppState) {
    if state.wait_until_ready_or_failed(120, 250).await {
        bootstrap_mcp_servers(&state).await;
    } else {
        tracing::warn!("mcp bootstrap: skipped because runtime startup failed or timed out");
    }
}

pub(super) async fn bootstrap_mcp_servers(state: &AppState) {
    let _ = ensure_builtin_github_mcp_server(state).await;

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
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct McpPatchInput {
    pub enabled: Option<bool>,
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
        let mut allowed_servers = if let Some(sid) = session_id {
            scoped_mcp_servers_for_session(&self.state, sid).await
        } else {
            Vec::new()
        };
        // If no automation-level MCP scoping, check session_allowed_tools
        // (set by per-request tool_allowlist from channel dispatchers).
        if allowed_servers.is_empty() {
            if let Some(sid) = session_id {
                if let Some(rt) = self.state.runtime.get() {
                    let session_tools = rt.engine_loop.get_session_allowed_tools(sid).await;
                    let allowed_segments: Vec<String> = session_tools
                        .iter()
                        .filter_map(|pattern| {
                            pattern
                                .strip_prefix("mcp.")
                                .and_then(|rest| rest.strip_suffix(".*"))
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if !allowed_segments.is_empty() {
                        snapshot =
                            filter_mcp_snapshot_by_namespace_segments(snapshot, &allowed_segments);
                    }
                }
            }
        } else {
            snapshot = filter_mcp_inventory_snapshot_to_servers(snapshot, &allowed_servers);
        }
        let output =
            serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
        Ok(ToolResult {
            output,
            metadata: snapshot,
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
) -> Json<Value> {
    let ok = state.mcp.connect(&name).await;
    let auth_challenge = if ok {
        None
    } else {
        current_mcp_auth_challenge(&state, &name).await
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
    if let Some(enabled) = input.enabled {
        changed = state.mcp.set_enabled(&name, enabled).await;
        if changed {
            if enabled {
                let _ = state.mcp.connect(&name).await;
                let count = sync_mcp_tools_for_server(&state, &name).await;
                state.event_bus.publish(EngineEvent::new(
                    "mcp.tools.updated",
                    json!({
                        "name": name,
                        "count": count,
                    }),
                ));
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
        }
    }
    Json(json!({"ok": changed}))
}

pub(super) async fn refresh_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let result = state.mcp.refresh(&name).await;
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
            let auth_challenge = current_mcp_auth_challenge(&state, &name).await;
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
) -> Json<Value> {
    authenticate_mcp(State(state), Path(name)).await
}

pub(super) async fn authenticate_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let ok = state.mcp.complete_auth(&name).await;
    let current = state.mcp.list().await.get(&name).cloned();
    let last_auth_challenge = current
        .as_ref()
        .and_then(|server| server.last_auth_challenge.clone());
    Json(json!({
        "ok": ok,
        "authenticated": ok,
        "connected": current.as_ref().map(|server| server.connected).unwrap_or(false),
        "pendingAuth": last_auth_challenge.is_some(),
        "lastAuthChallenge": last_auth_challenge,
        "authorizationUrl": last_auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
    }))
}

pub(super) async fn delete_auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    disconnect_mcp(State(state), Path(name)).await
}

pub(super) async fn mcp_catalog_index() -> Result<Json<Value>, StatusCode> {
    if let Some(index) = mcp_catalog::index() {
        return Ok(Json(index.clone()));
    }
    Err(StatusCode::SERVICE_UNAVAILABLE)
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
