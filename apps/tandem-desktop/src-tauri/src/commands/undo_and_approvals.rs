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
    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec =
        resolve_required_model_spec(&config_snapshot, None, None, "Conversation rewind")?;

    let new_session = state
        .sidecar
        .create_session(CreateSessionRequest {
            parent_id: None,
            title: Some(format!("Rewind from {}", session_id)),
            model: build_sidecar_session_model(
                Some(model_spec.model_id.clone()),
                Some(model_spec.provider_id.clone()),
            ),
            provider: Some(model_spec.provider_id.clone()),
            permission: sidecar_permissions_for_mode(&effective_session_mode(&state, &session_id)),
            directory: state
                .get_workspace_path()
                .map(|p| p.to_string_lossy().to_string()),
            workspace_root: state
                .get_workspace_path()
                .map(|p| p.to_string_lossy().to_string()),
            project_id: None,
        })
        .await?;
    set_session_mode(
        &state,
        &new_session.id,
        effective_session_mode(&state, &session_id),
    );

    tracing::info!("Created new branched session: {}", new_session.id);

    // 4. Replay messages up to (but not including) the target message
    // OpenCode doesn't have a direct API to copy messages, so we'll just return the new session
    // The frontend will handle displaying the branched conversation

    // TODO: In a future enhancement, we could replay messages by sending them to the new session
    // For now, we'll just create an empty session for the user to continue from

    // If edited content is provided, send it as the first message
    if let Some(content) = edited_content {
        tracing::info!("Sending edited message to new session");
        let mut request = SendMessageRequest::text(content);
        request.model = Some(ModelSpec {
            provider_id: model_spec.provider_id.clone(),
            model_id: model_spec.model_id.clone(),
        });
        state
            .sidecar
            .append_message_and_start_run(&new_session.id, request)
            .await?;
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

    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec =
        resolve_required_model_spec(&config_snapshot, None, None, "Undo command dispatch")?;

    // Send "/undo" as a regular prompt - same as typing it in the TUI
    // OpenCode intercepts slash commands and handles them specially
    let mut request = crate::sidecar::SendMessageRequest::text("/undo".to_string());
    request.model = Some(ModelSpec {
        provider_id: model_spec.provider_id,
        model_id: model_spec.model_id,
    });
    state
        .sidecar
        .append_message_and_start_run(&session_id, request)
        .await
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

fn effective_session_mode(state: &AppState, session_id: &str) -> ResolvedMode {
    if let Some(mode) = get_session_mode(state, session_id) {
        return mode;
    }
    crate::modes::built_in_modes()
        .into_iter()
        .find(|m| m.id == "immediate")
        .unwrap_or(ResolvedMode {
            id: "immediate".to_string(),
            label: "Immediate".to_string(),
            base_mode: crate::modes::ModeBase::Immediate,
            icon: None,
            system_prompt_append: None,
            allowed_tools: None,
            edit_globs: None,
            auto_approve: None,
            source: crate::modes::ModeSource::Builtin,
        })
}

fn normalize_tool_name_for_approval(name: &str) -> String {
    let mut normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    for prefix in [
        "default_api:",
        "default_api.",
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "builtin:",
        "builtin.",
    ] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                normalized = trimmed.to_string();
                break;
            }
        }
    }
    match normalized.as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        "run_command" | "shell" | "powershell" | "cmd" => "bash".to_string(),
        other => other.to_string(),
    }
}

