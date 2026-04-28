use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_types::{LocalImplicitTenant, SecretRef, TenantContext, ToolResult};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};

use crate::mcp_ready::{EnsureReadyPolicy, McpReadyError};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_CLIENT_NAME: &str = "tandem";
const MCP_CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MCP_AUTH_REPROBE_COOLDOWN_MS: u64 = 15_000;
const MCP_SECRET_PLACEHOLDER: &str = "";

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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_kind: String,
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
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub secret_headers: HashMap<String, McpSecretRef>,
    #[serde(default)]
    pub tool_cache: Vec<McpToolCacheEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_fetched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pending_auth_by_tool: HashMap<String, PendingMcpAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    #[serde(default)]
    pub grounding_required: bool,
    #[serde(default, skip)]
    pub secret_header_values: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpSecretRef {
    Store {
        secret_id: String,
        #[serde(default)]
        tenant_context: TenantContext,
    },
    Env {
        env: String,
    },
    BearerEnv {
        env: String,
    },
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
pub struct PendingMcpAuth {
    pub challenge_id: String,
    pub authorization_url: String,
    pub message: String,
    pub status: String,
    pub first_seen_ms: u64,
    pub last_probe_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    pub provider_id: String,
    pub token_endpoint: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_ref: Option<McpSecretRef>,
    #[serde(default, skip)]
    pub client_secret_value: Option<String>,
}

#[derive(Debug, Clone)]
enum DiscoverRemoteToolsError {
    Message(String),
    AuthChallenge(McpAuthChallenge),
}

impl From<String> for DiscoverRemoteToolsError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
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
        let (loaded_state, migrated) = load_state(&state_file);
        let loaded = loaded_state
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
                if v.secret_headers.is_empty() {
                    v.secret_headers = HashMap::new();
                }
                let tenant_context = local_tenant_context();
                v.secret_header_values =
                    resolve_secret_header_values(&v.secret_headers, &tenant_context);
                if let Some(oauth) = v.oauth.as_mut() {
                    oauth.client_secret_value =
                        oauth.client_secret_ref.as_ref().and_then(|secret_ref| {
                            resolve_secret_ref_value(secret_ref, &tenant_context)
                        });
                }
                (k, v)
            })
            .collect::<HashMap<_, _>>();
        if migrated {
            persist_state_blocking(&state_file, &loaded);
        }
        Self {
            servers: Arc::new(RwLock::new(loaded)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            state_file: Arc::new(state_file),
        }
    }

    pub async fn list(&self) -> HashMap<String, McpServer> {
        self.servers.read().await.clone()
    }

    pub async fn list_public(&self) -> HashMap<String, McpServer> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(name, server)| (name.clone(), redacted_server_view(server)))
            .collect()
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
        self.add_or_update_with_secret_refs(name, transport, headers, HashMap::new(), enabled)
            .await;
    }

    pub async fn add_or_update_with_secret_refs(
        &self,
        name: String,
        transport: String,
        headers: HashMap<String, String>,
        secret_headers: HashMap<String, McpSecretRef>,
        enabled: bool,
    ) {
        let normalized_name = name.trim().to_string();
        let tenant_context = local_tenant_context();
        let (persisted_headers, persisted_secret_headers, secret_header_values) =
            split_headers_for_storage(&normalized_name, headers, secret_headers, &tenant_context);
        let mut servers = self.servers.write().await;
        let existing = servers.get(&normalized_name).cloned();
        let preserve_cache = existing.as_ref().is_some_and(|row| {
            row.transport == transport
                && effective_headers(row)
                    == combine_headers(&persisted_headers, &secret_header_values)
        });
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
            name: normalized_name.clone(),
            transport,
            auth_kind: existing
                .as_ref()
                .map(|row| row.auth_kind.clone())
                .unwrap_or_default(),
            enabled,
            connected: false,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: persisted_headers,
            secret_headers: persisted_secret_headers,
            tool_cache: existing_tool_cache,
            tools_fetched_at_ms: existing_fetched_at,
            pending_auth_by_tool: HashMap::new(),
            allowed_tools: existing.as_ref().and_then(|row| row.allowed_tools.clone()),
            purpose: existing
                .as_ref()
                .map(|row| row.purpose.clone())
                .unwrap_or_default(),
            grounding_required: existing
                .as_ref()
                .map(|row| row.grounding_required)
                .unwrap_or(false),
            secret_header_values,
            oauth: existing.as_ref().and_then(|row| row.oauth.clone()),
        };
        servers.insert(normalized_name, server);
        drop(servers);
        self.persist_state().await;
    }

    pub async fn set_allowed_tools(&self, name: &str, allowed_tools: Option<Vec<String>>) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        let normalized = allowed_tools.map(normalize_allowed_tool_names);
        if server.allowed_tools == normalized {
            return true;
        }
        server.allowed_tools = normalized;
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn set_grounding_metadata(
        &self,
        name: &str,
        purpose: Option<String>,
        grounding_required: Option<bool>,
    ) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        let mut changed = false;
        if let Some(purpose) = purpose {
            let normalized = purpose.trim().to_ascii_lowercase();
            if server.purpose != normalized {
                server.purpose = normalized;
                changed = true;
            }
        }
        if let Some(grounding_required) = grounding_required {
            if server.grounding_required != grounding_required {
                server.grounding_required = grounding_required;
                changed = true;
            }
        }
        drop(servers);
        if changed {
            self.persist_state().await;
        }
        true
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
            server.pending_auth_by_tool.clear();
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
        let removed_server = {
            let mut servers = self.servers.write().await;
            servers.remove(name)
        };
        let Some(server) = removed_server else {
            return false;
        };
        let current_tenant = local_tenant_context();
        delete_secret_header_refs(&server.secret_headers, &current_tenant);
        delete_oauth_secret_ref(server.oauth.as_ref(), &current_tenant);

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
                entry.pending_auth_by_tool.clear();
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
            entry.pending_auth_by_tool.clear();
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

        let _ = self.ensure_oauth_bearer_token_fresh(name, false).await;
        let server = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(name) else {
                return Err("MCP server not found".to_string());
            };
            server.clone()
        };
        let request_headers = effective_headers(&server);
        let discovery = self
            .discover_remote_tools(name, &endpoint, &request_headers)
            .await;
        let (tools, session_id) = match discovery {
            Ok(result) => result,
            Err(DiscoverRemoteToolsError::AuthChallenge(challenge)) => {
                let mut servers = self.servers.write().await;
                if let Some(entry) = servers.get_mut(name) {
                    entry.connected = false;
                    entry.pid = None;
                    entry.last_error = Some(challenge.message.clone());
                    entry.last_auth_challenge = Some(challenge.clone());
                    entry.mcp_session_id = None;
                    entry.pending_auth_by_tool.clear();
                    entry.tool_cache.clear();
                    entry.tools_fetched_at_ms = None;
                }
                drop(servers);
                self.persist_state().await;
                return Err(format!(
                    "MCP server '{name}' requires authorization: {}",
                    challenge.message
                ));
            }
            Err(DiscoverRemoteToolsError::Message(err)) => {
                if should_retry_mcp_oauth_refresh(&server, &err)
                    && self.ensure_oauth_bearer_token_fresh(name, true).await?
                {
                    let refreshed_server = {
                        let servers = self.servers.read().await;
                        servers
                            .get(name)
                            .cloned()
                            .ok_or_else(|| "MCP server not found".to_string())?
                    };
                    match self
                        .discover_remote_tools(
                            name,
                            &endpoint,
                            &effective_headers(&refreshed_server),
                        )
                        .await
                    {
                        Ok(result) => result,
                        Err(DiscoverRemoteToolsError::AuthChallenge(challenge)) => {
                            let mut servers = self.servers.write().await;
                            if let Some(entry) = servers.get_mut(name) {
                                entry.connected = false;
                                entry.pid = None;
                                entry.last_error = Some(challenge.message.clone());
                                entry.last_auth_challenge = Some(challenge.clone());
                                entry.mcp_session_id = None;
                                entry.pending_auth_by_tool.clear();
                                entry.tool_cache.clear();
                                entry.tools_fetched_at_ms = None;
                            }
                            drop(servers);
                            self.persist_state().await;
                            return Err(format!(
                                "MCP server '{name}' requires authorization: {}",
                                challenge.message
                            ));
                        }
                        Err(DiscoverRemoteToolsError::Message(retry_err)) => {
                            let mut servers = self.servers.write().await;
                            if let Some(entry) = servers.get_mut(name) {
                                entry.connected = false;
                                entry.pid = None;
                                entry.last_error = Some(retry_err.clone());
                                entry.last_auth_challenge = None;
                                entry.mcp_session_id = None;
                                entry.pending_auth_by_tool.clear();
                                entry.tool_cache.clear();
                                entry.tools_fetched_at_ms = None;
                            }
                            drop(servers);
                            self.persist_state().await;
                            return Err(retry_err);
                        }
                    }
                } else {
                    let mut servers = self.servers.write().await;
                    if let Some(entry) = servers.get_mut(name) {
                        entry.connected = false;
                        entry.pid = None;
                        entry.last_error = Some(err.clone());
                        entry.last_auth_challenge = None;
                        entry.mcp_session_id = None;
                        entry.pending_auth_by_tool.clear();
                        entry.tool_cache.clear();
                        entry.tools_fetched_at_ms = None;
                    }
                    drop(servers);
                    self.persist_state().await;
                    return Err(err);
                }
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
            entry.pending_auth_by_tool.clear();
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
            server.pending_auth_by_tool.clear();
            drop(servers);
            self.persist_state().await;
            return true;
        }
        false
    }

    pub async fn complete_auth(&self, name: &str) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        server.last_error = None;
        server.last_auth_challenge = None;
        server.pending_auth_by_tool.clear();
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn set_auth_kind(&self, name: &str, auth_kind: String) -> bool {
        let normalized = normalize_auth_kind(&auth_kind);
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        server.auth_kind = normalized;
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn record_server_auth_challenge(
        &self,
        name: &str,
        challenge: McpAuthChallenge,
        last_error: Option<String>,
    ) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        let tool_key = canonical_tool_key(&challenge.tool_name);
        server.connected = false;
        server.pid = None;
        server.last_error = last_error.or_else(|| Some(challenge.message.clone()));
        server.last_auth_challenge = Some(challenge.clone());
        server.mcp_session_id = None;
        server.pending_auth_by_tool.clear();
        server
            .pending_auth_by_tool
            .insert(tool_key, pending_auth_from_challenge(&challenge));
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn clear_server_auth_challenge(&self, name: &str) -> bool {
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return false;
        };
        server.last_auth_challenge = None;
        server.pending_auth_by_tool.clear();
        drop(servers);
        self.persist_state().await;
        true
    }

    pub async fn set_bearer_token(&self, name: &str, token: &str) -> Result<bool, String> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err("oauth access token cannot be empty".to_string());
        }
        let current_tenant = local_tenant_context();
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return Ok(false);
        };
        let header_name = "Authorization".to_string();
        let secret_id = mcp_header_secret_id(name, &header_name);
        tandem_core::set_provider_auth(&secret_id, &format!("Bearer {trimmed}"))
            .map_err(|error| error.to_string())?;
        server.secret_headers.insert(
            header_name.clone(),
            McpSecretRef::Store {
                secret_id: secret_id.clone(),
                tenant_context: current_tenant,
            },
        );
        server
            .secret_header_values
            .insert(header_name.clone(), format!("Bearer {trimmed}"));
        server.headers.remove(&header_name);
        drop(servers);
        self.persist_state().await;
        Ok(true)
    }

    pub async fn set_oauth_refresh_config(
        &self,
        name: &str,
        provider_id: String,
        token_endpoint: String,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<bool, String> {
        let current_tenant = local_tenant_context();
        let mut servers = self.servers.write().await;
        let Some(server) = servers.get_mut(name) else {
            return Ok(false);
        };

        let client_secret_ref = client_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| -> Result<McpSecretRef, String> {
                let secret_id = mcp_oauth_client_secret_id(name);
                tandem_core::set_provider_auth(&secret_id, value)
                    .map_err(|error| error.to_string())?;
                Ok(McpSecretRef::Store {
                    secret_id,
                    tenant_context: current_tenant.clone(),
                })
            })
            .transpose()?;
        if client_secret_ref.is_none() {
            let secret_id = mcp_oauth_client_secret_id(name);
            let _ = tandem_core::delete_provider_auth(&secret_id);
        }

        server.oauth = Some(McpOAuthConfig {
            provider_id,
            token_endpoint,
            client_id,
            client_secret_ref,
            client_secret_value: client_secret
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        });
        drop(servers);
        self.persist_state().await;
        Ok(true)
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
        // Single readiness gate (Invariant 2 of `docs/SPINE.md`): one
        // attempt, no backoff — same shape as the previous inline check.
        let server = match self
            .ensure_ready(server_name, EnsureReadyPolicy::default())
            .await
        {
            Ok(server) => server,
            Err(McpReadyError::NotFound) => {
                return Err(format!("MCP server '{server_name}' not found"));
            }
            Err(McpReadyError::Disabled) => {
                return Err(format!("MCP server '{server_name}' is disabled"));
            }
            Err(McpReadyError::PermanentlyFailed { last_error }) => {
                let detail =
                    last_error.unwrap_or_else(|| "reconnect attempt failed".to_string());
                return Err(format!(
                    "MCP server '{server_name}' is not connected: {detail}"
                ));
            }
        };

        let endpoint = parse_remote_endpoint(&server.transport).ok_or_else(|| {
            "MCP tools/call currently supports HTTP/S transports only".to_string()
        })?;
        let canonical_tool = canonical_tool_key(tool_name);
        let now = now_ms();
        let _ = self
            .ensure_oauth_bearer_token_fresh(server_name, false)
            .await;
        let server = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(server_name) else {
                return Err(format!("MCP server '{server_name}' not found"));
            };
            server.clone()
        };
        if let Some(blocked) = pending_auth_short_circuit(
            &server,
            &canonical_tool,
            tool_name,
            now,
            MCP_AUTH_REPROBE_COOLDOWN_MS,
        ) {
            return Ok(ToolResult {
                output: blocked.output,
                metadata: json!({
                    "server": server_name,
                    "tool": tool_name,
                    "result": Value::Null,
                    "mcpAuth": blocked.mcp_auth
                }),
            });
        }
        let normalized_args = normalize_mcp_tool_args(&server, tool_name, args);

        {
            let mut servers = self.servers.write().await;
            if let Some(row) = servers.get_mut(server_name) {
                if let Some(pending) = row.pending_auth_by_tool.get_mut(&canonical_tool) {
                    pending.last_probe_ms = now;
                }
            }
        }

        let request = json!({
            "jsonrpc": "2.0",
            "id": format!("call-{}-{}", server_name, now_ms()),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": normalized_args
            }
        });
        let (response, session_id) = match post_json_rpc_with_session(
            &endpoint,
            &effective_headers(&server),
            request.clone(),
            server.mcp_session_id.as_deref(),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                if should_retry_mcp_oauth_refresh(&server, &error)
                    && self
                        .ensure_oauth_bearer_token_fresh(server_name, true)
                        .await?
                {
                    let refreshed_server = {
                        let servers = self.servers.read().await;
                        servers
                            .get(server_name)
                            .cloned()
                            .ok_or_else(|| format!("MCP server '{server_name}' not found"))?
                    };
                    post_json_rpc_with_session(
                        &endpoint,
                        &effective_headers(&refreshed_server),
                        request,
                        refreshed_server.mcp_session_id.as_deref(),
                    )
                    .await?
                } else {
                    return Err(error);
                }
            }
        };
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
                        row.pending_auth_by_tool.insert(
                            canonical_tool.clone(),
                            pending_auth_from_challenge(&challenge),
                        );
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
                if let Some(challenge) = auth_challenge.as_ref() {
                    row.pending_auth_by_tool.insert(
                        canonical_tool.clone(),
                        pending_auth_from_challenge(challenge),
                    );
                } else {
                    row.pending_auth_by_tool.remove(&canonical_tool);
                }
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
                    server.last_auth_challenge = None;
                    server.pending_auth_by_tool.clear();
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
                    server.last_auth_challenge = None;
                    server.pending_auth_by_tool.clear();
                }
                drop(servers);
                self.persist_state().await;
                false
            }
        }
    }

    async fn discover_remote_tools(
        &self,
        server_name: &str,
        endpoint: &str,
        headers: &HashMap<String, String>,
    ) -> Result<(Vec<McpRemoteTool>, Option<String>), DiscoverRemoteToolsError> {
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
            if let Some(challenge) = extract_auth_challenge(err, server_name) {
                return Err(DiscoverRemoteToolsError::AuthChallenge(challenge));
            }
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP initialize failed");
            return Err(DiscoverRemoteToolsError::Message(message.to_string()));
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
            if let Some(challenge) = extract_auth_challenge(err, server_name) {
                return Err(DiscoverRemoteToolsError::AuthChallenge(challenge));
            }
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("MCP tools/list failed");
            return Err(DiscoverRemoteToolsError::Message(message.to_string()));
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
        persist_state_blocking(self.state_file.as_path(), &snapshot);
    }

    async fn ensure_oauth_bearer_token_fresh(
        &self,
        name: &str,
        force: bool,
    ) -> Result<bool, String> {
        let server = {
            let servers = self.servers.read().await;
            servers.get(name).cloned()
        }
        .ok_or_else(|| format!("MCP server '{name}' not found"))?;
        let Some(oauth) = server.oauth.clone() else {
            return Ok(false);
        };
        let Some(credential) = tandem_core::load_provider_oauth_credential(&oauth.provider_id)
        else {
            return Ok(false);
        };

        let should_refresh = force
            || credential.expires_at_ms <= now_ms().saturating_add(60_000)
            || credential.access_token.trim().is_empty();
        if !should_refresh {
            return Ok(false);
        }

        let refreshed = refresh_mcp_oauth_credential(&oauth, &credential).await?;
        self.set_bearer_token(name, &refreshed.access_token).await?;
        tandem_core::set_provider_oauth_credential(&oauth.provider_id, refreshed)
            .map_err(|error| error.to_string())?;
        Ok(true)
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

fn normalize_allowed_tool_names(raw: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tool in raw {
        let value = tool.trim().to_string();
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        normalized.push(value);
    }
    normalized
}

fn persist_state_blocking(path: &Path, snapshot: &HashMap<String, McpServer>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(payload) = serde_json::to_string_pretty(snapshot) {
        let _ = std::fs::write(path, payload);
    }
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

fn load_state(path: &Path) -> (HashMap<String, McpServer>, bool) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (HashMap::new(), false);
    };
    let mut migrated = false;
    let mut parsed = serde_json::from_str::<HashMap<String, McpServer>>(&raw).unwrap_or_default();
    for (name, server) in parsed.iter_mut() {
        let tenant_context = local_tenant_context();
        let (headers, secret_headers, secret_header_values, server_migrated) =
            migrate_server_headers(name, server, &tenant_context);
        migrated = migrated || server_migrated;
        server.headers = headers;
        server.secret_headers = secret_headers;
        server.secret_header_values = secret_header_values;
    }
    (parsed, migrated)
}

