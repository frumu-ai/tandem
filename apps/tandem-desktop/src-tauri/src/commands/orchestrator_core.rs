// ============================================================================
// Orchestrator Commands
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrchestratorModelSelection {
    pub model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrchestratorModelRouting {
    #[serde(
        default,
        skip_serializing_if = "std::collections::HashMap::is_empty",
        flatten
    )]
    pub dynamic_roles: std::collections::HashMap<String, OrchestratorModelSelection>,
    // Optional compatibility bucket form: { roles: { ... } }
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub roles: std::collections::HashMap<String, OrchestratorModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<OrchestratorModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder: Option<OrchestratorModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<OrchestratorModelSelection>,
}

fn normalize_orchestrator_model_selection(
    selection: Option<OrchestratorModelSelection>,
) -> Option<ModelSelection> {
    let selection = selection?;
    let model = selection
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let provider = selection
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .map(|p| {
            normalize_provider_id_for_sidecar(Some(p)).unwrap_or_else(|| "opencode".to_string())
        });

    if model.is_none() && provider.is_none() {
        return None;
    }

    Some(ModelSelection { model, provider })
}

fn to_orchestrator_model_selection(
    selection: Option<ModelSelection>,
) -> Option<OrchestratorModelSelection> {
    selection.map(|s| OrchestratorModelSelection {
        model: s.model,
        provider: s.provider,
    })
}

fn normalize_orchestrator_model_routing(
    input: Option<OrchestratorModelRouting>,
) -> AgentModelRouting {
    let Some(input) = input else {
        return AgentModelRouting::default();
    };

    let mut roles = std::collections::HashMap::new();
    for (role, selection) in input.roles {
        if let Some(normalized_selection) = normalize_orchestrator_model_selection(Some(selection))
        {
            let normalized_role = crate::orchestrator::types::normalize_role_key(&role);
            roles.entry(normalized_role).or_insert(normalized_selection);
        }
    }
    for (role, selection) in input.dynamic_roles {
        if let Some(normalized_selection) = normalize_orchestrator_model_selection(Some(selection))
        {
            let normalized_role = crate::orchestrator::types::normalize_role_key(&role);
            roles.entry(normalized_role).or_insert(normalized_selection);
        }
    }

    if let Some(selection) = normalize_orchestrator_model_selection(input.planner) {
        roles
            .entry(crate::orchestrator::types::ROLE_ORCHESTRATOR.to_string())
            .or_insert(selection);
    }
    if let Some(selection) = normalize_orchestrator_model_selection(input.builder) {
        roles
            .entry(crate::orchestrator::types::ROLE_WORKER.to_string())
            .or_insert(selection);
    }
    if let Some(selection) = normalize_orchestrator_model_selection(input.validator) {
        roles
            .entry(crate::orchestrator::types::ROLE_REVIEWER.to_string())
            .or_insert(selection);
    }

    AgentModelRouting {
        roles,
        planner: None,
        builder: None,
        validator: None,
    }
    .canonicalized()
}

fn to_orchestrator_model_routing(routing: AgentModelRouting) -> OrchestratorModelRouting {
    let mut roles = std::collections::HashMap::new();
    for (role, selection) in routing.canonicalized().roles {
        if let Some(serialized) = to_orchestrator_model_selection(Some(selection)) {
            roles.insert(role, serialized);
        }
    }

    OrchestratorModelRouting {
        dynamic_roles: roles.clone(),
        roles: std::collections::HashMap::new(),
        planner: None,
        builder: None,
        validator: None,
    }
}

