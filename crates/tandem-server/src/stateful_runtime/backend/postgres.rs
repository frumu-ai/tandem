// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! PostgreSQL arm of the storage backend facade (TAN-715).
//!
//! Store SQL is authored in the SQLite dialect; [`translate_sql`] rewrites it
//! at execution time (`?N` → `$N`, `IS ?N` → `IS NOT DISTINCT FROM $N`). DDL
//! is not translated: this module owns a PostgreSQL-native rendering of
//! schema v5, including a `rowid BIGSERIAL` column on the tables whose
//! queries rely on SQLite's implicit rowid for insertion-order cursors
//! (`stateful_events` durable SSE cursors and the reliability record loads).
//!
//! Every runtime root maps to its own PostgreSQL schema, recorded in a
//! sticky `stateful_runtime.pg_schema` marker file beside where the SQLite
//! database would live. That keeps distinct runtime roots isolated inside a
//! shared database and gives tests per-tempdir isolation for free.
//!
//! Connections come from a process-global r2d2 pool per (url, schema) pair —
//! stores are constructed ad hoc throughout the server, so pooling must not
//! be tied to any single store instance. The sync `postgres` client is used
//! (not tokio-postgres) because the whole store contract is synchronous; it
//! performs blocking network I/O exactly where the SQLite backend performs
//! blocking file I/O. TLS follows the memory store's convention (`NoTls`).

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context};
use postgres::types::{to_sql_checked, IsNull, ToSql, Type};
use r2d2_postgres::PostgresConnectionManager;
use sha2::{Digest, Sha256};

use super::{Connection, Error, Result, Row, Value};

use super::POSTGRES_SCHEMA_MARKER_FILE as SCHEMA_MARKER_FILE;

/// Rewrites store SQL from the SQLite dialect to PostgreSQL:
/// `?N` placeholders become `$N`, and the null-safe `IS ?N` / `IS NOT ?N`
/// comparisons become `IS [NOT] DISTINCT FROM $N`. Text inside single-quoted
/// literals and double-quoted identifiers is left untouched.
pub(crate) fn translate_sql(sql: &str) -> String {
    let mut output = String::with_capacity(sql.len() + 16);
    let mut chars = sql.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        match character {
            '\'' | '"' => {
                output.push(character);
                let quote = character;
                while let Some((_, inner)) = chars.next() {
                    output.push(inner);
                    if inner == quote {
                        // Doubled quotes are escapes; anything else ends the literal.
                        if chars.peek().is_some_and(|(_, next)| *next == quote) {
                            let (_, next) = chars.next().expect("peeked");
                            output.push(next);
                        } else {
                            break;
                        }
                    }
                }
            }
            '?' => {
                let mut digits = String::new();
                while let Some((_, digit)) = chars.peek() {
                    if digit.is_ascii_digit() {
                        digits.push(*digit);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if digits.is_empty() {
                    output.push('?');
                    continue;
                }
                rewrite_null_safe_comparison(&mut output);
                output.push('$');
                output.push_str(&digits);
            }
            _ => output.push(character),
        }
    }
    output
}

/// If the already-emitted SQL ends with the keyword `IS` (optionally `IS
/// NOT`), replace it with `IS [NOT] DISTINCT FROM`: SQLite's `x IS ?` is a
/// null-safe equality, which PostgreSQL spells `IS NOT DISTINCT FROM`.
fn rewrite_null_safe_comparison(output: &mut String) {
    let trimmed_len = output.trim_end().len();
    let trimmed = &output[..trimmed_len];
    let (head, negated) = match trimmed
        .get(trimmed_len.saturating_sub(4)..)
        .filter(|tail| tail.eq_ignore_ascii_case(" not"))
    {
        Some(_) => (&trimmed[..trimmed_len - 4], true),
        None => (trimmed, false),
    };
    let head_trimmed = head.trim_end();
    let is_keyword = head_trimmed.len() >= 2
        && head_trimmed
            .get(head_trimmed.len() - 2..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case("is"))
        && head_trimmed[..head_trimmed.len() - 2]
            .chars()
            .next_back()
            .is_none_or(|preceding| !preceding.is_alphanumeric() && preceding != '_');
    if !is_keyword {
        return;
    }
    let replacement = if negated {
        "IS DISTINCT FROM "
    } else {
        "IS NOT DISTINCT FROM "
    };
    output.truncate(head_trimmed.len() - 2);
    output.push_str(replacement);
}

impl ToSql for Value {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut postgres::types::private::BytesMut,
    ) -> std::result::Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Value::Null => Ok(IsNull::Yes),
            Value::Integer(value) => match *ty {
                Type::INT8 => value.to_sql(ty, out),
                Type::INT4 => i32::try_from(*value)?.to_sql(ty, out),
                Type::INT2 => i16::try_from(*value)?.to_sql(ty, out),
                Type::FLOAT8 => (*value as f64).to_sql(ty, out),
                Type::BOOL => (*value != 0).to_sql(ty, out),
                _ => Err(format!("cannot bind integer parameter as {ty}").into()),
            },
            Value::Real(value) => match *ty {
                Type::FLOAT8 => value.to_sql(ty, out),
                Type::FLOAT4 => (*value as f32).to_sql(ty, out),
                _ => Err(format!("cannot bind real parameter as {ty}").into()),
            },
            Value::Text(value) => match *ty {
                Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
                    value.as_str().to_sql(ty, out)
                }
                _ => Err(format!("cannot bind text parameter as {ty}").into()),
            },
            Value::Blob(value) => match *ty {
                Type::BYTEA => value.as_slice().to_sql(ty, out),
                _ => Err(format!("cannot bind blob parameter as {ty}").into()),
            },
            Value::OutOfRange(original) => Err(format!(
                "integer parameter {original} is out of the signed 64-bit storage range"
            )
            .into()),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        // Values are dynamically typed; mismatches are reported by `to_sql`
        // with the offending type in the message.
        true
    }

    to_sql_checked!();
}

