// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Hosted operation grants are an additional gate. The handler must still
//! authorize the individual automation/run, owner, governance and data scope.
use axum::extract::{MatchedPath, Request};
use tandem_types::AccessPermission;

pub(super) fn required_permission(request: &Request) -> Option<AccessPermission> {
    let path = request.extensions().get::<MatchedPath>()?.as_str();
    let method = request.method().as_str();
    let read = matches!(method, "GET" | "HEAD");
    use AccessPermission::*;
    // The legacy automation and routine names are aliases for the same
    // handlers and must preserve the same hosted operation boundary.
    if let Some(suffix) = path
        .strip_prefix("/automations")
        .or_else(|| path.strip_prefix("/routines"))
    {
        let permission = match (method, suffix) {
            (
                _,
                ""
                | "/events"
                | "/{id}/history"
                | "/runs"
                | "/{id}/runs"
                | "/runs/{run_id}"
                | "/runs/{run_id}/artifacts",
            ) if read => Some(HostedAutomationRead),
            ("POST", "" | "/runs/{run_id}/artifacts") | ("PATCH" | "DELETE", "/{id}") => {
                Some(HostedAutomationWrite)
            }
            (
                "POST",
                "/{id}/run_now"
                | "/runs/{run_id}/approve"
                | "/runs/{run_id}/deny"
                | "/runs/{run_id}/pause"
                | "/runs/{run_id}/resume",
            ) => Some(HostedAutomationExecute),
            _ => None,
        };
        if permission.is_some() {
            return permission;
        }
    }
    match (method, path) {
        (
            _,
            "/automations/v2"
            | "/automations/v2/{id}"
            | "/automations/v2/events"
            | "/automations/v2/runs"
            | "/automations/v2/{id}/runs"
            | "/automations/v2/runs/{run_id}"
            | "/automations/v2/{id}/handoffs"
            | "/automations/v2/{id}/webhook-triggers"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries/{delivery_id}"
            | "/automations/v2/webhook-events"
            | "/automations/v2/webhook-events/{event_id}"
            | "/automations/v2/runs/{run_id}/webhook-events"
            | "/automations/v2/graduation/summary"
            | "/automations/v2/runs/{run_id}/tasks/{node_id}/reset_preview",
        ) if read => Some(HostedAutomationRead),
        ("POST", "/automations/v2/{id}/share") => Some(HostedAutomationShare),
        (
            "POST",
            "/automations/v2/{id}/run_now"
            | "/automations/v2/{id}/pause"
            | "/automations/v2/{id}/resume"
            | "/automations/v2/runs/{run_id}/pause"
            | "/automations/v2/runs/{run_id}/resume"
            | "/automations/v2/runs/{run_id}/cancel"
            | "/automations/v2/runs/{run_id}/recover"
            | "/automations/v2/runs/{run_id}/repair"
            | "/automations/v2/runs/{run_id}/tasks/{node_id}/retry"
            | "/automations/v2/runs/{run_id}/tasks/{node_id}/continue"
            | "/automations/v2/runs/{run_id}/tasks/{node_id}/requeue"
            | "/automations/v2/runs/{run_id}/backlog/tasks/{task_id}/claim"
            | "/automations/v2/runs/{run_id}/backlog/tasks/{task_id}/requeue",
        ) => Some(HostedAutomationExecute),
        ("POST", "/automations/v2")
        | ("POST", "/automations/v2/{id}/webhook-triggers")
        | ("PATCH" | "DELETE", "/automations/v2/{id}/webhook-triggers/{trigger_id}")
        | (
            "POST",
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/disable"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/rotate-secret"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/reveal-verification-token"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/reset-verification"
            | "/automations/v2/{id}/webhook-triggers/{trigger_id}/import-secret",
        )
        | ("PATCH" | "DELETE", "/automations/v2/{id}")
        | ("PATCH", "/automations/v2/runs/{run_id}/tasks/{node_id}/disposition") => {
            Some(HostedAutomationWrite)
        }
        // Gate decisions keep their independent reviewer and governance checks.
        ("POST", "/automations/v2/runs/{run_id}/gate") => Some(HostedAutomationExecute),
        (
            _,
            "/workflows"
            | "/workflows/{id}"
            | "/workflows/runs"
            | "/workflows/runs/{id}"
            | "/workflows/events"
            | "/workflow-hooks",
        ) if read => Some(HostedWorkflowRead),
        ("POST", "/workflows/simulate") => Some(HostedWorkflowRead),
        ("POST", "/workflows/validate" | "/workflows/{id}/run" | "/workflows/runs/{id}/gate")
        | ("PATCH", "/workflow-hooks/{id}") => Some(HostedUse),
        _ => None,
    }
}
