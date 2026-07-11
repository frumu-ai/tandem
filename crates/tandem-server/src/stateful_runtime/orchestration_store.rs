use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tandem_automation::{
    validate_orchestration_spec, AutomationV2RunRecord, GoalRunLink, LongRunningGoal,
    LongRunningGoalStatus, OrchestrationSpec, OrchestrationStatus, WorkflowHandoff,
    WorkflowHandoffStatus,
};

mod goal_control;
mod migration;
mod runtime_records;
mod transition;

pub use goal_control::{GoalCancellationResult, GoalControlOutcome};
pub use migration::{LegacyRuntimeMigrationPaths, LegacyRuntimeMigrationReport};
pub use transition::{
    GovernedTransitionRequest, GovernedTransitionResult, OrchestrationTransitionAuthority,
    WorkflowCompletionResult,
};

const SCHEMA_VERSION: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationStorePaths {
    pub database_path: PathBuf,
    pub engine_lock_path: PathBuf,
}

impl OrchestrationStorePaths {
    pub fn from_runtime_events_path(runtime_events_path: &Path) -> Self {
        let root = canonical_stateful_runtime_root(runtime_events_path);
        Self {
            database_path: root.join("stateful_runtime.sqlite3"),
            engine_lock_path: root.join("stateful_runtime.engine.lock"),
        }
    }

    pub fn from_automation_runs_path(automation_runs_path: &Path) -> Self {
        let root = automation_runs_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        Self {
            database_path: root.join("stateful_runtime.sqlite3"),
            engine_lock_path: root.join("stateful_runtime.engine.lock"),
        }
    }
}

fn canonical_stateful_runtime_root(path: &Path) -> &Path {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if parent.file_name().is_some_and(|name| name == "runtime") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    }
}

/// Process-lifetime guard preventing two local engines from sharing one state root.
#[derive(Debug)]
pub struct StatefulEngineLock {
    file: File,
    path: PathBuf,
}

impl StatefulEngineLock {
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("failed to open engine lock {}", path.display()))?;
        file.try_lock_exclusive().with_context(|| {
            format!(
                "another Tandem engine already owns runtime root lock {}",
                path.display()
            )
        })?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StatefulEngineLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Debug, Clone)]
pub struct OrchestrationStateStore {
    paths: OrchestrationStorePaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicHandoffCommit {
    Committed,
    AlreadyCommitted,
}

impl OrchestrationStateStore {
    pub fn from_runtime_events_path(runtime_events_path: &Path) -> anyhow::Result<Self> {
        Self::open(OrchestrationStorePaths::from_runtime_events_path(
            runtime_events_path,
        ))
    }

    pub fn from_automation_runs_path(automation_runs_path: &Path) -> anyhow::Result<Self> {
        Self::open(OrchestrationStorePaths::from_automation_runs_path(
            automation_runs_path,
        ))
    }

    pub fn open(paths: OrchestrationStorePaths) -> anyhow::Result<Self> {
        if let Some(parent) = paths.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { paths };
        store.with_connection(initialize_schema)?;
        Ok(store)
    }

    pub fn paths(&self) -> &OrchestrationStorePaths {
        &self.paths
    }

    pub fn acquire_engine_lock(&self) -> anyhow::Result<StatefulEngineLock> {
        StatefulEngineLock::acquire(&self.paths.engine_lock_path)
    }

