//! Neutral execution facade for the stateful orchestration store (TAN-714).
//!
//! The orchestration store's domain logic (idempotency, tenant scoping,
//! encryption, transactional invariants) is backend-independent; only the
//! statement execution layer differs between SQLite and PostgreSQL. This
//! module provides that layer: a deliberately small, `rusqlite`-shaped API
//! (`Connection`/`Transaction`/`Statement`/`Row`, a `params!` macro, and an
//! `OptionalExtension`) so store code reads identically over either backend.
//!
//! Store SQL is authored once in the SQLite dialect (`?N` placeholders,
//! `rowid` cursors, `IS` null-safe comparison); the PostgreSQL backend
//! translates it at execution time (see [`postgres`]). Schema DDL is the one
//! place the dialects genuinely diverge, so each backend owns its own
//! initialization instead of sharing translated DDL.

use std::fmt;

#[cfg(feature = "storage-postgres")]
pub(crate) mod postgres;
#[cfg(feature = "storage-sqlite")]
pub(crate) mod sqlite;

#[cfg(not(any(feature = "storage-sqlite", feature = "storage-postgres")))]
compile_error!(
    "tandem-server requires at least one stateful storage backend: enable the \
     `storage-sqlite` (default) or `storage-postgres` feature"
);

/// Environment variable selecting the stateful store backend
/// (`sqlite` | `postgres`; default `sqlite`).
pub const STORAGE_BACKEND_ENV: &str = "TANDEM_STORAGE_BACKEND";
/// Environment variable carrying the PostgreSQL connection URL when
/// `TANDEM_STORAGE_BACKEND=postgres` (same convention as
/// `TANDEM_MEMORY_POSTGRES_URL` for the memory store).
pub const STORAGE_POSTGRES_URL_ENV: &str = "TANDEM_STORAGE_POSTGRES_URL";

/// Runtime backend selection for the stateful orchestration store.
///
/// Selection is fail-closed: an unrecognized backend name or a missing
/// PostgreSQL URL is a startup error, never a silent fallback to SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageBackendConfig {
    Sqlite,
    Postgres { url: String },
}

impl StorageBackendConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let backend = std::env::var(STORAGE_BACKEND_ENV).unwrap_or_default();
        match backend.trim().to_ascii_lowercase().as_str() {
            "" | "sqlite" => Ok(Self::Sqlite),
            "postgres" | "postgresql" => {
                let url = std::env::var(STORAGE_POSTGRES_URL_ENV)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{STORAGE_BACKEND_ENV}=postgres requires {STORAGE_POSTGRES_URL_ENV} \
                             to be set to a PostgreSQL connection URL"
                        )
                    })?;
                Ok(Self::Postgres { url })
            }
            other => anyhow::bail!(
                "{STORAGE_BACKEND_ENV} has invalid value `{other}`; expected sqlite or postgres"
            ),
        }
    }
}

/// Marker file recording the PostgreSQL schema assigned to a runtime root.
/// Its presence is the Postgres analog of "the SQLite database file exists".
pub(crate) const POSTGRES_SCHEMA_MARKER_FILE: &str = "stateful_runtime.pg_schema";

/// Whether a stateful store has ever been initialized at the runtime root
/// that owns `database_path`, without opening it. SQLite roots are probed by
/// the database file; PostgreSQL roots by the sticky schema marker written on
/// first open. Callers use this to decide when the transactional store is
/// authoritative over legacy sidecar files.
pub(crate) fn store_initialized_hint(database_path: &std::path::Path) -> anyhow::Result<bool> {
    match StorageBackendConfig::from_env()? {
        StorageBackendConfig::Sqlite => Ok(database_path.exists()),
        StorageBackendConfig::Postgres { .. } => Ok(database_path
            .parent()
            .map(|root| root.join(POSTGRES_SCHEMA_MARKER_FILE).exists())
            .unwrap_or(false)),
    }
}

/// A dynamically typed SQL value, mirroring SQLite's storage classes. The
/// PostgreSQL backend maps these onto `BIGINT`/`DOUBLE PRECISION`/`TEXT`/
/// `BYTEA`, which is exactly the column palette the stateful schema uses.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    /// A Rust integer that exceeded the signed 64-bit storage range. Binding
    /// this value fails at execution time (matching rusqlite's `ToSql`
    /// behavior) instead of silently truncating or panicking.
    OutOfRange(String),
}

pub trait ToValue {
    fn to_value(&self) -> Value;
}

impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value {
        (**self).to_value()
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(value) => value.to_value(),
            None => Value::Null,
        }
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value {
        Value::Text(self.to_string())
    }
}

impl ToValue for std::borrow::Cow<'_, str> {
    fn to_value(&self) -> Value {
        Value::Text(self.to_string())
    }
}

impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Integer(i64::from(*self))
    }
}

impl ToValue for f64 {
    fn to_value(&self) -> Value {
        Value::Real(*self)
    }
}

impl ToValue for Vec<u8> {
    fn to_value(&self) -> Value {
        Value::Blob(self.clone())
    }
}

macro_rules! integer_to_value {
    ($($int:ty),+) => {
        $(impl ToValue for $int {
            fn to_value(&self) -> Value {
                match i64::try_from(*self) {
                    Ok(value) => Value::Integer(value),
                    Err(_) => Value::OutOfRange(self.to_string()),
                }
            }
        })+
    };
}

integer_to_value!(i8, i16, i32, i64, u8, u16, u32, u64, usize);

pub trait FromValue: Sized {
    fn from_value(value: &Value) -> Result<Self>;
}

fn conversion_error<T>(expected: &str, value: &Value) -> Result<T> {
    Err(Error::Conversion(format!(
        "expected {expected}, found {value:?}"
    )))
}

impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Text(text) => Ok(text.clone()),
            other => conversion_error("text", other),
        }
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Real(real) => Ok(*real),
            Value::Integer(int) => Ok(*int as f64),
            other => conversion_error("real", other),
        }
    }
}

impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Integer(int) => Ok(*int != 0),
            other => conversion_error("integer (boolean)", other),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Blob(blob) => Ok(blob.clone()),
            other => conversion_error("blob", other),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

macro_rules! integer_from_value {
    ($($int:ty),+) => {
        $(impl FromValue for $int {
            fn from_value(value: &Value) -> Result<Self> {
                match value {
                    Value::Integer(int) => <$int>::try_from(*int).map_err(|_| {
                        Error::Conversion(format!(
                            "integer {int} is out of range for {}",
                            stringify!($int)
                        ))
                    }),
                    other => conversion_error("integer", other),
                }
            }
        })+
    };
}

integer_from_value!(i8, i16, i32, i64, u8, u16, u32, u64, usize);

/// One materialized result row. Rows are decoupled from the underlying
/// backend cursor so mapping closures can be identical across backends.
#[derive(Debug, Clone)]
pub struct Row {
    values: Vec<Value>,
}

impl Row {
    pub(crate) fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Returns the raw row values for storage-internal bulk operations.
    /// Callers outside the stateful storage layer should prefer typed `get`.
    pub(crate) fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn get<I: RowIndex, T: FromValue>(&self, index: I) -> Result<T> {
        let index = index.index();
        let value = self.values.get(index).ok_or_else(|| {
            Error::Conversion(format!(
                "row has {} columns; column {index} does not exist",
                self.values.len()
            ))
        })?;
        T::from_value(value)
    }
}

pub trait RowIndex {
    fn index(&self) -> usize;
}

impl RowIndex for usize {
    fn index(&self) -> usize {
        *self
    }
}

#[derive(Debug)]
pub enum Error {
    /// A query expected to return rows returned none. [`OptionalExtension`]
    /// maps this (and only this) to `Ok(None)`.
    NoRows,
    #[cfg(feature = "storage-sqlite")]
    Sqlite(rusqlite::Error),
    #[cfg(feature = "storage-postgres")]
    Postgres(::postgres::Error),
    /// Parameter or column value could not be represented in the requested
    /// Rust type (or exceeded the storage integer range).
    Conversion(String),
    /// Backend selection, connection, or schema management failure.
    Backend(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoRows => write!(f, "query returned no rows"),
            #[cfg(feature = "storage-sqlite")]
            Error::Sqlite(error) => write!(f, "sqlite backend error: {error}"),
            #[cfg(feature = "storage-postgres")]
            Error::Postgres(error) => write!(f, "postgres backend error: {error}"),
            Error::Conversion(message) => write!(f, "storage value conversion error: {message}"),
            Error::Backend(message) => write!(f, "storage backend error: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "storage-sqlite")]
            Error::Sqlite(error) => Some(error),
            #[cfg(feature = "storage-postgres")]
            Error::Postgres(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(feature = "storage-sqlite")]
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => Error::NoRows,
            other => Error::Sqlite(other),
        }
    }
}

#[cfg(feature = "storage-postgres")]
impl From<::postgres::Error> for Error {
    fn from(error: ::postgres::Error) -> Self {
        Error::Postgres(error)
    }
}

pub trait OptionalExtension<T> {
    /// Maps [`Error::NoRows`] to `Ok(None)`; every other error is preserved.
    fn optional(self) -> Result<Option<T>>;
}

impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionBehavior {
    Deferred,
    Immediate,
}

pub trait IntoParams {
    fn into_params(self) -> Vec<Value>;
}

