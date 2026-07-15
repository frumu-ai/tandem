// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeSet;

fn planner_session_title_from_record(session: &WorkflowPlannerSessionRecord) -> String {
    let title = session.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    planner_session_default_title(&session.goal, session.created_at_ms)
}

fn planner_session_operation_error(
    status: StatusCode,
    payload: &Json<Value>,
    fallback: &str,
) -> String {
    payload
        .0
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{fallback} ({status})"))
}

fn workflow_plan_task_budget_exceeded_error(
    plan: &crate::WorkflowPlan,
) -> (StatusCode, Json<Value>) {
    let task_budget = compiler_api::workflow_task_budget_report_for_plan(
        plan,
        Some("rejected"),
        Some(plan.steps.len()),
        Some("rejected"),
    );
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": format!(
                "Generated workflow plans may include at most {} steps. Regenerate or compact this plan before applying.",
                compiler_api::GENERATED_WORKFLOW_MAX_STEPS
            ),
            "code": "WORKFLOW_PLAN_TASK_BUDGET_EXCEEDED",
            "task_budget": task_budget,
            "planner_diagnostics": {
                "fallback_reason": "task_budget_rejected",
                "detail": format!(
                    "Generated plan contained {} steps, above the {} step limit.",
                    plan.steps.len(),
                    compiler_api::GENERATED_WORKFLOW_MAX_STEPS
                ),
                "task_budget": task_budget,
            },
        })),
    )
}

fn planner_session_operation_running(session: &WorkflowPlannerSessionRecord) -> bool {
    session
        .operation
        .as_ref()
        .map(|operation| operation.status.eq_ignore_ascii_case("running"))
        .unwrap_or(false)
}

fn workflow_planner_collect_capabilities(
    plan_package: &compiler_api::PlanPackage,
    required_integrations: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut required = BTreeSet::new();
    let mut requested = BTreeSet::new();

    for intent in &plan_package.connector_intents {
        let capability = intent.capability.trim();
        if capability.is_empty() {
            continue;
        }
        requested.insert(capability.to_string());
        if intent.required {
            required.insert(capability.to_string());
        }
    }

    for integration in required_integrations {
        let capability = integration.trim();
        if capability.is_empty() {
            continue;
        }
        requested.insert(capability.to_string());
        required.insert(capability.to_string());
    }

    for routine in &plan_package.routine_graph {
        for step in &routine.steps {
            for requirement in &step.connector_requirements {
                let capability = requirement.capability.trim();
                if capability.is_empty() {
                    continue;
                }
                requested.insert(capability.to_string());
                if requirement.required {
                    required.insert(capability.to_string());
                }
            }
        }
    }

    (
        required.into_iter().collect::<Vec<_>>(),
        requested.into_iter().collect::<Vec<_>>(),
    )
}

fn workflow_planner_blocked_capabilities(
    readiness: Option<&CapabilityReadinessOutput>,
) -> Vec<String> {
    let mut blocked = BTreeSet::new();
    if let Some(readiness) = readiness {
        for capability in readiness
            .missing_required_capabilities
            .iter()
            .chain(readiness.unbound_capabilities.iter())
        {
            let capability = capability.trim();
            if !capability.is_empty() {
                blocked.insert(capability.to_string());
            }
        }
        for issue in &readiness.blocking_issues {
            for capability in &issue.capability_ids {
                let capability = capability.trim();
                if !capability.is_empty() {
                    blocked.insert(capability.to_string());
                }
            }
        }
    }
    blocked.into_iter().collect()
}

fn workflow_planner_validation_status(
    validation: Option<&compiler_api::PlanValidationReport>,
    readiness: Option<&CapabilityReadinessOutput>,
) -> String {
    let ready_for_apply = validation
        .map(|report| report.ready_for_apply)
        .unwrap_or(false);
    let ready_for_activation = validation
        .map(|report| report.ready_for_activation)
        .unwrap_or(false);
    let blocker_count = validation
        .map(|report| report.blocker_count)
        .unwrap_or_default();
    let warning_count = validation
        .map(|report| report.warning_count)
        .unwrap_or_default();
    let capability_blocked = readiness
        .map(|report| !report.blocking_issues.is_empty() || !report.runnable)
        .unwrap_or(false);

    if blocker_count == 0 && !capability_blocked && ready_for_apply {
        if ready_for_activation {
            "ready".to_string()
        } else {
            "ready_for_apply".to_string()
        }
    } else if blocker_count > 0 || capability_blocked {
        "blocked".to_string()
    } else if warning_count > 0 {
        "warning".to_string()
    } else {
        "pending".to_string()
    }
}

