impl MemoryDatabase {
    /// Override the memory payload crypto provider (used to select an explicit
    /// local-encrypted/hosted provider or in tests). Defaults to env resolution.
    pub fn with_crypto_provider(
        mut self,
        crypto: crate::crypto::MemoryCryptoProvider,
    ) -> MemoryResult<Self> {
        // This test/embedding override must enforce the same legacy-data gate
        // as env-selected hosted mode in new(). Otherwise a caller can open a
        // plaintext DB locally and only then switch to hosted encryption.
        let conn = self.conn.try_lock().map_err(|_| {
            MemoryError::Lock("cannot switch memory crypto while the database is busy".to_string())
        })?;
        Self::reject_legacy_global_records_for_hosted_connection(&conn, &crypto, false)?;
        Self::promote_hosted_global_provenance(&conn, &crypto)?;
        drop(conn);
        self.crypto = crypto;
        Ok(self)
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

    fn deny_unscoped_global_in_hosted(&self, operation: &str) -> MemoryResult<()> {
        if self.crypto.is_hosted()
            || self
                .strict_tenant_enforcement
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(MemoryError::TenantScopeViolation(format!(
                "{operation} requires an explicit tenant and owner scope in hosted mode"
            )));
        }
        Ok(())
    }

    /// Initialize or open the memory database
    pub async fn new(db_path: &Path) -> MemoryResult<Self> {
        // A missing main file is not fresh if SQLite sidecars from an earlier
        // database remain at this path. Never assume their pages are clean.
        let created_fresh = !db_path.exists()
            && ["-wal", "-shm"].iter().all(|suffix| {
                let mut sidecar = db_path.as_os_str().to_os_string();
                sidecar.push(suffix);
                !Path::new(&sidecar).exists()
            });
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

        // An in-place plaintext-to-hosted upgrade cannot erase old SQLite pages,
        // FTS segments, WAL frames or external backups. Require an explicit
        // offline migration into fresh storage before opening a legacy database.
        db.reject_legacy_global_records_for_hosted(created_fresh)
            .await?;

        // Initialize schema
        db.init_schema(created_fresh).await?;
        db.promote_hosted_global_provenance_after_init().await?;
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

    async fn reject_legacy_global_records_for_hosted(
        &self,
        created_fresh: bool,
    ) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        Self::reject_legacy_global_records_for_hosted_connection(&conn, &self.crypto, created_fresh)
    }

    fn reject_legacy_global_records_for_hosted_connection(
        conn: &Connection,
        crypto: &crate::crypto::MemoryCryptoProvider,
        created_fresh: bool,
    ) -> MemoryResult<()> {
        if !crypto.is_hosted() {
            return Ok(());
        }
        if created_fresh {
            return Ok(());
        }
        let marker_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'memory_record_crypto_provenance')",
            [],
            |row| row.get(0),
        )?;
        if !marker_exists {
            return Err(MemoryError::InvalidConfig(
                "hosted global memory requires a fresh database or an explicit offline migration; storage provenance is unknown".to_string(),
            ));
        }
        let provenance: Option<String> = conn
            .query_row(
                "SELECT state FROM memory_record_crypto_provenance WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if !matches!(provenance.as_deref(), Some("pristine" | "hosted")) {
            return Err(MemoryError::InvalidConfig(
                "hosted global memory cannot reuse SQLite storage with plaintext history; migrate SQLite, FTS, WAL and backups offline".to_string(),
            ));
        }
        let table_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'memory_records')",
            [],
            |row| row.get(0),
        )?;
        if !table_exists {
            return Ok(());
        }
        let columns: HashSet<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memory_records)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<Result<_, _>>()?
        };
        if ![
            "content_envelope",
            "metadata_envelope",
            "provenance_envelope",
        ]
        .iter()
        .all(|name| columns.contains(*name))
        {
            return Err(MemoryError::InvalidConfig(
                "hosted global memory requires a fresh encrypted database; migrate legacy SQLite, WAL and backups offline".to_string(),
            ));
        }
        let legacy_rows: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_records
             WHERE content_envelope IS NULL OR content NOT LIKE 'tce1:%'
                OR metadata_envelope IS NULL OR IFNULL(metadata, '') NOT LIKE 'tce1:%'
                OR provenance_envelope IS NULL OR IFNULL(provenance, '') NOT LIKE 'tce1:%')",
            [],
            |row| row.get(0),
        )?;
        if legacy_rows {
            return Err(MemoryError::InvalidConfig(
                "hosted global memory contains legacy plaintext rows; migrate SQLite, WAL and backups offline".to_string(),
            ));
        }
        Ok(())
    }

    async fn promote_hosted_global_provenance_after_init(&self) -> MemoryResult<()> {
        let conn = self.conn.lock().await;
        Self::promote_hosted_global_provenance(&conn, &self.crypto)
    }

    fn promote_hosted_global_provenance(
        conn: &Connection,
        crypto: &crate::crypto::MemoryCryptoProvider,
    ) -> MemoryResult<()> {
        if crypto.is_hosted() {
            conn.execute(
                "UPDATE memory_record_crypto_provenance SET state = 'hosted'
                 WHERE id = 1 AND state = 'pristine'",
                [],
            )?;
        }
        Ok(())
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
        if self.crypto.is_hosted() {
            return Err(MemoryError::InvalidConfig(
                "hosted memory reset requires a new database and explicit recovery of SQLite, WAL and backups".to_string(),
            ));
        }
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

        self.init_schema(false).await
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
        let key_scope = crate::types::memory_key_scope_from_metadata(
            &chunk.tenant_scope,
            chunk.metadata.as_ref(),
        );
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
