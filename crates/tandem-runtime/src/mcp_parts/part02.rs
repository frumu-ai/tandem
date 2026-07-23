fn resolve_secret_ref_value(
    secret_ref: &McpSecretRef,
    current_tenant: &TenantContext,
) -> Option<String> {
    resolve_secret_ref_value_with_loader(secret_ref, current_tenant, |tenant_context| {
        tandem_core::load_provider_auth_for_tenant(tenant_context)
    })
}

fn resolve_secret_ref_value_with_loader(
    secret_ref: &McpSecretRef,
    current_tenant: &TenantContext,
    load_provider_auth_for_tenant: impl FnOnce(&TenantContext) -> HashMap<String, String>,
) -> Option<String> {
    match secret_ref {
        McpSecretRef::Store {
            secret_id,
            tenant_context,
        } => {
            if tenant_context != current_tenant {
                return None;
            }
            let secret_ref = SecretRef {
                org_id: tenant_context.org_id.clone(),
                workspace_id: tenant_context.workspace_id.clone(),
                provider: "mcp_header".to_string(),
                secret_id: secret_id.trim().to_string(),
                name: secret_id.trim().to_string(),
            };
            if secret_ref.validate_for_tenant(current_tenant).is_err() {
                return None;
            }
            load_provider_auth_for_tenant(current_tenant)
                .get(&secret_id.trim().to_ascii_lowercase())
                .cloned()
                .filter(|value| !value.trim().is_empty())
        }
        McpSecretRef::Env { env } => current_tenant
            .is_local_implicit()
            .then(|| std::env::var(env).ok())
            .flatten()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        McpSecretRef::BearerEnv { env } => current_tenant
            .is_local_implicit()
            .then(|| std::env::var(env).ok())
            .flatten()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| format!("Bearer {value}")),
    }
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
            let _ = tandem_core::delete_provider_auth_for_tenant(current_tenant, secret_id);
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
            let _ = tandem_core::delete_provider_auth_for_tenant(current_tenant, secret_id);
        }
    }
}

