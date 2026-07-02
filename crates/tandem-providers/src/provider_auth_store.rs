use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::TenantContext;

const PROVIDER_AUTH_SERVICE: &str = "ai.frumu.tandem";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthBackend {
    Keychain,
    File,
}

fn provider_auth_security_dir() -> PathBuf {
    resolve_provider_auth_home_dir().join("security")
}

fn resolve_provider_auth_home_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("TANDEM_HOME") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(state_dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = state_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("tandem");
    }
    dirs::home_dir()
        .map(|home| home.join(".tandem"))
        .unwrap_or_else(|| PathBuf::from(".tandem"))
}

fn provider_auth_index_path() -> PathBuf {
    provider_auth_security_dir().join("provider_auth_index.json")
}

fn provider_auth_fallback_path() -> PathBuf {
    provider_auth_security_dir().join("provider_auth_fallback.json")
}

fn provider_credentials_index_path() -> PathBuf {
    provider_auth_security_dir().join("provider_credentials_index.json")
}

fn provider_credentials_fallback_path() -> PathBuf {
    provider_auth_security_dir().join("provider_credentials_fallback.json")
}

fn normalize_provider_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

fn provider_auth_account(provider_id: &str) -> String {
    format!("provider_api_key::{}", normalize_provider_id(provider_id))
}

fn provider_credential_account(provider_id: &str) -> String {
    format!(
        "provider_credential::{}",
        normalize_provider_id(provider_id)
    )
}