    pub fn put_orchestration(&self, spec: &OrchestrationSpec) -> anyhow::Result<()> {
        let validation = validate_orchestration_spec(spec);
        if !validation.valid {
            bail!(
                "invalid orchestration {} version {}: {}",
                spec.orchestration_id,
                spec.version,
                validation
                    .issues
                    .iter()
                    .map(|issue| issue.code.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let payload = serde_json::to_string(spec)?;
        self.with_connection(|connection| {
            let existing = connection
                .query_row(
                    "SELECT status, definition_json FROM orchestration_specs
                     WHERE orchestration_id = ?1 AND version = ?2",
                    params![spec.orchestration_id, spec.version],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((status, existing_payload)) = existing {
                if status == "published" && existing_payload != payload {
                    bail!(
                        "published orchestration {} version {} is immutable",
                        spec.orchestration_id,
                        spec.version
                    );
                }
                if status == "published" && matches!(spec.status, OrchestrationStatus::Draft) {
                    bail!("published orchestration versions cannot return to draft status");
                }
            }
            connection.execute(
                "INSERT INTO orchestration_specs (
                    orchestration_id, version, org_id, workspace_id, deployment_id,
                    status, definition_json, created_at_ms, updated_at_ms, published_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(orchestration_id, version) DO UPDATE SET
                    status = excluded.status,
                    definition_json = excluded.definition_json,
                    updated_at_ms = excluded.updated_at_ms,
                    published_at_ms = excluded.published_at_ms",
                params![
                    spec.orchestration_id,
                    spec.version,
                    spec.tenant_context.org_id,
                    spec.tenant_context.workspace_id,
                    spec.tenant_context.deployment_id,
                    serde_json::to_value(&spec.status)?
                        .as_str()
                        .unwrap_or("draft"),
                    payload,
                    spec.created_at_ms,
                    spec.updated_at_ms,
                    spec.published_at_ms,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_orchestration(
        &self,
        orchestration_id: &str,
        version: u64,
    ) -> anyhow::Result<Option<OrchestrationSpec>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT definition_json FROM orchestration_specs
                     WHERE orchestration_id = ?1 AND version = ?2",
                    params![orchestration_id, version],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    pub fn upsert_automation_runs<'a>(
        &self,
        runs: impl IntoIterator<Item = &'a AutomationV2RunRecord>,
    ) -> anyhow::Result<usize> {
        let runs = runs.into_iter().collect::<Vec<_>>();
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for run in &runs {
                upsert_automation_run(&transaction, run)?;
            }
            transaction.commit()?;
            Ok(runs.len())
        })
    }

    /// Replace the hot Automation V2 run index while retaining full historical
    /// rows for archive and audit reads.
    pub fn sync_hot_automation_runs<'a>(
        &self,
        runs: impl IntoIterator<Item = &'a AutomationV2RunRecord>,
    ) -> anyhow::Result<usize> {
        let runs = runs.into_iter().collect::<Vec<_>>();
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "UPDATE automation_runs SET is_hot = 0 WHERE is_hot != 0",
                [],
            )?;
            for run in &runs {
                upsert_automation_run(&transaction, run)?;
                transaction.execute(
                    "UPDATE automation_runs SET is_hot = 1 WHERE run_id = ?1",
                    [&run.run_id],
                )?;
            }
            transaction.commit()?;
            Ok(runs.len())
        })
    }

    pub fn load_automation_runs(&self) -> anyhow::Result<Vec<AutomationV2RunRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT run_json FROM automation_runs
                 WHERE is_hot = 1 ORDER BY created_at_ms, run_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut runs = Vec::new();
            for row in rows {
                runs.push(serde_json::from_str(&row?)?);
            }
            Ok(runs)
        })
    }

    pub fn get_automation_run(
        &self,
        run_id: &str,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT run_json FROM automation_runs WHERE run_id = ?1",
                    [run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    pub fn import_legacy_runs(
        &self,
        source_path: &Path,
        runs: &[AutomationV2RunRecord],
        imported_at_ms: u64,
    ) -> anyhow::Result<usize> {
        let source = source_path.to_string_lossy().to_string();
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for run in runs {
                upsert_automation_run(&transaction, run)?;
            }
            transaction.execute(
                "INSERT INTO migration_sources
                    (source_kind, source_path, imported_at_ms, record_count)
                 VALUES ('automation_runs', ?1, ?2, ?3)
                 ON CONFLICT(source_kind, source_path) DO UPDATE SET
                    imported_at_ms = excluded.imported_at_ms,
                    record_count = excluded.record_count",
                params![source, imported_at_ms, runs.len() as u64],
            )?;
            transaction.commit()?;
            Ok(runs.len())
        })
    }

