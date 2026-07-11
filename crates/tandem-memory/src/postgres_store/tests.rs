use super::*;
use crate::types::{
    GlobalMemoryRecord, LayerType, MemoryChunk, MemoryTenantScope, MemoryTier, NodeType,
    SourceObjectLifecycleRecord, SourceObjectLifecycleState,
};

fn config(url: String, max_pool_size: usize) -> PostgresMemoryStoreConfig {
    PostgresMemoryStoreConfig {
        url,
        embedding_dimension: 3,
        distance_metric: PostgresDistanceMetric::Cosine,
        max_pool_size,
        pool_wait_timeout: std::time::Duration::from_millis(100),
        search_surface_mode: PostgresSearchSurfaceMode::PlaintextPgvector,
        rerank_candidate_limit: 100,
    }
}

fn test_url() -> Option<String> {
    std::env::var("TANDEM_TEST_POSTGRES_URL").ok()
}

fn tenant(org: &str) -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: org.to_string(),
        workspace_id: "workspace".to_string(),
        deployment_id: Some("deployment".to_string()),
    }
}

fn chunk(id: &str, tenant_scope: MemoryTenantScope) -> MemoryChunk {
    MemoryChunk {
        id: id.to_string(),
        content: id.to_string(),
        tier: MemoryTier::Project,
        session_id: None,
        project_id: Some("project".to_string()),
        source: "postgres_contract_test".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope,
        subject: None,
        created_at: chrono::Utc::now(),
        token_count: 1,
        metadata: None,
    }
}

fn owned_chunk(
    id: &str,
    tier: MemoryTier,
    tenant_scope: MemoryTenantScope,
    subject: &str,
) -> MemoryChunk {
    let mut chunk = chunk(id, tenant_scope);
    chunk.tier = tier;
    chunk.session_id = (tier == MemoryTier::Session).then(|| "session".to_string());
    chunk.subject = Some(subject.to_string());
    chunk.metadata = Some(serde_json::json!({ "owner_org_unit_id": "finance" }));
    chunk
}

fn global_record(id: &str, tenant_scope: &MemoryTenantScope) -> GlobalMemoryRecord {
    GlobalMemoryRecord {
        id: id.to_string(),
        user_id: "legacy-user".to_string(),
        source_type: "postgres_contract_test".to_string(),
        content: format!("global record {id}"),
        content_hash: format!("hash-{id}"),
        run_id: "run".to_string(),
        session_id: None,
        message_id: Some(id.to_string()),
        tool_name: None,
        project_tag: None,
        channel_tag: None,
        host_tag: None,
        metadata: None,
        provenance: Some(serde_json::json!({ "tenant_context": {
            "org_id": tenant_scope.org_id,
            "workspace_id": tenant_scope.workspace_id,
            "deployment_id": tenant_scope.deployment_id,
        }})),
        redaction_status: "none".to_string(),
        redaction_count: 0,
        visibility: "shared".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1,
        updated_at_ms: 1,
        expires_at_ms: None,
    }
}

#[tokio::test]
async fn postgres_scopes_candidates_before_vector_top_k() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");

    for index in 0..20 {
        let tenant = tenant("other");
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: chunk(&format!("other-{index}"), tenant),
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("seed out-of-scope chunk");
    }
    let tenant = tenant("target");
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            chunk: chunk("target", tenant.clone()),
            embedding: vec![0.8, 0.2, 0.0],
        })
        .await
        .expect("seed in-scope chunk");
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            chunk: chunk("target-less-similar", tenant.clone()),
            embedding: vec![0.0, 1.0, 0.0],
        })
        .await
        .expect("seed less-similar in-scope chunk");

    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 2,
        })
        .await
        .expect("run scoped pgvector query");
    let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
        panic!("expected vector results");
    };
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].0.id, "target");
    assert!(hits[0].1 < hits[1].1, "best match must have lower distance");

    for (id, binding, embedding) in [
        ("denied-nearest", "drive-legal", vec![1.0, 0.0, 0.0]),
        ("allowed-near", "drive-finance", vec![0.9, 0.1, 0.0]),
    ] {
        let mut governed = chunk(id, tenant.clone());
        governed.metadata = Some(serde_json::json!({
            "enterprise_source_binding": {
                "binding_id": binding,
                "data_class": "confidential"
            }
        }));
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: governed,
                embedding,
            })
            .await
            .expect("write governed plaintext-search chunk");
    }
    let principal = crate::MemoryDecryptPrincipal::retrieval_gateway(
        "finance-reader",
        tenant.clone(),
        vec![tandem_enterprise_contract::DataClass::Confidential],
        vec!["drive-finance".to_string()],
    );
    let result = crate::decrypt_context::with_decrypt_principal(
        principal,
        store.query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 1,
        }),
    )
    .await
    .expect("run grant-filtered plaintext pgvector query");
    assert!(matches!(
        result,
        MemoryStoreQueryResult::SimilarChunks(hits)
            if hits.len() == 1 && hits[0].0.id == "allowed-near"
    ));
    let error = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant),
            selector: MemoryChunkSelector {
                tier: MemoryTier::Project,
                project_id: None,
                session_id: None,
            },
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 1,
        })
        .await
        .expect_err("unconstrained project search must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::InvalidRequest);
}