fn resolve_codex_cli_home() -> PathBuf {
    let configured = std::env::var("CODEX_HOME")
        .ok()
        .map(|value| value.trim().to_string());
    if let Some(configured) = configured {
        if configured.is_empty() {
            return dirs::home_dir()
                .map(|home| home.join(".codex"))
                .unwrap_or_else(|| PathBuf::from(".codex"));
        }
        if configured == "~" {
            return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        }
        if let Some(rest) = configured.strip_prefix("~/") {
            return dirs::home_dir()
                .map(|home| home.join(rest))
                .unwrap_or_else(|| PathBuf::from(rest));
        }

        // SECURITY: Validate CODEX_HOME to prevent path traversal attacks
        // Reject paths containing ".." or other problematic patterns
        if configured.contains("..") || configured.starts_with("-") {
            tracing::warn!(
                target: "tandem_providers::provider_auth_store",
                "rejecting invalid CODEX_HOME: contains path traversal attempt"
            );
            return dirs::home_dir()
                .map(|home| home.join(".codex"))
                .unwrap_or_else(|| PathBuf::from(".codex"));
        }

        // For absolute paths, canonicalize and ensure it's reasonable
        // (Don't allow /etc, /root, other system directories)
        let path = std::path::PathBuf::from(&configured);
        if path.is_absolute() {
            // Allow absolute paths only if they appear to be user directories
            let path_str = path.to_string_lossy().to_lowercase();
            if path_str.starts_with("/etc")
                || path_str.starts_with("/sys")
                || path_str.starts_with("/proc")
                || path_str.starts_with("/root")
                || path_str.starts_with("/boot")
            {
                tracing::warn!(
                    target: "tandem_providers::provider_auth_store",
                    "rejecting CODEX_HOME pointing to system directory"
                );
                return dirs::home_dir()
                    .map(|home| home.join(".codex"))
                    .unwrap_or_else(|| PathBuf::from(".codex"));
            }
        }

        return std::path::PathBuf::from(configured);
    }

    dirs::home_dir()
        .map(|home| home.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn resolve_codex_cli_auth_path() -> PathBuf {
    resolve_codex_cli_home().join("auth.json")
}

fn write_codex_cli_auth_json_at(path: &Path, auth_json: &Value) -> anyhow::Result<()> {
    write_secure_json(&path.to_path_buf(), auth_json)
}

fn keyring_entry(provider_id: &str) -> Option<keyring::Entry> {
    if provider_auth_keyring_disabled() {
        return None;
    }
    keyring::Entry::new(PROVIDER_AUTH_SERVICE, &provider_auth_account(provider_id)).ok()
}

fn credential_keyring_entry(provider_id: &str) -> Option<keyring::Entry> {
    if provider_auth_keyring_disabled() {
        return None;
    }
    keyring::Entry::new(
        PROVIDER_AUTH_SERVICE,
        &provider_credential_account(provider_id),
    )
    .ok()
}

fn provider_auth_keyring_disabled() -> bool {
    std::env::var("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

const TENANT_SCOPED_PROVIDER_PREFIX: &str = "__tenant__::";

fn tenant_scope_component(raw: &str) -> String {
    raw.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn tenant_scope_prefix(tenant_context: &TenantContext) -> Option<String> {
    if tenant_context.is_local_implicit() {
        return None;
    }
    Some(format!(
        "{TENANT_SCOPED_PROVIDER_PREFIX}{}::{}::{}::",
        tenant_scope_component(&tenant_context.org_id),
        tenant_scope_component(&tenant_context.workspace_id),
        tenant_scope_component(tenant_context.deployment_id.as_deref().unwrap_or_default())
    ))
}

fn tenant_scoped_provider_id(tenant_context: &TenantContext, provider_id: &str) -> String {
    let normalized = normalize_provider_id(provider_id);
    tenant_scope_prefix(tenant_context)
        .map(|prefix| format!("{prefix}{normalized}"))
        .unwrap_or(normalized)
}

fn strip_tenant_scoped_provider_id(
    tenant_context: &TenantContext,
    scoped_provider_id: &str,
) -> Option<String> {
    let normalized = normalize_provider_id(scoped_provider_id);
    match tenant_scope_prefix(tenant_context) {
        Some(prefix) => normalized
            .strip_prefix(&prefix)
            .map(str::to_string)
            .filter(|id| !id.is_empty()),
        None => (!normalized.starts_with(TENANT_SCOPED_PROVIDER_PREFIX)).then_some(normalized),
    }
}

fn credential_with_provider_id(
    credential: ProviderCredential,
    provider_id: String,
) -> ProviderCredential {
    match credential {
        ProviderCredential::ApiKey(mut api) => {
            api.provider_id = provider_id;
            ProviderCredential::ApiKey(api)
        }
        ProviderCredential::OAuth(mut oauth) => {
            oauth.provider_id = provider_id;
            ProviderCredential::OAuth(oauth)
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyProviderCredential {
    pub provider_id: String,
    pub token: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthProviderCredential {
    pub provider_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: u64,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub managed_by: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CodexCliAuthTokens {
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
    #[serde(alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(alias = "accountId")]
    account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CodexCliAuthFile {
    #[serde(alias = "authMode")]
    auth_mode: Option<String>,
    tokens: Option<CodexCliAuthTokens>,
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
    #[serde(alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(alias = "accountId")]
    account_id: Option<String>,
    #[serde(alias = "lastRefresh")]
    last_refresh: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderCredential {
    ApiKey(ApiKeyProviderCredential),
    OAuth(OAuthProviderCredential),
}

impl fmt::Debug for ApiKeyProviderCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKeyProviderCredential")
            .field("provider_id", &self.provider_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for OAuthProviderCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthProviderCredential")
            .field("provider_id", &self.provider_id)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("account_id", &self.account_id)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("managed_by", &self.managed_by)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl fmt::Debug for ProviderCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(credential) => f.debug_tuple("ApiKey").field(credential).finish(),
            Self::OAuth(credential) => f.debug_tuple("OAuth").field(credential).finish(),
        }
    }
}

impl ProviderCredential {
    pub fn provider_id(&self) -> &str {
        match self {
            Self::ApiKey(credential) => credential.provider_id.as_str(),
            Self::OAuth(credential) => credential.provider_id.as_str(),
        }
    }

    pub fn runtime_bearer_token(&self) -> Option<&str> {
        match self {
            Self::ApiKey(credential) => Some(credential.token.as_str()),
            Self::OAuth(credential) => credential
                .api_key
                .as_deref()
                .or(Some(credential.access_token.as_str())),
        }
    }
}

fn write_secure_json(path: &PathBuf, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(value)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(raw.as_bytes())?;
        file.flush()?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, raw)?;
    }

    Ok(())
}

fn read_json(path: &PathBuf) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({})))
}

fn load_provider_index() -> HashSet<String> {
    let path = provider_auth_index_path();
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashSet::new();
    if let Some(arr) = json.get("providers").and_then(Value::as_array) {
        for entry in arr {
            if let Some(id) = entry.as_str() {
                let normalized = normalize_provider_id(id);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
        }
    }
    out
}

fn load_provider_credentials_index() -> HashSet<String> {
    let path = provider_credentials_index_path();
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashSet::new();
    if let Some(arr) = json.get("providers").and_then(Value::as_array) {
        for entry in arr {
            if let Some(id) = entry.as_str() {
                let normalized = normalize_provider_id(id);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
        }
    }
    out
}

fn save_provider_index(ids: &HashSet<String>) -> anyhow::Result<()> {
    let mut sorted = ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    sorted.sort();
    let path = provider_auth_index_path();
    write_secure_json(&path, &json!({ "providers": sorted }))
}

fn save_provider_credentials_index(ids: &HashSet<String>) -> anyhow::Result<()> {
    let mut sorted = ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    sorted.sort();
    let path = provider_credentials_index_path();
    write_secure_json(&path, &json!({ "providers": sorted }))
}

fn load_fallback_map() -> HashMap<String, String> {
    let path = provider_auth_fallback_path();
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashMap::new();
    if let Some(obj) = json.as_object() {
        for (id, value) in obj {
            let provider_id = normalize_provider_id(id);
            if provider_id.is_empty() {
                continue;
            }
            let token = value
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if !token.is_empty() {
                out.insert(provider_id, token);
            }
        }
    }
    out
}

fn save_fallback_map(map: &HashMap<String, String>) -> anyhow::Result<()> {
    let path = provider_auth_fallback_path();
    let mut root = serde_json::Map::new();
    let mut pairs = map
        .iter()
        .filter_map(|(id, token)| {
            let provider_id = normalize_provider_id(id);
            let key = token.trim();
            if provider_id.is_empty() || key.is_empty() {
                None
            } else {
                Some((provider_id, key.to_string()))
            }
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, token) in pairs {
        root.insert(id, Value::String(token));
    }
    write_secure_json(&path, &Value::Object(root))
}

fn load_provider_index_from_dir(security_dir: &Path) -> HashSet<String> {
    let path = security_dir.join("provider_auth_index.json");
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashSet::new();
    if let Some(arr) = json.get("providers").and_then(Value::as_array) {
        for entry in arr {
            if let Some(id) = entry.as_str() {
                let normalized = normalize_provider_id(id);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
        }
    }
    out
}

fn save_provider_index_to_dir(security_dir: &Path, ids: &HashSet<String>) -> anyhow::Result<()> {
    let mut sorted = ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    sorted.sort();
    write_secure_json(
        &security_dir.join("provider_auth_index.json"),
        &json!({ "providers": sorted }),
    )
}

fn load_fallback_map_from_dir(security_dir: &Path) -> HashMap<String, String> {
    let path = security_dir.join("provider_auth_fallback.json");
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashMap::new();
    if let Some(obj) = json.as_object() {
        for (id, value) in obj {
            let provider_id = normalize_provider_id(id);
            if provider_id.is_empty() {
                continue;
            }
            let token = value
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if !token.is_empty() {
                out.insert(provider_id, token);
            }
        }
    }
    out
}

fn save_fallback_map_to_dir(
    security_dir: &Path,
    map: &HashMap<String, String>,
) -> anyhow::Result<()> {
    let mut root = serde_json::Map::new();
    let mut pairs = map
        .iter()
        .filter_map(|(id, token)| {
            let provider_id = normalize_provider_id(id);
            let key = token.trim();
            if provider_id.is_empty() || key.is_empty() {
                None
            } else {
                Some((provider_id, key.to_string()))
            }
        })
        .collect::<Vec<_>>();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, token) in pairs {
        root.insert(id, Value::String(token));
    }
    write_secure_json(
        &security_dir.join("provider_auth_fallback.json"),
        &Value::Object(root),
    )
}

fn normalize_provider_credential(
    credential: ProviderCredential,
) -> anyhow::Result<ProviderCredential> {
    match credential {
        ProviderCredential::ApiKey(mut api) => {
            api.provider_id = normalize_provider_id(&api.provider_id);
            api.token = api.token.trim().to_string();
            if api.provider_id.is_empty() {
                anyhow::bail!("provider id cannot be empty");
            }
            if api.token.is_empty() {
                anyhow::bail!("provider token cannot be empty");
            }
            Ok(ProviderCredential::ApiKey(api))
        }
        ProviderCredential::OAuth(mut oauth) => {
            oauth.provider_id = normalize_provider_id(&oauth.provider_id);
            oauth.access_token = oauth.access_token.trim().to_string();
            oauth.refresh_token = oauth.refresh_token.trim().to_string();
            oauth.managed_by = oauth.managed_by.trim().to_string();
            oauth.api_key = oauth
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            oauth.account_id = oauth
                .account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            oauth.email = oauth
                .email
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            oauth.display_name = oauth
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            if oauth.provider_id.is_empty() {
                anyhow::bail!("provider id cannot be empty");
            }
            if oauth.access_token.is_empty() {
                anyhow::bail!("oauth access token cannot be empty");
            }
            if oauth.refresh_token.is_empty() {
                anyhow::bail!("oauth refresh token cannot be empty");
            }
            if oauth.managed_by.is_empty() {
                anyhow::bail!("oauth managed_by cannot be empty");
            }
            Ok(ProviderCredential::OAuth(oauth))
        }
    }
}

fn decode_codex_jwt_claims(token: &str) -> Option<Value> {
    // These claims are display/cache metadata for a locally supplied Codex CLI
    // bearer token. They are not an authorization source; upstream provider
    // APIs remain the authority that validates the access token itself.
    // Reject unsigned tokens anyway so obviously malformed local metadata does
    // not hydrate account identity fields.

    let mut parts = token.split('.');
    let header_b64 = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;
    if signature.trim().is_empty() {
        return None; // Token has an empty signature segment
    }

    // Validate token has all three required parts (header.payload.signature)
    if parts.next().is_some() {
        return None; // Token has too many parts
    }

    // Decode and check header for algorithm
    let header_decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(header_b64)
        .ok()?;
    let header: Value = serde_json::from_slice(&header_decoded).ok()?;

    // SECURITY: Reject tokens with alg:"none" to prevent algorithm substitution attacks
    if let Some(alg) = header.get("alg").and_then(Value::as_str) {
        if alg.eq_ignore_ascii_case("none") {
            return None; // Reject unsigned tokens
        }
    } else {
        return None; // Missing algorithm
    }

    // Decode payload
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;

    Some(claims)
}

fn jwt_string_claim(claims: &Value, key: &str) -> Option<String> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn jwt_nested_string_claim(claims: &Value, scope: &str, key: &str) -> Option<String> {
    claims
        .get(scope)
        .and_then(Value::as_object)
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn resolve_codex_cli_identity(
    access_token: &str,
    account_id_hint: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, u64) {
    let claims = decode_codex_jwt_claims(access_token);
    let account_id = account_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            claims
                .as_ref()
                .and_then(|value| jwt_string_claim(value, "chatgpt_account_id"))
        })
        .or_else(|| {
            claims.as_ref().and_then(|value| {
                jwt_nested_string_claim(
                    value,
                    "https://api.openai.com/auth",
                    "chatgpt_account_user_id",
                )
            })
        })
        .or_else(|| {
            claims.as_ref().and_then(|value| {
                jwt_nested_string_claim(value, "https://api.openai.com/auth", "chatgpt_user_id")
            })
        })
        .or_else(|| {
            claims
                .as_ref()
                .and_then(|value| jwt_string_claim(value, "sub"))
        });
    let email = claims.as_ref().and_then(|value| {
        jwt_nested_string_claim(value, "https://api.openai.com/profile", "email")
            .or_else(|| jwt_string_claim(value, "email"))
    });
    let display_name = email.clone().or_else(|| {
        account_id.as_deref().map(|value| {
            format!(
                "id-{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
            )
        })
    });
    // SECURITY: Token MUST have explicit expiration claim
    // Do not accept tokens with missing or invalid "exp" claim
    let expires_at_ms = match claims
        .as_ref()
        .and_then(|value| value.get("exp"))
        .and_then(Value::as_i64)
    {
        Some(exp_secs) => {
            // Validate expiration timestamp is reasonable (not in year 3000+)
            if exp_secs > i64::MAX / 2000 {
                // Overflow or unreasonably far future - reject
                return (None, None, None, 0);
            }
            if let Ok(exp_u64) = u64::try_from(exp_secs) {
                exp_u64.saturating_mul(1000)
            } else {
                // Negative or invalid timestamp - reject token
                return (None, None, None, 0);
            }
        }
        None => {
            // SECURITY: Token without expiration claim is invalid
            // Previously defaulted to 50 minutes, allowing indefinite use
            tracing::warn!(
                target: "tandem_providers::provider_auth_store",
                "rejecting JWT without exp (expiration) claim"
            );
            return (None, None, None, 0);
        }
    };

    (account_id, email, display_name, expires_at_ms)
}

fn read_codex_cli_auth_file(path: &Path) -> Option<CodexCliAuthFile> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<CodexCliAuthFile>(&raw).ok()
}

fn load_codex_cli_oauth_credential_at(path: &Path) -> Option<OAuthProviderCredential> {
    oauth_credential_from_codex_auth_file(read_codex_cli_auth_file(path)?)
}

fn oauth_credential_from_codex_auth_file(
    auth: CodexCliAuthFile,
) -> Option<OAuthProviderCredential> {
    let auth_mode = auth.auth_mode.as_deref().map(str::trim).unwrap_or("");
    if !auth_mode.is_empty() && auth_mode != "chatgpt" && auth_mode != "oauth" {
        return None;
    }
    let tokens = auth.tokens.unwrap_or_default();
    let access_token = tokens
        .access_token
        .as_deref()
        .or(auth.access_token.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    let refresh_token = tokens
        .refresh_token
        .as_deref()
        .or(auth.refresh_token.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;

    let (account_id, email, display_name, expires_at_ms) = resolve_codex_cli_identity(
        &access_token,
        tokens.account_id.as_deref().or(auth.account_id.as_deref()),
    );

    Some(OAuthProviderCredential {
        provider_id: "openai-codex".to_string(),
        access_token,
        refresh_token,
        expires_at_ms,
        account_id,
        email,
        display_name,
        managed_by: "codex-cli".to_string(),
        api_key: None,
    })
}

pub fn load_openai_codex_cli_oauth_credential() -> Option<OAuthProviderCredential> {
    load_codex_cli_oauth_credential_at(&resolve_codex_cli_auth_path())
}

pub fn oauth_credential_from_codex_auth_json(auth_json: &Value) -> Option<OAuthProviderCredential> {
    let auth = serde_json::from_value::<CodexCliAuthFile>(auth_json.clone()).ok()?;
    oauth_credential_from_codex_auth_file(auth)
}

pub fn write_openai_codex_cli_auth_json(auth_json: &Value) -> anyhow::Result<PathBuf> {
    let path = resolve_codex_cli_auth_path();
    write_codex_cli_auth_json_at(&path, auth_json)?;
    Ok(path)
}

fn load_credential_fallback_map() -> HashMap<String, ProviderCredential> {
    let path = provider_credentials_fallback_path();
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashMap::new();
    let Some(obj) = json.as_object() else {
        return out;
    };

    for (id, value) in obj {
        let provider_id = normalize_provider_id(id);
        if provider_id.is_empty() {
            continue;
        }
        let Ok(mut credential) = serde_json::from_value::<ProviderCredential>(value.clone()) else {
            continue;
        };
        match &mut credential {
            ProviderCredential::ApiKey(api) => api.provider_id = provider_id.clone(),
            ProviderCredential::OAuth(oauth) => oauth.provider_id = provider_id.clone(),
        }
        if let Ok(normalized) = normalize_provider_credential(credential) {
            out.insert(provider_id, normalized);
        }
    }

    out
}

fn save_credential_fallback_map(map: &HashMap<String, ProviderCredential>) -> anyhow::Result<()> {
    let path = provider_credentials_fallback_path();
    let mut root = serde_json::Map::new();
    let mut entries = map
        .iter()
        .filter_map(|(id, credential)| {
            let provider_id = normalize_provider_id(id);
            if provider_id.is_empty() {
                return None;
            }
            let normalized = normalize_provider_credential(credential.clone()).ok()?;
            Some((provider_id, serde_json::to_value(normalized).ok()?))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, value) in entries {
        root.insert(id, value);
    }
    write_secure_json(&path, &Value::Object(root))
}

fn load_provider_credentials_index_from_dir(security_dir: &Path) -> HashSet<String> {
    let path = security_dir.join("provider_credentials_index.json");
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashSet::new();
    if let Some(arr) = json.get("providers").and_then(Value::as_array) {
        for entry in arr {
            if let Some(id) = entry.as_str() {
                let normalized = normalize_provider_id(id);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
        }
    }
    out
}

fn save_provider_credentials_index_to_dir(
    security_dir: &Path,
    ids: &HashSet<String>,
) -> anyhow::Result<()> {
    let mut sorted = ids
        .iter()
        .filter(|id| !id.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    sorted.sort();
    write_secure_json(
        &security_dir.join("provider_credentials_index.json"),
        &json!({ "providers": sorted }),
    )
}

fn load_credential_fallback_map_from_dir(
    security_dir: &Path,
) -> HashMap<String, ProviderCredential> {
    let path = security_dir.join("provider_credentials_fallback.json");
    let json = read_json(&path).unwrap_or_else(|_| json!({}));
    let mut out = HashMap::new();
    let Some(obj) = json.as_object() else {
        return out;
    };

    for (id, value) in obj {
        let provider_id = normalize_provider_id(id);
        if provider_id.is_empty() {
            continue;
        }
        let Ok(mut credential) = serde_json::from_value::<ProviderCredential>(value.clone()) else {
            continue;
        };
        match &mut credential {
            ProviderCredential::ApiKey(api) => api.provider_id = provider_id.clone(),
            ProviderCredential::OAuth(oauth) => oauth.provider_id = provider_id.clone(),
        }
        if let Ok(normalized) = normalize_provider_credential(credential) {
            out.insert(provider_id, normalized);
        }
    }

    out
}

fn save_credential_fallback_map_to_dir(
    security_dir: &Path,
    map: &HashMap<String, ProviderCredential>,
) -> anyhow::Result<()> {
    let mut root = serde_json::Map::new();
    let mut entries = map
        .iter()
        .filter_map(|(id, credential)| {
            let provider_id = normalize_provider_id(id);
            if provider_id.is_empty() {
                return None;
            }
            let normalized = normalize_provider_credential(credential.clone()).ok()?;
            Some((provider_id, serde_json::to_value(normalized).ok()?))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, value) in entries {
        root.insert(id, value);
    }
    write_secure_json(
        &security_dir.join("provider_credentials_fallback.json"),
        &Value::Object(root),
    )
}

pub fn load_provider_auth() -> HashMap<String, String> {
    let fallback = load_fallback_map();
    let mut known = load_provider_index();
    known.extend(fallback.keys().cloned());
    let mut out = HashMap::new();

    for provider_id in known {
        if let Some(entry) = keyring_entry(&provider_id) {
            if let Ok(secret) = entry.get_password() {
                let trimmed = secret.trim();
                if !trimmed.is_empty() {
                    out.insert(provider_id.clone(), trimmed.to_string());
                    continue;
                }
            }
        }
        if let Some(secret) = fallback.get(&provider_id) {
            let trimmed = secret.trim();
            if !trimmed.is_empty() {
                out.insert(provider_id.clone(), trimmed.to_string());
            }
        }
    }

    out
}

pub fn set_provider_auth(provider_id: &str, token: &str) -> anyhow::Result<ProviderAuthBackend> {
    let id = normalize_provider_id(provider_id);
    let secret = token.trim().to_string();
    if id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    if secret.is_empty() {
        anyhow::bail!("provider token cannot be empty");
    }

    let mut known = load_provider_index();
    known.insert(id.clone());

    if let Some(entry) = keyring_entry(&id) {
        if entry.set_password(&secret).is_ok() {
            let mut fallback = load_fallback_map();
            fallback.remove(&id);
            let _ = save_fallback_map(&fallback);
            save_provider_index(&known)?;
            return Ok(ProviderAuthBackend::Keychain);
        }
    }

    let mut fallback = load_fallback_map();
    fallback.insert(id.clone(), secret);
    save_fallback_map(&fallback)?;
    save_provider_index(&known)?;
    Ok(ProviderAuthBackend::File)
}

pub fn delete_provider_auth(provider_id: &str) -> anyhow::Result<bool> {
    let id = normalize_provider_id(provider_id);
    if id.is_empty() {
        return Ok(false);
    }

    let mut removed = false;

    if let Some(entry) = keyring_entry(&id) {
        // Ignore unsupported backend errors; we still clear file fallback/index below.
        if entry.delete_password().is_ok() {
            removed = true;
        }
    }

    let mut fallback = load_fallback_map();
    if fallback.remove(&id).is_some() {
        removed = true;
    }
    save_fallback_map(&fallback)?;

    let mut known = load_provider_index();
    if known.remove(&id) {
        removed = true;
    }
    save_provider_index(&known)?;

    Ok(removed)
}

pub fn load_provider_auth_for_tenant(tenant_context: &TenantContext) -> HashMap<String, String> {
    load_provider_auth()
        .into_iter()
        .filter_map(|(provider_id, token)| {
            strip_tenant_scoped_provider_id(tenant_context, &provider_id)
                .map(|stripped| (stripped, token))
        })
        .collect()
}

pub fn load_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
) -> HashMap<String, String> {
    let fallback = load_fallback_map_from_dir(security_dir);
    let mut known = load_provider_index_from_dir(security_dir);
    known.extend(fallback.keys().cloned());
    known
        .into_iter()
        .filter_map(|provider_id| {
            let token = fallback.get(&provider_id)?;
            strip_tenant_scoped_provider_id(tenant_context, &provider_id)
                .map(|stripped| (stripped, token.clone()))
        })
        .collect()
}

pub fn set_provider_auth_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
    token: &str,
) -> anyhow::Result<ProviderAuthBackend> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    set_provider_auth(&scoped_provider_id, token)
}

pub fn set_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    token: &str,
) -> anyhow::Result<ProviderAuthBackend> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    let id = normalize_provider_id(&scoped_provider_id);
    let secret = token.trim().to_string();
    if id.is_empty() {
        anyhow::bail!("provider id cannot be empty");
    }
    if secret.is_empty() {
        anyhow::bail!("provider token cannot be empty");
    }
    let mut fallback = load_fallback_map_from_dir(security_dir);
    fallback.insert(id.clone(), secret);
    save_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_index_from_dir(security_dir);
    known.insert(id);
    save_provider_index_to_dir(security_dir, &known)?;
    Ok(ProviderAuthBackend::File)
}

pub fn delete_provider_auth_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    delete_provider_auth(&scoped_provider_id)
}

pub fn delete_provider_auth_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id =
        normalize_provider_id(&tenant_scoped_provider_id(tenant_context, provider_id));
    if scoped_provider_id.is_empty() {
        return Ok(false);
    }
    let mut removed = false;
    let mut fallback = load_fallback_map_from_dir(security_dir);
    if fallback.remove(&scoped_provider_id).is_some() {
        removed = true;
    }
    save_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_index_from_dir(security_dir);
    if known.remove(&scoped_provider_id) {
        removed = true;
    }
    save_provider_index_to_dir(security_dir, &known)?;
    Ok(removed)
}

pub fn load_provider_credentials() -> HashMap<String, ProviderCredential> {
    let fallback = load_credential_fallback_map();
    let mut known = load_provider_credentials_index();
    known.extend(fallback.keys().cloned());
    let mut out = HashMap::new();

    for provider_id in known {
        if let Some(entry) = credential_keyring_entry(&provider_id) {
            if let Ok(secret) = entry.get_password() {
                if let Ok(credential) = serde_json::from_str::<ProviderCredential>(&secret) {
                    if let Ok(normalized) = normalize_provider_credential(credential) {
                        out.insert(provider_id.clone(), normalized);
                        continue;
                    }
                }
            }
        }

        if let Some(credential) = fallback.get(&provider_id) {
            out.insert(provider_id.clone(), credential.clone());
        }
    }

    out
}

pub fn load_provider_credentials_for_tenant(
    tenant_context: &TenantContext,
) -> HashMap<String, ProviderCredential> {
    load_provider_credentials()
        .into_iter()
        .filter_map(|(provider_id, credential)| {
            strip_tenant_scoped_provider_id(tenant_context, &provider_id).map(|stripped| {
                (
                    stripped.clone(),
                    credential_with_provider_id(credential, stripped),
                )
            })
        })
        .collect()
}

pub fn load_provider_credentials_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
) -> HashMap<String, ProviderCredential> {
    let fallback = load_credential_fallback_map_from_dir(security_dir);
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.extend(fallback.keys().cloned());
    known
        .into_iter()
        .filter_map(|provider_id| {
            let credential = fallback.get(&provider_id)?;
            strip_tenant_scoped_provider_id(tenant_context, &provider_id).map(|stripped| {
                (
                    stripped.clone(),
                    credential_with_provider_id(credential.clone(), stripped),
                )
            })
        })
        .collect()
}

pub fn load_provider_oauth_credential(provider_id: &str) -> Option<OAuthProviderCredential> {
    match load_provider_credentials().remove(&normalize_provider_id(provider_id)) {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_in_dir(
    security_dir: &Path,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_credential_fallback_map_from_dir(security_dir)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_provider_credentials_for_tenant(tenant_context)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn load_provider_oauth_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> Option<OAuthProviderCredential> {
    match load_provider_credentials_for_tenant_in_dir(security_dir, tenant_context)
        .remove(&normalize_provider_id(provider_id))
    {
        Some(ProviderCredential::OAuth(credential)) => Some(credential),
        Some(ProviderCredential::ApiKey(_)) | None => None,
    }
}

pub fn set_provider_credential(
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let normalized = normalize_provider_credential(credential)?;
    let provider_id = normalized.provider_id().to_string();
    let serialized = serde_json::to_string(&normalized)?;

    let mut known = load_provider_credentials_index();
    known.insert(provider_id.clone());

    if let Some(entry) = credential_keyring_entry(&provider_id) {
        if entry.set_password(&serialized).is_ok() {
            let mut fallback = load_credential_fallback_map();
            fallback.remove(&provider_id);
            let _ = save_credential_fallback_map(&fallback);
            save_provider_credentials_index(&known)?;
            return Ok(ProviderAuthBackend::Keychain);
        }
    }

    let mut fallback = load_credential_fallback_map();
    fallback.insert(provider_id.clone(), normalized);
    save_credential_fallback_map(&fallback)?;
    save_provider_credentials_index(&known)?;
    Ok(ProviderAuthBackend::File)
}

pub fn set_provider_credential_for_tenant(
    tenant_context: &TenantContext,
    credential: ProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let normalized = normalize_provider_credential(credential)?;
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, normalized.provider_id());
    set_provider_credential(credential_with_provider_id(normalized, scoped_provider_id))
}

pub fn set_provider_oauth_credential(
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    set_provider_credential(ProviderCredential::OAuth(credential))
}

pub fn set_provider_oauth_credential_in_dir(
    security_dir: &Path,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    let normalized = normalize_provider_credential(ProviderCredential::OAuth(credential))?;
    let provider_id = normalized.provider_id().to_string();
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    fallback.insert(provider_id.clone(), normalized);
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.insert(provider_id);
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    Ok(ProviderAuthBackend::File)
}

pub fn set_provider_oauth_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    set_provider_credential_for_tenant(tenant_context, ProviderCredential::OAuth(credential))
}

pub fn set_provider_oauth_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    credential: OAuthProviderCredential,
) -> anyhow::Result<ProviderAuthBackend> {
    let mut credential = credential;
    credential.provider_id = normalize_provider_id(provider_id);
    let normalized = normalize_provider_credential(ProviderCredential::OAuth(credential))?;
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, normalized.provider_id());
    let normalized = credential_with_provider_id(normalized, scoped_provider_id.clone());
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    fallback.insert(normalize_provider_id(&scoped_provider_id), normalized);
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    known.insert(normalize_provider_id(&scoped_provider_id));
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    Ok(ProviderAuthBackend::File)
}

pub fn delete_provider_credential(provider_id: &str) -> anyhow::Result<bool> {
    let id = normalize_provider_id(provider_id);
    if id.is_empty() {
        return Ok(false);
    }

    let mut removed = false;

    if let Some(entry) = credential_keyring_entry(&id) {
        if entry.delete_password().is_ok() {
            removed = true;
        }
    }

    let mut fallback = load_credential_fallback_map();
    if fallback.remove(&id).is_some() {
        removed = true;
    }
    save_credential_fallback_map(&fallback)?;

    let mut known = load_provider_credentials_index();
    if known.remove(&id) {
        removed = true;
    }
    save_provider_credentials_index(&known)?;

    Ok(removed)
}

pub fn delete_provider_credential_for_tenant(
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id = tenant_scoped_provider_id(tenant_context, provider_id);
    delete_provider_credential(&scoped_provider_id)
}

pub fn delete_provider_credential_for_tenant_in_dir(
    security_dir: &Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> anyhow::Result<bool> {
    let scoped_provider_id =
        normalize_provider_id(&tenant_scoped_provider_id(tenant_context, provider_id));
    if scoped_provider_id.is_empty() {
        return Ok(false);
    }
    let mut removed = false;
    let mut fallback = load_credential_fallback_map_from_dir(security_dir);
    if fallback.remove(&scoped_provider_id).is_some() {
        removed = true;
    }
    save_credential_fallback_map_to_dir(security_dir, &fallback)?;
    let mut known = load_provider_credentials_index_from_dir(security_dir);
    if known.remove(&scoped_provider_id) {
        removed = true;
    }
    save_provider_credentials_index_to_dir(security_dir, &known)?;
    Ok(removed)
}

#[cfg(test)]
#[path = "provider_auth_store_tests.rs"]
mod provider_auth_store_tests;
