#[tokio::test]
async fn schema_migration_ledger_records_bootstrap_once() {
    let (db, temp) = setup_test_db().await;
    let migration_count: i64 = {
        let conn = db.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(migration_count, 5);

    let db_path = temp.path().join("test_memory.db");
    drop(db);
    let reopened = MemoryDatabase::new(&db_path).await.unwrap();
    let reopened_migration_count: i64 = {
        let conn = reopened.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(reopened_migration_count, 5);

    let private_migration_count: i64 = {
        let conn = reopened.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations
             WHERE version = 5 AND name = 'private_owner_subject_scope'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(private_migration_count, 1);
}

fn create_v5_legacy_schema(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        );
        INSERT INTO schema_migrations VALUES (1, 'bootstrap_memory_schema', 1);
        INSERT INTO schema_migrations VALUES (2, 'memory_config_retention_columns', 2);
        INSERT INTO schema_migrations VALUES (3, 'chunk_owner_org_unit_scope', 3);
        INSERT INTO schema_migrations VALUES (4, 'memory_crypto_envelope', 4);

        CREATE TABLE session_memory_chunks (
            id TEXT PRIMARY KEY,
            subject TEXT,
            tenant_org_id TEXT NOT NULL DEFAULT 'local',
            tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
            tenant_deployment_id TEXT,
            owner_org_unit_id TEXT,
            session_id TEXT NOT NULL
        );
        CREATE TABLE project_memory_chunks (
            id TEXT PRIMARY KEY,
            subject TEXT,
            tenant_org_id TEXT NOT NULL DEFAULT 'local',
            tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
            tenant_deployment_id TEXT,
            owner_org_unit_id TEXT,
            project_id TEXT NOT NULL
        );
        CREATE TABLE global_memory_chunks (
            id TEXT PRIMARY KEY,
            subject TEXT,
            tenant_org_id TEXT NOT NULL DEFAULT 'local',
            tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
            tenant_deployment_id TEXT,
            owner_org_unit_id TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE memory_records (
            id TEXT PRIMARY KEY,
            tenant_org_id TEXT NOT NULL DEFAULT 'local',
            tenant_workspace_id TEXT NOT NULL DEFAULT 'local',
            tenant_deployment_id TEXT,
            user_id TEXT NOT NULL,
            source_type TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            run_id TEXT NOT NULL,
            session_id TEXT,
            message_id TEXT,
            tool_name TEXT,
            owner_org_unit_id TEXT,
            metadata TEXT,
            created_at_ms INTEGER NOT NULL
        );",
    )
    .unwrap();
}

#[test]
fn sqlite_migration_rejects_ledger_name_version_conflicts() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        );
        INSERT INTO schema_migrations
        VALUES (5, 'memory_crypto_envelope', 1);",
    )
    .unwrap();

    let error = crate::migrations::run_sqlite_migrations(&mut conn).unwrap_err();
    assert!(matches!(error, MemoryError::InvalidConfig(_)));
}

#[test]
fn failed_sqlite_migration_rolls_back_schema_data_and_ledger() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    create_v5_legacy_schema(&conn);
    conn.execute(
        "INSERT INTO session_memory_chunks (id, subject, session_id)
         VALUES ('chunk', 'alice', 'session')",
        [],
    )
    .unwrap();
    for id in ["record-1", "record-2"] {
        conn.execute(
            "INSERT INTO memory_records (
                id, user_id, source_type, content_hash, run_id, metadata, created_at_ms
             ) VALUES (?1, 'user', 'source', 'hash', 'run', NULL, 1)",
            params![id],
        )
        .unwrap();
    }

    let error = crate::migrations::run_sqlite_migrations(&mut conn).unwrap_err();
    assert!(matches!(error, MemoryError::Database(_)));

    let columns = conn
        .prepare("PRAGMA table_info(session_memory_chunks)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!columns.iter().any(|column| column == "private"));
    assert!(!columns.iter().any(|column| column == "owner_subject"));

    let migration_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migration_count, 0);
}