fn column_value(row: &postgres::Row, index: usize) -> Result<Value> {
    let ty = row.columns()[index].type_();
    let value = match *ty {
        Type::INT8 => row
            .try_get::<_, Option<i64>>(index)?
            .map(Value::Integer)
            .unwrap_or(Value::Null),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(index)?
            .map(|value| Value::Integer(value.into()))
            .unwrap_or(Value::Null),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(index)?
            .map(|value| Value::Integer(value.into()))
            .unwrap_or(Value::Null),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(index)?
            .map(Value::Real)
            .unwrap_or(Value::Null),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(index)?
            .map(|value| Value::Real(value.into()))
            .unwrap_or(Value::Null),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => row
            .try_get::<_, Option<String>>(index)?
            .map(Value::Text)
            .unwrap_or(Value::Null),
        Type::BOOL => row
            .try_get::<_, Option<bool>>(index)?
            .map(|value| Value::Integer(i64::from(value)))
            .unwrap_or(Value::Null),
        Type::BYTEA => row
            .try_get::<_, Option<Vec<u8>>>(index)?
            .map(Value::Blob)
            .unwrap_or(Value::Null),
        _ => {
            return Err(Error::Conversion(format!(
                "unsupported PostgreSQL column type {ty} at column {index}"
            )))
        }
    };
    Ok(value)
}

fn convert_rows(rows: Vec<postgres::Row>) -> Result<Vec<Row>> {
    rows.into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|index| column_value(&row, index))
                .collect::<Result<Vec<_>>>()
                .map(Row::new)
        })
        .collect()
}

fn sql_params(params: &[Value]) -> Vec<&(dyn ToSql + Sync)> {
    params
        .iter()
        .map(|value| value as &(dyn ToSql + Sync))
        .collect()
}

type PooledClient = r2d2::PooledConnection<PostgresConnectionManager<postgres::NoTls>>;
type Pool = r2d2::Pool<PostgresConnectionManager<postgres::NoTls>>;

/// A checked-out pooled connection. Interior mutability bridges the facade's
/// `&self` execution methods onto the sync client's `&mut self` API; the
/// facade is used strictly single-threaded per connection.
pub(crate) struct PostgresConnection {
    client: std::cell::RefCell<PooledClient>,
    write_lock_key: i64,
}

impl PostgresConnection {
    pub(crate) fn execute(&self, sql: &str, params: &[Value]) -> Result<usize> {
        let translated = translate_sql(sql);
        let count = self
            .client
            .borrow_mut()
            .execute(translated.as_str(), &sql_params(params))?;
        Ok(count as usize)
    }

