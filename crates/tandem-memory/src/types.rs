// Memory Context Types
// Type definitions and error types for the memory system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
use thiserror::Error;

/// Memory tier - determines persistence level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Ephemeral session memory - cleared when session ends
    Session,
    /// Persistent project memory - survives across sessions
    Project,
    /// Cross-project global memory - user preferences and patterns
    Global,
}

impl MemoryTier {
    /// Get the table prefix for this tier
    pub fn table_prefix(&self) -> &'static str {
        match self {
            MemoryTier::Session => "session",
            MemoryTier::Project => "project",
            MemoryTier::Global => "global",
        }
    }
}

impl std::fmt::Display for MemoryTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryTier::Session => write!(f, "session"),
            MemoryTier::Project => write!(f, "project"),
            MemoryTier::Global => write!(f, "global"),
        }
    }
}

/// A memory chunk - unit of storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub content: String,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub source: String, // e.g., "user_message", "assistant_response", "file_content"
    // File-derived fields (only set when source == "file")
    pub source_path: Option<String>,
    pub source_mtime: Option<i64>,
    pub source_size: Option<i64>,
    pub source_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub token_count: i64,
    pub metadata: Option<serde_json::Value>,
}

/// Search result with similarity score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub chunk: MemoryChunk,
    pub similarity: f64,
}

/// Memory configuration for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum chunks to store per project
    pub max_chunks: i64,
    /// Chunk size in tokens
    pub chunk_size: i64,
    /// Number of chunks to retrieve
    pub retrieval_k: i64,
    /// Whether auto-cleanup is enabled
    pub auto_cleanup: bool,
    /// Session memory retention in days
    pub session_retention_days: i64,
    /// Token budget for memory context injection
    pub token_budget: i64,
    /// Overlap between chunks in tokens
    pub chunk_overlap: i64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_chunks: 10_000,
            chunk_size: 512,
            retrieval_k: 5,
            auto_cleanup: true,
            session_retention_days: 30,
            token_budget: 5000,
            chunk_overlap: 64,
        }
    }
}

/// Memory storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total number of chunks
    pub total_chunks: i64,
    /// Number of session chunks
    pub session_chunks: i64,
    /// Number of project chunks
    pub project_chunks: i64,
    /// Number of global chunks
    pub global_chunks: i64,
    /// Total size in bytes
    pub total_bytes: i64,
    /// Session memory size in bytes
    pub session_bytes: i64,
    /// Project memory size in bytes
    pub project_bytes: i64,
    /// Global memory size in bytes
    pub global_bytes: i64,
    /// Database file size in bytes
    pub file_size: i64,
    /// Last cleanup timestamp
    pub last_cleanup: Option<DateTime<Utc>>,
}

/// Context to inject into messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    /// Recent messages from current session
    pub current_session: Vec<MemoryChunk>,
    /// Relevant historical chunks
    pub relevant_history: Vec<MemoryChunk>,
    /// Important project facts
    pub project_facts: Vec<MemoryChunk>,
    /// Total tokens in context
    pub total_tokens: i64,
}

/// Metadata describing how memory retrieval executed for a single query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalMeta {
    pub used: bool,
    pub chunks_total: usize,
    pub session_chunks: usize,
    pub history_chunks: usize,
    pub project_fact_chunks: usize,
    pub score_min: Option<f64>,
    pub score_max: Option<f64>,
}

