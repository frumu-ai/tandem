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

        let layers = ConfigLayers {
            global: read_json_file(&global_path)
                .await
                .unwrap_or_else(|_| empty_object()),
            project: read_json_file(&project_path)
                .await
                .unwrap_or_else(|_| empty_object()),
            managed: read_json_file(&managed_path)
                .await
                .unwrap_or_else(|_| empty_object()),
            env: env_layer(),
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
    let raw = serde_json::to_string_pretty(value)?;
    fs::write(path, raw).await?;
    Ok(())
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