    /// Idempotently indexes legacy inbox/approved/archived JSON envelopes. The
    /// source files remain untouched as migration backups and compatibility data.
    pub fn import_legacy_handoff_directory(
        &self,
        handoff_root: &Path,
        imported_at_ms: u64,
    ) -> anyhow::Result<usize> {
        let handoffs = migration::load_legacy_handoffs(Some(handoff_root))?;
        let source = handoff_root.to_string_lossy().to_string();
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut imported = 0usize;
            for (path, handoff, status) in &handoffs.imported {
                imported += transaction.execute(
                    "INSERT INTO legacy_handoffs
                        (handoff_id, source_path, status, consumed_by_run_id, handoff_json,
                         created_at_ms, imported_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(handoff_id) DO UPDATE SET
                        source_path = excluded.source_path,
                        status = excluded.status,
                        consumed_by_run_id = excluded.consumed_by_run_id,
                        handoff_json = excluded.handoff_json,
                        imported_at_ms = excluded.imported_at_ms",
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
                transaction.execute(
                    "DELETE FROM legacy_handoff_quarantine WHERE source_path = ?1",
                    [path.to_string_lossy().as_ref()],
                )?;
            }
            for quarantine in &handoffs.quarantined {
                transaction.execute(
                    "INSERT INTO legacy_handoff_quarantine
                        (source_path, source_digest, error, quarantined_at_ms)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(source_path) DO UPDATE SET
                        source_digest = excluded.source_digest, error = excluded.error,
                        quarantined_at_ms = excluded.quarantined_at_ms",
                    params![
                        quarantine.source_path.to_string_lossy(),
                        quarantine.source_digest,
                        quarantine.error,
                        imported_at_ms,
                    ],
                )?;
            }
            transaction.execute(
                "INSERT INTO migration_sources
                    (source_kind, source_path, imported_at_ms, record_count)
                 VALUES ('legacy_handoffs', ?1, ?2, ?3)
                 ON CONFLICT(source_kind, source_path) DO UPDATE SET
                    imported_at_ms = excluded.imported_at_ms,
                    record_count = excluded.record_count",
                params![
                    source,
                    imported_at_ms,
                    (handoffs.imported.len() + handoffs.quarantined.len()) as u64,
                ],
            )?;
            transaction.commit()?;
            Ok(imported)
        })
    }

    pub fn put_goal(&self, goal: &LongRunningGoal) -> anyhow::Result<()> {
        self.with_connection(|connection| upsert_goal(connection, goal))
    }

