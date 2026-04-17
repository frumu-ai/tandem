use super::*;

#[tokio::test]
async fn todo_updated_event_is_normalized() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    emit_tool_side_events(
        storage.clone(),
        &bus,
        ToolSideEventContext {
            session_id: &session_id,
            message_id: "m1",
            tool: "todo_write",
            args: &json!({"todos":[{"content":"ship parity"}]}),
            metadata: &json!({"todos":[{"content":"ship parity"}]}),
            workspace_root: Some("."),
            effective_cwd: Some("."),
        },
    )
    .await;

    let event = rx.recv().await.expect("event");
    assert_eq!(event.event_type, "todo.updated");
    let todos = event
        .properties
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 1);
    assert!(todos[0].get("id").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("ship parity")
    );
    assert!(todos[0].get("status").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn question_asked_event_contains_tool_reference() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    emit_tool_side_events(
        storage,
        &bus,
        ToolSideEventContext {
            session_id: &session_id,
            message_id: "msg-1",
            tool: "question",
            args: &json!({"questions":[{"header":"Topic","question":"Pick one","options":[{"label":"A","description":"d"}]}]}),
            metadata: &json!({"questions":[{"header":"Topic","question":"Pick one","options":[{"label":"A","description":"d"}]}]}),
            workspace_root: Some("."),
            effective_cwd: Some("."),
        },
    )
    .await;

    let event = rx.recv().await.expect("event");
    assert_eq!(event.event_type, "question.asked");
    assert_eq!(
        event
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        session_id
    );
    let tool = event
        .properties
        .get("tool")
        .cloned()
        .unwrap_or_else(|| json!({}));
    assert!(tool.get("callID").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        tool.get("messageID").and_then(|v| v.as_str()),
        Some("msg-1")
    );
}

#[test]
fn compact_chat_history_keeps_recent_and_inserts_summary() {
    let mut messages = Vec::new();
    for i in 0..60 {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: format!("message-{i}"),
            attachments: Vec::new(),
        });
    }
    let compacted = compact_chat_history(messages, ChatHistoryProfile::Standard);
    assert!(compacted.len() <= 41);
    assert_eq!(compacted[0].role, "system");
    assert!(compacted[0].content.contains("history compacted"));
    assert!(compacted.iter().any(|m| m.content.contains("message-59")));
}

