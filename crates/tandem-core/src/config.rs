use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::fs;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub url: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonalityProfileConfig {
    pub preset: Option<String>,
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersonalityConfig {
    pub default: Option<PersonalityProfileConfig>,
    #[serde(default)]
    pub per_agent: HashMap<String, PersonalityProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BotIdentityAliasesConfig {
    pub desktop: Option<String>,
    pub tui: Option<String>,
    pub portal: Option<String>,
    pub control_panel: Option<String>,
    pub channels: Option<String>,
    pub protocol: Option<String>,
    pub cli: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BotIdentityConfig {
    pub canonical_name: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub aliases: BotIdentityAliasesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdentityConfig {
    pub bot: Option<BotIdentityConfig>,
    pub personality: Option<PersonalityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserViewportConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for BrowserViewportConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

fn default_browser_headless() -> bool {
    true
}

fn default_browser_max_sessions() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sidecar_path: Option<String>,
    #[serde(default)]
    pub executable_path: Option<String>,
    #[serde(default = "default_browser_headless")]
    pub headless_default: bool,
    #[serde(default)]
    pub allow_no_sandbox: bool,
    #[serde(default)]
    pub user_data_root: Option<String>,
    #[serde(default = "default_browser_max_sessions")]
    pub max_sessions: u32,
    #[serde(default)]
    pub default_viewport: BrowserViewportConfig,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sidecar_path: None,
            executable_path: None,
            headless_default: true,
            allow_no_sandbox: false,
            user_data_root: None,
            max_sessions: 2,
            default_viewport: BrowserViewportConfig::default(),
            allowed_hosts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    pub default_provider: Option<String>,
    #[serde(default)]
    pub identity: IdentityConfig,
    #[serde(default)]
    pub browser: BrowserConfig,
    pub bot_name: Option<String>,
    pub persona: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ConfigLayers {
    global: Value,
    project: Value,
    managed: Value,
    env: Value,
    runtime: Value,
    cli: Value,
}

#[derive(Clone)]
pub struct ConfigStore {
    project_path: PathBuf,
    global_path: PathBuf,
    managed_path: PathBuf,
    layers: Arc<RwLock<ConfigLayers>>,
}

impl ConfigStore {
    pub async fn new(path: impl AsRef<Path>, cli_overrides: Option<Value>) -> anyhow::Result<Self> {
        let project_path = path.as_ref().to_path_buf();
        if let Some(parent) = project_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let managed_path = project_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("managed_config.json");
        let global_path = resolve_global_config_path().await?;

        let mut global = read_json_file(&global_path)
            .await
            .unwrap_or_else(|_| empty_object());
        let mut project = read_json_file(&project_path)
            .await
            .unwrap_or_else(|_| empty_object());
        let mut managed = read_json_file(&managed_path)
            .await
            .unwrap_or_else(|_| empty_object());

        scrub_persisted_secrets(&mut global, Some(&global_path)).await?;
        scrub_persisted_secrets(&mut project, Some(&project_path)).await?;
        scrub_persisted_secrets(&mut managed, Some(&managed_path)).await?;

        let layers = ConfigLayers {
            global,
            project,
            managed,
            env: env_layer(),
            runtime: empty_object(),
            cli: cli_overrides.unwrap_or_else(empty_object),
        };

        let store = Self {
            project_path,
            global_path,
            managed_path,
            layers: Arc::new(RwLock::new(layers)),
        };
        store.save_project().await?;
        store.save_global().await?;
        Ok(store)
    }

    pub async fn get(&self) -> AppConfig {
        let merged = self.get_effective_value().await;
        serde_json::from_value(merged).unwrap_or_default()
    }

    pub async fn get_effective_value(&self) -> Value {
        let layers = self.layers.read().await.clone();
        let mut merged = empty_object();
        deep_merge(&mut merged, &layers.global);
        deep_merge(&mut merged, &layers.project);
        deep_merge(&mut merged, &layers.managed);
        deep_merge(&mut merged, &layers.env);
        deep_merge(&mut merged, &layers.runtime);
        deep_merge(&mut merged, &layers.cli);
        inject_stored_provider_auth(&mut merged);
        inject_stored_channel_secrets(&mut merged);
        merged
    }

    pub async fn get_project_value(&self) -> Value {
        self.layers.read().await.project.clone()
    }

    pub async fn get_global_value(&self) -> Value {
        self.layers.read().await.global.clone()
    }

    pub async fn get_layers_value(&self) -> Value {
        let layers = self.layers.read().await;
        json!({
            "global": layers.global,
            "project": layers.project,
            "managed": layers.managed,
            "env": layers.env,
            "runtime": layers.runtime,
            "cli": layers.cli
        })
    }

    pub async fn set(&self, config: AppConfig) -> anyhow::Result<()> {
        let value = serde_json::to_value(config)?;
        self.set_project_value(value).await
    }

    pub async fn patch_project(&self, patch: Value) -> anyhow::Result<Value> {
        {
            let mut layers = self.layers.write().await;
            deep_merge(&mut layers.project, &patch);
        }
        self.save_project().await?;
        Ok(self.get_effective_value().await)
    }

    pub async fn patch_global(&self, patch: Value) -> anyhow::Result<Value> {
        {
            let mut layers = self.layers.write().await;
            deep_merge(&mut layers.global, &patch);
        }
        self.save_global().await?;
        Ok(self.get_effective_value().await)
    }

    pub async fn patch_runtime(&self, patch: Value) -> anyhow::Result<Value> {
        {
            let mut layers = self.layers.write().await;
            deep_merge(&mut layers.runtime, &patch);
        }
        Ok(self.get_effective_value().await)
    }

    pub async fn replace_project_value(&self, value: Value) -> anyhow::Result<Value> {
        self.set_project_value(value).await?;
        Ok(self.get_effective_value().await)
    }

    pub async fn delete_runtime_provider_key(&self, provider_id: &str) -> anyhow::Result<Value> {
        let provider = provider_id.trim().to_string();
        {
            let mut layers = self.layers.write().await;
            let Some(root) = layers.runtime.as_object_mut() else {
                return Ok(self.get_effective_value().await);
            };
            let Some(providers) = root.get_mut("providers").and_then(|v| v.as_object_mut()) else {
                return Ok(self.get_effective_value().await);
            };
            let existing_key = providers
                .keys()
                .find(|k| k.eq_ignore_ascii_case(&provider))
                .cloned();
            let Some(existing_key) = existing_key else {
                return Ok(self.get_effective_value().await);
            };
            let Some(cfg) = providers
                .get_mut(&existing_key)
                .and_then(|v| v.as_object_mut())
            else {
                return Ok(self.get_effective_value().await);
            };
            cfg.remove("api_key");
            cfg.remove("apiKey");
            if cfg.is_empty() {
                providers.remove(&existing_key);
            }
        }
        Ok(self.get_effective_value().await)
    }

    async fn set_project_value(&self, value: Value) -> anyhow::Result<()> {
        self.layers.write().await.project = value;
        self.save_project().await
    }

    async fn save_project(&self) -> anyhow::Result<()> {
        let snapshot = self.layers.read().await.project.clone();
        write_json_file(&self.project_path, &snapshot).await
    }

    async fn save_global(&self) -> anyhow::Result<()> {
        let snapshot = self.layers.read().await.global.clone();
        write_json_file(&self.global_path, &snapshot).await
    }

    #[allow(dead_code)]
    async fn save_managed(&self) -> anyhow::Result<()> {
        let snapshot = self.layers.read().await.managed.clone();
        write_json_file(&self.managed_path, &snapshot).await
    }
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

async fn write_json_file(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut to_write = value.clone();
    if !is_legacy_opencode_path(path) {
        hoist_persisted_secrets(&mut to_write)?;
        strip_persisted_secrets(&mut to_write);
    }
    let raw = serde_json::to_string_pretty(&to_write)?;
    fs::write(path, raw).await?;
    Ok(())
}

fn strip_persisted_secrets(value: &mut Value) {
    if let Value::Object(root) = value {
        if let Some(channels) = root.get_mut("channels").and_then(|v| v.as_object_mut()) {
            for channel in ["telegram", "discord", "slack"] {
                if let Some(cfg) = channels.get_mut(channel).and_then(|v| v.as_object_mut()) {
                    cfg.remove("bot_token");
                    cfg.remove("botToken");
                }
            }
            // The Slack signing secret (top-level and per-connection,
            // TAN-763) is hoisted to the secret store just like bot tokens —
            // no Slack credential may persist in the plaintext config file.
            if let Some(cfg) = channels.get_mut("slack").and_then(|v| v.as_object_mut()) {
                cfg.remove("signing_secret");
                cfg.remove("signingSecret");
            }
            for entry in slack_connection_entries(channels) {
                entry.remove("bot_token");
                entry.remove("botToken");
                entry.remove("signing_secret");
                entry.remove("signingSecret");
            }
        }

        let Some(providers) = root.get_mut("providers").and_then(|v| v.as_object_mut()) else {
            return;
        };
        for (_provider_id, provider_cfg) in providers.iter_mut() {
            let Value::Object(cfg) = provider_cfg else {
                continue;
            };
            cfg.remove("api_key");
            cfg.remove("apiKey");
        }
    }
}

/// Mutable views of every object entry under `channels.slack.connections`.
fn slack_connection_entries(
    channels: &mut serde_json::Map<String, Value>,
) -> Vec<&mut serde_json::Map<String, Value>> {
    channels
        .get_mut("slack")
        .and_then(|v| v.as_object_mut())
        .and_then(|cfg| cfg.get_mut("connections"))
        .and_then(|v| v.as_array_mut())
        .map(|entries| {
            entries
                .iter_mut()
                .filter_map(Value::as_object_mut)
                .collect()
        })
        .unwrap_or_default()
}

/// Stable secret-store id for one Slack connection entry's credential. Keyed
/// by the entry's OWN (team, app, channel) fields — inherited values are
/// deliberately excluded so editing the top level never orphans a stored
/// per-connection secret.
fn slack_connection_secret_store_id(entry: &serde_json::Map<String, Value>, field: &str) -> String {
    let raw = |keys: [&str; 2]| {
        keys.iter()
            .find_map(|key| entry.get(*key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("-")
            .to_string()
    };
    let team = raw(["team_id", "workspace_id"]);
    let app = raw(["app_id", "api_app_id"]);
    let channel_id = raw(["channel_id", "channel_id"]);
    // The provider-auth store normalizes ids to lowercase; match it so
    // store and lookup agree.
    format!("channel::slack::connection::{team}::{app}::{channel_id}::{field}").to_ascii_lowercase()
}

const SLACK_CONNECTION_SECRET_FIELDS: [(&str, &str); 2] = [
    ("bot_token", "botToken"),
    ("signing_secret", "signingSecret"),
];

/// Prefix of every stored top-level `channels.slack.signing_secret`. The
/// full id is keyed by the configured installation (`::{team}::{app}`
/// suffix): a signing secret is an app credential, so migrating the config
/// from app A to app B must NOT inject A's stored secret as B's — the new
/// identity resolves secretless and fails closed until its own secret is
/// entered.
pub const SLACK_SIGNING_SECRET_STORE_PREFIX: &str = "channel::slack::signing_secret";

/// Installation-keyed secret-store id for the top-level Slack signing
/// secret of this raw `channels.slack` object.
fn slack_signing_secret_store_id(cfg: &serde_json::Map<String, Value>) -> String {
    let raw = |keys: [&str; 2]| {
        keys.iter()
            .find_map(|key| cfg.get(*key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("-")
            .to_string()
    };
    let team = raw(["team_id", "workspace_id"]);
    let app = raw(["app_id", "api_app_id"]);
    format!("{SLACK_SIGNING_SECRET_STORE_PREFIX}::{team}::{app}").to_ascii_lowercase()
}

/// Prefix of every per-connection Slack credential in the secret store.
pub const SLACK_CONNECTION_SECRET_PREFIX: &str = "channel::slack::connection::";

/// Secret-store ids for one raw `connections[]` entry's credentials
/// (bot token + signing secret). Used by removal paths to purge stored
/// secrets for connections that no longer exist in config.
pub fn slack_connection_secret_ids(entry: &serde_json::Map<String, Value>) -> [String; 2] {
    [
        slack_connection_secret_store_id(entry, "bot_token"),
        slack_connection_secret_store_id(entry, "signing_secret"),
    ]
}

/// Delete every stored Slack credential — the top-level signing secret and
/// all per-connection entries. Called when the Slack channel is deleted so
/// a later re-add cannot silently resurrect revoked credentials.
pub fn purge_slack_channel_secrets() {
    let stored = crate::load_provider_auth();
    for id in stored.keys().filter(|key| {
        key.starts_with(SLACK_CONNECTION_SECRET_PREFIX)
            || key.starts_with(SLACK_SIGNING_SECRET_STORE_PREFIX)
    }) {
        let _ = crate::delete_provider_auth(id);
    }
}

fn hoist_persisted_secrets(value: &mut Value) -> anyhow::Result<()> {
    let Value::Object(root) = value else {
        return Ok(());
    };

    if let Some(channels) = root.get_mut("channels").and_then(|v| v.as_object_mut()) {
        for channel in ["telegram", "discord", "slack"] {
            let Some(cfg) = channels.get_mut(channel).and_then(|v| v.as_object_mut()) else {
                continue;
            };
            let token = cfg
                .get("bot_token")
                .or_else(|| cfg.get("botToken"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if token.is_empty() || channel_has_runtime_secret(channel) {
                continue;
            }
            if let Some(secret_id) = channel_secret_store_id(channel) {
                crate::set_provider_auth(&secret_id, &token)?;
            }
        }
        // Slack credentials (the top-level signing secret and per-connection
        // credentials, TAN-763) hoist alongside the top-level token so
        // `strip_persisted_secrets` can remove them from the plaintext file
        // without losing them. An EXPLICITLY cleared field (present but
        // empty, or null) is a revocation: the stored secret is deleted so
        // injection cannot silently resurrect a rotated or compromised
        // credential. An ABSENT field is the normal state of an
        // already-stripped config and leaves the stored secret alone.
        if let Some(cfg) = channels.get_mut("slack").and_then(|v| v.as_object_mut()) {
            if let Some(provided) = cfg
                .get("signing_secret")
                .or_else(|| cfg.get("signingSecret"))
            {
                let secret = provided
                    .as_str()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string();
                let secret_id = slack_signing_secret_store_id(cfg);
                if secret.is_empty() {
                    let _ = crate::delete_provider_auth(&secret_id);
                } else {
                    crate::set_provider_auth(&secret_id, &secret)?;
                }
            }
        }
        for entry in slack_connection_entries(channels) {
            for (field, alias) in SLACK_CONNECTION_SECRET_FIELDS {
                let Some(provided) = entry.get(field).or_else(|| entry.get(alias)) else {
                    continue;
                };
                let secret = provided
                    .as_str()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string();
                let secret_id = slack_connection_secret_store_id(entry, field);
                if secret.is_empty() {
                    let _ = crate::delete_provider_auth(&secret_id);
                } else {
                    crate::set_provider_auth(&secret_id, &secret)?;
                }
            }
        }
    }

    if let Some(providers) = root.get_mut("providers").and_then(|v| v.as_object_mut()) {
        for (provider_id, provider_cfg) in providers.iter_mut() {
            let Value::Object(cfg) = provider_cfg else {
                continue;
            };
            let token = cfg
                .get("api_key")
                .or_else(|| cfg.get("apiKey"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if token.is_empty() || provider_has_runtime_secret(provider_id) {
                continue;
            }
            crate::set_provider_auth(provider_id, &token)?;
        }
    }

    Ok(())
}

fn channel_has_runtime_secret(channel_id: &str) -> bool {
    let key = match channel_id {
        "telegram" => "TANDEM_TELEGRAM_BOT_TOKEN",
        "discord" => "TANDEM_DISCORD_BOT_TOKEN",
        "slack" => "TANDEM_SLACK_BOT_TOKEN",
        _ => return false,
    };
    std::env::var(key)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

pub fn channel_secret_store_id(channel_id: &str) -> Option<String> {
    let normalized = channel_id.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "telegram" | "discord" | "slack" => Some(format!("channel::{normalized}::bot_token")),
        _ => None,
    }
}

async fn scrub_persisted_secrets(value: &mut Value, path: Option<&Path>) -> anyhow::Result<()> {
    if let Some(target) = path {
        if is_legacy_opencode_path(target) {
            return Ok(());
        }
    }
    let before = value.clone();
    hoist_persisted_secrets(value)?;
    strip_persisted_secrets(value);
    if *value != before {
        if let Some(target) = path {
            write_json_file(target, value).await?;
        }
    }
    Ok(())
}

fn is_legacy_opencode_path(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("opencode")
}

fn provider_has_runtime_secret(provider_id: &str) -> bool {
    provider_env_candidates(provider_id).into_iter().any(|key| {
        std::env::var(&key)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

fn provider_env_candidates(provider_id: &str) -> Vec<String> {
    let normalized = provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_ascii_uppercase();

    let mut out = vec![format!("{}_API_KEY", normalized)];

    match provider_id.to_ascii_lowercase().as_str() {
        "openai" => out.push("OPENAI_API_KEY".to_string()),
        "openrouter" => out.push("OPENROUTER_API_KEY".to_string()),
        "anthropic" => out.push("ANTHROPIC_API_KEY".to_string()),
        "groq" => out.push("GROQ_API_KEY".to_string()),
        "mistral" => out.push("MISTRAL_API_KEY".to_string()),
        "together" => out.push("TOGETHER_API_KEY".to_string()),
        "azure" => out.push("AZURE_OPENAI_API_KEY".to_string()),
        "vertex" => out.push("VERTEX_API_KEY".to_string()),
        "bedrock" => out.push("BEDROCK_API_KEY".to_string()),
        "copilot" => out.push("GITHUB_TOKEN".to_string()),
        "cohere" => out.push("COHERE_API_KEY".to_string()),
        "zen" | "opencode_zen" | "opencodezen" => out.push("OPENCODE_ZEN_API_KEY".to_string()),
        _ => {}
    }

    out.sort();
    out.dedup();
    out
}

fn inject_stored_channel_secrets(value: &mut Value) {
    let Value::Object(root) = value else {
        return;
    };
    let Some(channels) = root.get_mut("channels").and_then(|v| v.as_object_mut()) else {
        return;
    };
    let secrets = crate::load_provider_auth();
    for channel in ["telegram", "discord", "slack"] {
        if channel_has_runtime_secret(channel) {
            continue;
        }
        let Some(cfg) = channels.get_mut(channel).and_then(|v| v.as_object_mut()) else {
            continue;
        };
        let already_present = cfg
            .get("bot_token")
            .or_else(|| cfg.get("botToken"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if already_present {
            continue;
        }
        let Some(secret_id) = channel_secret_store_id(channel) else {
            continue;
        };
        let Some(token) = secrets.get(&secret_id) else {
            continue;
        };
        if !token.trim().is_empty() {
            cfg.insert("bot_token".to_string(), Value::String(token.clone()));
        }
    }
    // Slack signing secrets (top-level and per-connection) come back from
    // the secret store the same way bot tokens do, so the effective config
    // resolves connections exactly as if the secrets were still in the file.
    if let Some(cfg) = channels.get_mut("slack").and_then(|v| v.as_object_mut()) {
        let already_present = cfg
            .get("signing_secret")
            .or_else(|| cfg.get("signingSecret"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !already_present {
            // Installation-keyed lookup: a config whose team/app changed
            // resolves a DIFFERENT id, so the previous app's secret is never
            // injected as the new app's — it fails closed instead.
            let secret_id = slack_signing_secret_store_id(cfg);
            if let Some(secret) = secrets.get(&secret_id) {
                if !secret.trim().is_empty() {
                    cfg.insert("signing_secret".to_string(), Value::String(secret.clone()));
                }
            }
        }
    }
    for entry in slack_connection_entries(channels) {
        for (field, alias) in SLACK_CONNECTION_SECRET_FIELDS {
            let already_present = entry
                .get(field)
                .or_else(|| entry.get(alias))
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if already_present {
                continue;
            }
            let secret_id = slack_connection_secret_store_id(entry, field);
            let Some(secret) = secrets.get(&secret_id) else {
                continue;
            };
            if !secret.trim().is_empty() {
                entry.insert(field.to_string(), Value::String(secret.clone()));
            }
        }
    }
}

fn inject_stored_provider_auth(value: &mut Value) {
    let Value::Object(root) = value else {
        return;
    };
    let Some(providers) = root.get_mut("providers").and_then(|v| v.as_object_mut()) else {
        return;
    };
    let secrets = crate::load_provider_auth();
    let credentials = crate::load_provider_credentials();
    for (provider_id, provider_cfg) in providers.iter_mut() {
        let Value::Object(cfg) = provider_cfg else {
            continue;
        };
        let already_present = cfg
            .get("api_key")
            .or_else(|| cfg.get("apiKey"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if already_present || provider_has_runtime_secret(provider_id) {
            continue;
        }
        let normalized_provider_id = provider_id.to_ascii_lowercase();

        if let Some(credential) = credentials.get(&normalized_provider_id) {
            if let Some(token) = credential.runtime_bearer_token() {
                cfg.insert("api_key".to_string(), Value::String(token.to_string()));
            }
            match credential {
                crate::ProviderCredential::ApiKey(_) => {
                    cfg.insert(
                        "auth_kind".to_string(),
                        Value::String("api_key".to_string()),
                    );
                }
                crate::ProviderCredential::OAuth(oauth) => {
                    cfg.insert("auth_kind".to_string(), Value::String("oauth".to_string()));
                    cfg.insert(
                        "expires_at_ms".to_string(),
                        Value::Number(serde_json::Number::from(oauth.expires_at_ms)),
                    );
                    cfg.insert(
                        "managed_by".to_string(),
                        Value::String(oauth.managed_by.clone()),
                    );
                    if let Some(account_id) = &oauth.account_id {
                        cfg.insert("account_id".to_string(), Value::String(account_id.clone()));
                    }
                    if let Some(email) = &oauth.email {
                        cfg.insert("email".to_string(), Value::String(email.clone()));
                    }
                    if let Some(display_name) = &oauth.display_name {
                        cfg.insert(
                            "display_name".to_string(),
                            Value::String(display_name.clone()),
                        );
                    }
                }
            }
            continue;
        }

        let Some(token) = secrets.get(&normalized_provider_id) else {
            continue;
        };
        if !token.trim().is_empty() {
            cfg.insert(
                "auth_kind".to_string(),
                Value::String("api_key".to_string()),
            );
            cfg.insert("api_key".to_string(), Value::String(token.clone()));
        }
    }
}

async fn read_json_file(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(empty_object());
    }
    let raw = fs::read_to_string(path).await?;
    Ok(serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| empty_object()))
}

async fn resolve_global_config_path() -> anyhow::Result<PathBuf> {
    if let Ok(path) =
        std::env::var("AGENT_GLOBAL_CONFIG").or_else(|_| std::env::var("TANDEM_GLOBAL_CONFIG"))
    {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        return Ok(path);
    }
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("tandem").join("config.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        return Ok(path);
    }
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".config").join("tandem").join("config.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        return Ok(path);
    }
    Ok(PathBuf::from("config.json"))
}

fn env_layer() -> Value {
    let mut root = empty_object();

    if let Ok(bot_name) =
        std::env::var("AGENT_BOT_NAME").or_else(|_| std::env::var("TANDEM_BOT_NAME"))
    {
        if !bot_name.trim().is_empty() {
            deep_merge(
                &mut root,
                &json!({
                    "identity": {
                        "bot": {
                            "canonical_name": bot_name.trim()
                        }
                    }
                }),
            );
        }
    }
    if let Ok(persona) = std::env::var("AGENT_PERSONA").or_else(|_| std::env::var("TANDEM_PERSONA"))
    {
        if !persona.trim().is_empty() {
            deep_merge(
                &mut root,
                &json!({
                    "identity": {
                        "personality": {
                            "default": {
                                "custom_instructions": persona.trim()
                            }
                        }
                    }
                }),
            );
        }
    }

    if let Ok(enabled) = std::env::var("TANDEM_WEB_UI") {
        if let Some(v) = parse_bool_like(&enabled) {
            deep_merge(&mut root, &json!({ "web_ui": { "enabled": v } }));
        }
    }
    if let Ok(prefix) = std::env::var("TANDEM_WEB_UI_PREFIX") {
        if !prefix.trim().is_empty() {
            deep_merge(&mut root, &json!({ "web_ui": { "path_prefix": prefix } }));
        }
    }
    if let Ok(enabled) = std::env::var("TANDEM_BROWSER_ENABLED") {
        if let Some(v) = parse_bool_like(&enabled) {
            deep_merge(&mut root, &json!({ "browser": { "enabled": v } }));
        }
    }
    if let Ok(path) = std::env::var("TANDEM_BROWSER_SIDECAR") {
        if !path.trim().is_empty() {
            deep_merge(
                &mut root,
                &json!({ "browser": { "sidecar_path": path.trim() } }),
            );
        }
    }
    if let Ok(path) = std::env::var("TANDEM_BROWSER_EXECUTABLE") {
        if !path.trim().is_empty() {
            deep_merge(
                &mut root,
                &json!({ "browser": { "executable_path": path.trim() } }),
            );
        }
    }
    if let Ok(headless) = std::env::var("TANDEM_BROWSER_HEADLESS") {
        if let Some(v) = parse_bool_like(&headless) {
            deep_merge(&mut root, &json!({ "browser": { "headless_default": v } }));
        }
    }
    if let Ok(allow_no_sandbox) = std::env::var("TANDEM_BROWSER_ALLOW_NO_SANDBOX") {
        if let Some(v) = parse_bool_like(&allow_no_sandbox) {
            deep_merge(&mut root, &json!({ "browser": { "allow_no_sandbox": v } }));
        }
    }
    if let Ok(path) = std::env::var("TANDEM_BROWSER_USER_DATA_ROOT") {
        if !path.trim().is_empty() {
            deep_merge(
                &mut root,
                &json!({ "browser": { "user_data_root": path.trim() } }),
            );
        }
    }
    if let Ok(hosts) = std::env::var("TANDEM_BROWSER_ALLOWED_HOSTS") {
        let parsed = parse_csv(&hosts);
        if !parsed.is_empty() {
            deep_merge(
                &mut root,
                &json!({ "browser": { "allowed_hosts": parsed } }),
            );
        }
    }
    if let Ok(token) = std::env::var("TANDEM_TELEGRAM_BOT_TOKEN") {
        if !token.trim().is_empty() {
            let allowed = std::env::var("TANDEM_TELEGRAM_ALLOWED_USERS")
                .map(|s| parse_csv(&s))
                .unwrap_or_else(|_| vec!["*".to_string()]);
            let mention_only = std::env::var("TANDEM_TELEGRAM_MENTION_ONLY")
                .ok()
                .and_then(|v| parse_bool_like(&v))
                .unwrap_or(false);
            deep_merge(
                &mut root,
                &json!({
                    "channels": {
                        "telegram": {
                            "bot_token": token,
                            "allowed_users": allowed,
                            "mention_only": mention_only
                        }
                    }
                }),
            );
        }
    }
    if let Ok(token) = std::env::var("TANDEM_DISCORD_BOT_TOKEN") {
        if !token.trim().is_empty() {
            let allowed = std::env::var("TANDEM_DISCORD_ALLOWED_USERS")
                .map(|s| parse_csv(&s))
                .unwrap_or_else(|_| vec!["*".to_string()]);
            let mention_only = std::env::var("TANDEM_DISCORD_MENTION_ONLY")
                .ok()
                .and_then(|v| parse_bool_like(&v))
                .unwrap_or(true);
            let guild_id = std::env::var("TANDEM_DISCORD_GUILD_ID").ok();
            deep_merge(
                &mut root,
                &json!({
                    "channels": {
                        "discord": {
                            "bot_token": token,
                            "guild_id": guild_id,
                            "allowed_users": allowed,
                            "mention_only": mention_only
                        }
                    }
                }),
            );
        }
    }
    if let Ok(token) = std::env::var("TANDEM_SLACK_BOT_TOKEN") {
        if !token.trim().is_empty() {
            if let Ok(channel_id) = std::env::var("TANDEM_SLACK_CHANNEL_ID") {
                if !channel_id.trim().is_empty() {
                    let allowed = std::env::var("TANDEM_SLACK_ALLOWED_USERS")
                        .map(|s| parse_csv(&s))
                        .unwrap_or_else(|_| vec!["*".to_string()]);
                    deep_merge(
                        &mut root,
                        &json!({
                            "channels": {
                                "slack": {
                                    "bot_token": token,
                                    "channel_id": channel_id,
                                    "allowed_users": allowed
                                }
                            }
                        }),
                    );
                }
            }
        }
    }

    add_openai_env(&mut root, "openai", "OPENAI_API_KEY");
    add_openai_env(&mut root, "openrouter", "OPENROUTER_API_KEY");
    add_openai_env(&mut root, "groq", "GROQ_API_KEY");
    add_openai_env(&mut root, "mistral", "MISTRAL_API_KEY");
    add_openai_env(&mut root, "together", "TOGETHER_API_KEY");
    add_openai_env(&mut root, "azure", "AZURE_OPENAI_API_KEY");
    add_openai_env(&mut root, "vertex", "VERTEX_API_KEY");
    add_openai_env(&mut root, "bedrock", "BEDROCK_API_KEY");
    add_openai_env(&mut root, "copilot", "GITHUB_TOKEN");
    add_openai_env(&mut root, "cohere", "COHERE_API_KEY");
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        deep_merge(
            &mut root,
            &json!({
                "providers": {
                    "anthropic": {
                        "api_key": api_key,
                        "url": "https://api.anthropic.com/v1",
                        "default_model": "claude-sonnet-4-6"
                    }
                }
            }),
        );
    }
    if let Ok(ollama_url) = std::env::var("OLLAMA_URL") {
        deep_merge(
            &mut root,
            &json!({
                "providers": {
                    "ollama": {
                        "url": ollama_url,
                        "default_model": "llama3.1:8b"
                    }
                }
            }),
        );
    } else if std::net::TcpStream::connect("127.0.0.1:11434").is_ok() {
        deep_merge(
            &mut root,
            &json!({
                "providers": {
                    "ollama": {
                        "url": "http://127.0.0.1:11434/v1",
                        "default_model": "llama3.1:8b"
                    }
                }
            }),
        );
    }

    root
}

fn parse_bool_like(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_csv(raw: &str) -> Vec<String> {
    if raw.trim() == "*" {
        return vec!["*".to_string()];
    }
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn first_nonempty_env(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key).ok().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    })
}

fn add_openai_env(root: &mut Value, provider: &str, key_env: &str) {
    let Ok(api_key) = std::env::var(key_env) else {
        return;
    };

    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return;
    }

    let mut provider_cfg = json!({
        "api_key": api_key,
    });

    // Preserve explicit model selection from config by default.
    // Only apply env-layer default_model when an explicit model env is provided.
    let provider_upper = provider.to_ascii_uppercase();
    let inferred_model_key = key_env.replace("API_KEY", "MODEL");
    let model_keys = vec![
        format!("{provider_upper}_MODEL"),
        format!("{provider_upper}_DEFAULT_MODEL"),
        inferred_model_key,
    ];
    let explicit_model = first_nonempty_env(&model_keys);
    if model_keys.iter().any(|key| {
        std::env::var(key)
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
    }) {
        if let Some(model) = explicit_model {
            provider_cfg["default_model"] = Value::String(model);
        }
    }

    deep_merge(
        root,
        &json!({
            "providers": {
                provider: provider_cfg
            }
        }),
    );
}

fn deep_merge(base: &mut Value, overlay: &Value) {
    if overlay.is_null() {
        return;
    }
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                if value.is_null() {
                    continue;
                }
                match base_map.get_mut(key) {
                    Some(existing) => deep_merge(existing, value),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value.clone();
        }
    }
}

impl From<ProviderConfig> for tandem_providers::ProviderConfig {
    fn from(value: ProviderConfig) -> Self {
        Self {
            api_key: value.api_key,
            url: value.url,
            default_model: value.default_model,
        }
    }
}

impl From<AppConfig> for tandem_providers::AppConfig {
    fn from(value: AppConfig) -> Self {
        Self {
            providers: value
                .providers
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            default_provider: value.default_provider,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn unique_temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!("tandem-core-config-{name}-{ts}.json"));
        path
    }

    fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock")
    }

    struct ProviderAuthTestGuard {
        _guard: tokio::sync::MutexGuard<'static, ()>,
        previous_disable_keyring: Option<OsString>,
        previous_tandem_home: Option<OsString>,
        tandem_home: PathBuf,
    }

    impl Drop for ProviderAuthTestGuard {
        fn drop(&mut self) {
            restore_env_var(
                "TANDEM_PROVIDER_AUTH_DISABLE_KEYRING",
                self.previous_disable_keyring.take(),
            );
            restore_env_var("TANDEM_HOME", self.previous_tandem_home.take());
            let _ = std::fs::remove_dir_all(&self.tandem_home);
        }
    }

    fn restore_env_var(name: &str, previous: Option<OsString>) {
        if let Some(value) = previous {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }

    async fn provider_auth_test_guard() -> ProviderAuthTestGuard {
        static PROVIDER_AUTH_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        let guard = PROVIDER_AUTH_TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let tandem_home =
            std::env::temp_dir().join(format!("tandem-core-provider-auth-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tandem_home).expect("provider auth test home");
        let previous_disable_keyring = std::env::var_os("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING");
        let previous_tandem_home = std::env::var_os("TANDEM_HOME");
        std::env::set_var("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING", "1");
        std::env::set_var("TANDEM_HOME", &tandem_home);
        ProviderAuthTestGuard {
            _guard: guard,
            previous_disable_keyring,
            previous_tandem_home,
            tandem_home,
        }
    }

    #[test]
    fn strip_persisted_secrets_removes_channel_bot_tokens_without_runtime_env() {
        let mut value = json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["*"]
                },
                "discord": {
                    "botToken": "dc-secret",
                    "allowed_users": ["*"],
                    "mention_only": true
                },
                "slack": {
                    "bot_token": "sl-secret",
                    "channel_id": "C123"
                }
            },
            "providers": {}
        });

        strip_persisted_secrets(&mut value);

        assert!(value
            .get("channels")
            .and_then(|v| v.get("telegram"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
        assert!(value
            .get("channels")
            .and_then(|v| v.get("discord"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("botToken")));
        assert!(value
            .get("channels")
            .and_then(|v| v.get("slack"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
    }

    #[tokio::test]
    async fn scrub_persisted_secrets_moves_channel_tokens_off_disk_without_runtime_env() {
        let _guard = provider_auth_test_guard().await;
        let path = unique_temp_file("scrub");
        let original = json!({
            "channels": {
                "slack": {
                    "bot_token": "sl-secret",
                    "channel_id": "C123"
                }
            },
            "providers": {}
        });
        let raw = serde_json::to_string_pretty(&original).expect("serialize");
        fs::write(&path, raw).await.expect("write");

        let mut loaded =
            serde_json::from_str::<Value>(&fs::read_to_string(&path).await.expect("read before"))
                .expect("parse");

        scrub_persisted_secrets(&mut loaded, Some(&path))
            .await
            .expect("scrub");

        let persisted =
            serde_json::from_str::<Value>(&fs::read_to_string(&path).await.expect("read after"))
                .expect("parse persisted");
        assert!(persisted
            .get("channels")
            .and_then(|v| v.get("slack"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
        let stored = crate::load_provider_auth();
        assert_eq!(
            stored.get("channel::slack::bot_token").map(|v| v.as_str()),
            Some("sl-secret")
        );
        let _ = crate::delete_provider_auth("channel::slack::bot_token");

        let _ = fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn scrub_persisted_secrets_covers_slack_connection_credentials() {
        // PR #1910 review (P1): per-connection Slack credentials must be
        // hoisted to the secret store and stripped from the plaintext file
        // exactly like top-level tokens — and injected back on read so the
        // effective config resolves connections unchanged.
        let _guard = provider_auth_test_guard().await;
        let path = unique_temp_file("scrub-connections");
        let original = json!({
            "channels": {
                "slack": {
                    "team_id": "T1",
                    "app_id": "A1",
                    "signing_secret": "shh-top-level",
                    "connections": [
                        {
                            "channel_id": "C_SALES",
                            "bot_token": "xoxb-sales",
                            "signing_secret": "shh-sales"
                        },
                        {
                            "channel_id": "C_ENG",
                            "team_id": "T2",
                            "app_id": "A2",
                            "bot_token": "xoxb-eng"
                        }
                    ]
                }
            },
            "providers": {}
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&original).expect("serialize"),
        )
        .await
        .expect("write");

        let mut loaded =
            serde_json::from_str::<Value>(&fs::read_to_string(&path).await.expect("read before"))
                .expect("parse");
        scrub_persisted_secrets(&mut loaded, Some(&path))
            .await
            .expect("scrub");

        // On disk: no credential survives — neither the top-level signing
        // secret nor any connection entry's.
        let persisted =
            serde_json::from_str::<Value>(&fs::read_to_string(&path).await.expect("read after"))
                .expect("parse persisted");
        assert!(
            persisted
                .pointer("/channels/slack/signing_secret")
                .is_none(),
            "the top-level signing secret must not persist in plaintext"
        );
        let entries = persisted
            .pointer("/channels/slack/connections")
            .and_then(Value::as_array)
            .expect("connections persisted");
        for entry in entries {
            let entry = entry.as_object().expect("entry object");
            assert!(
                !entry.contains_key("bot_token") && !entry.contains_key("signing_secret"),
                "per-connection credentials must not persist in plaintext: {entry:?}"
            );
        }

        // In the secret store: each credential under its own id.
        let stored = crate::load_provider_auth();
        assert_eq!(
            stored
                .get("channel::slack::signing_secret::t1::a1")
                .map(String::as_str),
            Some("shh-top-level")
        );
        assert_eq!(
            stored
                .get("channel::slack::connection::-::-::c_sales::bot_token")
                .map(String::as_str),
            Some("xoxb-sales")
        );
        assert_eq!(
            stored
                .get("channel::slack::connection::-::-::c_sales::signing_secret")
                .map(String::as_str),
            Some("shh-sales")
        );
        assert_eq!(
            stored
                .get("channel::slack::connection::t2::a2::c_eng::bot_token")
                .map(String::as_str),
            Some("xoxb-eng")
        );

        // Injection restores the credentials for the effective config.
        let mut rehydrated = persisted.clone();
        inject_stored_channel_secrets(&mut rehydrated);
        assert_eq!(
            rehydrated.pointer("/channels/slack/signing_secret"),
            Some(&json!("shh-top-level"))
        );
        assert_eq!(
            rehydrated.pointer("/channels/slack/connections/0/bot_token"),
            Some(&json!("xoxb-sales"))
        );
        assert_eq!(
            rehydrated.pointer("/channels/slack/connections/0/signing_secret"),
            Some(&json!("shh-sales"))
        );
        assert_eq!(
            rehydrated.pointer("/channels/slack/connections/1/bot_token"),
            Some(&json!("xoxb-eng"))
        );

        // A config migrated to a DIFFERENT app identity must not inherit the
        // previous app's stored signing secret — the id is keyed by the
        // installation, so the new identity resolves secretless.
        let mut migrated = json!({
            "channels": { "slack": { "team_id": "T1", "app_id": "A_NEW" } }
        });
        inject_stored_channel_secrets(&mut migrated);
        assert!(
            migrated.pointer("/channels/slack/signing_secret").is_none(),
            "a changed app identity must not inject the previous app's secret"
        );

        // An EXPLICIT clear (present-but-empty field) revokes the stored
        // secret — injection must not silently resurrect a rotated or
        // compromised credential.
        let mut cleared = json!({
            "channels": {
                "slack": {
                    "team_id": "T1",
                    "app_id": "A1",
                    "signing_secret": "",
                    "connections": [
                        { "channel_id": "C_SALES", "bot_token": "", "signing_secret": "" }
                    ]
                }
            }
        });
        scrub_persisted_secrets(&mut cleared, Some(&path))
            .await
            .expect("scrub cleared");
        let stored = crate::load_provider_auth();
        assert!(
            !stored.contains_key("channel::slack::connection::-::-::c_sales::bot_token"),
            "clearing the field must delete the stored secret"
        );
        assert!(!stored.contains_key("channel::slack::connection::-::-::c_sales::signing_secret"));
        assert!(
            !stored.contains_key("channel::slack::signing_secret::t1::a1"),
            "clearing the top-level signing secret must delete the stored secret"
        );
        let mut rehydrated = cleared.clone();
        inject_stored_channel_secrets(&mut rehydrated);
        assert!(
            rehydrated
                .pointer("/channels/slack/connections/0/bot_token")
                .is_none(),
            "a cleared credential must not be injected back"
        );

        for id in [
            "channel::slack::connection::-::-::c_sales::bot_token",
            "channel::slack::connection::-::-::c_sales::signing_secret",
            "channel::slack::connection::t2::a2::c_eng::bot_token",
            "channel::slack::signing_secret::t1::a1",
        ] {
            let _ = crate::delete_provider_auth(id);
        }
        let _ = fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn config_store_rehydrates_channel_token_from_secure_store() {
        let _guard = provider_auth_test_guard().await;
        let path = unique_temp_file("rehydrate-channel");
        let original = json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["@alice"]
                }
            }
        });
        fs::write(
            &path,
            serde_json::to_string_pretty(&original).expect("serialize"),
        )
        .await
        .expect("write");

        let store = ConfigStore::new(&path, None).await.expect("config store");
        let effective = store.get_effective_value().await;
        assert_eq!(
            effective
                .get("channels")
                .and_then(|v| v.get("telegram"))
                .and_then(|v| v.get("bot_token"))
                .and_then(Value::as_str),
            Some("tg-secret")
        );

        let persisted =
            serde_json::from_str::<Value>(&fs::read_to_string(&path).await.expect("read after"))
                .expect("parse persisted");
        assert!(persisted
            .get("channels")
            .and_then(|v| v.get("telegram"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));

        let _ = crate::delete_provider_auth("channel::telegram::bot_token");
        let _ = fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn strip_persisted_secrets_removes_channel_bot_tokens_with_runtime_env() {
        let _provider_guard = provider_auth_test_guard().await;
        let _guard = env_test_lock();
        std::env::set_var("TANDEM_TELEGRAM_BOT_TOKEN", "runtime-secret");
        std::env::set_var("TANDEM_DISCORD_BOT_TOKEN", "runtime-secret");
        std::env::set_var("TANDEM_SLACK_BOT_TOKEN", "runtime-secret");

        let mut value = json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret"
                },
                "discord": {
                    "botToken": "dc-secret"
                },
                "slack": {
                    "bot_token": "sl-secret"
                }
            }
        });

        strip_persisted_secrets(&mut value);

        assert!(value
            .get("channels")
            .and_then(|v| v.get("telegram"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
        assert!(value
            .get("channels")
            .and_then(|v| v.get("discord"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("botToken")));
        assert!(value
            .get("channels")
            .and_then(|v| v.get("slack"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));

        std::env::remove_var("TANDEM_TELEGRAM_BOT_TOKEN");
        std::env::remove_var("TANDEM_DISCORD_BOT_TOKEN");
        std::env::remove_var("TANDEM_SLACK_BOT_TOKEN");
    }

    #[test]
    fn openrouter_api_key_env_does_not_override_default_model_without_model_env() {
        let _guard = env_test_lock();
        std::env::set_var("OPENROUTER_API_KEY", "sk-test");
        std::env::remove_var("OPENROUTER_MODEL");
        std::env::remove_var("OPENROUTER_DEFAULT_MODEL");

        let env_layer: Value = env_layer();
        let default_model = env_layer
            .get("providers")
            .and_then(|v| v.get("openrouter"))
            .and_then(|v| v.get("default_model"));
        assert!(default_model.is_none());

        std::env::remove_var("OPENROUTER_API_KEY");
    }

    #[test]
    fn openrouter_model_env_overrides_default_model_when_explicitly_set() {
        let _guard = env_test_lock();
        std::env::set_var("OPENROUTER_API_KEY", "sk-test");
        std::env::set_var("OPENROUTER_MODEL", "z-ai/glm-5");

        let env_layer: Value = env_layer();
        let default_model = env_layer
            .get("providers")
            .and_then(|v| v.get("openrouter"))
            .and_then(|v| v.get("default_model"))
            .and_then(Value::as_str);
        assert_eq!(default_model, Some("z-ai/glm-5"));

        std::env::remove_var("OPENROUTER_API_KEY");
        std::env::remove_var("OPENROUTER_MODEL");
    }

    #[test]
    fn openai_api_key_env_does_not_override_explicit_openai_compatible_url() {
        let _guard = env_test_lock();
        std::env::set_var("OPENAI_API_KEY", "sk-test");

        let mut merged = json!({
            "providers": {
                "openai": {
                    "url": "https://api.minimax.io/v1",
                    "default_model": "MiniMax-M2"
                }
            }
        });
        deep_merge(&mut merged, &env_layer());

        assert_eq!(
            merged["providers"]["openai"]["url"],
            json!("https://api.minimax.io/v1")
        );
        assert_eq!(
            merged["providers"]["openai"]["default_model"],
            json!("MiniMax-M2")
        );
        assert_eq!(merged["providers"]["openai"]["api_key"], json!("sk-test"));

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn browser_env_layer_maps_browser_settings() {
        let _guard = env_test_lock();
        std::env::set_var("TANDEM_BROWSER_ENABLED", "true");
        std::env::set_var("TANDEM_BROWSER_SIDECAR", "/tmp/tandem-browser");
        std::env::set_var("TANDEM_BROWSER_EXECUTABLE", "/tmp/chromium");
        std::env::set_var("TANDEM_BROWSER_HEADLESS", "false");
        std::env::set_var("TANDEM_BROWSER_ALLOW_NO_SANDBOX", "true");
        std::env::set_var("TANDEM_BROWSER_USER_DATA_ROOT", "/tmp/tandem-browser-data");
        std::env::set_var("TANDEM_BROWSER_ALLOWED_HOSTS", "example.com,internal.local");

        let env_layer: Value = env_layer();
        let browser = env_layer.get("browser").expect("browser block");
        assert_eq!(browser.get("enabled").and_then(Value::as_bool), Some(true));
        assert_eq!(
            browser.get("sidecar_path").and_then(Value::as_str),
            Some("/tmp/tandem-browser")
        );
        assert_eq!(
            browser.get("executable_path").and_then(Value::as_str),
            Some("/tmp/chromium")
        );
        assert_eq!(
            browser.get("headless_default").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            browser.get("allow_no_sandbox").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            browser.get("user_data_root").and_then(Value::as_str),
            Some("/tmp/tandem-browser-data")
        );
        assert_eq!(
            browser
                .get("allowed_hosts")
                .and_then(Value::as_array)
                .map(|rows| rows.len()),
            Some(2)
        );

        std::env::remove_var("TANDEM_BROWSER_ENABLED");
        std::env::remove_var("TANDEM_BROWSER_SIDECAR");
        std::env::remove_var("TANDEM_BROWSER_EXECUTABLE");
        std::env::remove_var("TANDEM_BROWSER_HEADLESS");
        std::env::remove_var("TANDEM_BROWSER_ALLOW_NO_SANDBOX");
        std::env::remove_var("TANDEM_BROWSER_USER_DATA_ROOT");
        std::env::remove_var("TANDEM_BROWSER_ALLOWED_HOSTS");
    }
}
