use async_trait::async_trait;

use crate::db::MemoryDatabase;
use crate::types::{
    owner_org_unit_id_from_metadata, owner_subject_from_metadata, MemoryError, MemoryTier,
};

use super::*;

fn reject_unimplemented_narrowing(scope: &MemoryReadScope) -> MemoryStoreResult<()> {
    if scope.org_unit.is_some() || scope.subject.is_some() {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "this operation has no org-unit/subject predicate in the SQLite adapter; refusing to widen the scope",
        ));
    }
    Ok(())
}

fn reject_unimplemented_write_narrowing(scope: &MemoryWriteScope) -> MemoryStoreResult<()> {
    if scope.org_unit.is_some() || scope.subject.is_some() {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "this operation cannot persist org-unit/subject scope in the SQLite adapter",
        ));
    }
    Ok(())
}

fn validate_chunk_selector(selector: &MemoryChunkSelector) -> MemoryStoreResult<()> {
    match selector.tier {
        MemoryTier::Session if selector.session_id.as_deref().is_none_or(str::is_empty) => Err(
            MemoryStoreError::invalid("tier=session requires a non-empty session_id"),
        ),
        MemoryTier::Project if selector.project_id.as_deref().is_none_or(str::is_empty) => Err(
            MemoryStoreError::invalid("tier=project requires a non-empty project_id"),
        ),
        _ => Ok(()),
    }
}

fn validate_chunk_search_selector(selector: &MemoryChunkSelector) -> MemoryStoreResult<()> {
    match selector.tier {
        MemoryTier::Session if selector.session_id.as_deref().is_some_and(str::is_empty) => Err(
            MemoryStoreError::invalid("session_id must be non-empty when provided"),
        ),
        MemoryTier::Project | MemoryTier::Global => validate_chunk_selector(selector),
        MemoryTier::Session => Ok(()),
    }
}

fn validate_chunk_write_scope(
    scope: &MemoryWriteScope,
    chunk: &crate::types::MemoryChunk,
) -> MemoryStoreResult<()> {
    if scope.tenant != chunk.tenant_scope {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "chunk tenant scope does not match the write request scope",
        ));
    }
    if scope.subject != chunk.subject {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "chunk subject does not match the write request scope",
        ));
    }
    if scope.org_unit != owner_org_unit_id_from_metadata(chunk.metadata.as_ref()) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "chunk owner_org_unit_id metadata does not match the write request scope",
        ));
    }
    Ok(())
}

fn validate_global_write_scope(
    scope: &MemoryWriteScope,
    record: &crate::types::GlobalMemoryRecord,
) -> MemoryStoreResult<()> {
    if scope.tenant != tenant_scope_from_global_record(record) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record tenant context does not match the write request scope",
        ));
    }
    if scope.org_unit != owner_org_unit_id_from_metadata(record.metadata.as_ref()) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record owner_org_unit_id metadata does not match the write request scope",
        ));
    }
    if scope.subject != owner_subject_from_metadata(record.metadata.as_ref()) {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "global record owner_subject metadata does not match the write request scope",
        ));
    }
    Ok(())
}

fn validate_source_object_write_scope(
    scope: &MemoryWriteScope,
    record: &crate::types::SourceObjectLifecycleRecord,
) -> MemoryStoreResult<()> {
    reject_unimplemented_write_narrowing(scope)?;
    if scope.tenant != record.tenant_scope {
        return Err(MemoryStoreError::new(
            MemoryStoreErrorKind::ScopeViolation,
            "source object tenant scope does not match the write request scope",
        ));
    }
    Ok(())
}

