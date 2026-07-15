// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::TenantContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedResourceRecord {
    pub key: String,
    pub value: Value,
    pub rev: u64,
    pub updated_at_ms: u64,
    pub updated_by: String,
    #[serde(default = "TenantContext::local_implicit")]
    pub tenant_context: TenantContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceConflict {
    pub key: String,
    pub expected_rev: Option<u64>,
    pub current_rev: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceStoreError {
    InvalidKey { key: String },
    RevisionConflict(ResourceConflict),
    PersistFailed { message: String },
}
