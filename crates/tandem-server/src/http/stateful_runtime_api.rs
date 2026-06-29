use super::*;

use crate::stateful_runtime::{
    list_stateful_run_snapshots as list_snapshot_records, list_stateful_waits,
    query_stateful_run_events, read_stateful_run_snapshot_for_run, stateful_run_from_automation_v2,
    stateful_run_from_workflow, StatefulRunEventQuery, StatefulRuntimeStoragePaths,
    StatefulWaitQuery, StatefulWorkflowRunKind, StatefulWorkflowRunRecord,
};

const DEFAULT_STATEFUL_RUNTIME_LIMIT: usize = 250;
const MAX_STATEFUL_RUNTIME_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunEventsQuery {
    pub after_seq: Option<u64>,
    pub since_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: Option<usize>,
    pub tail: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunSnapshotsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunsQuery {
    pub limit: Option<usize>,
    pub status: Option<String>,
    pub phase: Option<String>,
    pub trigger: Option<String>,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub org_id: Option<String>,
    pub workspace_id: Option<String>,
    pub deployment_id: Option<String>,
    pub workflow_id: Option<String>,
    pub automation_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunDetailQuery {
    pub event_limit: Option<usize>,
    pub snapshot_limit: Option<usize>,
}

pub(super) async fn list_stateful_runs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<StatefulRunsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let mut rows = collect_stateful_runs(&state, &tenant_context).await;
    rows.retain(|run| run_matches_query(run, &query));
    rows.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    rows.truncate(limit);
    let runs = rows
        .into_iter()
        .map(|run| stateful_run_response(&paths, &tenant_context, run, false))
        .collect::<Vec<_>>();
    let count = runs.len();

    Json(json!({
        "runs": runs,
        "count": count,
        "limit": limit,
        "filters": {
            "status": query.status,
            "phase": query.phase,
            "trigger": query.trigger,
            "kind": query.kind.or(query.source),
            "org_id": query.org_id,
            "workspace_id": query.workspace_id,
            "deployment_id": query.deployment_id,
            "workflow_id": query.workflow_id,
            "automation_id": query.automation_id,
        },
        "source": "stateful_runtime",
    }))
}

pub(super) async fn get_stateful_run(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunDetailQuery>,
) -> Response {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let Some(run) = collect_stateful_runs(&state, &tenant_context)
        .await
        .into_iter()
        .find(|run| run.run_id == run_id)
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "stateful_run_not_found",
                "run_id": run_id,
            })),
        )
            .into_response();
    };

    let event_limit = query
        .event_limit
        .unwrap_or(50)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let snapshot_limit = query
        .snapshot_limit
        .unwrap_or(10)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let events = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: None,
            before_seq: None,
            limit: Some(event_limit),
            tail: true,
        },
    );
    let snapshots = list_snapshot_records(
        &paths.snapshots_root,
        &tenant_context,
        &run_id,
        Some(snapshot_limit),
    );
    let mut body = stateful_run_response(&paths, &tenant_context, run, true);
    if let Some(object) = body.as_object_mut() {
        object.insert("events".to_string(), json!(events));
        object.insert("snapshots".to_string(), json!(snapshots));
        object.insert("event_source".to_string(), json!("stateful_runtime"));
        object.insert(
            "event_authority".to_string(),
            json!("authoritative_runtime_log"),
        );
    }

    Json(body).into_response()
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
    let tail = query.tail.is_some();
    let rows = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: query.after_seq.or(query.since_seq),
            before_seq: query.before_seq,
            limit: Some(limit),
            tail,
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
        "event_source": "stateful_runtime",
        "event_authority": "authoritative_runtime_log",
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