#[async_trait]
impl MemoryStore for MemoryDatabase {
    async fn read(
        &self,
        request: MemoryStoreReadRequest,
    ) -> MemoryStoreResult<MemoryStoreReadResult> {
        match request {
            MemoryStoreReadRequest::Chunks {
                scope,
                selector,
                limit,
            } => {
                validate_chunk_selector(&selector)?;
                let chunks = self
                    .get_chunks_for_tenant_scoped(
                        selector.tier,
                        selector.project_id.as_deref(),
                        selector.session_id.as_deref(),
                        &scope.tenant,
                        scope.subject.as_deref(),
                        scope.org_unit.as_deref(),
                        limit.unwrap_or(1000),
                    )
                    .await?;
                Ok(MemoryStoreReadResult::Chunks(chunks))
            }
            MemoryStoreReadRequest::GlobalRecord { scope, id } => {
                self.enforce_store_tenant_scope("global memory read", &scope.tenant)?;
                let record = match scope.access {
                    MemoryReadAccess::Scoped => {
                        self.get_global_memory_for_tenant_scoped(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                            scope.org_unit.as_deref(),
                            scope.subject.as_deref(),
                        )
                        .await?
                    }
                    MemoryReadAccess::TrustedUnrestricted => {
                        self.get_global_memory_for_tenant(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                        )
                        .await?
                    }
                };
                Ok(MemoryStoreReadResult::GlobalRecord(record))
            }
            MemoryStoreReadRequest::ProjectConfig { scope, project_id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::ProjectConfig(
                    self.get_or_create_config_for_tenant(&project_id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreReadRequest::Stats { scope } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::Stats(
                    self.get_stats_for_tenant(&scope.tenant).await?,
                ))
            }
            MemoryStoreReadRequest::ProjectStats { scope, project_id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::ProjectStats(
                    self.get_project_stats_for_tenant(&project_id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreReadRequest::KnowledgeSpace { scope, id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::KnowledgeSpace(
                    self.get_knowledge_space_for_tenant(&id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreReadRequest::KnowledgeItem { scope, id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::KnowledgeItem(
                    self.get_knowledge_item_for_tenant(&id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreReadRequest::KnowledgeCoverage {
                scope,
                coverage_key,
                space_id,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::KnowledgeCoverage(
                    self.get_knowledge_coverage_for_tenant(&coverage_key, &space_id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreReadRequest::ImportIndexEntry {
                scope,
                selector,
                path,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                let entry = self
                    .get_import_index_entry_for_tenant(
                        selector.tier,
                        selector.session_id.as_deref(),
                        selector.project_id.as_deref(),
                        &path,
                        &scope.tenant,
                    )
                    .await?
                    .map(|(modified_at, size, hash)| MemoryImportIndexEntry {
                        modified_at,
                        size,
                        hash,
                    });
                Ok(MemoryStoreReadResult::ImportIndexEntry(entry))
            }
            MemoryStoreReadRequest::ContextNode { scope, uri } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::ContextNode(
                    self.get_node_by_uri(&uri, &scope.tenant).await?,
                ))
            }
            MemoryStoreReadRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreReadResult::ContextLayer(
                    self.get_layer(&node_id, layer_type, &scope.tenant).await?,
                ))
            }
        }
    }

    async fn query(
        &self,
        request: MemoryStoreQueryRequest,
    ) -> MemoryStoreResult<MemoryStoreQueryResult> {
        match request {
            MemoryStoreQueryRequest::SimilarChunks {
                scope,
                selector,
                query_embedding,
                limit,
            } => {
                validate_chunk_search_selector(&selector)?;
                let chunks = self
                    .search_similar_for_tenant(
                        &query_embedding,
                        selector.tier,
                        selector.project_id.as_deref(),
                        selector.session_id.as_deref(),
                        &scope.tenant,
                        limit,
                        scope.subject.as_deref(),
                        scope.org_unit.as_deref(),
                    )
                    .await?;
                Ok(MemoryStoreQueryResult::SimilarChunks(chunks))
            }
            MemoryStoreQueryRequest::SearchGlobalRecords {
                scope,
                user_id: _,
                query,
                limit,
                project_tag,
            } => {
                self.enforce_store_tenant_scope("global memory search", &scope.tenant)?;
                Ok(MemoryStoreQueryResult::GlobalSearchHits(
                    self.search_global_memory_for_tenant_scoped(
                        &scope.tenant.org_id,
                        &scope.tenant.workspace_id,
                        scope.tenant.deployment_id.as_deref(),
                        scope.subject.as_deref(),
                        &query,
                        limit,
                        project_tag.as_deref(),
                        None,
                        None,
                        scope.org_unit.as_deref(),
                    )
                    .await?,
                ))
            }
            MemoryStoreQueryRequest::ListGlobalRecords {
                scope,
                user_id: _,
                query,
                project_tag,
                channel_tag,
                limit,
                offset,
            } => {
                self.enforce_store_tenant_scope("global memory list", &scope.tenant)?;
                Ok(MemoryStoreQueryResult::GlobalRecords(
                    self.list_global_memory_for_tenant_scoped(
                        &scope.tenant.org_id,
                        &scope.tenant.workspace_id,
                        scope.tenant.deployment_id.as_deref(),
                        scope.subject.as_deref(),
                        query.as_deref(),
                        project_tag.as_deref(),
                        channel_tag.as_deref(),
                        limit,
                        offset,
                        scope.org_unit.as_deref(),
                    )
                    .await?,
                ))
            }
            MemoryStoreQueryRequest::KnowledgeSpaces { scope, project_id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::KnowledgeSpaces(
                    self.list_knowledge_spaces_for_tenant(project_id.as_deref(), &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreQueryRequest::KnowledgeItems {
                scope,
                space_id,
                coverage_key,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::KnowledgeItems(
                    self.list_knowledge_items_for_tenant(
                        &space_id,
                        coverage_key.as_deref(),
                        &scope.tenant,
                    )
                    .await?,
                ))
            }
            MemoryStoreQueryRequest::ImportIndexPaths { scope, selector } => {
                reject_unimplemented_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                Ok(MemoryStoreQueryResult::Paths(
                    self.list_import_index_paths_for_tenant(
                        selector.tier,
                        selector.session_id.as_deref(),
                        selector.project_id.as_deref(),
                        &scope.tenant,
                    )
                    .await?,
                ))
            }
            MemoryStoreQueryRequest::CleanupLog { scope, limit } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::CleanupLog(
                    self.get_cleanup_log_for_tenant(limit, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreQueryRequest::ContextNodes { scope, parent_uri } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::ContextNodes(
                    self.list_directory(&parent_uri, &scope.tenant).await?,
                ))
            }
            MemoryStoreQueryRequest::ContextTree {
                scope,
                parent_uri,
                max_depth,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::ContextTree(
                    self.get_children_tree(&parent_uri, max_depth, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreQueryRequest::SourceObjectLifecyclesForBinding {
                scope,
                source_binding_id,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreQueryResult::SourceObjectLifecycles(
                    self.list_source_object_lifecycle_for_binding_for_tenant(
                        &scope.tenant,
                        &source_binding_id,
                    )
                    .await?,
                ))
            }
        }
    }

    async fn write(
        &self,
        request: MemoryStoreWriteRequest,
    ) -> MemoryStoreResult<MemoryStoreWriteResult> {
        match request {
            MemoryStoreWriteRequest::Chunk {
                scope,
                chunk,
                embedding,
            } => {
                validate_chunk_write_scope(&scope, &chunk)?;
                self.store_chunk(&chunk, &embedding).await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::GlobalRecord { scope, record } => {
                self.enforce_store_tenant_scope("global memory write", &scope.tenant)?;
                validate_global_write_scope(&scope, &record)?;
                Ok(MemoryStoreWriteResult::GlobalRecord(
                    self.put_global_memory_record(&record).await?,
                ))
            }
            MemoryStoreWriteRequest::ProjectConfig {
                scope,
                project_id,
                config,
            } => {
                reject_unimplemented_write_narrowing(&scope)?;
                self.update_config_for_tenant(&project_id, &config, &scope.tenant)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeSpace { scope, record } => {
                reject_unimplemented_write_narrowing(&scope)?;
                self.upsert_knowledge_space_for_tenant(&record, &scope.tenant)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeItem { scope, record } => {
                reject_unimplemented_write_narrowing(&scope)?;
                self.upsert_knowledge_item_for_tenant(&record, &scope.tenant)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::KnowledgeCoverage { scope, record } => {
                reject_unimplemented_write_narrowing(&scope)?;
                self.upsert_knowledge_coverage_for_tenant(&record, &scope.tenant)
                    .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ImportIndexEntry {
                scope,
                selector,
                path,
                entry,
            } => {
                reject_unimplemented_write_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                self.upsert_import_index_entry_for_tenant(
                    selector.tier,
                    selector.session_id.as_deref(),
                    selector.project_id.as_deref(),
                    &path,
                    entry.modified_at,
                    entry.size,
                    &entry.hash,
                    &scope.tenant,
                )
                .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::SourceObjectLifecycle { scope, record } => {
                validate_source_object_write_scope(&scope, &record)?;
                self.upsert_source_object_active_for_tenant(&record).await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
            MemoryStoreWriteRequest::ContextNode {
                scope,
                uri,
                parent_uri,
                node_type,
                metadata,
            } => {
                reject_unimplemented_write_narrowing(&scope)?;
                Ok(MemoryStoreWriteResult::ContextNodeCreated(
                    self.create_node(
                        &uri,
                        parent_uri.as_deref(),
                        node_type,
                        metadata.as_ref(),
                        &scope.tenant,
                    )
                    .await?,
                ))
            }
            MemoryStoreWriteRequest::ContextLayer {
                scope,
                node_id,
                layer_type,
                content,
                token_count,
                source_chunk_id,
            } => {
                reject_unimplemented_write_narrowing(&scope)?;
                Ok(MemoryStoreWriteResult::ContextLayerCreated(
                    self.create_layer(
                        &node_id,
                        layer_type,
                        &content,
                        token_count,
                        source_chunk_id.as_deref(),
                        &scope.tenant,
                    )
                    .await?,
                ))
            }
            MemoryStoreWriteRequest::CleanupLog { scope, entry } => {
                reject_unimplemented_write_narrowing(&scope)?;
                self.log_cleanup_for_tenant(
                    &entry.cleanup_type,
                    entry.tier,
                    entry.project_id.as_deref(),
                    entry.session_id.as_deref(),
                    entry.chunks_deleted,
                    entry.bytes_reclaimed,
                    &scope.tenant,
                )
                .await?;
                Ok(MemoryStoreWriteResult::Stored)
            }
        }
    }

    async fn mutate(
        &self,
        request: MemoryStoreMutationRequest,
    ) -> MemoryStoreResult<MemoryStoreMutationResult> {
        match request {
            MemoryStoreMutationRequest::DeleteChunk {
                scope,
                selector,
                chunk_id,
            } => {
                validate_chunk_selector(&selector)?;
                self.enforce_store_tenant_scope("memory chunk delete", &scope.tenant)?;
                let deleted = match scope.access {
                    MemoryReadAccess::Scoped => {
                        self.delete_chunk_for_tenant_scoped(
                            selector.tier,
                            &chunk_id,
                            selector.project_id.as_deref(),
                            selector.session_id.as_deref(),
                            &scope.tenant,
                            scope.subject.as_deref(),
                            scope.org_unit.as_deref(),
                        )
                        .await?
                    }
                    MemoryReadAccess::TrustedUnrestricted => {
                        self.delete_chunk_for_tenant(
                            selector.tier,
                            &chunk_id,
                            selector.project_id.as_deref(),
                            selector.session_id.as_deref(),
                            &scope.tenant,
                        )
                        .await?
                    }
                };
                Ok(MemoryStoreMutationResult::Affected(deleted))
            }
            MemoryStoreMutationRequest::ClearSession { scope, session_id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Affected(
                    self.clear_session_memory_for_tenant(&session_id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreMutationRequest::ClearProject { scope, project_id } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Affected(
                    self.clear_project_memory_for_tenant(&project_id, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreMutationRequest::DeleteGlobalRecord { scope, id } => {
                self.enforce_store_tenant_scope("global memory delete", &scope.tenant)?;
                let deleted = match scope.access {
                    MemoryReadAccess::Scoped => {
                        self.delete_global_memory_for_tenant_scoped(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                            scope.org_unit.as_deref(),
                            scope.subject.as_deref(),
                        )
                        .await?
                    }
                    MemoryReadAccess::TrustedUnrestricted => {
                        self.delete_global_memory_for_tenant(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                        )
                        .await?
                    }
                };
                Ok(MemoryStoreMutationResult::Changed(deleted))
            }
            MemoryStoreMutationRequest::UpdateGlobalRecordContext {
                scope,
                id,
                visibility,
                demoted,
                metadata,
                provenance,
            } => {
                self.enforce_store_tenant_scope("global memory context update", &scope.tenant)?;
                let updated = match scope.access {
                    MemoryReadAccess::Scoped => {
                        self.update_global_memory_context_for_tenant_scoped(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                            scope.org_unit.as_deref(),
                            scope.subject.as_deref(),
                            &visibility,
                            demoted,
                            metadata.as_ref(),
                            provenance.as_ref(),
                        )
                        .await?
                    }
                    MemoryReadAccess::TrustedUnrestricted => {
                        self.update_global_memory_context_for_tenant(
                            &id,
                            &scope.tenant.org_id,
                            &scope.tenant.workspace_id,
                            scope.tenant.deployment_id.as_deref(),
                            &visibility,
                            demoted,
                            metadata.as_ref(),
                            provenance.as_ref(),
                        )
                        .await?
                    }
                };
                Ok(MemoryStoreMutationResult::Changed(updated))
            }
            MemoryStoreMutationRequest::PromoteKnowledgeItem { scope, request } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Promotion(
                    self.promote_knowledge_item_for_tenant(&request, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreMutationRequest::DeleteImportIndexEntry {
                scope,
                selector,
                path,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                self.delete_import_index_entry_for_tenant(
                    selector.tier,
                    selector.session_id.as_deref(),
                    selector.project_id.as_deref(),
                    &path,
                    &scope.tenant,
                )
                .await?;
                Ok(MemoryStoreMutationResult::Changed(true))
            }
            MemoryStoreMutationRequest::DeleteChunksBySourcePath {
                scope,
                selector,
                source_path,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                let (chunks_deleted, bytes_reclaimed) = self
                    .delete_file_chunks_by_path_for_tenant(
                        selector.tier,
                        selector.session_id.as_deref(),
                        selector.project_id.as_deref(),
                        &source_path,
                        &scope.tenant,
                    )
                    .await?;
                Ok(MemoryStoreMutationResult::SourcePathDelete(
                    MemorySourcePathDeleteResult {
                        chunks_deleted,
                        bytes_reclaimed,
                    },
                ))
            }
            MemoryStoreMutationRequest::UpdateChunkMetadataBySourcePath {
                scope,
                selector,
                source_path,
                metadata,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                validate_chunk_selector(&selector)?;
                let changed = self
                    .update_file_chunks_metadata_by_path_for_tenant(
                        selector.tier,
                        selector.session_id.as_deref(),
                        selector.project_id.as_deref(),
                        &source_path,
                        &scope.tenant,
                        &metadata,
                    )
                    .await?;
                Ok(MemoryStoreMutationResult::Affected(changed as u64))
            }
            MemoryStoreMutationRequest::TombstoneSourceObjectLifecycle {
                scope,
                source_binding_id,
                native_object_id,
                tombstoned_at_ms,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Changed(
                    self.tombstone_source_object_for_tenant(
                        &scope.tenant,
                        &source_binding_id,
                        &native_object_id,
                        tombstoned_at_ms,
                    )
                    .await?,
                ))
            }
            MemoryStoreMutationRequest::SetSourceObjectLifecycleState {
                scope,
                source_binding_id,
                source_object_id,
                state,
                changed_at_ms,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Changed(
                    self.mark_source_object_lifecycle_state_for_tenant(
                        &scope.tenant,
                        &source_binding_id,
                        &source_object_id,
                        state,
                        changed_at_ms,
                    )
                    .await?,
                ))
            }
            MemoryStoreMutationRequest::RunHygiene {
                scope,
                retention_days,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Affected(
                    self.run_hygiene_for_tenant(retention_days, &scope.tenant)
                        .await?,
                ))
            }
            MemoryStoreMutationRequest::RunHygieneAllTenants { retention_days } => {
                Ok(MemoryStoreMutationResult::Affected(
                    self.run_hygiene_all_tenants(retention_days).await?,
                ))
            }
            MemoryStoreMutationRequest::EnforceProjectChunkCap {
                scope,
                project_id,
                max_chunks,
            } => {
                reject_unimplemented_narrowing(&scope)?;
                Ok(MemoryStoreMutationResult::Affected(
                    self.enforce_project_chunk_cap_for_tenant(
                        &project_id,
                        max_chunks,
                        &scope.tenant,
                    )
                    .await?,
                ))
            }
            MemoryStoreMutationRequest::Vacuum => {
                self.vacuum().await?;
                Ok(MemoryStoreMutationResult::Completed)
            }
        }
    }

    async fn batch(
        &self,
        request: MemoryStoreBatchRequest,
    ) -> MemoryStoreResult<MemoryStoreBatchResult> {
        if request.mode == MemoryStoreBatchMode::Atomic {
            return self.execute_atomic_store_batch(request.operations).await;
        }

        let stop_on_error = request.mode == MemoryStoreBatchMode::StopOnError;
        let mut completed = true;
        let mut items = Vec::with_capacity(request.operations.len());
        for (index, operation) in request.operations.into_iter().enumerate() {
            let result = match operation {
                MemoryStoreBatchOperation::Write(request) => {
                    self.write(request).await.map(MemoryStoreBatchValue::Write)
                }
                MemoryStoreBatchOperation::Mutation(request) => self
                    .mutate(request)
                    .await
                    .map(MemoryStoreBatchValue::Mutation),
            };
            let failed = result.is_err();
            completed &= !failed;
            items.push(MemoryStoreBatchItemResult { index, result });
            if failed && stop_on_error {
                break;
            }
        }
        Ok(MemoryStoreBatchResult { completed, items })
    }

    async fn backend_health(
        &self,
        request: MemoryBackendHealthRequest,
    ) -> MemoryStoreResult<MemoryBackendHealthResult> {
        if request.repair {
            let repaired = self.ensure_vector_tables_healthy().await?;
            return Ok(MemoryBackendHealthResult {
                backend: MemoryBackendKind::Sqlite,
                status: MemoryBackendHealthStatus::Healthy,
                repaired,
                checks: vec![MemoryBackendHealthCheck {
                    name: "vector_index".to_string(),
                    healthy: true,
                    detail: repaired.then(|| "vector tables were rebuilt".to_string()),
                }],
            });
        }

        let check = match self.validate_vector_tables().await {
            Ok(()) => MemoryBackendHealthCheck {
                name: "vector_index".to_string(),
                healthy: true,
                detail: None,
            },
            Err(error) => MemoryBackendHealthCheck {
                name: "vector_index".to_string(),
                healthy: false,
                detail: Some(MemoryStoreError::from(error).to_string()),
            },
        };
        Ok(MemoryBackendHealthResult {
            backend: MemoryBackendKind::Sqlite,
            status: if check.healthy {
                MemoryBackendHealthStatus::Healthy
            } else {
                MemoryBackendHealthStatus::Degraded
            },
            repaired: false,
            checks: vec![check],
        })
    }

    async fn recover_backend(
        &self,
        request: MemoryBackendRecoveryRequest,
    ) -> MemoryStoreResult<MemoryBackendRecoveryResult> {
        let changed = match request.action {
            MemoryBackendRecoveryAction::RepairIndexes => {
                self.ensure_vector_tables_healthy().await?
            }
            MemoryBackendRecoveryAction::ResetAllData => {
                if !request.confirm_data_loss {
                    return Err(MemoryStoreError::invalid(
                        "resetting the memory backend requires confirm_data_loss=true",
                    ));
                }
                self.reset_all_memory_tables().await?;
                true
            }
        };
        Ok(MemoryBackendRecoveryResult {
            backend: MemoryBackendKind::Sqlite,
            action: request.action,
            changed,
        })
    }

    async fn migration_capabilities(
        &self,
        request: MemoryMigrationCapabilityRequest,
    ) -> MemoryStoreResult<MemoryMigrationCapabilityResult> {
        // MemoryDatabase applies its registered migrations while opening, but
        // does not yet expose applied-version inspection or an explicit portable
        // transaction/dry-run boundary to a migration coordinator.
        let mut result = MemoryMigrationCapabilityResult {
            backend: MemoryBackendKind::Sqlite,
            apply_mode: MemoryMigrationApplyMode::OnOpen,
            version_introspection: false,
            transactional_apply: false,
            online_apply: false,
            dry_run: false,
            requirements_satisfied: false,
        };
        result.requirements_satisfied = result.satisfies(&request);
        Ok(result)
    }
}

impl From<MemoryStoreError> for MemoryError {
    fn from(error: MemoryStoreError) -> Self {
        match error.kind {
            MemoryStoreErrorKind::ScopeViolation => {
                MemoryError::TenantScopeViolation(error.message)
            }
            MemoryStoreErrorKind::NotFound => MemoryError::NotFound(error.message),
            _ => MemoryError::InvalidConfig(error.message),
        }
    }
}
