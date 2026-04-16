use super::*;

#[test]
fn infer_selected_mcp_servers_does_not_select_any_servers_for_wildcard_allowlist() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &[],
        &["*".to_string()],
        &["gmail-main".to_string(), "slack-main".to_string()],
        false,
    );

    assert!(selected.is_empty());
}

#[test]
fn infer_selected_mcp_servers_uses_enabled_servers_for_email_delivery_fallback() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &[],
        &["glob".to_string(), "read".to_string()],
        &["gmail-main".to_string()],
        true,
    );

    assert_eq!(selected, vec!["gmail-main".to_string()]);
}

#[test]
fn infer_selected_mcp_servers_prefers_explicit_selection_when_present() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &["composio-1".to_string()],
        &["*".to_string()],
        &["gmail-main".to_string(), "composio-1".to_string()],
        true,
    );

    assert_eq!(selected, vec!["composio-1".to_string()]);
}

#[test]
fn session_read_paths_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-read-paths-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
    std::fs::write(workspace_root.join("src/lib.rs"), "pub fn demo() {}\n").expect("seed file");

    let mut session = Session::new(
        Some("json string read args".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: json!("{\"path\":\"src/lib.rs\"}"),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let paths = session_read_paths(
        &session,
        workspace_root.to_str().expect("workspace root string"),
    );

    assert_eq!(paths, vec!["src/lib.rs".to_string()]);
}

#[test]
fn session_write_candidates_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-write-candidates-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(
        Some("json string write args".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!("{\"path\":\"brief.md\",\"content\":\"Draft body\"}"),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "brief.md",
        None,
        None,
    );

    assert_eq!(candidates, vec!["Draft body".to_string()]);
}

#[test]
fn session_write_touched_output_detects_target_path_without_content() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-write-touched-output-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(
        Some("write touched output".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "output_path": "brief.md"
            }),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let touched = session_write_touched_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "brief.md",
        None,
        None,
    );

    assert!(
        touched,
        "write invocation should count as touching declared output path"
    );
}

#[test]
fn session_file_mutation_summary_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-mutation-summary-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");

    let mut session = Session::new(
        Some("json string mutation args".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!("{\"path\":\"src/lib.rs\",\"content\":\"pub fn demo() {}\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "apply_patch".to_string(),
                    args: json!("{\"patchText\":\"*** Begin Patch\\n*** Update File: src/other.rs\\n@@\\n-old\\n+new\\n*** End Patch\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

    let summary = session_file_mutation_summary(
        &session,
        workspace_root.to_str().expect("workspace root string"),
    );

    assert_eq!(
        summary
            .get("touched_files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![json!("src/lib.rs"), json!("src/other.rs")]
    );
    assert_eq!(
        summary
            .get("mutation_tool_by_file")
            .and_then(|value| value.get("src/lib.rs"))
            .cloned(),
        Some(json!(["write"]))
    );
    assert_eq!(
        summary
            .get("mutation_tool_by_file")
            .and_then(|value| value.get("src/other.rs"))
            .cloned(),
        Some(json!(["apply_patch"]))
    );
}
