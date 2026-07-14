use super::mcp_inventory::{
    compact_mcp_inventory_for_tool_output, filter_mcp_inventory_snapshot_to_servers,
    filter_mcp_snapshot_by_discovery_authorization, filter_mcp_snapshot_by_tool_scope,
    mcp_inventory_snapshot, session_mcp_tool_filter, McpToolScopeFilter,
};
use super::*;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tandem_core::tool_name_security_descriptor;
use tandem_runtime::{McpAuthChallenge, McpPrincipalRef};
use tandem_types::{RequestPrincipal, StrictTenantContext, TenantContext, VerifiedTenantContext};
use uuid::Uuid;

pub(crate) use super::mcp_run_as::{
    call_mcp_tool_for_tenant_with_audit, call_mcp_tool_for_tenant_with_verified_context,
};

mod mcp_base_url;
pub(super) use mcp_base_url::{mcp_public_base_url_from_config, mcp_public_base_url_from_env};

const BUILTIN_GITHUB_MCP_SERVER_NAME: &str = "github";
const BUILTIN_GITHUB_MCP_TRANSPORT_URL: &str = "https://api.githubcopilot.com/mcp/";
const BUILTIN_TANDEM_DOCS_MCP_SERVER_NAME: &str = "tandem-mcp";
const BUILTIN_TANDEM_DOCS_MCP_TRANSPORT_URL: &str = "https://tandem.ac/mcp";
const MCP_OAUTH_SESSION_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct McpOAuthSessionRecord {
    pub session_id: String,
    pub server_name: String,
    pub tenant_context: TenantContext,
    pub principal: McpPrincipalRef,
    pub connection_id: String,
    pub provider_id: String,
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
        // The registry held by RuntimeState was constructed by the host before
        // serve() could set the crate-level strict default, so flip the live
        // instance here once the runtime is ready.
        let memory_context_policy =
            crate::memory::policy_status::current_memory_context_policy_status();
        if memory_context_policy.strict_required {
            state.mcp.set_strict_tenant_enforcement(true);
            state.tools.set_strict_tenant_enforcement(true);
            tracing::info!(
                auth_mode = %memory_context_policy.runtime_auth_mode.as_str(),
                effective_memory_auth_mode = %memory_context_policy.effective_memory_auth_mode.as_str(),
                "strict tenant enforcement enabled for live MCP and tool registries"
            );
        }
        bootstrap_mcp_servers(&state).await;
    } else {
        tracing::warn!("mcp bootstrap: skipped because runtime startup failed or timed out");
    }
}

