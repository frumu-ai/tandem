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

/// Get sequenced run events (tail + since-seq).
#[tauri::command]
pub async fn orchestrator_get_events(
    state: State<'_, AppState>,
    run_id: String,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Result<Vec<crate::orchestrator::types::RunEventRecord>> {
    if let Ok(events) = state
        .sidecar
        .context_run_events(&run_id, since_seq, tail)
        .await
    {
        return Ok(events
            .into_iter()
            .map(context_event_to_run_event)
            .collect::<Vec<_>>());
    }
    // Compatibility fallback for legacy local-orchestrator runs.
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;
    let store = OrchestratorStore::new(&workspace_path)?;
    store.load_run_events(&run_id, since_seq, tail)
}

/// Get materialized blackboard for a run (engine-owned, read-only).
#[tauri::command]
pub async fn orchestrator_get_blackboard(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Blackboard> {
    // Prefer engine-derived context run blackboard (single source of truth).
    if let Ok(blackboard) = state.sidecar.context_run_blackboard(&run_id).await {
        return Ok(context_blackboard_to_orch(blackboard));
    }
    // Compatibility fallback for legacy local-orchestrator runs.
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;
    let store = OrchestratorStore::new(&workspace_path)?;
    store.load_blackboard(&run_id)
}

/// Get append-only blackboard patches for a run.
#[tauri::command]
pub async fn orchestrator_get_blackboard_patches(
    state: State<'_, AppState>,
    run_id: String,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Result<Vec<BlackboardPatchRecord>> {
    if let Ok(patches) = state
        .sidecar
        .context_run_blackboard_patches(&run_id, since_seq, tail)
        .await
    {
        return Ok(patches
            .into_iter()
            .filter_map(context_blackboard_patch_to_orch)
            .collect::<Vec<_>>());
    }
    // Compatibility fallback for legacy local-orchestrator runs.
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;
    let store = OrchestratorStore::new(&workspace_path)?;
    store.load_blackboard_patches(&run_id, since_seq, tail)
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

/// Re-queue a failed task so it can run again without restarting the whole run.
#[tauri::command]
pub async fn orchestrator_retry_task(
    state: State<'_, AppState>,
    run_id: String,
    task_id: String,
) -> Result<()> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    engine.retry_failed_task(&task_id).await?;
    Ok(())
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

/// Extend budget limits for a run so users can continue long workflows.
#[tauri::command]
pub async fn orchestrator_extend_budget(
    state: State<'_, AppState>,
    run_id: String,
    add_iterations: Option<u32>,
    add_tokens: Option<u64>,
    add_wall_time_secs: Option<u64>,
    add_subagent_runs: Option<u32>,
    clear_caps: Option<bool>,
) -> Result<RunSnapshot> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    engine
        .extend_budget_limits(
            add_iterations.unwrap_or(0),
            add_tokens.unwrap_or(0),
            add_wall_time_secs.unwrap_or(0),
            add_subagent_runs.unwrap_or(0),
            clear_caps.unwrap_or(false),
        )
        .await
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
pub async fn orchestrator_get_model_routing(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<OrchestratorModelRouting> {
    let engine = {
        let engines = state.orchestrator_engines.read().unwrap();
        engines
            .get(&run_id)
            .cloned()
            .ok_or_else(|| TandemError::NotFound(format!("Run not found: {}", run_id)))?
    };

    let routing = engine.get_run_model_routing().await;
    Ok(to_orchestrator_model_routing(routing))
}

#[tauri::command]
pub async fn orchestrator_set_model_routing(
    state: State<'_, AppState>,
    run_id: String,
    routing: OrchestratorModelRouting,
) -> Result<OrchestratorModelRouting> {
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
            "Run must be paused, failed, or cancelled to change agent model routing".to_string(),
        ));
    }

    let normalized = normalize_orchestrator_model_routing(Some(routing));
    engine.set_run_model_routing(normalized.clone()).await?;
    Ok(to_orchestrator_model_routing(normalized))
}

#[tauri::command]
pub async fn orchestrator_set_resume_model(
    app: AppHandle,
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
    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    validate_model_provider_auth_if_required(
        &app,
        &config_snapshot,
        Some(model.as_str()),
        normalized_provider.as_deref(),
    )
    .await?;
    let request = CreateSessionRequest {
        parent_id: Some(parent_id),
        title: Some(format!(
            "Orchestrator Resume: {}",
            &snapshot.objective[..snapshot.objective.len().min(50)]
        )),
        model: build_sidecar_session_model(Some(model.clone()), normalized_provider.clone()),
        provider: normalized_provider.clone(),
        permission: Some(orchestrator_permission_rules()),
        directory: state
            .get_workspace_path()
            .map(|p| p.to_string_lossy().to_string()),
        workspace_root: state
            .get_workspace_path()
            .map(|p| p.to_string_lossy().to_string()),
        project_id: None,
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

    // Restart planning/execution loop.
    // If planning previously failed before a plan existed, resume should retry planning
    // instead of trying to execute an empty task list.
    tauri::async_runtime::spawn(async move {
        let result = if engine.get_tasks().await.is_empty() {
            engine.start().await
        } else {
            engine.execute().await
        };
        if let Err(e) = result {
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
    // Prefer context-runs when available, but merge legacy local runs as well so
    // orchestrator sessions created through the desktop legacy path don't disappear.
    let mut summaries = if let Ok(rows) = state.sidecar.context_run_list().await {
        rows.into_iter()
            .filter_map(|run| {
                let source = context_run_source(&run.run_type)?;
                Some(RunSummary {
                    run_id: run.run_id.clone(),
                    session_id: format!("context-{}", run.run_id),
                    workspace_root: Some(run.workspace.canonical_path),
                    source,
                    objective: run.objective,
                    status: context_status_to_run_status(run.status),
                    created_at: ms_to_datetime(run.created_at_ms),
                    updated_at: ms_to_datetime(run.updated_at_ms),
                    started_at: ms_to_datetime(run.created_at_ms),
                    ended_at: None,
                    last_error: None,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // Also include legacy local-orchestrator runs (dedup by run_id).
    // Get workspace path
    let workspace_path = {
        let path_guard = state.workspace_path.read().unwrap();
        path_guard.clone()
    };

    if let Some(workspace_path) = workspace_path {
        let runs_dir = workspace_path.join(".tandem").join("orchestrator");
        if runs_dir.exists() {
            let store = OrchestratorStore::new(&workspace_path)?;
            if let Ok(entries) = fs::read_dir(&runs_dir) {
                for entry in entries.flatten() {
                    if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let run_id = entry.file_name().to_string_lossy().to_string();
                    if summaries.iter().any(|row| row.run_id == run_id) {
                        continue;
                    }
                    if let Ok(run) = store.load_run(&run_id) {
                        let ended_at = run.ended_at;
                        let updated_at = ended_at.unwrap_or_else(chrono::Utc::now);
                        let last_error = run.error_message.as_ref().and_then(|msg| {
                            let trimmed = msg.trim();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.chars().take(220).collect::<String>())
                            }
                        });
                        summaries.push(RunSummary {
                            run_id: run.run_id,
                            session_id: run.session_id,
                            workspace_root: run.workspace_root,
                            source: run.source,
                            objective: run.objective,
                            status: run.status,
                            created_at: run.started_at,
                            updated_at,
                            started_at: run.started_at,
                            ended_at,
                            last_error,
                        });
                    }
                }
            }
        }
    }

    summaries.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
    Ok(summaries)
}

/// Load a specific run from disk
#[tauri::command]
pub async fn orchestrator_load_run(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Run> {
    if let Ok(run) = state.sidecar.context_run_get(&run_id).await {
        return Ok(context_run_to_run(&run));
    }
    // Compatibility fallback for legacy local-orchestrator runs.
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::NotFound("No workspace configured".to_string()))?;

    let store = OrchestratorStore::new(&workspace_path)?;
    let mut run = store.load_run(&run_id)?;
    let mut checkpoint_task_sessions: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Ok(Some(checkpoint)) = store.load_latest_checkpoint(&run_id) {
        checkpoint_task_sessions = checkpoint.task_sessions;
        run = checkpoint.run;
    }

    // Check if engine already exists in memory
    {
        let engines = state.orchestrator_engines.read().unwrap();
        if engines.contains_key(&run_id) {
            return Ok(run);
        }
    }

    let run_workspace_path = run
        .workspace_root
        .as_deref()
        .and_then(normalize_workspace_path)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| workspace_path.clone());

    // Re-hydrate engine
    let policy_config = PolicyConfig::new(run_workspace_path.clone());
    let policy = PolicyEngine::new(policy_config);

    // Channel for events
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Create engine
    // NOTE: When loading a run, we should try to update its config to the latest defaults
    // if the existing config seems to have the old low limits.
    // However, the `Run` struct loaded from disk has the OLD config.
    // We should patch the config here before creating the engine.
    let mut run_to_load = run.clone();
    run_to_load.workspace_root = Some(run_workspace_path.to_string_lossy().to_string());
    run_to_load.agent_model_routing = run_to_load.agent_model_routing.canonicalized();
    for task in run_to_load.tasks.iter_mut() {
        let normalized = crate::orchestrator::types::normalize_role_key(&task.assigned_role);
        if normalized.trim().is_empty() {
            task.assigned_role = crate::orchestrator::types::ROLE_WORKER.to_string();
        } else {
            task.assigned_role = normalized;
        }
        let missing_task_session = task
            .session_id
            .as_deref()
            .map(|sid| sid.trim().is_empty())
            .unwrap_or(true);
        if missing_task_session {
            if let Some(restored) = checkpoint_task_sessions.get(&task.id) {
                task.session_id = Some(restored.clone());
            }
        }
    }

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
    if is_legacy_low_wall_time_for_command_center(
        run_to_load.source,
        run_to_load.config.max_wall_time_secs,
    ) {
        run_to_load.config.max_wall_time_secs = 48 * 60 * 60;
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
        state.stream_hub.clone(),
        run_workspace_path,
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
        if current_status == RunStatus::Running || current_status == RunStatus::Planning {
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
    // New engine-backed context runs (Command Center + newer orchestrator flow)
    // are persisted under shared `data/context_runs`. Delete those directly when present.
    if state.sidecar.context_run_get(&run_id).await.is_ok() {
        if let Some(engine) = {
            let mut engines = state.orchestrator_engines.write().unwrap();
            engines.remove(&run_id)
        } {
            let _ = engine.cancel_and_finalize().await;
        }
        if let Ok(paths) = resolve_shared_paths() {
            let context_run_dir = paths
                .canonical_root
                .join("data")
                .join("context_runs")
                .join(&run_id);
            if context_run_dir.exists() {
                fs::remove_dir_all(&context_run_dir)?;
            }
        }
        return Ok(());
    }

    // Legacy local orchestrator runs
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

// ============================================================================
