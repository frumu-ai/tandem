// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::Path;

use tandem_automation::{
    AutomationV2RunRecord, GoalLimitAction, GoalPolicy, GoalRunLink, LongRunningGoal,
    LongRunningGoalStatus, OrchestrationArtifactRef, OrchestrationSpec,
};
use tandem_types::{PrincipalRef, TenantContext};

use super::*;
use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, compact_stateful_run_event_log,
    stateful_run_event_compacted_event_ids, StatefulRunEventRecord, StatefulRuntimeScope,
};

fn paths(root: &Path) -> OrchestrationStorePaths {
    OrchestrationStorePaths {
        database_path: root.join("stateful_runtime.sqlite3"),
        engine_lock_path: root.join("stateful_runtime.engine.lock"),
    }
}

fn open_store(root: &Path) -> OrchestrationStateStore {
    OrchestrationStateStore::open(paths(root)).expect("open store")
}

fn event(run_id: &str, event_id: &str, occurred_at_ms: u64) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: 1,
        event_id: event_id.to_string(),
        run_id: run_id.to_string(),
        seq: 0,
        event_type: "stateful_runtime.synthetic".to_string(),
        occurred_at_ms,
        scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
        actor: None,
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: None,
        correlation_id: None,
        payload: serde_json::json!({"synthetic": true}),
    }
}

fn goal() -> LongRunningGoal {
    LongRunningGoal {
        schema_version: 1,
        goal_id: "goal-hardening".to_string(),
        orchestration_id: "orch-hardening".to_string(),
        orchestration_version: 1,
        objective: "Verify durable boundaries".to_string(),
        status: LongRunningGoalStatus::Active,
        tenant_context: TenantContext::local_implicit(),
        policy: GoalPolicy {
            max_hops: 4,
            deadline_at_ms: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
            on_limit: GoalLimitAction::PauseForReview,
        },
        active_run_id: Some("run-hardening".to_string()),
        current_node_id: Some("work".to_string()),
        hop_count: 0,
        total_tokens: 0,
        total_cost_usd: 0.0,
        created_at_ms: 1,
        updated_at_ms: 1,
        finished_at_ms: None,
        final_artifact: None,
        metadata: None,
    }
}

fn run() -> AutomationV2RunRecord {
    serde_json::from_value(serde_json::json!({
        "run_id": "run-hardening",
        "automation_id": "worker",
        "trigger_type": "goal_start",
        "status": "completed",
        "created_at_ms": 1,
        "updated_at_ms": 2,
        "checkpoint": {}
    }))
    .expect("run fixture")
}

fn link() -> GoalRunLink {
    GoalRunLink {
        goal_id: "goal-hardening".to_string(),
        run_id: "run-hardening".to_string(),
        orchestration_node_id: "work".to_string(),
        orchestration_version: 1,
        hop_index: 0,
        parent_run_id: None,
        triggering_handoff_id: None,
        created_at_ms: 1,
    }
}

fn terminal_orchestration() -> OrchestrationSpec {
    serde_json::from_value(serde_json::json!({
        "orchestration_id": "orch-hardening",
        "name": "Hardening terminal",
        "status": "published",
        "version": 1,
        "root_node_id": "work",
        "nodes": [
            {
                "node_id": "work",
                "name": "Work",
                "kind": "workflow",
                "automation_id": "worker",
                "pinned_definition_hash": "sha256:worker",
                "allowed_transition_keys": ["complete"],
                "emits_artifact_types": ["result"]
            },
            {
                "node_id": "done",
                "name": "Done",
                "kind": "terminal",
                "outcome": "complete",
                "final_artifact_type": "result"
            }
        ],
        "edges": [{
            "edge_id": "work-done",
            "from_node_id": "work",
            "to_node_id": "done",
            "transition_key": "complete",
            "artifact_contract": {"artifact_type": "result", "required": true}
        }],
        "goal_policy": {"max_hops": 4},
        "tenant_context": {
            "org_id": "local",
            "workspace_id": "local",
            "source": "local_implicit"
        },
        "created_at_ms": 1,
        "updated_at_ms": 1,
        "published_at_ms": 1
    }))
    .expect("orchestration fixture")
}

fn table_count(store: &OrchestrationStateStore, table: &str) -> u64 {
    store
        .with_connection(|connection| {
            Ok(
                connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?,
            )
        })
        .expect("count table")
}

