// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;

use tokio::fs;

use super::{
    automation_v2_hot_cutoff_ms, automation_v2_run_is_nonterminal_recovered_context_run,
    cleanup_stale_legacy_automation_v2_runs_file, compact_automation_v2_runs_for_hot_storage,
    serialize_automation_v2_runs_file, write_automation_v2_run_history_shard, AppState,
    AutomationV2RunRecord,
};
use crate::util::time::now_ms;

impl AppState {
    pub(super) fn acquire_stateful_engine_lock(&self) -> anyhow::Result<()> {
        let mut guard = self
            .stateful_engine_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("stateful engine lock guard was poisoned"))?;
        if guard.is_none() {
            let paths = crate::stateful_runtime::OrchestrationStorePaths::from_automation_runs_path(
                &self.automation_v2_runs_path,
            );
            let engine_lock =
                crate::stateful_runtime::OrchestrationStateStore::acquire_engine_lock_for_runtime(
                    paths,
                )?;
            *guard = Some(engine_lock);
        }
        Ok(())
    }

    pub(super) async fn load_automation_v2_runs_from_stateful_store(
        &self,
    ) -> anyhow::Result<Vec<AutomationV2RunRecord>> {
        let automation_runs_path = self.automation_v2_runs_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
                &automation_runs_path,
            )?
            .load_automation_runs()
        })
        .await
        .map_err(|error| anyhow::anyhow!("automation run database load task failed: {error}"))?
    }

    /// Imports the legacy runtime exactly once. Later loads must not read
    /// legacy files back into SQLite because the completed marker makes the
    /// transactional store authoritative.
    pub(super) async fn migrate_legacy_stateful_runtime(&self) -> anyhow::Result<bool> {
        let automation_runs_path = self.automation_v2_runs_path.clone();
        let runtime_events_path = self.runtime_events_path.clone();
        let imported_at_ms = now_ms();
        // The automations this engine can name separate local envelopes from
        // foreign ones during the once-only import. Workspace file handoffs
        // are imported separately post-ready (`import_legacy_workspace_handoffs`)
        // because the workspace root lives behind the runtime.
        let context = crate::stateful_runtime::LegacyImportContext {
            known_automation_ids: self.automations_v2.read().await.keys().cloned().collect(),
        };
        tokio::task::spawn_blocking(move || {
            let store =
                crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
                    &automation_runs_path,
                )?;
            if store.legacy_runtime_migration_complete()? {
                return Ok(true);
            }
            let paths = crate::stateful_runtime::LegacyRuntimeMigrationPaths::from_runtime_paths(
                automation_runs_path,
                &runtime_events_path,
            );
            store.import_legacy_runtime_state_with_context(&paths, &context, imported_at_ms)?;
            Ok(false)
        })
        .await
        .map_err(|error| anyhow::anyhow!("stateful runtime migration task failed: {error}"))?
    }

    /// Indexes legacy workspace file handoffs into the stateful store once the
    /// runtime (and with it the workspace root) is available. Idempotent: the
    /// importer upserts by handoff identity and re-quarantines bad envelopes,
    /// so re-running on every startup is safe.
    pub async fn import_legacy_workspace_handoffs(&self) -> anyhow::Result<usize> {
        let workspace_root = self.workspace_index.snapshot().await.root;
        let handoff_root = std::path::Path::new(&workspace_root).join("shared/handoffs");
        if !handoff_root.is_dir() {
            return Ok(0);
        }
        let automation_runs_path = self.automation_v2_runs_path.clone();
        let imported_at_ms = now_ms();
        let context = crate::stateful_runtime::LegacyImportContext {
            known_automation_ids: self.automations_v2.read().await.keys().cloned().collect(),
        };
        tokio::task::spawn_blocking(move || {
            crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
                &automation_runs_path,
            )?
            .import_legacy_handoff_directory_with_context(
                &handoff_root,
                &context,
                imported_at_ms,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("legacy handoff import task failed: {error}"))?
    }

    pub(super) async fn import_automation_v2_runs_to_stateful_store(
        &self,
        runs: &HashMap<String, AutomationV2RunRecord>,
    ) -> anyhow::Result<()> {
        let database_snapshot = runs.values().cloned().collect::<Vec<_>>();
        let automation_runs_path = self.automation_v2_runs_path.clone();
        let imported_at_ms = now_ms();
        tokio::task::spawn_blocking(move || {
            let store =
                crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
                    &automation_runs_path,
                )?;
            store.import_legacy_runs(&automation_runs_path, &database_snapshot, imported_at_ms)
        })
        .await
        .map_err(|error| {
            anyhow::anyhow!("automation run database import task failed: {error}")
        })??;
        Ok(())
    }

    pub async fn persist_automation_v2_runs(&self) -> anyhow::Result<()> {
        let (runs_snapshot, automations_snapshot) = {
            let runs = self.automation_v2_runs.read().await;
            let automations = self.automations_v2.read().await;
            (runs.clone(), automations.clone())
        };
        for run in runs_snapshot.values() {
            write_automation_v2_run_history_shard(&self.automation_v2_runs_path, run).await?;
        }
        let mut compacted = runs_snapshot;
        compacted.retain(|_, run| !automation_v2_run_is_nonterminal_recovered_context_run(run));
        compact_automation_v2_runs_for_hot_storage(
            &mut compacted,
            &automations_snapshot,
            automation_v2_hot_cutoff_ms(),
        );
        let database_snapshot = compacted.values().cloned().collect::<Vec<_>>();
        let automation_runs_path = self.automation_v2_runs_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
                &automation_runs_path,
            )?
            .sync_hot_automation_runs(database_snapshot.iter())
        })
        .await
        .map_err(|error| {
            anyhow::anyhow!("automation run database persist task failed: {error}")
        })??;
        let payload = serialize_automation_v2_runs_file(compacted)?;
        if let Some(parent) = self.automation_v2_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.automation_v2_runs_path, &payload).await?;
        let _ = cleanup_stale_legacy_automation_v2_runs_file(&self.automation_v2_runs_path).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completed_runtime_migration_ignores_later_legacy_run_file_changes() {
        let root = std::env::temp_dir().join(format!(
            "tandem-stateful-runtime-authoritative-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let runs_path = root.join("automation_v2_runs.json");
        std::fs::write(
            &runs_path,
            include_str!("tests/fixtures/automation_v2_runs_v1_envelope.json"),
        )
        .unwrap();

        let mut first = crate::app::state::tests::test_state_with_path(root.join("shared.json"));
        first.automation_v2_runs_path = runs_path.clone();
        first.load_automation_v2_runs().await.unwrap();
        assert!(first
            .automation_v2_runs
            .read()
            .await
            .contains_key("run-fixture-versioned"));

        std::fs::write(&runs_path, "{not-json}\n").unwrap();
        let mut restarted =
            crate::app::state::tests::test_state_with_path(root.join("shared-restart.json"));
        restarted.automation_v2_runs_path = runs_path;
        restarted.load_automation_v2_runs().await.unwrap();
        assert!(restarted
            .automation_v2_runs
            .read()
            .await
            .contains_key("run-fixture-versioned"));
    }

    #[tokio::test]
    async fn initial_runtime_migration_quarantines_a_corrupt_legacy_checkpoint() {
        let root = std::env::temp_dir().join(format!(
            "tandem-stateful-runtime-corrupt-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let runs_path = root.join("automation_v2_runs.json");
        let mut fixture: serde_json::Value = serde_json::from_str(include_str!(
            "tests/fixtures/automation_v2_runs_v1_envelope.json"
        ))
        .unwrap();
        fixture["runs"]["run-fixture-versioned"]["checkpoint"] =
            serde_json::json!("corrupted checkpoint payload");
        std::fs::write(&runs_path, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();

        let mut state = crate::app::state::tests::test_state_with_path(root.join("shared.json"));
        state.automation_v2_runs_path = runs_path;
        state.load_automation_v2_runs().await.unwrap();
        let run = state
            .automation_v2_runs
            .read()
            .await
            .get("run-fixture-versioned")
            .cloned()
            .unwrap();
        assert_eq!(
            run.status,
            crate::automation_v2::types::AutomationRunStatus::Blocked
        );
        assert_eq!(run.checkpoint.blocked_nodes, vec!["checkpoint"]);
        assert!(run
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("checkpoint could not be parsed")));
    }
}
