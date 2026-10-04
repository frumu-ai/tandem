// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use tandem_types::EngineEvent;
use tandem_workflows::plan_package::WorkflowPlanDraftReviewRecord;

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

fn workflow_planner_session_scope_error(session_id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "planner session not found",
            "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
            "session_id": session_id,
        })),
    )
}

pub(super) async fn ensure_workflow_planner_session_access(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    tenant_context: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    mutation: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    if tenant_context.is_local_implicit() {
        super::ensure_same_tenant(tenant_context, &session.tenant_context)
            .map_err(|_| workflow_planner_session_scope_error(&session.session_id))?;
        return Ok(());
    }
    let denied = || workflow_planner_session_scope_error(&session.session_id);
    let binding = if let Some(source) = session.source_workflow.as_ref() {
        WorkflowPlanDraftAccessBinding::Workflow(source.clone())
    } else if session.source_kind.trim_start_matches("forked_") == "workflow_learning_revision" {
        // Legacy revision rows have no immutable source identity.
        return Err(denied());
    } else {
        super::ensure_same_tenant(tenant_context, &session.tenant_context).map_err(|_| denied())?;
        WorkflowPlanDraftAccessBinding::Actor(session.tenant_context.clone())
    };
    if !workflow_plan_access_binding_allowed(state, tenant_context, verified, &binding, mutation)
        .await
    {
        return Err(denied());
    }
    if let Some(plan_id) = session.current_plan_id.as_deref() {
        let valid_draft = session.draft.as_ref().is_some_and(|draft| {
            draft.current_plan.plan_id == plan_id
                && draft.initial_plan.plan_id == plan_id
                && draft.conversation.plan_id == plan_id
        });
        if !valid_draft
            || state.workflow_plan_draft_authority(plan_id).await
                != Some(WorkflowPlanDraftAuthority::Bound {
                    binding,
                    session_id: Some(session.session_id.clone()),
                })
        {
            return Err(denied());
        }
    } else if session.draft.is_some() {
        return Err(denied());
    }
    Ok(())
}

pub(super) async fn ensure_current_workflow_planner_session_write(
    state: &AppState,
    session_id: &str,
    tenant: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let current = state
        .get_workflow_planner_session(session_id)
        .await
        .ok_or_else(|| workflow_planner_session_scope_error(session_id))?;
    ensure_workflow_planner_session_access(state, &current, tenant, verified, true).await
}

pub(crate) async fn workflow_plan_access_binding_allowed(
    state: &AppState,
    tenant: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    binding: &WorkflowPlanDraftAccessBinding,
    mutation: bool,
) -> bool {
    if tenant.is_local_implicit() {
        return true;
    }
    let Some(verified) = verified else {
        return false;
    };
    let actor_id = verified.human_actor.actor_id.trim();
    if actor_id.is_empty()
        || verified.is_expired_at(crate::now_ms())
        || !super::tenant_matches(tenant, &verified.tenant_context)
        || tenant.actor_id.as_deref() != Some(actor_id)
    {
        return false;
    }
    match binding {
        WorkflowPlanDraftAccessBinding::Workflow(source) => {
            let Some(automation) = state.get_automation_v2(&source.workflow_id).await else {
                return false;
            };
            automation.created_at_ms > 0
                && source.binding
                    == crate::WorkflowLearningCandidateSourceBinding::workflow(&automation)
                && if mutation {
                    super::automation_object_authority::can_write(
                        state,
                        tenant,
                        Some(verified),
                        &automation,
                    )
                } else {
                    super::automation_object_authority::can_read(
                        state,
                        tenant,
                        Some(verified),
                        &automation,
                    )
                }
        }
        WorkflowPlanDraftAccessBinding::Actor(owner) => {
            if !super::tenant_matches(tenant, owner) || owner.actor_id.as_deref() != Some(actor_id)
            {
                return false;
            }
            let mut current = verified.clone();
            if state
                .enterprise
                .hosted_policy
                .project(&mut current)
                .is_err()
            {
                return false;
            }
            state
                .enterprise
                .hosted_policy
                .authorize_permission(
                    Some(&current),
                    if mutation {
                        tandem_types::AccessPermission::HostedAutomationWrite
                    } else {
                        tandem_types::AccessPermission::HostedAutomationRead
                    },
                )
                .is_ok()
        }
    }
}

pub(super) async fn ensure_workflow_plan_id_access(
    state: &AppState,
    tenant: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    plan_id: &str,
    mutation: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    if tenant.is_local_implicit() {
        return Ok(());
    }
    let denied = || {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "workflow plan not found",
                "code": "WORKFLOW_PLAN_NOT_FOUND",
                "plan_id": plan_id,
            })),
        )
    };
    let Some(WorkflowPlanDraftAuthority::Bound { binding, .. }) =
        state.workflow_plan_draft_authority(plan_id).await
    else {
        return Err(denied());
    };
    if workflow_plan_access_binding_allowed(state, tenant, verified, &binding, mutation).await {
        Ok(())
    } else {
        Err(denied())
    }
}

