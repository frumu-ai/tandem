//! Backend conformance suite (TAN-714/TAN-715).
//!
//! Every test here runs the same store operations against each compiled
//! backend: SQLite always, and PostgreSQL when `TANDEM_TEST_POSTGRES_URL`
//! points at a reachable server (mirroring the memory store's
//! `postgres_store::tests` convention). The scenarios are chosen to cover
//! the dialect-sensitive surface: `ON CONFLICT` upsert semantics, null-safe
//! `IS` tenant scoping, `rowid` insertion-order cursors, `INSERT ..
//! RETURNING`, correlated-subquery retention, and the engine lock.

use super::*;
use tandem_automation::{
    GoalLimitAction, GoalPolicy, LongRunningGoalStatus, OrchestrationArtifactRef,
    WorkflowHandoffStatus,
};
use tandem_types::TenantContext;

use crate::stateful_runtime::{
    LegacyRuntimeMigrationPaths, StatefulOutboxRecord, StatefulOutboxStatus,
    StatefulReliabilityStoreFile, StatefulRunEventRecord, StatefulRuntimeScope, StatefulWaitKind,
    StatefulWaitRecord, StatefulWaitStatus,
};

/// Runs `test` once per available backend. The backend name is passed for
/// assertion messages so a Postgres-only failure is immediately attributable.
fn for_each_backend(test: impl Fn(&str, &OrchestrationStateStore)) {
    #[cfg(feature = "storage-sqlite")]
    {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open_with_config(
            store_paths(directory.path()),
            backend::StorageBackendConfig::Sqlite,
        )
        .expect("open sqlite store");
        test("sqlite", &store);
    }
    #[cfg(feature = "storage-postgres")]
    if let Some(url) = postgres_test_url() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open_with_config(
            store_paths(directory.path()),
            backend::StorageBackendConfig::Postgres { url: url.clone() },
        )
        .expect("open postgres store; is TANDEM_TEST_POSTGRES_URL reachable?");
        let schema = match &store.backend {
            StoreBackendSelection::Postgres(target) => target.schema().to_string(),
            #[allow(unreachable_patterns)]
            _ => unreachable!("postgres config selects the postgres backend"),
        };
        test("postgres", &store);
        drop(store);
        backend::postgres::drop_schema_for_tests(&url, &schema)
            .expect("drop conformance test schema");
    }
}

#[cfg(feature = "storage-postgres")]
fn postgres_test_url() -> Option<String> {
    std::env::var("TANDEM_TEST_POSTGRES_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn store_paths(root: &std::path::Path) -> OrchestrationStorePaths {
    OrchestrationStorePaths {
        database_path: root.join("stateful_runtime.sqlite3"),
        engine_lock_path: root.join("stateful_runtime.engine.lock"),
    }
}

fn run(run_id: &str) -> AutomationV2RunRecord {
    serde_json::from_value(serde_json::json!({
        "run_id": run_id,
        "automation_id": "executor",
        "trigger_type": "orchestration_handoff",
        "status": "queued",
        "created_at_ms": 20,
        "updated_at_ms": 20,
        "checkpoint": {}
    }))
    .expect("minimal run fixture")
}

fn goal(run_id: &str) -> LongRunningGoal {
    LongRunningGoal {
        schema_version: 1,
        goal_id: "goal-1".to_string(),
        orchestration_id: "orch-1".to_string(),
        orchestration_version: 3,
        objective: "Plan, execute, and verify".to_string(),
        status: LongRunningGoalStatus::Active,
        tenant_context: TenantContext::local_implicit(),
        policy: GoalPolicy {
            max_hops: 10,
            deadline_at_ms: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
            on_limit: GoalLimitAction::PauseForReview,
        },
        active_run_id: Some(run_id.to_string()),
        current_node_id: Some("execute".to_string()),
        hop_count: 1,
        total_tokens: 0,
        total_cost_usd: 0.0,
        created_at_ms: 1,
        updated_at_ms: 20,
        finished_at_ms: None,
        final_artifact: None,
        metadata: None,
    }
}

fn handoff() -> WorkflowHandoff {
    WorkflowHandoff {
        schema_version: 1,
        handoff_id: "handoff-1".to_string(),
        idempotency_key: "goal-1:plan:continue:1".to_string(),
        goal_id: "goal-1".to_string(),
        orchestration_id: "orch-1".to_string(),
        orchestration_version: 3,
        tenant_context: TenantContext::local_implicit(),
        edge_id: "plan-to-execute".to_string(),
        transition_key: "continue".to_string(),
        source_automation_id: "planner".to_string(),
        source_run_id: "run-1".to_string(),
        source_node_id: "plan".to_string(),
        target_automation_id: "executor".to_string(),
        target_node_id: "execute".to_string(),
        artifact: OrchestrationArtifactRef {
            artifact_type: "plan".to_string(),
            content_path: Some("artifacts/plan.json".to_string()),
            content_digest: Some("sha256:abc".to_string()),
            value: None,
        },
        status: WorkflowHandoffStatus::Approved,
        created_at_ms: 10,
        updated_at_ms: 10,
        consumed_by_run_id: None,
        metadata: None,
    }
}

fn link_for(downstream_run: &AutomationV2RunRecord) -> GoalRunLink {
    GoalRunLink {
        goal_id: "goal-1".to_string(),
        run_id: downstream_run.run_id.clone(),
        orchestration_node_id: "execute".to_string(),
        orchestration_version: 3,
        hop_index: 1,
        parent_run_id: Some("run-1".to_string()),
        triggering_handoff_id: Some("handoff-1".to_string()),
        created_at_ms: 20,
    }
}

fn event() -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: 1,
        event_id: "event-1".to_string(),
        run_id: "goal:goal-1".to_string(),
        seq: 0,
        event_type: "stateful_runtime.goal.transitioned".to_string(),
        occurred_at_ms: 20,
        scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
        actor: None,
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: Some("run-1".to_string()),
        correlation_id: Some("goal-1".to_string()),
        payload: serde_json::json!({"goal_id": "goal-1"}),
    }
}

