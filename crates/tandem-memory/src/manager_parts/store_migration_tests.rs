use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use super::*;
use crate::store::*;
use crate::types::DEFAULT_EMBEDDING_DIMENSION;

#[derive(Default)]
struct RecordingStore {
    events: StdMutex<Vec<String>>,
}

impl RecordingStore {
    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }

    fn node(uri: String, node_type: NodeType) -> MemoryNode {
        MemoryNode {
            id: format!("node-{node_type:?}"),
            uri,
            parent_uri: None,
            node_type,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: None,
        }
    }
}

#[async_trait]
impl MemoryStore for RecordingStore {
    async fn read(
        &self,
        request: MemoryStoreReadRequest,
    ) -> MemoryStoreResult<MemoryStoreReadResult> {
        match request {
            MemoryStoreReadRequest::ProjectConfig { scope, project_id } => {
                self.record(format!(
                    "read-config:{project_id}:{}:{}",
                    scope.tenant.org_id, scope.tenant.workspace_id
                ));
                Ok(MemoryStoreReadResult::ProjectConfig(MemoryConfig::default()))
            }
            MemoryStoreReadRequest::ContextNode { scope, uri } => {
                self.record(format!("read-node:{uri}:{}", scope.tenant.org_id));
                Ok(MemoryStoreReadResult::ContextNode(Some(Self::node(
                    uri,
                    NodeType::Directory,
                ))))
            }
            MemoryStoreReadRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
            } => {
                self.record(format!(
                    "read-layer:{node_id}:{layer_type:?}:{}",
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreReadResult::ContextLayer(None))
            }
            MemoryStoreReadRequest::Chunks {
                scope, selector, ..
            } => {
                self.record(format!(
                    "read-chunks:{:?}:{}:{}:{}",
                    selector.tier,
                    scope.subject.as_deref().unwrap_or("shared"),
                    scope.org_unit.as_deref().unwrap_or("all-org-units"),
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreReadResult::Chunks(Vec::new()))
            }
            _ => Err(MemoryStoreError::unsupported("unexpected read")),
        }
    }

    async fn query(
        &self,
        request: MemoryStoreQueryRequest,
    ) -> MemoryStoreResult<MemoryStoreQueryResult> {
        match request {
            MemoryStoreQueryRequest::SimilarChunks {
                scope, selector, ..
            } => {
                let selector_name = if selector == MemoryChunkSelector::all_sessions() {
                    "all-sessions".to_string()
                } else {
                    format!("{:?}", selector.tier)
                };
                self.record(format!(
                    "query:{selector_name}:{}:{}",
                    scope.subject.as_deref().unwrap_or("shared"),
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreQueryResult::SimilarChunks(Vec::new()))
            }
            MemoryStoreQueryRequest::ContextNodes { scope, parent_uri } => {
                self.record(format!("list-nodes:{parent_uri}:{}", scope.tenant.org_id));
                Ok(MemoryStoreQueryResult::ContextNodes(vec![Self::node(
                    format!("{parent_uri}/notes.md"),
                    NodeType::File,
                )]))
            }
            MemoryStoreQueryRequest::ContextTree {
                scope,
                parent_uri,
                max_depth,
            } => {
                self.record(format!(
                    "tree:{parent_uri}:{max_depth}:{}",
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreQueryResult::ContextTree(Vec::new()))
            }
            _ => Err(MemoryStoreError::unsupported("unexpected query")),
        }
    }

    async fn write(
        &self,
        request: MemoryStoreWriteRequest,
    ) -> MemoryStoreResult<MemoryStoreWriteResult> {
        match request {
            MemoryStoreWriteRequest::ProjectConfig {
                scope, project_id, ..
            } => {
                self.record(format!("write-config:{project_id}:{}", scope.tenant.org_id));
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ContextNode { scope, uri, .. } => {
                self.record(format!("write-node:{uri}:{}", scope.tenant.org_id));
                Ok(MemoryStoreWriteResult::ContextNodeCreated(
                    "created-node".to_string(),
                ))
            }
            MemoryStoreWriteRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
                ..
            } => {
                self.record(format!(
                    "write-layer:{node_id}:{layer_type:?}:{}",
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreWriteResult::ContextLayerCreated(
                    "created-layer".to_string(),
                ))
            }
            MemoryStoreWriteRequest::CleanupLog { scope, entry } => {
                self.record(format!(
                    "cleanup-log:{}:{:?}:{}:{}",
                    entry.cleanup_type, entry.tier, entry.chunks_deleted, scope.tenant.org_id
                ));
                Ok(MemoryStoreWriteResult::Stored)
            }
            _ => Err(MemoryStoreError::unsupported("unexpected write")),
        }
    }

    async fn mutate(
        &self,
        request: MemoryStoreMutationRequest,
    ) -> MemoryStoreResult<MemoryStoreMutationResult> {
        match request {
            MemoryStoreMutationRequest::ClearSession { scope, session_id } => {
                self.record(format!(
                    "clear-session:{session_id}:{}",
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreMutationResult::Affected(3))
            }
            MemoryStoreMutationRequest::RunHygiene {
                scope,
                retention_days,
            } => {
                self.record(format!("hygiene:{retention_days}:{}", scope.tenant.org_id));
                Ok(MemoryStoreMutationResult::Affected(101))
            }
            MemoryStoreMutationRequest::EnforceProjectChunkCap {
                scope,
                project_id,
                max_chunks,
            } => {
                self.record(format!(
                    "project-cap:{project_id}:{max_chunks}:{}",
                    scope.tenant.org_id
                ));
                Ok(MemoryStoreMutationResult::Affected(2))
            }
            MemoryStoreMutationRequest::Vacuum => {
                self.record("vacuum".to_string());
                Ok(MemoryStoreMutationResult::Completed)
            }
            _ => Err(MemoryStoreError::unsupported("unexpected mutation")),
        }
    }

    async fn batch(
        &self,
        _request: MemoryStoreBatchRequest,
    ) -> MemoryStoreResult<MemoryStoreBatchResult> {
        Err(MemoryStoreError::unsupported("unused in manager test"))
    }

    async fn backend_health(
        &self,
        _request: MemoryBackendHealthRequest,
    ) -> MemoryStoreResult<MemoryBackendHealthResult> {
        Err(MemoryStoreError::unsupported("unused in manager test"))
    }

    async fn recover_backend(
        &self,
        request: MemoryBackendRecoveryRequest,
    ) -> MemoryStoreResult<MemoryBackendRecoveryResult> {
        self.record(format!(
            "recover:{:?}:{}",
            request.action, request.confirm_data_loss
        ));
        Ok(MemoryBackendRecoveryResult {
            backend: MemoryBackendKind::Other("recording".to_string()),
            action: request.action,
            changed: true,
        })
    }

    async fn migration_capabilities(
        &self,
        _request: MemoryMigrationCapabilityRequest,
    ) -> MemoryStoreResult<MemoryMigrationCapabilityResult> {
        Err(MemoryStoreError::unsupported("unused in manager test"))
    }
}

#[tokio::test]
async fn manager_routes_portable_operations_through_memory_store() {
    let store = Arc::new(RecordingStore::default());
    let manager = MemoryManager::new_with_store(
        store.clone(),
        EmbeddingService::deterministic_for_tests(DEFAULT_EMBEDDING_DIMENSION),
    )
    .unwrap();
    let tenant = MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };

    manager
        .get_config_for_tenant("project-a", &tenant)
        .await
        .unwrap();
    manager
        .set_config_for_tenant("project-a", &MemoryConfig::default(), &tenant)
        .await
        .unwrap();

    let access_filter =
        crate::types::MemoryAccessFilter::local_noop(1).with_caller_subject("user-a");
    manager
        .search_for_tenant_with_access_filter(
            "portable search",
            Some(MemoryTier::Global),
            None,
            None,
            &tenant,
            Some(2),
            Some(&access_filter),
        )
        .await
        .unwrap();
    manager
        .read_chunks(
            MemoryChunkSelector::session("session-a"),
            MemoryReadScope {
                tenant: tenant.clone(),
                org_unit: Some("finance".to_string()),
                subject: Some("user-a".to_string()),
                access: MemoryReadAccess::Scoped,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        manager
            .clear_session_for_tenant("session-a", &tenant)
            .await
            .unwrap(),
        3
    );

    assert_eq!(
        *store.events.lock().unwrap(),
        vec![
            "read-config:project-a:org-a:workspace-a",
            "write-config:project-a:org-a",
            "query:Global:user-a:org-a",
            "read-chunks:Session:user-a:finance:org-a",
            "clear-session:session-a:org-a",
            "cleanup-log:manual:Session:3:org-a",
        ]
    );
}

#[tokio::test]
async fn manager_routes_context_operations_through_memory_store() {
    let store = Arc::new(RecordingStore::default());
    let manager = MemoryManager::new_with_store(
        store.clone(),
        EmbeddingService::deterministic_for_tests(DEFAULT_EMBEDDING_DIMENSION),
    )
    .unwrap();
    let tenant = MemoryTenantScope {
        org_id: "org-context".to_string(),
        workspace_id: "workspace-context".to_string(),
        deployment_id: None,
    };
    let root = "tandem://resources/project-a";

    assert!(manager.resolve_uri(root, &tenant).await.unwrap().is_some());
    assert_eq!(
        manager
            .list_directory(root, &tenant)
            .await
            .unwrap()
            .total_children,
        1
    );
    assert!(manager.tree(root, 2, &tenant).await.unwrap().is_empty());
    assert_eq!(
        manager
            .create_context_node(root, NodeType::Directory, None, &tenant)
            .await
            .unwrap(),
        "created-node"
    );
    assert!(manager
        .get_context_layer("node-a", LayerType::L1, &tenant)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        manager
            .store_content_with_layers(
                "tandem://resources/project-a/notes.md",
                "portable context",
                None,
                &tenant,
            )
            .await
            .unwrap(),
        "created-node"
    );

    assert_eq!(
        *store.events.lock().unwrap(),
        vec![
            "read-node:tandem://resources/project-a:org-context",
            "list-nodes:tandem://resources/project-a:org-context",
            "tree:tandem://resources/project-a:2:org-context",
            "write-node:tandem://resources/project-a:org-context",
            "read-layer:node-a:L1:org-context",
            "write-node:tandem://resources/project-a/notes.md:org-context",
            "write-layer:created-node:L2:org-context",
        ]
    );
}

#[tokio::test]
async fn manager_routes_maintenance_and_recovery_through_memory_store() {
    let store = Arc::new(RecordingStore::default());
    let manager = MemoryManager::new_with_store(
        store.clone(),
        EmbeddingService::deterministic_for_tests(DEFAULT_EMBEDDING_DIMENSION),
    )
    .unwrap();
    let tenant = MemoryTenantScope {
        org_id: "org-maintenance".to_string(),
        workspace_id: "workspace-maintenance".to_string(),
        deployment_id: None,
    };

    manager
        .search_for_tenant(
            "all session history",
            Some(MemoryTier::Session),
            None,
            None,
            &tenant,
            Some(2),
        )
        .await
        .unwrap();
    assert_eq!(
        manager.run_cleanup_for_tenant(None, &tenant).await.unwrap(),
        101
    );
    manager
        .maybe_cleanup(&Some("project-a".to_string()), &tenant)
        .await
        .unwrap();
    assert!(manager.repair_store().await);
    manager.reset_store().await.unwrap();

    assert_eq!(
        *store.events.lock().unwrap(),
        vec![
            "query:all-sessions:shared:org-maintenance",
            "hygiene:30:org-maintenance",
            "cleanup-log:auto:Session:101:org-maintenance",
            "vacuum",
            "read-config:project-a:org-maintenance:workspace-maintenance",
            "project-cap:project-a:10000:org-maintenance",
            "cleanup-log:auto:Project:2:org-maintenance",
            "recover:RepairIndexes:false",
            "recover:ResetAllData:true",
        ]
    );
}
