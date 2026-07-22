// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn normalize_run_event_adds_required_fields() {
    let event = EngineEvent::new(
        "message.part.updated",
        json!({
            "part": { "type": "text" },
            "delta": "hello"
        }),
    );
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let normalized = normalize_run_event(event, "s-1", "r-1", &tenant_context);
    assert_eq!(
        normalized
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str()),
        Some("s-1")
    );
    assert_eq!(
        normalized.properties.get("runID").and_then(|v| v.as_str()),
        Some("r-1")
    );
    assert_eq!(
        normalized
            .properties
            .get("channel")
            .and_then(|v| v.as_str()),
        Some("assistant")
    );
}

#[test]
fn infer_event_channel_routes_tool_message_parts() {
    let channel = infer_event_channel(
        "message.part.updated",
        &serde_json::from_value::<serde_json::Map<String, Value>>(json!({
            "part": { "type": "tool-result" }
        }))
        .expect("map"),
    );
    assert_eq!(channel, "tool");
}

#[test]
fn extract_persistable_tool_part_uses_streamed_args_preview_when_part_args_empty() {
    let properties = json!({
        "part": {
            "type": "tool",
            "tool": "write",
            "messageID": "msg_123",
            "args": {}
        },
        "toolCallDelta": {
            "parsedArgsPreview": {
                "path": "game.html",
                "content": "<html></html>"
            }
        }
    });
    let (message_id, part) = crate::extract_persistable_tool_part(&properties).expect("tool part");
    assert_eq!(message_id, "msg_123");
    match part {
        tandem_types::MessagePart::ToolInvocation { tool, args, .. } => {
            assert_eq!(tool, "write");
            assert_eq!(args["path"], "game.html");
            assert_eq!(args["content"], "<html></html>");
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }
}

#[test]
fn extract_persistable_tool_part_skips_running_tool_deltas() {
    let properties = json!({
        "part": {
            "type": "tool",
            "tool": "write",
            "messageID": "msg_123",
            "state": "running",
            "args": {
                "path": "draft.md",
                "content": "partial"
            }
        }
    });
    assert!(
        crate::extract_persistable_tool_part(&properties).is_none(),
        "running tool deltas should not be persisted"
    );
}

#[test]
fn extract_persistable_tool_part_keeps_completed_tool_updates() {
    let properties = json!({
        "part": {
            "type": "tool",
            "tool": "write",
            "messageID": "msg_123",
            "state": "completed",
            "args": {
                "path": "final.md",
                "content": "done"
            },
            "result": "ok"
        }
    });
    let (message_id, part) =
        crate::extract_persistable_tool_part(&properties).expect("completed tool part");
    assert_eq!(message_id, "msg_123");
    match part {
        tandem_types::MessagePart::ToolInvocation {
            tool, args, result, ..
        } => {
            assert_eq!(tool, "write");
            assert_eq!(args["path"], "final.md");
            assert_eq!(args["content"], "done");
            assert_eq!(result.as_ref(), Some(&json!("ok")));
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }
}

#[test]
fn extract_persistable_tool_part_accepts_snake_case_tool_types() {
    let properties = json!({
        "part": {
            "type": "tool_invocation",
            "tool": "websearch",
            "messageID": "msg_123",
            "state": "completed",
            "args": {
                "query": "tandem workflow reliability"
            },
            "result": {
                "result_count": 1
            }
        }
    });
    let (message_id, part) =
        crate::extract_persistable_tool_part(&properties).expect("snake_case tool part");
    assert_eq!(message_id, "msg_123");
    match part {
        tandem_types::MessagePart::ToolInvocation { tool, args, .. } => {
            assert_eq!(tool, "websearch");
            assert_eq!(args["query"], "tandem workflow reliability");
        }
        other => panic!("expected tool invocation, got {other:?}"),
    }
}

#[tokio::test]
async fn prompt_async_permission_approve_executes_tool_and_emits_todo_update() {
    let state = test_state().await;
    state
        .providers
        .replace_for_test(
            vec![Arc::new(ScriptedStrictKbProvider {
                steps: Arc::new(Mutex::new(VecDeque::from([
                    StrictKbProviderStep::ToolCall {
                        tool: "todo_write".to_string(),
                        args: json!({"todos":[{"content":"write tests"}]}),
                    },
                    StrictKbProviderStep::Text("done".to_string()),
                ]))),
            })],
            Some("strict-kb-test".to_string()),
        )
        .await;
    let mut session = Session::new(Some("perm".to_string()), Some(".".to_string()));
    session.model = Some(ModelSpec {
        provider_id: "strict-kb-test".to_string(),
        model_id: "strict-kb-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, vec!["todo_write".to_string()])
        .await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let prompt_body = json!({
        "parts": [
            {
                "type": "text",
                "text": "update the todo list"
            }
        ],
        "model": {
            "provider_id": "strict-kb-test",
            "model_id": "strict-kb-test-1"
        },
        "tool_mode": "required",
        "tool_allowlist": ["todo_write"]
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(prompt_body.to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let request_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "permission.asked" {
                let id = event
                    .properties
                    .get("requestID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !id.is_empty() {
                    return id;
                }
            }
        }
    })
    .await
    .expect("permission asked timeout");

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/sessions/{}/tools/{}/approve",
            session_id, request_id
        ))
        .body(Body::empty())
        .expect("approve request");
    let approve_resp = app.clone().oneshot(approve_req).await.expect("approve");
    assert_eq!(approve_resp.status(), StatusCode::OK);

    let todo_event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "todo.updated" {
                return event;
            }
        }
    })
    .await
    .expect("todo.updated timeout");

    assert_eq!(
        todo_event
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str()),
        Some(session_id.as_str())
    );
    let todos = todo_event
        .properties
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 1);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("write tests")
    );
}

