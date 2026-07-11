// Memory Manager Module
// High-level memory operations (store, retrieve, cleanup)

use crate::chunking::{chunk_text_semantic, ChunkingConfig, Tokenizer};
use crate::context_layers::ContextLayerGenerator;
use crate::context_uri::ContextUri;
use crate::db::MemoryDatabase;
use crate::embeddings::EmbeddingService;
use crate::envelope::validate_memory_envelope_for_write;
use crate::provider_egress::{
    complete_memory_prompt, MemoryProviderEgressContext, MemoryProviderEgressKind,
};
use crate::store::{
    MemoryBackendRecoveryAction, MemoryBackendRecoveryRequest, MemoryChunkSelector,
    MemoryCleanupLogWrite, MemoryReadScope, MemoryStore, MemoryStoreError,
    MemoryStoreMutationRequest, MemoryStoreMutationResult, MemoryStoreQueryRequest,
    MemoryStoreQueryResult, MemoryStoreReadRequest, MemoryStoreReadResult, MemoryStoreWriteRequest,
    MemoryStoreWriteResult, MemoryWriteScope,
};
use crate::types::{
    CleanupLogEntry, DirectoryListing, EmbeddingHealth, LayerType, MemoryChunk, MemoryConfig,
    MemoryContext, MemoryError, MemoryLayer, MemoryNode, MemoryResult, MemoryRetrievalMeta,
    MemorySearchResult, MemoryStats, MemoryTenantScope, MemoryTier, NodeType, StoreMessageRequest,
    TreeNode,
};
use chrono::Utc;
use std::path::Path;
use std::sync::Arc;
use tandem_providers::{MemoryConsolidationConfig, ProviderRegistry};
use tokio::sync::Mutex;

/// High-level memory manager that coordinates database, embeddings, and chunking
pub struct MemoryManager {
    store: Arc<dyn MemoryStore>,
    #[cfg(test)]
    compatibility_db: Option<Arc<MemoryDatabase>>,
    embedding_service: Arc<Mutex<EmbeddingService>>,
    tokenizer: Tokenizer,
}

const MAX_KNOWLEDGE_PACK_ITEMS: usize = 3;
const GUIDE_DOC_SOURCE_PREFIX: &str = "guide_docs:";
const GUIDE_DOC_RECENCY_HALFLIFE_MS: f64 = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0;
const GUIDE_DOC_RECENCY_WEIGHT: f64 = 0.12;
const ACCESS_FILTER_CANDIDATE_MULTIPLIER: i64 = 5;

impl MemoryManager {
    fn guide_doc_similarity(similarity: f64, chunk: &MemoryChunk, now_ms: i64) -> f64 {
        if !chunk.source.starts_with(GUIDE_DOC_SOURCE_PREFIX) {
            return similarity.clamp(0.0, 1.0);
        }

        let normalized_similarity = similarity.clamp(0.0, 1.0);
        let source_mtime = chunk
            .source_mtime
            .filter(|value| *value > 0)
            .unwrap_or_else(|| chunk.created_at.timestamp_millis());
        let age_ms = (now_ms - source_mtime).max(0) as f64;
        let recency_score = 1.0 / (1.0 + (age_ms / GUIDE_DOC_RECENCY_HALFLIFE_MS));
        ((1.0 - GUIDE_DOC_RECENCY_WEIGHT) * normalized_similarity
            + (GUIDE_DOC_RECENCY_WEIGHT * recency_score))
            .clamp(0.0, 1.0)
    }

    fn is_malformed_database_error(err: &impl std::fmt::Display) -> bool {
        err.to_string()
            .to_lowercase()
            .contains("database disk image is malformed")
    }

    /// The portable store used by memory business operations.
    pub fn store(&self) -> &Arc<dyn MemoryStore> {
        &self.store
    }

    /// Test-only compatibility access for fixtures that seed the SQLite adapter
    /// directly. Production business code only receives [`MemoryStore`].
    #[cfg(test)]
    pub fn db(&self) -> &Arc<MemoryDatabase> {
        self.compatibility_db.as_ref().expect(
            "MemoryManager::db is only available for managers constructed with a SQLite path",
        )
    }

    /// Initialize the memory manager
    pub async fn new(db_path: &Path) -> MemoryResult<Self> {
        Self::new_with_embedding_service(db_path, EmbeddingService::new()).await
    }

    /// Initialize the memory manager with a caller-provided embedding
    /// service. Tests use this to avoid depending on local model assets.
    pub async fn new_with_embedding_service(
        db_path: &Path,
        embedding_service: EmbeddingService,
    ) -> MemoryResult<Self> {
        let db = Arc::new(MemoryDatabase::new(db_path).await?);
        let store: Arc<dyn MemoryStore> = db.clone();
        let manager = Self::build(store, embedding_service)?;
        #[cfg(test)]
        {
            let mut manager = manager;
            manager.compatibility_db = Some(db);
            return Ok(manager);
        }
        #[cfg(not(test))]
        {
            Ok(manager)
        }
    }

