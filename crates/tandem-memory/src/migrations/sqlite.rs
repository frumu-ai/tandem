use super::{LogicalMigration, SqliteMigrationMode, MEMORY_SCHEMA_REGISTRY};
use crate::types::{MemoryError, MemoryResult};
use chrono::Utc;
use rusqlite::{params, Connection, Transaction};
use std::collections::{HashMap, HashSet};

const PRIVATE_OWNER_MIGRATION_VERSION: i64 = 5;
const CHUNK_TABLES: &[&str] = &[
    "session_memory_chunks",
    "project_memory_chunks",
    "global_memory_chunks",
];

/// Apply every pending current SQLite migration.
///
/// Versions 1-4 are an explicit compatibility boundary: their schema work is
/// still performed by the legacy bootstrap in `init_schema`, before this is
/// called. They are baselined here, while version 5 and later are translated
/// and recorded in the same SQLite transaction.
pub(crate) fn run_sqlite_migrations(conn: &mut Connection) -> MemoryResult<()> {
    validate_registry()?;

    let tx = conn.transaction()?;
    ensure_ledger(&tx)?;
    let applied_versions = validate_ledger(&tx)?;
    tx.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_schema_migrations_name
         ON schema_migrations(name)",
        [],
    )?;

    for migration in MEMORY_SCHEMA_REGISTRY.pending_current(&applied_versions) {
        match migration.sqlite_mode {
            SqliteMigrationMode::LegacyBootstrapBaseline => {}
            SqliteMigrationMode::Executable => translate(&tx, migration)?,
        }
        record_migration(&tx, migration)?;
    }

    tx.commit()?;
    Ok(())
}

fn validate_registry() -> MemoryResult<()> {
    let mut versions = HashSet::new();
    let mut names = HashSet::new();
    for migration in MEMORY_SCHEMA_REGISTRY.all() {
        if migration.version <= 0 {
            return Err(MemoryError::InvalidConfig(format!(
                "memory migration version must be positive: {}",
                migration.version
            )));
        }
        if !versions.insert(migration.version) {
            return Err(MemoryError::InvalidConfig(format!(
                "duplicate memory migration version {}",
                migration.version
            )));
        }
        if !names.insert(migration.name) {
            return Err(MemoryError::InvalidConfig(format!(
                "duplicate memory migration name '{}'",
                migration.name
            )));
        }
    }
    Ok(())
}