fn migrate_server_headers(
    server_name: &str,
    server: &McpServer,
    current_tenant: &TenantContext,
) -> (
    HashMap<String, String>,
    HashMap<String, McpSecretRef>,
    HashMap<String, String>,
    bool,
) {
    let original_effective = effective_headers(server);
    let mut persisted_secret_headers = server.secret_headers.clone();
    let mut secret_header_values =
        resolve_secret_header_values(&persisted_secret_headers, current_tenant);
    let mut persisted_headers = server.headers.clone();
    let mut migrated = false;

    let header_keys = persisted_headers.keys().cloned().collect::<Vec<_>>();
    for header_name in header_keys {
        let Some(value) = persisted_headers.get(&header_name).cloned() else {
            continue;
        };
        if persisted_secret_headers.contains_key(&header_name) {
            continue;
        }
        if let Some(secret_ref) = parse_secret_header_reference(value.trim()) {
            persisted_headers.remove(&header_name);
            let resolved =
                resolve_secret_ref_value(&secret_ref, current_tenant).unwrap_or_default();
            persisted_secret_headers.insert(header_name.clone(), secret_ref);
            if !resolved.is_empty() {
                secret_header_values.insert(header_name.clone(), resolved);
            }
            migrated = true;
            continue;
        }
        if header_name_is_sensitive(&header_name) && !value.trim().is_empty() {
            let secret_id = mcp_header_secret_id(server_name, &header_name);
            if tandem_core::set_provider_auth(&secret_id, &value).is_ok() {
                persisted_headers.remove(&header_name);
                persisted_secret_headers.insert(
                    header_name.clone(),
                    McpSecretRef::Store {
                        secret_id: secret_id.clone(),
                        tenant_context: current_tenant.clone(),
                    },
                );
                secret_header_values.insert(header_name.clone(), value);
                migrated = true;
            }
        }
    }

    if !migrated {
        let effective = combine_headers(&persisted_headers, &secret_header_values);
        migrated = effective != original_effective;
    }

    (
        persisted_headers,
        persisted_secret_headers,
        secret_header_values,
        migrated,
    )
}

