// ============================================================================
// Routines
// ============================================================================

#[tauri::command]
pub async fn workflow_plans_preview(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_preview(request).await
}

#[tauri::command]
pub async fn workflow_plans_apply(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_apply(request).await
}

#[tauri::command]
pub async fn mission_builder_preview(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.mission_builder_preview(request).await
}

#[tauri::command]
pub async fn mission_builder_apply(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.mission_builder_apply(request).await
}

#[tauri::command]
pub async fn workflow_plans_chat_start(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_chat_start(request).await
}

#[tauri::command]
pub async fn workflow_plans_chat_message(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_chat_message(request).await
}

#[tauri::command]
pub async fn workflow_plans_chat_reset(
    state: State<'_, AppState>,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_chat_reset(request).await
}

#[tauri::command]
pub async fn workflow_plans_get(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.workflow_plans_get(&plan_id).await
}

#[tauri::command]
pub async fn automations_v2_list(state: State<'_, AppState>) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_list().await
}

#[tauri::command]
pub async fn automations_v2_get(
    state: State<'_, AppState>,
    automation_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_get(&automation_id).await
}

#[tauri::command]
pub async fn automations_v2_update(
    state: State<'_, AppState>,
    automation_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_update(&automation_id, request)
        .await
}

#[tauri::command]
pub async fn automations_v2_delete(
    state: State<'_, AppState>,
    automation_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_delete(&automation_id).await
}

#[tauri::command]
pub async fn automations_v2_run_now(
    state: State<'_, AppState>,
    automation_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_run_now(&automation_id).await
}

#[tauri::command]
pub async fn automations_v2_pause(
    state: State<'_, AppState>,
    automation_id: String,
    request: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_pause(
            &automation_id,
            request.unwrap_or_else(|| serde_json::json!({ "reason": "" })),
        )
        .await
}

#[tauri::command]
pub async fn automations_v2_resume(
    state: State<'_, AppState>,
    automation_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_resume(&automation_id).await
}

#[tauri::command]
pub async fn automations_v2_runs(
    state: State<'_, AppState>,
    automation_id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_runs(&automation_id, limit)
        .await
}

#[tauri::command]
pub async fn automations_v2_runs_all(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_runs_all(limit).await
}

#[tauri::command]
pub async fn automations_v2_run_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.automations_v2_run_get(&run_id).await
}

#[tauri::command]
pub async fn automations_v2_run_pause(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_pause(
            &run_id,
            request.unwrap_or_else(|| serde_json::json!({ "reason": "" })),
        )
        .await
}

#[tauri::command]
pub async fn automations_v2_run_resume(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_resume(
            &run_id,
            request.unwrap_or_else(|| serde_json::json!({ "reason": "" })),
        )
        .await
}

#[tauri::command]
pub async fn automations_v2_run_cancel(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_cancel(
            &run_id,
            request.unwrap_or_else(|| serde_json::json!({ "reason": "" })),
        )
        .await
}

#[tauri::command]
pub async fn automations_v2_run_gate_decide(
    state: State<'_, AppState>,
    run_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_gate_decide(&run_id, request)
        .await
}

#[tauri::command]
pub async fn automations_v2_run_recover(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_recover(
            &run_id,
            request.unwrap_or_else(|| serde_json::json!({ "reason": "" })),
        )
        .await
}

#[tauri::command]
pub async fn automations_v2_run_repair(
    state: State<'_, AppState>,
    run_id: String,
    request: serde_json::Value,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .automations_v2_run_repair(&run_id, request)
        .await
}

#[tauri::command]
pub async fn routines_list(state: State<'_, AppState>) -> Result<Vec<RoutineSpec>> {
    state.sidecar.routines_list().await
}

#[tauri::command]
pub async fn routines_create(
    state: State<'_, AppState>,
    request: RoutineCreateRequest,
) -> Result<RoutineSpec> {
    state.sidecar.routines_create(request).await
}

#[tauri::command]
pub async fn routines_patch(
    state: State<'_, AppState>,
    routine_id: String,
    request: RoutinePatchRequest,
) -> Result<RoutineSpec> {
    state.sidecar.routines_patch(&routine_id, request).await
}