fn workflow_planner_validation_state(
    legacy_status: &str,
    validation: Option<&compiler_api::PlanValidationReport>,
    readiness: Option<&CapabilityReadinessOutput>,
    approval_status: &str,
) -> String {
    let capability_blocked = readiness
        .map(|report| !report.blocking_issues.is_empty() || !report.runnable)
        .unwrap_or(false);
    let legacy_blocked = matches!(
        legacy_status.to_ascii_lowercase().as_str(),
        "blocked" | "approval_required" | "needs_approval"
    );
    if capability_blocked || legacy_blocked {
        if approval_status.eq_ignore_ascii_case("requested") {
            return "needs_approval".to_string();
        }
        return "blocked".to_string();
    }

    let ready = validation
        .map(|report| report.ready_for_apply || report.ready_for_activation)
        .unwrap_or(false)
        || matches!(
            legacy_status.to_ascii_lowercase().as_str(),
            "ready" | "ready_for_apply" | "ready_for_activation"
        );
    if ready {
        return "valid".to_string();
    }

    "incomplete".to_string()
}

async fn workflow_planner_session_store_operation_result(
    state: &AppState,
    session_id: &str,
    request_id: &str,
    kind: &str,
    result: Result<Json<Value>, (StatusCode, Json<Value>)>,
) {
    let Some(mut session) = state.get_workflow_planner_session(session_id).await else {
        return;
    };
    let started_at_ms = session
        .operation
        .as_ref()
        .filter(|operation| operation.request_id == request_id)
        .map(|operation| operation.started_at_ms);
    let Some(started_at_ms) = started_at_ms else {
        return;
    };
    let finished_at_ms = crate::now_ms();
    match result {
        Ok(response) => {
            if let Some(next_session) = response.0.get("session").cloned().and_then(|value| {
                serde_json::from_value::<WorkflowPlannerSessionRecord>(value).ok()
            }) {
                session = next_session;
            }
            session.operation = Some(WorkflowPlannerSessionOperationRecord {
                request_id: request_id.to_string(),
                kind: kind.to_string(),
                status: "completed".to_string(),
                started_at_ms,
                finished_at_ms: Some(finished_at_ms),
                response: Some(response.0),
                error: None,
            });
        }
        Err((status, payload)) => {
            session.operation = Some(WorkflowPlannerSessionOperationRecord {
                request_id: request_id.to_string(),
                kind: kind.to_string(),
                status: "failed".to_string(),
                started_at_ms,
                finished_at_ms: Some(finished_at_ms),
                response: Some(payload.0.clone()),
                error: Some(planner_session_operation_error(
                    status,
                    &payload,
                    "Workflow planner failed",
                )),
            });
        }
    }
    let _ = state.put_workflow_planner_session(session).await;
}

