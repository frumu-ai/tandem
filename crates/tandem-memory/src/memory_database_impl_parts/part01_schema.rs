// Schema bootstrap and persisted global-memory crypto provenance.

impl MemoryDatabase {
    /// Initialize database schema
    async fn init_schema(&self, created_fresh: bool) -> MemoryResult<()> {
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
        Self::ensure_global_crypto_provenance(&mut conn, created_fresh)?;

        Ok(())
    }

    fn ensure_global_crypto_provenance(
        conn: &mut Connection,
        created_fresh: bool,
    ) -> MemoryResult<()> {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_record_crypto_provenance (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                state TEXT NOT NULL CHECK (state IN ('pristine', 'hosted', 'legacy_unknown', 'plaintext_history'))
            );
            CREATE TRIGGER IF NOT EXISTS memory_record_crypto_provenance_no_downgrade
            BEFORE UPDATE OF state ON memory_record_crypto_provenance
            WHEN NEW.state != OLD.state
              AND NOT ((OLD.state = 'pristine' AND NEW.state IN ('hosted', 'plaintext_history'))
                    OR (OLD.state = 'hosted' AND NEW.state = 'plaintext_history')
                    OR (OLD.state = 'legacy_unknown' AND NEW.state = 'plaintext_history'))
            BEGIN
                SELECT RAISE(ABORT, 'memory crypto provenance cannot be downgraded');
            END;
            CREATE TRIGGER IF NOT EXISTS memory_records_plaintext_history_ai
            AFTER INSERT ON memory_records
            WHEN NEW.content_envelope IS NULL OR NEW.content NOT GLOB 'tce1:*'
              OR NEW.metadata_envelope IS NULL OR IFNULL(NEW.metadata, '') NOT GLOB 'tce1:*'
              OR NEW.provenance_envelope IS NULL OR IFNULL(NEW.provenance, '') NOT GLOB 'tce1:*'
            BEGIN
                UPDATE memory_record_crypto_provenance
                SET state = 'plaintext_history' WHERE id = 1;
            END;
            CREATE TRIGGER IF NOT EXISTS memory_records_plaintext_history_au
            AFTER UPDATE OF content, metadata, provenance, content_envelope, metadata_envelope, provenance_envelope
            ON memory_records
            WHEN NEW.content_envelope IS NULL OR NEW.content NOT GLOB 'tce1:*'
              OR NEW.metadata_envelope IS NULL OR IFNULL(NEW.metadata, '') NOT GLOB 'tce1:*'
              OR NEW.provenance_envelope IS NULL OR IFNULL(NEW.provenance, '') NOT GLOB 'tce1:*'
            BEGIN
                UPDATE memory_record_crypto_provenance
                SET state = 'plaintext_history' WHERE id = 1;
            END;",
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO memory_record_crypto_provenance (id, state) VALUES (1, ?1)",
            params![if created_fresh {
                "pristine"
            } else {
                "legacy_unknown"
            }],
        )?;
        tx.commit()?;
        Ok(())
    }
}