#[tauri::command]
pub async fn routines_delete(state: State<'_, AppState>, routine_id: String) -> Result<bool> {
    state.sidecar.routines_delete(&routine_id).await
}

#[tauri::command]
pub async fn routines_run_now(
    state: State<'_, AppState>,
    routine_id: String,
    request: Option<RoutineRunNowRequest>,
) -> Result<RoutineRunNowResponse> {
    state
        .sidecar
        .routines_run_now(&routine_id, request.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn routines_history(
    state: State<'_, AppState>,
    routine_id: String,
    limit: Option<usize>,
) -> Result<Vec<RoutineHistoryEvent>> {
    state.sidecar.routines_history(&routine_id, limit).await
}

#[tauri::command]
pub async fn routines_runs(
    state: State<'_, AppState>,
    routine_id: String,
    limit: Option<usize>,
) -> Result<Vec<RoutineRunRecord>> {
    state.sidecar.routines_runs(&routine_id, limit).await
}

#[tauri::command]
pub async fn routines_runs_all(
    state: State<'_, AppState>,
    routine_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<RoutineRunRecord>> {
    state
        .sidecar
        .routines_runs_all(routine_id.as_deref(), limit)
        .await
}

#[tauri::command]
pub async fn routines_run_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RoutineRunRecord> {
    state.sidecar.routines_run_get(&run_id).await
}

#[tauri::command]
pub async fn routines_run_approve(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<RoutineRunDecisionRequest>,
) -> Result<RoutineRunRecord> {
    state
        .sidecar
        .routines_run_approve(&run_id, request.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn routines_run_deny(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<RoutineRunDecisionRequest>,
) -> Result<RoutineRunRecord> {
    state
        .sidecar
        .routines_run_deny(&run_id, request.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn routines_run_pause(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<RoutineRunDecisionRequest>,
) -> Result<RoutineRunRecord> {
    state
        .sidecar
        .routines_run_pause(&run_id, request.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn routines_run_resume(
    state: State<'_, AppState>,
    run_id: String,
    request: Option<RoutineRunDecisionRequest>,
) -> Result<RoutineRunRecord> {
    state
        .sidecar
        .routines_run_resume(&run_id, request.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn routines_run_artifacts(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Vec<RoutineRunArtifact>> {
    state.sidecar.routines_run_artifacts(&run_id).await
}

#[tauri::command]
pub async fn routines_run_add_artifact(
    state: State<'_, AppState>,
    run_id: String,
    request: RoutineRunArtifactAddRequest,
) -> Result<RoutineRunRecord> {
    state
        .sidecar
        .routines_run_add_artifact(&run_id, request)
        .await
}

#[tauri::command]
pub async fn mission_list(state: State<'_, AppState>) -> Result<Vec<MissionState>> {
    state.sidecar.mission_list().await
}

#[tauri::command]
pub async fn mission_create(
    state: State<'_, AppState>,
    request: MissionCreateRequest,
) -> Result<MissionState> {
    state.sidecar.mission_create(request).await
}

#[tauri::command]
pub async fn mission_get(state: State<'_, AppState>, mission_id: String) -> Result<MissionState> {
    state.sidecar.mission_get(&mission_id).await
}

#[tauri::command]
pub async fn mission_apply_event(
    state: State<'_, AppState>,
    mission_id: String,
    event: serde_json::Value,
) -> Result<MissionApplyEventResult> {
    state.sidecar.mission_apply_event(&mission_id, event).await
}

#[tauri::command]
pub async fn agent_team_list_templates(
    state: State<'_, AppState>,
) -> Result<Vec<AgentTeamTemplate>> {
    state.sidecar.agent_team_list_templates().await
}

#[tauri::command]
pub async fn agent_team_list_instances(
    state: State<'_, AppState>,
    mission_id: Option<String>,
    parent_instance_id: Option<String>,
    status: Option<String>,
) -> Result<Vec<AgentTeamInstance>> {
    state
        .sidecar
        .agent_team_list_instances(AgentTeamInstancesQuery {
            mission_id,
            parent_instance_id,
            status,
        })
        .await
}

#[tauri::command]
pub async fn agent_team_list_missions(
    state: State<'_, AppState>,
) -> Result<Vec<AgentTeamMissionSummary>> {
    state.sidecar.agent_team_list_missions().await
}

#[tauri::command]
pub async fn agent_team_list_approvals(state: State<'_, AppState>) -> Result<AgentTeamApprovals> {
    state.sidecar.agent_team_list_approvals().await
}

#[tauri::command]
pub async fn agent_team_spawn(
    state: State<'_, AppState>,
    request: AgentTeamSpawnRequest,
) -> Result<AgentTeamSpawnResult> {
    state.sidecar.agent_team_spawn(request).await
}

#[tauri::command]
pub async fn agent_team_cancel_instance(
    state: State<'_, AppState>,
    instance_id: String,
    reason: Option<String>,
) -> Result<AgentTeamDecisionResult> {
    state
        .sidecar
        .agent_team_cancel_instance(&instance_id, AgentTeamCancelRequest { reason })
        .await
}

#[tauri::command]
pub async fn agent_team_cancel_mission(
    state: State<'_, AppState>,
    mission_id: String,
    reason: Option<String>,
) -> Result<AgentTeamDecisionResult> {
    state
        .sidecar
        .agent_team_cancel_mission(&mission_id, AgentTeamCancelRequest { reason })
        .await
}

#[tauri::command]
pub async fn agent_team_approve_spawn(
    state: State<'_, AppState>,
    approval_id: String,
    reason: Option<String>,
) -> Result<AgentTeamDecisionResult> {
    state
        .sidecar
        .agent_team_approve_spawn(&approval_id, AgentTeamCancelRequest { reason })
        .await
}

#[tauri::command]
pub async fn agent_team_deny_spawn(
    state: State<'_, AppState>,
    approval_id: String,
    reason: Option<String>,
) -> Result<AgentTeamDecisionResult> {
    state
        .sidecar
        .agent_team_deny_spawn(&approval_id, AgentTeamCancelRequest { reason })
        .await
}

// ============================================================================
// Execution Planning / Staging Area
// ============================================================================

/// Stage a tool operation for batch execution
#[tauri::command]
pub async fn stage_tool_operation(
    app: AppHandle,
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

    let mode = effective_session_mode(&state, &session_id);
    if let Err(e) = crate::modes::mode_allows_tool_execution(
        &mode,
        state.get_workspace_path().as_deref(),
        &tool,
        &args,
    ) {
        let _ = state.sidecar.deny_tool(&session_id, &request_id).await;
        return Err(e);
    }

    // If auto-approved, execute immediately instead of staging
    if should_auto_approve {
        // Defense in depth: ensure any terminal tool can't bypass python policy via auto-approve.
        let ws = state
            .get_workspace_path()
            .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;
        if let Some(msg) = tool_policy::python_policy_violation(&ws, tool.as_str(), &args) {
            let _ = app.emit(
                "python-setup-required",
                serde_json::json!({
                    "reason": msg,
                    "workspace_path": ws.to_string_lossy().to_string()
                }),
            );
            return Err(TandemError::PermissionDenied(msg));
        }

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
pub async fn execute_staged_plan(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>> {
    let operations = state.staging_store.get_all();
    let mut executed_ids = Vec::new();

    tracing::info!("Executing staged plan with {} operations", operations.len());

    // Preflight: enforce strict python policy across all staged operations before approving any.
    let ws = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;
    for op in &operations {
        let mode = effective_session_mode(&state, &op.session_id);
        if let Err(e) =
            crate::modes::mode_allows_tool_execution(&mode, Some(ws.as_path()), &op.tool, &op.args)
        {
            let _ = state
                .sidecar
                .deny_tool(&op.session_id, &op.request_id)
                .await;
            return Err(e);
        }

        if let Some(msg) = tool_policy::python_policy_violation(&ws, op.tool.as_str(), &op.args) {
            tracing::info!(
                "[python_policy] Blocking staged plan execution due to terminal op {} ({})",
                op.id,
                op.tool
            );
            // Best-effort: notify UI to open the wizard immediately.
            let _ = app.emit(
                "python-setup-required",
                serde_json::json!({
                    "reason": msg,
                    "workspace_path": ws.to_string_lossy().to_string()
                }),
            );
            return Err(TandemError::PermissionDenied(msg));
        }
    }

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
