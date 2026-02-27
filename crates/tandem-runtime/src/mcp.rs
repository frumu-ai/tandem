use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_types::ToolResult;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_CLIENT_NAME: &str = "tandem";
const MCP_CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCacheEntry {
    pub tool_name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    pub fetched_at_ms: u64,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub transport: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_auth_challenge: Option<McpAuthChallenge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_session_id: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub tool_cache: Vec<McpToolCacheEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_fetched_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthChallenge {
    pub challenge_id: String,
    pub tool_name: String,
    pub authorization_url: String,
    pub message: String,
    pub requested_at_ms: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoteTool {
    pub server_name: String,
    pub tool_name: String,
    pub namespaced_name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    pub fetched_at_ms: u64,
    pub schema_hash: String,
}

#[derive(Clone)]
pub struct McpRegistry {
    servers: Arc<RwLock<HashMap<String, McpServer>>>,
    processes: Arc<Mutex<HashMap<String, Child>>>,
    state_file: Arc<PathBuf>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::new_with_state_file(resolve_state_file())
    }

    pub fn new_with_state_file(state_file: PathBuf) -> Self {
        let loaded = load_state(&state_file)
            .into_iter()
            .map(|(k, mut v)| {
                v.connected = false;
                v.pid = None;
                if v.name.trim().is_empty() {
                    v.name = k.clone();
                }
                if v.headers.is_empty() {
                    v.headers = HashMap::new();
                }
                (k, v)
            })
            .collect::<HashMap<_, _>>();
        Self {
            servers: Arc::new(RwLock::new(loaded)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            state_file: Arc::new(state_file),
        }
    }

    pub async fn list(&self) -> HashMap<String, McpServer> {
        self.servers.read().await.clone()
    }

    pub async fn add(&self, name: String, transport: String) {
        self.add_or_update(name, transport, HashMap::new(), true)
            .await;
    }

    pub async fn add_or_update(
        &self,
        name: String,
        transport: String,
        headers: HashMap<String, String>,
        enabled: bool,
    ) {
        let mut servers = self.servers.write().await;
        let existing = servers.get(&name).cloned();
        let preserve_cache = existing
            .as_ref()
            .is_some_and(|row| row.transport == transport && row.headers == headers);
        let existing_tool_cache = if preserve_cache {
            existing
                .as_ref()
                .map(|row| row.tool_cache.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let existing_fetched_at = if preserve_cache {
            existing.as_ref().and_then(|row| row.tools_fetched_at_ms)
        } else {
            None
        };
        let server = McpServer {
            name: name.clone(),
            transport,
            enabled,
            connected: false,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers,
            tool_cache: existing_tool_cache,
            tools_fetched_at_ms: existing_fetched_at,
        };
        servers.insert(name, server);
        drop(servers);
        self.persist_state().await;
    }

    pub async fn set_enabled(&self, name: &str, enabled: bool) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        server.enabled = enabled;
        if !enabled {
            server.connected = false;
            server.pid = None;
            server.last_auth_challenge = None;
            server.mcp_session_id = None;
        }
        drop(servers);
        if !enabled {
            if let Some(mut child) = self.processes.lock().await.remove(name) {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        self.persist_state().await;
        true
    }

    pub async fn remove(&self, name: &str) -> bool {
        let removed = {
            let mut servers = self.servers.write().await;
            servers.remove(name).is_some()
        };
        if !removed {
            return false;
        }

        if let Some(mut child) = self.processes.lock().await.remove(name) {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        self.persist_state().await;
        true
    }

    pub async fn connect(&self, name: &str) -> bool {
        let server = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(name) else {
                return false;
            };
            server.clone()
        };

        if !server.enabled {
            let mut servers = self.servers.write().await;
            if let Some(entry) = servers.get_mut(name) {
                entry.connected = false;
                entry.pid = None;
                entry.last_error = Some("MCP server is disabled".to_string());
                entry.last_auth_challenge = None;
                entry.mcp_session_id = None;
            }
            drop(servers);
            self.persist_state().await;
            return false;
        }

        if let Some(command_text) = parse_stdio_transport(&server.transport) {
            return self.connect_stdio(name, command_text).await;
        }

        if parse_remote_endpoint(&server.transport).is_some() {
            return self.refresh(name).await.is_ok();
        }

        let mut servers = self.servers.write().await;
        if let Some(entry) = servers.get_mut(name) {
            entry.connected = true;
            entry.pid = None;
            entry.last_error = None;
            entry.last_auth_challenge = None;
            entry.mcp_session_id = None;
        }
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn refresh(&self, name: &str) -> Result<Vec<McpRemoteTool>, String> {
        let server = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(name) else {
                return Err("MCP server not found".to_string());
            };
            server.clone()
        };

        if !server.enabled {
            return Err("MCP server is disabled".to_string());
        }

        let endpoint = parse_remote_endpoint(&server.transport)
            .ok_or_else(|| "MCP refresh currently supports HTTP/S transports only".to_string())?;

        let (tools, session_id) = match self.discover_remote_tools(&endpoint, &server.headers).await
        {
            Ok(result) => result,
            Err(err) => {
                let mut servers = self.servers.write().await;
                if let Some(entry) = servers.get_mut(name) {
                    entry.connected = false;
                    entry.pid = None;
                    entry.last_error = Some(err.clone());
                    entry.last_auth_challenge = None;
                    entry.mcp_session_id = None;
                    entry.tool_cache.clear();
                    entry.tools_fetched_at_ms = None;
                }
                drop(servers);
                self.persist_state().await;
                return Err(err);
            }
        };

        let now = now_ms();
        let cache = tools
            .iter()
            .map(|tool| McpToolCacheEntry {
                tool_name: tool.tool_name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                fetched_at_ms: now,
                schema_hash: schema_hash(&tool.input_schema),
            })
            .collect::<Vec<_>>();

        let mut servers = self.servers.write().await;
        if let Some(entry) = servers.get_mut(name) {
            entry.connected = true;
            entry.pid = None;
            entry.last_error = None;
            entry.last_auth_challenge = None;
            entry.mcp_session_id = session_id;
            entry.tool_cache = cache;
            entry.tools_fetched_at_ms = Some(now);
        }
        drop(servers);
        self.persist_state().await;
        Ok(self.server_tools(name).await)
    }

    pub async fn disconnect(&self, name: &str) -> bool {
        if let Some(mut child) = self.processes.lock().await.remove(name) {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.get_mut(name) {
            server.connected = false;
            server.pid = None;
            server.last_auth_challenge = None;
            server.mcp_session_id = None;
            drop(servers);
            self.persist_state().await;
            return true;
        }
        false
    }

    pub async fn list_tools(&self) -> Vec<McpRemoteTool> {
        let mut out = self
            .servers
            .read()
            .await
            .values()
            .filter(|server| server.enabled && server.connected)
            .flat_map(server_tool_rows)
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.namespaced_name.cmp(&b.namespaced_name));
        out
    }

    pub async fn server_tools(&self, name: &str) -> Vec<McpRemoteTool> {
        let Some(server) = self.servers.read().await.get(name).cloned() else {
            return Vec::new();
        };
        let mut rows = server_tool_rows(&server);
        rows.sort_by(|a, b| a.namespaced_name.cmp(&b.namespaced_name));
        rows
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<ToolResult, String> {
        let server = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(server_name) else {
                return Err(format!("MCP server '{server_name}' not found"));
            };
            server.clone()
        };

        if !server.enabled {
            return Err(format!("MCP server '{server_name}' is disabled"));
        }
        if !server.connected {
            return Err(format!("MCP server '{server_name}' is not connected"));
        }

        let endpoint = parse_remote_endpoint(&server.transport).ok_or_else(|| {
            "MCP tools/call currently supports HTTP/S transports only".to_string()
        })?;
        let normalized_args = normalize_mcp_tool_args(&server, tool_name, args);

        let request = json!({
            "jsonrpc": "2.0",
            "id": format!("call-{}-{}", server_name, now_ms()),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": normalized_args
            }
        });
        let (response, session_id) = post_json_rpc_with_session(
            &endpoint,
            &server.headers,
            request,
            server.mcp_session_id.as_deref(),
        )
        .await?;
        if session_id.is_some() {
            let mut servers = self.servers.write().await;
            if let Some(row) = servers.get_mut(server_name) {
                row.mcp_session_id = session_id;
            }
            drop(servers);
            self.persist_state().await;
        }

        if let Some(err) = response.get("error") {
            if let Some(challenge) = extract_auth_challenge(err, tool_name) {
                let output = format!(
                    "{}\n\nAuthorize here: {}",
                    challenge.message, challenge.authorization_url
                );
                {
                    let mut servers = self.servers.write().await;
                    if let Some(row) = servers.get_mut(server_name) {
                        row.last_auth_challenge = Some(challenge.clone());
                        row.last_error = None;
                    }
                }
                self.persist_state().await;
                return Ok(ToolResult {
                    output,
                    metadata: json!({
                        "server": server_name,
                        "tool": tool_name,
                        "result": Value::Null,
                        "mcpAuth": {
                            "required": true,
                            "challengeId": challenge.challenge_id,
                            "tool": challenge.tool_name,
                            "authorizationUrl": challenge.authorization_url,
                            "message": challenge.message,
                            "status": challenge.status
                        }
                    }),
                });
            }
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP tools/call failed");
            return Err(message.to_string());
        }

        let result = response.get("result").cloned().unwrap_or(Value::Null);
        let auth_challenge = extract_auth_challenge(&result, tool_name);
        let output = if let Some(challenge) = auth_challenge.as_ref() {
            format!(
                "{}\n\nAuthorize here: {}",
                challenge.message, challenge.authorization_url
            )
        } else {
            result
                .get("content")
                .map(render_mcp_content)
                .or_else(|| result.get("output").map(|v| v.to_string()))
                .unwrap_or_else(|| result.to_string())
        };

        {
            let mut servers = self.servers.write().await;
            if let Some(row) = servers.get_mut(server_name) {
                row.last_auth_challenge = auth_challenge.clone();
            }
        }
        self.persist_state().await;

        let auth_metadata = auth_challenge.as_ref().map(|challenge| {
            json!({
                "required": true,
                "challengeId": challenge.challenge_id,
                "tool": challenge.tool_name,
                "authorizationUrl": challenge.authorization_url,
                "message": challenge.message,
                "status": challenge.status
            })
        });

        Ok(ToolResult {
            output,
            metadata: json!({
                "server": server_name,
                "tool": tool_name,
                "result": result,
                "mcpAuth": auth_metadata
            }),
        })
    }

    async fn connect_stdio(&self, name: &str, command_text: &str) -> bool {
        match spawn_stdio_process(command_text).await {
            Ok(child) => {
                let pid = child.id();
                self.processes.lock().await.insert(name.to_string(), child);
                let mut servers = self.servers.write().await;
                if let Some(server) = servers.get_mut(name) {
                    server.connected = true;
                    server.pid = pid;
                    server.last_error = None;
                }
                drop(servers);
                self.persist_state().await;
                true
            }
            Err(err) => {
                let mut servers = self.servers.write().await;
                if let Some(server) = servers.get_mut(name) {
                    server.connected = false;
                    server.pid = None;
                    server.last_error = Some(err);
                }
                drop(servers);
                self.persist_state().await;
                false
            }
        }
    }

    async fn discover_remote_tools(
        &self,
        endpoint: &str,
        headers: &HashMap<String, String>,
    ) -> Result<(Vec<McpRemoteTool>, Option<String>), String> {
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": "initialize-1",
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": MCP_CLIENT_NAME,
                    "version": MCP_CLIENT_VERSION,
                }
            }
        });
        let (init_response, mut session_id) =
            post_json_rpc_with_session(endpoint, headers, initialize, None).await?;
        if let Some(err) = init_response.get("error") {
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP initialize failed");
            return Err(message.to_string());
        }

        let tools_list = json!({
            "jsonrpc": "2.0",
            "id": "tools-list-1",
            "method": "tools/list",
            "params": {}
        });
        let (tools_response, next_session_id) =
            post_json_rpc_with_session(endpoint, headers, tools_list, session_id.as_deref())
                .await?;
        if next_session_id.is_some() {
            session_id = next_session_id;
        }
        if let Some(err) = tools_response.get("error") {
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP tools/list failed");
            return Err(message.to_string());
        }

        let tools = tools_response
            .get("result")
            .and_then(|v| v.get("tools"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| "MCP tools/list result missing tools array".to_string())?;

        let now = now_ms();
        let mut out = Vec::new();
        for row in tools {
            let Some(tool_name) = row.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let description = row
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut input_schema = row
                .get("inputSchema")
                .or_else(|| row.get("input_schema"))
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            normalize_tool_input_schema(&mut input_schema);
            out.push(McpRemoteTool {
                server_name: String::new(),
                tool_name: tool_name.to_string(),
                namespaced_name: String::new(),
                description,
                input_schema,
                fetched_at_ms: now,
                schema_hash: String::new(),
            });
        }

        Ok((out, session_id))
    }

    async fn persist_state(&self) {
        let snapshot = self.servers.read().await.clone();
        if let Some(parent) = self.state_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Ok(payload) = serde_json::to_string_pretty(&snapshot) {
            let _ = tokio::fs::write(self.state_file.as_path(), payload).await;
        }
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn default_enabled() -> bool {
    true
}

fn resolve_state_file() -> PathBuf {
    if let Ok(path) = std::env::var("TANDEM_MCP_REGISTRY") {
        return PathBuf::from(path);
    }
    if let Ok(state_dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = state_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("mcp_servers.json");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir
            .join("tandem")
            .join("data")
            .join("mcp_servers.json");
    }
    dirs::home_dir()
        .map(|home| home.join(".tandem").join("data").join("mcp_servers.json"))
        .unwrap_or_else(|| PathBuf::from("mcp_servers.json"))
}

fn load_state(path: &Path) -> HashMap<String, McpServer> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<HashMap<String, McpServer>>(&raw).unwrap_or_default()
}

fn parse_stdio_transport(transport: &str) -> Option<&str> {
    transport.strip_prefix("stdio:").map(str::trim)
}

fn parse_remote_endpoint(transport: &str) -> Option<String> {
    let trimmed = transport.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    for prefix in ["http:", "https:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let endpoint = rest.trim();
            if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                return Some(endpoint.to_string());
            }
        }
    }
    None
}