fn snapshot_record(run_id: &str) -> crate::stateful_runtime::StatefulRunSnapshotRecord {
    crate::stateful_runtime::StatefulRunSnapshotRecord {
        schema_version: 1,
        snapshot_id: "snapshot-1".to_string(),
        run_id: run_id.to_string(),
        seq: 1,
        created_at_ms: 10,
        scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
        status: crate::stateful_runtime::StatefulWorkflowRunStatus::Running,
        phase: crate::stateful_runtime::StatefulWorkflowPhase::RunningPhase,
        phase_history: Vec::new(),
        allowed_next_phases: Vec::new(),
        phase_id: None,
        source_record_kind: None,
        checkpoint: None,
        payload_digest: None,
        workflow_definition_version: None,
        workflow_definition_snapshot_hash: None,
        metadata: None,
    }
}

fn wait(wait_id: &str, updated_at_ms: u64) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: 1,
        wait_id: wait_id.to_string(),
        run_id: "run-1".to_string(),
        wait_kind: StatefulWaitKind::Timer,
        status: StatefulWaitStatus::Waiting,
        scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
        phase_id: None,
        reason: None,
        created_at_ms: updated_at_ms,
        updated_at_ms,
        wake_at_ms: Some(updated_at_ms + 10),
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(serde_json::json!({ "source": "conformance" })),
    }
}

fn outbox(outbox_id: &str, updated_at_ms: u64) -> StatefulOutboxRecord {
    StatefulOutboxRecord {
        schema_version: 1,
        outbox_id: outbox_id.to_string(),
        run_id: Some("run-1".to_string()),
        scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
        operation: "conformance".to_string(),
        status: StatefulOutboxStatus::Pending,
        source_kind: None,
        source_id: None,
        node_id: None,
        provider: None,
        tool: None,
        target: None,
        idempotency_key: None,
        payload_digest: None,
        policy_decision_id: None,
        context_assertion_id: None,
        effect_id: None,
        receipt_id: None,
        compensation_id: None,
        dead_letter_id: None,
        attempts: 0,
        created_at_ms: updated_at_ms,
        updated_at_ms,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: None,
    }
}