#[tokio::test]
async fn load_chat_history_preserves_tool_args_and_error_context() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-error-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::User,
        vec![
            MessagePart::Text {
                text: "build the page".to_string(),
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"game.html","content":"<html>draft</html>"}),
                result: None,
                error: Some("WRITE_ARGS_EMPTY_FROM_PROVIDER".to_string()),
            },
        ],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = history
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("build the page"));
    assert!(content.contains("Tool write"));
    assert!(content.contains(r#"args={"content":"<html>draft</html>","path":"game.html"}"#));
    assert!(content.contains("error=WRITE_ARGS_EMPTY_FROM_PROVIDER"));
}

#[tokio::test]
async fn load_chat_history_preserves_tool_args_and_result_context() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-result-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "glob".to_string(),
            args: json!({"pattern":"src/**/*.rs"}),
            result: Some(json!({"output":"src/lib.rs\nsrc/main.rs"})),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = history
        .iter()
        .find(|message| message.role == "assistant")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("Tool glob"));
    assert!(content.contains(r#"args={"pattern":"src/**/*.rs"}"#));
    assert!(content.contains(r#"result={"output":"src/lib.rs\nsrc/main.rs"}"#));
}

#[test]
fn extracts_todos_from_checklist_and_numbered_lines() {
    let input = r#"
Plan:
- [ ] Audit current implementation
- [ ] Add planner fallback
1. Add regression test coverage
"#;
    let todos = extract_todo_candidates_from_text(input);
    assert_eq!(todos.len(), 3);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("Audit current implementation")
    );
}

#[test]
fn does_not_extract_todos_from_plain_prose_lines() {
    let input = r#"
I need more information to proceed.
Can you tell me the event size and budget?
Once I have that, I can provide a detailed plan.
"#;
    let todos = extract_todo_candidates_from_text(input);
    assert!(todos.is_empty());
}

#[test]
fn parses_wrapped_tool_call_from_markdown_response() {
    let input = r#"
Here is the tool call:
```json
{"tool_call":{"name":"todo_write","arguments":{"todos":[{"content":"a"}]}}}
```
"#;
    let parsed = parse_tool_invocation_from_response(input).expect("tool call");
    assert_eq!(parsed.0, "todo_write");
    assert!(parsed.1.get("todos").is_some());
}

#[test]
fn parses_top_level_name_args_tool_call() {
    let input = r#"{"name":"bash","args":{"command":"echo hi"}}"#;
    let parsed = parse_tool_invocation_from_response(input).expect("top-level tool call");
    assert_eq!(parsed.0, "bash");
    assert_eq!(
        parsed.1.get("command").and_then(|v| v.as_str()),
        Some("echo hi")
    );
}

#[test]
fn parses_function_style_todowrite_call() {
    let input = r#"Status: Completed
Call: todowrite(task_id=2, status="completed")"#;
    let parsed = parse_tool_invocation_from_response(input).expect("function-style tool call");
    assert_eq!(parsed.0, "todo_write");
    assert_eq!(parsed.1.get("task_id").and_then(|v| v.as_i64()), Some(2));
    assert_eq!(
        parsed.1.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test]
fn parses_multiple_function_style_todowrite_calls() {
    let input = r#"
Call: todowrite(task_id=2, status="completed")
Call: todowrite(task_id=3, status="in_progress")
"#;
    let parsed = parse_tool_invocations_from_response(input);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].0, "todo_write");
    assert_eq!(parsed[0].1.get("task_id").and_then(|v| v.as_i64()), Some(2));
    assert_eq!(
        parsed[0].1.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
    assert_eq!(parsed[1].1.get("task_id").and_then(|v| v.as_i64()), Some(3));
    assert_eq!(
        parsed[1].1.get("status").and_then(|v| v.as_str()),
        Some("in_progress")
    );
}

#[test]
fn applies_todo_status_update_from_task_id_args() {
    let current = vec![
        json!({"id":"todo-1","content":"a","status":"pending"}),
        json!({"id":"todo-2","content":"b","status":"pending"}),
        json!({"id":"todo-3","content":"c","status":"pending"}),
    ];
    let updated =
        apply_todo_updates_from_args(current, &json!({"task_id":2, "status":"completed"}))
            .expect("status update");
    assert_eq!(
        updated[1].get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test]
fn normalizes_todo_write_tasks_alias() {
    let normalized = normalize_todo_write_args(
        json!({"tasks":[{"title":"Book venue"},{"name":"Send invites"}]}),
        "",
    );
    let todos = normalized
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 2);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("Book venue")
    );
    assert_eq!(
        todos[1].get("content").and_then(|v| v.as_str()),
        Some("Send invites")
    );
}

#[test]
fn normalizes_todo_write_from_completion_when_args_empty() {
    let completion = "Plan:\n1. Secure venue\n2. Create playlist\n3. Send invites";
    let normalized = normalize_todo_write_args(json!({}), completion);
    let todos = normalized
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 3);
    assert!(!is_empty_todo_write_args(&normalized));
}

#[test]
fn empty_todo_write_args_allows_status_updates() {
    let args = json!({"task_id": 2, "status":"completed"});
    assert!(!is_empty_todo_write_args(&args));
}

#[test]
fn streamed_websearch_args_fallback_to_query_string() {
    let parsed = parse_streamed_tool_args("websearch", "meaning of life");
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("meaning of life")
    );
}

