//! Deterministic repository intelligence primitives.
//!
//! This crate owns source-derived repo facts for Tandem. It deliberately keeps
//! subjective ranking, memory search, and ACA-specific prompt construction out
//! of the core so other runtime surfaces can reuse the same index.

mod chunks;
mod context;
mod debug;
mod error;
mod extractors;
mod governed;
mod graph_core;
mod hybrid;
mod manifest;
mod model;
mod query;
mod query_text;
mod scanner;
mod store;

pub use chunks::repo_chunks;
pub use context::{repo_context_bundle, repo_impact, repo_neighbors};
pub use debug::{
    repo_context_bundle_metrics, repo_debug_export, repo_index_metrics, repo_intelligence_event,
};
pub use error::{RepoIntelligenceError, Result};
pub use extractors::{extract_file_facts, extract_repo_facts};
pub use governed::{
    edges_by_relation_governed, repo_context_bundle_governed, repo_file_governed,
    repo_impact_governed, repo_neighbors_governed, repo_search_governed, repo_symbol_governed,
    symbols_by_kind_governed,
};
pub use graph_core::{
    confidence_provenance, graph_fact_for_edge, graph_scope_for_repo, relation_edge_kind,
    snapshot_freshness,
};
pub use hybrid::{merge_hybrid_candidates, RepoHybridRetriever};
pub use manifest::{ManifestDelta, ManifestIndex};
pub use model::{
    Confidence, ConfigReference, DocHeading, ExtractedFacts, ExtractedSymbol, FileChangeKind,
    FileManifestEntry, FileProcessingDecision, GraphEdge, GraphRelation, ImportEdge, IndexStats,
    RepoChunk, RepoContextBundle, RepoContextBundleMetrics, RepoContextBundleOptions,
    RepoContextSpan, RepoDebugExport, RepoGraphNeighbor, RepoHybridCandidate, RepoImpactItem,
    RepoImpactSummary, RepoIndexMetrics, RepoIndexSnapshot, RepoIntelligenceEvent,
    RepoRetrievalTrace, RepoScanOptions, RepoSearchResult, SymbolKind,
};
pub use query::{edges_by_relation, repo_file, repo_search, repo_symbol, symbols_by_kind};
pub use scanner::{scan_repo, scan_repo_with_options};
pub use store::JsonRepoIndexStore;
pub use tandem_graph_core::{GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput};

#[cfg(test)]
mod tests;
