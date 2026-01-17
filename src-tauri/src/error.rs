// Tandem Error Types
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TandemError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Stronghold error: {0}")]
    Stronghold(String),

    #[error("Path not allowed: {0}")]
    PathNotAllowed(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Sidecar error: {0}")]
    Sidecar(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

// Implement serialization for Tauri commands
impl serde::Serialize for TandemError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TandemError>;