fn extract_websearch_query(args: &serde_json::Value) -> Option<String> {
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    for key in QUERY_KEYS {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in QUERY_KEYS {
                if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

fn websearch_query_present(args: Option<&serde_json::Value>) -> bool {
    args.and_then(extract_websearch_query).is_some()
}

fn extract_file_tool_path(args: &serde_json::Value) -> Option<String> {
    for key in ["filePath", "absolute_path", "path", "file"] {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_write_tool_content(args: &serde_json::Value) -> Option<String> {
    for key in ["content", "body", "text"] {
        if let Some(value) = args.get(key).and_then(|v| v.as_str()) {
            return Some(value.to_string());
        }
    }
    None
}

fn missing_file_tool_arg_reason(
    normalized_tool: &str,
    args: Option<&serde_json::Value>,
) -> Option<&'static str> {
    let Some(args) = args else {
        return Some("FILE_PATH_MISSING_APPROVAL");
    };

    let path_missing = extract_file_tool_path(args).is_none();
    match normalized_tool {
        "write" | "write_file" | "create_file" => {
            if path_missing {
                return Some("FILE_PATH_MISSING_APPROVAL");
            }
            if extract_write_tool_content(args).is_none() {
                return Some("WRITE_CONTENT_MISSING_APPROVAL");
            }
            None
        }
        "read" | "edit" | "delete" | "delete_file" => {
            if path_missing {
                Some("FILE_PATH_MISSING_APPROVAL")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn set_websearch_query(
    args: Option<serde_json::Value>,
    query: &str,
    query_source: &str,
) -> serde_json::Value {
    let mut obj = args
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert(
        "query".to_string(),
        serde_json::Value::String(query.to_string()),
    );
    obj.insert(
        "__query_source".to_string(),
        serde_json::Value::String(query_source.to_string()),
    );
    serde_json::Value::Object(obj)
}

fn infer_websearch_query_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    const PREFIXES: [&str; 10] = [
        "web search",
        "websearch",
        "search web",
        "search for",
        "search",
        "look up",
        "lookup",
        "find",
        "web lookup",
        "query",
    ];

    let mut candidate = trimmed;
    for prefix in PREFIXES {
        if lower.starts_with(prefix) && lower.len() >= prefix.len() {
            candidate = trimmed[prefix.len()..]
                .trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '-');
            break;
        }
    }

    let normalized = candidate
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?'))
        .trim()
        .to_string();
    if normalized.split_whitespace().count() < 2 {
        return None;
    }
    Some(normalized)
}

fn latest_user_text_from_messages(messages: &[SessionMessage]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if !message.info.role.eq_ignore_ascii_case("user") {
            return None;
        }
        let mut out = String::new();
        for part in &message.parts {
            let text = part
                .get("text")
                .and_then(|v| v.as_str())
                .or_else(|| part.get("content").and_then(|v| v.as_str()))
                .or_else(|| {
                    part.get("type")
                        .and_then(|v| v.as_str())
                        .filter(|t| *t == "text")
                        .and_then(|_| part.get("text"))
                        .and_then(|v| v.as_str())
                });
            if let Some(text) = text {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        let trimmed = out.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Approve a pending tool execution
#[tauri::command]
pub async fn approve_tool(
    app: AppHandle,
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

    let effective_tool = tool.clone();
    let mut effective_args = args.clone();
    let normalized_tool = effective_tool
        .as_deref()
        .map(normalize_tool_name_for_approval);
    let tenant_context = crate::state::resolve_session_tenant_context(&state, &session_id).await;
    let request_cache_key =
        crate::state::tenant_scoped_cache_key("permission_args", &tenant_context, &tool_call_id);
    let session_intent_cache_key = crate::state::tenant_scoped_cache_key(
        "session_websearch_intent",
        &tenant_context,
        &session_id,
    );

    if normalized_tool.as_deref() == Some("websearch")
        && !websearch_query_present(effective_args.as_ref())
    {
        // Try request-scoped cache first.
        if let Some(cached_args) = state
            .permission_args_cache
            .lock()
            .await
            .get(&request_cache_key)
            .cloned()
        {
            if let Some(query) = extract_websearch_query(&cached_args) {
                tracing::warn!(
                    "[approve_tool] recovered websearch query from request cache request_id={} query={}",
                    tool_call_id,
                    query
                );
                effective_args = Some(set_websearch_query(
                    Some(cached_args),
                    &query,
                    "recovered_from_context",
                ));
            }
        }
    }
    if normalized_tool.as_deref() == Some("websearch")
        && !websearch_query_present(effective_args.as_ref())
    {
        // Then per-session recovered intent cache.
        if let Some(query) = state
            .session_websearch_intent
            .lock()
            .await
            .get(&session_intent_cache_key)
            .cloned()
        {
            tracing::warn!(
                "[approve_tool] recovered websearch query from session intent session_id={} query={}",
                session_id,
                query
            );
            effective_args = Some(set_websearch_query(
                effective_args.clone(),
                &query,
                "recovered_from_context",
            ));
        }
    }
    if normalized_tool.as_deref() == Some("websearch")
        && !websearch_query_present(effective_args.as_ref())
    {
        // Last resort: infer from latest user message text in session history.
        if let Ok(messages) = state.sidecar.get_session_messages(&session_id).await {
            if let Some(user_text) = latest_user_text_from_messages(&messages) {
                if let Some(query) = infer_websearch_query_from_text(&user_text) {
                    tracing::warn!(
                        "[approve_tool] inferred websearch query from latest user text session_id={} query={}",
                        session_id,
                        query
                    );
                    effective_args = Some(set_websearch_query(
                        effective_args.clone(),
                        &query,
                        "inferred_from_user",
                    ));
                }
            }
        }
    }
    if normalized_tool.as_deref() == Some("websearch")
        && !websearch_query_present(effective_args.as_ref())
    {
        tracing::warn!(
            "[approve_tool] denying websearch due to missing query after recovery attempts request_id={}",
            tool_call_id
        );
        let _ = state.sidecar.deny_tool(&session_id, &tool_call_id).await;
        return Err(TandemError::PermissionDenied(
            "WEBSEARCH_QUERY_MISSING_APPROVAL".to_string(),
        ));
    }

    let is_file_tool = matches!(
        normalized_tool.as_deref(),
        Some("write" | "write_file" | "create_file" | "read" | "edit" | "delete" | "delete_file")
    );
    if is_file_tool {
        if missing_file_tool_arg_reason(
            normalized_tool.as_deref().unwrap_or_default(),
            effective_args.as_ref(),
        )
        .is_some()
        {
            if let Some(cached_args) = state
                .permission_args_cache
                .lock()
                .await
                .get(&request_cache_key)
                .cloned()
            {
                if missing_file_tool_arg_reason(
                    normalized_tool.as_deref().unwrap_or_default(),
                    Some(&cached_args),
                )
                .is_none()
                {
                    tracing::warn!(
                        "[approve_tool] recovered file tool args from request cache request_id={}",
                        tool_call_id
                    );
                    effective_args = Some(cached_args);
                }
            }
        }

        if let Some(reason) = missing_file_tool_arg_reason(
            normalized_tool.as_deref().unwrap_or_default(),
            effective_args.as_ref(),
        ) {
            tracing::warn!(
                "[approve_tool] denying file tool due to missing args reason={} request_id={} args={:?}",
                reason,
                tool_call_id,
                effective_args
            );
            state
                .permission_args_cache
                .lock()
                .await
                .remove(&request_cache_key);
            let _ = state.sidecar.deny_tool(&session_id, &tool_call_id).await;
            return Err(TandemError::PermissionDenied(reason.to_string()));
        }
    }

    // Keep session-level intent fresh once query is present.
    if normalized_tool.as_deref() == Some("websearch") {
        if let Some(args_val) = effective_args.as_ref() {
            if let Some(query) = extract_websearch_query(args_val) {
                state
                    .session_websearch_intent
                    .lock()
                    .await
                    .insert(session_intent_cache_key, query);
            }
        }
    }

    if let (Some(tool_name), Some(args_val)) = (effective_tool.clone(), effective_args.clone()) {
        let mode = effective_session_mode(&state, &session_id);
        if let Err(e) = crate::modes::mode_allows_tool_execution(
            &mode,
            state.get_workspace_path().as_deref(),
            &tool_name,
            &args_val,
        ) {
            let _ = state.sidecar.deny_tool(&session_id, &tool_call_id).await;
            return Err(e);
        }
    }

    // Strict Python venv enforcement for AI terminal-like tools.
    // Goal: prevent global pip installs and python runs outside workspace venv.
    if let (Some(tool_name), Some(args_val)) = (effective_tool.clone(), effective_args.clone()) {
        let ws = state
            .get_workspace_path()
            .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

        if let Some(msg) = tool_policy::python_policy_violation(&ws, tool_name.as_str(), &args_val)
        {
            // Best-effort: notify UI to open the wizard immediately.
            let _ = app.emit(
                "python-setup-required",
                serde_json::json!({
                    "reason": msg,
                    "workspace_path": ws.to_string_lossy().to_string()
                }),
            );

            tracing::info!(
                "[python_policy] Denying terminal tool (tool={}): args={}",
                tool_name,
                args_val
            );
            let _ = state.sidecar.deny_tool(&session_id, &tool_call_id).await;
            return Err(TandemError::PermissionDenied(msg));
        }
    }

    // Capture a snapshot BEFORE allowing the tool to run, so we can undo file changes later.
    // We only snapshot direct file tools (write/delete). Shell commands and reads are too broad.
    // Note: OpenCode's tool names are "write", "delete", "read", "bash", "list", "search", etc.
    if let (Some(tool_name), Some(args_val)) = (effective_tool.clone(), effective_args.clone()) {
        let is_file_tool = matches!(
            normalize_tool_name_for_approval(tool_name.as_str()).as_str(),
            "write" | "write_file" | "create_file" | "delete" | "delete_file"
        );

        if is_file_tool {
            tracing::info!("[approve_tool] File tool detected: {}", tool_name);

            // Try to extract a file path from args
            // OpenCode uses "filePath" for write operations
            let path_str = extract_file_tool_path(&args_val);

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

    // Clear request-scoped cache after approval resolution.
    state
        .permission_args_cache
        .lock()
        .await
        .remove(&request_cache_key);

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

    let tenant_context = crate::state::resolve_session_tenant_context(&state, &session_id).await;
    let request_cache_key =
        crate::state::tenant_scoped_cache_key("permission_args", &tenant_context, &tool_call_id);

    state
        .permission_args_cache
        .lock()
        .await
        .remove(&request_cache_key);

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
