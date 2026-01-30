// Memory Context Module
// Provides local vector-based memory for Tandem

pub mod chunking;
pub mod db;
pub mod embeddings;
pub mod manager;
pub mod types;

// Re-export commonly used types
pub use manager::MemoryManager;
// pub use manager::create_memory_manager;
// pub use types::{MemoryResult, MemorySearchResult, MemoryStats, MemoryTier, StoreMessageRequest};
