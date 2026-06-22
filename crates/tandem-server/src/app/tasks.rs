use std::{collections::HashMap, time::Duration};

use serde_json::Value;
use tandem_types::{EngineEvent, MessagePartInput, SendMessageRequest, Session, TenantContext};

use crate::app::state::{
    derive_status_index_update, extract_persistable_tool_part, truncate_text, AppState,
};
use crate::bug_monitor::types::{BugMonitorConfig, BugMonitorIncidentRecord};
use crate::http::context_runs::{
    append_context_run_event, ensure_session_context_run, session_run_status_to_context,
};
use crate::http::context_types::{ContextRunEventAppendInput, ContextRunStatus};
use crate::routines::types::{RoutineHistoryEvent, RoutineRunStatus};
use crate::runtime_event_log::{
    append_runtime_event_log_row, prune_runtime_event_log, RuntimeEventLogRow,
};
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

const CONTEXT_TOOL_EVENT_STRING_LIMIT: usize = 2_000;

#[derive(Debug, Default)]
struct RuntimeEventLogContextCache {
    sessions: HashMap<String, RuntimeEventLogSessionContext>,
}

#[derive(Debug, Clone, Default)]
struct RuntimeEventLogSessionContext {
    run_id: Option<String>,
    tenant_context: Option<TenantContext>,
}

fn is_running_tool_args_delta(properties: &Value) -> bool {
    let Some(part) = properties.get("part") else {
        return false;
    };
    let part_type = part
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if !matches!(
        part_type,
        "tool" | "tool-invocation" | "tool-result" | "tool_invocation" | "tool_result"
    ) {
        return false;
    }
    let tool_state = part
        .get("state")
        .and_then(|value| value.as_str())
        .unwrap_or("running");
    if matches!(tool_state, "completed" | "failed") {
        return false;
    }
    properties
        .get("toolCallDelta")
        .and_then(|delta| delta.get("argsDelta").or_else(|| delta.get("args_delta")))
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty())
}

fn compact_large_context_event_strings(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > CONTEXT_TOOL_EVENT_STRING_LIMIT => {
            *text = truncate_text(text, CONTEXT_TOOL_EVENT_STRING_LIMIT);
        }
        Value::Array(items) => {
            for item in items {
                compact_large_context_event_strings(item);
            }
        }
        Value::Object(map) => {
            for child in map.values_mut() {
                compact_large_context_event_strings(child);
            }
        }
        _ => {}
    }
}

fn compact_context_tool_value(value: Option<&Value>) -> Value {
    let mut compacted = value.cloned().unwrap_or(Value::Null);
    compact_large_context_event_strings(&mut compacted);
    compacted
}

async fn enrich_runtime_event_log_row(
    state: &AppState,
    row: RuntimeEventLogRow,
    context_cache: &mut RuntimeEventLogContextCache,
) -> RuntimeEventLogRow {
    let Some(session_id) = row.session_id().map(str::to_string) else {
        return row;
    };

    if row.run_id().is_none() {
        if let Some(active_run) = state.run_registry.get(&session_id).await {
            context_cache
                .sessions
                .entry(session_id.clone())
                .or_default()
                .run_id = Some(active_run.run_id);
        }
    }

    let session = if row.tenant_context().is_none() {
        state.storage.get_session(&session_id).await
    } else {
        None
    };

    enrich_runtime_event_log_row_from_session(row, session.as_ref(), context_cache)
}

