#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DEFAULT_EMBEDDING_DIMENSION;
    use tandem_orchestrator::{
        KnowledgeBinding, KnowledgePreflightRequest, KnowledgeReuseDecision, KnowledgeReuseMode,
        KnowledgeScope, KnowledgeTrustLevel,
    };
    use tempfile::TempDir;

    fn is_embeddings_disabled(err: &crate::types::MemoryError) -> bool {
        matches!(err, crate::types::MemoryError::Embedding(msg) if msg.to_ascii_lowercase().contains("embeddings disabled"))
    }

    async fn setup_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let manager = MemoryManager::new(&db_path).await.unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let (manager, _temp) = setup_test_manager().await;

        let request = StoreMessageRequest {
            content: "This is a test message about artificial intelligence and machine learning."
                .to_string(),
            tier: MemoryTier::Project,
            session_id: Some("session-1".to_string()),
            project_id: Some("project-1".to_string()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            metadata: None,
        };

        let chunk_ids = match manager.store_message(request).await {
            Ok(ids) => ids,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("store_message failed: {err}"),
        };
        assert!(!chunk_ids.is_empty());

        // Search for the content
        let results = manager
            .search(
                "artificial intelligence",
                None,
                Some("project-1"),
                None,
                None,
            )
            .await;
        let results = match results {
            Ok(results) => results,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("search failed: {err}"),
        };

        assert!(!results.is_empty());
        // Similarity can be 0.0 with random hash embeddings (orthogonal or negative correlation)
        assert!(results[0].similarity >= 0.0);
    }

    #[tokio::test]
    async fn test_source_bound_search_requires_matching_resource_grant() {
        let (manager, _temp) = setup_test_manager().await;
        let tenant_scope = MemoryTenantScope {
            org_id: "acme".to_string(),
            workspace_id: "hq".to_string(),
            deployment_id: Some("dep-1".to_string()),
        };
        let hr_resource = tandem_enterprise_contract::ResourceRef::new(
            "acme",
            "hq",
            tandem_enterprise_contract::ResourceKind::DocumentCollection,
            "hr-handbook",
        );
        let engineering_resource = tandem_enterprise_contract::ResourceRef::new(
            "acme",
            "hq",
            tandem_enterprise_contract::ResourceKind::DocumentCollection,
            "engineering-handbook",
        );
        let source_bound_metadata = serde_json::json!({
            "enterprise_source_binding": {
                "binding_id": "binding-hr",
                "connector_id": "manual-upload",
                "resource_ref": hr_resource,
                "data_class": "financial_record",
                "source_object_id": "source-object-hr",
                "native_object_id": "/imports/hr/payroll.md",
                "content_hash": "hash-hr"
            }
        });

        let unbound_request = StoreMessageRequest {
            content: "General onboarding has cafeteria hours and building access notes."
                .to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("hash-general".to_string()),
            tenant_scope: tenant_scope.clone(),
            metadata: None,
        };
        let bound_request = StoreMessageRequest {
            content: "Payroll compensation bands are restricted to the HR finance process."
                .to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("hash-hr".to_string()),
            tenant_scope: tenant_scope.clone(),
            metadata: Some(source_bound_metadata),
        };

        if let Err(err) = manager.store_message(unbound_request).await {
            if is_embeddings_disabled(&err) {
                return;
            }
            panic!("store unbound message failed: {err}");
        }
        if let Err(err) = manager.store_message(bound_request).await {
            if is_embeddings_disabled(&err) {
                return;
            }
            panic!("store bound message failed: {err}");
        }

        let unfiltered = manager
            .search_for_tenant(
                "payroll compensation cafeteria",
                Some(MemoryTier::Global),
                None,
                None,
                &tenant_scope,
                Some(10),
            )
            .await
            .expect("unfiltered search");
        assert!(
            unfiltered
                .iter()
                .all(|hit| !hit.chunk.content.contains("Payroll compensation")),
            "source-bound chunks must be hidden without an enterprise access filter"
        );

        let engineering_filter = crate::types::MemoryAccessFilter::strict(
            strict_context_for_resource(
                &tenant_scope,
                engineering_resource,
                tandem_enterprise_contract::DataClass::FinancialRecord,
            ),
            chrono::Utc::now().timestamp_millis() as u64,
        );
        let denied = manager
            .search_for_tenant_with_access_filter(
                "payroll compensation",
                Some(MemoryTier::Global),
                None,
                None,
                &tenant_scope,
                Some(10),
                Some(&engineering_filter),
            )
            .await
            .expect("denied filtered search");
        assert!(
            denied
                .iter()
                .all(|hit| !hit.chunk.content.contains("Payroll compensation")),
            "a grant for a different resource must not reveal the bound chunk"
        );

        let hr_filter = crate::types::MemoryAccessFilter::strict(
            strict_context_for_resource(
                &tenant_scope,
                hr_resource,
                tandem_enterprise_contract::DataClass::FinancialRecord,
            ),
            chrono::Utc::now().timestamp_millis() as u64,
        );
        let allowed = manager
            .search_for_tenant_with_access_filter(
                "payroll compensation",
                Some(MemoryTier::Global),
                None,
                None,
                &tenant_scope,
                Some(10),
                Some(&hr_filter),
            )
            .await
            .expect("allowed filtered search");
        assert!(
            allowed
                .iter()
                .any(|hit| hit.chunk.content.contains("Payroll compensation")),
            "a matching resource/data-class grant should reveal the source-bound chunk"
        );
    }

    #[tokio::test]
    async fn tenant_a_cannot_search_tenant_b_source_bound_chunks() {
        let (manager, _temp) = setup_test_manager().await;
        let tenant_a = MemoryTenantScope {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: Some("dep-a".to_string()),
        };
        let tenant_b = MemoryTenantScope {
            org_id: "org-b".to_string(),
            workspace_id: "workspace-b".to_string(),
            deployment_id: Some("dep-b".to_string()),
        };
        let resource_a = tandem_enterprise_contract::ResourceRef::new(
            "org-a",
            "workspace-a",
            tandem_enterprise_contract::ResourceKind::DocumentCollection,
            "finance-docs",
        );
        let resource_b = tandem_enterprise_contract::ResourceRef::new(
            "org-b",
            "workspace-b",
            tandem_enterprise_contract::ResourceKind::DocumentCollection,
            "finance-docs",
        );
        let shared_phrase = "cross tenant payroll source binding needle";

        let tenant_a_request = StoreMessageRequest {
            content: format!("{shared_phrase} belongs only to tenant A."),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("tenant-a-source-hash".to_string()),
            tenant_scope: tenant_a.clone(),
            metadata: Some(serde_json::json!({
                "enterprise_source_binding": {
                    "binding_id": "binding-finance",
                    "connector_id": "manual-upload",
                    "resource_ref": resource_a,
                    "data_class": "financial_record",
                    "source_object_id": "source-object-tenant-a",
                    "native_object_id": "shared-import/payroll.md",
                    "content_hash": "tenant-a-content-hash"
                }
            })),
        };
        let tenant_b_request = StoreMessageRequest {
            content: format!("{shared_phrase} belongs only to tenant B."),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("tenant-b-source-hash".to_string()),
            tenant_scope: tenant_b.clone(),
            metadata: Some(serde_json::json!({
                "enterprise_source_binding": {
                    "binding_id": "binding-finance",
                    "connector_id": "manual-upload",
                    "resource_ref": resource_b,
                    "data_class": "financial_record",
                    "source_object_id": "source-object-tenant-b",
                    "native_object_id": "shared-import/payroll.md",
                    "content_hash": "tenant-b-content-hash"
                }
            })),
        };

        if let Err(err) = manager.store_message(tenant_a_request).await {
            if is_embeddings_disabled(&err) {
                return;
            }
            panic!("store tenant A message failed: {err}");
        }
        if let Err(err) = manager.store_message(tenant_b_request).await {
            if is_embeddings_disabled(&err) {
                return;
            }
            panic!("store tenant B message failed: {err}");
        }

        let unfiltered = manager
            .search_for_tenant(
                shared_phrase,
                Some(MemoryTier::Global),
                None,
                None,
                &tenant_a,
                Some(10),
            )
            .await
            .expect("tenant A unfiltered search");
        assert!(
            unfiltered.is_empty(),
            "source-bound chunks must stay hidden without a strict tenant projection"
        );

        let tenant_a_filter = crate::types::MemoryAccessFilter::strict(
            strict_context_for_resource(
                &tenant_a,
                tandem_enterprise_contract::ResourceRef::new(
                    "org-a",
                    "workspace-a",
                    tandem_enterprise_contract::ResourceKind::DocumentCollection,
                    "finance-docs",
                ),
                tandem_enterprise_contract::DataClass::FinancialRecord,
            ),
            chrono::Utc::now().timestamp_millis() as u64,
        );
        let tenant_a_results = manager
            .search_for_tenant_with_access_filter(
                shared_phrase,
                Some(MemoryTier::Global),
                None,
                None,
                &tenant_a,
                Some(10),
                Some(&tenant_a_filter),
            )
            .await
            .expect("tenant A filtered search");

        assert!(
            tenant_a_results
                .iter()
                .any(|hit| hit.chunk.content.contains("tenant A")),
            "tenant A should see its own source-bound chunk with a matching strict grant"
        );
        assert!(
            tenant_a_results
                .iter()
                .all(|hit| hit.chunk.tenant_scope.org_id == tenant_a.org_id
                    && hit.chunk.tenant_scope.workspace_id == tenant_a.workspace_id
                    && hit.chunk.tenant_scope.deployment_id == tenant_a.deployment_id
                    && !hit.chunk.content.contains("tenant B")),
            "tenant A search must not return tenant B source-bound chunks"
        );
    }

    fn strict_context_for_resource(
        tenant_scope: &MemoryTenantScope,
        resource: tandem_enterprise_contract::ResourceRef,
        data_class: tandem_enterprise_contract::DataClass,
    ) -> tandem_enterprise_contract::StrictTenantContext {
        let tenant_context = tandem_enterprise_contract::TenantContext::explicit_user_workspace(
            tenant_scope.org_id.clone(),
            tenant_scope.workspace_id.clone(),
            tenant_scope.deployment_id.clone(),
            "user-a",
        );
        let principal = tandem_enterprise_contract::PrincipalRef::human_user("user-a");
        let request_principal =
            tandem_enterprise_contract::RequestPrincipal::authenticated_user("user-a", "test");
        let grant = tandem_enterprise_contract::ScopedGrant::new(
            "grant-read",
            principal.clone(),
            resource.clone(),
            tandem_enterprise_contract::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_enterprise_contract::AccessPermission::Read])
        .with_data_classes(vec![data_class]);
        tandem_enterprise_contract::StrictTenantContext::new(
            tenant_context,
            principal,
            tandem_enterprise_contract::AuthorityChain::from_request(request_principal),
            tandem_enterprise_contract::ResourceScope::root(resource),
            tandem_enterprise_contract::AssertionMetadata::new(
                "test",
                "tandem-runtime",
                1,
                u64::MAX,
                "assertion-test",
            ),
        )
        .with_grants(vec![grant])
        .with_data_boundary(tandem_enterprise_contract::DataBoundary::allow(vec![
            data_class,
        ]))
    }

    #[tokio::test]
    async fn test_search_global_guide_docs_reranks_newer_doc_by_mtime() {
        let (manager, _temp) = setup_test_manager().await;

        let now_ms = chrono::Utc::now().timestamp_millis();
        let old_age_ms = 30 * 24 * 60 * 60 * 1000;
        let old_request = StoreMessageRequest {
            content:
                "Workflow authoring and creation should define triggers before automations run."
                    .to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "guide_docs:old_self_operator_playbook.md".to_string(),
            source_path: None,
            source_mtime: Some(now_ms - old_age_ms),
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            metadata: None,
        };
        let new_request = StoreMessageRequest {
            content: old_request.content.clone(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "guide_docs:new_self_operator_playbook.md".to_string(),
            source_path: None,
            source_mtime: Some(now_ms),
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            metadata: None,
        };

        for request in [old_request, new_request] {
            if let Err(err) = manager.store_message(request).await {
                if is_embeddings_disabled(&err) {
                    return;
                }
                panic!("store_message failed: {err}");
            }
        }

        let results = manager
            .search(
                "workflow authoring and creation triggers",
                Some(MemoryTier::Global),
                None,
                None,
                Some(2),
            )
            .await;
        let results = match results {
            Ok(results) => results,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("search failed: {err}"),
        };

        assert!(results.len() >= 2);
        assert_eq!(
            results[0].chunk.source,
            "guide_docs:new_self_operator_playbook.md"
        );
    }

    #[tokio::test]
    async fn test_retrieve_context() {
        let (manager, _temp) = setup_test_manager().await;

        // Store some test data
        let request = StoreMessageRequest {
            content: "The project uses React and TypeScript for the frontend.".to_string(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some("project-1".to_string()),
            source: "assistant_response".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            metadata: None,
        };
        match manager.store_message(request).await {
            Ok(_) => {}
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("store_message failed: {err}"),
        }

        let context = manager
            .retrieve_context("What technologies are used?", Some("project-1"), None, None)
            .await;
        let context = match context {
            Ok(context) => context,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("retrieve_context failed: {err}"),
        };

        assert!(!context.project_facts.is_empty());
    }

    #[tokio::test]
    async fn test_retrieve_context_with_meta() {
        let (manager, _temp) = setup_test_manager().await;

        let request = StoreMessageRequest {
            content: "The backend uses Rust and sqlite-vec for retrieval.".to_string(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some("project-1".to_string()),
            source: "assistant_response".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            metadata: None,
        };
        match manager.store_message(request).await {
            Ok(_) => {}
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("store_message failed: {err}"),
        }

        let result = manager
            .retrieve_context_with_meta("What does the backend use?", Some("project-1"), None, None)
            .await;
        let (context, meta) = match result {
            Ok(v) => v,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("retrieve_context_with_meta failed: {err}"),
        };

        assert!(meta.chunks_total > 0);
        assert!(meta.used);
        assert_eq!(
            meta.chunks_total,
            context.current_session.len()
                + context.relevant_history.len()
                + context.project_facts.len()
        );
        assert!(meta.score_min.is_some());
        assert!(meta.score_max.is_some());
    }

    #[tokio::test]
    async fn test_retrieve_context_with_meta_is_tenant_scoped() {
        let (manager, _temp) = setup_test_manager().await;
        let tenant_a = MemoryTenantScope {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: Some("deployment-1".to_string()),
        };
        let tenant_b = MemoryTenantScope {
            org_id: "org-b".to_string(),
            workspace_id: "workspace-b".to_string(),
            deployment_id: Some("deployment-1".to_string()),
        };
        let mut embedding = vec![0.0f32; DEFAULT_EMBEDDING_DIMENSION];
        embedding[0] = 1.0;

        let tenant_a_chunk = MemoryChunk {
            id: "tenant-a-context".to_string(),
            content: "tenant a current session context".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("shared-session".to_string()),
            project_id: Some("shared-project".to_string()),
            source: "assistant_response".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: tenant_a.clone(),
            created_at: chrono::Utc::now(),
            token_count: 5,
            metadata: None,
        };
        let tenant_b_chunk = MemoryChunk {
            id: "tenant-b-context".to_string(),
            content: "tenant b current session context must not leak".to_string(),
            tenant_scope: tenant_b,
            ..tenant_a_chunk.clone()
        };

        manager
            .db()
            .store_chunk(&tenant_a_chunk, &embedding)
            .await
            .unwrap();
        manager
            .db()
            .store_chunk(&tenant_b_chunk, &embedding)
            .await
            .unwrap();

        let result = manager
            .retrieve_context_with_meta_for_tenant(
                "current session context",
                Some("shared-project"),
                Some("shared-session"),
                &tenant_a,
                None,
            )
            .await;
        let (context, _meta) = match result {
            Ok(value) => value,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("retrieve_context_with_meta_for_tenant failed: {err}"),
        };

        assert_eq!(context.current_session.len(), 1);
        assert_eq!(context.current_session[0].id, "tenant-a-context");
        assert!(!context
            .format_for_injection()
            .contains("tenant b current session context"));
    }

    #[tokio::test]
    async fn test_config_management() {
        let (manager, _temp) = setup_test_manager().await;

        let config = manager.get_config("project-1").await.unwrap();
        assert_eq!(config.max_chunks, 10000);

        let new_config = MemoryConfig {
            max_chunks: 5000,
            retrieval_k: 10,
            ..Default::default()
        };

        manager.set_config("project-1", &new_config).await.unwrap();

        let updated = manager.get_config("project-1").await.unwrap();
        assert_eq!(updated.max_chunks, 5000);
        assert_eq!(updated.retrieval_k, 10);
    }

    #[tokio::test]
    async fn test_knowledge_registry_roundtrip_via_manager() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-1".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: Some("Reusable debugging guidance".to_string()),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: Some(serde_json::json!({"owner":"eng"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-1".to_string(),
            space_id: "space-1".to_string(),
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            dedupe_key: "item-1-dedupe".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry only after startup has completed.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/startup-note".to_string()],
            source_memory_ids: vec!["memory-1".to_string()],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"source_kind":"run"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let loaded_space = manager
            .get_knowledge_space("space-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded_space.namespace.as_deref(),
            Some("engineering/debugging")
        );

        let loaded_item = manager.get_knowledge_item("item-1").await.unwrap().unwrap();
        assert_eq!(loaded_item.space_id, "space-1");
        assert_eq!(
            loaded_item.coverage_key,
            "project-1::engineering/debugging::startup::race"
        );

        let items = manager
            .list_knowledge_items(
                "space-1",
                Some("project-1::engineering/debugging::startup::race"),
            )
            .await
            .unwrap();
        assert_eq!(items.len(), 1);

        let coverage = KnowledgeCoverageRecord {
            coverage_key: "project-1::engineering/debugging::startup::race".to_string(),
            space_id: "space-1".to_string(),
            latest_item_id: Some("item-1".to_string()),
            latest_dedupe_key: Some("item-1-dedupe".to_string()),
            last_seen_at_ms: now,
            last_promoted_at_ms: Some(now),
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: Some(serde_json::json!({"reuse_reason":"same issue"})),
        };
        manager.upsert_knowledge_coverage(&coverage).await.unwrap();

        let loaded_coverage = manager
            .get_knowledge_coverage("project-1::engineering/debugging::startup::race", "space-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_coverage.space_id, "space-1");
        assert_eq!(loaded_coverage.latest_item_id.as_deref(), Some("item-1"));
    }

    #[tokio::test]
    async fn test_knowledge_promotion_via_manager() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-2".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-2".to_string()),
            namespace: Some("ops/runbooks".to_string()),
            title: Some("Ops runbooks".to_string()),
            description: Some("Reusable operational guidance".to_string()),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-2".to_string(),
            space_id: space.id.clone(),
            coverage_key: "project-2::ops/runbooks::restarts::stale-service".to_string(),
            dedupe_key: "dedupe-2".to_string(),
            item_type: "runbook".to_string(),
            title: "Restart stale service".to_string(),
            summary: Some("Restart the service before retrying.".to_string()),
            payload: serde_json::json!({"action":"restart"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Working,
            status: crate::types::KnowledgeItemStatus::Working,
            run_id: Some("run-2".to_string()),
            artifact_refs: vec!["artifact://run-2/runbook".to_string()],
            source_memory_ids: vec!["memory-2".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let result = manager
            .promote_knowledge_item(&crate::types::KnowledgePromotionRequest {
                item_id: item.id.clone(),
                target_status: crate::types::KnowledgeItemStatus::Promoted,
                promoted_at_ms: now + 5,
                freshness_expires_at_ms: Some(now + 86_400_000),
                reviewer_id: None,
                approval_id: None,
                reason: Some("manager wrapper".to_string()),
            })
            .await
            .unwrap()
            .expect("promotion result");
        assert_eq!(
            result.item.status,
            crate::types::KnowledgeItemStatus::Promoted
        );
        assert_eq!(result.coverage.latest_item_id.as_deref(), Some("item-2"));
    }

    #[tokio::test]
    async fn test_preflight_knowledge_disabled() {
        let (manager, _temp) = setup_test_manager().await;

        let request = KnowledgePreflightRequest {
            project_id: "project-1".to_string(),
            task_family: "debugging".to_string(),
            subject: "startup race".to_string(),
            binding: KnowledgeBinding {
                enabled: false,
                ..Default::default()
            },
        };

        let result = manager.preflight_knowledge(&request).await.unwrap();
        assert_eq!(result.decision, KnowledgeReuseDecision::Disabled);
        assert!(result.skip_reason.is_some());
    }

    #[tokio::test]
    async fn test_preflight_knowledge_reuses_promoted_item() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-preflight-1".to_string(),
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
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-preflight-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: tandem_orchestrator::build_knowledge_coverage_key(
                "project-1",
                Some("engineering/debugging"),
                "startup",
                "race",
            ),
            dedupe_key: "dedupe-preflight-1".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup-dependent retries".to_string(),
            summary: Some("Retry after startup completes.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-1".to_string()),
            artifact_refs: vec!["artifact://run-1/debug".to_string()],
            source_memory_ids: vec![],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePreflightRequest {
            project_id: "project-1".to_string(),
            task_family: "startup".to_string(),
            subject: "race".to_string(),
            binding: KnowledgeBinding {
                namespace: Some("engineering/debugging".to_string()),
                freshness_ms: Some(10_000),
                ..Default::default()
            },
        };

        let result = manager.preflight_knowledge(&request).await.unwrap();
        assert_eq!(result.decision, KnowledgeReuseDecision::ReusePromoted);
        assert_eq!(result.items.len(), 1);
        assert!(result.reuse_reason.is_some());
    }

    #[tokio::test]
    async fn test_preflight_knowledge_stale_requires_refresh() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-preflight-2".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("ops/runbooks".to_string()),
            title: Some("Ops runbooks".to_string()),
            description: Some("Reusable ops guidance".to_string()),
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-preflight-2".to_string(),
            space_id: space.id.clone(),
            coverage_key: tandem_orchestrator::build_knowledge_coverage_key(
                "project-1",
                Some("ops/runbooks"),
                "restart",
                "stale service",
            ),
            dedupe_key: "dedupe-preflight-2".to_string(),
            item_type: "runbook".to_string(),
            title: "Restart stale service".to_string(),
            summary: Some("Restart and verify health.".to_string()),
            payload: serde_json::json!({"action":"restart"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-2".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: Some(now - 1000),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let request = KnowledgePreflightRequest {
            project_id: "project-1".to_string(),
            task_family: "restart".to_string(),
            subject: "stale service".to_string(),
            binding: KnowledgeBinding {
                namespace: Some("ops/runbooks".to_string()),
                freshness_ms: Some(10_000),
                ..Default::default()
            },
        };

        let result = manager.preflight_knowledge(&request).await.unwrap();
        assert_eq!(result.decision, KnowledgeReuseDecision::RefreshRequired);
        assert!(result.freshness_reason.is_some());
        assert!(!result.items.is_empty());
        assert!(!result.is_reusable());
    }

    #[tokio::test]
    async fn test_preflight_knowledge_no_prior_knowledge() {
        let (manager, _temp) = setup_test_manager().await;

        let request = KnowledgePreflightRequest {
            project_id: "project-1".to_string(),
            task_family: "support".to_string(),
            subject: "triage".to_string(),
            binding: KnowledgeBinding {
                reuse_mode: KnowledgeReuseMode::Preflight,
                ..Default::default()
            },
        };

        let result = manager.preflight_knowledge(&request).await.unwrap();
        assert_eq!(result.decision, KnowledgeReuseDecision::NoPriorKnowledge);
        assert!(result.skip_reason.is_some());
    }

    #[tokio::test]
    async fn test_preflight_knowledge_requires_explicit_namespace_when_project_has_many() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let spaces = [
            ("space-alpha", "engineering/debugging", "Delay retries"),
            ("space-beta", "ops/runbooks", "Restart safely"),
        ];
        for (id, namespace, title) in spaces {
            manager
                .upsert_knowledge_space(&KnowledgeSpaceRecord {
                    id: id.to_string(),
                    scope: KnowledgeScope::Project,
                    project_id: Some("project-1".to_string()),
                    namespace: Some(namespace.to_string()),
                    title: Some(title.to_string()),
                    description: None,
                    trust_level: KnowledgeTrustLevel::Promoted,
                    metadata: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                })
                .await
                .unwrap();
        }

        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-1".to_string(),
                task_family: "debugging".to_string(),
                subject: "startup race".to_string(),
                binding: KnowledgeBinding::default(),
            })
            .await
            .unwrap();

        assert_eq!(result.decision, KnowledgeReuseDecision::NoPriorKnowledge);
        assert!(result.items.is_empty());
        assert!(result
            .skip_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no reusable knowledge spaces")));
    }

    #[tokio::test]
    async fn test_preflight_knowledge_infers_single_project_namespace() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-single-namespace".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-single-namespace".to_string(),
            space_id: space.id.clone(),
            coverage_key: tandem_orchestrator::build_knowledge_coverage_key(
                "project-1",
                Some("engineering/debugging"),
                "debugging",
                "startup race",
            ),
            dedupe_key: "dedupe-single-namespace".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup retries".to_string(),
            summary: Some("Wait for startup completion.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-single-namespace".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-1".to_string(),
                task_family: "debugging".to_string(),
                subject: "startup race".to_string(),
                binding: KnowledgeBinding::default(),
            })
            .await
            .unwrap();

        assert_eq!(result.decision, KnowledgeReuseDecision::ReusePromoted);
        assert_eq!(result.namespace.as_deref(), Some("engineering/debugging"));
        assert_eq!(result.items.len(), 1);
    }

    #[tokio::test]
    async fn test_knowledge_preflight_disabled_binding_returns_disabled() {
        let (manager, _temp) = setup_test_manager().await;
        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-1".to_string(),
                task_family: "debugging".to_string(),
                subject: "startup race".to_string(),
                binding: tandem_orchestrator::KnowledgeBinding {
                    enabled: false,
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(
            result.decision,
            tandem_orchestrator::KnowledgeReuseDecision::Disabled
        );
        assert!(result.items.is_empty());
        assert!(result
            .skip_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("disabled")));
    }

    #[tokio::test]
    async fn test_knowledge_preflight_fresh_item_is_reusable() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-preflight-1".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-1".to_string()),
            namespace: Some("engineering/debugging".to_string()),
            title: Some("Engineering debugging".to_string()),
            description: None,
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-preflight-1".to_string(),
            space_id: space.id.clone(),
            coverage_key: tandem_orchestrator::build_knowledge_coverage_key(
                "project-1",
                Some("engineering/debugging"),
                "debugging",
                "startup race",
            ),
            dedupe_key: "dedupe-preflight-1".to_string(),
            item_type: "decision".to_string(),
            title: "Delay startup retries".to_string(),
            summary: Some("Wait for startup completion before retrying.".to_string()),
            payload: serde_json::json!({"action":"delay_retry"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-preflight-1".to_string()),
            artifact_refs: vec!["artifact://run-preflight-1/report".to_string()],
            source_memory_ids: vec!["memory-preflight-1".to_string()],
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let coverage = KnowledgeCoverageRecord {
            coverage_key: item.coverage_key.clone(),
            space_id: space.id.clone(),
            latest_item_id: Some(item.id.clone()),
            latest_dedupe_key: Some(item.dedupe_key.clone()),
            last_seen_at_ms: now,
            last_promoted_at_ms: Some(now),
            freshness_expires_at_ms: Some(now + 86_400_000),
            metadata: None,
        };
        manager.upsert_knowledge_coverage(&coverage).await.unwrap();

        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-1".to_string(),
                task_family: "debugging".to_string(),
                subject: "startup race".to_string(),
                binding: tandem_orchestrator::KnowledgeBinding {
                    namespace: Some("engineering/debugging".to_string()),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(
            result.decision,
            tandem_orchestrator::KnowledgeReuseDecision::ReusePromoted
        );
        assert!(result.is_reusable());
        assert!(!result.items.is_empty());
        assert!(result
            .reuse_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("reusing")));
    }

    #[tokio::test]
    async fn test_knowledge_preflight_stale_item_requests_refresh() {
        let (manager, _temp) = setup_test_manager().await;
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "space-preflight-2".to_string(),
            scope: tandem_orchestrator::KnowledgeScope::Project,
            project_id: Some("project-2".to_string()),
            namespace: Some("support/runbooks".to_string()),
            title: Some("Support runbooks".to_string()),
            description: None,
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager.upsert_knowledge_space(&space).await.unwrap();

        let item = KnowledgeItemRecord {
            id: "item-preflight-2".to_string(),
            space_id: space.id.clone(),
            coverage_key: tandem_orchestrator::build_knowledge_coverage_key(
                "project-2",
                Some("support/runbooks"),
                "runbooks",
                "restart service",
            ),
            dedupe_key: "dedupe-preflight-2".to_string(),
            item_type: "runbook".to_string(),
            title: "Restart stale service".to_string(),
            summary: Some("Restart before retrying.".to_string()),
            payload: serde_json::json!({"action":"restart"}),
            trust_level: tandem_orchestrator::KnowledgeTrustLevel::Promoted,
            status: crate::types::KnowledgeItemStatus::Promoted,
            run_id: Some("run-preflight-2".to_string()),
            artifact_refs: vec![],
            source_memory_ids: vec![],
            freshness_expires_at_ms: Some(now.saturating_sub(1)),
            metadata: None,
            created_at_ms: now.saturating_sub(86_400_000),
            updated_at_ms: now,
        };
        manager.upsert_knowledge_item(&item).await.unwrap();

        let coverage = KnowledgeCoverageRecord {
            coverage_key: item.coverage_key.clone(),
            space_id: space.id.clone(),
            latest_item_id: Some(item.id.clone()),
            latest_dedupe_key: Some(item.dedupe_key.clone()),
            last_seen_at_ms: now,
            last_promoted_at_ms: Some(now.saturating_sub(1)),
            freshness_expires_at_ms: Some(now.saturating_sub(1)),
            metadata: None,
        };
        manager.upsert_knowledge_coverage(&coverage).await.unwrap();

        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-2".to_string(),
                task_family: "runbooks".to_string(),
                subject: "restart service".to_string(),
                binding: tandem_orchestrator::KnowledgeBinding {
                    namespace: Some("support/runbooks".to_string()),
                    freshness_ms: Some(86_400_000),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(
            result.decision,
            tandem_orchestrator::KnowledgeReuseDecision::RefreshRequired
        );
        assert!(!result.is_reusable());
        assert!(result.items.is_empty() || result.freshness_reason.is_some());
        assert!(result
            .freshness_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("expired") || reason.contains("freshness")));
    }

    #[tokio::test]
    async fn test_knowledge_preflight_no_prior_knowledge_returns_no_prior() {
        let (manager, _temp) = setup_test_manager().await;
        let result = manager
            .preflight_knowledge(&KnowledgePreflightRequest {
                project_id: "project-3".to_string(),
                task_family: "ops".to_string(),
                subject: "incident triage".to_string(),
                binding: Default::default(),
            })
            .await
            .unwrap();
        assert_eq!(
            result.decision,
            tandem_orchestrator::KnowledgeReuseDecision::NoPriorKnowledge
        );
        assert!(result.items.is_empty());
        assert!(result
            .skip_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no active promoted knowledge")));
    }
}
