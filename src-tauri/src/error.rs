// Tandem Error Types
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TandemError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Path not allowed: {0}")]
    #[allow(dead_code)]
    PathNotAllowed(String),

    #[error("Permission denied: {0}")]
    #[allow(dead_code)]
    PermissionDenied(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Sidecar error: {0}")]
    Sidecar(String),

    #[error("Provider error: {0}")]
    #[allow(dead_code)]
    Provider(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Ralph loop error: {0}")]
    Ralph(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    #[error("Memory error: {0}")]
    Memory(String),
}

// Allow conversion from String to TandemError
impl From<String> for TandemError {
    fn from(err: String) -> Self {
        TandemError::InvalidConfig(err)
    }
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