fn delete_oauth_credential(
    server_name: &str,
    oauth: Option<&McpOAuthConfig>,
    current_tenant: &TenantContext,
    oauth_security_dir: &Path,
) {
    let mut provider_ids = Vec::new();
    if let Some(provider_id) = oauth
        .map(|oauth| oauth.provider_id.trim())
        .filter(|provider_id| !provider_id.is_empty())
    {
        provider_ids.push(provider_id.to_string());
    }
    let canonical_provider_segment = server_name
        .trim()
        .replace(|ch: char| !ch.is_ascii_alphanumeric(), "_")
        .trim_matches('_')
        .to_ascii_lowercase();
    let canonical_provider_id = format!("mcp-oauth::{canonical_provider_segment}");
    if !canonical_provider_segment.is_empty()
        && !provider_ids
            .iter()
            .any(|provider_id| provider_id == &canonical_provider_id)
    {
        provider_ids.push(canonical_provider_id);
    }
    for provider_id in provider_ids {
        let _ = tandem_core::delete_provider_credential_for_tenant_in_dir(
            oauth_security_dir,
            current_tenant,
            &provider_id,
        );
        let _ = tandem_core::delete_provider_credential(&provider_id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSecretTenantMismatchAudit {
    pub server_name: String,
    pub tool_name: String,
    pub header_names: Vec<String>,
    pub tenant_context: TenantContext,
}

impl McpRegistry {
    pub async fn secret_tenant_mismatch_audit(
        &self,
        server_name: &str,
        tool_name: &str,
        current_tenant: &TenantContext,
    ) -> Option<McpSecretTenantMismatchAudit> {
        let server = self.servers.read().await.get(server_name).cloned()?;
        let header_names = mismatched_store_secret_headers(&server.secret_headers, current_tenant);
        if header_names.is_empty() {
            return None;
        }
        Some(McpSecretTenantMismatchAudit {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            header_names,
            tenant_context: current_tenant.clone(),
        })
    }
}

fn mismatched_store_secret_headers(
    secret_headers: &HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) -> Vec<String> {
    if current_tenant.is_local_implicit() {
        return Vec::new();
    }

    let mut out = secret_headers
        .iter()
        .filter_map(|(header_name, secret_ref)| match secret_ref {
            McpSecretRef::Store { tenant_context, .. } if tenant_context != current_tenant => {
                Some(header_name.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

fn store_secret_tenant_denial_error(
    server_name: &str,
    tool_name: &str,
    header_names: &[String],
) -> String {
    format!(
        "ToolDenied {{ reason: TenantScope }}: blocked MCP tool `{server_name}.{tool_name}` because store-backed secret header(s) [{}] belong to a different tenant context.",
        header_names.join(", ")
    )
}

fn local_tenant_context() -> TenantContext {
    LocalImplicitTenant.into()
}

fn parse_secret_header_reference(raw: &str) -> Option<McpSecretRef> {
    let trimmed = raw.trim();
    if let Some(env) = trimmed
        .strip_prefix("${env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::Env {
            env: env.to_string(),
        });
    }
    if let Some(env) = trimmed
        .strip_prefix("${bearer_env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::BearerEnv {
            env: env.to_string(),
        });
    }
    if let Some(env) = trimmed
        .strip_prefix("Bearer ${env:")
        .and_then(|rest| rest.strip_suffix("}"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(McpSecretRef::BearerEnv {
            env: env.to_string(),
        });
    }
    None
}

fn header_name_is_sensitive(header_name: &str) -> bool {
    let normalized = header_name.trim().to_ascii_lowercase();
    normalized == "authorization"
        || normalized == "proxy-authorization"
        || normalized == "x-api-key"
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.ends_with("api-key")
        || normalized.ends_with("api_key")
}

fn mcp_header_secret_id(server_name: &str, header_name: &str) -> String {
    format!(
        "mcp_header::{}::{}",
        sanitize_namespace_segment(server_name),
        sanitize_namespace_segment(header_name)
    )
}

fn mcp_header_secret_id_for_tenant(
    server_name: &str,
    header_name: &str,
    current_tenant: &TenantContext,
) -> String {
    if current_tenant.is_local_implicit() {
        return mcp_header_secret_id(server_name, header_name);
    }
    format!(
        "mcp_header::{}::{}::{}",
        sanitize_namespace_segment(server_name),
        mcp_principal_secret_scope(current_tenant),
        sanitize_namespace_segment(header_name)
    )
}

fn mcp_oauth_client_secret_id(server_name: &str) -> String {
    format!(
        "mcp_oauth_client_secret::{}",
        sanitize_namespace_segment(server_name)
    )
}

fn mcp_oauth_client_secret_id_for_tenant(
    server_name: &str,
    current_tenant: &TenantContext,
) -> String {
    if current_tenant.is_local_implicit() {
        return mcp_oauth_client_secret_id(server_name);
    }
    format!(
        "mcp_oauth_client_secret::{}::{}",
        sanitize_namespace_segment(server_name),
        mcp_principal_secret_scope(current_tenant)
    )
}

fn mcp_principal_secret_scope(current_tenant: &TenantContext) -> String {
    let stable_key = McpPrincipalRef::from_tenant_context(current_tenant).stable_key();
    let hash = sha256_hex(&mcp_connection_identity_key(
        "credential",
        current_tenant,
        &McpPrincipalRef::from_tenant_context(current_tenant),
    ));
    format!(
        "{}_{}",
        sanitize_namespace_segment(&stable_key),
        &hash[..16]
    )
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
    tool_cache_rows(server, &server.tool_cache)
}

fn mcp_tool_is_allowed(server: &McpServer, tool_name: &str) -> bool {
    let Some(allowed_tools) = server.allowed_tools.as_ref() else {
        return true;
    };
    if allowed_tools.is_empty() {
        return false;
    }
    let server_slug = sanitize_namespace_segment(&server.name);
    let tool_slug = sanitize_namespace_segment(tool_name);
    let namespaced_name = format!("mcp.{server_slug}.{tool_slug}");
    allowed_tools.iter().any(|entry| {
        let pattern = entry.trim();
        !pattern.is_empty()
            && (pattern == "*"
                || pattern == tool_name.trim()
                || pattern == namespaced_name
                || pattern == format!("mcp.{server_slug}.*"))
    })
}

fn tool_cache_rows(server: &McpServer, tool_cache: &[McpToolCacheEntry]) -> Vec<McpRemoteTool> {
    tool_cache
        .iter()
        .filter(|tool| mcp_tool_is_allowed(server, &tool.tool_name))
        .map(|tool| {
            let server_slug = sanitize_namespace_segment(&server.name);
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
    let authorization_url = find_string_with_priority(
        result,
        &[
            &["structuredContent", "authorization_url"],
            &["structuredContent", "authorizationUrl"],
            &["authorization_url"],
            &["authorizationUrl"],
            &["auth_url"],
        ],
        &["authorization_url", "authorizationUrl", "auth_url"],
    )?;
    let raw_message = find_string_with_priority(
        result,
        &[
            &["structuredContent", "message"],
            &["message"],
            &["structuredContent", "text"],
            &["text"],
            &["llm_instructions"],
        ],
        &["message", "text", "llm_instructions"],
    )
    .unwrap_or_else(|| "This tool requires authorization before it can run.".to_string());
    let message = sanitize_auth_message(&raw_message);
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

fn find_string_with_priority(
    value: &Value,
    paths: &[&[&str]],
    fallback_keys: &[&str],
) -> Option<String> {
    for path in paths {
        if let Some(found) = find_string_at_path(value, path) {
            return Some(found);
        }
    }
    find_string_by_any_key(value, fallback_keys)
}

fn find_string_at_path(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    let s = current.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn sanitize_auth_message(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "This tool requires authorization before it can run.".to_string();
    }
    if let Some((head, _)) = trimmed.split_once("Authorize here:") {
        let head = head.trim();
        if !head.is_empty() {
            return truncate_text(head, 280);
        }
    }
    let no_newlines = trimmed.replace(['\r', '\n'], " ");
    truncate_text(no_newlines.trim(), 280)
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let truncated = input.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn stable_id_seed(seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let encoded = format!("{:x}", hasher.finalize());
    encoded.chars().take(16).collect()
}

fn canonical_tool_key(tool_name: &str) -> String {
    tool_name.trim().to_ascii_lowercase()
}

fn pending_auth_from_challenge(challenge: &McpAuthChallenge) -> PendingMcpAuth {
    PendingMcpAuth {
        challenge_id: challenge.challenge_id.clone(),
        authorization_url: challenge.authorization_url.clone(),
        message: challenge.message.clone(),
        status: challenge.status.clone(),
        first_seen_ms: challenge.requested_at_ms,
        last_probe_ms: challenge.requested_at_ms,
    }
}

struct PendingAuthShortCircuit {
    output: String,
    mcp_auth: Value,
}

fn pending_auth_short_circuit(
    pending_auth_by_tool: &HashMap<String, PendingMcpAuth>,
    tool_key: &str,
    tool_name: &str,
    now_ms_value: u64,
    cooldown_ms: u64,
) -> Option<PendingAuthShortCircuit> {
    let pending = pending_auth_by_tool.get(tool_key)?;
    let elapsed = now_ms_value.saturating_sub(pending.last_probe_ms);
    if elapsed >= cooldown_ms {
        return None;
    }
    let retry_after_ms = cooldown_ms.saturating_sub(elapsed);
    let output = format!(
        "Authorization pending for `{}`.\n{}\n\nAuthorize here: {}\nRetry after {}s.",
        tool_name,
        pending.message,
        pending.authorization_url,
        retry_after_ms.div_ceil(1000)
    );
    Some(PendingAuthShortCircuit {
        output,
        mcp_auth: json!({
            "required": true,
            "pending": true,
            "blocked": true,
            "retryAfterMs": retry_after_ms,
            "challengeId": pending.challenge_id,
            "tool": tool_name,
            "authorizationUrl": pending.authorization_url,
            "message": pending.message,
            "status": pending.status
        }),
    })
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

fn should_retry_mcp_oauth_refresh(server: &McpServer, error: &str) -> bool {
    server.auth_kind.trim().eq_ignore_ascii_case("oauth")
        && server.oauth.is_some()
        && (error.contains("HTTP 401")
            || error.contains("invalid_token")
            || error.to_ascii_lowercase().contains("unauthorized"))
}

#[derive(Debug, Deserialize)]
struct McpRefreshTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug)]
struct McpResolvedHttpTarget {
    url: reqwest::Url,
    dns_override_host: Option<String>,
    dns_override_addrs: Vec<SocketAddr>,
}

impl McpResolvedHttpTarget {
    fn client(&self, timeout: std::time::Duration) -> Result<reqwest::Client, String> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout);
        if let Some(host) = self.dns_override_host.as_deref() {
            builder = builder.resolve_to_addrs(host, &self.dns_override_addrs);
        }
        builder
            .build()
            .map_err(|error| format!("failed to build hardened MCP HTTP client: {error}"))
    }
}

async fn resolve_mcp_http_target(
    raw: &str,
    allow_private_endpoint: bool,
) -> Result<McpResolvedHttpTarget, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "invalid MCP endpoint URL".to_string())?;
    let insecure_http = url.scheme() == "http";
    if url.scheme() != "https" && !(insecure_http && allow_private_endpoint) {
        return Err("MCP endpoint must use https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("MCP endpoint must not include URL credentials".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "MCP endpoint host is missing".to_string())?
        .trim_matches(['[', ']'])
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    if let Ok(ip) = host.parse::<IpAddr>() {
        let public = mcp_ip_is_publicly_routable(ip);
        if !public && !allow_private_endpoint {
            return Err("MCP endpoint resolves to a private or internal address".to_string());
        }
        if insecure_http && public {
            return Err("insecure MCP HTTP is limited to standalone private endpoints".to_string());
        }
        Ok(McpResolvedHttpTarget {
            url,
            dns_override_host: None,
            dns_override_addrs: Vec::new(),
        })
    } else {
        let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if !allow_private_endpoint
            && (normalized == "localhost" || normalized.ends_with(".localhost"))
        {
            return Err("MCP endpoint points to localhost/private network".to_string());
        }
        let addrs = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|error| format!("failed to resolve MCP endpoint: {error}"))?
            .collect::<Vec<_>>();
        if addrs.is_empty() {
            return Err("MCP endpoint host did not resolve".to_string());
        }
        let any_private = addrs
            .iter()
            .any(|addr| !mcp_ip_is_publicly_routable(addr.ip()));
        let any_public = addrs
            .iter()
            .any(|addr| mcp_ip_is_publicly_routable(addr.ip()));
        if any_private && !allow_private_endpoint {
            return Err("MCP endpoint resolves to a private or internal address".to_string());
        }
        if insecure_http && any_public {
            return Err("insecure MCP HTTP is limited to standalone private endpoints".to_string());
        }
        Ok(McpResolvedHttpTarget {
            url,
            dns_override_host: Some(host),
            dns_override_addrs: addrs,
        })
    }
}

fn mcp_ip_is_publicly_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => mcp_ipv4_is_publicly_routable(ip),
        IpAddr::V6(ip) => mcp_ipv6_is_publicly_routable(ip),
    }
}

fn mcp_ipv4_is_publicly_routable(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240)
}

fn mcp_ipv6_is_publicly_routable(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return mcp_ipv4_is_publicly_routable(mapped);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ((segments[0] & 0xfe00) == 0xfc00)
        || ((segments[0] & 0xffc0) == 0xfe80)
        || ((segments[0] & 0xffc0) == 0xfec0)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0]))
}