#[test]
fn handoff_transition_is_exactly_once_and_tenant_scoped() {
    for_each_backend(|name, store| {
        let downstream_run = run("run-2");
        let link = link_for(&downstream_run);
        assert_eq!(
            store
                .commit_handoff_transition_with_event(
                    &handoff(),
                    &downstream_run,
                    &link,
                    &goal("run-2"),
                    Some(&event()),
                )
                .unwrap(),
            AtomicHandoffCommit::Committed,
            "{name}: first commit"
        );
        assert_eq!(
            store
                .commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
                .unwrap(),
            AtomicHandoffCommit::AlreadyCommitted,
            "{name}: idempotent replay"
        );
        assert_eq!(store.load_automation_runs().unwrap().len(), 1, "{name}");
        assert_eq!(
            store.get_goal("goal-1").unwrap().unwrap().active_run_id,
            Some("run-2".to_string()),
            "{name}"
        );

        let local = TenantContext::local_implicit();
        let foreign = TenantContext::explicit("acme", "hq", None);
        assert!(store
            .get_goal_for_tenant(&local, "goal-1")
            .unwrap()
            .is_some());
        assert!(store
            .get_goal_for_tenant(&foreign, "goal-1")
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .list_goal_handoffs_for_tenant(&local, "goal-1")
                .unwrap()
                .len(),
            1,
            "{name}"
        );
        assert!(store
            .list_goal_handoffs_for_tenant(&foreign, "goal-1")
            .unwrap()
            .is_empty());

        // Durable event cursors must be monotonic per backend (SQLite rowid /
        // PostgreSQL BIGSERIAL) so SSE Last-Event-ID resume works.
        let events = store
            .query_goal_events_for_tenant(&local, "goal-1", None, 10)
            .unwrap();
        assert_eq!(events.len(), 1, "{name}");
        let bounds = store
            .goal_event_cursor_bounds_for_tenant(&local, "goal-1")
            .unwrap()
            .expect("cursor bounds exist");
        assert!(bounds.0 <= bounds.1, "{name}");
        assert!(store
            .query_goal_events_for_tenant(&local, "goal-1", Some(bounds.1), 10)
            .unwrap()
            .is_empty());
        assert!(store
            .query_goal_events_for_tenant(&foreign, "goal-1", None, 10)
            .unwrap()
            .is_empty());
    });
}

#[test]
fn orchestration_specs_publish_and_stay_immutable() {
    for_each_backend(|name, store| {
        let spec: OrchestrationSpec = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "orchestration_id": "orch-1",
            "name": "Plan and finish",
            "status": "published",
            "version": 1,
            "root_node_id": "plan",
            "nodes": [
                {
                    "node_id": "plan",
                    "name": "Plan",
                    "x": 0.0,
                    "y": 0.0,
                    "kind": "workflow",
                    "automation_id": "planner",
                    "pinned_definition_hash": "sha256:planner-v3",
                    "allowed_transition_keys": ["complete"],
                    "emits_artifact_types": ["plan"]
                },
                {
                    "node_id": "complete",
                    "name": "Complete",
                    "x": 200.0,
                    "y": 0.0,
                    "kind": "terminal",
                    "outcome": "complete",
                    "final_artifact_type": "plan"
                }
            ],
            "edges": [{
                "edge_id": "plan-complete",
                "from_node_id": "plan",
                "to_node_id": "complete",
                "transition_key": "complete",
                "artifact_contract": {"artifact_type": "plan", "required": true}
            }],
            "goal_policy": {"max_hops": 3},
            "tenant_context": {
                "org_id": "local",
                "workspace_id": "local",
                "source": "local_implicit"
            },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "published_at_ms": 2
        }))
        .expect("published orchestration fixture");
        store.put_orchestration(&spec).unwrap();
        assert_eq!(
            store.get_orchestration("orch-1", 1).unwrap(),
            Some(spec.clone()),
            "{name}"
        );

        let tenant = TenantContext::local_implicit();
        assert_eq!(
            store
                .latest_published_orchestration_version(&tenant, "orch-1")
                .unwrap(),
            Some(1),
            "{name}"
        );
        // Tenant-scoped listing exercises the null-safe deployment predicate.
        assert_eq!(store.list_orchestration_specs(&tenant).unwrap().len(), 1);

        let mut changed = spec;
        changed.name = "Changed after publish".to_string();
        changed.updated_at_ms += 1;
        assert!(store.put_orchestration(&changed).is_err(), "{name}");
    });
}

#[test]
fn tool_request_ledger_blocks_and_replays() {
    for_each_backend(|name, store| {
        let tenant = TenantContext::local_implicit();
        assert_eq!(
            store
                .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 100)
                .unwrap(),
            None,
            "{name}"
        );
        let error = store
            .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 101)
            .expect_err("a live reservation must block a concurrent replay");
        assert!(error.to_string().contains("still in flight"), "{name}");

        assert_eq!(
            store
                .begin_orchestration_tool_request(
                    &tenant,
                    "publish",
                    "request-1",
                    "digest-1",
                    30_100
                )
                .unwrap(),
            None,
            "{name}: stale lease reclaim"
        );
        let response = serde_json::json!({"version": 3});
        store
            .complete_orchestration_tool_request(
                &tenant,
                "publish",
                "request-1",
                "digest-1",
                &response,
                30_101,
            )
            .unwrap();
        assert_eq!(
            store
                .begin_orchestration_tool_request(
                    &tenant,
                    "publish",
                    "request-1",
                    "digest-1",
                    30_102
                )
                .unwrap(),
            Some(response),
            "{name}"
        );
    });
}

