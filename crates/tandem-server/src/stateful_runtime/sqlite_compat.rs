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
    if !paths.database_path.exists() {
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
    use serde_json::json;
    use tandem_types::TenantContext;

    use super::*;
    use crate::stateful_runtime::{
        load_stateful_reliability, load_stateful_waits, upsert_stateful_outbox,
        upsert_stateful_wait, LegacyRuntimeMigrationPaths, OrchestrationStateStore,
        StatefulOutboxRecord, StatefulOutboxStatus, StatefulRuntimeScope, StatefulWaitKind,
        StatefulWaitRecord, StatefulWaitStatus,
    };

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
}
