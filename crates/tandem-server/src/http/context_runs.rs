use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};

include!("context_runs_parts/part01.rs");
include!("context_runs_parts/part02.rs");
include!("context_runs_parts/part03.rs");
include!("context_runs_parts/part04.rs");

pub async fn context_run_effective_started_at_ms(
    state: &AppState,
    run_id: &str,
) -> Result<u64, StatusCode> {
    let run = load_context_run_state(state, run_id).await?;
    Ok(run.started_at_ms.unwrap_or(run.created_at_ms))
}

/// Diagnostic snapshot of a context run at the moment the bug-monitor
/// triage deadline fires. Returns enough state for the resulting
/// GitHub issue (and any human reading 10 timeout issues in a row) to
/// see _where_ the triage was when it timed out — which step was
/// active, how stale the run was, what the final status was —
/// without needing to dig through JSONL event logs.
pub async fn bug_monitor_triage_timeout_diagnostics(
    state: &AppState,
    run_id: &str,
    timeout_ms: u64,
) -> Option<serde_json::Value> {
    let run = load_context_run_state(state, run_id).await.ok()?;
    let now = crate::now_ms();
    let started_at_ms = run.started_at_ms.unwrap_or(run.created_at_ms);
    let elapsed_ms = now.saturating_sub(started_at_ms);
    let stale_ms = now.saturating_sub(run.updated_at_ms);
    let status = serde_json::to_value(&run.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let total_steps = run.steps.len();
    let active_step = run.steps.iter().find(|step| {
        matches!(
            step.status,
            ContextStepStatus::Pending
                | ContextStepStatus::Runnable
                | ContextStepStatus::InProgress
                | ContextStepStatus::Blocked
        )
    });
    let completed_steps = run
        .steps
        .iter()
        .filter(|step| matches!(step.status, ContextStepStatus::Done))
        .count();
    let failed_steps = run
        .steps
        .iter()
        .filter(|step| matches!(step.status, ContextStepStatus::Failed))
        .count();
    let active_step_summary = active_step.map(|step| {
        let step_status = serde_json::to_value(&step.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        json!({
            "step_id": step.step_id,
            "title": step.title,
            "status": step_status,
        })
    });
    Some(json!({
        "run_id": run.run_id,
        "run_status": status,
        "timeout_ms": timeout_ms,
        "elapsed_ms": elapsed_ms,
        "stale_ms": stale_ms,
        "last_event_seq": run.last_event_seq,
        "step_count": total_steps,
        "completed_steps": completed_steps,
        "failed_steps": failed_steps,
        "active_step": active_step_summary,
    }))
}

/// Render the diagnostics object as a markdown-friendly multi-line
/// string suitable for embedding in `last_post_error` or in an issue
/// body section.
pub fn format_bug_monitor_triage_timeout_diagnostics(value: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    let push_kv = |lines: &mut Vec<String>, key: &str, value: &str| {
        lines.push(format!("{key}: {value}"));
    };
    if let Some(timeout_ms) = value.get("timeout_ms").and_then(serde_json::Value::as_u64) {
        push_kv(&mut lines, "timeout_ms", &timeout_ms.to_string());
    }
    if let Some(elapsed_ms) = value.get("elapsed_ms").and_then(serde_json::Value::as_u64) {
        push_kv(&mut lines, "elapsed_ms", &elapsed_ms.to_string());
    }
    if let Some(stale_ms) = value.get("stale_ms").and_then(serde_json::Value::as_u64) {
        push_kv(
            &mut lines,
            "stale_ms",
            &format!(
                "{stale_ms} ({})",
                if stale_ms > 60_000 {
                    "no run-state updates for over a minute — likely stuck"
                } else {
                    "recent updates — likely slow rather than stuck"
                }
            ),
        );
    }
    if let Some(status) = value.get("run_status").and_then(serde_json::Value::as_str) {
        push_kv(&mut lines, "run_status", status);
    }
    if let Some(seq) = value
        .get("last_event_seq")
        .and_then(serde_json::Value::as_u64)
    {
        push_kv(&mut lines, "last_event_seq", &seq.to_string());
    }
    if let (Some(completed), Some(failed), Some(total)) = (
        value
            .get("completed_steps")
            .and_then(serde_json::Value::as_u64),
        value
            .get("failed_steps")
            .and_then(serde_json::Value::as_u64),
        value.get("step_count").and_then(serde_json::Value::as_u64),
    ) {
        push_kv(
            &mut lines,
            "steps",
            &format!("{completed}/{total} done, {failed} failed"),
        );
    }
    if let Some(active) = value.get("active_step") {
        if !active.is_null() {
            if let Some(step_id) = active.get("step_id").and_then(serde_json::Value::as_str) {
                push_kv(&mut lines, "active_step_id", step_id);
            }
            if let Some(title) = active.get("title").and_then(serde_json::Value::as_str) {
                push_kv(&mut lines, "active_step_title", title);
            }
            if let Some(status) = active.get("status").and_then(serde_json::Value::as_str) {
                push_kv(&mut lines, "active_step_status", status);
            }
        }
    }
    if let Some(node_attempts) = value
        .get("node_attempts")
        .and_then(serde_json::Value::as_array)
        .filter(|nodes| !nodes.is_empty())
    {
        // Per-step activity. Surfaces tool-call counts and the
        // wall-clock span we observed activity in for each node so an
        // operator can read "step X had 0 tool calls but ran 240s
        // before timing out" (model latency dominated) versus "step Y
        // had 18 tool calls in 240s" (tool round-trips dominated).
        // Per-LLM-call timing isn't here yet — that requires
        // persisting `provider.call.iteration.*` events to receipts,
        // which is a separate change in tandem-core.
        lines.push(String::new());
        lines.push("per_step_activity:".to_string());
        for node in node_attempts.iter().take(8) {
            let node_id = node
                .get("node_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let attempts = node
                .get("max_attempt")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let tool_invocations = node
                .get("tool_invocations")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let tool_failed = node
                .get("tool_failed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let activity_span = node
                .get("activity_span_ms")
                .and_then(serde_json::Value::as_u64);
            let span = activity_span
                .map(|ms| format!("{ms}ms"))
                .unwrap_or_else(|| "?ms".to_string());
            lines.push(format!(
                "  - {node_id}: attempt={attempts} tool_calls={tool_invocations} tool_failed={tool_failed} activity_span={span}"
            ));
        }
    }
    lines.join("\n")
}
