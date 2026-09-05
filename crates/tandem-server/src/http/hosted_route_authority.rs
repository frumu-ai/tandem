//! Hosted operation grants are an additional gate. The handler must still
//! authorize the individual automation/run, owner, governance and data scope.
use axum::extract::{MatchedPath, Request};
use tandem_types::AccessPermission;

pub(super) fn required_permission(request: &Request) -> Option<AccessPermission> {
    let path = request.extensions().get::<MatchedPath>()?.as_str();
    let method = request.method().as_str();
    let read = matches!(method, "GET" | "HEAD");
    use AccessPermission::*;
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
        | ("PATCH" | "DELETE", "/automations/v2/{id}")
        | ("PATCH", "/automations/v2/runs/{run_id}/tasks/{node_id}/disposition") => {
            Some(HostedAutomationWrite)
        }
        // Gate decisions keep their independent reviewer and governance checks.
        ("POST", "/automations/v2/runs/{run_id}/gate") => Some(HostedAutomationExecute),
        (_, "/workflows/runs" | "/workflows/runs/{id}" | "/workflows/events") if read => {
            Some(HostedWorkflowRead)
        }
        _ => None,
    }
}
