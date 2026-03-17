use crate::routines::types::RoutineMisfirePolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationV2Status {
    Active,
    Paused,
    Draft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationV2ScheduleType {
    Cron,
    Interval,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationV2Schedule {
    #[serde(rename = "type")]
    pub schedule_type: AutomationV2ScheduleType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<u64>,
    pub timezone: String,
    pub misfire_policy: RoutineMisfirePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentToolPolicy {
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub denylist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentMcpPolicy {
    #[serde(default)]
    pub allowed_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentProfile {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub skills: Vec<String>,
    pub tool_policy: AutomationAgentToolPolicy,
    pub mcp_policy: AutomationAgentMcpPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationNodeStageKind {
    Orchestrator,
    Workstream,
    Review,
    Test,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationApprovalGate {
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
pub struct AutomationFlowNode {
    pub node_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<AutomationFlowInputRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<AutomationFlowOutputContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_kind: Option<AutomationNodeStageKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<AutomationApprovalGate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowInputRef {
    pub from_step_id: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowOutputContract {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<AutomationOutputValidatorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_guidance: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOutputValidatorKind {
    CodePatch,
    ResearchBrief,
    ReviewDecision,
    StructuredJson,
    GenericArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowSpec {
    #[serde(default)]
    pub nodes: Vec<AutomationFlowNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationExecutionPolicy {
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
pub struct AutomationV2Spec {
    pub automation_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: AutomationV2Status,
    pub schedule: AutomationV2Schedule,
    #[serde(default)]
    pub agents: Vec<AutomationAgentProfile>,
    pub flow: AutomationFlowSpec,
    pub execution: AutomationExecutionPolicy,
    #[serde(default)]
    pub output_targets: Vec<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub creator_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanStep {
    pub step_id: String,
    pub kind: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub agent_role: String,
    #[serde(default)]
    pub input_refs: Vec<AutomationFlowInputRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<AutomationFlowOutputContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub plan_id: String,
    pub planner_version: String,
    pub plan_source: String,
    pub original_prompt: String,
    pub normalized_prompt: String,
    pub confidence: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub schedule: AutomationV2Schedule,
    pub execution_target: String,
    pub workspace_root: String,
    #[serde(default)]
    pub steps: Vec<WorkflowPlanStep>,
    #[serde(default)]
    pub requires_integrations: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_preferences: Option<Value>,
    pub save_options: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanChatMessage {
    pub role: String,
    pub text: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanConversation {
    pub conversation_id: String,
    pub plan_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub messages: Vec<WorkflowPlanChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlanDraftRecord {
    pub initial_plan: WorkflowPlan,
    pub current_plan: WorkflowPlan,
    pub conversation: WorkflowPlanConversation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_diagnostics: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationNodeOutput {
    pub contract_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_kind: Option<AutomationOutputValidatorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_summary: Option<AutomationValidatorSummary>,
    pub summary: String,
    pub content: Value,
    pub created_at_ms: u64,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_telemetry: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_validation: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationValidatorSummary {
    pub kind: AutomationOutputValidatorKind,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub unmet_requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_candidate_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_outcome: Option<String>,
    #[serde(default)]
    pub repair_attempted: bool,
    #[serde(default)]
    pub repair_succeeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Queued,
    Running,
    Pausing,
    Paused,
    AwaitingApproval,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPendingGate {
    pub node_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub rework_targets: Vec<String>,
    pub requested_at_ms: u64,
    #[serde(default)]
    pub upstream_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationGateDecisionRecord {
    pub node_id: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub decided_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStopKind {
    Cancelled,
    OperatorStopped,
    GuardrailStopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLifecycleRecord {
    pub event: String,
    pub recorded_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_kind: Option<AutomationStopKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFailureRecord {
    pub node_id: String,
    pub reason: String,
    pub failed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRunCheckpoint {
    #[serde(default)]
    pub completed_nodes: Vec<String>,
    #[serde(default)]
    pub pending_nodes: Vec<String>,
    #[serde(default)]
    pub node_outputs: std::collections::HashMap<String, Value>,
    #[serde(default)]
    pub node_attempts: std::collections::HashMap<String, u32>,
    #[serde(default)]
    pub blocked_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_gate: Option<AutomationPendingGate>,
    #[serde(default)]
    pub gate_history: Vec<AutomationGateDecisionRecord>,
    #[serde(default)]
    pub lifecycle_history: Vec<AutomationLifecycleRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<AutomationFailureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationV2RunRecord {
    pub run_id: String,
    pub automation_id: String,
    pub trigger_type: String,
    pub status: AutomationRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub active_session_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_session_id: Option<String>,
    #[serde(default)]
    pub active_instance_ids: Vec<String>,
    pub checkpoint: AutomationRunCheckpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_snapshot: Option<AutomationV2Spec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_kind: Option<AutomationStopKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
}