fn orchestrator_permission_rules() -> Vec<crate::sidecar::PermissionRule> {
    let allowed_tools = vec![
        "ls".to_string(),
        "list".to_string(),
        "glob".to_string(),
        "search".to_string(),
        "grep".to_string(),
        "codesearch".to_string(),
        "read".to_string(),
        "todowrite".to_string(),
        "todo_write".to_string(),
        "update_todo_list".to_string(),
        "websearch".to_string(),
        "webfetch".to_string(),
        "webfetch_html".to_string(),
        "task".to_string(),
    ];

    let mut rules = tandem_core::build_mode_permission_rules(Some(&allowed_tools))
        .into_iter()
        .map(|rule| crate::sidecar::PermissionRule {
            permission: rule.permission,
            pattern: rule.pattern,
            action: rule.action,
        })
        .collect::<Vec<_>>();

    for permission in [
        "todowrite",
        "todo_write",
        "update_todo_list",
        "websearch",
        "webfetch",
        "webfetch_html",
        "task",
    ] {
        rules.push(crate::sidecar::PermissionRule {
            permission: permission.to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    for permission in [
        "write",
        "edit",
        "apply_patch",
        "bash",
        "batch",
        "spawn_agent",
    ] {
        rules.push(crate::sidecar::PermissionRule {
            permission: permission.to_string(),
            pattern: "*".to_string(),
            action: "deny".to_string(),
        });
    }

    rules
}

fn normalize_provider_id_for_sidecar(provider: Option<String>) -> Option<String> {
    provider.map(|p| {
        if p == "opencode_zen" {
            "opencode".to_string()
        } else {
            p
        }
    })
}

fn build_sidecar_session_model(
    model: Option<String>,
    provider: Option<String>,
) -> Option<serde_json::Value> {
    match (model, provider) {
        (Some(model_id), Some(provider_id)) => Some(serde_json::json!({
            "providerID": provider_id,
            "modelID": model_id
        })),
        _ => None,
    }
}

fn canonical_workspace_dir_for_session(state: &AppState) -> Option<String> {
    state.get_workspace_path().and_then(|p| {
        if p.is_dir() {
            normalize_workspace_path(&p.to_string_lossy())
        } else {
            tracing::warn!(
                "Workspace path no longer exists or is not a directory: {}. Falling back to sidecar default cwd.",
                p.display()
            );
            None
        }
    })
}

fn orchestrator_strict_contract_flag() -> bool {
    match std::env::var("TANDEM_ORCH_STRICT_CONTRACT") {
        Ok(v) => matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => cfg!(debug_assertions),
    }
}

fn is_legacy_low_wall_time_for_command_center(source: RunSource, wall_time_secs: u64) -> bool {
    matches!(source, RunSource::CommandCenter) && wall_time_secs <= 60 * 60
}

fn context_status_to_run_status(status: ContextRunStatus) -> RunStatus {
    match status {
        ContextRunStatus::Queued => RunStatus::Queued,
        ContextRunStatus::Planning => RunStatus::Planning,
        ContextRunStatus::Running => RunStatus::Running,
        ContextRunStatus::AwaitingApproval => RunStatus::AwaitingApproval,
        ContextRunStatus::Paused => RunStatus::Paused,
        ContextRunStatus::Blocked => RunStatus::Blocked,
        ContextRunStatus::Failed => RunStatus::Failed,
        ContextRunStatus::Completed => RunStatus::Completed,
        ContextRunStatus::Cancelled => RunStatus::Cancelled,
    }
}

fn context_step_status_to_task_state(status: ContextStepStatus) -> TaskState {
    match status {
        ContextStepStatus::Pending => TaskState::Pending,
        ContextStepStatus::Runnable => TaskState::Runnable,
        ContextStepStatus::InProgress => TaskState::InProgress,
        ContextStepStatus::Blocked => TaskState::Blocked,
        ContextStepStatus::Done => TaskState::Done,
        ContextStepStatus::Failed => TaskState::Failed,
    }
}

fn context_task_kind_from_record(task_type: &str, payload: &serde_json::Value) -> Option<TaskKind> {
    let raw = payload
        .get("task_kind")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            let trimmed = task_type.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })?
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "implementation" => Some(TaskKind::Implementation),
        "inspection" => Some(TaskKind::Inspection),
        "research" => Some(TaskKind::Research),
        "validation" => Some(TaskKind::Validation),
        _ => None,
    }
}

