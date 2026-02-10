// Tandem Tauri Commands
// These are the IPC commands exposed to the frontend

use crate::error::{Result, TandemError};
use crate::keystore::{validate_api_key, validate_key_type, ApiKeyType, SecureKeyStore};
use crate::logs::{self, LogFileInfo};
use crate::memory::indexer::{index_workspace, IndexingStats};
use crate::memory::types::{ClearFileIndexResult, MemoryStats, ProjectMemoryStats};
use crate::orchestrator::{
    engine::OrchestratorEngine,
    policy::{PolicyConfig, PolicyEngine},
    store::OrchestratorStore,
    types::{Budget, OrchestratorConfig, Run, RunSnapshot, RunStatus, RunSummary, Task, TaskState},
};
use crate::python_env;
use crate::sidecar::{
    CreateSessionRequest, FilePartInput, ModelInfo, ModelSpec, Project, ProviderInfo,
    SendMessageRequest, Session, SessionMessage, SidecarState, StreamEvent, TodoItem,
};
use crate::sidecar_manager::{self, SidecarStatus};
use crate::state::{AppState, AppStateInfo, ProvidersConfig};
use crate::tool_proxy::{FileSnapshot, JournalEntry, OperationStatus, UndoAction};
use crate::vault::{self, EncryptedVaultKey, VaultStatus};
use crate::VaultState;
use futures::StreamExt;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

// ============================================================================
// Packs (guided workflows)
// ============================================================================

#[tauri::command]
pub fn packs_list() -> Vec<crate::packs::PackMeta> {
    crate::packs::list_packs()
}

#[tauri::command]
pub fn packs_install(
    app: AppHandle,
    pack_id: String,
    destination_dir: String,
) -> Result<crate::packs::PackInstallResult> {
    crate::packs::install_pack(&app, &pack_id, &destination_dir).map_err(TandemError::InvalidConfig)
}

#[tauri::command]
pub fn packs_install_default(
    app: AppHandle,
    pack_id: String,
) -> Result<crate::packs::PackInstallResult> {
    crate::packs::install_pack_default(&app, &pack_id).map_err(TandemError::InvalidConfig)
}

// ============================================================================
// Updater helpers
// ============================================================================

/// Returns an updater target override when we can reliably detect packaging.
///
/// Why: On Linux, `@tauri-apps/plugin-updater` defaults to `linux-x86_64`, which
/// in our `latest.json` maps to the AppImage. If the app is installed via a
/// `.deb` (e.g. `/usr/bin/tandem`), the updater will try to treat that AppImage
/// as a deb and fail with "update is not a valid deb package".
#[tauri::command]
pub fn get_updater_target() -> Option<String> {
    // Only override on Linux; other platforms can rely on defaults.
    #[cfg(not(target_os = "linux"))]
    {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        // AppImage runs set APPIMAGE; prefer explicit appimage target.
        if std::env::var_os("APPIMAGE").is_some() {
            let target = match std::env::consts::ARCH {
                "x86_64" => "linux-x86_64-appimage",
                "aarch64" => "linux-aarch64-appimage",
                _ => return None,
            };
            return Some(target.to_string());
        }

        // Detect deb-installed binary path.
        if let Ok(exe) = std::env::current_exe() {
            if exe == std::path::Path::new("/usr/bin/tandem") {
                let target = match std::env::consts::ARCH {
                    "x86_64" => "linux-x86_64-deb",
                    "aarch64" => "linux-aarch64-deb",
                    _ => return None,
                };
                return Some(target.to_string());
            }
        }

        None
    }
}

// ============================================================================
// Vault Commands (PIN-based encryption)
// ============================================================================

/// Get the current vault status
#[tauri::command]
pub fn get_vault_status(vault_state: State<'_, VaultState>) -> VaultStatus {
    vault_state.get_status()
}

/// Create a new vault with a PIN
#[tauri::command]
pub async fn create_vault(
    app: AppHandle,
    vault_state: State<'_, VaultState>,
    pin: String,
) -> Result<()> {
    // Validate PIN
    vault::validate_pin(&pin)?;

    // Check if vault already exists
    if vault::vault_exists(&vault_state.app_data_dir) {
        return Err(TandemError::Vault("Vault already exists".to_string()));
    }

    // Delete any existing legacy Stronghold snapshot (from previous installations)
    let stronghold_path = vault_state.app_data_dir.join("tandem.stronghold");
    if stronghold_path.exists() {
        tracing::warn!("Deleting old Stronghold snapshot: {:?}", stronghold_path);
        std::fs::remove_file(&stronghold_path).ok();
    }

    // Create encrypted vault key
    let (encrypted_key, master_key) = EncryptedVaultKey::create(&pin)?;

    // Save to file
    let vault_key_path = vault::get_vault_key_path(&vault_state.app_data_dir);
    encrypted_key.save(&vault_key_path)?;

    tracing::info!("Created new vault at {:?}", vault_key_path);

    // Store master key and mark as unlocked
    vault_state.set_master_key(master_key.clone());

    // Initialize keystore in background thread (it's CPU-intensive)
    let app_clone = app.clone();
    let master_key_clone = master_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::init_keystore_and_keys(&app_clone, &master_key_clone);
        tracing::info!("Keystore initialization complete");
    });

    Ok(())
}

// ============================================================================
// Memory Management
// ============================================================================

/// Get statistics about the vector database memory usage
#[tauri::command]
pub async fn get_memory_stats(state: State<'_, AppState>) -> Result<MemoryStats> {
    if let Some(manager) = &state.memory_manager {
        manager
            .get_stats()
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemorySettings {
    pub auto_index_on_project_load: bool,
}

#[tauri::command]
pub fn get_memory_settings(app: AppHandle) -> MemorySettings {
    let mut settings = MemorySettings {
        auto_index_on_project_load: false,
    };

    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("memory_auto_index_on_project_load") {
            if let Some(b) = value.as_bool() {
                settings.auto_index_on_project_load = b;
            }
        }
    }

    settings
}

#[tauri::command]
pub fn set_memory_settings(app: AppHandle, settings: MemorySettings) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "memory_auto_index_on_project_load",
            serde_json::json!(settings.auto_index_on_project_load),
        );
        let _ = store.save();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_project_memory_stats(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProjectMemoryStats> {
    if let Some(manager) = &state.memory_manager {
        manager
            .db()
            .get_project_stats(&project_id)
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

#[tauri::command]
pub async fn clear_project_file_index(
    state: State<'_, AppState>,
    project_id: String,
    vacuum: bool,
) -> Result<ClearFileIndexResult> {
    if let Some(manager) = &state.memory_manager {
        manager
            .db()
            .clear_project_file_index(&project_id, vacuum)
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

/// Index the current workspace
#[tauri::command]
pub async fn index_workspace_command(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<IndexingStats> {
    if let Some(manager) = &state.memory_manager {
        let workspace_path = state
            .get_workspace_path()
            .ok_or_else(|| TandemError::IoError("No workspace selected".to_string()))?;
        index_workspace(&app, &workspace_path, &project_id, manager).await
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

/// Unlock an existing vault with a PIN
#[tauri::command]
pub async fn unlock_vault(
    app: AppHandle,
    vault_state: State<'_, VaultState>,
    pin: String,
) -> Result<()> {
    // Check if vault exists
    if !vault::vault_exists(&vault_state.app_data_dir) {
        return Err(TandemError::Vault(
            "No vault exists. Create one first.".to_string(),
        ));
    }

    // Check if already unlocked
    if vault_state.is_unlocked() {
        return Ok(());
    }

    // Load encrypted key
    let vault_key_path = vault::get_vault_key_path(&vault_state.app_data_dir);
    let encrypted_key = EncryptedVaultKey::load(&vault_key_path)?;

    // Decrypt master key (this validates the PIN)
    let master_key = encrypted_key.decrypt(&pin)?;

    tracing::info!("Vault unlocked successfully");

    // Store master key and mark as unlocked
    vault_state.set_master_key(master_key.clone());

    // Initialize keystore in background thread (it's CPU-intensive)
    let app_clone = app.clone();
    let master_key_clone = master_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::init_keystore_and_keys(&app_clone, &master_key_clone);
        tracing::info!("Keystore initialization complete");
    });

    Ok(())
}

/// Lock the vault (clears master key from memory)
#[tauri::command]
pub fn lock_vault(vault_state: State<'_, VaultState>) -> Result<()> {
    vault_state.lock();
    tracing::info!("Vault locked");
    Ok(())
}

fn resolve_default_model_spec(config: &ProvidersConfig) -> Option<ModelSpec> {
    // If the user explicitly selected a model/provider (including custom provider IDs),
    // prefer that over the fixed provider slots.
    if let Some(sel) = &config.selected_model {
        let provider_id = if sel.provider_id == "opencode_zen" {
            // Back-compat: frontend uses "opencode_zen", sidecar expects "opencode".
            "opencode".to_string()
        } else {
            sel.provider_id.clone()
        };

        if !provider_id.trim().is_empty() && !sel.model_id.trim().is_empty() {
            return Some(ModelSpec {
                provider_id,
                model_id: sel.model_id.clone(),
            });
        }
    }

    let candidates: Vec<(&str, &crate::state::ProviderConfig)> = vec![
        ("openrouter", &config.openrouter),
        ("opencode", &config.opencode_zen), // OpenCode expects "opencode" not "opencode_zen"
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("ollama", &config.ollama),
        ("poe", &config.poe),
    ];

    // Prefer explicit default provider
    if let Some((provider_id, provider)) = candidates
        .iter()
        .find(|(_, p)| p.enabled && p.default)
        .map(|(id, p)| (*id, *p))
    {
        if let Some(model_id) = provider.model.clone() {
            return Some(ModelSpec {
                provider_id: provider_id.to_string(),
                model_id,
            });
        }
    }

    // Fallback to first enabled provider with a model
    for (provider_id, provider) in candidates {
        if provider.enabled {
            if let Some(model_id) = provider.model.clone() {
                return Some(ModelSpec {
                    provider_id: provider_id.to_string(),
                    model_id,
                });
            }
        }
    }

    None
}

fn resolve_default_provider_and_model(
    config: &ProvidersConfig,
) -> (Option<String>, Option<String>) {
    if let Some(sel) = &config.selected_model {
        let provider_id = if sel.provider_id == "opencode_zen" {
            "opencode".to_string()
        } else {
            sel.provider_id.clone()
        };

        if !provider_id.trim().is_empty() && !sel.model_id.trim().is_empty() {
            return (Some(provider_id), Some(sel.model_id.clone()));
        }
    }

    let candidates: Vec<(&str, &crate::state::ProviderConfig)> = vec![
        ("openrouter", &config.openrouter),
        ("opencode", &config.opencode_zen), // OpenCode expects "opencode" not "opencode_zen"
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("ollama", &config.ollama),
        ("poe", &config.poe),
    ];

    if let Some((provider_id, provider)) = candidates
        .iter()
        .find(|(_, p)| p.enabled && p.default)
        .map(|(id, p)| (*id, *p))
    {
        return (Some(provider_id.to_string()), provider.model.clone());
    }

    for (provider_id, provider) in candidates {
        if provider.enabled {
            return (Some(provider_id.to_string()), provider.model.clone());
        }
    }

    (None, None)
}

fn env_var_for_key(key_type: &ApiKeyType) -> Option<&'static str> {
    match key_type {
        ApiKeyType::OpenRouter => Some("OPENROUTER_API_KEY"),
        ApiKeyType::OpenCodeZen => Some("OPENCODE_ZEN_API_KEY"),
        ApiKeyType::Anthropic => Some("ANTHROPIC_API_KEY"),
        ApiKeyType::OpenAI => Some("OPENAI_API_KEY"),
        ApiKeyType::Poe => Some("POE_API_KEY"),
        ApiKeyType::Custom(_) => None,
    }
}

/// Check if a file operation should be auto-approved based on path
/// Auto-approve writes to .opencode/plans/ for real-time plan updates
fn is_plan_file_operation(path: &str, tool: &str) -> bool {
    // Only auto-approve write operations
    if tool != "write" && tool != "write_file" {
        return false;
    }

    // Normalize path separators for Windows/Unix compatibility
    let normalized_path = path.replace('\\', "/");

    // Check if the path is within .opencode/plans/
    normalized_path.contains("/.opencode/plans/") || normalized_path.starts_with(".opencode/plans/")
}

// ============================================================================
// Basic Commands
// ============================================================================

/// Simple greeting command for testing
#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Tandem.", name)
}

/// Log frontend errors to backend log file
#[tauri::command]
pub fn log_frontend_error(message: String, details: Option<String>) {
    if let Some(d) = details {
        tracing::error!("[FRONTEND ERROR] {}: {}", message, d);
    } else {
        tracing::error!("[FRONTEND ERROR] {}", message);
    }
}

/// Get the current application state
/// Get the current application state (with key status)
#[tauri::command]
pub async fn get_app_state(app: AppHandle, state: State<'_, AppState>) -> Result<AppStateInfo> {
    let mut info = AppStateInfo::from(state.inner());

    // Dynamically populate has_key status
    populate_provider_keys(&app, &mut info.providers_config);

    Ok(info)
}

/// Set the workspace path
#[tauri::command]
pub fn set_workspace_path(app: AppHandle, path: String, state: State<'_, AppState>) -> Result<()> {
    let path_buf = PathBuf::from(&path);

    // Verify the path exists and is a directory
    if !path_buf.exists() {
        return Err(TandemError::NotFound(format!(
            "Path does not exist: {}",
            path
        )));
    }

    if !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    state.set_workspace(path_buf);
    tracing::info!("Workspace set to: {}", path);

    // Save to store for persistence
    if let Ok(store) = app.store("settings.json") {
        store.set("workspace_path", serde_json::json!(path));
        let _ = store.save();
    }

    Ok(())
}

/// Get the current workspace path
#[tauri::command]
pub fn get_workspace_path(state: State<'_, AppState>) -> Option<String> {
    let workspace = state.workspace_path.read().unwrap();
    workspace.as_ref().map(|p| p.to_string_lossy().to_string())
}

// ============================================================================
// Project Management (Multi-Workspace Support)
// ============================================================================

/// Check if a directory is a Git repository
#[tauri::command]
pub fn is_git_repo(path: String) -> bool {
    let git_dir = PathBuf::from(&path).join(".git");
    git_dir.exists() && git_dir.is_dir()
}

/// Check if Git is installed on the system
#[tauri::command]
pub fn is_git_installed() -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("--version");

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.output().is_ok()
}

/// Initialize a Git repository in the specified directory
#[tauri::command]
pub fn initialize_git_repo(path: String) -> Result<()> {
    let path_buf = PathBuf::from(&path);

    // Verify the path exists and is a directory
    if !path_buf.exists() || !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Invalid directory: {}",
            path
        )));
    }

    // Check if already a git repo
    if is_git_repo(path.clone()) {
        return Ok(()); // Already initialized, no-op
    }

    // Run git init
    let mut cmd = std::process::Command::new("git");
    cmd.arg("init").current_dir(&path_buf);

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to run git init: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TandemError::Sidecar(format!("git init failed: {}", stderr)));
    }

    tracing::info!("Initialized Git repository at: {}", path);
    Ok(())
}

/// Get comprehensive Git status for a directory
#[derive(serde::Serialize)]
pub struct GitStatus {
    pub git_installed: bool,
    pub is_repo: bool,
    pub can_enable_undo: bool,
}

#[tauri::command]
pub fn check_git_status(path: String) -> GitStatus {
    let git_installed = is_git_installed();
    let is_repo = is_git_repo(path);
    let can_enable_undo = git_installed && !is_repo;

    GitStatus {
        git_installed,
        is_repo,
        can_enable_undo,
    }
}

/// Add a new project folder
#[tauri::command]
pub fn add_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<crate::state::UserProject> {
    use crate::state::UserProject;

    let path_buf = PathBuf::from(&path);

    // Verify the path exists and is a directory
    if !path_buf.exists() {
        return Err(TandemError::NotFound(format!(
            "Path does not exist: {}",
            path
        )));
    }

    if !path_buf.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    // Create new project
    let project = UserProject::new(path_buf, name);

    // Add to state
    {
        let mut projects = state.user_projects.write().unwrap();
        projects.push(project.clone());
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());
        let _ = store.save();
    }

    tracing::info!("Added project: {} at {}", project.name, project.path);

    Ok(project)
}

/// Remove a project
#[tauri::command]
pub fn remove_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<()> {
    // Remove from state
    {
        let mut projects = state.user_projects.write().unwrap();
        projects.retain(|p| p.id != project_id);
    }

    // If this was the active project, clear it
    {
        let active_id = state.active_project_id.read().unwrap();
        if active_id.as_ref() == Some(&project_id) {
            drop(active_id);
            let mut active = state.active_project_id.write().unwrap();
            *active = None;

            // Also clear workspace path
            let mut workspace = state.workspace_path.write().unwrap();
            *workspace = None;
        }
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());

        let active_id = state.active_project_id.read().unwrap();
        if active_id.is_none() {
            let _ = store.delete("active_project_id");
        }

        let _ = store.save();
    }

    tracing::info!("Removed project: {}", project_id);

    Ok(())
}

/// Get all user projects
#[tauri::command]
pub fn get_user_projects(state: State<'_, AppState>) -> Vec<crate::state::UserProject> {
    let projects = state.user_projects.read().unwrap();
    projects.clone()
}

