// Tandem Tauri Commands
// These are the IPC commands exposed to the frontend

use crate::error::{Result, TandemError};
use crate::keystore::SecureKeyStore;
use crate::sidecar::{
    CreateSessionRequest, FilePartInput, Message, ModelInfo, ModelSpec, Project, ProviderInfo,
    SendMessageRequest, Session, SessionMessage, SidecarState, StreamEvent,
};
use crate::sidecar_manager::{self, SidecarStatus};
use crate::state::{AppState, AppStateInfo, ProvidersConfig};
use crate::stronghold::{validate_api_key, validate_key_type, ApiKeyType};
use crate::tool_proxy::{FileSnapshot, JournalEntry, OperationStatus, UndoAction};
use crate::vault::{self, EncryptedVaultKey, VaultStatus};
use crate::VaultState;
use futures::StreamExt;
use std::path::PathBuf;
use std::fs;
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

    // Delete any existing Stronghold snapshot (from previous installations)
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

    // Initialize Stronghold in background thread (it's CPU-intensive)
    let app_clone = app.clone();
    let master_key_clone = master_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::init_stronghold_and_keys(&app_clone, &master_key_clone);
        tracing::info!("Stronghold initialization complete");
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

    // Initialize Stronghold in background thread (it's CPU-intensive)
    let app_clone = app.clone();
    let master_key_clone = master_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::init_stronghold_and_keys(&app_clone, &master_key_clone);
        tracing::info!("Stronghold initialization complete");
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
        let _ = store.set("workspace_path", serde_json::json!(path));
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
        let _ = store.set("user_projects", serde_json::to_value(&*projects).unwrap());
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
        let _ = store.set("user_projects", serde_json::to_value(&*projects).unwrap());
        
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
        
        // Get the sidecar path
        let sidecar_path = get_sidecar_path(&app)?;
        
        // Restart with new workspace
        state.sidecar.start(sidecar_path.to_string_lossy().as_ref()).await?;
        
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
        let _ = store.set("active_project_id", serde_json::json!(project_id));
        let projects = state.user_projects.read().unwrap();
        let _ = store.set("user_projects", serde_json::to_value(&*projects).unwrap());
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
    let key_type_for_log = key_type.clone();

    // Clone app handle so we can move it into spawn_blocking
    let app_clone = app.clone();

    // Insert the key in memory first (fast)
    let keystore = app_clone
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Stronghold("Keystore not initialized".to_string()))?;

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
        let sidecar_path = get_sidecar_path(&app)?;
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
        .ok_or_else(|| TandemError::Stronghold("Keystore not initialized".to_string()))?;

    keystore.delete(&key_name)?;

    if let Some(env_key) = env_var_for_key(&key_type_enum) {
        state.sidecar.remove_env(env_key).await;
        if matches!(state.sidecar.state().await, SidecarState::Running) {
            let sidecar_path = get_sidecar_path(&app)?;
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
        let _ = store.set(
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

/// Get the sidecar binary path
fn get_sidecar_path(app: &AppHandle) -> Result<PathBuf> {
    // Get the resource directory
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| TandemError::Sidecar(format!("Failed to get resource dir: {}", e)))?;

    // Determine binary name based on platform
    #[cfg(target_os = "windows")]
    let binary_name = "opencode-x86_64-pc-windows-msvc.exe";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let binary_name = "opencode-x86_64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let binary_name = "opencode-aarch64-apple-darwin";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let binary_name = "opencode-x86_64-unknown-linux-gnu";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let binary_name = "opencode-aarch64-unknown-linux-gnu";

    let binary_path = resource_dir.join("binaries").join(binary_name);

    if !binary_path.exists() {
        return Err(TandemError::Sidecar(format!(
            "Sidecar binary not found at: {}",
            binary_path.display()
        )));
    }

    Ok(binary_path)
}

/// Start the OpenCode sidecar
#[tauri::command]
pub async fn start_sidecar(app: AppHandle, state: State<'_, AppState>) -> Result<u16> {
    // Get the sidecar path
    let sidecar_path = get_sidecar_path(&app)?;

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
                        StreamEvent::FileEdited { session_id, .. } => {
                            session_id == &target_session_id
                        }
                        StreamEvent::Raw { .. } => true, // Include raw events for debugging
                    };

                    if is_our_session {
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
pub fn get_recent_file_operations(
    state: State<'_, AppState>,
    count: usize,
) -> Vec<JournalEntry> {
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
    tracing::info!(
        "Rewinding session {} to message {}",
        session_id,
        message_id
    );

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

    let target_index = target_index.ok_or_else(|| {
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
        tracing::warn!("[undo_message_with_files] OpenCode revert failed (continuing with file restore): {}", e);
    }

    // 2) Restore any files we journaled for this message
    tracing::info!("[undo_message_with_files] Looking for snapshots with message_id={}", message_id);
    let reverted_paths = state.operation_journal.undo_for_message(&message_id)?;
    tracing::info!("[undo_message_with_files] Restored {} files: {:?}", reverted_paths.len(), reverted_paths);

    Ok(reverted_paths)
}

/// Redo messages (unrevert)
/// Restores previously reverted messages
#[tauri::command]
pub async fn redo_message(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    tracing::info!("Redoing messages in session {}", session_id);
    state.sidecar.unrevert_message(&session_id).await
}

/// Execute undo via OpenCode's slash command API
/// This properly triggers Git-based file restoration
#[tauri::command]
pub async fn undo_via_command(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
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
        let is_file_tool = matches!(
            tool_name.as_str(),
            "write" | "delete"
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
                tracing::warn!("[approve_tool] Could not extract path from args: {:?}", args_val);
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
    message_id: Option<String>,
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