fn context_execution_mode_from_payload(payload: &serde_json::Value) -> Option<TaskExecutionMode> {
    match payload
        .get("execution_mode")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())?
        .to_ascii_lowercase()
        .as_str()
    {
        "strict_write" => Some(TaskExecutionMode::StrictWrite),
        "strict_nonwriting" => Some(TaskExecutionMode::StrictNonwriting),
        "best_effort" => Some(TaskExecutionMode::BestEffort),
        _ => None,
    }
}

fn context_output_target_from_payload(payload: &serde_json::Value) -> Option<OutputTarget> {
    let target = payload.get("output_target")?;
    let path = target
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())?
        .to_string();
    Some(OutputTarget {
        path,
        kind: target
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .map(ToString::to_string),
        operation: target
            .get("operation")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .map(ToString::to_string),
    })
}

fn ms_to_datetime(ms: u64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_else(chrono::Utc::now)
}

fn context_run_to_snapshot(run: &ContextRunState) -> RunSnapshot {
    let has_backend_tasks = !run.tasks.is_empty();
    let tasks_completed = if has_backend_tasks {
        run.tasks
            .iter()
            .filter(|task| task.status == ContextStepStatus::Done)
            .count()
    } else {
        run.steps
            .iter()
            .filter(|step| step.status == ContextStepStatus::Done)
            .count()
    };
    let tasks_failed = if has_backend_tasks {
        run.tasks
            .iter()
            .filter(|task| task.status == ContextStepStatus::Failed)
            .count()
    } else {
        run.steps
            .iter()
            .filter(|step| step.status == ContextStepStatus::Failed)
            .count()
    };
    let current_task_id = if has_backend_tasks {
        run.tasks
            .iter()
            .find(|task| task.status == ContextStepStatus::InProgress)
            .map(|task| task.id.clone())
    } else {
        run.steps
            .iter()
            .find(|step| step.status == ContextStepStatus::InProgress)
            .map(|step| step.step_id.clone())
    };
    RunSnapshot {
        run_id: run.run_id.clone(),
        status: context_status_to_run_status(run.status.clone()),
        objective: run.objective.clone(),
        task_count: if has_backend_tasks {
            run.tasks.len()
        } else {
            run.steps.len()
        },
        tasks_completed,
        tasks_failed,
        budget: Budget::from_config(&OrchestratorConfig::default()),
        current_task_id,
        error_message: None,
        created_at: ms_to_datetime(run.created_at_ms),
        updated_at: ms_to_datetime(run.updated_at_ms),
    }
}

fn context_run_to_tasks(run: &ContextRunState) -> Vec<Task> {
    if !run.tasks.is_empty() {
        return run
            .tasks
            .iter()
            .map(|row| {
                let payload = &row.payload;
                let title = payload
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| row.task_type.trim());
                let description = payload
                    .get("summary")
                    .or_else(|| payload.get("description"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("Backend blackboard task {}", row.id));
                let mut task = Task::new(row.id.clone(), title.to_string(), description);
                task.state = context_step_status_to_task_state(row.status.clone());
                task.dependencies = row.depends_on_task_ids.clone();
                task.retry_count = row.attempt;
                task.error_message = row.last_error.clone();
                task.output_target = context_output_target_from_payload(payload);
                task.task_kind = context_task_kind_from_record(&row.task_type, payload);
                task.execution_mode = context_execution_mode_from_payload(payload);
                task
            })
            .collect();
    }
    run.steps
        .iter()
        .map(|step| {
            let mut task = Task::new(
                step.step_id.clone(),
                step.title.clone(),
                format!("Engine context step {}", step.step_id),
            );
            task.state = context_step_status_to_task_state(step.status.clone());
            task
        })
        .collect()
}