/// Set the active project (and update workspace)
#[tauri::command]
pub async fn set_active_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<()> {
    use crate::state::UserProject;

    // Find the project
    let project: UserProject = {
        let projects = state.user_projects.read().unwrap();
        projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Project not found: {}", project_id)))?
    };

    // Set as active
    {
        let mut active = state.active_project_id.write().unwrap();
        *active = Some(project_id.clone());
    }

    // Update last accessed time
    {
        let mut projects = state.user_projects.write().unwrap();
        if let Some(p) = projects.iter_mut().find(|p| p.id == project_id) {
            p.last_accessed = chrono::Utc::now();
        }
    }

    // Set workspace path
    let path_buf = project.path_buf();
    state.set_workspace(path_buf.clone());

    // Update sidecar workspace - this sets it for when sidecar restarts
    state.sidecar.set_workspace(path_buf.clone()).await;

    // Restart the sidecar if it's running so it picks up the new workspace
    let sidecar_state = state.sidecar.state().await;
    if sidecar_state == crate::sidecar::SidecarState::Running {
        tracing::info!("Restarting sidecar with new workspace: {}", project.path);

        // Stop the sidecar
        let _ = state.sidecar.stop().await;

        // Wait a moment for cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Get the sidecar path (checks AppData first, then resources)
        let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;

        // Sync env vars BEFORE starting so the sidecar actually picks them up.
        let providers = {
            let config = state.providers_config.read().unwrap();
            config.clone()
        };
        sync_ollama_env(&state, &providers).await;
        sync_provider_keys_env(&app, &state, &providers).await;

        // Restart with new workspace
        state
            .sidecar
            .start(sidecar_path.to_string_lossy().as_ref())
            .await?;

        tracing::info!("Sidecar restarted successfully");
    }

    // Save to store
    if let Ok(store) = app.store("settings.json") {
        store.set("active_project_id", serde_json::json!(project_id));
        let projects = state.user_projects.read().unwrap();
        store.set("user_projects", serde_json::to_value(&*projects).unwrap());
        let _ = store.save();
    }

    tracing::info!("Set active project: {} ({})", project.name, project.path);

    Ok(())
}

/// Get the active project
#[tauri::command]
pub fn get_active_project(state: State<'_, AppState>) -> Option<crate::state::UserProject> {
    let active_id = state.active_project_id.read().unwrap();
    let project_id = active_id.as_ref()?;

    let projects = state.user_projects.read().unwrap();
    projects.iter().find(|p| &p.id == project_id).cloned()
}

// ============================================================================
// API Key Management
// ============================================================================

/// Store an API key in the stronghold vault
#[tauri::command]
pub async fn store_api_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key_type: String,
    api_key: String,
) -> Result<()> {
    // Validate inputs
    let key_type_enum = validate_key_type(&key_type)?;
    validate_api_key(&api_key)?;

    let key_name = key_type_enum.to_key_name();
    let api_key_value = api_key.clone();
    let _key_type_for_log = key_type.clone();

    // Clone app handle so we can move it into spawn_blocking
    let app_clone = app.clone();

    // Insert the key in memory first (fast)
    let keystore = app_clone
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;

    keystore.set(&key_name, &api_key_value)?;

    // Update environment variable immediately
    if let Some(env_key) = env_var_for_key(&key_type_enum) {
        // Never log secrets (even masked) to avoid accidental disclosure.
        tracing::info!("Setting environment variable {}", env_key);
        state.sidecar.set_env(env_key, &api_key).await;
    }

    tracing::info!("API key saved");

    {
        let mut providers = state.providers_config.write().unwrap();
        populate_provider_keys(&app, &mut providers);
    }

    // Restart sidecar if it's running to reload env vars
    if matches!(state.sidecar.state().await, SidecarState::Running) {
        let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
        state
            .sidecar
            .restart(sidecar_path.to_string_lossy().as_ref())
            .await?;
    }

    Ok(())
}

/// Check if an API key exists for a provider
#[tauri::command]
pub async fn has_api_key(app: tauri::AppHandle, key_type: String) -> Result<bool> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = match app.try_state::<SecureKeyStore>() {
        Some(ks) => ks,
        None => return Ok(false),
    };

    Ok(keystore.has(&key_name))
}

/// Delete an API key from the vault
#[tauri::command]
pub async fn delete_api_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key_type: String,
) -> Result<()> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = app
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;

    keystore.delete(&key_name)?;

    if let Some(env_key) = env_var_for_key(&key_type_enum) {
        state.sidecar.remove_env(env_key).await;
        {
            let mut providers = state.providers_config.write().unwrap();
            populate_provider_keys(&app, &mut providers);
        }
        if matches!(state.sidecar.state().await, SidecarState::Running) {
            let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
            state
                .sidecar
                .restart(sidecar_path.to_string_lossy().as_ref())
                .await?;
        }
    }

    tracing::info!("API key deleted for provider: {}", key_type);
    Ok(())
}

/// Get an API key from the vault (internal use only)
async fn get_api_key(app: &AppHandle, key_type: &str) -> Result<Option<String>> {
    let key_type_enum = validate_key_type(key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = match app.try_state::<SecureKeyStore>() {
        Some(ks) => ks,
        None => return Ok(None),
    };

    keystore.get(&key_name)
}

// ============================================================================
// Theme / Appearance
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomBackgroundFit {
    Cover,
    Contain,
    Tile,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomBackgroundSettings {
    pub enabled: bool,
    pub file_name: Option<String>,
    pub fit: CustomBackgroundFit,
    /// 0.0 - 1.0
    pub opacity: f32,
}

impl Default for CustomBackgroundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file_name: None,
            fit: CustomBackgroundFit::Cover,
            opacity: 0.30,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomBackgroundInfo {
    pub settings: CustomBackgroundSettings,
    pub file_path: Option<String>,
}

const CUSTOM_BG_STORE_KEY: &str = "custom_background";
const CUSTOM_BG_DIR_NAME: &str = "backgrounds";
const CUSTOM_BG_FILE_STEM: &str = "custom-background";
const CUSTOM_BG_MAX_BYTES: u64 = 20 * 1024 * 1024; // 20MB

fn custom_bg_dir(app: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| TandemError::IoError(format!("Failed to get app data dir: {}", e)))?;
    Ok(app_data_dir.join(CUSTOM_BG_DIR_NAME))
}

fn is_allowed_custom_bg_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

fn custom_bg_file_name_for_ext(ext: &str) -> String {
    format!("{}.{}", CUSTOM_BG_FILE_STEM, ext)
}

fn clear_existing_custom_bg_files(dir: &PathBuf) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with(&format!("{}.", CUSTOM_BG_FILE_STEM)) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

fn resolve_custom_bg_file_path(
    app: &AppHandle,
    file_name: &Option<String>,
) -> Result<Option<String>> {
    let Some(file_name) = file_name else {
        return Ok(None);
    };

    let dir = custom_bg_dir(app)?;
    let path = dir.join(file_name);
    if path.exists() {
        Ok(Some(path.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

fn read_custom_bg_settings(app: &AppHandle) -> CustomBackgroundSettings {
    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get(CUSTOM_BG_STORE_KEY) {
            if let Ok(settings) = serde_json::from_value::<CustomBackgroundSettings>(value.clone())
            {
                return settings;
            }
        }
    }

    CustomBackgroundSettings::default()
}

fn write_custom_bg_settings(app: &AppHandle, settings: &CustomBackgroundSettings) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set(CUSTOM_BG_STORE_KEY, serde_json::to_value(settings)?);
        let _ = store.save();
    }
    Ok(())
}

/// Get the user's selected theme id
#[tauri::command]
pub fn get_user_theme(app: AppHandle) -> Result<String> {
    // Default to the new design-system theme
    let default_theme = "charcoal_fire".to_string();

    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("user_theme") {
            if let Some(theme_id) = value.as_str() {
                return Ok(theme_id.to_string());
            }
        }
    }

    Ok(default_theme)
}

/// Persist the user's selected theme id
#[tauri::command]
pub fn set_user_theme(app: AppHandle, theme_id: String) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set("user_theme", serde_json::json!(theme_id));
        let _ = store.save();
    }
    Ok(())
}

/// Get the user's custom background configuration (and resolved path to the stored image, if any)
#[tauri::command]
pub fn get_custom_background(app: AppHandle) -> Result<CustomBackgroundInfo> {
    let mut settings = read_custom_bg_settings(&app);
    let file_path = resolve_custom_bg_file_path(&app, &settings.file_name)?;

    // Heal invalid state: if the file is missing, disable the feature.
    if settings.enabled && settings.file_name.is_some() && file_path.is_none() {
        settings.enabled = false;
        settings.file_name = None;
        let _ = write_custom_bg_settings(&app, &settings);
    }

    Ok(CustomBackgroundInfo {
        settings,
        file_path,
    })
}

/// Set the custom background image by copying from an existing path into AppData.
#[tauri::command]
pub fn set_custom_background_image(
    app: AppHandle,
    source_path: String,
) -> Result<CustomBackgroundInfo> {
    let src = PathBuf::from(&source_path);
    if !src.exists() || !src.is_file() {
        return Err(TandemError::NotFound(format!(
            "Image not found: {}",
            source_path
        )));
    }

    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| TandemError::ValidationError("File has no extension".to_string()))?;

    if !is_allowed_custom_bg_ext(&ext) {
        return Err(TandemError::ValidationError(format!(
            "Unsupported image type: .{} (allowed: png, jpg, jpeg, webp)",
            ext
        )));
    }

    let meta = fs::metadata(&src)?;
    if meta.len() > CUSTOM_BG_MAX_BYTES {
        return Err(TandemError::ValidationError(format!(
            "Image is too large (max 20MB): {} bytes",
            meta.len()
        )));
    }

    let dir = custom_bg_dir(&app)?;
    fs::create_dir_all(&dir)?;
    clear_existing_custom_bg_files(&dir)?;

    let file_name = custom_bg_file_name_for_ext(&ext);
    let dest = dir.join(&file_name);
    fs::copy(&src, &dest)?;

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = true;
    settings.file_name = Some(file_name);
    // Keep any existing fit/opacity preferences.
    write_custom_bg_settings(&app, &settings)?;

    Ok(CustomBackgroundInfo {
        settings,
        file_path: Some(dest.to_string_lossy().to_string()),
    })
}

/// Set the custom background image by writing bytes into AppData (used for drag/drop).
#[tauri::command]
pub fn set_custom_background_image_bytes(
    app: AppHandle,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<CustomBackgroundInfo> {
    if bytes.len() as u64 > CUSTOM_BG_MAX_BYTES {
        return Err(TandemError::ValidationError(format!(
            "Image is too large (max 20MB): {} bytes",
            bytes.len()
        )));
    }

    let ext = PathBuf::from(&file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| TandemError::ValidationError("File has no extension".to_string()))?;

    if !is_allowed_custom_bg_ext(&ext) {
        return Err(TandemError::ValidationError(format!(
            "Unsupported image type: .{} (allowed: png, jpg, jpeg, webp)",
            ext
        )));
    }

    let dir = custom_bg_dir(&app)?;
    fs::create_dir_all(&dir)?;
    clear_existing_custom_bg_files(&dir)?;

    let stored_file_name = custom_bg_file_name_for_ext(&ext);
    let dest = dir.join(&stored_file_name);
    fs::write(&dest, bytes)?;

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = true;
    settings.file_name = Some(stored_file_name);
    write_custom_bg_settings(&app, &settings)?;

    Ok(CustomBackgroundInfo {
        settings,
        file_path: Some(dest.to_string_lossy().to_string()),
    })
}

/// Update custom background settings (fit/opacity/enabled). Does not change the stored image.
#[tauri::command]
pub fn set_custom_background_settings(
    app: AppHandle,
    settings: CustomBackgroundSettings,
) -> Result<()> {
    if !(0.0..=1.0).contains(&settings.opacity) {
        return Err(TandemError::ValidationError(
            "Opacity must be between 0.0 and 1.0".to_string(),
        ));
    }

    write_custom_bg_settings(&app, &settings)?;
    Ok(())
}

/// Clear the stored custom background image and disable the feature.
#[tauri::command]
pub fn clear_custom_background_image(app: AppHandle) -> Result<()> {
    let dir = custom_bg_dir(&app)?;
    let _ = clear_existing_custom_bg_files(&dir);

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = false;
    settings.file_name = None;
    write_custom_bg_settings(&app, &settings)?;

    Ok(())
}

// ============================================================================
// Provider Configuration
// ============================================================================

/// Get the providers configuration
/// Get the providers configuration (with key status)
#[tauri::command]
pub async fn get_providers_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ProvidersConfig> {
    let mut config = state.providers_config.read().unwrap().clone();

    // Dynamically populate has_key status
    populate_provider_keys(&app, &mut config);

    Ok(config)
}

/// Helper to populate has_key status from keystore
// This function is local to commands but we need to ensure keys are populated on load too.
// Actually, `lib.rs` initializes keys into env vars via `init_keystore_and_keys`.
// `populate_provider_keys` here updates the *config object* in memory to say `has_key = true`.
// We need to make sure this happens on app startup after loading config.
pub fn populate_provider_keys(app: &AppHandle, config: &mut ProvidersConfig) {
    use crate::keystore::ApiKeyType;

    if let Some(keystore) = app.try_state::<SecureKeyStore>() {
        let openrouter_key = ApiKeyType::OpenRouter.to_key_name();
        let opencode_zen_key = ApiKeyType::OpenCodeZen.to_key_name();
        let anthropic_key = ApiKeyType::Anthropic.to_key_name();
        let openai_key = ApiKeyType::OpenAI.to_key_name();
        let poe_key = ApiKeyType::Poe.to_key_name();

        tracing::info!("[populate_provider_keys] Checking for keys:");
        tracing::info!(
            "  OpenRouter key '{}': {}",
            openrouter_key,
            keystore.has(&openrouter_key)
        );
        tracing::info!(
            "  OpenCodeZen key '{}': {}",
            opencode_zen_key,
            keystore.has(&opencode_zen_key)
        );
        tracing::info!(
            "  Anthropic key '{}': {}",
            anthropic_key,
            keystore.has(&anthropic_key)
        );
        tracing::info!(
            "  OpenAI key '{}': {}",
            openai_key,
            keystore.has(&openai_key)
        );
        tracing::info!("  Poe key '{}': {}", poe_key, keystore.has(&poe_key));

        config.openrouter.has_key = keystore.has(&openrouter_key);
        config.opencode_zen.has_key = keystore.has(&opencode_zen_key);
        config.anthropic.has_key = keystore.has(&anthropic_key);
        config.openai.has_key = keystore.has(&openai_key);
        config.poe.has_key = keystore.has(&poe_key);
        // For local models, we might consider them "having a key" or check connection
        config.ollama.has_key = true; // Local inference is always 'authed'
    } else {
        // Expected when the vault is locked; `get_app_state` calls this frequently.
        tracing::debug!("[populate_provider_keys] Keystore not available (vault locked?)");
        // Keystore not initialized (vault locked)
        config.openrouter.has_key = false;
        config.opencode_zen.has_key = false;
        config.anthropic.has_key = false;
        config.openai.has_key = false;
        config.poe.has_key = false;
        config.ollama.has_key = true; // Local is fine
    }
}

async fn sync_ollama_env(state: &AppState, config: &ProvidersConfig) {
    if config.ollama.enabled {
        let endpoint = config.ollama.endpoint.trim();
        if !endpoint.is_empty() {
            state.sidecar.set_env("OLLAMA_HOST", endpoint).await;
        }
    } else {
        state.sidecar.remove_env("OLLAMA_HOST").await;
    }
}

async fn sync_provider_keys_env(app: &AppHandle, state: &AppState, config: &ProvidersConfig) {
    // OPENROUTER
    if config.openrouter.enabled {
        if let Ok(Some(key)) = get_api_key(app, "openrouter").await {
            state.sidecar.set_env("OPENROUTER_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENROUTER_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENROUTER_API_KEY").await;
    }

    // OpenCode Zen
    if config.opencode_zen.enabled {
        if let Ok(Some(key)) = get_api_key(app, "opencode_zen").await {
            state.sidecar.set_env("OPENCODE_ZEN_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENCODE_ZEN_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENCODE_ZEN_API_KEY").await;
    }

    // Anthropic
    if config.anthropic.enabled {
        if let Ok(Some(key)) = get_api_key(app, "anthropic").await {
            state.sidecar.set_env("ANTHROPIC_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("ANTHROPIC_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("ANTHROPIC_API_KEY").await;
    }

    // OpenAI
    if config.openai.enabled {
        if let Ok(Some(key)) = get_api_key(app, "openai").await {
            state.sidecar.set_env("OPENAI_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENAI_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENAI_API_KEY").await;
    }

    // Poe
    if config.poe.enabled {
        if let Ok(Some(key)) = get_api_key(app, "poe").await {
            state.sidecar.set_env("POE_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("POE_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("POE_API_KEY").await;
    }
}

/// Set the providers configuration
#[tauri::command]
pub async fn set_providers_config(
    app: AppHandle,
    config: ProvidersConfig,
    state: State<'_, AppState>,
) -> Result<()> {
    let previous_config = {
        let providers = state.providers_config.read().unwrap();
        providers.clone()
    };

    {
        let mut providers = state.providers_config.write().unwrap();
        *providers = config.clone();
    }

    tracing::info!("Providers configuration updated");

    // Save to store for persistence
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "providers_config",
            serde_json::to_value(&config).unwrap_or_default(),
        );
        let _ = store.save();
    }

    let ollama_changed = previous_config.ollama.enabled != config.ollama.enabled
        || previous_config.ollama.endpoint != config.ollama.endpoint;

    let key_providers_changed = previous_config.openrouter.enabled != config.openrouter.enabled
        || previous_config.opencode_zen.enabled != config.opencode_zen.enabled
        || previous_config.anthropic.enabled != config.anthropic.enabled
        || previous_config.openai.enabled != config.openai.enabled
        || previous_config.poe.enabled != config.poe.enabled;

    if ollama_changed || key_providers_changed {
        sync_ollama_env(&state, &config).await;
        sync_provider_keys_env(&app, &state, &config).await;

        if matches!(state.sidecar.state().await, SidecarState::Running) {
            let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
            state
                .sidecar
                .restart(sidecar_path.to_string_lossy().as_ref())
                .await?;
        }
    }

    Ok(())
}

// ============================================================================
// Sidecar Management
// ============================================================================

/// Start the OpenCode sidecar
#[tauri::command]
pub async fn start_sidecar(app: AppHandle, state: State<'_, AppState>) -> Result<u16> {
    // Get the sidecar path (checks AppData first, then resources)
    let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;

    // Set workspace path on sidecar - clone before await
    let workspace_path = {
        let workspace = state.workspace_path.read().unwrap();
        workspace.clone()
    };
    if let Some(path) = workspace_path {
        state.sidecar.set_workspace(path).await;
    }

    // Get and set API keys as environment variables
    let providers = {
        let config = state.providers_config.read().unwrap();
        config.clone()
    };

    // Configure Ollama endpoint env (local models)
    sync_ollama_env(&state, &providers).await;

    // Set/remove API keys based on enabled providers.
    // (Important: remove_env only applies after restart, but we call this before start().)
    sync_provider_keys_env(&app, &state, &providers).await;

    // Start the sidecar
    state
        .sidecar
        .start(sidecar_path.to_string_lossy().as_ref())
        .await?;

    // Log provider availability for debugging
    match state.sidecar.list_providers().await {
        Ok(providers) => {
            let provider_list: Vec<String> = providers
                .iter()
                .map(|p| format!("{} ({})", p.id, p.name))
                .collect();
            tracing::debug!("Sidecar providers: {}", provider_list.join(", "));
        }
        Err(e) => {
            tracing::warn!("Failed to list sidecar providers: {}", e);
        }
    }

    match state.sidecar.list_models().await {
        Ok(models) => {
            let openrouter_count = models
                .iter()
                .filter(|m| m.provider.as_deref() == Some("openrouter"))
                .count();
            tracing::debug!(
                "Sidecar models: total={} openrouter={}",
                models.len(),
                openrouter_count
            );
        }
        Err(e) => {
            tracing::warn!("Failed to list sidecar models: {}", e);
        }
    }

    // Return the port
    state
        .sidecar
        .port()
        .await
        .ok_or_else(|| TandemError::Sidecar("Sidecar started but no port assigned".to_string()))
}

/// Stop the OpenCode sidecar
#[tauri::command]
pub async fn stop_sidecar(state: State<'_, AppState>) -> Result<()> {
    state.sidecar.stop().await
}

/// Get the sidecar status
#[tauri::command]
pub async fn get_sidecar_status(state: State<'_, AppState>) -> Result<SidecarState> {
    Ok(state.sidecar.state().await)
}

// ============================================================================
// File Tree Watcher (Files view auto-refresh)
// ============================================================================

#[tauri::command]
pub fn start_file_tree_watcher(
    app: AppHandle,
    state: State<'_, AppState>,
    root_path: String,
) -> Result<()> {
    let root = PathBuf::from(&root_path);
    if !state.is_path_allowed(&root) {
        return Err(TandemError::PermissionDenied(format!(
            "Watcher root is outside allowed workspace: {}",
            root_path
        )));
    }
    if !root.exists() || !root.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Watcher root is not a directory: {}",
            root_path
        )));
    }

    let watcher = crate::file_watcher::FileTreeWatcher::new(&root, app)
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to start file watcher: {}", e)))?;

    let mut guard = state
        .file_tree_watcher
        .lock()
        .map_err(|_| TandemError::InvalidOperation("Watcher lock poisoned".to_string()))?;
    *guard = Some(watcher);
    Ok(())
}

#[tauri::command]
pub fn stop_file_tree_watcher(state: State<'_, AppState>) -> Result<()> {
    let mut guard = state
        .file_tree_watcher
        .lock()
        .map_err(|_| TandemError::InvalidOperation("Watcher lock poisoned".to_string()))?;
    *guard = None;
    Ok(())
}

// ============================================================================
// Session Management
// ============================================================================

/// Create a new chat session
#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    title: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    allow_all_tools: Option<bool>,
) -> Result<Session> {
    let (default_provider, default_model) = {
        let config = state.providers_config.read().unwrap();
        resolve_default_provider_and_model(&config)
    };

    // IMPORTANT:
    // We intentionally do NOT send `permission="*"` allow to OpenCode.
    // Even when the UI toggle "Allow all tools" is enabled, the frontend auto-approves
    // permission prompts, and we still want the approve/deny hook to run so Tandem can
    // enforce safety policy (e.g. Python venv-only installs).
    //
    // Default safe tools allowed even if "Allow all" is off.
    let _allow_all = allow_all_tools.unwrap_or(false);
    let permission = Some(vec![
        crate::sidecar::PermissionRule {
            permission: "ls".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "read".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "todowrite".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "websearch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "webfetch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
    ]);

    let request = CreateSessionRequest {
        parent_id: None,
        title,
        model: model.or(default_model),
        provider: provider.or(default_provider),
        permission,
    };

    let session = state.sidecar.create_session(request).await?;

    // Store as current session
    {
        let mut current = state.current_session_id.write().unwrap();
        *current = Some(session.id.clone());
    }

    Ok(session)
}

/// Get a session by ID
#[tauri::command]
pub async fn get_session(state: State<'_, AppState>, session_id: String) -> Result<Session> {
    state.sidecar.get_session(&session_id).await
}

/// List all sessions
#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<Session>> {
    state.sidecar.list_sessions().await
}

/// Delete a session
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sidecar.delete_session(&session_id).await
}

