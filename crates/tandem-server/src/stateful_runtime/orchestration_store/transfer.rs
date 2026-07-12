//! Offline, verified transfers between the SQLite and PostgreSQL stateful
//! orchestration backends.
//!
//! A transfer never mutates the source. It acquires the source engine lock,
//! records a durable `in_progress` journal entry on the target, copies the
//! logical state in bounded batches, and makes the target authoritative only
//! after a deterministic fingerprint matches. A crashed import therefore
//! leaves the source usable and makes the target fail closed until the same
//! command resumes or completes the transfer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use rusqlite::{Connection as SqliteConnection, OpenFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::stateful_runtime::backend::{
    self, Executor, ExecutorRaw as _, OptionalExtension, Value,
};
use crate::util::time::now_ms;

use super::{OrchestrationStateStore, OrchestrationStorePaths, StoreBackendSelection};

const TRANSFER_ID: &str = "stateful_backend_transfer_v1";
const TRANSFER_TABLE: &str = "stateful_backend_transfers";
const BATCH_ROWS: i64 = 500;
const GENERATED_ROWID_TABLES: &[&str] = &[
    "stateful_events",
    "outbox_effects",
    "dead_letters",
    "compensations",
    "tool_effects",
];
const EXCLUDED_TABLES: &[&str] = &["schema_metadata", TRANSFER_TABLE];
const AUXILIARY_DATABASES: &[(&str, &str)] = &[
    ("sessions", "storage/sessions.sqlite3"),
    ("runtime_events", "runtime/events.sqlite3"),
];

/// A compiled stateful storage backend used as a migration endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulBackendKind {
    Sqlite,
    Postgres,
}

impl StatefulBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }

    fn config(self, postgres_url: Option<&str>) -> anyhow::Result<backend::StorageBackendConfig> {
        match self {
            Self::Sqlite => {
                if postgres_url.is_some_and(|value| !value.trim().is_empty()) {
                    bail!("a PostgreSQL URL was supplied for a SQLite stateful storage endpoint");
                }
                Ok(backend::StorageBackendConfig::Sqlite)
            }
            Self::Postgres => {
                let url = postgres_url
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "a PostgreSQL stateful storage endpoint requires a connection URL"
                        )
                    })?;
                Ok(backend::StorageBackendConfig::Postgres {
                    url: url.to_string(),
                })
            }
        }
    }
}

/// Explicit source and target locations for an offline backend transfer.
///
/// The source and target paths may be the same for SQLite-to-PostgreSQL: the
/// SQLite file remains as a read-only rollback source while PostgreSQL uses the
/// runtime-root schema marker. PostgreSQL-to-SQLite generally uses a distinct
/// target root so an existing source SQLite file is never overwritten.
#[derive(Debug, Clone)]
pub struct StatefulBackendMigrationRequest {
    pub source_paths: OrchestrationStorePaths,
    pub target_paths: OrchestrationStorePaths,
    pub source_backend: StatefulBackendKind,
    pub target_backend: StatefulBackendKind,
    pub source_postgres_url: Option<String>,
    pub target_postgres_url: Option<String>,
}

/// Evidence emitted after a verified backend transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatefulBackendMigrationReport {
    pub source_backend: StatefulBackendKind,
    pub target_backend: StatefulBackendKind,
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub source_fingerprint: String,
    pub target_fingerprint: String,
    pub record_count: u64,
    pub already_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransferTable {
    name: String,
    columns: Vec<String>,
}

#[derive(Debug, Clone)]
struct TransferJournal {
    source_fingerprint: String,
    target_fingerprint: Option<String>,
    record_count: u64,
    status: String,
}

