// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Hosted operation grants are an additional gate. The handler must still
//! authorize the individual automation/run, owner, governance and data scope.
use axum::extract::{MatchedPath, Request};
use tandem_types::AccessPermission;

#[cfg(test)]
#[path = "hosted_session_route_tests.rs"]
mod session_route_tests;

pub(super) fn required_permission(request: &Request) -> Option<AccessPermission> {
    let path = request.extensions().get::<MatchedPath>()?.as_str();
    let method = request.method().as_str();
    let read = matches!(method, "GET" | "HEAD");
    use AccessPermission::*;
    // Session state changes require execution authority on both HTTP aliases.
    // Reads, no-op init, separately host-authorized command/shell endpoints,
    // and independent governance reviewer routes retain their own boundaries.
    let session_path = path.strip_prefix("/api").unwrap_or(path);
    if matches!(
        (method, session_path),
        (
            "POST",
            "/session"
                | "/session/{id}/attach"
                | "/session/{id}/workspace/override"
                | "/session/{id}/message"
                | "/session/{id}/prompt_async"
                | "/session/{id}/prompt_sync"
                | "/session/{id}/abort"
                | "/session/{id}/cancel"
                | "/session/{id}/run/{run_id}/cancel"
                | "/session/{id}/fork"
                | "/session/{id}/revert"
                | "/session/{id}/unrevert"
                | "/session/{id}/share"
                | "/session/{id}/summarize"
        ) | ("PATCH" | "DELETE", "/session/{id}")
            | ("DELETE", "/session/{id}/share")
    ) {
        return Some(HostedUse);
    }
    // Long-running goals expose objectives, lineage, events and artifacts;
    // their mutations can create or control persistent root runs. Object
    // visibility and reviewer authority remain handler-level checks.
    match (method, path) {
        (_, "/provider/auth" | "/provider/{id}/oauth/status") if read => {
            return Some(HostedUse);
        }
        (_, "/global/storage/files") if read => return Some(HostedAdmin),
        (_, "/external-actions" | "/external-actions/{id}") if read => {
            return Some(HostedAutomationRead);
        }
        // Project discovery is a projection of session directories, so it
        // requires the same hosted use grant as session creation. The handler
        // independently filters each session to the requesting actor.
        (_, "/project") if read => return Some(HostedUse),
        (
            _,
            "/goal-capability-learning/decisions"
            | "/goal-capability-learning/decisions/{decision_id}",
        ) if read => return Some(HostedAutomationRead),
        ("POST", "/goal-capability-learning/discover") => return Some(HostedUse),
        (
            _,
            "/goals"
            | "/goals/{goal_id}"
            | "/goals/{goal_id}/projection"
            | "/goals/{goal_id}/graph"
            | "/goals/{goal_id}/runs"
            | "/goals/{goal_id}/events"
            | "/goals/{goal_id}/events/stream"
            | "/goals/{goal_id}/artifacts"
            | "/goals/{goal_id}/budgets"
            | "/goals/{goal_id}/handoffs"
            | "/goals/{goal_id}/waits"
            | "/goals/{goal_id}/waits/{wait_id}",
        ) if read => return Some(HostedAutomationRead),
        (
            "POST",
            "/goals"
            | "/goals/{goal_id}/actions/{action_id}"
            | "/goals/{goal_id}/pause"
            | "/goals/{goal_id}/resume"
            | "/goals/{goal_id}/cancel"
            | "/goals/{goal_id}/transitions"
            | "/goals/{goal_id}/completion"
            | "/goals/{goal_id}/handoffs/{handoff_id}/decision"
            | "/goals/{goal_id}/waits/{wait_id}/resolve",
        ) => return Some(HostedUse),
        _ => {}
    }
    match (method, path) {
        ("PUT", "/capabilities/bindings")
        | (
            "POST",
            "/capabilities/bindings/refresh-builtins" | "/capabilities/bindings/reset-to-builtins",
        ) => return Some(HostedAdmin),
        ("PUT", "/channels/{name}/tool-preferences") => return Some(HostedAdmin),
        ("GET" | "HEAD" | "POST", "/incident-monitor/intake/keys")
        | ("POST", "/incident-monitor/intake/keys/{id}/disable") => return Some(HostedAdmin),
        ("PATCH", "/config/incident-monitor")
        | ("POST", "/incident-monitor/pause" | "/incident-monitor/resume") => {
            return Some(HostedAdmin);
        }
        // These mutate the deployment-wide registry, not a caller-owned
        // automation. Export also writes a caller-selected filesystem target.
        ("POST", "/presets/fork" | "/presets/export_overrides")
        | ("PUT" | "DELETE", "/presets/overrides/{kind}/{id}") => return Some(HostedAdmin),
        // Both project (process cwd) and global skill roots are shared registry
        // state. Include the legacy import alias; discovery remains separate.
        (
            "POST",
            "/skills"
            | "/skills/import"
            | "/skills/generate/install"
            | "/skills/templates/{id}/install",
        )
        | ("DELETE", "/skills/{name}") => return Some(HostedAdmin),
        (_, "/automations/channel-drafts/pending") if read => return Some(HostedAutomationRead),
        (
            "POST",
            "/automations/channel-drafts"
            | "/automations/channel-drafts/{draft_id}/answer"
            | "/automations/channel-drafts/{draft_id}/confirm"
            | "/automations/channel-drafts/{draft_id}/cancel",
        ) => return Some(HostedAutomationWrite),
        _ => {}
    }
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
        (
            "POST",
            "/automations/v2"
                | "/workflow-plans/preview"
                | "/workflow-plans/chat/start"
                | "/workflow-plans/apply"
                | "/mission-builder/apply",
        )
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
        // Each of these may materialize and queue the system-owned triage
        // automation. Replay is an execution entrypoint too, not a read.
        // Draft approval and downstream governance checks remain independent.
        (
            "POST",
            "/incident-monitor/drafts/{id}/triage-run"
            | "/incident-monitor/drafts/{id}/approve"
            | "/incident-monitor/incidents/{id}/replay"
            | "/incident-monitor/log-sources/{project_id}/{source_id}/replay-latest",
        ) => Some(HostedAutomationExecute),
        (
            _,
            "/optimizations"
            | "/optimizations/{id}"
            | "/optimizations/{id}/experiments"
            | "/optimizations/{id}/experiments/{experiment_id}",
        ) if read => Some(HostedAutomationRead),
        (
            "POST",
            "/optimizations"
            | "/optimizations/{id}/actions"
            | "/optimizations/{id}/experiments/{experiment_id}",
        ) => Some(HostedAutomationWrite),
        (_, "/workflow-learning/candidates") if read => Some(HostedAutomationRead),
        (
            "POST",
            "/workflow-learning/candidates/{candidate_id}/review"
            | "/workflow-learning/candidates/{candidate_id}/promote"
            | "/workflow-learning/candidates/{candidate_id}/spawn-revision",
        ) => Some(HostedAutomationWrite),
        (_, "/workflow-plans/sessions" | "/workflow-plans/sessions/{session_id}") if read => {
            Some(HostedAutomationRead)
        }
        ("POST", "/workflow-plans/sessions")
        | ("PATCH" | "DELETE", "/workflow-plans/sessions/{session_id}")
        | (
            "POST",
            "/workflow-plans/sessions/{session_id}/duplicate"
            | "/workflow-plans/sessions/{session_id}/start"
            | "/workflow-plans/sessions/{session_id}/start-async"
            | "/workflow-plans/sessions/{session_id}/message"
            | "/workflow-plans/sessions/{session_id}/message-async"
            | "/workflow-plans/sessions/{session_id}/reset",
        ) => Some(HostedAutomationWrite),
        (
            _,
            "/orchestrations"
            | "/orchestrations/{orchestration_id}"
            | "/orchestrations/{orchestration_id}/versions"
            | "/orchestrations/{orchestration_id}/versions/{version}"
            | "/orchestrations/{orchestration_id}/stale-references",
        ) if read => Some(HostedAutomationRead),
        (
            "POST",
            "/orchestrations/{orchestration_id}/validate"
            | "/orchestrations/{orchestration_id}/dry-run",
        ) => Some(HostedAutomationRead),
        ("POST", "/orchestrations")
        | ("PUT", "/orchestrations/{orchestration_id}")
        | (
            "POST",
            "/orchestrations/{orchestration_id}/archive"
            | "/orchestrations/{orchestration_id}/publish"
            | "/orchestrations/{orchestration_id}/refresh-references",
        ) => Some(HostedAutomationWrite),
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
        ("POST", "/workflows/validate" | "/workflows/{id}/run" | "/workflows/runs/{id}/gate") => {
            Some(HostedUse)
        }
        // Static hook specs have no per-owner ACL; overrides change the shared registry.
        ("PATCH", "/workflow-hooks/{id}") => Some(HostedAdmin),
        _ => None,
    }
}
