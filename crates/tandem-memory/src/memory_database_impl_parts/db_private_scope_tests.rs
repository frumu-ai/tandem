fn tan_679_global_record(
    id: &str,
    tenant: &MemoryTenantScope,
    owner_org_unit_id: &str,
    owner_subject: Option<&str>,
    content_hash: &str,
) -> GlobalMemoryRecord {
    let now = Utc::now().timestamp_millis() as u64;
    let mut metadata = serde_json::json!({
        "owner_org_unit_id": owner_org_unit_id,
    });
    if let Some(owner_subject) = owner_subject {
        metadata["owner_subject"] = serde_json::json!(owner_subject);
    }

    GlobalMemoryRecord {
        id: id.to_string(),
        user_id: "collector".to_string(),
        source_type: "note".to_string(),
        content: "TAN-679 private scope fixture".to_string(),
        content_hash: content_hash.to_string(),
        run_id: "tan-679-run".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("tan-679".to_string()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(metadata),
        provenance: Some(serde_json::json!({
            "tenant_context": {
                "org_id": tenant.org_id,
                "workspace_id": tenant.workspace_id,
                "deployment_id": tenant.deployment_id,
            }
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: if owner_subject.is_some() {
            "private".to_string()
        } else {
            "shared".to_string()
        },
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    }
}

async fn tan_679_list_record_ids(
    db: &MemoryDatabase,
    tenant: &MemoryTenantScope,
    caller_subject: Option<&str>,
    owner_org_unit_id: Option<&str>,
) -> std::collections::BTreeSet<String> {
    db.list_global_memory_for_tenant_scoped(
        &tenant.org_id,
        &tenant.workspace_id,
        tenant.deployment_id.as_deref(),
        caller_subject,
        caller_subject.unwrap_or_default(),
        None,
        Some("tan-679"),
        None,
        100,
        0,
        owner_org_unit_id,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|record| record.id)
    .collect()
}

async fn tan_679_search_record_ids(
    db: &MemoryDatabase,
    tenant: &MemoryTenantScope,
    caller_subject: Option<&str>,
    owner_org_unit_id: Option<&str>,
) -> std::collections::BTreeSet<String> {
    db.search_global_memory_for_tenant_scoped(
        &tenant.org_id,
        &tenant.workspace_id,
        tenant.deployment_id.as_deref(),
        caller_subject,
        caller_subject.unwrap_or_default(),
        "legacy",
        100,
        Some("tan-679"),
        None,
        None,
        owner_org_unit_id,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|hit| hit.record.id)
    .collect()
}

async fn tan_679_search_chunk_ids(
    db: &MemoryDatabase,
    tenant: &MemoryTenantScope,
    vector: &[f32],
    caller_subject: Option<&str>,
    owner_org_unit_id: Option<&str>,
) -> std::collections::BTreeSet<String> {
    db.search_similar_for_tenant(
        vector,
        MemoryTier::Global,
        None,
        None,
        tenant,
        100,
        caller_subject,
        owner_org_unit_id,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|(chunk, _)| chunk.id)
    .collect()
}

#[tokio::test]
async fn tan_679_fresh_records_and_chunks_persist_and_enforce_private_scope() {
    let (db, _temp) = setup_test_db().await;
    let tenant_a = tenant_scope("org-a", "workspace-a");
    let tenant_b = tenant_scope("org-b", "workspace-b");
    let vector = embedding(0.25, 0.75);

    let shared_record =
        tan_679_global_record("record-shared", &tenant_a, "finance", None, "hash-shared");
    let private_record = tan_679_global_record(
        "record-private-a",
        &tenant_a,
        "finance",
        Some("subject-a"),
        "hash-private-a",
    );
    let foreign_record =
        tan_679_global_record("record-foreign", &tenant_b, "finance", None, "hash-foreign");
    for record in [&shared_record, &private_record, &foreign_record] {
        assert!(db.put_global_memory_record(record).await.unwrap().stored);
    }

    let mut shared_chunk = test_vector_chunk(
        "chunk-shared",
        MemoryTier::Global,
        tenant_a.clone(),
        "shared finance chunk",
        None,
    );
    shared_chunk.metadata = Some(serde_json::json!({ "owner_org_unit_id": "finance" }));
    let mut private_chunk = test_vector_chunk(
        "chunk-private-a",
        MemoryTier::Global,
        tenant_a.clone(),
        "private finance chunk",
        None,
    );
    private_chunk.subject = Some("subject-a".to_string());
    private_chunk.metadata = Some(serde_json::json!({ "owner_org_unit_id": "finance" }));
    let mut foreign_chunk = test_vector_chunk(
        "chunk-foreign",
        MemoryTier::Global,
        tenant_b.clone(),
        "foreign finance chunk",
        None,
    );
    foreign_chunk.metadata = Some(serde_json::json!({ "owner_org_unit_id": "finance" }));
    for chunk in [&shared_chunk, &private_chunk, &foreign_chunk] {
        db.store_chunk(chunk, &vector).await.unwrap();
    }

    {
        let conn = db.conn.lock().await;
        let shared_columns: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM memory_records WHERE id = ?1",
                params!["record-shared"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let private_columns: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM memory_records WHERE id = ?1",
                params!["record-private-a"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let shared_chunk_columns: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM global_memory_chunks WHERE id = ?1",
                params!["chunk-shared"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let private_chunk_columns: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM global_memory_chunks WHERE id = ?1",
                params!["chunk-private-a"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(shared_columns, (0, None));
        assert_eq!(private_columns, (1, Some("subject-a".to_string())));
        assert_eq!(shared_chunk_columns, (0, None));
        assert_eq!(private_chunk_columns, (1, Some("subject-a".to_string())));
    }

    let owner_records =
        tan_679_list_record_ids(&db, &tenant_a, Some("subject-a"), Some("finance")).await;
    assert_eq!(
        owner_records,
        ["record-private-a", "record-shared"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    let peer_records =
        tan_679_list_record_ids(&db, &tenant_a, Some("subject-b"), Some("finance")).await;
    assert_eq!(
        peer_records,
        ["record-shared".to_string()].into_iter().collect()
    );
    let shared_only_records = tan_679_list_record_ids(&db, &tenant_a, None, Some("finance")).await;
    assert_eq!(
        shared_only_records,
        ["record-shared".to_string()].into_iter().collect()
    );
    assert!(
        tan_679_list_record_ids(&db, &tenant_a, Some("subject-a"), Some("engineering"))
            .await
            .is_empty()
    );
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant_b, Some("subject-a"), Some("finance")).await,
        ["record-foreign".to_string()].into_iter().collect()
    );

    let owner_chunks =
        tan_679_search_chunk_ids(&db, &tenant_a, &vector, Some("subject-a"), Some("finance")).await;
    assert_eq!(
        owner_chunks,
        ["chunk-private-a", "chunk-shared"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    let peer_chunks =
        tan_679_search_chunk_ids(&db, &tenant_a, &vector, Some("subject-b"), Some("finance")).await;
    assert_eq!(
        peer_chunks,
        ["chunk-shared".to_string()].into_iter().collect()
    );
    let shared_only_chunks =
        tan_679_search_chunk_ids(&db, &tenant_a, &vector, None, Some("finance")).await;
    assert_eq!(
        shared_only_chunks,
        ["chunk-shared".to_string()].into_iter().collect()
    );
    assert!(tan_679_search_chunk_ids(
        &db,
        &tenant_a,
        &vector,
        Some("subject-a"),
        Some("engineering")
    )
    .await
    .is_empty());
    assert_eq!(
        tan_679_search_chunk_ids(&db, &tenant_b, &vector, Some("subject-a"), Some("finance")).await,
        ["chunk-foreign".to_string()].into_iter().collect()
    );
}

#[tokio::test]
async fn tan_679_global_record_dedupe_keeps_private_owner_subjects_distinct() {
    let (db, _temp) = setup_test_db().await;
    let tenant = tenant_scope("org-a", "workspace-a");
    let owner_a = tan_679_global_record(
        "dedupe-owner-a",
        &tenant,
        "finance",
        Some("subject-a"),
        "same-private-hash",
    );
    let mut owner_b = tan_679_global_record(
        "dedupe-owner-b",
        &tenant,
        "finance",
        Some("subject-b"),
        "same-private-hash",
    );
    owner_b.created_at_ms = owner_a.created_at_ms;
    owner_b.updated_at_ms = owner_a.updated_at_ms;

    assert!(db.put_global_memory_record(&owner_a).await.unwrap().stored);
    assert!(db.put_global_memory_record(&owner_b).await.unwrap().stored);
    assert!(db.put_global_memory_record(&owner_a).await.unwrap().deduped);

    let count: i64 = {
        let conn = db.conn.lock().await;
        conn.query_row(
            "SELECT COUNT(*) FROM memory_records WHERE content_hash = ?1",
            params!["same-private-hash"],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(count, 2);
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant, Some("subject-a"), Some("finance")).await,
        ["dedupe-owner-a".to_string()].into_iter().collect()
    );
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant, Some("subject-b"), Some("finance")).await,
        ["dedupe-owner-b".to_string()].into_iter().collect()
    );
}

#[tokio::test]
async fn tan_679_legacy_private_scope_backfill_does_not_widen_visibility() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("tan_679_legacy.db");
    let created_at = Utc::now().to_rfc3339();
    let now_ms = Utc::now().timestamp_millis();

    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memory_records (
                id TEXT PRIMARY KEY,
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
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
                expires_at_ms INTEGER
            );
            CREATE TABLE global_memory_chunks (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                metadata TEXT,
                source_path TEXT,
                source_mtime INTEGER,
                source_size INTEGER,
                source_hash TEXT,
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
                tenant_deployment_id TEXT,
                subject TEXT,
                owner_org_unit_id TEXT,
                tenant_shared INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        for (id, user_id, source_type, metadata, content_hash) in [
            (
                "legacy-record-private",
                "collector",
                "note",
                r#"{"owner_org_unit_id":"finance","owner_subject":"subject-a"}"#,
                "legacy-private-hash",
            ),
            (
                "legacy-record-shared",
                "collector",
                "note",
                r#"{"owner_org_unit_id":"finance"}"#,
                "legacy-shared-hash",
            ),
            (
                "legacy-record-user-message",
                "subject-a",
                "user_message",
                r#"{"owner_org_unit_id":"finance"}"#,
                "legacy-user-message-hash",
            ),
        ] {
            conn.execute(
                "INSERT INTO memory_records (
                    id, tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    user_id, source_type, content, content_hash, run_id, project_tag,
                    metadata, redaction_status, created_at_ms, updated_at_ms
                 ) VALUES (?1, 'org-a', 'workspace-a', 'deployment-1',
                    ?2, ?3, 'legacy TAN-679 record', ?4, 'legacy-run', 'tan-679',
                    ?5, 'passed', ?6, ?6)",
                params![id, user_id, source_type, content_hash, metadata, now_ms],
            )
            .unwrap();
        }
        for (id, subject, content) in [
            (
                "legacy-chunk-private",
                Some("subject-a"),
                "legacy private chunk",
            ),
            ("legacy-chunk-shared", None, "legacy shared chunk"),
        ] {
            conn.execute(
                "INSERT INTO global_memory_chunks (
                    id, content, source, created_at, token_count, metadata,
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                    subject, owner_org_unit_id
                 ) VALUES (?1, ?2, 'legacy', ?3, 3, ?4,
                    'org-a', 'workspace-a', 'deployment-1', ?5, 'finance')",
                params![
                    id,
                    content,
                    created_at,
                    r#"{"owner_org_unit_id":"finance"}"#,
                    subject
                ],
            )
            .unwrap();
        }
    }

    let db = MemoryDatabase::new(&db_path).await.unwrap();
    let vector = embedding(0.5, 0.5);
    let embedding_json = format!(
        "[{}]",
        vector
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    {
        let conn = db.conn.lock().await;
        let record_scope: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM memory_records WHERE id = ?1",
                params!["legacy-record-private"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let chunk_scope: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM global_memory_chunks WHERE id = ?1",
                params!["legacy-chunk-private"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let shared_record_scope: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM memory_records WHERE id = ?1",
                params!["legacy-record-shared"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let legacy_user_message_scope: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM memory_records WHERE id = ?1",
                params!["legacy-record-user-message"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let shared_chunk_scope: (i64, Option<String>) = conn
            .query_row(
                "SELECT private, owner_subject FROM global_memory_chunks WHERE id = ?1",
                params!["legacy-chunk-shared"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(record_scope, (1, Some("subject-a".to_string())));
        assert_eq!(legacy_user_message_scope, (1, Some("subject-a".to_string())));
        assert_eq!(chunk_scope, (1, Some("subject-a".to_string())));
        assert_eq!(shared_record_scope, (0, None));
        assert_eq!(shared_chunk_scope, (0, None));

        for id in ["legacy-chunk-private", "legacy-chunk-shared"] {
            conn.execute(
                "INSERT INTO global_memory_vectors (chunk_id, embedding) VALUES (?1, ?2)",
                params![id, embedding_json],
            )
            .unwrap();
        }
    }

    let tenant = tenant_scope("org-a", "workspace-a");
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant, Some("subject-a"), Some("finance")).await,
        [
            "legacy-record-private",
            "legacy-record-shared",
            "legacy-record-user-message",
        ]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant, Some("subject-b"), Some("finance")).await,
        ["legacy-record-shared".to_string()].into_iter().collect()
    );
    assert_eq!(
        tan_679_list_record_ids(&db, &tenant, None, Some("finance")).await,
        ["legacy-record-shared".to_string()].into_iter().collect()
    );
    assert_eq!(
        tan_679_search_record_ids(&db, &tenant, Some("subject-a"), Some("finance")).await,
        [
            "legacy-record-private",
            "legacy-record-shared",
            "legacy-record-user-message",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    );
    assert_eq!(
        tan_679_search_record_ids(&db, &tenant, Some("subject-b"), Some("finance")).await,
        ["legacy-record-shared".to_string()].into_iter().collect()
    );

    assert_eq!(
        tan_679_search_chunk_ids(&db, &tenant, &vector, Some("subject-a"), Some("finance")).await,
        ["legacy-chunk-private", "legacy-chunk-shared"]
            .into_iter()
            .map(str::to_string)
            .collect()
    );
    assert_eq!(
        tan_679_search_chunk_ids(&db, &tenant, &vector, Some("subject-b"), Some("finance")).await,
        ["legacy-chunk-shared".to_string()].into_iter().collect()
    );
    assert_eq!(
        tan_679_search_chunk_ids(&db, &tenant, &vector, None, Some("finance")).await,
        ["legacy-chunk-shared".to_string()].into_iter().collect()
    );
}