    /// Build a manager over any portable memory store.
    pub fn new_with_store(
        store: Arc<dyn MemoryStore>,
        embedding_service: EmbeddingService,
    ) -> MemoryResult<Self> {
        Self::build(store, embedding_service)
    }

    fn build(
        store: Arc<dyn MemoryStore>,
        embedding_service: EmbeddingService,
    ) -> MemoryResult<Self> {
        let embedding_service = Arc::new(Mutex::new(embedding_service));
        let tokenizer = Tokenizer::new()?;

        Ok(Self {
            store,
            #[cfg(test)]
            compatibility_db: None,
            embedding_service,
            tokenizer,
        })
    }

    fn read_scope(tenant_scope: &MemoryTenantScope) -> MemoryReadScope {
        MemoryReadScope::tenant(tenant_scope.clone())
    }

    fn search_scope(
        tenant_scope: &MemoryTenantScope,
        access_filter: Option<&crate::types::MemoryAccessFilter>,
    ) -> MemoryReadScope {
        let mut scope = Self::read_scope(tenant_scope);
        scope.subject = access_filter.and_then(|filter| filter.caller_subject.clone());
        scope.org_unit = access_filter
            .and_then(|filter| filter.caller_org_units.as_ref())
            .and_then(|units| {
                if units.len() == 1 {
                    units.iter().next().cloned()
                } else {
                    None
                }
            });
        scope
    }

    fn chunk_write_scope(chunk: &MemoryChunk) -> MemoryWriteScope {
        MemoryWriteScope {
            tenant: chunk.tenant_scope.clone(),
            org_unit: crate::types::owner_org_unit_id_from_metadata(chunk.metadata.as_ref()),
            subject: chunk.subject.clone(),
        }
    }