    pub fn get_goal(&self, goal_id: &str) -> anyhow::Result<Option<LongRunningGoal>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
                    [goal_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    /// Atomically records a governed handoff, its one downstream run, lineage,
    /// and the goal's new active position. Replaying the same idempotency key is
    /// a no-op only when it points to the same handoff and downstream run.
    pub fn commit_handoff_transition(
        &self,
        handoff: &WorkflowHandoff,
        downstream_run: &AutomationV2RunRecord,
        link: &GoalRunLink,
        updated_goal: &LongRunningGoal,
    ) -> anyhow::Result<AtomicHandoffCommit> {
        self.commit_handoff_transition_with_event(handoff, downstream_run, link, updated_goal, None)
    }

    fn commit_handoff_transition_with_event(
        &self,
        handoff: &WorkflowHandoff,
        downstream_run: &AutomationV2RunRecord,
        link: &GoalRunLink,
        updated_goal: &LongRunningGoal,
        transition_event: Option<&crate::stateful_runtime::StatefulRunEventRecord>,
    ) -> anyhow::Result<AtomicHandoffCommit> {
        validate_atomic_transition(handoff, downstream_run, link, updated_goal)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    "SELECT handoff_id, status, consumed_by_run_id FROM workflow_handoffs
                     WHERE goal_id = ?1 AND idempotency_key = ?2",
                    params![handoff.goal_id, handoff.idempotency_key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            let mut update_existing = false;
            if let Some((handoff_id, status, consumed_by_run_id)) = existing {
                if handoff_id == handoff.handoff_id
                    && consumed_by_run_id.as_deref() == Some(downstream_run.run_id.as_str())
                {
                    return Ok(AtomicHandoffCommit::AlreadyCommitted);
                }
                if handoff_id == handoff.handoff_id
                    && consumed_by_run_id.is_none()
                    && matches!(status.as_str(), "pending_approval" | "approved")
                {
                    update_existing = true;
                } else {
                    bail!(
                        "idempotency key {} is already bound to handoff {} and run {:?}",
                        handoff.idempotency_key,
                        handoff_id,
                        consumed_by_run_id
                    );
                }
            }
            ensure_current_goal_allows_handoff(&transaction, handoff)?;

            let mut consumed = handoff.clone();
            consumed.status = WorkflowHandoffStatus::Consumed;
            consumed.consumed_by_run_id = Some(downstream_run.run_id.clone());
            consumed.updated_at_ms = consumed.updated_at_ms.max(downstream_run.created_at_ms);
            if update_existing {
                transaction.execute(
                    "UPDATE workflow_handoffs SET status = 'consumed', consumed_by_run_id = ?2,
                        handoff_json = ?3, updated_at_ms = ?4 WHERE handoff_id = ?1",
                    params![
                        consumed.handoff_id,
                        consumed.consumed_by_run_id,
                        serde_json::to_string(&consumed)?,
                        consumed.updated_at_ms,
                    ],
                )?;
            } else {
                transaction.execute(
                    "INSERT INTO workflow_handoffs
                    (handoff_id, goal_id, idempotency_key, org_id, workspace_id, deployment_id,
                     source_run_id, target_automation_id, status, consumed_by_run_id,
                     handoff_json, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'consumed', ?9, ?10, ?11, ?12)",
                    params![
                        consumed.handoff_id,
                        consumed.goal_id,
                        consumed.idempotency_key,
                        consumed.tenant_context.org_id,
                        consumed.tenant_context.workspace_id,
                        consumed.tenant_context.deployment_id,
                        consumed.source_run_id,
                        consumed.target_automation_id,
                        consumed.consumed_by_run_id,
                        serde_json::to_string(&consumed)?,
                        consumed.created_at_ms,
                        consumed.updated_at_ms,
                    ],
                )?;
            }
            upsert_automation_run(&transaction, downstream_run)?;
            transaction.execute(
                "INSERT INTO goal_run_links
                    (goal_id, run_id, orchestration_node_id, orchestration_version, hop_index,
                     parent_run_id, triggering_handoff_id, link_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    link.goal_id,
                    link.run_id,
                    link.orchestration_node_id,
                    link.orchestration_version,
                    link.hop_index,
                    link.parent_run_id,
                    link.triggering_handoff_id,
                    serde_json::to_string(link)?,
                    link.created_at_ms,
                ],
            )?;
            upsert_goal(&transaction, updated_goal)?;
            if let Some(event) = transition_event {
                let mut event = event.clone();
                event.seq = next_event_seq(&transaction, &event.run_id)?;
                transaction.execute(
                    "INSERT INTO stateful_events
                        (event_id, goal_id, run_id, seq, event_json, created_at_ms,
                         org_id, workspace_id, deployment_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        event.event_id,
                        handoff.goal_id,
                        event.run_id,
                        event.seq,
                        serde_json::to_string(&event)?,
                        event.occurred_at_ms,
                        event.scope.tenant_context.org_id,
                        event.scope.tenant_context.workspace_id,
                        event.scope.tenant_context.deployment_id,
                    ],
                )?;
            }
            transaction.commit()?;
            Ok(AtomicHandoffCommit::Committed)
        })
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut connection = Connection::open(&self.paths.database_path).with_context(|| {
            format!(
                "failed to open orchestration store {}",
                self.paths.database_path.display()
            )
        })?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        operation(&mut connection)
    }
}

