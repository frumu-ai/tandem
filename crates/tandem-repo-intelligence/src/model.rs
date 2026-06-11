use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoScanOptions {
    pub max_file_size_bytes: u64,
    pub excluded_dirs: Vec<String>,
    pub excluded_extensions: Vec<String>,
}

impl Default for RepoScanOptions {
    fn default() -> Self {
        Self {
            max_file_size_bytes: 2 * 1024 * 1024,
            excluded_dirs: [
                ".git",
                "node_modules",
                "dist",
                "build",
                "target",
                ".next",
                ".turbo",
                ".cache",
                ".venv",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            excluded_extensions: [
                "exe", "dll", "so", "dylib", "bin", "obj", "iso", "zip", "png", "jpg", "jpeg",
                "gif", "ico", "pdf",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifestEntry {
    pub path: String,
    pub size_bytes: u64,
    pub modified_unix_ms: u128,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileProcessingDecision {
    Indexed,
    SkippedIgnored,
    SkippedDirectory,
    SkippedExtension,
    SkippedOversized,
    SkippedMetadataError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeKind {
    Added,
    Modified,
    Deleted,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStats {
    pub root: PathBuf,
    pub indexed_files: usize,
    pub added_files: usize,
    pub modified_files: usize,
    pub unchanged_files: usize,
    pub deleted_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    Extracted,
    Inferred,
    Summary,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Class,
    Interface,
    TypeAlias,
    Constant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedSymbol {
    pub file_path: String,
    pub line: usize,
    pub name: String,
    pub kind: SymbolKind,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportEdge {
    pub source_file: String,
    pub line: usize,
    pub target: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigReference {
    pub file_path: String,
    pub line: usize,
    pub key: String,
    pub value: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocHeading {
    pub file_path: String,
    pub line: usize,
    pub level: usize,
    pub title: String,
    pub excerpt: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedFacts {
    pub symbols: Vec<ExtractedSymbol>,
    pub imports: Vec<ImportEdge>,
    pub config_references: Vec<ConfigReference>,
    pub doc_headings: Vec<DocHeading>,
}

impl ExtractedFacts {
    pub fn extend(&mut self, next: ExtractedFacts) {
        self.symbols.extend(next.symbols);
        self.imports.extend(next.imports);
        self.config_references.extend(next.config_references);
        self.doc_headings.extend(next.doc_headings);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GraphRelation {
    Defines,
    Imports,
    Configures,
    Documents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: GraphRelation,
    pub line: usize,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSearchResult {
    pub file_path: String,
    pub line: usize,
    pub kind: String,
    pub label: String,
    pub reason: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoGraphNeighbor {
    pub node: String,
    pub edge: GraphEdge,
    pub depth: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoImpactItem {
    pub file_path: String,
    pub line: usize,
    pub relation: GraphRelation,
    pub reason: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoImpactSummary {
    pub changed_files: Vec<String>,
    pub directly_affected: Vec<RepoImpactItem>,
    pub import_neighbors: Vec<RepoImpactItem>,
    pub config_or_docs: Vec<RepoImpactItem>,
    pub likely_test_targets: Vec<RepoImpactItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoContextBundleOptions {
    pub budget_chars: usize,
    pub required_files: Vec<String>,
    pub changed_files: Vec<String>,
    pub path_scope: Option<String>,
    pub result_limit: usize,
}

impl Default for RepoContextBundleOptions {
    fn default() -> Self {
        Self {
            budget_chars: 6_000,
            required_files: Vec::new(),
            changed_files: Vec::new(),
            path_scope: None,
            result_limit: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoContextBundle {
    pub task: String,
    pub budget_chars: usize,
    pub likely_files: Vec<RepoSearchResult>,
    pub relevant_symbols: Vec<RepoSearchResult>,
    pub graph_edges: Vec<GraphEdge>,
    pub suggested_first_reads: Vec<String>,
    pub test_targets: Vec<String>,
    pub gaps: Vec<String>,
    pub estimated_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIndexMetrics {
    pub indexed_unix_ms: u128,
    pub files_indexed: usize,
    pub total_bytes: u64,
    pub symbols: usize,
    pub imports: usize,
    pub config_references: usize,
    pub doc_headings: usize,
    pub graph_edges: usize,
    pub top_file_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoContextBundleMetrics {
    pub task: String,
    pub estimated_chars: usize,
    pub budget_chars: usize,
    pub likely_files: usize,
    pub relevant_symbols: usize,
    pub graph_edges: usize,
    pub suggested_first_reads: usize,
    pub test_targets: usize,
    pub gaps: usize,
    pub top_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoDebugExport {
    pub root_label: String,
    pub generated_unix_ms: u128,
    pub metrics: RepoIndexMetrics,
    pub graph_edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntelligenceEvent {
    pub event: String,
    pub repo_root: String,
    pub unix_ms: u128,
    pub metrics: Option<RepoIndexMetrics>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIndexSnapshot {
    pub root_label: String,
    pub indexed_unix_ms: u128,
    pub manifest: Vec<FileManifestEntry>,
    pub facts: ExtractedFacts,
}

impl RepoIndexSnapshot {
    pub fn is_empty(&self) -> bool {
        self.manifest.is_empty() && self.facts == ExtractedFacts::default()
    }

    pub fn graph_edges(&self) -> Vec<GraphEdge> {
        let mut edges = Vec::new();
        for symbol in &self.facts.symbols {
            edges.push(GraphEdge {
                source: symbol.file_path.clone(),
                target: symbol.name.clone(),
                relation: GraphRelation::Defines,
                line: symbol.line,
                confidence: symbol.confidence.clone(),
            });
        }
        for import in &self.facts.imports {
            edges.push(GraphEdge {
                source: import.source_file.clone(),
                target: import.target.clone(),
                relation: GraphRelation::Imports,
                line: import.line,
                confidence: import.confidence.clone(),
            });
        }
        for reference in &self.facts.config_references {
            edges.push(GraphEdge {
                source: reference.file_path.clone(),
                target: reference.key.clone(),
                relation: GraphRelation::Configures,
                line: reference.line,
                confidence: reference.confidence.clone(),
            });
        }
        for heading in &self.facts.doc_headings {
            edges.push(GraphEdge {
                source: heading.file_path.clone(),
                target: heading.title.clone(),
                relation: GraphRelation::Documents,
                line: heading.line,
                confidence: heading.confidence.clone(),
            });
        }
        edges
    }
}
