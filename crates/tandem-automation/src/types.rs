use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_orchestrator::KnowledgeBinding;
use tandem_plan_compiler::api::{
    ContextObject, PlanScopeSnapshot, PlanValidationReport,
    ProjectedAutomationContextMaterialization, ProjectedRoutineContextPartition,
    ProjectedStepContextBindings,
};
use tandem_types::TenantContext;

use crate::routine::RoutineMisfirePolicy;

pub use crate::mcp_policy::{
    AutomationAgentMcpPolicy, AutomationMcpConnectionGrant, AutomationMcpRunAs,
};

pub const AUTOMATION_TENANT_CONTEXT_METADATA_KEY: &str = "tenant_context";

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

// ---------------------------------------------------------------------------
// Connected-agent coordination types
// ---------------------------------------------------------------------------

/// A file-based handoff envelope written by an upstream automation and consumed
/// by a downstream automation. Deposited in the workspace `shared/handoffs/`
/// directory and processed by the scheduler's watch-condition loop.
///
/// Lifecycle: `inbox/` → (auto-approve) → `approved/` → (consumed) → `archived/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffArtifact {
    /// Stable unique ID for this handoff, e.g. `hoff-20260406-<uuid>`.
    pub handoff_id: String,
    /// The automation that produced this handoff.
    pub source_automation_id: String,
    /// The run that produced this handoff.
    pub source_run_id: String,
    /// The node within that run that produced this handoff.
    pub source_node_id: String,
    /// The downstream automation that should consume this handoff.
    /// The watch evaluator enforces this match.
    pub target_automation_id: String,
    /// Semantic type of the artifact, e.g. `"shortlist"`, `"brief"`, `"report"`.
    /// Used to match against watch condition `artifact_type` filters.
    pub artifact_type: String,
    /// Unix epoch milliseconds when the handoff was created.
    pub created_at_ms: u64,
    /// Relative path (from workspace root) of the real content file.
    /// For example `"job-search/shortlists/2026-04-06.md"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    /// SHA-256 hex digest of the content at `content_path`, if computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    /// Arbitrary operator-controlled metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    // --- Fields added when the handoff is consumed and the file is archived ---
    /// The run ID of the automation that consumed this handoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_by_run_id: Option<String>,
    /// The automation ID of the consumer (mirrors `target_automation_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_by_automation_id: Option<String>,
    /// Unix epoch milliseconds when the handoff was consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_at_ms: Option<u64>,
}

/// The kind of watch condition. Only `HandoffAvailable` is implemented in Phase 1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WatchCondition {
    /// Fire when at least one handoff artifact is available in the `approved/`
    /// directory that matches all specified filter fields.
    HandoffAvailable {
        /// Optional filter: only match handoffs from this source automation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_automation_id: Option<String>,
        /// Optional filter: only match handoffs with this `artifact_type` value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_type: Option<String>,
    },
    // Phase 2: FileExists, FlagSet, UpstreamCompleted
}

/// Per-automation filesystem scope restriction.
///
/// When present, all paths accessed by agents in this automation are validated
/// against this policy in addition to the existing workspace-root sandbox.
/// Paths are relative to `workspace_root`.
///
/// If absent, the automation has full workspace-root access (backward-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationScopePolicy {
    /// Paths readable by agents in this automation.
    /// An empty list means "inherit workspace root" (no extra restriction).
    #[serde(default)]
    pub readable_paths: Vec<String>,
    /// Paths writable by agents in this automation.
    /// A write-allowed path is implicitly also readable.
    #[serde(default)]
    pub writable_paths: Vec<String>,
    /// Paths explicitly denied even if they fall inside readable/writable.
    /// Deny-wins: this list is checked first.
    #[serde(default)]
    pub denied_paths: Vec<String>,
    /// Paths the scheduler watch evaluator may scan on behalf of this automation.
    /// Defaults to readable_paths. Watching does not grant write access.
    #[serde(default)]
    pub watch_paths: Vec<String>,
}

impl AutomationScopePolicy {
    /// Returns `true` if this policy is effectively unrestricted (all lists empty).
    pub fn is_open(&self) -> bool {
        self.readable_paths.is_empty()
            && self.writable_paths.is_empty()
            && self.denied_paths.is_empty()
    }