/// Transfers the complete stateful orchestration store from SQLite to
/// PostgreSQL or from PostgreSQL to SQLite. The engine must be stopped; the
/// source lock is taken before its first read and held through verification.
pub fn migrate_stateful_storage_backend(
    request: &StatefulBackendMigrationRequest,
) -> anyhow::Result<StatefulBackendMigrationReport> {
    if request.source_backend == request.target_backend {
        bail!(
            "stateful storage migration requires different source and target backends; received {} -> {}",
            request.source_backend.as_str(),
            request.target_backend.as_str()
        );
    }

    let source_config = request
        .source_backend
        .config(request.source_postgres_url.as_deref())?;
    let target_config = request
        .target_backend
        .config(request.target_postgres_url.as_deref())?;
    let source = OrchestrationStateStore::open_for_backend_transfer_source(
        request.source_paths.clone(),
        source_config,
    )
    .context("open source stateful storage backend")?;
    let target = OrchestrationStateStore::open_for_backend_transfer_target(
        request.target_paths.clone(),
        target_config,
    )
    .context("open target stateful storage backend")?;

    let _source_lock = source
        .acquire_engine_lock()
        .context("acquire source stateful storage engine lock; stop the engine before migrating")?;
    let _target_guard = target
        .acquire_backend_transfer_target_guard()
        .context("acquire target PostgreSQL stateful storage advisory lock")?;

    let transfer_tables = transfer_tables(&source, &target)
        .context("inspect source and target stateful store schemas")?;
    let (source_primary_fingerprint, source_primary_records) =
        fingerprint_store(&source, &transfer_tables)
            .context("fingerprint locked source stateful storage")?;
    let (source_fingerprint, record_count) =
        fingerprint_transfer_state(&source, &transfer_tables, &runtime_root(source.paths()))
            .context("fingerprint locked source storage state")?;

    if let Some(journal) = read_journal(&target)? {
        if journal.status == "complete" {
            if journal.source_fingerprint != source_fingerprint {
                bail!(
                    "target stateful storage already contains a completed transfer from a different source fingerprint; choose an empty target root"
                );
            }
            let (target_fingerprint, target_records) = fingerprint_transfer_state(
                &target,
                &transfer_tables,
                &runtime_root(target.paths()),
            )
            .context("verify completed target stateful storage")?;
            if journal.target_fingerprint.as_deref() != Some(target_fingerprint.as_str())
                || target_fingerprint != source_fingerprint
                || target_records != record_count
            {
                bail!(
                    "completed stateful storage transfer fingerprint does not match its locked source; target remains unusable"
                );
            }
            return Ok(report(
                request,
                source_fingerprint,
                target_fingerprint,
                record_count,
                true,
            ));
        }
        if journal.source_fingerprint != source_fingerprint {
            bail!(
                "target stateful storage has an interrupted transfer from a different source fingerprint; repair that target before reusing it"
            );
        }
        if journal.status != "in_progress" {
            bail!(
                "target stateful storage has an unknown backend-transfer status `{}`; refusing to continue",
                journal.status
            );
        }
    } else {
        ensure_target_empty(&target, &transfer_tables)?;
        ensure_auxiliary_targets_absent(
            &runtime_root(source.paths()),
            &runtime_root(target.paths()),
        )?;
        write_in_progress_journal(&target, request, &source_fingerprint, record_count)?;
    }

    copy_store_if_needed(
        &source,
        &target,
        &transfer_tables,
        &source_primary_fingerprint,
        source_primary_records,
    )
    .context("copy stateful storage into target")?;
    copy_auxiliary_databases(&runtime_root(source.paths()), &runtime_root(target.paths()))
        .context("copy authoritative session and runtime-event stores")?;
    synchronize_postgres_sequences(&target)?;

    let (target_fingerprint, target_records) =
        fingerprint_transfer_state(&target, &transfer_tables, &runtime_root(target.paths()))
            .context("fingerprint imported target storage state")?;
    if target_fingerprint != source_fingerprint || target_records != record_count {
        bail!(
            "stateful storage transfer verification failed: source {source_fingerprint} ({record_count} rows), target {target_fingerprint} ({target_records} rows); target remains fail-closed"
        );
    }
    mark_journal_complete(&target, &target_fingerprint, record_count)?;

    Ok(report(
        request,
        source_fingerprint,
        target_fingerprint,
        record_count,
        false,
    ))
}

fn report(
    request: &StatefulBackendMigrationRequest,
    source_fingerprint: String,
    target_fingerprint: String,
    record_count: u64,
    already_complete: bool,
) -> StatefulBackendMigrationReport {
    StatefulBackendMigrationReport {
        source_backend: request.source_backend,
        target_backend: request.target_backend,
        source_root: runtime_root(&request.source_paths),
        target_root: runtime_root(&request.target_paths),
        source_fingerprint,
        target_fingerprint,
        record_count,
        already_complete,
    }
}

