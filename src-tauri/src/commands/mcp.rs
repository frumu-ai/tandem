// ============================================================================
// MCP Runtime
// ============================================================================

#[tauri::command]
pub async fn mcp_list_servers(state: State<'_, AppState>) -> Result<Vec<McpServerRecord>> {
    state.sidecar.mcp_list_servers().await
}

#[tauri::command]
pub async fn mcp_add_server(
    state: State<'_, AppState>,
    request: McpAddRequest,
) -> Result<McpActionResponse> {
    state.sidecar.mcp_add_server(request).await
}

#[tauri::command]
pub async fn mcp_set_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<McpActionResponse> {
    state.sidecar.mcp_set_enabled(&name, enabled).await
}

#[tauri::command]
pub async fn mcp_connect(state: State<'_, AppState>, name: String) -> Result<McpActionResponse> {
    state.sidecar.mcp_connect(&name).await
}

#[tauri::command]
pub async fn mcp_disconnect(state: State<'_, AppState>, name: String) -> Result<McpActionResponse> {
    state.sidecar.mcp_disconnect(&name).await
}

#[tauri::command]
pub async fn mcp_refresh(state: State<'_, AppState>, name: String) -> Result<McpActionResponse> {
    state.sidecar.mcp_refresh(&name).await
}

#[tauri::command]
pub async fn mcp_list_tools(state: State<'_, AppState>) -> Result<Vec<McpRemoteTool>> {
    state.sidecar.mcp_list_tools().await
}

#[tauri::command]
pub async fn mcp_catalog(state: State<'_, AppState>) -> Result<serde_json::Value> {
    state.sidecar.mcp_catalog().await
}

#[tauri::command]
pub async fn capability_readiness(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.capability_readiness(request).await
}

#[tauri::command]
pub async fn bug_monitor_get_config(state: State<'_, AppState>) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_get_config().await
}

#[tauri::command]
pub async fn bug_monitor_patch_config(
    state: State<'_, AppState>,
    config: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_patch_config(config).await
}

#[tauri::command]
pub async fn bug_monitor_get_status(state: State<'_, AppState>) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_get_status().await
}

#[tauri::command]
pub async fn bug_monitor_list_drafts(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_list_drafts(limit).await
}

#[tauri::command]
pub async fn bug_monitor_get_draft(
    state: State<'_, AppState>,
    draft_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_get_draft(&draft_id).await
}

#[tauri::command]
pub async fn bug_monitor_report(
    state: State<'_, AppState>,
    report: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_report(report).await
}

#[tauri::command]
pub async fn bug_monitor_approve_draft(
    state: State<'_, AppState>,
    draft_id: String,
    reason: Option<String>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .bug_monitor_approve_draft(&draft_id, reason)
        .await
}

#[tauri::command]
pub async fn bug_monitor_deny_draft(
    state: State<'_, AppState>,
    draft_id: String,
    reason: Option<String>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .bug_monitor_deny_draft(&draft_id, reason)
        .await
}

#[tauri::command]
pub async fn bug_monitor_create_triage_run(
    state: State<'_, AppState>,
    draft_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.bug_monitor_create_triage_run(&draft_id).await
}

#[tauri::command]
pub async fn coder_list_runs(
    state: State<'_, AppState>,
    limit: Option<usize>,
    workflow_mode: Option<String>,
    repo_slug: Option<String>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .coder_list_runs(limit, workflow_mode, repo_slug)
        .await
}

#[tauri::command]
pub async fn coder_get_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    let mut payload = state.sidecar.coder_get_run(&run_id).await?;
    let artifacts_payload = state.sidecar.coder_list_artifacts(&run_id).await.ok();
    if let Some(summary) = summarize_coder_run_telemetry(&payload, artifacts_payload.as_ref()) {
        if let Some(record) = payload.as_object_mut() {
            record.insert("coding_summary".to_string(), json!(summary));
        }
    }
    Ok(payload)
}

#[tauri::command]
pub async fn context_run_rollback_preview(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.context_run_rollback_preview(&run_id).await
}

#[tauri::command]
pub async fn context_run_rollback_execute(
    state: State<'_, AppState>,
    run_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .context_run_rollback_execute(&run_id, request)
        .await
}

#[tauri::command]
pub async fn context_run_rollback_history(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.context_run_rollback_history(&run_id).await
}

#[tauri::command]
pub async fn coder_get_project_binding(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.coder_get_project_binding(&project_id).await
}

#[tauri::command]
pub async fn coder_put_project_binding(
    state: State<'_, AppState>,
    project_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .coder_put_project_binding(&project_id, request)
        .await
}

#[tauri::command]
pub async fn coder_get_project_github_inbox(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .coder_get_project_github_inbox(&project_id)
        .await
}

#[tauri::command]
pub async fn coder_intake_project_item(
    state: State<'_, AppState>,
    project_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .coder_intake_project_item(&project_id, request)
        .await
}

#[tauri::command]
pub async fn coder_list_artifacts(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.coder_list_artifacts(&run_id).await
}

#[tauri::command]
pub async fn coder_get_memory_hits(
    state: State<'_, AppState>,
    run_id: String,
    query: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .coder_get_memory_hits(&run_id, query, limit)
        .await
}

#[tauri::command]
pub async fn coder_list_memory_candidates(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.coder_list_memory_candidates(&run_id).await
}

#[tauri::command]
pub async fn coder_approve_run(
    state: State<'_, AppState>,
    run_id: String,
    reason: Option<String>,
) -> Result<serde_json::Value> {
    state.sidecar.coder_approve_run(&run_id, reason).await
}

#[tauri::command]
pub async fn coder_cancel_run(
    state: State<'_, AppState>,
    run_id: String,
    reason: Option<String>,
) -> Result<serde_json::Value> {
    state.sidecar.coder_cancel_run(&run_id, reason).await
}

#[tauri::command]
pub async fn pack_builder_preview(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.pack_builder_preview(request).await
}

#[tauri::command]
pub async fn setup_understand(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.setup_understand(request).await
}

#[tauri::command]
pub async fn pack_builder_apply(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.pack_builder_apply(request).await
}

#[tauri::command]
pub async fn pack_builder_cancel(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.pack_builder_cancel(request).await
}

#[tauri::command]
pub async fn pack_builder_pending(
    state: State<'_, AppState>,
    session_id: String,
    thread_key: Option<String>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .pack_builder_pending(&session_id, thread_key.as_deref())
        .await
}

#[tauri::command]
pub async fn tool_ids(state: State<'_, AppState>) -> Result<Vec<String>> {
    state.sidecar.tool_ids().await
}
