use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use tandem_automation::AutomationV2RunRecord;
use tandem_types::TenantContext;

use super::{protected_records, OrchestrationStateStore};
use crate::stateful_runtime::reliability::{
    StatefulCompensationRecord, StatefulDeadLetterRecord, StatefulOutboxRecord,
    StatefulReliabilityStoreFile, StatefulToolEffectRecord,
};
use crate::stateful_runtime::types::{
    StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulWaitRecord,
};
use crate::stateful_runtime::{stateful_run_event_compacted_event_ids, StatefulRuntimeScope};

impl OrchestrationStateStore {
    pub fn resolve_goal_projection_snapshot(
        &self,
        expected_tenant: &TenantContext,
        reference: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let tenant: TenantContext = serde_json::from_value(
            reference
                .get("tenant_context")
                .cloned()
                .context("projection snapshot is missing tenant_context")?,
        )?;
        anyhow::ensure!(
            tenant.org_id == expected_tenant.org_id
                && tenant.workspace_id == expected_tenant.workspace_id
                && tenant.deployment_id == expected_tenant.deployment_id,
            "projection snapshot tenant does not match the authorized tenant"
        );
        self.with_connection(|connection| {
            let component = |key: &str| -> anyhow::Result<serde_json::Value> {
                let digest = reference
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .with_context(|| format!("projection snapshot is missing {key}"))?;
                let raw = connection
                    .query_row(
                        "SELECT payload_json FROM goal_projection_blobs WHERE digest = ?1",
                        [digest],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .with_context(|| format!("projection snapshot blob {digest} is missing"))?;
                protected_records::decode(&tenant, "projection", digest, &raw)
            };
            Ok(serde_json::json!({
                "goal": component("goal")?,
                "links": component("links")?,
                "runs": component("runs")?,
                "waits": component("waits")?,
                "handoffs": component("handoffs")?,
            }))
        })
    }

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
            let mut statement = connection.prepare(
                "SELECT event_id, org_id, workspace_id, deployment_id, event_json
                          FROM stateful_events ORDER BY seq, run_id, event_id",
            )?;
            let rows = statement.query_map([], scoped_payload_row)?;
            rows.map(|row| {
                let (id, org, workspace, deployment, payload) = row?;
                protected_records::decode_scoped(
                    &org,
                    &workspace,
                    deployment.as_deref(),
                    "event",
                    &id,
                    &payload,
                )
            })
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
            prune_unreferenced_goal_projection_blobs(&transaction)?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Replaces only the exact event snapshot observed by a compactor.
    /// Events committed after that snapshot are outside `observed_event_ids`
    /// and therefore survive alongside their projection blobs.
    pub fn replace_observed_stateful_runtime_events(
        &self,
        observed_event_ids: &[String],
        replacement_events: &[StatefulRunEventRecord],
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "CREATE TEMP TABLE compaction_observed_events (event_id TEXT PRIMARY KEY)",
                [],
            )?;
            for event_id in observed_event_ids {
                transaction.execute(
                    "INSERT INTO compaction_observed_events (event_id) VALUES (?1)",
                    [event_id],
                )?;
            }
            let present: usize = transaction.query_row(
                "SELECT COUNT(*) FROM stateful_events
                 WHERE event_id IN (SELECT event_id FROM compaction_observed_events)",
                [],
                |row| row.get(0),
            )?;
            if present != observed_event_ids.len() {
                bail!(
                    "stale stateful event compaction snapshot: observed {} events, found {present}",
                    observed_event_ids.len()
                );
            }
            transaction.execute(
                "DELETE FROM stateful_events
                 WHERE event_id IN (SELECT event_id FROM compaction_observed_events)",
                [],
            )?;
            for event in replacement_events {
                insert_event(&transaction, event)?;
            }
            prune_unreferenced_goal_projection_blobs(&transaction)?;
            transaction.execute("DROP TABLE compaction_observed_events", [])?;
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
                    protected_records::encode(
                        &snapshot.scope.tenant_context,
                        "snapshot",
                        &snapshot.snapshot_id,
                        snapshot,
                    )?,
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
                "SELECT snapshot_id, org_id, workspace_id, deployment_id, snapshot_json
                 FROM stateful_snapshots
                 WHERE run_id = ?1 ORDER BY seq, snapshot_id",
            )?;
            let rows = statement.query_map([run_id], scoped_payload_row)?;
            rows.map(|row| {
                let (id, org, workspace, deployment, payload) = row?;
                protected_records::decode_scoped(
                    &org,
                    &workspace,
                    deployment.as_deref(),
                    "snapshot",
                    &id,
                    &payload,
                )
            })
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
                    "SELECT org_id, workspace_id, deployment_id, snapshot_json
                     FROM stateful_snapshots WHERE snapshot_id = ?1",
                    [snapshot_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?;
            payload
                .map(|(org, workspace, deployment, payload)| {
                    protected_records::decode_scoped(
                        &org,
                        &workspace,
                        deployment.as_deref(),
                        "snapshot",
                        snapshot_id,
                        &payload,
                    )
                })
                .transpose()
        })
    }

    pub fn load_stateful_runtime_waits(&self) -> anyhow::Result<Vec<StatefulWaitRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT wait_id || ':' || run_id, org_id, workspace_id, deployment_id, wait_json
                 FROM automation_waits
                 ORDER BY updated_at_ms, wait_id, run_id",
            )?;
            let rows = statement.query_map([], scoped_payload_row)?;
            rows.map(|row| {
                let (id, org, workspace, deployment, payload) = row?;
                protected_records::decode_scoped(
                    &org,
                    &workspace,
                    deployment.as_deref(),
                    "wait",
                    &id,
                    &payload,
                )
            })
            .collect()
        })
    }

    pub fn upsert_stateful_runtime_waits(
        &self,
        waits: &[StatefulWaitRecord],
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for wait in waits {
                insert_wait(&transaction, wait)?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Deletes only records that still match the snapshot the caller pruned.
    /// This prevents a retention sweep from removing a concurrently updated wait.
    pub fn delete_stateful_runtime_waits_if_unchanged(
        &self,
        waits: &[StatefulWaitRecord],
    ) -> anyhow::Result<usize> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut deleted = 0;
            for wait in waits {
                let record_id = format!("{}:{}", wait.wait_id, wait.run_id);
                let current = transaction
                    .query_row(
                        "SELECT wait_json FROM automation_waits
                     WHERE wait_id = ?1 AND run_id = ?2 AND org_id = ?3
                       AND workspace_id = ?4 AND deployment_id = ?5",
                        params![
                            wait.wait_id,
                            wait.run_id,
                            wait.scope.tenant_context.org_id,
                            wait.scope.tenant_context.workspace_id,
                            wait.scope
                                .tenant_context
                                .deployment_id
                                .as_deref()
                                .unwrap_or(""),
                        ],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let unchanged = current
                    .map(|payload| -> anyhow::Result<bool> {
                        let stored: StatefulWaitRecord = protected_records::decode(
                            &wait.scope.tenant_context,
                            "wait",
                            &record_id,
                            &payload,
                        )?;
                        Ok(
                            protected_records::digest(&wait.scope.tenant_context, "wait", &stored)?
                                == protected_records::digest(
                                    &wait.scope.tenant_context,
                                    "wait",
                                    wait,
                                )?,
                        )
                    })
                    .transpose()?
                    .unwrap_or(false);
                if unchanged {
                    deleted += transaction.execute(
                        "DELETE FROM automation_waits
                         WHERE wait_id = ?1 AND run_id = ?2 AND org_id = ?3
                           AND workspace_id = ?4 AND deployment_id = ?5",
                        params![
                            wait.wait_id,
                            wait.run_id,
                            wait.scope.tenant_context.org_id,
                            wait.scope.tenant_context.workspace_id,
                            wait.scope
                                .tenant_context
                                .deployment_id
                                .as_deref()
                                .unwrap_or("")
                        ],
                    )?;
                }
            }
            transaction.commit()?;
            Ok(deleted)
        })
    }

    pub fn load_stateful_runtime_reliability(
        &self,
    ) -> anyhow::Result<StatefulReliabilityStoreFile> {
        self.with_connection(|connection| {
            Ok(StatefulReliabilityStoreFile {
                schema_version: crate::stateful_runtime::STATEFUL_RUNTIME_SCHEMA_VERSION,
                outbox: load_runtime_records(
                    connection,
                    "outbox_effects",
                    "effect_id",
                    "effect_json",
                    "outbox",
                )?,
                tool_effects: load_runtime_records(
                    connection,
                    "tool_effects",
                    "effect_id",
                    "effect_json",
                    "tool_effect",
                )?,
                dead_letters: load_runtime_records(
                    connection,
                    "dead_letters",
                    "dead_letter_id",
                    "record_json",
                    "dead_letter",
                )?,
                compensations: load_runtime_records(
                    connection,
                    "compensations",
                    "compensation_id",
                    "record_json",
                    "compensation",
                )?,
            })
        })
    }

    pub fn upsert_stateful_runtime_reliability(
        &self,
        reliability: &StatefulReliabilityStoreFile,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for row in &reliability.outbox {
                insert_reliability_record(
                    &transaction,
                    "outbox_effects",
                    "effect_id",
                    &row.outbox_id,
                    row.run_id.as_deref(),
                    &row.scope,
                    &row.status,
                    "effect_json",
                    "outbox",
                    row.updated_at_ms,
                    row,
                )?;
            }
            for row in &reliability.tool_effects {
                insert_reliability_record(
                    &transaction,
                    "tool_effects",
                    "effect_id",
                    &row.effect_id,
                    row.run_id.as_deref(),
                    &row.scope,
                    &row.status,
                    "effect_json",
                    "tool_effect",
                    row.updated_at_ms,
                    row,
                )?;
            }
            for row in &reliability.dead_letters {
                insert_reliability_record(
                    &transaction,
                    "dead_letters",
                    "dead_letter_id",
                    &row.dead_letter_id,
                    row.run_id.as_deref(),
                    &row.scope,
                    &row.status,
                    "record_json",
                    "dead_letter",
                    row.updated_at_ms,
                    row,
                )?;
            }
            for row in &reliability.compensations {
                insert_reliability_record(
                    &transaction,
                    "compensations",
                    "compensation_id",
                    &row.compensation_id,
                    row.run_id.as_deref(),
                    &row.scope,
                    &row.status,
                    "record_json",
                    "compensation",
                    row.updated_at_ms,
                    row,
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Deletes only settled reliability rows that still match the retained
    /// snapshot, preserving a record concurrently changed by a recovery path.
    pub fn delete_stateful_runtime_reliability_if_unchanged(
        &self,
        reliability: &StatefulReliabilityStoreFile,
    ) -> anyhow::Result<usize> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut deleted = 0;
            for row in &reliability.outbox {
                deleted += delete_reliability_record(
                    &transaction,
                    "outbox_effects",
                    "effect_id",
                    &row.outbox_id,
                    "effect_json",
                    "outbox",
                    &row.scope,
                    row,
                )?;
            }
            for row in &reliability.tool_effects {
                deleted += delete_reliability_record(
                    &transaction,
                    "tool_effects",
                    "effect_id",
                    &row.effect_id,
                    "effect_json",
                    "tool_effect",
                    &row.scope,
                    row,
                )?;
            }
            for row in &reliability.dead_letters {
                deleted += delete_reliability_record(
                    &transaction,
                    "dead_letters",
                    "dead_letter_id",
                    &row.dead_letter_id,
                    "record_json",
                    "dead_letter",
                    &row.scope,
                    row,
                )?;
            }
            for row in &reliability.compensations {
                deleted += delete_reliability_record(
                    &transaction,
                    "compensations",
                    "compensation_id",
                    &row.compensation_id,
                    "record_json",
                    "compensation",
                    &row.scope,
                    row,
                )?;
            }
            transaction.commit()?;
            Ok(deleted)
        })
    }
}

