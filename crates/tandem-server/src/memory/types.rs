// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_memory::{GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryPartition};

#[derive(Debug, Clone)]
pub struct GovernedMemoryRecord {
    pub id: String,
    pub run_id: String,
    pub partition: MemoryPartition,
    pub kind: MemoryContentKind,
    pub content: String,
    pub artifact_refs: Vec<String>,
    pub classification: MemoryClassification,
    pub metadata: Option<Value>,
    pub source_memory_id: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAuditEvent {
    pub audit_id: String,
    pub action: String,
    pub run_id: String,
    #[serde(default)]
    pub tenant_context: tandem_types::TenantContext,
    pub memory_id: Option<String>,
    pub source_memory_id: Option<String>,
    pub to_tier: Option<GovernedMemoryTier>,
    pub partition_key: String,
    pub actor: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at_ms: u64,
}
