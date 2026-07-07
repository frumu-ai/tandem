// Tests for the fail-close attribution rule in event memory ingestion
// (TAN-633): events that cannot be attributed to a session's run context must
// not be persisted at all, instead of being filed under a fabricated
// user_id="default" + default tenant scope.
#[cfg(test)]
mod event_ingestion_fail_close_tests {
    use super::*;
    use std::collections::HashMap;

    fn permission_event(session_id: &str) -> EngineEvent {
        EngineEvent {
            event_type: "permission.asked".to_string(),
            properties: json!({
                "sessionID": session_id,
                "tool": "bash",
                "query": "run the test suite",
            }),
            envelope: None,
        }
    }

    fn permission_reply_event(session_id: &str) -> EngineEvent {
        EngineEvent {
            event_type: "permission.replied".to_string(),
            properties: json!({
                "sessionID": session_id,
                "requestID": "perm-1",
                "reply": "allow",
            }),
            envelope: None,
        }
    }

    #[tokio::test]
    async fn event_without_run_context_is_dropped() {
        let state = crate::test_support::test_state().await;
        let db = MemoryDatabase::new(&state.memory_db_path)
            .await
            .expect("open memory db");

        ingest_event_memory_records(
            &state,
            &db,
            &permission_event("sess-unattributed"),
            &HashMap::new(),
        )
        .await;

        let records = db
            .list_global_memory("default", None, None, None, 50, 0)
            .await
            .expect("list records");
        assert!(
            records.is_empty(),
            "unattributable event must not persist memory: {records:?}"
        );
    }

    #[tokio::test]
    async fn event_with_run_context_persists_under_resolved_subject() {
        let state = crate::test_support::test_state().await;
        let db = MemoryDatabase::new(&state.memory_db_path)
            .await
            .expect("open memory db");

        let mut ctx_by_session = HashMap::new();
        ctx_by_session.insert(
            "sess-attributed".to_string(),
            RunMemoryContext {
                run_id: "run-1".to_string(),
                user_id: "user-42".to_string(),
                started_at_ms: 0,
                host_tag: None,
                tenant_context: TenantContext::default(),
                owner_org_unit_id: None,
            },
        );

        ingest_event_memory_records(
            &state,
            &db,
            &permission_event("sess-attributed"),
            &ctx_by_session,
        )
        .await;

        let records = db
            .list_global_memory("user-42", None, None, None, 50, 0)
            .await
            .expect("list records");
        assert_eq!(records.len(), 1, "attributed event should persist once");
        assert_eq!(records[0].user_id, "user-42");
        assert_eq!(records[0].source_type, "approval_request");

        let default_records = db
            .list_global_memory("default", None, None, None, 50, 0)
            .await
            .expect("list default records");
        assert!(
            default_records.is_empty(),
            "nothing may fall through to the catch-all subject"
        );
    }

    #[tokio::test]
    async fn permission_reply_with_run_context_persists_approval_decision() {
        let state = crate::test_support::test_state().await;
        let db = MemoryDatabase::new(&state.memory_db_path)
            .await
            .expect("open memory db");

        let mut ctx_by_session = HashMap::new();
        ctx_by_session.insert(
            "sess-reply".to_string(),
            RunMemoryContext {
                run_id: "run-reply".to_string(),
                user_id: "user-reply".to_string(),
                started_at_ms: 0,
                host_tag: None,
                tenant_context: TenantContext::default(),
                owner_org_unit_id: None,
            },
        );

        ingest_event_memory_records(
            &state,
            &db,
            &permission_reply_event("sess-reply"),
            &ctx_by_session,
        )
        .await;

        let records = db
            .list_global_memory("user-reply", None, None, None, 50, 0)
            .await
            .expect("list records");
        assert_eq!(
            records.len(),
            1,
            "attributed permission replies should persist once"
        );
        assert_eq!(records[0].source_type, "approval_decision");
        assert_eq!(records[0].session_id.as_deref(), Some("sess-reply"));
        assert_eq!(records[0].run_id, "run-reply");
    }
}
