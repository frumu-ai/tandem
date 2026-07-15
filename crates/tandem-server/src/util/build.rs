// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::time::UNIX_EPOCH;

use serde::Serialize;

pub fn build_id() -> String {
    if let Some(explicit) = option_env!("TANDEM_BUILD_ID") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(git_sha) = option_env!("VERGEN_GIT_SHA") {
        let trimmed = git_sha.trim();
        if !trimmed.is_empty() {
            return format!("{}+{}", env!("CARGO_PKG_VERSION"), trimmed);
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn git_sha() -> Option<String> {
    option_env!("VERGEN_GIT_SHA")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn binary_path_for_health() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

pub fn binary_modified_at_ms() -> Option<u64> {
    let path = std::env::current_exe().ok()?;
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(duration.as_millis()).ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildProvenance {
    pub version: String,
    pub build_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_modified_at_ms: Option<u64>,
}

pub fn build_provenance() -> BuildProvenance {
    BuildProvenance {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_id: build_id(),
        git_sha: git_sha(),
        binary_path: binary_path_for_health(),
        binary_modified_at_ms: binary_modified_at_ms(),
    }
}
