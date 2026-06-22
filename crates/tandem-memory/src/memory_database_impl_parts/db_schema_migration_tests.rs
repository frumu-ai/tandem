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
