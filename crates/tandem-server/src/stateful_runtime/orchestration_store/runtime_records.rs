use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::OrchestrationStateStore;
use crate::stateful_runtime::{
    stateful_run_event_compacted_event_ids, StatefulRunEventRecord, StatefulRunSnapshotRecord,
};

impl OrchestrationStateStore {
    pub fn append_stateful_runtime_event(
        &self,
        event: &StatefulRunEventRecord,
    ) -> anyhow::Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let inserted = insert_event(&transaction, event)?;
            transaction.commit()?;
            Ok(inserted)
        })
    }

    pub fn append_stateful_runtime_event_once(
        &self,
        event: &StatefulRunEventRecord,
    ) -> anyhow::Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if event_seq_by_id(&transaction, &event.run_id, &event.event_id)?.is_some() {
                transaction.commit()?;
                return Ok(false);
            }
            let inserted = insert_event(&transaction, event)?;
            transaction.commit()?;
            Ok(inserted)
        })
    }

    pub fn append_stateful_runtime_event_once_with_next_seq(
        &self,
        event: &StatefulRunEventRecord,
    ) -> anyhow::Result<(bool, u64)> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(seq) = event_seq_by_id(&transaction, &event.run_id, &event.event_id)? {
                transaction.commit()?;
                return Ok((false, seq));
            }

            let last_seq: Option<u64> = transaction.query_row(
                "SELECT MAX(seq) FROM stateful_events WHERE run_id = ?1",
                [&event.run_id],
                |row| row.get(0),
            )?;
            let seq = last_seq.unwrap_or(0).saturating_add(1).max(1);
            let mut next = event.clone();
            next.seq = seq;
            if !insert_event(&transaction, &next)? {
                let existing_run_id: String = transaction.query_row(
                    "SELECT run_id FROM stateful_events WHERE event_id = ?1",
                    [&event.event_id],
                    |row| row.get(0),
                )?;
                bail!(
                    "stateful event ID `{}` is already stored for run `{existing_run_id}`",
                    event.event_id
                );
            }
            transaction.commit()?;
            Ok((true, seq))
        })
    }

    pub fn load_stateful_runtime_events(&self) -> anyhow::Result<Vec<StatefulRunEventRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT event_json FROM stateful_events ORDER BY seq, run_id, event_id")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|row| serde_json::from_str(&row?).map_err(Into::into))
                .collect()
        })
    }

    pub fn replace_stateful_runtime_events(
        &self,
        events: &[StatefulRunEventRecord],
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute("DELETE FROM stateful_events", [])?;
            for event in events {
                insert_event(&transaction, event)?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn put_stateful_runtime_snapshot(
        &self,
        snapshot: &StatefulRunSnapshotRecord,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO stateful_snapshots
                    (snapshot_id, goal_id, run_id, seq, snapshot_json, created_at_ms,
                     org_id, workspace_id, deployment_id)
                 VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(snapshot_id) DO UPDATE SET
                    run_id = excluded.run_id,
                    seq = excluded.seq,
                    snapshot_json = excluded.snapshot_json,
                    created_at_ms = excluded.created_at_ms,
                    org_id = excluded.org_id,
                    workspace_id = excluded.workspace_id,
                    deployment_id = excluded.deployment_id",
                params![
                    snapshot.snapshot_id,
                    snapshot.run_id,
                    snapshot.seq,
                    serde_json::to_string(snapshot)?,
                    snapshot.created_at_ms,
                    snapshot.scope.tenant_context.org_id,
                    snapshot.scope.tenant_context.workspace_id,
                    snapshot.scope.tenant_context.deployment_id,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_stateful_runtime_snapshots(
        &self,
        run_id: &str,
    ) -> anyhow::Result<Vec<StatefulRunSnapshotRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT snapshot_json FROM stateful_snapshots
                 WHERE run_id = ?1 ORDER BY seq, snapshot_id",
            )?;
            let rows = statement.query_map([run_id], |row| row.get::<_, String>(0))?;
            rows.map(|row| serde_json::from_str(&row?).map_err(Into::into))
                .collect()
        })
    }

    pub fn get_stateful_runtime_snapshot(
        &self,
        snapshot_id: &str,
    ) -> anyhow::Result<Option<StatefulRunSnapshotRecord>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT snapshot_json FROM stateful_snapshots WHERE snapshot_id = ?1",
                    [snapshot_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|row| serde_json::from_str(&row).map_err(Into::into))
                .transpose()
        })
    }
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    event: &StatefulRunEventRecord,
) -> anyhow::Result<bool> {
    let inserted = transaction.execute(
        "INSERT INTO stateful_events
            (event_id, goal_id, run_id, seq, event_json, created_at_ms,
             org_id, workspace_id, deployment_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(event_id) DO NOTHING",
        params![
            event.event_id,
            event
                .payload
                .get("goal_id")
                .and_then(serde_json::Value::as_str),
            event.run_id,
            event.seq,
            serde_json::to_string(event)?,
            event.occurred_at_ms,
            event.scope.tenant_context.org_id,
            event.scope.tenant_context.workspace_id,
            event.scope.tenant_context.deployment_id,
        ],
    )?;
    Ok(inserted > 0)
}

fn event_seq_by_id(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    event_id: &str,
) -> anyhow::Result<Option<u64>> {
    let mut statement = transaction.prepare(
        "SELECT event_json FROM stateful_events WHERE run_id = ?1 ORDER BY seq, event_id",
    )?;
    let rows = statement.query_map([run_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        let event: StatefulRunEventRecord =
            serde_json::from_str(&row?).context("stored stateful event could not be decoded")?;
        if event.event_id == event_id
            || stateful_run_event_compacted_event_ids(&event)
                .iter()
                .any(|(compacted_id, _)| compacted_id == event_id)
        {
            return Ok(Some(
                stateful_run_event_compacted_event_ids(&event)
                    .into_iter()
                    .find_map(|(compacted_id, seq)| (compacted_id == event_id).then_some(seq))
                    .unwrap_or(event.seq),
            ));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::TenantContext;

    use super::*;
    use crate::stateful_runtime::StatefulRuntimeScope;

    fn event(run_id: &str) -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: "shared-event-id".to_string(),
            run_id: run_id.to_string(),
            seq: 0,
            event_type: "stateful_runtime.test".to_string(),
            occurred_at_ms: 100,
            scope: StatefulRuntimeScope::from_tenant_context(
                TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a"),
            ),
            actor: None,
            phase_id: None,
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({}),
        }
    }

    #[test]
    fn next_sequence_rejects_event_ids_owned_by_another_run() {
        let directory = tempfile::tempdir().expect("create test directory");
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .expect("open orchestration store");
        let first = event("run-a");

        assert_eq!(
            store
                .append_stateful_runtime_event_once_with_next_seq(&first)
                .expect("store first event"),
            (true, 1)
        );

        let error = store
            .append_stateful_runtime_event_once_with_next_seq(&event("run-b"))
            .expect_err("reject cross-run event ID collision");
        assert!(error.to_string().contains("already stored for run `run-a`"));
        assert_eq!(
            store.load_stateful_runtime_events().unwrap(),
            vec![{
                let mut stored = first;
                stored.seq = 1;
                stored
            }]
        );
    }
}