async fn collect_stateful_runs(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Vec<StatefulWorkflowRunRecord> {
    let mut by_run_id = HashMap::<String, StatefulWorkflowRunRecord>::new();
    let automation_runs = state.automation_v2_runs.read().await;
    for run in automation_runs
        .values()
        .map(stateful_run_from_automation_v2)
    {
        insert_visible_stateful_run(&mut by_run_id, tenant_context, run);
    }
    drop(automation_runs);

    let workflow_runs = state.workflow_runs.read().await;
    for run in workflow_runs.values().map(stateful_run_from_workflow) {
        insert_visible_stateful_run(&mut by_run_id, tenant_context, run);
    }

    by_run_id.into_values().collect()
}

fn insert_visible_stateful_run(
    by_run_id: &mut HashMap<String, StatefulWorkflowRunRecord>,
    tenant_context: &TenantContext,
    run: StatefulWorkflowRunRecord,
) {
    if !run.scope.visible_to_tenant(tenant_context) {
        return;
    }
    match by_run_id.get(&run.run_id) {
        Some(existing) if existing.updated_at_ms > run.updated_at_ms => {}
        _ => {
            by_run_id.insert(run.run_id.clone(), run);
        }
    }
}

fn stateful_run_response(
    paths: &StatefulRuntimeStoragePaths,
    tenant_context: &TenantContext,
    mut run: StatefulWorkflowRunRecord,
    include_details: bool,
) -> Value {
    let latest_snapshot =
        list_snapshot_records(&paths.snapshots_root, tenant_context, &run.run_id, Some(1))
            .into_iter()
            .next();
    if run.latest_snapshot_id.is_none() {
        run.latest_snapshot_id = latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_id.clone());
    }
    let current_wait = current_wait_for_run(paths, tenant_context, &run.run_id);
    let events = query_stateful_run_events(
        &paths.run_events_path,
        tenant_context,
        StatefulRunEventQuery {
            run_id: &run.run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    );
    let latest_event = events.last().map(stateful_event_summary);
    let first_event_seq = events.first().map(|event| event.seq);
    let latest_event_seq = events.last().map(|event| event.seq);
    let latest_snapshot_summary = latest_snapshot.as_ref().map(stateful_snapshot_summary);
    let replay_boundaries = json!({
        "earliest_event_seq": first_event_seq,
        "latest_event_seq": latest_event_seq,
        "latest_snapshot_id": latest_snapshot.as_ref().map(|snapshot| snapshot.snapshot_id.as_str()),
        "latest_snapshot_seq": latest_snapshot.as_ref().map(|snapshot| snapshot.seq),
        "can_replay_from_event_log": latest_event_seq.is_some(),
        "can_replay_from_snapshot": latest_snapshot.is_some(),
    });

    json!({
        "run": run,
        "current_wait": current_wait,
        "latest_event": latest_event,
        "latest_snapshot": latest_snapshot_summary,
        "replay_boundaries": replay_boundaries,
        "event_source": "stateful_runtime",
        "event_authority": "authoritative_runtime_log",
        "detail_level": if include_details { "detail" } else { "list" },
    })
}

fn current_wait_for_run(
    paths: &StatefulRuntimeStoragePaths,
    tenant_context: &TenantContext,
    run_id: &str,
) -> Option<Value> {
    let waits = list_stateful_waits(
        &paths.waits_path,
        tenant_context,
        StatefulWaitQuery {
            run_id: Some(run_id),
            wait_kind: None,
            status: None,
            limit: None,
        },
    );
    waits
        .iter()
        .find(|wait| !wait.status.is_terminal())
        .or_else(|| waits.last())
        .map(|wait| {
            json!({
                "wait_id": &wait.wait_id,
                "wait_kind": &wait.wait_kind,
                "status": &wait.status,
                "phase_id": &wait.phase_id,
                "reason": &wait.reason,
                "wake_at_ms": wait.wake_at_ms,
                "timeout_policy": &wait.timeout_policy,
                "event_seq": wait.event_seq,
            })
        })
}

fn stateful_event_summary(event: &crate::stateful_runtime::StatefulRunEventRecord) -> Value {
    json!({
        "event_id": &event.event_id,
        "seq": event.seq,
        "event_type": &event.event_type,
        "occurred_at_ms": event.occurred_at_ms,
        "phase_id": &event.phase_id,
        "wait_kind": &event.wait_kind,
        "authoritative": true,
    })
}

fn stateful_snapshot_summary(
    snapshot: &crate::stateful_runtime::StatefulRunSnapshotRecord,
) -> Value {
    json!({
        "snapshot_id": &snapshot.snapshot_id,
        "seq": snapshot.seq,
        "created_at_ms": snapshot.created_at_ms,
        "status": &snapshot.status,
        "phase": &snapshot.phase,
        "phase_id": &snapshot.phase_id,
        "payload_digest": &snapshot.payload_digest,
        "workflow_definition_version": &snapshot.workflow_definition_version,
        "workflow_definition_snapshot_hash": &snapshot.workflow_definition_snapshot_hash,
    })
}

fn run_matches_query(run: &StatefulWorkflowRunRecord, query: &StatefulRunsQuery) -> bool {
    string_filter_matches(query.status.as_deref(), &serialized_key(&run.status))
        && string_filter_matches(query.phase.as_deref(), &serialized_key(&run.phase))
        && string_filter_matches(query.org_id.as_deref(), run.scope.organization_id())
        && string_filter_matches(query.workspace_id.as_deref(), run.scope.workspace_id())
        && option_filter_matches(query.deployment_id.as_deref(), run.scope.deployment_id())
        && option_filter_matches(query.workflow_id.as_deref(), run.workflow_id.as_deref())
        && option_filter_matches(query.automation_id.as_deref(), run.automation_id.as_deref())
        && trigger_filter_matches(run, query.trigger.as_deref())
        && kind_filter_matches(run, query.kind.as_deref().or(query.source.as_deref()))
}

fn trigger_filter_matches(run: &StatefulWorkflowRunRecord, expected: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    [
        run.trigger_type.as_deref(),
        run.trigger_event.as_deref(),
        run.source_event_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| normalize_filter_value(value).contains(&expected))
}

fn kind_filter_matches(run: &StatefulWorkflowRunRecord, expected: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    let kind = serialized_key(&run.kind);
    let source_alias = match run.kind {
        StatefulWorkflowRunKind::AutomationV2 => "automation",
        StatefulWorkflowRunKind::Workflow => "workflow",
        StatefulWorkflowRunKind::ContextRun => "context",
        StatefulWorkflowRunKind::Unknown => "unknown",
    };
    kind == expected || source_alias == expected
}

fn option_filter_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    actual
        .map(|value| normalize_filter_value(value) == expected)
        .unwrap_or(false)
}