fn ensure_ledger(tx: &Transaction<'_>) -> MemoryResult<()> {
    tx.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

fn validate_ledger(tx: &Transaction<'_>) -> MemoryResult<Vec<i64>> {
    let expected_by_version = MEMORY_SCHEMA_REGISTRY
        .all()
        .iter()
        .map(|migration| (migration.version, migration.name))
        .collect::<HashMap<_, _>>();
    let expected_by_name = MEMORY_SCHEMA_REGISTRY
        .all()
        .iter()
        .map(|migration| (migration.name, migration.version))
        .collect::<HashMap<_, _>>();

    let mut stmt = tx.prepare("SELECT version, name FROM schema_migrations ORDER BY version")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut applied = Vec::with_capacity(rows.len());
    for (version, name) in rows {
        let expected_name = expected_by_version.get(&version).copied();
        let expected_version = expected_by_name.get(name.as_str()).copied();
        if expected_name != Some(name.as_str()) || expected_version != Some(version) {
            return Err(MemoryError::InvalidConfig(format!(
                "memory migration ledger conflict for version {version} and name '{name}'"
            )));
        }
        applied.push(version);
    }
    Ok(applied)
}

fn record_migration(tx: &Transaction<'_>, migration: &LogicalMigration) -> MemoryResult<()> {
    tx.execute(
        "INSERT INTO schema_migrations (version, name, applied_at_ms) VALUES (?1, ?2, ?3)",
        params![
            migration.version,
            migration.name,
            Utc::now().timestamp_millis()
        ],
    )?;
    Ok(())
}

fn translate(tx: &Transaction<'_>, migration: &LogicalMigration) -> MemoryResult<()> {
    match migration.version {
        PRIVATE_OWNER_MIGRATION_VERSION => migrate_private_owner_scope(tx),
        version => Err(MemoryError::InvalidConfig(format!(
            "no SQLite translator for executable memory migration {version} ('{}')",
            migration.name
        ))),
    }
}

fn migrate_private_owner_scope(tx: &Transaction<'_>) -> MemoryResult<()> {
    for table in CHUNK_TABLES {
        add_private_owner_columns(tx, table)?;
        backfill_chunk_private_owner(tx, table)?;
    }

    add_private_owner_columns(tx, "memory_records")?;
    backfill_memory_record_private_owner(tx)?;
    create_private_owner_indexes(tx)?;

    for table in CHUNK_TABLES
        .iter()
        .copied()
        .chain(std::iter::once("memory_records"))
    {
        create_private_owner_constraint_triggers(tx, table)?;
    }
    Ok(())
}

fn table_columns(tx: &Transaction<'_>, table: &str) -> MemoryResult<HashSet<String>> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<HashSet<_>, _>>()?;
    if columns.is_empty() {
        return Err(MemoryError::InvalidConfig(format!(
            "memory migration requires missing table '{table}'"
        )));
    }
    Ok(columns)
}

fn add_private_owner_columns(tx: &Transaction<'_>, table: &str) -> MemoryResult<()> {
    let columns = table_columns(tx, table)?;
    if !columns.contains("private") {
        tx.execute(
            &format!("ALTER TABLE {table} ADD COLUMN private INTEGER NOT NULL DEFAULT 0"),
            [],
        )?;
    }
    if !columns.contains("owner_subject") {
        tx.execute(
            &format!("ALTER TABLE {table} ADD COLUMN owner_subject TEXT"),
            [],
        )?;
    }
    Ok(())
}

fn backfill_chunk_private_owner(tx: &Transaction<'_>, table: &str) -> MemoryResult<()> {
    tx.execute(
        &format!(
            "UPDATE {table}
             SET owner_subject = COALESCE(
                     NULLIF(TRIM(owner_subject), ''),
                     NULLIF(TRIM(subject), '')
                 ),
                 private = CASE
                     WHEN COALESCE(
                         NULLIF(TRIM(owner_subject), ''),
                         NULLIF(TRIM(subject), '')
                     ) IS NULL THEN 0
                     ELSE 1
                 END"
        ),
        [],
    )?;
    Ok(())
}

fn backfill_memory_record_private_owner(tx: &Transaction<'_>) -> MemoryResult<()> {
    tx.execute(
        "UPDATE memory_records
         SET owner_subject = COALESCE(
                 NULLIF(TRIM(owner_subject), ''),
                 CASE
                     WHEN metadata IS NOT NULL AND metadata <> '' AND json_valid(metadata)
                     THEN NULLIF(TRIM(json_extract(metadata, '$.owner_subject')), '')
                 END,
                 CASE
                     WHEN source_type IN (
                         'user_message', 'assistant_final', 'tool_event', 'tool_input',
                         'tool_output', 'question_prompt', 'plan_todos'
                     )
                     THEN COALESCE(NULLIF(TRIM(user_id), ''), 'legacy-unowned:' || id)
                 END
             ),
             private = CASE
                 WHEN COALESCE(
                     NULLIF(TRIM(owner_subject), ''),
                     CASE
                         WHEN metadata IS NOT NULL AND metadata <> '' AND json_valid(metadata)
                         THEN NULLIF(TRIM(json_extract(metadata, '$.owner_subject')), '')
                     END,
                     CASE
                         WHEN source_type IN (
                             'user_message', 'assistant_final', 'tool_event', 'tool_input',
                             'tool_output', 'question_prompt', 'plan_todos'
                         )
                         THEN COALESCE(NULLIF(TRIM(user_id), ''), 'legacy-unowned:' || id)
                     END
                 ) IS NULL THEN 0
                 ELSE 1
             END",
        [],
    )?;
    Ok(())
}

fn create_private_owner_indexes(tx: &Transaction<'_>) -> MemoryResult<()> {
    tx.execute("DROP INDEX IF EXISTS idx_session_chunks_private_owner", [])?;
    tx.execute(
        "CREATE INDEX idx_session_chunks_private_owner
         ON session_memory_chunks(tenant_org_id, tenant_workspace_id,
             IFNULL(tenant_deployment_id, ''), owner_org_unit_id, private,
             owner_subject, session_id)",
        [],
    )?;
    tx.execute("DROP INDEX IF EXISTS idx_project_chunks_private_owner", [])?;
    tx.execute(
        "CREATE INDEX idx_project_chunks_private_owner
         ON project_memory_chunks(tenant_org_id, tenant_workspace_id,
             IFNULL(tenant_deployment_id, ''), owner_org_unit_id, private,
             owner_subject, project_id)",
        [],
    )?;
    tx.execute("DROP INDEX IF EXISTS idx_global_chunks_private_owner", [])?;
    tx.execute(
        "CREATE INDEX idx_global_chunks_private_owner
         ON global_memory_chunks(tenant_org_id, tenant_workspace_id,
             IFNULL(tenant_deployment_id, ''), owner_org_unit_id, private,
             owner_subject, created_at DESC)",
        [],
    )?;
    tx.execute("DROP INDEX IF EXISTS idx_memory_records_dedup", [])?;
    tx.execute(
        "CREATE UNIQUE INDEX idx_memory_records_dedup
         ON memory_records(tenant_org_id, tenant_workspace_id,
             IFNULL(tenant_deployment_id, ''), user_id, source_type, content_hash,
             run_id, IFNULL(session_id, ''), IFNULL(message_id, ''),
             IFNULL(tool_name, ''), IFNULL(owner_org_unit_id, ''), private,
             IFNULL(owner_subject, ''))",
        [],
    )?;
    tx.execute("DROP INDEX IF EXISTS idx_memory_records_private_owner", [])?;
    tx.execute(
        "CREATE INDEX idx_memory_records_private_owner
         ON memory_records(tenant_org_id, tenant_workspace_id,
             IFNULL(tenant_deployment_id, ''), owner_org_unit_id, private,
             owner_subject, created_at_ms DESC)",
        [],
    )?;
    Ok(())
}

fn create_private_owner_constraint_triggers(tx: &Transaction<'_>, table: &str) -> MemoryResult<()> {
    let invalid = "NEW.private IS NULL
        OR NEW.private NOT IN (0, 1)
        OR (NEW.private = 0 AND NEW.owner_subject IS NOT NULL)
        OR (NEW.private = 1 AND
            (NEW.owner_subject IS NULL OR TRIM(NEW.owner_subject) = ''))";
    // SQLite cannot add a table CHECK without rebuilding it. Trigger-backed
    // constraints preserve the chunk tables and their sqlite-vec companions.
    tx.execute_batch(&format!(
        "DROP TRIGGER IF EXISTS {table}_private_owner_insert;
         DROP TRIGGER IF EXISTS {table}_private_owner_update;
         CREATE TRIGGER {table}_private_owner_insert
         BEFORE INSERT ON {table}
         WHEN {invalid}
         BEGIN
             SELECT RAISE(ABORT, 'private owner subject constraint failed');
         END;
         CREATE TRIGGER {table}_private_owner_update
         BEFORE UPDATE OF private, owner_subject ON {table}
         WHEN {invalid}
         BEGIN
             SELECT RAISE(ABORT, 'private owner subject constraint failed');
         END;"
    ))?;
    Ok(())
}
