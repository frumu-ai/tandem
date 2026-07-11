use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tandem_automation::{AutomationV2RunRecord, HandoffArtifact};

use super::{collect_json_files, upsert_automation_run, OrchestrationStateStore};
use crate::stateful_runtime::{
    stable_definition_snapshot_hash, StatefulReliabilityStoreFile, StatefulRunEventRecord,
    StatefulRunSnapshotRecord, StatefulWaitRecord,
};

const LEGACY_RUNTIME_MIGRATION_ID: &str = "legacy_stateful_runtime_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRuntimeMigrationPaths {
    pub automation_runs_path: PathBuf,
    pub run_events_path: PathBuf,
    pub snapshots_root: PathBuf,
    pub waits_path: PathBuf,
    pub reliability_path: PathBuf,
    pub handoff_root: Option<PathBuf>,
}

impl LegacyRuntimeMigrationPaths {
    pub fn from_runtime_root(runtime_root: &Path) -> Self {
        Self {
            automation_runs_path: runtime_root.join("automation_v2_runs.json"),
            run_events_path: runtime_root.join("stateful_events.jsonl"),
            snapshots_root: runtime_root.join("stateful_snapshots"),
            waits_path: runtime_root.join("stateful_waits.json"),
            reliability_path: runtime_root.join("stateful_reliability.json"),
            handoff_root: None,
        }
    }

