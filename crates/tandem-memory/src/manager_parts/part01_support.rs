/// Trusted ownership coordinates for turning one session into project memory.
/// Callers derive this from authenticated runtime context, never request-body
/// tenant or subject fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedMemoryConsolidationRequest {
    pub tenant_scope: MemoryTenantScope,
    pub org_unit: Option<String>,
    pub subject: Option<String>,
    pub project_id: String,
    pub session_id: String,
}

fn consolidation_chunk_has_exact_ownership(
    chunk: &MemoryChunk,
    request: &ScopedMemoryConsolidationRequest,
) -> bool {
    chunk.subject == request.subject
        && crate::types::owner_org_unit_id_from_metadata(chunk.metadata.as_ref())
            == request.org_unit
}

fn memory_chunk_visible_to_access_filter(
    chunk: &MemoryChunk,
    access_filter: Option<&crate::types::MemoryAccessFilter>,
) -> bool {
    if access_filter.is_none()
        && crate::types::MemorySourceAccessTarget::from_chunk(chunk).is_none()
        && !crate::knowledge_scope::metadata_has_knowledge_scope(chunk.metadata.as_ref())
        && crate::derived_lineage::DerivedMemoryLineage::from_metadata(chunk.metadata.as_ref()).is_ok_and(|lineage| lineage.is_none())
    {
        return true;
    }
    access_filter
        .map(|filter| filter.allows_chunk(chunk))
        .unwrap_or(false)
}

impl MemoryManager {
    async fn memory_chunk_visible_with_resolved_sources(&self, chunk: &MemoryChunk,
        access_filter: Option<&crate::types::MemoryAccessFilter>, scope: &MemoryReadScope) -> bool {
        let lineage = match crate::derived_lineage::DerivedMemoryLineage::from_metadata(chunk.metadata.as_ref()) {
            Ok(Some(lineage)) => lineage,
            Ok(None) => return memory_chunk_visible_to_access_filter(chunk, access_filter),
            Err(_) => return false,
        };
        let Some(filter) = access_filter else { return false; };
        if let Some(resolver) = &self.derived_memory_access_resolver {
            return resolver(self.store.clone(), scope.clone(), lineage, filter.clone()).await
                .is_some_and(|resolved| resolved.allows_chunk(chunk));
        }
        let inputs = match lineage.all_input_refs() { Ok(inputs) => inputs, Err(_) => return false };
        let has_native_messages = inputs.iter().any(|input| matches!(input,
            crate::derived_lineage::CanonicalInputReference::SessionMessage { .. }));
        if has_native_messages && !filter.resolved_derived_lineages.contains(&lineage) {
            return false;
        }
        match crate::derived_lineage::resolve_derived_lineage(self.store.as_ref(), scope, &lineage).await {
            Ok(proof) => filter.clone().with_resolved_derived_lineage(proof).allows_chunk(chunk),
            Err(_) => false,
        }
    }
}

/// Create memory manager with default database path.
pub async fn create_memory_manager(app_data_dir: &Path) -> MemoryResult<MemoryManager> {
    let db_path = app_data_dir.join("tandem_memory.db");
    MemoryManager::new(&db_path).await
}
