use std::path::Path;

use super::{OrchestrationStateStore, OrchestrationStorePaths};

const STATEFUL_RELIABILITY_FILE_NAME: &str = "stateful_reliability.json";
const STATEFUL_WAITS_FILE_NAME: &str = "stateful_waits.json";
const STATEFUL_EVENTS_FILE_NAME: &str = "stateful_events.jsonl";

pub(crate) fn authoritative_stateful_store_for_wait_path(
    path: &Path,
) -> anyhow::Result<Option<OrchestrationStateStore>> {
    authoritative_stateful_store_for_path(path, STATEFUL_WAITS_FILE_NAME)
}

pub(crate) fn authoritative_stateful_store_for_reliability_path(
    path: &Path,
) -> anyhow::Result<Option<OrchestrationStateStore>> {
    authoritative_stateful_store_for_path(path, STATEFUL_RELIABILITY_FILE_NAME)
}

fn authoritative_stateful_store_for_path(
    path: &Path,
    expected_file_name: &str,
) -> anyhow::Result<Option<OrchestrationStateStore>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_file_name) {
        return Ok(None);
    }
    let runtime_events_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(STATEFUL_EVENTS_FILE_NAME);
    let paths = OrchestrationStorePaths::from_runtime_events_path(&runtime_events_path);
    if !crate::stateful_runtime::backend::store_initialized_hint(&paths.database_path)? {
        return Ok(None);
    }
    let store = OrchestrationStateStore::open(paths)?;
    if store.legacy_runtime_migration_complete()? {
        Ok(Some(store))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use serde_json::json;
    use tandem_types::TenantContext;

    use super::*;
    use crate::stateful_runtime::{
        append_stateful_run_event, list_stateful_run_snapshots, load_stateful_reliability,
        load_stateful_run_events, load_stateful_waits, upsert_stateful_outbox,
        upsert_stateful_wait, write_stateful_run_snapshot, LegacyRuntimeMigrationPaths,
        OrchestrationStateStore, StatefulOutboxRecord, StatefulOutboxStatus,
        StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulRuntimeScope, StatefulWaitKind,
        StatefulWaitRecord, StatefulWaitStatus, StatefulWorkflowPhase, StatefulWorkflowRunStatus,
    };

    const COMPATIBILITY_MIRRORS_ENV: &str = "TANDEM_STATEFUL_RUNTIME_COMPATIBILITY_MIRRORS_ENABLED";

    struct CompatibilityMirrorsEnvGuard(Option<OsString>);

    impl CompatibilityMirrorsEnvGuard {
        fn set(enabled: Option<bool>) -> Self {
            let previous = std::env::var_os(COMPATIBILITY_MIRRORS_ENV);
            match enabled {
                Some(enabled) => std::env::set_var(COMPATIBILITY_MIRRORS_ENV, enabled.to_string()),
                None => std::env::remove_var(COMPATIBILITY_MIRRORS_ENV),
            }
            Self(previous)
        }
    }

    impl Drop for CompatibilityMirrorsEnvGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var(COMPATIBILITY_MIRRORS_ENV, value),
                None => std::env::remove_var(COMPATIBILITY_MIRRORS_ENV),
            }
        }
    }

    fn scope() -> StatefulRuntimeScope {
        StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit())
    }

    fn wait() -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-1".to_string(),
            run_id: "run-1".to_string(),
            wait_kind: StatefulWaitKind::Timer,
            status: StatefulWaitStatus::Waiting,
            scope: scope(),
            phase_id: None,
            reason: None,
            created_at_ms: 10,
            updated_at_ms: 10,
            wake_at_ms: Some(20),
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: Some(json!({ "source": "test" })),
        }
    }

    fn outbox() -> StatefulOutboxRecord {
        StatefulOutboxRecord {
            schema_version: 1,
            outbox_id: "outbox-1".to_string(),
            run_id: Some("run-1".to_string()),
            scope: scope(),
            operation: "test".to_string(),
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
            created_at_ms: 10,
            updated_at_ms: 10,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            metadata: None,
        }
    }

    fn event() -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: "event-1".to_string(),
            run_id: "run-1".to_string(),
            seq: 1,
            event_type: "stateful_runtime.test".to_string(),
            occurred_at_ms: 10,
            scope: scope(),
            actor: None,
            phase_id: None,
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({ "source": "test" }),
        }
    }

    fn snapshot() -> StatefulRunSnapshotRecord {
        StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-1".to_string(),
            run_id: "run-1".to_string(),
            seq: 1,
            created_at_ms: 10,
            scope: scope(),
            status: StatefulWorkflowRunStatus::Completed,
            phase: StatefulWorkflowPhase::Completed,
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

    fn migrate(root: &Path) {
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(root);
        OrchestrationStateStore::from_runtime_events_path(&root.join(STATEFUL_EVENTS_FILE_NAME))
            .unwrap()
            .import_legacy_runtime_state(&paths, 100)
            .unwrap();
    }

    #[tokio::test]
    async fn completed_migration_makes_waits_authoritative_over_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let wait_path = directory.path().join(STATEFUL_WAITS_FILE_NAME);
        migrate(directory.path());

        upsert_stateful_wait(&wait_path, wait()).await.unwrap();
        let mut scoped_duplicate = wait();
        scoped_duplicate.run_id = "run-2".to_string();
        scoped_duplicate.scope = StatefulRuntimeScope::from_tenant_context(
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b"),
        );
        upsert_stateful_wait(&wait_path, scoped_duplicate)
            .await
            .unwrap();
        std::fs::write(&wait_path, "{corrupt-sidecar").unwrap();

        let rows = load_stateful_waits(&wait_path);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].wait_id, "wait-1");
    }

    #[tokio::test]
    async fn completed_migration_makes_reliability_authoritative_over_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let reliability_path = directory.path().join(STATEFUL_RELIABILITY_FILE_NAME);
        migrate(directory.path());

        upsert_stateful_outbox(&reliability_path, outbox())
            .await
            .unwrap();
        std::fs::write(&reliability_path, "{corrupt-sidecar").unwrap();

        let records = load_stateful_reliability(&reliability_path);
        assert_eq!(records.outbox.len(), 1);
        assert_eq!(records.outbox[0].outbox_id, "outbox-1");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn completed_migration_defaults_to_authority_without_sidecar_writes() {
        let _mirrors = CompatibilityMirrorsEnvGuard::set(None);
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let event_path = root.join(STATEFUL_EVENTS_FILE_NAME);
        let snapshots_root = root.join("stateful_snapshots");
        let wait_path = root.join(STATEFUL_WAITS_FILE_NAME);
        let reliability_path = root.join(STATEFUL_RELIABILITY_FILE_NAME);
        migrate(root);

        let event = event();
        let snapshot = snapshot();
        append_stateful_run_event(&event_path, &event)
            .await
            .unwrap();
        let snapshot_path = write_stateful_run_snapshot(&snapshots_root, &snapshot)
            .await
            .unwrap();
        upsert_stateful_wait(&wait_path, wait()).await.unwrap();
        upsert_stateful_outbox(&reliability_path, outbox())
            .await
            .unwrap();

        assert!(!event_path.exists());
        assert!(!snapshot_path.exists());
        assert!(!wait_path.exists());
        assert!(!reliability_path.exists());

        // Sidecar divergence cannot alter post-migration reads: the store is
        // authoritative even when someone leaves malformed legacy artifacts.
        std::fs::write(&event_path, "{corrupt-sidecar\n").unwrap();
        std::fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();
        std::fs::write(&snapshot_path, "{corrupt-sidecar\n").unwrap();
        std::fs::write(&wait_path, "{corrupt-sidecar").unwrap();
        std::fs::write(&reliability_path, "{corrupt-sidecar").unwrap();

        assert_eq!(load_stateful_run_events(&event_path), vec![event]);
        assert_eq!(
            list_stateful_run_snapshots(
                &snapshots_root,
                &TenantContext::local_implicit(),
                "run-1",
                None
            ),
            vec![snapshot]
        );
        assert_eq!(load_stateful_waits(&wait_path).len(), 1);
        assert_eq!(load_stateful_reliability(&reliability_path).outbox.len(), 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn completed_migration_opt_in_restores_stateful_sidecar_dual_writes() {
        let _mirrors = CompatibilityMirrorsEnvGuard::set(Some(true));
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let event_path = root.join(STATEFUL_EVENTS_FILE_NAME);
        let snapshots_root = root.join("stateful_snapshots");
        let wait_path = root.join(STATEFUL_WAITS_FILE_NAME);
        let reliability_path = root.join(STATEFUL_RELIABILITY_FILE_NAME);
        migrate(root);

        let event = event();
        let snapshot = snapshot();
        append_stateful_run_event(&event_path, &event)
            .await
            .unwrap();
        let snapshot_path = write_stateful_run_snapshot(&snapshots_root, &snapshot)
            .await
            .unwrap();
        upsert_stateful_wait(&wait_path, wait()).await.unwrap();
        upsert_stateful_outbox(&reliability_path, outbox())
            .await
            .unwrap();

        assert!(std::fs::read_to_string(&event_path)
            .unwrap()
            .contains(&event.event_id));
        assert!(std::fs::read_to_string(&snapshot_path)
            .unwrap()
            .contains(&snapshot.snapshot_id));
        assert!(std::fs::read_to_string(&wait_path)
            .unwrap()
            .contains("wait-1"));
        assert!(std::fs::read_to_string(&reliability_path)
            .unwrap()
            .contains("outbox-1"));
    }
}
