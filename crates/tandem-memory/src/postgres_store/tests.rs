use super::*;
use crate::types::{GlobalMemoryRecord, MemoryChunk, MemoryTenantScope, MemoryTier};

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
    let mut encrypted_config = config(url, 4);
    encrypted_config.search_surface_mode = PostgresSearchSurfaceMode::EncryptedRerank;
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

    let mut project_file = chunk("project-file", tenant.clone());
    project_file.source = "file".to_string();
    project_file.source_path = Some("guide.md".to_string());
    let mut project_connector = chunk("project-connector", tenant.clone());
    project_connector.source = "connector".to_string();
    project_connector.source_path = Some("connector/item".to_string());
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
        .mutate(MemoryStoreMutationRequest::ClearProjectFileIndex {
            scope: MemoryReadScope::trusted_unrestricted(tenant.clone()),
            project_id: "project".to_string(),
            vacuum: false,
        })
        .await
        .expect("clear project file index");
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