    pub(crate) fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        let translated = translate_sql(sql);
        let rows = self
            .client
            .borrow_mut()
            .query(translated.as_str(), &sql_params(params))?;
        convert_rows(rows)
    }

    pub(crate) fn batch_execute(&self, sql: &str) -> Result<()> {
        self.client.borrow_mut().batch_execute(sql)?;
        Ok(())
    }

    /// Begins a transaction. `immediate` reproduces SQLite's `BEGIN
    /// IMMEDIATE` contract: store writers assume they are serialized (e.g.
    /// `MAX(seq)+1` cursor allocation and check-then-insert idempotency), so
    /// immediate transactions take a transaction-scoped advisory lock keyed
    /// to this runtime root's schema. The lock releases automatically at
    /// commit or rollback, and reads outside transactions stay concurrent —
    /// the same single-writer-per-root model SQLite enforces today.
    pub(crate) fn begin_transaction(
        &mut self,
        immediate: bool,
    ) -> Result<postgres::Transaction<'_>> {
        let key = self.write_lock_key;
        let mut transaction = self.client.get_mut().transaction()?;
        if immediate {
            transaction.execute("SELECT pg_advisory_xact_lock($1)", &[&key])?;
        }
        Ok(transaction)
    }
}

pub(crate) fn transaction_execute(
    transaction: &mut postgres::Transaction<'_>,
    sql: &str,
    params: &[Value],
) -> Result<usize> {
    let translated = translate_sql(sql);
    let count = transaction.execute(translated.as_str(), &sql_params(params))?;
    Ok(count as usize)
}

pub(crate) fn transaction_query(
    transaction: &mut postgres::Transaction<'_>,
    sql: &str,
    params: &[Value],
) -> Result<Vec<Row>> {
    let translated = translate_sql(sql);
    let rows = transaction.query(translated.as_str(), &sql_params(params))?;
    convert_rows(rows)
}

pub(crate) fn transaction_batch(
    transaction: &mut postgres::Transaction<'_>,
    sql: &str,
) -> Result<()> {
    transaction.batch_execute(sql)?;
    Ok(())
}

/// One runtime root's PostgreSQL binding: the connection URL, the root's
/// dedicated schema, and a shared pool for that pair.
#[derive(Clone)]
pub(crate) struct PostgresTarget {
    url: String,
    schema: String,
    pool: Pool,
}

impl fmt::Debug for PostgresTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The URL may embed credentials; never expose it through Debug.
        f.debug_struct("PostgresTarget")
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}

impl PostgresTarget {
    pub(crate) fn for_root(url: &str, database_path: &Path) -> anyhow::Result<Self> {
        let schema = schema_for_root(database_path)?;
        let pool = pool_for(url, &schema)?;
        Ok(Self {
            url: url.to_string(),
            schema,
            pool,
        })
    }

    pub(crate) fn connect(&self) -> anyhow::Result<PostgresConnection> {
        let client = self.pool.get().with_context(|| {
            format!(
                "failed to acquire a PostgreSQL connection for stateful schema {}",
                self.schema
            )
        })?;
        Ok(PostgresConnection {
            client: std::cell::RefCell::new(client),
            write_lock_key: write_transaction_lock_key(&self.schema),
        })
    }

    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    /// Session-level advisory lock scoped to this runtime root's schema. The
    /// file-based engine lock protects one host; this protects the shared
    /// database when several hosts point at the same PostgreSQL schema. The
    /// lock is held by a dedicated (non-pooled) session and released when the
    /// returned guard drops.
    pub(crate) fn acquire_advisory_lock(&self) -> anyhow::Result<PostgresAdvisoryLock> {
        let mut client =
            postgres::Client::connect(&self.url, postgres::NoTls).with_context(|| {
                format!(
                    "failed to open the PostgreSQL engine-lock session for schema {}",
                    self.schema
                )
            })?;
        let key = advisory_lock_key(&self.schema);
        // A releasing holder's session close is processed asynchronously by
        // the server, so a hand-off between engines can observe the lock as
        // briefly held. Retry across that window before declaring contention.
        for attempt in 0..10 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let row = client.query_one("SELECT pg_try_advisory_lock($1)", &[&key])?;
            if row.get::<_, bool>(0) {
                return Ok(PostgresAdvisoryLock { _client: client });
            }
        }
        bail!(
            "another Tandem engine already holds the PostgreSQL stateful engine lock \
             for schema {} - stop that engine before starting another on this runtime root",
            self.schema
        );
    }
}

/// Holds a `pg_try_advisory_lock` session open; dropping the guard closes
/// the session, which releases the advisory lock server-side.
pub(crate) struct PostgresAdvisoryLock {
    _client: postgres::Client,
}

impl fmt::Debug for PostgresAdvisoryLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresAdvisoryLock")
            .finish_non_exhaustive()
    }
}

