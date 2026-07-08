// Memory Manager Module
// High-level memory operations (store, retrieve, cleanup)

use crate::chunking::{chunk_text_semantic, ChunkingConfig, Tokenizer};
use crate::context_layers::ContextLayerGenerator;
use crate::context_uri::ContextUri;
use crate::db::MemoryDatabase;
use crate::embeddings::EmbeddingService;
use crate::envelope::validate_memory_envelope_for_write;
use crate::types::{
    CleanupLogEntry, DirectoryListing, EmbeddingHealth, KnowledgeCoverageRecord,
    KnowledgeItemRecord, KnowledgePromotionRequest, KnowledgePromotionResult, KnowledgeSpaceRecord,
    LayerType, MemoryChunk, MemoryConfig, MemoryContext, MemoryError, MemoryLayer, MemoryNode,
    MemoryResult, MemoryRetrievalMeta, MemorySearchResult, MemoryStats, MemoryTenantScope,
    MemoryTier, NodeType, StoreMessageRequest, TreeNode,
};
use chrono::Utc;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tandem_orchestrator::{
    build_knowledge_coverage_key, normalize_knowledge_segment, KnowledgeBinding, KnowledgePackItem,
    KnowledgePreflightRequest, KnowledgePreflightResult, KnowledgeReuseDecision,
    KnowledgeReuseMode, KnowledgeScope, KnowledgeTrustLevel,
};
use tandem_providers::{MemoryConsolidationConfig, ProviderRegistry};
use tokio::sync::Mutex;

/// High-level memory manager that coordinates database, embeddings, and chunking
pub struct MemoryManager {
    db: Arc<MemoryDatabase>,
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

    fn is_malformed_database_error(err: &crate::types::MemoryError) -> bool {
        err.to_string()
            .to_lowercase()
            .contains("database disk image is malformed")
    }

    pub fn db(&self) -> &Arc<MemoryDatabase> {
        &self.db
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
        let embedding_service = Arc::new(Mutex::new(embedding_service));
        let tokenizer = Tokenizer::new()?;

        Ok(Self {
            db,
            embedding_service,
            tokenizer,
        })
    }

