// Tandem Application State
use crate::sidecar::{SidecarConfig, SidecarManager};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Provider configuration for LLM routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub enabled: bool,
    #[serde(default)]
    pub default: bool,
    pub endpoint: String,
    #[serde(default)]
    pub model: Option<String>,
}

/// All provider configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_openrouter")]
    pub openrouter: ProviderConfig,
    #[serde(default = "default_anthropic")]
    pub anthropic: ProviderConfig,
    #[serde(default = "default_openai")]
    pub openai: ProviderConfig,
    #[serde(default = "default_ollama")]
    pub ollama: ProviderConfig,
    #[serde(default)]
    pub custom: Vec<ProviderConfig>,
}

fn default_openrouter() -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        default: true,
        endpoint: "https://openrouter.ai/api/v1".to_string(),
        model: Some("xiaomi/mimo-v2-flash:free".to_string()),
    }
}

fn default_anthropic() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "https://api.anthropic.com".to_string(),
        model: None,
    }
}

fn default_openai() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "https://api.openai.com/v1".to_string(),
        model: None,
    }
}

fn default_ollama() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "http://localhost:11434".to_string(),
        model: Some("llama3.2".to_string()),
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            openrouter: default_openrouter(),
            anthropic: default_anthropic(),
            openai: default_openai(),
            ollama: default_ollama(),
            custom: Vec::new(),
        }
    }
}

/// Permission rule for file/folder access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub pattern: String,
    pub permission_type: PermissionType,
    pub decision: PermissionDecision,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionType {
    Read,
    Write,
    Delete,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionDecision {
    Allow,
    AllowOnce,
    AllowForSession,
    AllowAlways,
    Deny,
    DenyAlways,
}

/// Main application state managed by Tauri
pub struct AppState {
    /// Currently selected workspace path
    pub workspace_path: RwLock<Option<PathBuf>>,
    /// Paths that are allowed for access
    pub allowed_paths: RwLock<HashSet<PathBuf>>,
    /// Paths/patterns that are always denied
    pub denied_patterns: RwLock<Vec<String>>,
    /// Session-level permission approvals
    pub session_approvals: RwLock<HashSet<String>>,
    /// Persistent permission rules
    pub permission_rules: RwLock<Vec<PermissionRule>>,
    /// Provider configuration
    pub providers_config: RwLock<ProvidersConfig>,
    /// Sidecar manager for OpenCode
    pub sidecar: Arc<SidecarManager>,
    /// Current chat session ID
    pub current_session_id: RwLock<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        let mut denied_patterns = Vec::new();
        // Default denied patterns for security
        denied_patterns.push("**/.env".to_string());
        denied_patterns.push("**/.env.*".to_string());
        denied_patterns.push("**/*.pem".to_string());
        denied_patterns.push("**/*.key".to_string());
        denied_patterns.push("**/.ssh/*".to_string());
        denied_patterns.push("**/.gnupg/*".to_string());
        denied_patterns.push("**/secrets/*".to_string());
        denied_patterns.push("**/*.stronghold".to_string());

        Self {
            workspace_path: RwLock::new(None),
            allowed_paths: RwLock::new(HashSet::new()),
            denied_patterns: RwLock::new(denied_patterns),
            session_approvals: RwLock::new(HashSet::new()),
            permission_rules: RwLock::new(Vec::new()),
            providers_config: RwLock::new(ProvidersConfig::default()),
            sidecar: Arc::new(SidecarManager::new(SidecarConfig::default())),
            current_session_id: RwLock::new(None),
        }
    }

    /// Set the workspace path and add it to allowed paths
    pub fn set_workspace(&self, path: PathBuf) {
        {
            let mut workspace = self.workspace_path.write().unwrap();
            *workspace = Some(path.clone());
        }
        {
            let mut allowed = self.allowed_paths.write().unwrap();
            allowed.insert(path);
        }
    }

    /// Check if a path is within the allowed workspace
    pub fn is_path_allowed(&self, path: &PathBuf) -> bool {
        let allowed = self.allowed_paths.read().unwrap();

        // Check if the path is within any allowed path
        for allowed_path in allowed.iter() {
            if path.starts_with(allowed_path) {
                // Also check against denied patterns
                let denied = self.denied_patterns.read().unwrap();
                let path_str = path.to_string_lossy();

                for pattern in denied.iter() {
                    // Simple glob matching (could use glob crate for more complex patterns)
                    if pattern.contains("**") {
                        let pattern_suffix = pattern.trim_start_matches("**/");
                        if path_str.ends_with(pattern_suffix)
                            || path_str.contains(&format!("/{}", pattern_suffix))
                        {
                            return false;
                        }
                    } else if path_str.contains(pattern) {
                        return false;
                    }
                }

                return true;
            }
        }

        false
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable state info for frontend
#[derive(Debug, Serialize)]
pub struct AppStateInfo {
    pub workspace_path: Option<String>,
    pub has_workspace: bool,
    pub providers_config: ProvidersConfig,
}

impl From<&AppState> for AppStateInfo {
    fn from(state: &AppState) -> Self {
        let workspace = state.workspace_path.read().unwrap();
        let providers = state.providers_config.read().unwrap();

        Self {
            workspace_path: workspace.as_ref().map(|p| p.to_string_lossy().to_string()),
            has_workspace: workspace.is_some(),
            providers_config: providers.clone(),
        }
    }
}