/// Removes a conformance test's schema so shared test databases do not
/// accumulate one schema per test run.
#[cfg(test)]
pub(crate) fn drop_schema_for_tests(url: &str, schema: &str) -> anyhow::Result<()> {
    validate_schema_name(schema)?;
    let mut client = postgres::Client::connect(url, postgres::NoTls)?;
    client.batch_execute(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))?;
    Ok(())
}

fn advisory_lock_key(schema: &str) -> i64 {
    let digest = Sha256::digest(format!("tandem-stateful-engine-lock:{schema}").as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("sha256 yields 32 bytes"))
}

/// Distinct from the engine-lock key: this one serializes immediate write
/// transactions within a runtime root's schema (see `begin_transaction`).
fn write_transaction_lock_key(schema: &str) -> i64 {
    let digest = Sha256::digest(format!("tandem-stateful-write-txn:{schema}").as_bytes());
    i64::from_be_bytes(digest[..8].try_into().expect("sha256 yields 32 bytes"))
}

/// Resolves (and pins) the PostgreSQL schema for a runtime root. The name is
/// derived from the root path and recorded in a marker file so it survives
/// path canonicalization changes; operators can also pre-seed the marker to
/// choose an explicit schema name.
fn schema_for_root(database_path: &Path) -> anyhow::Result<String> {
    let root = database_path.parent().unwrap_or_else(|| Path::new("."));
    let marker = root.join(SCHEMA_MARKER_FILE);
    if let Ok(existing) = std::fs::read_to_string(&marker) {
        let name = existing.trim().to_string();
        validate_schema_name(&name)?;
        return Ok(name);
    }
    let canonical = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string();
    let digest = Sha256::digest(canonical.as_bytes());
    let name = format!(
        "tandem_rt_{}",
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    std::fs::create_dir_all(root)?;
    std::fs::write(&marker, format!("{name}\n"))
        .with_context(|| format!("failed to record schema marker {}", marker.display()))?;
    Ok(name)
}

fn validate_schema_name(name: &str) -> anyhow::Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && name
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_');
    if !valid {
        bail!(
            "invalid PostgreSQL schema name `{name}` in {SCHEMA_MARKER_FILE}: expected \
             lowercase letters, digits, and underscores (max 63 chars)"
        );
    }
    Ok(())
}

#[derive(Debug)]
struct SchemaCustomizer {
    schema: String,
}

impl r2d2::CustomizeConnection<postgres::Client, postgres::Error> for SchemaCustomizer {
    fn on_acquire(
        &self,
        client: &mut postgres::Client,
    ) -> std::result::Result<(), postgres::Error> {
        // Schema names are validated to [a-z0-9_], so direct interpolation is
        // safe; quoting keeps reserved words usable as operator-chosen names.
        client.batch_execute(&format!("SET search_path TO \"{}\"", self.schema))
    }
}

fn pool_for(url: &str, schema: &str) -> anyhow::Result<Pool> {
    static POOLS: OnceLock<Mutex<HashMap<(String, String), Pool>>> = OnceLock::new();
    let pools = POOLS.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (url.to_string(), schema.to_string());
    {
        let pools = pools.lock().expect("postgres pool registry poisoned");
        if let Some(pool) = pools.get(&key) {
            return Ok(pool.clone());
        }
    }
    let config: postgres::Config = url.parse().context("invalid TANDEM_STORAGE_POSTGRES_URL")?;
    let manager = PostgresConnectionManager::new(config, postgres::NoTls);
    let pool = r2d2::Pool::builder()
        .max_size(8)
        .min_idle(Some(0))
        .connection_timeout(std::time::Duration::from_secs(15))
        .connection_customizer(Box::new(SchemaCustomizer {
            schema: schema.to_string(),
        }))
        .build(manager)
        .context("failed to build the PostgreSQL stateful storage pool")?;
    ensure_schema_exists(&pool, schema)?;
    let mut pools = pools.lock().expect("postgres pool registry poisoned");
    Ok(pools.entry(key).or_insert(pool).clone())
}

fn ensure_schema_exists(pool: &Pool, schema: &str) -> anyhow::Result<()> {
    let mut client = pool
        .get()
        .context("failed to connect to PostgreSQL for schema creation")?;
    let create = format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\"");
    if let Err(error) = client.batch_execute(&create) {
        // Two engines creating the same schema concurrently can both pass the
        // IF NOT EXISTS check and one loses with a duplicate error; treat the
        // schema now existing as success.
        let exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
                &[&schema],
            )
            .map(|row| row.get(0))
            .unwrap_or(false);
        if !exists {
            return Err(error).context(format!("failed to create PostgreSQL schema {schema}"));
        }
    }
    Ok(())
}