#[tokio::test]
async fn postgres_context_tree_recurses_and_entity_reads_fail_closed() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("context-tree");
    for (uri, parent_uri, node_type) in [
        (
            "memory://root/dir",
            Some("memory://root"),
            NodeType::Directory,
        ),
        (
            "memory://root/dir/nested",
            Some("memory://root/dir"),
            NodeType::Directory,
        ),
        (
            "memory://root/dir/nested/file.txt",
            Some("memory://root/dir/nested"),
            NodeType::File,
        ),
    ] {
        store
            .write(MemoryStoreWriteRequest::ContextNode {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                uri: uri.to_string(),
                parent_uri: parent_uri.map(ToString::to_string),
                node_type,
                metadata: None,
            })
            .await
            .expect("write PostgreSQL context node");
    }
    let tree = store
        .query(MemoryStoreQueryRequest::ContextTree {
            scope: MemoryReadScope::tenant(tenant.clone()),
            parent_uri: "memory://root".to_string(),
            max_depth: 3,
        })
        .await
        .expect("read recursive PostgreSQL context tree");
    assert!(matches!(
        tree,
        MemoryStoreQueryResult::ContextTree(tree)
            if tree.len() == 1
                && tree[0].children.len() == 1
                && tree[0].children[0].children.len() == 1
                && tree[0].children[0].children[0].node.uri.ends_with("file.txt")
    ));
    let original = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope: MemoryReadScope::tenant(tenant.clone()),
            uri: "memory://root/dir".to_string(),
        })
        .await
        .expect("read original context URI");
    let MemoryStoreReadResult::ContextNode(Some(original)) = original else {
        panic!("expected original context node");
    };
    store
        .write(MemoryStoreWriteRequest::ContextLayer {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            node_id: original.id.clone(),
            layer_type: LayerType::L1,
            content: "preserved context layer".to_string(),
            token_count: 3,
            source_chunk_id: None,
        })
        .await
        .expect("write original context layer");
    let error = store
        .write(MemoryStoreWriteRequest::ContextNode {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            uri: "memory://root/dir".to_string(),
            parent_uri: Some("memory://other".to_string()),
            node_type: NodeType::Directory,
            metadata: None,
        })
        .await
        .expect_err("duplicate context URI must be rejected");
    assert_eq!(error.kind, MemoryStoreErrorKind::InvalidRequest);
    let layer = store
        .read(MemoryStoreReadRequest::ContextLayer {
            scope: MemoryReadScope::tenant(tenant.clone()),
            node_id: original.id,
            layer_type: LayerType::L1,
        })
        .await
        .expect("read preserved context layer");
    assert!(matches!(
        layer,
        MemoryStoreReadResult::ContextLayer(Some(layer)) if layer.content == "preserved context layer"
    ));
    let tree = store
        .query(MemoryStoreQueryRequest::ContextTree {
            scope: MemoryReadScope::tenant(tenant.clone()),
            parent_uri: "memory://root".to_string(),
            max_depth: 3,
        })
        .await
        .expect("read PostgreSQL context tree layer summaries");
    assert!(matches!(
        tree,
        MemoryStoreQueryResult::ContextTree(tree)
            if tree[0].layer_summary.as_ref().is_some_and(|summary|
                summary.l1_preview.as_deref() == Some("preserved context layer")
                    && !summary.has_l2)
    ));
    let error = store
        .write(MemoryStoreWriteRequest::ContextLayer {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            node_id: "missing-context-node".to_string(),
            layer_type: LayerType::L1,
            content: "orphan".to_string(),
            token_count: 1,
            source_chunk_id: None,
        })
        .await
        .expect_err("orphan context layer must be rejected");
    assert_eq!(error.kind, MemoryStoreErrorKind::NotFound);
    let mut narrowed = MemoryReadScope::tenant(tenant.clone());
    narrowed.subject = Some("user-a".to_string());
    let error = store
        .read(MemoryStoreReadRequest::ContextNode {
            scope: narrowed.clone(),
            uri: "memory://root/dir".to_string(),
        })
        .await
        .expect_err("narrowed entity read must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    let error = store
        .read(MemoryStoreReadRequest::Stats { scope: narrowed })
        .await
        .expect_err("narrowed stats read must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);
    let error = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::all_sessions(),
            limit: None,
        })
        .await
        .expect_err("unconstrained session chunk list must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::InvalidRequest);
    let mut narrowed_write = MemoryWriteScope::tenant(tenant.clone());
    narrowed_write.subject = Some("user-a".to_string());
    let error = store
        .write(MemoryStoreWriteRequest::ContextNode {
            scope: narrowed_write,
            uri: "memory://root/other.txt".to_string(),
            parent_uri: Some("memory://root".to_string()),
            node_type: NodeType::File,
            metadata: None,
        })
        .await
        .expect_err("narrowed entity write must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::ScopeViolation);

    store
        .write(MemoryStoreWriteRequest::SourceObjectLifecycle {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            record: SourceObjectLifecycleRecord {
                source_object_id: "object-1".to_string(),
                tenant_scope: tenant.clone(),
                source_binding_id: "binding-1".to_string(),
                connector_id: "connector-1".to_string(),
                state: SourceObjectLifecycleState::Active,
                tier: MemoryTier::Project,
                session_id: None,
                project_id: Some("project".to_string()),
                import_namespace: "namespace".to_string(),
                indexed_path: "object.md".to_string(),
                native_object_id: "native-1".to_string(),
                resource_ref: serde_json::json!({"id": "native-1"}),
                data_class: "internal".to_string(),
                content_hash: Some("hash".to_string()),
                source_hash: Some("source-hash".to_string()),
                first_seen_at_ms: 1,
                last_seen_at_ms: 1,
                tombstoned_at_ms: None,
                metadata: None,
            },
        })
        .await
        .expect("write source lifecycle fixture");
    let tombstoned = store
        .mutate(MemoryStoreMutationRequest::TombstoneSourceObjectLifecycle {
            scope: MemoryReadScope::tenant(tenant.clone()),
            source_binding_id: "binding-1".to_string(),
            native_object_id: "native-1".to_string(),
            tombstoned_at_ms: 10,
        })
        .await
        .expect("tombstone source lifecycle");
    assert!(matches!(
        tombstoned,
        MemoryStoreMutationResult::Changed(true)
    ));
    let lifecycle = store
        .query(MemoryStoreQueryRequest::SourceObjectLifecyclesForBinding {
            scope: MemoryReadScope::tenant(tenant),
            source_binding_id: "binding-1".to_string(),
        })
        .await
        .expect("read tombstoned source lifecycle");
    assert!(matches!(
        lifecycle,
        MemoryStoreQueryResult::SourceObjectLifecycles(records)
            if records.len() == 1
                && records[0].state == SourceObjectLifecycleState::Tombstoned
                && records[0].tombstoned_at_ms == Some(10)
    ));
}

#[tokio::test]
async fn postgres_atomic_batch_rolls_back_on_primary_key_failure() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("atomic");
    let first = global_record("duplicate", &tenant);
    let mut conflicting = first.clone();
    conflicting.content = "different payload".to_string();
    conflicting.content_hash = "different-hash".to_string();

    store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: vec![
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant.clone()),
                    record: first,
                }),
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant.clone()),
                    record: conflicting,
                }),
            ],
        })
        .await
        .expect_err("duplicate key must abort the transaction");

    let read = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            id: "duplicate".to_string(),
        })
        .await
        .expect("read after rollback");
    assert!(matches!(read, MemoryStoreReadResult::GlobalRecord(None)));

    let first = global_record("dedupe-first", &tenant);
    let mut equivalent = first.clone();
    equivalent.id = "dedupe-second".to_string();
    let result = store
        .batch(MemoryStoreBatchRequest {
            mode: MemoryStoreBatchMode::Atomic,
            operations: vec![
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant.clone()),
                    record: first,
                }),
                MemoryStoreBatchOperation::Write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant),
                    record: equivalent,
                }),
            ],
        })
        .await
        .expect("atomic equivalent records dedupe");
    assert!(matches!(
        &result.items[1].result,
        Ok(MemoryStoreBatchValue::Write(MemoryStoreWriteResult::GlobalRecord(result)))
            if result.deduped && !result.stored && result.id == "dedupe-first"
    ));
}

