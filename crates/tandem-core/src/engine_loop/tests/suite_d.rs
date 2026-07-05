use super::*;
use serde::Deserialize;
use serde_json::Value;
use tandem_tools::approval_classifier::classify;

#[derive(Debug, Deserialize)]
struct ToolInvocationFixture {
    input: String,
    expected_tool: String,
}

fn parse_fixture_invocation(input: &str) -> Vec<(String, Value)> {
    if input.trim_start().starts_with("/tool ") {
        parse_tool_invocation(input).into_iter().collect()
    } else {
        parse_tool_invocations_from_response(input)
    }
}

#[test]
fn tool_invocation_fixture_corpus_round_trips_shared_normalization() {
    let fixtures: Vec<ToolInvocationFixture> =
        serde_json::from_str(include_str!("fixtures/tool_invocation_corpus.json"))
            .expect("fixture corpus is valid JSON");
    assert!(
        fixtures.len() >= 30,
        "TAN-206 fixture corpus must keep at least 30 cases"
    );

    for fixture in fixtures {
        let parsed = parse_fixture_invocation(&fixture.input);
        assert_eq!(
            parsed.len(),
            1,
            "expected one parsed invocation for {:?}, got {parsed:?}",
            fixture.input
        );
        let expected = tandem_types::canonical_tool_name(&fixture.expected_tool);
        assert_eq!(parsed[0].0, expected, "{:?}", fixture.input);
        assert_eq!(
            classify(&fixture.expected_tool),
            classify(&parsed[0].0),
            "classifier drift for {:?}",
            fixture.input
        );
    }
}

#[test]
fn generated_invocation_text_never_panics_or_drifts_from_shared_classification() {
    let names = [
        "read",
        "default_api:read",
        "functions.shell",
        "default_api:run_command",
        "tools.write",
        "tool.edit",
        "builtin:websearch",
        "mcp.linear.list_issues",
        "functions.mcp.linear.create_issue",
        "mcp.github.list_issues",
        "tools.mcp.github.create_issue",
        "todowrite",
        "update_todos",
    ];
    let args = [
        "path=\"README.md\"",
        "{\"path\":\"Cargo.toml\"}",
        "query=\"enterprise mcp\"",
        "command=\"echo hi\"",
        "title=\"Follow up\", description=\"Check state\"",
        "task_id=2, status=\"completed\"",
    ];

    for name in names {
        let expected = tandem_types::canonical_tool_name(name);
        for arg in args {
            let samples = [
                format!("{name}({arg})"),
                format!("Tool call: {name}({arg}) after."),
                format!(
                    r#"{{"name":"{name}","args":{{"path":"README.md","command":"echo hi","query":"enterprise mcp","title":"Follow up","description":"Check state","task_id":2,"status":"completed"}}}}"#
                ),
            ];
            for sample in samples {
                let parsed = parse_tool_invocations_from_response(&sample);
                for (tool, _) in parsed {
                    assert_eq!(tool, expected, "{sample}");
                    assert_eq!(classify(name), classify(&tool), "{sample}");
                }
            }
        }
    }

    let mut seed = 0x5eed_u64;
    for _ in 0..512 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let name = random_ascii_fragment(seed);
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let arg = random_ascii_fragment(seed);
        let sample = format!("{name}({arg})");
        for (tool, _) in parse_tool_invocations_from_response(&sample) {
            let canonical = tandem_types::canonical_tool_name(&tool);
            assert_eq!(tool, canonical, "{sample}");
            assert_eq!(classify(&tool), classify(&canonical), "{sample}");
        }
    }
}

#[test]
fn function_style_parser_ignores_prose_and_fenced_code() {
    let prose = r#"
The helper read(path: &str) should stay in the final answer.

```rust
fn read(path: &str) -> String {
    path.to_string()
}
```
"#;
    assert!(parse_tool_invocations_from_response(prose).is_empty());

    let explicit = r#"Tool call: read(path="README.md")"#;
    let parsed = parse_tool_invocation_from_response(explicit).expect("explicit tool call");
    assert_eq!(parsed.0, "read");
}