fn enrich_runtime_event_log_row_from_session(
    mut row: RuntimeEventLogRow,
    session: Option<&Session>,
    context_cache: &mut RuntimeEventLogContextCache,
) -> RuntimeEventLogRow {
    let Some(session_id) = row.session_id().map(str::to_string) else {
        return row;
    };

    let run_id = row.run_id().map(str::to_string);
    let tenant_context = row
        .tenant_context()
        .cloned()
        .or_else(|| session.map(|session| session.tenant_context.clone()));

    if run_id.is_some() || tenant_context.is_some() {
        let context = context_cache
            .sessions
            .entry(session_id.clone())
            .or_default();
        if let Some(run_id) = run_id {
            context.run_id = Some(run_id);
        }
        if let Some(tenant_context) = tenant_context {
            context.tenant_context = Some(tenant_context);
        }
    }

    if let Some(context) = context_cache.sessions.get(&session_id) {
        if row.event.envelope.run_id.is_none() {
            row.event.envelope.run_id = context.run_id.clone();
        }
        if row.event.envelope.tenant_context.is_none() {
            row.event.envelope.tenant_context = context.tenant_context.clone();
        }
    }

    row
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
            if is_running_tool_args_delta(&event.properties) {
                return None;
            }
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
                    "part": compact_context_tool_value(Some(part)),
                    "toolCallDelta": compact_context_tool_value(event.properties.get("toolCallDelta")),
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
        "policy.decision.recorded" => {
            let record = event.properties.get("record")?;
            let decision = record
                .get("decision")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let tool = record
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("runtime");
            Some(ContextRunEventAppendInput {
                event_type: "policy_decision_recorded".to_string(),
                status: if decision == "allow" {
                    ContextRunStatus::Running
                } else {
                    ContextRunStatus::Blocked
                },
                step_id: Some("policy-decision".to_string()),
                payload: serde_json::json!({
                    "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                    "messageID": event.properties.get("messageID").cloned().unwrap_or(Value::Null),
                    "runID": event.properties.get("runID").cloned().unwrap_or(Value::Null),
                    "automationID": event.properties.get("automationID").cloned().unwrap_or(Value::Null),
                    "tool": event.properties.get("tool").cloned().unwrap_or(Value::Null),
                    "decisionID": event.properties.get("decisionID").cloned().unwrap_or(Value::Null),
                    "record": record.clone(),
                    "tenantContext": event_tenant_context_value(event),
                    "why_next_step": format!("policy decision `{decision}` recorded for `{tool}`"),
                    "step_status": if decision == "allow" {
                        "in_progress"
                    } else {
                        "blocked"
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

    fn runtime_event(event_type: &str, properties: Value, seq: u64) -> EngineEvent {
        let envelope = tandem_types::RuntimeEventEnvelope::derive(seq, 1_000 + seq, &properties);
        EngineEvent::new(event_type, properties).with_envelope(envelope)
    }

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
    fn session_context_run_event_input_skips_running_tool_arg_deltas() {
        let input = session_context_run_event_input(&EngineEvent::new(
            "message.part.updated",
            serde_json::json!({
                "sessionID": "session-1",
                "runID": "run-1",
                "part": {
                    "type": "tool",
                    "tool": "write",
                    "state": "running",
                    "args": {"content": "{\"partial\":"}
                },
                "toolCallDelta": {
                    "argsDelta": "large streamed write body chunk"
                }
            }),
        ));

        assert!(input.is_none());
    }

    #[test]
    fn session_context_run_event_input_compacts_completed_tool_payloads() {
        let oversized_content = "x".repeat(CONTEXT_TOOL_EVENT_STRING_LIMIT + 200);
        let input = session_context_run_event_input(&EngineEvent::new(
            "message.part.updated",
            serde_json::json!({
                "sessionID": "session-1",
                "runID": "run-1",
                "part": {
                    "type": "tool",
                    "tool": "write",
                    "state": "completed",
                    "args": {"content": oversized_content}
                },
                "toolCallDelta": Value::Null
            }),
        ))
        .expect("completed tool update should still be journaled");

        let compacted = input
            .payload
            .get("part")
            .and_then(|part| part.get("args"))
            .and_then(|args| args.get("content"))
            .and_then(Value::as_str)
            .expect("compacted content");
        assert!(compacted.len() < CONTEXT_TOOL_EVENT_STRING_LIMIT + 100);
        assert!(compacted.ends_with("...<truncated>"));
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

    #[tokio::test]
    async fn runtime_event_log_enrichment_maps_session_only_events_to_run_and_tenant() {
        let path = std::env::temp_dir().join(format!(
            "runtime-events-persister-enrichment-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let mut session = Session::new(Some("Session A".to_string()), Some(".".to_string()));
        session.id = "session-a".to_string();
        session.tenant_context = tenant_context.clone();
        let mut context_cache = RuntimeEventLogContextCache::default();

        let started = runtime_event(
            "session.run.started",
            serde_json::json!({
                "sessionID": "session-a",
                "runID": "run-a",
                "tenantContext": tenant_context.clone(),
            }),
            1,
        );
        let started_row = RuntimeEventLogRow::from_engine_event(&started).expect("started row");
        let started_row = enrich_runtime_event_log_row_from_session(
            started_row,
            Some(&session),
            &mut context_cache,
        );
        append_runtime_event_log_row(&path, &started_row)
            .await
            .expect("append started");

        let provider_iteration = runtime_event(
            "provider.call.iteration.start",
            serde_json::json!({
                "sessionID": "session-a",
                "provider": "openai",
            }),
            2,
        );
        let provider_row =
            RuntimeEventLogRow::from_engine_event(&provider_iteration).expect("provider row");
        assert_eq!(provider_row.run_id(), None);
        assert_eq!(provider_row.tenant_context(), None);

        let provider_row = enrich_runtime_event_log_row_from_session(
            provider_row,
            Some(&session),
            &mut context_cache,
        );
        append_runtime_event_log_row(&path, &provider_row)
            .await
            .expect("append provider");

        let rows = crate::runtime_event_log::query_runtime_event_log(
            &path,
            &session.tenant_context,
            crate::runtime_event_log::RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: Some(1),
                limit: None,
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq(), 2);
        assert_eq!(rows[0].run_id(), Some("run-a"));
        assert_eq!(
            rows[0]
                .tenant_context()
                .map(|tenant| tenant.org_id.as_str()),
            Some("org-a")
        );
        assert_eq!(
            rows[0].event.event_type.as_str(),
            "provider.call.iteration.start"
        );

        let _ = tokio::fs::remove_file(path).await;
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

pub async fn run_runtime_event_log_persister(state: AppState) {
    let Some(mut rx) = state.event_bus.register_runtime_event_log_receiver() else {
        tracing::warn!("runtime event log persister: skipped because queue was already registered");
        return;
    };

    if !wait_for_runtime_ready_or_exit(&state, "runtime_event_log_persister").await {
        return;
    }

    let retention_days = std::env::var("TANDEM_RUNTIME_EVENT_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(30);
    if retention_days > 0 {
        let retention_ms = retention_days.saturating_mul(24 * 60 * 60 * 1_000);
        match prune_runtime_event_log(&state.runtime_events_path, retention_ms, now_ms()).await {
            Ok(pruned) if pruned > 0 => {
                tracing::info!(
                    pruned,
                    retention_days,
                    "runtime event log persister pruned stale events"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "runtime event log persister could not prune stale events"
                );
            }
        }
    }

    let mut context_cache = RuntimeEventLogContextCache::default();
    while let Some(event) = rx.recv().await {
        tandem_observability::record_engine_event_metrics(&event.event_type, &event.properties);
        let Some(row) = RuntimeEventLogRow::from_engine_event(&event) else {
            continue;
        };
        let row = enrich_runtime_event_log_row(&state, row, &mut context_cache).await;
        if let Err(error) = append_runtime_event_log_row(&state.runtime_events_path, &row).await {
            tracing::warn!(
                error = %error,
                event_id = row.event_id(),
                seq = row.seq(),
                "runtime event log persister failed to append event"
            );
        }
    }
}

#[cfg(test)]
mod runtime_event_log_persister_tests {
    use serde_json::json;
    use tandem_types::EngineEvent;

    use super::*;

    async fn wait_for_persisted_event(
        path: &std::path::Path,
        tenant: &TenantContext,
        run_id: &str,
    ) -> Vec<RuntimeEventLogRow> {
        for _ in 0..50 {
            let rows = crate::runtime_event_log::query_runtime_event_log(
                path,
                tenant,
                crate::runtime_event_log::RuntimeEventLogQuery {
                    run_id,
                    after_seq: None,
                    limit: None,
                },
            );
            if !rows.is_empty() {
                return rows;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Vec::new()
    }

    #[tokio::test]
    async fn persister_flushes_events_published_after_queue_registration() {
        let mut state = crate::test_support::test_state().await;
        state.runtime_events_path = std::env::temp_dir().join(format!(
            "runtime-events-prestart-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");

        let persister = tokio::spawn(run_runtime_event_log_persister(state.clone()));
        for _ in 0..50 {
            if state.event_bus.runtime_event_log_queue_is_registered() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            state.event_bus.runtime_event_log_queue_is_registered(),
            "persister should register its queue before consuming runtime events"
        );

        state.event_bus.publish(EngineEvent::new(
            "session.run.started",
            json!({
                "sessionID": "session-a",
                "runID": "run-a",
                "tenantContext": tenant.clone(),
            }),
        ));

        let rows = wait_for_persisted_event(&state.runtime_events_path, &tenant, "run-a").await;

        persister.abort();
        let _ = tokio::fs::remove_file(&state.runtime_events_path).await;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id(), Some("run-a"));
        assert_eq!(rows[0].session_id(), Some("session-a"));
        assert_eq!(rows[0].event.event_type.as_str(), "session.run.started");
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
    if is_automation_v2_context_mirror_failure(event) {
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
            | "automation_v2.run.paused_stale_no_provider_activity"
            | "coder.run.failed"
    )
}

fn event_string_property<'a>(event: &'a EngineEvent, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| event.properties.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_automation_v2_context_mirror_failure(event: &EngineEvent) -> bool {
    if !matches!(
        event.event_type.as_str(),
        "context.task.failed" | "context.task.blocked" | "context.run.failed"
    ) {
        return false;
    }

    if event_string_property(event, &["source"]).is_some_and(|source| source == "automation_v2") {
        return true;
    }
    if event_string_property(event, &["automation_id", "automationID"]).is_some() {
        return true;
    }
    event_string_property(event, &["run_id", "runID"]).is_some_and(|run_id| {
        run_id.starts_with("automation-v2-") || run_id.starts_with("automation_v2-")
    })
}

pub async fn run_bug_monitor(state: AppState) {
    let mut wait_ms = 250u64;
    loop {
        let startup = state.startup_snapshot().await;
        if matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
            break;
        }
        if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
            tracing::warn!(
                startup_status = ?startup.status,
                startup_phase = %startup.phase,
                attempt_id = %startup.attempt_id,
                "bug monitor: exiting because startup failed before monitoring began"
            );
            return;
        }

        state
            .update_bug_monitor_runtime_status(|runtime| {
                runtime.monitoring_active = false;
                runtime.last_runtime_error =
                    Some("Waiting for runtime readiness before starting bug monitor".to_string());
            })
            .await;

        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        wait_ms = (wait_ms * 2).min(2_000);
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

/// Periodic deadline sweep for Bug Monitor triage runs. Without this
/// loop, `recover_overdue_bug_monitor_triage_runs` only fires when
/// something polls `bug_monitor_status` (the status panel, the
/// dashboard, an API caller). On a quiet engine — UI closed, no
/// outside pollers — an overdue triage just sits there: issue #60
/// ran 3.95 hours past its 30-minute deadline before anything noticed.
/// A 30-second tick guarantees the timeout fires within ~30s of the
/// deadline regardless of UI state.
///
/// Concurrency note: `try_mark_triage_timed_out` (called inside
/// `recover_overdue_bug_monitor_triage_runs`) is an atomic CAS, so
/// running this loop alongside an on-demand `bug_monitor_status` call
/// can't double-publish — only one caller wins the status flip per
/// draft.
pub async fn run_bug_monitor_recovery_sweep(state: AppState) {
    if !wait_for_runtime_ready_or_exit(&state, "run_bug_monitor_recovery_sweep").await {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let status = state.bug_monitor_status_snapshot().await;
        if !status.config.enabled || status.config.paused {
            continue;
        }
        let recovered = match crate::bug_monitor::service::recover_overdue_bug_monitor_triage_runs(
            &state,
        )
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "bug monitor recovery sweep: recover_overdue failed"
                );
                continue;
            }
        };
        for (draft_id, incident_id) in recovered {
            if let Err(error) = crate::bug_monitor_github::publish_draft(
                &state,
                &draft_id,
                incident_id.as_deref(),
                crate::bug_monitor_github::PublishMode::Recovery,
            )
            .await
            {
                tracing::warn!(
                    draft_id = %draft_id,
                    error = %error,
                    "bug monitor recovery sweep: publish_draft failed"
                );
            }
        }
    }
}

pub async fn run_usage_aggregator(state: AppState) {
    if crate::benchmarking::benchmark_config_from_env().profiling_enabled {
        tokio::spawn(crate::benchmarking::run_benchmark_profiler(state.clone()));
    }
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
            sampling: Default::default(),
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
        let tick_started = std::time::Instant::now();
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
                let tenant_context = run.tenant_context.clone();
                state.event_bus.publish(EngineEvent::new(
                    "automation.v2.run.created",
                    serde_json::json!({
                        "automationID": automation_id,
                        "run": run,
                        "tenantContext": tenant_context,
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
                    let tenant_context = run.tenant_context.clone();
                    state.event_bus.publish(EngineEvent::new(
                        "automation.v2.run.created",
                        serde_json::json!({
                            "automationID": automation_id,
                            "run": run,
                            "tenantContext": tenant_context,
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
        tandem_observability::record_scheduler_tick_latency_ms(
            tick_started.elapsed().as_millis() as u64
        );
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
            "automation_v2.run.paused_stale_no_provider_activity",
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

    #[test]
    fn bug_monitor_candidate_detection_ignores_automation_v2_context_mirror_failures() {
        for event in [
            EngineEvent::new(
                "context.task.failed",
                serde_json::json!({
                    "source": "automation_v2",
                    "automation_id": "automation-v2-123",
                    "run_id": "automation-v2-run-123",
                    "task_id": "node-downstream",
                }),
            ),
            EngineEvent::new(
                "context.task.blocked",
                serde_json::json!({
                    "automationID": "automation-v2-123",
                    "runID": "automation-v2-run-123",
                    "taskID": "node-downstream",
                }),
            ),
            EngineEvent::new(
                "context.run.failed",
                serde_json::json!({
                    "runID": "automation-v2-automation-v2-run-123",
                }),
            ),
        ] {
            assert!(
                !is_bug_monitor_candidate_event(&event),
                "{} from automation v2 context mirror should be grouped under automation_v2.run.failed",
                event.event_type
            );
        }
    }

    #[test]
    fn bug_monitor_candidate_detection_keeps_standalone_context_failures() {
        assert!(is_bug_monitor_candidate_event(&EngineEvent::new(
            "context.task.failed",
            serde_json::json!({
                "source": "context_run",
                "run_id": "context-run-123",
                "task_id": "inspect_failure",
            }),
        )));
    }
}
