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
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub tool_cache: Vec<McpToolCacheEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_fetched_at_ms: Option<u64>,
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
        self.add_or_update(name, transport, HashMap::new(), true).await;
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
        let existing_tool_cache = existing
            .as_ref()
            .map(|row| row.tool_cache.clone())
            .unwrap_or_default();
        let existing_fetched_at = existing.as_ref().and_then(|row| row.tools_fetched_at_ms);
        let server = McpServer {
            name: name.clone(),
            transport,
            enabled,
            connected: false,
            pid: None,
            last_error: None,
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

        let tools = match self.discover_remote_tools(&endpoint, &server.headers).await {
            Ok(tools) => tools,
            Err(err) => {
                let mut servers = self.servers.write().await;
                if let Some(entry) = servers.get_mut(name) {
                    entry.connected = false;
                    entry.pid = None;
                    entry.last_error = Some(err.clone());
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

        let endpoint = parse_remote_endpoint(&server.transport)
            .ok_or_else(|| "MCP tools/call currently supports HTTP/S transports only".to_string())?;

        let request = json!({
            "jsonrpc": "2.0",
            "id": format!("call-{}-{}", server_name, now_ms()),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": args
            }
        });
        let response = post_json_rpc(&endpoint, &server.headers, request).await?;

        if let Some(err) = response.get("error") {
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP tools/call failed");
            return Err(message.to_string());
        }

        let result = response.get("result").cloned().unwrap_or(Value::Null);
        let output = result
            .get("content")
            .map(render_mcp_content)
            .or_else(|| result.get("output").map(|v| v.to_string()))
            .unwrap_or_else(|| result.to_string());

        Ok(ToolResult {
            output,
            metadata: json!({
                "server": server_name,
                "tool": tool_name,
                "result": result
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
    ) -> Result<Vec<McpRemoteTool>, String> {
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
        let init_response = post_json_rpc(endpoint, headers, initialize).await?;
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
        let tools_response = post_json_rpc(endpoint, headers, tools_list).await?;
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
            let input_schema = row
                .get("inputSchema")
                .or_else(|| row.get("input_schema"))
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
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

        Ok(out)
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
    PathBuf::from(".tandem").join("mcp_servers.json")
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

async fn post_json_rpc(
    endpoint: &str,
    headers: &HashMap<String, String>,
    request: Value,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    let response = client
        .post(endpoint)
        .headers(build_headers(headers)?)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("MCP request failed: {e}"))?;
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
    serde_json::from_str::<Value>(&payload)
        .map_err(|e| format!("Invalid MCP JSON response: {e}"))
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
}
