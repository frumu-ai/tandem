
    fn retention_record(
        id: &str,
        source_type: &str,
        created_at_ms: i64,
        expires_at_ms: Option<i64>,
        scope: &MemoryTenantScope,
    ) -> GlobalMemoryRecord {
        GlobalMemoryRecord {
            id: id.to_string(),
            user_id: "user-retention".to_string(),
            source_type: source_type.to_string(),
            content: format!("retention record {id}"),
            content_hash: format!("hash-{id}"),
            run_id: "run-retention".to_string(),
            session_id: None,
            message_id: None,
            tool_name: None,
            project_tag: None,
            channel_tag: None,
            host_tag: None,
            metadata: None,
            provenance: Some(serde_json::json!({
                "tenant_context": {
                    "org_id": scope.org_id.as_str(),
                    "workspace_id": scope.workspace_id.as_str(),
                    "deployment_id": scope.deployment_id.as_deref(),
                }
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: created_at_ms as u64,
            updated_at_ms: created_at_ms as u64,
            expires_at_ms: expires_at_ms.map(|v| v as u64),
        }
    }

    async fn get_record_for_scope(
        db: &MemoryDatabase,
        id: &str,
        scope: &MemoryTenantScope,
    ) -> Option<GlobalMemoryRecord> {
        db.get_global_memory_for_tenant(
            id,
            scope.org_id.as_str(),
            scope.workspace_id.as_str(),
            scope.deployment_id.as_deref(),
        )
        .await
        .unwrap()
    }

    async fn count_rows(db: &MemoryDatabase, sql: &str) -> i64 {
        let conn = db.conn.lock().await;
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[tokio::test]
    async fn test_hygiene_reaps_expired_memory_records_and_keeps_live_ones() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let now_ms = Utc::now().timestamp_millis();

        for record in [
            retention_record("rec-expired", "note", now_ms - 10_000, Some(now_ms - 1_000), &tenant_a),
            retention_record("rec-live-ttl", "note", now_ms, Some(now_ms + 86_400_000), &tenant_a),
            retention_record("rec-no-ttl", "note", now_ms, None, &tenant_a),
        ] {
            assert!(db.put_global_memory_record(&record).await.unwrap().stored);
        }

        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 1);

        assert!(get_record_for_scope(&db, "rec-expired", &tenant_a)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "rec-live-ttl", &tenant_a)
            .await
            .is_some());
        assert!(get_record_for_scope(&db, "rec-no-ttl", &tenant_a)
            .await
            .is_some());
        // The AFTER DELETE trigger must have removed the FTS mirror row too.
        assert_eq!(
            count_rows(
                &db,
                "SELECT COUNT(*) FROM memory_records_fts WHERE id = 'rec-expired'"
            )
            .await,
            0
        );
    }

    #[tokio::test]
    async fn test_hygiene_expired_record_reaping_is_tenant_isolated() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let now_ms = Utc::now().timestamp_millis();

        let record_a =
            retention_record("rec-a-expired", "note", now_ms - 10_000, Some(now_ms - 1_000), &tenant_a);
        let record_b =
            retention_record("rec-b-expired", "note", now_ms - 10_000, Some(now_ms - 1_000), &tenant_b);
        assert!(db.put_global_memory_record(&record_a).await.unwrap().stored);
        assert!(db.put_global_memory_record(&record_b).await.unwrap().stored);

        db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();

        assert!(get_record_for_scope(&db, "rec-a-expired", &tenant_a)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "rec-b-expired", &tenant_b)
            .await
            .is_some());
    }

    #[tokio::test]
    async fn test_hygiene_prunes_old_exchange_records() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let now_ms = Utc::now().timestamp_millis();
        let old_ms = (Utc::now() - chrono::Duration::days(400)).timestamp_millis();

        for record in [
            retention_record("ex-old-user", "user_message", old_ms, None, &tenant_a),
            retention_record("ex-old-assistant", "assistant_final", old_ms, None, &tenant_a),
            retention_record("ex-new-user", "user_message", now_ms, None, &tenant_a),
            retention_record("ex-old-note", "note", old_ms, None, &tenant_a),
        ] {
            assert!(db.put_global_memory_record(&record).await.unwrap().stored);
        }

        // No __global__ config row: exchange_retention_days defaults to 365.
        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 2);

        assert!(get_record_for_scope(&db, "ex-old-user", &tenant_a)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "ex-old-assistant", &tenant_a)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "ex-new-user", &tenant_a)
            .await
            .is_some());
        assert!(get_record_for_scope(&db, "ex-old-note", &tenant_a)
            .await
            .is_some());
    }

    #[tokio::test]
    async fn test_hygiene_exchange_retention_zero_keeps_forever() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let old_ms = (Utc::now() - chrono::Duration::days(400)).timestamp_millis();

        let config = MemoryConfig {
            exchange_retention_days: 0,
            ..Default::default()
        };
        db.update_config_for_tenant("__global__", &config, &tenant_a)
            .await
            .unwrap();

        let record = retention_record("ex-kept-forever", "user_message", old_ms, None, &tenant_a);
        assert!(db.put_global_memory_record(&record).await.unwrap().stored);

        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 0);
        assert!(get_record_for_scope(&db, "ex-kept-forever", &tenant_a)
            .await
            .is_some());
    }

    #[tokio::test]
    async fn test_hygiene_evicts_oldest_project_chunks_over_cap() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        let config = MemoryConfig {
            max_chunks: 3,
            ..Default::default()
        };
        db.update_config_for_tenant("shared-project", &config, &tenant_a)
            .await
            .unwrap();

        for age_minutes in 1..=5 {
            let mut chunk = test_vector_chunk(
                &format!("proj-cap-{age_minutes}"),
                MemoryTier::Project,
                tenant_a.clone(),
                &format!("project cap chunk {age_minutes}"),
                None,
            );
            chunk.created_at = Utc::now() - chrono::Duration::minutes(age_minutes);
            db.store_chunk(&chunk, &embedding(0.1, 0.9)).await.unwrap();
        }
        // An even older chunk for another tenant must survive tenant A hygiene.
        let mut chunk_b = test_vector_chunk(
            "proj-cap-tenant-b",
            MemoryTier::Project,
            tenant_b.clone(),
            "tenant b oldest project chunk",
            None,
        );
        chunk_b.created_at = Utc::now() - chrono::Duration::minutes(60);
        db.store_chunk(&chunk_b, &embedding(0.1, 0.9)).await.unwrap();

        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 2);

        let remaining = db
            .get_project_chunks_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap();
        let mut remaining_ids: Vec<String> =
            remaining.iter().map(|chunk| chunk.id.clone()).collect();
        remaining_ids.sort();
        // The two oldest (4 and 5 minutes) were evicted; the newest three remain.
        assert_eq!(
            remaining_ids,
            vec![
                "proj-cap-1".to_string(),
                "proj-cap-2".to_string(),
                "proj-cap-3".to_string()
            ]
        );
        // Vector rows for evicted chunks are gone too (3 tenant A + 1 tenant B remain).
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM project_memory_vectors").await,
            4
        );
        assert_eq!(
            db.get_project_chunks_for_tenant("shared-project", &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_hygiene_prunes_global_chunks_only_when_configured() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");

        let mut chunk = test_vector_chunk(
            "global-old-chunk",
            MemoryTier::Global,
            tenant_a.clone(),
            "old global archived memory",
            None,
        );
        chunk.created_at = Utc::now() - chrono::Duration::days(90);
        db.store_chunk(&chunk, &embedding(0.4, 0.6)).await.unwrap();

        // Default global_retention_days = 0: the chunk is never age-pruned.
        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM global_memory_chunks").await,
            1
        );

        let config = MemoryConfig {
            global_retention_days: 30,
            ..Default::default()
        };
        db.update_config_for_tenant("__global__", &config, &tenant_a)
            .await
            .unwrap();

        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM global_memory_chunks").await,
            0
        );
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM global_memory_vectors").await,
            0
        );
    }

    #[tokio::test]
    async fn test_hygiene_all_tenants_reaps_every_partition() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let local = MemoryTenantScope::local();
        let now_ms = Utc::now().timestamp_millis();

        for (id, scope) in [
            ("all-a-expired", &tenant_a),
            ("all-b-expired", &tenant_b),
            ("all-local-expired", &local),
        ] {
            let record =
                retention_record(id, "note", now_ms - 10_000, Some(now_ms - 1_000), scope);
            assert!(db.put_global_memory_record(&record).await.unwrap().stored);
        }

        let scopes = db.list_memory_tenant_scopes().await.unwrap();
        assert!(scopes.contains(&tenant_a));
        assert!(scopes.contains(&tenant_b));
        assert!(scopes.contains(&local));

        // The scheduled job entry point must reap all three partitions, not
        // just the local one.
        let deleted = db.run_hygiene_all_tenants(0).await.unwrap();
        assert_eq!(deleted, 3);
        assert!(get_record_for_scope(&db, "all-a-expired", &tenant_a)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "all-b-expired", &tenant_b)
            .await
            .is_none());
        assert!(get_record_for_scope(&db, "all-local-expired", &local)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_hygiene_writes_cleanup_log_and_get_cleanup_log_reads_it() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let now_ms = Utc::now().timestamp_millis();

        let record =
            retention_record("rec-logged", "note", now_ms - 10_000, Some(now_ms - 1_000), &tenant_a);
        assert!(db.put_global_memory_record(&record).await.unwrap().stored);

        assert_eq!(db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap(), 1);

        let entries = db
            .get_cleanup_log_for_tenant(10, &tenant_a)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cleanup_type, "hygiene_expired_records");
        assert_eq!(entries[0].tier, MemoryTier::Global);
        assert_eq!(entries[0].chunks_deleted, 1);

        // Tenant isolation and the local-scope convenience reader.
        let tenant_b = tenant_scope("org-b", "workspace-b");
        assert!(db
            .get_cleanup_log_for_tenant(10, &tenant_b)
            .await
            .unwrap()
            .is_empty());
        db.log_cleanup("manual", MemoryTier::Session, None, Some("session-1"), 2, 0)
            .await
            .unwrap();
        let local_entries = db.get_cleanup_log(10).await.unwrap();
        assert_eq!(local_entries.len(), 1);
        assert_eq!(local_entries[0].cleanup_type, "manual");
        assert_eq!(local_entries[0].session_id.as_deref(), Some("session-1"));
        assert_eq!(local_entries[0].chunks_deleted, 2);
    }
