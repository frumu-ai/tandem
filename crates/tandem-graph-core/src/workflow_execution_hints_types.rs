use crate::WorkflowBlocker;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionHintsQuery {
    pub tool_risk_hints: Vec<WorkflowToolRiskHint>,
    pub failure_history: Vec<WorkflowStepFailureHistory>,
    pub default_budget_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowToolRiskHint {
    pub tool_name: String,
    pub authority_level: String,
    pub side_effects: bool,
    pub data_classes: Vec<String>,
    pub approval_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepFailureHistory {
    pub step_id: String,
    pub failure_count: u32,
    pub recent_failure_rate_bps: Option<u32>,
    pub last_failure_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionHintsReport {
    pub step_hints: Vec<WorkflowStepExecutionHint>,
    pub metrics: WorkflowRoutingMetrics,
    pub blockers: Vec<WorkflowBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepExecutionHint {
    pub step_id: String,
    pub risk_tier: WorkflowRiskTier,
    pub model_tier: WorkflowModelTier,
    pub budget_tokens: u64,
    pub timeout_ms: u64,
    pub max_retries: u8,
    pub approval_posture: WorkflowApprovalPosture,
    pub reasons: Vec<String>,
    pub required_tools: Vec<String>,
    pub policy_scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRiskTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowModelTier {
    Small,
    Standard,
    Large,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowApprovalPosture {
    None,
    HumanReview,
    StrongReview,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRoutingMetrics {
    pub high_risk_steps: usize,
    pub historical_failure_steps: usize,
    pub approval_required_steps: usize,
    pub baseline_budget_tokens: u64,
    pub recommended_budget_tokens: u64,
    pub budget_delta_tokens: i64,
    pub historical_failure_rate_bps: Option<u32>,
    pub graph_guided_failure_rate_bps: Option<u32>,
}
