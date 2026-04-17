fn normalize_workspace_path(path: &PathBuf) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let normalized = if absolute.exists() {
        absolute.canonicalize().ok()?
    } else {
        absolute
    };
    Some(normalized.to_string_lossy().to_string())
}

fn default_tui_permission_rules() -> Vec<serde_json::Value> {
    tandem_core::default_tui_permission_rules()
        .into_iter()
        .map(|rule| {
            serde_json::json!({
                "permission": rule.permission,
                "pattern": rule.pattern,
                "action": rule.action
            })
        })
        .collect()
}

fn parse_sse_payload(buffer: &mut String) -> Option<serde_json::Value> {
    let (end_idx, delim_len) = if let Some(i) = buffer.find("\r\n\r\n") {
        (i, 4)
    } else if let Some(i) = buffer.find("\n\n") {
        (i, 2)
    } else {
        return None;
    };

    let event_str = buffer[..end_idx].to_string();
    *buffer = buffer[end_idx + delim_len..].to_string();

    let mut data_lines: Vec<String> = Vec::new();
    for raw_line in event_str.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&data).ok()
}

fn parse_stream_event_envelope(payload: serde_json::Value) -> Option<StreamEventEnvelope> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?.to_string();
    let props = payload
        .get("properties")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(StreamEventEnvelope {
        event_type,
        session_id: props
            .get("sessionID")
            .or_else(|| props.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        run_id: props
            .get("runID")
            .or_else(|| props.get("run_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        agent_id: props
            .get("agentID")
            .or_else(|| props.get("agent"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        channel: props
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        payload,
    })
}

pub fn extract_delta_text(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "message.part.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    if let Some(delta) = props.get("delta") {
        let extracted = match delta {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(map) => map
                .get("text")
                .or_else(|| map.get("delta").and_then(|d| d.get("text")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            serde_json::Value::Array(items) => {
                let text = items
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(map) => map
                            .get("text")
                            .or_else(|| map.get("delta").and_then(|d| d.get("text")))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        };
        if extracted.is_some() {
            return extracted;
        }
    }
    // Some runtime snapshots only include the final text payload without explicit delta.
    let from_part_text = props
        .get("part")
        .and_then(|p| p.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());
    if from_part_text.is_some() {
        return from_part_text;
    }

    // Some providers emit content arrays with typed text chunks.
    props
        .get("part")
        .and_then(|p| p.get("content"))
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Object(map) => map
                        .get("text")
                        .or_else(|| map.get("value"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|s| !s.trim().is_empty())
}

pub fn extract_stream_activity(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    match event_type {
        "permission.asked" => {
            let tool = props.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Waiting for permission: `{}` (request `{}`)",
                tool, request_id
            ))
        }
        "permission.replied" => {
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let reply = props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Permission `{}` replied with `{}`.",
                request_id, reply
            ))
        }
        "question.asked" => Some("Agent is waiting for your input.".to_string()),
        "message.part.updated" => {
            let Some(part) = props.get("part") else {
                return None;
            };
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type != "tool" {
                return None;
            }
            let tool = part
                .get("tool")
                .or_else(|| part.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            let status = part
                .get("state")
                .and_then(|s| s.get("status"))
                .or_else(|| part.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match status {
                "running" => Some(format!("Running tool `{}`...", tool)),
                "pending" => Some(format!("Tool `{}` pending...", tool)),
                "completed" | "done" => Some(format!("Tool `{}` completed.", tool)),
                "failed" | "error" => Some(format!("Tool `{}` failed.", tool)),
                "cancelled" | "canceled" => Some(format!("Tool `{}` cancelled.", tool)),
                _ => Some(format!("Tool `{}` update.", tool)),
            }
        }
        _ => None,
    }
}

pub fn extract_stream_tool_delta(payload: &serde_json::Value) -> Option<StreamToolDelta> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "message.part.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    let tool_delta = props.get("toolCallDelta")?;
    let tool_call_id = tool_delta.get("id").and_then(|v| v.as_str())?.to_string();
    let tool_name = tool_delta
        .get("tool")
        .or_else(|| tool_delta.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let args_delta = tool_delta
        .get("argsDelta")
        .or_else(|| tool_delta.get("delta"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let args_preview = tool_delta
        .get("parsedArgsPreview")
        .or_else(|| tool_delta.get("argsPreview"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_else(|| args_delta.clone());
    Some(StreamToolDelta {
        tool_call_id,
        tool_name,
        args_delta,
        args_preview,
    })
}

pub fn extract_stream_request(payload: &serde_json::Value) -> Option<StreamRequestEvent> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?.clone();

    match event_type {
        "permission.asked" => {
            let request = serde_json::from_value::<PermissionRequest>(serde_json::json!({
                "id": props
                    .get("requestID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "status": "pending",
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
                "args": props.get("args").cloned().unwrap_or(serde_json::Value::Null),
                "argsSource": props
                    .get("argsSource")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "argsIntegrity": props
                    .get("argsIntegrity")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "query": props.get("query").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::PermissionAsked(request))
        }
        "permission.replied" => Some(StreamRequestEvent::PermissionReplied {
            request_id: props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            reply: props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "question.asked" => {
            let mut questions_value = props
                .get("questions")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let has_questions = questions_value
                .as_array()
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            if !has_questions {
                let fallback_question = props
                    .get("question")
                    .and_then(|v| v.as_str())
                    .or_else(|| props.get("prompt").and_then(|v| v.as_str()))
                    .or_else(|| props.get("query").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(question) = fallback_question {
                    let options = props
                        .get("choices")
                        .or_else(|| props.get("options"))
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|entry| {
                            if let Some(label) = entry.as_str() {
                                serde_json::json!({ "label": label, "description": "" })
                            } else if entry.is_object() {
                                entry
                            } else {
                                serde_json::json!({ "label": entry.to_string(), "description": "" })
                            }
                        })
                        .collect::<Vec<_>>();
                    questions_value = serde_json::json!([{
                        "header": "Question",
                        "question": question,
                        "options": options,
                        "multiple": false,
                        "custom": true
                    }]);
                }
            }
            let request = serde_json::from_value::<QuestionRequest>(serde_json::json!({
                "id": props.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "questions": questions_value,
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::QuestionAsked(request))
        }
        _ => None,
    }
}

pub fn extract_stream_agent_team_event(
    payload: &serde_json::Value,
) -> Option<StreamAgentTeamEvent> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if !event_type.starts_with("agent_team.") {
        return None;
    }
    let properties = payload.get("properties")?;
    Some(StreamAgentTeamEvent {
        event_type: event_type.to_string(),
        team_name: properties
            .get("teamName")
            .or_else(|| properties.get("team_name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        recipient: properties
            .get("recipient")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message_type: properties
            .get("messageType")
            .or_else(|| properties.get("message_type"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        request_id: properties
            .get("requestId")
            .or_else(|| properties.get("request_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message_id: properties
            .get("messageID")
            .or_else(|| properties.get("message_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

pub fn extract_stream_todo_update(
    payload: &serde_json::Value,
) -> Option<(String, Vec<serde_json::Value>)> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "todo.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    let session_id = props
        .get("sessionID")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let todos = props
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Some((session_id, todos))
}

pub fn extract_stream_error(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    if event_type == "session.error" {
        if let Some(message) = props
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
        {
            let code = props
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("ENGINE_ERROR");
            return Some(format!("{}: {}", code, message));
        }
        return Some("Engine reported an error.".to_string());
    }

    if event_type == "session.run.finished" {
        let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "completed" {
            let reason = props
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("run did not complete");
            return Some(format!("Run {}: {}", status, reason));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_single_response_server(
        expected_path: &'static str,
        response_status: &'static str,
        response_body: &'static str,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            assert!(
                first_line.contains(expected_path),
                "expected path {}, got {}",
                expected_path,
                first_line
            );
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_status,
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write_all");
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn cancel_run_by_id_posts_expected_endpoint() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_42/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":true}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_42")
            .await
            .expect("cancel");
        assert!(cancelled);
    }

    #[tokio::test]
    async fn cancel_run_by_id_returns_false_for_non_active_run() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_missing/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":false}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_missing")
            .await
            .expect("cancel");
        assert!(!cancelled);
    }

    #[tokio::test]
    async fn mission_list_reads_engine_missions_endpoint() {
        let base = spawn_single_response_server(
            "/mission",
            "200 OK",
            r#"{"missions":[{"mission_id":"m1","status":"draft","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let missions = client.mission_list().await.expect("mission_list");
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].mission_id, "m1");
    }

    #[tokio::test]
    async fn mission_get_reads_engine_mission_endpoint() {
        let base = spawn_single_response_server(
            "/mission/m1",
            "200 OK",
            r#"{"mission":{"mission_id":"m1","status":"draft","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let mission = client.mission_get("m1").await.expect("mission_get");
        assert_eq!(mission.mission_id, "m1");
        assert_eq!(mission.spec.title, "Demo");
    }

    #[tokio::test]
    async fn mission_create_posts_to_engine_mission_endpoint() {
        let base = spawn_single_response_server(
            "/mission",
            "200 OK",
            r#"{"mission":{"mission_id":"m2","status":"draft","spec":{"mission_id":"m2","title":"Create","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let mission = client
            .mission_create(MissionCreateRequest {
                title: "Create".to_string(),
                goal: "Test".to_string(),
                work_items: vec![],
            })
            .await
            .expect("mission_create");
        assert_eq!(mission.mission_id, "m2");
    }

    #[tokio::test]
    async fn mission_apply_event_posts_event_payload() {
        let base = spawn_single_response_server(
            "/mission/m1/event",
            "200 OK",
            r#"{"mission":{"mission_id":"m1","status":"running","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":1,"updated_at_ms":2},"commands":[{"type":"emit_notice"}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let result = client
            .mission_apply_event(
                "m1",
                serde_json::json!({
                    "type": "mission_started",
                    "mission_id": "m1"
                }),
            )
            .await
            .expect("mission_apply_event");
        assert_eq!(result.mission.revision, 1);
        assert_eq!(result.commands.len(), 1);
    }

    #[tokio::test]
    async fn context_runs_list_reads_engine_context_runs_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs",
            "200 OK",
            r#"{"runs":[{"run_id":"ctx-1","run_type":"interactive","status":"running","objective":"Ship context-driving","workspace":{"workspace_id":"ws1","canonical_path":"/tmp/ws","lease_epoch":1},"steps":[{"step_id":"s1","title":"Plan","status":"in_progress"}],"why_next_step":"Need plan before execution","revision":3,"created_at_ms":1,"updated_at_ms":2}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let runs = client.context_runs_list().await.expect("context_runs_list");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "ctx-1");
        assert_eq!(runs[0].status, ContextRunStatus::Running);
    }

    #[tokio::test]
    async fn context_run_get_reads_engine_context_run_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-2",
            "200 OK",
            r#"{"run":{"run_id":"ctx-2","run_type":"cron","status":"paused","objective":"Nightly pipeline","workspace":{"workspace_id":"ws2","canonical_path":"/tmp/cron","lease_epoch":2},"steps":[],"why_next_step":null,"revision":7,"created_at_ms":3,"updated_at_ms":4},"rollback_preview_summary":{"step_count":1},"rollback_history_summary":{"entry_count":2},"last_rollback_outcome":{"outcome":"blocked"},"rollback_policy":{"eligible":true}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let detail = client
            .context_run_get("ctx-2")
            .await
            .expect("context_run_get");
        assert_eq!(detail.run.run_id, "ctx-2");
        assert_eq!(detail.run.run_type, "cron");
        assert_eq!(detail.run.status, ContextRunStatus::Paused);
        assert_eq!(detail.rollback_policy["eligible"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn context_run_rollback_history_reads_engine_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-rollback/checkpoints/mutations/rollback-history",
            "200 OK",
            r#"{"entries":[{"seq":14,"ts_ms":1234,"event_id":"evt-rollback","outcome":"applied","selected_event_ids":["evt-1"],"applied_step_count":1,"applied_operation_count":2,"applied_by_action":{"rewrite_file":2}}],"summary":{"entry_count":1,"by_outcome":{"applied":1}}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let history = client
            .context_run_rollback_history("ctx-rollback")
            .await
            .expect("context_run_rollback_history");
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].outcome, "applied");
        assert_eq!(
            history.entries[0]
                .applied_by_action
                .as_ref()
                .and_then(|counts| counts.get("rewrite_file"))
                .copied(),
            Some(2)
        );
    }

    #[tokio::test]
    async fn context_run_rollback_preview_reads_engine_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-preview/checkpoints/mutations/rollback-preview",
            "200 OK",
            r#"{"steps":[{"seq":9,"event_id":"evt-preview","tool":"write_file","executable":true,"operation_count":3}],"step_count":1,"executable_step_count":1,"advisory_step_count":0,"executable":true}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let preview = client
            .context_run_rollback_preview("ctx-preview")
            .await
            .expect("context_run_rollback_preview");
        assert_eq!(preview.step_count, 1);
        assert_eq!(preview.steps[0].event_id, "evt-preview");
        assert!(preview.steps[0].executable);
        assert_eq!(preview.steps[0].operation_count, 3);
    }

    #[tokio::test]
    async fn context_run_rollback_execute_posts_engine_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-exec/checkpoints/mutations/rollback-execute",
            "200 OK",
            r#"{"applied":true,"selected_event_ids":["evt-preview"],"applied_step_count":1,"applied_operation_count":3}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let result = client
            .context_run_rollback_execute(
                "ctx-exec",
                vec!["evt-preview".to_string()],
                Some("allow_rollback_execution".to_string()),
            )
            .await
            .expect("context_run_rollback_execute");
        assert!(result.applied);
        assert_eq!(result.selected_event_ids, vec!["evt-preview".to_string()]);
        assert_eq!(result.applied_operation_count, Some(3));
    }

    #[tokio::test]
    async fn context_run_events_reads_engine_context_run_events_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-3/events",
            "200 OK",
            r#"{"events":[{"event_id":"evt-1","run_id":"ctx-3","seq":12,"ts_ms":1000,"type":"step_started","status":"running","step_id":"s-plan","payload":{"why_next_step":"execute plan"}}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let events = client
            .context_run_events("ctx-3", Some(10), Some(5))
            .await
            .expect("context_run_events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 12);
        assert_eq!(events[0].event_type, "step_started");
        assert_eq!(events[0].status, ContextRunStatus::Running);
        assert_eq!(events[0].step_id.as_deref(), Some("s-plan"));
    }

    #[tokio::test]
    async fn context_run_append_event_posts_to_engine_context_run_events_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-4/events",
            "200 OK",
            r#"{"event":{"event_id":"evt-2","run_id":"ctx-4","seq":3,"ts_ms":2000,"type":"run_paused","status":"paused","step_id":null,"payload":{"source":"tui"}}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let event = client
            .context_run_append_event(
                "ctx-4",
                "run_paused",
                ContextRunStatus::Paused,
                None,
                serde_json::json!({ "source": "tui" }),
            )
            .await
            .expect("context_run_append_event");
        assert_eq!(event.run_id, "ctx-4");
        assert_eq!(event.seq, 3);
        assert_eq!(event.status, ContextRunStatus::Paused);
    }

    #[tokio::test]
    async fn context_run_blackboard_reads_engine_context_run_blackboard_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-5/blackboard",
            "200 OK",
            r#"{"blackboard":{"facts":[{"id":"f1","ts_ms":1,"text":"fact","step_id":null,"source_event_id":null}],"decisions":[],"open_questions":[],"artifacts":[],"summaries":{"rolling":"summary","latest_context_pack":"pack"},"revision":9}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let blackboard = client
            .context_run_blackboard("ctx-5")
            .await
            .expect("context_run_blackboard");
        assert_eq!(blackboard.revision, 9);
        assert_eq!(blackboard.facts.len(), 1);
        assert_eq!(blackboard.summaries.rolling, "summary");
    }

    #[tokio::test]
    async fn context_run_replay_reads_engine_context_run_replay_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-6/replay",
            "200 OK",
            r#"{"ok":true,"run_id":"ctx-6","from_checkpoint":true,"checkpoint_seq":9,"events_applied":2,"replay":{"run_id":"ctx-6","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[],"why_next_step":"next","revision":3,"created_at_ms":1,"updated_at_ms":2},"persisted":{"run_id":"ctx-6","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[],"why_next_step":"next","revision":3,"created_at_ms":1,"updated_at_ms":2},"drift":{"mismatch":false,"status_mismatch":false,"why_next_step_mismatch":false,"step_count_mismatch":false}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let replay = client
            .context_run_replay("ctx-6", Some(10), Some(true))
            .await
            .expect("context_run_replay");
        assert_eq!(replay.run_id, "ctx-6");
        assert!(!replay.drift.mismatch);
        assert_eq!(replay.events_applied, 2);
    }

    #[tokio::test]
    async fn context_run_driver_next_posts_engine_context_run_driver_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-7/driver/next",
            "200 OK",
            r#"{"ok":true,"dry_run":false,"run_id":"ctx-7","selected_step_id":"s2","target_status":"running","why_next_step":"selected runnable step","run":{"run_id":"ctx-7","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[{"step_id":"s2","title":"Exec","status":"in_progress"}],"why_next_step":"selected runnable step","revision":4,"created_at_ms":1,"updated_at_ms":2}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let next = client
            .context_run_driver_next("ctx-7", false)
            .await
            .expect("context_run_driver_next");
        assert_eq!(next.run_id, "ctx-7");
        assert_eq!(next.selected_step_id.as_deref(), Some("s2"));
        assert_eq!(next.target_status, ContextRunStatus::Running);
    }

    #[tokio::test]
    async fn context_run_sync_todos_posts_engine_context_todos_sync_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-8/todos/sync",
            "200 OK",
            r#"{"run":{"run_id":"ctx-8","run_type":"interactive","status":"planning","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[{"step_id":"task-1","title":"Plan","status":"in_progress"}],"why_next_step":"continue task `task-1` from synced todo list","revision":5,"created_at_ms":1,"updated_at_ms":2}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let run = client
            .context_run_sync_todos(
                "ctx-8",
                vec![ContextTodoSyncItem {
                    id: Some("task-1".to_string()),
                    content: "Plan".to_string(),
                    status: Some("in_progress".to_string()),
                }],
                true,
                Some("s-1".to_string()),
                Some("r-1".to_string()),
            )
            .await
            .expect("context_run_sync_todos");
        assert_eq!(run.run_id, "ctx-8");
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].step_id, "task-1");
    }

    #[tokio::test]
    async fn packs_list_reads_engine_packs_endpoint() {
        let base = spawn_single_response_server(
            "/packs",
            "200 OK",
            r#"{"packs":[{"pack_id":"p1","name":"pack-one","version":"1.0.0"}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let packs = client.packs_list().await.expect("packs_list");
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_id, "p1");
        assert_eq!(packs[0].name, "pack-one");
    }

    #[tokio::test]
    async fn capabilities_bindings_get_reads_engine_endpoint() {
        let base = spawn_single_response_server(
            "/capabilities/bindings",
            "200 OK",
            r#"{"bindings":{"schema_version":"v1","bindings":[{"capability_id":"github.create_pull_request","provider":"composio","tool_name":"mcp.composio.github_create_pull_request","metadata":{}}]}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let bindings = client
            .capabilities_bindings_get()
            .await
            .expect("capabilities_bindings_get");
        assert_eq!(bindings.schema_version, "v1");
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(
            bindings.bindings[0].capability_id,
            "github.create_pull_request"
        );
    }

    #[tokio::test]
    async fn capabilities_resolve_posts_engine_endpoint() {
        let base = spawn_single_response_server(
            "/capabilities/resolve",
            "200 OK",
            r#"{"resolution":{"resolved":[{"capability_id":"github.create_pull_request","provider":"arcade","tool_name":"mcp.arcade.github_create_pull_request"}],"missing_required":[]}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let resolved = client
            .capabilities_resolve(CapabilityResolveRequest {
                workflow_id: Some("wf-pr".to_string()),
                required_capabilities: vec!["github.create_pull_request".to_string()],
                optional_capabilities: vec![],
                provider_preference: vec!["arcade".to_string(), "composio".to_string()],
                available_tools: vec![],
            })
            .await
            .expect("capabilities_resolve");
        let provider = resolved
            .resolution
            .get("resolved")
            .and_then(|v| v.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("provider"))
            .and_then(|v| v.as_str());
        assert_eq!(provider, Some("arcade"));
    }

    #[test]
    fn parse_stream_event_envelope_extracts_core_fields() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s1",
                "runID": "r1",
                "agentID": "A2",
                "channel": "assistant",
                "delta": "hello"
            }
        });
        let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
        assert_eq!(envelope.event_type, "message.part.updated");
        assert_eq!(envelope.session_id.as_deref(), Some("s1"));
        assert_eq!(envelope.run_id.as_deref(), Some("r1"));
        assert_eq!(envelope.agent_id.as_deref(), Some("A2"));
        assert_eq!(envelope.channel.as_deref(), Some("assistant"));
        assert_eq!(envelope.payload, payload);
    }

    #[test]
    fn parse_sse_payload_reads_data_block() {
        let mut buffer =
            "event: message\ndata: {\"type\":\"message.part.updated\",\"properties\":{\"delta\":\"x\"}}\n\n"
                .to_string();
        let parsed = parse_sse_payload(&mut buffer).expect("payload");
        assert_eq!(
            parsed.get("type").and_then(|v| v.as_str()),
            Some("message.part.updated")
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn parse_stream_event_envelope_handles_mission_events_contract_shape() {
        let payload = serde_json::json!({
            "type": "mission.created",
            "properties": {
                "missionID": "m-123",
                "workItemCount": 2
            }
        });
        let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
        assert_eq!(envelope.event_type, "mission.created");
        assert_eq!(envelope.session_id, None);
        assert_eq!(envelope.run_id, None);
        assert_eq!(envelope.agent_id, None);
        assert_eq!(envelope.channel, None);
        assert_eq!(
            envelope
                .payload
                .get("properties")
                .and_then(|p| p.get("missionID"))
                .and_then(|v| v.as_str()),
            Some("m-123")
        );
        assert_eq!(
            envelope
                .payload
                .get("properties")
                .and_then(|p| p.get("workItemCount"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn parse_stream_event_envelope_handles_routine_policy_events_contract_shape() {
        let payloads = vec![
            serde_json::json!({
                "type": "routine.fired",
                "properties": {
                    "routineID": "r-1",
                    "runCount": 1,
                    "triggerType": "manual",
                    "firedAtMs": 123
                }
            }),
            serde_json::json!({
                "type": "routine.approval_required",
                "properties": {
                    "routineID": "r-2",
                    "runCount": 1,
                    "triggerType": "manual",
                    "reason": "manual approval required before external side effects (manual)"
                }
            }),
            serde_json::json!({
                "type": "routine.blocked",
                "properties": {
                    "routineID": "r-3",
                    "runCount": 1,
                    "triggerType": "manual",
                    "reason": "external integrations are disabled by policy"
                }
            }),
        ];

        for payload in payloads {
            let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
            assert!(envelope.event_type.starts_with("routine."));
            assert_eq!(envelope.session_id, None);
            assert_eq!(envelope.run_id, None);
            assert_eq!(
                envelope
                    .payload
                    .get("properties")
                    .and_then(|p| p.get("routineID"))
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty()),
                Some(true)
            );
            assert_eq!(
                envelope
                    .payload
                    .get("properties")
                    .and_then(|p| p.get("runCount"))
                    .and_then(|v| v.as_u64()),
                Some(1)
            );
        }
    }

    #[test]
    fn extract_stream_error_reads_session_error() {
        let payload = serde_json::json!({
            "type": "session.error",
            "properties": {
                "error": { "code": "PROVIDER_AUTH", "message": "missing API key" }
            }
        });
        let msg = extract_stream_error(&payload).expect("error");
        assert!(msg.contains("PROVIDER_AUTH"));
        assert!(msg.contains("missing API key"));
    }

    #[test]
    fn extract_stream_tool_delta_reads_tool_call_delta_payload() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "toolCallDelta": {
                    "id": "call_1",
                    "tool": "write",
                    "argsDelta": "{\"path\":\"src/main.rs\"",
                    "parsedArgsPreview": { "path": "src/main.rs" }
                }
            }
        });
        let delta = extract_stream_tool_delta(&payload).expect("tool delta");
        assert_eq!(delta.tool_call_id, "call_1");
        assert_eq!(delta.tool_name, "write");
        assert!(delta.args_delta.contains("path"));
        assert!(delta.args_preview.contains("src/main.rs"));
    }

    #[test]
    fn extract_stream_tool_delta_ignores_non_tool_payloads() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "part": { "type": "text", "text": "hello" }
            }
        });
        assert!(extract_stream_tool_delta(&payload).is_none());
    }

    #[test]
    fn extract_stream_agent_team_event_reads_mailbox_properties() {
        let payload = serde_json::json!({
            "type": "agent_team.mailbox.enqueued",
            "properties": {
                "teamName": "alpha",
                "recipient": "A2",
                "messageType": "task_prompt",
                "messageID": "m-1"
            }
        });
        let event = extract_stream_agent_team_event(&payload).expect("agent-team event");
        assert_eq!(event.event_type, "agent_team.mailbox.enqueued");
        assert_eq!(event.team_name.as_deref(), Some("alpha"));
        assert_eq!(event.recipient.as_deref(), Some("A2"));
        assert_eq!(event.message_type.as_deref(), Some("task_prompt"));
        assert_eq!(event.message_id.as_deref(), Some("m-1"));
    }
}