impl MemoryContext {
    /// Format the context for injection into a prompt
    pub fn format_for_injection(&self) -> String {
        let mut parts = Vec::new();

        if !self.current_session.is_empty() {
            parts.push("<current_session>".to_string());
            for chunk in &self.current_session {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</current_session>".to_string());
        }

        if !self.relevant_history.is_empty() {
            parts.push("<relevant_history>".to_string());
            for chunk in &self.relevant_history {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</relevant_history>".to_string());
        }

        if !self.project_facts.is_empty() {
            parts.push("<project_facts>".to_string());
            for chunk in &self.project_facts {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</project_facts>".to_string());
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("<memory_context>\n{}\n</memory_context>", parts.join("\n"))
        }
    }
}

/// Request to store a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMessageRequest {
    pub content: String,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub source: String,
    // File-derived fields (only set when source == "file")
    pub source_path: Option<String>,
    pub source_mtime: Option<i64>,
    pub source_size: Option<i64>,
    pub source_hash: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Project-scoped memory statistics (filtered by project_id)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryStats {
    pub project_id: String,
    /// Total chunks stored under this project_id (all sources)
    pub project_chunks: i64,
    pub project_bytes: i64,
    /// Chunks/bytes that came from workspace file indexing (source == "file")
    pub file_index_chunks: i64,
    pub file_index_bytes: i64,
    /// Number of indexed files currently tracked for this project_id
    pub indexed_files: i64,
    /// Last time indexing completed for this project_id (if known)
    pub last_indexed_at: Option<DateTime<Utc>>,
    /// Last run totals (if known)
    pub last_total_files: Option<i64>,
    pub last_processed_files: Option<i64>,
    pub last_indexed_files: Option<i64>,
    pub last_skipped_files: Option<i64>,
    pub last_errors: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearFileIndexResult {
    pub chunks_deleted: i64,
    pub bytes_estimated: i64,
    pub did_vacuum: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportFormat {
    Directory,
    Openclaw,
}

impl std::fmt::Display for MemoryImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryImportFormat::Directory => write!(f, "directory"),
            MemoryImportFormat::Openclaw => write!(f, "openclaw"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryImportRequest {
    pub root_path: String,
    pub format: MemoryImportFormat,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub sync_deletes: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryImportStats {
    pub discovered_files: usize,
    pub files_processed: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub chunks_created: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryImportProgress {
    pub files_processed: usize,
    pub total_files: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub errors: usize,
    pub chunks_created: usize,
    pub current_file: String,
}

/// Request to search memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub tier: Option<MemoryTier>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<i64>,
}

/// Embedding backend health surfaced to UI/events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingHealth {
    /// "ok" when embeddings are available, "degraded_disabled" otherwise.
    pub status: String,
    /// Optional reason when degraded.
    pub reason: Option<String>,
}

/// Memory error types
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Chunking error: {0}")]
    Chunking(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Tokenization error: {0}")]
    Tokenization(String),

    #[error("Lock error: {0}")]
    Lock(String),
}

impl From<String> for MemoryError {
    fn from(err: String) -> Self {
        MemoryError::InvalidConfig(err)
    }
}

impl From<&str> for MemoryError {
    fn from(err: &str) -> Self {
        MemoryError::InvalidConfig(err.to_string())
    }
}

// Implement serialization for Tauri commands
impl serde::Serialize for MemoryError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type MemoryResult<T> = Result<T, MemoryError>;

/// Cleanup log entry for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupLogEntry {
    pub id: String,
    pub cleanup_type: String,
    pub tier: MemoryTier,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub chunks_deleted: i64,
    pub bytes_reclaimed: i64,
    pub created_at: DateTime<Utc>,
}

/// Default embedding dimension for all-MiniLM-L6-v2
pub const DEFAULT_EMBEDDING_DIMENSION: usize = 384;

/// Default embedding model name
pub const DEFAULT_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";

/// Maximum content length for a single chunk (in characters)
pub const MAX_CHUNK_LENGTH: usize = 4000;

/// Minimum content length for a chunk (in characters)
pub const MIN_CHUNK_LENGTH: usize = 50;

/// Persistent global memory record keyed by user identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemoryRecord {
    pub id: String,
    pub user_id: String,
    pub source_type: String,
    pub content: String,
    pub content_hash: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub tool_name: Option<String>,
    pub project_tag: Option<String>,
    pub channel_tag: Option<String>,
    pub host_tag: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub provenance: Option<serde_json::Value>,
    pub redaction_status: String,
    pub redaction_count: u32,
    pub visibility: String,
    pub demoted: bool,
    pub score_boost: f64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemoryWriteResult {
    pub id: String,
    pub stored: bool,
    pub deduped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemorySearchHit {
    pub record: GlobalMemoryRecord,
    pub score: f64,
}

/// Status for a reusable knowledge item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeItemStatus {
    Working,
    Promoted,
    ApprovedDefault,
    Deprecated,
}

impl std::fmt::Display for KnowledgeItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Working => write!(f, "working"),
            Self::Promoted => write!(f, "promoted"),
            Self::ApprovedDefault => write!(f, "approved_default"),
            Self::Deprecated => write!(f, "deprecated"),
        }
    }
}

impl std::str::FromStr for KnowledgeItemStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "working" => Ok(Self::Working),
            "promoted" => Ok(Self::Promoted),
            "approved_default" => Ok(Self::ApprovedDefault),
            "deprecated" => Ok(Self::Deprecated),
            other => Err(format!("unknown knowledge item status: {}", other)),
        }
    }
}

impl Default for KnowledgeItemStatus {
    fn default() -> Self {
        Self::Working
    }
}

impl KnowledgeItemStatus {
    pub fn as_trust_level(self) -> Option<KnowledgeTrustLevel> {
        match self {
            Self::Working => Some(KnowledgeTrustLevel::Working),
            Self::Promoted => Some(KnowledgeTrustLevel::Promoted),
            Self::ApprovedDefault => Some(KnowledgeTrustLevel::ApprovedDefault),
            Self::Deprecated => None,
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::Deprecated)
    }
}

