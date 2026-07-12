use super::*;
use tandem_automation::{
    GoalLimitAction, GoalPolicy, LongRunningGoalStatus, OrchestrationArtifactRef,
    WorkflowHandoffStatus,
};
use tandem_types::TenantContext;

use crate::stateful_runtime::{StatefulRunEventRecord, StatefulRuntimeScope};

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
        payload: serde_json::json!({"handoff_id": "handoff-1"}),
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

fn assert_transition_record_count(store: &OrchestrationStateStore, expected: u64) {
    store
        .with_connection(|connection| {
            for table in [
                "workflow_handoffs",
                "automation_runs",
                "goal_run_links",
                "long_running_goals",
                "stateful_events",
            ] {
                let count: u64 =
                    connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(count, expected, "unexpected {table} record count");
            }
            Ok(())
        })
        .unwrap();
}

fn published_spec() -> OrchestrationSpec {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "orchestration_id": "orch-1",
        "name": "Plan and finish",
        "status": "published",
        "version": 3,
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
    .expect("published orchestration fixture")
}

#[test]
fn atomic_handoff_commit_is_exactly_once() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let downstream_run = run("run-2");
    let link = GoalRunLink {
        goal_id: "goal-1".to_string(),
        run_id: downstream_run.run_id.clone(),
        orchestration_node_id: "execute".to_string(),
        orchestration_version: 3,
        hop_index: 1,
        parent_run_id: Some("run-1".to_string()),
        triggering_handoff_id: Some("handoff-1".to_string()),
        created_at_ms: 20,
    };

    assert_eq!(
        store
            .commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
            .unwrap(),
        AtomicHandoffCommit::Committed
    );
    assert_eq!(
        store
            .commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
            .unwrap(),
        AtomicHandoffCommit::AlreadyCommitted
    );
    assert_eq!(store.load_automation_runs().unwrap().len(), 1);
    assert_eq!(
        store.get_goal("goal-1").unwrap().unwrap().active_run_id,
        Some("run-2".to_string())
    );
    let mut cross_tenant_run = downstream_run;
    cross_tenant_run.tenant_context = TenantContext::explicit("other", "other", None);
    assert!(store
        .commit_handoff_transition(&handoff(), &cross_tenant_run, &link, &goal("run-2"))
        .is_err());
}

/// TAN-705: cross-tenant access is absence at the store layer itself —
/// every scoped read runs its org/workspace/deployment predicate in SQL,
/// so no future caller can leak records by skipping an entrypoint check.
#[test]
fn tenant_scoped_reads_fail_closed_at_the_store_layer() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let downstream_run = run("run-2");
    let link = GoalRunLink {
        goal_id: "goal-1".to_string(),
        run_id: downstream_run.run_id.clone(),
        orchestration_node_id: "execute".to_string(),
        orchestration_version: 3,
        hop_index: 1,
        parent_run_id: Some("run-1".to_string()),
        triggering_handoff_id: Some("handoff-1".to_string()),
        created_at_ms: 20,
    };
    let mut transition_event = event();
    transition_event.payload = serde_json::json!({"goal_id": "goal-1"});
    store
        .commit_handoff_transition_with_event(
            &handoff(),
            &downstream_run,
            &link,
            &goal("run-2"),
            Some(&transition_event),
        )
        .unwrap();

    let local = TenantContext::local_implicit();
    assert!(store
        .get_goal_for_tenant(&local, "goal-1")
        .unwrap()
        .is_some());
    assert_eq!(
        store
            .list_goal_run_links_for_tenant(&local, "goal-1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .list_goal_handoffs_for_tenant(&local, "goal-1")
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .get_workflow_handoff_for_tenant(&local, "handoff-1")
        .unwrap()
        .is_some());
    assert!(!store
        .query_goal_events_for_tenant(&local, "goal-1", None, 10)
        .unwrap()
        .is_empty());

    let foreign = TenantContext::explicit("acme", "hq", None);
    assert!(store
        .get_goal_for_tenant(&foreign, "goal-1")
        .unwrap()
        .is_none());
    assert!(store
        .list_goal_run_links_for_tenant(&foreign, "goal-1")
        .unwrap()
        .is_empty());
    assert!(store
        .list_goal_handoffs_for_tenant(&foreign, "goal-1")
        .unwrap()
        .is_empty());
    assert!(store
        .get_workflow_handoff_for_tenant(&foreign, "handoff-1")
        .unwrap()
        .is_none());
    assert!(store
        .query_goal_events_for_tenant(&foreign, "goal-1", None, 10)
        .unwrap()
        .is_empty());
}