impl IntoParams for Vec<Value> {
    fn into_params(self) -> Vec<Value> {
        self
    }
}

/// Lets a literal `[]` (no parameters) compile, mirroring rusqlite's
/// dedicated empty-array impl: the trait-object element type is what allows
/// inference to resolve a zero-length array literal.
impl IntoParams for [&(dyn ToValue + Send + Sync); 0] {
    fn into_params(self) -> Vec<Value> {
        Vec::new()
    }
}

macro_rules! array_into_params {
    ($($len:literal),+) => {
        $(impl<T: ToValue> IntoParams for [T; $len] {
            fn into_params(self) -> Vec<Value> {
                self.iter().map(ToValue::to_value).collect()
            }
        })+
    };
}

array_into_params!(1, 2, 3, 4, 5, 6, 7, 8);

macro_rules! params {
    () => {
        Vec::<$crate::stateful_runtime::backend::Value>::new()
    };
    ($($value:expr),+ $(,)?) => {
        vec![$($crate::stateful_runtime::backend::ToValue::to_value(&$value)),+]
    };
}

pub(crate) use params;

fn reject_out_of_range(params: &[Value]) -> Result<()> {
    for value in params {
        if let Value::OutOfRange(original) = value {
            return Err(Error::Conversion(format!(
                "integer parameter {original} is out of the signed 64-bit storage range"
            )));
        }
    }
    Ok(())
}

/// Object-safe execution core. [`Executor`] layers the generic conveniences
/// (closure row mapping, `params!` inputs) on top.
pub trait ExecutorRaw {
    fn execute_raw(&self, sql: &str, params: &[Value]) -> Result<usize>;
    fn query_raw(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>>;
    fn execute_batch(&self, sql: &str) -> Result<()>;
}

pub trait Executor: ExecutorRaw {
    fn execute(&self, sql: &str, params: impl IntoParams) -> Result<usize>
    where
        Self: Sized,
    {
        self.execute_raw(sql, &params.into_params())
    }

    fn query_row<T, P, F>(&self, sql: &str, params: P, mut map: F) -> Result<T>
    where
        P: IntoParams,
        F: FnMut(&Row) -> Result<T>,
        Self: Sized,
    {
        let rows = self.query_raw(sql, &params.into_params())?;
        match rows.first() {
            Some(row) => map(row),
            None => Err(Error::NoRows),
        }
    }

    fn prepare(&self, sql: &str) -> Result<Statement<'_>>
    where
        Self: Sized,
    {
        Ok(Statement {
            executor: self,
            sql: sql.to_string(),
        })
    }
}

impl<E: ExecutorRaw> Executor for E {}

/// A deferred statement: execution happens at `query_map` time. Statements
/// are not cached across calls; per-operation connections make statement
/// caching moot and keeping this stateless keeps both backends trivial.
pub struct Statement<'a> {
    executor: &'a dyn ExecutorRaw,
    sql: String,
}

impl Statement<'_> {
    pub fn query_map<T, P, F>(&mut self, params: P, mut map: F) -> Result<MappedRows<T>>
    where
        P: IntoParams,
        F: FnMut(&Row) -> Result<T>,
    {
        let rows = self.executor.query_raw(&self.sql, &params.into_params())?;
        let mapped: Vec<Result<T>> = rows.iter().map(|row| map(row)).collect();
        Ok(MappedRows {
            inner: mapped.into_iter(),
        })
    }
}

pub struct MappedRows<T> {
    inner: std::vec::IntoIter<Result<T>>,
}

impl<T> Iterator for MappedRows<T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// One open backend connection. Store code receives `&mut Connection` from
/// `OrchestrationStateStore::with_connection` and never constructs these
/// directly.
pub struct Connection {
    inner: ConnectionInner,
}

enum ConnectionInner {
    #[cfg(feature = "storage-sqlite")]
    Sqlite(rusqlite::Connection),
    #[cfg(feature = "storage-postgres")]
    Postgres(postgres::PostgresConnection),
}

impl Connection {
    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn from_sqlite(connection: rusqlite::Connection) -> Self {
        Self {
            inner: ConnectionInner::Sqlite(connection),
        }
    }

    #[cfg(feature = "storage-postgres")]
    pub(crate) fn from_postgres(connection: postgres::PostgresConnection) -> Self {
        Self {
            inner: ConnectionInner::Postgres(connection),
        }
    }

