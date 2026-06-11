use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    #[serde(rename = "extracted")]
    Extracted,
    #[serde(rename = "configured")]
    Configured,
    #[serde(rename = "observed")]
    Observed,
    #[serde(rename = "inferred")]
    Inferred,
    #[serde(rename = "summarized")]
    Summarized,
    #[serde(rename = "ambiguous")]
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessSource {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "commit")]
    Commit,
    #[serde(rename = "index_revision")]
    IndexRevision,
    #[serde(rename = "workflow_version")]
    WorkflowVersion,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "memory_snapshot")]
    MemorySnapshot,
    #[serde(rename = "policy_hash")]
    PolicyHash,
    #[serde(rename = "tool_schema_hash")]
    ToolSchemaHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    pub source: FreshnessSource,
    pub revision: Option<String>,
}

impl Freshness {
    pub fn unknown() -> Self {
        Self {
            source: FreshnessSource::Unknown,
            revision: None,
        }
    }

    pub fn from_revision(source: FreshnessSource, revision: impl Into<String>) -> Self {
        Self {
            source,
            revision: Some(revision.into()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visibility {
    pub tenant_id: Option<String>,
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub readable_paths: Vec<String>,
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    #[serde(rename = "allowed")]
    Allowed,
    #[serde(rename = "denied")]
    Denied { reason: String },
    #[serde(rename = "requires_approval")]
    RequiresApproval { approval_gate: String },
}