pub(super) async fn bootstrap_mcp_servers(state: &AppState) {
    let _ = ensure_builtin_github_mcp_server(state).await;
    let _ = ensure_builtin_tandem_docs_mcp_server(state).await;
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
        // Retry the initial connect with bounded backoff. Co-located MCP
        // services on hosted Tandem (notably tandem-kb-mcp) can pass their
        // /health check a beat before their JSON-RPC /mcp endpoint accepts
        // requests, so a single startup connect attempt races with the
        // sidecar's readiness and shows up in the control panel as
        // "Disconnected — error sending request". Refreshing the UI used
        // to be the workaround. The retry below removes the manual step.
        let connected = connect_with_backoff(state, &name).await;
        if !connected {
            tracing::warn!(
                "mcp bootstrap: gave up connecting server '{}' after retries",
                name
            );
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

/// Connect to an MCP server during startup with bounded exponential
/// backoff. Retries a handful of times across roughly ~30 seconds before
/// giving up, so an upstream that's a beat behind on readiness gets a
/// chance to come online without requiring the operator to hit Refresh
/// in the control panel.
async fn connect_with_backoff(state: &AppState, name: &str) -> bool {
    // Backoff schedule (seconds): 0, 2, 4, 6, 8, 10. Total ≈ 30s.
    const DELAYS_SECS: &[u64] = &[0, 2, 4, 6, 8, 10];
    for (attempt, delay) in DELAYS_SECS.iter().enumerate() {
        if *delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
        }
        if state.mcp.connect(name).await {
            if attempt > 0 {
                tracing::info!(
                    "mcp bootstrap: connected '{}' after {} retr{}",
                    name,
                    attempt,
                    if attempt == 1 { "y" } else { "ies" }
                );
            }
            return true;
        }
        tracing::debug!(
            "mcp bootstrap: connect to '{}' attempt {} failed; will retry",
            name,
            attempt + 1
        );
    }
    false
}

fn builtin_tandem_docs_mcp_transport_url() -> String {
    std::env::var("TANDEM_DOCS_MCP_TRANSPORT_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| BUILTIN_TANDEM_DOCS_MCP_TRANSPORT_URL.to_string())
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
    let connected = ensure_remote_mcp_server(state, "kb", &transport, HashMap::new()).await;
    let _ = state
        .mcp
        .set_grounding_metadata("kb", Some("knowledgebase".to_string()), Some(true))
        .await;
    connected
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

pub(super) async fn ensure_builtin_tandem_docs_mcp_server(state: &AppState) -> bool {
    let transport = builtin_tandem_docs_mcp_transport_url();
    ensure_remote_mcp_server(
        state,
        BUILTIN_TANDEM_DOCS_MCP_SERVER_NAME,
        &transport,
        HashMap::new(),
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
    pub purpose: Option<String>,
    pub grounding_required: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct McpPatchInput {
    pub enabled: Option<bool>,
    pub allowed_tools: Option<Vec<String>>,
    pub clear_allowed_tools: Option<bool>,
    pub purpose: Option<String>,
    pub grounding_required: Option<bool>,
}

#[derive(Clone)]
pub(super) struct McpBridgeTool {
    pub schema: ToolSchema,
    pub state: AppState,
    pub server_name: String,
    pub tool_name: String,
}

#[async_trait]
impl Tool for McpBridgeTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        call_mcp_tool_for_tenant_with_audit(
            &self.state,
            &self.server_name,
            &self.tool_name,
            args,
            &tenant_context,
        )
        .await
        .map_err(anyhow::Error::msg)
    }
}

fn strict_context_from_tool_args(args: &Value) -> Option<StrictTenantContext> {
    args.get("__verified_tenant_context")
        .cloned()
        .and_then(|value| serde_json::from_value::<VerifiedTenantContext>(value).ok())
        .and_then(|verified| verified.strict_projection)
}

pub(super) async fn add_mcp(
    State(state): State<AppState>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    Json(input): Json<McpAddInput>,
) -> Response {
    let name = input.name.unwrap_or_else(|| "default".to_string());
    let transport = input.transport.unwrap_or_else(|| "stdio".to_string());
    if transport.trim_start().starts_with("stdio:") {
        return Json(json!({
            "ok": false,
            "error": "stdio MCP transports cannot be registered through the HTTP API",
            "code": ErrorCode::McpStdioTransportDenied,
            "retryable": false
        }))
        .into_response();
    }
    let auth_kind = normalize_mcp_auth_kind(input.auth_kind.as_deref().unwrap_or_default());
    let audit_transport = transport.clone();
    state
        .mcp
        .add_or_update_with_secret_refs(
            name.clone(),
            transport,
            input.headers.unwrap_or_default(),
            input.secret_headers.unwrap_or_default(),
            &tenant_context,
            input.enabled.unwrap_or(true),
        )
        .await;
    if let Some(allowed_tools) = input.allowed_tools.clone() {
        let _ = state
            .mcp
            .set_allowed_tools(&name, Some(allowed_tools))
            .await;
    }
    if input.purpose.is_some() || input.grounding_required.is_some() {
        let _ = state
            .mcp
            .set_grounding_metadata(&name, input.purpose.clone(), input.grounding_required)
            .await;
    }
    if !auth_kind.is_empty() {
        let _ = state.mcp.set_auth_kind(&name, auth_kind.clone()).await;
    }
    if let Err(error) = crate::audit::append_protected_audit_event(
        &state,
        "mcp.server.updated",
        &tenant_context,
        tenant_context.actor_id.clone(),
        json!({
                "name": name,
                "transport": audit_transport,
            "enabled": input.enabled.unwrap_or(true),
            "auth_kind": auth_kind,
            "allowed_tools": input.allowed_tools,
            "purpose": input.purpose,
            "grounding_required": input.grounding_required,
        }),
    )
    .await
    {
        return super::protected_audit_error_response(error).into_response();
    }
    state.event_bus.publish(EngineEvent::new(
        "mcp.server.updated",
        json!({
            "name": name,
        }),
    ));
    Json(json!({"ok": true})).into_response()
}

fn normalize_mcp_auth_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oauth" | "auto" | "bearer" | "x-api-key" | "custom" | "none" => {
            raw.trim().to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn mcp_auth_challenge_from_session(session: &McpOAuthSessionRecord) -> McpAuthChallenge {
    McpAuthChallenge {
        challenge_id: format!("mcp-oauth-session-{}", session.session_id),
        tool_name: session.server_name.clone(),
        authorization_url: session.authorization_url.clone(),
        message: "Authorization required. Open the link to connect this MCP server.".to_string(),
        requested_at_ms: session.created_at_ms,
        status: session.status.clone(),
    }
}
async fn current_mcp_auth_challenge_for_tenant(
    state: &AppState,
    name: &str,
    tenant_context: &TenantContext,
) -> Option<McpAuthChallenge> {
    if let Some(session) = find_pending_mcp_oauth_session(state, name, tenant_context).await {
        return Some(mcp_auth_challenge_from_session(&session));
    }
    state
        .mcp
        .auth_challenge_for_tenant(name, tenant_context)
        .await
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
    mcp_public_base_url_from_config(cfg)
        .or_else(mcp_public_base_url_from_env)
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
fn mcp_oauth_redirect_uri_from_authorization_url(authorization_url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(authorization_url).ok()?;
    parsed
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.into_owned())
}
fn generate_mcp_oauth_state() -> String {
    let entropy = format!("{}:{}", Uuid::new_v4(), Uuid::new_v4());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy)
}
fn generate_mcp_pkce_pair() -> (String, String) {
    let entropy = format!("{}:{}", Uuid::new_v4(), Uuid::new_v4());
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy);
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}
fn mcp_oauth_provider_id(server_name: &str) -> String {
    format!("mcp-oauth::{}", mcp_namespace_segment(server_name))
}
fn mcp_oauth_provider_id_for_tenant(
    server_name: &str,
    tenant_context: &TenantContext,
    connection_id: &str,
) -> String {
    let base = mcp_oauth_provider_id(server_name);
    if tenant_context.is_local_implicit() {
        base
    } else {
        format!("{base}::{connection_id}")
    }
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
    tenant_context: &TenantContext,
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
    let effective_cfg = state.config.get_effective_value().await;
    let public_base_url = public_base_url_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| mcp_public_base_url(state, &effective_cfg));
    let redirect_uri = mcp_oauth_redirect_uri_for_base(&public_base_url, name);
    let existing_challenge = state
        .mcp
        .auth_challenge_for_tenant(name, tenant_context)
        .await;
    if let Some(existing) = existing_challenge {
        let existing_redirect =
            mcp_oauth_redirect_uri_from_authorization_url(&existing.authorization_url);
        if public_base_url_hint
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
            || existing_redirect.as_deref() == Some(redirect_uri.as_str())
        {
            return Ok(existing);
        }
        let _ = state
            .mcp
            .clear_auth_challenge_for_tenant(name, tenant_context)
            .await;
        state
            .oauth
            .retain_mcp_sessions(|session| {
                session.server_name != name || session.tenant_context != *tenant_context
            })
            .await;
    }

    let bootstrap =
        discover_mcp_oauth_bootstrap(&endpoint, &effective_mcp_headers(&server)).await?;
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
    let connection_id = state.mcp.connection_id_for_tenant(name, tenant_context);
    let provider_id = mcp_oauth_provider_id_for_tenant(name, tenant_context, &connection_id);
    let created_at_ms = crate::now_ms();
    let session = McpOAuthSessionRecord {
        session_id: session_id.clone(),
        server_name: name.to_string(),
        tenant_context: tenant_context.clone(),
        principal: McpPrincipalRef::from_tenant_context(tenant_context),
        connection_id,
        provider_id,
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
    };
    state
        .oauth
        .insert_mcp_session(session_id.clone(), session.clone())
        .await;
    publish_mcp_oauth_event(state, "mcp.connection.oauth_started", &session, None).await?;
    let _ = state
        .mcp
        .record_auth_challenge_for_tenant(name, tenant_context, challenge.clone(), None)
        .await;
    Ok(challenge)
}

