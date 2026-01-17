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
use crate::vault::{self, EncryptedVaultKey, VaultStatus};
use crate::VaultState;
use futures::StreamExt;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;

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
        return Err(TandemError::Vault("No vault exists. Create one first.".to_string()));
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
pub fn set_providers_config(app: AppHandle, config: ProvidersConfig, state: State<'_, AppState>) -> Result<()> {
    let mut providers = state.providers_config.write().unwrap();
    *providers = config.clone();

    tracing::info!("Providers configuration updated");

    // Save to store for persistence
    if let Ok(store) = app.store("settings.json") {
        let _ = store.set("providers_config", serde_json::to_value(&config).unwrap_or_default());
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
                        StreamEvent::ToolStart { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::ToolEnd { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::SessionStatus { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::SessionIdle { session_id } => session_id == &target_session_id,
                        StreamEvent::SessionError { session_id, .. } => session_id == &target_session_id,
                        StreamEvent::PermissionAsked { session_id, .. } => session_id == &target_session_id,
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
// Tool Approval
// ============================================================================

/// Approve a pending tool execution
#[tauri::command]
pub async fn approve_tool(
    state: State<'_, AppState>,
    session_id: String,
    tool_call_id: String,
) -> Result<()> {
    state.sidecar.approve_tool(&session_id, &tool_call_id).await
}

/// Deny a pending tool execution
#[tauri::command]
pub async fn deny_tool(
    state: State<'_, AppState>,
    session_id: String,
    tool_call_id: String,
) -> Result<()> {
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
