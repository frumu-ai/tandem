// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan_package::PartialFailureMode;

use crate::materialization::ProjectedAutomationContextMaterialization;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedAutomationStageKind {
    Workstream,
    Review,
    Test,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationAgentProfile {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationApprovalGate {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub rework_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationNode<I, O> {
    pub node_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<I>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<O>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_kind: Option<ProjectedAutomationStageKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<ProjectedAutomationApprovalGate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_failure_mode: Option<PartialFailureMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel_agents: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_runtime_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tool_calls: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationDraft<I, O> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    #[serde(default)]
    pub agents: Vec<ProjectedAutomationAgentProfile>,
    #[serde(default)]
    pub nodes: Vec<ProjectedAutomationNode<I, O>>,
    pub execution: ProjectedAutomationExecutionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProjectedAutomationContextMaterialization>,
    pub metadata: Value,
}