/// List all projects
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>> {
    state.sidecar.list_projects().await
}

/// Get messages for a session
#[tauri::command]
pub async fn get_session_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionMessage>> {
    state.sidecar.get_session_messages(&session_id).await
}

/// Get todos for a session
#[tauri::command]
pub async fn get_session_todos(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<TodoItem>> {
    state.sidecar.get_session_todos(&session_id).await
}

/// Get the current session ID
#[tauri::command]
pub fn get_current_session_id(state: State<'_, AppState>) -> Option<String> {
    let current = state.current_session_id.read().unwrap();
    current.clone()
}

/// Set the current session ID
#[tauri::command]
pub fn set_current_session_id(state: State<'_, AppState>, session_id: Option<String>) {
    let mut current = state.current_session_id.write().unwrap();
    *current = session_id;
}

// ============================================================================
// Message Handling
// ============================================================================

/// File attachment from frontend
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FileAttachmentInput {
    pub mime: String,
    pub filename: Option<String>,
    pub url: String,
}

/// Send a message to a session (async, starts generation)
/// The actual response comes via the event stream
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
) -> Result<()> {
    let mut request = if let Some(files) = attachments {
        let file_parts: Vec<FilePartInput> = files
            .into_iter()
            .map(|f| FilePartInput {
                part_type: "file".to_string(),
                mime: f.mime,
                filename: f.filename,
                url: f.url,
            })
            .collect();
        SendMessageRequest::with_attachments(content, file_parts)
    } else {
        SendMessageRequest::text(content)
    };

    let model_spec = {
        let config = state.providers_config.read().unwrap();
        let resolved = resolve_default_model_spec(&config);
        if let Some(spec) = &resolved {
            tracing::debug!(
                "Resolved model spec: provider={} model={} (openrouter enabled={} default={} has_key={}, ollama enabled={} default={})",
                spec.provider_id,
                spec.model_id,
                config.openrouter.enabled,
                config.openrouter.default,
                config.openrouter.has_key,
                config.ollama.enabled,
                config.ollama.default
            );
        } else {
            tracing::debug!(
                "No model spec resolved (openrouter enabled={} default={} has_key={}, ollama enabled={} default={})",
                config.openrouter.enabled,
                config.openrouter.default,
                config.openrouter.has_key,
                config.ollama.enabled,
                config.ollama.default
            );
        }
        resolved
    };
    request.model = model_spec;

    state.sidecar.send_message(&session_id, request).await
}

/// Send a message and subscribe to events for the response
/// This emits events to the frontend as chunks arrive
#[tauri::command]
pub async fn send_message_streaming(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
    agent: Option<String>,
) -> Result<()> {
    // IMPORTANT: Subscribe to events BEFORE sending the message
    // This ensures we don't miss any events that OpenCode sends
    let stream = state.sidecar.subscribe_events().await?;

    // Now send the prompt
    let mut request = if let Some(files) = attachments {
        let file_parts: Vec<FilePartInput> = files
            .into_iter()
            .map(|f| FilePartInput {
                part_type: "file".to_string(),
                mime: f.mime,
                filename: f.filename,
                url: f.url,
            })
            .collect();
        SendMessageRequest::with_attachments(content, file_parts)
    } else {
        SendMessageRequest::text(content)
    };

    let model_spec = {
        let config = state.providers_config.read().unwrap();
        let resolved = resolve_default_model_spec(&config);
        if let Some(spec) = &resolved {
            tracing::debug!(
                "Resolved model spec (streaming): provider={} model={} (openrouter enabled={} default={} has_key={}, ollama enabled={} default={})",
                spec.provider_id,
                spec.model_id,
                config.openrouter.enabled,
                config.openrouter.default,
                config.openrouter.has_key,
                config.ollama.enabled,
                config.ollama.default
            );
        } else {
            tracing::debug!(
                "No model spec resolved for streaming (openrouter enabled={} default={} has_key={}, ollama enabled={} default={})",
                config.openrouter.enabled,
                config.openrouter.default,
                config.openrouter.has_key,
                config.ollama.enabled,
                config.ollama.default
            );
        }
        resolved
    };
    request.model = model_spec;

    // Set agent if specified
    if let Some(agent_name) = agent {
        request.agent = Some(agent_name);
    }

    state.sidecar.send_message(&session_id, request).await?;

    let target_session_id = session_id.clone();
    let sidecar_manager = state.sidecar.clone();

    // Process the stream and emit events to frontend
    tokio::spawn(async move {
        futures::pin_mut!(stream);
        use std::collections::HashMap;
        use std::time::{Duration, Instant};

        // Safety limit: 100k characters (approx 25k tokens) to prevent infinite loops
        const MAX_RESPONSE_CHARS: usize = 100_000;

        // Track pending tools so we can fail-fast instead of spinning forever when a tool never finishes.
        let mut pending_tools: HashMap<String, (String, Instant)> = HashMap::new();
        let tool_timeout = Duration::from_secs(120);
        let idle_timeout = Duration::from_secs(10 * 60);
        let mut last_progress = Instant::now();

        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    // Timeout if a tool has been pending too long.
                    if let Some((part_id, (tool, _started))) = pending_tools
                        .iter()
                        .find(|(_, (_, started))| started.elapsed() > tool_timeout)
                    {
                        tracing::warn!(
                            "Tool '{}' (part_id={}) exceeded timeout of {:?}, cancelling session {}",
                            tool,
                            part_id,
                            tool_timeout,
                            target_session_id
                        );
                        let _ = sidecar_manager.cancel_generation(&target_session_id).await;
                        let _ = app.emit(
                            "sidecar_event",
                            StreamEvent::SessionError {
                                session_id: target_session_id.clone(),
                                error: format!(
                                    "Tool '{}' did not complete after {:?}. Cancelled the request to prevent a stuck session.",
                                    tool,
                                    tool_timeout
                                ),
                            },
                        );
                        break;
                    }

                    // Also protect against total inactivity. (We ignore heartbeats in the parser.)
                    if pending_tools.is_empty() && last_progress.elapsed() > idle_timeout {
                        tracing::warn!(
                            "Session {} exceeded inactivity timeout of {:?}; cancelling generation",
                            target_session_id,
                            idle_timeout
                        );
                        let _ = sidecar_manager.cancel_generation(&target_session_id).await;
                        let _ = app.emit(
                            "sidecar_event",
                            StreamEvent::SessionError {
                                session_id: target_session_id.clone(),
                                error: format!(
                                    "No response progress for {:?}. Cancelled to avoid an indefinite hang.",
                                    idle_timeout
                                ),
                            },
                        );
                        break;
                    }
                }

                maybe = stream.next() => {
                    let Some(result) = maybe else { break };
                    match result {
                        Ok(event) => {
                            // Filter events for our session
                            let is_our_session = match &event {
                                StreamEvent::Content { session_id, content, .. } => {
                                    if session_id == &target_session_id && content.len() > MAX_RESPONSE_CHARS {
                                        tracing::warn!(
                                            "Response exceeded safety limit ({} chars), cancelling session {}",
                                            MAX_RESPONSE_CHARS,
                                            target_session_id
                                        );
                                        let _ = sidecar_manager.cancel_generation(&target_session_id).await;
                                        let _ = app.emit(
                                            "sidecar_event",
                                            &StreamEvent::SessionError {
                                                session_id: target_session_id.clone(),
                                                error: format!(
                                                    "Response stopped: exceeded safety limit of {} characters.",
                                                    MAX_RESPONSE_CHARS
                                                ),
                                            },
                                        );
                                        break;
                                    }
                                    session_id == &target_session_id
                                }
                                StreamEvent::ToolStart { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::ToolEnd { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::SessionStatus { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::SessionIdle { session_id } => session_id == &target_session_id,
                                StreamEvent::SessionError { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::PermissionAsked { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::QuestionAsked { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::FileEdited { session_id, .. } => session_id == &target_session_id,
                                StreamEvent::TodoUpdated { session_id, .. } => session_id == &target_session_id,
                                // Don't forward raw event spam to the UI.
                                StreamEvent::Raw { .. } => false,
                            };

                            if is_our_session {
                                last_progress = Instant::now();

                                // Maintain a best-effort pending tool table.
                                match &event {
                                    StreamEvent::ToolStart { part_id, tool, .. } => {
                                        pending_tools.insert(part_id.clone(), (tool.clone(), Instant::now()));
                                    }
                                    StreamEvent::ToolEnd { part_id, .. } => {
                                        pending_tools.remove(part_id);
                                    }
                                    _ => {}
                                }

                                // Log event for debugging - summarize large payloads
                                match &event {
                                    StreamEvent::Content { content, delta, .. } => {
                                        tracing::info!(
                                            "[StreamEvent] Emitting Content: len={}, delta={}",
                                            content.len(),
                                            delta.as_ref().map(|d| d.len()).unwrap_or(0)
                                        );
                                    }
                                    StreamEvent::ToolStart { tool, .. } => {
                                        tracing::info!("[StreamEvent] Emitting ToolStart: tool={}", tool);
                                    }
                                    StreamEvent::ToolEnd { tool, error, .. } => {
                                        tracing::info!(
                                            "[StreamEvent] Emitting ToolEnd: tool={}, success={}",
                                            tool,
                                            error.is_none()
                                        );
                                    }
                                    _ => {
                                        tracing::info!(
                                            "[StreamEvent] Emitting to frontend: type={:?}",
                                            event
                                        );
                                    }
                                }

                                // Emit the event to the frontend
                                if let Err(e) = app.emit("sidecar_event", &event) {
                                    tracing::error!("Failed to emit sidecar event: {}", e);
                                    break;
                                }

                                // Stop streaming for terminal session events (prevents spinner hangs).
                                if matches!(event, StreamEvent::SessionIdle { .. } | StreamEvent::SessionError { .. }) {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            tracing::warn!("[StreamEvent] Stream error: {}", err_msg);

                            // If it's a common timeout error, provide a more user-friendly message
                            let friendly_error = if err_msg.contains("error decoding response body") {
                                "Connection to AI engine timed out. The AI might be taking too long to respond.".to_string()
                            } else {
                                format!("Stream error: {}", err_msg)
                            };

                            // Emit error event
                            let _ = app.emit(
                                "sidecar_event",
                                StreamEvent::SessionError {
                                    session_id: target_session_id.clone(),
                                    error: friendly_error,
                                },
                            );
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

/// Cancel ongoing generation
#[tauri::command]
pub async fn cancel_generation(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sidecar.cancel_generation(&session_id).await
}

// ============================================================================
// Model & Provider Info
// ============================================================================

/// List available models from the sidecar
#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    state.sidecar.list_models().await
}

/// List available providers from the sidecar
#[tauri::command]
pub async fn list_providers_from_sidecar(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>> {
    state.sidecar.list_providers().await
}

// ============================================================================
// Logs (on-demand streaming)
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct LogStreamBatch {
    pub stream_id: String,
    pub source: String, // "tandem" | "sidecar"
    pub lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u64>,
    pub ts_ms: u64,
}

#[tauri::command]
pub async fn list_app_log_files(app: AppHandle) -> Result<Vec<LogFileInfo>> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to get app data dir: {}", e)))?;
    let logs_dir = app_data_dir.join("logs");
    logs::list_log_files(&logs_dir)
}

#[tauri::command]
pub async fn start_log_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    source: String,
    file_name: Option<String>,
    tail_lines: Option<u32>,
) -> Result<String> {
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| TandemError::InvalidConfig(format!("Window not found: {}", window_label)))?;

    let stream_id = format!("log_{}", Uuid::new_v4());
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    state
        .active_log_streams
        .lock()
        .await
        .insert(stream_id.clone(), stop_tx);

    let tail_lines = tail_lines.unwrap_or(500).clamp(10, 5000) as usize;

    let stream_id_clone = stream_id.clone();
    let source_clone = source.clone();
    let sidecar = state.sidecar.clone();
    let active_map = state.active_log_streams.clone();

    tokio::spawn(async move {
        let source_kind = source_clone.clone();
        let stream_id_for_emit = stream_id_clone.clone();
        let source_for_emit = source_clone.clone();
        let window_for_emit = window.clone();

        let send_batch = |lines: Vec<String>, dropped: Option<u64>| {
            let window = window_for_emit.clone();
            let stream_id = stream_id_for_emit.clone();
            let source = source_for_emit.clone();
            async move {
                if lines.is_empty() {
                    return;
                }
                let payload = LogStreamBatch {
                    stream_id,
                    source,
                    lines,
                    dropped,
                    ts_ms: logs::now_ms(),
                };
                let _ = window.emit("log_stream_event", payload);
            }
        };

        match source_kind.as_str() {
            "sidecar" => {
                let (snap, dropped_total) = sidecar.sidecar_logs_snapshot(tail_lines);
                let mut last_seq = 0u64;
                let mut out = Vec::new();
                for (seq, text) in snap {
                    last_seq = last_seq.max(seq);
                    out.push(text);
                }
                send_batch(out, Some(dropped_total)).await;

                let mut tick = tokio::time::interval(Duration::from_millis(200));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tick.tick() => {
                            let (lines, dropped_total) = sidecar.sidecar_logs_since(last_seq);
                            let mut out = Vec::new();
                            for (seq, text) in lines {
                                last_seq = last_seq.max(seq);
                                out.push(text);
                            }
                            for chunk in out.chunks(200) {
                                send_batch(chunk.to_vec(), Some(dropped_total)).await;
                            }
                        }
                    }
                }
            }
            "tandem" => {
                let app_data_dir = match app.path().app_data_dir() {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = send_batch(
                            vec![format!("ERROR Failed to resolve app data dir: {e}")],
                            None,
                        )
                        .await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };
                let logs_dir = app_data_dir.join("logs");

                let file_name = match file_name {
                    Some(n) => match logs::sanitize_log_file_name(&n) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = send_batch(vec![format!("ERROR {e}")], None).await;
                            let _ = active_map.lock().await.remove(&stream_id_clone);
                            return;
                        }
                    },
                    None => {
                        let _ =
                            send_batch(vec!["ERROR Missing log file name".to_string()], None).await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };

                let path = logs::join_logs_dir(&logs_dir, &file_name);
                let (initial, mut offset) = match logs::tail_file(&path, tail_lines, 256 * 1024) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ =
                            send_batch(vec![format!("ERROR Failed to open log: {e}")], None).await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };
                send_batch(initial, None).await;

                let mut partial = String::new();
                let mut tick = tokio::time::interval(Duration::from_millis(200));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tick.tick() => {
                            use std::io::{Read, Seek, SeekFrom};
                            let mut f = match std::fs::File::open(&path) {
                                Ok(f) => f,
                                Err(_) => continue,
                            };
                            let meta = match f.metadata() {
                                Ok(m) => m,
                                Err(_) => continue,
                            };
                            let len = meta.len();
                            if len < offset {
                                offset = 0;
                                partial.clear();
                            }
                            if f.seek(SeekFrom::Start(offset)).is_err() {
                                continue;
                            }
                            let mut buf = Vec::new();
                            if f.read_to_end(&mut buf).is_err() {
                                continue;
                            }
                            offset = len;
                            if buf.is_empty() {
                                continue;
                            }
                            partial.push_str(&String::from_utf8_lossy(&buf));

                            // Avoid borrowing issues by processing an owned copy of the buffer.
                            let data = std::mem::take(&mut partial);
                            let bytes = data.as_bytes();
                            let mut lines = Vec::new();
                            let mut start = 0usize;
                            for (i, b) in bytes.iter().enumerate() {
                                if *b == b'\n' {
                                    let mut slice = &data[start..i];
                                    if slice.ends_with('\r') {
                                        slice = &slice[..slice.len().saturating_sub(1)];
                                    }
                                    if !slice.is_empty() {
                                        lines.push(slice.to_string());
                                    }
                                    start = i + 1;
                                }
                            }
                            // Remainder (no trailing newline yet)
                            if start < data.len() {
                                partial = data[start..].to_string();
                            }
                            for chunk in lines.chunks(200) {
                                send_batch(chunk.to_vec(), None).await;
                            }
                        }
                    }
                }
            }
            other => {
                let _ = send_batch(vec![format!("ERROR Unknown log source: {other}")], None).await;
            }
        }

        // Best-effort cleanup
        active_map.lock().await.remove(&stream_id_clone);
    });

    Ok(stream_id)
}

#[tauri::command]
pub async fn stop_log_stream(state: State<'_, AppState>, stream_id: String) -> Result<()> {
    if let Some(tx) = state.active_log_streams.lock().await.remove(&stream_id) {
        let _ = tx.send(());
    }
    Ok(())
}

/// List models installed locally via Ollama
#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<ModelInfo>> {
    let output = Command::new("ollama").arg("list").output().map_err(|e| {
        TandemError::Sidecar(format!(
            "Failed to execute 'ollama list': {}. Is Ollama installed?",
            e
        ))
    })?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    // Skip header line
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        // Ollama names are often like 'llama3:latest' or just 'llama3'
        // We use the full name as the ID as well for simplify
        models.push(ModelInfo {
            id: name.clone(),
            name: name.clone(),
            provider: Some("ollama".to_string()),
            context_length: None,
        });
    }

    Ok(models)
}

/// List running Ollama models (ollama ps)
#[tauri::command]
pub async fn list_running_ollama_models() -> Result<Vec<ModelInfo>> {
    let output = Command::new("ollama")
        .arg("ps")
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama ps': {}", e)))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    // Skip header line
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        models.push(ModelInfo {
            id: name.clone(),
            name: name.clone(),
            provider: Some("ollama".to_string()),
            context_length: None,
        });
    }

    Ok(models)
}

/// Stop a running Ollama model
#[tauri::command]
pub async fn stop_ollama_model(name: String) -> Result<()> {
    Command::new("ollama")
        .arg("stop")
        .arg(name)
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama stop': {}", e)))?;
    Ok(())
}

/// Run (load) an Ollama model
#[tauri::command]
pub async fn run_ollama_model(name: String) -> Result<()> {
    // We run with an empty prompt to just trigger loading
    Command::new("ollama")
        .arg("run")
        .arg(name)
        .arg("")
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama run': {}", e)))?;
    Ok(())
}

// ============================================================================
// File Operation Undo
// ============================================================================

/// Result of an undo operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct UndoResult {
    pub reverted_entry_id: String,
    pub path: String,
    pub operation: String,
}