fn string_filter_matches(expected: Option<&str>, actual: &str) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    normalize_filter_value(actual) == expected
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    let value = normalize_filter_value(value.unwrap_or_default());
    if value.is_empty() || value == "all" {
        None
    } else {
        Some(value)
    }
}

fn normalize_filter_value(value: &str) -> String {
    value.trim().replace('-', "_").to_ascii_lowercase()
}

fn serialized_key<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{PrincipalKind, PrincipalRef, TenantContext};
    use uuid::Uuid;

    use super::*;
    use crate::automation_v2::types::{
        AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
    };
    use crate::stateful_runtime::{
        append_stateful_run_event, phase_state_from_status, upsert_stateful_wait,
        write_stateful_run_snapshot, StatefulRunEventRecord, StatefulRunSnapshotRecord,
        StatefulRuntimeScope, StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus,
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

    fn automation_run(
        run_id: &str,
        tenant_context: TenantContext,
        status: AutomationRunStatus,
        updated_at_ms: u64,
    ) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: run_id.to_string(),
            automation_id: format!("automation-{run_id}"),
            tenant_context,
            trigger_type: "webhook".to_string(),
            status,
            created_at_ms: 1_000,
            updated_at_ms,
            started_at_ms: Some(1_100),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: vec![format!("context-{run_id}")],
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: Default::default(),
                node_attempts: Default::default(),
                node_attempt_verdicts: Default::default(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile: Default::default(),
            requested_execution_profile: None,
        }
    }

    fn wait(run_id: &str, tenant_context: TenantContext) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-a".to_string(),
            run_id: run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            phase_id: Some("phase-a".to_string()),
            reason: Some("wait for provider callback".to_string()),
            created_at_ms: 1_200,
            updated_at_ms: 1_200,
            wake_at_ms: None,
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: None,
        }
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
                before_seq: None,
                limit: Some(10),
                tail: None,
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

    #[tokio::test]
    async fn run_list_and_detail_use_canonical_stateful_sources() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        state.automation_v2_runs.write().await.insert(
            "run-a".to_string(),
            automation_run(
                "run-a",
                tenant_a.clone(),
                AutomationRunStatus::Running,
                4_000,
            ),
        );
        state.automation_v2_runs.write().await.insert(
            "run-b".to_string(),
            automation_run("run-b", tenant_b, AutomationRunStatus::Failed, 5_000),
        );
        append_stateful_run_event(&paths.run_events_path, &event(1, "run-a", tenant_a.clone()))
            .await
            .expect("append event");
        write_stateful_run_snapshot(
            &paths.snapshots_root,
            &snapshot(1, "run-a", tenant_a.clone()),
        )
        .await
        .expect("write snapshot");
        upsert_stateful_wait(&paths.waits_path, wait("run-a", tenant_a.clone()))
            .await
            .expect("write wait");

        let Json(body) = list_stateful_runs(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Query(StatefulRunsQuery {
                status: Some("running".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                limit: Some(25),
                ..Default::default()
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        let rows = body.get("runs").and_then(Value::as_array).expect("runs");
        assert_eq!(rows[0]["run"]["run_id"], "run-a");
        assert_eq!(rows[0]["current_wait"]["wait_kind"], "webhook");
        assert_eq!(rows[0]["latest_event"]["seq"], 1);
        assert_eq!(rows[0]["latest_snapshot"]["snapshot_id"], "snapshot-1");
        assert_eq!(
            rows[0]["replay_boundaries"]["can_replay_from_snapshot"],
            true
        );

        let response = get_stateful_run(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(StatefulRunDetailQuery {
                event_limit: Some(5),
                snapshot_limit: Some(5),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let hidden = get_stateful_run(
            State(state.clone()),
            Extension(tenant("org-a", "other-workspace")),
            Path("run-a".to_string()),
            Query(StatefulRunDetailQuery::default()),
        )
        .await;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }
}
