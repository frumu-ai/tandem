#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let manager = MemoryManager::new(&db_path).await.unwrap();
        (manager, temp_dir)
    }

    fn tenant_scope(org_id: &str, workspace_id: &str) -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: org_id.to_string(),
            workspace_id: workspace_id.to_string(),
            deployment_id: Some("deployment-1".to_string()),
        }
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
}
