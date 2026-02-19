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
pub struct AppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    pub default_provider: Option<String>,
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
        }

        let Some(providers) = root.get_mut("providers").and_then(|v| v.as_object_mut()) else {
            return;
        };
        for (provider_id, provider_cfg) in providers.iter_mut() {
            let Value::Object(cfg) = provider_cfg else {
                continue;
            };
            if !cfg.contains_key("api_key") && !cfg.contains_key("apiKey") {
                continue;
            }
            if provider_has_runtime_secret(provider_id) {
                cfg.remove("api_key");
                cfg.remove("apiKey");
            }
        }
    }
}

async fn scrub_persisted_secrets(value: &mut Value, path: Option<&Path>) -> anyhow::Result<()> {
    if let Some(target) = path {
        if is_legacy_opencode_path(target) {
            return Ok(());
        }
    }
    let before = value.clone();
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

async fn read_json_file(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(empty_object());
    }
    let raw = fs::read_to_string(path).await?;
    Ok(serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| empty_object()))
}

async fn resolve_global_config_path() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("TANDEM_GLOBAL_CONFIG") {
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
    Ok(PathBuf::from(".tandem/global_config.json"))
}

fn env_layer() -> Value {
    let mut root = empty_object();

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

    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        deep_merge(
            &mut root,
            &json!({
                "providers": {
                    "openai": {
                        "api_key": api_key,
                        "url": "https://api.openai.com/v1",
                        "default_model": "gpt-4o-mini"
                    }
                }
            }),
        );
    }
    add_openai_env(
        &mut root,
        "openrouter",
        "OPENROUTER_API_KEY",
        "https://openrouter.ai/api/v1",
        "openai/gpt-4o-mini",
    );
    add_openai_env(
        &mut root,
        "groq",
        "GROQ_API_KEY",
        "https://api.groq.com/openai/v1",
        "llama-3.1-8b-instant",
    );
    add_openai_env(
        &mut root,
        "mistral",
        "MISTRAL_API_KEY",
        "https://api.mistral.ai/v1",
        "mistral-small-latest",
    );
    add_openai_env(
        &mut root,
        "together",
        "TOGETHER_API_KEY",
        "https://api.together.xyz/v1",
        "meta-llama/Llama-3.1-8B-Instruct-Turbo",
    );
    add_openai_env(
        &mut root,
        "azure",
        "AZURE_OPENAI_API_KEY",
        "https://example.openai.azure.com/openai/deployments/default",
        "gpt-4o-mini",
    );
    add_openai_env(
        &mut root,
        "vertex",
        "VERTEX_API_KEY",
        "https://aiplatform.googleapis.com/v1",
        "gemini-1.5-flash",
    );
    add_openai_env(
        &mut root,
        "bedrock",
        "BEDROCK_API_KEY",
        "https://bedrock-runtime.us-east-1.amazonaws.com",
        "anthropic.claude-3-5-sonnet-20240620-v1:0",
    );
    add_openai_env(
        &mut root,
        "copilot",
        "GITHUB_TOKEN",
        "https://api.githubcopilot.com",
        "gpt-4o-mini",
    );
    add_openai_env(
        &mut root,
        "cohere",
        "COHERE_API_KEY",
        "https://api.cohere.com/v2",
        "command-r-plus",
    );
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        deep_merge(
            &mut root,
            &json!({
                "providers": {
                    "anthropic": {
                        "api_key": api_key,
                        "url": "https://api.anthropic.com/v1",
                        "default_model": "claude-3-5-sonnet-latest"
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

fn add_openai_env(root: &mut Value, provider: &str, key_env: &str, default_url: &str, model: &str) {
    if let Ok(api_key) = std::env::var(key_env) {
        deep_merge(
            root,
            &json!({
                "providers": {
                    provider: {
                        "api_key": api_key,
                        "url": default_url,
                        "default_model": model
                    }
                }
            }),
        );
    }
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!("tandem-core-config-{name}-{ts}.json"));
        path
    }

    #[test]
    fn strip_persisted_secrets_removes_channel_bot_tokens() {
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
    async fn scrub_persisted_secrets_rewrites_channel_tokens_on_disk() {
        let path = unique_temp_file("scrub");
        let original = json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["@alice"]
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
            .and_then(|v| v.get("telegram"))
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));

        let _ = fs::remove_file(&path).await;
    }
}
