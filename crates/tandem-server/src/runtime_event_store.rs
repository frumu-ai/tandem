use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, Error as SqliteError, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use tandem_types::TenantContext;

use super::{RuntimeEventLogRow, RuntimeEventLogWindowQuery};

const IMPORT_MIGRATION: &str = "runtime_events_jsonl_import_v1";
const BUSY_RETRY_DELAYS_MS: [u64; 7] = [10, 25, 50, 100, 200, 400, 800];

#[derive(Clone)]
pub(crate) struct RuntimeEventStore {
    database_path: PathBuf,
}

impl RuntimeEventStore {
    pub(crate) fn from_events_path(events_path: &Path) -> Self {
        Self {
            database_path: events_path.with_extension("sqlite3"),
        }
    }

    pub(crate) fn append(&self, legacy_path: &Path, row: &RuntimeEventLogRow) -> Result<()> {
        retry_busy_operation(|| {
            self.with_connection(|connection| {
                self.import_legacy_if_needed(connection, legacy_path)?;
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                insert_row(&transaction, row)?;
                transaction.commit()?;
                Ok(())
            })
        })
    }

    pub(crate) fn query(
        &self,
        legacy_path: &Path,
        tenant: &TenantContext,
        query: RuntimeEventLogWindowQuery<'_>,
    ) -> Result<Vec<RuntimeEventLogRow>> {
        retry_busy_operation(|| {
            self.with_connection(|connection| {
                self.import_legacy_if_needed(connection, legacy_path)?;
                query_rows(connection, tenant, query)
            })
        })
    }

    pub(crate) fn load_all(&self, legacy_path: &Path) -> Result<Vec<RuntimeEventLogRow>> {
        retry_busy_operation(|| {
            self.with_connection(|connection| {
                self.import_legacy_if_needed(connection, legacy_path)?;
                let mut statement = connection.prepare(
                    "SELECT row_json FROM runtime_event_rows ORDER BY seq ASC, event_id ASC",
                )?;
                let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
                rows.map(|row| {
                    let raw = row?;
                    serde_json::from_str(&raw).context("failed to decode stored runtime event")
                })
                .collect()
            })
        })
    }

    pub(crate) fn prune(&self, legacy_path: &Path, cutoff_ms: u64) -> Result<usize> {
        retry_busy_operation(|| {
            self.with_connection(|connection| {
                self.import_legacy_if_needed(connection, legacy_path)?;
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let deleted = transaction.execute(
                    "DELETE FROM runtime_event_rows WHERE occurred_at_ms < ?1",
                    [cutoff_ms as i64],
                )?;
                transaction.commit()?;
                connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
                Ok(deleted)
            })
        })
    }