fn install_failure_trigger(store: &OrchestrationStateStore, sql: &str) {
    store
        .with_connection(|connection| {
            connection.execute_batch(sql)?;
            Ok(())
        })
        .expect("install failure trigger");
}

#[cfg(target_os = "linux")]
#[test]
fn engine_lock_refuses_live_child_pid_and_recovers_after_exit() {
    use std::process::{Command, Stdio};

    let directory = tempfile::tempdir().expect("tempdir");
    let lock_path = directory.path().join("engine.lock");
    let mut child = Command::new("sh")
        .args(["-c", "sleep 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn live child");
    let owner = EngineLockOwner::for_pid(child.id(), 1);
    assert!(owner.process_start_hint.is_some());
    std::fs::write(&lock_path, serde_json::to_vec(&owner).unwrap()).unwrap();

    let error = StatefulEngineLock::acquire(&lock_path)
        .expect_err("live metadata owner must prevent takeover")
        .to_string();
    assert!(error.contains("still alive"), "{error}");
    assert_eq!(read_engine_lock_owner(&lock_path), Some(owner.clone()));

    let dead_child_owner = owner.clone();
    let mut reused_pid_owner = owner;
    reused_pid_owner.process_start_hint = Some("linux-start:not-the-child".to_string());
    std::fs::write(&lock_path, serde_json::to_vec(&reused_pid_owner).unwrap()).unwrap();
    let recovered = StatefulEngineLock::acquire(&lock_path)
        .expect("a reused PID must not impersonate the prior engine");
    assert_eq!(recovered.owner().unwrap().pid, std::process::id());
    drop(recovered);

    child.kill().expect("kill child");
    child.wait().expect("reap child");
    std::fs::write(&lock_path, serde_json::to_vec(&dead_child_owner).unwrap()).unwrap();
    let recovered = StatefulEngineLock::acquire(&lock_path).expect("recover dead child lock");
    assert_eq!(recovered.owner().unwrap().pid, std::process::id());
}

#[tokio::test]
async fn compaction_survives_store_reopen_and_preserves_idempotency() {
    let directory = tempfile::tempdir().expect("tempdir");
    let runtime_path = directory.path().join("runtime/stateful_events.jsonl");
    std::fs::create_dir_all(runtime_path.parent().unwrap()).unwrap();
    let store = open_store(directory.path());
    store
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO stateful_migrations
                 (migration_id, status, source_fingerprint, record_count, started_at_ms, completed_at_ms)
                 VALUES ('legacy_stateful_runtime_v1', 'complete', 'test', 0, 1, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    for index in 1..=6 {
        let occurred_at_ms = if index <= 4 { index * 10 } else { 900 + index };
        append_stateful_run_event_once_with_next_seq(
            &runtime_path,
            &TenantContext::local_implicit(),
            &event("reopen-run", &format!("reopen-{index}"), occurred_at_ms),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        compact_stateful_run_event_log(&runtime_path, 500, 1_000)
            .await
            .unwrap(),
        4
    );
    drop(store);

    let reopened = open_store(directory.path());
    let rows = reopened.load_stateful_runtime_events().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.last().unwrap().seq, 6);
    assert_eq!(stateful_run_event_compacted_event_ids(&rows[0]).len(), 4);
    let duplicate = event("reopen-run", "reopen-2", 2_000);
    assert_eq!(
        reopened
            .append_stateful_runtime_event_once_with_next_seq(&duplicate)
            .unwrap(),
        (false, 2)
    );
    let next = event("reopen-run", "reopen-next", 2_000);
    assert_eq!(
        reopened
            .append_stateful_runtime_event_once_with_next_seq(&next)
            .unwrap(),
        (true, 7)
    );
}

#[tokio::test]
async fn bounded_multi_run_workload_compacts_and_reopens() {
    const RUNS: usize = 12;
    const EVENTS_PER_RUN: usize = 48;
    const RETAINED_PER_RUN: usize = 8;

    let directory = tempfile::tempdir().expect("tempdir");
    let runtime_path = directory.path().join("synthetic-events.jsonl");
    let tenant = TenantContext::local_implicit();
    for run_index in 0..RUNS {
        for event_index in 1..=EVENTS_PER_RUN {
            let occurred_at_ms = if event_index <= EVENTS_PER_RUN - RETAINED_PER_RUN {
                event_index as u64
            } else {
                900 + event_index as u64
            };
            let record = event(
                &format!("synthetic-run-{run_index}"),
                &format!("synthetic-{run_index}-{event_index}"),
                occurred_at_ms,
            );
            let (inserted, seq) =
                append_stateful_run_event_once_with_next_seq(&runtime_path, &tenant, &record)
                    .await
                    .unwrap();
            assert!(inserted);
            assert_eq!(seq, event_index as u64);
        }
    }

    let pruned = compact_stateful_run_event_log(&runtime_path, 500, 1_000)
        .await
        .unwrap();
    assert_eq!(pruned, RUNS * (EVENTS_PER_RUN - RETAINED_PER_RUN));
    let reopened_rows = crate::stateful_runtime::load_stateful_run_events(&runtime_path);
    assert_eq!(reopened_rows.len(), RUNS * (RETAINED_PER_RUN + 1));
    for run_index in 0..RUNS {
        let run_id = format!("synthetic-run-{run_index}");
        let rows = reopened_rows
            .iter()
            .filter(|row| row.run_id == run_id)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), RETAINED_PER_RUN + 1);
        assert_eq!(rows.last().unwrap().seq, EVENTS_PER_RUN as u64);
        assert_eq!(
            stateful_run_event_compacted_event_ids(rows[0]).len(),
            EVENTS_PER_RUN - RETAINED_PER_RUN
        );
    }
}