/// PostgreSQL rendering of stateful schema v5. Fresh deployments start at the
/// current version directly — the SQLite v1→v5 migration chain is historical
/// and never existed on PostgreSQL.
pub(crate) fn initialize_schema(connection: &mut Connection) -> anyhow::Result<()> {
    use super::{Executor as _, ExecutorRaw as _};
    connection.execute_batch(POSTGRES_SCHEMA_V5)?;
    let version: i64 = connection.query_row(
        "SELECT schema_version FROM schema_metadata LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    if version != crate::stateful_runtime::orchestration_store::SCHEMA_VERSION {
        bail!(
            "unsupported orchestration store schema version {version}; expected {}",
            crate::stateful_runtime::orchestration_store::SCHEMA_VERSION
        );
    }
    Ok(())
}

const POSTGRES_SCHEMA_V5: &str = "
CREATE TABLE IF NOT EXISTS schema_metadata (
    schema_version BIGINT NOT NULL
);
INSERT INTO schema_metadata (schema_version)
    SELECT 5 WHERE NOT EXISTS (SELECT 1 FROM schema_metadata);

CREATE TABLE IF NOT EXISTS orchestration_specs (
    orchestration_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    deployment_id TEXT,
    deployment_key TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    definition_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    published_at_ms BIGINT,
    PRIMARY KEY (org_id, workspace_id, deployment_key, orchestration_id, version)
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
    is_hot BIGINT NOT NULL DEFAULT 1,
    run_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_automation_runs_scope_status
    ON automation_runs (org_id, workspace_id, status);

CREATE TABLE IF NOT EXISTS long_running_goals (
    goal_id TEXT PRIMARY KEY,
    orchestration_id TEXT NOT NULL,
    orchestration_version BIGINT NOT NULL,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    deployment_id TEXT,
    status TEXT NOT NULL,
    active_run_id TEXT,
    goal_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_goals_scope_status
    ON long_running_goals (org_id, workspace_id, status);

CREATE TABLE IF NOT EXISTS orchestration_tool_requests (
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    deployment_key TEXT NOT NULL DEFAULT '',
    operation TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    response_json TEXT,
    created_at_ms BIGINT NOT NULL,
    completed_at_ms BIGINT,
    PRIMARY KEY (
        org_id, workspace_id, deployment_key, operation, idempotency_key
    )
);

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
    created_at_ms BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
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
    created_at_ms BIGINT NOT NULL,
    imported_at_ms BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS legacy_handoff_quarantine (
    source_path TEXT PRIMARY KEY,
    source_digest TEXT,
    error TEXT NOT NULL,
    quarantined_at_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS goal_run_links (
    goal_id TEXT NOT NULL,
    run_id TEXT NOT NULL UNIQUE,
    orchestration_node_id TEXT NOT NULL,
    orchestration_version BIGINT NOT NULL,
    hop_index BIGINT NOT NULL,
    parent_run_id TEXT,
    triggering_handoff_id TEXT UNIQUE,
    link_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    PRIMARY KEY (goal_id, hop_index)
);

CREATE TABLE IF NOT EXISTS automation_waits (
    wait_id TEXT NOT NULL,
    goal_id TEXT,
    run_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    wait_json TEXT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    PRIMARY KEY (wait_id, run_id, org_id, workspace_id, deployment_id)
);
CREATE INDEX IF NOT EXISTS idx_automation_waits_scope_status
    ON automation_waits (org_id, workspace_id, status);

CREATE TABLE IF NOT EXISTS wait_resolutions (
    wait_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    resolution_json TEXT NOT NULL,
    resolved_at_ms BIGINT NOT NULL,
    PRIMARY KEY (wait_id, idempotency_key)
);

CREATE TABLE IF NOT EXISTS stateful_events (
    event_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    seq BIGINT NOT NULL,
    event_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    deployment_id TEXT,
    rowid BIGSERIAL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_stateful_events_run_seq
    ON stateful_events (run_id, seq);
CREATE UNIQUE INDEX IF NOT EXISTS idx_stateful_events_rowid
    ON stateful_events (rowid);
CREATE INDEX IF NOT EXISTS idx_stateful_events_scope
    ON stateful_events (org_id, workspace_id);
CREATE INDEX IF NOT EXISTS idx_stateful_events_goal_rowid
    ON stateful_events (goal_id, rowid);

CREATE TABLE IF NOT EXISTS goal_projection_blobs (
    digest TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS stateful_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    seq BIGINT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    deployment_id TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_stateful_snapshots_run_seq
    ON stateful_snapshots (run_id, seq);
CREATE INDEX IF NOT EXISTS idx_stateful_snapshots_scope
    ON stateful_snapshots (org_id, workspace_id);

CREATE TABLE IF NOT EXISTS outbox_effects (
    effect_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    status TEXT NOT NULL,
    effect_json TEXT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    deployment_id TEXT,
    rowid BIGSERIAL
);
CREATE INDEX IF NOT EXISTS idx_outbox_effects_scope
    ON outbox_effects (org_id, workspace_id);

CREATE TABLE IF NOT EXISTS dead_letters (
    dead_letter_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    status TEXT NOT NULL,
    record_json TEXT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    deployment_id TEXT,
    rowid BIGSERIAL
);
CREATE INDEX IF NOT EXISTS idx_dead_letters_scope
    ON dead_letters (org_id, workspace_id);

CREATE TABLE IF NOT EXISTS compensations (
    compensation_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    status TEXT NOT NULL,
    record_json TEXT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    org_id TEXT NOT NULL DEFAULT '',
    workspace_id TEXT NOT NULL DEFAULT '',
    deployment_id TEXT,
    rowid BIGSERIAL
);
CREATE INDEX IF NOT EXISTS idx_compensations_scope
    ON compensations (org_id, workspace_id);

CREATE TABLE IF NOT EXISTS tool_effects (
    effect_id TEXT PRIMARY KEY,
    goal_id TEXT,
    run_id TEXT,
    org_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    deployment_id TEXT,
    status TEXT NOT NULL,
    effect_json TEXT NOT NULL,
    updated_at_ms BIGINT NOT NULL,
    rowid BIGSERIAL
);
CREATE INDEX IF NOT EXISTS idx_tool_effects_scope_status
    ON tool_effects (org_id, workspace_id, status);

CREATE TABLE IF NOT EXISTS migration_sources (
    source_kind TEXT NOT NULL,
    source_path TEXT NOT NULL,
    imported_at_ms BIGINT NOT NULL,
    record_count BIGINT NOT NULL,
    PRIMARY KEY (source_kind, source_path)
);

CREATE TABLE IF NOT EXISTS stateful_migrations (
    migration_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    record_count BIGINT NOT NULL,
    started_at_ms BIGINT NOT NULL,
    completed_at_ms BIGINT
);

CREATE TABLE IF NOT EXISTS stateful_migration_attempts (
    attempt_id BIGSERIAL PRIMARY KEY,
    migration_id TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    started_at_ms BIGINT NOT NULL,
    outcome TEXT,
    completed_at_ms BIGINT
);
";

#[cfg(test)]
mod tests {
    use super::translate_sql;

    #[test]
    fn translates_numbered_placeholders() {
        assert_eq!(
            translate_sql("SELECT a FROM t WHERE x = ?1 AND y = ?12"),
            "SELECT a FROM t WHERE x = $1 AND y = $12"
        );
    }

    #[test]
    fn preserves_question_marks_inside_literals() {
        assert_eq!(
            translate_sql("SELECT '?1' FROM t WHERE x = ?2 AND note = 'it''s ?3'"),
            "SELECT '?1' FROM t WHERE x = $2 AND note = 'it''s ?3'"
        );
    }

    #[test]
    fn rewrites_null_safe_is_comparison() {
        assert_eq!(
            translate_sql("WHERE (deployment_id IS ?4 OR deployment_id = ?4)"),
            "WHERE (deployment_id IS NOT DISTINCT FROM $4 OR deployment_id = $4)"
        );
        assert_eq!(
            translate_sql("WHERE deployment_id IS NOT ?2"),
            "WHERE deployment_id IS DISTINCT FROM $2"
        );
    }

    #[test]
    fn leaves_is_null_untouched() {
        assert_eq!(
            translate_sql("WHERE outcome IS NULL AND id = ?1"),
            "WHERE outcome IS NULL AND id = $1"
        );
    }

    #[test]
    fn ignores_words_ending_in_is() {
        assert_eq!(translate_sql("WHERE analysis = ?1"), "WHERE analysis = $1");
    }
}
