// Memory Context Types
// Type definitions and error types for the memory system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

/// Request to search memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub tier: Option<MemoryTier>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<i64>,
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
