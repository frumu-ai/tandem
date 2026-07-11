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
        .is_some_and(|reason| reason.contains("no reusable knowledge spaces")
            || reason.contains("no active promoted knowledge")));
}

#[tokio::test]
async fn context_tree_is_tenant_scoped() {
    let (manager, _temp) = setup_test_manager().await;
    let scope_a = crate::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let scope_b = crate::types::MemoryTenantScope {
        org_id: "org-b".to_string(),
        workspace_id: "workspace-b".to_string(),
        deployment_id: None,
    };

    let node_id = manager
        .store_content_with_layers(
            "tandem://resources/proj-a/notes.md",
            "tenant A secret notes",
            None,
            &scope_a,
        )
        .await
        .unwrap();

    // The owner resolves the node and reads its layer.
    let node = manager
        .resolve_uri("tandem://resources/proj-a/notes.md", &scope_a)
        .await
        .unwrap()
        .expect("owner resolves node");
    assert_eq!(node.id, node_id);
    let layer = manager
        .get_context_layer(&node_id, crate::types::LayerType::L2, &scope_a)
        .await
        .unwrap();
    assert!(layer.is_some(), "owner reads L2 layer");

    // A different tenant sees nothing — same URI, same node id, no signal
    // distinguishing a foreign node from a missing one.
    assert!(manager
        .resolve_uri("tandem://resources/proj-a/notes.md", &scope_b)
        .await
        .unwrap()
        .is_none());
    assert!(manager
        .get_context_layer(&node_id, crate::types::LayerType::L2, &scope_b)
        .await
        .unwrap()
        .is_none());
    assert!(manager
        .tree("tandem://resources/proj-a", 3, &scope_b)
        .await
        .unwrap()
        .is_empty());
    assert!(manager
        .list_directory("tandem://resources/proj-a", &scope_b)
        .await
        .unwrap()
        .nodes
        .is_empty());

    // Writing a layer onto a foreign node id fails like a missing node.
    let err = manager
        .db()
        .create_layer(
            &node_id,
            crate::types::LayerType::L0,
            "injected",
            1,
            None,
            &scope_b,
        )
        .await
        .expect_err("cross-tenant layer write is rejected");
    assert!(matches!(err, crate::types::MemoryError::NotFound(_)));

    // The owner still traverses their own tree.
    let tree = manager
        .tree("tandem://resources/proj-a", 3, &scope_a)
        .await
        .unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].node.uri, "tandem://resources/proj-a/notes.md");
}

#[tokio::test]
async fn context_tree_same_uri_coexists_across_tenants() {
    let (manager, _temp) = setup_test_manager().await;
    let scope_a = crate::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let scope_b = crate::types::MemoryTenantScope {
        org_id: "org-b".to_string(),
        workspace_id: "workspace-b".to_string(),
        deployment_id: None,
    };

    // The legacy schema had a global UNIQUE(uri); per-tenant trees must be
    // able to own the same URI independently.
    let id_a = manager
        .store_content_with_layers(
            "tandem://user/profile.md",
            "tenant A profile",
            None,
            &scope_a,
        )
        .await
        .unwrap();
    let id_b = manager
        .store_content_with_layers(
            "tandem://user/profile.md",
            "tenant B profile",
            None,
            &scope_b,
        )
        .await
        .unwrap();
    assert_ne!(id_a, id_b);

    let content_a = manager
        .get_layer_content(&id_a, crate::types::LayerType::L2, &scope_a)
        .await
        .unwrap()
        .expect("tenant A content");
    let content_b = manager
        .get_layer_content(&id_b, crate::types::LayerType::L2, &scope_b)
        .await
        .unwrap()
        .expect("tenant B content");
    assert_eq!(content_a, "tenant A profile");
    assert_eq!(content_b, "tenant B profile");

    // Duplicate URI within the SAME tenant is still rejected.
    assert!(manager
        .store_content_with_layers("tandem://user/profile.md", "dup", None, &scope_a)
        .await
        .is_err());
}

