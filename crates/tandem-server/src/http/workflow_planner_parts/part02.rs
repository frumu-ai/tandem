fn planner_session_title_from_record(session: &WorkflowPlannerSessionRecord) -> String {
    let title = session.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    planner_session_default_title(&session.goal, session.created_at_ms)
}

async fn workflow_planner_session_response(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    response: Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan = response.0.get("plan").cloned().unwrap_or(Value::Null);
    let conversation = response
        .0
        .get("conversation")
        .cloned()
        .unwrap_or(Value::Null);
    let planner_diagnostics = response
        .0
        .get("planner_diagnostics")
        .or_else(|| response.0.get("plannerDiagnostics"))
        .cloned()
        .unwrap_or(Value::Null);
    let change_summary = response
        .0
        .get("change_summary")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let clarifier = response
        .0
        .get("clarifier")
        .cloned()
        .unwrap_or_else(|| json!({"status":"none"}));
    let draft = state
        .get_workflow_plan_draft(session.current_plan_id.as_deref().unwrap_or_default())
        .await;
    let mut next_session = session.clone();
    if let Some(draft) = draft {
        next_session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        next_session.draft = Some(draft.clone());
        next_session.updated_at_ms = crate::now_ms();
        if next_session.title.trim().is_empty()
            || next_session.title.starts_with("Plan ")
            || next_session.title.starts_with("Untitled")
        {
            next_session.title = draft.current_plan.title.clone();
        }
        if next_session.title.trim().is_empty() {
            next_session.title = planner_session_title_from_record(&next_session);
        }
        let _ = state
            .put_workflow_planner_session(next_session.clone())
            .await;
    }
    Ok(Json(json!({
        "session": next_session,
        "plan": plan,
        "conversation": conversation,
        "change_summary": change_summary,
        "planner_diagnostics": planner_diagnostics,
        "clarifier": clarifier,
    })))
}

pub(super) async fn workflow_planner_session_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowPlannerSessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sessions = state
        .list_workflow_planner_sessions(query.project_slug.as_deref())
        .await;
    let items = sessions
        .iter()
        .map(workflow_planner_session_list_item)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "sessions": items,
        "count": items.len(),
    })))
}

