// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn runtime_event(event_type: &str, properties: Value, seq: u64) -> EngineEvent {
    let envelope = tandem_types::RuntimeEventEnvelope::derive(seq, 1_000 + seq, &properties);
    EngineEvent::new(event_type, properties).with_envelope(envelope)
}

#[tokio::test]
async fn hosted_routine_workers_wait_through_initial_policy_delay() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    let pending = wait_for_runtime_ready_or_exit(&state, "delayed-policy-test");
    tokio::pin!(pending);
    // Exceed the production 120 * 250ms startup window. Missing policy is
    // recoverable and must not permanently discard a healthy worker.
    assert!(
        tokio::time::timeout(Duration::from_secs(35), pending.as_mut())
            .await
            .is_err()
    );
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "policy_version": 1,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::Utc::now(), "users": [], "org_units": [],
            "org_unit_memberships": [], "deployment_grants": []
        }))
        .unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    state.reload_hosted_policy().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap());
}

#[tokio::test]
async fn hosted_routine_worker_wait_preserves_startup_and_shutdown_exits() {
    let starting = AppState::new_starting("worker-startup-budget".into(), true);
    assert!(!starting.wait_for_worker_ready_or_failed(1, 0).await);
    let ready = crate::test_support::test_state().await;
    assert!(ready.wait_for_worker_ready_or_failed(0, 0).await);
    for shutdown in [false, true] {
        let state = crate::test_support::test_state().await;
        let temp = tempfile::tempdir().unwrap();
        state.enterprise.hosted_policy.configure_test_source(
            "org-a",
            "dep-a",
            temp.path().join("missing.json"),
        );
        // The separately exposed bounded readiness API retains its timeout.
        assert!(!state.wait_until_ready_or_failed(0, 0).await);
        let waiting = state.wait_for_worker_ready_or_failed(0, 1);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), waiting.as_mut())
                .await
                .is_err()
        );
        if shutdown {
            state.set_automation_scheduler_stopping(true);
        } else {
            state.mark_failed("test_failed", "startup failed").await;
        }
        assert!(!tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap());
    }
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
    let started_row =
        enrich_runtime_event_log_row_from_session(started_row, Some(&session), &mut context_cache);
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

    let provider_row =
        enrich_runtime_event_log_row_from_session(provider_row, Some(&session), &mut context_cache);
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
