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

    migrate_workspace_legacy_namespace_if_needed(&path_buf)?;
    state.set_workspace(path_buf);
    // Keep sidecar workspace scoping in sync with the selected workspace path.
    let sidecar = state.sidecar.clone();
    let sidecar_workspace = PathBuf::from(&path);
    tauri::async_runtime::spawn(async move {
        sidecar.set_workspace(sidecar_workspace).await;
    });
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
