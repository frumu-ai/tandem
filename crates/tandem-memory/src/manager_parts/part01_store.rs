impl MemoryManager {
    async fn repair_store(&self) -> bool {
        self.store
            .recover_backend(MemoryBackendRecoveryRequest {
                action: MemoryBackendRecoveryAction::RepairIndexes,
                confirm_data_loss: false,
            })
            .await
            .map(|result| result.changed)
            .unwrap_or(false)
    }

    async fn reset_store(&self) -> MemoryResult<()> {
        self.store
            .recover_backend(MemoryBackendRecoveryRequest {
                action: MemoryBackendRecoveryAction::ResetAllData,
                confirm_data_loss: true,
            })
            .await
            .map(|_| ())
            .map_err(MemoryError::from)
    }

    async fn write_cleanup_log(
        &self,
        cleanup_type: &str,
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        chunks_deleted: i64,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        match self
            .store
            .write(MemoryStoreWriteRequest::CleanupLog {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                entry: MemoryCleanupLogWrite {
                    cleanup_type: cleanup_type.to_string(),
                    tier,
                    project_id: project_id.map(ToString::to_string),
                    session_id: session_id.map(ToString::to_string),
                    chunks_deleted,
                    bytes_reclaimed: 0,
                },
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(Self::unexpected_store_result("write cleanup log")),
        }
    }

    async fn write_context_layer(
        &self,
        node_id: &str,
        layer_type: LayerType,
        content: &str,
        token_count: i64,
        source_chunk_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        match self
            .store
            .write(MemoryStoreWriteRequest::ContextLayer {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                node_id: node_id.to_string(),
                layer_type,
                content: content.to_string(),
                token_count,
                source_chunk_id: source_chunk_id.map(ToString::to_string),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::ContextLayerCreated(id) => Ok(id),
            _ => Err(Self::unexpected_store_result("create context layer")),
        }
    }
}
