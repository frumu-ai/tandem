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