/// A reusable knowledge space scoped to a project, run, or global namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSpaceRecord {
    pub id: String,
    pub scope: KnowledgeScope,
    pub project_id: Option<String>,
    pub namespace: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub trust_level: KnowledgeTrustLevel,
    pub metadata: Option<serde_json::Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// A reusable knowledge item promoted from run artifacts or validated memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItemRecord {
    pub id: String,
    pub space_id: String,
    pub coverage_key: String,
    pub dedupe_key: String,
    pub item_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub payload: serde_json::Value,
    pub trust_level: KnowledgeTrustLevel,
    pub status: KnowledgeItemStatus,
    pub run_id: Option<String>,
    pub artifact_refs: Vec<String>,
    pub source_memory_ids: Vec<String>,
    pub freshness_expires_at_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Coverage state for a task/topic key within a knowledge space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCoverageRecord {
    pub coverage_key: String,
    pub space_id: String,
    pub latest_item_id: Option<String>,
    pub latest_dedupe_key: Option<String>,
    pub last_seen_at_ms: u64,
    pub last_promoted_at_ms: Option<u64>,
    pub freshness_expires_at_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
}

/// Request to promote or retire a knowledge item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePromotionRequest {
    pub item_id: String,
    pub target_status: KnowledgeItemStatus,
    pub promoted_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Result of a knowledge item promotion or retirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePromotionResult {
    pub previous_status: KnowledgeItemStatus,
    pub previous_trust_level: KnowledgeTrustLevel,
    pub promoted: bool,
    pub item: KnowledgeItemRecord,
    pub coverage: KnowledgeCoverageRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Directory,
    File,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Directory => write!(f, "directory"),
            NodeType::File => write!(f, "file"),
        }
    }
}

impl std::str::FromStr for NodeType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "directory" => Ok(NodeType::Directory),
            "file" => Ok(NodeType::File),
            _ => Err(format!("unknown node type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LayerType {
    L0,
    L1,
    L2,
}

impl std::fmt::Display for LayerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayerType::L0 => write!(f, "L0"),
            LayerType::L1 => write!(f, "L1"),
            LayerType::L2 => write!(f, "L2"),
        }
    }
}

impl std::str::FromStr for LayerType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "L0" | "L0_ABSTRACT" => Ok(LayerType::L0),
            "L1" | "L1_OVERVIEW" => Ok(LayerType::L1),
            "L2" | "L2_DETAIL" => Ok(LayerType::L2),
            _ => Err(format!("unknown layer type: {}", s)),
        }
    }
}

impl LayerType {
    pub fn default_tokens(&self) -> usize {
        match self {
            LayerType::L0 => 100,
            LayerType::L1 => 2000,
            LayerType::L2 => 4000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalStep {
    pub step_type: String,
    pub description: String,
    pub layer_accessed: Option<LayerType>,
    pub nodes_evaluated: usize,
    pub scores: std::collections::HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeVisit {
    pub uri: String,
    pub node_type: NodeType,
    pub score: f64,
    pub depth: usize,
    pub layer_loaded: Option<LayerType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalTrajectory {
    pub id: String,
    pub query: String,
    pub root_uri: String,
    pub steps: Vec<RetrievalStep>,
    pub visited_nodes: Vec<NodeVisit>,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub node_id: String,
    pub uri: String,
    pub content: String,
    pub layer_type: LayerType,
    pub score: f64,
    pub trajectory: RetrievalTrajectory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    pub id: String,
    pub uri: String,
    pub parent_uri: Option<String>,
    pub node_type: NodeType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLayer {
    pub id: String,
    pub node_id: String,
    pub layer_type: LayerType,
    pub content: String,
    pub token_count: i64,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source_chunk_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub node: MemoryNode,
    pub children: Vec<TreeNode>,
    pub layer_summary: Option<LayerSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSummary {
    pub l0_preview: Option<String>,
    pub l1_preview: Option<String>,
    pub has_l2: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListing {
    pub uri: String,
    pub nodes: Vec<MemoryNode>,
    pub total_children: usize,
    pub directories: Vec<MemoryNode>,
    pub files: Vec<MemoryNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledFact {
    pub id: String,
    pub distillation_id: String,
    pub content: String,
    pub category: FactCategory,
    pub importance_score: f64,
    pub source_message_ids: Vec<String>,
    pub contradicts_fact_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactCategory {
    UserPreference,
    TaskOutcome,
    Learning,
    Fact,
}

impl std::fmt::Display for FactCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactCategory::UserPreference => write!(f, "user_preference"),
            FactCategory::TaskOutcome => write!(f, "task_outcome"),
            FactCategory::Learning => write!(f, "learning"),
            FactCategory::Fact => write!(f, "fact"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillationReport {
    pub distillation_id: String,
    pub session_id: String,
    pub distilled_at: DateTime<Utc>,
    pub facts_extracted: usize,
    pub importance_threshold: f64,
    pub user_memory_updated: bool,
    pub agent_memory_updated: bool,
    #[serde(default)]
    pub stored_count: usize,
    #[serde(default)]
    pub deduped_count: usize,
    #[serde(default)]
    pub memory_ids: Vec<String>,
    #[serde(default)]
    pub candidate_ids: Vec<String>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDistillation {
    pub id: String,
    pub session_id: String,
    pub distilled_at: DateTime<Utc>,
    pub input_token_count: i64,
    pub output_memory_count: usize,
    pub key_facts_extracted: usize,
    pub importance_threshold: f64,
}