fn context_run_to_run(run: &ContextRunState) -> Run {
    let mut out = Run::new(
        run.run_id.clone(),
        format!("context-{}", run.run_id),
        run.objective.clone(),
        OrchestratorConfig::default(),
    );
    out.status = context_status_to_run_status(run.status.clone());
    out.tasks = context_run_to_tasks(run);
    out.started_at = ms_to_datetime(run.created_at_ms);
    out.ended_at = Some(ms_to_datetime(run.updated_at_ms));
    out.why_next_step = run.why_next_step.clone();
    out.workspace_lease.workspace_id = run.workspace.workspace_id.clone();
    out.workspace_lease.canonical_path = run.workspace.canonical_path.clone();
    out.workspace_lease.lease_epoch = run.workspace.lease_epoch;
    out
}

fn context_run_source(run_type: &str) -> Option<RunSource> {
    let normalized = run_type.trim();
    if normalized.eq_ignore_ascii_case("interactive")
        || normalized.eq_ignore_ascii_case("orchestrator")
    {
        Some(RunSource::Orchestrator)
    } else if normalized.eq_ignore_ascii_case("scheduled")
        || normalized.eq_ignore_ascii_case("cron")
        || normalized.eq_ignore_ascii_case("command_center")
        || normalized.eq_ignore_ascii_case("command-center")
    {
        Some(RunSource::CommandCenter)
    } else {
        None
    }
}

fn context_event_to_run_event(
    event: ContextRunEventRecord,
) -> crate::orchestrator::types::RunEventRecord {
    crate::orchestrator::types::RunEventRecord {
        event_id: event.event_id,
        run_id: event.run_id,
        seq: event.seq,
        ts_ms: event.ts_ms,
        event_type: event.event_type,
        status: context_status_to_run_status(event.status),
        step_id: event.step_id,
        tenant_context: tandem_enterprise_contract::LocalImplicitTenant.into(),
        authority_chain: None,
        payload: event.payload,
    }
}

fn context_blackboard_to_orch(blackboard: ContextBlackboardState) -> Blackboard {
    let facts = blackboard
        .facts
        .into_iter()
        .map(|row| crate::orchestrator::types::BlackboardItem {
            id: row.id,
            ts_ms: row.ts_ms,
            text: row.text,
            step_id: row.step_id,
            source_event_id: row.source_event_id,
        })
        .collect::<Vec<_>>();
    let decisions = blackboard
        .decisions
        .into_iter()
        .map(|row| crate::orchestrator::types::BlackboardItem {
            id: row.id,
            ts_ms: row.ts_ms,
            text: row.text,
            step_id: row.step_id,
            source_event_id: row.source_event_id,
        })
        .collect::<Vec<_>>();
    let open_questions = blackboard
        .open_questions
        .into_iter()
        .map(|row| crate::orchestrator::types::BlackboardItem {
            id: row.id,
            ts_ms: row.ts_ms,
            text: row.text,
            step_id: row.step_id,
            source_event_id: row.source_event_id,
        })
        .collect::<Vec<_>>();
    let artifacts = blackboard
        .artifacts
        .into_iter()
        .map(|row| crate::orchestrator::types::BlackboardArtifactRef {
            id: row.id,
            ts_ms: row.ts_ms,
            path: row.path,
            artifact_type: match row.artifact_type.as_str() {
                "patch" => crate::orchestrator::types::ArtifactType::Patch,
                "notes" => crate::orchestrator::types::ArtifactType::Notes,
                "sources" => crate::orchestrator::types::ArtifactType::Sources,
                "fact_cards" => crate::orchestrator::types::ArtifactType::FactCards,
                _ => crate::orchestrator::types::ArtifactType::File,
            },
            step_id: row.step_id,
            source_event_id: row.source_event_id,
        })
        .collect::<Vec<_>>();
    Blackboard {
        facts,
        decisions,
        open_questions,
        artifacts,
        summaries: crate::orchestrator::types::BlackboardSummaries {
            rolling: blackboard.summaries.rolling,
            latest_context_pack: blackboard.summaries.latest_context_pack,
        },
        revision: blackboard.revision,
    }
}

