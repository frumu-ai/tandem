use std::time::Duration;

use serde_json::Value;
use tandem_types::{EngineEvent, MessagePartInput, SendMessageRequest, Session};

use crate::app::state::{
    derive_status_index_update, extract_persistable_tool_part, truncate_text, AppState,
};
use crate::bug_monitor::types::{BugMonitorConfig, BugMonitorIncidentRecord};
use crate::http::context_runs::{
    append_context_run_event, ensure_session_context_run, session_run_status_to_context,
};
use crate::http::context_types::{ContextRunEventAppendInput, ContextRunStatus};
use crate::routines::types::{RoutineHistoryEvent, RoutineRunStatus};
use crate::util::time::now_ms;

async fn wait_for_runtime_ready_or_exit(state: &AppState, component: &str) -> bool {
    if state.wait_until_ready_or_failed(120, 250).await {
        return true;
    }
    let startup = state.startup_snapshot().await;
    tracing::warn!(
        component,
        startup_status = ?startup.status,
        startup_phase = %startup.phase,
        attempt_id = %startup.attempt_id,
        "background task exiting before runtime access because startup did not become ready"
    );
    false
}

fn extract_event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .or_else(|| properties.get("sessionId"))
        .or_else(|| properties.get("id"))
        .or_else(|| {
            properties
                .get("record")
                .and_then(|record| record.get("session_id"))
        })
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionID"))
        })
        .or_else(|| {
            properties
                .get("part")
                .and_then(|part| part.get("sessionId"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_event_correlation_id(properties: &Value) -> Option<String> {
    properties
        .get("correlationID")
        .or_else(|| properties.get("correlationId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn apply_provider_usage_to_routine_run(
    state: &AppState,
    run_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) {
    let rate = state.token_cost_per_1k_usd.max(0.0);
    let delta_cost = (total_tokens as f64 / 1000.0) * rate;
    let mut guard = state.routine_runs.write().await;
    if let Some(run) = guard.get_mut(run_id) {
        run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
        run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
        run.total_tokens = run.total_tokens.saturating_add(total_tokens);
        run.estimated_cost_usd += delta_cost;
        run.updated_at_ms = now_ms();
    }
    drop(guard);
    let _ = state.persist_routine_runs().await;
}

async fn apply_provider_usage_to_automation_v2_run(
    state: &AppState,
    run_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) {
    let rate = state.token_cost_per_1k_usd.max(0.0);
    let delta_cost = (total_tokens as f64 / 1000.0) * rate;
    let mut guard = state.automation_v2_runs.write().await;
    if let Some(run) = guard.get_mut(run_id) {
        run.prompt_tokens = run.prompt_tokens.saturating_add(prompt_tokens);
        run.completion_tokens = run.completion_tokens.saturating_add(completion_tokens);
        run.total_tokens = run.total_tokens.saturating_add(total_tokens);
        run.estimated_cost_usd += delta_cost;
        run.updated_at_ms = now_ms();
    }
    drop(guard);
    let _ = state.persist_automation_v2_runs().await;
    let _ = state
        .record_automation_v2_spend(
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            delta_cost,
        )
        .await;
}

fn event_tenant_context_value(event: &EngineEvent) -> Value {
    event
        .properties
        .get("tenantContext")
        .cloned()
        .unwrap_or(Value::Null)
}

fn session_context_run_event_input(event: &EngineEvent) -> Option<ContextRunEventAppendInput> {
    match event.event_type.as_str() {
        "session.run.started" => Some(ContextRunEventAppendInput {
            event_type: "session_run_started".to_string(),
            status: ContextRunStatus::Running,
            step_id: Some("session-run".to_string()),
            payload: serde_json::json!({
                "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                "runID": event.properties.get("runID").cloned().unwrap_or(Value::Null),
                "agentID": event.properties.get("agentID").cloned().unwrap_or(Value::Null),
                "agentProfile": event.properties.get("agentProfile").cloned().unwrap_or(Value::Null),
                "tenantContext": event_tenant_context_value(event),
                "why_next_step": "session run in progress",
                "step_status": "in_progress",
            }),
        }),
        "message.part.updated" => {
            let part = event.properties.get("part")?;
            let part_type = part
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if !matches!(
                part_type,
                "tool" | "tool-invocation" | "tool-result" | "tool_invocation" | "tool_result"
            ) {
                return None;
            }
            let tool_name = part
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("tool");
            let tool_state = part
                .get("state")
                .and_then(|value| value.as_str())
                .unwrap_or("running");
            let why_next_step = match tool_state {
                "completed" => format!("tool `{tool_name}` completed"),
                "failed" => format!("tool `{tool_name}` failed"),
                _ => format!("tool `{tool_name}` running"),
            };
            Some(ContextRunEventAppendInput {
                event_type: "session_tool_updated".to_string(),
                status: ContextRunStatus::Running,
                step_id: Some("session-run".to_string()),
                payload: serde_json::json!({
                    "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                    "runID": event.properties.get("runID").cloned().unwrap_or(Value::Null),
                    "part": part.clone(),
                    "toolCallDelta": event.properties.get("toolCallDelta").cloned().unwrap_or(Value::Null),
                    "tenantContext": event_tenant_context_value(event),
                    "why_next_step": why_next_step,
                    "step_status": if tool_state == "completed" { "done" } else { "in_progress" },
                    "error": part.get("error").cloned().unwrap_or(Value::Null),
                }),
            })
        }
        "session.run.finished" => {
            let status = event
                .properties
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("completed");
            Some(ContextRunEventAppendInput {
                event_type: "session_run_finished".to_string(),
                status: session_run_status_to_context(status),
                step_id: Some("session-run".to_string()),
                payload: serde_json::json!({
                    "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                    "runID": event.properties.get("runID").cloned().unwrap_or(Value::Null),
                    "status": status,
                    "error": event.properties.get("error").cloned().unwrap_or(Value::Null),
                    "tenantContext": event_tenant_context_value(event),
                    "why_next_step": format!("session run finished with status `{status}`"),
                    "step_status": if matches!(status, "completed") { "done" } else if matches!(status, "cancelled") { "blocked" } else { "failed" },
                }),
            })
        }
        "tool.effect.recorded" => {
            let record = event.properties.get("record")?;
            let tool = record
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("tool");
            let status = record
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("started");
            let phase = record
                .get("phase")
                .and_then(|value| value.as_str())
                .unwrap_or("invocation");
            let summary = match status {
                "succeeded" => format!("tool `{tool}` {phase} succeeded"),
                "failed" => format!("tool `{tool}` {phase} failed"),
                "blocked" => format!("tool `{tool}` {phase} blocked"),
                _ => format!("tool `{tool}` {phase} started"),
            };
            Some(ContextRunEventAppendInput {
                event_type: "tool_effect_recorded".to_string(),
                status: ContextRunStatus::Running,
                step_id: Some("session-run".to_string()),
                payload: serde_json::json!({
                    "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                    "messageID": event.properties.get("messageID").cloned().unwrap_or(Value::Null),
                    "tool": event.properties.get("tool").cloned().unwrap_or(Value::Null),
                    "record": record.clone(),
                    "tenantContext": event_tenant_context_value(event),
                    "why_next_step": summary,
                    "step_status": if matches!(status, "failed" | "blocked") {
                        "blocked"
                    } else {
                        "in_progress"
                    },
                }),
            })
        }
        "mutation.checkpoint.recorded" => {
            let record = event.properties.get("record")?;
            let tool = record
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("tool");
            let outcome = record
                .get("outcome")
                .and_then(|value| value.as_str())
                .unwrap_or("succeeded");
            let changed_file_count = record
                .get("changed_file_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            Some(ContextRunEventAppendInput {
                event_type: "mutation_checkpoint_recorded".to_string(),
                status: ContextRunStatus::Running,
                step_id: Some("session-run".to_string()),
                payload: serde_json::json!({
                    "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                    "messageID": event.properties.get("messageID").cloned().unwrap_or(Value::Null),
                    "tool": event.properties.get("tool").cloned().unwrap_or(Value::Null),
                    "record": record.clone(),
                    "tenantContext": event_tenant_context_value(event),
                    "why_next_step": format!(
                        "mutation checkpoint for `{tool}` recorded with outcome `{outcome}` and {changed_file_count} changed files"
                    ),
                    "step_status": if matches!(outcome, "failed" | "blocked") {
                        "blocked"
                    } else {
                        "in_progress"
                    },
                }),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn routine_background_tasks_exit_without_runtime_when_startup_failed() {
        let state = AppState::new_starting("routine-startup-guard-test".to_string(), true);
        state.mark_failed("test_failed", "startup failed").await;

        tokio::time::timeout(
            Duration::from_millis(250),
            run_routine_scheduler(state.clone()),
        )
        .await
        .expect("scheduler should exit when startup has failed");

        tokio::time::timeout(
            Duration::from_millis(250),
            run_routine_executor(state.clone()),
        )
        .await
        .expect("executor should exit when startup has failed");
    }

    #[test]
    fn session_context_run_event_input_maps_tool_effect_events() {
        let input = session_context_run_event_input(&EngineEvent::new(
            "tool.effect.recorded",
            serde_json::json!({
                "sessionID": "session-1",
                "messageID": "message-1",
                "tool": "write",
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": "write",
                    "phase": "outcome",
                    "status": "succeeded",
                    "args_summary": {"path": "src/lib.rs"}
                }
            }),
        ))
        .expect("tool effect append input");

        assert_eq!(input.event_type, "tool_effect_recorded");
        assert_eq!(input.status, ContextRunStatus::Running);
        assert_eq!(
            input.payload.get("tool").and_then(Value::as_str),
            Some("write")
        );
        assert_eq!(
            input
                .payload
                .get("record")
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str),
            Some("succeeded")
        );
    }

    #[test]
    fn session_context_run_event_input_maps_mutation_checkpoint_events() {
        let input = session_context_run_event_input(&EngineEvent::new(
            "mutation.checkpoint.recorded",
            serde_json::json!({
                "sessionID": "session-1",
                "messageID": "message-1",
                "tool": "write",
                "record": {
                    "session_id": "session-1",
                    "message_id": "message-1",
                    "tool": "write",
                    "outcome": "succeeded",
                    "file_count": 1,
                    "changed_file_count": 1,
                    "files": [{
                        "path": "src/lib.rs",
                        "resolved_path": "/workspace/src/lib.rs",
                        "existed_before": false,
                        "existed_after": true,
                        "changed": true,
                        "rollback_snapshot": {
                            "status": "not_needed"
                        }
                    }]
                }
            }),
        ))
        .expect("mutation checkpoint append input");

        assert_eq!(input.event_type, "mutation_checkpoint_recorded");
        assert_eq!(
            input.payload.get("tool").and_then(Value::as_str),
            Some("write")
        );
        assert_eq!(
            input
                .payload
                .get("record")
                .and_then(|value| value.get("changed_file_count"))
                .and_then(Value::as_u64),
            Some(1)
        );
    }
}

pub async fn run_session_part_persister(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("session part persister: skipped because runtime did not become ready");
        return;
    }
    let Some(mut rx) = state.event_bus.take_session_part_receiver() else {
        tracing::warn!("session part persister: skipped because receiver was already taken");
        return;
    };
    while let Some(event) = rx.recv().await {
        if event.event_type != "message.part.updated" {
            continue;
        }
        let Some(session_id) = extract_event_session_id(&event.properties) else {
            continue;
        };
        let Some((message_id, part)) = extract_persistable_tool_part(&event.properties) else {
            continue;
        };
        if let Err(error) = state
            .storage
            .append_message_part(&session_id, &message_id, part)
            .await
        {
            tracing::warn!(
                "session part persister failed for session={} message={}: {error:#}",
                session_id,
                message_id
            );
        }
    }
}

pub async fn run_status_indexer(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("status indexer: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Some(update) = derive_status_index_update(&event) {
                    if let Err(error) = state
                        .put_shared_resource(
                            update.key,
                            update.value,
                            None,
                            "system.status_indexer".to_string(),
                            None,
                        )
                        .await
                    {
                        tracing::warn!("status indexer failed to persist update: {error:?}");
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_session_context_run_journaler(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!(
            "session context run journaler: skipped because runtime did not become ready"
        );
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                let Some(session_id) = extract_event_session_id(&event.properties) else {
                    continue;
                };
                let Some(input) = session_context_run_event_input(&event) else {
                    continue;
                };
                let Some(session) = state.storage.get_session(&session_id).await else {
                    continue;
                };
                let Ok(run_id) = ensure_session_context_run(&state, &session).await else {
                    tracing::warn!(
                        "session context run journaler could not ensure context run for session={session_id}"
                    );
                    continue;
                };
                if let Err(error) = append_context_run_event(&state, &run_id, input).await {
                    tracing::warn!(
                        "session context run journaler failed for session={} run={}: {:?}",
                        session_id,
                        run_id,
                        error
                    );
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn run_agent_team_supervisor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("agent team supervisor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                state.agent_teams.handle_engine_event(&state, &event).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

fn is_bug_monitor_candidate_event(event: &EngineEvent) -> bool {
    if event.event_type.starts_with("bug_monitor.") {
        return false;
    }
    matches!(
        event.event_type.as_str(),
        "context.task.failed"
            | "context.task.blocked"
            | "context.run.failed"
            | "workflow.run.failed"
            | "workflow.validation.failed"
            | "routine.run.failed"
            | "session.error"
            | "automation.run.failed"
            | "automation_v2.run.failed"
            | "coder.run.failed"
    )
}

pub async fn run_bug_monitor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("bug monitor: skipped because runtime did not become ready");
        return;
    }
    state
        .update_bug_monitor_runtime_status(|runtime| {
            runtime.monitoring_active = false;
            runtime.last_runtime_error = None;
        })
        .await;
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if !is_bug_monitor_candidate_event(&event) {
                    continue;
                }
                let status = state.bug_monitor_status().await;
                if !status.config.enabled || status.config.paused || !status.readiness.repo_valid {
                    state
                        .update_bug_monitor_runtime_status(|runtime| {
                            runtime.monitoring_active = status.config.enabled
                                && !status.config.paused
                                && status.readiness.repo_valid;
                            runtime.paused = status.config.paused;
                            runtime.last_runtime_error = status.last_error.clone();
                        })
                        .await;
                    continue;
                }
                match crate::bug_monitor::service::process_event(&state, &event, &status.config)
                    .await
                {
                    Ok(incident) => {
                        state
                            .update_bug_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type =
                                    Some(incident.event_type.clone());
                                runtime.last_runtime_error = None;
                            })
                            .await;
                    }
                    Err(error) => {
                        let detail = truncate_text(&error.to_string(), 500);
                        state
                            .update_bug_monitor_runtime_status(|runtime| {
                                runtime.monitoring_active = true;
                                runtime.paused = status.config.paused;
                                runtime.last_processed_at_ms = Some(now_ms());
                                runtime.last_incident_event_type = Some(event.event_type.clone());
                                runtime.last_runtime_error = Some(detail.clone());
                            })
                            .await;
                        state.event_bus.publish(EngineEvent::new(
                            "bug_monitor.error",
                            serde_json::json!({
                                "eventType": event.event_type,
                                "detail": detail,
                            }),
                        ));
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                state
                    .update_bug_monitor_runtime_status(|runtime| {
                        runtime.last_runtime_error =
                            Some(format!("Bug monitor lagged and dropped {count} events."));
                    })
                    .await;
            }
        }
    }
}

pub async fn run_usage_aggregator(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("usage aggregator: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if event.event_type != "provider.usage" {
                    continue;
                }
                let prompt_tokens = event
                    .properties
                    .get("promptTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let completion_tokens = event
                    .properties
                    .get("completionTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total_tokens = event
                    .properties
                    .get("totalTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
                if let Some(correlation_id) = extract_event_correlation_id(&event.properties) {
                    if let Some(run_id) = correlation_id.strip_prefix("routine:") {
                        apply_provider_usage_to_routine_run(
                            &state,
                            run_id,
                            prompt_tokens,
                            completion_tokens,
                            total_tokens,
                        )
                        .await;
                        continue;
                    }
                    if let Some(run_id) = correlation_id.strip_prefix("automation-v2:") {
                        apply_provider_usage_to_automation_v2_run(
                            &state,
                            run_id,
                            prompt_tokens,
                            completion_tokens,
                            total_tokens,
                        )
                        .await;
                        continue;
                    }
                }
                let session_id = event
                    .properties
                    .get("sessionID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session_id.is_empty() {
                    continue;
                }
                state
                    .apply_provider_usage_to_runs(
                        session_id,
                        prompt_tokens,
                        completion_tokens,
                        total_tokens,
                    )
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

async fn process_bug_monitor_event(
    state: &AppState,
    event: &EngineEvent,
    config: &BugMonitorConfig,
) -> anyhow::Result<BugMonitorIncidentRecord> {
    crate::app::state::process_bug_monitor_event(state, event, config).await
}

pub async fn run_routine_scheduler(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "routine_scheduler").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let now = now_ms();
        let plans = state.evaluate_routine_misfires(now).await;
        for plan in plans {
            let Some(routine) = state.get_routine(&plan.routine_id).await else {
                continue;
            };
            match crate::app::state::evaluate_routine_execution_policy(&routine, "scheduled") {
                crate::app::state::RoutineExecutionDecision::Allowed => {
                    let _ = state.mark_routine_fired(&plan.routine_id, now).await;
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::Queued,
                            None,
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "queued".to_string(),
                            detail: None,
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.fired",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "scheduledAtMs": plan.scheduled_at_ms,
                            "nextFireAtMs": plan.next_fire_at_ms,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
                crate::app::state::RoutineExecutionDecision::RequiresApproval { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::PendingApproval,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "pending_approval".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.approval_required",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
                crate::app::state::RoutineExecutionDecision::Blocked { reason } => {
                    let run = state
                        .create_routine_run(
                            &routine,
                            "scheduled",
                            plan.run_count,
                            RoutineRunStatus::BlockedPolicy,
                            Some(reason.clone()),
                        )
                        .await;
                    state
                        .append_routine_history(RoutineHistoryEvent {
                            routine_id: plan.routine_id.clone(),
                            trigger_type: "scheduled".to_string(),
                            run_count: plan.run_count,
                            fired_at_ms: now,
                            status: "blocked_policy".to_string(),
                            detail: Some(reason.clone()),
                        })
                        .await;
                    state.event_bus.publish(EngineEvent::new(
                        "routine.blocked",
                        serde_json::json!({
                            "routineID": plan.routine_id,
                            "runID": run.run_id,
                            "runCount": plan.run_count,
                            "triggerType": "scheduled",
                            "reason": reason,
                        }),
                    ));
                    state.event_bus.publish(EngineEvent::new(
                        "routine.run.created",
                        serde_json::json!({
                            "run": run,
                        }),
                    ));
                }
            }
        }
    }
}

pub async fn run_routine_executor(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "routine_executor").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(run) = state.claim_next_queued_routine_run().await else {
            continue;
        };

        state.event_bus.publish(EngineEvent::new(
            "routine.run.started",
            serde_json::json!({
                "runID": run.run_id,
                "routineID": run.routine_id,
                "triggerType": run.trigger_type,
                "startedAtMs": now_ms(),
            }),
        ));

        let workspace_root = state.workspace_index.snapshot().await.root;
        let mut session = Session::new(
            Some(format!("Routine {}", run.routine_id)),
            Some(workspace_root.clone()),
        );
        let session_id = session.id.clone();
        session.workspace_root = Some(workspace_root);

        if let Err(error) = state.storage.save_session(session).await {
            let detail = format!("failed to create routine session: {error}");
            let _ = state
                .update_routine_run_status(
                    &run.run_id,
                    RoutineRunStatus::Failed,
                    Some(detail.clone()),
                )
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.run.failed",
                serde_json::json!({
                    "runID": run.run_id,
                    "routineID": run.routine_id,
                    "reason": detail,
                }),
            ));
            continue;
        }

        state
            .set_routine_session_policy(
                session_id.clone(),
                run.run_id.clone(),
                run.routine_id.clone(),
                run.allowed_tools.clone(),
            )
            .await;
        state
            .add_active_session_id(&run.run_id, session_id.clone())
            .await;
        state
            .engine_loop
            .set_session_allowed_tools(&session_id, run.allowed_tools.clone())
            .await;
        state
            .engine_loop
            .set_session_auto_approve_permissions(&session_id, true)
            .await;

        let (selected_model, model_source) =
            crate::app::routines::resolve_routine_model_spec_for_run(&state, &run).await;
        if let Some(spec) = selected_model.as_ref() {
            state.event_bus.publish(EngineEvent::new(
                "routine.run.model_selected",
                serde_json::json!({
                    "runID": run.run_id,
                    "routineID": run.routine_id,
                    "providerID": spec.provider_id,
                    "modelID": spec.model_id,
                    "source": model_source,
                }),
            ));
        }

        let request = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: crate::app::routines::build_routine_prompt(&state, &run).await,
            }],
            model: selected_model,
            agent: None,
            tool_mode: None,
            tool_allowlist: None,
            strict_kb_grounding: None,
            context_mode: None,
            write_required: None,
            prewrite_requirements: None,
        };

        let run_result = state
            .engine_loop
            .run_prompt_async_with_context(
                session_id.clone(),
                request,
                Some(format!("routine:{}", run.run_id)),
            )
            .await;

        state.clear_routine_session_policy(&session_id).await;
        state
            .clear_active_session_id(&run.run_id, &session_id)
            .await;
        state
            .engine_loop
            .clear_session_allowed_tools(&session_id)
            .await;
        state
            .engine_loop
            .clear_session_auto_approve_permissions(&session_id)
            .await;

        match run_result {
            Ok(()) => {
                crate::app::routines::append_configured_output_artifacts(&state, &run).await;
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Completed,
                        Some("routine run completed".to_string()),
                    )
                    .await;
                state.event_bus.publish(EngineEvent::new(
                    "routine.run.completed",
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "sessionID": session_id,
                        "finishedAtMs": now_ms(),
                    }),
                ));
            }
            Err(error) => {
                if let Some(latest) = state.get_routine_run(&run.run_id).await {
                    if latest.status == RoutineRunStatus::Paused {
                        state.event_bus.publish(EngineEvent::new(
                            "routine.run.paused",
                            serde_json::json!({
                                "runID": run.run_id,
                                "routineID": run.routine_id,
                                "sessionID": session_id,
                                "finishedAtMs": now_ms(),
                            }),
                        ));
                        continue;
                    }
                }
                let detail = truncate_text(&error.to_string(), 500);
                let _ = state
                    .update_routine_run_status(
                        &run.run_id,
                        RoutineRunStatus::Failed,
                        Some(detail.clone()),
                    )
                    .await;
                state.event_bus.publish(EngineEvent::new(
                    "routine.run.failed",
                    serde_json::json!({
                        "runID": run.run_id,
                        "routineID": run.routine_id,
                        "sessionID": session_id,
                        "reason": detail,
                        "finishedAtMs": now_ms(),
                    }),
                ));
            }
        }
    }
}

pub async fn run_automation_v2_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if state.is_automation_scheduler_stopping() {
            break;
        }
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            continue;
        }
        let now = now_ms();

        // --- Existing: timer-based misfires ---
        let due = state.evaluate_automation_v2_misfires(now).await;
        for automation_id in due {
            let Some(automation) = state.get_automation_v2(&automation_id).await else {
                continue;
            };
            if let Ok(run) = state
                .create_automation_v2_run(&automation, "scheduled")
                .await
            {
                state.event_bus.publish(EngineEvent::new(
                    "automation.v2.run.created",
                    serde_json::json!({
                        "automationID": automation_id,
                        "run": run,
                        "triggerType": "scheduled",
                    }),
                ));
            }
        }

        // --- New (Phase 1): watch-condition-based triggers ---
        let watch_due = state.evaluate_automation_v2_watches().await;
        for (automation_id, trigger_reason, maybe_handoff) in watch_due {
            let Some(automation) = state.get_automation_v2(&automation_id).await else {
                continue;
            };

            // If this watch was triggered by a handoff, consume it before creating
            // the run so no other automation on this tick can claim the same handoff.
            let consumed_handoff_id = if let Some(ref handoff) = maybe_handoff {
                let workspace_root = state.workspace_index.snapshot().await.root;
                let handoff_cfg = automation.effective_handoff_config();
                match state
                    .consume_automation_v2_handoff(
                        &workspace_root,
                        handoff,
                        &handoff_cfg,
                        // Use a placeholder run ID; the real run ID is assigned below.
                        // consume_automation_v2_handoff writes to the archive immediately,
                        // so we pass the handoff_id so the audit trail is useful even
                        // if run creation subsequently fails.
                        &format!("pending-{}", handoff.handoff_id),
                        &automation_id,
                    )
                    .await
                {
                    Ok(Some(_)) => Some(handoff.handoff_id.clone()),
                    Ok(None) => {
                        // Already consumed by a race — skip this trigger.
                        tracing::warn!(
                            automation_id = %automation_id,
                            handoff_id = %handoff.handoff_id,
                            "handoff watch: skipping — handoff already consumed (race)"
                        );
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(
                            automation_id = %automation_id,
                            handoff_id = %handoff.handoff_id,
                            "handoff watch: failed to consume handoff: {err}"
                        );
                        continue;
                    }
                }
            } else {
                None
            };

            match state
                .create_automation_v2_watch_run(
                    &automation,
                    trigger_reason.clone(),
                    consumed_handoff_id,
                )
                .await
            {
                Ok(run) => {
                    state.event_bus.publish(EngineEvent::new(
                        "automation.v2.run.created",
                        serde_json::json!({
                            "automationID": automation_id,
                            "run": run,
                            "triggerType": "watch_condition",
                            "triggerReason": trigger_reason,
                        }),
                    ));
                }
                Err(err) => {
                    tracing::warn!(
                        automation_id = %automation_id,
                        "watch condition run creation failed: {err}"
                    );
                }
            }
        }
    }
}