/// TAN-705/TAN-675: the MCP tool-replay ledger seals its stored responses
/// with the scoped crypto provider — ciphertext at rest, identical replay
/// through decryption, and fail-closed reads without the key.
#[tokio::test]
async fn tool_replay_ledger_round_trips_encrypted_records() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let tenant = TenantContext::explicit("acme", "hq", None);
    let response = serde_json::json!({"orchestration": {"objective": "confidential"}});

    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x42; 32]),
        None,
        async {
            assert!(store
                .begin_orchestration_tool_request(
                    &tenant,
                    "orchestration_publish",
                    "key-1",
                    "digest-1",
                    10
                )
                .unwrap()
                .is_none());
            store
                .complete_orchestration_tool_request(
                    &tenant,
                    "orchestration_publish",
                    "key-1",
                    "digest-1",
                    &response,
                    11,
                )
                .unwrap();
            store
                .with_connection(|connection| {
                    let stored: String = connection.query_row(
                        "SELECT response_json FROM orchestration_tool_requests",
                        [],
                        |row| row.get(0),
                    )?;
                    assert!(
                        stored.starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX),
                        "replay ledger row must be ciphertext at rest"
                    );
                    assert!(!stored.contains("confidential"));
                    Ok(())
                })
                .unwrap();
            let replayed = store
                .begin_orchestration_tool_request(
                    &tenant,
                    "orchestration_publish",
                    "key-1",
                    "digest-1",
                    12,
                )
                .unwrap();
            assert_eq!(replayed, Some(response.clone()));
            let completed = store
                .completed_orchestration_tool_request(
                    &tenant,
                    "orchestration_publish",
                    "key-1",
                    "digest-1",
                )
                .unwrap();
            assert_eq!(completed, Some(response.clone()));

            assert!(store
                .begin_orchestration_action_request(
                    &tenant,
                    "goal_action:goal-1:pause",
                    "key-2",
                    "digest-2",
                    13,
                )
                .unwrap()
                .is_none());
            store
                .complete_orchestration_tool_request(
                    &tenant,
                    "goal_action:goal-1:pause",
                    "key-2",
                    "digest-2",
                    &response,
                    14,
                )
                .unwrap();
            let action_replayed = store
                .begin_orchestration_action_request(
                    &tenant,
                    "goal_action:goal-1:pause",
                    "key-2",
                    "digest-2",
                    15,
                )
                .unwrap();
            assert_eq!(action_replayed, Some(response.clone()));
        },
    )
    .await;

    // A different key cannot read the sealed record: fail closed.
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x43; 32]),
        None,
        async {
            assert!(store
                .begin_orchestration_tool_request(
                    &tenant,
                    "orchestration_publish",
                    "key-1",
                    "digest-1",
                    13
                )
                .is_err());
        },
    )
    .await;
}

#[test]
fn atomic_handoff_commit_rolls_back_every_write_boundary() {
    for table in [
        "workflow_handoffs",
        "automation_runs",
        "goal_run_links",
        "long_running_goals",
        "stateful_events",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        let downstream_run = run("run-2");
        let link = GoalRunLink {
            goal_id: "goal-1".to_string(),
            run_id: downstream_run.run_id.clone(),
            orchestration_node_id: "execute".to_string(),
            orchestration_version: 3,
            hop_index: 1,
            parent_run_id: Some("run-1".to_string()),
            triggering_handoff_id: Some("handoff-1".to_string()),
            created_at_ms: 20,
        };
        store
            .with_connection(|connection| {
                connection.execute_batch(&format!(
                    "CREATE TRIGGER injected_atomic_failure AFTER INSERT ON {table}
                         BEGIN SELECT RAISE(ABORT, 'injected atomic failure'); END;"
                ))?;
                Ok(())
            })
            .unwrap();

        assert!(store
            .commit_handoff_transition_with_event(
                &handoff(),
                &downstream_run,
                &link,
                &goal("run-2"),
                Some(&event()),
            )
            .is_err());
        assert_transition_record_count(&store, 0);
        store
            .with_connection(|connection| {
                connection.execute_batch("DROP TRIGGER injected_atomic_failure")?;
                Ok(())
            })
            .unwrap();
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
            AtomicHandoffCommit::Committed
        );
        assert_transition_record_count(&store, 1);
    }
}

