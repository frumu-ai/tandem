// Tandem Application State
use crate::sidecar::{SidecarConfig, SidecarManager};
use crate::tool_proxy::{OperationJournal, StagingStore};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
    // Not saved to config, but populated at runtime
    #[serde(skip_deserializing)]
    #[serde(default)]
    pub has_key: bool,
}

/// All provider configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_openrouter")]
    pub openrouter: ProviderConfig,
    #[serde(default = "default_opencode_zen")]
    pub opencode_zen: ProviderConfig,
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
        enabled: false,
        default: false,
        endpoint: "https://openrouter.ai/api/v1".to_string(),
        model: Some("xiaomi/mimo-v2-flash:free".to_string()),
        has_key: false,
    }
}

fn default_opencode_zen() -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        default: true,
        endpoint: "https://opencode.ai/zen/v1".to_string(),
        model: Some("minimax-m2.1-free".to_string()),
        has_key: false,
    }
}

fn default_anthropic() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "https://api.anthropic.com".to_string(),
        model: None,
        has_key: false,
    }
}

fn default_openai() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "https://api.openai.com/v1".to_string(),
        model: None,
        has_key: false,
    }
}

fn default_ollama() -> ProviderConfig {
    ProviderConfig {
        enabled: false,
        default: false,
        endpoint: "http://localhost:11434".to_string(),
        model: Some("llama3.2".to_string()),
        has_key: false,
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            openrouter: default_openrouter(),
            opencode_zen: default_opencode_zen(),
            anthropic: default_anthropic(),
            openai: default_openai(),
            ollama: default_ollama(),
            custom: Vec::new(),
        }
    }
}

/// User-managed project (workspace folder)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProject {
    pub id: String,
    pub name: String,
    pub path: String, // Store as string for serialization
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
}

impl UserProject {
    pub fn new(path: PathBuf, name: Option<String>) -> Self {
        let default_name = if let Some(provided_name) = name.as_ref() {
            provided_name.clone()
        } else {
            // Try to get the last component of the path
            path.file_name()
                .and_then(|n| n.to_str())
                .or_else(|| {
                    // If that fails, try getting the last non-empty component
                    path.components()
                        .filter_map(|c| {
                            if let std::path::Component::Normal(name) = c {
                                name.to_str()
                            } else {
                                None
                            }
                        })
                        .next_back()
                })
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Unnamed Project".to_string())
        };

        let now = chrono::Utc::now();

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: default_name,
            path: path.to_string_lossy().to_string(),
            created_at: now,
            last_accessed: now,
        }
    }

    pub fn path_buf(&self) -> PathBuf {
        PathBuf::from(&self.path)
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
    /// Currently selected workspace path (legacy, kept for backwards compat)
    pub workspace_path: RwLock<Option<PathBuf>>,
    /// User-managed projects
    pub user_projects: RwLock<Vec<UserProject>>,
    /// Currently active project ID
    pub active_project_id: RwLock<Option<String>>,
    /// Paths that are allowed for access
    pub allowed_paths: RwLock<HashSet<PathBuf>>,
    /// Paths/patterns that are always denied
    pub denied_patterns: RwLock<Vec<String>>,
    /// Session-level permission approvals (reserved for future use)
    #[allow(dead_code)]
    pub session_approvals: RwLock<HashSet<String>>,
    /// Persistent permission rules (reserved for future use)
    #[allow(dead_code)]
    pub permission_rules: RwLock<Vec<PermissionRule>>,
    /// Provider configuration
    pub providers_config: RwLock<ProvidersConfig>,
    /// Sidecar manager for OpenCode
    pub sidecar: Arc<SidecarManager>,
    /// Current chat session ID
    pub current_session_id: RwLock<Option<String>>,
    /// Operation journal for file undo
    pub operation_journal: Arc<OperationJournal>,
    /// Staging store for execution planning
    pub staging_store: Arc<StagingStore>,
}

impl AppState {
    pub fn new() -> Self {
        let denied_patterns = vec![
            "**/.env".to_string(),
            "**/.env.*".to_string(),
            "**/*.pem".to_string(),
            "**/*.key".to_string(),
            "**/.ssh/*".to_string(),
            "**/.gnupg/*".to_string(),
            "**/secrets/*".to_string(),
            "**/*.stronghold".to_string(),
        ];

        Self {
            workspace_path: RwLock::new(None),
            user_projects: RwLock::new(Vec::new()),
            active_project_id: RwLock::new(None),
            allowed_paths: RwLock::new(HashSet::new()),
            denied_patterns: RwLock::new(denied_patterns),
            session_approvals: RwLock::new(HashSet::new()),
            permission_rules: RwLock::new(Vec::new()),
            providers_config: RwLock::new(ProvidersConfig::default()),
            sidecar: Arc::new(SidecarManager::new(SidecarConfig::default())),
            current_session_id: RwLock::new(None),
            operation_journal: Arc::new(OperationJournal::new(100)),
            staging_store: Arc::new(StagingStore::new()),
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
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        let allowed = self.allowed_paths.read().unwrap();

        // Check if the path is within any allowed path
        for allowed_path in allowed.iter() {
            if path.starts_with(allowed_path) {
                // Also check against denied patterns
                let denied = self.denied_patterns.read().unwrap();
                let path_str = path.to_string_lossy().replace('\\', "/");

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
    pub user_projects: Vec<UserProject>,
    pub active_project_id: Option<String>,
    pub providers_config: ProvidersConfig,
}

impl From<&AppState> for AppStateInfo {
    fn from(state: &AppState) -> Self {
        let workspace = state.workspace_path.read().unwrap();
        let providers = state.providers_config.read().unwrap();
        let user_projects = state.user_projects.read().unwrap();
        let active_project_id = state.active_project_id.read().unwrap();

        Self {
            workspace_path: workspace.as_ref().map(|p| p.to_string_lossy().to_string()),
            has_workspace: workspace.is_some(),
            user_projects: user_projects.clone(),
            active_project_id: active_project_id.clone(),
            providers_config: providers.clone(),
        }
    }
}