#[tokio::test]
async fn approve_route_returns_error_envelope_for_unknown_request() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/sessions/s1/tools/missing/approve")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("APPROVAL_REQUEST_NOT_FOUND")
    );
    assert_eq!(
        payload.get("retryable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(payload.get("error").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn prompt_async_return_run_returns_202_with_run_id_and_attach_stream() {
    let state = test_state().await;
    let session = Session::new(Some("return-run".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async?return=run"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"hello return=run"}]}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let run_id = payload.get("runID").and_then(|v| v.as_str()).unwrap_or("");
    let attach = payload
        .get("attachEventStream")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(!run_id.is_empty());
    assert_eq!(
        context_run_id,
        crate::http::session_context_run_id(&session_id)
    );
    assert_eq!(
        attach,
        format!("/event?sessionID={session_id}&runID={run_id}")
    );
    let context_run_resp = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn existing_session_context_run_backfills_slack_attribution() {
    let state = test_state().await;
    let mut session = Session::new(Some("existing thread".to_string()), Some(".".to_string()));
    let context_run_id = crate::http::context_runs::ensure_session_context_run(&state, &session)
        .await
        .expect("create context run");
    let initial = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
        .await
        .expect("load initial context run");
    assert_eq!(initial.source_client.as_deref(), Some("session_api"));

    session.source_kind = Some("channel".to_string());
    session.source_metadata = Some(json!({
        "channel": "slack",
        "user_id": "U_EXISTING",
        "slack_team_id": "T_EXISTING",
        "slack_channel_id": "C_EXISTING",
        "slack_thread_ts": "1800000400.000001",
    }));
    crate::http::context_runs::ensure_session_context_run(&state, &session)
        .await
        .expect("backfill context run");

    let updated = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
        .await
        .expect("load updated context run");
    assert_eq!(updated.source_client.as_deref(), Some("channel:slack"));
    assert_eq!(
        updated
            .source_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("user_id"))
            .and_then(Value::as_str),
        Some("U_EXISTING")
    );
    assert!(updated.revision > initial.revision);
}

#[tokio::test]
async fn prompt_async_stream_error_persists_error_message_and_finishes_run() {
    let state = test_state().await;
    state
        .providers
        .replace_for_test(
            vec![Arc::new(ScriptedStrictKbProvider {
                steps: Arc::new(Mutex::new(VecDeque::from([
                    StrictKbProviderStep::StreamError("provider stream exploded".to_string()),
                ]))),
            })],
            Some("strict-kb-test".to_string()),
        )
        .await;
    let mut session = Session::new(Some("stream-error".to_string()), Some(".".to_string()));
    session.model = Some(ModelSpec {
        provider_id: "strict-kb-test".to_string(),
        model_id: "strict-kb-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async?return=run"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts":[{"type":"text","text":"trigger a stream failure"}],
                "model": {
                    "provider_id": "strict-kb-test",
                    "model_id": "strict-kb-test-1"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let run_id = payload
        .get("runID")
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let finished = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "session.run.finished"
                && event.properties.get("sessionID").and_then(Value::as_str)
                    == Some(session_id.as_str())
                && event.properties.get("runID").and_then(Value::as_str) == Some(run_id.as_str())
            {
                return event;
            }
        }
    })
    .await
    .expect("session.run.finished timeout");
    assert_eq!(
        finished.properties.get("status").and_then(Value::as_str),
        Some("error")
    );
    assert!(finished
        .properties
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("provider stream exploded")));

    let stored = state
        .storage
        .get_session(&session_id)
        .await
        .expect("stored session");
    assert!(stored.messages.iter().any(|message| {
        matches!(message.role, MessageRole::Assistant)
            && message.parts.iter().any(|part| {
                matches!(
                    part,
                    MessagePart::Text { text }
                        if text.contains("ENGINE_ERROR")
                            && text.contains("provider stream exploded")
                )
            })
    }));

    let run_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/run"))
        .body(Body::empty())
        .expect("run request");
    let run_resp = app.oneshot(run_req).await.expect("run response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    assert!(run_payload.get("active").is_none_or(Value::is_null));
}

#[tokio::test]
async fn get_session_run_returns_active_metadata_while_run_is_in_flight() {
    let state = test_state().await;
    let session = Session::new(Some("active-run".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"hold run\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
    let first_body = to_bytes(first_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
    let run_id = first_payload
        .get("runID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!run_id.is_empty());

    let run_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/run"))
        .body(Body::empty())
        .expect("request");
    let run_resp = app.oneshot(run_req).await.expect("response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("json");
    let active = run_payload.get("active").cloned().unwrap_or(Value::Null);
    assert_eq!(
        active.get("runID").and_then(|v| v.as_str()),
        Some(run_id.as_str())
    );
    assert_eq!(
        run_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(crate::http::session_context_run_id(&session_id).as_str())
    );
    let context_run_resp = app_router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/runs/{}",
                    crate::http::session_context_run_id(&session_id)
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn session_context_run_journaler_persists_session_run_lineage() {
    let state = test_state().await;
    let task = tokio::spawn(crate::run_session_context_run_journaler(state.clone()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut session = Session::new(
        Some("journal interactive run".to_string()),
        Some(".".to_string()),
    );
    session.workspace_root = Some("/tmp/tandem-session-journal".to_string());
    session.project_id = Some("proj-session-journal".to_string());
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let tenant_context = tandem_types::TenantContext::local_implicit();

    state.event_bus.publish(EngineEvent::new(
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "agentID": "interactive",
            "agentProfile": "interactive",
            "tenantContext": tenant_context.clone(),
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "message.part.updated",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "part": {
                "type": "tool",
                "tool": "read",
                "state": "running",
                "args": { "path": "README.md" }
            },
            "tenantContext": tenant_context.clone(),
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "session.run.finished",
        json!({
            "sessionID": session_id,
            "runID": "run-session-journal-1",
            "status": "completed",
            "tenantContext": tenant_context,
        }),
    ));

    let context_run_id = crate::http::session_context_run_id(&session_id);
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(run) =
                crate::http::context_runs::load_context_run_state(&state, &context_run_id).await
            {
                if run.last_event_seq >= 3 {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("context run journal persisted");

    let run = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
        .await
        .expect("context run");
    assert_eq!(run.run_id, context_run_id);
    assert_eq!(run.run_type, "session");
    assert_eq!(
        run.status,
        crate::http::context_types::ContextRunStatus::Completed
    );
    assert_eq!(run.workspace.canonical_path, "/tmp/tandem-session-journal");
    assert!(run.started_at_ms.is_some());

    let app = app_router(state.clone());
    let events_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{context_run_id}/events"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(events_resp.status(), StatusCode::OK);
    let events_body = to_bytes(events_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let events_payload: Value = serde_json::from_slice(&events_body).expect("json");
    let events = events_payload
        .get("events")
        .and_then(|value| value.as_array())
        .expect("events array");
    let event_types = events
        .iter()
        .filter_map(|row| row.get("type").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        vec![
            "session_run_started",
            "session_tool_updated",
            "session_run_finished",
        ]
    );
    let first_event = events.first().cloned().expect("first event");
    assert_eq!(
        first_event
            .get("payload")
            .and_then(|payload| payload.get("tenantContext"))
            .and_then(|value| value.get("org_id"))
            .and_then(Value::as_str),
        Some("local")
    );
    assert_eq!(
        first_event
            .get("payload")
            .and_then(|payload| payload.get("tenantContext"))
            .and_then(|value| value.get("workspace_id"))
            .and_then(Value::as_str),
        Some("local")
    );

    task.abort();
}

#[tokio::test]
async fn concurrent_prompt_async_returns_conflict_with_nested_active_run() {
    let state = test_state().await;
    let session = Session::new(Some("conflict".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async?return=run"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [
                    {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block\"}]}"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
    let first_body = to_bytes(first_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
    let active_run_id = first_payload
        .get("runID")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!active_run_id.is_empty());

    let second_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"second prompt"}]}).to_string(),
        ))
        .expect("request");
    let second_resp = app.clone().oneshot(second_req).await.expect("response");
    assert_eq!(second_resp.status(), StatusCode::CONFLICT);
    let second_body = to_bytes(second_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let second_payload: Value = serde_json::from_slice(&second_body).expect("json");
    assert_eq!(
        second_payload.get("code").and_then(|v| v.as_str()),
        Some("SESSION_RUN_CONFLICT")
    );
    assert_eq!(
        second_payload
            .get("activeRun")
            .and_then(|v| v.get("runID"))
            .and_then(|v| v.as_str()),
        Some(active_run_id.as_str())
    );
    assert!(second_payload
        .get("activeRun")
        .and_then(|v| v.get("startedAtMs"))
        .and_then(|v| v.as_i64())
        .is_some());
    assert!(second_payload
        .get("activeRun")
        .and_then(|v| v.get("lastActivityAtMs"))
        .and_then(|v| v.as_i64())
        .is_some());
    assert!(second_payload
        .get("retryAfterMs")
        .and_then(|v| v.as_u64())
        .is_some());
    assert_eq!(
        second_payload
            .get("attachEventStream")
            .and_then(|v| v.as_str()),
        Some(format!("/event?sessionID={session_id}&runID={active_run_id}").as_str())
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn append_message_succeeds_while_run_is_active() {
    let state = test_state().await;
    let session = Session::new(Some("append-active".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save");
    let app = app_router(state.clone());

    let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block append\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
    let first_resp = app.clone().oneshot(first_req).await.expect("response");
    assert_eq!(first_resp.status(), StatusCode::ACCEPTED);

    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message?mode=append"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"parts":[{"type":"text","text":"appended while active"}]}).to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);
    let _ = to_bytes(append_resp.into_body(), usize::MAX)
        .await
        .expect("body");

    let list_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}/message"))
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    let list = list_payload.as_array().cloned().unwrap_or_default();
    assert!(!list.is_empty());
    let has_appended_text = list.iter().any(|message| {
        message
            .get("parts")
            .and_then(|v| v.as_array())
            .map(|parts| {
                parts.iter().any(|part| {
                    part.get("text").and_then(|v| v.as_str()) == Some("appended while active")
                })
            })
            .unwrap_or(false)
    });
    assert!(has_appended_text);

    let cancel_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/cancel"))
        .body(Body::empty())
        .expect("cancel request");
    let cancel_resp = app_router(state)
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auto_rename_session_on_first_message() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // 1. Create session
    let create_req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "title": null }).to_string()))
        .expect("create request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let session_id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "New session");

    // 2. Append first message
    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type": "text", "text": "Hello world this is a test message"}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);

    // 3. Verify title changed
    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "Hello world this is a test message");

    // 4. Append second message
    let append_req_2 = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type": "text", "text": "Another message"}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp_2 = app.clone().oneshot(append_req_2).await.expect("response");
    assert_eq!(append_resp_2.status(), StatusCode::OK);

    // 5. Verify title did NOT change
    let get_req_2 = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp_2 = app.clone().oneshot(get_req_2).await.expect("response");

    let body = to_bytes(get_resp_2.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    // Title should remain as the first message
    assert_eq!(title, "Hello world this is a test message");
}

#[tokio::test]
async fn auto_rename_ignores_memory_context_wrappers() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "title": null }).to_string()))
        .expect("create request");
    let create_resp = app.clone().oneshot(create_req).await.expect("response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let session_id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    let wrapped = "<memory_context>\n<current_session>\n- fact\n</current_session>\n</memory_context>\n\n[Mode instructions]\nUse tools.\n\n[User request]\nShip the fix quickly";
    let append_req = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/message"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{"type":"text","text": wrapped}]
            })
            .to_string(),
        ))
        .expect("append request");
    let append_resp = app.clone().oneshot(append_req).await.expect("response");
    assert_eq!(append_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let session: Value = serde_json::from_slice(&body).expect("json");
    let title = session
        .get("title")
        .and_then(|v| v.as_str())
        .expect("title");
    assert_eq!(title, "Ship the fix quickly");
}

#[tokio::test]
async fn get_config_redacts_channel_bot_token() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["*"],
                    "mention_only": false
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/config")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload
            .get("effective")
            .and_then(|v| v.get("channels"))
            .and_then(|v| v.get("telegram"))
            .and_then(|v| v.get("bot_token"))
            .and_then(Value::as_str),
        Some("[REDACTED]")
    );
}

// WRC-03 (TAN-47): session runtime edge cases — interrupting in-flight tool work
// on abort, abort/archival lifecycle failure handling, and permission-denial
// decisions surfaced through the session permission registry.

#[tokio::test]
async fn delete_session_defers_when_run_is_active() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "title": "delete-active", "directory": "." }).to_string(),
        ))
        .expect("create request");
    let create_resp = app.clone().oneshot(create).await.expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let session_id = payload
        .get("id")
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    state
        .run_registry
        .acquire(&session_id, "run-delete-1".to_string(), None, None, None)
        .await
        .expect("acquire active run");

    let delete = Request::builder()
        .method("DELETE")
        .uri(format!("/session/{session_id}"))
        .body(Body::empty())
        .expect("delete request");
    let delete_resp = app.clone().oneshot(delete).await.expect("delete response");
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_payload: Value = serde_json::from_slice(
        &to_bytes(delete_resp.into_body(), usize::MAX)
            .await
            .expect("delete body"),
    )
    .expect("delete json");
    assert_eq!(
        delete_payload.get("deleted").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        delete_payload.get("reason").and_then(Value::as_str),
        Some("active_run")
    );

    assert!(
        state.storage.get_session(&session_id).await.is_some(),
        "active session must remain available for run finalization"
    );
    assert!(
        state.run_registry.get(&session_id).await.is_some(),
        "active run stays registered until execute_run observes cancellation"
    );

    let delayed_token = state.cancellations.create(&session_id).await;
    assert!(
        delayed_token.is_cancelled(),
        "delete must latch cancellation until the run creates its token"
    );
}