    async fn read_project_config(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryConfig> {
        match self
            .store
            .read(MemoryStoreReadRequest::ProjectConfig {
                scope: Self::read_scope(tenant_scope),
                project_id: project_id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::ProjectConfig(config) => Ok(config),
            _ => Err(Self::unexpected_store_result("read project config")),
        }
    }

    async fn read_chunks(
        &self,
        selector: MemoryChunkSelector,
        scope: MemoryReadScope,
        limit: Option<i64>,
    ) -> MemoryResult<Vec<MemoryChunk>> {
        match self
            .store
            .read(MemoryStoreReadRequest::Chunks {
                scope,
                selector,
                limit,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::Chunks(chunks) => Ok(chunks),
            _ => Err(Self::unexpected_store_result("read chunks")),
        }
    }

    async fn write_chunk(
        &self,
        chunk: &MemoryChunk,
        embedding: &[f32],
    ) -> Result<(), MemoryStoreError> {
        match self
            .store
            .write(MemoryStoreWriteRequest::Chunk {
                scope: Self::chunk_write_scope(chunk),
                chunk: chunk.clone(),
                embedding: embedding.to_vec(),
            })
            .await?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(MemoryStoreError::new(
                crate::store::MemoryStoreErrorKind::Internal,
                "memory store returned the wrong result for write chunk",
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn query_similar_chunks(
        &self,
        query_embedding: &[f32],
        tier: MemoryTier,
        project_id: Option<&str>,
        session_id: Option<&str>,
        scope: &MemoryReadScope,
        limit: i64,
    ) -> Result<Vec<(MemoryChunk, f64)>, MemoryStoreError> {
        let selector = if tier == MemoryTier::Session && session_id.is_none() {
            MemoryChunkSelector::all_sessions()
        } else {
            MemoryChunkSelector {
                tier,
                project_id: project_id.map(ToString::to_string),
                session_id: session_id.map(ToString::to_string),
            }
        };

        match self
            .store
            .query(MemoryStoreQueryRequest::SimilarChunks {
                scope: scope.clone(),
                selector,
                query_embedding: query_embedding.to_vec(),
                limit,
            })
            .await?
        {
            MemoryStoreQueryResult::SimilarChunks(results) => Ok(results),
            _ => Err(MemoryStoreError::new(
                crate::store::MemoryStoreErrorKind::Internal,
                "memory store returned the wrong result for similar-chunk query",
            )),
        }
    }

    fn unexpected_store_result(operation: &str) -> MemoryError {
        MemoryError::InvalidConfig(format!(
            "memory store returned an unexpected result for {operation}"
        ))
    }

    /// Store a message in memory
    ///
    /// This will:
    /// 1. Chunk the message content
    /// 2. Generate embeddings for each chunk
    /// 3. Store chunks and embeddings in the database
    pub async fn store_message(&self, request: StoreMessageRequest) -> MemoryResult<Vec<String>> {
        validate_memory_envelope_for_write(&request.tenant_scope, request.metadata.as_ref())?;

        if self.repair_store().await {
            tracing::warn!("Memory vector tables were repaired before storing message chunks");
        }

        let config = if let Some(ref pid) = request.project_id {
            self.read_project_config(pid, &request.tenant_scope).await?
        } else {
            MemoryConfig::default()
        };

        // Chunk the content
        let chunking_config = ChunkingConfig {
            chunk_size: config.chunk_size as usize,
            chunk_overlap: config.chunk_overlap as usize,
            separator: None,
        };

        let text_chunks = chunk_text_semantic(&request.content, &chunking_config)?;

        if text_chunks.is_empty() {
            return Ok(Vec::new());
        }

        let mut chunk_ids = Vec::with_capacity(text_chunks.len());
        let embedding_service = self.embedding_service.lock().await;

        for text_chunk in text_chunks {
            let chunk_id = uuid::Uuid::new_v4().to_string();

            // Generate embedding
            let embedding = embedding_service.embed(&text_chunk.content).await?;

            // Create memory chunk
            let chunk = MemoryChunk {
                id: chunk_id.clone(),
                content: text_chunk.content,
                tier: request.tier,
                session_id: request.session_id.clone(),
                project_id: request.project_id.clone(),
                source: request.source.clone(),
                source_path: request.source_path.clone(),
                source_mtime: request.source_mtime,
                source_size: request.source_size,
                subject: request
                    .subject
                    .as_deref()
                    .map(str::trim)
                    .filter(|subject| !subject.is_empty())
                    .map(ToString::to_string),
                source_hash: request.source_hash.clone(),
                tenant_scope: request.tenant_scope.clone(),
                created_at: Utc::now(),
                token_count: text_chunk.token_count as i64,
                metadata: request.metadata.clone(),
            };

            // Store through the backend-neutral contract, retrying once after
            // backend-owned vector/index repair.
            if let Err(err) = self.write_chunk(&chunk, &embedding).await {
                tracing::warn!("Failed to store memory chunk {}: {}", chunk.id, err);
                let repaired = self.repair_store().await;
                if repaired {
                    tracing::warn!(
                        "Retrying memory chunk insert after vector table repair: {}",
                        chunk.id
                    );
                    if let Err(retry_err) = self.write_chunk(&chunk, &embedding).await {
                        if Self::is_malformed_database_error(&retry_err) {
                            tracing::warn!(
                                "Memory DB still malformed after vector repair. Resetting memory tables and retrying chunk insert: {}",
                                chunk.id
                            );
                            self.reset_store().await?;
                            self.write_chunk(&chunk, &embedding)
                                .await
                                .map_err(MemoryError::from)?;
                        } else {
                            return Err(retry_err.into());
                        }
                    }
                } else {
                    return Err(err.into());
                }
            }
            chunk_ids.push(chunk_id);
        }

        // Check if cleanup is needed
        if config.auto_cleanup {
            self.maybe_cleanup(&request.project_id, &request.tenant_scope)
                .await?;
        }

        Ok(chunk_ids)
    }

    /// Search memory for relevant chunks
    pub async fn search(
        &self,
        query: &str,
        tier: Option<MemoryTier>,
        project_id: Option<&str>,
        session_id: Option<&str>,
        limit: Option<i64>,
    ) -> MemoryResult<Vec<MemorySearchResult>> {
        self.search_for_tenant(
            query,
            tier,
            project_id,
            session_id,
            &MemoryTenantScope::local(),
            limit,
        )
        .await
    }

    pub async fn search_for_tenant(
        &self,
        query: &str,
        tier: Option<MemoryTier>,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        limit: Option<i64>,
    ) -> MemoryResult<Vec<MemorySearchResult>> {
        self.search_for_tenant_with_access_filter(
            query,
            tier,
            project_id,
            session_id,
            tenant_scope,
            limit,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_for_tenant_with_access_filter(
        &self,
        query: &str,
        tier: Option<MemoryTier>,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        limit: Option<i64>,
        access_filter: Option<&crate::types::MemoryAccessFilter>,
    ) -> MemoryResult<Vec<MemorySearchResult>> {
        let effective_limit = limit.unwrap_or(5);
        let candidate_limit = if access_filter.is_some() {
            effective_limit.saturating_mul(ACCESS_FILTER_CANDIDATE_MULTIPLIER)
        } else {
            effective_limit
        };

        // Generate query embedding
        let embedding_service = self.embedding_service.lock().await;
        let query_embedding = embedding_service.embed(query).await?;
        drop(embedding_service);

        let mut results = Vec::new();

        // Search in specified tier or all tiers
        let tiers_to_search = match tier {
            Some(t) => vec![t],
            None => {
                if project_id.is_some() {
                    vec![MemoryTier::Session, MemoryTier::Project, MemoryTier::Global]
                } else {
                    vec![MemoryTier::Session, MemoryTier::Global]
                }
            }
        };

        let now_ms = Utc::now().timestamp_millis();
        // Push supported scope restrictions into the SQL top-k so a caller's
        // own chunks cannot be starved out of the candidate window by other
        // subjects/departments' closer matches; the access filter below remains
        // as the authoritative post-check.
        let scope = Self::search_scope(tenant_scope, access_filter);
        for search_tier in tiers_to_search {
            let tier_results = match self
                .query_similar_chunks(
                    &query_embedding,
                    search_tier,
                    project_id,
                    session_id,
                    &scope,
                    candidate_limit,
                )
                .await
            {
                Ok(results) => results,
                Err(err) => {
                    tracing::warn!(
                        "Memory tier search failed for {:?}: {}. Attempting vector repair.",
                        search_tier,
                        err
                    );
                    let repaired = self.repair_store().await;
                    if repaired {
                        match self
                            .query_similar_chunks(
                                &query_embedding,
                                search_tier,
                                project_id,
                                session_id,
                                &scope,
                                candidate_limit,
                            )
                            .await
                        {
                            Ok(results) => results,
                            Err(retry_err) => {
                                tracing::warn!(
                                    "Memory tier search still failing for {:?} after repair: {}",
                                    search_tier,
                                    retry_err
                                );
                                continue;
                            }
                        }
                    } else {
                        continue;
                    }
                }
            };

            for (chunk, distance) in tier_results {
                if !memory_chunk_visible_to_access_filter(&chunk, access_filter) {
                    continue;
                }
                // Convert distance to similarity (cosine similarity)
                // sqlite-vec returns distance, where lower is more similar
                // Cosine similarity ranges from -1 to 1, but for normalized vectors it's 0 to 1
                let similarity = 1.0 - distance.clamp(0.0, 1.0);
                let similarity = if search_tier == MemoryTier::Global {
                    Self::guide_doc_similarity(similarity, &chunk, now_ms)
                } else {
                    similarity
                };

                results.push(MemorySearchResult { chunk, similarity });
            }
        }

        // Sort by similarity (highest first) and limit results
        results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        results.truncate(effective_limit as usize);

        Ok(results)
    }

    /// Retrieve context for a message
    ///
    /// This retrieves relevant chunks from all tiers and formats them
    /// for injection into the prompt
    pub async fn retrieve_context(
        &self,
        query: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        token_budget: Option<i64>,
    ) -> MemoryResult<MemoryContext> {
        self.retrieve_context_for_tenant(
            query,
            project_id,
            session_id,
            &MemoryTenantScope::local(),
            token_budget,
        )
        .await
    }

    pub async fn retrieve_context_for_tenant(
        &self,
        query: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        token_budget: Option<i64>,
    ) -> MemoryResult<MemoryContext> {
        let (context, _) = self
            .retrieve_context_with_meta_for_tenant(
                query,
                project_id,
                session_id,
                tenant_scope,
                token_budget,
            )
            .await?;
        Ok(context)
    }

    /// Retrieve context plus retrieval metadata for observability.
    pub async fn retrieve_context_with_meta(
        &self,
        query: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        token_budget: Option<i64>,
    ) -> MemoryResult<(MemoryContext, MemoryRetrievalMeta)> {
        self.retrieve_context_with_meta_for_tenant(
            query,
            project_id,
            session_id,
            &MemoryTenantScope::local(),
            token_budget,
        )
        .await
    }

    pub async fn retrieve_context_with_meta_for_tenant(
        &self,
        query: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        token_budget: Option<i64>,
    ) -> MemoryResult<(MemoryContext, MemoryRetrievalMeta)> {
        self.retrieve_context_with_meta_for_tenant_with_access_filter(
            query,
            project_id,
            session_id,
            tenant_scope,
            token_budget,
            None,
        )
        .await
    }

    pub async fn retrieve_context_with_meta_for_tenant_with_access_filter(
        &self,
        query: &str,
        project_id: Option<&str>,
        session_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
        token_budget: Option<i64>,
        access_filter: Option<&crate::types::MemoryAccessFilter>,
    ) -> MemoryResult<(MemoryContext, MemoryRetrievalMeta)> {
        let config = if let Some(pid) = project_id {
            self.read_project_config(pid, tenant_scope).await?
        } else {
            MemoryConfig::default()
        };
        let budget = token_budget.unwrap_or(config.token_budget);
        let retrieval_limit = config.retrieval_k.max(1);

        // Get recent session chunks
        let current_session = if let Some(sid) = session_id {
            self.read_chunks(
                MemoryChunkSelector::session(sid),
                Self::search_scope(tenant_scope, access_filter),
                None,
            )
            .await?
            .into_iter()
            .filter(|chunk| memory_chunk_visible_to_access_filter(chunk, access_filter))
            .collect()
        } else {
            Vec::new()
        };

        // Search for relevant history
        let search_results = self
            .search_for_tenant_with_access_filter(
                query,
                None,
                project_id,
                session_id,
                tenant_scope,
                Some(retrieval_limit),
                access_filter,
            )
            .await?;

        let mut score_min: Option<f64> = None;
        let mut score_max: Option<f64> = None;
        for result in &search_results {
            score_min = Some(match score_min {
                Some(current) => current.min(result.similarity),
                None => result.similarity,
            });
            score_max = Some(match score_max {
                Some(current) => current.max(result.similarity),
                None => result.similarity,
            });
        }

        let mut current_session = current_session;
        let mut relevant_history = Vec::new();
        let mut project_facts = Vec::new();

        for result in search_results {
            match result.chunk.tier {
                MemoryTier::Project => {
                    project_facts.push(result.chunk);
                }
                MemoryTier::Global => {
                    project_facts.push(result.chunk);
                }
                MemoryTier::Session => {
                    // Only add to relevant_history if not in current_session
                    if !current_session.iter().any(|c| c.id == result.chunk.id) {
                        relevant_history.push(result.chunk);
                    }
                }
            }
        }

        // Calculate total tokens and trim if necessary
        let mut total_tokens: i64 = current_session.iter().map(|c| c.token_count).sum();
        total_tokens += relevant_history.iter().map(|c| c.token_count).sum::<i64>();
        total_tokens += project_facts.iter().map(|c| c.token_count).sum::<i64>();

        // Trim to fit budget if necessary
        if total_tokens > budget {
            let excess = total_tokens - budget;
            self.trim_context(
                &mut current_session,
                &mut relevant_history,
                &mut project_facts,
                excess,
            )?;
            total_tokens = current_session.iter().map(|c| c.token_count).sum::<i64>()
                + relevant_history.iter().map(|c| c.token_count).sum::<i64>()
                + project_facts.iter().map(|c| c.token_count).sum::<i64>();
        }

        let context = MemoryContext {
            current_session,
            relevant_history,
            project_facts,
            total_tokens,
        };
        let chunks_total = context.current_session.len()
            + context.relevant_history.len()
            + context.project_facts.len();
        let meta = MemoryRetrievalMeta {
            used: chunks_total > 0,
            chunks_total,
            session_chunks: context.current_session.len(),
            history_chunks: context.relevant_history.len(),
            project_fact_chunks: context.project_facts.len(),
            score_min,
            score_max,
        };

        Ok((context, meta))
    }

    /// Trim context to fit within token budget
    fn trim_context(
        &self,
        current_session: &mut Vec<MemoryChunk>,
        relevant_history: &mut Vec<MemoryChunk>,
        project_facts: &mut Vec<MemoryChunk>,
        excess_tokens: i64,
    ) -> MemoryResult<()> {
        let mut tokens_to_remove = excess_tokens;

        // First, trim relevant_history (less important than project_facts)
        while tokens_to_remove > 0 && !relevant_history.is_empty() {
            if let Some(chunk) = relevant_history.pop() {
                tokens_to_remove -= chunk.token_count;
            }
        }

        // If still over budget, trim project_facts
        while tokens_to_remove > 0 && !project_facts.is_empty() {
            if let Some(chunk) = project_facts.pop() {
                tokens_to_remove -= chunk.token_count;
            }
        }

        while tokens_to_remove > 0 && !current_session.is_empty() {
            if let Some(chunk) = current_session.pop() {
                tokens_to_remove -= chunk.token_count;
            }
        }

        Ok(())
    }

    /// Clear session memory
    pub async fn clear_session(&self, session_id: &str) -> MemoryResult<u64> {
        self.clear_session_for_tenant(session_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn clear_session_for_tenant(
        &self,
        session_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let count = match self
            .store
            .mutate(MemoryStoreMutationRequest::ClearSession {
                scope: Self::read_scope(tenant_scope),
                session_id: session_id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Affected(count) => count,
            _ => return Err(Self::unexpected_store_result("clear session")),
        };

        self.write_cleanup_log(
            "manual",
            MemoryTier::Session,
            None,
            Some(session_id),
            count as i64,
            tenant_scope,
        )
        .await?;

        Ok(count)
    }

    /// Clear project memory
    pub async fn clear_project(&self, project_id: &str) -> MemoryResult<u64> {
        self.clear_project_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn clear_project_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let count = match self
            .store
            .mutate(MemoryStoreMutationRequest::ClearProject {
                scope: Self::read_scope(tenant_scope),
                project_id: project_id.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Affected(count) => count,
            _ => return Err(Self::unexpected_store_result("clear project")),
        };

        self.write_cleanup_log(
            "manual",
            MemoryTier::Project,
            Some(project_id),
            None,
            count as i64,
            tenant_scope,
        )
        .await?;

        Ok(count)
    }

    /// Get memory statistics
    pub async fn get_stats(&self) -> MemoryResult<MemoryStats> {
        self.get_stats_for_tenant(&MemoryTenantScope::local()).await
    }

    pub async fn get_stats_for_tenant(
        &self,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryStats> {
        match self
            .store
            .read(MemoryStoreReadRequest::Stats {
                scope: Self::read_scope(tenant_scope),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::Stats(stats) => Ok(stats),
            _ => Err(Self::unexpected_store_result("read memory stats")),
        }
    }

    /// Get memory configuration for a project
    pub async fn get_config(&self, project_id: &str) -> MemoryResult<MemoryConfig> {
        self.get_config_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn get_config_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryConfig> {
        self.read_project_config(project_id, tenant_scope).await
    }

    /// Update memory configuration for a project
    pub async fn set_config(&self, project_id: &str, config: &MemoryConfig) -> MemoryResult<()> {
        self.set_config_for_tenant(project_id, config, &MemoryTenantScope::local())
            .await
    }

    pub async fn set_config_for_tenant(
        &self,
        project_id: &str,
        config: &MemoryConfig,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        match self
            .store
            .write(MemoryStoreWriteRequest::ProjectConfig {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                project_id: project_id.to_string(),
                config: config.clone(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::Stored => Ok(()),
            _ => Err(Self::unexpected_store_result("write project config")),
        }
    }

    pub async fn resolve_uri(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<MemoryNode>> {
        match self
            .store
            .read(MemoryStoreReadRequest::ContextNode {
                scope: Self::read_scope(tenant_scope),
                uri: uri.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::ContextNode(node) => Ok(node),
            _ => Err(Self::unexpected_store_result("resolve context URI")),
        }
    }

    pub async fn list_directory(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<DirectoryListing> {
        let nodes = match self
            .store
            .query(MemoryStoreQueryRequest::ContextNodes {
                scope: Self::read_scope(tenant_scope),
                parent_uri: uri.to_string(),
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreQueryResult::ContextNodes(nodes) => nodes,
            _ => return Err(Self::unexpected_store_result("list context directory")),
        };
        let directories: Vec<MemoryNode> = nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Directory)
            .cloned()
            .collect();
        let files: Vec<MemoryNode> = nodes
            .iter()
            .filter(|n| n.node_type == NodeType::File)
            .cloned()
            .collect();

        Ok(DirectoryListing {
            uri: uri.to_string(),
            nodes,
            total_children: directories.len() + files.len(),
            directories,
            files,
        })
    }

    pub async fn tree(
        &self,
        uri: &str,
        max_depth: usize,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<TreeNode>> {
        match self
            .store
            .query(MemoryStoreQueryRequest::ContextTree {
                scope: Self::read_scope(tenant_scope),
                parent_uri: uri.to_string(),
                max_depth,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreQueryResult::ContextTree(tree) => Ok(tree),
            _ => Err(Self::unexpected_store_result("read context tree")),
        }
    }

    pub async fn create_context_node(
        &self,
        uri: &str,
        node_type: NodeType,
        metadata: Option<serde_json::Value>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        let parsed_uri =
            ContextUri::parse(uri).map_err(|e| MemoryError::InvalidConfig(e.message))?;
        let parent_uri = parsed_uri.parent().map(|p| p.to_string());
        match self
            .store
            .write(MemoryStoreWriteRequest::ContextNode {
                scope: MemoryWriteScope::tenant(tenant_scope.clone()),
                uri: uri.to_string(),
                parent_uri,
                node_type,
                metadata,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreWriteResult::ContextNodeCreated(id) => Ok(id),
            _ => Err(Self::unexpected_store_result("create context node")),
        }
    }

    pub async fn get_context_layer(
        &self,
        node_id: &str,
        layer_type: LayerType,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<MemoryLayer>> {
        match self
            .store
            .read(MemoryStoreReadRequest::ContextLayer {
                scope: Self::read_scope(tenant_scope),
                node_id: node_id.to_string(),
                layer_type,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreReadResult::ContextLayer(layer) => Ok(layer),
            _ => Err(Self::unexpected_store_result("read context layer")),
        }
    }

    pub async fn store_content_with_layers(
        &self,
        uri: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        let parsed_uri =
            ContextUri::parse(uri).map_err(|e| MemoryError::InvalidConfig(e.message))?;
        let node_type = if parsed_uri
            .last_segment()
            .map(|s| s.ends_with(".md") || s.ends_with(".txt") || s.contains("."))
            .unwrap_or(false)
        {
            NodeType::File
        } else {
            NodeType::Directory
        };

        let node_id = self
            .create_context_node(uri, node_type, metadata, tenant_scope)
            .await?;

        let token_count = self.tokenizer.count_tokens(content) as i64;
        self.write_context_layer(
            &node_id,
            LayerType::L2,
            content,
            token_count,
            None,
            tenant_scope,
        )
        .await?;

        Ok(node_id)
    }

    pub async fn generate_layers_for_node(
        &self,
        node_id: &str,
        providers: &ProviderRegistry,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        self.generate_layers_for_node_with_egress(node_id, providers, tenant_scope, None)
            .await
    }

    pub async fn generate_layers_for_node_with_egress(
        &self,
        node_id: &str,
        providers: &ProviderRegistry,
        tenant_scope: &MemoryTenantScope,
        provider_egress: Option<&MemoryProviderEgressContext>,
    ) -> MemoryResult<()> {
        let l2_layer = self
            .get_context_layer(node_id, LayerType::L2, tenant_scope)
            .await?;
        let l2_content = match l2_layer {
            Some(layer) => layer.content,
            None => return Ok(()),
        };

        let mut generator = ContextLayerGenerator::new(Arc::new(providers.clone()));
        if let Some(provider_egress) = provider_egress {
            generator = generator.with_provider_egress(provider_egress.clone());
        }

        let (l0_content, l1_content) = generator.generate_layers(&l2_content).await?;

        let l0_tokens = self.tokenizer.count_tokens(&l0_content) as i64;
        let l1_tokens = self.tokenizer.count_tokens(&l1_content) as i64;

        if self
            .get_context_layer(node_id, LayerType::L0, tenant_scope)
            .await?
            .is_none()
        {
            self.write_context_layer(
                node_id,
                LayerType::L0,
                &l0_content,
                l0_tokens,
                None,
                tenant_scope,
            )
            .await?;
        }

        if self
            .get_context_layer(node_id, LayerType::L1, tenant_scope)
            .await?
            .is_none()
        {
            self.write_context_layer(
                node_id,
                LayerType::L1,
                &l1_content,
                l1_tokens,
                None,
                tenant_scope,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn get_layer_content(
        &self,
        node_id: &str,
        layer_type: LayerType,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<String>> {
        let layer = self
            .get_context_layer(node_id, layer_type, tenant_scope)
            .await?;
        Ok(layer.map(|l| l.content))
    }

    pub async fn store_content_with_layers_auto(
        &self,
        uri: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
        providers: Option<&ProviderRegistry>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<String> {
        let node_id = self
            .store_content_with_layers(uri, content, metadata, tenant_scope)
            .await?;

        if let Some(p) = providers {
            if let Err(e) = self
                .generate_layers_for_node(&node_id, p, tenant_scope)
                .await
            {
                tracing::warn!("Failed to generate layers for node {}: {}", node_id, e);
            }
        }

        Ok(node_id)
    }

    /// Run cleanup based on retention policies
    pub async fn run_cleanup(&self, project_id: Option<&str>) -> MemoryResult<u64> {
        self.run_cleanup_for_tenant(project_id, &MemoryTenantScope::local())
            .await
    }

    pub async fn run_cleanup_for_tenant(
        &self,
        project_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<u64> {
        let mut total_cleaned = 0u64;

        let retention_days = if let Some(pid) = project_id {
            let config = self.read_project_config(pid, tenant_scope).await?;
            if !config.auto_cleanup {
                return Ok(0);
            }
            config.session_retention_days
        } else {
            30
        };
        let retention_days = u32::try_from(retention_days).map_err(|_| {
            MemoryError::InvalidConfig("session_retention_days must fit in u32".to_string())
        })?;

        let cleaned = match self
            .store
            .mutate(MemoryStoreMutationRequest::RunHygiene {
                scope: Self::read_scope(tenant_scope),
                retention_days,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Affected(cleaned) => cleaned,
            _ => return Err(Self::unexpected_store_result("run memory hygiene")),
        };
        total_cleaned += cleaned;

        if cleaned > 0 {
            self.write_cleanup_log(
                "auto",
                MemoryTier::Session,
                project_id,
                None,
                cleaned as i64,
                tenant_scope,
            )
            .await?;
        }
        if total_cleaned > 100 {
            match self
                .store
                .mutate(MemoryStoreMutationRequest::Vacuum)
                .await
                .map_err(MemoryError::from)?
            {
                MemoryStoreMutationResult::Completed => {}
                _ => return Err(Self::unexpected_store_result("vacuum memory store")),
            }
        }

        Ok(total_cleaned)
    }

    /// Check if cleanup is needed and run it
    async fn maybe_cleanup(
        &self,
        project_id: &Option<String>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        if let Some(pid) = project_id {
            let config = self.read_project_config(pid, tenant_scope).await?;

            let evicted = match self
                .store
                .mutate(MemoryStoreMutationRequest::EnforceProjectChunkCap {
                    scope: Self::read_scope(tenant_scope),
                    project_id: pid.clone(),
                    max_chunks: config.max_chunks,
                })
                .await
                .map_err(MemoryError::from)?
            {
                MemoryStoreMutationResult::Affected(evicted) => evicted,
                _ => {
                    return Err(Self::unexpected_store_result(
                        "enforce project memory chunk cap",
                    ))
                }
            };
            if evicted > 0 {
                self.write_cleanup_log(
                    "auto",
                    MemoryTier::Project,
                    Some(pid),
                    None,
                    evicted as i64,
                    tenant_scope,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Get cleanup log entries (newest first)
    pub async fn get_cleanup_log(&self, limit: i64) -> MemoryResult<Vec<CleanupLogEntry>> {
        match self
            .store
            .query(MemoryStoreQueryRequest::CleanupLog {
                scope: Self::read_scope(&MemoryTenantScope::local()),
                limit,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreQueryResult::CleanupLog(entries) => Ok(entries),
            _ => Err(Self::unexpected_store_result("read cleanup log")),
        }
    }

    /// Count tokens in text
    pub fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer.count_tokens(text)
    }

    /// Report embedding backend health for UI/telemetry.
    pub async fn embedding_health(&self) -> EmbeddingHealth {
        let service = self.embedding_service.lock().await;
        if service.is_available() {
            EmbeddingHealth {
                status: "ok".to_string(),
                reason: None,
            }
        } else {
            EmbeddingHealth {
                status: "degraded_disabled".to_string(),
                reason: service.disabled_reason().map(ToString::to_string),
            }
        }
    }

    /// Consolidate visible session memory into a summary with the same trusted
    /// ownership scope. Summary creation and source cleanup commit atomically.
    pub async fn consolidate_scoped_session(
        &self,
        request: &ScopedMemoryConsolidationRequest,
        providers: &ProviderRegistry,
        config: &MemoryConsolidationConfig,
        provider_egress: &MemoryProviderEgressContext,
    ) -> MemoryResult<Option<String>> {
        if !config.enabled {
            return Ok(None);
        }
        if request.session_id.trim().is_empty() || request.project_id.trim().is_empty() {
            return Err(MemoryError::InvalidConfig(
                "memory consolidation requires non-empty session and project ids".to_string(),
            ));
        }

        let read_scope = MemoryReadScope {
            tenant: request.tenant_scope.clone(),
            org_unit: request.org_unit.clone(),
            subject: request.subject.clone(),
            access: crate::store::MemoryReadAccess::Scoped,
        };

        let chunks = self
            .read_chunks(
                MemoryChunkSelector::session_in_project(
                    &request.session_id,
                    &request.project_id,
                ),
                read_scope.clone(),
                None,
            )
            .await?;
        if chunks.is_empty() {
            return Ok(None);
        }

        // Assemble text
        let mut text_parts = Vec::new();
        for chunk in &chunks {
            text_parts.push(chunk.content.clone());
        }
        let full_text = text_parts.join("\n\n---\n\n");

        // Build prompt
        let prompt = format!(
            "Please provide a concise but comprehensive summary of the following chat session. \
            Focus on the key decisions, technical details, code changes, and unresolved issues. \
            Do NOT include conversational filler, greetings, or sign-offs. \
            This summary will be used as long-term memory to recall the context of this work.\n\n\
            Session transcripts:\n\n{}",
            full_text
        );

        let provider_override = config.provider.as_deref().filter(|s| !s.is_empty());
        let model_override = config.model.as_deref().filter(|s| !s.is_empty());

        let operation_id = format!("{}:memory_consolidation", request.session_id);
        let summary_text = match complete_memory_prompt(
            providers,
            &prompt,
            provider_override,
            model_override,
            Some(provider_egress),
            MemoryProviderEgressKind::Consolidation,
            &operation_id,
            "memory.session_consolidation",
        )
        .await
        {
            Ok(s) => s,
            Err(error @ MemoryError::TenantScopeViolation(_)) => return Err(error),
            Err(e) => {
                tracing::warn!(
                    "Memory consolidation LLM failed for session {}: {e}",
                    request.session_id
                );
                return Ok(None);
            }
        };

        if summary_text.trim().is_empty() {
            return Ok(None);
        }

        // Generate embedding for the summary
        let embedding = {
            let service = self.embedding_service.lock().await;
            service
                .embed(&summary_text)
                .await
                .map_err(|e| crate::types::MemoryError::Embedding(e.to_string()))?
        };

        // Store the summary chunk
        let source_chunk_ids = chunks
            .iter()
            .map(|chunk| chunk.id.clone())
            .collect::<Vec<_>>();
        let mut metadata = serde_json::Map::new();
        if let Some(org_unit) = request.org_unit.as_ref() {
            metadata.insert(
                crate::types::OWNER_ORG_UNIT_METADATA_KEY.to_string(),
                serde_json::Value::String(org_unit.clone()),
            );
        }
        if let Some(subject) = request.subject.as_ref() {
            metadata.insert(
                crate::types::OWNER_SUBJECT_METADATA_KEY.to_string(),
                serde_json::Value::String(subject.clone()),
            );
        } else if request.org_unit.is_none() {
            metadata.insert(
                crate::types::TENANT_SHARED_METADATA_KEY.to_string(),
                serde_json::Value::Bool(true),
            );
        }
        metadata.insert(
            "consolidation_provenance".to_string(),
            serde_json::json!({
                "session_id": request.session_id,
                "source_chunk_ids": source_chunk_ids,
                "source_count": chunks.len(),
                "tenant_context": {
                    "org_id": request.tenant_scope.org_id,
                    "workspace_id": request.tenant_scope.workspace_id,
                    "deployment_id": request.tenant_scope.deployment_id,
                }
            }),
        );

        let chunk = MemoryChunk {
            id: uuid::Uuid::new_v4().to_string(),
            content: summary_text.clone(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some(request.project_id.clone()),
            created_at: Utc::now(),
            source: "consolidation".to_string(),
            token_count: self.count_tokens(&summary_text) as i64,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: request.tenant_scope.clone(),
            subject: request.subject.clone(),
            metadata: Some(serde_json::Value::Object(metadata)),
        };

        match self
            .store
            .mutate(MemoryStoreMutationRequest::ReplaceSessionWithSummary {
                scope: read_scope,
                session_id: request.session_id.clone(),
                project_id: request.project_id.clone(),
                source_chunk_ids,
                summary_scope: Self::chunk_write_scope(&chunk),
                summary: Box::new(chunk),
                embedding,
            })
            .await
            .map_err(MemoryError::from)?
        {
            MemoryStoreMutationResult::Affected(_) => {}
            _ => return Err(Self::unexpected_store_result("consolidate session")),
        }

        tracing::info!(
            "Session {} consolidated into a scoped summary chunk",
            request.session_id
        );

        Ok(Some(summary_text))
    }
}