/// Check if undo is available
#[tauri::command]
pub fn can_undo_file_change(state: State<'_, AppState>) -> bool {
    state.operation_journal.can_undo()
}

/// Undo the last file operation
#[tauri::command]
pub fn undo_last_file_change(state: State<'_, AppState>) -> Result<Option<UndoResult>> {
    match state.operation_journal.undo_last()? {
        Some(entry_id) => {
            // Get the journal entry to return details
            let entries = state.operation_journal.get_recent_entries(100);
            if let Some(entry) = entries.iter().find(|e| e.id == entry_id) {
                Ok(Some(UndoResult {
                    reverted_entry_id: entry_id,
                    path: entry
                        .before_state
                        .as_ref()
                        .map(|s| s.path.clone())
                        .unwrap_or_default(),
                    operation: entry.tool_name.clone(),
                }))
            } else {
                Ok(Some(UndoResult {
                    reverted_entry_id: entry_id,
                    path: String::new(),
                    operation: "unknown".to_string(),
                }))
            }
        }
        None => Ok(None),
    }
}

/// Get recent file operations
#[tauri::command]
pub fn get_recent_file_operations(state: State<'_, AppState>, count: usize) -> Vec<JournalEntry> {
    state.operation_journal.get_recent_entries(count)
}

// ============================================================================
// Conversation Rewind
// ============================================================================

/// Rewind to a specific message by creating a new branched session
#[tauri::command]
pub async fn rewind_to_message(
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
    edited_content: Option<String>,
) -> Result<Session> {
    tracing::info!("Rewinding session {} to message {}", session_id, message_id);

    // 1. Get all messages from the current session
    let messages = state.sidecar.get_session_messages(&session_id).await?;

    // 2. Find the index of the target message
    let mut target_index = None;
    for (i, msg) in messages.iter().enumerate() {
        if msg.info.id == message_id {
            target_index = Some(i);
            break;
        }
    }

    let _target_index = target_index.ok_or_else(|| {
        TandemError::Sidecar(format!("Message {} not found in session", message_id))
    })?;

    // 3. Create a new session
    let (default_provider, default_model) = {
        let config = state.providers_config.read().unwrap();
        resolve_default_provider_and_model(&config)
    };

    let new_session = state
        .sidecar
        .create_session(CreateSessionRequest {
            parent_id: None,
            title: Some(format!("Rewind from {}", session_id)),
            model: default_model,
            provider: default_provider,
            permission: Some(vec![
                crate::sidecar::PermissionRule {
                    permission: "ls".to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                },
                crate::sidecar::PermissionRule {
                    permission: "read".to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                },
                crate::sidecar::PermissionRule {
                    permission: "todowrite".to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                },
                crate::sidecar::PermissionRule {
                    permission: "websearch".to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                },
                crate::sidecar::PermissionRule {
                    permission: "webfetch".to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                },
            ]),
        })
        .await?;

    tracing::info!("Created new branched session: {}", new_session.id);

    // 4. Replay messages up to (but not including) the target message
    // OpenCode doesn't have a direct API to copy messages, so we'll just return the new session
    // The frontend will handle displaying the branched conversation

    // TODO: In a future enhancement, we could replay messages by sending them to the new session
    // For now, we'll just create an empty session for the user to continue from

    // If edited content is provided, send it as the first message
    if let Some(content) = edited_content {
        tracing::info!("Sending edited message to new session");
        let request = SendMessageRequest::text(content);
        state.sidecar.send_message(&new_session.id, request).await?;
    }

    // Update current session
    {
        let mut current = state.current_session_id.write().unwrap();
        *current = Some(new_session.id.clone());
    }

    Ok(new_session)
}

// ============================================================================
// Message Undo/Redo (OpenCode native revert/unrevert)
// ============================================================================

/// Undo a message (revert)
/// Reverts the specified message and any file changes it made
#[tauri::command]
pub async fn undo_message(
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
) -> Result<()> {
    tracing::info!("Undoing message {} in session {}", message_id, session_id);
    state.sidecar.revert_message(&session_id, &message_id).await
}

/// Undo a message and also revert any file changes we recorded for that message.
/// This uses Tandem's local operation journal (captured at tool-approval time) to restore files.
#[tauri::command]
pub async fn undo_message_with_files(
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
) -> Result<Vec<String>> {
    tracing::info!(
        "[undo_message_with_files] Undoing message {} in session {}",
        message_id,
        session_id
    );

    // 1) Revert the OpenCode message (conversation state)
    if let Err(e) = state.sidecar.revert_message(&session_id, &message_id).await {
        tracing::warn!(
            "[undo_message_with_files] OpenCode revert failed (continuing with file restore): {}",
            e
        );
    }

    // 2) Restore any files we journaled for this message
    tracing::info!(
        "[undo_message_with_files] Looking for snapshots with message_id={}",
        message_id
    );
    let reverted_paths = state.operation_journal.undo_for_message(&message_id)?;
    tracing::info!(
        "[undo_message_with_files] Restored {} files: {:?}",
        reverted_paths.len(),
        reverted_paths
    );

    Ok(reverted_paths)
}

/// Redo messages (unrevert)
/// Restores previously reverted messages
#[tauri::command]
pub async fn redo_message(state: State<'_, AppState>, session_id: String) -> Result<()> {
    tracing::info!("Redoing messages in session {}", session_id);
    state.sidecar.unrevert_message(&session_id).await
}

/// Execute undo via OpenCode's slash command API
/// This properly triggers Git-based file restoration
#[tauri::command]
pub async fn undo_via_command(state: State<'_, AppState>, session_id: String) -> Result<()> {
    tracing::info!("Executing /undo via prompt in session {}", session_id);

    // Send "/undo" as a regular prompt - same as typing it in the TUI
    // OpenCode intercepts slash commands and handles them specially
    let request = crate::sidecar::SendMessageRequest::text("/undo".to_string());
    state.sidecar.send_message(&session_id, request).await
}

// ============================================================================
// File Snapshot (for undo)
// ============================================================================

/// Capture a snapshot of a file BEFORE it's about to be modified.
/// This is called at tool_start time (not approval time) since OpenCode may execute tools without permission.
#[tauri::command]
pub fn snapshot_file_for_message(
    state: State<'_, AppState>,
    file_path: String,
    tool: String,
    message_id: String,
) -> Result<()> {
    tracing::info!(
        "[snapshot_file_for_message] Capturing snapshot for path='{}', tool='{}', message_id='{}'",
        file_path,
        tool,
        message_id
    );

    let path_buf = PathBuf::from(&file_path);

    // Validate path against allowed workspace scope
    if !state.is_path_allowed(&path_buf) {
        tracing::warn!(
            "[snapshot_file_for_message] Skipping snapshot for disallowed path '{}'",
            file_path
        );
        return Ok(());
    }

    let (exists, is_directory) = match fs::metadata(&path_buf) {
        Ok(meta) => (true, meta.is_dir()),
        Err(_) => (false, false),
    };

    let content = if exists && !is_directory {
        fs::read_to_string(&path_buf).ok()
    } else {
        None
    };

    let snapshot = FileSnapshot {
        path: file_path.clone(),
        content,
        exists,
        is_directory,
    };

    let entry = JournalEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        tool_name: tool.clone(),
        args: serde_json::json!({"filePath": file_path}),
        status: OperationStatus::Completed,
        before_state: Some(snapshot.clone()),
        after_state: None,
        user_approved: true, // Auto-approved since OpenCode already decided to execute
    };

    let undo_action = UndoAction {
        journal_entry_id: entry.id.clone(),
        snapshot,
        message_id: Some(message_id.clone()),
    };

    tracing::info!(
        "[snapshot_file_for_message] Recorded snapshot for path='{}', exists={}, message_id='{}'",
        file_path,
        exists,
        message_id
    );

    state.operation_journal.record(entry, Some(undo_action));

    Ok(())
}

// ============================================================================
// Tool Approval
// ============================================================================

/// Approve a pending tool execution
#[tauri::command]
pub async fn approve_tool(
    state: State<'_, AppState>,
    session_id: String,
    tool_call_id: String,
    tool: Option<String>,
    args: Option<serde_json::Value>,
    message_id: Option<String>,
) -> Result<()> {
    tracing::info!(
        "[approve_tool] tool={:?}, message_id={:?}, args={:?}",
        tool,
        message_id,
        args
    );

    // Strict Python venv enforcement for AI terminal-like tools.
    // Goal: prevent global pip installs and python runs outside `.opencode/.venv`.
    if let (Some(tool_name), Some(args_val)) = (tool.clone(), args.clone()) {
        let is_terminal_tool = matches!(
            tool_name.as_str(),
            "bash" | "shell" | "run_command" | "terminal" | "cmd"
        );

        if is_terminal_tool {
            let command = args_val
                .get("command")
                .and_then(|v| v.as_str())
                .or_else(|| args_val.get("cmd").and_then(|v| v.as_str()))
                .or_else(|| args_val.get("script").and_then(|v| v.as_str()));

            if let Some(command) = command {
                let ws = state
                    .get_workspace_path()
                    .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

                if let Some(msg) = python_env::check_terminal_command_policy(&ws, command) {
                    tracing::info!(
                        "[python_policy] Denying terminal command (tool={}): {}",
                        tool_name,
                        command.replace('\n', " ")
                    );
                    let _ = state.sidecar.deny_tool(&session_id, &tool_call_id).await;
                    return Err(TandemError::PermissionDenied(msg));
                }
            }
        }
    }

    // Capture a snapshot BEFORE allowing the tool to run, so we can undo file changes later.
    // We only snapshot direct file tools (write/delete). Shell commands and reads are too broad.
    // Note: OpenCode's tool names are "write", "delete", "read", "bash", "list", "search", etc.
    if let (Some(tool_name), Some(args_val)) = (tool.clone(), args.clone()) {
        let is_file_tool = matches!(
            tool_name.as_str(),
            "write" | "write_file" | "create_file" | "delete" | "delete_file"
        );

        if is_file_tool {
            tracing::info!("[approve_tool] File tool detected: {}", tool_name);

            // Try to extract a file path from args
            // OpenCode uses "filePath" for write operations
            let path_str = args_val
                .get("filePath")
                .and_then(|v| v.as_str())
                .or_else(|| args_val.get("absolute_path").and_then(|v| v.as_str()))
                .or_else(|| args_val.get("path").and_then(|v| v.as_str()))
                .or_else(|| args_val.get("file").and_then(|v| v.as_str()))
                .map(|s| s.to_string());

            tracing::info!("[approve_tool] Extracted path: {:?}", path_str);

            if let Some(path) = path_str {
                let path_buf = PathBuf::from(&path);

                // Validate path against allowed workspace scope
                if state.is_path_allowed(&path_buf) {
                    let _fs_write_permit = state
                        .fs_write_semaphore
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|_| {
                        TandemError::InvalidOperation(
                            "Failed to acquire fs_write permit".to_string(),
                        )
                    })?;
                    let _path_lock = state.path_locks.write_lock(&path_buf).await;

                    let (exists, is_directory) = match fs::metadata(&path_buf) {
                        Ok(meta) => (true, meta.is_dir()),
                        Err(_) => (false, false),
                    };

                    let content = if exists && !is_directory {
                        fs::read_to_string(&path_buf).ok()
                    } else {
                        None
                    };

                    let snapshot = FileSnapshot {
                        path: path.clone(),
                        content,
                        exists,
                        is_directory,
                    };

                    let entry = JournalEntry {
                        id: Uuid::new_v4().to_string(),
                        timestamp: chrono::Utc::now(),
                        tool_name: tool_name.clone(),
                        args: args_val.clone(),
                        status: OperationStatus::Approved,
                        before_state: Some(snapshot.clone()),
                        after_state: None,
                        user_approved: true,
                    };

                    let undo_action = UndoAction {
                        journal_entry_id: entry.id.clone(),
                        snapshot,
                        message_id: message_id.clone(),
                    };

                    tracing::info!(
                        "[approve_tool] Recorded snapshot for path '{}' with message_id {:?}, exists={}",
                        path,
                        message_id,
                        exists
                    );
                    state.operation_journal.record(entry, Some(undo_action));
                } else {
                    tracing::warn!(
                        "Skipping snapshot for disallowed path '{}' on approve_tool",
                        path
                    );
                }
            } else {
                tracing::warn!(
                    "[approve_tool] Could not extract path from args: {:?}",
                    args_val
                );
            }
        }
    } else {
        tracing::info!("[approve_tool] No tool/args provided, skipping snapshot");
    }

    state.sidecar.approve_tool(&session_id, &tool_call_id).await
}

/// Deny a pending tool execution
#[tauri::command]
pub async fn deny_tool(
    state: State<'_, AppState>,
    session_id: String,
    tool_call_id: String,
    tool: Option<String>,
    args: Option<serde_json::Value>,
    _message_id: Option<String>,
) -> Result<()> {
    // Record denied operations for visibility/debugging (no undo action).
    if let (Some(tool_name), Some(args_val)) = (tool.clone(), args.clone()) {
        let entry = JournalEntry {
            id: Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            tool_name,
            args: args_val,
            status: OperationStatus::Denied,
            before_state: None,
            after_state: None,
            user_approved: false,
        };
        state.operation_journal.record(entry, None);
    }

    state.sidecar.deny_tool(&session_id, &tool_call_id).await
}

/// Answer a question from the LLM
#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    session_id: String,
    question_id: String,
    answer: String,
) -> Result<()> {
    state
        .sidecar
        .answer_question(&session_id, &question_id, answer)
        .await
}

/// List pending question requests from the sidecar (OpenCode).
#[tauri::command]
pub async fn list_questions(
    state: State<'_, AppState>,
) -> Result<Vec<crate::sidecar::QuestionRequest>> {
    state.sidecar.list_questions().await
}