async fn workflow_planner_session_response(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    response: Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut payload = response.0;
    let plan = payload.get("plan").cloned().unwrap_or(Value::Null);
    let conversation = payload.get("conversation").cloned().unwrap_or(Value::Null);
    let planner_diagnostics = payload
        .get("planner_diagnostics")
        .or_else(|| payload.get("plannerDiagnostics"))
        .cloned()
        .unwrap_or(Value::Null);
    let change_summary = payload
        .get("change_summary")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let clarifier = payload
        .get("clarifier")
        .cloned()
        .unwrap_or_else(|| json!({"status":"none"}));
    let plan_package_value = payload.get("plan_package").cloned();
    let plan_package_validation_value = payload.get("plan_package_validation").cloned();
    let draft = state
        .get_workflow_plan_draft(session.current_plan_id.as_deref().unwrap_or_default())
        .await;
    let draft_was_present = session.draft.is_some() || draft.is_some();
    let mut next_session = session.clone();
    let mut review_record: Option<WorkflowPlanDraftReviewRecord> = None;
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
    }

    if let Some(plan_package_value) = plan_package_value {
        if let Ok(plan_package) =
            serde_json::from_value::<compiler_api::PlanPackage>(plan_package_value.clone())
        {
            let (required_capabilities, requested_capabilities) =
                workflow_planner_collect_capabilities(
                    &plan_package,
                    next_session
                        .draft
                        .as_ref()
                        .map(|draft| draft.current_plan.requires_integrations.as_slice())
                        .unwrap_or(&[]),
                );
            let optional_capabilities = requested_capabilities
                .iter()
                .filter(|capability| !required_capabilities.contains(capability))
                .cloned()
                .collect::<Vec<_>>();
            let plan_validation = plan_package_validation_value.as_ref().and_then(|value| {
                serde_json::from_value::<compiler_api::PlanValidationReport>(value.clone()).ok()
            });
            let readiness = if required_capabilities.is_empty() && optional_capabilities.is_empty()
            {
                None
            } else {
                let readiness_input = CapabilityReadinessInput {
                    workflow_id: Some(
                        next_session
                            .current_plan_id
                            .clone()
                            .unwrap_or_else(|| next_session.session_id.clone()),
                    ),
                    required_capabilities: required_capabilities.clone(),
                    optional_capabilities: optional_capabilities.clone(),
                    provider_preference: next_session
                        .planner_provider
                        .trim()
                        .is_empty()
                        .then(Vec::new)
                        .unwrap_or_else(|| vec![next_session.planner_provider.clone()]),
                    available_tools: Vec::new(),
                    allow_unbound: false,
                };
                evaluate_capability_readiness(state, &readiness_input)
                    .await
                    .ok()
            };
            let validation_status =
                workflow_planner_validation_status(plan_validation.as_ref(), readiness.as_ref());
            let blocked_capabilities = workflow_planner_blocked_capabilities(readiness.as_ref());
            let docs_mcp_used = next_session
                .planning
                .as_ref()
                .and_then(|planning| planning.docs_mcp_enabled)
                .unwrap_or(false);
            let mut planning = next_session.planning.clone().unwrap_or_default();
            let now = crate::now_ms();
            normalize_workflow_planning_record(
                &mut planning,
                next_session.current_plan_id.as_deref(),
                now,
            );
            planning.allowed_tools = requested_capabilities.clone();
            planning.blocked_tools = blocked_capabilities.clone();
            planning.known_requirements = required_capabilities.clone();
            planning.missing_requirements = blocked_capabilities.clone();
            planning.docs_mcp_enabled = Some(docs_mcp_used);
            let preview_payload = payload.clone();
            let approval_status = workflow_planner_request_capability_approval(
                state,
                &next_session,
                &planning,
                &blocked_capabilities,
                &requested_capabilities,
                &preview_payload,
                &validation_status,
            )
            .await;
            planning.validation_status = validation_status.clone();
            planning.approval_status = approval_status.clone();
            planning.validation_state = workflow_planner_validation_state(
                &validation_status,
                plan_validation.as_ref(),
                readiness.as_ref(),
                &approval_status,
            );
            next_session.planning = Some(planning.clone());
            review_record = Some(WorkflowPlanDraftReviewRecord {
                required_capabilities: required_capabilities.clone(),
                requested_capabilities: requested_capabilities.clone(),
                blocked_capabilities: blocked_capabilities.clone(),
                docs_mcp_used,
                preview_payload: Some(preview_payload.clone()),
                created_at_ms: Some(now),
                updated_at_ms: Some(now),
                validation_state: planning.validation_state.clone(),
                validation_status,
                approval_status,
            });
        }
    }
    if let Some(review) = review_record {
        if let Some(draft) = next_session.draft.as_mut() {
            draft.review = Some(review);
        }
    }
    next_session.updated_at_ms = crate::now_ms();
    let _ = state
        .put_workflow_planner_session(next_session.clone())
        .await;
    if let Some(draft) = next_session.draft.clone() {
        let _ = state.put_workflow_plan_draft(draft).await;
    }
    if let Some(planning) = next_session.planning.as_ref() {
        let review = next_session
            .draft
            .as_ref()
            .and_then(|draft| draft.review.as_ref());
        workflow_planner_publish_session_events(
            state,
            &next_session,
            planning,
            review,
            draft_was_present,
        );
    }
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "session".to_string(),
            serde_json::to_value(next_session).unwrap_or(Value::Null),
        );
    } else {
        payload = json!({
            "session": next_session,
            "response": payload,
        });
    }
    Ok(Json(payload))
}