fn random_ascii_fragment(mut seed: u64) -> String {
    const ALPHABET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-. :=,{}[]\"";
    let len = (seed % 48 + 1) as usize;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        seed = seed
            .wrapping_mul(2862933555777941757)
            .wrapping_add(3037000493);
        let idx = (seed % ALPHABET.len() as u64) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

#[tokio::test]
async fn load_chat_history_demotes_stale_tool_invocations_with_provenance() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-demote-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    // 12 tool invocations: with the default keep-recent window of 8, the
    // oldest 4 must be demoted to concise summaries with provenance handles.
    let mut message_ids = Vec::new();
    for index in 0..12 {
        let payload = format!("PAYLOAD_{index}_{}", "x".repeat(5_000));
        let message = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "grep".to_string(),
                args: json!({"pattern": format!("needle_{index}"), "path": "src/"}),
                result: Some(json!({"output": payload})),
                error: None,
            }],
        );
        message_ids.push(message.id.clone());
        storage
            .append_message(&session_id, message)
            .await
            .expect("append message");
    }

    let history =
        load_chat_history(storage.clone(), &session_id, ChatHistoryProfile::Standard).await;
    assert_eq!(history.demoted_tool_invocations, 4);
    assert_eq!(history.dropped_messages, 0);
    assert!(
        history.demoted_tool_invocation_chars > 18_000,
        "expected large savings, got {}",
        history.demoted_tool_invocation_chars
    );

    let contents = history
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>();
    let demoted_lines = contents
        .iter()
        .filter(|content| content.contains("result=[stale;"))
        .collect::<Vec<_>>();
    assert_eq!(demoted_lines.len(), 4);

    for (index, message_id) in message_ids.iter().take(4).enumerate() {
        let line = contents
            .iter()
            .find(|content| content.contains(&format!("needle_{index}")))
            .expect("demoted invocation projected");
        // Provenance handles: source message id, original tool and args
        // preview, and follow-up retrieval instructions.
        assert!(line.contains("Tool grep"), "tool name kept: {line}");
        assert!(line.contains(message_id), "message id handle kept: {line}");
        assert!(
            line.contains("re-run grep with the original arguments"),
            "retrieval instructions kept: {line}"
        );
        assert!(line.contains("status=ok"));
        assert!(
            !line.contains(&format!("PAYLOAD_{index}_")),
            "stale payload must not reach provider history: {line}"
        );
        assert!(
            line.len() < 500,
            "demoted line must be concise, got {} chars",
            line.len()
        );
    }

    // The 8 most recent invocations keep their (compacted) payload
    // projection so in-flight work is unaffected.
    for index in 4..12 {
        let line = contents
            .iter()
            .find(|content| content.contains(&format!("needle_{index}")))
            .expect("recent invocation projected");
        assert!(
            line.contains(&format!("PAYLOAD_{index}_")),
            "recent result content retained: {line}"
        );
        assert!(!line.contains("result=[stale;"));
    }
}

#[tokio::test]
async fn stale_tool_invocation_demotion_preserves_errors() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-demote-error-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    // First (stale-destined) invocation failed; its error must stay visible.
    let failed = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "bash".to_string(),
            args: json!({"command": "cargo test"}),
            result: Some(json!({"output": "y".repeat(4_000)})),
            error: Some("EXIT_CODE_101: test suite failed".to_string()),
        }],
    );
    storage
        .append_message(&session_id, failed)
        .await
        .expect("append failed invocation");
    for index in 0..8 {
        let message = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path": format!("src/file_{index}.rs")}),
                result: Some(json!({"output": format!("fn f{index}() {{}}")})),
                error: None,
            }],
        );
        storage
            .append_message(&session_id, message)
            .await
            .expect("append message");
    }

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    assert_eq!(history.demoted_tool_invocations, 1);
    let demoted = history
        .messages
        .iter()
        .find(|message| message.content.contains("result=[stale;"))
        .map(|message| message.content.clone())
        .expect("demoted failed invocation");
    assert!(demoted.contains("error=EXIT_CODE_101: test suite failed"));
    assert!(!demoted.contains("status=ok"));
    assert!(!demoted.contains(&"y".repeat(200)));
}

#[tokio::test]
async fn full_context_profile_never_demotes_tool_invocations() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-full-no-demote-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    for index in 0..12 {
        let message = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "grep".to_string(),
                args: json!({"pattern": format!("needle_{index}")}),
                result: Some(json!({"output": format!("PAYLOAD_{index}_match")})),
                error: None,
            }],
        );
        storage
            .append_message(&session_id, message)
            .await
            .expect("append message");
    }

    // Full mode's contract is "everything preserved"; demotion must not run.
    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Full).await;
    assert_eq!(history.demoted_tool_invocations, 0);
    assert!(history
        .messages
        .iter()
        .all(|message| !message.content.contains("result=[stale;")));
}

#[tokio::test]
async fn stale_write_demotion_keeps_target_path_visible() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-demote-path-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    // serde_json orders object keys alphabetically, so `content` serializes
    // before `path`; the preview must still surface the target path.
    let stale_write = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "content": "z".repeat(5_000),
                "path": "src/pages/Dashboard.tsx",
            }),
            result: Some(json!({"output": "ok"})),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, stale_write)
        .await
        .expect("append write");
    for index in 0..8 {
        let message = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path": format!("src/file_{index}.rs")}),
                result: Some(json!({"output": format!("fn f{index}() {{}}")})),
                error: None,
            }],
        );
        storage
            .append_message(&session_id, message)
            .await
            .expect("append message");
    }

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    assert_eq!(history.demoted_tool_invocations, 1);
    let demoted = history
        .messages
        .iter()
        .find(|message| message.content.contains("result=[stale;"))
        .map(|message| message.content.clone())
        .expect("demoted write invocation");
    assert!(
        demoted.contains("path=src/pages/Dashboard.tsx"),
        "target path must survive the args preview: {demoted}"
    );
    assert!(!demoted.contains(&"z".repeat(200)));
}