#[tokio::test]
async fn postgres_concurrent_global_writes_dedupe_without_error() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("concurrent-dedupe");
    let first = global_record("concurrent-first", &tenant);
    let mut second = first.clone();
    second.id = "concurrent-second".to_string();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let spawn_write = |store: PostgresMemoryStore,
                       barrier: std::sync::Arc<tokio::sync::Barrier>,
                       tenant: MemoryTenantScope,
                       record: GlobalMemoryRecord| {
        tokio::spawn(async move {
            barrier.wait().await;
            store
                .write(MemoryStoreWriteRequest::GlobalRecord {
                    scope: MemoryWriteScope::tenant(tenant),
                    record,
                })
                .await
        })
    };
    let first_write = spawn_write(store.clone(), barrier.clone(), tenant.clone(), first);
    let second_write = spawn_write(store, barrier.clone(), tenant, second);
    barrier.wait().await;
    let first_result = first_write
        .await
        .expect("join first write")
        .expect("first write");
    let second_result = second_write
        .await
        .expect("join second write")
        .expect("second write");
    let results = [first_result, second_result]
        .into_iter()
        .map(|result| match result {
            MemoryStoreWriteResult::GlobalRecord(result) => result,
            other => panic!("expected global write result, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.stored).count(), 1);
    assert_eq!(results.iter().filter(|result| result.deduped).count(), 1);
    assert_eq!(results[0].id, results[1].id);
}

#[tokio::test]
async fn postgres_encrypted_mode_seals_payloads_and_reranks_in_scope() {
    let Some(url) = test_url() else {
        return;
    };
    let temp = tempfile::TempDir::new().expect("create key directory");
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "local-file");
    std::env::set_var(
        "TANDEM_MEMORY_LOCAL_KEY_FILE",
        temp.path().join("memory.key"),
    );
    std::env::set_var(
        "TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID",
        "postgres-test-runtime",
    );
    let mut encrypted_config = config(url.clone(), 4);
    encrypted_config.search_surface_mode = PostgresSearchSurfaceMode::EncryptedRerank;
    encrypted_config.rerank_candidate_limit = 2;
    let store = PostgresMemoryStore::connect(encrypted_config)
        .await
        .expect("open encrypted PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("encrypted");
    for (id, embedding) in [
        ("less-similar", vec![0.4, 0.6, 0.0]),
        ("best", vec![0.9, 0.1, 0.0]),
    ] {
        let mut encrypted_chunk = chunk(id, tenant.clone());
        encrypted_chunk.source = "file".to_string();
        encrypted_chunk.source_path = Some("encrypted-source".to_string());
        encrypted_chunk.metadata = Some(serde_json::json!({
            "enterprise_source_binding": {
                "binding_id": "drive-finance",
                "data_class": "confidential"
            }
        }));
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: encrypted_chunk,
                embedding,
            })
            .await
            .expect("write encrypted PostgreSQL chunk");
    }

    let client = store.client().await.expect("inspect raw PostgreSQL rows");
    let raw = client
        .query_one(
            "SELECT embedding IS NULL,data IS NULL,embedding_ciphertext,data_ciphertext,
                    data_class,source_binding_id
             FROM tandem_memory_chunks WHERE id='best'",
            &[],
        )
        .await
        .expect("read raw encrypted row");
    assert!(raw.get::<_, bool>(0));
    assert!(raw.get::<_, bool>(1));
    assert!(raw.get::<_, String>(2).starts_with("tce1:"));
    assert!(raw.get::<_, String>(3).starts_with("tce1:"));
    assert_eq!(raw.get::<_, String>(4), "confidential");
    assert_eq!(
        raw.get::<_, Option<String>>(5).as_deref(),
        Some("drive-finance")
    );

    let node_id = match store
        .write(MemoryStoreWriteRequest::ContextNode {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            uri: "memory://encrypted/context".to_string(),
            parent_uri: Some("memory://encrypted".to_string()),
            node_type: NodeType::File,
            metadata: None,
        })
        .await
        .expect("write encrypted context node")
    {
        MemoryStoreWriteResult::ContextNodeCreated(id) => id,
        other => panic!("expected context node id, got {other:?}"),
    };
    store
        .write(MemoryStoreWriteRequest::ContextLayer {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            node_id: node_id.clone(),
            layer_type: LayerType::L2,
            content: "sensitive semantic context".to_string(),
            token_count: 3,
            source_chunk_id: None,
        })
        .await
        .expect("write encrypted context layer");
    let raw_entity = client
        .query_one(
            "SELECT data IS NULL,data_ciphertext FROM tandem_memory_entities
             WHERE entity_type='context_layer' AND key1=$1",
            &[&node_id],
        )
        .await
        .expect("inspect encrypted entity row");
    assert!(raw_entity.get::<_, bool>(0));
    assert!(raw_entity.get::<_, String>(1).starts_with("tce1:"));
    let entity_storage = client
        .query(
            "SELECT entity_type,data IS NULL,data_ciphertext FROM tandem_memory_entities
             WHERE tenant_org_id=$1 AND tenant_workspace_id=$2 AND tenant_deployment_id=$3",
            &[
                &tenant.org_id,
                &tenant.workspace_id,
                &tenant.deployment_id.as_deref().unwrap_or(""),
            ],
        )
        .await
        .expect("inspect all protected entity rows");
    assert!(entity_storage.len() >= 3);
    for row in entity_storage {
        assert!(row.get::<_, bool>(1), "entity remained plaintext");
        assert!(
            row.get::<_, String>(2).starts_with("tce1:"),
            "entity ciphertext was not sealed"
        );
    }
    let layer = store
        .read(MemoryStoreReadRequest::ContextLayer {
            scope: MemoryReadScope::tenant(tenant.clone()),
            node_id: node_id.clone(),
            layer_type: LayerType::L2,
        })
        .await
        .expect("read encrypted context layer");
    assert!(matches!(
        layer,
        MemoryStoreReadResult::ContextLayer(Some(layer))
            if layer.content == "sensitive semantic context"
    ));

    for (id, binding, created_at_ms) in [
        ("allowed-global", "drive-finance", 10),
        ("denied-global-a", "drive-legal", 30),
        ("denied-global-b", "drive-legal", 20),
    ] {
        let mut record = global_record(id, &tenant);
        record.content = format!("mixed grant payload {id}");
        record.created_at_ms = created_at_ms;
        record.metadata = Some(serde_json::json!({
            "enterprise_source_binding": {
                "binding_id": binding,
                "data_class": "confidential"
            }
        }));
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                record,
            })
            .await
            .expect("write mixed-grant encrypted global record");
    }
    let principal = crate::MemoryDecryptPrincipal::retrieval_gateway(
        "finance-reader",
        tenant.clone(),
        vec![tandem_enterprise_contract::DataClass::Confidential],
        vec!["drive-finance".to_string()],
    );
    let records = crate::decrypt_context::with_decrypt_principal(
        principal,
        store.query(MemoryStoreQueryRequest::ListGlobalRecords {
            scope: MemoryReadScope::tenant(tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: None,
            project_tag: None,
            channel_tag: None,
            limit: 1,
            offset: 0,
        }),
    )
    .await
    .expect("list only authorized encrypted global records");
    assert!(matches!(
        records,
        MemoryStoreQueryResult::GlobalRecords(records)
            if records.len() == 1 && records[0].id == "allowed-global"
    ));

    for (id, content, created_at_ms) in [
        ("newest-no-match", "ordinary payload", 60),
        ("newer-no-match", "another ordinary payload", 50),
        ("first-match", "pagination needle one", 40),
        ("second-match", "pagination needle two", 35),
    ] {
        let mut record = global_record(id, &tenant);
        record.content = content.to_string();
        record.created_at_ms = created_at_ms;
        record.metadata = Some(serde_json::json!({
            "enterprise_source_binding": {
                "binding_id": "drive-finance",
                "data_class": "confidential"
            }
        }));
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                record,
            })
            .await
            .expect("write encrypted pagination fixture");
    }
    let principal = crate::MemoryDecryptPrincipal::retrieval_gateway(
        "finance-reader",
        tenant.clone(),
        vec![tandem_enterprise_contract::DataClass::Confidential],
        vec!["drive-finance".to_string()],
    );
    let records = crate::decrypt_context::with_decrypt_principal(
        principal,
        store.query(MemoryStoreQueryRequest::ListGlobalRecords {
            scope: MemoryReadScope::tenant(tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: Some("pagination needle".to_string()),
            project_tag: None,
            channel_tag: None,
            limit: 1,
            offset: 1,
        }),
    )
    .await
    .expect("paginate after encrypted global filtering");
    assert!(matches!(
        records,
        MemoryStoreQueryResult::GlobalRecords(records)
            if records.len() == 1 && records[0].id == "second-match"
    ));

    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 1,
        })
        .await
        .expect("rerank encrypted candidates");
    let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
        panic!("expected vector hits");
    };
    assert_eq!(hits[0].0.id, "best");

    store
        .mutate(
            MemoryStoreMutationRequest::UpdateChunkMetadataBySourcePath {
                scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
                selector: MemoryChunkSelector::project("project"),
                source_path: "encrypted-source".to_string(),
                metadata: serde_json::json!({
                    "owner_org_unit_id": "legal",
                    "tenant_shared": true,
                    "enterprise_source_binding": {
                        "binding_id": "drive-legal",
                        "data_class": "confidential"
                    }
                }),
            },
        )
        .await
        .expect("rekey encrypted PostgreSQL payloads and embeddings");
    let rekeyed_scope = client
        .query_one(
            "SELECT data_class,source_binding_id,owner_org_unit_id,tenant_shared FROM tandem_memory_chunks WHERE id='best'",
            &[],
        )
        .await
        .expect("inspect rekeyed encrypted row");
    assert_eq!(rekeyed_scope.get::<_, String>(0), "confidential");
    assert_eq!(
        rekeyed_scope.get::<_, Option<String>>(1).as_deref(),
        Some("drive-legal")
    );
    assert_eq!(
        rekeyed_scope.get::<_, Option<String>>(2).as_deref(),
        Some("legal")
    );
    assert!(rekeyed_scope.get::<_, bool>(3));
    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 1,
        })
        .await
        .expect("search rekeyed encrypted candidates");
    assert!(matches!(
        result,
        MemoryStoreQueryResult::SimilarChunks(hits) if hits[0].0.id == "best"
    ));

    store
        .write(MemoryStoreWriteRequest::GlobalRecord {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            record: global_record("encrypted-fts", &tenant),
        })
        .await
        .expect("write encrypted global record");
    let raw_global = client
        .query_one(
            "SELECT data IS NULL,search_content,data_ciphertext
             FROM tandem_memory_global_records WHERE id='encrypted-fts'",
            &[],
        )
        .await
        .expect("read raw encrypted global row");
    assert!(raw_global.get::<_, bool>(0));
    assert_eq!(raw_global.get::<_, String>(1), "");
    assert!(raw_global.get::<_, String>(2).starts_with("tce1:"));
    let global = store
        .query(MemoryStoreQueryRequest::SearchGlobalRecords {
            scope: MemoryReadScope::tenant(tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: "global record".to_string(),
            limit: 5,
            project_tag: None,
        })
        .await
        .expect("search encrypted global records");
    let MemoryStoreQueryResult::GlobalSearchHits(global) = global else {
        panic!("expected global hits");
    };
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].record.id, "encrypted-fts");

    let mut disabled_config = config(url.clone(), 4);
    disabled_config.search_surface_mode = PostgresSearchSurfaceMode::Disabled;
    let disabled_store = PostgresMemoryStore::connect(disabled_config)
        .await
        .expect("open disabled-search PostgreSQL store");
    let error = disabled_store
        .query(MemoryStoreQueryRequest::SearchGlobalRecords {
            scope: MemoryReadScope::tenant(tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: "global record".to_string(),
            limit: 5,
            project_tag: None,
        })
        .await
        .expect_err("disabled global search must fail closed");
    assert_eq!(error.kind, MemoryStoreErrorKind::Unsupported);

    let plaintext_search_store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open local-encrypted plaintext-search store");
    plaintext_search_store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset plaintext-search fixtures");
    let plaintext_search_tenant = MemoryTenantScope {
        org_id: "local-encrypted-plaintext-search".to_string(),
        workspace_id: "workspace".to_string(),
        deployment_id: Some("deployment".to_string()),
    };
    plaintext_search_store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(plaintext_search_tenant.clone()),
            chunk: chunk("local-encrypted", plaintext_search_tenant),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write local-encrypted plaintext-search chunk");
    let raw = plaintext_search_store
        .client()
        .await
        .expect("inspect plaintext-search storage")
        .query_one(
            "SELECT embedding IS NOT NULL,data IS NULL,data_ciphertext
             FROM tandem_memory_chunks WHERE id='local-encrypted'",
            &[],
        )
        .await
        .expect("read local-encrypted plaintext-search row");
    assert!(raw.get::<_, bool>(0));
    assert!(raw.get::<_, bool>(1));
    assert!(raw.get::<_, String>(2).starts_with("tce1:"));

    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PROVIDER");
    std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");
    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");
}