/// Reply to a pending question request.
#[tauri::command]
pub async fn reply_question(
    state: State<'_, AppState>,
    request_id: String,
    answers: Vec<Vec<String>>,
) -> Result<()> {
    state.sidecar.reply_question(&request_id, answers).await
}

/// Reject a pending question request.
#[tauri::command]
pub async fn reject_question(state: State<'_, AppState>, request_id: String) -> Result<()> {
    state.sidecar.reject_question(&request_id).await
}

// ============================================================================
// Execution Planning / Staging Area
// ============================================================================

/// Stage a tool operation for batch execution
#[tauri::command]
pub async fn stage_tool_operation(
    state: State<'_, AppState>,
    request_id: String,
    session_id: String,
    tool: String,
    args: serde_json::Value,
    message_id: Option<String>,
) -> Result<()> {
    use crate::tool_proxy::StagedOperation;

    // Extract file path if it's a file operation
    let path_str = args
        .get("filePath")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("absolute_path").and_then(|v| v.as_str()))
        .or_else(|| args.get("path").and_then(|v| v.as_str()))
        .or_else(|| args.get("file").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    // Check if this operation should be auto-approved (e.g., plan file writes)
    let should_auto_approve = if let Some(path) = path_str.as_ref() {
        is_plan_file_operation(path, &tool)
    } else {
        false
    };

    // If auto-approved, execute immediately instead of staging
    if should_auto_approve {
        if let Some(path) = path_str.as_ref() {
            tracing::info!("Auto-approving plan file operation: {} on {}", tool, path);
        }
        // Approve the request ID so OpenCode can proceed
        state.sidecar.approve_tool(&session_id, &request_id).await?;
        return Ok(());
    }

    // Create snapshot for file operations
    let (before_snapshot, proposed_content) = if let Some(path) = path_str.as_ref() {
        let path_buf = PathBuf::from(path);

        if state.is_path_allowed(&path_buf) {
            if matches!(
                tool.as_str(),
                "write" | "delete" | "write_file" | "delete_file" | "create_file"
            ) {
                let _fs_write_permit = state
                    .fs_write_semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| {
                        TandemError::InvalidOperation(
                            "Failed to acquire fs_write permit".to_string(),
                        )
                    })?;
                let _path_lock = state.path_locks.write_lock(&path_buf).await;
            }

            let (exists, is_directory) = match fs::metadata(&path_buf) {
                Ok(meta) => (true, meta.is_dir()),
                Err(_) => (false, false),
            };

            let content = if exists && !is_directory {
                fs::read_to_string(&path_buf).ok()
            } else {
                None
            };

            let snapshot = crate::tool_proxy::FileSnapshot {
                path: path.clone(),
                content,
                exists,
                is_directory,
            };

            // Extract proposed content for write operations
            let proposed = if tool == "write" {
                args.get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            };

            (Some(snapshot), proposed)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // Generate description
    let description = if let Some(path) = path_str.as_ref() {
        let short_path = if path.len() > 50 {
            format!("...{}", &path[path.len() - 47..])
        } else {
            path.clone()
        };

        match tool.as_str() {
            "write" => format!("Write to {}", short_path),
            "delete" => format!("Delete {}", short_path),
            "bash" | "shell" => {
                if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                    let short_cmd = if cmd.len() > 50 {
                        format!("{}...", &cmd[..47])
                    } else {
                        cmd.to_string()
                    };
                    format!("Run: {}", short_cmd)
                } else {
                    "Run command".to_string()
                }
            }
            _ => format!("{} {}", tool, short_path),
        }
    } else if tool == "bash" || tool == "shell" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            let short_cmd = if cmd.len() > 50 {
                format!("{}...", &cmd[..47])
            } else {
                cmd.to_string()
            };
            format!("Run: {}", short_cmd)
        } else {
            "Run command".to_string()
        }
    } else {
        format!("Execute {}", tool)
    };

    let operation = StagedOperation {
        id: uuid::Uuid::new_v4().to_string(),
        request_id,
        session_id,
        tool,
        args,
        before_snapshot,
        proposed_content,
        timestamp: chrono::Utc::now(),
        description,
        message_id,
    };

    state.staging_store.stage(operation);
    tracing::info!("Staged operation for batch execution");

    Ok(())
}

/// Get all staged operations
#[tauri::command]
pub fn get_staged_operations(
    state: State<'_, AppState>,
) -> Vec<crate::tool_proxy::StagedOperation> {
    state.staging_store.get_all()
}

/// Execute all staged operations in sequence
#[tauri::command]
pub async fn execute_staged_plan(state: State<'_, AppState>) -> Result<Vec<String>> {
    let operations = state.staging_store.get_all();
    let mut executed_ids = Vec::new();

    tracing::info!("Executing staged plan with {} operations", operations.len());

    for op in operations {
        tracing::info!("Executing staged operation: {} ({})", op.id, op.tool);

        // Approve the tool with OpenCode sidecar
        if let Err(e) = state
            .sidecar
            .approve_tool(&op.session_id, &op.request_id)
            .await
        {
            tracing::error!("Failed to execute staged operation {}: {}", op.id, e);
            // Continue with other operations even if one fails
            continue;
        }

        // Record in journal for undo
        if op.before_snapshot.is_some() {
            use crate::tool_proxy::{JournalEntry, OperationStatus, UndoAction};

            let entry = JournalEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: op.timestamp,
                tool_name: op.tool.clone(),
                args: op.args.clone(),
                status: OperationStatus::Completed,
                before_state: op.before_snapshot.clone(),
                after_state: None,
                user_approved: true,
            };

            if let Some(snapshot) = op.before_snapshot {
                let undo_action = UndoAction {
                    journal_entry_id: entry.id.clone(),
                    snapshot,
                    message_id: op.message_id.clone(),
                };
                state.operation_journal.record(entry, Some(undo_action));
            } else {
                state.operation_journal.record(entry, None);
            }
        }

        executed_ids.push(op.id);
    }

    // Clear staging area after execution
    state.staging_store.clear();

    tracing::info!("Executed {} staged operations", executed_ids.len());
    Ok(executed_ids)
}

/// Remove a single staged operation
#[tauri::command]
pub fn remove_staged_operation(state: State<'_, AppState>, operation_id: String) -> Result<bool> {
    Ok(state.staging_store.remove(&operation_id).is_some())
}

/// Clear all staged operations
#[tauri::command]
pub fn clear_staging_area(state: State<'_, AppState>) -> Result<usize> {
    let cleared = state.staging_store.clear();
    Ok(cleared.len())
}

/// Get count of staged operations
#[tauri::command]
pub fn get_staged_count(state: State<'_, AppState>) -> usize {
    state.staging_store.count()
}

// ============================================================================
// Sidecar Binary Management
// ============================================================================

/// Check the sidecar binary status (installed, version, updates available)
#[tauri::command]
pub async fn check_sidecar_status(app: AppHandle) -> Result<SidecarStatus> {
    sidecar_manager::check_sidecar_status(&app).await
}

/// Download/update the sidecar binary
#[tauri::command]
pub async fn download_sidecar(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    // Stop the sidecar first to release the binary file lock
    tracing::info!("Stopping sidecar before download");
    let _ = state.sidecar.stop().await;

    // Give the process extra time to fully terminate and release file handles
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    sidecar_manager::download_sidecar(app).await
}

// ============================================================================
// Tool Definitions (for conditional tool injection)
// ============================================================================

/// Tool guidance for LLM context injection
/// Instead of custom OpenCode tools, we provide structured instructions
/// for using existing tools (like 'write') to create specialized files
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolGuidance {
    pub category: String,
    pub instructions: String,
    pub json_schema: serde_json::Value,
    pub example: String,
}

