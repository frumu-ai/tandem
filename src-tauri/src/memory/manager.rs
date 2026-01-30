// Memory Manager Module
// High-level memory operations (store, retrieve, cleanup)

use crate::memory::chunking::{chunk_text_semantic, ChunkingConfig, Tokenizer};
use crate::memory::db::MemoryDatabase;
use crate::memory::embeddings::EmbeddingService;
use crate::memory::types::{
    CleanupLogEntry, MemoryChunk, MemoryConfig, MemoryContext, MemoryResult, MemorySearchResult,
    MemoryStats, MemoryTier, StoreMessageRequest,
};
use chrono::Utc;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// High-level memory manager that coordinates database, embeddings, and chunking
pub struct MemoryManager {
    db: Arc<MemoryDatabase>,
    embedding_service: Arc<Mutex<EmbeddingService>>,
    tokenizer: Tokenizer,
}

impl MemoryManager {
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
                created_at: Utc::now(),
                token_count: text_chunk.token_count as i64,
                metadata: request.metadata.clone(),
            };

            // Store in database
            self.db.store_chunk(&chunk, &embedding).await?;
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
            None => vec![MemoryTier::Session, MemoryTier::Project, MemoryTier::Global],
        };

        for search_tier in tiers_to_search {
            let tier_results = self
                .db
                .search_similar(
                    &query_embedding,
                    search_tier,
                    project_id,
                    session_id,
                    effective_limit,
                )
                .await?;

            for (chunk, distance) in tier_results {
                // Convert distance to similarity (cosine similarity)
                // sqlite-vec returns distance, where lower is more similar
                // Cosine similarity ranges from -1 to 1, but for normalized vectors it's 0 to 1
                let similarity = 1.0 - (distance as f64).clamp(0.0, 1.0);

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
        let budget = token_budget.unwrap_or(5000);

        // Get recent session chunks
        let current_session = if let Some(sid) = session_id {
            self.db.get_session_chunks(sid).await?
        } else {
            Vec::new()
        };

        // Search for relevant history
        let search_results = self
            .search(query, None, project_id, session_id, Some(10))
            .await?;

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
            self.trim_context(&mut relevant_history, &mut project_facts, excess)?;
            total_tokens = budget;
        }

        Ok(MemoryContext {
            current_session,
            relevant_history,
            project_facts,
            total_tokens,
        })
    }

    /// Trim context to fit within token budget
    fn trim_context(
        &self,
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
}

/// Create memory manager with default database path
pub async fn create_memory_manager(app_data_dir: &Path) -> MemoryResult<MemoryManager> {
    let db_path = app_data_dir.join("tandem_memory.db");
    MemoryManager::new(&db_path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_manager() -> (MemoryManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_memory.db");
        let manager = MemoryManager::new(&db_path).await.unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let (manager, _temp) = setup_test_manager().await;

        let request = StoreMessageRequest {
            content: "This is a test message about artificial intelligence and machine learning."
                .to_string(),
            tier: MemoryTier::Project,
            session_id: Some("session-1".to_string()),
            project_id: Some("project-1".to_string()),
            source: "user_message".to_string(),
            metadata: None,
        };

        let chunk_ids = manager.store_message(request).await.unwrap();
        assert!(!chunk_ids.is_empty());

        // Search for the content
        let results = manager
            .search(
                "artificial intelligence",
                None,
                Some("project-1"),
                None,
                None,
            )
            .await
            .unwrap();

        assert!(!results.is_empty());
        assert!(results[0].similarity > 0.0);
    }

    #[tokio::test]
    async fn test_retrieve_context() {
        let (manager, _temp) = setup_test_manager().await;

        // Store some test data
        let request = StoreMessageRequest {
            content: "The project uses React and TypeScript for the frontend.".to_string(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some("project-1".to_string()),
            source: "assistant_response".to_string(),
            metadata: None,
        };
        manager.store_message(request).await.unwrap();

        let context = manager
            .retrieve_context("What technologies are used?", Some("project-1"), None, None)
            .await
            .unwrap();

        assert!(!context.project_facts.is_empty());
    }

    #[tokio::test]
    async fn test_config_management() {
        let (manager, _temp) = setup_test_manager().await;

        let config = manager.get_config("project-1").await.unwrap();
        assert_eq!(config.max_chunks, 10000);

        let new_config = MemoryConfig {
            max_chunks: 5000,
            retrieval_k: 10,
            ..Default::default()
        };

        manager.set_config("project-1", &new_config).await.unwrap();

        let updated = manager.get_config("project-1").await.unwrap();
        assert_eq!(updated.max_chunks, 5000);
        assert_eq!(updated.retrieval_k, 10);
    }
}