#[tokio::test]
async fn postgres_consolidation_is_exactly_scoped_and_atomic() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("consolidation");
    let owner_write = MemoryWriteScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("alice".to_string()),
    };
    let owner_read = MemoryReadScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("alice".to_string()),
        access: MemoryReadAccess::Scoped,
    };
    for id in ["source-a", "source-b"] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: owner_write.clone(),
                chunk: owned_chunk(id, MemoryTier::Session, tenant.clone(), "alice"),
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("seed consolidation source");
    }
    let summary = owned_chunk("summary", MemoryTier::Project, tenant.clone(), "alice");
    let result = store
        .mutate(MemoryStoreMutationRequest::ReplaceSessionWithSummary {
            scope: owner_read.clone(),
            session_id: "session".to_string(),
            project_id: "project".to_string(),
            source_chunk_ids: vec!["source-a".to_string(), "source-b".to_string()],
            summary_scope: owner_write.clone(),
            summary: Box::new(summary),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("replace session with summary");
    assert!(matches!(result, MemoryStoreMutationResult::Affected(2)));
    let project = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: owner_read.clone(),
            selector: MemoryChunkSelector::project("project"),
            limit: None,
        })
        .await
        .expect("read summary");
    let MemoryStoreReadResult::Chunks(project) = project else {
        panic!("expected chunks");
    };
    assert_eq!(
        project
            .iter()
            .map(|chunk| chunk.id.as_str())
            .collect::<Vec<_>>(),
        vec!["summary"]
    );

    let own = owned_chunk("rollback-own", MemoryTier::Session, tenant.clone(), "alice");
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: owner_write.clone(),
            chunk: own,
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("seed rollback owner source");
    let peer_write = MemoryWriteScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("bob".to_string()),
    };
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: peer_write,
            chunk: owned_chunk("rollback-peer", MemoryTier::Session, tenant.clone(), "bob"),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("seed rollback peer source");
    let error = store
        .mutate(MemoryStoreMutationRequest::ReplaceSessionWithSummary {
            scope: owner_read.clone(),
            session_id: "session".to_string(),
            project_id: "project".to_string(),
            source_chunk_ids: vec!["rollback-own".to_string(), "rollback-peer".to_string()],
            summary_scope: owner_write,
            summary: Box::new(owned_chunk(
                "rollback-summary",
                MemoryTier::Project,
                tenant,
                "alice",
            )),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect_err("peer source must roll back consolidation");
    assert_eq!(error.kind, MemoryStoreErrorKind::Conflict);
    let sources = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: owner_read,
            selector: MemoryChunkSelector::session("session"),
            limit: None,
        })
        .await
        .expect("read rollback source");
    let MemoryStoreReadResult::Chunks(sources) = sources else {
        panic!("expected chunks");
    };
    assert!(sources.iter().any(|chunk| chunk.id == "rollback-own"));
}

