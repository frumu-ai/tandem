use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableGraphHashError {
    message: String,
}

impl StableGraphHashError {
    fn from_serde(error: serde_json::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for StableGraphHashError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StableGraphHashError {}

pub trait StableGraphHash: Serialize {
    fn stable_graph_hash(&self) -> Result<String, StableGraphHashError> {
        stable_graph_hash(self)
    }
}

impl<T> StableGraphHash for T where T: Serialize {}

pub fn stable_graph_hash<T>(value: &T) -> Result<String, StableGraphHashError>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec(value).map_err(StableGraphHashError::from_serde)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}