#[test]
fn goal_start_rolls_back_and_restarts_at_each_write_boundary() {
    for table in [
        "long_running_goals",
        "automation_runs",
        "goal_run_links",
        "stateful_events",
    ] {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = open_store(directory.path());
        install_failure_trigger(
            &store,
            &format!(
                "CREATE TRIGGER fail_goal_start AFTER INSERT ON {table}
                 BEGIN SELECT RAISE(ABORT, 'injected goal start failure'); END;"
            ),
        );
        assert!(store
            .start_goal(
                &goal(),
                &run(),
                &link(),
                &PrincipalRef::human_user("tester")
            )
            .is_err());
        drop(store);

        let reopened = open_store(directory.path());
        for persisted_table in [
            "long_running_goals",
            "automation_runs",
            "goal_run_links",
            "stateful_events",
        ] {
            assert_eq!(
                table_count(&reopened, persisted_table),
                0,
                "failed at {table}"
            );
        }
    }
}

#[test]
fn event_append_failure_has_no_restart_visible_partial_write() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = open_store(directory.path());
    install_failure_trigger(
        &store,
        "CREATE TRIGGER fail_event_append AFTER INSERT ON stateful_events
         BEGIN SELECT RAISE(ABORT, 'injected event append failure'); END;",
    );
    assert!(store
        .append_stateful_runtime_event_once_with_next_seq(&event("run", "event", 1))
        .is_err());
    drop(store);

    let reopened = open_store(directory.path());
    assert_eq!(table_count(&reopened, "stateful_events"), 0);
    reopened
        .with_connection(|connection| {
            connection.execute_batch("DROP TRIGGER fail_event_append")?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        reopened
            .append_stateful_runtime_event_once_with_next_seq(&event("run", "event", 2))
            .unwrap(),
        (true, 1)
    );
}

#[test]
fn terminal_goal_and_event_roll_back_together_across_restart() {
    for trigger in [
        "CREATE TRIGGER fail_terminal_goal AFTER UPDATE ON long_running_goals
         BEGIN SELECT RAISE(ABORT, 'injected terminal goal failure'); END;",
        "CREATE TRIGGER fail_terminal_event AFTER INSERT ON stateful_events
         BEGIN SELECT RAISE(ABORT, 'injected terminal event failure'); END;",
    ] {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = open_store(directory.path());
        store.put_goal(&goal()).unwrap();
        install_failure_trigger(&store, trigger);
        let artifact = OrchestrationArtifactRef {
            artifact_type: "result".to_string(),
            content_path: None,
            content_digest: None,
            value: Some(serde_json::json!({"ok": true})),
        };
        let authority = OrchestrationTransitionAuthority {
            actor: PrincipalRef::human_user("tester"),
            can_emit: true,
            can_approve: false,
        };
        assert!(store
            .settle_workflow_completion(
                &terminal_orchestration(),
                &goal(),
                &run(),
                Some("complete"),
                Some(artifact),
                &authority,
                10,
            )
            .is_err());
        drop(store);

        let reopened = open_store(directory.path());
        assert_eq!(
            reopened.get_goal("goal-hardening").unwrap().unwrap().status,
            LongRunningGoalStatus::Active
        );
        assert_eq!(table_count(&reopened, "stateful_events"), 0);
    }
}