#[tokio::test]
async fn postgres_denies_cross_department_and_cross_subject_reads() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("ownership");
    let alice_write = MemoryWriteScope {
        tenant: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("alice".to_string()),
    };
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: alice_write,
            chunk: owned_chunk(
                "alice-finance",
                MemoryTier::Project,
                tenant.clone(),
                "alice",
            ),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write owned PostgreSQL chunk");

    for (department, subject) in [("finance", "bob"), ("legal", "alice")] {
        let result = store
            .query(MemoryStoreQueryRequest::SimilarChunks {
                scope: MemoryReadScope {
                    tenant: tenant.clone(),
                    org_unit: Some(department.to_string()),
                    subject: Some(subject.to_string()),
                    access: MemoryReadAccess::Scoped,
                },
                selector: MemoryChunkSelector::project("project"),
                query_embedding: vec![1.0, 0.0, 0.0],
                limit: 10,
            })
            .await
            .expect("query unauthorized PostgreSQL scope");
        let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
            panic!("expected vector results");
        };
        assert!(
            hits.is_empty(),
            "{department}/{subject} crossed ownership scope"
        );
    }

    let mut tenant_shared = owned_chunk(
        "tenant-shared-legal",
        MemoryTier::Project,
        tenant.clone(),
        "alice",
    );
    tenant_shared.metadata = Some(serde_json::json!({
        "owner_org_unit_id": "legal",
        "tenant_shared": true
    }));
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope {
                tenant: tenant.clone(),
                org_unit: Some("legal".to_string()),
                subject: Some("alice".to_string()),
            },
            chunk: tenant_shared,
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write tenant-shared PostgreSQL chunk");
    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope {
                tenant: tenant.clone(),
                org_unit: Some("finance".to_string()),
                subject: Some("alice".to_string()),
                access: MemoryReadAccess::Scoped,
            },
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 10,
        })
        .await
        .expect("query tenant-shared PostgreSQL chunk");
    let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
        panic!("expected vector results");
    };
    assert!(hits
        .iter()
        .any(|(chunk, _)| chunk.id == "tenant-shared-legal"));
}