pub(super) fn workflow_plan_mutation_actor_id(
    tenant_context: &tandem_types::TenantContext,
    verified_tenant_context: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<String, (StatusCode, Json<Value>)> {
    if let Some(verified) = verified_tenant_context {
        if super::ensure_same_tenant(tenant_context, &verified.tenant_context).is_err() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "verified principal does not match the request tenant",
                    "code": "WORKFLOW_PLAN_TENANT_MISMATCH",
                })),
            ));
        }
        let actor_id = verified.human_actor.actor_id.trim();
        if !actor_id.is_empty() {
            return Ok(actor_id.to_string());
        }
    }
    if tenant_context.is_local_implicit() {
        return Ok("local-operator".to_string());
    }
    Err((
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "workflow materialization requires a verified tenant principal",
            "code": "WORKFLOW_PLAN_AUTH_REQUIRED",
        })),
    ))
}

async fn append_workflow_plan_materialization_audit(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    actor_id: &str,
    requested_creator_id: Option<&str>,
    plan_id: Option<&str>,
    automation_id: &str,
    plan_source: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    crate::audit::append_protected_audit_event(
        state,
        "workflow_plan.materialized",
        tenant_context,
        Some(actor_id.to_string()),
        json!({
            "plan_id": plan_id,
            "automation_id": automation_id,
            "actor_id": actor_id,
            "requested_creator_id": requested_creator_id,
            "plan_source": plan_source,
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)
}

fn normalize_workflow_planning_record(
    planning: &mut WorkflowPlannerSessionPlanningRecord,
    current_plan_id: Option<&str>,
    now_ms: u64,
) {
    let mode = planning.mode.trim().to_ascii_lowercase();
    if planning.mode.trim().is_empty() || matches!(mode.as_str(), "planner" | "channel") {
        planning.mode = "workflow_planning".to_string();
    }
    if let Some(plan_id) = current_plan_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if planning.draft_id.is_none() {
            planning.draft_id = Some(plan_id.to_string());
        }
        if planning.linked_draft_plan_id.is_none() {
            planning.linked_draft_plan_id = Some(plan_id.to_string());
        }
    }
    if planning.source_platform.trim().is_empty() {
        planning.source_platform = "control_panel".to_string();
    }
    if planning.created_by_agent.is_none()
        && planning
            .source_platform
            .trim()
            .eq_ignore_ascii_case("control_panel")
    {
        planning.created_by_agent = Some("human".to_string());
    }
    if planning.validation_state.trim().is_empty() {
        planning.validation_state = match planning.validation_status.to_ascii_lowercase().as_str() {
            "ready" | "ready_for_apply" | "ready_for_activation" => "valid".to_string(),
            "blocked" => {
                if planning.approval_status.eq_ignore_ascii_case("requested") {
                    "needs_approval".to_string()
                } else {
                    "blocked".to_string()
                }
            }
            "needs_approval" => "needs_approval".to_string(),
            _ => "incomplete".to_string(),
        };
    }
    if planning.validation_status.trim().is_empty() {
        planning.validation_status = match planning.validation_state.to_ascii_lowercase().as_str() {
            "valid" => "ready_for_apply".to_string(),
            "needs_approval" | "blocked" => "blocked".to_string(),
            _ => "pending".to_string(),
        };
    }
    if planning.approval_status.trim().is_empty() {
        planning.approval_status = "not_required".to_string();
    }
    if planning.started_at_ms.is_none() {
        planning.started_at_ms = Some(now_ms);
    }
    planning.updated_at_ms = Some(now_ms);
}

fn workflow_planner_event_payload(
    session: &WorkflowPlannerSessionRecord,
    planning: &WorkflowPlannerSessionPlanningRecord,
    review: Option<&WorkflowPlanDraftReviewRecord>,
) -> Value {
    json!({
        "session_id": session.session_id,
        "project_slug": session.project_slug,
        "title": session.title,
        "plan_id": session.current_plan_id,
        "mode": planning.mode,
        "source_platform": planning.source_platform,
        "source_channel": planning.source_channel,
        "requesting_actor": planning.requesting_actor,
        "created_by_agent": planning.created_by_agent,
        "draft_id": planning.draft_id,
        "linked_channel_session_id": planning.linked_channel_session_id,
        "linked_draft_plan_id": planning.linked_draft_plan_id,
        "allowed_tools": planning.allowed_tools,
        "blocked_tools": planning.blocked_tools,
        "known_requirements": planning.known_requirements,
        "missing_requirements": planning.missing_requirements,
        "validation_state": planning.validation_state,
        "validation_status": planning.validation_status,
        "approval_status": planning.approval_status,
        "docs_mcp_enabled": planning.docs_mcp_enabled,
        "review": review.map(|review| json!({
            "required_capabilities": review.required_capabilities,
            "requested_capabilities": review.requested_capabilities,
            "blocked_capabilities": review.blocked_capabilities,
            "docs_mcp_used": review.docs_mcp_used,
            "validation_state": review.validation_state,
            "validation_status": review.validation_status,
            "approval_status": review.approval_status,
            "preview_payload": review.preview_payload,
        })),
    })
}

fn workflow_planner_publish_event(state: &AppState, event_type: &str, payload: Value) {
    state
        .event_bus
        .publish(EngineEvent::new(event_type.to_string(), payload));
}

async fn workflow_planner_request_capability_approval(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    planning: &WorkflowPlannerSessionPlanningRecord,
    blocked_capabilities: &[String],
    requested_capabilities: &[String],
    preview_payload: &Value,
    validation_status: &str,
) -> String {
    if blocked_capabilities.is_empty() {
        return "not_required".to_string();
    }
    if planning.approval_status.eq_ignore_ascii_case("requested") {
        return "requested".to_string();
    }

    let mcp_name = blocked_capabilities
        .first()
        .cloned()
        .unwrap_or_else(|| "workflow_planner".to_string());
    let rationale = format!(
        "Workflow planner draft `{}` needs capability review for blocked capabilities: {}",
        session.session_id,
        blocked_capabilities.join(", ")
    );
    let context = json!({
        "session_id": session.session_id,
        "project_slug": session.project_slug,
        "title": session.title,
        "plan_id": session.current_plan_id,
        "source_platform": planning.source_platform,
        "source_channel": planning.source_channel,
        "requesting_actor": planning.requesting_actor,
        "created_by_agent": planning.created_by_agent,
        "linked_channel_session_id": planning.linked_channel_session_id,
        "linked_draft_plan_id": planning.linked_draft_plan_id,
        "required_capabilities": planning.known_requirements,
        "missing_requirements": planning.missing_requirements,
        "blocked_capabilities": blocked_capabilities,
        "requested_capabilities": requested_capabilities,
        "docs_mcp_enabled": planning.docs_mcp_enabled,
        "validation_status": validation_status,
        "preview_payload": preview_payload,
    });
    let args = json!({
        "agent_id": session.session_id,
        "mcp_name": mcp_name.clone(),
        "catalog_slug": mcp_name,
        "rationale": rationale,
        "requested_tools": blocked_capabilities,
        "context": context,
        "expires_at_ms": crate::now_ms() + 7 * 24 * 60 * 60 * 1000,
    });
    let dispatch_context = state.tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("workflow_planner"),
        TenantContext::local_implicit(),
        vec!["mcp_request_capability".to_string()],
    );
    match state
        .tool_dispatcher
        .dispatch("mcp_request_capability", args, dispatch_context)
        .await
    {
        Ok(_) => "requested".to_string(),
        Err(_) => "blocked".to_string(),
    }
}