#[test]
fn parse_scalar_like_value_handles_single_quote_character_without_panicking() {
    assert_eq!(
        parse_scalar_like_value("\""),
        Value::String("\"".to_string())
    );
    assert_eq!(parse_scalar_like_value("'"), Value::String("'".to_string()));
}

#[test]
fn streamed_websearch_stringified_json_args_are_unwrapped() {
    let parsed = parse_streamed_tool_args("websearch", r#""donkey gestation period""#);
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("donkey gestation period")
    );
}

#[test]
fn streamed_websearch_args_strip_arg_key_value_wrappers() {
    let parsed = parse_streamed_tool_args(
        "websearch",
        "query</arg_key><arg_value>taj card what is it benefits how to apply</arg_value>",
    );
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("taj card what is it benefits how to apply")
    );
}

#[test]
fn normalize_tool_args_websearch_infers_from_user_text() {
    let normalized = normalize_tool_args("websearch", json!({}), "web search meaning of life", "");
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("meaning of life")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_websearch_keeps_existing_query() {
    let normalized = normalize_tool_args(
        "websearch",
        json!({"query":"already set"}),
        "web search should not override",
        "",
    );
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("already set")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_websearch_fails_when_unrecoverable() {
    let normalized = normalize_tool_args("websearch", json!({}), "search", "");
    assert!(normalized.query.is_none());
    assert!(normalized.missing_terminal);
    assert_eq!(normalized.args_source, "missing");
    assert_eq!(normalized.args_integrity, "empty");
}

#[test]
fn normalize_tool_args_webfetch_infers_url_from_user_prompt() {
    let normalized = normalize_tool_args(
        "webfetch",
        json!({}),
        "Please fetch `https://docs.tandem.ac/` in markdown mode",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("url").and_then(|v| v.as_str()),
        Some("https://docs.tandem.ac/")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_webfetch_recovers_nested_url_alias() {
    let normalized = normalize_tool_args(
        "webfetch",
        json!({"args":{"uri":"https://example.com/page"}}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("url").and_then(|v| v.as_str()),
        Some("https://example.com/page")
    );
    assert_eq!(normalized.args_source, "provider_json");
}

#[test]
fn normalize_tool_args_webfetch_fails_when_url_unrecoverable() {
    let normalized = normalize_tool_args("webfetch", json!({}), "fetch the site", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WEBFETCH_URL_MISSING")
    );
}

#[test]
fn normalize_tool_args_answer_how_to_infers_task_from_user_prompt() {
    let user_text = "what is tandem and how do i use it?";
    let normalized = normalize_tool_args("mcp.tandem_mcp.answer_how_to", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("task").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_answer_how_to_keeps_existing_task() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.answer_how_to",
        json!({"task":"install tandem locally"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("task").and_then(|v| v.as_str()),
        Some("install tandem locally")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_search_docs_infers_query_from_user_prompt() {
    let user_text = "https://docs.tandem.ac/start-here/";
    let normalized = normalize_tool_args("mcp.tandem_mcp.search_docs", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_search_docs_keeps_existing_query() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.search_docs",
        json!({"query":"oauth setup"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("oauth setup")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_get_doc_infers_path_from_user_url() {
    let user_text = "https://docs.tandem.ac/start-here/";
    let normalized = normalize_tool_args("mcp.tandem_mcp.get_doc", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_get_doc_keeps_existing_path() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.get_doc",
        json!({"path":"/start-here/"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("/start-here/")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_pack_builder_infers_goal_from_user_prompt() {
    let user_text =
        "Create a pack that checks latest headline news every day at 8 AM and emails me.";
    let normalized = normalize_tool_args("pack_builder", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("preview")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_pack_builder_keeps_existing_goal_and_mode() {
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({"mode":"apply","goal":"existing goal","plan_id":"plan-1"}),
        "new goal should not override",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some("existing goal")
    );
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_pack_builder_confirm_reuses_plan_from_context() {
    let assistant_context =
        "Pack Builder Preview\n- Plan ID: plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let normalized = normalize_tool_args("pack_builder", json!({}), "confirm", assistant_context);
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        normalized.args.get("plan_id").and_then(|v| v.as_str()),
        Some("plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
    assert_eq!(
        normalized
            .args
            .get("approve_pack_install")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(normalized.args_source, "recovered_from_context");
}

#[test]
fn normalize_tool_args_pack_builder_apply_recovers_missing_plan_id() {
    let assistant_context =
        "{\"mode\":\"preview\",\"plan_id\":\"plan-11111111-2222-3333-4444-555555555555\"}";
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({"mode":"apply"}),
        "yes",
        assistant_context,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        normalized.args.get("plan_id").and_then(|v| v.as_str()),
        Some("plan-11111111-2222-3333-4444-555555555555")
    );
}

#[test]
fn normalize_tool_args_pack_builder_short_new_goal_does_not_force_apply() {
    let assistant_context =
        "Pack Builder Preview\n- Plan ID: plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({}),
        "create jira sync",
        assistant_context,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("preview")
    );
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some("create jira sync")
    );
}

#[test]
fn normalize_tool_args_write_requires_path() {
    let normalized = normalize_tool_args("write", json!({}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn persisted_failed_tool_args_prefers_normalized_when_raw_is_empty() {
    let args = persisted_failed_tool_args(
        &json!({}),
        &json!({"path":"game.html","content":"<html></html>"}),
    );
    assert_eq!(args["path"], "game.html");
    assert_eq!(args["content"], "<html></html>");
}

#[test]
fn persisted_failed_tool_args_keeps_non_empty_raw_payload() {
    let args = persisted_failed_tool_args(
        &json!("path=game.html content"),
        &json!({"path":"game.html"}),
    );
    assert_eq!(args, json!("path=game.html content"));
}

#[test]
fn normalize_tool_args_write_recovers_alias_path_key() {
    let normalized = normalize_tool_args(
        "write",
        json!({"filePath":"docs/CONCEPT.md","content":"hello"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/CONCEPT.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("hello")
    );
}

#[test]
fn normalize_tool_args_write_recovers_html_output_target_path() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({"content":"<html></html>"}),
        "Execute task.\n\nRequired output target:\n{\n  \"path\": \"game.html\",\n  \"kind\": \"source\",\n  \"operation\": \"create_or_update\"\n}\n",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("game.html")
    );
}

#[test]
fn normalize_tool_args_read_infers_path_from_user_prompt() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "Please inspect `FEATURE_LIST.md` and summarize key sections.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("FEATURE_LIST.md")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_does_not_infer_path_from_assistant_context() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "generic instruction",
        "I will read src-tauri/src/orchestrator/engine.rs first.",
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_recovers_path_from_nested_array_payload() {
    let normalized = normalize_tool_args(
        "write",
        json!({"args":[{"file_path":"docs/CONCEPT.md"}],"content":"hello"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/CONCEPT.md")
    );
}

#[test]
fn normalize_tool_args_write_recovers_content_alias() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md","body":"feature notes"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("feature notes")
    );
}

#[test]
fn normalize_tool_args_write_fails_when_content_missing() {
    let normalized = normalize_tool_args("write", json!({"path":"docs/FEATURES.md"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WRITE_CONTENT_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_output_target_only_rejects_freeform_guess() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({}),
        "Please implement the screen/state structure in the workspace.",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_output_target_only_recovers_from_dot_slash_path() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({"path":"./","content":"{}"}),
        "Required Workspace Output:\n- Create or update `.tandem/runs/automation-v2-run-123/artifacts/research-sources.json` relative to the workspace root.",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some(".tandem/runs/automation-v2-run-123/artifacts/research-sources.json")
    );
}

#[test]
fn normalize_tool_args_write_recovers_content_from_assistant_context() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md"}),
        "",
        "## Features\n\n- Neon arcade gameplay\n- Single-file HTML structure\n",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/FEATURES.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("## Features\n\n- Neon arcade gameplay\n- Single-file HTML structure")
    );
    assert_eq!(normalized.args_source, "recovered_from_context");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_write_recovers_raw_nested_string_content() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md","args":"Line 1\nLine 2"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/FEATURES.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("Line 1\nLine 2")
    );
}

#[test]
fn normalize_tool_args_write_does_not_treat_path_as_content() {
    let normalized = normalize_tool_args("write", json!("docs/FEATURES.md"), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WRITE_CONTENT_MISSING")
    );
}

#[test]
fn normalize_tool_args_gmail_send_email_omits_empty_attachment() {
    let normalized = normalize_tool_args(
        "mcp.composio_1.gmail_send_email",
        json!({
            "to": "user123@example.com",
            "subject": "Test",
            "body": "Hello",
            "attachment": {
                "s3key": ""
            }
        }),
        "",
        "",
    );
    assert!(normalized.args.get("attachment").is_none());
    assert_eq!(normalized.args_source, "sanitized_attachment");
}

#[test]
fn normalize_tool_args_gmail_send_email_keeps_valid_attachment() {
    let normalized = normalize_tool_args(
        "mcp.composio_1.gmail_send_email",
        json!({
            "to": "user123@example.com",
            "subject": "Test",
            "body": "Hello",
            "attachment": {
                "s3key": "file_123"
            }
        }),
        "",
        "",
    );
    assert_eq!(
        normalized
            .args
            .get("attachment")
            .and_then(|value| value.get("s3key"))
            .and_then(|value| value.as_str()),
        Some("file_123")
    );
}

#[test]
fn classify_required_tool_failure_detects_empty_provider_write_args() {
    let reason = classify_required_tool_failure(
        &[String::from("WRITE_ARGS_EMPTY_FROM_PROVIDER")],
        true,
        1,
        false,
        false,
    );
    assert_eq!(reason, RequiredToolFailureKind::WriteArgsEmptyFromProvider);
}

#[test]
fn normalize_tool_args_read_infers_path_from_bold_markdown() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "Please read **FEATURE_LIST.md** and summarize.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("FEATURE_LIST.md")
    );
}

#[test]
fn normalize_tool_args_shell_infers_command_from_user_prompt() {
    let normalized = normalize_tool_args("bash", json!({}), "Run `rg -n \"TODO\" .`", "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("command").and_then(|v| v.as_str()),
        Some("rg -n \"TODO\" .")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_rejects_root_only_path() {
    let normalized = normalize_tool_args("read", json!({"path":"/"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_recovers_when_provider_path_is_root_only() {
    let normalized =
        normalize_tool_args("read", json!({"path":"/"}), "Please open `CONCEPT.md`", "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("CONCEPT.md")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_rejects_tool_call_markup_path() {
    let normalized = normalize_tool_args(
        "read",
        json!({
            "path":"<tool_call>\n<function=glob>\n<parameter=pattern>**/*</parameter>\n</function>\n</tool_call>"
        }),
        "",
        "",
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_glob_pattern_path() {
    let normalized = normalize_tool_args("read", json!({"path":"**/*"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_placeholder_path() {
    let normalized = normalize_tool_args("read", json!({"path":"files/directories"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_tool_policy_placeholder_path() {
    let normalized = normalize_tool_args("read", json!({"path":"tool/policy"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_recovers_pdf_path_from_user_text() {
    let normalized = normalize_tool_args(
        "read",
        json!({"path":"tool/policy"}),
        "Read `T1011U kitöltési útmutató.pdf` and summarize.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("T1011U kitöltési útmutató.pdf")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_name_strips_default_api_namespace() {
    assert_eq!(normalize_tool_name("default_api:read"), "read");
    assert_eq!(normalize_tool_name("functions.shell"), "bash");
}

#[test]
fn mcp_server_from_tool_name_parses_server_segment() {
    assert_eq!(
        mcp_server_from_tool_name("mcp.arcade.jira_getboards"),
        Some("arcade")
    );
    assert_eq!(mcp_server_from_tool_name("read"), None);
    assert_eq!(mcp_server_from_tool_name("mcp"), None);
}

#[test]
fn mcp_tools_are_exempt_from_workspace_sandbox_path_checks() {
    assert!(is_mcp_tool_name("mcp_list"));
    assert!(is_mcp_tool_name("mcp.tandem_mcp.get_doc"));
    assert!(is_mcp_tool_name("MCP.TANDEM_MCP.GET_DOC"));
    assert!(!is_mcp_tool_name("read"));
    assert!(!is_mcp_tool_name("glob"));
}

#[test]
fn batch_helpers_use_name_when_tool_is_wrapper() {
    let args = json!({
        "tool_calls":[
            {"tool":"default_api","name":"read","args":{"path":"CONCEPT.md"}},
            {"tool":"default_api:glob","args":{"pattern":"*.md"}}
        ]
    });
    let calls = extract_batch_calls(&args);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "read");
    assert_eq!(calls[1].0, "glob");
    assert!(is_read_only_batch_call(&args));
    let sig = batch_tool_signature(&args).unwrap_or_default();
    assert!(sig.contains("read:"));
    assert!(sig.contains("glob:"));
}

#[test]
fn batch_helpers_resolve_nested_function_name() {
    let args = json!({
        "tool_calls":[
            {"tool":"default_api","function":{"name":"read"},"args":{"path":"CONCEPT.md"}}
        ]
    });
    let calls = extract_batch_calls(&args);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "read");
    assert!(is_read_only_batch_call(&args));
}

#[test]
fn batch_output_classifier_detects_non_productive_unknown_results() {
    let output = r#"
[
  {"tool":"default_api","output":"Unknown tool: default_api","metadata":{}},
  {"tool":"default_api","output":"Unknown tool: default_api","metadata":{}}
]
"#;
    assert!(is_non_productive_batch_output(output));
}

#[test]
fn runtime_prompt_includes_execution_environment_block() {
    let prompt = tandem_runtime_system_prompt(
        &HostRuntimeContext {
            os: HostOs::Windows,
            arch: "x86_64".to_string(),
            shell_family: ShellFamily::Powershell,
            path_style: PathStyle::Windows,
        },
        &[],
    );
    assert!(prompt.contains("[Execution Environment]"));
    assert!(prompt.contains("Host OS: windows"));
    assert!(prompt.contains("Shell: powershell"));
    assert!(prompt.contains("Path style: windows"));
}

#[test]
fn runtime_prompt_includes_connected_integrations_block() {
    let prompt = tandem_runtime_system_prompt(
        &HostRuntimeContext {
            os: HostOs::Linux,
            arch: "x86_64".to_string(),
            shell_family: ShellFamily::Posix,
            path_style: PathStyle::Posix,
        },
        &["notion".to_string(), "github".to_string()],
    );
    assert!(prompt.contains("[Connected Integrations]"));
    assert!(prompt.contains("- notion"));
    assert!(prompt.contains("- github"));
}

#[test]
fn detects_web_research_prompt_keywords() {
    assert!(requires_web_research_prompt(
        "research todays top news stories and include links"
    ));
    assert!(!requires_web_research_prompt(
        "say hello and summarize this text"
    ));
}

#[test]
fn detects_email_delivery_prompt_keywords() {
    assert!(requires_email_delivery_prompt(
        "send a full report with links to user123@example.com"
    ));
    assert!(!requires_email_delivery_prompt("draft a summary for later"));
}

#[test]
fn completion_claim_detector_flags_sent_language() {
    assert!(completion_claims_email_sent(
        "Email Status: Sent to user123@example.com."
    ));
    assert!(!completion_claims_email_sent(
        "I could not send email in this run."
    ));
}