#[test]
fn snapshot_retention_keeps_newest_per_run() {
    for_each_backend(|name, store| {
        for (snapshot_id, seq, created_at_ms) in [
            ("snapshot-old-1", 1_u64, 10_u64),
            ("snapshot-old-2", 2, 20),
            ("snapshot-new", 3, 30),
        ] {
            let mut snapshot = snapshot_record("run-a");
            snapshot.snapshot_id = snapshot_id.to_string();
            snapshot.seq = seq;
            snapshot.created_at_ms = created_at_ms;
            store.put_stateful_runtime_snapshot(&snapshot).unwrap();
        }
        let pruned = store.prune_stateful_runtime_snapshots(1_000, 1).unwrap();
        assert_eq!(
            pruned,
            vec!["snapshot-old-1".to_string(), "snapshot-old-2".to_string()],
            "{name}"
        );
        assert_eq!(
            store.latest_stateful_snapshot_seqs().unwrap().get("run-a"),
            Some(&3),
            "{name}"
        );
    });
}

#[test]
fn legacy_migration_journal_records_attempts_once() {
    for_each_backend(|name, store| {
        let sources = tempfile::tempdir().unwrap();
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(sources.path());
        let report = store.import_legacy_runtime_state(&paths, 100).unwrap();
        assert!(!report.already_complete, "{name}");
        assert!(store.legacy_runtime_migration_complete().unwrap(), "{name}");
        let replay = store.import_legacy_runtime_state(&paths, 200).unwrap();
        assert!(replay.already_complete, "{name}");
        // Exactly one journaled attempt, completed (INSERT .. RETURNING path).
        store
            .with_connection(|connection| {
                let (attempts, completed): (u64, u64) = connection.query_row(
                    "SELECT COUNT(*),
                            COUNT(CASE WHEN outcome = 'complete' THEN 1 END)
                     FROM stateful_migration_attempts",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!((attempts, completed), (1, 1), "{name}");
                Ok(())
            })
            .unwrap();
    });
}

#[test]
fn waits_and_reliability_records_round_trip() {
    for_each_backend(|name, store| {
        store
            .upsert_stateful_runtime_waits(&[wait("wait-2", 20), wait("wait-1", 10)])
            .unwrap();
        let waits = store.load_stateful_runtime_waits().unwrap();
        assert_eq!(waits.len(), 2, "{name}");
        assert_eq!(waits[0].wait_id, "wait-1", "{name}: ordered by update time");

        let reliability = StatefulReliabilityStoreFile {
            schema_version: crate::stateful_runtime::STATEFUL_RUNTIME_SCHEMA_VERSION,
            outbox: vec![outbox("outbox-2", 20), outbox("outbox-1", 10)],
            tool_effects: Vec::new(),
            dead_letters: Vec::new(),
            compensations: Vec::new(),
        };
        store
            .upsert_stateful_runtime_reliability(&reliability)
            .unwrap();
        let loaded = store.load_stateful_runtime_reliability().unwrap();
        assert_eq!(loaded.outbox.len(), 2, "{name}");
        // Insertion-order tie-breaking relies on rowid (SQLite) / BIGSERIAL
        // (PostgreSQL); update-time ordering must hold across backends.
        assert_eq!(loaded.outbox[0].outbox_id, "outbox-1", "{name}");
    });
}

#[test]
fn hot_sync_retains_archived_rows() {
    for_each_backend(|name, store| {
        let archived = run("run-archived");
        let hot = run("run-hot");
        store.sync_hot_automation_runs([&archived, &hot]).unwrap();
        store.sync_hot_automation_runs([&hot]).unwrap();
        let loaded = store.load_automation_runs().unwrap();
        assert_eq!(loaded.len(), 1, "{name}");
        assert_eq!(loaded[0].run_id, "run-hot", "{name}");
        assert!(store.get_automation_run("run-archived").unwrap().is_some());
    });
}

#[test]
fn engine_lock_is_exclusive_per_backend() {
    for_each_backend(|name, store| {
        let first = store.acquire_engine_lock().unwrap();
        assert!(store.acquire_engine_lock().is_err(), "{name}");
        drop(first);
        assert!(store.acquire_engine_lock().is_ok(), "{name}: reacquire");
    });
}
