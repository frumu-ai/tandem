use super::*;
use crate::types::{GlobalMemoryRecord, MemoryChunk, MemoryTenantScope, MemoryTier};

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
    let store = PostgresMemoryStore::connect(PostgresMemoryStoreConfig {
        url,
        embedding_dimension: 3,
        distance_metric: PostgresDistanceMetric::Cosine,
        max_pool_size: 4,
        search_surface_mode: PostgresSearchSurfaceMode::PlaintextPgvector,
        rerank_candidate_limit: 100,
    })
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

    let result = store
        .query(MemoryStoreQueryRequest::SimilarChunks {
            scope: MemoryReadScope::tenant(tenant.clone()),
            selector: MemoryChunkSelector::project("project"),
            query_embedding: vec![1.0, 0.0, 0.0],
            limit: 1,
        })
        .await
        .expect("run scoped pgvector query");
    let MemoryStoreQueryResult::SimilarChunks(hits) = result else {
        panic!("expected vector results");
    };
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0.id, "target");
}

#[tokio::test]
async fn postgres_atomic_batch_rolls_back_on_primary_key_failure() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(PostgresMemoryStoreConfig {
        url,
        embedding_dimension: 3,
        distance_metric: PostgresDistanceMetric::Cosine,
        max_pool_size: 4,
        search_surface_mode: PostgresSearchSurfaceMode::PlaintextPgvector,
        rerank_candidate_limit: 100,
    })
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
            scope: MemoryReadScope::trusted_unrestricted(tenant),
            id: "duplicate".to_string(),
        })
        .await
        .expect("read after rollback");
    assert!(matches!(read, MemoryStoreReadResult::GlobalRecord(None)));
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
    let store = PostgresMemoryStore::connect(PostgresMemoryStoreConfig {
        url,
        embedding_dimension: 3,
        distance_metric: PostgresDistanceMetric::Cosine,
        max_pool_size: 4,
        search_surface_mode: PostgresSearchSurfaceMode::EncryptedRerank,
        rerank_candidate_limit: 100,
    })
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
        store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: MemoryWriteScope::tenant(tenant.clone()),
                chunk: chunk(id, tenant.clone()),
                embedding,
            })
            .await
            .expect("write encrypted PostgreSQL chunk");
    }

    let client = store.client().await.expect("inspect raw PostgreSQL rows");
    let raw = client
        .query_one(
            "SELECT embedding IS NULL,data IS NULL,embedding_ciphertext,data_ciphertext
             FROM tandem_memory_chunks WHERE id='best'",
            &[],
        )
        .await
        .expect("read raw encrypted row");
    assert!(raw.get::<_, bool>(0));
    assert!(raw.get::<_, bool>(1));
    assert!(raw.get::<_, String>(2).starts_with("tce1:"));
    assert!(raw.get::<_, String>(3).starts_with("tce1:"));

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
            scope: MemoryReadScope::tenant(tenant),
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

    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PROVIDER");
    std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");
    std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");
}

#[tokio::test]
async fn postgres_consolidation_is_exactly_scoped_and_atomic() {
    let Some(url) = test_url() else {
        return;
    };
    let store = PostgresMemoryStore::connect(PostgresMemoryStoreConfig {
        url,
        embedding_dimension: 3,
        distance_metric: PostgresDistanceMetric::Cosine,
        max_pool_size: 4,
        search_surface_mode: PostgresSearchSurfaceMode::PlaintextPgvector,
        rerank_candidate_limit: 100,
    })
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
