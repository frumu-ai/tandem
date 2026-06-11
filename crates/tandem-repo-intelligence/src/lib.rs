//! Deterministic repository intelligence primitives.
//!
//! This crate owns source-derived repo facts for Tandem. It deliberately keeps
//! subjective ranking, memory search, and ACA-specific prompt construction out
//! of the core so other runtime surfaces can reuse the same index.

mod error;
mod extractors;
mod manifest;
mod model;
mod scanner;

pub use error::{RepoIntelligenceError, Result};
pub use extractors::{extract_file_facts, extract_repo_facts};
pub use manifest::{ManifestDelta, ManifestIndex};
pub use model::{
    Confidence, ConfigReference, DocHeading, ExtractedFacts, ExtractedSymbol, FileChangeKind,
    FileManifestEntry, FileProcessingDecision, ImportEdge, IndexStats, RepoScanOptions, SymbolKind,
};
pub use scanner::{scan_repo, scan_repo_with_options};

#[cfg(test)]
mod tests;
