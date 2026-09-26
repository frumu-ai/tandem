// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Pure solution planning. This crate does not provision identities, download
//! packs, grant permissions, or mutate a running Tandem installation.
mod contract;
mod customer_config;
mod customer_contract;
mod resolve;
mod validate;

pub use contract::*;
pub use customer_config::*;
pub use customer_contract::*;
pub use resolve::{resolve, ResolutionInput};
pub use validate::{blueprint_hash, parse_blueprint, validate_blueprint};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: &str = "1";
pub const RESOLVER_VERSION: &str = "1.0.1";
pub const MAX_BLUEPRINT_BYTES: usize = 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COMPONENTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionError {
    pub code: String,
    pub path: String,
    pub message: String,
}

impl SolutionError {
    pub(crate) fn new(code: &str, path: impl Into<String>, message: &str) -> Self {
        Self {
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl std::error::Error for SolutionError {}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// Object ordering is explicit even when another workspace crate enables
// serde_json/preserve_order. Arrays retain order; set-like fields use BTreeSet.
pub fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, SolutionError> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let ordered: std::collections::BTreeMap<_, _> = map.into_iter().collect();
                serde_json::Value::Object(
                    ordered.into_iter().map(|(k, v)| (k, sorted(v))).collect(),
                )
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sorted).collect())
            }
            value => value,
        }
    }
    let value = serde_json::to_value(value)
        .map_err(|_| SolutionError::new("serialization", "$", "Cannot encode contract"))?;
    serde_json::to_vec(&sorted(value))
        .map_err(|_| SolutionError::new("serialization", "$", "Cannot encode contract"))
}

pub fn blueprint_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(SolutionBlueprint)
}
