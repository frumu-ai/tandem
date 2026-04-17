// Memory Manager Module
// High-level memory operations (store, retrieve, cleanup)

use crate::chunking::{chunk_text_semantic, ChunkingConfig, Tokenizer};
use crate::context_layers::ContextLayerGenerator;
use crate::context_uri::ContextUri;
use crate::db::MemoryDatabase;
use crate::embeddings::EmbeddingService;
use crate::types::{
    CleanupLogEntry, DirectoryListing, EmbeddingHealth, KnowledgeCoverageRecord,
    KnowledgeItemRecord, KnowledgePromotionRequest, KnowledgePromotionResult, KnowledgeSpaceRecord,
    LayerType, MemoryChunk, MemoryConfig, MemoryContext, MemoryError, MemoryLayer, MemoryNode,
    MemoryResult, MemoryRetrievalMeta, MemorySearchResult, MemoryStats, MemoryTier, NodeType,
    StoreMessageRequest, TreeNode,
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

impl MemoryManager {
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
        let db = Arc::new(MemoryDatabase::new(db_path).await?);
        let embedding_service = Arc::new(Mutex::new(EmbeddingService::new()));
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
        if self
            .db
            .ensure_vector_tables_healthy()
            .await
            .unwrap_or(false)
        {
            tracing::warn!("Memory vector tables were repaired before storing message chunks");
        }

        let config = if let Some(ref pid) = request.project_id {
            self.db.get_or_create_config(pid).await?
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
                source_hash: request.source_hash.clone(),
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
            self.maybe_cleanup(&request.project_id).await?;
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
        let effective_limit = limit.unwrap_or(5);

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

        for search_tier in tiers_to_search {
            let tier_results = match self
                .db
                .search_similar(
                    &query_embedding,
                    search_tier,
                    project_id,
                    session_id,
                    effective_limit,
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
                            .search_similar(
                                &query_embedding,
                                search_tier,
                                project_id,
                                session_id,
                                effective_limit,
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
                // Convert distance to similarity (cosine similarity)
                // sqlite-vec returns distance, where lower is more similar
                // Cosine similarity ranges from -1 to 1, but for normalized vectors it's 0 to 1
                let similarity = 1.0 - distance.clamp(0.0, 1.0);

                results.push(MemorySearchResult { chunk, similarity });
            }
        }

        // Sort by similarity (highest first) and limit results
        results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        results.truncate(effective_limit as usize);

        Ok(results)
    }

    pub async fn upsert_knowledge_space(&self, space: &KnowledgeSpaceRecord) -> MemoryResult<()> {
        self.db.upsert_knowledge_space(space).await
    }

    pub async fn get_knowledge_space(
        &self,
        id: &str,
    ) -> MemoryResult<Option<KnowledgeSpaceRecord>> {
        self.db.get_knowledge_space(id).await
    }

    pub async fn list_knowledge_spaces(
        &self,
        project_id: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeSpaceRecord>> {
        self.db.list_knowledge_spaces(project_id).await
    }

    pub async fn upsert_knowledge_item(&self, item: &KnowledgeItemRecord) -> MemoryResult<()> {
        self.db.upsert_knowledge_item(item).await
    }

    pub async fn get_knowledge_item(&self, id: &str) -> MemoryResult<Option<KnowledgeItemRecord>> {
        self.db.get_knowledge_item(id).await
    }

    pub async fn list_knowledge_items(
        &self,
        space_id: &str,
        coverage_key: Option<&str>,
    ) -> MemoryResult<Vec<KnowledgeItemRecord>> {
        self.db.list_knowledge_items(space_id, coverage_key).await
    }

    pub async fn upsert_knowledge_coverage(
        &self,
        coverage: &KnowledgeCoverageRecord,
    ) -> MemoryResult<()> {
        self.db.upsert_knowledge_coverage(coverage).await
    }

    pub async fn get_knowledge_coverage(
        &self,
        coverage_key: &str,
        space_id: &str,
    ) -> MemoryResult<Option<KnowledgeCoverageRecord>> {
        self.db.get_knowledge_coverage(coverage_key, space_id).await
    }

    pub async fn promote_knowledge_item(
        &self,
        request: &KnowledgePromotionRequest,
    ) -> MemoryResult<Option<KnowledgePromotionResult>> {
        self.db.promote_knowledge_item(request).await
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
                    if let Some(space) = self.get_knowledge_space(space_id).await? {
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
                            .list_knowledge_spaces(Some(candidate_project_id))
                            .await?;
                        for space in project_spaces.into_iter().filter(|space| {
                            Self::space_matches_ref(space, space_ref, &request.project_id)
                        }) {
                            push_space(space, &mut spaces, &mut seen_space_ids);
                        }
                    }
                    KnowledgeScope::Global => {
                        let global_spaces = self.list_knowledge_spaces(None).await?;
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
            .list_knowledge_spaces(Some(&request.project_id))
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
        let binding = &request.binding;
        let project_spaces = if request.project_id.trim().is_empty() {
            Vec::new()
        } else {
            self.list_knowledge_spaces(Some(&request.project_id))
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
            .resolve_preflight_spaces(request, &coverage_key)
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
                .list_knowledge_items(&space.id, Some(&coverage_key))
                .await?;
            let coverage = self
                .get_knowledge_coverage(&coverage_key, &space.id)
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
        let (context, _) = self
            .retrieve_context_with_meta(query, project_id, session_id, token_budget)
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
        let config = if let Some(pid) = project_id {
            self.db.get_or_create_config(pid).await?
        } else {
            MemoryConfig::default()
        };
        let budget = token_budget.unwrap_or(config.token_budget);
        let retrieval_limit = config.retrieval_k.max(1);

        // Get recent session chunks
        let current_session = if let Some(sid) = session_id {
            self.db.get_session_chunks(sid).await?
        } else {
            Vec::new()
        };

        // Search for relevant history
        let search_results = self
            .search(query, None, project_id, session_id, Some(retrieval_limit))
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
        let count = self.db.clear_session_memory(session_id).await?;

        // Log cleanup
        self.db
            .log_cleanup(
                "manual",
                MemoryTier::Session,
                None,
                Some(session_id),
                count as i64,
                0,
            )
            .await?;

        Ok(count)
    }

    /// Clear project memory
    pub async fn clear_project(&self, project_id: &str) -> MemoryResult<u64> {
        let count = self.db.clear_project_memory(project_id).await?;

        // Log cleanup
        self.db
            .log_cleanup(
                "manual",
                MemoryTier::Project,
                Some(project_id),
                None,
                count as i64,
                0,
            )
            .await?;

        Ok(count)
    }

    /// Get memory statistics
    pub async fn get_stats(&self) -> MemoryResult<MemoryStats> {
        self.db.get_stats().await
    }

    /// Get memory configuration for a project
    pub async fn get_config(&self, project_id: &str) -> MemoryResult<MemoryConfig> {
        self.db.get_or_create_config(project_id).await
    }

    /// Update memory configuration for a project
    pub async fn set_config(&self, project_id: &str, config: &MemoryConfig) -> MemoryResult<()> {
        self.db.update_config(project_id, config).await
    }

    pub async fn resolve_uri(&self, uri: &str) -> MemoryResult<Option<MemoryNode>> {
        self.db.get_node_by_uri(uri).await
    }

    pub async fn list_directory(&self, uri: &str) -> MemoryResult<DirectoryListing> {
        let nodes = self.db.list_directory(uri).await?;
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

    pub async fn tree(&self, uri: &str, max_depth: usize) -> MemoryResult<Vec<TreeNode>> {
        self.db.get_children_tree(uri, max_depth).await
    }

    pub async fn create_context_node(
        &self,
        uri: &str,
        node_type: NodeType,
        metadata: Option<serde_json::Value>,
    ) -> MemoryResult<String> {
        let parsed_uri =
            ContextUri::parse(uri).map_err(|e| MemoryError::InvalidConfig(e.message))?;
        let parent_uri = parsed_uri.parent().map(|p| p.to_string());
        self.db
            .create_node(uri, parent_uri.as_deref(), node_type, metadata.as_ref())
            .await
    }

    pub async fn get_context_layer(
        &self,
        node_id: &str,
        layer_type: LayerType,
    ) -> MemoryResult<Option<MemoryLayer>> {
        self.db.get_layer(node_id, layer_type).await
    }

    pub async fn store_content_with_layers(
        &self,
        uri: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
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
            .create_node(uri, parent_uri.as_deref(), node_type, metadata.as_ref())
            .await?;

        let token_count = self.tokenizer.count_tokens(content) as i64;
        self.db
            .create_layer(&node_id, LayerType::L2, content, token_count, None)
            .await?;

        Ok(node_id)
    }

    pub async fn generate_layers_for_node(
        &self,
        node_id: &str,
        providers: &ProviderRegistry,
    ) -> MemoryResult<()> {
        let l2_layer = self.db.get_layer(node_id, LayerType::L2).await?;
        let l2_content = match l2_layer {
            Some(layer) => layer.content,
            None => return Ok(()),
        };

        let generator = ContextLayerGenerator::new(Arc::new(providers.clone()));

        let (l0_content, l1_content) = generator.generate_layers(&l2_content).await?;

        let l0_tokens = self.tokenizer.count_tokens(&l0_content) as i64;
        let l1_tokens = self.tokenizer.count_tokens(&l1_content) as i64;

        if self.db.get_layer(node_id, LayerType::L0).await?.is_none() {
            self.db
                .create_layer(node_id, LayerType::L0, &l0_content, l0_tokens, None)
                .await?;
        }

        if self.db.get_layer(node_id, LayerType::L1).await?.is_none() {
            self.db
                .create_layer(node_id, LayerType::L1, &l1_content, l1_tokens, None)
                .await?;
        }

        Ok(())
    }

    pub async fn get_layer_content(
        &self,
        node_id: &str,
        layer_type: LayerType,
    ) -> MemoryResult<Option<String>> {
        let layer = self.db.get_layer(node_id, layer_type).await?;
        Ok(layer.map(|l| l.content))
    }

    pub async fn store_content_with_layers_auto(
        &self,
        uri: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
        providers: Option<&ProviderRegistry>,
    ) -> MemoryResult<String> {
        let node_id = self
            .store_content_with_layers(uri, content, metadata)
            .await?;

        if let Some(p) = providers {
            if let Err(e) = self.generate_layers_for_node(&node_id, p).await {
                tracing::warn!("Failed to generate layers for node {}: {}", node_id, e);
            }
        }

        Ok(node_id)
    }

    /// Run cleanup based on retention policies
    pub async fn run_cleanup(&self, project_id: Option<&str>) -> MemoryResult<u64> {
        let mut total_cleaned = 0u64;

        if let Some(pid) = project_id {
            // Get config for this project
            let config = self.db.get_or_create_config(pid).await?;

            if config.auto_cleanup {
                // Clean up old session memory
                let cleaned = self
                    .db
                    .cleanup_old_sessions(config.session_retention_days)
                    .await?;
                total_cleaned += cleaned;

                if cleaned > 0 {
                    self.db
                        .log_cleanup(
                            "auto",
                            MemoryTier::Session,
                            Some(pid),
                            None,
                            cleaned as i64,
                            0,
                        )
                        .await?;
                }
            }
        } else {
            // Clean up all projects with auto_cleanup enabled
            // This would require listing all projects, for now just clean session memory
            // with a default retention period
            let cleaned = self.db.cleanup_old_sessions(30).await?;
            total_cleaned += cleaned;
        }

        // Vacuum if significant cleanup occurred
        if total_cleaned > 100 {
            self.db.vacuum().await?;
        }

        Ok(total_cleaned)
    }

    /// Check if cleanup is needed and run it
    async fn maybe_cleanup(&self, project_id: &Option<String>) -> MemoryResult<()> {
        if let Some(pid) = project_id {
            let stats = self.db.get_stats().await?;
            let config = self.db.get_or_create_config(pid).await?;

            // Check if we're over the chunk limit
            if stats.project_chunks > config.max_chunks {
                // Remove oldest chunks
                let excess = stats.project_chunks - config.max_chunks;
                // This would require a new DB method to delete oldest chunks
                // For now, just log
                tracing::info!("Project {} has {} excess chunks", pid, excess);
            }
        }

        Ok(())
    }

    /// Get cleanup log entries
    pub async fn get_cleanup_log(&self, _limit: i64) -> MemoryResult<Vec<CleanupLogEntry>> {
        // This would be implemented in the DB layer
        // For now, return empty
        Ok(Vec::new())
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

/// Create memory manager with default database path
pub async fn create_memory_manager(app_data_dir: &Path) -> MemoryResult<MemoryManager> {
    let db_path = app_data_dir.join("tandem_memory.db");
    MemoryManager::new(&db_path).await
}

