// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use crate::runtime_event_log::{query_runtime_event_log_window, RuntimeEventLogWindowQuery};

const DEFAULT_RUNTIME_EVENTS_LIMIT: usize = 250;
const MAX_RUNTIME_EVENTS_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct RuntimeEventsQuery {
    pub after_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub since_seq: Option<u64>,
    pub limit: Option<usize>,
    pub tail: Option<usize>,
}

pub(super) async fn get_run_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<RuntimeEventsQuery>,
) -> Json<Value> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_RUNTIME_EVENTS_LIMIT)
        .clamp(1, MAX_RUNTIME_EVENTS_LIMIT);
    let tail = query
        .tail
        .filter(|tail| *tail > 0)
        .map(|tail| tail.clamp(1, MAX_RUNTIME_EVENTS_LIMIT));
    let rows = query_runtime_event_log_window(
        &state.runtime_events_path,
        &tenant_context,
        RuntimeEventLogWindowQuery {
            run_id: &run_id,
            after_seq: query.after_seq.or(query.since_seq),
            before_seq: query.before_seq,
            limit: tail.map(|_| None).unwrap_or(Some(limit)),
            tail,
        },
    );
    let first_seq = rows.first().map(|row| row.seq());
    let last_seq = rows.last().map(|row| row.seq());
    let events = rows
        .iter()
        .map(|row| serde_json::to_value(row).unwrap_or(Value::Null))
        .collect::<Vec<_>>();

    Json(json!({
        "run_id": run_id,
        "events": events,
        "count": events.len(),
        "first_seq": first_seq,
        "last_seq": last_seq,
        "limit": limit,
        "tail": tail,
        "sequence_scope": "runtime_event_bus",
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{EngineEvent, RuntimeEventEnvelope};
    use uuid::Uuid;

    use super::*;
    use crate::runtime_event_log::{append_runtime_event_log_row, RuntimeEventLogRow};

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn event(seq: u64, run_id: &str, tenant_context: TenantContext) -> EngineEvent {
        EngineEvent::new(
            "session.run.started",
            json!({
                "runID": run_id,
                "sessionID": "session-a",
                "tenantContext": tenant_context,
            }),
        )
        .with_envelope(RuntimeEventEnvelope {
            event_id: format!("evt-{seq}"),
            seq,
            schema_version: 1,
            occurred_at_ms: 1_000 + seq,
            session_id: Some("session-a".to_string()),
            run_id: Some(run_id.to_string()),
            node_id: None,
            tenant_context: Some(tenant_context),
        })
    }

    #[tokio::test]
    async fn get_run_events_filters_by_tenant_and_sequence() {
        let mut state = crate::test_support::test_state().await;
        state.runtime_events_path =
            std::env::temp_dir().join(format!("runtime-events-api-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");

        for event in [
            event(1, "run-a", tenant_a.clone()),
            event(2, "run-a", tenant_b),
            event(3, "run-a", tenant_a.clone()),
            event(4, "run-b", tenant_a.clone()),
        ] {
            let row = RuntimeEventLogRow::from_engine_event(&event).expect("runtime row");
            append_runtime_event_log_row(&state.runtime_events_path, &row)
                .await
                .expect("append row");
        }

        let Json(body) = get_run_events(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(RuntimeEventsQuery {
                after_seq: Some(1),
                before_seq: None,
                since_seq: None,
                limit: Some(10),
                tail: None,
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(body.get("last_seq").and_then(Value::as_u64), Some(3));
        let events = body
            .get("events")
            .and_then(Value::as_array)
            .expect("events array");
        assert_eq!(
            events[0].get("event_type").and_then(Value::as_str),
            Some("session.run.started")
        );

        let _ = tokio::fs::remove_file(state.runtime_events_path).await;
    }

    #[tokio::test]
    async fn get_run_events_supports_tail_and_previous_page_cursor() {
        let mut state = crate::test_support::test_state().await;
        state.runtime_events_path =
            std::env::temp_dir().join(format!("runtime-events-tail-api-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");

        for seq in 1..=5 {
            let row = RuntimeEventLogRow::from_engine_event(&event(seq, "run-a", tenant_a.clone()))
                .expect("runtime row");
            append_runtime_event_log_row(&state.runtime_events_path, &row)
                .await
                .expect("append row");
        }

        let Json(tail_body) = get_run_events(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Path("run-a".to_string()),
            Query(RuntimeEventsQuery {
                after_seq: None,
                before_seq: None,
                since_seq: None,
                limit: Some(2),
                tail: Some(2),
            }),
        )
        .await;
        assert_eq!(tail_body.get("first_seq").and_then(Value::as_u64), Some(4));
        assert_eq!(tail_body.get("last_seq").and_then(Value::as_u64), Some(5));

        let Json(previous_body) = get_run_events(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(RuntimeEventsQuery {
                after_seq: None,
                before_seq: Some(4),
                since_seq: None,
                limit: Some(2),
                tail: Some(2),
            }),
        )
        .await;
        assert_eq!(
            previous_body.get("first_seq").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            previous_body.get("last_seq").and_then(Value::as_u64),
            Some(3)
        );

        let _ = tokio::fs::remove_file(state.runtime_events_path).await;
    }
}