    /// Store a message in memory
    ///
    /// This will:
    /// 1. Chunk the message content
    /// 2. Generate embeddings for each chunk
    /// 3. Store chunks and embeddings in the database
    pub async fn store_message(&self, request: StoreMessageRequest) -> MemoryResult<Vec<String>> {
        validate_memory_envelope_for_write(&request.tenant_scope, request.metadata.as_ref())?;

        if self
            .db
            .ensure_vector_tables_healthy()
            .await
            .unwrap_or(false)
        {
            tracing::warn!("Memory vector tables were repaired before storing message chunks");
        }

        let config = if let Some(ref pid) = request.project_id {
            self.db
                .get_or_create_config_for_tenant(pid, &request.tenant_scope)
                .await?
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

            // Store in database (retry once after vector-table self-heal).
            if let Err(err) = self.db.store_chunk(&chunk, &embedding).await {
                tracing::warn!("Failed to store memory chunk {}: {}", chunk.id, err);
                let repaired = {
                    let repaired_after_error =
                        self.db.try_repair_after_error(&err).await.unwrap_or(false);
                    repaired_after_error
                        || self
                            .db
                            .ensure_vector_tables_healthy()
                            .await
                            .unwrap_or(false)
                };
                if repaired {
                    tracing::warn!(
                        "Retrying memory chunk insert after vector table repair: {}",
                        chunk.id
                    );
                    if let Err(retry_err) = self.db.store_chunk(&chunk, &embedding).await {
                        if Self::is_malformed_database_error(&retry_err) {
                            tracing::warn!(
                                "Memory DB still malformed after vector repair. Resetting memory tables and retrying chunk insert: {}",
                                chunk.id
                            );
                            self.db.reset_all_memory_tables().await?;
                            self.db.store_chunk(&chunk, &embedding).await?;
                        } else {
                            return Err(retry_err);
                        }
                    }
                } else {
                    return Err(err);
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
        let visible_subject = access_filter.and_then(|filter| filter.caller_subject.as_deref());
        let visible_owner_org_unit = access_filter
            .and_then(|filter| filter.caller_org_units.as_ref())
            .and_then(|units| {
                if units.len() == 1 {
                    units.iter().next().map(String::as_str)
                } else {
                    None
                }
            });
        for search_tier in tiers_to_search {
            let tier_results = match self
                .db
                .search_similar_for_tenant(
                    &query_embedding,
                    search_tier,
                    project_id,
                    session_id,
                    tenant_scope,
                    candidate_limit,
                    visible_subject,
                    visible_owner_org_unit,
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
                    let repaired = {
                        let repaired_after_error =
                            self.db.try_repair_after_error(&err).await.unwrap_or(false);
                        repaired_after_error
                            || self
                                .db
                                .ensure_vector_tables_healthy()
                                .await
                                .unwrap_or(false)
                    };
                    if repaired {
                        match self
                            .db
                            .search_similar_for_tenant(
                                &query_embedding,
                                search_tier,
                                project_id,
                                session_id,
                                tenant_scope,
                                candidate_limit,
                                visible_subject,
                                visible_owner_org_unit,
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

    pub async fn upsert_knowledge_space(&self, space: &KnowledgeSpaceRecord) -> MemoryResult<()> {
        validate_memory_envelope_for_write(&MemoryTenantScope::local(), space.metadata.as_ref())?;
        self.db.upsert_knowledge_space(space).await
    }

    pub async fn upsert_knowledge_space_for_tenant(
        &self,
        space: &KnowledgeSpaceRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, space.metadata.as_ref())?;
        self.db
            .upsert_knowledge_space_for_tenant(space, tenant_scope)
            .await
    }

    pub async fn get_knowledge_space(
        &self,
        id: &str,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        self.db.get_knowledge_space(id).await
    }

    pub async fn get_knowledge_space_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        self.db
            .get_knowledge_space_for_tenant(id, tenant_scope)
            .await
    }

    pub async fn list_knowledge_spaces(
        &self,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        self.db.list_knowledge_spaces(project_id).await
    }

    pub async fn list_knowledge_spaces_for_tenant(
        &self,
        project_id: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        self.db
            .list_knowledge_spaces_for_tenant(project_id, tenant_scope)
            .await
    }

    pub async fn upsert_knowledge_item(&self, item: &KnowledgeItemRecord) -> MemoryResult<()> {
        validate_memory_envelope_for_write(&MemoryTenantScope::local(), item.metadata.as_ref())?;
        self.db.upsert_knowledge_item(item).await
    }

    pub async fn upsert_knowledge_item_for_tenant(
        &self,
        item: &KnowledgeItemRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, item.metadata.as_ref())?;
        self.db
            .upsert_knowledge_item_for_tenant(item, tenant_scope)
            .await
    }

    pub async fn get_knowledge_item(&self, id: &str) -> MemoryResult<Option<KnowledgeItemRecord>> {
        self.db.get_knowledge_item(id).await
    }

    pub async fn get_knowledge_item_for_tenant(
        &self,
        id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeItemRecord>> {
        self.db
            .get_knowledge_item_for_tenant(id, tenant_scope)
            .await
    }

    pub async fn list_knowledge_items(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        self.db.list_knowledge_items(space_id, coverage_key).await
    }

    pub async fn list_knowledge_items_for_tenant(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        self.db
            .list_knowledge_items_for_tenant(space_id, coverage_key, tenant_scope)
            .await
    }

    pub async fn upsert_knowledge_coverage(
        &self,
        coverage: &KnowledgeCoverageRecord,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(
            &MemoryTenantScope::local(),
            coverage.metadata.as_ref(),
        )?;
        self.db.upsert_knowledge_coverage(coverage).await
    }

    pub async fn upsert_knowledge_coverage_for_tenant(
        &self,
        coverage: &KnowledgeCoverageRecord,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        validate_memory_envelope_for_write(tenant_scope, coverage.metadata.as_ref())?;
        self.db
            .upsert_knowledge_coverage_for_tenant(coverage, tenant_scope)
            .await
    }

    pub async fn get_knowledge_coverage(
        &self,
        coverage_key: &str,
        space_id: &str,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        self.db.get_knowledge_coverage(coverage_key, space_id).await
    }

    pub async fn get_knowledge_coverage_for_tenant(
        &self,
        coverage_key: &str,
        space_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        self.db
            .get_knowledge_coverage_for_tenant(coverage_key, space_id, tenant_scope)
            .await
    }

    pub async fn promote_knowledge_item(
        &self,
        request: &KnowledgePromotionRequest,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        self.db.promote_knowledge_item(request).await
    }

    pub async fn promote_knowledge_item_for_tenant(
        &self,
        request: &KnowledgePromotionRequest,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        self.db
            .promote_knowledge_item_for_tenant(request, tenant_scope)
            .await
    }

    fn space_matches_ref(
        space: &KnowledgeSpaceRecord,
        space_ref: &tandem_orchestrator::KnowledgeSpaceRef,
        project_id: &str,
    ) -> bool {
        if space.scope != space_ref.scope {
            return false;
        }
        match space_ref.scope {
            KnowledgeScope::Project | KnowledgeScope::Run => {
                let target_project = space_ref.project_id.as_deref().unwrap_or(project_id);
                if space.project_id.as_deref() != Some(target_project) {
                    return false;
                }
            }
            KnowledgeScope::Global => {}
        }
        if let Some(namespace) = space_ref.namespace.as_deref() {
            if space.namespace.as_deref() != Some(namespace) {
                return false;
            }
        }
        true
    }

    fn select_preflight_namespace(
        binding: &KnowledgeBinding,
        spaces: &[KnowledgeSpaceRecord],
    ) -> Option<String> {
        if let Some(namespace) = binding.namespace.clone() {
            return Some(namespace);
        }
        if binding.read_spaces.len() == 1 {
            if let Some(namespace) = binding.read_spaces[0].namespace.clone() {
                return Some(namespace);
            }
        }
        if spaces.len() == 1 {
            return spaces[0].namespace.clone();
        }
        let mut unique = HashSet::new();
        for space in spaces {
            if let Some(namespace) = space.namespace.as_ref() {
                unique.insert(namespace);
            }
        }
        if unique.len() == 1 {
            unique.into_iter().next().map(|value| value.to_string())
        } else {
            None
        }
    }

    fn binding_uses_explicit_spaces(binding: &KnowledgeBinding) -> bool {
        !binding.read_spaces.is_empty() || !binding.promote_spaces.is_empty()
    }

    fn namespace_matches(space_namespace: Option<&str>, binding_namespace: Option<&str>) -> bool {
        match (space_namespace, binding_namespace) {
            (None, None) => true,
            (Some(space), Some(binding)) => {
                normalize_knowledge_segment(space) == normalize_knowledge_segment(binding)
            }
            _ => false,
        }
    }

    fn is_fresh_enough(
        freshness_expires_at_ms: Option<u64>,
        freshness_policy_ms: Option<u64>,
        coverage_last_promoted_at_ms: Option<u64>,
        item_created_at_ms: u64,
        now_ms: u64,
    ) -> bool {
        if let Some(expires_at_ms) = freshness_expires_at_ms {
            return expires_at_ms > now_ms;
        }
        let Some(policy_ms) = freshness_policy_ms else {
            return true;
        };
        let basis_ms = coverage_last_promoted_at_ms.unwrap_or(item_created_at_ms);
        now_ms.saturating_sub(basis_ms) <= policy_ms
    }

    async fn resolve_preflight_spaces(
        &self,
        request: &KnowledgePreflightRequest,
        _coverage_key: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        let binding = &request.binding;
        let mut spaces = Vec::new();
        let mut seen_space_ids = HashSet::new();

        let push_space = |space: KnowledgeSpaceRecord,
                          spaces: &mut Vec<KnowledgeSpaceRecord>,
                          seen_space_ids: &mut HashSet<String>| {
            if seen_space_ids.insert(space.id.clone()) {
                spaces.push(space);
            }
        };

        if Self::binding_uses_explicit_spaces(binding) {
            for space_ref in binding
                .read_spaces
                .iter()
                .chain(binding.promote_spaces.iter())
            {
                if let Some(space_id) = space_ref.space_id.as_deref() {
                    if let Some(space) = self
                        .get_knowledge_space_for_tenant(space_id, tenant_scope)
                        .await?
                    {
                        push_space(space, &mut spaces, &mut seen_space_ids);
                    }
                    continue;
                }

                match space_ref.scope {
                    KnowledgeScope::Run => {}
                    KnowledgeScope::Project => {
                        let candidate_project_id = space_ref
                            .project_id
                            .as_deref()
                            .unwrap_or(&request.project_id);
                        let project_spaces = self
                            .list_knowledge_spaces_for_tenant(
                                Some(candidate_project_id),
                                tenant_scope,
                            )
                            .await?;
                        for space in project_spaces.into_iter().filter(|space| {
                            Self::space_matches_ref(space, space_ref, &request.project_id)
                        }) {
                            push_space(space, &mut spaces, &mut seen_space_ids);
                        }
                    }
                    KnowledgeScope::Global => {
                        let global_spaces = self
                            .list_knowledge_spaces_for_tenant(None, tenant_scope)
                            .await?;
                        for space in global_spaces.into_iter().filter(|space| {
                            Self::space_matches_ref(space, space_ref, &request.project_id)
                        }) {
                            push_space(space, &mut spaces, &mut seen_space_ids);
                        }
                    }
                }
            }
            return Ok(spaces);
        }

        if request.project_id.trim().is_empty() {
            return Ok(spaces);
        }

        let project_spaces = self
            .list_knowledge_spaces_for_tenant(Some(&request.project_id), tenant_scope)
            .await?;
        let requested_namespace = if binding.namespace.is_some() {
            binding.namespace.clone()
        } else {
            Self::select_preflight_namespace(binding, &project_spaces)
        };
        let Some(requested_namespace) = requested_namespace else {
            return Ok(spaces);
        };

        for space in project_spaces.into_iter().filter(|space| {
            space.scope == KnowledgeScope::Project
                && Self::namespace_matches(
                    space.namespace.as_deref(),
                    Some(requested_namespace.as_str()),
                )
        }) {
            push_space(space, &mut spaces, &mut seen_space_ids);
        }
        Ok(spaces)
    }

    pub async fn preflight_knowledge(
        &self,
        request: &KnowledgePreflightRequest,
    ) -> MemoryResult<KnowledgePreflightResult> {
        self.preflight_knowledge_for_tenant(request, &MemoryTenantScope::local())
            .await
    }

    pub async fn preflight_knowledge_for_tenant(
        &self,
        request: &KnowledgePreflightRequest,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<KnowledgePreflightResult> {
        let binding = &request.binding;
        let project_spaces = if request.project_id.trim().is_empty() {
            Vec::new()
        } else {
            self.list_knowledge_spaces_for_tenant(Some(&request.project_id), tenant_scope)
                .await?
        };
        let namespace = binding
            .namespace
            .clone()
            .or_else(|| Self::select_preflight_namespace(binding, &project_spaces));
        let coverage_key = build_knowledge_coverage_key(
            &request.project_id,
            namespace.as_deref(),
            &request.task_family,
            &request.subject,
        );

        if !binding.enabled || binding.reuse_mode == KnowledgeReuseMode::Disabled {
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::Disabled,
                reuse_reason: None,
                skip_reason: Some("knowledge reuse is disabled for this binding".to_string()),
                freshness_reason: None,
                items: Vec::new(),
            });
        }

        let spaces = self
            .resolve_preflight_spaces(request, &coverage_key, tenant_scope)
            .await?;
        if spaces.is_empty() {
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::NoPriorKnowledge,
                reuse_reason: None,
                skip_reason: Some("no reusable knowledge spaces were found".to_string()),
                freshness_reason: None,
                items: Vec::new(),
            });
        }

        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let mut fresh_items = Vec::new();
        let mut stale_items = Vec::new();
        let mut freshest_reason = None;

        for space in &spaces {
            let items = self
                .list_knowledge_items_for_tenant(&space.id, Some(&coverage_key), tenant_scope)
                .await?;
            let coverage = self
                .get_knowledge_coverage_for_tenant(&coverage_key, &space.id, tenant_scope)
                .await?;
            for item in items {
                if !item.status.is_active() {
                    continue;
                }
                let Some(trust_level) = item.status.as_trust_level() else {
                    continue;
                };
                if !trust_level.meets_floor(binding.trust_floor) {
                    continue;
                }
                let freshness_expires_at_ms = item.freshness_expires_at_ms.or_else(|| {
                    coverage
                        .as_ref()
                        .and_then(|coverage| coverage.freshness_expires_at_ms)
                });
                let pack_item = KnowledgePackItem {
                    item_id: item.id.clone(),
                    space_id: space.id.clone(),
                    coverage_key: item.coverage_key.clone(),
                    title: item.title.clone(),
                    summary: item.summary.clone(),
                    trust_level,
                    status: item.status.to_string(),
                    artifact_refs: item.artifact_refs.clone(),
                    source_memory_ids: item.source_memory_ids.clone(),
                    freshness_expires_at_ms,
                };
                if Self::is_fresh_enough(
                    freshness_expires_at_ms,
                    binding.freshness_ms,
                    coverage
                        .as_ref()
                        .and_then(|coverage| coverage.last_promoted_at_ms),
                    item.created_at_ms,
                    now_ms,
                ) {
                    fresh_items.push(pack_item);
                } else {
                    freshest_reason = Some(match freshness_expires_at_ms {
                        Some(expires_at_ms) => format!(
                            "coverage `{}` in space `{}` expired at {}",
                            coverage_key, space.id, expires_at_ms
                        ),
                        None => format!(
                            "coverage `{}` in space `{}` lacks freshness metadata",
                            coverage_key, space.id
                        ),
                    });
                    stale_items.push(pack_item);
                }
            }
        }

        fresh_items.sort_by(|left, right| {
            right
                .trust_level
                .rank()
                .cmp(&left.trust_level.rank())
                .then_with(|| {
                    right
                        .freshness_expires_at_ms
                        .unwrap_or(0)
                        .cmp(&left.freshness_expires_at_ms.unwrap_or(0))
                })
                .then_with(|| left.title.cmp(&right.title))
        });
        stale_items.sort_by(|left, right| {
            right
                .trust_level
                .rank()
                .cmp(&left.trust_level.rank())
                .then_with(|| left.title.cmp(&right.title))
        });

        if let Some(freshest_trust_level) = fresh_items.first().map(|item| item.trust_level) {
            let selected = fresh_items
                .into_iter()
                .take(MAX_KNOWLEDGE_PACK_ITEMS)
                .collect::<Vec<_>>();
            let decision = match freshest_trust_level {
                KnowledgeTrustLevel::ApprovedDefault => {
                    KnowledgeReuseDecision::ReuseApprovedDefault
                }
                _ => KnowledgeReuseDecision::ReusePromoted,
            };
            let selected_count = selected.len();
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision,
                reuse_reason: Some(format!(
                    "reusing {} promoted knowledge item(s) from {} space(s)",
                    selected_count,
                    spaces.len()
                )),
                skip_reason: None,
                freshness_reason: None,
                items: selected,
            });
        }

        if !stale_items.is_empty() {
            let selected = stale_items
                .into_iter()
                .take(MAX_KNOWLEDGE_PACK_ITEMS)
                .collect::<Vec<_>>();
            return Ok(KnowledgePreflightResult {
                project_id: request.project_id.clone(),
                namespace,
                task_family: request.task_family.clone(),
                subject: request.subject.clone(),
                coverage_key,
                decision: KnowledgeReuseDecision::RefreshRequired,
                reuse_reason: None,
                skip_reason: Some(
                    "prior knowledge exists but is not fresh enough to reuse".to_string(),
                ),
                freshness_reason: freshest_reason.or_else(|| {
                    Some("matching knowledge exists but freshness policy rejected it".to_string())
                }),
                items: selected,
            });
        }

        Ok(KnowledgePreflightResult {
            project_id: request.project_id.clone(),
            namespace,
            task_family: request.task_family.clone(),
            subject: request.subject.clone(),
            coverage_key,
            decision: KnowledgeReuseDecision::NoPriorKnowledge,
            reuse_reason: None,
            skip_reason: Some("no active promoted knowledge matched this coverage key".to_string()),
            freshness_reason: None,
            items: Vec::new(),
        })
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
            self.db
                .get_or_create_config_for_tenant(pid, tenant_scope)
                .await?
        } else {
            MemoryConfig::default()
        };
        let budget = token_budget.unwrap_or(config.token_budget);
        let retrieval_limit = config.retrieval_k.max(1);

        // Get recent session chunks
        let current_session = if let Some(sid) = session_id {
            self.db
                .get_session_chunks_for_tenant(sid, tenant_scope)
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
        let count = self
            .db
            .clear_session_memory_for_tenant(session_id, tenant_scope)
            .await?;

        // Log cleanup
        self.db
            .log_cleanup_for_tenant(
                "manual",
                MemoryTier::Session,
                None,
                Some(session_id),
                count as i64,
                0,
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
        let count = self
            .db
            .clear_project_memory_for_tenant(project_id, tenant_scope)
            .await?;

        // Log cleanup
        self.db
            .log_cleanup_for_tenant(
                "manual",
                MemoryTier::Project,
                Some(project_id),
                None,
                count as i64,
                0,
                tenant_scope,
            )
            .await?;

        Ok(count)
    }

    /// Get memory statistics
    pub async fn get_stats(&self) -> MemoryResult<MemoryStats> {
        self.db.get_stats().await
    }

    pub async fn get_stats_for_tenant(
        &self,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryStats> {
        self.db.get_stats_for_tenant(tenant_scope).await
    }

    /// Get memory configuration for a project
    pub async fn get_config(&self, project_id: &str) -> MemoryResult<MemoryConfig> {
        self.db.get_or_create_config(project_id).await
    }

    pub async fn get_config_for_tenant(
        &self,
        project_id: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<MemoryConfig> {
        self.db
            .get_or_create_config_for_tenant(project_id, tenant_scope)
            .await
    }

    /// Update memory configuration for a project
    pub async fn set_config(&self, project_id: &str, config: &MemoryConfig) -> MemoryResult<()> {
        self.db.update_config(project_id, config).await
    }

    pub async fn set_config_for_tenant(
        &self,
        project_id: &str,
        config: &MemoryConfig,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        self.db
            .update_config_for_tenant(project_id, config, tenant_scope)
            .await
    }

    pub async fn resolve_uri(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<MemoryNode>> {
        self.db.get_node_by_uri(uri, tenant_scope).await
    }

    pub async fn list_directory(
        &self,
        uri: &str,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<DirectoryListing> {
        let nodes = self.db.list_directory(uri, tenant_scope).await?;
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
        self.db.get_children_tree(uri, max_depth, tenant_scope).await
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
        self.db
            .create_node(
                uri,
                parent_uri.as_deref(),
                node_type,
                metadata.as_ref(),
                tenant_scope,
            )
            .await
    }

    pub async fn get_context_layer(
        &self,
        node_id: &str,
        layer_type: LayerType,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<Option<MemoryLayer>> {
        self.db.get_layer(node_id, layer_type, tenant_scope).await
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

        let parent_uri = parsed_uri.parent().map(|p| p.to_string());
        let node_id = self
            .db
            .create_node(
                uri,
                parent_uri.as_deref(),
                node_type,
                metadata.as_ref(),
                tenant_scope,
            )
            .await?;

        let token_count = self.tokenizer.count_tokens(content) as i64;
        self.db
            .create_layer(&node_id, LayerType::L2, content, token_count, None, tenant_scope)
            .await?;

        Ok(node_id)
    }

    pub async fn generate_layers_for_node(
        &self,
        node_id: &str,
        providers: &ProviderRegistry,
        tenant_scope: &MemoryTenantScope,
    ) -> MemoryResult<()> {
        let l2_layer = self.db.get_layer(node_id, LayerType::L2, tenant_scope).await?;
        let l2_content = match l2_layer {
            Some(layer) => layer.content,
            None => return Ok(()),
        };

        let generator = ContextLayerGenerator::new(Arc::new(providers.clone()));

        let (l0_content, l1_content) = generator.generate_layers(&l2_content).await?;

        let l0_tokens = self.tokenizer.count_tokens(&l0_content) as i64;
        let l1_tokens = self.tokenizer.count_tokens(&l1_content) as i64;

        if self
            .db
            .get_layer(node_id, LayerType::L0, tenant_scope)
            .await?
            .is_none()
        {
            self.db
                .create_layer(node_id, LayerType::L0, &l0_content, l0_tokens, None, tenant_scope)
                .await?;
        }

        if self
            .db
            .get_layer(node_id, LayerType::L1, tenant_scope)
            .await?
            .is_none()
        {
            self.db
                .create_layer(node_id, LayerType::L1, &l1_content, l1_tokens, None, tenant_scope)
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
        let layer = self.db.get_layer(node_id, layer_type, tenant_scope).await?;
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

        if let Some(pid) = project_id {
            // Get config for this project
            let config = self
                .db
                .get_or_create_config_for_tenant(pid, tenant_scope)
                .await?;

            if config.auto_cleanup {
                // Clean up old session memory
                let cleaned = self
                    .db
                    .cleanup_old_sessions_for_tenant(config.session_retention_days, tenant_scope)
                    .await?;
                total_cleaned += cleaned;

                if cleaned > 0 {
                    self.db
                        .log_cleanup_for_tenant(
                            "auto",
                            MemoryTier::Session,
                            Some(pid),
                            None,
                            cleaned as i64,
                            0,
                            tenant_scope,
                        )
                        .await?;
                }
            }
        } else {
            // Clean up all projects with auto_cleanup enabled
            // This would require listing all projects, for now just clean session memory
            // with a default retention period
            let cleaned = self
                .db
                .cleanup_old_sessions_for_tenant(30, tenant_scope)
                .await?;
            total_cleaned += cleaned;
        }

        // Vacuum if significant cleanup occurred
        if total_cleaned > 100 {
            self.db.vacuum().await?;
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
            let config = self
                .db
                .get_or_create_config_for_tenant(pid, tenant_scope)
                .await?;

            // Evict oldest project chunks (and their vector rows) over the cap
            let evicted = self
                .db
                .enforce_project_chunk_cap_for_tenant(pid, config.max_chunks, tenant_scope)
                .await?;
            if evicted > 0 {
                self.db
                    .log_cleanup_for_tenant(
                        "auto",
                        MemoryTier::Project,
                        Some(pid),
                        None,
                        evicted as i64,
                        0,
                        tenant_scope,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Get cleanup log entries (newest first)
    pub async fn get_cleanup_log(&self, limit: i64) -> MemoryResult<Vec<CleanupLogEntry>> {
        self.db.get_cleanup_log(limit).await
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

    /// Consolidate a session's memory into a summary chunk using the cheapest available provider.
    pub async fn consolidate_session(
        &self,
        session_id: &str,
        project_id: Option<&str>,
        providers: &ProviderRegistry,
        config: &MemoryConsolidationConfig,
    ) -> MemoryResult<Option<String>> {
        if !config.enabled {
            return Ok(None);
        }

        let chunks = self.db.get_session_chunks(session_id).await?;
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

        let summary_text = match providers
            .complete_cheapest(&prompt, provider_override, model_override)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Memory consolidation LLM failed for session {session_id}: {e}");
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
        let chunk_id = uuid::Uuid::new_v4().to_string();
        let chunk = MemoryChunk {
            id: chunk_id,
            content: summary_text.clone(),
            tier: MemoryTier::Project,
            session_id: None, // The summary belongs to the project, not the ephemeral session
            project_id: project_id.map(ToString::to_string),
            created_at: Utc::now(),
            source: "consolidation".to_string(),
            token_count: self.count_tokens(&summary_text) as i64,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            // Consolidated summaries merge multiple sources and stay shared
            // within their project scope rather than being user-restricted.
            subject: None,
            metadata: None,
        };

        self.db.store_chunk(&chunk, &embedding).await?;

        // Clear original chunks now that they are consolidated
        self.db.clear_session_memory(session_id).await?;

        tracing::info!(
            "Session {session_id} consolidated into summary chunk. Original chunks cleared."
        );

        Ok(Some(summary_text))
    }
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

/// Create memory manager with default database path
pub async fn create_memory_manager(app_data_dir: &Path) -> MemoryResult<MemoryManager> {
    let db_path = app_data_dir.join("tandem_memory.db");
    MemoryManager::new(&db_path).await
}
