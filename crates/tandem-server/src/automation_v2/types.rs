use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_orchestrator::KnowledgeBinding;
use tandem_plan_compiler::api::{
    ContextObject, PlanScopeSnapshot, PlanValidationReport,
    ProjectedAutomationContextMaterialization, ProjectedRoutineContextPartition,
    ProjectedStepContextBindings,
};

use crate::routines::types::RoutineMisfirePolicy;

pub type AutomationV2Schedule =
    tandem_workflows::plan_package::AutomationV2Schedule<RoutineMisfirePolicy>;
pub use tandem_workflows::plan_package::AutomationV2ScheduleType;

pub type WorkflowPlanStep = tandem_workflows::plan_package::WorkflowPlanStep<
    AutomationFlowInputRef,
    AutomationFlowOutputContract,
>;
pub type WorkflowPlan =
    tandem_workflows::plan_package::WorkflowPlan<AutomationV2Schedule, WorkflowPlanStep>;
pub use tandem_workflows::plan_package::{WorkflowPlanChatMessage, WorkflowPlanConversation};
pub type WorkflowPlanDraftRecord =
    tandem_workflows::plan_package::WorkflowPlanDraftRecord<WorkflowPlan>;
pub type AutomationRuntimeContextMaterialization = ProjectedAutomationContextMaterialization;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationV2Status {
    Active,
    Paused,
    Draft,
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

impl From<tandem_plan_compiler::api::ProjectedAutomationAgentProfile> for AutomationAgentProfile {
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationAgentProfile) -> Self {
        Self {
            agent_id: value.agent_id,
            template_id: value.template_id,
            display_name: value.display_name,
            avatar_url: None,
            model_policy: value.model_policy,
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: value.tool_allowlist,
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: value.allowed_mcp_servers,
                allowed_tools: None,
            },
            approval_policy: None,
        }
    }
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

impl From<tandem_plan_compiler::api::ProjectedAutomationStageKind> for AutomationNodeStageKind {
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationStageKind) -> Self {
        match value {
            tandem_plan_compiler::api::ProjectedAutomationStageKind::Workstream => Self::Workstream,
            tandem_plan_compiler::api::ProjectedAutomationStageKind::Review => Self::Review,
            tandem_plan_compiler::api::ProjectedAutomationStageKind::Test => Self::Test,
            tandem_plan_compiler::api::ProjectedAutomationStageKind::Approval => Self::Approval,
        }
    }
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

