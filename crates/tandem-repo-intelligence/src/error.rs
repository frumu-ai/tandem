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

    #[error("failed to write index store {path}: {source}")]
    WriteStore {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read index store {path}: {source}")]
    ReadStore {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to encode index store {path}: {source}")]
    EncodeStore {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("failed to decode index store {path}: {source}")]
    DecodeStore {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, RepoIntelligenceError>;