#[test]
fn legacy_private_owner_migration_backfills_enforces_and_reruns() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    create_v5_legacy_schema(&conn);
    conn.execute(
        "INSERT INTO session_memory_chunks (id, subject, session_id)
         VALUES ('chunk', ' alice ', 'session')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_records (
            id, user_id, source_type, content_hash, run_id, metadata, created_at_ms
         ) VALUES (
            'record', 'user', 'source', 'hash', 'run',
            '{\"owner_subject\":\" bob \"}', 1
         )",
        [],
    )
    .unwrap();

    crate::migrations::run_sqlite_migrations(&mut conn).unwrap();
    crate::migrations::run_sqlite_migrations(&mut conn).unwrap();

    let chunk_scope: (i64, Option<String>) = conn
        .query_row(
            "SELECT private, owner_subject FROM session_memory_chunks WHERE id = 'chunk'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(chunk_scope, (1, Some("alice".to_string())));

    let record_scope: (i64, Option<String>) = conn
        .query_row(
            "SELECT private, owner_subject FROM memory_records WHERE id = 'record'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(record_scope, (1, Some("bob".to_string())));

    let constraint_error = conn
        .execute(
            "UPDATE session_memory_chunks
             SET private = 1, owner_subject = NULL
             WHERE id = 'chunk'",
            [],
        )
        .unwrap_err();
    assert!(constraint_error
        .to_string()
        .contains("private owner subject constraint failed"));

    let migration_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migration_count, 1);
}

#[tokio::test]
async fn legacy_chunk_tables_gain_owner_org_unit_column_and_backfill() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("legacy_chunks.db");
    let created_at = chrono::Utc::now().to_rfc3339();

    {
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
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE session_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                session_id TEXT NOT NULL,
                project_id TEXT,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT,
                subject TEXT
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE project_memory_chunks (
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
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE global_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO session_memory_chunks
             (id, content, session_id, project_id, source, created_at, token_count, metadata, subject)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "legacy-session",
                "session memory",
                "session-1",
                "project-1",
                "test",
                created_at,
                2,
                r#"{"owner_org_unit_id":"finance"}"#,
                " legacy-user "
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_memory_chunks
             (id, content, project_id, session_id, source, created_at, token_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "legacy-project",
                "project memory",
                "project-1",
                "session-1",
                "test",
                created_at,
                2,
                r#"{"owner_org_unit_id":"engineering"}"#
            ],
        )
        .unwrap();

        conn.execute(
            &format!(
                "CREATE VIRTUAL TABLE session_memory_vectors USING vec0(
                    chunk_id TEXT PRIMARY KEY,
                    embedding float[{}]
                )",
                DEFAULT_EMBEDDING_DIMENSION
            ),
            [],
        )
        .unwrap();
        let embedding = format!(
            "[{}]",
            vec!["0.0"; DEFAULT_EMBEDDING_DIMENSION].join(",")
        );
        conn.execute(
            "INSERT INTO session_memory_vectors (chunk_id, embedding) VALUES (?1, ?2)",
            params!["legacy-session", embedding],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO global_memory_chunks
             (id, content, source, created_at, token_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "legacy-global",
                "global memory",
                "test",
                created_at,
                2,
                r#"{"owner_org_unit_id":"sales"}"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO global_memory_chunks
             (id, content, source, created_at, token_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "legacy-tenant-shared",
                "global shared memory",
                "test",
                created_at,
                2,
                r#"{"tenant_shared":true}"#
            ],
        )
        .unwrap();
    }

    let db = MemoryDatabase::new(&db_path).await.unwrap();
    let conn = db.conn.lock().await;
    for table in [
        "session_memory_chunks",
        "project_memory_chunks",
        "global_memory_chunks",
    ] {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let cols = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            cols.iter().any(|col| col == "owner_org_unit_id"),
            "{table} should gain owner_org_unit_id"
        );
        assert!(
            cols.iter().any(|col| col == "tenant_shared"),
            "{table} should gain tenant_shared"
        );
        assert!(
            cols.iter().any(|col| col == "private"),
            "{table} should gain private"
        );
        assert!(
            cols.iter().any(|col| col == "owner_subject"),
            "{table} should gain owner_subject"
        );
    }

    let session_owner: Option<String> = conn
        .query_row(
            "SELECT owner_org_unit_id FROM session_memory_chunks WHERE id = 'legacy-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let project_owner: Option<String> = conn
        .query_row(
            "SELECT owner_org_unit_id FROM project_memory_chunks WHERE id = 'legacy-project'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let global_owner: Option<String> = conn
        .query_row(
            "SELECT owner_org_unit_id FROM global_memory_chunks WHERE id = 'legacy-global'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(session_owner.as_deref(), Some("finance"));
    assert_eq!(project_owner.as_deref(), Some("engineering"));
    assert_eq!(global_owner.as_deref(), Some("sales"));

    let global_shared: i64 = conn
        .query_row(
            "SELECT tenant_shared FROM global_memory_chunks WHERE id = 'legacy-tenant-shared'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(global_shared, 1);

    let session_private_scope: (i64, Option<String>) = conn
        .query_row(
            "SELECT private, owner_subject
             FROM session_memory_chunks WHERE id = 'legacy-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        session_private_scope,
        (1, Some("legacy-user".to_string()))
    );

    let vector_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM session_memory_vectors
             WHERE chunk_id = 'legacy-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(vector_count, 1);
}