async fn find_pending_mcp_oauth_session(
    state: &AppState,
    server_name: &str,
    tenant_context: &TenantContext,
) -> Option<McpOAuthSessionRecord> {
    state
        .oauth
        .find_mcp_session(|session| {
            session.server_name == server_name
                && session.tenant_context == *tenant_context
                && session.status.trim().eq_ignore_ascii_case("pending")
                && session.expires_at_ms > crate::now_ms()
        })
        .await
}

async fn remove_mcp_oauth_sessions_for_server(state: &AppState, server_name: &str) -> usize {
    state
        .oauth
        .retain_mcp_sessions(|session| session.server_name != server_name)
        .await
}

async fn publish_mcp_oauth_event(
    state: &AppState,
    event: &str,
    session: &McpOAuthSessionRecord,
    reason: Option<&str>,
) -> Result<(), String> {
    let payload = json!({
        "server_name": session.server_name,
        "serverName": session.server_name,
        "connection_id": session.connection_id,
        "connectionId": session.connection_id,
        "provider_id": session.provider_id,
        "providerId": session.provider_id,
        "principal": session.principal,
        "tenant_context": session.tenant_context,
        "tenantContext": session.tenant_context,
        "reason": reason,
    });
    state
        .event_bus
        .publish(EngineEvent::new(event, payload.clone()));
    let actor_id = session.tenant_context.actor_id.clone();
    crate::audit::append_protected_audit_event(
        state,
        event,
        &session.tenant_context,
        actor_id,
        payload,
    )
    .await
    .map(|_| ())
    .map_err(|error| format!("required MCP OAuth lifecycle receipt could not be written: {error}"))
}