fn server_tool_rows(server: &McpServer) -> Vec<McpRemoteTool> {
    let server_slug = sanitize_namespace_segment(&server.name);
    server
        .tool_cache
        .iter()
        .map(|tool| {
            let tool_slug = sanitize_namespace_segment(&tool.tool_name);
            McpRemoteTool {
                server_name: server.name.clone(),
                tool_name: tool.tool_name.clone(),
                namespaced_name: format!("mcp.{server_slug}.{tool_slug}"),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                fetched_at_ms: tool.fetched_at_ms,
                schema_hash: tool.schema_hash.clone(),
            }
        })
        .collect()
}

fn sanitize_namespace_segment(raw: &str) -> String {
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
        "tool".to_string()
    } else {
        cleaned.to_string()
    }
}

fn schema_hash(schema: &Value) -> String {
    let payload = serde_json::to_vec(schema).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(payload);
    format!("{:x}", hasher.finalize())
}

fn extract_auth_challenge(result: &Value, tool_name: &str) -> Option<McpAuthChallenge> {
    let authorization_url = find_string_by_any_key(
        result,
        &["authorization_url", "authorizationUrl", "auth_url"],
    )?;
    let message = find_string_by_any_key(result, &["llm_instructions", "message", "text"])
        .unwrap_or_else(|| "This tool requires authorization before it can run.".to_string());
    let challenge_id = stable_id_seed(&format!("{tool_name}:{authorization_url}"));
    Some(McpAuthChallenge {
        challenge_id,
        tool_name: tool_name.to_string(),
        authorization_url,
        message,
        requested_at_ms: now_ms(),
        status: "pending".to_string(),
    })
}