fn prune_unreferenced_goal_projection_blobs(
    transaction: &rusqlite::Transaction<'_>,
) -> anyhow::Result<()> {
    transaction.execute(
        "CREATE TEMP TABLE retained_projection_blobs (digest TEXT PRIMARY KEY)",
        [],
    )?;
    let payloads = {
        let mut statement = transaction.prepare(
            "SELECT event_id, org_id, workspace_id, deployment_id, event_json FROM stateful_events",
        )?;
        let rows = statement.query_map([], scoped_payload_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (id, org, workspace, deployment, payload) in payloads {
        let event: StatefulRunEventRecord = protected_records::decode_scoped(
            &org,
            &workspace,
            deployment.as_deref(),
            "event",
            &id,
            &payload,
        )?;
        if let Some(reference) = event
            .payload
            .get("projection_snapshot_ref")
            .and_then(serde_json::Value::as_object)
        {
            for digest in reference.values().filter_map(serde_json::Value::as_str) {
                transaction.execute(
                    "INSERT OR IGNORE INTO retained_projection_blobs (digest) VALUES (?1)",
                    [digest],
                )?;
            }
        }
    }
    transaction.execute(
        "DELETE FROM goal_projection_blobs
         WHERE digest NOT IN (SELECT digest FROM retained_projection_blobs)",
        [],
    )?;
    transaction.execute("DROP TABLE retained_projection_blobs", [])?;
    Ok(())
}

fn insert_wait(
    transaction: &rusqlite::Transaction<'_>,
    wait: &StatefulWaitRecord,
) -> anyhow::Result<()> {
    transaction.execute(
        "INSERT INTO automation_waits
            (wait_id, goal_id, run_id, org_id, workspace_id, deployment_id,
             status, wait_json, updated_at_ms)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(wait_id, run_id, org_id, workspace_id, deployment_id) DO UPDATE SET
            status = excluded.status, wait_json = excluded.wait_json,
            updated_at_ms = excluded.updated_at_ms
         WHERE automation_waits.status NOT IN ('woken', 'timed_out', 'escalated', 'cancelled')
            OR (
                automation_waits.status IN ('timed_out', 'escalated')
                AND excluded.status IN ('woken', 'cancelled')
               )",
        params![
            wait.wait_id,
            wait.run_id,
            wait.scope.tenant_context.org_id,
            wait.scope.tenant_context.workspace_id,
            wait.scope
                .tenant_context
                .deployment_id
                .as_deref()
                .unwrap_or(""),
            enum_name(&wait.status)?,
            protected_records::encode(
                &wait.scope.tenant_context,
                "wait",
                &format!("{}:{}", wait.wait_id, wait.run_id),
                wait,
            )?,
            wait.updated_at_ms,
        ],
    )?;
    Ok(())
}

fn load_runtime_records<T>(
    connection: &rusqlite::Connection,
    table: &str,
    id_column: &str,
    json_column: &str,
    kind: &str,
) -> anyhow::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut statement = connection.prepare(&format!(
        "SELECT {id_column}, org_id, workspace_id, deployment_id, {json_column}
         FROM {table} ORDER BY updated_at_ms, rowid"
    ))?;
    let rows = statement.query_map([], scoped_payload_row)?;
    rows.map(|row| {
        let (id, org, workspace, deployment, payload) = row?;
        protected_records::decode_scoped(
            &org,
            &workspace,
            deployment.as_deref(),
            kind,
            &id,
            &payload,
        )
    })
    .collect()
}

#[allow(clippy::too_many_arguments)]
fn insert_reliability_record<T: serde::Serialize, S: serde::Serialize>(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    id_column: &str,
    id: &str,
    run_id: Option<&str>,
    scope: &StatefulRuntimeScope,
    status: &S,
    json_column: &str,
    kind: &str,
    updated_at_ms: u64,
    record: &T,
) -> anyhow::Result<()> {
    let sql = format!(
        "INSERT INTO {table}
            ({id_column}, goal_id, run_id, status, {json_column}, updated_at_ms,
             org_id, workspace_id, deployment_id)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT({id_column}) DO UPDATE SET status = excluded.status,
             {json_column} = excluded.{json_column}, updated_at_ms = excluded.updated_at_ms,
             org_id = excluded.org_id, workspace_id = excluded.workspace_id,
             deployment_id = excluded.deployment_id
         WHERE excluded.updated_at_ms >= {table}.updated_at_ms"
    );
    transaction.execute(
        &sql,
        params![
            id,
            run_id,
            enum_name(status)?,
            protected_records::encode(&scope.tenant_context, kind, id, record)?,
            updated_at_ms,
            scope.tenant_context.org_id,
            scope.tenant_context.workspace_id,
            scope.tenant_context.deployment_id,
        ],
    )?;
    Ok(())
}

fn delete_reliability_record<T: serde::Serialize + serde::de::DeserializeOwned>(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    id_column: &str,
    id: &str,
    json_column: &str,
    kind: &str,
    scope: &StatefulRuntimeScope,
    record: &T,
) -> anyhow::Result<usize> {
    let current = transaction
        .query_row(
            &format!("SELECT {json_column} FROM {table} WHERE {id_column} = ?1"),
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(payload) = current else {
        return Ok(0);
    };
    let stored: T = protected_records::decode(&scope.tenant_context, kind, id, &payload)?;
    if protected_records::digest(&scope.tenant_context, kind, &stored)?
        != protected_records::digest(&scope.tenant_context, kind, record)?
    {
        return Ok(0);
    }
    transaction
        .execute(&format!("DELETE FROM {table} WHERE {id_column} = ?1"), [id])
        .map_err(Into::into)
}

type ScopedPayloadRow = (String, String, String, Option<String>, String);

fn scoped_payload_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScopedPayloadRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn enum_name<T: serde::Serialize>(value: &T) -> anyhow::Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("serialized stateful status was not a string"))
}

