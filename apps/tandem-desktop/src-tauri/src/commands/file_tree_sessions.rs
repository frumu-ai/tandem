// ============================================================================
// File Tree Watcher (Files view auto-refresh)
// ============================================================================

#[tauri::command]
pub fn start_file_tree_watcher(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    root_path: String,
) -> Result<()> {
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| TandemError::InvalidConfig(format!("Window not found: {}", window_label)))?;

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

    let watcher = crate::file_watcher::FileTreeWatcher::new(&root, app, window)
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

#[tauri::command]
pub fn list_modes(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<ResolvedMode>> {
    crate::modes::list_modes(&app, state.get_workspace_path().as_deref())
}

#[tauri::command]
pub fn upsert_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ModeScope,
    mode: ModeDefinition,
) -> Result<()> {
    crate::modes::upsert_mode(&app, state.get_workspace_path().as_deref(), scope, mode)
}

#[tauri::command]
pub fn delete_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ModeScope,
    id: String,
) -> Result<()> {
    crate::modes::delete_mode(&app, state.get_workspace_path().as_deref(), scope, &id)
}

#[tauri::command]
pub fn import_modes(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ModeScope,
    json: String,
) -> Result<()> {
    crate::modes::import_modes(&app, state.get_workspace_path().as_deref(), scope, &json)
}

#[tauri::command]
pub fn export_modes(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: ModeScope,
) -> Result<String> {
    crate::modes::export_modes(&app, state.get_workspace_path().as_deref(), scope)
}

/// Create a new chat session
#[tauri::command]
pub async fn create_session(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    allow_all_tools: Option<bool>,
    mode_id: Option<String>,
) -> Result<Session> {
    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec =
        resolve_required_model_spec(&config_snapshot, model, provider, "Chat session creation")?;

    // IMPORTANT:
    // We intentionally do NOT send `permission="*"` allow to OpenCode.
    // Even when the UI toggle "Allow all tools" is enabled, the frontend auto-approves
    // permission prompts, and we still want the approve/deny hook to run so Tandem can
    // enforce safety policy.
    let _allow_all = allow_all_tools.unwrap_or(false);
    let mode_resolution = resolve_effective_mode(&app, &state, mode_id.as_deref(), None)?;
    if let Some(reason) = mode_resolution.fallback_reason.as_ref() {
        tracing::warn!("[create_session] {}", reason);
    }
    let permission = sidecar_permissions_for_mode(&mode_resolution.mode);

    validate_model_provider_auth_if_required(
        &app,
        &config_snapshot,
        Some(model_spec.model_id.as_str()),
        Some(model_spec.provider_id.as_str()),
    )
    .await?;
    let request = CreateSessionRequest {
        parent_id: None,
        title,
        model: build_sidecar_session_model(
            Some(model_spec.model_id.clone()),
            Some(model_spec.provider_id.clone()),
        ),
        provider: Some(model_spec.provider_id),
        permission,
        directory: state
            .get_workspace_path()
            .map(|p| p.to_string_lossy().to_string()),
        workspace_root: state
            .get_workspace_path()
            .map(|p| p.to_string_lossy().to_string()),
        project_id: None,
    };

    let session = state.sidecar.create_session(request).await?;
    set_session_mode(&state, &session.id, mode_resolution.mode);

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

/// Get currently active run for a session.
#[tauri::command]
pub async fn get_session_active_run(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<ActiveRunStatusResponse>> {
    state.sidecar.get_active_run(&session_id).await
}

/// Delete a session
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sidecar.delete_session(&session_id).await?;
    let mut modes = state.session_modes.write().unwrap();
    modes.remove(&session_id);
    Ok(())
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

/// List persisted tool executions for a session (session-scoped only)
#[tauri::command]
pub async fn list_tool_executions(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    limit: Option<u32>,
    before_ts_ms: Option<u64>,
) -> Result<Vec<ToolExecutionRow>> {
    let tenant_context = match state.sidecar.get_session(&session_id).await {
        Ok(session) => session.tenant_context,
        Err(_) => tandem_types::TenantContext::local_implicit(),
    };
    crate::tool_history::list_tool_executions(
        &app,
        &session_id,
        &tenant_context,
        limit.unwrap_or(200),
        before_ts_ms,
    )
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