fn find_string_by_any_key(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(|v| v.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_string_by_any_key(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_by_any_key(item, keys)),
        _ => None,
    }
}

fn stable_id_seed(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let encoded = format!("{:x}", hasher.finalize());
    encoded.chars().take(16).collect()
}

fn normalize_tool_input_schema(schema: &mut Value) {
    normalize_schema_node(schema);
}

fn normalize_schema_node(node: &mut Value) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };

    // Some MCP servers publish enums on non-string/object/array fields, which
    // OpenAI-compatible providers may reject (e.g. Gemini via OpenRouter).
    // Keep enum only when values are all strings and schema type is string-like.
    if let Some(enum_values) = obj.get("enum").and_then(|v| v.as_array()) {
        let all_strings = enum_values.iter().all(|v| v.is_string());
        let string_like_type = schema_type_allows_string_enum(obj.get("type"));
        if !all_strings || !string_like_type {
            obj.remove("enum");
        }
    }

    if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
        for value in properties.values_mut() {
            normalize_schema_node(value);
        }
    }

    if let Some(items) = obj.get_mut("items") {
        normalize_schema_node(items);
    }

    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(array) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            for child in array.iter_mut() {
                normalize_schema_node(child);
            }
        }
    }

    if let Some(additional) = obj.get_mut("additionalProperties") {
        normalize_schema_node(additional);
    }
}

