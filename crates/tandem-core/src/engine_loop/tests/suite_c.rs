use super::*;

#[tokio::test]
async fn final_context_budget_event_emitted_before_provider_send() {
    let _guard = env_test_lock();
    let base = std::env::temp_dir().join(format!("engine-loop-context-budget-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("context budget".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "Please review the project goals and summarize the current state of \
                           the context budgeting work in enough detail that a teammate could \
                           pick the task up without rereading the entire history."
                        .to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await
        .expect("prompt runs");

    let mut budget_event = None;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "context.budget.final" {
            budget_event = Some(event.properties.clone());
        }
        assert_ne!(event.event_type, "context.mode.full.selected");
    }
    let budget = budget_event.expect("context.budget.final event emitted");
    assert_eq!(
        budget.get("sessionID").and_then(Value::as_str),
        Some(session_id.as_str())
    );
    assert_eq!(
        budget.get("historyProfile").and_then(Value::as_str),
        Some("standard")
    );
    assert_eq!(
        budget.get("fullContextMode").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        budget.get("compactionOccurred").and_then(Value::as_bool),
        Some(false)
    );
    let final_message_count = budget
        .get("finalMessageCount")
        .and_then(Value::as_u64)
        .expect("final message count");
    assert!(final_message_count >= 2, "system + user expected");
    let final_chars = budget
        .get("finalMessageChars")
        .and_then(Value::as_u64)
        .expect("final message chars");
    assert!(final_chars > 0);
    let estimated_tokens = budget
        .get("estimatedPromptTokens")
        .and_then(Value::as_u64)
        .expect("estimated prompt tokens");
    assert!(estimated_tokens > 0);
    let contribution = budget.get("contribution").expect("contribution breakdown");
    assert!(
        contribution
            .get("systemChars")
            .and_then(Value::as_u64)
            .expect("system chars")
            > 0
    );
    assert!(
        contribution
            .get("historyChars")
            .and_then(Value::as_u64)
            .expect("history chars")
            > 0
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn full_context_mode_emits_dedicated_selection_event_with_correlation_kind() {
    let _guard = env_test_lock();
    let base = std::env::temp_dir().join(format!("engine-loop-full-selected-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("full selected".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    engine
        .run_prompt_async_with_context(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: Some(ContextMode::Full),
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
            Some("coder:run-1:issue_fix_worker".to_string()),
        )
        .await
        .expect("prompt runs");

    let mut selection_event = None;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "context.mode.full.selected" {
            selection_event = Some(event.properties.clone());
        }
    }
    let selected = selection_event.expect("context.mode.full.selected emitted");
    assert_eq!(
        selected.get("autonomousLike").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        selected.get("correlationKind").and_then(Value::as_str),
        Some("coder")
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn full_context_hard_budget_fails_closed_before_provider_send() {
    let _guard = env_test_lock();
    std::env::set_var("TANDEM_FULL_CONTEXT_SOFT_BUDGET_CHARS", "500");
    std::env::set_var("TANDEM_FULL_CONTEXT_HARD_BUDGET_CHARS", "1000");
    std::env::remove_var("TANDEM_FULL_CONTEXT_HARD_BUDGET_OVERRIDE");
    let base = std::env::temp_dir().join(format!("engine-loop-full-hard-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("full hard budget".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    let result = engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "x".repeat(10_000),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: Some(ContextMode::Full),
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await;

    std::env::remove_var("TANDEM_FULL_CONTEXT_SOFT_BUDGET_CHARS");
    std::env::remove_var("TANDEM_FULL_CONTEXT_HARD_BUDGET_CHARS");

    let err = result.expect_err("hard budget should fail closed");
    assert!(err
        .to_string()
        .contains("FULL_CONTEXT_HARD_BUDGET_EXCEEDED"));
    assert!(
        captured.lock().unwrap().is_none(),
        "provider must not be called after hard budget failure"
    );
    let mut saw_exceeded = false;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "context.full.budget.exceeded" {
            saw_exceeded = true;
            assert!(event
                .properties
                .get("topContributors")
                .and_then(Value::as_array)
                .is_some_and(|rows| !rows.is_empty()));
            assert_eq!(
                event
                    .properties
                    .get("overrideApplied")
                    .and_then(Value::as_bool),
                Some(false)
            );
        }
    }
    assert!(saw_exceeded);
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn full_context_soft_budget_warns_without_blocking_send() {
    let _guard = env_test_lock();
    std::env::set_var("TANDEM_FULL_CONTEXT_SOFT_BUDGET_CHARS", "100");
    std::env::remove_var("TANDEM_FULL_CONTEXT_HARD_BUDGET_CHARS");
    std::env::remove_var("TANDEM_FULL_CONTEXT_HARD_BUDGET_OVERRIDE");
    let base = std::env::temp_dir().join(format!("engine-loop-full-soft-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("full soft budget".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    let result = engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "please answer this moderately sized question once".to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: Some(ContextMode::Full),
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await;

    std::env::remove_var("TANDEM_FULL_CONTEXT_SOFT_BUDGET_CHARS");

    result.expect("soft budget should not block the send");
    assert!(
        captured.lock().unwrap().is_some(),
        "provider should still be called after soft budget warning"
    );
    let mut saw_warning = false;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "context.full.budget.warning" {
            saw_warning = true;
            assert!(event
                .properties
                .get("topContributors")
                .and_then(Value::as_array)
                .is_some_and(|rows| !rows.is_empty()));
        }
    }
    assert!(saw_warning);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn autonomous_correlation_kind_classifies_known_prefixes() {
    assert_eq!(
        autonomous_correlation_kind(Some("coder:run:worker")),
        Some("coder")
    );
    assert_eq!(
        autonomous_correlation_kind(Some("workflow:wf-1")),
        Some("workflow")
    );
    assert_eq!(
        autonomous_correlation_kind(Some("routine:run-1")),
        Some("routine")
    );
    assert_eq!(
        autonomous_correlation_kind(Some("automation-v2:run-1")),
        Some("automation")
    );
    assert_eq!(autonomous_correlation_kind(Some("benchmark:x")), None);
    assert_eq!(autonomous_correlation_kind(None), None);
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
    assert!(is_mcp_sandbox_exempt_server("tandem_mcp"));
    assert!(is_mcp_sandbox_exempt_server("tandem-mcp"));
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
    assert!(requires_web_research_prompt(
        "Use web_research and web_fetch to collect current market coverage"
    ));
    assert!(!requires_web_research_prompt(
        "Synthesize the upstream web research artifact into the final report body; do not repeat discovery or fresh web research"
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

#[test]
fn compact_chat_history_pins_decision_messages_and_emits_provenance() {
    let mut messages = Vec::new();
    for i in 0..60 {
        let content = if i == 5 {
            "Approval granted: deploy migration 042 to production".to_string()
        } else {
            format!("message-{i}")
        };
        messages.push(ChatMessage {
            role: "user".to_string(),
            content,
            attachments: Vec::new(),
        });
    }
    let compacted = compact_chat_history(messages, ChatHistoryProfile::Standard);
    let note = &compacted.messages[0];
    assert_eq!(note.role, "system");
    assert!(note.content.contains("history compacted"));
    assert!(note.content.contains("source messages 0-19"));
    assert!(note
        .content
        .contains("1 guardrail/decision messages pinned below"));
    assert!(compacted.messages.iter().any(|m| m
        .content
        .contains("Approval granted: deploy migration 042 to production")
        && m.content.contains("pinned from compacted history")));
    assert_eq!(compacted.pinned_messages, 1);
    assert_eq!(compacted.dropped_messages, 19);
    assert!(compacted
        .messages
        .iter()
        .any(|m| m.content.contains("message-59")));
}

#[tokio::test]
async fn load_chat_history_compacts_large_shell_tool_results_and_keeps_raw_intact() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-tool-result-compaction-{}",
        Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("tool compaction".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let big_output = format!("head-marker\n{}\ntail-marker", "x".repeat(20_000));
    let message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "bash".to_string(),
            args: json!({"command":"cat big.log"}),
            result: Some(json!({"output": big_output.clone()})),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let loaded =
        load_chat_history(storage.clone(), &session_id, ChatHistoryProfile::Standard).await;
    let content = loaded
        .messages
        .iter()
        .find(|message| message.role == "assistant")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("tool output compacted for provider history"));
    assert!(content.contains("head-marker"));
    assert!(content.contains("tail-marker"));
    assert!(
        content.len() < big_output.len() / 2,
        "projection should be much smaller than raw output"
    );
    assert_eq!(loaded.compacted_tool_results, 1);
    assert!(loaded.compacted_tool_result_chars > 10_000);

    // Raw stored tool result must remain unchanged.
    let raw = storage
        .get_session(&session_id)
        .await
        .expect("session still stored");
    let raw_result = raw
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .find_map(|part| match part {
            MessagePart::ToolInvocation { result, .. } => result.clone(),
            _ => None,
        })
        .expect("raw tool result present");
    assert_eq!(
        raw_result.get("output").and_then(Value::as_str),
        Some(big_output.as_str())
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn load_chat_history_caps_unknown_tool_result_shapes() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-tool-result-fallback-{}",
        Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("fallback".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "custom_connector".to_string(),
            args: json!({}),
            result: Some(json!({
                "rows": vec![json!({"payload": "y".repeat(500)}); 40]
            })),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let loaded = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = loaded
        .messages
        .iter()
        .find(|message| message.role == "assistant")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("custom_connector result compacted for chat history"));
    assert!(content.contains("omittedChars"));
    assert!(content.len() < 4_000);
    assert_eq!(loaded.compacted_tool_results, 1);
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn load_chat_history_long_session_emits_provenance_handles_and_preserves_raw() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-long-session-provenance-{}",
        Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("long session".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let mut first_message_id = None;
    for i in 0..50 {
        let parts = if i == 3 {
            vec![MessagePart::Text {
                text: "Approval granted: ship the workflow to staging".to_string(),
            }]
        } else if i % 7 == 0 {
            vec![MessagePart::ToolInvocation {
                tool: "grep".to_string(),
                args: json!({"pattern": format!("needle-{i}")}),
                result: Some(json!({"output": format!("match-{i}\n{}", "z".repeat(3_000))})),
                error: None,
            }]
        } else {
            vec![MessagePart::Text {
                text: format!("turn-{i}: {}", "w".repeat(120)),
            }]
        };
        let message = Message::new(
            if i % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            },
            parts,
        );
        if i == 0 {
            first_message_id = Some(message.id.clone());
        }
        storage
            .append_message(&session_id, message)
            .await
            .expect("append message");
    }

    let loaded =
        load_chat_history(storage.clone(), &session_id, ChatHistoryProfile::Standard).await;
    assert!(loaded.dropped_messages > 0);
    assert!(loaded.messages.len() < 50);
    let note = &loaded.messages[0];
    assert!(note.content.contains("history compacted"));
    assert!(note.content.contains("source messages 0-"));
    assert!(note
        .content
        .contains(first_message_id.as_deref().expect("first id captured")));
    // The human-approval boundary from the dropped prefix survives, pinned.
    assert!(loaded.messages.iter().any(|m| m
        .content
        .contains("Approval granted: ship the workflow to staging")));
    assert_eq!(loaded.pinned_messages, 1);

    // Raw stored history is untouched: all 50 messages with original parts.
    let raw = storage
        .get_session(&session_id)
        .await
        .expect("session still stored");
    assert_eq!(raw.messages.len(), 50);
    assert!(raw.messages.iter().any(|m| m.parts.iter().any(|part| {
        matches!(part, MessagePart::Text { text } if text.contains("Approval granted: ship the workflow to staging"))
    })));
    let _ = std::fs::remove_dir_all(base);
}