fn context_blackboard_patch_to_orch(
    patch: ContextPatchRecord,
) -> Option<crate::orchestrator::types::BlackboardPatchRecord> {
    let op = match patch.op.trim() {
        "add_fact" => crate::orchestrator::types::BlackboardPatchOp::AddFact,
        "add_decision" => crate::orchestrator::types::BlackboardPatchOp::AddDecision,
        "add_open_question" => crate::orchestrator::types::BlackboardPatchOp::AddOpenQuestion,
        "add_artifact" => crate::orchestrator::types::BlackboardPatchOp::AddArtifact,
        "set_rolling_summary" => crate::orchestrator::types::BlackboardPatchOp::SetRollingSummary,
        "set_latest_context_pack" => {
            crate::orchestrator::types::BlackboardPatchOp::SetLatestContextPack
        }
        // Legacy desktop views currently only handle canonical reasoning patch types.
        _ => return None,
    };
    Some(crate::orchestrator::types::BlackboardPatchRecord {
        patch_id: patch.patch_id,
        run_id: patch.run_id,
        seq: patch.seq,
        ts_ms: patch.ts_ms,
        op,
        payload: patch.payload,
    })
}

#[tauri::command]
pub async fn orchestrator_engine_create_run(
    state: State<'_, AppState>,
    objective: String,
    source: Option<RunSource>,
) -> Result<String> {
    let workspace_dir = canonical_workspace_dir_for_session(state.inner())
        .ok_or_else(|| TandemError::InvalidConfig("No workspace selected".to_string()))?;
    let canonical_workspace = std::fs::canonicalize(PathBuf::from(&workspace_dir))
        .unwrap_or_else(|_| PathBuf::from(&workspace_dir))
        .to_string_lossy()
        .to_string();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical_workspace.hash(&mut hasher);
    let run = state
        .sidecar
        .context_run_create(ContextRunCreateRequest {
            run_id: None,
            objective,
            run_type: Some(match source.unwrap_or(RunSource::Orchestrator) {
                RunSource::Orchestrator => "interactive".to_string(),
                RunSource::CommandCenter => "scheduled".to_string(),
            }),
            workspace: Some(crate::sidecar::ContextWorkspaceLease {
                workspace_id: format!("ws-{:x}", hasher.finish()),
                canonical_path: canonical_workspace,
                lease_epoch: 1,
            }),
        })
        .await?;
    Ok(run.run_id)
}

