
    async fn setup_test_db() -> (MemoryDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let db = MemoryDatabase::new(&db_path).await.unwrap();
        (db, temp_dir)
    }

    fn tenant_scope(org_id: &str, workspace_id: &str) -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: org_id.to_string(),
            workspace_id: workspace_id.to_string(),
            deployment_id: Some("deployment-1".to_string()),
        }
    }

    fn test_vector_chunk(
        id: &str,
        tier: MemoryTier,
        tenant_scope: MemoryTenantScope,
        content: &str,
        source_hash: Option<&str>,
    ) -> MemoryChunk {
        MemoryChunk {
            id: id.to_string(),
            content: content.to_string(),
            tier,
            session_id: Some("shared-session".to_string()),
            project_id: Some("shared-project".to_string()),
            source: "test_vector".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: source_hash.map(ToString::to_string),
            tenant_scope,
            subject: None,
            created_at: Utc::now(),
            token_count: 4,
            metadata: None,
        }
    }

    fn embedding(first: f32, second: f32) -> Vec<f32> {
        let mut values = vec![0.0f32; DEFAULT_EMBEDDING_DIMENSION];
        values[0] = first;
        values[1] = second;
        values
    }

    fn source_object_record(
        source_object_id: &str,
        tenant_scope: MemoryTenantScope,
    ) -> SourceObjectLifecycleRecord {
        SourceObjectLifecycleRecord {
            source_object_id: source_object_id.to_string(),
            tenant_scope,
            source_binding_id: "shared-binding".to_string(),
            connector_id: "manual_upload".to_string(),
            state: SourceObjectLifecycleState::Active,
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            import_namespace: "shared-import".to_string(),
            indexed_path: "shared-import/note.md".to_string(),
            native_object_id: "shared-import/note.md".to_string(),
            resource_ref: serde_json::json!({
                "organization_id": "org-a",
                "workspace_id": "workspace-a",
                "resource_kind": "document_collection",
                "resource_id": "shared-docs"
            }),
            data_class: "internal".to_string(),
            content_hash: Some("content-hash".to_string()),
            source_hash: Some("source-hash".to_string()),
            first_seen_at_ms: 1_000,
            last_seen_at_ms: 1_000,
            tombstoned_at_ms: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_init_schema() {
        let (db, _temp) = setup_test_db().await;
        // If we get here, schema was initialized successfully
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.total_chunks, 0);
    }

    #[tokio::test]
    async fn source_object_lifecycle_native_ids_are_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.upsert_source_object_active_for_tenant(&source_object_record(
            "source-object-a",
            tenant_a.clone(),
        ))
        .await
        .unwrap();
        db.upsert_source_object_active_for_tenant(&source_object_record(
            "source-object-b",
            tenant_b.clone(),
        ))
        .await
        .unwrap();

        let object_a = db
            .get_source_object_lifecycle_by_native_for_tenant(
                &tenant_a,
                "shared-binding",
                "shared-import/note.md",
            )
            .await
            .unwrap()
            .expect("tenant A source object");
        let object_b = db
            .get_source_object_lifecycle_by_native_for_tenant(
                &tenant_b,
                "shared-binding",
                "shared-import/note.md",
            )
            .await
            .unwrap()
            .expect("tenant B source object");

        assert_eq!(object_a.source_object_id, "source-object-a");
        assert_eq!(object_b.source_object_id, "source-object-b");
        assert_ne!(object_a.source_object_id, object_b.source_object_id);
    }

    #[tokio::test]
    async fn test_knowledge_registry_roundtrip() {
        let (db, _temp) = setup_test_db().await;

        let space = KnowledgeSpaceRecord {
            id: "space-1".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("support".to_string()),
            title: Some("Support Knowledge".to_string()),
            description: Some("Reusable support guidance".to_string()),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: Some(serde_json::json!({"owner": "ops"})),
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        db.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-1/support/debugging/slow-start".to_string(),
            dedupe_key: "dedupe-1".to_string(),
            item_type: "decision".to_string(),
            title: "Restart service before retry".to_string(),
            summary: Some("When the service is stale, restart before retrying.".to_string()),
            payload: serde_json::json!({"action": "restart"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            status: KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/report".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: Some(10),
            metadata: Some(serde_json::json!({"source": "run"})),
            created_at_ms: 3,
            updated_at_ms: 4,
        };
        db.upsert_knowledge_item(&item).await.unwrap();

        let coverage = KnowledgeCoverageRecord {
            coverage_key: item.coverage_key.clone(),
            space_id: space.id.clone(),
            latest_item_id: Some(item.id.clone()),
            latest_dedupe_key: Some(item.dedupe_key.clone()),
            last_seen_at_ms: 5,
            last_promoted_at_ms: Some(6),
            freshness_expires_at_ms: Some(10),
            metadata: Some(serde_json::json!({"coverage": true})),
        };
        db.upsert_knowledge_coverage(&coverage).await.unwrap();

        let loaded_space = db.get_knowledge_space(&space.id).await.unwrap().unwrap();
        assert_eq!(loaded_space.namespace.as_deref(), Some("support"));

        let loaded_items = db
            .list_knowledge_items(&space.id, Some(&item.coverage_key))
            .await
            .unwrap();
        assert_eq!(loaded_items.len(), 1);
        assert_eq!(loaded_items[0].title, item.title);

        let loaded_coverage = db
            .get_knowledge_coverage(&item.coverage_key, &space.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_coverage.latest_item_id.as_deref(), Some("item-1"));
    }

    #[tokio::test]
    async fn test_knowledge_registry_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        let mut space_a = KnowledgeSpaceRecord {
            id: "tenant-a-space".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("shared-project".to_string()),
            namespace: Some("shared-namespace".to_string()),
            title: Some("Tenant A Knowledge".to_string()),
            description: None,
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let mut space_b = space_a.clone();
        space_b.id = "tenant-b-space".to_string();
        space_b.title = Some("Tenant B Knowledge".to_string());

        db.upsert_knowledge_space_for_tenant(&space_a, &tenant_a)
            .await
            .unwrap();
        db.upsert_knowledge_space_for_tenant(&space_b, &tenant_b)
            .await
            .unwrap();

        let spaces_a = db
            .list_knowledge_spaces_for_tenant(Some("shared-project"), &tenant_a)
            .await
            .unwrap();
        let spaces_b = db
            .list_knowledge_spaces_for_tenant(Some("shared-project"), &tenant_b)
            .await
            .unwrap();
        assert_eq!(spaces_a.len(), 1);
        assert_eq!(spaces_a[0].id, "tenant-a-space");
        assert_eq!(spaces_b.len(), 1);
        assert_eq!(spaces_b[0].id, "tenant-b-space");
        assert!(db
            .get_knowledge_space_for_tenant("tenant-b-space", &tenant_a)
            .await
            .unwrap()
            .is_none());

        let item_b = KnowledgeItemRecord {
            id: "tenant-b-item".to_string(),
            space_id: space_b.id.clone(),
            coverage_key: "shared-project/topic/debugging".to_string(),
            dedupe_key: "shared-dedupe".to_string(),
            item_type: "decision".to_string(),
            title: "Tenant B item".to_string(),
            summary: None,
            payload: serde_json::json!({"tenant": "b"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("run-b".to_string()),
            artifact_refs: Vec::new(),
            source_memory_ids: Vec::new(),
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: 3,
            updated_at_ms: 4,
        };
        db.upsert_knowledge_item_for_tenant(&item_b, &tenant_b)
            .await
            .unwrap();
        assert!(db
            .upsert_knowledge_item_for_tenant(&item_b, &tenant_a)
            .await
            .is_err());
        assert!(db
            .get_knowledge_item_for_tenant("tenant-b-item", &tenant_a)
            .await
            .unwrap()
            .is_none());
        assert!(db
            .list_knowledge_items_for_tenant(&space_b.id, None, &tenant_a)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.list_knowledge_items_for_tenant(&space_b.id, None, &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );

        let coverage_b = KnowledgeCoverageRecord {
            coverage_key: item_b.coverage_key.clone(),
            space_id: space_b.id.clone(),
            latest_item_id: Some(item_b.id.clone()),
            latest_dedupe_key: Some(item_b.dedupe_key.clone()),
            last_seen_at_ms: 5,
            last_promoted_at_ms: None,
            freshness_expires_at_ms: None,
            metadata: None,
        };
        db.upsert_knowledge_coverage_for_tenant(&coverage_b, &tenant_b)
            .await
            .unwrap();
        assert!(db
            .upsert_knowledge_coverage_for_tenant(&coverage_b, &tenant_a)
            .await
            .is_err());
        assert!(db
            .get_knowledge_coverage_for_tenant(&coverage_b.coverage_key, &space_b.id, &tenant_a)
            .await
            .unwrap()
            .is_none());
        assert!(db
            .get_knowledge_coverage_for_tenant(&coverage_b.coverage_key, &space_b.id, &tenant_b)
            .await
            .unwrap()
            .is_some());

        let promote = KnowledgePromotionRequest {
            item_id: item_b.id.clone(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: 10,
            freshness_expires_at_ms: None,
            reviewer_id: None,
            approval_id: None,
            reason: None,
        };
        assert!(db
            .promote_knowledge_item_for_tenant(&promote, &tenant_a)
            .await
            .unwrap()
            .is_none());
        assert!(db
            .promote_knowledge_item_for_tenant(&promote, &tenant_b)
            .await
            .unwrap()
            .is_some());

        space_a.updated_at_ms = 11;
        db.upsert_knowledge_space_for_tenant(&space_a, &tenant_a)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_store_and_retrieve_chunk() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "test-1".to_string(),
            content: "Test content".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("session-1".to_string()),
            project_id: Some("project-1".to_string()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            subject: None,
            created_at: Utc::now(),
            token_count: 10,
            metadata: None,
        };

        let embedding = vec![0.1f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        let chunks = db.get_session_chunks("session-1").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Test content");
    }

    #[tokio::test]
    async fn chunk_content_is_ciphertext_at_rest_with_local_key() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("encrypted_memory.db");
        let db = MemoryDatabase::new(&path)
            .await
            .unwrap()
            .with_crypto_provider(crate::crypto::MemoryCryptoProvider::local_key([5u8; 32]));

        let chunk = MemoryChunk {
            id: "enc-1".to_string(),
            content: "tenant secret launch plan".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("session-enc".to_string()),
            project_id: None,
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            subject: None,
            created_at: Utc::now(),
            token_count: 5,
            metadata: Some(serde_json::json!({"classification": "confidential"})),
        };
        let embedding = vec![0.1f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        // Raw DB read must NOT expose plaintext content or metadata.
        {
            let conn = db.conn.lock().await;
            let raw_content: String = conn
                .query_row(
                    "SELECT content FROM session_memory_chunks WHERE id = ?1",
                    params!["enc-1"],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                raw_content.starts_with("tce1:"),
                "content stored as ciphertext"
            );
            assert!(!raw_content.contains("launch plan"));

            let raw_metadata: String = conn
                .query_row(
                    "SELECT metadata FROM session_memory_chunks WHERE id = ?1",
                    params!["enc-1"],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                raw_metadata.starts_with("tce1:"),
                "metadata stored as ciphertext"
            );
            assert!(!raw_metadata.contains("confidential"));
        }

        // Authorized read through the provider transparently decrypts.
        let chunks = db.get_session_chunks("session-enc").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "tenant secret launch plan");
        assert_eq!(
            chunks[0]
                .metadata
                .as_ref()
                .and_then(|m| m.get("classification"))
                .and_then(|v| v.as_str()),
            Some("confidential")
        );

        // A different key cannot read the ciphertext (cross-key isolation).
        let other = MemoryDatabase::new(&path)
            .await
            .unwrap()
            .with_crypto_provider(crate::crypto::MemoryCryptoProvider::local_key([6u8; 32]));
        assert!(other.get_session_chunks("session-enc").await.is_err());
    }

    #[tokio::test]
    async fn test_store_and_retrieve_global_chunk() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "global-1".to_string(),
            content: "Global note".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "agent_note".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            subject: None,
            created_at: Utc::now(),
            token_count: 7,
            metadata: Some(serde_json::json!({"kind":"test"})),
        };

        let embedding = vec![0.2f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        let chunks = db.get_global_chunks(10).await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Global note");
        assert_eq!(chunks[0].source, "agent_note");
        assert_eq!(chunks[0].token_count, 7);
        assert_eq!(chunks[0].tier, MemoryTier::Global);
    }

    #[tokio::test]
    async fn test_global_chunk_exists_by_source_hash() {
        let (db, _temp) = setup_test_db().await;

        let chunk = MemoryChunk {
            id: "global-hash".to_string(),
            content: "Global hash note".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "chat_exchange".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("hash-123".to_string()),
            tenant_scope: MemoryTenantScope::local(),
            subject: None,
            created_at: Utc::now(),
            token_count: 5,
            metadata: None,
        };

        let embedding = vec![0.3f32; DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding).await.unwrap();

        assert!(db
            .global_chunk_exists_by_source_hash("hash-123")
            .await
            .unwrap());
        assert!(!db
            .global_chunk_exists_by_source_hash("missing-hash")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_vector_search_is_tenant_partitioned_before_top_k() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let query = embedding(1.0, 0.0);

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-vector",
                MemoryTier::Project,
                tenant_a.clone(),
                "tenant a memory",
                None,
            ),
            &embedding(0.8, 0.2),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-vector",
                MemoryTier::Project,
                tenant_b.clone(),
                "tenant b closer memory",
                None,
            ),
            &query,
        )
        .await
        .unwrap();

        let results = db
            .search_similar_for_tenant(
                &query,
                MemoryTier::Project,
                Some("shared-project"),
                None,
                &tenant_a,
                1,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.id, "tenant-a-vector");
        assert_eq!(results[0].0.tenant_scope, tenant_a);
    }

    #[tokio::test]
    async fn test_identical_vector_content_only_returns_request_tenant() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let vector = embedding(0.4, 0.6);

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-identical",
                MemoryTier::Global,
                tenant_a.clone(),
                "identical memory body",
                Some("same-source-hash"),
            ),
            &vector,
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-identical",
                MemoryTier::Global,
                tenant_b,
                "identical memory body",
                Some("same-source-hash"),
            ),
            &vector,
        )
        .await
        .unwrap();

        let results = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &tenant_a,
                10,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.id, "tenant-a-identical");
    }

    #[tokio::test]
    async fn test_vector_chunk_department_scope_is_stamped_and_enforced_in_query() {
        let (db, _temp) = setup_test_db().await;
        let tenant = tenant_scope("org-a", "workspace-a");
        let vector = embedding(0.4, 0.6);

        let mut finance = test_vector_chunk(
            "finance-vector",
            MemoryTier::Global,
            tenant.clone(),
            "finance acme context",
            None,
        );
        finance.metadata = Some(serde_json::json!({ "owner_org_unit_id": "finance" }));

        let mut engineering = test_vector_chunk(
            "engineering-vector",
            MemoryTier::Global,
            tenant.clone(),
            "engineering acme context",
            None,
        );
        engineering.metadata = Some(serde_json::json!({ "owner_org_unit_id": "engineering" }));

        let unstamped = test_vector_chunk(
            "unstamped-vector",
            MemoryTier::Global,
            tenant.clone(),
            "unstamped acme context",
            None,
        );

        db.store_chunk(&finance, &vector).await.unwrap();
        db.store_chunk(&engineering, &vector).await.unwrap();
        db.store_chunk(&unstamped, &vector).await.unwrap();

        let stored_owner: Option<String> = {
            let conn = db.conn.lock().await;
            conn.query_row(
                "SELECT owner_org_unit_id FROM global_memory_chunks WHERE id = ?1",
                params!["finance-vector"],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(stored_owner.as_deref(), Some("finance"));

        let finance_results = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &tenant,
                10,
                Some("user-a"),
                Some("finance"),
            )
            .await
            .unwrap();
        assert_eq!(finance_results.len(), 1);
        assert_eq!(finance_results[0].0.id, "finance-vector");

        use crate::store::{
            MemoryChunkSelector, MemoryReadScope, MemoryStore, MemoryStoreQueryRequest,
            MemoryStoreQueryResult,
        };
        let mut scope = MemoryReadScope::tenant(tenant.clone());
        scope.subject = Some("user-a".to_string());
        scope.org_unit = Some("engineering".to_string());
        let trait_result = MemoryStore::query(
            &db,
            MemoryStoreQueryRequest::SimilarChunks {
                scope: scope.clone(),
                selector: MemoryChunkSelector::global(),
                query_embedding: vector.clone(),
                limit: 10,
            },
        )
        .await
        .unwrap();
        let MemoryStoreQueryResult::SimilarChunks(trait_results) = trait_result else {
            panic!("unexpected memory store query result");
        };
        assert_eq!(trait_results.len(), 1);
        assert_eq!(trait_results[0].0.id, "engineering-vector");

        let mut private_without_department = test_vector_chunk(
            "private-without-department",
            MemoryTier::Global,
            tenant.clone(),
            "private user context",
            None,
        );
        private_without_department.subject = Some("user-a".to_string());
        db.store_chunk(&private_without_department, &vector)
            .await
            .unwrap();

        let mut tenant_shared = test_vector_chunk(
            "tenant-shared-vector",
            MemoryTier::Global,
            scope.tenant.clone(),
            "shared tenant context",
            None,
        );
        tenant_shared.metadata = Some(serde_json::json!({ "tenant_shared": true }));
        db.store_chunk(&tenant_shared, &vector).await.unwrap();
        let stored_tenant_shared: i64 = {
            let conn = db.conn.lock().await;
            conn.query_row(
                "SELECT tenant_shared FROM global_memory_chunks WHERE id = ?1",
                params!["tenant-shared-vector"],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(stored_tenant_shared, 1);

        let finance_results = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &scope.tenant,
                10,
                Some("user-a"),
                Some("finance"),
            )
            .await
            .unwrap();
        let result_ids = finance_results
            .iter()
            .map(|(chunk, _)| chunk.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(result_ids.contains("finance-vector"));
        // Private ownership is additive to department scope. A subject match
        // cannot make an unstamped row visible inside a department query.
        assert!(!result_ids.contains("private-without-department"));
        assert!(result_ids.contains("tenant-shared-vector"));
        assert!(!result_ids.contains("engineering-vector"));
        assert!(!result_ids.contains("unstamped-vector"));
    }

    #[tokio::test]
    async fn test_session_tier_search_does_not_leak_across_tenants() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let vector = embedding(0.7, 0.3);

        // Both tenants share the same session_id ("shared-session" from the
        // chunk helper) so only the tenant clause separates them.
        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-session",
                MemoryTier::Session,
                tenant_a.clone(),
                "tenant a session memory",
                None,
            ),
            &vector,
        )
        .await
        .unwrap();

        let foreign = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Session,
                None,
                Some("shared-session"),
                &tenant_b,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(
            foreign.is_empty(),
            "tenant B must not see tenant A session chunks"
        );

        let own = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Session,
                None,
                Some("shared-session"),
                &tenant_a,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].0.id, "tenant-a-session");
    }

    #[tokio::test]
    async fn test_search_isolates_deployments_within_same_org_workspace() {
        let (db, _temp) = setup_test_db().await;
        let deployment_one = tenant_scope("org-a", "workspace-a");
        let deployment_two = MemoryTenantScope {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: Some("deployment-2".to_string()),
        };
        let no_deployment = MemoryTenantScope {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: None,
        };
        let vector = embedding(0.5, 0.5);

        db.store_chunk(
            &test_vector_chunk(
                "deployment-one-chunk",
                MemoryTier::Global,
                deployment_one.clone(),
                "deployment one memory",
                None,
            ),
            &vector,
        )
        .await
        .unwrap();

        let cross_deployment = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &deployment_two,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(
            cross_deployment.is_empty(),
            "a different deployment in the same org/workspace must not match"
        );

        let missing_deployment = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &no_deployment,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(
            missing_deployment.is_empty(),
            "a scope without deployment_id must not match deployment-scoped rows"
        );

        let own = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &deployment_one,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].0.id, "deployment-one-chunk");
    }

    #[tokio::test]
    async fn test_strict_mode_denies_local_scope_reads_and_writes() {
        let (db, _temp) = setup_test_db().await;
        let local_scope = MemoryTenantScope::local();
        let vector = embedding(0.3, 0.7);

        // Default (local single-tenant) mode: local scope works.
        db.store_chunk(
            &test_vector_chunk(
                "local-chunk",
                MemoryTier::Global,
                local_scope.clone(),
                "local memory",
                None,
            ),
            &vector,
        )
        .await
        .expect("local scope writes succeed before strict mode is enabled");

        db.set_strict_tenant_enforcement(true);

        let read_err = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &local_scope,
                10,
                None,
                None,
            )
            .await
            .expect_err("strict mode must deny local-scope reads");
        assert!(matches!(read_err, MemoryError::TenantScopeViolation(_)));

        let write_err = db
            .store_chunk(
                &test_vector_chunk(
                    "local-chunk-2",
                    MemoryTier::Global,
                    local_scope.clone(),
                    "local memory two",
                    None,
                ),
                &vector,
            )
            .await
            .expect_err("strict mode must deny local-scope writes");
        assert!(matches!(write_err, MemoryError::TenantScopeViolation(_)));

        // Explicit tenants remain unaffected by strict mode.
        let tenant_a = tenant_scope("org-a", "workspace-a");
        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-strict",
                MemoryTier::Global,
                tenant_a.clone(),
                "tenant a strict memory",
                None,
            ),
            &vector,
        )
        .await
        .expect("explicit tenant writes succeed in strict mode");
        let results = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &tenant_a,
                10,
                None,
                None,
            )
            .await
            .expect("explicit tenant reads succeed in strict mode");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.id, "tenant-a-strict");
    }

    #[tokio::test]
    async fn test_tenant_delete_does_not_remove_other_tenant_vector_memory() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let vector = embedding(0.2, 0.8);

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-delete",
                MemoryTier::Global,
                tenant_a.clone(),
                "tenant a delete target",
                None,
            ),
            &vector,
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-delete",
                MemoryTier::Global,
                tenant_b.clone(),
                "tenant b delete target",
                None,
            ),
            &vector,
        )
        .await
        .unwrap();

        let cross_delete = db
            .delete_chunk_for_tenant(MemoryTier::Global, "tenant-b-delete", None, None, &tenant_a)
            .await
            .unwrap();
        assert_eq!(cross_delete, 0);

        let tenant_b_results = db
            .search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &tenant_b,
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(tenant_b_results.len(), 1);
        assert_eq!(tenant_b_results[0].0.id, "tenant-b-delete");

        let own_delete = db
            .delete_chunk_for_tenant(MemoryTier::Global, "tenant-a-delete", None, None, &tenant_a)
            .await
            .unwrap();
        assert_eq!(own_delete, 1);
        assert_eq!(
            db.search_similar_for_tenant(
                &vector,
                MemoryTier::Global,
                None,
                None,
                &tenant_b,
                10,
                None,
                None,
            )
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_same_source_hash_does_not_dedupe_across_tenants() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let source_hash = "shared-source-hash";

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-hash",
                MemoryTier::Global,
                tenant_a.clone(),
                "same source hash",
                Some(source_hash),
            ),
            &embedding(0.7, 0.1),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-hash",
                MemoryTier::Global,
                tenant_b.clone(),
                "same source hash",
                Some(source_hash),
            ),
            &embedding(0.7, 0.1),
        )
        .await
        .unwrap();

        assert!(db
            .global_chunk_exists_by_source_hash_for_tenant(source_hash, &tenant_a)
            .await
            .unwrap());
        assert!(db
            .global_chunk_exists_by_source_hash_for_tenant(source_hash, &tenant_b)
            .await
            .unwrap());

        let tenant_a_chunks = db
            .get_global_chunks_for_tenant(&tenant_a, 10)
            .await
            .unwrap();
        let tenant_b_chunks = db
            .get_global_chunks_for_tenant(&tenant_b, 10)
            .await
            .unwrap();
        assert_eq!(tenant_a_chunks.len(), 1);
        assert_eq!(tenant_b_chunks.len(), 1);
        assert_ne!(tenant_a_chunks[0].id, tenant_b_chunks[0].id);
    }

    #[tokio::test]
    async fn test_memory_stats_are_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-session-stat",
                MemoryTier::Session,
                tenant_a.clone(),
                "tenant a session stats",
                None,
            ),
            &embedding(0.1, 0.2),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-project-stat",
                MemoryTier::Project,
                tenant_a.clone(),
                "tenant a project stats",
                None,
            ),
            &embedding(0.2, 0.3),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-global-stat",
                MemoryTier::Global,
                tenant_a.clone(),
                "tenant a global stats",
                None,
            ),
            &embedding(0.3, 0.4),
        )
        .await
        .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-project-stat",
                MemoryTier::Project,
                tenant_b.clone(),
                "tenant b project stats should not count",
                None,
            ),
            &embedding(0.4, 0.5),
        )
        .await
        .unwrap();

        db.log_cleanup_for_tenant(
            "test",
            MemoryTier::Project,
            Some("shared-project"),
            None,
            1,
            10,
            &tenant_b,
        )
        .await
        .unwrap();

        let tenant_a_stats = db.get_stats_for_tenant(&tenant_a).await.unwrap();
        assert_eq!(tenant_a_stats.session_chunks, 1);
        assert_eq!(tenant_a_stats.project_chunks, 1);
        assert_eq!(tenant_a_stats.global_chunks, 1);
        assert_eq!(tenant_a_stats.total_chunks, 3);
        assert!(tenant_a_stats.total_bytes > 0);
        assert!(tenant_a_stats.last_cleanup.is_none());

        let tenant_b_stats = db.get_stats_for_tenant(&tenant_b).await.unwrap();
        assert_eq!(tenant_b_stats.session_chunks, 0);
        assert_eq!(tenant_b_stats.project_chunks, 1);
        assert_eq!(tenant_b_stats.global_chunks, 0);
        assert_eq!(tenant_b_stats.total_chunks, 1);
        assert!(tenant_b_stats.last_cleanup.is_some());
    }

    #[tokio::test]
    async fn test_project_stats_are_tenant_scoped_for_vector_chunks() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.store_chunk(
            &test_vector_chunk(
                "tenant-a-project-stat-1",
                MemoryTier::Project,
                tenant_a.clone(),
                "tenant a project stat one",
                None,
            ),
            &embedding(0.5, 0.1),
        )
        .await
        .unwrap();
        let mut tenant_a_file = test_vector_chunk(
            "tenant-a-project-file-stat",
            MemoryTier::Project,
            tenant_a.clone(),
            "tenant a file stat",
            None,
        );
        tenant_a_file.source = "file".to_string();
        db.store_chunk(&tenant_a_file, &embedding(0.6, 0.1))
            .await
            .unwrap();
        db.store_chunk(
            &test_vector_chunk(
                "tenant-b-project-stat-1",
                MemoryTier::Project,
                tenant_b,
                "tenant b project stat",
                None,
            ),
            &embedding(0.7, 0.1),
        )
        .await
        .unwrap();

        let stats = db
            .get_project_stats_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap();
        assert_eq!(stats.project_chunks, 2);
        assert_eq!(stats.file_index_chunks, 1);
        assert!(stats.project_bytes > 0);
        assert!(stats.file_index_bytes > 0);
    }

    #[tokio::test]
    async fn test_import_index_paths_are_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        db.upsert_import_index_entry_for_tenant(
            MemoryTier::Project,
            None,
            Some("shared-project"),
            "repo/README.md",
            10,
            100,
            "hash-a",
            &tenant_a,
        )
        .await
        .unwrap();
        db.upsert_import_index_entry_for_tenant(
            MemoryTier::Project,
            None,
            Some("shared-project"),
            "repo/README.md",
            20,
            200,
            "hash-b",
            &tenant_b,
        )
        .await
        .unwrap();

        let tenant_a_paths = db
            .list_import_index_paths_for_tenant(
                MemoryTier::Project,
                None,
                Some("shared-project"),
                &tenant_a,
            )
            .await
            .unwrap();
        assert_eq!(tenant_a_paths, vec!["repo/README.md".to_string()]);

        let tenant_a_entry = db
            .get_import_index_entry_for_tenant(
                MemoryTier::Project,
                None,
                Some("shared-project"),
                "repo/README.md",
                &tenant_a,
            )
            .await
            .unwrap()
            .unwrap();
        let tenant_b_entry = db
            .get_import_index_entry_for_tenant(
                MemoryTier::Project,
                None,
                Some("shared-project"),
                "repo/README.md",
                &tenant_b,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tenant_a_entry.2, "hash-a");
        assert_eq!(tenant_b_entry.2, "hash-b");
    }

    #[tokio::test]
    async fn test_delete_import_index_entry_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        for (tenant, hash) in [(&tenant_a, "hash-a"), (&tenant_b, "hash-b")] {
            db.upsert_import_index_entry_for_tenant(
                MemoryTier::Global,
                None,
                None,
                "shared/path.md",
                1,
                10,
                hash,
                tenant,
            )
            .await
            .unwrap();
        }

        db.delete_import_index_entry_for_tenant(
            MemoryTier::Global,
            None,
            None,
            "shared/path.md",
            &tenant_a,
        )
        .await
        .unwrap();

        assert!(db
            .get_import_index_entry_for_tenant(
                MemoryTier::Global,
                None,
                None,
                "shared/path.md",
                &tenant_a
            )
            .await
            .unwrap()
            .is_none());
        let tenant_b_entry = db
            .get_import_index_entry_for_tenant(
                MemoryTier::Global,
                None,
                None,
                "shared/path.md",
                &tenant_b,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tenant_b_entry.2, "hash-b");
    }

    #[tokio::test]
    async fn test_file_chunk_delete_by_path_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        let mut chunk_a = test_vector_chunk(
            "tenant-a-file-delete",
            MemoryTier::Project,
            tenant_a.clone(),
            "same file content",
            Some("same-hash"),
        );
        chunk_a.source = "file".to_string();
        chunk_a.source_path = Some("repo/file.md".to_string());
        let mut chunk_b = test_vector_chunk(
            "tenant-b-file-delete",
            MemoryTier::Project,
            tenant_b.clone(),
            "same file content",
            Some("same-hash"),
        );
        chunk_b.source = "file".to_string();
        chunk_b.source_path = Some("repo/file.md".to_string());

        db.store_chunk(&chunk_a, &embedding(0.1, 0.2))
            .await
            .unwrap();
        db.store_chunk(&chunk_b, &embedding(0.1, 0.2))
            .await
            .unwrap();

        let (deleted, _) = db
            .delete_file_chunks_by_path_for_tenant(
                MemoryTier::Project,
                None,
                Some("shared-project"),
                "repo/file.md",
                &tenant_a,
            )
            .await
            .unwrap();
        assert_eq!(deleted, 1);

        assert!(db
            .get_project_chunks_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap()
            .is_empty());
        let tenant_b_chunks = db
            .get_project_chunks_for_tenant("shared-project", &tenant_b)
            .await
            .unwrap();
        assert_eq!(tenant_b_chunks.len(), 1);
        assert_eq!(tenant_b_chunks[0].id, "tenant-b-file-delete");
    }

    #[tokio::test]
    async fn test_project_file_index_clear_is_tenant_scoped() {
        let (db, _temp) = setup_test_db().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");

        for (tenant, id, hash) in [
            (&tenant_a, "tenant-a-clear-file-index", "hash-a"),
            (&tenant_b, "tenant-b-clear-file-index", "hash-b"),
        ] {
            db.upsert_file_index_entry_for_tenant(
                "shared-project",
                "repo/file.md",
                1,
                10,
                hash,
                tenant,
            )
            .await
            .unwrap();
            db.upsert_project_index_status_for_tenant("shared-project", 5, 4, 3, 2, 1, tenant)
                .await
                .unwrap();
            let mut chunk = test_vector_chunk(
                id,
                MemoryTier::Project,
                tenant.clone(),
                "file index clear content",
                Some(hash),
            );
            chunk.source = "file".to_string();
            chunk.source_path = Some("repo/file.md".to_string());
            db.store_chunk(&chunk, &embedding(0.4, 0.5)).await.unwrap();
        }

        let result = db
            .clear_project_file_index_for_tenant("shared-project", false, &tenant_a)
            .await
            .unwrap();
        assert_eq!(result.chunks_deleted, 1);

        assert_eq!(
            db.project_file_index_count_for_tenant("shared-project", &tenant_a)
                .await
                .unwrap(),
            0
        );
        assert!(db
            .get_project_chunks_for_tenant("shared-project", &tenant_a)
            .await
            .unwrap()
            .is_empty());

        assert_eq!(
            db.project_file_index_count_for_tenant("shared-project", &tenant_b)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.get_project_chunks_for_tenant("shared-project", &tenant_b)
                .await
                .unwrap()
                .len(),
            1
        );
        let tenant_b_stats = db
            .get_project_stats_for_tenant("shared-project", &tenant_b)
            .await
            .unwrap();
        assert_eq!(tenant_b_stats.last_indexed_files, Some(3));
    }