#[test]
fn concurrent_idempotent_handoffs_create_one_downstream_run() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let downstream_run = run("run-2");
    let link = GoalRunLink {
        goal_id: "goal-1".to_string(),
        run_id: downstream_run.run_id.clone(),
        orchestration_node_id: "execute".to_string(),
        orchestration_version: 3,
        hop_index: 1,
        parent_run_id: Some("run-1".to_string()),
        triggering_handoff_id: Some("handoff-1".to_string()),
        created_at_ms: 20,
    };
    let barrier = std::sync::Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            store.commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
        });
        let second = scope.spawn(|| {
            barrier.wait();
            store.commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
        });
        (
            first.join().expect("first worker panicked").unwrap(),
            second.join().expect("second worker panicked").unwrap(),
        )
    });
    assert!(matches!(
        (first, second),
        (
            AtomicHandoffCommit::Committed,
            AtomicHandoffCommit::AlreadyCommitted
        ) | (
            AtomicHandoffCommit::AlreadyCommitted,
            AtomicHandoffCommit::Committed
        )
    ));
    store
        .with_connection(|connection| {
            for table in ["workflow_handoffs", "automation_runs", "goal_run_links"] {
                let count: u64 =
                    connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(count, 1, "{table} should have exactly one row");
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn hot_sync_retains_archived_rows_without_reloading_them() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let archived = run("run-archived");
    let hot = run("run-hot");

    store.sync_hot_automation_runs([&archived, &hot]).unwrap();
    store.sync_hot_automation_runs([&hot]).unwrap();

    let loaded = store.load_automation_runs().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].run_id, "run-hot");
    assert_eq!(
        store
            .get_automation_run("run-archived")
            .unwrap()
            .unwrap()
            .run_id,
        "run-archived"
    );
}

#[test]
fn engine_lock_rejects_a_second_owner() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let first = store.acquire_engine_lock().unwrap();
    assert_eq!(first.path(), directory.path().join("engine.lock"));
    assert!(store.acquire_engine_lock().is_err());
}

#[test]
fn engine_lock_records_owner_and_diagnoses_the_holder() {
    let directory = tempfile::tempdir().unwrap();
    let lock_path = directory.path().join("engine.lock");
    let lock = StatefulEngineLock::acquire(&lock_path).unwrap();
    let owner = read_engine_lock_owner(&lock_path).expect("owner metadata");
    assert_eq!(owner.pid, std::process::id());
    assert_eq!(lock.owner().map(|owner| owner.pid), Some(owner.pid));

    let error = StatefulEngineLock::acquire(&lock_path)
        .expect_err("second owner must be rejected")
        .to_string();
    assert!(error.contains(&format!("pid {}", owner.pid)), "{error}");
    // This process holds the lock, so the diagnostics must prove the
    // owner alive (or report that liveness cannot be determined here).
    assert!(
        error.contains("held by live engine") || error.contains("liveness unknown"),
        "{error}"
    );

    // Once the holder exits, acquisition recovers without manual cleanup.
    drop(lock);
    assert!(StatefulEngineLock::acquire(&lock_path).is_ok());
}

#[test]
fn snapshot_prune_keeps_newest_snapshots_per_run() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
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
    // Everything is older than the cutoff; only the newest survives.
    let pruned = store.prune_stateful_runtime_snapshots(1_000, 1).unwrap();
    assert_eq!(
        pruned,
        vec!["snapshot-old-1".to_string(), "snapshot-old-2".to_string()]
    );
    let remaining = store.list_stateful_runtime_snapshots("run-a").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].snapshot_id, "snapshot-new");
    assert_eq!(
        store.latest_stateful_snapshot_seqs().unwrap().get("run-a"),
        Some(&3)
    );
}

#[test]
fn published_versions_are_immutable() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let spec = published_spec();
    store.put_orchestration(&spec).unwrap();
    assert_eq!(
        store.get_orchestration("orch-1", 3).unwrap(),
        Some(spec.clone())
    );

    let mut changed = spec;
    changed.name = "Changed after publish".to_string();
    changed.updated_at_ms += 1;
    assert!(store.put_orchestration(&changed).is_err());
}

#[test]
fn tool_request_ledger_blocks_concurrent_replays_and_reclaims_stale_leases() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let tenant = TenantContext::local_implicit();

    assert_eq!(
        store
            .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 100)
            .unwrap(),
        None
    );
    let error = store
        .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 101)
        .expect_err("a live reservation must block a concurrent replay");
    assert!(error.to_string().contains("still in flight"));

    assert_eq!(
        store
            .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 30_100,)
            .unwrap(),
        None
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
            .begin_orchestration_tool_request(&tenant, "publish", "request-1", "digest-1", 30_102,)
            .unwrap(),
        Some(response)
    );
}