/// Get tool guidance based on enabled categories
/// This injects structured instructions for the LLM to follow
#[tauri::command]
pub fn get_tool_guidance(categories: Vec<String>) -> Vec<ToolGuidance> {
    let mut guidance = Vec::new();

    for category in &categories {
        // Fix: borrow categories instead of moving
        match category.as_str() {
            "presentations" => {
                guidance.push(ToolGuidance {
                    category: "presentations".to_string(),
                    instructions: r#"# High-Fidelity 16:9 HTML Slideshows

Use this capability to create premium, interactive presentations that look like modern dashboards.

## TWO-PHASE WORKFLOW:

### PHASE 1: PLANNING
1. **Outline**: Present a structured outline (Title, Theme, Slide-by-slide layout).
2. **Review**: Allow the user to request changes to colors, layout, or content.
3. **Approval**: Once the user approves the outline, proceed to Phase 2.

### PHASE 2: IMPLEMENTATION
1. **Apply Feedback**: Incorporate all requested refinements from the planning phase.
2. **Generate Code**: Use the `write` tool to create the `{filename}.slides.html` file.
3. **Summary**: Briefly confirm that the file has been generated with the requested styles.

## TECHNICAL REQUIREMENTS:

### 1. Slide Stacking (Critical)
- **Absolute Stacking**: All `.slide` elements must be stacked on top of each other.
- **Visibility**: Only the `.active` slide should be visible; all others MUST be `display: none !important`.
- **Content Containment**: Add `overflow: hidden` to `.slide` to prevent content spill.

### 2. Layout & Scaling
- **16:9 aspect ratio** (1920x1080).
- **Safe Margins**: 100px padding for all content.
- **Scale to Fit**: Multi-directional scaling for the entire deck.

### 3. Content Density Limits (STRICT)
- **Max List Items**: 6 per slide.
- **Max Columns**: 2 per slide.
- **Vertical Space**: Leave 200px empty at the bottom.

### 4. High-Fidelity PDF Export
- Add an "Export to PDF" button that triggers `window.print()`.
- **CSS Requirements for Clean PDFs**:
  - `@page { margin: 0; size: landscape; }` (Crucial: Removes headers/footers).
  - `html, body { -webkit-print-color-adjust: exact !important; print-color-adjust: exact !important; }` (Preserves background colors/gradients).
  - Hide all navigation buttons and counters via `.no-print { display: none !important; }`.

## SLIDESHOW HTML TEMPLATE:
```html
<!DOCTYPE html>
<html>
<head>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <link href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" rel="stylesheet">
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700&display=swap" rel="stylesheet">
    <style>
        @page { margin: 0; size: landscape; }
        body, html { 
            margin: 0; padding: 0; width: 100%; height: 100%; overflow: hidden; background: #020617; 
            -webkit-print-color-adjust: exact !important; print-color-adjust: exact !important;
        }
        #viewport { width: 100vw; height: 100vh; display: flex; align-items: center; justify-content: center; }
        #deck { 
            width: 1920px; height: 1080px; 
            position: relative; 
            transform-origin: center;
        }
        .slide { 
            position: absolute; inset: 0; 
            display: none; 
            padding: 100px;
            flex-direction: column;
            overflow: hidden;
        }
        .slide.active { display: flex; }
        @media print {
            body { background: white; overflow: visible; height: auto; }
            #viewport, #deck { width: 100%; height: auto; transform: none !important; display: block; }
            .slide { position: relative; display: block !important; break-after: page; width: 100%; height: auto; aspect-ratio: 16/9; page-break-after: always; overflow: visible; }
            .no-print { display: none !important; }
        }
    </style>
</head>
<body>
    <div id="viewport">
        <div id="deck">
            <!-- SLIDE 1 -->
            <div class="slide active bg-slate-900 text-white">
                <h1 class="text-9xl font-bold italic tracking-tighter">TITLE</h1>
            </div>
            <!-- MORE SLIDES -->
        </div>
    </div>
    <!-- Nav buttons -->
    <div class="no-print fixed bottom-8 right-8 flex gap-4 items-center bg-black/40 backdrop-blur-xl p-4 rounded-2xl border border-white/10">
        <button onclick="window.print()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-emerald-600/20 hover:bg-emerald-600/30 text-emerald-400" title="Export to PDF"><i class="fas fa-file-pdf"></i></button>
        <div class="w-px h-8 bg-white/10"></div>
        <button onclick="prev()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-white/10 hover:bg-white/20"><i class="fas fa-chevron-left"></i></button>
        <span id="counter" class="text-white font-mono min-w-[60px] text-center">1 / X</span>
        <button onclick="next()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-white/10 hover:bg-white/20"><i class="fas fa-chevron-right"></i></button>
    </div>
    <script>
        let current = 0;
        const slides = document.querySelectorAll('.slide');
        function update() {
            slides.forEach((s, i) => s.classList.toggle('active', i === current));
            document.getElementById('counter').innerText = `${current + 1} / ${slides.length}`;
        }
        function next() { current = (current + 1) % slides.length; update(); }
        function prev() { current = (current - 1 + slides.length) % slides.length; update(); }
        window.onkeydown = (e) => { 
            if (['ArrowRight', 'Space', 'ArrowDown'].includes(e.code)) next();
            if (['ArrowLeft', 'ArrowUp'].includes(e.code)) prev();
        };
        function fit() {
            const scale = Math.min(window.innerWidth / 1920, window.innerHeight / 1080);
            document.getElementById('deck').style.transform = `scale(${scale})`;
        }
        window.onresize = fit; fit();
    </script>
</body>
</html>
```
"#.to_string(),
                    json_schema: serde_json::json!({
                        "file_type": "HTML Slideshow",
                        "scaling": "Auto-fit viewport",
                        "navigation": "Arrows, Space, Click",
                        "pdf_export": "Print button with optimized layout"
                    }),
                    example: "Generate 'strategic_path.slides.html' with 6 absolutely stacked slides, overflow:hidden, and a Print to PDF button.".to_string(),
                });
            }
            "canvas" => {
                guidance.push(ToolGuidance {
                    category: "canvas".to_string(),
                    instructions: r#"# HTML Canvas / Report Creation

Use this capability when the user asks for "reports", "visualizations", "dashboards", or "canvases".

You can create rich, interactive HTML files that render directly in Tandem's preview.

## Requirements:
1. Create a SINGLE standalone HTML file (e.g., `report.html`, `dashboard.html`).
2. Use **Tailwind CSS** via CDN for styling.
3. Use **Chart.js** via CDN for charts.
4. Use **Font Awesome** via CDN for icons.
5. Use **Google Fonts** (Inter) for typography.
6. The HTML must be self-contained (CSS/JS inside `<style>` and `<script>` tags).

## Template:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Report Title</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <link href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" rel="stylesheet">
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&display=swap" rel="stylesheet">
    <style>
        body { font-family: 'Inter', sans-serif; }
    </style>
</head>
<body class="bg-slate-50 text-slate-900">
    <div class="max-w-7xl mx-auto p-8">
        <!-- Content Here -->
        <canvas id="myChart"></canvas>
    </div>
    <script>
        // Chart.js logic here
    </script>
</body>
</html>
```

## Workflow:
1. **Plan:** Propose the structure/content of the report (Plan Mode).
2. **Execute:** Use the `write` tool to create the HTML file.
"#.to_string(),
                    json_schema: serde_json::json!({
                        "file_type": "HTML",
                        "libraries": ["Tailwind CSS", "Chart.js", "Font Awesome"],
                        "structure": "Single file, self-contained"
                    }),
                    example: "Use the `write` tool to create `quarterly_report.html` with Tailwind and Chart.js code.".to_string(),
                });
            }
            "research" => {
                guidance.push(ToolGuidance {
                    category: "research".to_string(),
                    instructions: r#"# Web Research & Browsing

Use this capability for finding information, verifying facts, or gathering data from the web.

## Best Practices:
1. **Search First:** Always start with `websearch` to find valid, up-to-date URLs.
2. **Avoid Dead Links:** Do not `webfetch` URLs that likely don't exist or are deep links without verifying them first.
3. **Handle Blocking:** Many sites (e.g., Statista, Airbnb, LinkedIn) block bots.
   - If `webfetch` returns 403/404/Timeout:
     - Do NOT retry the exact same URL immediately.
     - Try searching for the specific information on a different site.
     - Try fetching the root domain or a generic page if appropriate.
4. **Prefer Text:** `webfetch` works best on content-heavy pages (docs, blogs, articles). It may fail on heavy SPAs.

## Workflow:
1. **Search:** `websearch` query: "latest real estate trends asia 2025"
2. **Select:** Pick 1-2 promising URLs from the search results.
3. **Fetch:** `webfetch` url: "..."
4. **Fallback:** If fetch fails, go back to step 1 with a refined query or try the next URL.
"#.to_string(),
                    json_schema: serde_json::json!({
                        "strategy": "Search -> Select -> Fetch -> Fallback",
                        "error_handling": "Stop retrying failing URLs; use alternatives",
                        "limitations": "Some sites block automated access"
                    }),
                    example: "Search for 'rust tauri docs', then fetch the official documentation page.".to_string(),
                });
            }
            "diagrams" => {
                // Future: Mermaid diagram guidance
                tracing::debug!("Diagrams tool category not yet implemented");
            }
            "spreadsheets" => {
                // Future: Table/CSV guidance
                tracing::debug!("Spreadsheets tool category not yet implemented");
            }
            _ => {
                tracing::debug!("Unknown tool category: {}", category);
            }
        }
    }

    tracing::debug!(
        "Returning {} tool guidance items for categories: {:?}",
        guidance.len(),
        categories
    );
    guidance
}

// ============================================================================
// Presentation Export (ppt-rs)
// ============================================================================

const SLIDE_WIDTH: i32 = 12192000; // 13.33 inches in EMUs
const SLIDE_HEIGHT: i32 = 6858000; // 7.5 inches in EMUs

struct PptxTheme {
    bg: String,
    title: String,
    subtitle: String,
    text: String,
}

fn get_pptx_theme(theme: &crate::presentation::PresentationTheme) -> PptxTheme {
    use crate::presentation::PresentationTheme;
    match theme {
        PresentationTheme::Dark => PptxTheme {
            bg: "121212".to_string(),
            title: "FFFFFF".to_string(),
            subtitle: "A0A0A0".to_string(),
            text: "E0E0E0".to_string(),
        },
        PresentationTheme::Corporate => PptxTheme {
            bg: "1A365D".to_string(),
            title: "FFFFFF".to_string(),
            subtitle: "BEE3F8".to_string(),
            text: "E2E8F0".to_string(),
        },
        PresentationTheme::Minimal => PptxTheme {
            bg: "FFFFFF".to_string(),
            title: "1A202C".to_string(),
            subtitle: "718096".to_string(),
            text: "4A5568".to_string(),
        },
        _ => PptxTheme {
            bg: "F7FAFC".to_string(),
            title: "1A202C".to_string(),
            subtitle: "718096".to_string(),
            text: "2D3748".to_string(),
        },
    }
}

fn to_emu(percent: f64, total: i32) -> i32 {
    ((percent / 100.0) * total as f64) as i32
}

/// Export a .tandem.ppt.json file to a binary .pptx file using ppt-rs
#[tauri::command]
pub async fn export_presentation(json_path: String, output_path: String) -> Result<String> {
    use crate::presentation::{ElementContent, Presentation, SlideLayout};
    use std::fs::File;
    use std::io::Write;
    use zip::write::{FileOptions, ZipWriter};

    tracing::info!(
        "Exporting presentation from {} to {}",
        json_path,
        output_path
    );

    // 1. Read and parse JSON
    let json_content = std::fs::read_to_string(&json_path).map_err(TandemError::Io)?;

    let presentation: Presentation = serde_json::from_str(&json_content)
        .map_err(|e| TandemError::InvalidConfig(format!("Invalid presentation JSON: {}", e)))?;

    tracing::debug!(
        "Parsed presentation: {} with {} slides",
        presentation.title,
        presentation.slides.len()
    );

    let file = File::create(&output_path).map_err(TandemError::Io)?;

    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Helper to escape XML
    let escape_xml = |text: &str| -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    };

    // === [Content_Types].xml ===
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
"#,
    );

    for i in 1..=presentation.slides.len() {
        content_types.push_str(&format!(
            r#"  <Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
"#, i));
    }
    content_types.push_str("</Types>");

    zip.start_file("[Content_Types].xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(content_types.as_bytes())
        .map_err(TandemError::Io)?;

    // === _rels/.rels ===
    zip.start_file("_rels/.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#;
    zip.write_all(rels.as_bytes()).map_err(TandemError::Io)?;

    // === ppt/presentation.xml ===
    let mut pres_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" saveSubsetFonts="1">
  <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
  <p:sldIdLst>
"#,
    );

    for (i, _) in presentation.slides.iter().enumerate() {
        pres_xml.push_str(&format!(
            r#"    <p:sldId id="{}" r:id="rId{}"/>
"#,
            256 + i,
            i + 2
        ));
    }

    pres_xml.push_str(
        r#"  </p:sldIdLst>
  <p:sldSz cx="9144000" cy="6858000"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#,
    );

    zip.start_file("ppt/presentation.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_xml.as_bytes())
        .map_err(TandemError::Io)?;

    // === ppt/_rels/presentation.xml.rels ===
    let mut pres_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
"#,
    );

    for (i, _) in presentation.slides.iter().enumerate() {
        pres_rels.push_str(&format!(
            r#"  <Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>
"#, i + 2, i + 1));
    }
    pres_rels.push_str("</Relationships>");

    zip.start_file("ppt/_rels/presentation.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_rels.as_bytes())
        .map_err(TandemError::Io)?;

    // === Generate slides ===
    let ppt_theme = get_pptx_theme(&presentation.theme.unwrap_or_default());

    for (i, slide) in presentation.slides.iter().enumerate() {
        let slide_num = i + 1;
        let mut slide_xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
"#,
        );

        // Background shape
        slide_xml.push_str(&format!(
            r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="1000" name="Background"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
          <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
        </p:spPr>
      </p:sp>
"#,
            SLIDE_WIDTH, SLIDE_HEIGHT, ppt_theme.bg
        ));

        let mut shape_id = 2;

        // Title shape
        if let Some(title) = &slide.title {
            let (x, y, w, h) = match slide.layout {
                SlideLayout::Title => (10.0, 30.0, 80.0, 15.0),
                SlideLayout::Section => (10.0, 40.0, 80.0, 15.0),
                _ => (5.0, 5.0, 90.0, 10.0),
            };

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="ctr" vertical="ctr" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
          <a:p>
            <a:pPr algn="{}"/>
            <a:r>
              <a:rPr lang="en-US" sz="{}" b="1">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                if matches!(slide.layout, SlideLayout::Title | SlideLayout::Section) { "ctr" } else { "l" },
                if matches!(slide.layout, SlideLayout::Title) { 5400 } else { 3600 },
                ppt_theme.title,
                escape_xml(title)
            ));
            shape_id += 1;
        }

        // Subtitle
        if let Some(subtitle) = &slide.subtitle {
            let (x, y, w, h) = match slide.layout {
                SlideLayout::Title => (10.0, 45.0, 80.0, 10.0),
                SlideLayout::Section => (10.0, 55.0, 80.0, 10.0),
                _ => (5.0, 15.0, 90.0, 5.0),
            };

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Subtitle"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="ctr" vertical="ctr" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
          <a:p>
            <a:pPr algn="{}"/>
            <a:r>
              <a:rPr lang="en-US" sz="{}">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                if matches!(slide.layout, SlideLayout::Title | SlideLayout::Section) { "ctr" } else { "l" },
                2400,
                ppt_theme.subtitle,
                escape_xml(subtitle)
            ));
            shape_id += 1;
        }

        // Elements
        for element in &slide.elements {
            let (x, y, w, h) = if let Some(pos) = &element.position {
                (pos.x, pos.y, pos.w, pos.h)
            } else {
                (5.0, 25.0, 90.0, 65.0)
            };

            let mut content_xml = String::new();
            match &element.content {
                ElementContent::Bullets(bullets) => {
                    for bullet in bullets {
                        content_xml.push_str(&format!(
                            r#"          <a:p>
            <a:pPr lvl="0">
              <a:buFont typeface="Arial"/>
              <a:buChar char="•"/>
            </a:pPr>
            <a:r>
              <a:rPr lang="en-US" sz="1800">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
"#,
                            ppt_theme.text,
                            escape_xml(bullet)
                        ));
                    }
                }
                ElementContent::Text(t) => {
                    content_xml.push_str(&format!(
                        r#"          <a:p>
            <a:r>
              <a:rPr lang="en-US" sz="1800">
                <a:solidFill><a:srgbClr val="{}"/></a:solidFill>
              </a:rPr>
              <a:t>{}</a:t>
            </a:r>
          </a:p>
"#,
                        ppt_theme.text,
                        escape_xml(t)
                    ));
                }
            }

            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Content"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm>
            <a:off x="{}" y="{}"/>
            <a:ext cx="{}" cy="{}"/>
          </a:xfrm>
        </p:spPr>
        <p:txBody>
          <a:bodyPr anchor="t" wrap="square"><a:spAutoFit/></a:bodyPr>
          <a:lstStyle/>
{}        </p:txBody>
      </p:sp>
"#,
                shape_id,
                to_emu(x, SLIDE_WIDTH), to_emu(y, SLIDE_HEIGHT),
                to_emu(w, SLIDE_WIDTH), to_emu(h, SLIDE_HEIGHT),
                content_xml
            ));
            shape_id += 1;
        }

        slide_xml.push_str(
            r#"    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
        );

        zip.start_file(format!("ppt/slides/slide{}.xml", slide_num), options)
            .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        zip.write_all(slide_xml.as_bytes())
            .map_err(TandemError::Io)?;

        // === Slide relationship file (critical for Google Slides) ===
        zip.start_file(
            format!("ppt/slides/_rels/slide{}.xml.rels", slide_num),
            options,
        )
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        let slide_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
        zip.write_all(slide_rels.as_bytes())
            .map_err(TandemError::Io)?;
    }

    // === Minimal slideMaster (required) ===
    let slide_master = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
  <p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;

    zip.start_file("ppt/slideMasters/slideMaster1.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(slide_master.as_bytes())
        .map_err(TandemError::Io)?;

    zip.start_file("ppt/slideMasters/_rels/slideMaster1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let master_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
    zip.write_all(master_rels.as_bytes())
        .map_err(TandemError::Io)?;

    let slide_layout = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
  <p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;

    zip.start_file("ppt/slideLayouts/slideLayout1.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(slide_layout.as_bytes())
        .map_err(TandemError::Io)?;

    zip.start_file("ppt/slideLayouts/_rels/slideLayout1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let layout_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#;
    zip.write_all(layout_rels.as_bytes())
        .map_err(TandemError::Io)?;

    zip.finish()
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;

    tracing::info!("Successfully exported presentation to {}", output_path);
    Ok(format!("Exported to {}", output_path))
}

// ============================================================================
// File Browser Commands
// ============================================================================

/// File entry information for directory listings
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub extension: Option<String>,
}

/// Read directory contents with gitignore support
#[tauri::command]
pub async fn read_directory(_state: State<'_, AppState>, path: String) -> Result<Vec<FileEntry>> {
    use ignore::WalkBuilder;

    let dir_path = PathBuf::from(&path);

    if !dir_path.exists() {
        return Err(TandemError::NotFound(format!(
            "Path does not exist: {}",
            path
        )));
    }

    if !dir_path.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let mut entries = Vec::new();

    // Use ignore crate to respect .gitignore
    let walker = WalkBuilder::new(&dir_path)
        .max_depth(Some(1)) // Only immediate children
        .hidden(false) // Show hidden files
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let entry_path = entry.path();

                // Skip the directory itself
                if entry_path == dir_path {
                    continue;
                }

                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Failed to read metadata for {:?}: {}", entry_path, e);
                        continue;
                    }
                };

                let name = entry_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let path_str = entry_path.to_string_lossy().to_string();
                let is_directory = metadata.is_dir();
                let size = if is_directory {
                    None
                } else {
                    Some(metadata.len())
                };
                let extension = if is_directory {
                    None
                } else {
                    entry_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_string())
                };

                entries.push(FileEntry {
                    name,
                    path: path_str,
                    is_directory,
                    size,
                    extension,
                });
            }
            Err(e) => {
                tracing::warn!("Error walking directory: {}", e);
            }
        }
    }

    // Sort: directories first, then files, alphabetically
    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

/// Read file content with size limit
#[tauri::command]
pub async fn read_file_content(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
) -> Result<String> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(TandemError::NotFound(format!(
            "File does not exist: {}",
            path
        )));
    }

    if !file_path.is_file() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a file: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let metadata = fs::metadata(&file_path).map_err(TandemError::Io)?;

    let file_size = metadata.len();
    let size_limit = max_size.unwrap_or(1024 * 1024); // Default 1MB

    if file_size > size_limit {
        return Err(TandemError::InvalidConfig(format!(
            "File too large: {} bytes (limit: {} bytes)",
            file_size, size_limit
        )));
    }

    let content = fs::read_to_string(&file_path).map_err(TandemError::Io)?;

    Ok(content)
}

/// Read a binary file and return it as base64
#[tauri::command]
pub fn read_binary_file(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(TandemError::NotFound(format!(
            "File does not exist: {}",
            path
        )));
    }

    if !file_path.is_file() {
        return Err(TandemError::InvalidConfig(format!(
            "Path is not a file: {}",
            path
        )));
    }

    // Note: Path allowlist check removed - was causing Windows path normalization issues

    let metadata = fs::metadata(&file_path).map_err(TandemError::Io)?;
    let file_size = metadata.len();
    let size_limit = max_size.unwrap_or(10 * 1024 * 1024);

    if file_size > size_limit {
        return Err(TandemError::InvalidConfig(format!(
            "File too large: {} bytes (limit: {} bytes)",
            file_size, size_limit
        )));
    }

    let bytes = fs::read(&file_path).map_err(TandemError::Io)?;
    Ok(STANDARD.encode(&bytes))
}

/// Read a file as text, with best-effort extraction for common document formats.
///
/// Supported extraction (pure Rust):
/// - PDF: `.pdf`
/// - Word: `.docx`
/// - PowerPoint: `.pptx`
/// - Excel: `.xlsx`, `.xls`, `.ods`, `.xlsb`
/// - Rich Text: `.rtf`
///
/// All other file types fall back to UTF-8 text reading.
#[tauri::command]
pub async fn read_file_text(
    _state: State<'_, AppState>,
    path: String,
    max_size: Option<u64>,
    max_chars: Option<usize>,
) -> Result<String> {
    let file_path = PathBuf::from(&path);

    let mut limits = crate::document_text::ExtractLimits::default();
    if let Some(max_size) = max_size {
        limits.max_file_bytes = max_size;
    }
    if let Some(max_chars) = max_chars {
        limits.max_output_chars = max_chars;
    }

    crate::document_text::extract_file_text(&file_path, limits)
}

// ============================================================================
// Python Environment (Workspace Venv Wizard)
// ============================================================================

#[tauri::command]
pub async fn python_get_status(state: State<'_, AppState>) -> Result<python_env::PythonStatus> {
    let ws = state.get_workspace_path();
    Ok(python_env::get_status(ws.as_deref()))
}

#[tauri::command]
pub async fn python_create_venv(
    state: State<'_, AppState>,
    selected: Option<String>,
) -> Result<python_env::PythonStatus> {
    let ws = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    tokio::task::spawn_blocking(move || python_env::create_venv(&ws, selected))
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to create venv: {}", e)))?
}

#[tauri::command]
pub async fn python_install_requirements(
    state: State<'_, AppState>,
    requirements_path: String,
) -> Result<python_env::PythonInstallResult> {
    let ws = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    let req_path = PathBuf::from(&requirements_path);
    if !state.is_path_allowed(&req_path) {
        return Err(TandemError::PermissionDenied(format!(
            "Requirements path is outside the allowed workspace: {}",
            requirements_path
        )));
    }

    tokio::task::spawn_blocking(move || python_env::install_requirements(&ws, &req_path))
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to install requirements: {}", e)))?
}

// ============================================================================
// Skills Management Commands
// ============================================================================

fn import_skill_from_content(
    state: &State<'_, AppState>,
    content: &str,
    location: crate::skills::SkillLocation,
) -> Result<crate::skills::SkillInfo> {
    // Parse content to get name
    let (name, description, _body) = crate::skills::parse_skill_content(content)?;

    // Determine target directory
    let target_dir = match location {
        crate::skills::SkillLocation::Project => {
            let ws = state.workspace_path.read().unwrap();
            let workspace = ws
                .as_ref()
                .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;
            workspace.join(".opencode").join("skill").join(&name)
        }
        crate::skills::SkillLocation::Global => {
            let config_dir = dirs::config_dir()
                .ok_or_else(|| TandemError::InvalidConfig("No config directory".to_string()))?;
            config_dir.join("opencode").join("skills").join(&name)
        }
    };

    // Create directory and write file
    fs::create_dir_all(&target_dir).map_err(TandemError::Io)?;

    // Write original content directly (don't reconstruct to avoid formatting issues)
    fs::write(target_dir.join("SKILL.md"), content).map_err(TandemError::Io)?;

    tracing::info!("Imported skill '{}' to {:?}", name, location);

    Ok(crate::skills::SkillInfo {
        name,
        description,
        location,
        path: target_dir.to_string_lossy().to_string(),
    })
}

/// List all installed skills
#[tauri::command]
pub fn list_skills(state: State<'_, AppState>) -> Result<Vec<crate::skills::SkillInfo>> {
    let workspace = state.workspace_path.read().unwrap();
    let workspace_str = workspace.as_ref().map(|p| p.to_str().unwrap());

    tracing::info!("Listing skills for workspace: {:?}", workspace_str);
    let skills = crate::skills::discover_skills(workspace_str);
    tracing::info!("Found {} skills", skills.len());
    for skill in &skills {
        tracing::info!("  - {} ({:?}): {}", skill.name, skill.location, skill.path);
    }

    Ok(skills)
}

/// Import a skill from raw SKILL.md content
#[tauri::command]
pub fn import_skill(
    state: State<'_, AppState>,
    content: String,
    location: crate::skills::SkillLocation,
) -> Result<crate::skills::SkillInfo> {
    import_skill_from_content(&state, &content, location)
}

/// Delete a skill
#[tauri::command]
pub fn delete_skill(
    state: State<'_, AppState>,
    name: String,
    location: crate::skills::SkillLocation,
) -> Result<()> {
    let target_dir = match location {
        crate::skills::SkillLocation::Project => {
            let ws = state.workspace_path.read().unwrap();
            let workspace = ws
                .as_ref()
                .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;
            workspace.join(".opencode").join("skill").join(&name)
        }
        crate::skills::SkillLocation::Global => {
            let config_dir = dirs::config_dir()
                .ok_or_else(|| TandemError::InvalidConfig("No config directory".to_string()))?;
            config_dir.join("opencode").join("skills").join(&name)
        }
    };

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(TandemError::Io)?;
        tracing::info!("Deleted skill '{}' from {:?}", name, location);
    }

    Ok(())
}

// ============================================================================
// Starter Skill Templates (offline)
// ============================================================================

#[tauri::command]
pub fn skills_list_templates(
    app: AppHandle,
) -> Result<Vec<crate::skill_templates::SkillTemplateInfo>> {
    crate::skill_templates::list_skill_templates(&app).map_err(TandemError::InvalidConfig)
}

#[tauri::command]
pub fn skills_install_template(
    state: State<'_, AppState>,
    app: AppHandle,
    template_id: String,
    location: crate::skills::SkillLocation,
) -> Result<crate::skills::SkillInfo> {
    let content = crate::skill_templates::read_skill_template_content(&app, &template_id)
        .map_err(TandemError::InvalidConfig)?;
    import_skill_from_content(&state, &content, location)
}

// ============================================================================
// OpenCode: Plugins + MCP Config Commands
// ============================================================================

/// List configured OpenCode plugins for the given scope.
#[tauri::command]
pub fn opencode_list_plugins(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());
    let path = crate::opencode_config::get_config_path(scope, ws)?;

    let cfg = crate::opencode_config::read_config(&path)?;
    let plugins = cfg
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new);

    Ok(plugins)
}

/// Add a plugin to OpenCode config for the given scope (idempotent).
#[tauri::command]
pub fn opencode_add_plugin(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
    name: String,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());

    let updated = crate::opencode_config::update_config(scope, ws, |cfg| {
        crate::opencode_config::ensure_schema(cfg);

        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;

        let entry = root
            .entry("plugin".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));

        // Normalize non-array values.
        if !entry.is_array() {
            *entry = serde_json::Value::Array(Vec::new());
        }

        let arr = entry.as_array_mut().unwrap();
        let already = arr.iter().any(|v| v.as_str() == Some(name.as_str()));
        if !already {
            arr.push(serde_json::Value::String(name.clone()));
        }
        Ok(())
    })?;

    Ok(updated
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new))
}

/// Remove a plugin from OpenCode config for the given scope.
#[tauri::command]
pub fn opencode_remove_plugin(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
    name: String,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());

    let updated = crate::opencode_config::update_config(scope, ws, |cfg| {
        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        if let Some(v) = root.get_mut("plugin") {
            if let Some(arr) = v.as_array_mut() {
                arr.retain(|p| p.as_str() != Some(name.as_str()));
            }
        }
        Ok(())
    })?;

    Ok(updated
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpencodeMcpServerEntry {
    pub name: String,
    pub config: serde_json::Value,
}

/// List configured MCP servers for the given scope.
#[tauri::command]
pub fn opencode_list_mcp_servers(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());
    let path = crate::opencode_config::get_config_path(scope, ws)?;

    let cfg = crate::opencode_config::read_config(&path)?;
    let mut out: Vec<OpencodeMcpServerEntry> = Vec::new();

    if let Some(mcp) = cfg.get("mcp").and_then(|v| v.as_object()) {
        for (name, config) in mcp {
            out.push(OpencodeMcpServerEntry {
                name: name.clone(),
                config: config.clone(),
            });
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Add or update an MCP server config for the given scope.
#[tauri::command]
pub fn opencode_add_mcp_server(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
    name: String,
    config: serde_json::Value,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());

    crate::opencode_config::update_config(scope, ws, |cfg| {
        crate::opencode_config::ensure_schema(cfg);

        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        let mcp_val = root
            .entry("mcp".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !mcp_val.is_object() {
            *mcp_val = serde_json::Value::Object(serde_json::Map::new());
        }
        let mcp_obj = mcp_val.as_object_mut().unwrap();
        mcp_obj.insert(name.clone(), config.clone());
        Ok(())
    })?;

    opencode_list_mcp_servers(state, scope)
}

/// Remove an MCP server config for the given scope.
#[tauri::command]
pub fn opencode_remove_mcp_server(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
    name: String,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());

    crate::opencode_config::update_config(scope, ws, |cfg| {
        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        if let Some(mcp_val) = root.get_mut("mcp") {
            if let Some(mcp_obj) = mcp_val.as_object_mut() {
                mcp_obj.remove(&name);
            }
        }
        Ok(())
    })?;

    opencode_list_mcp_servers(state, scope)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpencodeMcpTestResult {
    // "connected" | "auth_required" | "wrong_url" | "wrong_method" | "gone" | "unreachable"
    // | "failed" | "invalid_response" | "not_supported" | "not_found"
    pub status: String,
    pub ok: bool,
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

/// Best-effort connectivity probe for MCP servers (HTTP only).
#[tauri::command]
pub async fn opencode_test_mcp_connection(
    state: State<'_, AppState>,
    scope: crate::opencode_config::OpenCodeConfigScope,
    name: String,
) -> Result<OpencodeMcpTestResult> {
    use futures::StreamExt;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
    use std::time::Duration;

    let workspace = state.get_workspace_path();
    let ws = workspace.as_ref().map(|p| p.as_path());
    let path = crate::opencode_config::get_config_path(scope, ws)?;
    let cfg = crate::opencode_config::read_config(&path)?;

    let server = match cfg
        .get("mcp")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get(&name))
    {
        Some(v) => v,
        None => {
            return Ok(OpencodeMcpTestResult {
                status: "not_found".to_string(),
                ok: false,
                http_status: None,
                error: None,
            })
        }
    };

    let server_type = server
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if server_type != "remote" {
        return Ok(OpencodeMcpTestResult {
            status: "not_supported".to_string(),
            ok: false,
            http_status: None,
            error: None,
        });
    }

    let url = server
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TandemError::InvalidConfig("Remote MCP server missing 'url'".into()))?;

    // MCP protocol version used for the `initialize` handshake.
    // This should track the MCP spec date-version.
    const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

    let debug_enabled = std::env::var("TANDEM_MCP_DEBUG")
        .ok()
        .is_some_and(|v| v != "0" && !v.is_empty());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| TandemError::Sidecar(format!("Failed to build HTTP client: {}", e)))?;

    // Build request headers (defaults + user-provided).
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(arr) = server.get("headers").and_then(|v| v.as_array()) {
        for h in arr {
            let Some(line) = h.as_str() else { continue };
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() {
                continue;
            }
            let Ok(hn) = HeaderName::from_bytes(name.as_bytes()) else {
                continue;
            };
            let Ok(hv) = HeaderValue::from_str(value) else {
                continue;
            };
            headers.insert(hn, hv);
        }
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "tandem",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    });
    let body_bytes = serde_json::to_vec(&body).map_err(TandemError::Serialization)?;

    if debug_enabled {
        let mut header_lines: Vec<String> = Vec::new();
        for (k, v) in headers.iter() {
            let key = k.as_str();
            let key_lc = key.to_ascii_lowercase();
            let val = if key_lc == "authorization"
                || key_lc == "proxy-authorization"
                || key_lc.contains("api-key")
                || key_lc.contains("apikey")
                || key_lc.contains("token")
            {
                "<redacted>".to_string()
            } else {
                v.to_str().unwrap_or("<binary>").to_string()
            };
            header_lines.push(format!("{}: {}", key, val));
        }
        let body_preview = String::from_utf8_lossy(&body_bytes)
            .chars()
            .take(2048)
            .collect::<String>();
        tracing::info!(
            "[mcp-test] POST {} headers=[{}] body={}",
            url,
            header_lines.join(", "),
            body_preview
        );
    }

    let resp = client
        .post(url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let http_status = r.status().as_u16();
            let resp_headers = r.headers().clone();
            let content_type = r
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if debug_enabled {
                let mut header_lines: Vec<String> = Vec::new();
                for (k, v) in resp_headers.iter() {
                    let key = k.as_str();
                    let key_lc = key.to_ascii_lowercase();
                    let val = if key_lc == "set-cookie"
                        || key_lc == "authorization"
                        || key_lc == "proxy-authorization"
                        || key_lc.contains("api-key")
                        || key_lc.contains("apikey")
                        || key_lc.contains("token")
                    {
                        "<redacted>".to_string()
                    } else {
                        v.to_str().unwrap_or("<binary>").to_string()
                    };
                    header_lines.push(format!("{}: {}", key, val));
                }
                tracing::info!("[mcp-test] response headers=[{}]", header_lines.join(", "));
            }

            // Best-effort read of SSE response bodies that may not terminate.
            async fn read_sse_first_event_data_json(
                resp: reqwest::Response,
                max_bytes: usize,
            ) -> std::result::Result<(String, serde_json::Value), String> {
                let mut buf: Vec<u8> = Vec::new();
                let mut stream = resp.bytes_stream();

                while let Some(next) = stream.next().await {
                    let chunk = next.map_err(|e| e.to_string())?;
                    buf.extend_from_slice(&chunk);
                    if buf.len() > max_bytes {
                        break;
                    }

                    // Find end of first SSE event.
                    let event_end = buf
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|i| (i, 4))
                        .or_else(|| buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)));

                    let Some((idx, sep_len)) = event_end else {
                        continue;
                    };

                    let event_str = String::from_utf8_lossy(&buf[..idx]);
                    let mut data_lines: Vec<&str> = Vec::new();
                    for line in event_str.lines() {
                        let line = line.trim_end_matches('\r');
                        if let Some(rest) = line.strip_prefix("data:") {
                            data_lines.push(rest.trim_start());
                        }
                    }

                    if !data_lines.is_empty() {
                        let data = data_lines.join("\n");
                        let json: serde_json::Value =
                            serde_json::from_str(&data).map_err(|e| e.to_string())?;

                        let snippet = String::from_utf8_lossy(&buf[..idx + sep_len])
                            .chars()
                            .take(2048)
                            .collect::<String>();
                        return Ok((snippet, json));
                    }

                    // No data lines; continue reading (but avoid unbounded memory).
                    if buf.len() > max_bytes / 2 {
                        buf.drain(..idx + sep_len);
                    }
                }

                let snippet = String::from_utf8_lossy(&buf)
                    .chars()
                    .take(2048)
                    .collect::<String>();
                Err(format!(
                    "SSE response did not include a JSON data event within {} bytes. Snippet: {}",
                    max_bytes, snippet
                ))
            }

            // Read body (JSON or SSE) so we can provide actionable feedback.
            let (body_snippet, json, parse_err) =
                if http_status == 200 && content_type.starts_with("text/event-stream") {
                    match read_sse_first_event_data_json(r, 64 * 1024).await {
                        Ok((snippet, json)) => (Some(snippet), Some(json), None),
                        Err(e) => (None, None, Some(e)),
                    }
                } else {
                    // Small bodies are expected for `initialize`; safe to read fully.
                    let text = r.text().await.unwrap_or_default();
                    let snippet = text.chars().take(2048).collect::<String>();
                    let json = serde_json::from_str::<serde_json::Value>(&text).ok();
                    (Some(snippet), json, None)
                };

            if debug_enabled {
                let snippet = body_snippet.clone().unwrap_or_default();
                tracing::info!(
                    "[mcp-test] response status={} content-type={} snippet={}",
                    http_status,
                    content_type,
                    snippet
                );
            }

            // Status mapping (protocol-aware)
            match http_status {
                200 => {
                    let Some(v) = json else {
                        return Ok(OpencodeMcpTestResult {
                            status: "invalid_response".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(
                                parse_err.unwrap_or_else(|| {
                                    "Server returned 200 but response was not valid JSON-RPC."
                                        .into()
                                }),
                            ),
                        });
                    };

                    let ok_jsonrpc = v
                        .get("jsonrpc")
                        .and_then(|x| x.as_str())
                        .is_some_and(|s| s == "2.0");

                    if ok_jsonrpc && v.get("result").is_some() {
                        Ok(OpencodeMcpTestResult {
                            status: "connected".to_string(),
                            ok: true,
                            http_status: Some(http_status),
                            error: None,
                        })
                    } else if ok_jsonrpc && v.get("error").is_some() {
                        let msg = v
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("MCP server returned an error");
                        Ok(OpencodeMcpTestResult {
                            status: "failed".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(msg.to_string()),
                        })
                    } else {
                        Ok(OpencodeMcpTestResult {
                            status: "invalid_response".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(
                                "Server returned 200 but response did not look like JSON-RPC 2.0."
                                    .into(),
                            ),
                        })
                    }
                }
                401 | 403 => Ok(OpencodeMcpTestResult {
                    status: "auth_required".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some("Authentication required. Add an Authorization header or API key.".into()),
                }),
                404 => Ok(OpencodeMcpTestResult {
                    status: "wrong_url".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some("Endpoint not found (404). Check the URL/path.".into()),
                }),
                405 => Ok(OpencodeMcpTestResult {
                    status: "wrong_method".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some(
                        "Method not allowed (405). This endpoint may require a different MCP transport or path."
                            .into(),
                    ),
                }),
                406 => Ok(OpencodeMcpTestResult {
                    status: "wrong_method".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some(
                        "Not acceptable (406). Some MCP servers require Accept: application/json, text/event-stream."
                            .into(),
                    ),
                }),
                410 => {
                    let hint = if url.contains("/sse") {
                        "Endpoint is gone (410). DeepWiki deprecated /sse; use https://mcp.deepwiki.com/mcp instead."
                    } else {
                        "Endpoint is gone (410). The server may have deprecated this URL."
                    };
                    Ok(OpencodeMcpTestResult {
                        status: "gone".to_string(),
                        ok: false,
                        http_status: Some(http_status),
                        error: Some(hint.into()),
                    })
                }
                _ => Ok(OpencodeMcpTestResult {
                    status: "failed".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: body_snippet
                        .filter(|s| !s.is_empty())
                        .or_else(|| Some(format!("HTTP {}", http_status))),
                }),
            }
        }
        Err(e) => Ok(OpencodeMcpTestResult {
            status: "unreachable".to_string(),
            ok: false,
            http_status: None,
            error: Some(e.to_string()),
        }),
    }
}

// ============================================================================
// Plan Management Commands
// ============================================================================

/// Information about a plan file
#[derive(serde::Serialize, Clone)]
pub struct PlanInfo {
    /// Session name (parent directory name)
    pub session_name: String,
    /// File name (e.g., "PLAN_jwt_tokens.md")
    pub file_name: String,
    /// Full absolute path to the plan file
    pub full_path: String,
    /// Last modified timestamp (Unix timestamp in milliseconds)
    pub last_modified: u64,
}

/// List all plan files in the workspace, grouped by session
#[tauri::command]
pub fn list_plans(state: State<'_, AppState>) -> Result<Vec<PlanInfo>> {
    let workspace = state.workspace_path.read().unwrap();
    let workspace_path = workspace
        .as_ref()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    let plans_dir = workspace_path.join(".opencode").join("plans");

    // Create plans directory if it doesn't exist
    if !plans_dir.exists() {
        tracing::debug!(
            "[list_plans] Plans directory doesn't exist, creating: {:?}",
            plans_dir
        );
        fs::create_dir_all(&plans_dir).map_err(TandemError::Io)?;
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();

    // Recursively scan for .md files in session subdirectories
    for session_entry in fs::read_dir(&plans_dir).map_err(TandemError::Io)? {
        let session_entry = session_entry.map_err(TandemError::Io)?;
        let session_path = session_entry.path();

        // Skip if not a directory
        if !session_path.is_dir() {
            continue;
        }

        let session_name = session_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Scan for plan files in this session directory
        for plan_entry in fs::read_dir(&session_path).map_err(TandemError::Io)? {
            let plan_entry = plan_entry.map_err(TandemError::Io)?;
            let plan_path = plan_entry.path();

            // Only include .md files that start with "PLAN_"
            if let Some(file_name) = plan_path.file_name().and_then(|n| n.to_str()) {
                if file_name.ends_with(".md") && file_name.starts_with("PLAN_") {
                    let metadata = fs::metadata(&plan_path).map_err(TandemError::Io)?;
                    let last_modified = metadata
                        .modified()
                        .map_err(TandemError::Io)?
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    plans.push(PlanInfo {
                        session_name: session_name.clone(),
                        file_name: file_name.to_string(),
                        full_path: plan_path.to_string_lossy().to_string(),
                        last_modified,
                    });
                }
            }
        }
    }

    // Sort by last modified (newest first)
    plans.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    tracing::debug!("[list_plans] Found {} plans", plans.len());
    Ok(plans)
}

/// Read the content of a plan file
#[tauri::command]
pub fn read_plan_content(plan_path: String) -> Result<String> {
    let path = PathBuf::from(&plan_path);

    // Security check: ensure the path is within .opencode/plans/
    if !path.components().any(|c| c.as_os_str() == ".opencode") {
        return Err(TandemError::InvalidConfig(
            "Plan path must be within .opencode/plans/".to_string(),
        ));
    }

    fs::read_to_string(&path).map_err(|e| TandemError::Io(e))
}

/// Result of starting a plan session
#[derive(serde::Serialize)]
pub struct PlanSessionResult {
    pub session: Session,
    pub plan_path: String,
}

/// Start a new planning session with a guaranteed pre-created plan file
#[tauri::command]
pub async fn start_plan_session(
    _app: AppHandle,
    state: State<'_, AppState>,
    goal: Option<String>,
) -> Result<PlanSessionResult> {
    // 1. Generate Session ID and Plan Name
    let session_id = Uuid::new_v4().to_string();

    // If goal is provided, sanitize it for the filename. Otherwise use "draft".
    let plan_name = if let Some(g) = goal.as_ref() {
        let sanitized: String = g
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        format!("PLAN_{}", sanitized)
    } else {
        "PLAN_draft".to_string()
    };

    // 2. Prepare Directory structure: .opencode/plans/{session_id}/
    // We use session_id for the folder to ensure uniqueness and "frictionless" start (no name collision)
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No workspace selected".to_string()))?;

    let plans_dir = PathBuf::from(workspace_path)
        .join(".opencode")
        .join("plans")
        .join(&session_id);
    let plan_file_path = plans_dir.join(format!("{}.md", plan_name));

    // 3. Pre-create the file
    fs::create_dir_all(&plans_dir).map_err(|e| TandemError::Io(e))?;

    let template = format!(
        "# Plan: {}\n\n## Goal\n{}\n\n## Proposed Changes\n- [ ] Analyze requirements\n- [ ] Design solution\n\n## Verification\n- [ ] Test case 1",
        goal.as_deref().unwrap_or("Draft Plan"),
        goal.as_deref().unwrap_or("Describe the goal here")
    );

    fs::write(&plan_file_path, template).map_err(|e| TandemError::Io(e))?;

    let absolute_path = plan_file_path.to_string_lossy().to_string();
    tracing::info!("Pre-created plan file at: {}", absolute_path);

    // 4. Create Sidecar Session
    // We explicitly instruct the AI about the file we just made via the title or a follow-up message.
    // Ideally we would inject a system prompt, but we'll handle that by ensuring the frontend
    // or the "System" recognizes this session type.
    // For now, we create the session with a specific Title that hints at the plan.

    let (default_provider, default_model) = {
        let config = state.providers_config.read().unwrap();
        crate::commands::resolve_default_provider_and_model(&config)
    };

    let session = state
        .sidecar
        .create_session(CreateSessionRequest {
            parent_id: None,
            title: Some(goal.clone().unwrap_or_else(|| "Plan Mode".to_string())),
            model: default_model,
            provider: default_provider,
            permission: None,
        })
        .await?;

    // 5. Inject System Directive (as a user message, since we can't set system role easily)
    // This ensures the AI context is primed with the file path immediately.
    let system_directive = format!(
        "SYSTEM NOTE: A dedicated plan file has been pre-created at:\n`{}`\n\nYour GOAL is: \"{}\".\n\nYour FIRST action MUST be to use the `write_file` tool to update this exact file. Do not create a new plan file. Edit this one directly.",
        absolute_path.replace("\\", "/"),
        goal.as_deref().unwrap_or("Draft a new plan")
    );

    // We fire-and-forget this message so the frontend doesn't hang waiting for a response (though it returns quickly)
    // Actually, we should wait to ensure it's in history before the user chats.
    let request = SendMessageRequest::text(system_directive);

    // We ignore the result of the message send itself, as long as session exists
    if let Err(e) = state.sidecar.send_message(&session.id, request).await {
        tracing::warn!("Failed to inject plan directive: {}", e);
    } else {
        tracing::info!("Injected plan directive into session {}", session.id);
    }

    Ok(PlanSessionResult {
        session,
        plan_path: absolute_path,
    })
}

// ============================================================================
// Ralph Loop Commands
// ============================================================================

use crate::ralph::types::{IterationRecord, RalphConfig, RalphStateSnapshot};

/// Start a new Ralph Loop
#[tauri::command]
pub async fn ralph_start(
    state: State<'_, AppState>,
    session_id: String,
    prompt: String,
    config: Option<RalphConfig>,
) -> Result<String> {
    let workspace_path = {
        let workspace = state.workspace_path.read().unwrap();
        workspace
            .as_ref()
            .ok_or_else(|| TandemError::Ralph("No workspace set".to_string()))?
            .clone()
    };

    let config = config.unwrap_or_default();
    let run_id = state
        .ralph_manager
        .start(
            session_id,
            prompt,
            config,
            workspace_path,
            state.sidecar.clone(),
        )
        .await?;

    Ok(run_id)
}

/// Cancel a running Ralph Loop
#[tauri::command]
pub async fn ralph_cancel(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.cancel(&run_id).await
}

/// Pause a running Ralph Loop
#[tauri::command]
pub async fn ralph_pause(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.pause(&run_id).await
}

/// Resume a paused Ralph Loop
#[tauri::command]
pub async fn ralph_resume(state: State<'_, AppState>, run_id: String) -> Result<()> {
    state.ralph_manager.resume(&run_id).await
}

/// Add context to be injected in next iteration
#[tauri::command]
pub async fn ralph_add_context(
    state: State<'_, AppState>,
    run_id: String,
    text: String,
) -> Result<()> {
    state.ralph_manager.add_context(&run_id, text).await
}

/// Get current Ralph Loop status
#[tauri::command]
pub async fn ralph_status(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RalphStateSnapshot> {
    state.ralph_manager.status(&run_id).await
}

/// Get Ralph Loop iteration history
#[tauri::command]
pub async fn ralph_history(
    state: State<'_, AppState>,
    run_id: String,
    limit: Option<usize>,
) -> Result<Vec<IterationRecord>> {
    state
        .ralph_manager
        .history(&run_id, limit.unwrap_or(50))
        .await
}
// ============================================================================
// Orchestrator Commands
// ============================================================================

#[derive(Debug, serde::Serialize)]
pub struct OrchestratorModelSelection {
    pub model: Option<String>,
    pub provider: Option<String>,
}

fn orchestrator_permission_rules() -> Vec<crate::sidecar::PermissionRule> {
    vec![
        crate::sidecar::PermissionRule {
            permission: "ls".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "read".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "todowrite".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "websearch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "webfetch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "glob".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "grep".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
        crate::sidecar::PermissionRule {
            permission: "search".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        },
    ]
}

fn normalize_provider_id_for_sidecar(provider: Option<String>) -> Option<String> {
    provider.map(|p| {
        if p == "opencode_zen" {
            "opencode".to_string()
        } else {
            p
        }
    })
}

/// Create a new orchestration run
#[tauri::command]
pub async fn orchestrator_create_run(
    app: AppHandle,
    state: State<'_, AppState>,
    objective: String,
    config: OrchestratorConfig,
    model: Option<String>,
    provider: Option<String>,
) -> Result<String> {
    use crate::sidecar::CreateSessionRequest;

    let run_id = Uuid::new_v4().to_string();

    // Get default provider/model (same as chat sessions)
    let (default_provider, default_model) = {
        let config = state.providers_config.read().unwrap();
        resolve_default_provider_and_model(&config)
    };

    // Use provided values or fallback to defaults
    let final_model = model.or(default_model);
    let final_provider = normalize_provider_id_for_sidecar(provider.or(default_provider));

    // Create a NEW session specifically for the orchestrator
    let session_request = CreateSessionRequest {
        parent_id: None,
        title: Some(format!(
            "Orchestrator: {}",
            &objective[..objective.len().min(50)]
        )),
        // Clone so we can also persist the selection onto the Run object below.
        model: final_model.clone(),
        provider: final_provider.clone(),
        permission: Some(orchestrator_permission_rules()),
    };

    let session = state
        .sidecar
        .create_session(session_request)
        .await
        .map_err(|e| {
            TandemError::Sidecar(format!("Failed to create orchestrator session: {}", e))
        })?;

    let session_id = session.id;
    tracing::info!(
        "Created orchestrator session: {} with model: {:?}, provider: {:?}",
        session_id,
        session.model,
        session.provider
    );

    // Guard against old UI defaults when creating new runs.
    let mut config = config;
    if config.max_iterations == 10 || config.max_iterations == 30 || config.max_iterations == 200 {
        config.max_iterations = 500;
    }
    if config.max_subagent_runs == 20
        || config.max_subagent_runs == 50
        || config.max_subagent_runs == 500
    {
        config.max_subagent_runs = 2000;
    }
    if config.max_wall_time_secs == 20 * 60 {
        config.max_wall_time_secs = 60 * 60;
    }

    // Create the run object
    let mut run = Run::new(run_id.clone(), session_id, objective, config);
    // Persist model/provider selection into the run so the orchestrator can always send explicit
    // model specs even if the sidecar session object doesn't echo them back.
    run.model = final_model.clone();
    run.provider = final_provider.clone();

    // Initialize dependencies
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No workspace selected".to_string()))?;

    let policy_config = PolicyConfig::new(workspace_path.clone());
    let policy = PolicyEngine::new(policy_config);
    let store = OrchestratorStore::new(&workspace_path)?;

    // Channel for events
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Create engine
    let engine = Arc::new(OrchestratorEngine::new(
        run,
        policy,
        store,
        state.sidecar.clone(),
        workspace_path,
        event_tx,
    ));

    // Store engine in state
    {
        let mut engines = state.orchestrator_engines.write().unwrap();
        engines.insert(run_id.clone(), engine.clone());
    }

    // Spawn event forwarder
    let app_handle = app.clone();
    let run_id_clone = run_id.clone();

    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            // Emit to frontend
            if let Err(e) = app_handle.emit("orchestrator-event", event) {
                tracing::error!("Failed to emit orchestrator event: {}", e);
            }
        }
        tracing::info!("Orchestrator event loop ended for run {}", run_id_clone);
    });

    tracing::info!("Created orchestrator run: {}", run_id);
    Ok(run_id)
}

/// Start the planning phase
#[tauri::command]
pub async fn orchestrator_start(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    // Spawn execution to avoid blocking the command
    tauri::async_runtime::spawn(async move {
        if let Err(e) = engine.start().await {
            tracing::error!("Orchestrator run {} failed to start: {}", run_id, e);
        }
    });

    Ok(())
}

/// Get the current status of a run
#[tauri::command]
pub async fn orchestrator_get_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RunSnapshot> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    Ok(engine.get_snapshot().await)
}

/// Get the current budget status
#[tauri::command]
pub async fn orchestrator_get_budget(state: State<'_, AppState>, run_id: String) -> Result<Budget> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    Ok(engine.get_budget().await)
}

/// Get the task list
#[tauri::command]
pub async fn orchestrator_list_tasks(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Vec<Task>> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    Ok(engine.get_tasks().await)
}

#[tauri::command]
pub async fn orchestrator_get_config(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<OrchestratorConfig> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    Ok(engine.get_config().await)
}

#[tauri::command]
pub async fn orchestrator_get_run_model(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<OrchestratorModelSelection> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    // Prefer the model/provider persisted on the run. Some OpenCode builds don't populate
    // legacy session.model/provider in GET /session responses.
    let (run_model, run_provider) = engine.get_run_model_provider().await;
    if run_model.is_some() || run_provider.is_some() {
        return Ok(OrchestratorModelSelection {
            model: run_model,
            provider: run_provider,
        });
    }

    // Fallback: ask sidecar for the session.
    let session_id = engine.get_base_session_id().await;
    let session = state.sidecar.get_session(&session_id).await?;

    Ok(OrchestratorModelSelection {
        model: session.model,
        provider: session.provider,
    })
}

#[tauri::command]
pub async fn orchestrator_set_resume_model(
    state: State<'_, AppState>,
    run_id: String,
    model: String,
    provider: String,
) -> Result<OrchestratorModelSelection> {
    use crate::sidecar::CreateSessionRequest;

    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    let snapshot = engine.get_snapshot().await;
    if snapshot.status != RunStatus::Paused
        && snapshot.status != RunStatus::Cancelled
        && snapshot.status != RunStatus::Failed
    {
        return Err(TandemError::InvalidOperation(
            "Run must be paused, failed, or cancelled to change model".to_string(),
        ));
    }

    let parent_id = engine.get_base_session_id().await;
    let normalized_provider = normalize_provider_id_for_sidecar(Some(provider.clone()));
    let request = CreateSessionRequest {
        parent_id: Some(parent_id),
        title: Some(format!(
            "Orchestrator Resume: {}",
            &snapshot.objective[..snapshot.objective.len().min(50)]
        )),
        model: Some(model.clone()),
        provider: normalized_provider.clone(),
        permission: Some(orchestrator_permission_rules()),
    };

    let session = state.sidecar.create_session(request).await?;

    engine
        .set_base_session_for_resume(session.id.clone())
        .await?;
    engine
        .set_run_model_provider(Some(model.clone()), normalized_provider.clone())
        .await;

    Ok(OrchestratorModelSelection {
        model: session.model.or(Some(model)),
        provider: session.provider.or(normalized_provider),
    })
}

/// Approve the plan and start execution
#[tauri::command]
pub async fn orchestrator_approve(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    // Execute in background
    tauri::async_runtime::spawn(async move {
        if let Err(e) = engine.approve().await {
            tracing::error!("Failed to approve run {}: {}", run_id, e);
        }
    });

    Ok(())
}

/// Request revision of the plan
#[tauri::command]
pub async fn orchestrator_request_revision(
    state: State<'_, AppState>,
    run_id: String,
    feedback: String,
) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    // Execute in background
    tauri::async_runtime::spawn(async move {
        if let Err(e) = engine.request_revision(feedback).await {
            tracing::error!("Failed to request revision for run {}: {}", run_id, e);
        }
    });

    Ok(())
}

/// Pause an executing run
#[tauri::command]
pub async fn orchestrator_pause(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    engine.pause().await;
    Ok(())
}

/// Resume a paused run
#[tauri::command]
pub async fn orchestrator_resume(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    engine.resume().await?;

    // Restart execution loop
    tauri::async_runtime::spawn(async move {
        if let Err(e) = engine.execute().await {
            tracing::error!("Failed to resume run {}: {}", run_id, e);
        }
    });

    Ok(())
}

/// Cancel a run
#[tauri::command]
pub async fn orchestrator_cancel(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    engine.cancel_and_finalize().await?;
    Ok(())
}

/// List all orchestrator runs (from disk)
#[tauri::command]
pub async fn orchestrator_list_runs(state: State<'_, AppState>) -> Result<Vec<RunSummary>> {
    // Get workspace path
    let workspace_path = {
        let path_guard = state.workspace_path.read().unwrap();
        path_guard.clone()
    };

    let workspace_path = match workspace_path {
        Some(p) => p,
        None => return Ok(Vec::new()), // No workspace, no runs
    };

    // List all run directories
    let runs_dir = workspace_path.join(".tandem").join("orchestrator");
    if !runs_dir.exists() {
        return Ok(Vec::new());
    }

    // Create store to access disk
    let store = OrchestratorStore::new(&workspace_path)?;

    let mut summaries = Vec::new();
    if let Ok(entries) = fs::read_dir(&runs_dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }

            let run_id = entry.file_name().to_string_lossy().to_string();

            // Try to load run from disk
            if let Ok(run) = store.load_run(&run_id) {
                summaries.push(RunSummary {
                    run_id: run.run_id,
                    session_id: run.session_id,
                    objective: run.objective,
                    status: run.status,
                    created_at: run.started_at,
                    updated_at: run.ended_at.unwrap_or_else(chrono::Utc::now),
                });
            }
        }
    }

    // Sort by updated_at descending (most recent first)
    summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(summaries)
}

/// Load a specific run from disk
#[tauri::command]
pub async fn orchestrator_load_run(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Run> {
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;

    let store = OrchestratorStore::new(&workspace_path)?;
    let run = store.load_run(&run_id)?;

    // Check if engine already exists in memory
    {
        let engines = state.orchestrator_engines.read().unwrap();
        if engines.contains_key(&run_id) {
            return Ok(run);
        }
    }

    // Re-hydrate engine
    let policy_config = PolicyConfig::new(workspace_path.clone());
    let policy = PolicyEngine::new(policy_config);

    // Channel for events
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Create engine
    // NOTE: When loading a run, we should try to update its config to the latest defaults
    // if the existing config seems to have the old low limits.
    // However, the `Run` struct loaded from disk has the OLD config.
    // We should patch the config here before creating the engine.
    let mut run_to_load = run.clone();

    // Patch limits if they match the old defaults (to allow continuation)
    if run_to_load.config.max_subagent_runs == 20
        || run_to_load.config.max_subagent_runs == 50
        || run_to_load.config.max_subagent_runs == 500
    {
        run_to_load.config.max_subagent_runs = 2000;
    }
    if run_to_load.config.max_iterations == 10
        || run_to_load.config.max_iterations == 30
        || run_to_load.config.max_iterations == 200
    {
        run_to_load.config.max_iterations = 500;
    }
    if run_to_load.config.max_wall_time_secs == 20 * 60 {
        run_to_load.config.max_wall_time_secs = 60 * 60;
    }

    // Keep budget max values aligned with config so "still_exceeded" is accurate.
    run_to_load.budget.max_iterations = run_to_load.config.max_iterations;
    run_to_load.budget.max_subagent_runs = run_to_load.config.max_subagent_runs;
    run_to_load.budget.max_wall_time_secs = run_to_load.config.max_wall_time_secs;
    if run_to_load.budget.max_wall_time_secs == 20 * 60 {
        run_to_load.budget.max_wall_time_secs = 60 * 60;
    }
    let still_exceeded = run_to_load.budget.iterations_used >= run_to_load.budget.max_iterations
        || run_to_load.budget.tokens_used >= run_to_load.budget.max_tokens
        || run_to_load.budget.wall_time_secs >= run_to_load.budget.max_wall_time_secs
        || run_to_load.budget.subagent_runs_used >= run_to_load.budget.max_subagent_runs;

    if run_to_load.budget.exceeded && !still_exceeded {
        run_to_load.budget.exceeded = false;
        run_to_load.budget.exceeded_reason = None;

        if run_to_load.status == RunStatus::Failed
            && run_to_load
                .error_message
                .as_deref()
                .is_some_and(|msg| msg.contains("Budget exceeded:"))
        {
            run_to_load.status = RunStatus::Paused;
            run_to_load.ended_at = None;
            run_to_load.error_message = None;
            for task in run_to_load.tasks.iter_mut() {
                if task.state == TaskState::InProgress {
                    task.state = TaskState::Pending;
                }
            }
        }
    }

    let engine = Arc::new(OrchestratorEngine::new(
        run_to_load.clone(),
        policy,
        store,
        state.sidecar.clone(),
        workspace_path,
        event_tx,
    ));

    // Also explicitly update the budget tracker limits in the engine
    // (The engine constructor does init the tracker from the run, but we want to be sure)
    // Actually, Engine::new calls BudgetTracker::from_budget(run.budget)
    // which copies the *old* limits from the saved budget snapshot.
    // So we need to update the tracker's limits after creation.
    engine.update_budget_limits().await;

    // Store engine in state
    {
        let mut engines = state.orchestrator_engines.write().unwrap();
        engines.insert(run_id.clone(), engine.clone());
    }

    // Check if the run was in an active state when last saved (e.g. app crash/close)
    // If so, force it to Paused so the user can explicitly Resume.
    {
        let current_status = engine.get_snapshot().await.status;
        if current_status == RunStatus::Executing || current_status == RunStatus::Planning {
            tracing::info!(
                "Run {} loaded in state {:?}, forcing to Paused",
                run_id,
                current_status
            );
            engine.force_pause_persisted().await?;
            run_to_load.status = RunStatus::Paused;
            run_to_load.ended_at = None;
        }
    }

    // Spawn event forwarder
    let app_handle = app.clone();
    let run_id_clone = run_id.clone();

    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            // Emit to frontend
            if let Err(e) = app_handle.emit("orchestrator-event", event) {
                tracing::error!("Failed to emit orchestrator event: {}", e);
            }
        }
        tracing::info!("Orchestrator event loop ended for run {}", run_id_clone);
    });

    // Do NOT auto-restart the execution loop on load.
    // Users can explicitly resume or restart from the UI for full control.

    tracing::info!("Loaded and re-hydrated orchestrator run: {}", run_id);
    Ok(run_to_load)
}

/// Restart execution manually (even after failure or cancellation)
#[tauri::command]
pub async fn orchestrator_restart_run(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    // Execute in background
    tauri::async_runtime::spawn(async move {
        if let Err(e) = engine.restart().await {
            tracing::error!("Failed to restart run {}: {}", run_id, e);
        }
    });

    Ok(())
}

/// Delete an orchestrator run (and its backing sidecar session)
#[tauri::command]
pub async fn orchestrator_delete_run(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;

    let store = OrchestratorStore::new(&workspace_path)?;
    let run = store.load_run(&run_id)?;

    // Stop any in-memory engine first so it doesn't keep writing to disk while we delete.
    if let Some(engine) = {
        let mut engines = state.orchestrator_engines.write().unwrap();
        engines.remove(&run_id)
    } {
        let _ = engine.cancel_and_finalize().await;
    }

    // Delete the root orchestrator session (child task/resume sessions were created as children and
    // won't show up in the user's main session list; they will become unreachable without the root).
    let _ = state.sidecar.delete_session(&run.session_id).await;

    store.delete_run(&run_id)?;
    Ok(())
}