fn split_headers_for_storage(
    server_name: &str,
    headers: HashMap<String, String>,
    explicit_secret_headers: HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) -> (
    HashMap<String, String>,
    HashMap<String, McpSecretRef>,
    HashMap<String, String>,
) {
    let mut persisted_headers = HashMap::new();
    let mut persisted_secret_headers = HashMap::new();
    let mut secret_header_values = HashMap::new();

    for (header_name, raw_value) in headers {
        let value = raw_value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        if let Some(secret_ref) = parse_secret_header_reference(&value) {
            if let Some(resolved) = resolve_secret_ref_value(&secret_ref, current_tenant) {
                secret_header_values.insert(header_name.clone(), resolved);
            }
            persisted_secret_headers.insert(header_name, secret_ref);
            continue;
        }
        if header_name_is_sensitive(&header_name) {
            let secret_id = mcp_header_secret_id(server_name, &header_name);
            if tandem_core::set_provider_auth(&secret_id, &value).is_ok() {
                persisted_secret_headers.insert(
                    header_name.clone(),
                    McpSecretRef::Store {
                        secret_id: secret_id.clone(),
                        tenant_context: current_tenant.clone(),
                    },
                );
                secret_header_values.insert(header_name, value);
                continue;
            }
        }
        persisted_headers.insert(header_name, value);
    }

    for (header_name, secret_ref) in explicit_secret_headers {
        if let Some(resolved) = resolve_secret_ref_value(&secret_ref, current_tenant) {
            secret_header_values.insert(header_name.clone(), resolved);
        }
        persisted_headers.remove(&header_name);
        persisted_secret_headers.insert(header_name, secret_ref);
    }

    (
        persisted_headers,
        persisted_secret_headers,
        secret_header_values,
    )
}

