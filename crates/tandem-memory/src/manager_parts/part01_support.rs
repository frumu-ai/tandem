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

fn memory_chunk_visible_to_access_filter(
    chunk: &MemoryChunk,
    access_filter: Option<&crate::types::MemoryAccessFilter>,
) -> bool {
    if access_filter.is_none()
        && crate::types::MemorySourceAccessTarget::from_chunk(chunk).is_none()
        && !crate::knowledge_scope::metadata_has_knowledge_scope(chunk.metadata.as_ref())
    {
        return true;
    }
    access_filter
        .map(|filter| filter.allows_chunk(chunk))
        .unwrap_or(false)
}

/// Create memory manager with default database path.
pub async fn create_memory_manager(app_data_dir: &Path) -> MemoryResult<MemoryManager> {
    let db_path = app_data_dir.join("tandem_memory.db");
    MemoryManager::new(&db_path).await
}
