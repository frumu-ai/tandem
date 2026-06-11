use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RepoIntelligenceError {
    #[error("repository root does not exist: {0}")]
    RootMissing(PathBuf),

    #[error("repository root is not a directory: {0}")]
    RootNotDirectory(PathBuf),

    #[error("failed to walk repository: {0}")]
    Walk(String),

    #[error("failed to read metadata for {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read file for hashing {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, RepoIntelligenceError>;