fn schema_type_allows_string_enum(schema_type: Option<&Value>) -> bool {
    let Some(schema_type) = schema_type else {
        // No explicit type: keep enum to avoid over-normalizing loosely-typed schemas.
        return true;
    };

    if let Some(kind) = schema_type.as_str() {
        return kind == "string";
    }

    if let Some(kinds) = schema_type.as_array() {
        let mut saw_string = false;
        for kind in kinds {
            let Some(kind) = kind.as_str() else {
                return false;
            };
            if kind == "string" {
                saw_string = true;
                continue;
            }
            if kind != "null" {
                return false;
            }
        }
        return saw_string;
    }

    false
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn build_headers(headers: &HashMap<String, String>) -> Result<HeaderMap, String> {
    let mut map = HeaderMap::new();
    map.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (key, value) in headers {
        let name = HeaderName::from_bytes(key.trim().as_bytes())
            .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
        let header = HeaderValue::from_str(value.trim())
            .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
        map.insert(name, header);
    }
    Ok(map)
}

async fn post_json_rpc_with_session(
    endpoint: &str,
    headers: &HashMap<String, String>,
    request: Value,
    session_id: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    let mut req = client.post(endpoint).headers(build_headers(headers)?);
    if let Some(id) = session_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            req = req.header("Mcp-Session-Id", trimmed);
        }
    }
    let response = req
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("MCP request failed: {e}"))?;
    let response_session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|e| format!("Failed to read MCP response: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "MCP endpoint returned HTTP {}: {}",
            status.as_u16(),
            payload.chars().take(400).collect::<String>()
        ));
    }
    let value = serde_json::from_str::<Value>(&payload)
        .map_err(|e| format!("Invalid MCP JSON response: {e}"))?;
    Ok((value, response_session_id))
}

