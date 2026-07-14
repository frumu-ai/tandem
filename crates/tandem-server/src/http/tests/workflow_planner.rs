use super::*;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

include!("workflow_planner_parts/part01.rs");
include!("workflow_planner_parts/part03.rs");

use crate::workflow_planner::{WorkflowPlannerSessionPlanningRecord, WorkflowPlannerSessionRecord};
use tandem_types::EngineEvent;
use tandem_workflows::plan_package::WorkflowPlanDraftReviewRecord;

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
        tandem_tools::ToolDispatchSource::new("workflow_planner_test"),
        tandem_types::TenantContext::local_implicit(),
        Vec::new(),
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
include!("workflow_planner_parts/part02.rs");