pub(super) async fn workflow_planner_session_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    Query(query): Query<WorkflowPlannerSessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let linked_chat_session_id = query
        .linked_chat_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let sessions = state
        .list_workflow_planner_sessions(query.project_slug.as_deref())
        .await
        .into_iter()
        .filter(|session| {
            ensure_workflow_planner_session_tenant(session, &tenant_context).is_ok()
        })
        .filter(|session| {
            linked_chat_session_id.is_none_or(|chat_session_id| {
                session.linked_chat_session_id.as_deref() == Some(chat_session_id)
            })
        })
        .collect::<Vec<_>>();
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
    Ok(Json(json!({
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
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
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let session = state
        .get_workflow_planner_session(&session_id)
        .await
        .ok_or_else(|| workflow_planner_session_scope_error(&session_id))?;
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
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
    ensure_workflow_planner_session_tenant(&source, &tenant_context)?;
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

pub(super) async fn workflow_planner_session_start(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
    let chat_start = WorkflowPlanChatStartRequest {
        prompt: input.prompt,
        schedule: input.schedule,
        plan_source: input
            .plan_source
            .or_else(|| Some(session.plan_source.clone())),
        progress_id: input.progress_id,
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
    let response = workflow_plan_chat_start(
        State(state.clone()),
        Extension(tenant_context),
        verified_tenant_context,
        Json(chat_start),
    )
    .await?;
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

pub(super) async fn workflow_planner_session_start_async(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
    if planner_session_operation_running(&session) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "planner session already has an operation in progress",
                "code": "WORKFLOW_PLAN_SESSION_BUSY",
                "session_id": session_id,
            })),
        ));
    }
    let request_id = format!("wfplan-op-{}", Uuid::new_v4());
    session.operation = Some(WorkflowPlannerSessionOperationRecord {
        request_id: request_id.clone(),
        kind: "start".to_string(),
        status: "running".to_string(),
        started_at_ms: crate::now_ms(),
        finished_at_ms: None,
        response: None,
        error: None,
    });
    let session = state
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
    tokio::spawn(workflow_planner_session_start_background(
        state.clone(),
        session_id.clone(),
        input,
        request_id,
        tenant_context,
        verified_tenant_context.map(|Extension(verified)| verified),
    ));
    Ok(Json(json!({
        "ok": true,
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
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
        Extension(tenant_context),
        verified_tenant_context,
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

pub(super) async fn workflow_planner_session_message_async(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
    if planner_session_operation_running(&session) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "planner session already has an operation in progress",
                "code": "WORKFLOW_PLAN_SESSION_BUSY",
                "session_id": session_id,
            })),
        ));
    }
    let request_id = format!("wfplan-op-{}", Uuid::new_v4());
    session.operation = Some(WorkflowPlannerSessionOperationRecord {
        request_id: request_id.clone(),
        kind: "message".to_string(),
        status: "running".to_string(),
        started_at_ms: crate::now_ms(),
        finished_at_ms: None,
        response: None,
        error: None,
    });
    let session = state
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
    tokio::spawn(workflow_planner_session_message_background(
        state.clone(),
        session_id.clone(),
        input,
        request_id,
        tenant_context,
        verified_tenant_context.map(|Extension(verified)| verified),
    ));
    Ok(Json(json!({
        "ok": true,
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_reset(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
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
    ensure_workflow_planner_session_tenant(&session, &tenant_context)?;
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
        tenant_context: tandem_types::TenantContext::local_implicit(),
        linked_chat_session_id: None,
        linked_chat_run_id: None,
        last_referenced_at_ms: None,
        artifact_links: Vec::new(),
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
        source_pack_id: None,
        source_pack_version: None,
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
        operation: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        planning: None,
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

const WORKFLOW_PACK_MARKER: &str = "tandempack.yaml";
const WORKFLOW_PACK_MAX_COVER_BYTES: u64 = 5 * 1024 * 1024;
const WORKFLOW_PACK_MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

fn workflow_pack_slug(input: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | ' ' | '.' | '/') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn workflow_pack_read_manifest(path: &FsPath) -> anyhow::Result<crate::pack_manager::PackManifest> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut manifest_file = archive.by_name(WORKFLOW_PACK_MARKER)?;
    let mut text = String::new();
    manifest_file.read_to_string(&mut text)?;
    Ok(serde_yaml::from_str(&text)?)
}

fn workflow_pack_validate_entry_path(path: &str) -> anyhow::Result<()> {
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\0') {
        return Err(anyhow::anyhow!("invalid pack path: {path}"));
    }
    let mut depth = 0usize;
    for component in FsPath::new(path).components() {
        match component {
            std::path::Component::Normal(_) => {
                depth = depth.saturating_add(1);
                if depth > 24 {
                    return Err(anyhow::anyhow!("pack path too deep: {path}"));
                }
            }
            std::path::Component::CurDir => {}
            _ => return Err(anyhow::anyhow!("unsafe pack path: {path}")),
        }
    }
    Ok(())
}

fn workflow_pack_validate_zip(path: &FsPath) -> anyhow::Result<()> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut has_marker = false;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        if name == WORKFLOW_PACK_MARKER {
            has_marker = true;
        }
        workflow_pack_validate_entry_path(&name)?;
        if entry.size() > WORKFLOW_PACK_MAX_ENTRY_BYTES {
            return Err(anyhow::anyhow!("pack entry too large: {name}"));
        }
    }
    if !has_marker {
        return Err(anyhow::anyhow!(
            "zip does not contain root marker tandempack.yaml"
        ));
    }
    Ok(())
}

fn workflow_pack_entries(manifest: &crate::pack_manager::PackManifest) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Some(rows) = manifest
        .contents
        .get("workflows")
        .and_then(|value| value.as_array())
    {
        for (index, row) in rows.iter().enumerate() {
            if let Some(path) = row
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                entries.push((format!("workflow-{}", index + 1), path.to_string()));
                continue;
            }
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("workflow-{}", index + 1));
            if let Some(path) = row
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                entries.push((id, path.to_string()));
            }
        }
    }
    entries
}

