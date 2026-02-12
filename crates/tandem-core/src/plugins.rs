use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use tokio::sync::RwLock;

use crate::permissions::PermissionAction;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub system_prompt_prefix: Option<String>,
    pub system_prompt_suffix: Option<String>,
    #[serde(default)]
    pub allow_tools: Vec<String>,
    #[serde(default)]
    pub deny_tools: Vec<String>,
    #[serde(default)]
    pub shell_env: HashMap<String, String>,
    pub tool_output_suffix: Option<String>,
}

#[derive(Clone)]
pub struct PluginRegistry {
    plugins: Arc<RwLock<Vec<PluginManifest>>>,
}

impl PluginRegistry {
    pub async fn new(workspace_root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root: PathBuf = workspace_root.into();
        let plugins = load_plugins(root.join(".tandem").join("plugins")).await?;
        Ok(Self {
            plugins: Arc::new(RwLock::new(plugins)),
        })
    }

    pub async fn list(&self) -> Vec<PluginManifest> {
        self.plugins.read().await.clone()
    }

    pub async fn transform_prompt(&self, prompt: String) -> String {
        let plugins = self.plugins.read().await;
        let mut transformed = prompt;
        for plugin in plugins.iter().filter(|p| p.enabled) {
            if let Some(prefix) = &plugin.system_prompt_prefix {
                transformed = format!("{prefix}\n\n{transformed}");
            }
            if let Some(suffix) = &plugin.system_prompt_suffix {
                transformed = format!("{transformed}\n\n{suffix}");
            }
        }
        transformed
    }

    pub async fn permission_override(&self, tool_name: &str) -> Option<PermissionAction> {
        let plugins = self.plugins.read().await;
        let mut action = None;
        for plugin in plugins.iter().filter(|p| p.enabled) {
            if plugin.deny_tools.iter().any(|t| t == tool_name) {
                action = Some(PermissionAction::Deny);
            }
            if plugin.allow_tools.iter().any(|t| t == tool_name) {
                action = Some(PermissionAction::Allow);
            }
        }
        action
    }

    pub async fn inject_tool_args(&self, tool_name: &str, mut args: Value) -> Value {
        if tool_name != "bash" {
            return args;
        }

        let plugins = self.plugins.read().await;
        let mut merged_env = serde_json::Map::new();
        for plugin in plugins.iter().filter(|p| p.enabled) {
            for (k, v) in &plugin.shell_env {
                merged_env.insert(k.clone(), Value::String(v.clone()));
            }
        }
        if !merged_env.is_empty() {
            args["env"] = Value::Object(merged_env);
        }
        args
    }

    pub async fn transform_tool_output(&self, output: String) -> String {
        let plugins = self.plugins.read().await;
        let mut transformed = output;
        for plugin in plugins.iter().filter(|p| p.enabled) {
            if let Some(suffix) = &plugin.tool_output_suffix {
                transformed = format!("{transformed}\n{suffix}");
            }
        }
        transformed
    }
}

fn default_true() -> bool {
    true
}

async fn load_plugins(dir: PathBuf) -> anyhow::Result<Vec<PluginManifest>> {
    let mut out = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(err.into()),
    };

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|v| v.to_str()) else {
            continue;
        };
        if ext != "json" {
            continue;
        }
        let raw = fs::read_to_string(&path).await?;
        if let Ok(parsed) = serde_json::from_str::<PluginManifest>(&raw) {
            out.push(parsed);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