    /// Builds migration sources from the live paths, which may be configured
    /// outside the default runtime root in a desktop or test deployment.
    pub fn from_runtime_paths(automation_runs_path: PathBuf, runtime_events_path: &Path) -> Self {
        let runtime_root = runtime_events_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        Self {
            automation_runs_path,
            run_events_path: runtime_root.join("stateful_events.jsonl"),
            snapshots_root: runtime_root.join("stateful_snapshots"),
            waits_path: runtime_root.join("stateful_waits.json"),
            reliability_path: runtime_root.join("stateful_reliability.json"),
            handoff_root: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyRuntimeMigrationReport {
    pub already_complete: bool,
    pub automation_runs: usize,
    pub events: usize,
    pub snapshots: usize,
    pub waits: usize,
    pub outbox: usize,
    pub tool_effects: usize,
    pub dead_letters: usize,
    pub compensations: usize,
    pub handoffs: usize,
    pub quarantined_handoffs: usize,
}

impl LegacyRuntimeMigrationReport {
    fn total_records(&self) -> usize {
        self.automation_runs
            + self.events
            + self.snapshots
            + self.waits
            + self.outbox
            + self.tool_effects
            + self.dead_letters
            + self.compensations
            + self.handoffs
            + self.quarantined_handoffs
    }
}

#[derive(Serialize)]
struct LegacyRuntimeRows {
    automation_runs: Vec<AutomationV2RunRecord>,
    events: Vec<StatefulRunEventRecord>,
    snapshots: Vec<StatefulRunSnapshotRecord>,
    waits: Vec<StatefulWaitRecord>,
    reliability: StatefulReliabilityStoreFile,
    handoffs: LegacyHandoffRows,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LegacyHandoffRows {
    pub(super) imported: Vec<(PathBuf, HandoffArtifact, String)>,
    pub(super) quarantined: Vec<LegacyHandoffQuarantine>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct LegacyHandoffQuarantine {
    pub(super) source_path: PathBuf,
    pub(super) source_digest: Option<String>,
    pub(super) error: String,
}

impl OrchestrationStateStore {
    /// Imports every legacy stateful-runtime store in one SQLite transaction.
    /// Source files are read-only migration backups. The completion marker is
    /// committed with the rows, so a crash exposes either no import or all of it.
    pub fn import_legacy_runtime_state(
        &self,
        paths: &LegacyRuntimeMigrationPaths,
        imported_at_ms: u64,
    ) -> anyhow::Result<LegacyRuntimeMigrationReport> {
        self.with_connection(|connection| {
            let existing = connection
                .query_row(
                    "SELECT status, source_fingerprint FROM stateful_migrations
                     WHERE migration_id = ?1",
                    [LEGACY_RUNTIME_MIGRATION_ID],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((status, _existing_fingerprint)) = existing {
                if status == "complete" {
                    return Ok(LegacyRuntimeMigrationReport {
                        already_complete: true,
                        ..Default::default()
                    });
                }
            }

            let rows = load_legacy_rows(paths)?;
            let fingerprint = stable_definition_snapshot_hash(&rows);
            let report = LegacyRuntimeMigrationReport {
                automation_runs: rows.automation_runs.len(),
                events: rows.events.len(),
                snapshots: rows.snapshots.len(),
                waits: rows.waits.len(),
                outbox: rows.reliability.outbox.len(),
                tool_effects: rows.reliability.tool_effects.len(),
                dead_letters: rows.reliability.dead_letters.len(),
                compensations: rows.reliability.compensations.len(),
                handoffs: rows.handoffs.imported.len(),
                quarantined_handoffs: rows.handoffs.quarantined.len(),
                ..Default::default()
            };

            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            transaction.execute(
                "INSERT INTO stateful_migrations
                    (migration_id, status, source_fingerprint, record_count,
                     started_at_ms, completed_at_ms)
                 VALUES (?1, 'in_progress', ?2, 0, ?3, NULL)
                 ON CONFLICT(migration_id) DO UPDATE SET
                    status = 'in_progress',
                    source_fingerprint = excluded.source_fingerprint,
                    record_count = 0,
                    started_at_ms = excluded.started_at_ms,
                    completed_at_ms = NULL",
                params![LEGACY_RUNTIME_MIGRATION_ID, fingerprint, imported_at_ms],
            )?;
            import_rows(&transaction, &rows, imported_at_ms)?;
            transaction.execute(
                "UPDATE stateful_migrations SET status = 'complete', record_count = ?2,
                    completed_at_ms = ?3 WHERE migration_id = ?1",
                params![
                    LEGACY_RUNTIME_MIGRATION_ID,
                    report.total_records() as u64,
                    imported_at_ms
                ],
            )?;
            transaction.commit()?;
            Ok(report)
        })
    }

    pub fn legacy_runtime_migration_complete(&self) -> anyhow::Result<bool> {
        self.with_connection(|connection| {
            Ok(connection
                .query_row(
                    "SELECT status FROM stateful_migrations WHERE migration_id = ?1",
                    [LEGACY_RUNTIME_MIGRATION_ID],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .is_some_and(|status| status == "complete"))
        })
    }
}

fn load_legacy_rows(paths: &LegacyRuntimeMigrationPaths) -> anyhow::Result<LegacyRuntimeRows> {
    let automation_runs = load_automation_runs(&paths.automation_runs_path)?;
    let events = load_jsonl_strict(&paths.run_events_path, "stateful event")?;
    validate_event_sequences(&events)?;
    let snapshots = load_snapshots(&paths.snapshots_root)?;
    let waits = load_json_file_or_default(&paths.waits_path)?;
    let reliability = load_json_file_or_default(&paths.reliability_path)?;
    let handoffs = load_legacy_handoffs(paths.handoff_root.as_deref())?;

    Ok(LegacyRuntimeRows {
        automation_runs,
        events,
        snapshots,
        waits,
        reliability,
        handoffs,
    })
}

fn load_automation_runs(path: &Path) -> anyhow::Result<Vec<AutomationV2RunRecord>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let (runs, _) = crate::app::state::parse_automation_v2_runs_file(&raw)?;
    let mut rows = runs.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.run_id.cmp(&right.run_id));
    Ok(rows)
}

fn load_jsonl_strict<T: DeserializeOwned>(path: &Path, label: &str) -> anyhow::Result<Vec<T>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    raw.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("invalid {label} at {}:{}", path.display(), index + 1))
        })
        .collect()
}

fn load_json_file_or_default<T>(path: &Path) -> anyhow::Result<T>
where
    T: DeserializeOwned + Default,
{
    match std::fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(T::default()),
        Ok(raw) => serde_json::from_str(&raw)
            .with_context(|| format!("invalid legacy state file {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn load_snapshots(root: &Path) -> anyhow::Result<Vec<StatefulRunSnapshotRecord>> {
    let mut paths = Vec::new();
    collect_json_files(root, &mut paths)?;
    let mut rows = paths
        .into_iter()
        .map(|path| crate::stateful_runtime::read_stateful_run_snapshot(&path))
        .collect::<anyhow::Result<Vec<_>>>()?;
    rows.sort_by(|left: &StatefulRunSnapshotRecord, right| {
        (&left.run_id, left.seq).cmp(&(&right.run_id, right.seq))
    });
    Ok(rows)
}

pub(super) fn load_legacy_handoffs(root: Option<&Path>) -> anyhow::Result<LegacyHandoffRows> {
    let Some(root) = root else {
        return Ok(LegacyHandoffRows {
            imported: Vec::new(),
            quarantined: Vec::new(),
        });
    };
    let mut paths = Vec::new();
    collect_json_files(root, &mut paths)?;
    paths.sort();
    let mut imported = Vec::new();
    let mut quarantined = Vec::new();
    for path in paths {
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                quarantined.push(LegacyHandoffQuarantine {
                    source_path: path,
                    source_digest: None,
                    error: format!("failed to read legacy handoff: {error}"),
                });
                continue;
            }
        };
        let source_digest = stable_definition_snapshot_hash(&raw);
        let handoff = match serde_json::from_str::<HandoffArtifact>(&raw) {
            Ok(handoff) => handoff,
            Err(error) => {
                quarantined.push(LegacyHandoffQuarantine {
                    source_path: path,
                    source_digest: Some(source_digest),
                    error: format!("invalid legacy handoff: {error}"),
                });
                continue;
            }
        };
        let status = if handoff.consumed_by_run_id.is_some() {
            "archived"
        } else if path.components().any(|part| part.as_os_str() == "approved") {
            "approved"
        } else {
            "inbox"
        };
        imported.push((path, handoff, status.to_string()));
    }
    Ok(LegacyHandoffRows {
        imported,
        quarantined,
    })
}

fn validate_event_sequences(events: &[StatefulRunEventRecord]) -> anyhow::Result<()> {
    let mut seen = std::collections::HashSet::new();
    for event in events {
        let key = (event.run_id.as_str(), event.seq);
        if !seen.insert(key) {
            bail!(
                "duplicate stateful event sequence {} for run {}",
                event.seq,
                event.run_id
            );
        }
    }
    Ok(())
}

fn import_rows(
    transaction: &Transaction<'_>,
    rows: &LegacyRuntimeRows,
    imported_at_ms: u64,
) -> anyhow::Result<()> {
    for run in &rows.automation_runs {
        upsert_automation_run(transaction, run)?;
    }
    for event in &rows.events {
        transaction.execute(
            "INSERT INTO stateful_events
                (event_id, goal_id, run_id, seq, event_json, created_at_ms,
                 org_id, workspace_id, deployment_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(event_id) DO NOTHING",
            params![
                event.event_id,
                goal_id_from_value(&event.payload),
                event.run_id,
                event.seq,
                serde_json::to_string(event)?,
                event.occurred_at_ms,
                event.scope.tenant_context.org_id,
                event.scope.tenant_context.workspace_id,
                event.scope.tenant_context.deployment_id,
            ],
        )?;
    }
    for snapshot in &rows.snapshots {
        transaction.execute(
            "INSERT INTO stateful_snapshots
                (snapshot_id, goal_id, run_id, seq, snapshot_json, created_at_ms,
                 org_id, workspace_id, deployment_id)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(snapshot_id) DO NOTHING",
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
    }
    for wait in &rows.waits {
        transaction.execute(
            "INSERT INTO automation_waits
                (wait_id, goal_id, run_id, status, wait_json, updated_at_ms,
                 org_id, workspace_id, deployment_id)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(wait_id) DO UPDATE SET wait_json = excluded.wait_json,
                 status = excluded.status, updated_at_ms = excluded.updated_at_ms",
            params![
                wait.wait_id,
                wait.run_id,
                enum_name(&wait.status)?,
                serde_json::to_string(wait)?,
                wait.updated_at_ms,
                wait.scope.tenant_context.org_id,
                wait.scope.tenant_context.workspace_id,
                wait.scope.tenant_context.deployment_id,
            ],
        )?;
    }
    import_reliability(transaction, &rows.reliability)?;
    for (path, handoff, status) in &rows.handoffs.imported {
        transaction.execute(
            "INSERT INTO legacy_handoffs
                (handoff_id, source_path, status, consumed_by_run_id, handoff_json,
                 created_at_ms, imported_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(handoff_id) DO UPDATE SET source_path = excluded.source_path,
                 status = excluded.status, consumed_by_run_id = excluded.consumed_by_run_id,
                 handoff_json = excluded.handoff_json, imported_at_ms = excluded.imported_at_ms",
            params![
                handoff.handoff_id,
                path.to_string_lossy(),
                status,
                handoff.consumed_by_run_id,
                serde_json::to_string(handoff)?,
                handoff.created_at_ms,
                imported_at_ms,
            ],
        )?;
    }
    for quarantine in &rows.handoffs.quarantined {
        transaction.execute(
            "INSERT INTO legacy_handoff_quarantine
                (source_path, source_digest, error, quarantined_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source_path) DO UPDATE SET source_digest = excluded.source_digest,
                 error = excluded.error, quarantined_at_ms = excluded.quarantined_at_ms",
            params![
                quarantine.source_path.to_string_lossy(),
                quarantine.source_digest,
                quarantine.error,
                imported_at_ms,
            ],
        )?;
    }
    Ok(())
}

fn import_reliability(
    transaction: &Transaction<'_>,
    reliability: &StatefulReliabilityStoreFile,
) -> anyhow::Result<()> {
    for row in &reliability.outbox {
        insert_reliability_row(
            transaction,
            "outbox_effects",
            "effect_id",
            &row.outbox_id,
            row.run_id.as_deref(),
            &row.scope,
            enum_name(&row.status)?,
            row.updated_at_ms,
            row,
        )?;
    }
    for row in &reliability.tool_effects {
        insert_reliability_row(
            transaction,
            "tool_effects",
            "effect_id",
            &row.effect_id,
            row.run_id.as_deref(),
            &row.scope,
            enum_name(&row.status)?,
            row.updated_at_ms,
            row,
        )?;
    }
    for row in &reliability.dead_letters {
        insert_reliability_row(
            transaction,
            "dead_letters",
            "dead_letter_id",
            &row.dead_letter_id,
            row.run_id.as_deref(),
            &row.scope,
            enum_name(&row.status)?,
            row.updated_at_ms,
            row,
        )?;
    }
    for row in &reliability.compensations {
        insert_reliability_row(
            transaction,
            "compensations",
            "compensation_id",
            &row.compensation_id,
            row.run_id.as_deref(),
            &row.scope,
            enum_name(&row.status)?,
            row.updated_at_ms,
            row,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_reliability_row<T: Serialize>(
    transaction: &Transaction<'_>,
    table: &str,
    id_column: &str,
    id: &str,
    run_id: Option<&str>,
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    status: String,
    updated_at_ms: u64,
    row: &T,
) -> anyhow::Result<()> {
    let json_column = if table == "outbox_effects" || table == "tool_effects" {
        "effect_json"
    } else {
        "record_json"
    };
    let sql = format!(
        "INSERT INTO {table}
            ({id_column}, goal_id, run_id, status, {json_column}, updated_at_ms,
             org_id, workspace_id, deployment_id)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT({id_column}) DO UPDATE SET status = excluded.status,
             {json_column} = excluded.{json_column}, updated_at_ms = excluded.updated_at_ms"
    );
    transaction.execute(
        &sql,
        params![
            id,
            run_id,
            status,
            serde_json::to_string(row)?,
            updated_at_ms,
            scope.tenant_context.org_id,
            scope.tenant_context.workspace_id,
            scope.tenant_context.deployment_id,
        ],
    )?;
    Ok(())
}

fn enum_name<T: Serialize>(value: &T) -> anyhow::Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_string)
        .context("serialized state must be a string")
}

fn goal_id_from_value(value: &Value) -> Option<&str> {
    value.get("goal_id").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::TenantContext;

    use super::*;
    use crate::stateful_runtime::{
        OrchestrationStorePaths, StatefulRuntimeScope, StatefulWorkflowPhase,
        StatefulWorkflowRunStatus,
    };

    fn store(root: &Path) -> OrchestrationStateStore {
        OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: root.join("runtime.sqlite3"),
            engine_lock_path: root.join("engine.lock"),
        })
        .unwrap()
    }

    fn event() -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: "event-1".to_string(),
            run_id: "run-1".to_string(),
            seq: 7,
            event_type: "stateful_runtime.test".to_string(),
            occurred_at_ms: 10,
            scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
            actor: None,
            phase_id: None,
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({"goal_id": "goal-1"}),
        }
    }

    fn snapshot() -> StatefulRunSnapshotRecord {
        StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-1".to_string(),
            run_id: "run-1".to_string(),
            seq: 7,
            created_at_ms: 11,
            scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
            status: StatefulWorkflowRunStatus::Running,
            phase: StatefulWorkflowPhase::RunningPhase,
            phase_history: Vec::new(),
            allowed_next_phases: Vec::new(),
            phase_id: None,
            source_record_kind: None,
            checkpoint: Some(json!({"completed_nodes": ["plan"]})),
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        }
    }

