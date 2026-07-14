use super::*;

pub(super) fn session_context_run_event_input(
    event: &EngineEvent,
) -> Option<ContextRunEventAppendInput> {
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
                    "failureCategory": event.properties.get("failureCategory").cloned().unwrap_or(Value::Null),
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
        "tool.routing.decision" => Some(ContextRunEventAppendInput {
            event_type: "tool_routing_decision".to_string(),
            status: ContextRunStatus::Running,
            step_id: Some("session-run".to_string()),
            payload: serde_json::json!({
                "sessionID": event.properties.get("sessionID").cloned().unwrap_or(Value::Null),
                "messageID": event.properties.get("messageID").cloned().unwrap_or(Value::Null),
                "iteration": event.properties.get("iteration").cloned().unwrap_or(Value::Null),
                "mode": event.properties.get("mode").cloned().unwrap_or(Value::Null),
                "intent": event.properties.get("intent").cloned().unwrap_or(Value::Null),
                "selectedToolCount": event.properties.get("selectedToolCount").cloned().unwrap_or(Value::Null),
                "totalAvailableTools": event.properties.get("totalAvailableTools").cloned().unwrap_or(Value::Null),
                "offeredTools": event.properties.get("offeredTools").cloned().unwrap_or(Value::Array(Vec::new())),
                "hiddenByScope": event.properties.get("hiddenByScope").cloned().unwrap_or(Value::Array(Vec::new())),
                "strictProjectionActive": event.properties.get("strictProjectionActive").cloned().unwrap_or(Value::Bool(false)),
                "scopeAllowlist": event.properties.get("scopeAllowlist").cloned().unwrap_or(Value::Array(Vec::new())),
                "tenantContext": event_tenant_context_value(event),
                "why_next_step": "tool routing manifest recorded",
                "step_status": "in_progress",
            }),
        }),
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