    /// Raw SQLite handle for backend-specific paths (schema initialization,
    /// PRAGMA maintenance, trigger-based fault injection in tests).
    #[cfg(feature = "storage-sqlite")]
    pub(crate) fn sqlite(&mut self) -> Option<&mut rusqlite::Connection> {
        match &mut self.inner {
            ConnectionInner::Sqlite(connection) => Some(connection),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    pub fn is_sqlite(&self) -> bool {
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            ConnectionInner::Sqlite(_) => true,
            #[allow(unreachable_patterns)]
            _ => false,
        }
    }

    pub fn transaction_with_behavior(
        &mut self,
        behavior: TransactionBehavior,
    ) -> Result<Transaction<'_>> {
        match &mut self.inner {
            #[cfg(feature = "storage-sqlite")]
            ConnectionInner::Sqlite(connection) => {
                let behavior = match behavior {
                    TransactionBehavior::Deferred => rusqlite::TransactionBehavior::Deferred,
                    TransactionBehavior::Immediate => rusqlite::TransactionBehavior::Immediate,
                };
                Ok(Transaction {
                    inner: TransactionInner::Sqlite(
                        connection.transaction_with_behavior(behavior)?,
                    ),
                })
            }
            #[cfg(feature = "storage-postgres")]
            ConnectionInner::Postgres(connection) => {
                // `Immediate` carries SQLite's serialized-writer contract;
                // the Postgres backend honors it with a transaction-scoped
                // advisory lock (see `PostgresConnection::begin_transaction`).
                let immediate = matches!(behavior, TransactionBehavior::Immediate);
                Ok(Transaction {
                    inner: TransactionInner::Postgres(std::cell::RefCell::new(
                        connection.begin_transaction(immediate)?,
                    )),
                })
            }
        }
    }
}

impl ExecutorRaw for Connection {
    fn execute_raw(&self, sql: &str, params: &[Value]) -> Result<usize> {
        reject_out_of_range(params)?;
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            ConnectionInner::Sqlite(connection) => sqlite::execute(connection, sql, params),
            #[cfg(feature = "storage-postgres")]
            ConnectionInner::Postgres(connection) => connection.execute(sql, params),
        }
    }

    fn query_raw(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        reject_out_of_range(params)?;
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            ConnectionInner::Sqlite(connection) => sqlite::query(connection, sql, params),
            #[cfg(feature = "storage-postgres")]
            ConnectionInner::Postgres(connection) => connection.query(sql, params),
        }
    }

    fn execute_batch(&self, sql: &str) -> Result<()> {
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            ConnectionInner::Sqlite(connection) => {
                connection.execute_batch(sql).map_err(Error::from)
            }
            #[cfg(feature = "storage-postgres")]
            ConnectionInner::Postgres(connection) => connection.batch_execute(sql),
        }
    }
}

pub struct Transaction<'c> {
    inner: TransactionInner<'c>,
}

enum TransactionInner<'c> {
    #[cfg(feature = "storage-sqlite")]
    Sqlite(rusqlite::Transaction<'c>),
    #[cfg(feature = "storage-postgres")]
    Postgres(std::cell::RefCell<::postgres::Transaction<'c>>),
}

impl Transaction<'_> {
    /// Commits the transaction. Dropping without commit rolls back on both
    /// backends, preserving rusqlite's drop semantics.
    pub fn commit(self) -> Result<()> {
        match self.inner {
            #[cfg(feature = "storage-sqlite")]
            TransactionInner::Sqlite(transaction) => transaction.commit().map_err(Error::from),
            #[cfg(feature = "storage-postgres")]
            TransactionInner::Postgres(transaction) => {
                transaction.into_inner().commit().map_err(Error::from)
            }
        }
    }
}

impl ExecutorRaw for Transaction<'_> {
    fn execute_raw(&self, sql: &str, params: &[Value]) -> Result<usize> {
        reject_out_of_range(params)?;
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            TransactionInner::Sqlite(transaction) => sqlite::execute(transaction, sql, params),
            #[cfg(feature = "storage-postgres")]
            TransactionInner::Postgres(transaction) => {
                postgres::transaction_execute(&mut transaction.borrow_mut(), sql, params)
            }
        }
    }

    fn query_raw(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        reject_out_of_range(params)?;
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            TransactionInner::Sqlite(transaction) => sqlite::query(transaction, sql, params),
            #[cfg(feature = "storage-postgres")]
            TransactionInner::Postgres(transaction) => {
                postgres::transaction_query(&mut transaction.borrow_mut(), sql, params)
            }
        }
    }

    fn execute_batch(&self, sql: &str) -> Result<()> {
        match &self.inner {
            #[cfg(feature = "storage-sqlite")]
            TransactionInner::Sqlite(transaction) => {
                transaction.execute_batch(sql).map_err(Error::from)
            }
            #[cfg(feature = "storage-postgres")]
            TransactionInner::Postgres(transaction) => {
                postgres::transaction_batch(&mut transaction.borrow_mut(), sql)
            }
        }
    }
}
