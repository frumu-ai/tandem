    #[tokio::test]
    async fn test_project_stats_file_index_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.upsert_file_index_entry_for_tenant(
            "shared-project",
            "repo/a.md",
            1,
            10,
            "hash-a",
            &tenant_a,
        )
        .await
        .unwrap();
        db.upsert_project_index_status_for_tenant("shared-project", 10, 9, 8, 1, 0, &tenant_a)
            .await
            .unwrap();
        db.upsert_file_index_entry_for_tenant(
            "shared-project",
            "repo/b.md",
            2,
            20,
            "hash-b",
            &tenant_b,
        )
        .await
        .unwrap();
        db.upsert_project_index_status_for_tenant("shared-project", 3, 2, 1, 1, 0, &tenant_b)
            .await
            .unwrap();

        let stats_a = db
            .get_project_stats_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap();
        let stats_b = db
            .get_project_stats_for_tenant("shared-project", &tenant_b)
            .await
            .unwrap();

        assert_eq!(stats_a.indexed_files, 1);
        assert_eq!(stats_a.last_total_files, Some(10));
        assert_eq!(stats_a.last_indexed_files, Some(8));
        assert_eq!(stats_b.indexed_files, 1);
        assert_eq!(stats_b.last_total_files, Some(3));
        assert_eq!(stats_b.last_indexed_files, Some(1));
    }

    #[tokio::test]
    async fn test_clear_session_and_project_memory_are_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-clear-session",
                MemoryTier::Session,
                tenant_a.clone(),
                "tenant a session clear target",
                None,
            ),
            &embedding(0.1, 0.9),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-clear-session",
                MemoryTier::Session,
                tenant_b.clone(),
                "tenant b session must remain",
                None,
            ),
            &embedding(0.1, 0.9),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-clear-project",
                MemoryTier::Project,
                tenant_a.clone(),
                "tenant a project clear target",
                None,
            ),
            &embedding(0.2, 0.8),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-clear-project",
                MemoryTier::Project,
                tenant_b.clone(),
                "tenant b project must remain",
                None,
            ),
            &embedding(0.2, 0.8),
        )
        .await
        .unwrap();

        assert_eq!(
            db.clear_session_memory_for_tenant("shared-session", &tenant_a)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.clear_project_memory_for_tenant("shared-project", &tenant_a)
                .await
                .unwrap(),
            1
        );

        let tenant_b_session = db
            .get_session_chunks_for_tenant("shared-session", &tenant_b)
            .await
            .unwrap();
        let tenant_b_project = db
            .get_project_chunks_for_tenant("shared-project", &tenant_b)
            .await
            .unwrap();
        assert_eq!(tenant_b_session.len(), 1);
        assert_eq!(tenant_b_project.len(), 1);
    }

    #[tokio::test]
    async fn test_old_session_cleanup_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let old = Utc::now() - chrono::Duration::days(90);

        let mut tenant_a_old = test_vector_chunk(
            "tenant-a-old-session",
            MemoryTier::Session,
            tenant_a.clone(),
            "tenant a old session",
            None,
        );
        tenant_a_old.created_at = old;
        db.store_chunk(&tenant_a_old, &embedding(0.3, 0.7))
            .await
            .unwrap();

        let mut tenant_b_old = test_vector_chunk(
            "tenant-b-old-session",
            MemoryTier::Session,
            tenant_b.clone(),
            "tenant b old session",
            None,
        );
        tenant_b_old.created_at = old;
        db.store_chunk(&tenant_b_old, &embedding(0.3, 0.7))
            .await
            .unwrap();

        assert_eq!(
            db.cleanup_old_sessions_for_tenant(30, &tenant_a)
                .await
                .unwrap(),
            1
        );
        assert!(db
            .get_session_chunks_for_tenant("shared-session", &tenant_a)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.get_session_chunks_for_tenant("shared-session", &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );
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

    #[tokio::test]
    async fn test_config_crud_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        let config_a = MemoryConfig {
            max_chunks: 111,
            session_retention_days: 7,
            ..Default::default()
        };
        let config_b = MemoryConfig {
            max_chunks: 222,
            session_retention_days: 14,
            ..Default::default()
        };
        db.update_config_for_tenant("shared-project", &config_a, &tenant_a)
            .await
            .unwrap();
        db.update_config_for_tenant("shared-project", &config_b, &tenant_b)
            .await
            .unwrap();

        let loaded_a = db
            .get_or_create_config_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap();
        let loaded_b = db
            .get_or_create_config_for_tenant("shared-project", &tenant_b)
            .await
            .unwrap();

        assert_eq!(loaded_a.max_chunks, 111);
        assert_eq!(loaded_a.session_retention_days, 7);
        assert_eq!(loaded_b.max_chunks, 222);
        assert_eq!(loaded_b.session_retention_days, 14);
    }

    #[tokio::test]
    async fn test_prune_old_session_chunks_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let old = Utc::now() - chrono::Duration::days(10);

        let mut chunk_a = test_vector_chunk(
            "tenant-a-old-session-prune",
            MemoryTier::Session,
            tenant_a.clone(),
            "old tenant a session chunk",
            None,
        );
        chunk_a.created_at = old;
        let mut chunk_b = test_vector_chunk(
            "tenant-b-old-session-prune",
            MemoryTier::Session,
            tenant_b.clone(),
            "old tenant b session chunk",
            None,
        );
        chunk_b.created_at = old;

        db.store_chunk(&chunk_a, &embedding(0.2, 0.8))
            .await
            .unwrap();
        db.store_chunk(&chunk_b, &embedding(0.2, 0.8))
            .await
            .unwrap();

        let deleted = db
            .prune_old_session_chunks_for_tenant(1, &tenant_a)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(db
            .get_session_chunks_for_tenant("shared-session", &tenant_a)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.get_session_chunks_for_tenant("shared-session", &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_run_hygiene_reads_tenant_scoped_global_config() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let old = Utc::now() - chrono::Duration::days(10);

        let config_a = MemoryConfig {
            session_retention_days: 1,
            ..Default::default()
        };
        let config_b = MemoryConfig {
            session_retention_days: 0,
            ..Default::default()
        };
        db.update_config_for_tenant("__global__", &config_a, &tenant_a)
            .await
            .unwrap();
        db.update_config_for_tenant("__global__", &config_b, &tenant_b)
            .await
            .unwrap();

        let mut chunk_a = test_vector_chunk(
            "tenant-a-hygiene",
            MemoryTier::Session,
            tenant_a.clone(),
            "tenant a old hygiene chunk",
            None,
        );
        chunk_a.created_at = old;
        let mut chunk_b = test_vector_chunk(
            "tenant-b-hygiene",
            MemoryTier::Session,
            tenant_b.clone(),
            "tenant b old hygiene chunk",
            None,
        );
        chunk_b.created_at = old;

        db.store_chunk(&chunk_a, &embedding(0.3, 0.7))
            .await
            .unwrap();
        db.store_chunk(&chunk_b, &embedding(0.3, 0.7))
            .await
            .unwrap();

        let deleted = db.run_hygiene_for_tenant(0, &tenant_a).await.unwrap();
        assert_eq!(deleted, 1);
        assert!(db
            .get_session_chunks_for_tenant("shared-session", &tenant_a)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.get_session_chunks_for_tenant("shared-session", &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_global_memory_put_search_and_dedup() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let record = GlobalMemoryRecord {
            id: "gm-1".to_string(),
            user_id: "user-a".to_string(),
            source_type: "user_message".to_string(),
            content: "remember rust workspace layout".to_string(),
            content_hash: "h1".to_string(),
            run_id: "run-1".to_string(),
            session_id: Some("s1".to_string()),
            message_id: Some("m1".to_string()),
            tool_name: None,
            project_tag: Some("proj-x".to_string()),
            channel_tag: None,
            host_tag: None,
            metadata: None,
            provenance: None,
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let first = db.put_global_memory_record(&record).await.unwrap();
        assert!(first.stored);
        let second = db.put_global_memory_record(&record).await.unwrap();
        assert!(second.deduped);

        let hits = db
            .search_global_memory("user-a", "rust workspace", 5, Some("proj-x"), None, None)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].record.id, "gm-1");
    }

    #[tokio::test]
    async fn test_global_memory_tenant_filtered_fts_list_get_and_delete() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let tenant_a = GlobalMemoryRecord {
            id: "gm-tenant-a".to_string(),
            user_id: "same-user".to_string(),
            source_type: "note".to_string(),
            content: "shared tenant phrase".to_string(),
            content_hash: "same-hash".to_string(),
            run_id: "same-run".to_string(),
            session_id: Some("same-session".to_string()),
            message_id: Some("same-message".to_string()),
            tool_name: None,
            project_tag: Some("same-project".to_string()),
            channel_tag: None,
            host_tag: None,
            metadata: None,
            provenance: Some(serde_json::json!({
                "tenant_context": {
                    "org_id": "org-a",
                    "workspace_id": "workspace-a",
                    "source": "explicit"
                }
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let mut tenant_b = tenant_a.clone();
        tenant_b.id = "gm-tenant-b".to_string();
        tenant_b.provenance = Some(serde_json::json!({
            "tenant_context": {
                "org_id": "org-b",
                "workspace_id": "workspace-b",
                "source": "explicit"
            }
        }));

        assert!(db.put_global_memory_record(&tenant_a).await.unwrap().stored);
        assert!(db.put_global_memory_record(&tenant_b).await.unwrap().stored);

        let hits_a = db
            .search_global_memory_for_tenant(
                "org-a",
                "workspace-a",
                None,
                "same-user",
                "shared tenant phrase",
                10,
                Some("same-project"),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(hits_a.len(), 1);
        assert_eq!(hits_a[0].record.id, "gm-tenant-a");

        let rows_b = db
            .list_global_memory_for_tenant(
                "org-b",
                "workspace-b",
                None,
                "same-user",
                Some("shared tenant"),
                Some("same-project"),
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(rows_b.len(), 1);
        assert_eq!(rows_b[0].id, "gm-tenant-b");

        assert!(db
            .get_global_memory_for_tenant("gm-tenant-b", "org-a", "workspace-a", None)
            .await
            .unwrap()
            .is_none());
        assert!(!db
            .delete_global_memory_for_tenant("gm-tenant-b", "org-a", "workspace-a", None)
            .await
            .unwrap());
        assert!(db
            .delete_global_memory_for_tenant("gm-tenant-b", "org-b", "workspace-b", None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_global_memory_department_scoped_read_is_enforced_in_query() {
        // TAN-645: owner_org_unit_id is a real column with a SQL predicate. Three
        // records in the same tenant + user: Finance, Engineering, and an
        // unstamped (tenant-wide) row. A department-scoped read must return only
        // that department's rows; a tenant-only read (None) returns all three.
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let base = GlobalMemoryRecord {
            id: "dept-finance".to_string(),
            user_id: "same-user".to_string(),
            source_type: "note".to_string(),
            content: "acme quarterly context".to_string(),
            content_hash: "hash-finance".to_string(),
            run_id: "run-finance".to_string(),
            session_id: None,
            message_id: None,
            tool_name: None,
            project_tag: Some("acme".to_string()),
            channel_tag: None,
            host_tag: None,
            metadata: Some(serde_json::json!({ "owner_org_unit_id": "finance" })),
            provenance: None,
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let mut engineering = base.clone();
        engineering.id = "dept-engineering".to_string();
        engineering.content_hash = "hash-engineering".to_string();
        engineering.run_id = "run-engineering".to_string();
        engineering.metadata = Some(serde_json::json!({ "owner_org_unit_id": "engineering" }));
        let mut unstamped = base.clone();
        unstamped.id = "dept-none".to_string();
        unstamped.content_hash = "hash-none".to_string();
        unstamped.run_id = "run-none".to_string();
        unstamped.metadata = None;

        assert!(db.put_global_memory_record(&base).await.unwrap().stored);
        assert!(db
            .put_global_memory_record(&engineering)
            .await
            .unwrap()
            .stored);
        assert!(db.put_global_memory_record(&unstamped).await.unwrap().stored);

        // Department-scoped search returns only that department's row (in-query).
        let finance_hits = db
            .search_global_memory_for_tenant_scoped(
                "local",
                "local",
                None,
                "same-user",
                "acme quarterly context",
                10,
                Some("acme"),
                None,
                None,
                Some("finance"),
            )
            .await
            .unwrap();
        assert_eq!(finance_hits.len(), 1);
        assert_eq!(finance_hits[0].record.id, "dept-finance");

        // Department-scoped list likewise, and the unstamped row is NOT visible to
        // a department read (fail-closed: NULL owner_org_unit_id != 'engineering').
        let engineering_rows = db
            .list_global_memory_for_tenant_scoped(
                "local",
                "local",
                None,
                "same-user",
                Some("acme"),
                Some("acme"),
                None,
                10,
                0,
                Some("engineering"),
            )
            .await
            .unwrap();
        assert_eq!(engineering_rows.len(), 1);
        assert_eq!(engineering_rows[0].id, "dept-engineering");

        // Tenant-only read (no department narrowing) still returns all three rows.
        let all_rows = db
            .list_global_memory_for_tenant(
                "local", "local", None, "same-user", None, None, None, 10, 0,
            )
            .await
            .unwrap();
        assert_eq!(all_rows.len(), 3);

        // The MemoryStore trait seam wires scope.org_unit into the predicate.
        use crate::store::{MemoryReadScope, MemoryStore};
        use crate::types::MemoryTenantScope;
        let mut scope = MemoryReadScope::tenant(MemoryTenantScope {
            org_id: "local".to_string(),
            workspace_id: "local".to_string(),
            deployment_id: None,
        });
        scope.org_unit = Some("finance".to_string());
        let trait_hits = MemoryStore::search_global_records(
            &db,
            &scope,
            "same-user",
            "acme quarterly context",
            10,
            Some("acme"),
        )
        .await
        .unwrap();
        assert_eq!(trait_hits.len(), 1);
        assert_eq!(trait_hits[0].record.id, "dept-finance");
    }

    #[tokio::test]
    async fn test_knowledge_registry_round_trip() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-1".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::ApprovedDefault,
            metadata: Some(serde_json::json!({"owner":"marketing"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let loaded_space = db.get_knowledge_space("space-1").await.unwrap().unwrap();
        assert_eq!(loaded_space.id, "space-1");
        assert_eq!(loaded_space.scope, KnowledgeScope::Project);
        assert_eq!(loaded_space.project_id.as_deref(), Some("project-1"));
        assert_eq!(
            loaded_space.namespace.as_deref(),
            Some("marketing/positioning")
        );

        let item = KnowledgeItemRecord {
            id: "item-1".to_string(),
            space_id: "space-1".to_string(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "item-1-dedupe".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing sensitivity observation".to_string(),
            summary: Some("Customers reacted to annual pricing changes".to_string()),
            payload: serde_json::json!({"claim":"Annual pricing changes created friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/research-sources".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"source_kind":"web"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let loaded_item = db.get_knowledge_item("item-1").await.unwrap().unwrap();
        assert_eq!(loaded_item.id, "item-1");
        assert_eq!(loaded_item.space_id, "space-1");
        assert_eq!(
            loaded_item.coverage_key,
            "project-1::marketing/positioning::strategy::pricing"
        );
        assert_eq!(loaded_item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(
            loaded_item.artifact_refs,
            vec!["artifact://run-1/research-sources".to_string()]
        );

        let by_space = db.list_knowledge_items("space-1", None).await.unwrap();
        assert_eq!(by_space.len(), 1);
        let by_coverage = db
            .list_knowledge_items(
                "space-1",
                Some("project-1::marketing/positioning::strategy::pricing"),
            )
            .await
            .unwrap();
        assert_eq!(by_coverage.len(), 1);

        let coverage = KnowledgeCoverageRecord {
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            space_id: "space-1".to_string(),
            latest_item_id: Some("item-1".to_string()),
            latest_dedupe_key: Some("item-1-dedupe".to_string()),
            last_seen_at_ms: now,
            last_promoted_at_ms: Some(now),
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"reuse_reason":"same topic"})),
        };
        db.upsert_knowledge_coverage(&coverage).await.unwrap();

        let loaded_coverage = db
            .get_knowledge_coverage(
                "project-1::marketing/positioning::strategy::pricing",
                "space-1",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_coverage.space_id, "space-1");
        assert_eq!(loaded_coverage.latest_item_id.as_deref(), Some("item-1"));
        assert_eq!(
            loaded_coverage.latest_dedupe_key.as_deref(),
            Some("item-1-dedupe")
        );
    }

    #[tokio::test]
    async fn test_knowledge_promotion_working_to_promoted_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-1".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: Some("Reusable debugging guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            dedupe_key: "dedupe-promote-1".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry only after startup completed.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/debug".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 10,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("validated in workflow".to_string()),
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(result.promoted);
        assert_eq!(result.item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(result.item.trust_level, KnowledgeTrustLevel::Promoted);
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-1")
        );
        assert_eq!(
            result.coverage.latest_dedupe_key.as_deref(),
            Some("dedupe-promote-1")
        );
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 10));
    }

    #[tokio::test]
    async fn test_knowledge_promotion_promoted_to_approved_default_requires_review() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-2".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-2".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "dedupe-promote-2".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing observation".to_string(),
            summary: Some("Annual pricing changes created friction".to_string()),
            payload: serde_json::json!({"claim":"pricing friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-2".to_string()),
            artifact_refs: vec!["artifact://run-2/research".to_string()],
            source_memory_ids: vec!["memory-2".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 5,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: Some("should require review".to_string()),
        };

        let err = db.promote_knowledge_item(&promote).await.unwrap_err();
        match err {
            MemoryError::InvalidConfig(_) => {}
            other => panic!("unexpected error: {}", other),
        }
    }

    #[tokio::test]
    async fn test_knowledge_promotion_promoted_to_approved_default_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-3".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support/runbooks".to_string()),
            title: Some("Support runbooks".to_string()),
            description: Some("Reusable runbook guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-3".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::support/runbooks::oncall::restart".to_string(),
            dedupe_key: "dedupe-promote-3".to_string(),
            item_type: "runbook".to_string(),
            title: "Restart service and verify".to_string(),
            summary: Some("Restart then validate health endpoint.".to_string()),
            payload: serde_json::json!({"steps":["restart","healthcheck"]}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-3".to_string()),
            artifact_refs: vec!["artifact://run-3/runbook".to_string()],
            source_memory_ids: vec!["memory-3".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 12,
            freshness_expires_at_ms: Some(now + 172_800_000),
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("reviewed by ops".to_string()),
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(result.promoted);
        assert_eq!(result.item.status, KnowledgeItemStatus::ApprovedDefault);
        assert_eq!(
            result.item.trust_level,
            KnowledgeTrustLevel::ApprovedDefault
        );
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 12));
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-3")
        );
    }

    #[tokio::test]
    async fn test_knowledge_promotion_rejects_deprecated() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-4".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("ops".to_string()),
            title: Some("Ops knowledge".to_string()),
            description: Some("Reusable ops knowledge".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-4".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::ops::incident::latency".to_string(),
            dedupe_key: "dedupe-promote-4".to_string(),
            item_type: "decision".to_string(),
            title: "Ignore deprecated item".to_string(),
            summary: None,
            payload: serde_json::json!({"decision":"deprecated"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Deprecated,
            run_id: Some("run-4".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: None,
        };

        let err = db.promote_knowledge_item(&promote).await.unwrap_err();
        match err {
            MemoryError::InvalidConfig(_) => {}
            other => panic!("unexpected error: {}", other),
        }
    }

    #[tokio::test]
    async fn test_knowledge_promotion_idempotent_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote-5".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/ops".to_string()),
            title: Some("Engineering ops".to_string()),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote-5".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/ops::deploy::guardrails".to_string(),
            dedupe_key: "dedupe-promote-5".to_string(),
            item_type: "pattern".to_string(),
            title: "Deploy guardrails".to_string(),
            summary: None,
            payload: serde_json::json!({"pattern":"guardrails"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-5".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 20,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: None,
        };

        let result = db.promote_knowledge_item(&promote).await.unwrap().unwrap();
        assert!(!result.promoted);
        assert_eq!(result.coverage.last_promoted_at_ms, Some(now + 20));
        assert_eq!(
            result.coverage.latest_item_id.as_deref(),
            Some("item-promote-5")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_promotion_updates_coverage() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-promote".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: Some("Reusable debugging guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-promote".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            dedupe_key: "dedupe-promote".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry only after startup completes.".to_string()),
            payload: serde_json::json!({"action": "delay_retry"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-promote".to_string()),
            artifact_refs: vec!["artifact://run-promote/report".to_string()],
            source_memory_ids: vec!["memory-promote".to_string()],
            freshness_expires_at_ms: None,
            metadata: Some(serde_json::json!({"source_kind":"run"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 10,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("validated".to_string()),
        };
        let promoted = db
            .promote_knowledge_item(&request)
            .await
            .unwrap()
            .expect("promotion result");
        assert_eq!(promoted.previous_status, KnowledgeItemStatus::Working);
        assert!(promoted.promoted);
        assert_eq!(promoted.item.status, KnowledgeItemStatus::Promoted);
        assert_eq!(promoted.item.trust_level, KnowledgeTrustLevel::Promoted);
        assert_eq!(
            promoted.item.freshness_expires_at_ms,
            Some(now + 86_400_000)
        );
        assert_eq!(
            promoted
                .item
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("to_status"))
                .and_then(Value::as_str),
            Some("promoted")
        );
        assert_eq!(
            promoted.coverage.latest_item_id.as_deref(),
            Some("item-promote")
        );
        assert_eq!(
            promoted.coverage.latest_dedupe_key.as_deref(),
            Some("dedupe-promote")
        );
        assert_eq!(promoted.coverage.last_promoted_at_ms, Some(now + 10));
        assert_eq!(
            promoted.coverage.freshness_expires_at_ms,
            Some(now + 86_400_000)
        );

        let loaded = db
            .get_knowledge_item("item-promote")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, KnowledgeItemStatus::Promoted);
        assert_eq!(
            loaded
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("from_status"))
                .and_then(Value::as_str),
            Some("working")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_approved_default_requires_review() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-approved".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("marketing/positioning".to_string()),
            title: Some("Marketing positioning".to_string()),
            description: Some("Reusable positioning guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-approved".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::marketing/positioning::strategy::pricing".to_string(),
            dedupe_key: "dedupe-approved".to_string(),
            item_type: "evidence".to_string(),
            title: "Pricing sensitivity observation".to_string(),
            summary: Some("Customers reacted to annual pricing changes".to_string()),
            payload: serde_json::json!({"claim":"Annual pricing changes created friction"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-approved".to_string()),
            artifact_refs: vec!["artifact://run-approved/research".to_string()],
            source_memory_ids: vec!["memory-approved".to_string()],
            freshness_expires_at_ms: Some(now + 1234),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 20,
            freshness_expires_at_ms: Some(now + 90_000_000),
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("approved as default guidance".to_string()),
        };
        let promoted = db
            .promote_knowledge_item(&request)
            .await
            .unwrap()
            .expect("promotion result");
        assert_eq!(promoted.previous_status, KnowledgeItemStatus::Promoted);
        assert_eq!(promoted.item.status, KnowledgeItemStatus::ApprovedDefault);
        assert_eq!(
            promoted.item.trust_level,
            KnowledgeTrustLevel::ApprovedDefault
        );
        assert_eq!(promoted.coverage.last_promoted_at_ms, Some(now + 20));
        assert_eq!(
            promoted
                .item
                .metadata
                .as_ref()
                .and_then(|value| value.get("promotion"))
                .and_then(|value| value.get("approval_id"))
                .and_then(Value::as_str),
            Some("approval-1")
        );
    }

    #[tokio::test]
    async fn test_knowledge_item_promotion_rejects_invalid_transition() {
        let (db, _temp) = setup_test_db().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-invalid".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support".to_string()),
            title: Some("Support".to_string()),
            description: Some("Support guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-invalid".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1::support::workflow::triage".to_string(),
            dedupe_key: "dedupe-invalid".to_string(),
            item_type: "decision".to_string(),
            title: "Triage first".to_string(),
            summary: None,
            payload: serde_json::json!({"action":"triage"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-invalid".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: KnowledgeItemStatus::ApprovedDefault,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: None,
            reviewer_id: Some("reviewer-1".to_string()),
            approval_id: Some("approval-1".to_string()),
            reason: Some("should fail".to_string()),
        };
        let err = db.promote_knowledge_item(&request).await.unwrap_err();
        assert!(matches!(err, MemoryError::InvalidConfig(_)));
        let loaded = db.get_knowledge_item(&item.id).await.unwrap().unwrap();
        assert_eq!(loaded.status, KnowledgeItemStatus::Working);
    }