fn mcp_tenant_event_payload(
    state: &AppState,
    name: &str,
    tenant_context: &TenantContext,
    extras: Value,
) -> Value {
    let mut payload = json!({
        "name": name,
        "serverName": name,
        "connectionId": state.mcp.connection_id_for_tenant(name, tenant_context),
        "principal": McpPrincipalRef::from_tenant_context(tenant_context),
        "tenantContext": tenant_context,
    });
    if let (Some(payload), Some(extras)) = (payload.as_object_mut(), extras.as_object()) {
        payload.extend(extras.clone());
    }
    payload
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
    request_tenant: Option<TenantContext>,
    input: McpOAuthCallbackInput,
) -> Result<(), String> {
    let state_token = input
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing oauth state".to_string())?;
    let session_id = state
        .oauth
        .find_mcp_session_id(|session| session.server_name == name && session.state == state_token)
        .await
        .ok_or_else(|| "mcp oauth session not found or expired".to_string())?;

    let session = state
        .oauth
        .get_mcp_session(&session_id)
        .await
        .ok_or_else(|| "mcp oauth session not found".to_string())?;
    if let Some(request_tenant) = request_tenant.as_ref() {
        if &session.tenant_context != request_tenant {
            publish_mcp_oauth_event(
                &state,
                "mcp.connection.oauth_denied",
                &session,
                Some("tenant_context_mismatch"),
            )
            .await?;
            return Err("mcp oauth callback tenant context did not match session".to_string());
        }
    }
    if !session.status.trim().eq_ignore_ascii_case("pending") {
        publish_mcp_oauth_event(
            &state,
            "mcp.connection.oauth_denied",
            &session,
            Some("session_already_completed"),
        )
        .await?;
        return Err("mcp oauth session already completed".to_string());
    }
    if session.expires_at_ms <= crate::now_ms() {
        publish_mcp_oauth_event(
            &state,
            "mcp.connection.oauth_denied",
            &session,
            Some("session_expired"),
        )
        .await?;
        return Err("mcp oauth session expired before callback completed".to_string());
    }

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
        state
            .oauth
            .update_mcp_session(&session_id, |session| {
                session.status = "error".to_string();
                session.error = Some(detail.clone());
            })
            .await;
        publish_mcp_oauth_event(
            &state,
            "mcp.connection.oauth_denied",
            &session,
            Some("provider_error"),
        )
        .await?;
        return Err(detail);
    }

    let code = input
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing authorization code".to_string())?;

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
        .set_bearer_token_for_tenant(&name, &access_token, &session.tenant_context)
        .await
        .map_err(|error| format!("failed to store mcp oauth token: {error}"))?;
    state
        .mcp
        .set_oauth_refresh_config_for_tenant(
            &name,
            session.provider_id.clone(),
            session.token_endpoint.clone(),
            session.client_id.clone(),
            session.client_secret.clone(),
            &session.tenant_context,
        )
        .await
        .map_err(|error| format!("failed to store mcp oauth refresh metadata: {error}"))?;
    let credential = tandem_core::OAuthProviderCredential {
        provider_id: session.provider_id.clone(),
        access_token: access_token.clone(),
        refresh_token,
        expires_at_ms,
        account_id: None,
        email: None,
        display_name: None,
        managed_by: "tandem".to_string(),
        api_key: None,
    };
    let credential_result = if session.tenant_context.is_local_implicit() {
        tandem_core::set_provider_oauth_credential(&session.provider_id, credential.clone())
    } else {
        tandem_core::set_provider_oauth_credential_for_tenant(
            &session.tenant_context,
            &session.provider_id,
            credential.clone(),
        )
    };
    if let Err(error) = credential_result {
        tracing::warn!(%error, provider_id = %session.provider_id, "failed to persist MCP OAuth credential");
    }
    if let Err(error) = state
        .mcp
        .set_oauth_credential_for_tenant(&session.provider_id, credential, &session.tenant_context)
        .await
    {
        tracing::warn!(
            %error,
            provider_id = %session.provider_id,
            "failed to persist MCP OAuth credential in registry store"
        );
    }

    match state
        .mcp
        .refresh_for_tenant(&name, &session.tenant_context)
        .await
    {
        Ok(_) => {
            let count =
                sync_mcp_tools_for_server_for_tenant(&state, &name, &session.tenant_context).await;
            let _ = state
                .mcp
                .clear_auth_challenge_for_tenant(&name, &session.tenant_context)
                .await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.connected",
                mcp_tenant_event_payload(
                    &state,
                    &name,
                    &session.tenant_context,
                    json!({
                        "connection_id": session.connection_id,
                        "status": "connected",
                        "source": "oauth_callback"
                    }),
                ),
            ));
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                mcp_tenant_event_payload(
                    &state,
                    &name,
                    &session.tenant_context,
                    json!({
                        "count": count,
                        "source": "oauth_callback"
                    }),
                ),
            ));
        }
        Err(error) => {
            state
                .oauth
                .update_mcp_session(&session_id, |session| {
                    session.status = "error".to_string();
                    session.error = Some(error.clone());
                })
                .await;
            return Err(error);
        }
    }
    publish_mcp_oauth_event(&state, "mcp.connection.oauth_completed", &session, None).await?;

    state
        .oauth
        .update_mcp_session(&session_id, |session| {
            session.status = "connected".to_string();
            session.error = None;
        })
        .await;
    Ok(())
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
        let strict_context = strict_context_from_tool_args(&args);
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
        snapshot = filter_mcp_snapshot_by_discovery_authorization(
            snapshot,
            strict_context.as_ref(),
            crate::now_ms(),
        );
        let compact_snapshot = compact_mcp_inventory_for_tool_output(&snapshot);
        let output = serde_json::to_string_pretty(&compact_snapshot)
            .unwrap_or_else(|_| compact_snapshot.to_string());
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
    sync_mcp_tools_for_server_for_tenant(state, name, &TenantContext::local_implicit()).await
}

