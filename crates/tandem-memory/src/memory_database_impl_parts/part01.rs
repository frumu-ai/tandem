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
            "CREATE INDEX IF NOT EXISTS idx_session_file_chunks ON session_memory_chunks(session_id, source, source_path)",
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
            "CREATE INDEX IF NOT EXISTS idx_global_file_chunks ON global_memory_chunks(source, source_path)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cleanup_log_created ON memory_cleanup_log(created_at)",
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
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_spaces_scope_project_namespace
                ON knowledge_spaces(scope, IFNULL(project_id, ''), IFNULL(namespace, ''))",
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
                expires_at_ms INTEGER
            )",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_records_dedup
                ON memory_records(user_id, source_type, content_hash, run_id, IFNULL(session_id, ''), IFNULL(message_id, ''), IFNULL(tool_name, ''))",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_records_user_created
                ON memory_records(user_id, created_at_ms DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_records_run
                ON memory_records(run_id)",
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
                uri TEXT NOT NULL UNIQUE,
                parent_uri TEXT,
                node_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_nodes_uri ON memory_nodes(uri)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_nodes_parent ON memory_nodes(parent_uri)",
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
                FOREIGN KEY (node_id) REFERENCES memory_nodes(id)
            )",
            [],
        )?;
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
                        "INSERT INTO {} (
                            id, content, session_id, project_id, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                        metadata_str,
                        chunk.source_path.clone(),
                        chunk.source_mtime,
                        chunk.source_size,
                        chunk.source_hash.clone()
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
                        "INSERT INTO {} (
                            id, content, source, created_at, token_count, metadata,
                            source_path, source_mtime, source_size, source_hash
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        chunks_table
                    ),
                    params![
                        chunk.id,
                        chunk.content,
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
                                c.source_path, c.source_mtime, c.source_size, c.source_hash,
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
                            Ok((row_to_chunk(row, tier)?, row.get::<_, f64>(12)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    results
                } else if let Some(pid) = project_id {
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
        };

        Ok(results)
    }

    /// Get chunks by session ID
    pub async fn get_session_chunks(&self, session_id: &str) -> MemoryResult<Vec<MemoryChunk>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT id, content, session_id, project_id, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash
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
            "SELECT id, content, source, created_at, token_count, metadata,
                    source_path, source_mtime, source_size, source_hash
             FROM global_memory_chunks
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;

        let chunks = stmt
            .query_map(params![limit], |row| row_to_chunk(row, MemoryTier::Global))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(chunks)
    }

    pub async fn global_chunk_exists_by_source_hash(
        &self,
        source_hash: &str,
    ) -> MemoryResult<bool> {
        let conn = self.conn.lock().await;
        let exists = conn
            .query_row(
                "SELECT 1 FROM global_memory_chunks WHERE source_hash = ?1 LIMIT 1",
                params![source_hash],
                |_row| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(exists)
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

    /// Clear global memory chunks by source prefix (and matching vectors).
    pub async fn clear_global_memory_by_source_prefix(
        &self,
        source_prefix: &str,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;
        let like = format!("{}%", source_prefix);

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM global_memory_chunks WHERE source LIKE ?1",
            params![like],
            |row| row.get(0),
        )?;

        conn.execute(
            "DELETE FROM global_memory_vectors WHERE chunk_id IN
             (SELECT id FROM global_memory_chunks WHERE source LIKE ?1)",
            params![like],
        )?;

        conn.execute(
            "DELETE FROM global_memory_chunks WHERE source LIKE ?1",
            params![like],
        )?;

        Ok(count as u64)
    }

    /// Delete a single memory chunk by id within the requested scope.
    pub async fn delete_chunk(
        &self,
        tier: MemoryTier,
        chunk_id: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
    ) -> MemoryResult<u64> {
        let conn = self.conn.lock().await;

        let deleted = match tier {
            MemoryTier::Session => {
                let Some(session_id) = session_id else {
                    return Err(MemoryError::InvalidConfig(
                        "session_id is required to delete session memory chunks".to_string(),
                    ));
                };
                conn.execute(
                    "DELETE FROM session_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM session_memory_chunks WHERE id = ?1 AND session_id = ?2)",
                    params![chunk_id, session_id],
                )?;
                conn.execute(
                    "DELETE FROM session_memory_chunks WHERE id = ?1 AND session_id = ?2",
                    params![chunk_id, session_id],
                )?
            }
            MemoryTier::Project => {
                let Some(project_id) = project_id else {
                    return Err(MemoryError::InvalidConfig(
                        "project_id is required to delete project memory chunks".to_string(),
                    ));
                };
                conn.execute(
                    "DELETE FROM project_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM project_memory_chunks WHERE id = ?1 AND project_id = ?2)",
                    params![chunk_id, project_id],
                )?;
                conn.execute(
                    "DELETE FROM project_memory_chunks WHERE id = ?1 AND project_id = ?2",
                    params![chunk_id, project_id],
                )?
            }
            MemoryTier::Global => {
                conn.execute(
                    "DELETE FROM global_memory_vectors WHERE chunk_id IN
                     (SELECT id FROM global_memory_chunks WHERE id = ?1)",
                    params![chunk_id],
                )?;
                conn.execute(
                    "DELETE FROM global_memory_chunks WHERE id = ?1",
                    params![chunk_id],
                )?
            }
        };

        Ok(deleted as u64)
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

    /// Insert or update a reusable knowledge space.
    pub async fn upsert_knowledge_space(&self, space: &KnowledgeSpaceRecord) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_spaces
             (id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                space.id,
                space.scope.to_string(),
                space.project_id,
                space.namespace,
                space.title,
                space.description,
                space.trust_level.to_string(),
                space.metadata.as_ref().map(|value| value.to_string()),
                space.created_at_ms as i64,
                space.updated_at_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// Fetch a knowledge space by ID.
    pub async fn get_knowledge_space(
        &self,
        id: &str,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces WHERE id = ?1",
                params![id],
                row_to_knowledge_space,
            )
            .optional()?,
        )
    }

    /// List knowledge spaces, optionally filtered by project.
    pub async fn list_knowledge_spaces(
        &self,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = if project_id.is_some() {
            conn.prepare(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces WHERE project_id = ?1 ORDER BY updated_at_ms DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, scope, project_id, namespace, title, description, trust_level, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_spaces ORDER BY updated_at_ms DESC",
            )?
        };
        let rows = if let Some(project_id) = project_id {
            stmt.query_map(params![project_id], row_to_knowledge_space)?
        } else {
            stmt.query_map([], row_to_knowledge_space)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Insert or update a reusable knowledge item.
    pub async fn upsert_knowledge_item(&self, item: &KnowledgeItemRecord) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_items
             (id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                item.id,
                item.space_id,
                item.coverage_key,
                item.dedupe_key,
                item.item_type,
                item.title,
                item.summary,
                item.payload.to_string(),
                item.trust_level.to_string(),
                item.status.to_string(),
                item.run_id,
                serde_json::to_string(&item.artifact_refs)?,
                serde_json::to_string(&item.source_memory_ids)?,
                item.freshness_expires_at_ms.map(|value| value as i64),
                item.metadata.as_ref().map(|value| value.to_string()),
                item.created_at_ms as i64,
                item.updated_at_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// List knowledge items for a knowledge space.
    pub async fn list_knowledge_items(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = if coverage_key.is_some() {
            conn.prepare(
                "SELECT id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_items WHERE space_id = ?1 AND coverage_key = ?2 ORDER BY created_at_ms DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_items WHERE space_id = ?1 ORDER BY created_at_ms DESC",
            )?
        };
        let rows = if let Some(coverage_key) = coverage_key {
            stmt.query_map(params![space_id, coverage_key], row_to_knowledge_item)?
        } else {
            stmt.query_map(params![space_id], row_to_knowledge_item)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Fetch a knowledge item by ID.
    pub async fn get_knowledge_item(&self, id: &str) -> MemoryResult<Option<KnowledgeItemRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_items WHERE id = ?1",
                params![id],
                row_to_knowledge_item,
            )
            .optional()?,
        )
    }

    /// Promote or retire a knowledge item and update its coverage record atomically.
    pub async fn promote_knowledge_item(
        &self,
        request: &KnowledgePromotionRequest,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction()?;

        let Some(mut item) = tx
            .query_row(
                "SELECT id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms
                 FROM knowledge_items WHERE id = ?1",
                params![request.item_id],
                row_to_knowledge_item,
            )
            .optional()? else {
            return Ok(None);
        };

        let previous_status = item.status;
        let previous_trust_level = item.trust_level;

        if previous_status == KnowledgeItemStatus::Deprecated
            && request.target_status != KnowledgeItemStatus::Deprecated
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "cannot promote a deprecated knowledge item".to_string(),
            ));
        }

        let next_status = request.target_status;
        match (previous_status, next_status) {
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::Promoted)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Promoted)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::ApprovedDefault)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::ApprovedDefault)
            | (KnowledgeItemStatus::Working, KnowledgeItemStatus::Deprecated)
            | (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Deprecated)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Deprecated) => {}
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::ApprovedDefault) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "approved_default requires an intermediate promoted item".to_string(),
                ));
            }
            (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Promoted) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "approved_default items do not downgrade back to promoted".to_string(),
                ));
            }
            (KnowledgeItemStatus::Promoted, KnowledgeItemStatus::Working)
            | (KnowledgeItemStatus::ApprovedDefault, KnowledgeItemStatus::Working) => {
                return Err(crate::types::MemoryError::InvalidConfig(
                    "knowledge items cannot be demoted back to working".to_string(),
                ));
            }
            (KnowledgeItemStatus::Working, KnowledgeItemStatus::Working) => {}
            (KnowledgeItemStatus::Deprecated, _) => {}
        }

        if next_status == KnowledgeItemStatus::ApprovedDefault
            && (request.reviewer_id.is_none() || request.approval_id.is_none())
        {
            return Err(crate::types::MemoryError::InvalidConfig(
                "approved_default promotion requires reviewer_id and approval_id".to_string(),
            ));
        }

        let promoted = next_status.is_active()
            && (previous_status != next_status || request.freshness_expires_at_ms.is_some());

        let mut metadata_obj = item
            .metadata
            .clone()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        metadata_obj.insert(
            "promotion".to_string(),
            serde_json::json!({
                "from_status": previous_status.to_string(),
                "to_status": next_status.to_string(),
                "promoted_at_ms": request.promoted_at_ms,
                "reason": request.reason,
                "reviewer_id": request.reviewer_id,
                "approval_id": request.approval_id,
                "freshness_expires_at_ms": request.freshness_expires_at_ms,
            }),
        );

        item.status = next_status;
        if let Some(next_trust) = next_status.as_trust_level() {
            item.trust_level = next_trust;
        }
        if let Some(freshness_expires_at_ms) = request.freshness_expires_at_ms {
            item.freshness_expires_at_ms = Some(freshness_expires_at_ms);
        }
        item.metadata = Some(serde_json::Value::Object(metadata_obj));
        item.updated_at_ms = request.promoted_at_ms;
        let persisted_item = item.clone();
        let item_id = persisted_item.id.clone();
        let space_id = persisted_item.space_id.clone();
        let coverage_key = persisted_item.coverage_key.clone();
        let dedupe_key = persisted_item.dedupe_key.clone();

        tx.execute(
            "INSERT OR REPLACE INTO knowledge_items
             (id, space_id, coverage_key, dedupe_key, item_type, title, summary, payload, trust_level, status, run_id, artifact_refs, source_memory_ids, freshness_expires_at_ms, metadata, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                persisted_item.id,
                persisted_item.space_id,
                persisted_item.coverage_key,
                persisted_item.dedupe_key,
                persisted_item.item_type,
                persisted_item.title,
                persisted_item.summary,
                persisted_item.payload.to_string(),
                persisted_item.trust_level.to_string(),
                persisted_item.status.to_string(),
                persisted_item.run_id,
                serde_json::to_string(&persisted_item.artifact_refs)?,
                serde_json::to_string(&persisted_item.source_memory_ids)?,
                persisted_item.freshness_expires_at_ms.map(|value| value as i64),
                persisted_item.metadata.as_ref().map(|value| value.to_string()),
                persisted_item.created_at_ms as i64,
                persisted_item.updated_at_ms as i64,
            ],
        )?;

        let mut coverage = tx
            .query_row(
                "SELECT coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata
                 FROM knowledge_coverage WHERE coverage_key = ?1 AND space_id = ?2",
                params![coverage_key.as_str(), space_id.as_str()],
                row_to_knowledge_coverage,
            )
            .optional()?
            .unwrap_or(KnowledgeCoverageRecord {
                coverage_key: coverage_key.clone(),
                space_id: space_id.clone(),
                latest_item_id: None,
                latest_dedupe_key: None,
                last_seen_at_ms: request.promoted_at_ms,
                last_promoted_at_ms: None,
                freshness_expires_at_ms: None,
                metadata: None,
            });
        coverage.latest_item_id = Some(item_id.clone());
        coverage.latest_dedupe_key = Some(dedupe_key.clone());
        coverage.last_seen_at_ms = request.promoted_at_ms;
        if next_status.is_active() {
            coverage.last_promoted_at_ms = Some(request.promoted_at_ms);
        }
        if let Some(freshness_expires_at_ms) = request.freshness_expires_at_ms {
            coverage.freshness_expires_at_ms = Some(freshness_expires_at_ms);
        }
        let mut coverage_metadata = coverage
            .metadata
            .clone()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        coverage_metadata.insert(
            "promotion".to_string(),
            serde_json::json!({
                "item_id": item_id,
                "from_status": previous_status.to_string(),
                "to_status": next_status.to_string(),
                "promoted_at_ms": request.promoted_at_ms,
                "reason": request.reason,
                "reviewer_id": request.reviewer_id,
                "approval_id": request.approval_id,
            }),
        );
        coverage.metadata = Some(serde_json::Value::Object(coverage_metadata));

        tx.execute(
            "INSERT OR REPLACE INTO knowledge_coverage
             (coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                coverage.coverage_key,
                coverage.space_id,
                coverage.latest_item_id,
                coverage.latest_dedupe_key,
                coverage.last_seen_at_ms as i64,
                coverage.last_promoted_at_ms.map(|value| value as i64),
                coverage.freshness_expires_at_ms.map(|value| value as i64),
                coverage.metadata.as_ref().map(|value| value.to_string()),
            ],
        )?;

        tx.commit()?;
        Ok(Some(KnowledgePromotionResult {
            previous_status,
            previous_trust_level,
            promoted,
            item: persisted_item,
            coverage,
        }))
    }

    /// Insert or update a coverage record for a reusable knowledge key.
    pub async fn upsert_knowledge_coverage(
        &self,
        coverage: &KnowledgeCoverageRecord,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_coverage
             (coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                coverage.coverage_key,
                coverage.space_id,
                coverage.latest_item_id,
                coverage.latest_dedupe_key,
                coverage.last_seen_at_ms as i64,
                coverage.last_promoted_at_ms.map(|value| value as i64),
                coverage.freshness_expires_at_ms.map(|value| value as i64),
                coverage.metadata.as_ref().map(|value| value.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Fetch a coverage row for a key and space.
    pub async fn get_knowledge_coverage(
        &self,
        coverage_key: &str,
        space_id: &str,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        let conn = self.conn.lock().await;
        Ok(
            conn.query_row(
                "SELECT coverage_key, space_id, latest_item_id, latest_dedupe_key, last_seen_at_ms, last_promoted_at_ms, freshness_expires_at_ms, metadata
                 FROM knowledge_coverage WHERE coverage_key = ?1 AND space_id = ?2",
                params![coverage_key, space_id],
                row_to_knowledge_coverage,
            )
            .optional()?,
        )
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
}