#[tokio::test]
async fn legacy_memory_nodes_table_migrates_to_tenant_scoped_schema() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("legacy.db");
    // Pre-create the pre-tenancy table shape: global UNIQUE(uri), no tenant
    // columns. Opening the manager must rebuild it in place.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memory_nodes (
                    id TEXT PRIMARY KEY,
                    uri TEXT NOT NULL UNIQUE,
                    parent_uri TEXT,
                    node_type TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    metadata TEXT
                );
                INSERT INTO memory_nodes
                    (id, uri, parent_uri, node_type, created_at, updated_at, metadata)
                VALUES
                    ('legacy-node', 'tandem://resources/legacy/doc.md',
                     'tandem://resources/legacy', 'file',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL);",
        )
        .unwrap();
    }

    let manager = MemoryManager::new(&db_path).await.unwrap();

    // Legacy rows are backfilled into the local tenant scope.
    let node = manager
        .resolve_uri(
            "tandem://resources/legacy/doc.md",
            &crate::types::MemoryTenantScope::local(),
        )
        .await
        .unwrap()
        .expect("legacy node visible under local scope");
    assert_eq!(node.id, "legacy-node");

    // Other tenants cannot see the migrated row, and the former global
    // UNIQUE(uri) no longer blocks them from owning the same URI.
    let scope_b = crate::types::MemoryTenantScope {
        org_id: "org-b".to_string(),
        workspace_id: "workspace-b".to_string(),
        deployment_id: None,
    };
    assert!(manager
        .resolve_uri("tandem://resources/legacy/doc.md", &scope_b)
        .await
        .unwrap()
        .is_none());
    manager
        .store_content_with_layers(
            "tandem://resources/legacy/doc.md",
            "tenant B copy",
            None,
            &scope_b,
        )
        .await
        .expect("same URI storable by another tenant after migration");

    // Re-opening does not re-run the rebuild (migration is idempotent).
    drop(manager);
    let reopened = MemoryManager::new(&db_path).await.unwrap();
    let node = reopened
        .resolve_uri(
            "tandem://resources/legacy/doc.md",
            &crate::types::MemoryTenantScope::local(),
        )
        .await
        .unwrap()
        .expect("legacy node survives reopen");
    assert_eq!(node.id, "legacy-node");
}

#[tokio::test]
async fn chunk_subject_round_trips_through_store_and_search() {
    let (manager, _temp) = setup_deterministic_test_manager().await;
    let scope = crate::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let request = StoreMessageRequest {
        content: "subject scoped archived exchange about deployment windows".to_string(),
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        source: "chat_exchange".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: scope.clone(),
        subject: Some("user-a".to_string()),
        metadata: None,
    };
    let ids = manager.store_message(request).await.unwrap();
    assert!(!ids.is_empty());

    let access_filter =
        crate::types::MemoryAccessFilter::local_noop(chrono::Utc::now().timestamp_millis() as u64)
            .with_caller_subject("user-a");
    let results = manager
        .search_for_tenant_with_access_filter(
            "deployment windows",
            Some(MemoryTier::Global),
            None,
            None,
            &scope,
            Some(5),
            Some(&access_filter),
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].chunk.subject.as_deref(), Some("user-a"));
}

#[tokio::test]
async fn subject_chunks_survive_topk_ranking_against_other_subjects() {
    let (manager, _temp) = setup_deterministic_test_manager().await;
    let scope = crate::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let store = |content: String, subject: &str| StoreMessageRequest {
        content,
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        source: "chat_exchange".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: scope.clone(),
        subject: Some(subject.to_string()),
        metadata: None,
    };

    // Fill the candidate window (limit * ACCESS_FILTER_CANDIDATE_MULTIPLIER)
    // with another subject's chunks that match the query at least as well.
    let query = "quarterly infrastructure budget planning";
    for i in 0..12 {
        manager
            .store_message(store(format!("{query} details volume {i}"), "user-b"))
            .await
            .unwrap();
    }
    manager
        .store_message(store(format!("{query} owner note"), "user-a"))
        .await
        .unwrap();

    // Governed search as user-a: their chunk must reach the results even
    // though user-b's closer chunks would otherwise fill the top-k window.
    let tenant = tandem_enterprise_contract::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        None,
        "user-a",
    );
    let principal =
        tandem_enterprise_contract::RequestPrincipal::authenticated_user("user-a", "test");
    let strict = tandem_enterprise_contract::StrictTenantContext::new(
        tenant,
        tandem_enterprise_contract::PrincipalRef::human_user("user-a"),
        tandem_enterprise_contract::AuthorityChain::from_request(principal),
        tandem_enterprise_contract::ResourceScope::root(
            tandem_enterprise_contract::ResourceRef::new(
                "org-a",
                "workspace-a",
                tandem_enterprise_contract::ResourceKind::Workspace,
                "workspace-a",
            ),
        ),
        tandem_enterprise_contract::AssertionMetadata::new(
            "issuer",
            "runtime",
            1_000,
            9_999_999_999_999,
            "assertion-a",
        ),
    );
    let filter =
        crate::types::MemoryAccessFilter::strict(strict, 2_000).with_caller_subject("user-a");
    let results = manager
        .search_for_tenant_with_access_filter(
            query,
            Some(MemoryTier::Global),
            None,
            None,
            &scope,
            Some(2),
            Some(&filter),
        )
        .await
        .unwrap();
    assert!(
        results
            .iter()
            .all(|result| result.chunk.subject.as_deref() != Some("user-b")),
        "no foreign-subject chunks in governed results"
    );
    assert!(
        results
            .iter()
            .any(|result| result.chunk.subject.as_deref() == Some("user-a")),
        "owner's chunk must not be starved out of the candidate window"
    );
}