pub(crate) async fn sync_mcp_tools_for_server_for_tenant(
    state: &AppState,
    name: &str,
    tenant_context: &TenantContext,
) -> usize {
    let scoped_count = state
        .mcp
        .server_tools_for_tenant(name, tenant_context)
        .await
        .len();
    let _ = resync_mcp_bridge_tools_for_server(state, name).await;
    scoped_count
}

async fn resync_mcp_bridge_tools_for_server(state: &AppState, name: &str) -> (usize, usize) {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(name));
    let removed = state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.bridge_tools_for_server(name).await;
    for tool in &tools {
        let schema = ToolSchema::new(
            tool.namespaced_name.clone(),
            if tool.description.trim().is_empty() {
                format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
            } else {
                tool.description.clone()
            },
            tool.input_schema.clone(),
        )
        .with_security(tool_name_security_descriptor(&tool.namespaced_name));
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

pub(super) async fn connect_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let ok = state.mcp.connect_for_tenant(&name, &tenant_context).await;
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    let auth_challenge = if ok {
        None
    } else {
        let current = current_mcp_auth_challenge_for_tenant(&state, &name, &tenant_context).await;
        if current.is_some() {
            current
        } else {
            let server = state.mcp.list().await.get(&name).cloned();
            if server.as_ref().is_some_and(mcp_uses_oauth) {
                start_mcp_oauth_session(&state, &name, &tenant_context, public_base_url.as_deref())
                    .await
                    .ok()
            } else {
                None
            }
        }
    };
    if ok {
        let count = sync_mcp_tools_for_server_for_tenant(&state, &name, &tenant_context).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.connected",
            mcp_tenant_event_payload(
                &state,
                &name,
                &tenant_context,
                json!({
                    "status": "connected",
                }),
            ),
        ));
        state.event_bus.publish(EngineEvent::new(
            "mcp.tools.updated",
            mcp_tenant_event_payload(
                &state,
                &name,
                &tenant_context,
                json!({
                    "count": count,
                }),
            ),
        ));
    } else {
        let (removed, remaining) = resync_mcp_bridge_tools_for_server(&state, &name).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.disconnected",
            mcp_tenant_event_payload(
                &state,
                &name,
                &tenant_context,
                json!({
                    "removedToolCount": removed,
                    "remainingToolCount": remaining,
                    "reason": "connect_failed"
                }),
            ),
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
) -> Response {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
    let removed_tool_count = state.tools.unregister_by_prefix(&prefix).await;
    let ok = state.mcp.remove(&name).await;
    if ok {
        if let Err(error) = crate::audit::append_protected_audit_event(
            &state,
            "mcp.server.deleted",
            &tandem_types::TenantContext::local_implicit(),
            None,
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
            }),
        )
        .await
        {
            return super::protected_audit_error_response(error).into_response();
        }
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.deleted",
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
            }),
        ));
    }
    Json(json!({ "ok": ok, "removedToolCount": removed_tool_count })).into_response()
}