#[tauri::command]
pub async fn orchestrator_engine_start(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "planning_started".to_string(),
                status: ContextRunStatus::Planning,
                step_id: None,
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_get_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<RunSnapshot> {
    let run = state.sidecar.context_run_get(&run_id).await?;
    Ok(context_run_to_snapshot(&run))
}

#[tauri::command]
pub async fn orchestrator_engine_list_tasks(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Vec<Task>> {
    let run = state.sidecar.context_run_get(&run_id).await?;
    Ok(context_run_to_tasks(&run))
}

#[tauri::command]
pub async fn orchestrator_engine_get_events(
    state: State<'_, AppState>,
    run_id: String,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Result<Vec<crate::orchestrator::types::RunEventRecord>> {
    let events = state
        .sidecar
        .context_run_events(&run_id, since_seq, tail)
        .await?;
    Ok(events
        .into_iter()
        .map(context_event_to_run_event)
        .collect::<Vec<_>>())
}

#[tauri::command]
pub async fn orchestrator_engine_get_blackboard(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Blackboard> {
    let blackboard = state.sidecar.context_run_blackboard(&run_id).await?;
    Ok(context_blackboard_to_orch(blackboard))
}

#[tauri::command]
pub async fn orchestrator_engine_get_latest_checkpoint(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Option<ContextCheckpointRecord>> {
    state.sidecar.context_run_checkpoint_latest(&run_id).await
}

#[tauri::command]
pub async fn orchestrator_engine_get_replay(
    state: State<'_, AppState>,
    run_id: String,
    upto_seq: Option<u64>,
    from_checkpoint: Option<bool>,
) -> Result<ContextReplayResponse> {
    state
        .sidecar
        .context_run_replay(&run_id, upto_seq, from_checkpoint)
        .await
}

#[tauri::command]
pub async fn orchestrator_engine_list_runs(state: State<'_, AppState>) -> Result<Vec<RunSummary>> {
    let rows = state.sidecar.context_run_list().await?;
    Ok(rows
        .into_iter()
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
        .collect::<Vec<_>>())
}

#[tauri::command]
pub async fn orchestrator_engine_load_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Run> {
    let run = state.sidecar.context_run_get(&run_id).await?;
    Ok(context_run_to_run(&run))
}

#[tauri::command]
pub async fn orchestrator_engine_pause(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "run_paused".to_string(),
                status: ContextRunStatus::Paused,
                step_id: None,
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_resume(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "run_resumed".to_string(),
                status: ContextRunStatus::Running,
                step_id: None,
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_cancel(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "run_cancelled".to_string(),
                status: ContextRunStatus::Cancelled,
                step_id: None,
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_approve(state: State<'_, AppState>, run_id: String) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "plan_approved".to_string(),
                status: ContextRunStatus::Running,
                step_id: None,
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_request_revision(
    state: State<'_, AppState>,
    run_id: String,
    feedback: String,
) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "revision_requested".to_string(),
                status: ContextRunStatus::Blocked,
                step_id: None,
                payload: json!({ "feedback": feedback }),
            },
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn orchestrator_engine_retry_task(
    state: State<'_, AppState>,
    run_id: String,
    task_id: String,
) -> Result<()> {
    let _ = state
        .sidecar
        .context_run_append_event(
            &run_id,
            ContextRunEventAppendRequest {
                event_type: "task_retry_requested".to_string(),
                status: ContextRunStatus::Running,
                step_id: Some(task_id),
                payload: json!({}),
            },
        )
        .await?;
    Ok(())
}

/// Create a new orchestration run
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn orchestrator_create_run(
    app: AppHandle,
    state: State<'_, AppState>,
    objective: String,
    config: OrchestratorConfig,
    model: Option<String>,
    provider: Option<String>,
    agent_model_routing: Option<OrchestratorModelRouting>,
    source: Option<RunSource>,
) -> Result<String> {
    use crate::sidecar::CreateSessionRequest;

    let run_id = Uuid::new_v4().to_string();
    let workspace_dir = canonical_workspace_dir_for_session(state.inner())
        .ok_or_else(|| TandemError::InvalidConfig("No workspace selected".to_string()))?;
    let workspace_path = PathBuf::from(&workspace_dir);
    if !workspace_path.is_dir() {
        return Err(TandemError::InvalidConfig(format!(
            "Selected workspace does not exist or is not a directory: {}",
            workspace_path.display()
        )));
    }

    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let resolved_model_spec = resolve_required_model_spec(
        &config_snapshot,
        model,
        provider,
        "Orchestrator run creation",
    )?;
    let final_model = Some(resolved_model_spec.model_id.clone());
    let final_provider = Some(resolved_model_spec.provider_id.clone());
    wait_for_sidecar_api_ready(state.inner(), Duration::from_secs(25)).await?;
    validate_model_provider_in_sidecar_catalog(
        state.inner(),
        final_model.as_deref(),
        final_provider.as_deref(),
    )
    .await?;
    validate_model_provider_auth_if_required(
        &app,
        &config_snapshot,
        Some(resolved_model_spec.model_id.as_str()),
        Some(resolved_model_spec.provider_id.as_str()),
    )
    .await?;

    // Create a NEW session specifically for the orchestrator.
    let session_request = CreateSessionRequest {
        parent_id: None,
        title: Some(format!(
            "Orchestrator: {}",
            &objective[..objective.len().min(50)]
        )),
        // Clone so we can also persist the selection onto the Run object below.
        model: build_sidecar_session_model(final_model.clone(), final_provider.clone()),
        provider: final_provider.clone(),
        permission: Some(orchestrator_permission_rules()),
        directory: Some(workspace_dir.clone()),
        workspace_root: Some(workspace_dir.clone()),
        project_id: None,
    };

    let session = state
        .sidecar
        .create_session(session_request)
        .await
        .map_err(|e| {
            TandemError::Sidecar(format!("Failed to create orchestrator session: {}", e))
        })?;

    let session_id = session.id;
    tracing::info!(
        "Created orchestrator session: {} run_id={} workspace_root={} directory={} model={:?} provider={:?}",
        session_id,
        run_id,
        workspace_dir,
        session.directory.clone().unwrap_or_else(|| ".".to_string()),
        session.model,
        session.provider,
    );
    if let (Some(expected_provider), Some(expected_model)) = (&final_provider, &final_model) {
        if let (Some(actual_provider), Some(actual_model)) = (&session.provider, &session.model) {
            if actual_provider != expected_provider || actual_model != expected_model {
                return Err(TandemError::Sidecar(format!(
                    "Created session model/provider mismatch (expected {} / {}, got {} / {}).",
                    expected_provider, expected_model, actual_provider, actual_model
                )));
            }
        }
    }

    // Guard against old UI defaults when creating new runs.
    let run_source = source.unwrap_or(RunSource::Orchestrator);
    let mut config = config;
    if config.max_iterations == 10 || config.max_iterations == 30 || config.max_iterations == 200 {
        config.max_iterations = 500;
    }
    if config.max_subagent_runs == 20
        || config.max_subagent_runs == 50
        || config.max_subagent_runs == 500
    {
        config.max_subagent_runs = 2000;
    }
    if config.max_wall_time_secs == 20 * 60 {
        config.max_wall_time_secs = 60 * 60;
    }
    if is_legacy_low_wall_time_for_command_center(run_source, config.max_wall_time_secs) {
        config.max_wall_time_secs = 48 * 60 * 60;
    }
    if orchestrator_strict_contract_flag() {
        config.strict_planner_json = true;
        config.strict_validator_json = true;
        config.contract_warnings_enabled = true;
        // Keep fallback enabled in phase 1 unless explicitly disabled by user config later.
        if !config.allow_prose_fallback {
            tracing::warn!(
                "orchestrator strict contract flag enabled with prose fallback disabled via config"
            );
        }
    }

    // Create the run object
    let mut run = Run::new(run_id.clone(), session_id, objective, config);
    run.workspace_root = Some(workspace_dir.clone());
    let canonical_workspace = std::fs::canonicalize(&workspace_path)
        .unwrap_or_else(|_| workspace_path.clone())
        .to_string_lossy()
        .to_string();
    let mut lease_hasher = std::collections::hash_map::DefaultHasher::new();
    canonical_workspace.hash(&mut lease_hasher);
    run.workspace_lease.workspace_id = format!("ws-{:x}", lease_hasher.finish());
    run.workspace_lease.canonical_path = canonical_workspace;
    run.workspace_lease.lease_epoch = 1;
    run.source = run_source;
    // Persist model/provider selection into the run so the orchestrator can always send explicit
    // model specs even if the sidecar session object doesn't echo them back.
    run.model = final_model.clone();
    run.provider = final_provider.clone();
    run.agent_model_routing = normalize_orchestrator_model_routing(agent_model_routing);

    // Initialize dependencies.
    let policy_config = PolicyConfig::new(workspace_path.clone());
    let policy = PolicyEngine::new(policy_config);
    let store = OrchestratorStore::new(&workspace_path)?;

    // Channel for events
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Create engine
    let engine = Arc::new(OrchestratorEngine::new(
        run,
        policy,
        store,
        state.sidecar.clone(),
        state.stream_hub.clone(),
        workspace_path,
        event_tx,
    ));

    // Store engine in state
    {
        let mut engines = state.orchestrator_engines.write().unwrap();
        engines.insert(run_id.clone(), engine.clone());
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

    tracing::info!(
        "Created orchestrator run: run_id={} session_id={} workspace_root={}",
        run_id,
        engine.get_base_session_id().await,
        workspace_dir
    );
    Ok(run_id)
}
