// TAN-637: tool-event capture mode + memory record content cap tests.

#[cfg(test)]
mod tool_event_capture_tests {
    use super::*;
    use crate::test_support::test_state;
    use serial_test::serial;
    use tandem_types::{Message, MessagePart, MessageRole, Session, TenantContext};

    /// Restore an env var to its prior value on drop so serial tests that mutate
    /// `TANDEM_MEMORY_TOOL_EVENT_CAPTURE` never leak into each other.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }

        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn run_ctx(user_id: &str) -> RunMemoryContext {
        RunMemoryContext {
            run_id: "run-tan637".to_string(),
            user_id: user_id.to_string(),
            started_at_ms: 0,
            host_tag: None,
            tenant_context: TenantContext::default(),
            owner_org_unit_id: None,
        }
    }

    async fn seed_session_with_tool_invocation(state: &AppState) -> String {
        let mut session = Session::new(Some("capture".to_string()), Some(".".to_string()));
        session.messages = vec![
            Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: "please run the tool".to_string(),
                }],
            ),
            Message::new(
                MessageRole::Assistant,
                vec![MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({ "command": "ls -la /secret/path" }),
                    result: Some(json!({ "stdout": "total 0\ndrwx secret listing" })),
                    error: None,
                }],
            ),
        ];
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        session_id
    }

    async fn open_memory_db(state: &AppState) -> MemoryDatabase {
        if let Some(parent) = state.memory_db_path.parent() {
            tokio::fs::create_dir_all(parent).await.expect("memory dir");
        }
        MemoryDatabase::new(&state.memory_db_path)
            .await
            .expect("memory db")
    }

    #[test]
    #[serial]
    fn capture_mode_parses_env() {
        let _off = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "off");
        assert_eq!(tool_event_capture_mode(), ToolEventCaptureMode::Off);
        let _full = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", " FULL ");
        assert_eq!(tool_event_capture_mode(), ToolEventCaptureMode::Full);
        let _summary = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "summary");
        assert_eq!(tool_event_capture_mode(), ToolEventCaptureMode::Summary);
        // Unknown / unset both fall through to the summary default.
        let _garbage = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "loud");
        assert_eq!(tool_event_capture_mode(), ToolEventCaptureMode::Summary);
        let _unset = EnvGuard::unset("TANDEM_MEMORY_TOOL_EVENT_CAPTURE");
        assert_eq!(tool_event_capture_mode(), ToolEventCaptureMode::Summary);
    }

    #[tokio::test]
    #[serial]
    async fn summary_mode_writes_single_outcome_record_without_verbatim_payload() {
        let _guard = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "summary");
        let state = test_state().await;
        let session_id = seed_session_with_tool_invocation(&state).await;
        let db = open_memory_db(&state).await;

        ingest_run_messages(&state, &db, &session_id, &run_ctx("user-summary")).await;

        let records = db
            .list_global_memory("user-summary", None, None, None, 50, 0)
            .await
            .expect("list");
        let tool_records: Vec<_> = records
            .iter()
            .filter(|r| r.source_type.starts_with("tool_"))
            .collect();
        assert_eq!(tool_records.len(), 1, "summary keeps exactly one tool record");
        assert_eq!(tool_records[0].source_type, "tool_event");
        assert_eq!(tool_records[0].content, "tool=bash outcome=ok");
        // The noisy verbatim payload must not have leaked into memory.
        assert!(!tool_records[0].content.contains("/secret/path"));
        assert!(!tool_records[0].content.contains("secret listing"));
        // The user/assistant text pathway is unaffected.
        assert!(records.iter().any(|r| r.source_type == "user_message"));
    }

    #[tokio::test]
    #[serial]
    async fn summary_mode_captures_error_outcome() {
        let _guard = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "summary");
        let state = test_state().await;
        let mut session = Session::new(Some("capture".to_string()), Some(".".to_string()));
        session.messages = vec![Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({ "command": "false" }),
                result: None,
                error: Some("command exited with status 1".to_string()),
            }],
        )];
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let db = open_memory_db(&state).await;

        ingest_run_messages(&state, &db, &session_id, &run_ctx("user-error")).await;

        let records = db
            .list_global_memory("user-error", None, None, None, 50, 0)
            .await
            .expect("list");
        let tool_record = records
            .iter()
            .find(|r| r.source_type == "tool_event")
            .expect("tool_event record");
        assert_eq!(
            tool_record.content,
            "tool=bash outcome=error command exited with status 1"
        );
    }

    #[tokio::test]
    #[serial]
    async fn full_mode_keeps_verbatim_input_and_output_records() {
        let _guard = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "full");
        let state = test_state().await;
        let session_id = seed_session_with_tool_invocation(&state).await;
        let db = open_memory_db(&state).await;

        ingest_run_messages(&state, &db, &session_id, &run_ctx("user-full")).await;

        let records = db
            .list_global_memory("user-full", None, None, None, 50, 0)
            .await
            .expect("list");
        assert!(records.iter().any(|r| r.source_type == "tool_input"));
        let output = records
            .iter()
            .find(|r| r.source_type == "tool_output")
            .expect("tool_output record");
        assert!(output.content.contains("secret listing"));
    }

    #[tokio::test]
    #[serial]
    async fn off_mode_drops_tool_records_but_keeps_messages() {
        let _guard = EnvGuard::set("TANDEM_MEMORY_TOOL_EVENT_CAPTURE", "off");
        let state = test_state().await;
        let session_id = seed_session_with_tool_invocation(&state).await;
        let db = open_memory_db(&state).await;

        ingest_run_messages(&state, &db, &session_id, &run_ctx("user-off")).await;

        let records = db
            .list_global_memory("user-off", None, None, None, 50, 0)
            .await
            .expect("list");
        assert!(
            !records.iter().any(|r| r.source_type.starts_with("tool_")),
            "off mode persists no tool records"
        );
        assert!(records.iter().any(|r| r.source_type == "user_message"));
    }

    #[tokio::test]
    #[serial]
    async fn oversized_record_content_is_capped() {
        let _guard = EnvGuard::unset("TANDEM_MEMORY_TOOL_EVENT_CAPTURE");
        let state = test_state().await;
        let db = open_memory_db(&state).await;

        let now = crate::now_ms();
        let huge = "x".repeat(MAX_MEMORY_RECORD_CONTENT_CHARS * 3);
        persist_global_memory_record(
            &state,
            &db,
            GlobalMemoryRecord {
                id: Uuid::new_v4().to_string(),
                user_id: "user-cap".to_string(),
                source_type: "user_message".to_string(),
                content: huge,
                content_hash: String::new(),
                run_id: "run-cap".to_string(),
                session_id: None,
                message_id: None,
                tool_name: None,
                project_tag: None,
                channel_tag: None,
                host_tag: None,
                metadata: None,
                provenance: None,
                redaction_status: "passed".to_string(),
                redaction_count: 0,
                visibility: "private".to_string(),
                demoted: false,
                score_boost: 0.0,
                created_at_ms: now,
                updated_at_ms: now,
                expires_at_ms: None,
            },
        )
        .await;

        let records = db
            .list_global_memory("user-cap", None, None, None, 5, 0)
            .await
            .expect("list");
        assert_eq!(records.len(), 1);
        // Capped to the ceiling plus the truncation marker; well under the raw 3x.
        assert!(records[0].content.len() <= MAX_MEMORY_RECORD_CONTENT_CHARS + 32);
        assert!(records[0].content.ends_with("...<truncated>"));
    }
}