pub async fn run_optimization_scheduler(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            continue;
        }
        if let Err(error) = state.reconcile_optimization_campaigns().await {
            tracing::warn!("optimization scheduler reconciliation failed: {error}");
        }
    }
}

#[cfg(test)]
mod bug_monitor_candidate_tests {
    use super::*;

    #[test]
    fn bug_monitor_candidate_detection_includes_terminal_failures() {
        for event_type in [
            "context.task.failed",
            "context.task.blocked",
            "context.run.failed",
            "workflow.run.failed",
            "workflow.validation.failed",
            "routine.run.failed",
            "session.error",
            "automation.run.failed",
            "automation_v2.run.failed",
            "coder.run.failed",
        ] {
            assert!(
                is_bug_monitor_candidate_event(&EngineEvent::new(
                    event_type,
                    serde_json::json!({})
                )),
                "{event_type} should be monitored"
            );
        }
    }

    #[test]
    fn bug_monitor_candidate_detection_ignores_progress_and_monitor_events() {
        for event_type in [
            "context.task.started",
            "context.task.requeued",
            "workflow.action.completed",
            "automation_v2.run.started",
            "routine.run.completed",
            "bug_monitor.incident.detected",
        ] {
            assert!(
                !is_bug_monitor_candidate_event(&EngineEvent::new(
                    event_type,
                    serde_json::json!({})
                )),
                "{event_type} should not be monitored"
            );
        }
    }
}