    fn wait() -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-1".to_string(),
            run_id: "run-1".to_string(),
            wait_kind: crate::stateful_runtime::StatefulWaitKind::Timer,
            status: crate::stateful_runtime::StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
            phase_id: None,
            reason: None,
            created_at_ms: 12,
            updated_at_ms: 12,
            wake_at_ms: Some(20),
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

    fn handoff() -> HandoffArtifact {
        HandoffArtifact {
            handoff_id: "handoff-1".to_string(),
            source_automation_id: "planner".to_string(),
            source_run_id: "run-1".to_string(),
            source_node_id: "plan".to_string(),
            target_automation_id: "executor".to_string(),
            artifact_type: "plan".to_string(),
            created_at_ms: 13,
            content_path: Some("artifacts/plan.json".to_string()),
            content_digest: Some("sha256:plan".to_string()),
            metadata: None,
            consumed_by_run_id: None,
            consumed_by_automation_id: None,
            consumed_at_ms: None,
        }
    }

    #[test]
    fn legacy_import_is_atomic_idempotent_and_preserves_sources() {
        let directory = tempfile::tempdir().unwrap();
        let mut paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        std::fs::write(
            &paths.run_events_path,
            format!("{}\n", serde_json::to_string(&event()).unwrap()),
        )
        .unwrap();
        let snapshot_dir = paths.snapshots_root.join("run-1");
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        std::fs::write(
            snapshot_dir.join("snapshot-1.json"),
            serde_json::to_vec(&snapshot()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &paths.waits_path,
            serde_json::to_vec(&vec![wait()]).unwrap(),
        )
        .unwrap();
        let handoff_root = directory.path().join("handoffs");
        let approved = handoff_root.join("approved");
        std::fs::create_dir_all(&approved).unwrap();
        let valid_handoff = approved.join("handoff-1.json");
        let corrupt_handoff = handoff_root.join("corrupt.json");
        std::fs::write(&valid_handoff, serde_json::to_vec(&handoff()).unwrap()).unwrap();
        std::fs::write(&corrupt_handoff, "{not-json}").unwrap();
        paths.handoff_root = Some(handoff_root);

        let store = store(directory.path());
        let first = store.import_legacy_runtime_state(&paths, 100).unwrap();
        assert!(!first.already_complete);
        assert_eq!((first.events, first.snapshots, first.waits), (1, 1, 1));
        assert_eq!((first.handoffs, first.quarantined_handoffs), (1, 1));
        assert!(store.legacy_runtime_migration_complete().unwrap());
        assert!(paths.run_events_path.exists());
        assert!(paths.snapshots_root.exists());
        assert!(paths.waits_path.exists());
        assert!(valid_handoff.exists());
        assert!(corrupt_handoff.exists());

        let second = store.import_legacy_runtime_state(&paths, 200).unwrap();
        assert!(second.already_complete);
        store
            .with_connection(|connection| {
                let events: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM stateful_events", [], |row| row.get(0))?;
                let snapshots: u64 =
                    connection.query_row("SELECT COUNT(*) FROM stateful_snapshots", [], |row| {
                        row.get(0)
                    })?;
                let waits: u64 =
                    connection.query_row("SELECT COUNT(*) FROM automation_waits", [], |row| {
                        row.get(0)
                    })?;
                let handoffs: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM legacy_handoffs", [], |row| row.get(0))?;
                let quarantine: (Option<String>, String) = connection.query_row(
                    "SELECT source_digest, error FROM legacy_handoff_quarantine",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!((events, snapshots, waits, handoffs), (1, 1, 1, 1));
                assert!(quarantine.0.is_some());
                assert!(quarantine.1.contains("invalid legacy handoff"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn corrupt_legacy_row_does_not_publish_migration_marker() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        std::fs::write(&paths.run_events_path, "{not-json}\n").unwrap();
        let store = store(directory.path());

        assert!(store.import_legacy_runtime_state(&paths, 100).is_err());
        assert!(!store.legacy_runtime_migration_complete().unwrap());
        store
            .with_connection(|connection| {
                let events: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM stateful_events", [], |row| row.get(0))?;
                assert_eq!(events, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn completed_migration_keeps_sqlite_authoritative_when_legacy_files_change() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        std::fs::write(
            &paths.run_events_path,
            format!("{}\n", serde_json::to_string(&event()).unwrap()),
        )
        .unwrap();
        let store = store(directory.path());

        store.import_legacy_runtime_state(&paths, 100).unwrap();
        let mut later_event = event();
        later_event.event_id = "event-written-after-cutover".to_string();
        later_event.seq = 8;
        std::fs::write(
            &paths.run_events_path,
            format!("{}\n", serde_json::to_string(&later_event).unwrap()),
        )
        .unwrap();

        let report = store.import_legacy_runtime_state(&paths, 200).unwrap();
        assert!(report.already_complete);
        store
            .with_connection(|connection| {
                let event_count: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM stateful_events", [], |row| row.get(0))?;
                assert_eq!(event_count, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn direct_handoff_import_quarantines_bad_envelopes() {
        let directory = tempfile::tempdir().unwrap();
        let handoff_root = directory.path().join("handoffs");
        let approved = handoff_root.join("approved");
        std::fs::create_dir_all(&approved).unwrap();
        std::fs::write(
            approved.join("handoff-1.json"),
            serde_json::to_vec(&handoff()).unwrap(),
        )
        .unwrap();
        let corrupt = handoff_root.join("corrupt.json");
        std::fs::write(&corrupt, "{not-json}").unwrap();
        let store = store(directory.path());

        assert_eq!(
            store
                .import_legacy_handoff_directory(&handoff_root, 100)
                .unwrap(),
            1
        );
        assert!(corrupt.exists());
        store
            .with_connection(|connection| {
                let handoffs: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM legacy_handoffs", [], |row| row.get(0))?;
                let quarantined: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM legacy_handoff_quarantine",
                    [],
                    |row| row.get(0),
                )?;
                let source_count: u64 = connection.query_row(
                    "SELECT record_count FROM migration_sources
                     WHERE source_kind = 'legacy_handoffs'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!((handoffs, quarantined, source_count), (1, 1, 2));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn existing_v2_store_upgrades_to_handoff_quarantine_schema() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("runtime.sqlite3");
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_metadata (schema_version INTEGER NOT NULL);
                 INSERT INTO schema_metadata (schema_version) VALUES (2);",
            )
            .unwrap();
        drop(connection);

        let store = store(directory.path());
        store
            .with_connection(|connection| {
                let version: u64 = connection.query_row(
                    "SELECT schema_version FROM schema_metadata",
                    [],
                    |row| row.get(0),
                )?;
                let table: String = connection.query_row(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'table' AND name = 'legacy_handoff_quarantine'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(version, 3);
                assert_eq!(table, "legacy_handoff_quarantine");
                Ok(())
            })
            .unwrap();
    }
}
