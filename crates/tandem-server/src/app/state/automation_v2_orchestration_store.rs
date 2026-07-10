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
                crate::stateful_runtime::StatefulEngineLock::acquire(&paths.engine_lock_path)?;
            let _store = crate::stateful_runtime::OrchestrationStateStore::open(paths)?;
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