fn workflow_planner_publish_session_events(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    planning: &WorkflowPlannerSessionPlanningRecord,
    review: Option<&WorkflowPlanDraftReviewRecord>,
    draft_was_present: bool,
) {
    let event_payload = workflow_planner_event_payload(session, planning, review);
    workflow_planner_publish_event(
        state,
        if draft_was_present {
            "workflow_planner.draft.updated"
        } else {
            "workflow_planner.draft.created"
        },
        event_payload.clone(),
    );
    if !planning.missing_requirements.is_empty() {
        workflow_planner_publish_event(
            state,
            "workflow_planner.requirements.missing",
            event_payload.clone(),
        );
    }
    if !planning.blocked_tools.is_empty() {
        workflow_planner_publish_event(
            state,
            "workflow_planner.capability.blocked",
            event_payload.clone(),
        );
    }
    if planning.approval_status.eq_ignore_ascii_case("requested") {
        workflow_planner_publish_event(
            state,
            "workflow_planner.approval.requested",
            event_payload.clone(),
        );
    }
    if planning.docs_mcp_enabled == Some(true) {
        workflow_planner_publish_event(
            state,
            "workflow_planner.docs_mcp.used",
            event_payload.clone(),
        );
    }
    if planning.validation_state != "incomplete" {
        workflow_planner_publish_event(
            state,
            "workflow_planner.draft.validated",
            event_payload.clone(),
        );
        workflow_planner_publish_event(state, "workflow_planner.review.ready", event_payload);
    }
}