fn workflow_pack_cover_path(manifest_value: &Value) -> Option<String> {
    manifest_value
        .pointer("/marketplace/listing/cover_image")
        .or_else(|| manifest_value.pointer("/marketplace/listing/icon"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn workflow_pack_image_mime(path: &str) -> Option<&'static str> {
    match FsPath::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn workflow_pack_read_zip_text(path: &FsPath, entry_path: &str) -> anyhow::Result<String> {
    workflow_pack_validate_entry_path(entry_path)?;
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_name(entry_path)?;
    if entry.size() > WORKFLOW_PACK_MAX_ENTRY_BYTES {
        return Err(anyhow::anyhow!("pack entry too large: {entry_path}"));
    }
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(text)
}

fn workflow_pack_cover_data_url(path: &FsPath, cover_path: &str) -> anyhow::Result<Option<String>> {
    workflow_pack_validate_entry_path(cover_path)?;
    let Some(mime) = workflow_pack_image_mime(cover_path) else {
        return Err(anyhow::anyhow!(
            "cover image must be PNG, JPEG, or WebP: {cover_path}"
        ));
    };
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_name(cover_path)?;
    if entry.size() > WORKFLOW_PACK_MAX_COVER_BYTES {
        return Err(anyhow::anyhow!("cover image exceeds 5MB: {cover_path}"));
    }
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(format!("data:{mime};base64,{encoded}")))
}

async fn workflow_pack_preview_from_bundle(
    state: &AppState,
    bundle: &compiler_api::PlanPackageImportBundle,
    creator_id: &str,
) -> (
    compiler_api::PlanReplayReport,
    Value,
    Value,
    Value,
    Vec<String>,
    String,
) {
    let report = compiler_api::validate_plan_package_bundle(bundle);
    if !report.compatible {
        return (
            report,
            Value::Null,
            Value::Null,
            Value::Null,
            Vec::new(),
            String::new(),
        );
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    let import_preview =
        compiler_api::preview_plan_package_import_bundle(bundle, &workspace_root, creator_id);
    let plan_package_validation = compiler_api::validate_plan_package(&import_preview.plan_package);
    (
        report,
        serde_json::to_value(&import_preview.plan_package).unwrap_or(Value::Null),
        serde_json::to_value(plan_package_validation).unwrap_or(Value::Null),
        workflow_plan_import_summary(&import_preview.plan_package),
        import_preview.import_transform_log,
        import_preview.source_bundle_digest,
    )
}

fn workflow_pack_sha256_file(path: &FsPath) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn workflow_pack_add_file(
    zip: &mut ZipWriter<File>,
    path: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(path, options)?;
    zip.write_all(bytes)?;
    Ok(())
}

fn workflow_pack_validate_cover_file(path: &FsPath) -> anyhow::Result<&'static str> {
    let display = path.to_string_lossy();
    let Some(mime) = workflow_pack_image_mime(&display) else {
        return Err(anyhow::anyhow!("cover image must be PNG, JPEG, or WebP"));
    };
    let metadata = fs::metadata(path)?;
    if metadata.len() > WORKFLOW_PACK_MAX_COVER_BYTES {
        return Err(anyhow::anyhow!("cover image exceeds 5MB"));
    }
    Ok(mime)
}

async fn workflow_plan_pack_export_bundle(
    state: &AppState,
    input: &WorkflowPlanPackExportRequest,
) -> Result<(crate::WorkflowPlan, u32), (StatusCode, Json<Value>)> {
    if let Some(session_id) = input
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let Some(session) = state.get_workflow_planner_session(session_id).await else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "workflow planner session not found"})),
            ));
        };
        if let Some(draft) = session.draft {
            return Ok((draft.current_plan, draft.plan_revision));
        }
        if let Some(plan_id) = session.current_plan_id.as_deref() {
            if let Some(plan) = state.get_workflow_plan(plan_id).await {
                return Ok((plan, 1));
            }
        }
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "workflow session does not contain an exportable plan"})),
        ));
    }
    if let Some(plan_id) = input
        .plan_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if let Some(draft) = state.get_workflow_plan_draft(plan_id).await {
            return Ok((draft.current_plan, draft.plan_revision));
        }
        if let Some(plan) = state.get_workflow_plan(plan_id).await {
            return Ok((plan, 1));
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "export requires session_id or plan_id"})),
    ))
}