#[tokio::test]
async fn postgres_shared_global_point_reads_and_chunk_upserts_match_contract() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");

    let contract_tenant = tenant("contract");
    store
        .write(MemoryStoreWriteRequest::GlobalRecord {
            scope: MemoryWriteScope::tenant(contract_tenant.clone()),
            record: global_record("tenant-wide-shared", &contract_tenant),
        })
        .await
        .expect("write tenant-wide shared global record");
    let result = store
        .read(MemoryStoreReadRequest::GlobalRecord {
            scope: MemoryReadScope::tenant(contract_tenant.clone()),
            id: "tenant-wide-shared".to_string(),
        })
        .await
        .expect("read tenant-wide shared global record");
    assert!(matches!(
        result,
        MemoryStoreReadResult::GlobalRecord(Some(record)) if record.id == "tenant-wide-shared"
    ));
    store
        .mutate(MemoryStoreMutationRequest::UpdateGlobalRecordContext {
            scope: MemoryReadScope::tenant(contract_tenant.clone()),
            id: "tenant-wide-shared".to_string(),
            visibility: "shared".to_string(),
            demoted: true,
            metadata: None,
            provenance: None,
        })
        .await
        .expect("demote tenant-wide shared global record");
    let result = store
        .query(MemoryStoreQueryRequest::ListGlobalRecords {
            scope: MemoryReadScope::tenant(contract_tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: Some("   ".to_string()),
            project_tag: None,
            channel_tag: None,
            limit: 10,
            offset: 0,
        })
        .await
        .expect("list demoted record with blank query");
    assert!(matches!(
        result,
        MemoryStoreQueryResult::GlobalRecords(records)
            if records.iter().any(|record| record.id == "tenant-wide-shared" && record.demoted)
    ));
    let result = store
        .query(MemoryStoreQueryRequest::SearchGlobalRecords {
            scope: MemoryReadScope::tenant(contract_tenant.clone()),
            user_id: "legacy-user".to_string(),
            query: "tenant-wide".to_string(),
            limit: 10,
            project_tag: None,
        })
        .await
        .expect("search excludes demoted records");
    assert!(matches!(
        result,
        MemoryStoreQueryResult::GlobalSearchHits(records) if records.is_empty()
    ));

    let mut source_match = global_record("source-filter-record", &contract_tenant);
    source_match.source_type = "incident_note".to_string();
    let mut run_match = global_record("run-filter-record", &contract_tenant);
    run_match.run_id = "workflow-run-42".to_string();
    for record in [source_match, run_match] {
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope::tenant(contract_tenant.clone()),
                record,
            })
            .await
            .expect("write global list filter fixture");
    }
    for (query, expected_id) in [
        ("incident_note", "source-filter-record"),
        ("workflow-run-42", "run-filter-record"),
    ] {
        let result = store
            .query(MemoryStoreQueryRequest::ListGlobalRecords {
                scope: MemoryReadScope::tenant(contract_tenant.clone()),
                user_id: "legacy-user".to_string(),
                query: Some(query.to_string()),
                project_tag: None,
                channel_tag: None,
                limit: 10,
                offset: 0,
            })
            .await
            .expect("filter global list by source/run");
        assert!(matches!(
            result,
            MemoryStoreQueryResult::GlobalRecords(records)
                if records.iter().any(|record| record.id == expected_id)
        ));
    }

    let mut finance_record = global_record("governed-finance", &contract_tenant);
    finance_record.content_hash = "shared-governed-hash".to_string();
    finance_record.message_id = Some("shared-governed-message".to_string());
    finance_record.metadata = Some(serde_json::json!({
        "enterprise_source_binding": {
            "binding_id": "drive-finance",
            "data_class": "confidential"
        }
    }));
    let mut legal_record = finance_record.clone();
    legal_record.id = "governed-legal".to_string();
    legal_record.metadata = Some(serde_json::json!({
        "enterprise_source_binding": {
            "binding_id": "drive-legal",
            "data_class": "financial"
        }
    }));
    for record in [finance_record, legal_record] {
        let result = store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope::tenant(contract_tenant.clone()),
                record,
            })
            .await
            .expect("write governed dedupe record");
        assert!(matches!(
            result,
            MemoryStoreWriteResult::GlobalRecord(result) if result.stored && !result.deduped
        ));
    }
    let governed_count: i64 = store
        .client()
        .await
        .expect("inspect governed dedupe records")
        .query_one(
            "SELECT COUNT(*) FROM tandem_memory_global_records WHERE content_hash='shared-governed-hash'",
            &[],
        )
        .await
        .expect("count governed dedupe records")
        .get(0);
    assert_eq!(governed_count, 2);

    let mut original = chunk("scope-collision", contract_tenant.clone());
    original.content = "original payload".to_string();
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(contract_tenant.clone()),
            chunk: original,
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write original chunk");
    let mut indexed = chunk("indexed-source-upsert", contract_tenant.clone());
    indexed.source = "connector".to_string();
    indexed.source_path = Some("old-path".to_string());
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(contract_tenant.clone()),
            chunk: indexed.clone(),
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write indexed chunk columns");
    indexed.source = "file".to_string();
    indexed.source_path = Some("new-path".to_string());
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(contract_tenant.clone()),
            chunk: indexed,
            embedding: vec![0.0, 1.0, 0.0],
        })
        .await
        .expect("update indexed chunk columns");
    let indexed_columns = store
        .client()
        .await
        .expect("inspect indexed chunk columns")
        .query_one(
            "SELECT source,source_path FROM tandem_memory_chunks WHERE id='indexed-source-upsert'",
            &[],
        )
        .await
        .expect("read indexed chunk columns");
    assert_eq!(indexed_columns.get::<_, String>(0), "file");
    assert_eq!(
        indexed_columns.get::<_, Option<String>>(1).as_deref(),
        Some("new-path")
    );
    let deleted = store
        .mutate(MemoryStoreMutationRequest::DeleteChunksBySourcePath {
            scope: MemoryReadScope::tenant(contract_tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            source_path: "new-path".to_string(),
        })
        .await
        .expect("delete chunk by updated indexed path");
    assert!(matches!(
        deleted,
        MemoryStoreMutationResult::SourcePathDelete(result) if result.chunks_deleted == 1
    ));
    let other_tenant = tenant("other-contract");
    let mut conflicting = chunk("scope-collision", other_tenant.clone());
    conflicting.content = "cross-scope replacement".to_string();
    let error = store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(other_tenant),
            chunk: conflicting,
            embedding: vec![0.0, 1.0, 0.0],
        })
        .await
        .expect_err("cross-scope chunk upsert must be rejected");
    assert_eq!(error.kind, MemoryStoreErrorKind::Conflict);

    let result = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(contract_tenant),
            selector: MemoryChunkSelector::project("project"),
            limit: None,
        })
        .await
        .expect("read original chunk after rejected collision");
    let MemoryStoreReadResult::Chunks(chunks) = result else {
        panic!("expected chunks");
    };
    assert!(chunks
        .iter()
        .any(|chunk| chunk.id == "scope-collision" && chunk.content == "original payload"));
}

