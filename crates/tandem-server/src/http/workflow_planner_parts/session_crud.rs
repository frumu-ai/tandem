// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn workflow_planner_session_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Query(query): Query<WorkflowPlannerSessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let linked_chat_session_id = query
        .linked_chat_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut sessions = Vec::new();
    for session in state
        .list_workflow_planner_sessions(query.project_slug.as_deref())
        .await
    {
        if ensure_workflow_planner_session_access(
            &state,
            &session,
            &tenant_context,
            verified_tenant_context.as_deref(),
            false,
        )
        .await
        .is_ok()
            && linked_chat_session_id.is_none_or(|chat_session_id| {
                session.linked_chat_session_id.as_deref() == Some(chat_session_id)
            })
        {
            sessions.push(session);
        }
    }
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
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
        tenant_context,
        linked_chat_session_id: None,
        linked_chat_run_id: None,
        last_referenced_at_ms: None,
        artifact_links: Vec::new(),
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
        source_workflow: None,
        source_bundle_digest: None,
        source_pack_id: None,
        source_pack_version: None,
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
        planning: input.planning,
        operation: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let mut session = session;
    if let Some(plan) = input.plan {
        if compiler_api::workflow_plan_generated_task_budget_exceeded(&plan) {
            return Err(workflow_plan_task_budget_exceeded_error(&plan));
        }
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
            review: None,
        };
        session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        session.draft = Some(draft);
    }
    if session.planning.is_none() && session.draft.is_some() {
        session.planning = Some(WorkflowPlannerSessionPlanningRecord::default());
    }
    if let Some(planning) = session.planning.as_mut() {
        normalize_workflow_planning_record(planning, session.current_plan_id.as_deref(), now);
    }
    if !workflow_plan_access_binding_allowed(
        &state,
        &session.tenant_context,
        verified_tenant_context.as_deref(),
        &WorkflowPlanDraftAccessBinding::Actor(session.tenant_context.clone()),
        true,
    )
    .await
    {
        return Err(workflow_planner_session_scope_error(&session.session_id));
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
    if let Some(planning) = stored.planning.as_ref() {
        let review = stored
            .draft
            .as_ref()
            .and_then(|draft| draft.review.as_ref());
        workflow_planner_publish_event(
            &state,
            "workflow_planner.session.started",
            workflow_planner_event_payload(&stored, planning, review),
        );
    }
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_access(
        &state,
        &session,
        &tenant_context,
        verified_tenant_context.as_deref(),
        false,
    )
    .await?;
    Ok(Json(json!({
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_access(
        &state,
        &session,
        &tenant_context,
        verified_tenant_context.as_deref(),
        true,
    )
    .await?;
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
    if let Some(planning) = input.planning {
        session.planning = Some(planning);
    }
    if let Some(published_at_ms) = input.published_at_ms {
        session.published_at_ms = Some(published_at_ms);
    }
    if let Some(published_tasks) = input.published_tasks {
        session.published_tasks = published_tasks;
    }
    let now = crate::now_ms();
    if let Some(planning) = session.planning.as_mut() {
        normalize_workflow_planning_record(planning, session.current_plan_id.as_deref(), now);
    }
    let stored = state
        .put_workflow_planner_session_checked(
            session,
            &tenant_context,
            verified_tenant_context.as_deref(),
        )
        .await
        .map_err(|error| {
            if error.is::<crate::app::state::WorkflowPlannerSessionWriteDenied>() {
                return workflow_planner_session_scope_error(&session_id);
            }
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
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session = state
        .get_workflow_planner_session(&session_id)
        .await
        .ok_or_else(|| workflow_planner_session_scope_error(&session_id))?;
    ensure_workflow_planner_session_access(
        &state,
        &session,
        &tenant_context,
        verified_tenant_context.as_deref(),
        true,
    )
    .await?;
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
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_access(
        &state,
        &source,
        &tenant_context,
        verified_tenant_context.as_deref(),
        true,
    )
    .await?;
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
    next.linked_chat_session_id = None;
    next.linked_chat_run_id = None;
    next.last_referenced_at_ms = None;
    next.artifact_links.clear();
    next.operation = None;
    next.published_at_ms = None;
    next.published_tasks.clear();
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
    if let Some(planning) = next.planning.as_mut() {
        planning.linked_channel_session_id = None;
        normalize_workflow_planning_record(planning, next.current_plan_id.as_deref(), now);
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
