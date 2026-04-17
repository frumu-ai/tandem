
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use std::sync::Arc;

    #[tokio::test]
    async fn todos_are_normalized_to_wire_shape() {
        let base = std::env::temp_dir().join(format!("tandem-core-test-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("test".to_string()), Some(".".to_string()));
        let id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        storage
            .set_todos(
                &id,
                vec![
                    json!({"content":"first"}),
                    json!({"text":"second", "status":"in_progress"}),
                    json!({"id":"keep-id","content":"third","status":"completed"}),
                ],
            )
            .await
            .expect("set todos");

        let todos = storage.get_todos(&id).await;
        assert_eq!(todos.len(), 3);
        for todo in todos {
            assert!(todo.get("id").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("content").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("status").and_then(|v| v.as_str()).is_some());
        }
    }

    #[tokio::test]
    async fn imports_legacy_opencode_session_index_when_sessions_json_missing() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-legacy-import-{}", Uuid::new_v4()));
        let legacy_session_dir = base.join("session").join("global");
        stdfs::create_dir_all(&legacy_session_dir).expect("legacy session dir");
        stdfs::write(
            legacy_session_dir.join("ses_test.json"),
            r#"{
  "id": "ses_test",
  "slug": "test",
  "version": "1.0.0",
  "projectID": "proj_1",
  "directory": "C:\\work\\demo",
  "title": "Legacy Session",
  "time": { "created": 1770913145613, "updated": 1770913146613 }
}"#,
        )
        .expect("legacy session write");

        let storage = Storage::new(&base).await.expect("storage");
        let sessions = storage.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "ses_test");
        assert_eq!(sessions[0].title, "Legacy Session");
        assert!(base.join("sessions.json").exists());
    }

    #[tokio::test]
    async fn imports_legacy_messages_and_parts_for_session() {
        let base = std::env::temp_dir().join(format!("tandem-core-legacy-msg-{}", Uuid::new_v4()));
        let session_dir = base.join("session").join("global");
        let message_dir = base.join("message").join("ses_test");
        let part_dir = base.join("part").join("msg_1");
        stdfs::create_dir_all(&session_dir).expect("session dir");
        stdfs::create_dir_all(&message_dir).expect("message dir");
        stdfs::create_dir_all(&part_dir).expect("part dir");

        stdfs::write(
            session_dir.join("ses_test.json"),
            r#"{
  "id": "ses_test",
  "projectID": "proj_1",
  "directory": "C:\\work\\demo",
  "title": "Legacy Session",
  "time": { "created": 1770913145613, "updated": 1770913146613 }
}"#,
        )
        .expect("write session");

        stdfs::write(
            message_dir.join("msg_1.json"),
            r#"{
  "id": "msg_1",
  "sessionID": "ses_test",
  "role": "assistant",
  "time": { "created": 1770913145613 }
}"#,
        )
        .expect("write msg");

        stdfs::write(
            part_dir.join("prt_1.json"),
            r#"{
  "id": "prt_1",
  "sessionID": "ses_test",
  "messageID": "msg_1",
  "type": "text",
  "text": "hello from legacy"
}"#,
        )
        .expect("write part");

        let storage = Storage::new(&base).await.expect("storage");
        let sessions = storage.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].messages.len(), 1);
        assert_eq!(sessions[0].messages[0].parts.len(), 1);
    }

    #[tokio::test]
    async fn skips_legacy_merge_when_sessions_json_exists() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-legacy-merge-{}", Uuid::new_v4()));
        stdfs::create_dir_all(&base).expect("base");
        stdfs::write(
            base.join("sessions.json"),
            r#"{
  "ses_current": {
    "id": "ses_current",
    "slug": null,
    "version": "v1",
    "project_id": null,
    "title": "Current Session",
    "directory": ".",
    "workspace_root": null,
    "origin_workspace_root": null,
    "attached_from_workspace": null,
    "attached_to_workspace": null,
    "attach_timestamp_ms": null,
    "attach_reason": null,
    "time": {"created":"2026-01-01T00:00:00Z","updated":"2026-01-01T00:00:00Z"},
    "model": null,
    "provider": null,
    "messages": []
  }
}"#,
        )
        .expect("sessions.json");

        let legacy_session_dir = base.join("session").join("global");
        stdfs::create_dir_all(&legacy_session_dir).expect("legacy session dir");
        stdfs::write(
            legacy_session_dir.join("ses_legacy.json"),
            r#"{
  "id": "ses_legacy",
  "slug": "legacy",
  "version": "1.0.0",
  "projectID": "proj_legacy",
  "directory": "C:\\work\\legacy",
  "title": "Legacy Session",
  "time": { "created": 1770913145613, "updated": 1770913146613 }
}"#,
        )
        .expect("legacy session write");

        let storage = Storage::new(&base).await.expect("storage");
        let sessions = storage.list_sessions().await;
        let ids = sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert!(ids.contains(&"ses_current".to_string()));
        assert!(!ids.contains(&"ses_legacy".to_string()));
    }

    #[tokio::test]
    async fn list_sessions_scoped_filters_by_workspace_root() {
        let base = std::env::temp_dir().join(format!("tandem-core-scope-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let ws_a = base.join("ws-a");
        let ws_b = base.join("ws-b");
        stdfs::create_dir_all(&ws_a).expect("ws_a");
        stdfs::create_dir_all(&ws_b).expect("ws_b");
        let ws_a_str = ws_a.to_string_lossy().to_string();
        let ws_b_str = ws_b.to_string_lossy().to_string();

        let mut a = Session::new(Some("a".to_string()), Some(ws_a_str.clone()));
        a.workspace_root = Some(ws_a_str.clone());
        storage.save_session(a).await.expect("save a");

        let mut b = Session::new(Some("b".to_string()), Some(ws_b_str.clone()));
        b.workspace_root = Some(ws_b_str);
        storage.save_session(b).await.expect("save b");

        let scoped = storage
            .list_sessions_scoped(SessionListScope::Workspace {
                workspace_root: ws_a_str,
            })
            .await;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].title, "a");
    }

    #[tokio::test]
    async fn attach_session_persists_audit_metadata() {
        let base = std::env::temp_dir().join(format!("tandem-core-attach-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let ws_a = base.join("ws-a");
        let ws_b = base.join("ws-b");
        stdfs::create_dir_all(&ws_a).expect("ws_a");
        stdfs::create_dir_all(&ws_b).expect("ws_b");
        let ws_a_str = ws_a.to_string_lossy().to_string();
        let ws_b_str = ws_b.to_string_lossy().to_string();
        let mut session = Session::new(Some("s".to_string()), Some(ws_a_str.clone()));
        session.workspace_root = Some(ws_a_str);
        let id = session.id.clone();
        storage.save_session(session).await.expect("save");

        let updated = storage
            .attach_session_to_workspace(&id, &ws_b_str, "manual")
            .await
            .expect("attach")
            .expect("session exists");
        let normalized_expected = normalize_workspace_path(&ws_b_str).expect("normalized path");
        assert_eq!(
            updated.workspace_root.as_deref(),
            Some(normalized_expected.as_str())
        );
        assert_eq!(
            updated.attached_to_workspace.as_deref(),
            Some(normalized_expected.as_str())
        );
        assert_eq!(updated.attach_reason.as_deref(), Some("manual"));
        assert!(updated.attach_timestamp_ms.is_some());
    }

    #[tokio::test]
    async fn append_message_part_persists_tool_invocation_and_result() {
        let base = std::env::temp_dir().join(format!("tandem-core-tool-parts-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("tool parts".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html></html>"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({}),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                result,
                error,
                ..
            } => {
                assert_eq!(tool, "write");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_retains_failed_tool_error() {
        let base = std::env::temp_dir().join(format!("tandem-core-tool-error-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("tool errors".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "write file".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({}),
                    result: None,
                    error: Some("WRITE_CONTENT_MISSING".to_string()),
                },
            )
            .await
            .expect("append error");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        match &message.parts[1] {
            MessagePart::ToolInvocation { error, .. } => {
                assert_eq!(error.as_deref(), Some("WRITE_CONTENT_MISSING"));
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_coalesces_repeated_tool_invocation_updates() {
        let base = std::env::temp_dir().join(format!("tandem-core-tool-merge-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("tool merge".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append first invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html></html>"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append updated invocation");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation { tool, args, .. } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "game.html");
                assert_eq!(args["content"], "<html></html>");
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_upgrades_raw_string_args_to_structured_invocation_args() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-tool-raw-upgrade-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("tool raw upgrade".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!("{\"path\":\"game.html\",\"content\":\"<html>draft</html>\"}"),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append raw invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html>draft</html>"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append structured invocation");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation { tool, args, .. } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "game.html");
                assert_eq!(args["content"], "<html>draft</html>");
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_upgrades_raw_string_args_when_result_arrives_with_structure() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-tool-raw-result-upgrade-{}",
            Uuid::new_v4()
        ));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(
            Some("tool raw result upgrade".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!("{\"path\":\"game.html\",\"content\":\"<html>draft</html>\"}"),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append raw invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html>draft</html>"}),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append structured result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "game.html");
                assert_eq!(args["content"], "<html>draft</html>");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_upgrades_partial_structured_args_when_result_adds_fields() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-tool-structured-result-upgrade-{}",
            Uuid::new_v4()
        ));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(
            Some("tool structured result upgrade".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append partial invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html>draft</html>"}),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append richer result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "game.html");
                assert_eq!(args["content"], "<html>draft</html>");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_replaces_malformed_object_args_with_structured_result_args() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-tool-malformed-args-replace-{}",
            Uuid::new_v4()
        ));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(
            Some("tool malformed args replacement".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"{\"allow_empty": null}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append malformed invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"game.html","content":"<html>draft</html>"}),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append structured result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "game.html");
                assert_eq!(args["content"], "<html>draft</html>");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_replaces_partial_write_args_when_result_adds_path_and_content() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-tool-partial-write-args-replace-{}",
            Uuid::new_v4()
        ));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(
            Some("tool partial write args replacement".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build ui".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"content": ""}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append partial invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":"notes/report.md","content":"# Report\n"}),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append structured result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } => {
                assert_eq!(tool, "write");
                assert_eq!(args["path"], "notes/report.md");
                assert_eq!(args["content"], "# Report\n");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_prefers_executed_write_args_with_context_over_pending_raw_args() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-tool-executed-args-preferred-{}",
            Uuid::new_v4()
        ));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(
            Some("tool executed args preferred".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "build report".to_string(),
            }],
        );
        let message_id = user.id.clone();
        storage
            .append_message(&session_id, user)
            .await
            .expect("append user");

        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({"path":".","content":"draft"}),
                    result: None,
                    error: None,
                },
            )
            .await
            .expect("append raw pending invocation");
        storage
            .append_message_part(
                &session_id,
                &message_id,
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path":".tandem/runs/run-123/artifacts/research-sources.json",
                        "content":"draft",
                        "__workspace_root":"/home/user/marketing-tandem",
                        "__effective_cwd":"/home/user/marketing-tandem"
                    }),
                    result: Some(json!("ok")),
                    error: None,
                },
            )
            .await
            .expect("append executed result");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("message");
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } => {
                assert_eq!(tool, "write");
                assert_eq!(
                    args["path"],
                    ".tandem/runs/run-123/artifacts/research-sources.json"
                );
                assert_eq!(args["content"], "draft");
                assert_eq!(args["__workspace_root"], "/home/user/marketing-tandem");
                assert_eq!(result.as_ref(), Some(&json!("ok")));
                assert_eq!(error.as_deref(), None);
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_message_part_falls_back_to_latest_user_message_when_id_missing() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-tool-fallback-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("tool fallback".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let first = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "first prompt".to_string(),
            }],
        );
        let second = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "second prompt".to_string(),
            }],
        );
        let second_id = second.id.clone();
        storage
            .append_message(&session_id, first)
            .await
            .expect("append first");
        storage
            .append_message(&session_id, second)
            .await
            .expect("append second");

        storage
            .append_message_part(
                &session_id,
                "missing-message-id",
                MessagePart::ToolInvocation {
                    tool: "glob".to_string(),
                    args: json!({"pattern":"*"}),
                    result: Some(json!(["README.md"])),
                    error: None,
                },
            )
            .await
            .expect("append fallback tool part");

        let session = storage.get_session(&session_id).await.expect("session");
        let message = session
            .messages
            .iter()
            .find(|message| message.id == second_id)
            .expect("latest user message");
        match &message.parts[1] {
            MessagePart::ToolInvocation { tool, result, .. } => {
                assert_eq!(tool, "glob");
                assert_eq!(result.as_ref(), Some(&json!(["README.md"])));
            }
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn commit_temp_file_replaces_existing_destination() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-commit-temp-file-{}", Uuid::new_v4()));
        stdfs::create_dir_all(&base).expect("base dir");
        let destination = base.join("sessions.json");
        let temp = base.join("sessions.json.tmp");
        stdfs::write(&destination, "{\"version\":\"old\"}").expect("write destination");
        stdfs::write(&temp, "{\"version\":\"new\"}").expect("write temp");

        commit_temp_file(&temp, &destination)
            .await
            .expect("replace destination");

        let raw = stdfs::read_to_string(&destination).expect("read destination");
        assert_eq!(raw, "{\"version\":\"new\"}");
        assert!(!temp.exists());
    }

    #[tokio::test]
    async fn startup_compacts_session_snapshot_metadata() {
        let base = std::env::temp_dir().join(format!(
            "tandem-core-snapshot-compaction-{}",
            Uuid::new_v4()
        ));
        stdfs::create_dir_all(&base).expect("base dir");

        let mut session = Session::new(
            Some("snapshot compaction".to_string()),
            Some(".".to_string()),
        );
        session.messages.push(Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "current".to_string(),
            }],
        ));
        let session_id = session.id.clone();

        let mut sessions = HashMap::new();
        sessions.insert(session_id.clone(), session);
        stdfs::write(
            base.join("sessions.json"),
            serde_json::to_string_pretty(&sessions).expect("serialize sessions"),
        )
        .expect("write sessions");

        let mut snapshots = Vec::new();
        for label in ["a", "a", "b", "c", "d", "e", "f"] {
            snapshots.push(vec![Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: label.to_string(),
                }],
            )]);
        }
        let mut metadata = HashMap::new();
        metadata.insert(
            session_id.clone(),
            SessionMeta {
                snapshots,
                ..SessionMeta::default()
            },
        );
        metadata.insert("orphan".to_string(), SessionMeta::default());
        stdfs::write(
            base.join("session_meta.json"),
            serde_json::to_string_pretty(&metadata).expect("serialize metadata"),
        )
        .expect("write metadata");
        stdfs::write(base.join("questions.json"), "{}").expect("write questions");

        let _storage = Storage::new(&base).await.expect("storage");

        let raw = stdfs::read_to_string(base.join("session_meta.json")).expect("read metadata");
        let stored: HashMap<String, SessionMeta> =
            serde_json::from_str(&raw).expect("parse metadata");
        assert_eq!(stored.len(), 1);
        let compacted = stored.get(&session_id).expect("session metadata");
        assert_eq!(compacted.snapshots.len(), MAX_SESSION_SNAPSHOTS);

        let labels = compacted
            .snapshots
            .iter()
            .map(|snapshot| {
                snapshot[0]
                    .parts
                    .iter()
                    .find_map(|part| match part {
                        MessagePart::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .expect("snapshot text")
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["b", "c", "d", "e", "f"]);
    }

    #[tokio::test]
    async fn startup_repairs_placeholder_titles_from_wrapped_user_messages() {
        let base =
            std::env::temp_dir().join(format!("tandem-core-title-repair-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let wrapped = "<memory_context>\n<current_session>\n- fact\n</current_session>\n</memory_context>\n\n[Mode instructions]\nUse tools.\n\n[User request]\nExplain this bug";
        let mut session = Session::new(Some("<memory_context>".to_string()), Some(".".to_string()));
        let id = session.id.clone();
        session.messages.push(Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: wrapped.to_string(),
            }],
        ));
        storage.save_session(session).await.expect("save");
        drop(storage);

        let storage = Storage::new(&base).await.expect("storage");
        let repaired = storage.get_session(&id).await.expect("session");
        assert_eq!(repaired.title, "Explain this bug");
    }

    #[tokio::test]
    async fn concurrent_storage_flushes_do_not_fail() {
        let base = std::env::temp_dir().join(format!("tandem-core-flush-race-{}", Uuid::new_v4()));
        let storage = Arc::new(Storage::new(&base).await.expect("storage"));
        let session = Session::new(Some("flush race".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        let mut tasks = Vec::new();
        for task_index in 0..12 {
            let storage = Arc::clone(&storage);
            let session_id = session_id.clone();
            tasks.push(tokio::spawn(async move {
                for part_index in 0..8 {
                    let message = Message::new(
                        MessageRole::User,
                        vec![MessagePart::Text {
                            text: format!("task {task_index} part {part_index}"),
                        }],
                    );
                    storage
                        .append_message(&session_id, message)
                        .await
                        .expect("append message");
                }
            }));
        }

        for task in tasks {
            task.await.expect("join task");
        }

        let session = storage.get_session(&session_id).await.expect("session");
        assert_eq!(session.messages.len(), 12 * 8);
        assert!(base.join("sessions.json").exists());
    }
}