fn combine_headers(
    headers: &HashMap<String, String>,
    secret_header_values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut combined = headers.clone();
    for (key, value) in secret_header_values {
        if !value.trim().is_empty() {
            combined.insert(key.clone(), value.clone());
        }
    }
    combined
}

fn effective_headers(server: &McpServer) -> HashMap<String, String> {
    combine_headers(&server.headers, &server.secret_header_values)
}

fn redacted_server_view(server: &McpServer) -> McpServer {
    let mut clone = server.clone();
    for (header_name, secret_ref) in &clone.secret_headers {
        clone.headers.insert(
            header_name.clone(),
            redacted_secret_header_value(secret_ref),
        );
    }
    clone.secret_header_values.clear();
    if let Some(oauth) = clone.oauth.as_mut() {
        oauth.client_secret_ref = None;
        oauth.client_secret_value = None;
    }
    clone
}

fn normalize_auth_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oauth" | "auto" | "bearer" | "x-api-key" | "custom" | "none" => {
            raw.trim().to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn redacted_secret_header_value(secret_ref: &McpSecretRef) -> String {
    match secret_ref {
        McpSecretRef::BearerEnv { .. } => "Bearer ".to_string(),
        McpSecretRef::Env { .. } | McpSecretRef::Store { .. } => MCP_SECRET_PLACEHOLDER.to_string(),
    }
}

fn resolve_secret_header_values(
    secret_headers: &HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (header_name, secret_ref) in secret_headers {
        if let Some(value) = resolve_secret_ref_value(secret_ref, current_tenant) {
            if !value.trim().is_empty() {
                out.insert(header_name.clone(), value);
            }
        }
    }
    out
}

fn delete_secret_header_refs(
    secret_headers: &HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) {
    for secret_ref in secret_headers.values() {
        if let McpSecretRef::Store {
            secret_id,
            tenant_context,
        } = secret_ref
        {
            if tenant_context != current_tenant {
                continue;
            }
            let _ = tandem_core::delete_provider_auth(secret_id);
        }
    }
}

fn delete_oauth_secret_ref(oauth: Option<&McpOAuthConfig>, current_tenant: &TenantContext) {
    let Some(secret_ref) = oauth.and_then(|oauth| oauth.client_secret_ref.as_ref()) else {
        return;
    };
    if let McpSecretRef::Store {
        secret_id,
        tenant_context,
    } = secret_ref
    {
        if tenant_context == current_tenant {
            let _ = tandem_core::delete_provider_auth(secret_id);
        }
    }
}