fn initialize_schema(connection: &mut Connection) -> anyhow::Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS schema_metadata (
            schema_version INTEGER NOT NULL
         );
         INSERT INTO schema_metadata (schema_version)
            SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_metadata);

         CREATE TABLE IF NOT EXISTS orchestration_specs (
            orchestration_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            deployment_id TEXT,
            status TEXT NOT NULL,
            definition_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            published_at_ms INTEGER,
            PRIMARY KEY (orchestration_id, version)
         );
         CREATE INDEX IF NOT EXISTS idx_orchestration_scope_status
            ON orchestration_specs (org_id, workspace_id, status);

         CREATE TABLE IF NOT EXISTS automation_runs (
            run_id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            deployment_id TEXT,
            status TEXT NOT NULL,
            is_hot INTEGER NOT NULL DEFAULT 1,
            run_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_automation_runs_scope_status
            ON automation_runs (org_id, workspace_id, status);

         CREATE TABLE IF NOT EXISTS long_running_goals (
            goal_id TEXT PRIMARY KEY,
            orchestration_id TEXT NOT NULL,
            orchestration_version INTEGER NOT NULL,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            deployment_id TEXT,
            status TEXT NOT NULL,
            active_run_id TEXT,
            goal_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_goals_scope_status
            ON long_running_goals (org_id, workspace_id, status);

         CREATE TABLE IF NOT EXISTS workflow_handoffs (
            handoff_id TEXT PRIMARY KEY,
            goal_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            deployment_id TEXT,
            source_run_id TEXT NOT NULL,
            target_automation_id TEXT NOT NULL,
            status TEXT NOT NULL,
            consumed_by_run_id TEXT UNIQUE,
            handoff_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE (goal_id, idempotency_key)
         );
         CREATE INDEX IF NOT EXISTS idx_handoffs_scope_status
            ON workflow_handoffs (org_id, workspace_id, status);

         CREATE TABLE IF NOT EXISTS legacy_handoffs (
            handoff_id TEXT PRIMARY KEY,
            source_path TEXT NOT NULL,
            status TEXT NOT NULL,
            consumed_by_run_id TEXT,
            handoff_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            imported_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS legacy_handoff_quarantine (
            source_path TEXT PRIMARY KEY,
            source_digest TEXT,
            error TEXT NOT NULL,
            quarantined_at_ms INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS goal_run_links (
            goal_id TEXT NOT NULL,
            run_id TEXT NOT NULL UNIQUE,
            orchestration_node_id TEXT NOT NULL,
            orchestration_version INTEGER NOT NULL,
            hop_index INTEGER NOT NULL,
            parent_run_id TEXT,
            triggering_handoff_id TEXT UNIQUE,
            link_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            PRIMARY KEY (goal_id, hop_index)
         );

         CREATE TABLE IF NOT EXISTS automation_waits (
            wait_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, status TEXT NOT NULL,
            wait_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS wait_resolutions (
            wait_id TEXT NOT NULL, idempotency_key TEXT NOT NULL, resolution_json TEXT NOT NULL,
            resolved_at_ms INTEGER NOT NULL, PRIMARY KEY (wait_id, idempotency_key)
         );
         CREATE TABLE IF NOT EXISTS stateful_events (
            event_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, seq INTEGER NOT NULL,
            event_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS stateful_snapshots (
            snapshot_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, seq INTEGER NOT NULL,
            snapshot_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS outbox_effects (
            effect_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, status TEXT NOT NULL,
            effect_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS dead_letters (
            dead_letter_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, status TEXT NOT NULL,
            record_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS compensations (
            compensation_id TEXT PRIMARY KEY, goal_id TEXT, run_id TEXT, status TEXT NOT NULL,
            record_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS migration_sources (
            source_kind TEXT NOT NULL, source_path TEXT NOT NULL, imported_at_ms INTEGER NOT NULL,
            record_count INTEGER NOT NULL, PRIMARY KEY (source_kind, source_path)
         );
         COMMIT;",
    )?;
    if !table_has_column(connection, "automation_runs", "is_hot")? {
        connection.execute(
            "ALTER TABLE automation_runs ADD COLUMN is_hot INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    let mut version: i64 = connection.query_row(
        "SELECT schema_version FROM schema_metadata LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    if version == 1 {
        migrate_schema_v1_to_v2(connection)?;
        version = 2;
    }
    if version == 2 {
        migrate_schema_v2_to_v3(connection)?;
        version = 3;
    }
    if version != SCHEMA_VERSION {
        bail!(
            "unsupported orchestration store schema version {version}; expected {SCHEMA_VERSION}"
        );
    }
    Ok(())
}

fn migrate_schema_v1_to_v2(connection: &mut Connection) -> anyhow::Result<()> {
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS stateful_migrations (
            migration_id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            source_fingerprint TEXT NOT NULL,
            record_count INTEGER NOT NULL,
            started_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER
         );
         CREATE TABLE IF NOT EXISTS tool_effects (
            effect_id TEXT PRIMARY KEY,
            goal_id TEXT,
            run_id TEXT,
            org_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            deployment_id TEXT,
            status TEXT NOT NULL,
            effect_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_tool_effects_scope_status
            ON tool_effects (org_id, workspace_id, status);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_stateful_events_run_seq
            ON stateful_events (run_id, seq);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_stateful_snapshots_run_seq
            ON stateful_snapshots (run_id, seq);
         COMMIT;",
    )?;
    add_scope_columns(connection, "automation_waits")?;
    add_scope_columns(connection, "stateful_events")?;
    add_scope_columns(connection, "stateful_snapshots")?;
    add_scope_columns(connection, "outbox_effects")?;
    add_scope_columns(connection, "dead_letters")?;
    add_scope_columns(connection, "compensations")?;
    connection.execute("UPDATE schema_metadata SET schema_version = 2", [])?;
    Ok(())
}

fn migrate_schema_v2_to_v3(connection: &mut Connection) -> anyhow::Result<()> {
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS legacy_handoff_quarantine (
            source_path TEXT PRIMARY KEY,
            source_digest TEXT,
            error TEXT NOT NULL,
            quarantined_at_ms INTEGER NOT NULL
         );
         UPDATE schema_metadata SET schema_version = 3;
         COMMIT;",
    )?;
    Ok(())
}

fn add_scope_columns(connection: &Connection, table: &str) -> anyhow::Result<()> {
    for (column, definition) in [
        ("org_id", "TEXT NOT NULL DEFAULT ''"),
        ("workspace_id", "TEXT NOT NULL DEFAULT ''"),
        ("deployment_id", "TEXT"),
    ] {
        if !table_has_column(connection, table, column)? {
            connection.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
    }
    connection.execute(
        &format!("CREATE INDEX IF NOT EXISTS idx_{table}_scope ON {table} (org_id, workspace_id)"),
        [],
    )?;
    Ok(())
}

fn upsert_automation_run(
    connection: &Connection,
    run: &AutomationV2RunRecord,
) -> anyhow::Result<()> {
    let status = serde_json::to_value(&run.status)?;
    connection.execute(
        "INSERT INTO automation_runs
            (run_id, automation_id, org_id, workspace_id, deployment_id, status,
             is_hot, run_json, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9)
         ON CONFLICT(run_id) DO UPDATE SET
            status = excluded.status,
            is_hot = 1,
            run_json = excluded.run_json,
            updated_at_ms = excluded.updated_at_ms
         WHERE excluded.updated_at_ms >= automation_runs.updated_at_ms",
        params![
            run.run_id,
            run.automation_id,
            run.tenant_context.org_id,
            run.tenant_context.workspace_id,
            run.tenant_context.deployment_id,
            status.as_str().unwrap_or("unknown"),
            serde_json::to_string(run)?,
            run.created_at_ms,
            run.updated_at_ms,
        ],
    )?;
    Ok(())
}

fn table_has_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> anyhow::Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn upsert_goal(connection: &Connection, goal: &LongRunningGoal) -> anyhow::Result<()> {
    let status = serde_json::to_value(&goal.status)?;
    connection.execute(
        "INSERT INTO long_running_goals
            (goal_id, orchestration_id, orchestration_version, org_id, workspace_id,
             deployment_id, status, active_run_id, goal_json, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(goal_id) DO UPDATE SET
            status = excluded.status,
            active_run_id = excluded.active_run_id,
            goal_json = excluded.goal_json,
            updated_at_ms = excluded.updated_at_ms
         WHERE excluded.updated_at_ms >= long_running_goals.updated_at_ms",
        params![
            goal.goal_id,
            goal.orchestration_id,
            goal.orchestration_version,
            goal.tenant_context.org_id,
            goal.tenant_context.workspace_id,
            goal.tenant_context.deployment_id,
            status.as_str().unwrap_or("unknown"),
            goal.active_run_id,
            serde_json::to_string(goal)?,
            goal.created_at_ms,
            goal.updated_at_ms,
        ],
    )?;
    Ok(())
}

fn next_event_seq(connection: &Connection, run_id: &str) -> anyhow::Result<u64> {
    let last: Option<u64> = connection.query_row(
        "SELECT MAX(seq) FROM stateful_events WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    Ok(last.unwrap_or(0).saturating_add(1))
}

fn validate_atomic_transition(
    handoff: &WorkflowHandoff,
    downstream_run: &AutomationV2RunRecord,
    link: &GoalRunLink,
    goal: &LongRunningGoal,
) -> anyhow::Result<()> {
    if handoff.goal_id != goal.goal_id || link.goal_id != goal.goal_id {
        bail!("handoff, lineage, and goal must use the same goal_id");
    }
    if handoff.tenant_context != goal.tenant_context
        || downstream_run.tenant_context != goal.tenant_context
    {
        bail!("handoff and downstream run must remain in the goal tenant scope");
    }
    if downstream_run.run_id != link.run_id
        || link.triggering_handoff_id.as_deref() != Some(handoff.handoff_id.as_str())
    {
        bail!("lineage must bind the downstream run to the triggering handoff");
    }
    if downstream_run.automation_id != handoff.target_automation_id {
        bail!("downstream run automation does not match the handoff target");
    }
    if goal.active_run_id.as_deref() != Some(downstream_run.run_id.as_str())
        || goal.hop_count != link.hop_index
    {
        bail!("updated goal must point at the linked downstream run and hop");
    }
    if handoff.orchestration_version != link.orchestration_version
        || goal.orchestration_version != link.orchestration_version
    {
        bail!("handoff, lineage, and goal must use the same orchestration version");
    }
    Ok(())
}

fn ensure_current_goal_allows_handoff(
    transaction: &rusqlite::Transaction<'_>,
    handoff: &WorkflowHandoff,
) -> anyhow::Result<()> {
    let payload = transaction
        .query_row(
            "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
            [&handoff.goal_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(());
    };
    let goal: LongRunningGoal = serde_json::from_str(&payload)?;
    if goal.tenant_context != handoff.tenant_context
        || goal.orchestration_id != handoff.orchestration_id
        || goal.orchestration_version != handoff.orchestration_version
    {
        bail!("handoff no longer matches the persisted goal")
    }
    if goal.status.is_terminal() || goal.status == LongRunningGoalStatus::Paused {
        bail!("terminal or paused goals reject handoff consumption")
    }
    if goal.active_run_id.as_deref() != Some(handoff.source_run_id.as_str())
        || goal.current_node_id.as_deref() != Some(handoff.source_node_id.as_str())
    {
        bail!("handoff source is no longer active for the persisted goal")
    }
    Ok(())
}

fn collect_json_files(root: &Path, output: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)
        .with_context(|| format!("failed to read handoff directory {}", root.display()))?
    {
        let path = entry?.path();
        if path.is_dir() {
            collect_json_files(&path, output)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_automation::{
        GoalLimitAction, GoalPolicy, LongRunningGoalStatus, OrchestrationArtifactRef,
        WorkflowHandoffStatus,
    };
    use tandem_types::TenantContext;

    fn run(run_id: &str) -> AutomationV2RunRecord {
        serde_json::from_value(serde_json::json!({
            "run_id": run_id,
            "automation_id": "executor",
            "trigger_type": "orchestration_handoff",
            "status": "queued",
            "created_at_ms": 20,
            "updated_at_ms": 20,
            "checkpoint": {}
        }))
        .expect("minimal run fixture")
    }

    fn goal(run_id: &str) -> LongRunningGoal {
        LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 3,
            objective: "Plan, execute, and verify".to_string(),
            status: LongRunningGoalStatus::Active,
            tenant_context: TenantContext::local_implicit(),
            policy: GoalPolicy {
                max_hops: 10,
                deadline_at_ms: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
                on_limit: GoalLimitAction::PauseForReview,
            },
            active_run_id: Some(run_id.to_string()),
            current_node_id: Some("execute".to_string()),
            hop_count: 1,
            total_tokens: 0,
            total_cost_usd: 0.0,
            created_at_ms: 1,
            updated_at_ms: 20,
            finished_at_ms: None,
            final_artifact: None,
            metadata: None,
        }
    }

    fn handoff() -> WorkflowHandoff {
        WorkflowHandoff {
            schema_version: 1,
            handoff_id: "handoff-1".to_string(),
            idempotency_key: "goal-1:plan:continue:1".to_string(),
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 3,
            tenant_context: TenantContext::local_implicit(),
            edge_id: "plan-to-execute".to_string(),
            transition_key: "continue".to_string(),
            source_automation_id: "planner".to_string(),
            source_run_id: "run-1".to_string(),
            source_node_id: "plan".to_string(),
            target_automation_id: "executor".to_string(),
            target_node_id: "execute".to_string(),
            artifact: OrchestrationArtifactRef {
                artifact_type: "plan".to_string(),
                content_path: Some("artifacts/plan.json".to_string()),
                content_digest: Some("sha256:abc".to_string()),
                value: None,
            },
            status: WorkflowHandoffStatus::Approved,
            created_at_ms: 10,
            updated_at_ms: 10,
            consumed_by_run_id: None,
            metadata: None,
        }
    }

    fn published_spec() -> OrchestrationSpec {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "orchestration_id": "orch-1",
            "name": "Plan and finish",
            "status": "published",
            "version": 3,
            "root_node_id": "plan",
            "nodes": [
                {
                    "node_id": "plan",
                    "name": "Plan",
                    "x": 0.0,
                    "y": 0.0,
                    "kind": "workflow",
                    "automation_id": "planner",
                    "pinned_definition_hash": "sha256:planner-v3",
                    "allowed_transition_keys": ["complete"],
                    "emits_artifact_types": ["plan"]
                },
                {
                    "node_id": "complete",
                    "name": "Complete",
                    "x": 200.0,
                    "y": 0.0,
                    "kind": "terminal",
                    "outcome": "complete",
                    "final_artifact_type": "plan"
                }
            ],
            "edges": [{
                "edge_id": "plan-complete",
                "from_node_id": "plan",
                "to_node_id": "complete",
                "transition_key": "complete",
                "artifact_contract": {"artifact_type": "plan", "required": true}
            }],
            "goal_policy": {"max_hops": 3},
            "tenant_context": {
                "org_id": "local",
                "workspace_id": "local",
                "source": "local_implicit"
            },
            "created_at_ms": 1,
            "updated_at_ms": 2,
            "published_at_ms": 2
        }))
        .expect("published orchestration fixture")
    }

    #[test]
    fn atomic_handoff_commit_is_exactly_once() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        let downstream_run = run("run-2");
        let link = GoalRunLink {
            goal_id: "goal-1".to_string(),
            run_id: downstream_run.run_id.clone(),
            orchestration_node_id: "execute".to_string(),
            orchestration_version: 3,
            hop_index: 1,
            parent_run_id: Some("run-1".to_string()),
            triggering_handoff_id: Some("handoff-1".to_string()),
            created_at_ms: 20,
        };

        assert_eq!(
            store
                .commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
                .unwrap(),
            AtomicHandoffCommit::Committed
        );
        assert_eq!(
            store
                .commit_handoff_transition(&handoff(), &downstream_run, &link, &goal("run-2"))
                .unwrap(),
            AtomicHandoffCommit::AlreadyCommitted
        );
        assert_eq!(store.load_automation_runs().unwrap().len(), 1);
        assert_eq!(
            store.get_goal("goal-1").unwrap().unwrap().active_run_id,
            Some("run-2".to_string())
        );
        let mut cross_tenant_run = downstream_run;
        cross_tenant_run.tenant_context = TenantContext::explicit("other", "other", None);
        assert!(store
            .commit_handoff_transition(&handoff(), &cross_tenant_run, &link, &goal("run-2"))
            .is_err());
    }

    #[test]
    fn hot_sync_retains_archived_rows_without_reloading_them() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        let archived = run("run-archived");
        let hot = run("run-hot");

        store.sync_hot_automation_runs([&archived, &hot]).unwrap();
        store.sync_hot_automation_runs([&hot]).unwrap();

        let loaded = store.load_automation_runs().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].run_id, "run-hot");
        assert_eq!(
            store
                .get_automation_run("run-archived")
                .unwrap()
                .unwrap()
                .run_id,
            "run-archived"
        );
    }

    #[test]
    fn engine_lock_rejects_a_second_owner() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        let first = store.acquire_engine_lock().unwrap();
        assert_eq!(first.path(), directory.path().join("engine.lock"));
        assert!(store.acquire_engine_lock().is_err());
    }

    #[test]
    fn published_versions_are_immutable() {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        let spec = published_spec();
        store.put_orchestration(&spec).unwrap();
        assert_eq!(
            store.get_orchestration("orch-1", 3).unwrap(),
            Some(spec.clone())
        );

        let mut changed = spec;
        changed.name = "Changed after publish".to_string();
        changed.updated_at_ms += 1;
        assert!(store.put_orchestration(&changed).is_err());
    }
}