pub(super) async fn workflow_plan_export_pack(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPackExportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (plan, revision) = workflow_plan_pack_export_bundle(&state, &input).await?;
    let plan_json = compiler_api::workflow_plan_to_json(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error, "code": "WORKFLOW_PLAN_INVALID"})),
        )
    })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        revision,
    );
    let validation = compiler_api::validate_plan_package(&plan_package);
    if validation.blocker_count > 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan package validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "plan_package_validation": validation,
            })),
        ));
    }
    let bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| plan.title.trim())
        .to_string();
    let name = workflow_pack_slug(
        input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&title),
        "workflow-pack",
    );
    let version = input
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("0.1.0")
        .to_string();
    let workflow_id = workflow_pack_slug(&plan.plan_id, "workflow");
    let workflow_path = format!("workflows/{workflow_id}/plan-package.json");
    let cover_source = input
        .cover_image_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let cover_ext = cover_source
        .as_ref()
        .and_then(|path| path.extension().and_then(|value| value.to_str()))
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "png".to_string());
    let cover_path = cover_source
        .as_ref()
        .map(|_| format!("assets/cover.{cover_ext}"));
    if let Some(path) = cover_source.as_ref() {
        workflow_pack_validate_cover_file(path).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID_COVER"})),
            )
        })?;
    }
    let creator_id = input
        .creator_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tandem");
    let description = input
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            plan.description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("")
        .to_string();
    let manifest = json!({
        "manifest_schema_version": "1",
        "name": name,
        "version": version,
        "type": "workflow",
        "pack_id": name,
        "marketplace": {
            "publisher": {
                "publisher_id": creator_id,
                "display_name": creator_id,
                "verification_tier": "unverified"
            },
            "listing": {
                "display_name": title,
                "description": if description.is_empty() { plan.original_prompt.clone() } else { description.clone() },
                "license_spdx": "Proprietary",
                "categories": ["workflow"],
                "tags": ["workflow", "tandem"],
                "cover_image": cover_path.clone()
            }
        },
        "capabilities": {
            "required": plan.requires_integrations,
            "optional": [],
            "provider_specific": []
        },
        "entrypoints": {
            "workflows": [workflow_id]
        },
        "contents": {
            "workflows": [{
                "id": workflow_id,
                "path": workflow_path,
                "format": "workflow_plan_bundle"
            }]
        }
    });
    let manifest_yaml = serde_yaml::to_string(&manifest).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    let bundle_json = serde_json::to_vec_pretty(&bundle).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    let exports_dir = crate::pack_manager::PackManager::default_root()
        .join("exports")
        .join("workflow-packs");
    fs::create_dir_all(&exports_dir).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    let output = exports_dir.join(format!("{}-{}.zip", name, version));
    let file = File::create(&output).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    let mut zip = ZipWriter::new(file);
    workflow_pack_add_file(&mut zip, WORKFLOW_PACK_MARKER, manifest_yaml.as_bytes())
        .and_then(|_| {
            let readme = format!(
                "# {title}\n\n{}\n",
                if description.is_empty() {
                    &plan.original_prompt
                } else {
                    &description
                }
            );
            workflow_pack_add_file(&mut zip, "README.md", readme.as_bytes())
        })
        .and_then(|_| workflow_pack_add_file(&mut zip, &workflow_path, &bundle_json))
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
        })?;
    if let (Some(source), Some(path)) = (cover_source.as_ref(), cover_path.as_ref()) {
        let bytes = fs::read(source).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID_COVER"})),
            )
        })?;
        workflow_pack_add_file(&mut zip, path, &bytes).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
        })?;
    }
    zip.finish().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    let bytes = fs::metadata(&output)
        .map(|meta| meta.len())
        .unwrap_or_default();
    let sha256 = workflow_pack_sha256_file(&output).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "pack": {
            "name": manifest.get("name"),
            "version": manifest.get("version"),
            "pack_id": manifest.get("pack_id"),
            "pack_type": "workflow",
            "cover_image": cover_path,
        },
        "exported": {
            "path": output.to_string_lossy(),
            "download_url": format!(
                "/workflow-plans/export/pack/download?path={}",
                urlencoding::encode(&output.to_string_lossy())
            ),
            "sha256": sha256,
            "bytes": bytes,
        },
        "manifest": manifest,
        "bundle": bundle,
        "marketplace_ready": true,
    })))
}

