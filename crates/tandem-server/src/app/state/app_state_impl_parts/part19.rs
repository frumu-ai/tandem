// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

impl AppState {
    pub async fn put_workflow_run(&self, run: WorkflowRunRecord) -> anyhow::Result<()> {
        let _transaction = self.workflow_runs_persistence.lock().await;
        let run_id = run.run_id.clone();
        let previous = self.workflow_runs.write().await.insert(run_id.clone(), run);
        if let Err(error) = self.persist_workflow_runs().await {
            let mut guard = self.workflow_runs.write().await;
            if let Some(previous) = previous {
                guard.insert(run_id, previous);
            } else {
                guard.remove(&run_id);
            }
            drop(guard);
            let _ = self.persist_workflow_runs().await;
            return Err(error);
        }
        Ok(())
    }

    pub async fn update_workflow_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut WorkflowRunRecord),
    ) -> Option<WorkflowRunRecord> {
        let _transaction = self.workflow_runs_persistence.lock().await;
        let mut guard = self.workflow_runs.write().await;
        let row = guard.get_mut(run_id)?;
        let previous = row.clone();
        update(row);
        row.updated_at_ms = now_ms();
        if matches!(
            row.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Failed
        ) {
            row.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = row.clone();
        drop(guard);
        if self.persist_workflow_runs().await.is_err() {
            self.workflow_runs
                .write()
                .await
                .insert(run_id.to_string(), previous);
            let _ = self.persist_workflow_runs().await;
            return None;
        }
        Some(out)
    }

    pub async fn update_workflow_run_persisted(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut WorkflowRunRecord) -> bool,
    ) -> anyhow::Result<Option<(WorkflowRunRecord, bool)>> {
        let _transaction = self.workflow_runs_persistence.lock().await;
        let (previous, out, applied) = {
            let mut guard = self.workflow_runs.write().await;
            let Some(row) = guard.get_mut(run_id) else {
                return Ok(None);
            };
            let previous = row.clone();
            let applied = update(row);
            if applied {
                row.updated_at_ms = now_ms();
                if matches!(
                    row.status,
                    WorkflowRunStatus::Completed | WorkflowRunStatus::Failed
                ) {
                    row.finished_at_ms.get_or_insert_with(now_ms);
                }
            }
            (previous, row.clone(), applied)
        };
        if !applied {
            return Ok(Some((out, false)));
        }
        if let Err(error) = self.persist_workflow_runs().await {
            self.workflow_runs
                .write()
                .await
                .insert(run_id.to_string(), previous);
            let _ = self.persist_workflow_runs().await;
            return Err(error);
        }
        Ok(Some((out, true)))
    }

    pub async fn list_workflow_runs(
        &self,
        workflow_id: Option<&str>,
        limit: usize,
    ) -> Vec<WorkflowRunRecord> {
        let mut rows = self
            .workflow_runs
            .read()
            .await
            .values()
            .filter(|row| workflow_id.map(|id| row.workflow_id == id).unwrap_or(true))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn get_workflow_run(&self, run_id: &str) -> Option<WorkflowRunRecord> {
        self.workflow_runs.read().await.get(run_id).cloned()
    }
}