fn runtime_root(paths: &OrchestrationStorePaths) -> PathBuf {
    paths
        .database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

pub(super) fn ensure_backend_transfer_schema(
    store: &OrchestrationStateStore,
) -> anyhow::Result<()> {
    store.with_connection(|connection| {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS stateful_backend_transfers (
                transfer_id TEXT PRIMARY KEY,
                source_backend TEXT NOT NULL,
                target_backend TEXT NOT NULL,
                source_fingerprint TEXT NOT NULL,
                target_fingerprint TEXT,
                record_count BIGINT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms BIGINT NOT NULL,
                completed_at_ms BIGINT
             );",
        )?;
        Ok(())
    })
}

pub(super) fn ensure_target_is_not_mid_transfer(
    store: &OrchestrationStateStore,
) -> anyhow::Result<()> {
    ensure_backend_transfer_schema(store)?;
    if let Some(journal) = read_journal(store)? {
        if journal.status == "in_progress" {
            bail!(
                "stateful storage backend transfer is in progress for {}; rerun `tandem-engine storage migrate` to verify it before starting the engine",
                runtime_root(store.paths()).display()
            );
        }
        if journal.status != "complete" {
            bail!(
                "stateful storage backend transfer has unknown status `{}`; refusing startup",
                journal.status
            );
        }
    }
    Ok(())
}