#[tokio::test]
async fn abort_session_interrupts_active_run() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "title": "interrupt", "directory": "." }).to_string(),
        ))
        .expect("create request");
    let create_resp = app.clone().oneshot(create).await.expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let session_id = payload
        .get("id")
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    // Simulate an in-flight run / tool stream for this session.
    state
        .run_registry
        .acquire(&session_id, "run-interrupt-1".to_string(), None, None, None)
        .await
        .expect("acquire active run");
    assert!(state.run_registry.get(&session_id).await.is_some());

    let abort = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/abort"))
        .header("x-tandem-actor-id", "operator-interrupt")
        .body(Body::empty())
        .expect("abort request");
    let abort_resp = app.clone().oneshot(abort).await.expect("abort response");
    assert_eq!(abort_resp.status(), StatusCode::OK);
    let abort_payload: Value = serde_json::from_slice(
        &to_bytes(abort_resp.into_body(), usize::MAX)
            .await
            .expect("abort body"),
    )
    .expect("abort json");
    assert_eq!(abort_payload.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        abort_payload.get("cancelled").and_then(Value::as_bool),
        Some(true)
    );

    // The in-flight run is interrupted and cleared from the registry.
    assert!(state.run_registry.get(&session_id).await.is_none());
}

#[tokio::test]
async fn abort_unknown_session_returns_not_found() {
    let state = test_state().await;
    let app = app_router(state);
    let abort = Request::builder()
        .method("POST")
        .uri("/session/does-not-exist/abort")
        .body(Body::empty())
        .expect("abort request");
    let abort_resp = app.oneshot(abort).await.expect("abort response");
    assert_eq!(abort_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_session_permission_rules_enforce_allow_deny_and_default_ask() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/session")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "permission denial",
                "directory": ".",
                "permission": [
                    {"permission": "write", "pattern": "*", "action": "allow"},
                    {"permission": "todo_write", "pattern": "todo_write", "action": "deny"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let rules = state.permissions.list_rules().await;
    let session_id = rules[0].session_id.as_deref().expect("session-scoped rule");
    assert!(matches!(
        state
            .permissions
            .evaluate_for_tenant_and_session(
                &TenantContext::local_implicit(),
                Some(session_id),
                "write",
                "anything",
            )
            .await,
        tandem_core::PermissionAction::Allow
    ));
    assert!(matches!(
        state
            .permissions
            .evaluate_for_tenant_and_session(
                &TenantContext::local_implicit(),
                Some(session_id),
                "todo_write",
                "todo_write",
            )
            .await,
        tandem_core::PermissionAction::Deny
    ));
    assert!(matches!(
        state
            .permissions
            .evaluate_for_tenant_and_session(
                &TenantContext::local_implicit(),
                Some("different-session"),
                "write",
                "anything",
            )
            .await,
        tandem_core::PermissionAction::Ask
    ));
    // A tool with no matching rule falls back to Ask (operator approval required).
    assert!(matches!(
        state
            .permissions
            .evaluate_for_tenant_and_session(
                &TenantContext::local_implicit(),
                Some(session_id),
                "unmatched_tool",
                "x",
            )
            .await,
        tandem_core::PermissionAction::Ask
    ));
}
