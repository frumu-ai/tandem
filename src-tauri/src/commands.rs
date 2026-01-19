// Tandem Tauri Commands
// These are the IPC commands exposed to the frontend

use crate::error::{Result, TandemError};
use crate::keystore::{validate_api_key, validate_key_type, ApiKeyType, SecureKeyStore};
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
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

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
    let candidates: Vec<(&str, &crate::state::ProviderConfig)> = vec![
        ("openrouter", &config.openrouter),
        ("opencode", &config.opencode_zen), // OpenCode expects "opencode" not "opencode_zen"
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("ollama", &config.ollama),
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
    let candidates: Vec<(&str, &crate::state::ProviderConfig)> = vec![
        ("openrouter", &config.openrouter),
        ("opencode", &config.opencode_zen), // OpenCode expects "opencode" not "opencode_zen"
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("ollama", &config.ollama),
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
        ApiKeyType::Custom(_) => None,
    }
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
#[tauri::command]
pub fn get_app_state(state: State<'_, AppState>) -> AppStateInfo {
    AppStateInfo::from(state.inner())
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

        // Restart with new workspace
        state
            .sidecar
            .start(sidecar_path.to_string_lossy().as_ref())
            .await?;

        // Re-set API keys
        let providers = {
            let config = state.providers_config.read().unwrap();
            config.clone()
        };

        if providers.openrouter.enabled {
            if let Ok(Some(key)) = get_api_key(&app, "openrouter").await {
                state.sidecar.set_env("OPENROUTER_API_KEY", &key).await;
            }
        }
        if providers.opencode_zen.enabled {
            if let Ok(Some(key)) = get_api_key(&app, "opencode_zen").await {
                state.sidecar.set_env("OPENCODE_ZEN_API_KEY", &key).await;
            }
        }
        if providers.anthropic.enabled {
            if let Ok(Some(key)) = get_api_key(&app, "anthropic").await {
                state.sidecar.set_env("ANTHROPIC_API_KEY", &key).await;
            }
        }
        if providers.openai.enabled {
            if let Ok(Some(key)) = get_api_key(&app, "openai").await {
                state.sidecar.set_env("OPENAI_API_KEY", &key).await;
            }
        }

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
        let masked = if api_key.len() > 8 {
            format!("{}...{}", &api_key[..4], &api_key[api_key.len() - 4..])
        } else {
            "[REDACTED]".to_string()
        };
        tracing::info!("Setting environment variable {} = {}", env_key, masked);
        state.sidecar.set_env(env_key, &api_key).await;
    }

    tracing::info!("API key saved");

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
// Provider Configuration
// ============================================================================

/// Get the providers configuration
#[tauri::command]
pub fn get_providers_config(state: State<'_, AppState>) -> ProvidersConfig {
    let config = state.providers_config.read().unwrap();
    config.clone()
}

/// Set the providers configuration
#[tauri::command]
pub fn set_providers_config(
    app: AppHandle,
    config: ProvidersConfig,
    state: State<'_, AppState>,
) -> Result<()> {
    let mut providers = state.providers_config.write().unwrap();
    *providers = config.clone();

    tracing::info!("Providers configuration updated");

    // Save to store for persistence
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "providers_config",
            serde_json::to_value(&config).unwrap_or_default(),
        );
        let _ = store.save();
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

    // Set API key for the default/enabled provider
    if providers.openrouter.enabled {
        if let Ok(Some(key)) = get_api_key(&app, "openrouter").await {
            state.sidecar.set_env("OPENROUTER_API_KEY", &key).await;
        }
    }
    if providers.opencode_zen.enabled {
        if let Ok(Some(key)) = get_api_key(&app, "opencode_zen").await {
            state.sidecar.set_env("OPENCODE_ZEN_API_KEY", &key).await;
        }
    }
    if providers.anthropic.enabled {
        if let Ok(Some(key)) = get_api_key(&app, "anthropic").await {
            state.sidecar.set_env("ANTHROPIC_API_KEY", &key).await;
        }
    }
    if providers.openai.enabled {
        if let Ok(Some(key)) = get_api_key(&app, "openai").await {
            state.sidecar.set_env("OPENAI_API_KEY", &key).await;
        }
    }

    // Start the sidecar
    state
        .sidecar
        .start(sidecar_path.to_string_lossy().as_ref())
        .await?;

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
// Session Management
// ============================================================================

/// Create a new chat session
#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    title: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<Session> {
    let (default_provider, default_model) = {
        let config = state.providers_config.read().unwrap();
        resolve_default_provider_and_model(&config)
    };

    let request = CreateSessionRequest {
        title,
        model: model.or(default_model),
        provider: provider.or(default_provider),
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
        resolve_default_model_spec(&config)
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
        resolve_default_model_spec(&config)
    };
    request.model = model_spec;

    // Set agent if specified
    if let Some(agent_name) = agent {
        request.agent = Some(agent_name);
    }

    state.sidecar.send_message(&session_id, request).await?;

    let target_session_id = session_id.clone();

    // Process the stream and emit events to frontend
    tokio::spawn(async move {
        futures::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    // Filter events for our session
                    let is_our_session = match &event {
                        StreamEvent::Content { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::ToolStart { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::ToolEnd { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::SessionStatus { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::SessionIdle { session_id } => session_id == &target_session_id,
                        StreamEvent::SessionError { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::PermissionAsked { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::QuestionAsked { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::FileEdited { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::TodoUpdated { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::Raw { .. } => true, // Include raw events for debugging
                    };

                    if is_our_session {
                        // Log event for debugging
                        tracing::info!("[StreamEvent] Emitting to frontend: type={:?}", event);

                        // Emit the event to the frontend
                        if let Err(e) = app.emit("sidecar_event", &event) {
                            tracing::error!("Failed to emit sidecar event: {}", e);
                            break;
                        }

                        // Check if this is the done event
                        if matches!(event, StreamEvent::SessionIdle { .. }) {
                            break;
                        }
                    }
                }
                Err(e) => {
                    // Emit error event
                    let _ = app.emit(
                        "sidecar_event",
                        StreamEvent::SessionError {
                            session_id: target_session_id.clone(),
                            error: e.to_string(),
                        },
                    );
                    break;
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
            title: Some(format!("Rewind from {}", session_id)),
            model: default_model,
            provider: default_provider,
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

    // Capture a snapshot BEFORE allowing the tool to run, so we can undo file changes later.
    // We only snapshot direct file tools (write/delete). Shell commands and reads are too broad.
    // Note: OpenCode's tool names are "write", "delete", "read", "bash", "list", "search", etc.
    if let (Some(tool_name), Some(args_val)) = (tool.clone(), args.clone()) {
        let is_file_tool = matches!(tool_name.as_str(), "write" | "delete");

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

    // Create snapshot for file operations
    let (before_snapshot, proposed_content) = if let Some(path) = path_str.as_ref() {
        let path_buf = PathBuf::from(path);

        if state.is_path_allowed(&path_buf) {
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
    
    for category in &categories {  // Fix: borrow categories instead of moving
        match category.as_str() {
            "presentations" => {
                guidance.push(ToolGuidance {
                    category: "presentations".to_string(),
                    instructions: r#"# PowerPoint Presentation Creation

IMPORTANT: This capability works best in Plan Mode for review and approval.

When user requests a presentation, follow a TWO-PHASE workflow:

## PHASE 1: PLANNING (Required First Step)

Present a structured outline in this EXACT format:

```
# 📊 Presentation Plan: [Title]

**Theme:** [light/dark/corporate/minimal]
**Author:** [Author name or "User"]

## Slide Structure:

### Slide 1: [Layout Type]
**Title:** [Slide title]
**Subtitle:** [Subtitle if applicable]
**Content:**
- Bullet point 1
- Bullet point 2
- Bullet point 3
**Notes:** [Speaker notes if applicable]

### Slide 2: [Layout Type]
[... repeat structure ...]

---

**Ready to create this presentation?**
Please review the structure and let me know if you'd like any changes.
Once approved, I'll generate the .tandem.ppt.json file.
```

**Layout Types:**
- `title` - Title slide with title and subtitle (opening slide)
- `content` - Content slide with title and bullet points
- `section` - Section divider with title and subtitle
- `blank` - Blank slide for custom content

## PHASE 2: EXECUTION (After User Approval)

Only after user confirms the plan, use the 'write' tool to create `{filename}.tandem.ppt.json` with this structure.

NOTE: When the JSON file is created, Tandem will automatically export it to a `.pptx` file that can be opened in PowerPoint, Google Slides, or any presentation software.

"#.to_string(),
                    json_schema: serde_json::json!({
                        "title": "Presentation title (string, required)",
                        "author": "Author name (string, optional)",
                        "theme": "Visual theme: light|dark|corporate|minimal (optional, default: light)",
                        "slides": "Array of slide objects (required, minimum 1 slide)",
                        "slide_structure": {
                            "id": "Unique identifier like 'slide_1', 'slide_2' (string)",
                            "layout": "Layout type: title|content|section|blank (required)",
                            "title": "Slide title (string, optional)",
                            "subtitle": "Slide subtitle (string, optional, for title/section layouts)",
                            "elements": "Array of content elements",
                            "element_structure": {
                                "type": "Element type: bullet_list|text|image",
                                "content": "For bullet_list: array of strings; for text: single string"
                            },
                            "notes": "Speaker notes (string, optional)"
                        }
                    }),
                    example: serde_json::json!({
                        "title": "Q1 Strategy 2026",
                        "author": "Product Team",
                        "theme": "corporate",
                        "slides": [
                            {
                                "id": "slide_1",
                                "layout": "title",
                                "title": "Q1 Strategy 2026",
                                "subtitle": "Growth & Innovation",
                                "elements": []
                            },
                            {
                                "id": "slide_2",
                                "layout": "content",
                                "title": "Key Objectives",
                                "elements": [{
                                    "type": "bullet_list",
                                    "content": [
                                        "Increase ARR by 40%",
                                        "Launch 3 major features",
                                        "Expand to EMEA region"
                                    ]
                                }],
                                "notes": "Emphasize the timeline for each objective"
                            },
                            {
                                "id": "slide_3",
                                "layout": "section",
                                "title": "Revenue Targets",
                                "subtitle": "Quarterly Breakdown",
                                "elements": []
                            }
                        ]
                    }).to_string(),
                });
            },
            "diagrams" => {
                // Future: Mermaid diagram guidance
                tracing::debug!("Diagrams tool category not yet implemented");
            },
            "spreadsheets" => {
                // Future: Table/CSV guidance
                tracing::debug!("Spreadsheets tool category not yet implemented");
            },
            _ => {
                tracing::warn!("Unknown tool category: {}", category);
            }
        }
    }
    
    tracing::info!("Returning {} tool guidance items for categories: {:?}", guidance.len(), categories);
    guidance
}

// ============================================================================
// Presentation Export (ppt-rs)
// ============================================================================

/// Export a .tandem.ppt.json file to a binary .pptx file using ppt-rs
#[tauri::command]
pub async fn export_presentation(
    json_path: String,
    output_path: String,
) -> Result<String> {
    use crate::presentation::{Presentation, ElementContent, SlideLayout};
    use std::fs::File;
    use std::io::Write;
    use zip::write::{FileOptions, ZipWriter};
    
    tracing::info!("Exporting presentation from {} to {}", json_path, output_path);
    
    // 1. Read and parse JSON
    let json_content = std::fs::read_to_string(&json_path)
        .map_err(TandemError::Io)?;
    
    let presentation: Presentation = serde_json::from_str(&json_content)
        .map_err(|e| TandemError::InvalidConfig(format!("Invalid presentation JSON: {}", e)))?;
    
    tracing::debug!("Parsed presentation: {} with {} slides", presentation.title, presentation.slides.len());
    
    let file = File::create(&output_path)
        .map_err(TandemError::Io)?;
    
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
    let mut content_types = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
"#);
    
    for i in 1..=presentation.slides.len() {
        content_types.push_str(&format!(
            r#"  <Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
"#, i));
    }
    content_types.push_str("</Types>");
    
    zip.start_file("[Content_Types].xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(content_types.as_bytes()).map_err(TandemError::Io)?;
    
    // === _rels/.rels ===
    zip.start_file("_rels/.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#;
    zip.write_all(rels.as_bytes()).map_err(TandemError::Io)?;
    
    // === ppt/presentation.xml ===
    let mut pres_xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" saveSubsetFonts="1">
  <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
  <p:sldIdLst>
"#);
    
    for (i, _) in presentation.slides.iter().enumerate() {
        pres_xml.push_str(&format!(
            r#"    <p:sldId id="{}" r:id="rId{}"/>
"#, 256 + i, i + 2));
    }
    
    pres_xml.push_str(r#"  </p:sldIdLst>
  <p:sldSz cx="9144000" cy="6858000"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#);
    
    zip.start_file("ppt/presentation.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_xml.as_bytes()).map_err(TandemError::Io)?;
    
    // === ppt/_rels/presentation.xml.rels ===
    let mut pres_rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
"#);
    
    for (i, _) in presentation.slides.iter().enumerate() {
        pres_rels.push_str(&format!(
            r#"  <Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>
"#, i + 2, i + 1));
    }
    pres_rels.push_str("</Relationships>");
    
    zip.start_file("ppt/_rels/presentation.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(pres_rels.as_bytes()).map_err(TandemError::Io)?;
    
    // === Generate slides ===
    for (i, slide) in presentation.slides.iter().enumerate() {
        let slide_num = i + 1;
        let mut slide_xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
"#);
        
        let mut shape_id = 2;
        
        // Title shape
        if let Some(title) = &slide.title {
            slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Title {}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:spPr/>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
          <a:p><a:r><a:rPr lang="en-US" sz="4400" b="1"/><a:t>{}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
"#, shape_id, shape_id, escape_xml(title)));
            shape_id += 1;
        }
        
        // Subtitle (for title slide layout)
        if matches!(slide.layout, SlideLayout::Title) {
            if let Some(subtitle) = &slide.subtitle {
                slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Subtitle {}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="subTitle" idx="1"/></p:nvPr></p:nvSpPr>
        <p:spPr/>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
          <a:p><a:r><a:rPr lang="en-US" sz="2400"/><a:t>{}</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
"#, shape_id, shape_id, escape_xml(subtitle)));
                shape_id += 1;
            }
        }
        
        // Content (bullets and text elements)
        if !slide.elements.is_empty() {
            let mut content_text = String::new();
            for element in &slide.elements {
                match &element.content {
                    ElementContent::Bullets(bullets) => {
                        for bullet in bullets {
                            content_text.push_str(&format!(
                                r#"          <a:p><a:pPr lvl="0"><a:buFont typeface="Arial"/><a:buChar char="•"/></a:pPr><a:r><a:rPr lang="en-US" sz="2000"/><a:t>{}</a:t></a:r></a:p>
"#, escape_xml(bullet)));
                        }
                    },
                    ElementContent::Text(t) => {
                        content_text.push_str(&format!(
                            r#"          <a:p><a:r><a:rPr lang="en-US" sz="2000"/><a:t>{}</a:t></a:r></a:p>
"#, escape_xml(t)));
                    },
                }
            }
            
            if !content_text.is_empty() {
                slide_xml.push_str(&format!(r#"      <p:sp>
        <p:nvSpPr><p:cNvPr id="{}" name="Content {}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
        <p:spPr/>
        <p:txBody>
          <a:bodyPr/>
          <a:lstStyle/>
{}        </p:txBody>
      </p:sp>
"#, shape_id, shape_id, content_text));
            }
        }
        
        slide_xml.push_str(r#"    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#);
        
        zip.start_file(format!("ppt/slides/slide{}.xml", slide_num), options)
            .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        zip.write_all(slide_xml.as_bytes()).map_err(TandemError::Io)?;
        
        // === Slide relationship file (critical for Google Slides) ===
        zip.start_file(format!("ppt/slides/_rels/slide{}.xml.rels", slide_num), options)
            .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
        let slide_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
        zip.write_all(slide_rels.as_bytes()).map_err(TandemError::Io)?;
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
    zip.write_all(slide_master.as_bytes()).map_err(TandemError::Io)?;
    
    zip.start_file("ppt/slideMasters/_rels/slideMaster1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let master_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
    zip.write_all(master_rels.as_bytes()).map_err(TandemError::Io)?;
    
    let slide_layout = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
  <p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#;
    
    zip.start_file("ppt/slideLayouts/slideLayout1.xml", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    zip.write_all(slide_layout.as_bytes()).map_err(TandemError::Io)?;
    
    zip.start_file("ppt/slideLayouts/_rels/slideLayout1.xml.rels", options)
        .map_err(|e| TandemError::Io(std::io::Error::other(e)))?;
    let layout_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#;
    zip.write_all(layout_rels.as_bytes()).map_err(TandemError::Io)?;
    
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
pub async fn read_directory(path: String) -> Result<Vec<FileEntry>> {
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
pub async fn read_file_content(path: String, max_size: Option<u64>) -> Result<String> {
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
pub fn read_binary_file(path: String) -> Result<String> {
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

    let bytes = fs::read(&file_path).map_err(TandemError::Io)?;
    Ok(STANDARD.encode(&bytes))
}
