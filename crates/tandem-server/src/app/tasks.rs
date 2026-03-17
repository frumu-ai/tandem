use std::time::Duration;

use serde_json::Value;
use tandem_types::{
    EngineEvent, MessagePart, MessagePartInput, MessageRole, ModelSpec, SendMessageRequest, Session,
};

use crate::app::state::{
    derive_status_index_update, extract_persistable_tool_part, sha256_hex, truncate_text, AppState,
};
use crate::bug_monitor::types::{BugMonitorConfig, BugMonitorIncidentRecord, BugMonitorSubmission};
use crate::http::context_runs::{
    append_context_run_event, ensure_session_context_run, session_run_status_to_context,
};
use crate::http::context_types::{ContextRunEventAppendInput, ContextRunStatus};
use crate::routines::types::{RoutineHistoryEvent, RoutineRunRecord, RoutineRunStatus};
use crate::util::time::now_ms;

fn extract_event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .or_else(|| properties.get("sessionId"))
        .or_else(|| properties.get("id"))
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
            if !matches!(part_type, "tool" | "tool-invocation" | "tool-result") {
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
                    "why_next_step": format!("session run finished with status `{status}`"),
                    "step_status": if matches!(status, "completed") { "done" } else if matches!(status, "cancelled") { "blocked" } else { "failed" },
                }),
            })
        }
        _ => None,
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
        "context.task.failed" | "workflow.run.failed" | "routine.run.failed" | "session.error"
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
                let session_id = event
                    .properties
                    .get("sessionID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if session_id.is_empty() {
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
        let startup = state.startup_snapshot().await;
        if !matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            continue;
        }
        let now = now_ms();
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
    }
}
