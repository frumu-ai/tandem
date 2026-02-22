// Database Layer Module
// SQLite + sqlite-vec for vector storage

use crate::types::{
    ClearFileIndexResult, MemoryChunk, MemoryConfig, MemoryResult, MemoryStats, MemoryTier,
    ProjectMemoryStats, DEFAULT_EMBEDDING_DIMENSION,
};
use chrono::{DateTime, Utc};
use rusqlite::{ffi::sqlite3_auto_extension, params, Connection, OptionalExtension, Row};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

type ProjectIndexStatusRow = (
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

/// Database connection manager
pub struct MemoryDatabase {
    conn: Arc<Mutex<Connection>>,
    db_path: std::path::PathBuf,
}

impl MemoryDatabase {
    /// Initialize or open the memory database
    pub async fn new(db_path: &Path) -> MemoryResult<Self> {
        // Register sqlite-vec extension
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(sqlite3_vec_init as *const ())));
        }

        let conn = Connection::open(db_path)?;
        conn.busy_timeout(Duration::from_secs(10))?;

        // Enable WAL mode for better concurrency
        // PRAGMA journal_mode returns a row, so we use query_row to ignore it
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute("PRAGMA synchronous = NORMAL", [])?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: db_path.to_path_buf(),
        };

        // Initialize schema
        db.init_schema().await?;
        if let Err(err) = db.validate_vector_tables().await {
            match &err {
                crate::types::MemoryError::Database(db_err)
                    if Self::is_vector_table_error(db_err) =>
                {
                    tracing::warn!(
                        "Detected vector table corruption during startup ({}). Recreating vector tables.",
                        db_err
                    );
                    db.recreate_vector_tables().await?;
                }
                _ => return Err(err),
            }
        }
        db.validate_integrity().await?;

        Ok(db)
    }

    /// Validate base SQLite integrity early so startup recovery can heal corrupt DB files.
    async fn validate_integrity(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let check = match conn.query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))
        {
            Ok(value) => value,
            Err(err) => {
                // sqlite-vec virtual tables can intermittently return generic SQL logic errors
                // during integrity probing even when runtime reads/writes still work.
                // Do not block startup on this probe failure.
                tracing::warn!(
                    "Skipping strict PRAGMA quick_check due to probe error: {}",
                    err
                );
                return Ok(());
            }
        };
        if check.trim().eq_ignore_ascii_case("ok") {
            return Ok(());
        }

        let lowered = check.to_lowercase();
        if lowered.contains("malformed")
            || lowered.contains("corrupt")
            || lowered.contains("database disk image is malformed")
        {
            return Err(crate::types::MemoryError::InvalidConfig(format!(
                "malformed database integrity check: {}",
                check
            )));
        }

        tracing::warn!(
            "PRAGMA quick_check returned non-ok status but not a hard corruption signal: {}",
            check
        );
        Ok(())
    }

    /// Initialize database schema
    async fn init_schema(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        // Extension is already registered globally in new()

        // Session memory chunks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                session_id TEXT NOT NULL,
                project_id TEXT,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )",
            [],
        )?;

        // Session memory vectors (virtual table)
        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS session_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        // Project memory chunks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS project_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                project_id TEXT NOT NULL,
                session_id TEXT,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )",
            [],
        )?;

        // Migrations: file-derived columns on project_memory_chunks
        // (SQLite doesn't support IF NOT EXISTS for columns, so we inspect table_info)
        let existing_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(project_memory_chunks)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };

        if !existing_cols.contains("source_path") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN source_path TEXT",
                [],
            )?;
        }
        if !existing_cols.contains("source_mtime") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN source_mtime INTEGER",
                [],
            )?;
        }
        if !existing_cols.contains("source_size") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN source_size INTEGER",
                [],
            )?;
        }
        if !existing_cols.contains("source_hash") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN source_hash TEXT",
                [],
            )?;
        }

        // Project memory vectors (virtual table)
        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS project_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        // File indexing tables (project-scoped)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS project_file_index (
                project_id TEXT NOT NULL,
                path TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL,
                hash TEXT NOT NULL,
                indexed_at TEXT NOT NULL,
                PRIMARY KEY(project_id, path)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS project_index_status (
                project_id TEXT PRIMARY KEY,
                last_indexed_at TEXT,
                last_total_files INTEGER,
                last_processed_files INTEGER,
                last_indexed_files INTEGER,
                last_skipped_files INTEGER,
                last_errors INTEGER
            )",
            [],
        )?;

        // Global memory chunks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS global_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )",
            [],
        )?;

        // Global memory vectors (virtual table)
        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS global_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        // Memory configuration table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_config (
                project_id TEXT PRIMARY KEY,
                max_chunks INTEGER NOT NULL DEFAULT 10000,
                chunk_size INTEGER NOT NULL DEFAULT 512,
                retrieval_k INTEGER NOT NULL DEFAULT 5,
                auto_cleanup INTEGER NOT NULL DEFAULT 1,
                session_retention_days INTEGER NOT NULL DEFAULT 30,
                token_budget INTEGER NOT NULL DEFAULT 5000,
                chunk_overlap INTEGER NOT NULL DEFAULT 64,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Cleanup log table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_cleanup_log (
                id TEXT PRIMARY KEY,
                cleanup_type TEXT NOT NULL,
                tier TEXT NOT NULL,
                project_id TEXT,
                session_id TEXT,
                chunks_deleted INTEGER NOT NULL DEFAULT 0,
                bytes_reclaimed INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create indexes for better query performance
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_session ON session_memory_chunks(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_project ON session_memory_chunks(project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_chunks_project ON project_memory_chunks(project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_file_chunks ON project_memory_chunks(project_id, source, source_path)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_created ON session_memory_chunks(created_at)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cleanup_log_created ON memory_cleanup_log(created_at)",
            [],
        )?;

        Ok(())
    }

    /// Validate that sqlite-vec tables are readable.
    /// This catches legacy/corrupted vector blobs early so startup can recover.
    pub async fn validate_vector_tables(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let probe_embedding = format!("[{}]", vec!["0.0"; DEFAULT_EMBEDDING_DIMENSION].join(","));

        for table in [
            "session_memory_vectors",
            "project_memory_vectors",
            "global_memory_vectors",
        ] {
            let sql = format!("SELECT COUNT(*) FROM {}", table);
            let row_count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;

            // COUNT(*) can pass even when vector chunk blobs are unreadable.
            // Probe sqlite-vec MATCH execution to surface latent blob corruption.
            if row_count > 0 {
                let probe_sql = format!(
                    "SELECT chunk_id, distance
                     FROM {}
                     WHERE embedding MATCH ?1 AND k = 1",
                    table
                );
                let mut stmt = conn.prepare(&probe_sql)?;
                let mut rows = stmt.query(params![probe_embedding.as_str()])?;
                let _ = rows.next()?;
            }
        }
        Ok(())
    }

    fn is_vector_table_error(err: &rusqlite::Error) -> bool {
        let text = err.to_string().to_lowercase();
        text.contains("vector blob")
            || text.contains("chunks iter error")
            || text.contains("chunks iter")
            || text.contains("internal sqlite-vec error")
            || text.contains("insert rowids id")
            || text.contains("sql logic error")
            || text.contains("database disk image is malformed")
            || text.contains("session_memory_vectors")
            || text.contains("project_memory_vectors")
            || text.contains("global_memory_vectors")
            || text.contains("vec0")
    }

    async fn recreate_vector_tables(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        for base in [
            "session_memory_vectors",
            "project_memory_vectors",
            "global_memory_vectors",
        ] {
            // Drop vec virtual table and common sqlite-vec shadow tables first.
            for name in [
                base.to_string(),
                format!("{}_chunks", base),
                format!("{}_info", base),
                format!("{}_rowids", base),
                format!("{}_vector_chunks00", base),
            ] {
                let sql = format!("DROP TABLE IF EXISTS \"{}\"", name.replace('"', "\"\""));
                conn.execute(&sql, [])?;
            }

            // Drop any additional shadow tables (e.g. *_vector_chunks01).
            let like_pattern = format!("{base}_%");
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE ?1 ORDER BY name",
            )?;
            let table_names = stmt
                .query_map(params![like_pattern], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for name in table_names {
                let sql = format!("DROP TABLE IF EXISTS \"{}\"", name.replace('"', "\"\""));
                conn.execute(&sql, [])?;
            }
        }

        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS session_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS project_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS global_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )?;

        Ok(())
    }

    /// Ensure vector tables are readable and recreate them if corruption is detected.
    /// Returns true when a repair was performed.
    pub async fn ensure_vector_tables_healthy(&self) -> MemoryResult<bool> {
        match self.validate_vector_tables().await {
            Ok(()) => Ok(false),
            Err(crate::types::MemoryError::Database(err)) if Self::is_vector_table_error(&err) => {
                tracing::warn!(
                    "Memory vector tables appear corrupted ({}). Recreating vector tables.",
                    err
                );
                self.recreate_vector_tables().await?;
                Ok(true)
            }
            Err(err) => Err(err),
        }
    }

    /// Last-resort runtime repair for malformed DB states: drop user memory tables
    /// and recreate the schema in-place so new writes can proceed.
    /// This intentionally clears memory content for the active DB file.
    pub async fn reset_all_memory_tables(&self) -> MemoryResult<()> {
        let table_names = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master
                 WHERE type='table'
                   AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            names
        };

        {
            let conn = self.conn.lock().await;
            for table in table_names {
                let sql = format!("DROP TABLE IF EXISTS \"{}\"", table.replace('"', "\"\""));
                let _ = conn.execute(&sql, []);
            }
        }

        self.init_schema().await
    }

    /// Attempt an immediate vector-table repair when a concrete DB error indicates
    /// sqlite-vec internals are failing at statement/rowid level.
    pub async fn try_repair_after_error(
        &self,
        err: &crate::types::MemoryError,
    ) -> MemoryResult<bool> {
        match err {
            crate::types::MemoryError::Database(db_err) if Self::is_vector_table_error(db_err) => {
                tracing::warn!(
                    "Memory write/read hit vector DB error ({}). Recreating vector tables immediately.",
                    db_err
                );
                self.recreate_vector_tables().await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Store a chunk with its embedding
    pub async fn store_chunk(&self, chunk: &MemoryChunk, embedding: &[f32]) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        let (chunks_table, vectors_table) = match chunk.tier {
            MemoryTier::Session => ("session_memory_chunks", "session_memory_vectors"),
            MemoryTier::Project => ("project_memory_chunks", "project_memory_vectors"),
            MemoryTier::Global => ("global_memory_chunks", "global_memory_vectors"),
        };

        let created_at_str = chunk.created_at.to_rfc3339();
        let metadata_str = chunk
            .metadata
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default();

        // Insert chunk
        match chunk.tier {
            MemoryTier::Session => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (id, content, session_id, project_id, source, created_at, token_count, metadata) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        chunk.content,
                        chunk.session_id.as_ref().unwrap_or(&String::new()),
                        chunk.project_id,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str
                    ],
                )?;
            }
            MemoryTier::Project => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (
                            id, content, project_id, session_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        chunk.content,
                        chunk.project_id.as_ref().unwrap_or(&String::new()),
                        chunk.session_id,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str,
                        chunk.source_path.clone(),
                        chunk.source_mtime,
                        chunk.source_size,
                        chunk.source_hash.clone()
                    ],
                )?;
            }
            MemoryTier::Global => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (id, content, source, created_at, token_count, metadata) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        chunk.content,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str
                    ],
                )?;
            }
        }

        // Insert embedding
        let embedding_json = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        conn.execute(
            &format!(
                "INSERT INTO {} (chunk_id, embedding) VALUES (?1, ?2)",
                vectors_table
            ),
            params![chunk.id, embedding_json],
        )?;

        Ok(())
    }

    /// Search for similar chunks
    pub async fn search_similar(
        &self,
        query_embedding: &[f32],
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> MemoryResult<Vec<(MemoryChunk, f64)>> {
        let conn = self.conn.lock().await;

        let (chunks_table, vectors_table) = match tier {
            MemoryTier::Session => ("session_memory_chunks", "session_memory_vectors"),
            MemoryTier::Project => ("project_memory_chunks", "project_memory_vectors"),
            MemoryTier::Global => ("global_memory_chunks", "global_memory_vectors"),
        };

        let embedding_json = format!(
            "[{}]",
            query_embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        // Build query based on tier and filters
        let results = match tier {
            MemoryTier::Session => {
                if let Some(sid) = session_id {
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                v.distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.session_id = ?1 AND v.embedding MATCH ?2 AND k = ?3
                         ORDER BY v.distance",
                        vectors_table, chunks_table
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(params![sid, embedding_json, limit], |row| {
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(8)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else if let Some(pid) = project_id {
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                v.distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.project_id = ?1 AND v.embedding MATCH ?2 AND k = ?3
                         ORDER BY v.distance",
                        vectors_table, chunks_table
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(params![pid, embedding_json, limit], |row| {
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(8)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else {
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                v.distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE v.embedding MATCH ?1 AND k = ?2
                         ORDER BY v.distance",
                        vectors_table, chunks_table
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(params![embedding_json, limit], |row| {
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(8)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                }
            }
            MemoryTier::Project => {
                if let Some(pid) = project_id {
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                v.distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.project_id = ?1 AND v.embedding MATCH ?2 AND k = ?3
                         ORDER BY v.distance",
                        vectors_table, chunks_table
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(params![pid, embedding_json, limit], |row| {
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(12)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else {
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                v.distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE v.embedding MATCH ?1 AND k = ?2
                         ORDER BY v.distance",
                        vectors_table, chunks_table
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(params![embedding_json, limit], |row| {
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(12)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                }
            }
            MemoryTier::Global => {
                let sql = format!(
                    "SELECT c.id, c.content, NULL as session_id, NULL as project_id, c.source, c.created_at, c.token_count, c.metadata,
                            v.distance
                     FROM {} AS v
                     JOIN {} AS c ON v.chunk_id = c.id
                     WHERE v.embedding MATCH ?1 AND k = ?2
                     ORDER BY v.distance",
                    vectors_table, chunks_table
                );
                let mut stmt = conn.prepare(&sql)?;
                let results = stmt
                    .query_map(params![embedding_json, limit], |row| {
                        Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(8)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                results
            }
        };

        Ok(results)
    }

    /// Get chunks by session ID
    pub async fn get_session_chunks(&self, session_id: &str) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata
             FROM session_memory_chunks
             WHERE session_id = ?1
             ORDER BY created_at DESC",
        )?;

        let chunks = stmt
            .query_map(params![session_id], |row| {
                row_to_chunk(row, MemoryTier::Session)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    /// Get chunks by project ID
    pub async fn get_project_chunks(&self, project_id: &str) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash
             FROM project_memory_chunks
             WHERE project_id = ?1
             ORDER BY created_at DESC",
        )?;

        let chunks = stmt
            .query_map(params![project_id], |row| {
                row_to_chunk(row, MemoryTier::Project)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    /// Get global chunks
    pub async fn get_global_chunks(&self, limit: i64) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, source, created_at, token_count, metadata
             FROM global_memory_chunks
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;

        let chunks = stmt
            .query_map(params![limit], |row| {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let source: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                let token_count: i64 = row.get(4)?;
                let metadata_str: Option<String> = row.get(5)?;

                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);

                let metadata = metadata_str
                    .filter(|s| !s.is_empty())
                    .and_then(|s| serde_json::from_str(&s).ok());

                Ok(MemoryChunk {
                    id,
                    content,
                    tier: MemoryTier::Global,
                    session_id: None,
                    project_id: None,
                    source,
                    source_path: None,
                    source_mtime: None,
                    source_size: None,
                    source_hash: None,
                    created_at,
                    token_count,
                    metadata,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    /// Clear session memory
    pub async fn clear_session_memory(&self, session_id: &str) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        // Get count before deletion
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_memory_chunks WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;

        // Delete vectors first (foreign key constraint)
        conn.execute(
            "DELETE FROM session_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM session_memory_chunks WHERE session_id = ?1)",
            params![session_id],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM session_memory_chunks WHERE session_id = ?1",
            params![session_id],
        )?;

        Ok(count as u64)
    }

    /// Clear project memory
    pub async fn clear_project_memory(&self, project_id: &str) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        // Get count before deletion
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        // Delete vectors first
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM project_memory_chunks WHERE project_id = ?1)",
            params![project_id],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
        )?;

        Ok(count as u64)
    }

    /// Clear old session memory based on retention policy
    pub async fn cleanup_old_sessions(&self, retention_days: i64) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        let cutoff = Utc::now() - chrono::Duration::days(retention_days);
        let cutoff_str = cutoff.to_rfc3339();

        // Get count before deletion
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_memory_chunks WHERE created_at < ?1",
            params![cutoff_str],
            |row| row.get(0),
        )?;

        // Delete vectors first
        conn.execute(
            "DELETE FROM session_memory_vectors WHERE chunk_id IN 
             (SELECT id FROM session_memory_chunks WHERE created_at < ?1)",
            params![cutoff_str],
        )?;

        // Delete chunks
        conn.execute(
            "DELETE FROM session_memory_chunks WHERE created_at < ?1",
            params![cutoff_str],
        )?;

        Ok(count as u64)
    }

    /// Get or create memory config for a project
    pub async fn get_or_create_config(&self, project_id: &str) -> MemoryResult<MemoryConfig> {
        let conn = self.conn.lock().await;

        let result: Option<MemoryConfig> = conn
            .query_row(
                "SELECT max_chunks, chunk_size, retrieval_k, auto_cleanup, 
                        session_retention_days, token_budget, chunk_overlap
                 FROM memory_config WHERE project_id = ?1",
                params![project_id],
                |row| {
                    Ok(MemoryConfig {
                        max_chunks: row.get(0)?,
                        chunk_size: row.get(1)?,
                        retrieval_k: row.get(2)?,
                        auto_cleanup: row.get::<_, i64>(3)? != 0,
                        session_retention_days: row.get(4)?,
                        token_budget: row.get(5)?,
                        chunk_overlap: row.get(6)?,
                    })
                },
            )
            .optional()?;

        match result {
            Some(config) => Ok(config),
            None => {
                // Create default config
                let config = MemoryConfig::default();
                let updated_at = Utc::now().to_rfc3339();

                conn.execute(
                    "INSERT INTO memory_config 
                     (project_id, max_chunks, chunk_size, retrieval_k, auto_cleanup, 
                      session_retention_days, token_budget, chunk_overlap, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        project_id,
                        config.max_chunks,
                        config.chunk_size,
                        config.retrieval_k,
                        config.auto_cleanup as i64,
                        config.session_retention_days,
                        config.token_budget,
                        config.chunk_overlap,
                        updated_at
                    ],
                )?;

                Ok(config)
            }
        }
    }

    /// Update memory config for a project
    pub async fn update_config(&self, project_id: &str, config: &MemoryConfig) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        let updated_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO memory_config 
             (project_id, max_chunks, chunk_size, retrieval_k, auto_cleanup, 
              session_retention_days, token_budget, chunk_overlap, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                project_id,
                config.max_chunks,
                config.chunk_size,
                config.retrieval_k,
                config.auto_cleanup as i64,
                config.session_retention_days,
                config.token_budget,
                config.chunk_overlap,
                updated_at
            ],
        )?;

        Ok(())
    }

    /// Get memory statistics
    pub async fn get_stats(&self) -> MemoryResult<MemoryStats> {
        let conn = self.conn.lock().await;

        // Count chunks
        let session_chunks: i64 =
            conn.query_row("SELECT COUNT(*) FROM session_memory_chunks", [], |row| {
                row.get(0)
            })?;

        let project_chunks: i64 =
            conn.query_row("SELECT COUNT(*) FROM project_memory_chunks", [], |row| {
                row.get(0)
            })?;

        let global_chunks: i64 =
            conn.query_row("SELECT COUNT(*) FROM global_memory_chunks", [], |row| {
                row.get(0)
            })?;

        // Calculate sizes
        let session_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM session_memory_chunks",
            [],
            |row| row.get(0),
        )?;

        let project_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks",
            [],
            |row| row.get(0),
        )?;

        let global_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM global_memory_chunks",
            [],
            |row| row.get(0),
        )?;

        // Get last cleanup
        let last_cleanup: Option<String> = conn
            .query_row(
                "SELECT created_at FROM memory_cleanup_log ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        let last_cleanup = last_cleanup.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        // Get file size
        let file_size = std::fs::metadata(&self.db_path)?.len() as i64;

        Ok(MemoryStats {
            total_chunks: session_chunks + project_chunks + global_chunks,
            session_chunks,
            project_chunks,
            global_chunks,
            total_bytes: session_bytes + project_bytes + global_bytes,
            session_bytes,
            project_bytes,
            global_bytes,
            file_size,
            last_cleanup,
        })
    }

    /// Log cleanup operation
    pub async fn log_cleanup(
        &self,
        cleanup_type: &str,
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        chunks_deleted: i64,
        bytes_reclaimed: i64,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO memory_cleanup_log 
             (id, cleanup_type, tier, project_id, session_id, chunks_deleted, bytes_reclaimed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                cleanup_type,
                tier.to_string(),
                project_id,
                session_id,
                chunks_deleted,
                bytes_reclaimed,
                created_at
            ],
        )?;

        Ok(())
    }

    /// Vacuum the database to reclaim space
    pub async fn vacuum(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute("VACUUM", [])?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Project file indexing helpers
    // ---------------------------------------------------------------------

    pub async fn project_file_index_count(&self, project_id: &str) -> MemoryResult<i64> {
        let conn = self.conn.lock().await;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_file_index WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    pub async fn project_has_file_chunks(&self, project_id: &str) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' LIMIT 1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    pub async fn get_file_index_entry(
        &self,
        project_id: &str,
        path: &str,
    ) -> MemoryResult<Option<(i64, i64, String)>> {
        let conn = self.conn.lock().await;
        let row: Option<(i64, i64, String)> = conn
            .query_row(
                "SELECT mtime, size, hash FROM project_file_index WHERE project_id = ?1 AND path = ?2",
                params![project_id, path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub async fn upsert_file_index_entry(
        &self,
        project_id: &str,
        path: &str,
        mtime: i64,
        size: i64,
        hash: &str,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let indexed_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_file_index (project_id, path, mtime, size, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(project_id, path) DO UPDATE SET
                mtime = excluded.mtime,
                size = excluded.size,
                hash = excluded.hash,
                indexed_at = excluded.indexed_at",
            params![project_id, path, mtime, size, hash, indexed_at],
        )?;
        Ok(())
    }

    pub async fn delete_file_index_entry(&self, project_id: &str, path: &str) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM project_file_index WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
        )?;
        Ok(())
    }

    pub async fn list_file_index_paths(&self, project_id: &str) -> MemoryResult<Vec<String>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT path FROM project_file_index WHERE project_id = ?1")?;
        let rows = stmt.query_map(params![project_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub async fn delete_project_file_chunks_by_path(
        &self,
        project_id: &str,
        source_path: &str,
    ) -> MemoryResult<(i64, i64)> {
        let conn = self.conn.lock().await;

        let chunks_deleted: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks
             WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
            |row| row.get(0),
        )?;

        let bytes_estimated: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks
             WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
            |row| row.get(0),
        )?;

        // Delete vectors first (keep order consistent with other clears)
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN
             (SELECT id FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' AND source_path = ?2)",
            params![project_id, source_path],
        )?;

        conn.execute(
            "DELETE FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file' AND source_path = ?2",
            params![project_id, source_path],
        )?;

        Ok((chunks_deleted, bytes_estimated))
    }

    pub async fn upsert_project_index_status(
        &self,
        project_id: &str,
        total_files: i64,
        processed_files: i64,
        indexed_files: i64,
        skipped_files: i64,
        errors: i64,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        let last_indexed_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_index_status (
                project_id, last_indexed_at, last_total_files, last_processed_files,
                last_indexed_files, last_skipped_files, last_errors
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(project_id) DO UPDATE SET
                last_indexed_at = excluded.last_indexed_at,
                last_total_files = excluded.last_total_files,
                last_processed_files = excluded.last_processed_files,
                last_indexed_files = excluded.last_indexed_files,
                last_skipped_files = excluded.last_skipped_files,
                last_errors = excluded.last_errors",
            params![
                project_id,
                last_indexed_at,
                total_files,
                processed_files,
                indexed_files,
                skipped_files,
                errors
            ],
        )?;
        Ok(())
    }

    pub async fn get_project_stats(&self, project_id: &str) -> MemoryResult<ProjectMemoryStats> {
        let conn = self.conn.lock().await;

        let project_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let project_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let file_index_chunks: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let file_index_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let indexed_files: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_file_index WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;

        let status_row: Option<ProjectIndexStatusRow> =
            conn
                .query_row(
                    "SELECT last_indexed_at, last_total_files, last_processed_files, last_indexed_files, last_skipped_files, last_errors
                     FROM project_index_status WHERE project_id = ?1",
                    params![project_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .optional()?;

        let (
            last_indexed_at,
            last_total_files,
            last_processed_files,
            last_indexed_files,
            last_skipped_files,
            last_errors,
        ) = status_row.unwrap_or((None, None, None, None, None, None));

        let last_indexed_at = last_indexed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });

        Ok(ProjectMemoryStats {
            project_id: project_id.to_string(),
            project_chunks,
            project_bytes,
            file_index_chunks,
            file_index_bytes,
            indexed_files,
            last_indexed_at,
            last_total_files,
            last_processed_files,
            last_indexed_files,
            last_skipped_files,
            last_errors,
        })
    }

    pub async fn clear_project_file_index(
        &self,
        project_id: &str,
        vacuum: bool,
    ) -> MemoryResult<ClearFileIndexResult> {
        let conn = self.conn.lock().await;

        let chunks_deleted: i64 = conn.query_row(
            "SELECT COUNT(*) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        let bytes_estimated: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
            |row| row.get(0),
        )?;

        // Delete vectors first
        conn.execute(
            "DELETE FROM project_memory_vectors WHERE chunk_id IN
             (SELECT id FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file')",
            params![project_id],
        )?;

        // Delete file chunks
        conn.execute(
            "DELETE FROM project_memory_chunks WHERE project_id = ?1 AND source = 'file'",
            params![project_id],
        )?;

        // Clear file index tracking + status
        conn.execute(
            "DELETE FROM project_file_index WHERE project_id = ?1",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM project_index_status WHERE project_id = ?1",
            params![project_id],
        )?;

        drop(conn); // release lock before VACUUM (which needs exclusive access)

        if vacuum {
            self.vacuum().await?;
        }

        Ok(ClearFileIndexResult {
            chunks_deleted,
            bytes_estimated,
            did_vacuum: vacuum,
        })
    }

    // ------------------------------------------------------------------
    // Memory hygiene
    // ------------------------------------------------------------------

    /// Delete session memory chunks older than `retention_days` days.
    ///
    /// Also removes orphaned vector entries for the deleted chunks so the
    /// sqlite-vec virtual table stays consistent.
    ///
    /// Returns the number of chunk rows deleted.
    /// If `retention_days` is 0 hygiene is disabled and this returns Ok(0).
    pub async fn prune_old_session_chunks(&self, retention_days: u32) -> MemoryResult<u64> {
        if retention_days == 0 {
            return Ok(0);
        }

        let conn = self.conn.lock().await;

        // WAL is already active (set in new()) — no need to set it again here.
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days))).to_rfc3339();

        // Remove orphaned vector entries first (chunk_id FK would dangle otherwise)
        conn.execute(
            "DELETE FROM session_memory_vectors
             WHERE chunk_id IN (
                 SELECT id FROM session_memory_chunks WHERE created_at < ?1
             )",
            params![cutoff],
        )?;

        let deleted = conn.execute(
            "DELETE FROM session_memory_chunks WHERE created_at < ?1",
            params![cutoff],
        )?;

        if deleted > 0 {
            tracing::info!(
                retention_days,
                deleted,
                "memory hygiene: pruned old session chunks"
            );
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(deleted as u64)
    }

    /// Run scheduled hygiene: read `session_retention_days` from `memory_config`
    /// (falling back to `env_override` if provided) and prune stale session chunks.
    ///
    /// Returns `Ok(chunks_deleted)`. This method is intentionally best-effort —
    /// callers should log errors and continue.
    pub async fn run_hygiene(&self, env_override_days: u32) -> MemoryResult<u64> {
        // Prefer the env override, fall back to the DB config for the null project.
        let retention_days = if env_override_days > 0 {
            env_override_days
        } else {
            // Try to read the global (project_id = '__global__') config if present.
            let conn = self.conn.lock().await;
            let days: Option<i64> = conn
                .query_row(
                    "SELECT session_retention_days FROM memory_config
                     WHERE project_id = '__global__' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            drop(conn);
            days.unwrap_or(30) as u32
        };

        self.prune_old_session_chunks(retention_days).await
    }
}

/// Convert a database row to a MemoryChunk
fn row_to_chunk(row: &Row, tier: MemoryTier) -> Result<MemoryChunk, rusqlite::Error> {
    let id: String = row.get(0)?;
    let content: String = row.get(1)?;

    let session_id: Option<String> = match tier {
        MemoryTier::Session => Some(row.get(2)?),
        MemoryTier::Project => row.get(2)?,
        MemoryTier::Global => None,
    };

    let project_id: Option<String> = match tier {
        MemoryTier::Session => row.get(3)?,
        MemoryTier::Project => Some(row.get(3)?),
        MemoryTier::Global => None,
    };

    let source: String = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let token_count: i64 = row.get(6)?;
    let metadata_str: Option<String> = row.get(7)?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);

    let metadata = metadata_str
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok());

    let source_path = row.get::<_, Option<String>>("source_path").ok().flatten();
    let source_mtime = row.get::<_, Option<i64>>("source_mtime").ok().flatten();
    let source_size = row.get::<_, Option<i64>>("source_size").ok().flatten();
    let source_hash = row.get::<_, Option<String>>("source_hash").ok().flatten();

    Ok(MemoryChunk {
        id,
        content,
        tier,
        session_id,
        project_id,
        source,
        source_path,
        source_mtime,
        source_size,
        source_hash,
        created_at,
        token_count,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_db() -> (MemoryDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let db = MemoryDatabase::new(&db_path).await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_init_schema() {
        let (db, _temp) = setup_test_db().await;
        // If we get here, schema was initialized successfully
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.total_chunks, 0);
    }

    #[tokio::test]
    async fn test_store_and_retrieve_chunk() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "test-1".to_string(),
            content: "Test content".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("session-1".to_string()),
            project_id: Some("project-1".to_string()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            created_at: Utc::now(),
            token_count: 10,
            metadata: None,
        };

        let embedding = vec![0.1f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        let chunks = db.get_session_chunks("session-1").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Test content");
    }

    #[tokio::test]
    async fn test_config_crud() {
        let (db, _temp) = setup_test_db().await;

        let config = db.get_or_create_config("project-1").await.unwrap();
        assert_eq!(config.max_chunks, 10000);

        let new_config = MemoryConfig {
            max_chunks: 5000,
            ..Default::default()
        };
        db.update_config("project-1", &new_config).await.unwrap();

        let updated = db.get_or_create_config("project-1").await.unwrap();
        assert_eq!(updated.max_chunks, 5000);
    }
}
