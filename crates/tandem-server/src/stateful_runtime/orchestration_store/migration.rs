use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::stateful_runtime::backend::{params, Executor, OptionalExtension, Transaction};
use anyhow::{bail, Context};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tandem_automation::{AutomationV2RunRecord, HandoffArtifact};
use tandem_types::TenantContext;

use super::{
    collect_json_files, protected_records, upsert_automation_run, OrchestrationStateStore,
};
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

/// Runtime knowledge used to separate this root's legacy envelopes from
/// foreign ones. `known_automation_ids` is the union of automations the
/// caller can name; the importer extends it with every automation that has a
/// legacy run on this root. An empty union is not evidence of ownership, so
/// envelopes are quarantined until the caller can name a matching automation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyImportContext {
    pub known_automation_ids: BTreeSet<String>,
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
        self.import_legacy_runtime_state_with_context(
            paths,
            &LegacyImportContext::default(),
            imported_at_ms,
        )
    }

    pub fn import_legacy_runtime_state_with_context(
        &self,
        paths: &LegacyRuntimeMigrationPaths,
        context: &LegacyImportContext,
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

            let rows = load_legacy_rows(paths, context)?;
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

            // The attempt row commits before the import transaction so an
            // interrupted import leaves durable evidence. The atomic marker
            // below rolls back with the rows, which is correct for retries
            // but indistinguishable from "never attempted" on its own.
            let aborted_attempts = connection.query_row(
                "SELECT COUNT(*) FROM stateful_migration_attempts
                 WHERE migration_id = ?1 AND outcome IS NULL",
                [LEGACY_RUNTIME_MIGRATION_ID],
                |row| row.get::<_, i64>(0),
            )?;
            if aborted_attempts > 0 {
                tracing::warn!(
                    aborted_attempts,
                    "a previous legacy runtime migration attempt did not complete; retrying from unchanged sources"
                );
            }
            let attempt_id: i64 = connection.query_row(
                "INSERT INTO stateful_migration_attempts
                    (migration_id, source_fingerprint, started_at_ms)
                 VALUES (?1, ?2, ?3)
                 RETURNING attempt_id",
                params![LEGACY_RUNTIME_MIGRATION_ID, fingerprint, imported_at_ms],
                |row| row.get(0),
            )?;

            let transaction =
                connection.transaction_with_behavior(
                crate::stateful_runtime::backend::TransactionBehavior::Immediate,
            )?;
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
            transaction.execute(
                "UPDATE stateful_migration_attempts
                 SET outcome = 'complete', completed_at_ms = ?2
                 WHERE attempt_id = ?1",
                params![attempt_id, imported_at_ms],
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

fn load_legacy_rows(
    paths: &LegacyRuntimeMigrationPaths,
    context: &LegacyImportContext,
) -> anyhow::Result<LegacyRuntimeRows> {
    let automation_runs = load_automation_runs(&paths.automation_runs_path)?;
    let events = load_jsonl_strict(&paths.run_events_path, "stateful event")?;
    validate_event_sequences(&events)?;
    let snapshots = load_snapshots(&paths.snapshots_root)?;
    let waits = load_json_file_or_default(&paths.waits_path)?;
    let reliability = load_json_file_or_default(&paths.reliability_path)?;
    // Every automation with a legacy run on this root is locally known even
    // when the caller could not name it (e.g. the spec was deleted).
    let mut known_automation_ids = context.known_automation_ids.clone();
    known_automation_ids.extend(automation_runs.iter().map(|run| run.automation_id.clone()));
    let handoffs =
        load_legacy_handoffs(paths.handoff_root.as_deref(), Some(&known_automation_ids))?;

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

pub(super) fn load_legacy_handoffs(
    root: Option<&Path>,
    known_automation_ids: Option<&BTreeSet<String>>,
) -> anyhow::Result<LegacyHandoffRows> {
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
    let mut digests_by_handoff_id = std::collections::HashMap::<String, String>::new();
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
        if let Err(reason) = validate_legacy_handoff(&handoff, known_automation_ids) {
            quarantined.push(LegacyHandoffQuarantine {
                source_path: path,
                source_digest: Some(source_digest),
                error: reason,
            });
            continue;
        }
        match digests_by_handoff_id.get(&handoff.handoff_id) {
            // The same envelope copied to another location imports once.
            Some(existing) if existing == &source_digest => continue,
            // Two different envelopes claiming one identity: keep the first,
            // quarantine the impostor instead of silently overwriting.
            Some(_) => {
                quarantined.push(LegacyHandoffQuarantine {
                    source_path: path,
                    source_digest: Some(source_digest),
                    error: format!(
                        "conflicting legacy handoff: id `{}` already imported with different content",
                        handoff.handoff_id
                    ),
                });
                continue;
            }
            None => {
                digests_by_handoff_id.insert(handoff.handoff_id.clone(), source_digest);
            }
        }
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

/// Semantic validation of a well-formed legacy envelope. Malformed JSON is
/// caught earlier; this rejects envelopes that parse but must not import:
/// missing identity, workspace-escaping content paths, and foreign envelopes
/// naming automations this runtime has never known.
fn validate_legacy_handoff(
    handoff: &HandoffArtifact,
    known_automation_ids: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    for (field, value) in [
        ("handoff_id", &handoff.handoff_id),
        ("source_automation_id", &handoff.source_automation_id),
        ("target_automation_id", &handoff.target_automation_id),
        ("artifact_type", &handoff.artifact_type),
    ] {
        if value.trim().is_empty() {
            return Err(format!("invalid legacy handoff: empty {field}"));
        }
    }
    if let Some(content_path) = handoff.content_path.as_deref() {
        let path = Path::new(content_path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!(
                "invalid legacy handoff: content_path `{content_path}` escapes the workspace root"
            ));
        }
    }
    let has_local_automation = known_automation_ids.is_some_and(|known| {
        known.contains(&handoff.source_automation_id)
            || known.contains(&handoff.target_automation_id)
    });
    if !has_local_automation {
        return Err(format!(
            "foreign legacy handoff: neither source `{}` nor target `{}` is an automation known to this runtime root",
            handoff.source_automation_id, handoff.target_automation_id
        ));
    }
    Ok(())
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
                protected_records::encode(
                    &event.scope.tenant_context,
                    "event",
                    &event.event_id,
                    event,
                )?,
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
    }
    for wait in &rows.waits {
        transaction.execute(
            "INSERT INTO automation_waits
                (wait_id, goal_id, run_id, org_id, workspace_id, deployment_id,
                 status, wait_json, updated_at_ms)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(wait_id, run_id, org_id, workspace_id, deployment_id)
             DO UPDATE SET wait_json = excluded.wait_json, status = excluded.status,
                 updated_at_ms = excluded.updated_at_ms",
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
                protected_records::encode(
                    &TenantContext::local_implicit(),
                    "legacy_handoff",
                    &handoff.handoff_id,
                    handoff,
                )?,
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
            "outbox",
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
            "tool_effect",
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
            "dead_letter",
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
            "compensation",
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
    kind: &str,
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
            protected_records::encode(&scope.tenant_context, kind, id, row)?,
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
    use crate::stateful_runtime::backend::ExecutorRaw as _;
    use crate::stateful_runtime::{
        OrchestrationStorePaths, StatefulReliabilityStoreFile, StatefulRuntimeScope,
        StatefulWorkflowPhase, StatefulWorkflowRunStatus,
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

    fn local_handoff_context() -> LegacyImportContext {
        LegacyImportContext {
            known_automation_ids: ["planner", "executor"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    fn reliability() -> StatefulReliabilityStoreFile {
        let scope = serde_json::to_value(StatefulRuntimeScope::from_tenant_context(
            TenantContext::local_implicit(),
        ))
        .unwrap();
        serde_json::from_value(json!({
            "schema_version": 1,
            "outbox": [{
                "schema_version": 1, "outbox_id": "outbox-1", "scope": scope.clone(),
                "operation": "test", "status": "sent", "created_at_ms": 10, "updated_at_ms": 10
            }],
            "tool_effects": [{
                "schema_version": 1, "effect_id": "effect-1", "scope": scope.clone(),
                "status": "succeeded", "operation": "test", "audit_hash": "sha256:audit",
                "created_at_ms": 10, "updated_at_ms": 10
            }],
            "dead_letters": [{
                "schema_version": 1, "dead_letter_id": "dead-letter-1", "source_type": "tool_effect",
                "source_id": "effect-1", "scope": scope.clone(), "reason": "test", "status": "resolved",
                "created_at_ms": 10, "updated_at_ms": 10
            }],
            "compensations": [{
                "schema_version": 1, "compensation_id": "compensation-1", "scope": scope,
                "status": "completed", "compensation_type": "test", "created_at_ms": 10, "updated_at_ms": 10
            }]
        }))
        .unwrap()
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
        let context = local_handoff_context();
        let first = store
            .import_legacy_runtime_state_with_context(&paths, &context, 100)
            .unwrap();
        assert!(!first.already_complete);
        assert_eq!((first.events, first.snapshots, first.waits), (1, 1, 1));
        assert_eq!((first.handoffs, first.quarantined_handoffs), (1, 1));
        assert!(store.legacy_runtime_migration_complete().unwrap());
        assert!(paths.run_events_path.exists());
        assert!(paths.snapshots_root.exists());
        assert!(paths.waits_path.exists());
        assert!(valid_handoff.exists());
        assert!(corrupt_handoff.exists());

        let second = store
            .import_legacy_runtime_state_with_context(&paths, &context, 200)
            .unwrap();
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
    fn foreign_conflicting_and_unsafe_legacy_handoffs_quarantine_without_importing() {
        let directory = tempfile::tempdir().unwrap();
        let mut paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        let handoff_root = directory.path().join("handoffs");
        let approved = handoff_root.join("approved");
        std::fs::create_dir_all(&approved).unwrap();
        // Valid envelope: source and target are automations this runtime knows.
        std::fs::write(
            approved.join("handoff-1.json"),
            serde_json::to_vec(&handoff()).unwrap(),
        )
        .unwrap();
        // Foreign envelope: well-formed, but names automations from another root.
        let mut foreign = handoff();
        foreign.handoff_id = "handoff-foreign".to_string();
        foreign.source_automation_id = "other-root-producer".to_string();
        foreign.target_automation_id = "other-root-consumer".to_string();
        std::fs::write(
            approved.join("handoff-foreign.json"),
            serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        // Conflicting envelope: reuses handoff-1's identity with different content.
        let mut conflicting = handoff();
        conflicting.artifact_type = "forged".to_string();
        std::fs::write(
            approved.join("zz-conflicting.json"),
            serde_json::to_vec(&conflicting).unwrap(),
        )
        .unwrap();
        // Unsafe envelope: content path escapes the workspace root.
        let mut escaping = handoff();
        escaping.handoff_id = "handoff-unsafe".to_string();
        escaping.content_path = Some("../../etc/passwd".to_string());
        std::fs::write(
            approved.join("handoff-unsafe.json"),
            serde_json::to_vec(&escaping).unwrap(),
        )
        .unwrap();
        paths.handoff_root = Some(handoff_root);

        let store = store(directory.path());
        let context = local_handoff_context();
        let report = store
            .import_legacy_runtime_state_with_context(&paths, &context, 100)
            .unwrap();
        assert_eq!(report.handoffs, 1);
        assert_eq!(report.quarantined_handoffs, 3);
        store
            .with_connection(|connection| {
                let imported: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM legacy_handoffs", [], |row| row.get(0))?;
                assert_eq!(imported, 1);
                let mut statement = connection
                    .prepare("SELECT error FROM legacy_handoff_quarantine ORDER BY source_path")?;
                let errors = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                assert!(errors
                    .iter()
                    .any(|error| error.contains("foreign legacy handoff")));
                assert!(errors
                    .iter()
                    .any(|error| error.contains("escapes the workspace root")));
                assert!(errors
                    .iter()
                    .any(|error| error.contains("conflicting legacy handoff")));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn fresh_runtime_quarantines_foreign_handoff_once() {
        let directory = tempfile::tempdir().unwrap();
        let mut paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        let handoff_root = directory.path().join("handoffs");
        std::fs::create_dir_all(&handoff_root).unwrap();
        std::fs::write(
            handoff_root.join("handoff-1.json"),
            serde_json::to_vec(&handoff()).unwrap(),
        )
        .unwrap();
        paths.handoff_root = Some(handoff_root);

        let store = store(directory.path());
        let first = store.import_legacy_runtime_state(&paths, 100).unwrap();
        assert_eq!((first.handoffs, first.quarantined_handoffs), (0, 1));

        // Completing the migration makes SQLite authoritative. Supplying local
        // ownership later must not turn the quarantined envelope into an import.
        let second = store
            .import_legacy_runtime_state_with_context(&paths, &local_handoff_context(), 200)
            .unwrap();
        assert!(second.already_complete);
        store
            .with_connection(|connection| {
                let imported: u64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM legacy_handoffs", [], |row| row.get(0))?;
                let quarantined: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM legacy_handoff_quarantine",
                    [],
                    |row| row.get(0),
                )?;
                let attempts: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM stateful_migration_attempts",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!((imported, quarantined, attempts), (0, 1, 1));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn interrupted_migration_leaves_durable_attempt_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        std::fs::write(
            &paths.run_events_path,
            format!("{}\n", serde_json::to_string(&event()).unwrap()),
        )
        .unwrap();
        let store = store(directory.path());

        // Force the import transaction to abort after the attempt row commits.
        store
            .with_connection(|connection| {
                connection.execute_batch(
                    "CREATE TRIGGER fail_import AFTER INSERT ON stateful_events
                     BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();
        assert!(store.import_legacy_runtime_state(&paths, 100).is_err());
        assert!(!store.legacy_runtime_migration_complete().unwrap());
        store
            .with_connection(|connection| {
                let aborted: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM stateful_migration_attempts WHERE outcome IS NULL",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(aborted, 1, "aborted attempt must survive the rollback");
                connection.execute_batch("DROP TRIGGER fail_import;")?;
                Ok(())
            })
            .unwrap();

        let report = store.import_legacy_runtime_state(&paths, 200).unwrap();
        assert!(!report.already_complete);
        store
            .with_connection(|connection| {
                let (aborted, completed): (u64, u64) = connection.query_row(
                    "SELECT
                        SUM(CASE WHEN outcome IS NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN outcome = 'complete' THEN 1 ELSE 0 END)
                     FROM stateful_migration_attempts",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!((aborted, completed), (1, 1));
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
        let context = local_handoff_context();

        assert_eq!(
            store
                .import_legacy_handoff_directory_with_context(&handoff_root, &context, 100)
                .unwrap(),
            1
        );
        assert!(corrupt.exists());
        let mut recovered = handoff();
        recovered.handoff_id = "handoff-2".to_string();
        std::fs::write(&corrupt, serde_json::to_vec(&recovered).unwrap()).unwrap();
        assert_eq!(
            store
                .import_legacy_handoff_directory_with_context(&handoff_root, &context, 101)
                .unwrap(),
            2
        );
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
                assert_eq!((handoffs, quarantined, source_count), (2, 0, 2));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn migration_preserves_duplicate_wait_ids_in_distinct_scope_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let paths = LegacyRuntimeMigrationPaths::from_runtime_root(directory.path());
        let first = wait();
        let mut second = wait();
        second.run_id = "run-2".to_string();
        second.scope = StatefulRuntimeScope::from_tenant_context(
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b"),
        );
        std::fs::write(
            &paths.waits_path,
            serde_json::to_vec(&vec![first, second]).unwrap(),
        )
        .unwrap();

        let store = store(directory.path());
        store.import_legacy_runtime_state(&paths, 100).unwrap();
        assert_eq!(store.load_stateful_runtime_waits().unwrap().len(), 2);
    }

    #[test]
    fn migration_write_failures_roll_back_every_imported_record_type() {
        for table in [
            "stateful_migrations",
            "stateful_events",
            "stateful_snapshots",
            "automation_waits",
            "outbox_effects",
            "tool_effects",
            "dead_letters",
            "compensations",
            "legacy_handoffs",
            "legacy_handoff_quarantine",
        ] {
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
            std::fs::write(
                &paths.reliability_path,
                serde_json::to_vec(&reliability()).unwrap(),
            )
            .unwrap();
            let handoff_root = directory.path().join("handoffs");
            let approved = handoff_root.join("approved");
            std::fs::create_dir_all(&approved).unwrap();
            std::fs::write(
                approved.join("handoff-1.json"),
                serde_json::to_vec(&handoff()).unwrap(),
            )
            .unwrap();
            std::fs::write(handoff_root.join("corrupt.json"), "{not-json}").unwrap();
            paths.handoff_root = Some(handoff_root);
            let store = store(directory.path());
            let context = local_handoff_context();
            store
                .with_connection(|connection| {
                    connection.execute_batch(&format!(
                        "CREATE TRIGGER injected_migration_failure AFTER INSERT ON {table}
                         BEGIN SELECT RAISE(ABORT, 'injected migration failure'); END;"
                    ))?;
                    Ok(())
                })
                .unwrap();

            assert!(store
                .import_legacy_runtime_state_with_context(&paths, &context, 100)
                .is_err());
            assert!(!store.legacy_runtime_migration_complete().unwrap());
            store
                .with_connection(|connection| {
                    for table in [
                        "stateful_migrations",
                        "stateful_events",
                        "stateful_snapshots",
                        "automation_waits",
                        "outbox_effects",
                        "tool_effects",
                        "dead_letters",
                        "compensations",
                        "legacy_handoffs",
                        "legacy_handoff_quarantine",
                    ] {
                        let count: u64 = connection.query_row(
                            &format!("SELECT COUNT(*) FROM {table}"),
                            [],
                            |row| row.get(0),
                        )?;
                        assert_eq!(count, 0, "{table} retained a partial migration");
                    }
                    connection.execute_batch("DROP TRIGGER injected_migration_failure")?;
                    Ok(())
                })
                .unwrap();

            let report = store
                .import_legacy_runtime_state_with_context(&paths, &context, 101)
                .unwrap();
            assert_eq!(
                (
                    report.events,
                    report.snapshots,
                    report.waits,
                    report.outbox,
                    report.tool_effects,
                    report.dead_letters,
                    report.compensations,
                    report.handoffs,
                ),
                (1, 1, 1, 1, 1, 1, 1, 1),
                "{table} retry should import every valid record"
            );
            assert_eq!(report.quarantined_handoffs, 1);
            assert!(store.legacy_runtime_migration_complete().unwrap());
            store
                .with_connection(|connection| {
                    for table in [
                        "stateful_events",
                        "stateful_snapshots",
                        "automation_waits",
                        "outbox_effects",
                        "tool_effects",
                        "dead_letters",
                        "compensations",
                        "legacy_handoffs",
                        "legacy_handoff_quarantine",
                    ] {
                        let count: u64 = connection.query_row(
                            &format!("SELECT COUNT(*) FROM {table}"),
                            [],
                            |row| row.get(0),
                        )?;
                        assert_eq!(count, 1, "{table} was not restored by retry");
                    }
                    Ok(())
                })
                .unwrap();
        }
    }

    #[test]
    fn existing_v2_store_upgrades_to_current_schema() {
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
                let deployment_key_columns: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('orchestration_specs')
                     WHERE name = 'deployment_key'",
                    [],
                    |row| row.get(0),
                )?;
                let attempts_table: String = connection.query_row(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'table' AND name = 'stateful_migration_attempts'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(version, 5);
                assert_eq!(table, "legacy_handoff_quarantine");
                assert_eq!(deployment_key_columns, 1);
                assert_eq!(attempts_table, "stateful_migration_attempts");
                Ok(())
            })
            .unwrap();
    }
}
