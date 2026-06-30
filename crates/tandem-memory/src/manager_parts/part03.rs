#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use crate::types::DEFAULT_EMBEDDING_DIMENSION;
    use tempfile::TempDir;

    async fn setup_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let manager = MemoryManager::new(&db_path).await.unwrap();
        (manager, temp_dir)
    }

    async fn setup_deterministic_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let manager = MemoryManager::new_with_embedding_service(
            &db_path,
            crate::embeddings::EmbeddingService::deterministic_for_tests(
                DEFAULT_EMBEDDING_DIMENSION,
            ),
        )
        .await
        .unwrap();
        (manager, temp_dir)
    }

    fn tenant_scope(org_id: &str, workspace_id: &str) -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: org_id.to_string(),
            workspace_id: workspace_id.to_string(),
            deployment_id: Some("deployment-1".to_string()),
        }
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
    async fn promoted_knowledge_item_remains_bound_to_owning_tenant() {
        let (manager, _temp) = setup_test_manager().await;
        let tenant_a = tenant_scope("org-a", "workspace-a");
        let tenant_b = tenant_scope("org-b", "workspace-b");
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: "tenant-b-promoted-space".to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("shared-project".to_string()),
            namespace: Some("support/runbooks".to_string()),
            title: Some("Tenant B runbooks".to_string()),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_space_for_tenant(&space, &tenant_b)
            .await
            .unwrap();

        let item = KnowledgeItemRecord {
            id: "tenant-b-promoted-item".to_string(),
            space_id: space.id.clone(),
            coverage_key: "shared-project::support/runbooks::billing::refunds".to_string(),
            dedupe_key: "tenant-b-promoted-dedupe".to_string(),
            item_type: "runbook".to_string(),
            title: "Tenant B refund runbook".to_string(),
            summary: Some("Tenant B internal refund steps.".to_string()),
            payload: serde_json::json!({"tenant": "b", "action": "refund"}),
            trust_level: KnowledgeTrustLevel::Working,
            status: crate::types::KnowledgeItemStatus::Working,
            run_id: Some("tenant-b-run".to_string()),
            artifact_refs: vec!["artifact://tenant-b/refunds".to_string()],
            source_memory_ids: vec!["memory://tenant-b/refunds".to_string()],
            freshness_expires_at_ms: None,
            metadata: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_item_for_tenant(&item, &tenant_b)
            .await
            .unwrap();

        let promote = KnowledgePromotionRequest {
            item_id: item.id.clone(),
            target_status: crate::types::KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 10,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("ct-03 tenant scope regression".to_string()),
        };
        assert!(manager
            .promote_knowledge_item_for_tenant(&promote, &tenant_a)
            .await
            .unwrap()
            .is_none());

        let promoted = manager
            .promote_knowledge_item_for_tenant(&promote, &tenant_b)
            .await
            .unwrap()
            .expect("tenant-b promotion");
        assert_eq!(
            promoted.item.status,
            crate::types::KnowledgeItemStatus::Promoted
        );
        assert_eq!(
            promoted.coverage.latest_item_id.as_deref(),
            Some(item.id.as_str())
        );

        assert!(manager
            .get_knowledge_item_for_tenant(&item.id, &tenant_a)
            .await
            .unwrap()
            .is_none());
        assert!(manager
            .list_knowledge_items_for_tenant(&space.id, Some(&item.coverage_key), &tenant_a)
            .await
            .unwrap()
            .is_empty());
        assert!(manager
            .get_knowledge_coverage_for_tenant(&item.coverage_key, &space.id, &tenant_a)
            .await
            .unwrap()
            .is_none());
        assert!(manager
            .get_knowledge_item_for_tenant(&item.id, &tenant_b)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn retrieve_context_enforces_knowledge_scope_phase_before_prompt_assembly() {
        let (manager, _temp) = setup_deterministic_test_manager().await;
        let tenant_scope = tenant_scope("acme", "hq");
        let resource = tandem_enterprise_contract::ResourceRef::new(
            "acme",
            "hq",
            tandem_enterprise_contract::ResourceKind::KnowledgeSpace,
            "knowledge-space-ops",
        )
        .with_project_id("project-ops");
        let policy = crate::KnowledgeScopePolicy {
            registry_id: "registry-ops".to_string(),
            resource_ref: resource.clone(),
            data_class: tandem_enterprise_contract::DataClass::Confidential,
            collection_id: Some("collection-ops".to_string()),
            source_binding_id: Some("binding-ops".to_string()),
            source_object_id: Some("source-ops".to_string()),
            owner_org_unit_id: Some("ou-ops".to_string()),
            risk_tier: Some("confidential".to_string()),
            allowed_workflow_phases: vec!["draft".to_string()],
            allowed_write_tiers: vec![crate::GovernedMemoryTier::Session],
            allowed_promotion_tiers: vec![crate::GovernedMemoryTier::Project],
            retention_expires_at_ms: Some(u64::MAX),
            required_trust_label: Some(crate::MemoryTrustLabel::HumanApproved),
            promotion_requires_approval: true,
        };
        let embedding = crate::embeddings::EmbeddingService::deterministic_for_tests(
            DEFAULT_EMBEDDING_DIMENSION,
        )
        .embed("scoped operational memory")
        .await
        .unwrap();
        let scoped_chunk = MemoryChunk {
            id: "scoped-ops-memory".to_string(),
            content: "scoped operational memory for draft phase only".to_string(),
            tier: MemoryTier::Session,
            session_id: Some("session-ops".to_string()),
            project_id: Some("project-ops".to_string()),
            source: "workflow_memory".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: tenant_scope.clone(),
            created_at: chrono::Utc::now(),
            token_count: 7,
            metadata: Some(policy.metadata_value()),
        };
        manager
            .db()
            .store_chunk(&scoped_chunk, &embedding)
            .await
            .unwrap();

        let (unfiltered_context, _) = manager
            .retrieve_context_with_meta_for_tenant(
                "scoped operational memory",
                Some("project-ops"),
                Some("session-ops"),
                &tenant_scope,
                None,
            )
            .await
            .expect("unfiltered context retrieval");
        assert!(!unfiltered_context
            .format_for_injection()
            .contains("scoped operational memory"));

        let wrong_phase_filter = crate::types::MemoryAccessFilter::strict(
            strict_context_for_resource(
                &tenant_scope,
                resource.clone(),
                tandem_enterprise_contract::DataClass::Confidential,
            ),
            chrono::Utc::now().timestamp_millis() as u64,
        )
        .with_workflow_phase("review");
        let (wrong_phase_context, _) = manager
            .retrieve_context_with_meta_for_tenant_with_access_filter(
                "scoped operational memory",
                Some("project-ops"),
                Some("session-ops"),
                &tenant_scope,
                None,
                Some(&wrong_phase_filter),
            )
            .await
            .expect("wrong phase context retrieval");
        assert!(!wrong_phase_context
            .format_for_injection()
            .contains("scoped operational memory"));

        let draft_filter = crate::types::MemoryAccessFilter::strict(
            strict_context_for_resource(
                &tenant_scope,
                resource,
                tandem_enterprise_contract::DataClass::Confidential,
            ),
            chrono::Utc::now().timestamp_millis() as u64,
        )
        .with_workflow_phase("draft");
        let (draft_context, _) = manager
            .retrieve_context_with_meta_for_tenant_with_access_filter(
                "scoped operational memory",
                Some("project-ops"),
                Some("session-ops"),
                &tenant_scope,
                None,
                Some(&draft_filter),
            )
            .await
            .expect("draft context retrieval");
        assert!(draft_context
            .format_for_injection()
            .contains("scoped operational memory"));
    }
}
