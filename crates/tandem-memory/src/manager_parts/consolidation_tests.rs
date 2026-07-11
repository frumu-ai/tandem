use std::sync::Arc;

use async_trait::async_trait;
use tandem_data_boundary::{DataBoundaryTenantRef, ProviderEgressAuthority};
use tandem_providers::{AppConfig, MemoryConsolidationConfig, Provider, ProviderRegistry};
use tandem_types::ProviderInfo;

use super::*;

struct FailingConsolidationProvider;

#[test]
fn consolidation_ownership_match_excludes_shared_and_peer_chunks() {
    let tenant = MemoryTenantScope::local();
    let request = ScopedMemoryConsolidationRequest {
        tenant_scope: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("alice".to_string()),
        project_id: "project-a".to_string(),
        session_id: "session-a".to_string(),
    };
    let chunk = |id: &str, subject: Option<&str>, org_unit: &str| MemoryChunk {
        id: id.to_string(),
        content: id.to_string(),
        tier: MemoryTier::Session,
        session_id: Some("session-a".to_string()),
        project_id: Some("project-a".to_string()),
        source: "test".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: tenant.clone(),
        subject: subject.map(ToString::to_string),
        created_at: chrono::Utc::now(),
        token_count: 1,
        metadata: Some(serde_json::json!({ "owner_org_unit_id": org_unit })),
    };

    assert!(consolidation_chunk_has_exact_ownership(
        &chunk("owned", Some("alice"), "finance"),
        &request
    ));
    assert!(!consolidation_chunk_has_exact_ownership(
        &chunk("shared", None, "finance"),
        &request
    ));
    assert!(!consolidation_chunk_has_exact_ownership(
        &chunk("peer", Some("bob"), "finance"),
        &request
    ));
    assert!(!consolidation_chunk_has_exact_ownership(
        &chunk("other-unit", Some("alice"), "legal"),
        &request
    ));
}

#[async_trait]
impl Provider for FailingConsolidationProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "failing-consolidation".to_string(),
            name: "Failing consolidation".to_string(),
            models: Vec::new(),
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        anyhow::bail!("provider unavailable")
    }
}

#[tokio::test]
async fn consolidation_provider_failure_leaves_scoped_source_chunks_intact() {
    let temp_dir = tempfile::TempDir::new().expect("temporary memory directory");
    let manager = MemoryManager::new(&temp_dir.path().join("memory.db"))
        .await
        .expect("memory manager");
    let tenant = MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: Some("deployment-a".to_string()),
    };
    let metadata = serde_json::json!({
        "owner_org_unit_id": "finance",
        "owner_subject": "channel:slack:dm:alice"
    });
    let source = MemoryChunk {
        id: "alice-source".to_string(),
        content: "forbidden marker belongs to Alice".to_string(),
        tier: MemoryTier::Session,
        session_id: Some("session-a".to_string()),
        project_id: Some("project-a".to_string()),
        source: "channel_message".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: tenant.clone(),
        subject: Some("channel:slack:dm:alice".to_string()),
        created_at: chrono::Utc::now(),
        token_count: 7,
        metadata: Some(metadata),
    };
    manager
        .store()
        .write(MemoryStoreWriteRequest::Chunk {
            scope: MemoryWriteScope {
                tenant: tenant.clone(),
                org_unit: Some("finance".to_string()),
                subject: Some("channel:slack:dm:alice".to_string()),
            },
            chunk: source,
            embedding: vec![0.0; crate::types::DEFAULT_EMBEDDING_DIMENSION],
        })
        .await
        .expect("seed source chunk");

    let providers = ProviderRegistry::new(AppConfig::default());
    providers
        .replace_for_test(
            vec![Arc::new(FailingConsolidationProvider)],
            Some("failing-consolidation".to_string()),
        )
        .await;
    let authority = ProviderEgressAuthority::new(DataBoundaryTenantRef {
        organization_id: Some(tenant.org_id.clone()),
        workspace_id: Some(tenant.workspace_id.clone()),
        deployment_id: tenant.deployment_id.clone(),
    });
    let egress = MemoryProviderEgressContext::new(authority);
    let request = ScopedMemoryConsolidationRequest {
        tenant_scope: tenant.clone(),
        org_unit: Some("finance".to_string()),
        subject: Some("channel:slack:dm:alice".to_string()),
        project_id: "project-a".to_string(),
        session_id: "session-a".to_string(),
    };

    let result = manager
        .consolidate_scoped_session(
            &request,
            &providers,
            &MemoryConsolidationConfig::default(),
            &egress,
        )
        .await;
    assert!(
        result.as_ref().is_err() || result.as_ref().is_ok_and(Option::is_none),
        "a failed dispatch must not produce a summary"
    );

    let remaining = manager
        .store()
        .read(MemoryStoreReadRequest::Chunks {
            scope: MemoryReadScope {
                tenant,
                org_unit: request.org_unit,
                subject: request.subject,
                access: crate::store::MemoryReadAccess::Scoped,
            },
            selector: MemoryChunkSelector::session_in_project("session-a", "project-a"),
            limit: None,
        })
        .await
        .expect("read source after failed provider call");
    let MemoryStoreReadResult::Chunks(remaining) = remaining else {
        panic!("expected source chunks");
    };
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "alice-source");
}
