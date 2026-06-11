use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    Extracted,
    Configured,
    Observed,
    Inferred,
    Summarized,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessSource {
    Unknown,
    Commit,
    IndexRevision,
    WorkflowVersion,
    Run,
    MemorySnapshot,
    PolicyHash,
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
    Allowed,
    Denied { reason: String },
    RequiresApproval { approval_gate: String },
}
