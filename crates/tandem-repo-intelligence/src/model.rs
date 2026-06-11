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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