#[tokio::test]
async fn postgres_clear_operations_preserve_other_memory_tiers() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 4))
        .await
        .expect("open PostgreSQL test store");
    store
        .recover_backend(MemoryBackendRecoveryRequest {
            action: MemoryBackendRecoveryAction::ResetAllData,
            confirm_data_loss: true,
        })
        .await
        .expect("reset PostgreSQL fixtures");
    let tenant = tenant("tier-clears");

    let mut session = chunk("session-row", tenant.clone());
    session.tier = MemoryTier::Session;
    session.session_id = Some("shared-session".to_string());
    let mut project = chunk("project-row", tenant.clone());
    project.session_id = Some("shared-session".to_string());
    for row in [session, project] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: row,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("write tier-clear fixture");
    }
    store
        .mutate(MemoryStoreMutationRequest::ClearSession {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            session_id: "shared-session".to_string(),
        })
        .await
        .expect("clear session tier");
    let project_rows = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            limit: None,
        })
        .await
        .expect("read project after session clear");
    assert!(matches!(
        project_rows,
        MemoryStoreReadResult::Chunks(rows) if rows.iter().any(|row| row.id == "project-row")
    ));

    let mut session = chunk("session-row-2", tenant.clone());
    session.tier = MemoryTier::Session;
    session.session_id = Some("active-session".to_string());
    store
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            chunk: session,
            embedding: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("write active session fixture");
    store
        .mutate(MemoryStoreMutationRequest::ClearProject {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            project_id: "project".to_string(),
        })
        .await
        .expect("clear project tier");
    let session_rows = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::session("active-session"),
            limit: None,
        })
        .await
        .expect("read session after project clear");
    assert!(matches!(
        session_rows,
        MemoryStoreReadResult::Chunks(rows) if rows.iter().any(|row| row.id == "session-row-2")
    ));

    let mut old_session = chunk("old-session", tenant.clone());
    old_session.tier = MemoryTier::Session;
    old_session.session_id = Some("old-session".to_string());
    old_session.created_at = chrono::Utc::now() - chrono::Duration::days(2);
    let mut old_project = chunk("old-project", tenant.clone());
    old_project.created_at = chrono::Utc::now() - chrono::Duration::days(2);
    for row in [old_session, old_project] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: row,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("write hygiene fixture");
    }
    store
        .mutate(MemoryStoreMutationRequest::RunHygiene {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            retention_days: 1,
        })
        .await
        .expect("run session hygiene");
    let project_rows = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            limit: None,
        })
        .await
        .expect("read project after hygiene");
    assert!(matches!(
        project_rows,
        MemoryStoreReadResult::Chunks(rows) if rows.iter().any(|row| row.id == "old-project")
    ));

    let mut active_session = chunk("cap-session", tenant.clone());
    active_session.tier = MemoryTier::Session;
    active_session.session_id = Some("cap-session".to_string());
    for row in [
        active_session,
        chunk("cap-project-a", tenant.clone()),
        chunk("cap-project-b", tenant.clone()),
    ] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: row,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("write project-cap fixture");
    }
    store
        .mutate(MemoryStoreMutationRequest::EnforceProjectChunkCap {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            project_id: "project".to_string(),
            max_chunks: 1,
        })
        .await
        .expect("enforce project cap");
    let sessions = store
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::session("cap-session"),
            limit: None,
        })
        .await
        .expect("read session after project cap");
    assert!(matches!(
        sessions,
        MemoryStoreReadResult::Chunks(rows) if rows.iter().any(|row| row.id == "cap-session")
    ));
    let stats = store
        .read(MemoryStoreReadRequest::ProjectStats {
            scope: MemoryReadScope::tenant(tenant.clone()),
            project_id: "project".to_string(),
        })
        .await
        .expect("read project-tier stats");
    assert!(matches!(
        stats,
        MemoryStoreReadResult::ProjectStats(stats) if stats.project_chunks == 1
    ));

    let mut project_file = chunk("project-file", tenant.clone());
    project_file.source = "file".to_string();
    project_file.source_path = Some("guide.md".to_string());
    let mut project_connector = chunk("project-connector", tenant.clone());
    project_connector.source = "connector".to_string();
    project_connector.source_path = Some("guide.md".to_string());
    let mut session_file = chunk("session-file", tenant.clone());
    session_file.tier = MemoryTier::Session;
    session_file.session_id = Some("file-session".to_string());
    session_file.source = "file".to_string();
    session_file.source_path = Some("guide.md".to_string());
    for row in [project_file, project_connector, session_file] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: row,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("write file-clear fixture");
    }
    store
        .write(MemoryStoreWriteRequest::ProjectIndexStatus {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            project_id: "project".to_string(),
            total_files: 8,
            processed_files: 7,
            indexed_files: 5,
            skipped_files: 1,
            errors: 1,
        })
        .await
        .expect("write project index status");
    let stats = store
        .read(MemoryStoreReadRequest::ProjectStats {
            scope: MemoryReadScope::tenant(tenant.clone()),
            project_id: "project".to_string(),
        })
        .await
        .expect("read file and index stats");
    assert!(matches!(
        stats,
        MemoryStoreReadResult::ProjectStats(stats)
            if stats.project_chunks == 3
                && stats.file_index_chunks == 1
                && stats.last_indexed_at.is_some()
                && stats.last_total_files == Some(8)
                && stats.last_processed_files == Some(7)
                && stats.last_indexed_files == Some(5)
                && stats.last_skipped_files == Some(1)
                && stats.last_errors == Some(1)
    ));
    let mut narrowed_scope = MemoryReadScope::tenant(tenant.clone());
    narrowed_scope.org_unit = Some("finance".to_string());
    let source_path_error = store
        .mutate(MemoryStoreMutationRequest::DeleteChunksBySourcePath {
            scope: narrowed_scope.clone(),
            selector: MemoryChunkSelector::project("project"),
            source_path: "guide.md".to_string(),
        })
        .await
        .expect_err("narrowed source-path cleanup must fail closed");
    assert_eq!(source_path_error.kind, MemoryStoreErrorKind::ScopeViolation);
    let metadata_error = store
        .mutate(
            MemoryStoreMutationRequest::UpdateChunkMetadataBySourcePath {
                scope: narrowed_scope.clone(),
                selector: MemoryChunkSelector::project("project"),
                source_path: "guide.md".to_string(),
                metadata: serde_json::json!({"owner_org_unit_id": "finance"}),
            },
        )
        .await
        .expect_err("narrowed source-path metadata update must fail closed");
    assert_eq!(metadata_error.kind, MemoryStoreErrorKind::ScopeViolation);
    let narrowed_error = store
        .mutate(MemoryStoreMutationRequest::ClearProjectFileIndex {
            scope: narrowed_scope,
            project_id: "project".to_string(),
            vacuum: false,
        })
        .await
        .expect_err("narrowed file-index cleanup must fail closed");
    assert_eq!(narrowed_error.kind, MemoryStoreErrorKind::ScopeViolation);
    let unscoped_selector_error = store
        .mutate(MemoryStoreMutationRequest::DeleteChunksBySourcePath {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            selector: MemoryChunkSelector {
                tier: MemoryTier::Project,
                project_id: None,
                session_id: None,
            },
            source_path: "guide.md".to_string(),
        })
        .await
        .expect_err("unconstrained source-path delete must fail closed");
    assert_eq!(
        unscoped_selector_error.kind,
        MemoryStoreErrorKind::InvalidRequest
    );
    let source_path_deleted = store
        .mutate(MemoryStoreMutationRequest::DeleteChunksBySourcePath {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            source_path: "guide.md".to_string(),
        })
        .await
        .expect("delete file chunks by source path");
    assert!(matches!(
        source_path_deleted,
        MemoryStoreMutationResult::SourcePathDelete(result) if result.chunks_deleted == 1
    ));
    let cleared = store
        .mutate(MemoryStoreMutationRequest::ClearProjectFileIndex {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            project_id: "project".to_string(),
            vacuum: true,
        })
        .await
        .expect("clear project file index");
    assert!(matches!(
        cleared,
        MemoryStoreMutationResult::ClearFileIndex(result) if result.did_vacuum
    ));
    let client = store.client().await.expect("inspect file-clear rows");
    let ids = client
        .query(
            "SELECT id FROM tandem_memory_chunks WHERE id IN ('project-file','project-connector','session-file') ORDER BY id",
            &[],
        )
        .await
        .expect("query file-clear rows")
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["project-connector", "session-file"]);

    let mut global_config = crate::types::MemoryConfig::default();
    global_config.exchange_retention_days = 1;
    global_config.global_retention_days = 1;
    store
        .write(MemoryStoreWriteRequest::ProjectConfig {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            project_id: "__global__".to_string(),
            config: global_config,
        })
        .await
        .expect("write global hygiene config");
    let mut project_config = crate::types::MemoryConfig::default();
    project_config.max_chunks = 1;
    store
        .write(MemoryStoreWriteRequest::ProjectConfig {
            scope: MemoryWriteScope::tenant(tenant.clone()),
            project_id: "hygiene-project".to_string(),
            config: project_config,
        })
        .await
        .expect("write project hygiene config");

    let mut expired = global_record("hygiene-expired", &tenant);
    expired.expires_at_ms =
        Some((chrono::Utc::now() - chrono::Duration::days(1)).timestamp_millis() as u64);
    let mut old_exchange = global_record("hygiene-exchange", &tenant);
    old_exchange.source_type = "user_message".to_string();
    old_exchange.created_at_ms =
        (chrono::Utc::now() - chrono::Duration::days(2)).timestamp_millis() as u64;
    for record in [expired, old_exchange] {
        store
            .write(MemoryStoreWriteRequest::GlobalRecord {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                record,
            })
            .await
            .expect("write global hygiene fixture");
    }
    let mut old_global = chunk("hygiene-global", tenant.clone());
    old_global.tier = MemoryTier::Global;
    old_global.project_id = None;
    old_global.created_at = chrono::Utc::now() - chrono::Duration::days(2);
    let mut project_a = chunk("hygiene-project-a", tenant.clone());
    project_a.project_id = Some("hygiene-project".to_string());
    project_a.created_at = chrono::Utc::now() - chrono::Duration::minutes(2);
    let mut project_b = chunk("hygiene-project-b", tenant.clone());
    project_b.project_id = Some("hygiene-project".to_string());
    for row in [old_global, project_a, project_b] {
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: row,
                embedding: vec![1.0, 0.0, 0.0],
            })
            .await
            .expect("write chunk hygiene fixture");
    }
    store
        .mutate(MemoryStoreMutationRequest::RunHygieneAllTenants { retention_days: 1 })
        .await
        .expect("run complete PostgreSQL hygiene");
    let remaining: i64 = client
        .query_one(
            "SELECT
               (SELECT COUNT(*) FROM tandem_memory_global_records
                 WHERE id IN ('hygiene-expired','hygiene-exchange'))
             + (SELECT COUNT(*) FROM tandem_memory_chunks WHERE id='hygiene-global')
             + (SELECT GREATEST(COUNT(*) - 1, 0) FROM tandem_memory_chunks
                 WHERE project_id='hygiene-project' AND tier='project')",
            &[],
        )
        .await
        .expect("inspect complete hygiene result")
        .get(0);
    assert_eq!(remaining, 0);
    let cleanup = store
        .query(MemoryStoreQueryRequest::CleanupLog {
            scope: MemoryReadScope::tenant(tenant),
            limit: 20,
        })
        .await
        .expect("read PostgreSQL cleanup evidence");
    assert!(matches!(
        cleanup,
        MemoryStoreQueryResult::CleanupLog(rows)
            if rows.iter().any(|row| row.cleanup_type == "hygiene_expired_records")
                && rows.iter().any(|row| row.cleanup_type == "hygiene_project_cap")
                && rows.iter().any(|row| row.cleanup_type == "hygiene_global_retention")
    ));
}