    #[cfg(test)]
    pub(super) fn append_rows_for_benchmark(
        &self,
        legacy_path: &Path,
        rows: impl IntoIterator<Item = RuntimeEventLogRow>,
    ) -> Result<()> {
        self.with_connection(|connection| {
            self.import_legacy_if_needed(connection, legacy_path)?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for row in rows {
                insert_row(&transaction, &row)?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    fn import_legacy_if_needed(
        &self,
        connection: &mut Connection,
        legacy_path: &Path,
    ) -> Result<()> {
        let imported = connection
            .query_row(
                "SELECT 1 FROM runtime_event_migrations WHERE migration_name = ?1",
                [IMPORT_MIGRATION],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if imported {
            return Ok(());
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let imported = transaction
            .query_row(
                "SELECT 1 FROM runtime_event_migrations WHERE migration_name = ?1",
                [IMPORT_MIGRATION],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if imported {
            transaction.commit()?;
            return Ok(());
        }

        let raw = std::fs::read_to_string(legacy_path).unwrap_or_default();
        let digest = if legacy_path.exists() {
            Some(format!("{:x}", Sha256::digest(raw.as_bytes())))
        } else {
            None
        };
        let mut imported_count = 0usize;
        for (line, source) in raw.lines().enumerate() {
            match serde_json::from_str::<RuntimeEventLogRow>(source) {
                Ok(row) => {
                    insert_row(&transaction, &row)?;
                    imported_count += 1;
                }
                Err(error) => tracing::warn!(
                    line = line + 1,
                    error = %error,
                    "skipping invalid legacy runtime event row during import"
                ),
            }
        }
        transaction.execute(
            "INSERT INTO runtime_event_migrations (migration_name, completed_at_ms) VALUES (?1, ?2)",
            params![IMPORT_MIGRATION, now_ms()],
        )?;
        if legacy_path.exists() {
            transaction.execute(
                "INSERT INTO runtime_event_migration_sources
                 (migration_name, source_path, source_digest, imported_at_ms, record_count)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    IMPORT_MIGRATION,
                    legacy_path.to_string_lossy(),
                    digest,
                    now_ms(),
                    imported_count as i64,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        if let Some(parent) = self.database_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create runtime event store directory {}",
                    parent.display()
                )
            })?;
        }
        let mut connection = Connection::open(&self.database_path).with_context(|| {
            format!(
                "failed to open runtime event store {}",
                self.database_path.display()
            )
        })?;
        // Appends and retention use short immediate transactions. Give a burst
        // of event persisters enough time to serialize rather than dropping a
        // durable observability row under ordinary SQLite write contention.
        connection.busy_timeout(Duration::from_secs(30))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        initialize_schema(&mut connection)?;
        operation(&mut connection)
    }
}

fn retry_busy_operation<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    for delay_ms in BUSY_RETRY_DELAYS_MS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if sqlite_busy_or_locked(&error) => {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }
    operation()
}

fn sqlite_busy_or_locked(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<SqliteError>(),
            Some(SqliteError::SqliteFailure(_, Some(message)))
                if message.contains("database is locked") || message.contains("database is busy")
        )
    })
}

fn initialize_schema(connection: &mut Connection) -> Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_event_migrations (
             migration_name TEXT PRIMARY KEY,
             completed_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS runtime_event_migration_sources (
             migration_name TEXT NOT NULL,
             source_path TEXT NOT NULL,
             source_digest TEXT,
             imported_at_ms INTEGER NOT NULL,
             record_count INTEGER NOT NULL,
             PRIMARY KEY (migration_name, source_path),
             FOREIGN KEY (migration_name) REFERENCES runtime_event_migrations(migration_name)
         );
         CREATE TABLE IF NOT EXISTS runtime_event_rows (
             event_id TEXT PRIMARY KEY,
             run_id TEXT,
             session_id TEXT,
             seq INTEGER NOT NULL,
             occurred_at_ms INTEGER NOT NULL,
             org_id TEXT,
             workspace_id TEXT,
             deployment_id TEXT,
             row_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS runtime_event_rows_run_tenant_seq_idx
             ON runtime_event_rows
                (run_id, org_id, workspace_id, deployment_id, seq, event_id);
         CREATE INDEX IF NOT EXISTS runtime_event_rows_session_seq_idx
             ON runtime_event_rows (session_id, seq, event_id);
         CREATE INDEX IF NOT EXISTS runtime_event_rows_retention_idx
             ON runtime_event_rows (occurred_at_ms);",
    )?;
    Ok(())
}

fn insert_row(connection: &Connection, row: &RuntimeEventLogRow) -> Result<()> {
    let tenant = row.tenant_context();
    connection.execute(
        "INSERT OR IGNORE INTO runtime_event_rows
         (event_id, run_id, session_id, seq, occurred_at_ms, org_id, workspace_id, deployment_id, row_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.event_id(),
            row.run_id(),
            row.session_id(),
            row.seq() as i64,
            row.occurred_at_ms() as i64,
            tenant.map(|value| value.org_id.as_str()),
            tenant.map(|value| value.workspace_id.as_str()),
            tenant.and_then(|value| value.deployment_id.as_deref()),
            serde_json::to_string(row)?,
        ],
    )?;
    Ok(())
}

fn query_rows(
    connection: &Connection,
    tenant: &TenantContext,
    query: RuntimeEventLogWindowQuery<'_>,
) -> Result<Vec<RuntimeEventLogRow>> {
    let mut sql = String::from(
        "SELECT row_json FROM runtime_event_rows WHERE run_id = ?1
         AND (?2 IS NULL OR seq > ?2)
         AND (?3 IS NULL OR seq < ?3)
         AND (?4 = 1 OR (org_id = ?5 AND workspace_id = ?6 AND deployment_id IS ?7))",
    );
    let tail = query.tail.filter(|value| *value > 0);
    sql.push_str(if tail.is_some() {
        " ORDER BY seq DESC, event_id DESC"
    } else {
        " ORDER BY seq ASC, event_id ASC"
    });
    let limit = tail.or_else(|| query.limit.filter(|value| *value > 0));
    if limit.is_some() {
        sql.push_str(" LIMIT ?8");
    }

    let after = query.after_seq.map(|value| value as i64);
    let before = query.before_seq.map(|value| value as i64);
    let mut statement = connection.prepare(&sql)?;
    let local = tenant.is_local_implicit() as i64;
    let org_id = (!tenant.is_local_implicit()).then_some(tenant.org_id.as_str());
    let workspace_id = (!tenant.is_local_implicit()).then_some(tenant.workspace_id.as_str());
    let deployment_id = (!tenant.is_local_implicit())
        .then(|| tenant.deployment_id.as_deref())
        .flatten();
    let mut rows = match limit {
        Some(limit) => statement.query(params![
            query.run_id,
            after,
            before,
            local,
            org_id,
            workspace_id,
            deployment_id,
            limit as i64
        ])?,
        None => statement.query(params![
            query.run_id,
            after,
            before,
            local,
            org_id,
            workspace_id,
            deployment_id
        ])?,
    };
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(
            serde_json::from_str(&row.get::<_, String>(0)?)
                .context("failed to decode stored runtime event")?,
        );
    }
    if tail.is_some() {
        result.reverse();
    }
    Ok(result)
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
