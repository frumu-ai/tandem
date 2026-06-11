//! Deterministic repository intelligence primitives.
//!
//! This crate owns source-derived repo facts for Tandem. It deliberately keeps
//! subjective ranking, memory search, and ACA-specific prompt construction out
//! of the core so other runtime surfaces can reuse the same index.

mod error;
mod manifest;
mod model;
mod scanner;

pub use error::{RepoIntelligenceError, Result};
pub use manifest::{ManifestDelta, ManifestIndex};
pub use model::{
    FileChangeKind, FileManifestEntry, FileProcessingDecision, IndexStats, RepoScanOptions,
};
pub use scanner::{scan_repo, scan_repo_with_options};

#[cfg(test)]
mod tests;