fn insert_event(
    transaction: &rusqlite::Transaction<'_>,
    event: &StatefulRunEventRecord,
) -> anyhow::Result<bool> {
    let goal_id = event
        .payload
        .get("goal_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            transaction
                .query_row(
                    "SELECT goal_id FROM goal_run_links WHERE run_id = ?1",
                    [&event.run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
        });
    let has_projection_reference = event.payload.get("projection_snapshot_ref").is_some()
        || event.payload.get("projection_snapshot").is_some();
    let stored_event = match (goal_id.as_deref(), has_projection_reference) {
        (_, true) => event.clone(),
        (Some(goal_id), false) => event_with_projection_snapshot(transaction, event, goal_id)?,
        (None, false) => event.clone(),
    };
    let inserted = transaction.execute(
        "INSERT INTO stateful_events
            (event_id, goal_id, run_id, seq, event_json, created_at_ms,
             org_id, workspace_id, deployment_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(event_id) DO NOTHING",
        params![
            event.event_id,
            goal_id,
            event.run_id,
            event.seq,
            protected_records::encode(
                &stored_event.scope.tenant_context,
                "event",
                &stored_event.event_id,
                &stored_event,
            )?,
            event.occurred_at_ms,
            event.scope.tenant_context.org_id,
            event.scope.tenant_context.workspace_id,
            event.scope.tenant_context.deployment_id,
        ],
    )?;
    Ok(inserted > 0)
}

pub(super) fn event_with_projection_snapshot(
    transaction: &rusqlite::Transaction<'_>,
    event: &StatefulRunEventRecord,
    goal_id: &str,
) -> anyhow::Result<StatefulRunEventRecord> {
    let mut stored_event = event.clone();
    let mut payload = stored_event
        .payload
        .as_object()
        .cloned()
        .unwrap_or_default();
    payload.insert(
        "goal_id".to_string(),
        serde_json::Value::String(goal_id.to_string()),
    );
    payload.insert(
        "projection_snapshot_ref".to_string(),
        projection_snapshot_for_goal(transaction, goal_id)?,
    );
    stored_event.payload = serde_json::Value::Object(payload);
    Ok(stored_event)
}

pub(super) fn projection_snapshot_for_goal(
    transaction: &rusqlite::Transaction<'_>,
    goal_id: &str,
) -> anyhow::Result<serde_json::Value> {
    let goal_row = transaction
        .query_row(
            "SELECT org_id, workspace_id, deployment_id, goal_json
             FROM long_running_goals WHERE goal_id = ?1",
            [goal_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .context("goal projection snapshot is missing its goal")?;
    let (org, workspace, deployment, goal_payload) = goal_row;
    let goal: serde_json::Value = protected_records::decode_scoped(
        &org,
        &workspace,
        deployment.as_deref(),
        "goal",
        goal_id,
        &goal_payload,
    )?;
    let tenant = protected_records::tenant_from_scope(&org, &workspace, deployment.as_deref());
    let active_run_id = goal
        .get("active_run_id")
        .and_then(serde_json::Value::as_str);
    let links = json_rows(
        transaction,
        "SELECT run_id, link_json FROM (
            SELECT run_id, link_json, hop_index FROM goal_run_links
            WHERE goal_id = ?1 ORDER BY hop_index DESC LIMIT 250
         ) ORDER BY hop_index",
        goal_id,
        &tenant,
        "link",
    )?;
    let runs = active_run_id
        .map(|run_id| {
            transaction
                .query_row(
                    "SELECT run_json FROM automation_runs WHERE run_id = ?1",
                    [run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        })
        .transpose()?
        .flatten()
        .map(|raw| -> anyhow::Result<serde_json::Value> {
            let mut run: AutomationV2RunRecord =
                protected_records::decode(&tenant, "run", active_run_id.unwrap_or_default(), &raw)?;
            run.active_session_ids.clear();
            run.latest_session_id = None;
            run.active_instance_ids.clear();
            run.runtime_context = None;
            run.automation_snapshot = None;
            run.execution_claim = None;
            run.scheduler = None;
            run.learning_summary = None;
            Ok(serde_json::to_value(run)?)
        })
        .transpose()?
        .into_iter()
        .collect::<Vec<_>>();
    let waits = json_rows(
        transaction,
        "SELECT wait_id || ':' || run_id, wait_json FROM (
            SELECT w.wait_id, w.run_id, w.wait_json, w.updated_at_ms
            FROM automation_waits w
            INNER JOIN goal_run_links l ON l.run_id = w.run_id
            WHERE l.goal_id = ?1
            ORDER BY w.updated_at_ms DESC, w.wait_id DESC LIMIT 250
         ) ORDER BY updated_at_ms, wait_id",
        goal_id,
        &tenant,
        "wait",
    )?;
    let handoffs = json_rows(
        transaction,
        "SELECT handoff_id, handoff_json FROM (
            SELECT handoff_json, created_at_ms, handoff_id FROM workflow_handoffs
            WHERE goal_id = ?1 ORDER BY created_at_ms DESC, handoff_id DESC LIMIT 250
         ) ORDER BY created_at_ms, handoff_id",
        goal_id,
        &tenant,
        "handoff",
    )?;
    Ok(serde_json::json!({
        "schema_version": 2,
        "tenant_context": tenant.clone(),
        "goal": store_projection_blob(transaction, &tenant, &goal)?,
        "links": store_projection_blob(transaction, &tenant, &links)?,
        "runs": store_projection_blob(transaction, &tenant, &runs)?,
        "waits": store_projection_blob(transaction, &tenant, &waits)?,
        "handoffs": store_projection_blob(transaction, &tenant, &handoffs)?,
    }))
}

fn store_projection_blob<T: serde::Serialize>(
    transaction: &rusqlite::Transaction<'_>,
    tenant: &TenantContext,
    value: &T,
) -> anyhow::Result<String> {
    let digest = protected_records::digest(tenant, "projection", value)?;
    transaction.execute(
        "INSERT INTO goal_projection_blobs (digest, payload_json, created_at_ms)
         VALUES (?1, ?2, ?3) ON CONFLICT(digest) DO NOTHING",
        params![
            digest,
            protected_records::encode(tenant, "projection", &digest, value)?,
            crate::now_ms()
        ],
    )?;
    Ok(digest)
}

fn json_rows(
    transaction: &rusqlite::Transaction<'_>,
    sql: &str,
    value: &str,
    tenant: &TenantContext,
    kind: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut statement = transaction.prepare(sql)?;
    let rows = statement.query_map([value], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (id, payload) = row?;
        protected_records::decode(tenant, kind, &id, &payload)
    })
    .collect()
}

fn event_seq_by_id(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    event_id: &str,
) -> anyhow::Result<Option<u64>> {
    let mut statement = transaction.prepare(
        "SELECT event_id, org_id, workspace_id, deployment_id, event_json
         FROM stateful_events WHERE run_id = ?1 ORDER BY seq, event_id",
    )?;
    let rows = statement.query_map([run_id], scoped_payload_row)?;
    for row in rows {
        let (id, org, workspace, deployment, payload) = row?;
        let event: StatefulRunEventRecord = protected_records::decode_scoped(
            &org,
            &workspace,
            deployment.as_deref(),
            "event",
            &id,
            &payload,
        )
        .context("stored stateful event could not be decoded")?;
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
    use crate::stateful_runtime::{
        StatefulOutboxRecord, StatefulOutboxStatus, StatefulReliabilityStoreFile,
        StatefulRuntimeScope, StatefulWaitKind, StatefulWaitStatus,
    };

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

    fn wait(status: StatefulWaitStatus, updated_at_ms: u64) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-1".to_string(),
            run_id: "run-1".to_string(),
            wait_kind: StatefulWaitKind::Timer,
            status,
            scope: StatefulRuntimeScope::from_tenant_context(
                TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a"),
            ),
            phase_id: None,
            reason: None,
            created_at_ms: 1,
            updated_at_ms,
            wake_at_ms: Some(10),
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

    fn outbox(id: &str, updated_at_ms: u64) -> StatefulOutboxRecord {
        StatefulOutboxRecord {
            schema_version: 1,
            outbox_id: id.to_string(),
            run_id: Some("run-1".to_string()),
            scope: StatefulRuntimeScope::from_tenant_context(
                TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a"),
            ),
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
            created_at_ms: 1,
            updated_at_ms,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            metadata: None,
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

    #[test]
    fn event_replacement_retains_projection_blobs_injected_during_storage() {
        let directory = tempfile::tempdir().expect("create test directory");
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .expect("open orchestration store");
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO long_running_goals
                        (goal_id, orchestration_id, orchestration_version, org_id, workspace_id,
                         deployment_id, status, active_run_id, goal_json, created_at_ms,
                         updated_at_ms)
                     VALUES (?1, ?2, 1, ?3, ?4, NULL, 'active', NULL, ?5, 1, 1)",
                    rusqlite::params![
                        "goal-1",
                        "orchestration-1",
                        "org-a",
                        "workspace-a",
                        serde_json::to_string(&json!({
                            "goal_id": "goal-1",
                            "active_run_id": null,
                        }))?,
                    ],
                )?;
                connection.execute(
                    "INSERT INTO goal_run_links
                        (goal_id, run_id, orchestration_node_id, orchestration_version,
                         hop_index, parent_run_id, triggering_handoff_id, link_json,
                         created_at_ms)
                     VALUES (?1, ?2, ?3, 1, 1, NULL, NULL, '{}', 1)",
                    ["goal-1", "run-1", "node-1"],
                )?;
                Ok(())
            })
            .expect("seed goal projection");

        let mut compacted = event("run-1");
        compacted.event_id = "compacted-event".to_string();
        compacted.event_type = "stateful_runtime.event_log_compacted".to_string();
        store
            .replace_stateful_runtime_events(&[compacted])
            .expect("replace events");

        let stored = store
            .load_stateful_runtime_events()
            .expect("load stored events")
            .pop()
            .expect("stored compacted event");
        let reference = &stored.payload["projection_snapshot_ref"];
        assert!(reference.is_object());
        let projection = store
            .resolve_goal_projection_snapshot(&stored.scope.tenant_context, reference)
            .expect("resolve retained projection snapshot");
        assert_eq!(projection["goal"]["goal_id"], "goal-1");
    }

    #[test]
    fn observed_event_replacement_preserves_late_events_and_projection_blobs() {
        let directory = tempfile::tempdir().expect("create test directory");
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .expect("open orchestration store");
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO long_running_goals
                        (goal_id, orchestration_id, orchestration_version, org_id, workspace_id,
                         deployment_id, status, active_run_id, goal_json, created_at_ms,
                         updated_at_ms)
                     VALUES (?1, ?2, 1, ?3, ?4, NULL, 'active', NULL, ?5, 1, 1)",
                    rusqlite::params![
                        "goal-1",
                        "orchestration-1",
                        "org-a",
                        "workspace-a",
                        serde_json::to_string(&json!({
                            "goal_id": "goal-1",
                            "active_run_id": null,
                        }))?,
                    ],
                )?;
                connection.execute(
                    "INSERT INTO goal_run_links
                        (goal_id, run_id, orchestration_node_id, orchestration_version,
                         hop_index, parent_run_id, triggering_handoff_id, link_json,
                         created_at_ms)
                     VALUES (?1, ?2, ?3, 1, 1, NULL, NULL, '{}', 1)",
                    ["goal-1", "run-1", "node-1"],
                )?;
                Ok(())
            })
            .expect("seed goal projection");

        let mut observed = event("run-1");
        observed.event_id = "observed-event".to_string();
        observed.seq = 1;
        store
            .replace_stateful_runtime_events(&[observed.clone()])
            .expect("seed observed snapshot");

        let mut late = event("run-1");
        late.event_id = "late-event".to_string();
        late.seq = 2;
        store
            .append_stateful_runtime_event(&late)
            .expect("append event after compaction snapshot");
        let late_before = store
            .load_stateful_runtime_events()
            .unwrap()
            .into_iter()
            .find(|row| row.event_id == late.event_id)
            .unwrap();
        let late_projection_ref = late_before.payload["projection_snapshot_ref"].clone();
        assert!(late_projection_ref.is_object());

        let mut marker = event("run-1");
        marker.event_id = "compaction-marker".to_string();
        marker.seq = 1;
        marker.event_type = "stateful_runtime.event_log_compacted".to_string();
        store
            .replace_observed_stateful_runtime_events(&[observed.event_id], &[marker])
            .expect("replace only the observed snapshot");

        let ids = store
            .load_stateful_runtime_events()
            .unwrap()
            .into_iter()
            .map(|row| row.event_id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            ids,
            ["compaction-marker".to_string(), "late-event".to_string()]
                .into_iter()
                .collect()
        );
        let projection = store
            .resolve_goal_projection_snapshot(
                &late_before.scope.tenant_context,
                &late_projection_ref,
            )
            .expect("late event projection blobs survive compaction");
        assert_eq!(projection["goal"]["goal_id"], "goal-1");
    }

    #[test]
    fn observed_event_replacement_rejects_a_stale_snapshot() {
        let directory = tempfile::tempdir().expect("create test directory");
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .expect("open orchestration store");
        let mut stored = event("run-1");
        stored.event_id = "stored-event".to_string();
        stored.seq = 1;
        store
            .append_stateful_runtime_event(&stored)
            .expect("seed event");
        let mut replacement = event("run-1");
        replacement.event_id = "replacement-event".to_string();
        replacement.seq = 1;

        let error = store
            .replace_observed_stateful_runtime_events(
                &["already-compacted-event".to_string()],
                &[replacement],
            )
            .expect_err("stale snapshot must abort");
        assert!(error
            .to_string()
            .contains("stale stateful event compaction snapshot"));
        assert_eq!(store.load_stateful_runtime_events().unwrap(), vec![stored]);
    }

    #[test]
    fn stale_wait_upsert_cannot_revive_a_terminal_wait() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .unwrap();
        let stale = wait(StatefulWaitStatus::Waiting, 10);
        store
            .upsert_stateful_runtime_waits(&[stale.clone()])
            .unwrap();

        let mut cancelled = stale.clone();
        cancelled.status = StatefulWaitStatus::Cancelled;
        cancelled.updated_at_ms = 20;
        cancelled.completed_at_ms = Some(20);
        store.upsert_stateful_runtime_waits(&[cancelled]).unwrap();
        store.upsert_stateful_runtime_waits(&[stale]).unwrap();

        let rows = store.load_stateful_runtime_waits().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, StatefulWaitStatus::Cancelled);
    }

    #[test]
    fn approval_settlement_can_close_an_escalated_wait_with_an_earlier_scheduler_clock() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .unwrap();
        let mut waiting = wait(StatefulWaitStatus::Waiting, 10);
        waiting.wait_kind = StatefulWaitKind::Approval;
        store
            .upsert_stateful_runtime_waits(&[waiting.clone()])
            .unwrap();

        let mut escalated = waiting.clone();
        escalated.status = StatefulWaitStatus::Escalated;
        escalated.updated_at_ms = 20;
        escalated.completed_at_ms = Some(20);
        store.upsert_stateful_runtime_waits(&[escalated]).unwrap();

        let mut settled = waiting;
        settled.status = StatefulWaitStatus::Woken;
        settled.updated_at_ms = 5;
        settled.completed_at_ms = Some(5);
        store.upsert_stateful_runtime_waits(&[settled]).unwrap();

        let rows = store.load_stateful_runtime_waits().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, StatefulWaitStatus::Woken);
    }

    #[test]
    fn reliability_upsert_preserves_concurrently_inserted_outbox_rows() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::from_automation_runs_path(
            &directory.path().join("automation_v2_runs.json"),
        )
        .unwrap();
        let stale = StatefulReliabilityStoreFile {
            schema_version: 1,
            outbox: vec![outbox("outbox-stale", 10)],
            ..Default::default()
        };
        store.upsert_stateful_runtime_reliability(&stale).unwrap();
        let concurrent = outbox("goal-cancel:goal-1:run-1", 20);
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO outbox_effects
                        (effect_id, goal_id, run_id, status, effect_json, updated_at_ms,
                         org_id, workspace_id, deployment_id)
                     VALUES (?1, NULL, ?2, 'pending', ?3, ?4, ?5, ?6, ?7)",
                    params![
                        concurrent.outbox_id,
                        concurrent.run_id,
                        serde_json::to_string(&concurrent)?,
                        concurrent.updated_at_ms,
                        concurrent.scope.tenant_context.org_id,
                        concurrent.scope.tenant_context.workspace_id,
                        concurrent.scope.tenant_context.deployment_id,
                    ],
                )?;
                Ok(())
            })
            .unwrap();

        store.upsert_stateful_runtime_reliability(&stale).unwrap();
        let records = store.load_stateful_runtime_reliability().unwrap();
        assert_eq!(records.outbox.len(), 2);
        assert!(records
            .outbox
            .iter()
            .any(|row| row.outbox_id == concurrent.outbox_id));
    }
}