pub(super) async fn workflow_plan_export_pack_download(
    Query(query): Query<WorkflowPlanPackDownloadQuery>,
) -> Result<Response, StatusCode> {
    let requested = PathBuf::from(query.path.trim());
    let root = crate::pack_manager::PackManager::default_root()
        .join("exports")
        .join("workflow-packs");
    let root = root.canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    let path = requested
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if path != root && !path.starts_with(&root) {
        return Err(StatusCode::FORBIDDEN);
    }
    if path.extension().and_then(|value| value.to_str()) != Some("zip") {
        return Err(StatusCode::FORBIDDEN);
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workflow-pack.zip")
        .replace('"', "");
    let mut response = Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(response)
}

async fn workflow_plan_import_pack_inner(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPackImportRequest>,
    persist: bool,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pack_path = PathBuf::from(input.path.trim());
    workflow_pack_validate_zip(&pack_path).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID"})),
        )
    })?;
    let manifest = workflow_pack_read_manifest(&pack_path).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID"})),
        )
    })?;
    let manifest_value = serde_json::to_value(&manifest).unwrap_or(Value::Null);
    let cover_path = workflow_pack_cover_path(&manifest_value);
    let cover_data_url = match cover_path.as_deref() {
        Some(path) => workflow_pack_cover_data_url(&pack_path, path).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID_COVER"})),
            )
        })?,
        None => None,
    };
    let entries = workflow_pack_entries(&manifest);
    if entries.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": "workflow pack does not declare workflow contents", "code": "WORKFLOW_PACK_EMPTY"}),
            ),
        ));
    }
    let selected = input
        .selected_workflow_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let creator_id = input.creator_id.as_deref().unwrap_or("workflow_planner");
    let mut workflow_previews = Vec::new();
    let mut import_requests = Vec::new();
    for (workflow_id, workflow_path) in entries {
        if !selected.is_empty() && !selected.contains(&workflow_id) {
            continue;
        }
        let text = workflow_pack_read_zip_text(&pack_path, &workflow_path).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID"})),
            )
        })?;
        let bundle: compiler_api::PlanPackageImportBundle =
            serde_json::from_str(&text).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(
                        json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INVALID_BUNDLE"}),
                    ),
                )
            })?;
        let (
            validation,
            plan_package_preview,
            plan_package_validation,
            summary,
            transform_log,
            digest,
        ) = workflow_pack_preview_from_bundle(&state, &bundle, creator_id).await;
        if !validation.compatible {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "plan bundle import validation failed",
                    "code": "WORKFLOW_PLAN_INVALID",
                    "workflow_id": workflow_id,
                    "import_validation": validation,
                })),
            ));
        }
        workflow_previews.push(json!({
            "workflow_id": workflow_id,
            "path": workflow_path,
            "bundle": bundle,
            "import_validation": validation,
            "plan_package_preview": plan_package_preview,
            "plan_package_validation": plan_package_validation,
            "summary": summary,
            "import_transform_log": transform_log,
            "import_source_bundle_digest": digest,
        }));
        import_requests.push(WorkflowPlanImportRequest {
            bundle,
            creator_id: input.creator_id.clone(),
            project_slug: input
                .project_slug
                .clone()
                .or_else(|| Some("workflow-imports".to_string())),
            title: input.title.clone(),
        });
    }
    if workflow_previews.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": "no selected workflows found in pack", "code": "WORKFLOW_PACK_EMPTY"}),
            ),
        ));
    }
    if !persist {
        return Ok(Json(json!({
            "ok": true,
            "persisted": false,
            "is_pack": true,
            "manifest": manifest_value,
            "cover_image": cover_path,
            "cover_image_data_url": cover_data_url,
            "workflows": workflow_previews,
            "pack": {
                "name": manifest.name,
                "version": manifest.version,
                "pack_id": manifest.pack_id,
                "pack_type": manifest.pack_type,
            },
        })));
    }
    let installed = match state
        .pack_manager
        .install(crate::pack_manager::PackInstallRequest {
            path: Some(pack_path.to_string_lossy().to_string()),
            url: None,
            source: json!({"kind": "workflow_pack_import", "path": pack_path.to_string_lossy()}),
        })
        .await
    {
        Ok(record) => record,
        Err(error) if error.to_string().contains("pack already installed") => {
            let packs = state.pack_manager.list().await.map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": err.to_string(), "code": "WORKFLOW_PACK_INSTALL_FAILED"})),
                )
            })?;
            packs.into_iter()
                .find(|record| record.name == manifest.name && record.version == manifest.version)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INSTALL_FAILED"})),
                    )
                })?
        }
        Err(error) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string(), "code": "WORKFLOW_PACK_INSTALL_FAILED"})),
            ));
        }
    };
    let mut sessions = Vec::new();
    for request in import_requests {
        let response =
            workflow_plan_import_inner(State(state.clone()), Json(request), true).await?;
        let mut payload = response.0;
        if let Some(session_value) = payload.get_mut("session") {
            if let Ok(mut session) =
                serde_json::from_value::<WorkflowPlannerSessionRecord>(session_value.clone())
            {
                session.source_kind = "workflow_pack".to_string();
                session.source_pack_id = Some(installed.pack_id.clone());
                session.source_pack_version = Some(installed.version.clone());
                let source_bundle_digest = session.source_bundle_digest.clone();
                session.operator_preferences = Some(json!({
                    "source_kind": "workflow_pack",
                    "source_pack_id": installed.pack_id.clone(),
                    "source_pack_version": installed.version.clone(),
                    "source_bundle_digest": source_bundle_digest,
                }));
                if let Ok(stored) = state.put_workflow_planner_session(session).await {
                    *session_value = serde_json::to_value(&stored).unwrap_or(Value::Null);
                    sessions.push(stored);
                }
            }
        }
    }
    Ok(Json(json!({
        "ok": true,
        "persisted": true,
        "is_pack": true,
        "manifest": manifest_value,
        "cover_image": cover_path,
        "cover_image_data_url": cover_data_url,
        "workflows": workflow_previews,
        "installed": installed,
        "sessions": sessions,
    })))
}

pub(super) async fn workflow_plan_import_pack(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPackImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_pack_inner(State(state), Json(input), true).await
}

pub(super) async fn workflow_plan_import_pack_preview(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPackImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_pack_inner(State(state), Json(input), false).await
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
