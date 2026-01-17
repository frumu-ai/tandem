// Tandem Tauri Commands
// These are the IPC commands exposed to the frontend

use crate::error::{Result, TandemError};
use crate::sidecar::{
    CreateSessionRequest, Message, ModelInfo, ProviderInfo, SendMessageRequest, Session,
    SidecarState, StreamEvent,
};
use crate::state::{AppState, AppStateInfo, ProvidersConfig};
use crate::stronghold::{validate_api_key, validate_key_type};
use futures::StreamExt;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_stronghold::stronghold::Stronghold;

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
pub fn set_workspace_path(path: String, state: State<'_, AppState>) -> Result<()> {
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
    key_type: String,
    api_key: String,
) -> Result<()> {
    // Validate inputs
    let key_type_enum = validate_key_type(&key_type)?;
    validate_api_key(&api_key)?;

    let key_name = key_type_enum.to_key_name();

    let stronghold = app
        .try_state::<Stronghold>()
        .ok_or_else(|| TandemError::Stronghold("Stronghold not initialized".to_string()))?;

    let client_path = b"tandem";
    let client = stronghold
        .get_client(client_path)
        .or_else(|_| stronghold.create_client(client_path))
        .map_err(|e| TandemError::Stronghold(format!("Failed to load client: {}", e)))?;

    client
        .store()
        .insert(
            key_name.as_bytes().to_vec(),
            api_key.as_bytes().to_vec(),
            None,
        )
        .map_err(|e| TandemError::Stronghold(format!("Failed to save key: {}", e)))?;

    stronghold
        .save()
        .map_err(|e| TandemError::Stronghold(format!("Failed to persist vault: {}", e)))?;

    tracing::info!("API key stored for provider: {}", key_type);
    Ok(())
}

/// Check if an API key exists for a provider
#[tauri::command]
pub async fn has_api_key(app: tauri::AppHandle, key_type: String) -> Result<bool> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let stronghold = match app.try_state::<Stronghold>() {
        Some(stronghold) => stronghold,
        None => return Ok(false),
    };

    let client_path = b"tandem";
    let client = stronghold
        .get_client(client_path)
        .or_else(|_| stronghold.create_client(client_path))
        .map_err(|e| TandemError::Stronghold(format!("Failed to load client: {}", e)))?;

    match client.store().get(key_name.as_ref()) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(e) => Err(TandemError::Stronghold(format!(
            "Failed to check key: {}",
            e
        ))),
    }
}

/// Delete an API key from the vault
#[tauri::command]
pub async fn delete_api_key(app: tauri::AppHandle, key_type: String) -> Result<()> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let stronghold = app
        .try_state::<Stronghold>()
        .ok_or_else(|| TandemError::Stronghold("Stronghold not initialized".to_string()))?;

    let client_path = b"tandem";
    let client = stronghold
        .get_client(client_path)
        .or_else(|_| stronghold.create_client(client_path))
        .map_err(|e| TandemError::Stronghold(format!("Failed to load client: {}", e)))?;

    client
        .store()
        .delete(key_name.as_ref())
        .map_err(|e| TandemError::Stronghold(format!("Failed to delete key: {}", e)))?;

    stronghold
        .save()
        .map_err(|e| TandemError::Stronghold(format!("Failed to persist vault: {}", e)))?;

    tracing::info!("API key deleted for provider: {}", key_type);
    Ok(())
}

/// Get an API key from the vault (internal use only)
async fn get_api_key(app: &AppHandle, key_type: &str) -> Result<Option<String>> {
    let key_type_enum = validate_key_type(key_type)?;
    let key_name = key_type_enum.to_key_name();

    let stronghold = match app.try_state::<Stronghold>() {
        Some(stronghold) => stronghold,
        None => return Ok(None),
    };

    let client_path = b"tandem";
    let client = stronghold
        .get_client(client_path)
        .or_else(|_| stronghold.create_client(client_path))
        .map_err(|e| TandemError::Stronghold(format!("Failed to load client: {}", e)))?;

    match client.store().get(key_name.as_ref()) {
        Ok(Some(data)) => {
            let key = String::from_utf8(data).map_err(|e| {
                TandemError::Stronghold(format!("Failed to decode key: {}", e))
            })?;
            Ok(Some(key))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(TandemError::Stronghold(format!(
            "Failed to get key: {}",
            e
        ))),
    }
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
pub fn set_providers_config(config: ProvidersConfig, state: State<'_, AppState>) -> Result<()> {
    let mut providers = state.providers_config.write().unwrap();
    *providers = config;

    tracing::info!("Providers configuration updated");

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
    let request = CreateSessionRequest {
        title,
        model,
        provider,
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

/// Send a message to a session (non-streaming)
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    model: Option<String>,
) -> Result<Message> {
    let request = SendMessageRequest { content, model };
    state.sidecar.send_message(&session_id, request).await
}

/// Send a message and stream the response
/// This emits events to the frontend as chunks arrive
#[tauri::command]
pub async fn send_message_streaming(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    model: Option<String>,
) -> Result<()> {
    let request = SendMessageRequest { content, model };

    let stream = state
        .sidecar
        .send_message_streaming(&session_id, request)
        .await?;

    // Process the stream and emit events to frontend
    tokio::spawn(async move {
        futures::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    // Emit the event to the frontend
                    if let Err(e) = app.emit("sidecar_event", &event) {
                        tracing::error!("Failed to emit sidecar event: {}", e);
                        break;
                    }

                    // Check if this is the done event
                    if matches!(event, StreamEvent::Done { .. }) {
                        break;
                    }
                }
                Err(e) => {
                    // Emit error event
                    let _ = app.emit(
                        "sidecar_event",
                        StreamEvent::Error {
                            message: e.to_string(),
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