async fn refresh_mcp_oauth_credential(
    oauth: &McpOAuthConfig,
    credential: &tandem_core::OAuthProviderCredential,
    allow_private_endpoint: bool,
) -> Result<tandem_core::OAuthProviderCredential, String> {
    let refresh_token = credential.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err("missing MCP OAuth refresh token".to_string());
    }
    let target = resolve_mcp_http_target(&oauth.token_endpoint, allow_private_endpoint).await?;
    let client = target.client(std::time::Duration::from_secs(15))?;
    let mut params = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("client_id", oauth.client_id.clone()),
    ];
    if let Some(client_secret) = oauth
        .client_secret_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.push(("client_secret", client_secret.to_string()));
    }
    let mut response = client
        .post(target.url)
        .header(
            "user-agent",
            format!("tandem/{}", env!("CARGO_PKG_VERSION")),
        )
        .header(ACCEPT, "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|error| format!("mcp oauth token refresh failed: {error}"))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|len| len as usize > MCP_HTTP_RESPONSE_MAX_BYTES)
    {
        return Err(format!(
            "MCP OAuth refresh response too large: exceeds {} bytes",
            MCP_HTTP_RESPONSE_MAX_BYTES
        ));
    }
    let mut body_bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("failed to read MCP OAuth refresh response: {error}"))?
    {
        if body_bytes.len().saturating_add(chunk.len()) > MCP_HTTP_RESPONSE_MAX_BYTES {
            return Err(format!(
                "MCP OAuth refresh response too large: exceeds {} bytes",
                MCP_HTTP_RESPONSE_MAX_BYTES
            ));
        }
        body_bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body_bytes)
        .map_err(|error| format!("MCP OAuth refresh response was not valid UTF-8: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "mcp oauth token refresh failed with HTTP {}: {}",
            status.as_u16(),
            body.chars().take(240).collect::<String>()
        ));
    }
    let exchanged: McpRefreshTokenResponse = serde_json::from_str(&body)
        .map_err(|error| format!("invalid mcp oauth refresh response: {error}"))?;
    let access_token = exchanged
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "mcp oauth refresh returned no access token".to_string())?
        .to_string();
    let next_refresh_token = exchanged
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| credential.refresh_token.clone());

    Ok(tandem_core::OAuthProviderCredential {
        provider_id: credential.provider_id.clone(),
        access_token,
        refresh_token: next_refresh_token,
        expires_at_ms: now_ms()
            .saturating_add(exchanged.expires_in.unwrap_or(3600).saturating_mul(1000)),
        account_id: credential.account_id.clone(),
        email: credential.email.clone(),
        display_name: credential.display_name.clone(),
        managed_by: credential.managed_by.clone(),
        api_key: credential.api_key.clone(),
    })
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
    allow_private_endpoint: bool,
) -> Result<(Value, Option<String>), String> {
    let target = resolve_mcp_http_target(endpoint, allow_private_endpoint).await?;
    let client = target.client(std::time::Duration::from_secs(12))?;
    let mut req = client.post(target.url).headers(build_headers(headers)?);
    if let Some(id) = session_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            req = req.header("Mcp-Session-Id", trimmed);
        }
    }
    let mut response = req
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("MCP request failed: {e}"))?;
    if response
        .content_length()
        .is_some_and(|len| len as usize > MCP_HTTP_RESPONSE_MAX_BYTES)
    {
        return Err(format!(
            "MCP response too large: exceeds {} bytes",
            MCP_HTTP_RESPONSE_MAX_BYTES
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let response_session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let status = response.status();
    let mut payload_bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Failed to read MCP response: {e}"))?
    {
        if payload_bytes.len().saturating_add(chunk.len()) > MCP_HTTP_RESPONSE_MAX_BYTES {
            return Err(format!(
                "MCP response too large: exceeds {} bytes",
                MCP_HTTP_RESPONSE_MAX_BYTES
            ));
        }
        payload_bytes.extend_from_slice(&chunk);
    }
    let payload = String::from_utf8(payload_bytes)
        .map_err(|e| format!("MCP response was not valid UTF-8: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "MCP endpoint returned HTTP {}: {}",
            status.as_u16(),
            payload.chars().take(400).collect::<String>()
        ));
    }

    let value = if content_type.starts_with("text/event-stream") {
        parse_sse_first_event_json(&payload).map_err(|e| {
            format!(
                "Invalid MCP SSE JSON response: {} (snippet: {})",
                e,
                payload.chars().take(400).collect::<String>()
            )
        })?
    } else if let Ok(value) = serde_json::from_str::<Value>(&payload) {
        value
    } else if let Ok(value) = parse_sse_first_event_json(&payload) {
        // Some MCP servers return SSE payloads without setting text/event-stream.
        value
    } else {
        return Err(format!(
            "Invalid MCP JSON response: {}",
            payload.chars().take(400).collect::<String>()
        ));
    };

    Ok((value, response_session_id))
}

fn parse_sse_first_event_json(payload: &str) -> Result<Value, String> {
    let mut data_lines: Vec<&str> = Vec::new();
    for raw in payload.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
        if line.is_empty() {
            if !data_lines.is_empty() {
                break;
            }
            continue;
        }
    }
    if data_lines.is_empty() {
        return Err("no SSE data event found".to_string());
    }
    let joined = data_lines.join("\n");
    serde_json::from_str::<Value>(&joined).map_err(|e| e.to_string())
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
    normalize_mcp_tool_args_with_cache(server, &server.tool_cache, tool_name, raw_args)
}

fn normalize_mcp_tool_args_with_cache(
    _server: &McpServer,
    tool_cache: &[McpToolCacheEntry],
    tool_name: &str,
    raw_args: Value,
) -> Value {
    let Some(schema) = tool_cache
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