    /// Check whether `path` (relative to workspace root) is readable under this
    /// policy. Returns `Err(reason)` if the access is denied.
    ///
    /// Rules (evaluated in order):
    /// 1. If `path` is covered by `denied_paths` → deny.
    /// 2. If `writable_paths` is non-empty and `path` is covered → allow.
    /// 3. If `readable_paths` is non-empty and `path` is covered → allow.
    /// 4. If both `readable_paths` and `writable_paths` are empty → allow (open policy).
    /// 5. Otherwise → deny.
    pub fn check_read(&self, path: &str) -> Result<(), String> {
        let path = path.trim_start_matches('/');
        if self.path_is_denied(path) {
            return Err(format!(
                "scope policy: read denied for `{path}` (path is in denied_paths)"
            ));
        }
        if self.readable_paths.is_empty() && self.writable_paths.is_empty() {
            return Ok(()); // open policy
        }
        if self.path_is_readable(path) || self.path_is_writable(path) {
            return Ok(());
        }
        Err(format!(
            "scope policy: read denied for `{path}` (not in readable_paths or writable_paths)"
        ))
    }

    /// Check whether `path` is writable under this policy.
    pub fn check_write(&self, path: &str) -> Result<(), String> {
        let path = path.trim_start_matches('/');
        if self.path_is_denied(path) {
            return Err(format!(
                "scope policy: write denied for `{path}` (path is in denied_paths)"
            ));
        }
        if self.writable_paths.is_empty() {
            return Ok(()); // no write restriction
        }
        if self.path_is_writable(path) {
            return Ok(());
        }
        Err(format!(
            "scope policy: write denied for `{path}` (not in writable_paths)"
        ))
    }

    /// Check whether `path` is scannable by the watch evaluator.
    pub fn check_watch(&self, path: &str) -> Result<(), String> {
        let path = path.trim_start_matches('/');
        if self.path_is_denied(path) {
            return Err(format!(
                "scope policy: watch denied for `{path}` (path is in denied_paths)"
            ));
        }
        let watch_paths = if self.watch_paths.is_empty() {
            &self.readable_paths
        } else {
            &self.watch_paths
        };
        if watch_paths.is_empty() {
            return Ok(()); // open watch policy
        }
        if watch_paths
            .iter()
            .any(|prefix| scope_path_matches_prefix(path, prefix))
        {
            return Ok(());
        }
        Err(format!(
            "scope policy: watch denied for `{path}` (not in watch_paths / readable_paths)"
        ))
    }

    fn path_is_denied(&self, path: &str) -> bool {
        self.denied_paths
            .iter()
            .any(|prefix| scope_path_matches_prefix(path, prefix))
    }

    fn path_is_readable(&self, path: &str) -> bool {
        self.readable_paths
            .iter()
            .any(|prefix| scope_path_matches_prefix(path, prefix))
    }

    fn path_is_writable(&self, path: &str) -> bool {
        self.writable_paths
            .iter()
            .any(|prefix| scope_path_matches_prefix(path, prefix))
    }
}

/// Returns true if `path` is equal to `prefix` or starts with `prefix + "/"`.
pub(crate) fn scope_path_matches_prefix(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_matches('/');
    let path = path.trim_matches('/');
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Per-automation handoff directory configuration.
///
/// Paths are relative to `workspace_root` (or the automation's scoped workspace).
/// Defaults follow the standard layout: `shared/handoffs/{inbox,approved,archived}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationHandoffConfig {
    /// Directory where newly created handoffs are deposited.
    /// Default: `"shared/handoffs/inbox"`
    #[serde(default = "default_handoff_inbox_dir")]
    pub inbox_dir: String,
    /// Directory where approved handoffs wait for consumption.
    /// Default: `"shared/handoffs/approved"`
    #[serde(default = "default_handoff_approved_dir")]
    pub approved_dir: String,
    /// Directory where consumed handoffs are archived.
    /// Default: `"shared/handoffs/archived"`
    #[serde(default = "default_handoff_archived_dir")]
    pub archived_dir: String,
    /// When `true`, newly created handoffs bypass the approval step and are
    /// moved directly from `inbox/` to `approved/`. Default: `true` (Phase 1).
    #[serde(default = "default_auto_approve")]
    pub auto_approve: bool,
}

fn default_handoff_inbox_dir() -> String {
    "shared/handoffs/inbox".to_string()
}
fn default_handoff_approved_dir() -> String {
    "shared/handoffs/approved".to_string()
}
fn default_handoff_archived_dir() -> String {
    "shared/handoffs/archived".to_string()
}
fn default_auto_approve() -> bool {
    true
}