impl From<tandem_plan_compiler::api::ProjectedAutomationApprovalGate> for AutomationApprovalGate {
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationApprovalGate) -> Self {
        Self {
            required: value.required,
            decisions: value.decisions,
            rework_targets: value.rework_targets,
            instructions: value.instructions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationFlowNode {
    pub node_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub knowledge: KnowledgeBinding,
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

impl<I, O> From<tandem_plan_compiler::api::ProjectedAutomationNode<I, O>> for AutomationFlowNode
where
    I: Into<AutomationFlowInputRef>,
    O: Into<AutomationFlowOutputContract>,
{
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationNode<I, O>) -> Self {
        fn knowledge_from_metadata(metadata: Option<&Value>, objective: &str) -> KnowledgeBinding {
            let mut binding = KnowledgeBinding::default();
            if let Some(parsed) = metadata
                .and_then(|metadata| metadata.get("builder"))
                .and_then(Value::as_object)
                .and_then(|builder| builder.get("knowledge"))
                .cloned()
                .and_then(|value| serde_json::from_value::<KnowledgeBinding>(value).ok())
            {
                binding = parsed;
            }
            if binding
                .subject
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                let subject = objective.trim();
                if !subject.is_empty() {
                    binding.subject = Some(subject.to_string());
                }
            }
            binding
        }

        let objective = value.objective;
        let knowledge = knowledge_from_metadata(value.metadata.as_ref(), &objective);

        Self {
            node_id: value.node_id,
            agent_id: value.agent_id,
            objective,
            knowledge,
            depends_on: value.depends_on,
            input_refs: value.input_refs.into_iter().map(Into::into).collect(),
            output_contract: value.output_contract.map(Into::into),
            retry_policy: value.retry_policy,
            timeout_ms: value.timeout_ms,
            stage_kind: value.stage_kind.map(Into::into),
            gate: value.gate.map(Into::into),
            metadata: value.metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationFlowInputRef {
    pub from_step_id: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationFlowOutputContract {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<AutomationOutputValidatorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforcement: Option<AutomationOutputEnforcement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_guidance: Option<String>,
}

impl From<tandem_plan_compiler::api::ProjectedMissionInputRef> for AutomationFlowInputRef {
    fn from(value: tandem_plan_compiler::api::ProjectedMissionInputRef) -> Self {
        Self {
            from_step_id: value.from_step_id,
            alias: value.alias,
        }
    }
}

impl tandem_plan_compiler::api::WorkflowInputRefLike for AutomationFlowInputRef {
    fn from_step_id(&self) -> &str {
        self.from_step_id.as_str()
    }
}

impl From<tandem_plan_compiler::api::OutputContractSeed> for AutomationFlowOutputContract {
    fn from(value: tandem_plan_compiler::api::OutputContractSeed) -> Self {
        Self {
            kind: value.kind,
            validator: value.validator_kind.map(|kind| match kind {
                tandem_plan_compiler::api::ProjectedOutputValidatorKind::ResearchBrief => {
                    AutomationOutputValidatorKind::ResearchBrief
                }
                tandem_plan_compiler::api::ProjectedOutputValidatorKind::ReviewDecision => {
                    AutomationOutputValidatorKind::ReviewDecision
                }
                tandem_plan_compiler::api::ProjectedOutputValidatorKind::StructuredJson => {
                    AutomationOutputValidatorKind::StructuredJson
                }
                tandem_plan_compiler::api::ProjectedOutputValidatorKind::CodePatch => {
                    AutomationOutputValidatorKind::CodePatch
                }
                tandem_plan_compiler::api::ProjectedOutputValidatorKind::GenericArtifact => {
                    AutomationOutputValidatorKind::GenericArtifact
                }
            }),
            enforcement: value
                .enforcement
                .and_then(|raw| serde_json::from_value(raw).ok()),
            schema: value.schema,
            summary_guidance: value.summary_guidance,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AutomationOutputEnforcement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_profile: Option<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub required_evidence: Vec<String>,
    #[serde(default)]
    pub required_sections: Vec<String>,
    #[serde(default)]
    pub prewrite_gates: Vec<String>,
    #[serde(default)]
    pub retry_on_missing: Vec<String>,
    #[serde(default)]
    pub terminal_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_text_recovery: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOutputValidatorKind {
    CodePatch,
    ResearchBrief,
    ReviewDecision,
    StructuredJson,
    GenericArtifact,
    /// Standup participant nodes. Produces a JSON object with `yesterday`, `today`, and
    /// `blockers` fields. Status detection short-circuits all review-approval and
    /// research-brief logic for this kind — participants either complete or need repair.
    StandupUpdate,
}

impl AutomationOutputValidatorKind {
    pub fn stable_key(self) -> &'static str {
        match self {
            Self::CodePatch => "code_patch",
            Self::ResearchBrief => "research_brief",
            Self::ReviewDecision => "review_decision",
            Self::StructuredJson => "structured_json",
            Self::GenericArtifact => "generic_artifact",
            Self::StandupUpdate => "standup_update",
        }
    }
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

impl From<tandem_plan_compiler::api::ProjectedAutomationExecutionPolicy>
    for AutomationExecutionPolicy
{
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationExecutionPolicy) -> Self {
        Self {
            max_parallel_agents: value.max_parallel_agents,
            max_total_runtime_ms: value.max_total_runtime_ms,
            max_total_tool_calls: value.max_total_tool_calls,
            max_total_tokens: value.max_total_tokens,
            max_total_cost_usd: value.max_total_cost_usd,
        }
    }
}

impl AutomationV2Spec {
    fn metadata_value<T>(&self, key: &str) -> Option<T>
    where
        T: DeserializeOwned,
    {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key).cloned())
            .and_then(|value| serde_json::from_value(value).ok())
    }

    pub fn runtime_context_materialization(
        &self,
    ) -> Option<AutomationRuntimeContextMaterialization> {
        self.metadata_value("context_materialization")
    }

    pub fn approved_plan_runtime_context_materialization(
        &self,
    ) -> Option<AutomationRuntimeContextMaterialization> {
        let approved_plan = self.approved_plan_materialization()?;
        let scope_snapshot = self.plan_scope_snapshot_materialization()?;
        let context_objects = scope_snapshot
            .context_objects
            .into_iter()
            .map(|context_object: ContextObject| {
                (context_object.context_object_id.clone(), context_object)
            })
            .collect::<HashMap<_, _>>();
        let routines = approved_plan
            .routines
            .into_iter()
            .map(|routine| ProjectedRoutineContextPartition {
                routine_id: routine.routine_id,
                visible_context_objects: routine
                    .visible_context_object_ids
                    .into_iter()
                    .filter_map(|context_object_id| {
                        context_objects.get(&context_object_id).cloned()
                    })
                    .collect(),
                step_context_bindings: routine
                    .step_context_bindings
                    .into_iter()
                    .map(|binding| ProjectedStepContextBindings {
                        step_id: binding.step_id,
                        context_reads: binding.context_reads,
                        context_writes: binding.context_writes,
                    })
                    .collect(),
            })
            .collect();
        Some(AutomationRuntimeContextMaterialization { routines })
    }

    pub fn requires_runtime_context(&self) -> bool {
        self.runtime_context_materialization().is_some()
            || self.approved_plan_materialization().is_some()
            || !crate::http::context_packs::shared_context_pack_ids_from_metadata(
                self.metadata.as_ref(),
            )
            .is_empty()
    }

    pub fn plan_scope_snapshot_materialization(&self) -> Option<PlanScopeSnapshot> {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("plan_package_bundle"))
            .and_then(|bundle| bundle.get("scope_snapshot"))
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    pub(crate) fn plan_package_validation_report(&self) -> Option<PlanValidationReport> {
        self.metadata_value("plan_package_validation")
    }

    pub(crate) fn approved_plan_materialization(
        &self,
    ) -> Option<tandem_plan_compiler::api::ApprovedPlanMaterialization> {
        self.metadata_value("approved_plan_materialization")
    }
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
    pub knowledge: KnowledgeBinding,
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
    pub preflight: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_preflight: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_resolution: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_evidence: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_timeline: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_quality_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency_rollback_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_used: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_validation: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<AutomationNodeOutputProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationValidatorSummary {
    pub kind: AutomationOutputValidatorKind,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub unmet_requirements: Vec<String>,
    #[serde(default)]
    pub warning_requirements: Vec<String>,
    #[serde(default)]
    pub warning_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_candidate_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_basis: Option<Value>,
    #[serde(default)]
    pub repair_attempted: bool,
    #[serde(default)]
    pub repair_attempt: u32,
    #[serde(default)]
    pub repair_attempts_remaining: u32,
    #[serde(default)]
    pub repair_succeeded: bool,
    #[serde(default)]
    pub repair_exhausted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationNodeOutputFreshness {
    pub current_run: bool,
    pub current_attempt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationNodeOutputProvenance {
    pub session_id: String,
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_candidate_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_attempt: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_succeeded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse_allowed: Option<bool>,
    pub freshness: AutomationNodeOutputFreshness,
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
    Panic,
    Shutdown,
    ServerRestart,
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
    pub runtime_context: Option<AutomationRuntimeContextMaterialization>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<crate::app::state::automation::scheduler::SchedulerMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_orchestrator::{KnowledgeReuseMode, KnowledgeTrustLevel};
    use tandem_plan_compiler::api::{
        OutputContractSeed, ProjectedAutomationNode, ProjectedMissionInputRef,
    };

    #[test]
    fn projected_node_metadata_lifts_knowledge_binding() {
        let projected = ProjectedAutomationNode::<ProjectedMissionInputRef, OutputContractSeed> {
            node_id: "node-a".to_string(),
            agent_id: "agent-a".to_string(),
            objective: "Map the topic".to_string(),
            depends_on: vec![],
            input_refs: vec![],
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "knowledge": {
                        "enabled": true,
                        "reuse_mode": "preflight",
                        "trust_floor": "promoted",
                        "read_spaces": [{"scope": "project"}],
                        "promote_spaces": [{"scope": "project"}],
                        "subject": "Topic map"
                    }
                }
            })),
        };

        let node = AutomationFlowNode::from(projected);
        assert!(node.knowledge.enabled);
        assert_eq!(node.knowledge.reuse_mode, KnowledgeReuseMode::Preflight);
        assert_eq!(node.knowledge.trust_floor, KnowledgeTrustLevel::Promoted);
        assert_eq!(node.knowledge.subject.as_deref(), Some("Topic map"));
        assert_eq!(node.knowledge.read_spaces.len(), 1);
        assert_eq!(node.knowledge.promote_spaces.len(), 1);
    }
}