fn read_journal(store: &OrchestrationStateStore) -> anyhow::Result<Option<TransferJournal>> {
    ensure_backend_transfer_schema(store)?;
    store.with_connection(|connection| {
        connection
            .query_row(
                "SELECT source_fingerprint, target_fingerprint, record_count, status
                 FROM stateful_backend_transfers WHERE transfer_id = ?1",
                [TRANSFER_ID],
                |row| {
                    Ok(TransferJournal {
                        source_fingerprint: row.get(0)?,
                        target_fingerprint: row.get(1)?,
                        record_count: row.get::<_, i64>(2)? as u64,
                        status: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    })
}

fn write_in_progress_journal(
    target: &OrchestrationStateStore,
    request: &StatefulBackendMigrationRequest,
    source_fingerprint: &str,
    record_count: u64,
) -> anyhow::Result<()> {
    ensure_backend_transfer_schema(target)?;
    target.with_connection(|connection| {
        connection.execute_raw(
            "INSERT INTO stateful_backend_transfers
             (transfer_id, source_backend, target_backend, source_fingerprint,
              target_fingerprint, record_count, status, started_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, 'in_progress', ?6, NULL)",
            &[
                Value::Text(TRANSFER_ID.to_string()),
                Value::Text(request.source_backend.as_str().to_string()),
                Value::Text(request.target_backend.as_str().to_string()),
                Value::Text(source_fingerprint.to_string()),
                Value::Integer(record_count as i64),
                Value::Integer(now_ms() as i64),
            ],
        )?;
        Ok(())
    })
}

fn mark_journal_complete(
    target: &OrchestrationStateStore,
    target_fingerprint: &str,
    record_count: u64,
) -> anyhow::Result<()> {
    target.with_connection(|connection| {
        let changed = connection.execute_raw(
            "UPDATE stateful_backend_transfers
             SET target_fingerprint = ?2, record_count = ?3, status = 'complete', completed_at_ms = ?4
             WHERE transfer_id = ?1 AND status = 'in_progress'",
            &[
                Value::Text(TRANSFER_ID.to_string()),
                Value::Text(target_fingerprint.to_string()),
                Value::Integer(record_count as i64),
                Value::Integer(now_ms() as i64),
            ],
        )?;
        if changed != 1 {
            bail!("stateful backend transfer journal changed while verification was running");
        }
        Ok(())
    })
}

fn transfer_tables(
    source: &OrchestrationStateStore,
    target: &OrchestrationStateStore,
) -> anyhow::Result<Vec<TransferTable>> {
    let source_names = table_names(source)?;
    let target_names = table_names(target)?;
    if source_names != target_names {
        bail!("source and target stateful storage schemas expose different tables");
    }

    let mut tables = Vec::new();
    for name in source_names {
        let source_columns = table_columns(source, &name)?;
        let target_columns = table_columns(target, &name)?;
        let source_column_set = source_columns
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let target_column_set = target_columns
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let unsupported_source_columns = source_column_set
            .difference(&target_column_set)
            .filter(|column| {
                !(**column == "rowid" && GENERATED_ROWID_TABLES.contains(&name.as_str()))
            })
            .copied()
            .collect::<Vec<_>>();
        let unsupported_target_columns = target_column_set
            .difference(&source_column_set)
            .filter(|column| {
                !(**column == "rowid" && GENERATED_ROWID_TABLES.contains(&name.as_str()))
            })
            .copied()
            .collect::<Vec<_>>();
        if !unsupported_source_columns.is_empty() || !unsupported_target_columns.is_empty() {
            bail!(
                "source and target stateful storage table `{name}` expose incompatible columns (source-only: {}, target-only: {})",
                unsupported_source_columns.join(", "),
                unsupported_target_columns.join(", ")
            );
        }

        let mut columns = source_columns
            .into_iter()
            .filter(|column| target_column_set.contains(column.as_str()))
            .filter(|column| column != "rowid")
            .collect::<Vec<_>>();
        if GENERATED_ROWID_TABLES.contains(&name.as_str()) {
            columns.insert(0, "rowid".to_string());
        }
        if columns.is_empty() {
            bail!("stateful storage table `{name}` has no transferable columns");
        }
        tables.push(TransferTable { name, columns });
    }
    if tables.is_empty() {
        bail!("source stateful storage has no transferable tables");
    }
    Ok(tables)
}

fn table_names(store: &OrchestrationStateStore) -> anyhow::Result<BTreeSet<String>> {
    let names = store.with_connection(|connection| match &store.backend {
        #[cfg(feature = "storage-sqlite")]
        StoreBackendSelection::Sqlite => connection
            .query_raw(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                &[],
            )
            .map_err(Into::into),
        #[cfg(feature = "storage-postgres")]
        StoreBackendSelection::Postgres(_) => connection
            .query_raw(
                "SELECT table_name FROM information_schema.tables
                 WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'
                 ORDER BY table_name",
                &[],
            )
            .map_err(Into::into),
    })?;
    names
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .filter(|result| {
            result
                .as_ref()
                .map(|name| !EXCLUDED_TABLES.contains(&name.as_str()))
                .unwrap_or(true)
        })
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(Into::into)
}

fn table_columns(store: &OrchestrationStateStore, table: &str) -> anyhow::Result<Vec<String>> {
    let quoted = quote_identifier(table);
    let rows = store.with_connection(|connection| match &store.backend {
        #[cfg(feature = "storage-sqlite")]
        StoreBackendSelection::Sqlite => connection
            .query_raw(&format!("PRAGMA table_info({quoted})"), &[])
            .map_err(Into::into),
        #[cfg(feature = "storage-postgres")]
        StoreBackendSelection::Postgres(_) => connection
            .query_raw(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_schema = current_schema() AND table_name = ?1
                 ORDER BY ordinal_position",
                &[Value::Text(table.to_string())],
            )
            .map_err(Into::into),
    })?;
    let columns = rows
        .into_iter()
        .map(|row| {
            if store.backend_is_sqlite() {
                row.get(1)
            } else {
                row.get(0)
            }
        })
        .collect::<Result<Vec<String>, _>>()?;
    if columns.is_empty() {
        bail!("stateful storage table `{table}` has no columns");
    }
    Ok(columns)
}

fn ensure_target_empty(
    target: &OrchestrationStateStore,
    tables: &[TransferTable],
) -> anyhow::Result<()> {
    for table in tables {
        let count = target.with_connection(|connection| {
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {}", quote_identifier(&table.name)),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })?;
        if count != 0 {
            bail!(
                "target stateful storage table `{}` is not empty; refusing to overwrite existing state",
                table.name
            );
        }
    }
    Ok(())
}

fn copy_store_if_needed(
    source: &OrchestrationStateStore,
    target: &OrchestrationStateStore,
    tables: &[TransferTable],
    source_fingerprint: &str,
    source_records: u64,
) -> anyhow::Result<()> {
    let (target_fingerprint, target_records) = fingerprint_store(target, tables)?;
    if target_records == 0 {
        return copy_store(source, target, tables);
    }
    if target_fingerprint == source_fingerprint && target_records == source_records {
        return Ok(());
    }
    bail!(
        "target stateful storage contains an interrupted or unrelated transfer that does not match the locked source"
    );
}

fn ensure_auxiliary_targets_absent(source_root: &Path, target_root: &Path) -> anyhow::Result<()> {
    if source_root == target_root {
        return Ok(());
    }
    for (name, relative_path) in AUXILIARY_DATABASES {
        let target = target_root.join(relative_path);
        if target.exists() {
            bail!(
                "target {name} database {} already exists; choose an empty target state directory",
                target.display()
            );
        }
    }
    Ok(())
}

fn copy_auxiliary_databases(source_root: &Path, target_root: &Path) -> anyhow::Result<()> {
    for (name, relative_path) in AUXILIARY_DATABASES {
        let source = source_root.join(relative_path);
        let target = target_root.join(relative_path);
        if source == target {
            continue;
        }
        match (source.exists(), target.exists()) {
            (false, false) => continue,
            (false, true) => bail!(
                "target {name} database {} exists although the source has none",
                target.display()
            ),
            (true, false) => copy_sqlite_database(&source, &target)
                .with_context(|| format!("copy {name} database {}", source.display()))?,
            (true, true) => {
                let source_fingerprint = fingerprint_sqlite_database(&source)?;
                let target_fingerprint = fingerprint_sqlite_database(&target)?;
                if source_fingerprint != target_fingerprint {
                    bail!(
                        "target {name} database {} does not match the locked source; refusing to overwrite it",
                        target.display()
                    );
                }
            }
        }

        let source_fingerprint = fingerprint_sqlite_database(&source)?;
        let target_fingerprint = fingerprint_sqlite_database(&target)?;
        if source_fingerprint != target_fingerprint {
            bail!(
                "{name} database copy verification failed for target {}",
                target.display()
            );
        }
    }
    Ok(())
}

fn copy_sqlite_database(source: &Path, target: &Path) -> anyhow::Result<()> {
    let parent = target
        .parent()
        .context("auxiliary SQLite target path has no parent")?;
    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "create auxiliary SQLite target directory {}",
            parent.display()
        )
    })?;
    let connection = SqliteConnection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open auxiliary SQLite source {}", source.display()))?;
    connection
        .execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .with_context(|| format!("snapshot auxiliary SQLite source {}", source.display()))?;
    Ok(())
}

fn fingerprint_transfer_state(
    store: &OrchestrationStateStore,
    tables: &[TransferTable],
    root: &Path,
) -> anyhow::Result<(String, u64)> {
    let (stateful_fingerprint, stateful_records) = fingerprint_store(store, tables)?;
    let (auxiliary_fingerprint, auxiliary_records) = fingerprint_auxiliary_databases(root)?;
    let mut digest = Sha256::new();
    update_bytes(&mut digest, b"stateful");
    update_bytes(&mut digest, stateful_fingerprint.as_bytes());
    update_bytes(&mut digest, b"auxiliary");
    update_bytes(&mut digest, auxiliary_fingerprint.as_bytes());
    Ok((
        format!("{:x}", digest.finalize()),
        stateful_records + auxiliary_records,
    ))
}

fn fingerprint_auxiliary_databases(root: &Path) -> anyhow::Result<(String, u64)> {
    let mut digest = Sha256::new();
    let mut record_count = 0_u64;
    for (name, relative_path) in AUXILIARY_DATABASES {
        update_bytes(&mut digest, name.as_bytes());
        let (fingerprint, records) = fingerprint_sqlite_database(&root.join(relative_path))?;
        update_bytes(&mut digest, fingerprint.as_bytes());
        record_count += records;
    }
    Ok((format!("{:x}", digest.finalize()), record_count))
}

fn fingerprint_sqlite_database(path: &Path) -> anyhow::Result<(String, u64)> {
    let mut digest = Sha256::new();
    if !path.exists() {
        update_bytes(&mut digest, b"missing");
        return Ok((format!("{:x}", digest.finalize()), 0));
    }
    let connection = SqliteConnection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open auxiliary SQLite database {}", path.display()))?;
    let table_names = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut record_count = 0_u64;
    for table in table_names {
        update_bytes(&mut digest, table.as_bytes());
        let quoted_table = quote_identifier(&table);
        let columns = connection
            .prepare(&format!("PRAGMA table_info({quoted_table})"))?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        for column in &columns {
            update_bytes(&mut digest, column.as_bytes());
        }
        let quoted_columns = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>();
        let order = quoted_columns
            .iter()
            .map(|column| format!("({column} IS NOT NULL), {column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {} FROM {quoted_table} ORDER BY {order}",
            quoted_columns.join(", ")
        );
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            for index in 0..columns.len() {
                update_sqlite_value(&mut digest, &row.get::<_, rusqlite::types::Value>(index)?);
            }
            record_count += 1;
        }
    }
    Ok((format!("{:x}", digest.finalize()), record_count))
}

fn fingerprint_store(
    store: &OrchestrationStateStore,
    tables: &[TransferTable],
) -> anyhow::Result<(String, u64)> {
    let mut digest = Sha256::new();
    let mut record_count = 0_u64;
    for table in tables {
        update_bytes(&mut digest, table.name.as_bytes());
        for column in &table.columns {
            update_bytes(&mut digest, column.as_bytes());
        }
        scan_table(store, table, |rows| {
            for row in rows {
                for value in row {
                    update_value(&mut digest, value);
                }
                record_count += 1;
            }
            Ok(())
        })?;
    }
    Ok((format!("{:x}", digest.finalize()), record_count))
}

fn copy_store(
    source: &OrchestrationStateStore,
    target: &OrchestrationStateStore,
    tables: &[TransferTable],
) -> anyhow::Result<()> {
    target.with_connection(|connection| {
        let transaction = connection
            .transaction_with_behavior(backend::TransactionBehavior::Immediate)
            .context("begin target stateful storage import transaction")?;
        for table in tables {
            let insert = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                quote_identifier(&table.name),
                table
                    .columns
                    .iter()
                    .map(|column| quote_identifier(column))
                    .collect::<Vec<_>>()
                    .join(", "),
                (1..=table.columns.len())
                    .map(|index| format!("?{index}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            scan_table(source, table, |rows| {
                for row in rows {
                    transaction.execute_raw(&insert, row)?;
                }
                Ok(())
            })?;
        }
        transaction
            .commit()
            .context("commit target stateful storage import")?;
        Ok(())
    })
}

fn scan_table(
    store: &OrchestrationStateStore,
    table: &TransferTable,
    mut visit: impl FnMut(&[Vec<Value>]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let columns = table
        .columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let order = table
        .columns
        .iter()
        .map(|column| {
            let column = quote_identifier(column);
            format!("({column} IS NOT NULL), {column}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {columns} FROM {} ORDER BY {order} LIMIT ?1 OFFSET ?2",
        quote_identifier(&table.name)
    );
    let mut offset = 0_i64;
    loop {
        let rows = store.with_connection(|connection| {
            connection
                .query_raw(&sql, &[Value::Integer(BATCH_ROWS), Value::Integer(offset)])
                .map_err(Into::into)
        })?;
        if rows.is_empty() {
            break;
        }
        let values = rows
            .iter()
            .map(|row| row.values().to_vec())
            .collect::<Vec<_>>();
        if values.iter().any(|row| row.len() != table.columns.len()) {
            bail!(
                "stateful storage table `{}` returned an unexpected row shape",
                table.name
            );
        }
        offset += values.len() as i64;
        visit(&values)?;
    }
    Ok(())
}

fn synchronize_postgres_sequences(target: &OrchestrationStateStore) -> anyhow::Result<()> {
    #[cfg(feature = "storage-postgres")]
    if matches!(target.backend, StoreBackendSelection::Postgres(_)) {
        target.with_connection(|connection| {
            for (table, column) in [
                ("stateful_events", "rowid"),
                ("outbox_effects", "rowid"),
                ("dead_letters", "rowid"),
                ("compensations", "rowid"),
                ("tool_effects", "rowid"),
                ("stateful_migration_attempts", "attempt_id"),
            ] {
                connection.query_raw(
                    &format!(
                        "SELECT setval(pg_get_serial_sequence('{}', '{}'), \
                         COALESCE((SELECT MAX({}) FROM {}), 1), \
                         EXISTS (SELECT 1 FROM {}))",
                        table,
                        column,
                        quote_identifier(column),
                        quote_identifier(table),
                        quote_identifier(table),
                    ),
                    &[],
                )?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn update_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn update_value(digest: &mut Sha256, value: &Value) {
    match value {
        Value::Null => digest.update([0]),
        Value::Integer(value) => {
            digest.update([1]);
            digest.update(value.to_be_bytes());
        }
        Value::Real(value) => {
            digest.update([2]);
            digest.update(value.to_bits().to_be_bytes());
        }
        Value::Text(value) => {
            digest.update([3]);
            update_bytes(digest, value.as_bytes());
        }
        Value::Blob(value) => {
            digest.update([4]);
            update_bytes(digest, value);
        }
        Value::OutOfRange(value) => {
            digest.update([5]);
            update_bytes(digest, value.to_string().as_bytes());
        }
    }
}

fn update_sqlite_value(digest: &mut Sha256, value: &rusqlite::types::Value) {
    match value {
        rusqlite::types::Value::Null => digest.update([0]),
        rusqlite::types::Value::Integer(value) => {
            digest.update([1]);
            digest.update(value.to_be_bytes());
        }
        rusqlite::types::Value::Real(value) => {
            digest.update([2]);
            digest.update(value.to_bits().to_be_bytes());
        }
        rusqlite::types::Value::Text(value) => {
            digest.update([3]);
            update_bytes(digest, value.as_bytes());
        }
        rusqlite::types::Value::Blob(value) => {
            digest.update([4]);
            update_bytes(digest, value);
        }
    }
}

impl OrchestrationStateStore {
    fn backend_is_sqlite(&self) -> bool {
        #[cfg(feature = "storage-sqlite")]
        if matches!(self.backend, StoreBackendSelection::Sqlite) {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stateful_runtime::backend::Executor;

    fn paths(root: &std::path::Path) -> OrchestrationStorePaths {
        OrchestrationStorePaths {
            database_path: root.join("stateful_runtime.sqlite3"),
            engine_lock_path: root.join("stateful_runtime.engine.lock"),
        }
    }

    #[cfg(feature = "storage-postgres")]
    fn postgres_url() -> Option<String> {
        std::env::var("TANDEM_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }

    #[cfg(feature = "storage-postgres")]
    #[test]
    fn sqlite_postgres_round_trip_preserves_rows_and_cursor_values() {
        let Some(url) = postgres_url() else {
            eprintln!("skipping stateful backend transfer test without TANDEM_TEST_POSTGRES_URL");
            return;
        };
        let source_root = tempfile::tempdir().unwrap();
        let postgres_root = tempfile::tempdir().unwrap();
        let restored_root = tempfile::tempdir().unwrap();
        let source_paths = paths(source_root.path());
        let postgres_paths = paths(postgres_root.path());
        let restored_paths = paths(restored_root.path());
        let source = OrchestrationStateStore::open_with_config(
            source_paths.clone(),
            backend::StorageBackendConfig::Sqlite,
        )
        .unwrap();
        source
            .with_connection(|connection| {
                connection.execute_raw(
                    "INSERT INTO automation_runs
                     (run_id, automation_id, org_id, workspace_id, deployment_id, status,
                      is_hot, run_json, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9)",
                    &[
                        Value::Text("run-transfer".to_string()),
                        Value::Text("automation-transfer".to_string()),
                        Value::Text("org-transfer".to_string()),
                        Value::Text("workspace-transfer".to_string()),
                        Value::Text("queued".to_string()),
                        Value::Integer(1),
                        Value::Text("{\"sealed\":\"tce1:opaque\"}".to_string()),
                        Value::Integer(1),
                        Value::Integer(2),
                    ],
                )?;
                connection.execute_raw(
                    "INSERT INTO stateful_events
                     (rowid, event_id, goal_id, run_id, seq, event_json, created_at_ms,
                      org_id, workspace_id, deployment_id)
                     VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
                    &[
                        Value::Integer(41),
                        Value::Text("event-transfer".to_string()),
                        Value::Text("run-transfer".to_string()),
                        Value::Integer(7),
                        Value::Text("{\"event\":\"sealed\"}".to_string()),
                        Value::Integer(3),
                        Value::Text("org-transfer".to_string()),
                        Value::Text("workspace-transfer".to_string()),
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        seed_session_messages(source_root.path());
        seed_runtime_events(source_root.path());

        let request = StatefulBackendMigrationRequest {
            source_paths: source_paths.clone(),
            target_paths: postgres_paths.clone(),
            source_backend: StatefulBackendKind::Sqlite,
            target_backend: StatefulBackendKind::Postgres,
            source_postgres_url: None,
            target_postgres_url: Some(url.clone()),
        };
        let staged_target = OrchestrationStateStore::open_for_backend_transfer_target(
            postgres_paths.clone(),
            backend::StorageBackendConfig::Postgres { url: url.clone() },
        )
        .unwrap();
        let source_tables = transfer_tables(&source, &staged_target).unwrap();
        let (source_fingerprint, source_records) =
            fingerprint_transfer_state(&source, &source_tables, source_root.path()).unwrap();
        write_in_progress_journal(
            &staged_target,
            &request,
            &source_fingerprint,
            source_records,
        )
        .unwrap();
        copy_store(&source, &staged_target, &source_tables).unwrap();
        synchronize_postgres_sequences(&staged_target).unwrap();
        drop(staged_target);

        let first = migrate_stateful_storage_backend(&request).unwrap();
        assert!(!first.already_complete);
        assert!(first.record_count >= 4);

        let restored = migrate_stateful_storage_backend(&StatefulBackendMigrationRequest {
            source_paths: postgres_paths.clone(),
            target_paths: restored_paths.clone(),
            source_backend: StatefulBackendKind::Postgres,
            target_backend: StatefulBackendKind::Sqlite,
            source_postgres_url: Some(url.clone()),
            target_postgres_url: None,
        })
        .unwrap();
        assert_eq!(restored.source_fingerprint, restored.target_fingerprint);

        let round_trip = OrchestrationStateStore::open_with_config(
            restored_paths,
            backend::StorageBackendConfig::Sqlite,
        )
        .unwrap();
        round_trip
            .with_connection(|connection| {
                let rowid: i64 = connection.query_row(
                    "SELECT rowid FROM stateful_events WHERE event_id = ?1",
                    ["event-transfer"],
                    |row| row.get(0),
                )?;
                assert_eq!(rowid, 41);
                let payload: String = connection.query_row(
                    "SELECT run_json FROM automation_runs WHERE run_id = ?1",
                    ["run-transfer"],
                    |row| row.get(0),
                )?;
                assert_eq!(payload, "{\"sealed\":\"tce1:opaque\"}");
                Ok(())
            })
            .unwrap();

        let restored_sessions = rusqlite::Connection::open(
            restored_root
                .path()
                .join("storage")
                .join("sessions.sqlite3"),
        )
        .unwrap();
        let session_part: String = restored_sessions
            .query_row(
                "SELECT part_json FROM session_message_parts LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(session_part.contains("tce1:session-ciphertext"));
        let restored_events =
            rusqlite::Connection::open(restored_root.path().join("runtime").join("events.sqlite3"))
                .unwrap();
        let runtime_event: String = restored_events
            .query_row(
                "SELECT row_json FROM runtime_event_rows LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(runtime_event.contains("tce1:event-ciphertext"));

        let schema = match &OrchestrationStateStore::open_for_backend_transfer_target(
            postgres_paths,
            backend::StorageBackendConfig::Postgres { url: url.clone() },
        )
        .unwrap()
        .backend
        {
            StoreBackendSelection::Postgres(target) => target.schema().to_string(),
            _ => unreachable!(),
        };
        crate::stateful_runtime::backend::postgres::drop_schema_for_tests(&url, &schema).unwrap();
    }

    #[cfg(feature = "storage-postgres")]
    fn seed_session_messages(root: &std::path::Path) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let storage = tandem_core::Storage::new(root.join("storage"))
                .await
                .unwrap();
            let mut session = tandem_types::Session::new(
                Some("transfer session".to_string()),
                Some(".".to_string()),
            );
            session.messages.push(tandem_types::Message::new(
                tandem_types::MessageRole::User,
                vec![tandem_types::MessagePart::Text {
                    text: "tce1:session-ciphertext".to_string(),
                }],
            ));
            storage.save_session(session).await.unwrap();
        });
    }

    #[cfg(feature = "storage-postgres")]
    fn seed_runtime_events(root: &std::path::Path) {
        let path = root.join("runtime").join("events.sqlite3");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE runtime_event_rows (
                    event_id TEXT PRIMARY KEY,
                    run_id TEXT,
                    session_id TEXT,
                    seq INTEGER NOT NULL,
                    occurred_at_ms INTEGER NOT NULL,
                    org_id TEXT,
                    workspace_id TEXT,
                    deployment_id TEXT,
                    row_json TEXT NOT NULL
                 );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO runtime_event_rows
                 (event_id, run_id, session_id, seq, occurred_at_ms, org_id, workspace_id,
                  deployment_id, row_json)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, NULL, ?7)",
                rusqlite::params![
                    "runtime-event-transfer",
                    "run-transfer",
                    7_i64,
                    3_i64,
                    "org-transfer",
                    "workspace-transfer",
                    r#"{"payload":"tce1:event-ciphertext"}"#,
                ],
            )
            .unwrap();
    }
}
