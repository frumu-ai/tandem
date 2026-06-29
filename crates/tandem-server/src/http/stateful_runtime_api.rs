use super::*;

use crate::stateful_runtime::{
    list_stateful_run_snapshots as list_snapshot_records, query_stateful_run_events,
    read_stateful_run_snapshot_for_run, StatefulRunEventQuery, StatefulRuntimeStoragePaths,
};

const DEFAULT_STATEFUL_RUNTIME_LIMIT: usize = 250;
const MAX_STATEFUL_RUNTIME_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunEventsQuery {
    pub after_seq: Option<u64>,
    pub since_seq: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunSnapshotsQuery {
    pub limit: Option<usize>,
}

pub(super) async fn get_stateful_run_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunEventsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let rows = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: query.after_seq.or(query.since_seq),
            limit: Some(limit),
        },
    );
    let last_seq = rows.last().map(|row| row.seq);
    let count = rows.len();

    Json(json!({
        "run_id": run_id,
        "events": rows,
        "count": count,
        "last_seq": last_seq,
        "limit": limit,
        "sequence_scope": "stateful_runtime",
    }))
}

pub(super) async fn list_stateful_run_snapshots(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunSnapshotsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let snapshots =
        list_snapshot_records(&paths.snapshots_root, &tenant_context, &run_id, Some(limit));
    let latest_seq = snapshots.last().map(|snapshot| snapshot.seq);
    let count = snapshots.len();

    Json(json!({
        "run_id": run_id,
        "snapshots": snapshots,
        "count": count,
        "latest_seq": latest_seq,
        "limit": limit,
    }))
}

pub(super) async fn get_stateful_run_snapshot(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path((run_id, snapshot_id)): Path<(String, String)>,
) -> Response {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    match read_stateful_run_snapshot_for_run(
        &paths.snapshots_root,
        &tenant_context,
        &run_id,
        &snapshot_id,
    ) {
        Ok(Some(snapshot)) => Json(json!({ "snapshot": snapshot })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "stateful_snapshot_not_found",
                "run_id": run_id,
                "snapshot_id": snapshot_id,
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "stateful_snapshot_read_failed",
                "message": error.to_string(),
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{PrincipalKind, PrincipalRef, TenantContext};
    use uuid::Uuid;

    use super::*;
    use crate::stateful_runtime::{
        append_stateful_run_event, phase_state_from_status, write_stateful_run_snapshot,
        StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulRuntimeScope,
        StatefulWorkflowRunStatus,
    };

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn event(seq: u64, run_id: &str, tenant_context: TenantContext) -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("evt-{seq}"),
            run_id: run_id.to_string(),
            seq,
            event_type: "workflow.phase.changed".to_string(),
            occurred_at_ms: 1_000 + seq,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            actor: Some(PrincipalRef::new(PrincipalKind::Automation, "automation-a")),
            phase_id: Some("phase-a".to_string()),
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({ "seq": seq }),
        }
    }

    fn snapshot(
        seq: u64,
        run_id: &str,
        tenant_context: TenantContext,
    ) -> StatefulRunSnapshotRecord {
        let status = StatefulWorkflowRunStatus::Running;
        let phase_state = phase_state_from_status(run_id, &status, 2_000 + seq, Some("phase-a"));
        StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: format!("snapshot-{seq}"),
            run_id: run_id.to_string(),
            seq,
            created_at_ms: 2_000 + seq,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            status,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-a".to_string()),
            source_record_kind: None,
            checkpoint: Some(json!({ "seq": seq })),
            payload_digest: Some(format!("sha256:{seq}")),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        }
    }

    async fn stateful_test_state() -> AppState {
        let mut state = crate::test_support::test_state().await;
        let root = std::env::temp_dir().join(format!("stateful-runtime-api-{}", Uuid::new_v4()));
        state.runtime_events_path = root.join("events.jsonl");
        state
    }

    #[tokio::test]
    async fn get_events_filters_by_tenant_and_sequence() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for record in [
            event(1, "run-a", tenant_a.clone()),
            event(2, "run-a", tenant_b),
            event(3, "run-a", tenant_a.clone()),
        ] {
            append_stateful_run_event(&paths.run_events_path, &record)
                .await
                .expect("append event");
        }

        let Json(body) = get_stateful_run_events(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(StatefulRunEventsQuery {
                after_seq: Some(1),
                since_seq: None,
                limit: Some(10),
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(body.get("last_seq").and_then(Value::as_u64), Some(3));
        assert_eq!(
            body.get("sequence_scope").and_then(Value::as_str),
            Some("stateful_runtime")
        );
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }

    #[tokio::test]
    async fn snapshot_endpoints_filter_by_tenant() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for record in [
            snapshot(1, "run-a", tenant_a.clone()),
            snapshot(2, "run-a", tenant_b),
            snapshot(3, "run-a", tenant_a.clone()),
        ] {
            write_stateful_run_snapshot(&paths.snapshots_root, &record)
                .await
                .expect("write snapshot");
        }

        let Json(body) = list_stateful_run_snapshots(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Path("run-a".to_string()),
            Query(StatefulRunSnapshotsQuery { limit: Some(10) }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(2));
        assert_eq!(body.get("latest_seq").and_then(Value::as_u64), Some(3));

        let response = get_stateful_run_snapshot(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Path(("run-a".to_string(), "snapshot-3".to_string())),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let hidden = get_stateful_run_snapshot(
            State(state.clone()),
            Extension(tenant_a),
            Path(("run-a".to_string(), "snapshot-2".to_string())),
        )
        .await;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let _ = tokio::fs::remove_dir_all(&paths.snapshots_root).await;
    }
}
