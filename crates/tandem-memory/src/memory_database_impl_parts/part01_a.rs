impl MemoryDatabase {
    /// Override the memory payload crypto provider (used to select an explicit
    /// local-encrypted/hosted provider or in tests). Defaults to env resolution.
    pub fn with_crypto_provider(mut self, crypto: crate::crypto::MemoryCryptoProvider) -> Self {
        self.crypto = crypto;
        self
    }

    /// Override strict tenant enforcement for this instance (instances inherit
    /// the process default from `set_strict_tenant_enforcement_default`).
    pub fn set_strict_tenant_enforcement(&self, enabled: bool) {
        self.strict_tenant_enforcement
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    /// In hosted/enterprise (strict) mode the local-implicit scope must never
    /// reach the store: it would silently read or write the shared "local"
    /// partition instead of an explicit tenant partition.
    fn deny_local_scope_in_strict_mode(
        &self,
        operation: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        if tenant_scope.is_local()
            && self
                .strict_tenant_enforcement
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            tracing::warn!(
                operation = operation,
                "memory access denied: local-implicit tenant scope reached a strict-mode store"
            );
            return Err(MemoryError::TenantScopeViolation(format!(
                "{operation} denied: local-implicit tenant scope is not permitted in hosted/enterprise mode"
            )));
        }
        Ok(())
    }

    /// Initialize or open the memory database
    pub async fn new(db_path: &Path) -> MemoryResult<Self> {
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

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
            crypto: crate::crypto::MemoryCryptoProvider::from_env(),
            strict_tenant_enforcement: std::sync::atomic::AtomicBool::new(
                crate::db::strict_tenant_enforcement_default(),
            ),
        };

        let _schema_init_guard = SCHEMA_INIT_LOCK.lock().await;

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
        let mut conn = self.conn.lock().await;

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
                metadata TEXT,
                owner_org_unit_id TEXT,
                tenant_shared INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        let session_existing_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(session_memory_chunks)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !session_existing_cols.contains("source_path") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN source_path TEXT",
                [],
            )?;
        }
        if !session_existing_cols.contains("source_mtime") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN source_mtime INTEGER",
                [],
            )?;
        }
        if !session_existing_cols.contains("source_size") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN source_size INTEGER",
                [],
            )?;
        }
        if !session_existing_cols.contains("source_hash") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN source_hash TEXT",
                [],
            )?;
        }
        if !session_existing_cols.contains("tenant_org_id") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN tenant_org_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !session_existing_cols.contains("tenant_workspace_id") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN tenant_workspace_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !session_existing_cols.contains("tenant_deployment_id") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN tenant_deployment_id TEXT",
                [],
            )?;
        }
        if !session_existing_cols.contains("subject") {
            conn.execute(
                "ALTER TABLE session_memory_chunks ADD COLUMN subject TEXT",
                [],
            )?;
        }
        self.ensure_chunk_scope_columns(&conn, "session_memory_chunks", &session_existing_cols)?;
        conn.execute(
            "UPDATE session_memory_chunks SET tenant_org_id = 'local' WHERE tenant_org_id IS NULL OR tenant_org_id = ''",
            [],
        )?;
        conn.execute(
            "UPDATE session_memory_chunks SET tenant_workspace_id = 'local' WHERE tenant_workspace_id IS NULL OR tenant_workspace_id = ''",
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
                metadata TEXT,
                owner_org_unit_id TEXT,
                tenant_shared INTEGER NOT NULL DEFAULT 0
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
        if !existing_cols.contains("tenant_org_id") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN tenant_org_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !existing_cols.contains("tenant_workspace_id") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN tenant_workspace_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !existing_cols.contains("tenant_deployment_id") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN tenant_deployment_id TEXT",
                [],
            )?;
        }
        if !existing_cols.contains("subject") {
            conn.execute(
                "ALTER TABLE project_memory_chunks ADD COLUMN subject TEXT",
                [],
            )?;
        }
        self.ensure_chunk_scope_columns(&conn, "project_memory_chunks", &existing_cols)?;
        conn.execute(
            "UPDATE project_memory_chunks SET tenant_org_id = 'local' WHERE tenant_org_id IS NULL OR tenant_org_id = ''",
            [],
        )?;
        conn.execute(
            "UPDATE project_memory_chunks SET tenant_workspace_id = 'local' WHERE tenant_workspace_id IS NULL OR tenant_workspace_id = ''",
            [],
        )?;

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
        let project_file_index_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(project_file_index)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !project_file_index_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE project_file_index_new (
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    project_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    mtime INTEGER NOT NULL,
                    size INTEGER NOT NULL,
                    hash TEXT NOT NULL,
                    indexed_at TEXT NOT NULL,
                    PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id, path)
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO project_file_index_new
                 (tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id, path, mtime, size, hash, indexed_at)
                 SELECT 'local', 'local', '', project_id, path, mtime, size, hash, indexed_at
                 FROM project_file_index",
                [],
            )?;
            conn.execute("DROP TABLE project_file_index", [])?;
            conn.execute(
                "ALTER TABLE project_file_index_new RENAME TO project_file_index",
                [],
            )?;
        }
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_file_index (
                session_id TEXT NOT NULL,
                path TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL,
                hash TEXT NOT NULL,
                indexed_at TEXT NOT NULL,
                PRIMARY KEY(session_id, path)
            )",
            [],
        )?;
        let session_file_index_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(session_file_index)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !session_file_index_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE session_file_index_new (
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    session_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    mtime INTEGER NOT NULL,
                    size INTEGER NOT NULL,
                    hash TEXT NOT NULL,
                    indexed_at TEXT NOT NULL,
                    PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, session_id, path)
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO session_file_index_new
                 (tenant_org_id, tenant_workspace_id, tenant_deployment_id, session_id, path, mtime, size, hash, indexed_at)
                 SELECT 'local', 'local', '', session_id, path, mtime, size, hash, indexed_at
                 FROM session_file_index",
                [],
            )?;
            conn.execute("DROP TABLE session_file_index", [])?;
            conn.execute(
                "ALTER TABLE session_file_index_new RENAME TO session_file_index",
                [],
            )?;
        }

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
        let project_index_status_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(project_index_status)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !project_index_status_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE project_index_status_new (
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    project_id TEXT NOT NULL,
                    last_indexed_at TEXT,
                    last_total_files INTEGER,
                    last_processed_files INTEGER,
                    last_indexed_files INTEGER,
                    last_skipped_files INTEGER,
                    last_errors INTEGER,
                    PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id)
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO project_index_status_new
                 (tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id, last_indexed_at, last_total_files, last_processed_files, last_indexed_files, last_skipped_files, last_errors)
                 SELECT 'local', 'local', '', project_id, last_indexed_at, last_total_files, last_processed_files, last_indexed_files, last_skipped_files, last_errors
                 FROM project_index_status",
                [],
            )?;
            conn.execute("DROP TABLE project_index_status", [])?;
            conn.execute(
                "ALTER TABLE project_index_status_new RENAME TO project_index_status",
                [],
            )?;
        }

        // Global memory chunks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS global_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT,
                owner_org_unit_id TEXT,
                tenant_shared INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        let global_existing_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(global_memory_chunks)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !global_existing_cols.contains("source_path") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN source_path TEXT",
                [],
            )?;
        }
        if !global_existing_cols.contains("source_mtime") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN source_mtime INTEGER",
                [],
            )?;
        }
        if !global_existing_cols.contains("source_size") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN source_size INTEGER",
                [],
            )?;
        }
        if !global_existing_cols.contains("source_hash") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN source_hash TEXT",
                [],
            )?;
        }
        if !global_existing_cols.contains("tenant_org_id") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN tenant_org_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !global_existing_cols.contains("tenant_workspace_id") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN tenant_workspace_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !global_existing_cols.contains("tenant_deployment_id") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN tenant_deployment_id TEXT",
                [],
            )?;
        }
        if !global_existing_cols.contains("subject") {
            conn.execute(
                "ALTER TABLE global_memory_chunks ADD COLUMN subject TEXT",
                [],
            )?;
        }
        self.ensure_chunk_scope_columns(&conn, "global_memory_chunks", &global_existing_cols)?;
        conn.execute(
            "UPDATE global_memory_chunks SET tenant_org_id = 'local' WHERE tenant_org_id IS NULL OR tenant_org_id = ''",
            [],
        )?;
        conn.execute(
            "UPDATE global_memory_chunks SET tenant_workspace_id = 'local' WHERE tenant_workspace_id IS NULL OR tenant_workspace_id = ''",
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
        let memory_config_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_config)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !memory_config_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE memory_config_new (
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    project_id TEXT NOT NULL,
                    max_chunks INTEGER NOT NULL DEFAULT 10000,
                    chunk_size INTEGER NOT NULL DEFAULT 512,
                    retrieval_k INTEGER NOT NULL DEFAULT 5,
                    auto_cleanup INTEGER NOT NULL DEFAULT 1,
                    session_retention_days INTEGER NOT NULL DEFAULT 30,
                    token_budget INTEGER NOT NULL DEFAULT 5000,
                    chunk_overlap INTEGER NOT NULL DEFAULT 64,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id)
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO memory_config_new
                 (tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id,
                  max_chunks, chunk_size, retrieval_k, auto_cleanup, session_retention_days,
                  token_budget, chunk_overlap, updated_at)
                 SELECT 'local', 'local', '', project_id, max_chunks, chunk_size, retrieval_k,
                        auto_cleanup, session_retention_days, token_budget, chunk_overlap, updated_at
                 FROM memory_config",
                [],
            )?;
            conn.execute("DROP TABLE memory_config", [])?;
            conn.execute("ALTER TABLE memory_config_new RENAME TO memory_config", [])?;
        }
        // Retention columns (added after the tenant rebuild so both fresh and
        // legacy tables converge on the same shape).
        if !memory_config_cols.contains("exchange_retention_days") {
            conn.execute(
                "ALTER TABLE memory_config ADD COLUMN exchange_retention_days INTEGER NOT NULL DEFAULT 365",
                [],
            )?;
        }
        if !memory_config_cols.contains("global_retention_days") {
            conn.execute(
                "ALTER TABLE memory_config ADD COLUMN global_retention_days INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_config_tenant_project
                ON memory_config(tenant_org_id, tenant_workspace_id, tenant_deployment_id, project_id)",
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
        let cleanup_log_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_cleanup_log)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !cleanup_log_cols.contains("tenant_org_id") {
            conn.execute(
                "ALTER TABLE memory_cleanup_log ADD COLUMN tenant_org_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !cleanup_log_cols.contains("tenant_workspace_id") {
            conn.execute(
                "ALTER TABLE memory_cleanup_log ADD COLUMN tenant_workspace_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !cleanup_log_cols.contains("tenant_deployment_id") {
            conn.execute(
                "ALTER TABLE memory_cleanup_log ADD COLUMN tenant_deployment_id TEXT",
                [],
            )?;
        }
        conn.execute(
            "UPDATE memory_cleanup_log SET tenant_org_id = 'local' WHERE tenant_org_id IS NULL OR tenant_org_id = ''",
            [],
        )?;
        conn.execute(
            "UPDATE memory_cleanup_log SET tenant_workspace_id = 'local' WHERE tenant_workspace_id IS NULL OR tenant_workspace_id = ''",
            [],
        )?;

        // Create indexes for better query performance
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_session ON session_memory_chunks(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_tenant_session ON session_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_tenant_org_unit_session ON session_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), owner_org_unit_id, session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_chunks_project ON session_memory_chunks(project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_file_chunks ON session_memory_chunks(session_id, source, source_path)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_chunks_project ON project_memory_chunks(project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_chunks_tenant_project ON project_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_chunks_tenant_org_unit_project ON project_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), owner_org_unit_id, project_id)",
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
            "CREATE INDEX IF NOT EXISTS idx_global_file_chunks ON global_memory_chunks(source, source_path)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_global_chunks_tenant_created ON global_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), created_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_global_chunks_tenant_org_unit_created ON global_memory_chunks(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), owner_org_unit_id, created_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cleanup_log_created ON memory_cleanup_log(created_at)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cleanup_log_tenant_created ON memory_cleanup_log(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), created_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS global_file_index (
                path TEXT PRIMARY KEY,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL,
                hash TEXT NOT NULL,
                indexed_at TEXT NOT NULL
            )",
            [],
        )?;
        let global_file_index_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(global_file_index)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !global_file_index_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE global_file_index_new (
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    path TEXT NOT NULL,
                    mtime INTEGER NOT NULL,
                    size INTEGER NOT NULL,
                    hash TEXT NOT NULL,
                    indexed_at TEXT NOT NULL,
                    PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, path)
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO global_file_index_new
                 (tenant_org_id, tenant_workspace_id, tenant_deployment_id, path, mtime, size, hash, indexed_at)
                 SELECT 'local', 'local', '', path, mtime, size, hash, indexed_at
                 FROM global_file_index",
                [],
            )?;
            conn.execute("DROP TABLE global_file_index", [])?;
            conn.execute(
                "ALTER TABLE global_file_index_new RENAME TO global_file_index",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_file_index_tenant_project ON project_file_index(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), project_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_file_index_tenant_session ON session_file_index(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_index_status_tenant_project ON project_index_status(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), project_id)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS source_object_lifecycle (
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
                tenant_deployment_id TEXT NOT NULL DEFAULT '',
                source_object_id TEXT NOT NULL,
                source_binding_id TEXT NOT NULL,
                connector_id TEXT NOT NULL,
                state TEXT NOT NULL,
                tier TEXT NOT NULL,
                session_id TEXT,
                project_id TEXT,
                import_namespace TEXT NOT NULL,
                indexed_path TEXT NOT NULL,
                native_object_id TEXT NOT NULL,
                resource_ref TEXT NOT NULL,
                data_class TEXT NOT NULL,
                content_hash TEXT,
                source_hash TEXT,
                first_seen_at_ms INTEGER NOT NULL,
                last_seen_at_ms INTEGER NOT NULL,
                tombstoned_at_ms INTEGER,
                metadata TEXT,
                PRIMARY KEY(tenant_org_id, tenant_workspace_id, tenant_deployment_id, source_object_id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_source_object_lifecycle_binding
             ON source_object_lifecycle(tenant_org_id, tenant_workspace_id, tenant_deployment_id, source_binding_id, state)",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_source_object_lifecycle_native
             ON source_object_lifecycle(tenant_org_id, tenant_workspace_id, tenant_deployment_id, source_binding_id, native_object_id)",
            [],
        )?;

        // Knowledge registry tables (scoped reusable knowledge, separate from raw memory)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS knowledge_spaces (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                project_id TEXT,
                namespace TEXT,
                title TEXT,
                description TEXT,
                trust_level TEXT NOT NULL,
                metadata TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "DROP INDEX IF EXISTS idx_knowledge_spaces_scope_project_namespace",
            [],
        )?;
        let knowledge_space_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(knowledge_spaces)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !knowledge_space_cols.contains("tenant_org_id") {
            conn.execute(
                "CREATE TABLE knowledge_spaces_new (
                    id TEXT PRIMARY KEY,
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT NOT NULL DEFAULT '',
                    scope TEXT NOT NULL,
                    project_id TEXT,
                    namespace TEXT,
                    title TEXT,
                    description TEXT,
                    trust_level TEXT NOT NULL,
                    metadata TEXT,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO knowledge_spaces_new
                 (id, tenant_org_id, tenant_workspace_id, tenant_deployment_id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms)
                 SELECT id, 'local', 'local', '', scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces",
                [],
            )?;
            conn.execute("DROP TABLE knowledge_spaces", [])?;
            conn.execute(
                "ALTER TABLE knowledge_spaces_new RENAME TO knowledge_spaces",
                [],
            )?;
        }
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_spaces_tenant_scope_project_namespace
                ON knowledge_spaces(tenant_org_id, tenant_workspace_id, tenant_deployment_id, scope, IFNULL(project_id, ''), IFNULL(namespace, ''))",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_spaces_tenant_project_updated
                ON knowledge_spaces(tenant_org_id, tenant_workspace_id, tenant_deployment_id, IFNULL(project_id, ''), updated_at_ms DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS knowledge_items (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                coverage_key TEXT NOT NULL,
                dedupe_key TEXT NOT NULL,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT,
                payload TEXT NOT NULL,
                trust_level TEXT NOT NULL,
                status TEXT NOT NULL,
                run_id TEXT,
                artifact_refs TEXT NOT NULL,
                source_memory_ids TEXT NOT NULL,
                freshness_expires_at_ms INTEGER,
                metadata TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                FOREIGN KEY(space_id) REFERENCES knowledge_spaces(id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_items_space_dedupe
                ON knowledge_items(space_id, dedupe_key)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_items_space_coverage
                ON knowledge_items(space_id, coverage_key)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_items_space_created
                ON knowledge_items(space_id, created_at_ms DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS knowledge_coverage (
                coverage_key TEXT NOT NULL,
                space_id TEXT NOT NULL,
                latest_item_id TEXT,
                latest_dedupe_key TEXT,
                last_seen_at_ms INTEGER NOT NULL,
                last_promoted_at_ms INTEGER,
                freshness_expires_at_ms INTEGER,
                metadata TEXT,
                PRIMARY KEY(coverage_key, space_id),
                FOREIGN KEY(space_id) REFERENCES knowledge_spaces(id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_coverage_space_seen
                ON knowledge_coverage(space_id, last_seen_at_ms DESC)",
            [],
        )?;

        // Global user memory records (FTS-backed baseline retrieval path)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_records (
                id TEXT PRIMARY KEY,
                tenant_org_id TEXT NOT NULL DEFAULT 'local',
                tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                tenant_deployment_id TEXT,
                user_id TEXT NOT NULL,
                source_type TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                run_id TEXT NOT NULL,
                session_id TEXT,
                message_id TEXT,
                tool_name TEXT,
                project_tag TEXT,
                channel_tag TEXT,
                host_tag TEXT,
                metadata TEXT,
                provenance TEXT,
                redaction_status TEXT NOT NULL,
                redaction_count INTEGER NOT NULL DEFAULT 0,
                visibility TEXT NOT NULL DEFAULT 'private',
                demoted INTEGER NOT NULL DEFAULT 0,
                score_boost REAL NOT NULL DEFAULT 0.0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                expires_at_ms INTEGER,
                owner_org_unit_id TEXT
            )",
            [],
        )?;
        let memory_record_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_records)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !memory_record_cols.contains("tenant_org_id") {
            conn.execute(
                "ALTER TABLE memory_records ADD COLUMN tenant_org_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !memory_record_cols.contains("tenant_workspace_id") {
            conn.execute(
                "ALTER TABLE memory_records ADD COLUMN tenant_workspace_id TEXT NOT NULL DEFAULT 'local'",
                [],
            )?;
        }
        if !memory_record_cols.contains("tenant_deployment_id") {
            conn.execute(
                "ALTER TABLE memory_records ADD COLUMN tenant_deployment_id TEXT",
                [],
            )?;
        }
        // Department (org-unit) ownership as a first-class, indexed scope column
        // (TAN-645). Promotes what was previously only a JSON metadata key
        // (`OWNER_ORG_UNIT_METADATA_KEY`) post-filtered in Rust into a real column
        // enforced by a SQL predicate mirroring the tenant clause. NULL = tenant-wide
        // (the pre-org-unit behavior); a department-scoped read excludes NULL rows
        // (fail-closed, TAN-647).
        if !memory_record_cols.contains("owner_org_unit_id") {
            conn.execute(
                "ALTER TABLE memory_records ADD COLUMN owner_org_unit_id TEXT",
                [],
            )?;
        }
        conn.execute(
            "UPDATE memory_records
             SET tenant_org_id = 'local'
             WHERE tenant_org_id IS NULL OR tenant_org_id = ''",
            [],
        )?;
        conn.execute(
            "UPDATE memory_records
             SET tenant_workspace_id = 'local'
             WHERE tenant_workspace_id IS NULL OR tenant_workspace_id = ''",
            [],
        )?;
        // Backfill the new column from the legacy metadata key so rows written
        // before TAN-645 remain department-filterable once callers scope reads.
        // Normalize with TRIM + NULLIF to match owner_org_unit_id_from_metadata
        // (which trims and drops empties), so a backfilled `" finance "` compares
        // equal to a freshly-written `"finance"` under the exact-match predicate.
        conn.execute(
            "UPDATE memory_records
             SET owner_org_unit_id =
                 NULLIF(TRIM(json_extract(metadata, '$.owner_org_unit_id')), '')
             WHERE owner_org_unit_id IS NULL
               AND metadata IS NOT NULL
               AND metadata <> ''
               AND json_valid(metadata)
               AND NULLIF(TRIM(json_extract(metadata, '$.owner_org_unit_id')), '') IS NOT NULL",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_records_user_created
                ON memory_records(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), user_id, created_at_ms DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_records_run
                ON memory_records(run_id)",
            [],
        )?;
        // Supports the department-scoped read predicate (TAN-645): tenant + org-unit
        // + user, most-recent-first, mirroring idx_memory_records_user_created.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_records_org_unit
                ON memory_records(tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''), owner_org_unit_id, user_id, created_at_ms DESC)",
            [],
        )?;
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_records_fts USING fts5(
                id UNINDEXED,
                user_id UNINDEXED,
                content
            )",
            [],
        )?;
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memory_records_ai AFTER INSERT ON memory_records BEGIN
                INSERT INTO memory_records_fts(id, user_id, content) VALUES (new.id, new.user_id, new.content);
            END",
            [],
        )?;
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memory_records_ad AFTER DELETE ON memory_records BEGIN
                DELETE FROM memory_records_fts WHERE id = old.id;
            END",
            [],
        )?;
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memory_records_au AFTER UPDATE OF content, user_id ON memory_records BEGIN
                DELETE FROM memory_records_fts WHERE id = old.id;
                INSERT INTO memory_records_fts(id, user_id, content) VALUES (new.id, new.user_id, new.content);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_nodes (
                id TEXT PRIMARY KEY,
                uri TEXT NOT NULL,
                parent_uri TEXT,
                node_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT,
                tenant_org_id TEXT NOT NULL DEFAULT 'local',
                tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                tenant_deployment_id TEXT
            )",
            [],
        )?;
        // Legacy memory_nodes tables predate tenant scoping and carried a global
        // UNIQUE(uri) constraint, which both leaks across tenants and prevents two
        // tenants from owning the same context URI. SQLite cannot drop an inline
        // UNIQUE constraint, so rebuild the table once (FK enforcement is off, and
        // renaming the new table re-links memory_layers' textual FK reference).
        let nodes_existing_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_nodes)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !nodes_existing_cols.contains("tenant_org_id") {
            conn.execute_batch(
                "CREATE TABLE memory_nodes_tenant_migration (
                    id TEXT PRIMARY KEY,
                    uri TEXT NOT NULL,
                    parent_uri TEXT,
                    node_type TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    metadata TEXT,
                    tenant_org_id TEXT NOT NULL DEFAULT 'local',
                    tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
                    tenant_deployment_id TEXT
                );
                INSERT INTO memory_nodes_tenant_migration
                    (id, uri, parent_uri, node_type, created_at, updated_at, metadata,
                     tenant_org_id, tenant_workspace_id, tenant_deployment_id)
                    SELECT id, uri, parent_uri, node_type, created_at, updated_at, metadata,
                           'local', 'local', NULL
                    FROM memory_nodes;
                DROP TABLE memory_nodes;
                ALTER TABLE memory_nodes_tenant_migration RENAME TO memory_nodes;",
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_nodes_uri ON memory_nodes(uri)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_nodes_parent ON memory_nodes(parent_uri)",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_nodes_uri_tenant
             ON memory_nodes(uri, tenant_org_id, tenant_workspace_id, IFNULL(tenant_deployment_id, ''))",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_layers (
                id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                layer_type TEXT NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                embedding_id TEXT,
                created_at TEXT NOT NULL,
                source_chunk_id TEXT,
                crypto_envelope TEXT,
                FOREIGN KEY (node_id) REFERENCES memory_nodes(id)
            )",
            [],
        )?;
        // Per-scope envelope for hosted-KMS encryption (TAN-668) on existing DBs.
        let layer_existing_cols: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_layers)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        if !layer_existing_cols.contains("crypto_envelope") {
            conn.execute(
                "ALTER TABLE memory_layers ADD COLUMN crypto_envelope TEXT",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_layers_node ON memory_layers(node_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_layers_type ON memory_layers(layer_type)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_retrieval_state (
                node_id TEXT PRIMARY KEY,
                active_layer TEXT NOT NULL DEFAULT 'L0',
                last_accessed TEXT,
                access_count INTEGER DEFAULT 0,
                FOREIGN KEY (node_id) REFERENCES memory_nodes(id)
            )",
            [],
        )?;

        // Versions 1-4 are still owned by the bootstrap above. All new schema
        // changes are translated by the pending-version coordinator, which
        // records a version only in the transaction that applies it.
        crate::migrations::run_sqlite_migrations(&mut conn)?;

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

    fn is_vector_table_error(err: &str) -> bool {
        let text = err.to_lowercase();
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
        let _schema_init_guard = SCHEMA_INIT_LOCK.lock().await;
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

    /// Seal a row's `content` + optional `metadata` under the row's at-rest key
    /// scope (TAN-668). Returns the stored content, stored metadata (empty when
    /// there is none), and the serialized `crypto_envelope` to persist — `None` in
    /// local/plaintext modes (a no-op there, so single-tenant storage is
    /// unchanged). In hosted mode both fields share one DEK/envelope.
    fn seal_row_columns(
        &self,
        content: &str,
        metadata_plain: &str,
        key_scope: &crate::envelope::MemoryKeyScope,
    ) -> MemoryResult<(String, String, Option<String>)> {
        let (policy_decision_id, audit_id) = memory_write_authorization_ids();
        let (mut ciphertexts, envelope) = if metadata_plain.is_empty() {
            self.crypto
                .encrypt_row_scoped(&[content], key_scope, &policy_decision_id, &audit_id)?
        } else {
            self.crypto.encrypt_row_scoped(
                &[content, metadata_plain],
                key_scope,
                &policy_decision_id,
                &audit_id,
            )?
        };
        let content_stored = ciphertexts.remove(0);
        let metadata_stored = if metadata_plain.is_empty() {
            String::new()
        } else {
            ciphertexts.remove(0)
        };
        let crypto_envelope = envelope
            .map(|envelope| serde_json::to_string(&envelope))
            .transpose()?;
        Ok((content_stored, metadata_stored, crypto_envelope))
    }

    /// Store a chunk with its embedding
    pub async fn store_chunk(&self, chunk: &MemoryChunk, embedding: &[f32]) -> MemoryResult<()> {
        self.deny_local_scope_in_strict_mode("memory store", &chunk.tenant_scope)?;
        let conn = self.conn.lock().await;

        let (chunks_table, vectors_table) = match chunk.tier {
            MemoryTier::Session => ("session_memory_chunks", "session_memory_vectors"),
            MemoryTier::Project => ("project_memory_chunks", "project_memory_vectors"),
            MemoryTier::Global => ("global_memory_chunks", "global_memory_vectors"),
        };

        let created_at_str = chunk.created_at.to_rfc3339();
        // Department (owner_org_unit_id) and tenant_shared persist as first-class
        // scope columns (TAN-645/647).
        let owner_org_unit_id = owner_org_unit_id_from_metadata(chunk.metadata.as_ref());
        let tenant_shared = tenant_shared_from_metadata(chunk.metadata.as_ref());
        let owner_subject = chunk
            .subject
            .as_deref()
            .map(str::trim)
            .filter(|subject| !subject.is_empty());
        let private = owner_subject.is_some();
        // Encrypt semantic payloads at rest under the row's key scope (no-op in
        // local plaintext mode; per-scope KMS envelope in hosted mode). This
        // supersedes the unscoped encrypt_field for content and metadata.
        let metadata_plain = chunk
            .metadata
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default();
        let key_scope =
            crate::types::memory_key_scope_from_metadata(&chunk.tenant_scope, chunk.metadata.as_ref());
        let (content_stored, metadata_str, crypto_envelope) =
            self.seal_row_columns(&chunk.content, &metadata_plain, &key_scope)?;

        // Insert chunk
        match chunk.tier {
            MemoryTier::Session => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (
                            id, content, session_id, project_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, owner_org_unit_id, tenant_shared, private, owner_subject, crypto_envelope
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        content_stored,
                        chunk.session_id.as_ref().unwrap_or(&String::new()),
                        chunk.project_id,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str,
                        chunk.source_path.clone(),
                        chunk.source_mtime,
                        chunk.source_size,
                        chunk.source_hash.clone(),
                        chunk.tenant_scope.org_id.as_str(),
                        chunk.tenant_scope.workspace_id.as_str(),
                        chunk.tenant_scope.deployment_id.as_deref(),
                        chunk.subject.as_deref(),
                        owner_org_unit_id.as_deref(),
                        i64::from(tenant_shared),
                        i64::from(private),
                        owner_subject,
                        crypto_envelope
                    ],
                )?;
            }
            MemoryTier::Project => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (
                            id, content, project_id, session_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, owner_org_unit_id, tenant_shared, private, owner_subject, crypto_envelope
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        content_stored,
                        chunk.project_id.as_ref().unwrap_or(&String::new()),
                        chunk.session_id,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str,
                        chunk.source_path.clone(),
                        chunk.source_mtime,
                        chunk.source_size,
                        chunk.source_hash.clone(),
                        chunk.tenant_scope.org_id.as_str(),
                        chunk.tenant_scope.workspace_id.as_str(),
                        chunk.tenant_scope.deployment_id.as_deref(),
                        chunk.subject.as_deref(),
                        owner_org_unit_id.as_deref(),
                        i64::from(tenant_shared),
                        i64::from(private),
                        owner_subject,
                        crypto_envelope
                    ],
                )?;
            }
            MemoryTier::Global => {
                conn.execute(
                    &format!(
                        "INSERT INTO {} (
                            id, content, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash,
                            tenant_org_id, tenant_workspace_id, tenant_deployment_id, subject, owner_org_unit_id, tenant_shared, private, owner_subject, crypto_envelope
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        content_stored,
                        chunk.source,
                        created_at_str,
                        chunk.token_count,
                        metadata_str,
                        chunk.source_path.clone(),
                        chunk.source_mtime,
                        chunk.source_size,
                        chunk.source_hash.clone(),
                        chunk.tenant_scope.org_id.as_str(),
                        chunk.tenant_scope.workspace_id.as_str(),
                        chunk.tenant_scope.deployment_id.as_deref(),
                        chunk.subject.as_deref(),
                        owner_org_unit_id.as_deref(),
                        i64::from(tenant_shared),
                        i64::from(private),
                        owner_subject,
                        crypto_envelope
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
        self.search_similar_for_tenant(
            query_embedding,
            tier,
            project_id,
            session_id,
            &MemoryTenantScope::local(),
            limit,
            None,
            None,
        )
        .await
    }

    /// Search for similar chunks within a tenant partition.
    ///
    /// This uses sqlite-vec distance functions over rows already filtered by
    /// tenant in the chunk table, avoiding global top-k results that could let
    /// another tenant's closer vectors suppress this tenant's candidates.
    pub async fn search_similar_for_tenant(
        &self,
        query_embedding: &[f32],
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        limit: i64,
        visible_subject: Option<&str>,
        owner_org_unit_id: Option<&str>,
    ) -> MemoryResult<Vec<(MemoryChunk, f64)>> {
        self.deny_local_scope_in_strict_mode("memory search", tenant_scope)?;
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

        // Build query based on tier and filters. These are exact per-tenant
        // top-k scans rather than global ANN followed by post-filtering.
        let results = match tier {
            MemoryTier::Session => {
                if let Some(sid) = session_id {
                    let tenant_clause = tenant_scope_matches_sql_clause("c", 2);
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                                vec_distance_cosine(v.embedding, ?5) AS distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.session_id = ?1 AND {}
                           AND (c.private = 0 OR c.owner_subject = ?7)
                           AND (?8 IS NULL OR c.owner_org_unit_id = ?8 OR c.tenant_shared = 1)
                         ORDER BY distance
                         LIMIT ?6",
                        vectors_table, chunks_table, tenant_clause
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(
                            params![
                                sid,
                                tenant_scope.org_id.as_str(),
                                tenant_scope.workspace_id.as_str(),
                                tenant_scope.deployment_id.as_deref(),
                                embedding_json,
                                limit,
                                visible_subject,
                                owner_org_unit_id
                            ],
                            |row| {
                                Ok((
                                    row_to_chunk(row, tier, &self.crypto)?,
                                    row.get::<_, f64>("distance")?,
                                ))
                            },
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else if let Some(pid) = project_id {
                    let tenant_clause = tenant_scope_matches_sql_clause("c", 2);
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                                vec_distance_cosine(v.embedding, ?5) AS distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.project_id = ?1 AND {}
                           AND (c.private = 0 OR c.owner_subject = ?7)
                           AND (?8 IS NULL OR c.owner_org_unit_id = ?8 OR c.tenant_shared = 1)
                         ORDER BY distance
                         LIMIT ?6",
                        vectors_table, chunks_table, tenant_clause
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(
                            params![
                                pid,
                                tenant_scope.org_id.as_str(),
                                tenant_scope.workspace_id.as_str(),
                                tenant_scope.deployment_id.as_deref(),
                                embedding_json,
                                limit,
                                visible_subject,
                                owner_org_unit_id
                            ],
                            |row| {
                                Ok((
                                    row_to_chunk(row, tier, &self.crypto)?,
                                    row.get::<_, f64>("distance")?,
                                ))
                            },
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else {
                    let tenant_clause = tenant_scope_matches_sql_clause("c", 1);
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                                vec_distance_cosine(v.embedding, ?4) AS distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE {}
                           AND (c.private = 0 OR c.owner_subject = ?6)
                           AND (?7 IS NULL OR c.owner_org_unit_id = ?7 OR c.tenant_shared = 1)
                         ORDER BY distance
                         LIMIT ?5",
                        vectors_table, chunks_table, tenant_clause
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(
                            params![
                                tenant_scope.org_id.as_str(),
                                tenant_scope.workspace_id.as_str(),
                                tenant_scope.deployment_id.as_deref(),
                                embedding_json,
                                limit,
                                visible_subject,
                                owner_org_unit_id
                            ],
                            |row| {
                                Ok((
                                    row_to_chunk(row, tier, &self.crypto)?,
                                    row.get::<_, f64>("distance")?,
                                ))
                            },
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                }
            }
            MemoryTier::Project => {
                if let Some(pid) = project_id {
                    let tenant_clause = tenant_scope_matches_sql_clause("c", 2);
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                                vec_distance_cosine(v.embedding, ?5) AS distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE c.project_id = ?1 AND {}
                           AND (c.private = 0 OR c.owner_subject = ?7)
                           AND (?8 IS NULL OR c.owner_org_unit_id = ?8 OR c.tenant_shared = 1)
                         ORDER BY distance
                         LIMIT ?6",
                        vectors_table, chunks_table, tenant_clause
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(
                            params![
                                pid,
                                tenant_scope.org_id.as_str(),
                                tenant_scope.workspace_id.as_str(),
                                tenant_scope.deployment_id.as_deref(),
                                embedding_json,
                                limit,
                                visible_subject,
                                owner_org_unit_id
                            ],
                            |row| {
                                Ok((
                                    row_to_chunk(row, tier, &self.crypto)?,
                                    row.get::<_, f64>("distance")?,
                                ))
                            },
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else {
                    let tenant_clause = tenant_scope_matches_sql_clause("c", 1);
                    let sql = format!(
                        "SELECT c.id, c.content, c.session_id, c.project_id, c.source, c.created_at, c.token_count, c.metadata,
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
                                c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                                vec_distance_cosine(v.embedding, ?4) AS distance
                         FROM {} AS v
                         JOIN {} AS c ON v.chunk_id = c.id
                         WHERE {}
                           AND (c.private = 0 OR c.owner_subject = ?6)
                           AND (?7 IS NULL OR c.owner_org_unit_id = ?7 OR c.tenant_shared = 1)
                         ORDER BY distance
                         LIMIT ?5",
                        vectors_table, chunks_table, tenant_clause
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let results = stmt
                        .query_map(
                            params![
                                tenant_scope.org_id.as_str(),
                                tenant_scope.workspace_id.as_str(),
                                tenant_scope.deployment_id.as_deref(),
                                embedding_json,
                                limit,
                                visible_subject,
                                owner_org_unit_id
                            ],
                            |row| {
                                Ok((
                                    row_to_chunk(row, tier, &self.crypto)?,
                                    row.get::<_, f64>("distance")?,
                                ))
                            },
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                }
            }
            MemoryTier::Global => {
                let tenant_clause = tenant_scope_matches_sql_clause("c", 1);
                let sql = format!(
                    "SELECT c.id, c.content, c.source, c.created_at, c.token_count, c.metadata,
                            c.source_path, c.source_mtime, c.source_size, c.source_hash,
                            c.tenant_org_id, c.tenant_workspace_id, c.tenant_deployment_id, c.subject, c.crypto_envelope,
                            vec_distance_cosine(v.embedding, ?4) AS distance
                     FROM {} AS v
                     JOIN {} AS c ON v.chunk_id = c.id
                     WHERE {}
                       AND (c.private = 0 OR c.owner_subject = ?6)
                       AND (?7 IS NULL OR c.owner_org_unit_id = ?7 OR c.tenant_shared = 1)
                     ORDER BY distance
                     LIMIT ?5",
                    vectors_table, chunks_table, tenant_clause
                );
                let mut stmt = conn.prepare(&sql)?;
                let results = stmt
                    .query_map(
                        params![
                            tenant_scope.org_id.as_str(),
                            tenant_scope.workspace_id.as_str(),
                            tenant_scope.deployment_id.as_deref(),
                            embedding_json,
                            limit,
                            visible_subject,
                            owner_org_unit_id
                        ],
                        |row| {
                            Ok((
                                row_to_chunk(row, tier, &self.crypto)?,
                                row.get::<_, f64>("distance")?,
                            ))
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                results
            }
        };

        Ok(results)
    }
}
