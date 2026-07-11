use super::*;
use crate::types::{MemoryChunk, MemoryTenantScope, MemoryTier};

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
            scope: MemoryReadScope::tenant(tenant),
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