fn render_mcp_content(value: &Value) -> String {
    let Some(items) = value.as_array() else {
        return value.to_string();
    };
    let mut chunks = Vec::new();
    for item in items {
        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
            chunks.push(text.to_string());
            continue;
        }
        chunks.push(item.to_string());
    }
    if chunks.is_empty() {
        value.to_string()
    } else {
        chunks.join("\n")
    }
}

fn normalize_mcp_tool_args(server: &McpServer, tool_name: &str, raw_args: Value) -> Value {
    let Some(schema) = server
        .tool_cache
        .iter()
        .find(|row| row.tool_name.eq_ignore_ascii_case(tool_name))
        .map(|row| &row.input_schema)
    else {
        return raw_args;
    };

    let mut args_obj = match raw_args {
        Value::Object(obj) => obj,
        other => return other,
    };

    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if properties.is_empty() {
        return Value::Object(args_obj);
    }

    // Build a normalized-key lookup so taskTitle -> task_title and list-id -> list_id resolve.
    let mut normalized_existing: HashMap<String, String> = HashMap::new();
    for key in args_obj.keys() {
        normalized_existing.insert(normalize_arg_key(key), key.clone());
    }

    // Copy values from normalized aliases to canonical schema property names.
    let canonical_keys = properties.keys().cloned().collect::<Vec<_>>();
    for canonical in &canonical_keys {
        if args_obj.contains_key(canonical) {
            continue;
        }
        if let Some(existing_key) = normalized_existing.get(&normalize_arg_key(canonical)) {
            if let Some(value) = args_obj.get(existing_key).cloned() {
                args_obj.insert(canonical.clone(), value);
            }
        }
    }

    // Fill required fields using conservative aliases when models choose common alternatives.
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for required_key in required {
        if args_obj.contains_key(&required_key) {
            continue;
        }
        if let Some(alias_value) = find_required_alias_value(&required_key, &args_obj) {
            args_obj.insert(required_key, alias_value);
        }
    }

    Value::Object(args_obj)
}