pub(super) async fn workflow_planner_session_create(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlannerSessionCreateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_slug = input.project_slug.trim();
    if project_slug.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "project_slug is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let now = crate::now_ms();
    let session = WorkflowPlannerSessionRecord {
        session_id: format!("wfplan-session-{}", Uuid::new_v4()),
        project_slug: project_slug.to_string(),
        title: input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                planner_session_default_title(input.goal.as_deref().unwrap_or(""), now)
            }),
        workspace_root: input
            .workspace_root
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string(),
        source_kind: default_workflow_planner_source_kind(),
        source_bundle_digest: None,
        current_plan_id: None,
        draft: None,
        goal: input.goal.unwrap_or_default(),
        notes: input.notes.unwrap_or_default(),
        planner_provider: input.planner_provider.unwrap_or_default(),
        planner_model: input.planner_model.unwrap_or_default(),
        plan_source: input
            .plan_source
            .unwrap_or_else(|| "coding_task_planning".to_string()),
        allowed_mcp_servers: input.allowed_mcp_servers,
        operator_preferences: input.operator_preferences,
        import_validation: None,
        import_transform_log: Vec::new(),
        import_scope_snapshot: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let mut session = session;
    if let Some(plan) = input.plan {
        let conversation = input
            .conversation
            .unwrap_or_else(|| crate::WorkflowPlanConversation {
                conversation_id: format!("wfchat-{}", Uuid::new_v4()),
                plan_id: plan.plan_id.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                messages: Vec::new(),
            });
        let draft = crate::WorkflowPlanDraftRecord {
            initial_plan: plan.clone(),
            current_plan: plan,
            plan_revision: input.plan_revision.unwrap_or(1),
            conversation,
            planner_diagnostics: input.planner_diagnostics,
            last_success_materialization: input.last_success_materialization,
        };
        session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        session.draft = Some(draft);
    }
    let stored = state
        .put_workflow_planner_session(session.clone())
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_get(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    Ok(Json(json!({
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_patch(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionPatchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if let Some(title) = input.title.as_deref() {
        let title = title.trim();
        if title.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "title cannot be empty",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
        session.title = title.to_string();
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
        session.workspace_root = workspace_root.trim().to_string();
    }
    if let Some(goal) = input.goal {
        session.goal = goal;
    }
    if let Some(notes) = input.notes {
        session.notes = notes;
    }
    if let Some(provider) = input.planner_provider {
        session.planner_provider = provider;
    }
    if let Some(model) = input.planner_model {
        session.planner_model = model;
    }
    if let Some(plan_source) = input.plan_source {
        session.plan_source = plan_source;
    }
    if let Some(allowed) = input.allowed_mcp_servers {
        session.allowed_mcp_servers = allowed;
    }
    if let Some(preferences) = input.operator_preferences {
        session.operator_preferences = Some(preferences);
    }
    if let Some(current_plan_id) = input.current_plan_id {
        let current_plan_id = current_plan_id.trim();
        session.current_plan_id = if current_plan_id.is_empty() {
            None
        } else {
            Some(current_plan_id.to_string())
        };
    }
    if let Some(draft) = input.draft {
        session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        session.draft = Some(draft);
    }
    if let Some(published_at_ms) = input.published_at_ms {
        session.published_at_ms = Some(published_at_ms);
    }
    if let Some(published_tasks) = input.published_tasks {
        session.published_tasks = published_tasks;
    }
    let stored = state
        .put_workflow_planner_session(session)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_delete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(session) = state.delete_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    Ok(Json(json!({
        "ok": true,
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_duplicate(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionDuplicateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(source) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    let now = crate::now_ms();
    let mut next = source.clone();
    next.session_id = format!("wfplan-session-{}", Uuid::new_v4());
    next.title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Copy of {}", source.title));
    next.source_kind = workflow_planner_session_fork_source_kind(&source.source_kind);
    next.created_at_ms = now;
    next.updated_at_ms = now;
    if let Some(draft) = source.draft.as_ref() {
        let new_plan_id = format!("wfplan-{}", Uuid::new_v4());
        let duplicated = retag_workflow_plan_draft(draft, &new_plan_id).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
        next.current_plan_id = Some(new_plan_id);
        next.draft = Some(duplicated);
    }
    let stored = state
        .put_workflow_planner_session(next)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_start(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionStartRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    let chat_start = WorkflowPlanChatStartRequest {
        prompt: input.prompt,
        schedule: input.schedule,
        plan_source: input
            .plan_source
            .or_else(|| Some(session.plan_source.clone())),
        allowed_mcp_servers: if input.allowed_mcp_servers.is_empty() {
            session.allowed_mcp_servers.clone()
        } else {
            input.allowed_mcp_servers
        },
        workspace_root: input
            .workspace_root
            .or_else(|| Some(session.workspace_root.clone())),
        operator_preferences: input
            .operator_preferences
            .or(session.operator_preferences.clone()),
    };
    let response = workflow_plan_chat_start(State(state.clone()), Json(chat_start)).await?;
    if let Some(plan) = response.0.get("plan").cloned() {
        if let Ok(plan) = serde_json::from_value::<crate::WorkflowPlan>(plan) {
            session.current_plan_id = Some(plan.plan_id.clone());
            session.goal = response
                .0
                .get("plan")
                .and_then(|plan| plan.get("title"))
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| session.goal.clone());
            session.title = if session.title.trim().is_empty()
                || session.title.starts_with("Plan ")
                || session.title.starts_with("Untitled")
            {
                plan.title.clone()
            } else {
                session.title.clone()
            };
        }
    }
    if let Some(plan_id) = response
        .0
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
    {
        if let Some(draft) = state.get_workflow_plan_draft(plan_id).await {
            session.current_plan_id = Some(plan_id.to_string());
            session.draft = Some(draft.clone());
            session.updated_at_ms = crate::now_ms();
            let _ = state.put_workflow_planner_session(session.clone()).await;
            return workflow_planner_session_response(&state, &session, response).await;
        }
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_planner_session_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionMessageRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if session.current_plan_id.is_none() {
        if let Some(draft) = session.draft.clone() {
            session.current_plan_id = Some(draft.current_plan.plan_id.clone());
            let _ = state.put_workflow_plan_draft(draft).await;
        }
    }
    let Some(plan_id) = session.current_plan_id.clone() else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session has no active plan",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if state.get_workflow_plan_draft(&plan_id).await.is_none() {
        if let Some(draft) = session.draft.clone() {
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let response = workflow_plan_chat_message(
        State(state.clone()),
        Json(WorkflowPlanChatMessageRequest {
            plan_id: plan_id.clone(),
            message: input.message,
        }),
    )
    .await?;
    if let Some(draft) = state.get_workflow_plan_draft(&plan_id).await {
        session.draft = Some(draft.clone());
        session.updated_at_ms = crate::now_ms();
        if session.title.trim().is_empty()
            || session.title.starts_with("Plan ")
            || session.title.starts_with("Untitled")
        {
            session.title = draft.current_plan.title.clone();
        }
        let _ = state.put_workflow_planner_session(session.clone()).await;
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_planner_session_reset(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if session.current_plan_id.is_none() {
        if let Some(draft) = session.draft.clone() {
            session.current_plan_id = Some(draft.current_plan.plan_id.clone());
            let _ = state.put_workflow_plan_draft(draft).await;
        }
    }
    let Some(plan_id) = session.current_plan_id.clone() else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session has no active plan",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if state.get_workflow_plan_draft(&plan_id).await.is_none() {
        if let Some(draft) = session.draft.clone() {
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let response = workflow_plan_chat_reset(
        State(state.clone()),
        Json(WorkflowPlanChatResetRequest {
            plan_id: plan_id.clone(),
        }),
    )
    .await?;
    if let Some(draft) = state.get_workflow_plan_draft(&plan_id).await {
        session.draft = Some(draft.clone());
        session.updated_at_ms = crate::now_ms();
        if session.title.trim().is_empty()
            || session.title.starts_with("Plan ")
            || session.title.starts_with("Untitled")
        {
            session.title = draft.current_plan.title.clone();
        }
        let _ = state.put_workflow_planner_session(session.clone()).await;
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_plan_apply(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input
        .plan_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let plan = match (input.plan, plan_id.as_deref()) {
        (Some(plan), _) => plan,
        (None, Some(plan_id)) => state.get_workflow_plan(plan_id).await.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "workflow plan not found",
                    "code": "WORKFLOW_PLAN_NOT_FOUND",
                    "plan_id": plan_id,
                })),
            )
        })?,
        (None, None) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "plan or plan_id is required",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
    };
    compiler_api::validate_workflow_plan(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let draft_context = if let Some(plan_id) = plan_id.as_deref() {
        state.get_workflow_plan_draft(plan_id).await
    } else {
        None
    };
    let apply_revision = draft_context
        .as_ref()
        .map(|draft| draft.plan_revision)
        .unwrap_or(1);
    let planner_diagnostics = draft_context
        .as_ref()
        .and_then(|draft| draft.planner_diagnostics.clone());
    let plan_json = compiler_api::workflow_plan_to_json(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let mut plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        apply_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let mut overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    if plan_package_validation.blocker_count > 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan package validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
            })),
        ));
    }
    let requested_overlap_decision = parse_overlap_decision(input.overlap_decision.as_deref())
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    if overlap_analysis.requires_user_confirmation && requested_overlap_decision.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "overlap confirmation is required before apply",
                "code": "WORKFLOW_PLAN_OVERLAP_CONFIRMATION_REQUIRED",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
                "overlap_analysis": overlap_analysis,
            })),
        ));
    }
    if overlap_analysis.matched_plan_id.is_none() && requested_overlap_decision.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "overlap_decision was provided but no prior overlap was detected",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(decision) = requested_overlap_decision {
        overlap_analysis.decision = decision;
        overlap_analysis.requires_user_confirmation = false;
    }
    if let Some(entry) = compiler_api::overlap_log_entry_from_analysis(
        &overlap_analysis,
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
        &chrono::Utc::now().to_rfc3339(),
    ) {
        plan_package
            .overlap_policy
            .get_or_insert_with(Default::default)
            .overlap_log
            .push(entry);
    }

    let mut automation = compile_plan_to_automation_v2(
        &plan,
        Some(&plan_package),
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
    );
    let approved_plan_materialization = compiler_api::approved_plan_materialization(&plan_package);
    let approved_plan_success_memory =
        compiler_api::approved_plan_success_memory_value(&plan_package);
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
        metadata.insert(
            "plan_source".to_string(),
            serde_json::to_value(&plan.plan_source).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package".to_string(),
            serde_json::to_value(&plan_package).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_bundle".to_string(),
            serde_json::to_value(&plan_package_bundle).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_validation".to_string(),
            serde_json::to_value(&plan_package_validation).unwrap_or(Value::Null),
        );
        metadata.insert(
            "overlap_analysis".to_string(),
            serde_json::to_value(&overlap_analysis).unwrap_or(Value::Null),
        );
        metadata.insert(
            "approved_plan_materialization".to_string(),
            approved_plan_success_memory.clone(),
        );
        metadata.insert(
            "planner_diagnostics".to_string(),
            planner_diagnostics.clone().unwrap_or(Value::Null),
        );
    } else {
        automation.metadata = Some(json!({
            "plan_package": plan_package,
            "plan_package_bundle": plan_package_bundle.clone(),
            "plan_package_validation": plan_package_validation,
            "overlap_analysis": overlap_analysis,
            "approved_plan_materialization": approved_plan_success_memory.clone(),
            "planner_diagnostics": planner_diagnostics,
        }));
    }
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "WORKFLOW_PLAN_APPLY_FAILED",
            })),
        )
    })?;
    if let Some(plan_id) = plan_id.as_deref() {
        if let Some(mut draft) = state.get_workflow_plan_draft(plan_id).await {
            draft.last_success_materialization = Some(approved_plan_success_memory);
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let pack_builder_export = match input.pack_builder_export {
        Some(export) if export.enabled.unwrap_or(true) => {
            Some(export_workflow_plan_to_pack_builder(&state, &plan, &export).await)
        }
        _ => None,
    };
    Ok(Json(json!({
        "ok": true,
        "plan": plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "overlap_analysis": overlap_analysis,
        "approved_plan_materialization": approved_plan_materialization,
        "automation": stored,
        "pack_builder_export": pack_builder_export,
    })))
}

async fn workflow_plan_import_inner(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
    persist: bool,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let report = compiler_api::validate_plan_package_bundle(&input.bundle);
    if !report.compatible {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan bundle import validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "bundle": input.bundle,
                "import_validation": report,
            })),
        ));
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    let import_preview = compiler_api::preview_plan_package_import_bundle(
        &input.bundle,
        &workspace_root,
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
    );
    let plan_package_validation = compiler_api::validate_plan_package(&import_preview.plan_package);
    let plan_package_preview = import_preview.plan_package.clone();
    let source_bundle_digest = import_preview.source_bundle_digest.clone();
    let derived_scope_snapshot = import_preview.derived_scope_snapshot.clone();
    let import_transform_log = import_preview.import_transform_log.clone();
    let source_plan_id = import_preview.plan_package.plan_id.clone();
    let imported_goal = import_preview.plan_package.mission.goal.clone();
    let summary = workflow_plan_import_summary(&plan_package_preview);
    if !persist {
        return Ok(Json(json!({
            "ok": true,
            "persisted": false,
            "bundle": input.bundle,
            "import_validation": report,
            "plan_package_preview": plan_package_preview,
            "plan_package_validation": plan_package_validation,
            "derived_scope_snapshot": derived_scope_snapshot,
            "summary": summary,
            "import_transform_log": import_transform_log,
            "import_source_bundle_digest": source_bundle_digest,
        })));
    }

    let project_slug = input
        .project_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("workflow-imports")
        .to_string();
    let draft = workflow_plan_import_draft(&import_preview, &workspace_root);
    let session = WorkflowPlannerSessionRecord {
        session_id: format!("wfplan-session-{}", Uuid::new_v4()),
        project_slug,
        title: input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| workflow_plan_import_title(&imported_goal, &source_bundle_digest)),
        workspace_root,
        source_kind: "imported_bundle".to_string(),
        source_bundle_digest: Some(source_bundle_digest.clone()),
        current_plan_id: Some(draft.current_plan.plan_id.clone()),
        draft: Some(draft),
        goal: imported_goal.clone(),
        notes: import_transform_log.join("\n"),
        planner_provider: String::new(),
        planner_model: String::new(),
        plan_source: "workflow_plan_import".to_string(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "source_kind": "imported_bundle",
            "source_bundle_digest": source_bundle_digest.clone(),
            "source_plan_id": source_plan_id.clone(),
        })),
        import_validation: Some(report.clone()),
        import_transform_log: import_transform_log.clone(),
        import_scope_snapshot: Some(derived_scope_snapshot.clone()),
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
    };
    let stored = state
        .put_workflow_planner_session(session)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "persisted": true,
        "bundle": input.bundle,
        "import_validation": report,
        "plan_package_preview": plan_package_preview,
        "plan_package_validation": plan_package_validation,
        "derived_scope_snapshot": derived_scope_snapshot,
        "summary": summary,
        "import_transform_log": import_transform_log,
        "import_source_bundle_digest": source_bundle_digest,
        "session": stored,
    })))
}

pub(super) async fn workflow_plan_import(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_inner(State(state), Json(input), true).await
}

pub(super) async fn workflow_plan_import_preview(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_inner(State(state), Json(input), false).await
}

async fn export_workflow_plan_to_pack_builder(
    state: &AppState,
    plan: &crate::WorkflowPlan,
    export: &WorkflowPlanPackBuilderExportRequest,
) -> Value {
    let args = compiler_api::pack_builder_export_args(
        plan,
        &compiler_api::PackBuilderExportOptions {
            session_id: export.session_id.clone(),
            thread_key: export.thread_key.clone(),
            auto_apply: export.auto_apply.unwrap_or(false),
        },
    );
    match super::pack_builder::run_pack_builder_tool(state, args).await {
        Ok(payload) => payload,
        Err(code) => json!({
            "status": "export_failed",
            "error": "pack_builder_export_failed",
            "http_status": code.as_u16(),
        }),
    }
}
