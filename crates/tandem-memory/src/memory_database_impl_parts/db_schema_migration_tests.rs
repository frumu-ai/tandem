#[tokio::test]
async fn schema_migration_ledger_records_bootstrap_once() {
    let (db, temp) = setup_test_db().await;
    let migration_count: i64 = {
        let conn = db.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 1 AND name = 'bootstrap_memory_schema'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(migration_count, 1);

    let db_path = temp.path().join("test_memory.db");
    drop(db);
    let reopened = MemoryDatabase::new(&db_path).await.unwrap();
    let reopened_migration_count: i64 = {
        let conn = reopened.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 1 AND name = 'bootstrap_memory_schema'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(reopened_migration_count, 1);
}

#[tokio::test]
async fn legacy_chunk_tables_gain_owner_org_unit_column_and_backfill() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("legacy_chunks.db");
    let created_at = chrono::Utc::now().to_rfc3339();

    {
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
                metadata TEXT
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
             (id, content, session_id, project_id, source, created_at, token_count, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "legacy-session",
                "session memory",
                "session-1",
                "project-1",
                "test",
                created_at,
                2,
                r#"{"owner_org_unit_id":"finance"}"#
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
}