impl Default for AutomationHandoffConfig {
    fn default() -> Self {
        Self {
            inbox_dir: default_handoff_inbox_dir(),
            approved_dir: default_handoff_approved_dir(),
            archived_dir: default_handoff_archived_dir(),
            auto_approve: default_auto_approve(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAgentToolPolicy {
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub denylist: Vec<String>,
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
                allowed_connections: Vec::new(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_policy: Option<AutomationGateExpiryPolicy>,
}

impl From<tandem_plan_compiler::api::ProjectedAutomationApprovalGate> for AutomationApprovalGate {
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationApprovalGate) -> Self {
        Self {
            required: value.required,
            decisions: value.decisions,
            rework_targets: value.rework_targets,
            instructions: value.instructions,
            expiry_policy: value.expiry_policy.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationGateExpiryAction {
    Cancel,
    Escalate,
    Remind,
}

impl From<tandem_plan_compiler::api::ProjectedAutomationGateExpiryAction>
    for AutomationGateExpiryAction
{
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationGateExpiryAction) -> Self {
        match value {
            tandem_plan_compiler::api::ProjectedAutomationGateExpiryAction::Cancel => Self::Cancel,
            tandem_plan_compiler::api::ProjectedAutomationGateExpiryAction::Escalate => {
                Self::Escalate
            }
            tandem_plan_compiler::api::ProjectedAutomationGateExpiryAction::Remind => Self::Remind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationGateExpiryPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_expiry: Option<AutomationGateExpiryAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_every_ms: Option<u64>,
}

impl From<tandem_plan_compiler::api::ProjectedAutomationGateExpiryPolicy>
    for AutomationGateExpiryPolicy
{
    fn from(value: tandem_plan_compiler::api::ProjectedAutomationGateExpiryPolicy) -> Self {
        Self {
            expires_after_ms: value.expires_after_ms,
            on_expiry: value.on_expiry.map(Into::into),
            escalate_to: value.escalate_to,
            remind_every_ms: value.remind_every_ms,
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
    pub tool_policy: Option<AutomationAgentToolPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_policy: Option<AutomationAgentMcpPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
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
        fn metadata_with_partial_failure_mode(
            metadata: Option<Value>,
            partial_failure_mode: Option<&tandem_plan_compiler::api::PartialFailureMode>,
        ) -> Option<Value> {
            let Some(mode) = partial_failure_mode else {
                return metadata;
            };
            let mode = match mode {
                tandem_plan_compiler::api::PartialFailureMode::ContinueIndependent => {
                    "continue_independent"
                }
                tandem_plan_compiler::api::PartialFailureMode::PauseDownstreamOnly => {
                    "pause_downstream_only"
                }
                tandem_plan_compiler::api::PartialFailureMode::PauseAll => "pause_all",
            };
            let mut metadata = metadata.unwrap_or_else(|| json!({}));
            if !metadata.is_object() {
                metadata = json!({ "projected_metadata": metadata });
            }
            if let Some(object) = metadata.as_object_mut() {
                object
                    .entry("partial_failure_mode".to_string())
                    .or_insert_with(|| Value::String(mode.to_string()));
            }
            Some(metadata)
        }

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
        let metadata =
            metadata_with_partial_failure_mode(value.metadata, value.partial_failure_mode.as_ref());

        Self {
            node_id: value.node_id,
            agent_id: value.agent_id,
            objective,
            knowledge,
            depends_on: value.depends_on,
            input_refs: value.input_refs.into_iter().map(Into::into).collect(),
            output_contract: value.output_contract.map(Into::into),
            tool_policy: None,
            mcp_policy: None,
            retry_policy: value.retry_policy,
            timeout_ms: value.timeout_ms,
            max_tool_calls: None,
            stage_kind: value.stage_kind.map(Into::into),
            gate: value.gate.map(Into::into),
            metadata,
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
pub struct AutomationRequiredToolCall {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_key: Option<String>,
    #[serde(default = "default_required_tool_call_success")]
    pub required_success: bool,
}

fn default_required_tool_call_success() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AutomationOutputEnforcement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_profile: Option<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
    #[serde(default)]
    pub required_tool_calls: Vec<AutomationRequiredToolCall>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutomationExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<crate::execution_profile::ExecutionProfile>,
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
            profile: None,
            max_parallel_agents: value.max_parallel_agents,
            max_total_runtime_ms: value.max_total_runtime_ms,
            max_total_tool_calls: value.max_total_tool_calls,
            max_total_tokens: value.max_total_tokens,
            max_total_cost_usd: value.max_total_cost_usd,
        }
    }
}

/// Effective profile precedence: explicit run override → workflow policy →
/// tenant default (TANDEM_DEFAULT_EXECUTION_PROFILE env var) → system default (Guided).
pub fn resolve_effective_execution_profile(
    automation: &AutomationV2Spec,
    requested: Option<crate::execution_profile::ExecutionProfile>,
) -> crate::execution_profile::ExecutionProfile {
    resolve_effective_execution_profile_with_tenant(
        automation,
        requested,
        crate::execution_profile::tenant_default_execution_profile_from_env(),
    )
}

/// Pure resolver variant for tests and call sites that want to supply
/// the tenant default explicitly. Real run creation should call
/// [`resolve_effective_execution_profile`] which reads
/// `TANDEM_DEFAULT_EXECUTION_PROFILE` from the environment.
pub fn resolve_effective_execution_profile_with_tenant(
    automation: &AutomationV2Spec,
    requested: Option<crate::execution_profile::ExecutionProfile>,
    tenant_default: Option<crate::execution_profile::ExecutionProfile>,
) -> crate::execution_profile::ExecutionProfile {
    requested
        .or(automation.execution.profile)
        .or(tenant_default)
        .unwrap_or(crate::execution_profile::ExecutionProfile::Guided)
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
            || !crate::context_metadata::shared_context_pack_ids_from_metadata(
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

    pub fn plan_package_validation_report(&self) -> Option<PlanValidationReport> {
        self.metadata_value("plan_package_validation")
    }

    pub fn approved_plan_materialization(
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
    /// Optional per-automation filesystem scope restrictions.
    /// When absent, the automation has full workspace-root access (backward-compatible).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_policy: Option<AutomationScopePolicy>,
    /// Watch conditions evaluated by the scheduler on each tick.
    /// When any condition matches, a new run is created with `trigger_type: "watch_condition"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_conditions: Vec<WatchCondition>,
    /// Handoff directory configuration. Uses defaults if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_config: Option<AutomationHandoffConfig>,
}

impl AutomationV2Spec {
    pub fn tenant_context(&self) -> TenantContext {
        self.metadata
            .as_ref()
            .and_then(|metadata| metadata.get(AUTOMATION_TENANT_CONTEXT_METADATA_KEY))
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_else(TenantContext::local_implicit)
    }

    pub fn set_tenant_context(&mut self, tenant_context: &TenantContext) {
        let tenant_value = serde_json::to_value(tenant_context).unwrap_or(Value::Null);
        match self.metadata.as_mut() {
            Some(Value::Object(map)) => {
                map.insert(
                    AUTOMATION_TENANT_CONTEXT_METADATA_KEY.to_string(),
                    tenant_value,
                );
            }
            Some(_) | None => {
                let mut map = serde_json::Map::new();
                map.insert(
                    AUTOMATION_TENANT_CONTEXT_METADATA_KEY.to_string(),
                    tenant_value,
                );
                self.metadata = Some(Value::Object(map));
            }
        }
    }

    /// Returns the effective handoff config, using defaults if none is set.
    pub fn effective_handoff_config(&self) -> AutomationHandoffConfig {
        self.handoff_config.clone().unwrap_or_default()
    }

    /// Returns true if this automation has any watch conditions configured.
    pub fn has_watch_conditions(&self) -> bool {
        !self.watch_conditions.is_empty()
    }
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
    pub attempt_verdict: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_context: Option<Value>,
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
pub struct AutomationAttemptReview {
    pub tone: String,
    pub progress_label: String,
    pub progress_score: u8,
    #[serde(default)]
    pub completed_correctly: Vec<String>,
    #[serde(default)]
    pub still_needed: Vec<String>,
    #[serde(default)]
    pub why_it_matters: Vec<String>,
    #[serde(default)]
    pub next_moves: Vec<String>,
}

impl Default for AutomationAttemptReview {
    fn default() -> Self {
        Self {
            tone: "calm_teammate_v1".to_string(),
            progress_label: "none".to_string(),
            progress_score: 0,
            completed_correctly: Vec::new(),
            still_needed: Vec::new(),
            why_it_matters: Vec::new(),
            next_moves: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAttemptVerdict {
    pub version: u32,
    pub node_id: String,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    pub consumes_model_attempt_budget: bool,
    pub consumes_repair_budget: bool,
    pub expected: Value,
    pub observed: Value,
    #[serde(default)]
    pub unmet_requirements: Vec<String>,
    #[serde(default)]
    pub required_next_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_reason: Option<String>,
    #[serde(default)]
    pub attempt_review: AutomationAttemptReview,
    pub created_at_ms: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_policy: Option<AutomationGateExpiryPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationGateDecisionRecord {
    pub node_id: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub decided_at_ms: u64,
    /// Verified actor that recorded this decision. `None` only for legacy records
    /// persisted before decider attribution was enforced (GOV-B1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<crate::governance::GovernanceActorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
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
    StaleReaped,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowLearningCandidateKind {
    MemoryFact,
    RepairHint,
    PromptPatch,
    GraphPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowLearningCandidateStatus {
    Proposed,
    Approved,
    Rejected,
    Applied,
    Superseded,
    Regressed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowLearningMetricsSnapshot {
    #[serde(default)]
    pub sample_size: usize,
    #[serde(default)]
    pub completion_rate: f64,
    #[serde(default)]
    pub validation_pass_rate: f64,
    #[serde(default)]
    pub mean_attempts_per_node: f64,
    #[serde(default)]
    pub repairable_failure_rate: f64,
    #[serde(default)]
    pub median_wall_clock_ms: u64,
    #[serde(default)]
    pub human_intervention_count: u64,
    #[serde(default)]
    pub computed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLearningCandidate {
    pub candidate_id: String,
    pub workflow_id: String,
    pub project_id: String,
    pub source_run_id: String,
    pub kind: WorkflowLearningCandidateKind,
    pub status: WorkflowLearningCandidateStatus,
    #[serde(default)]
    pub confidence: f64,
    pub summary: String,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_family: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<Value>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_memory_payload: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_revision_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_memory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_memory_id: Option<String>,
    #[serde(default)]
    pub needs_plan_bundle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_before: Option<WorkflowLearningMetricsSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_observed_metrics: Option<WorkflowLearningMetricsSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision_session_id: Option<String>,
    #[serde(default)]
    pub run_ids: Vec<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowLearningRunSummary {
    #[serde(default)]
    pub generated_candidate_ids: Vec<String>,
    #[serde(default)]
    pub injected_learning_ids: Vec<String>,
    #[serde(default)]
    pub approved_learning_ids_considered: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_run_metrics: Option<WorkflowLearningMetricsSnapshot>,
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
    pub node_attempt_verdicts: std::collections::HashMap<String, Vec<Value>>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationRunExecutionClaim {
    pub claim_id: String,
    pub claimant_id: String,
    pub claimed_at_ms: u64,
    pub lease_expires_at_ms: u64,
    pub lease_epoch: u64,
}

impl AutomationRunExecutionClaim {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.lease_expires_at_ms <= now_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationV2RunRecord {
    pub run_id: String,
    pub automation_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
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
    pub workflow_definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_definition_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_claim: Option<AutomationRunExecutionClaim>,
    #[serde(default)]
    pub execution_claim_epoch: u64,
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
    pub scheduler: Option<crate::scheduler::SchedulerMetadata>,
    /// Human-readable description of why this run was triggered, e.g.
    /// `"handoff shortlist from opportunity-scout approved"`.
    /// Populated for `trigger_type: "watch_condition"` runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
    /// The `handoff_id` of the `HandoffArtifact` that triggered this run, if any.
    /// Used for idempotency: a retry of this run will not re-consume a second handoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_handoff_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learning_summary: Option<WorkflowLearningRunSummary>,
    /// Effective execution profile for this run, resolved at run start
    /// from optional run-level override, automation policy, or Strict.
    /// See `automation_v2::types::resolve_effective_execution_profile`.
    #[serde(default)]
    pub effective_execution_profile: crate::execution_profile::ExecutionProfile,
    /// What the caller explicitly requested at run-now time, if any.
    /// `None` means the run inherited from the automation/system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_execution_profile: Option<crate::execution_profile::ExecutionProfile>,
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}
