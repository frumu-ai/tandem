use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailureCausalityReport {
    pub run_id: String,
    pub root_causes: Vec<RunFailureCause>,
    pub cascading_failures: Vec<RunCascadingFailure>,
    pub repair_context: RunRepairContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailureCause {
    pub event_id: String,
    pub step_id: Option<String>,
    pub kind: RunFailureCauseKind,
    pub target: Option<String>,
    pub summary: String,
    pub evidence: Vec<RunRepairEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCascadingFailure {
    pub event_id: String,
    pub step_id: Option<String>,
    pub upstream_root_steps: Vec<String>,
    pub summary: String,
    pub evidence: Vec<RunRepairEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunFailureCauseKind {
    ToolFailure,
    MemoryFailure,
    PolicyDenied,
    ModelFailure,
    ArtifactFailure,
    ApprovalInterruption,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRepairContext {
    pub summary: String,
    pub related_steps: Vec<String>,
    pub relevant_tools: Vec<String>,
    pub memory_tiers: Vec<String>,
    pub policy_scopes: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub evidence: Vec<RunRepairEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRepairEvidence {
    pub event_id: String,
    pub event_kind: String,
    pub step_id: Option<String>,
    pub target: Option<String>,
    pub safe_summary: String,
}