pub(super) async fn patch_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<McpPatchInput>,
) -> Response {
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
    if input.purpose.is_some() || input.grounding_required.is_some() {
        changed |= state
            .mcp
            .set_grounding_metadata(&name, input.purpose.clone(), input.grounding_required)
            .await;
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
            if let Err(error) = crate::audit::append_protected_audit_event(
                &state,
                "mcp.server.updated",
                &tandem_types::TenantContext::local_implicit(),
                None,
                json!({
                    "name": name,
                    "enabled": enabled,
                }),
            )
            .await
            {
                return super::protected_audit_error_response(error).into_response();
            }
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.updated",
                json!({
                    "name": name,
                    "enabled": enabled,
                }),
            ));
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
    Json(json!({"ok": changed})).into_response()
}

pub(super) async fn refresh_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let result = state.mcp.refresh_for_tenant(&name, &tenant_context).await;
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    match result {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server_for_tenant(&state, &name, &tenant_context).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                mcp_tenant_event_payload(
                    &state,
                    &name,
                    &tenant_context,
                    json!({
                        "count": count,
                    }),
                ),
            ));
            Json(json!({
                "ok": true,
                "count": tools.len(),
            }))
        }
        Err(error) => {
            let mut auth_challenge =
                current_mcp_auth_challenge_for_tenant(&state, &name, &tenant_context).await;
            if auth_challenge.is_none() {
                let server = state.mcp.list().await.get(&name).cloned();
                if server.as_ref().is_some_and(mcp_uses_oauth) {
                    auth_challenge = start_mcp_oauth_session(
                        &state,
                        &name,
                        &tenant_context,
                        public_base_url.as_deref(),
                    )
                    .await
                    .ok();
                }
            }
            let (removed, remaining) = resync_mcp_bridge_tools_for_server(&state, &name).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.disconnected",
                mcp_tenant_event_payload(
                    &state,
                    &name,
                    &tenant_context,
                    json!({
                        "removedToolCount": removed,
                        "remainingToolCount": remaining,
                        "reason": "refresh_failed"
                    }),
                ),
            ));
            Json(json!({
                "ok": false,
                "error": error,
                "code": ErrorCode::McpRefreshFailed,
                "retryable": ErrorCode::McpRefreshFailed.retryable(),
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
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    if let Some(auth_challenge) =
        current_mcp_auth_challenge_for_tenant(&state, &name, &tenant_context).await
    {
        let existing_redirect =
            mcp_oauth_redirect_uri_from_authorization_url(&auth_challenge.authorization_url);
        let desired_redirect = public_base_url
            .as_deref()
            .map(|base_url| mcp_oauth_redirect_uri_for_base(base_url, &name));
        let redirect_matches = match desired_redirect.as_deref() {
            Some(redirect_uri) => existing_redirect.as_deref() == Some(redirect_uri),
            None => true,
        };
        if redirect_matches {
            return Json(json!({
                "ok": true,
                "pending": true,
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.authorization_url,
            }));
        }
        let _ = state
            .mcp
            .clear_auth_challenge_for_tenant(&name, &tenant_context)
            .await;
        state
            .oauth
            .retain_mcp_sessions(|pending| {
                pending.server_name != name || pending.tenant_context != tenant_context
            })
            .await;
    }
    let server = state.mcp.list().await.get(&name).cloned();
    if server.as_ref().is_some_and(mcp_uses_oauth) {
        if let Ok(auth_challenge) =
            start_mcp_oauth_session(&state, &name, &tenant_context, public_base_url.as_deref())
                .await
        {
            return Json(json!({
                "ok": true,
                "pending": true,
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.authorization_url,
            }));
        }
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
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
    headers: HeaderMap,
) -> Json<Value> {
    let tenant_context = tenant_context
        .map(|extension| extension.0)
        .unwrap_or_else(TenantContext::local_implicit);
    authenticate_mcp(
        State(state),
        Path(name),
        axum::extract::Extension(tenant_context),
        headers,
    )
    .await
}

pub(super) async fn callback_mcp_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
    Query(input): Query<McpOAuthCallbackInput>,
) -> impl IntoResponse {
    let request_tenant = tenant_context.map(|extension| extension.0);
    match finish_mcp_oauth_callback(state, name, request_tenant, input).await {
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
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    if let Some(session) = find_pending_mcp_oauth_session(&state, &name, &tenant_context).await {
        let desired_redirect_uri = public_base_url
            .as_deref()
            .map(|base_url| mcp_oauth_redirect_uri_for_base(base_url, &name));
        if desired_redirect_uri
            .as_deref()
            .is_some_and(|redirect_uri| redirect_uri != session.redirect_uri)
        {
            let _ = state
                .mcp
                .clear_auth_challenge_for_tenant(&name, &tenant_context)
                .await;
            state
                .oauth
                .retain_mcp_sessions(|pending| {
                    pending.server_name != name || pending.tenant_context != tenant_context
                })
                .await;
        } else {
            let last_auth_challenge = mcp_auth_challenge_from_session(&session);
            let authorization_url = last_auth_challenge.authorization_url.clone();
            return Json(json!({
                "ok": true,
                "authenticated": false,
                "connected": false,
                "pendingAuth": true,
                "lastAuthChallenge": last_auth_challenge,
                "authorizationUrl": authorization_url,
            }));
        }
    }

    let refresh = state.mcp.refresh_for_tenant(&name, &tenant_context).await;
    let current = state.mcp.list().await.get(&name).cloned();
    let last_auth_challenge = state
        .mcp
        .auth_challenge_for_tenant(&name, &tenant_context)
        .await;
    match refresh {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server_for_tenant(&state, &name, &tenant_context).await;
            let _ = state
                .mcp
                .clear_auth_challenge_for_tenant(&name, &tenant_context)
                .await;
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
            let connected = if let Some(server) = current.as_ref() {
                state
                    .mcp
                    .runtime_connected_for_tenant(&name, server, &tenant_context)
                    .await
            } else {
                false
            };
            if auth_challenge.is_none() {
                let server = state.mcp.list().await.get(&name).cloned();
                if server.as_ref().is_some_and(mcp_uses_oauth) {
                    auth_challenge = start_mcp_oauth_session(
                        &state,
                        &name,
                        &tenant_context,
                        public_base_url.as_deref(),
                    )
                    .await
                    .ok();
                }
            }
            Json(json!({
                "ok": false,
                "authenticated": false,
                "connected": connected,
                "pendingAuth": auth_challenge.is_some(),
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
                "error": error,
                "code": ErrorCode::McpOauthFailed,
                "retryable": false,
            }))
        }
    }
}

pub(super) async fn delete_auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Value> {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
    let removed_tool_count = state.tools.unregister_by_prefix(&prefix).await;
    let removed_oauth_session_count = remove_mcp_oauth_sessions_for_server(&state, &name).await;
    let ok = state.mcp.clear_auth_material(&name).await;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.auth.deleted",
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
                "removedOauthSessionCount": removed_oauth_session_count,
            }),
        ));
    }
    Json(json!({
        "ok": ok,
        "removedToolCount": removed_tool_count,
        "removedOauthSessionCount": removed_oauth_session_count,
    }))
}

pub(super) async fn mcp_tools(
    State(state): State<AppState>,
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
) -> Json<Value> {
    let tenant_context = tenant_context
        .map(|extension| extension.0)
        .unwrap_or_else(TenantContext::local_implicit);
    Json(json!(
        state.mcp.list_tools_for_tenant(&tenant_context).await
    ))
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