#[tokio::test]
async fn postgres_migrations_are_restart_safe_and_reject_dimension_drift() {
    let Some(url) = test_url() else {
        return;
    };
    PostgresMemoryStore::connect(config(url.clone(), 2))
        .await
        .expect("apply PostgreSQL migrations");
    PostgresMemoryStore::connect(config(url.clone(), 2))
        .await
        .expect("reapply PostgreSQL migrations after restart");

    let mut drifted = config(url, 2);
    drifted.embedding_dimension = 4;
    let error = PostgresMemoryStore::connect(drifted)
        .await
        .expect_err("dimension drift must fail startup");
    assert_eq!(error.kind, MemoryStoreErrorKind::InvalidRequest);
    assert!(error.message.contains("dimension mismatch"));
}

#[tokio::test]
async fn postgres_health_degrades_when_embedding_dimension_check_fails() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 2))
        .await
        .expect("open PostgreSQL health test store");
    let mut drifted = store.clone();
    drifted.embedding_dimension = 4;
    let health = drifted
        .backend_health(MemoryBackendHealthRequest { repair: false })
        .await
        .expect("probe drifted PostgreSQL health");
    assert_eq!(health.status, MemoryBackendHealthStatus::Degraded);
    assert!(health
        .checks
        .iter()
        .any(|check| check.name == "embedding_dimension" && !check.healthy));
}

#[tokio::test]
async fn postgres_pool_exhaustion_returns_retryable_unavailable() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(config(url, 1))
        .await
        .expect("open one-connection PostgreSQL pool");
    let held = store.client().await.expect("hold PostgreSQL connection");
    let error = store
        .client()
        .await
        .expect_err("pool acquisition must time out");
    assert_eq!(error.kind, MemoryStoreErrorKind::Unavailable);
    assert!(error.retryable);
    drop(held);
}

#[tokio::test]
async fn postgres_outage_fails_connect_without_hanging() {
    let mut unavailable = config(
        "postgres://postgres:tandem@127.0.0.1:1/tandem?connect_timeout=1".to_string(),
        1,
    );
    unavailable.pool_wait_timeout = std::time::Duration::from_millis(100);
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        PostgresMemoryStore::connect(unavailable),
    )
    .await
    .expect("outage handling exceeded its deadline")
    .expect_err("unreachable PostgreSQL must fail startup");
    assert_eq!(error.kind, MemoryStoreErrorKind::Unavailable);
    assert!(error.retryable);
}