fn find_required_alias_value(
    required_key: &str,
    args_obj: &serde_json::Map<String, Value>,
) -> Option<Value> {
    let mut alias_candidates = vec![
        required_key.to_string(),
        required_key.to_ascii_lowercase(),
        required_key.replace('_', ""),
    ];

    // Common fallback for fields like task_title where models often send `name`.
    if required_key.contains("title") {
        alias_candidates.extend([
            "name".to_string(),
            "title".to_string(),
            "task_name".to_string(),
            "taskname".to_string(),
        ]);
    }

    // Common fallback for *_id fields where models emit `<base>` or `<base>Id`.
    if let Some(base) = required_key.strip_suffix("_id") {
        alias_candidates.extend([base.to_string(), format!("{base}id"), format!("{base}_id")]);
    }

    let mut by_normalized: HashMap<String, &Value> = HashMap::new();
    for (key, value) in args_obj {
        by_normalized.insert(normalize_arg_key(key), value);
    }

    alias_candidates
        .into_iter()
        .find_map(|candidate| by_normalized.get(&normalize_arg_key(&candidate)).cloned())
        .cloned()
}

fn normalize_arg_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

async fn spawn_stdio_process(command_text: &str) -> Result<Child, String> {
    if command_text.is_empty() {
        return Err("Missing stdio command".to_string());
    }
    #[cfg(windows)]
    let mut command = {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", command_text]);
        cmd
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command_text]);
        cmd
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.spawn().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn add_connect_disconnect_non_stdio_server() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("example".to_string(), "sse:https://example.com".to_string())
            .await;
        assert!(registry.connect("example").await);
        let listed = registry.list().await;
        assert!(listed.get("example").map(|s| s.connected).unwrap_or(false));
        assert!(registry.disconnect("example").await);
    }

    #[test]
    fn parse_remote_endpoint_supports_http_prefixes() {
        assert_eq!(
            parse_remote_endpoint("https://mcp.example.com/mcp"),
            Some("https://mcp.example.com/mcp".to_string())
        );
        assert_eq!(
            parse_remote_endpoint("http:https://mcp.example.com/mcp"),
            Some("https://mcp.example.com/mcp".to_string())
        );
    }

    #[test]
    fn normalize_schema_removes_non_string_enums_recursively() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "good": { "type": "string", "enum": ["a", "b"] },
                "good_nullable": { "type": ["string", "null"], "enum": ["asc", "desc"] },
                "bad_object": { "type": "object", "enum": ["asc", "desc"] },
                "bad_array": { "type": "array", "enum": ["asc", "desc"] },
                "bad_number": { "type": "number", "enum": [1, 2] },
                "bad_mixed": { "enum": ["ok", 1] },
                "nested": {
                    "type": "object",
                    "properties": {
                        "child": { "enum": [true, false] }
                    }
                }
            }
        });

        normalize_tool_input_schema(&mut schema);

        assert!(
            schema["properties"]["good"]["enum"].is_array(),
            "string enums should be preserved"
        );
        assert!(
            schema["properties"]["good_nullable"]["enum"].is_array(),
            "string|null enums should be preserved"
        );
        assert!(
            schema["properties"]["bad_object"]["enum"].is_null(),
            "object enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_array"]["enum"].is_null(),
            "array enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_number"]["enum"].is_null(),
            "non-string enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_mixed"]["enum"].is_null(),
            "mixed enums should be dropped"
        );
        assert!(
            schema["properties"]["nested"]["properties"]["child"]["enum"].is_null(),
            "recursive non-string enums should be dropped"
        );
    }

    #[test]
    fn extract_auth_challenge_from_result_payload() {
        let payload = json!({
            "content": [
                {
                    "type": "text",
                    "llm_instructions": "Authorize Gmail access first.",
                    "authorization_url": "https://example.com/oauth/start"
                }
            ]
        });
        let challenge = extract_auth_challenge(&payload, "gmail_whoami")
            .expect("auth challenge should be detected");
        assert_eq!(challenge.tool_name, "gmail_whoami");
        assert_eq!(
            challenge.authorization_url,
            "https://example.com/oauth/start"
        );
        assert_eq!(challenge.status, "pending");
    }

    #[test]
    fn extract_auth_challenge_returns_none_without_url() {
        let payload = json!({
            "content": [
                {"type":"text","text":"No authorization needed"}
            ]
        });
        assert!(extract_auth_challenge(&payload, "gmail_whoami").is_none());
    }

    #[test]
    fn normalize_mcp_tool_args_maps_clickup_aliases() {
        let server = McpServer {
            name: "arcade".to_string(),
            transport: "https://example.com/mcp".to_string(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::new(),
            tool_cache: vec![McpToolCacheEntry {
                tool_name: "Clickup_CreateTask".to_string(),
                description: "Create task".to_string(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "list_id":{"type":"string"},
                        "task_title":{"type":"string"}
                    },
                    "required":["list_id","task_title"]
                }),
                fetched_at_ms: 0,
                schema_hash: "x".to_string(),
            }],
            tools_fetched_at_ms: None,
        };

        let normalized = normalize_mcp_tool_args(
            &server,
            "Clickup_CreateTask",
            json!({
                "listId": "123",
                "name": "Prep fish"
            }),
        );
        assert_eq!(
            normalized.get("list_id").and_then(|v| v.as_str()),
            Some("123")
        );
        assert_eq!(
            normalized.get("task_title").and_then(|v| v.as_str()),
            Some("Prep fish")
        );
    }

    #[test]
    fn normalize_arg_key_ignores_case_and_separators() {
        assert_eq!(normalize_arg_key("task_title"), "tasktitle");
        assert_eq!(normalize_arg_key("taskTitle"), "tasktitle");
        assert_eq!(normalize_arg_key("task-title"), "tasktitle");
    }
}
