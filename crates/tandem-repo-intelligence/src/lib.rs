//! Deterministic repository intelligence primitives.
//!
//! This crate owns source-derived repo facts for Tandem. It deliberately keeps
//! subjective ranking, memory search, and ACA-specific prompt construction out
//! of the core so other runtime surfaces can reuse the same index.

mod context;
mod debug;
mod error;
mod extractors;
mod manifest;
mod model;
mod query;
mod scanner;
mod store;

pub use context::{repo_context_bundle, repo_impact, repo_neighbors};
pub use debug::{
    repo_context_bundle_metrics, repo_debug_export, repo_index_metrics, repo_intelligence_event,
};
pub use error::{RepoIntelligenceError, Result};
pub use extractors::{extract_file_facts, extract_repo_facts};
pub use manifest::{ManifestDelta, ManifestIndex};
pub use model::{
    Confidence, ConfigReference, DocHeading, ExtractedFacts, ExtractedSymbol, FileChangeKind,
    FileManifestEntry, FileProcessingDecision, GraphEdge, GraphRelation, ImportEdge, IndexStats,
    RepoContextBundle, RepoContextBundleMetrics, RepoContextBundleOptions, RepoDebugExport,
    RepoGraphNeighbor, RepoImpactItem, RepoImpactSummary, RepoIndexMetrics, RepoIndexSnapshot,
    RepoIntelligenceEvent, RepoScanOptions, RepoSearchResult, SymbolKind,
};
pub use query::{edges_by_relation, repo_file, repo_search, repo_symbol, symbols_by_kind};
pub use scanner::{scan_repo, scan_repo_with_options};
pub use store::JsonRepoIndexStore;

#[cfg(test)]
mod tests;
