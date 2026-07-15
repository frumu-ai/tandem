use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::TenantContext;

fn default_schema_version() -> u32 {
    1
}

fn default_max_hops() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationStatus {
    Draft,
    Published,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationSpec {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub orchestration_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: OrchestrationStatus,
    pub version: u64,
    pub root_node_id: String,
    #[serde(default)]
    pub nodes: Vec<OrchestrationNodeSpec>,
    #[serde(default)]
    pub edges: Vec<OrchestrationEdgeSpec>,
    #[serde(default)]
    pub goal_policy: GoalPolicy,
    pub tenant_context: TenantContext,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationNodeSpec {
    pub node_id: String,
    pub name: String,
    #[serde(default)]
    pub position: OrchestrationCanvasPosition,
    #[serde(flatten)]
    pub node: OrchestrationNodeKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationCanvasPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrchestrationNodeKind {
    Workflow {
        automation_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned_definition_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_transition_keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        accepts_artifact_types: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        emits_artifact_types: Vec<String>,
    },
    Wait {
        wait: AutomationWaitSpec,
    },
    Terminal {
        outcome: GoalTerminalOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_artifact_type: Option<String>,
    },
}

/// Explicit outcomes that can close or suspend a long-running goal.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalTerminalOutcome {
    Complete,
    Pause,
    Fail,
}

impl OrchestrationNodeKind {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationEdgeSpec {
    pub edge_id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub transition_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_contract: Option<OrchestrationArtifactContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<TransitionApprovalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationArtifactContract {
    pub artifact_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionApprovalPolicy {
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver_scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationWaitSpec {
    Timer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delay_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wake_at: Option<OrchestrationValueBinding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<WaitTimeoutPolicy>,
    },
    Approval {
        decisions: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_after_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<WaitTimeoutPolicy>,
    },
    Webhook {
        trigger_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_event_kind: Option<String>,
        correlation: WebhookCorrelationBinding,
        timeout: WaitTimeoutPolicy,
    },
    ExternalCondition {
        condition_key: OrchestrationValueBinding,
        timeout: WaitTimeoutPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload_schema: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum OrchestrationValueBinding {
    Literal {
        value: Value,
    },
    NodeOutput {
        node_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        json_pointer: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebhookCorrelationBinding {
    pub field: WebhookCorrelationField,
    pub value: OrchestrationValueBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookCorrelationField {
    ProviderEventId,
    IdempotencyKey,
    BodyDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaitTimeoutPolicy {
    pub expires_after_ms: u64,
    pub on_timeout: WaitTimeoutAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_every_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitTimeoutAction {
    Cancel,
    Escalate,
    Remind,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationWaitValidationIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// Validate the public wait-node contract on an Automation V2 definition.
/// Existing approval gates remain valid and are projected at runtime through
/// `AutomationFlowNode::effective_wait`; only explicit `wait` fields are
/// subject to the non-agent node restrictions here.
pub fn validate_automation_wait_nodes(
    automation: &crate::types::AutomationV2Spec,
) -> Vec<AutomationWaitValidationIssue> {
    let node_ids = automation
        .flow
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<HashSet<_>>();
    let mut issues = Vec::new();
    for node in &automation.flow.nodes {
        let Some(wait) = node.wait.as_ref() else {
            continue;
        };
        let mut push = |code: &str, message: &str| {
            issues.push(AutomationWaitValidationIssue {
                code: code.to_string(),
                message: message.to_string(),
                node_id: Some(node.node_id.clone()),
            });
        };
        if node.gate.is_some() {
            push(
                "wait_gate_conflict",
                "a node cannot define both wait and legacy gate",
            );
        }
        if node.tool_policy.is_some()
            || node.mcp_policy.is_some()
            || node.retry_policy.is_some()
            || node.timeout_ms.is_some()
            || node.max_tool_calls.is_some()
        {
            push(
                "wait_node_has_execution_policy",
                "wait nodes cannot define model, retry, timeout, tool, or MCP execution policy",
            );
        }
        for (code, message) in wait_spec_issues(wait) {
            push(code, message);
        }
        for binding in wait_bindings(wait) {
            let OrchestrationValueBinding::NodeOutput {
                node_id: source_node_id,
                json_pointer,
            } = binding
            else {
                continue;
            };
            if !node_ids.contains(source_node_id.as_str()) {
                push(
                    "wait_binding_unknown_node",
                    "wait binding references an unknown Automation V2 node",
                );
            } else if !node.depends_on.iter().any(|id| id == source_node_id) {
                push(
                    "wait_binding_not_dependency",
                    "wait binding must reference a declared upstream dependency",
                );
            }
            if json_pointer
                .as_deref()
                .is_some_and(|pointer| !pointer.is_empty() && !pointer.starts_with('/'))
            {
                push(
                    "wait_binding_invalid_json_pointer",
                    "wait binding json_pointer must be empty or start with '/'",
                );
            }
        }
    }
    issues
}

pub fn validate_automation_wait_spec(
    wait: &AutomationWaitSpec,
) -> Vec<AutomationWaitValidationIssue> {
    wait_spec_issues(wait)
        .into_iter()
        .map(|(code, message)| AutomationWaitValidationIssue {
            code: code.to_string(),
            message: message.to_string(),
            node_id: None,
        })
        .collect()
}

fn wait_bindings(wait: &AutomationWaitSpec) -> Vec<&OrchestrationValueBinding> {
    match wait {
        AutomationWaitSpec::Timer { wake_at, .. } => wake_at.iter().collect(),
        AutomationWaitSpec::Approval { .. } => Vec::new(),
        AutomationWaitSpec::Webhook { correlation, .. } => vec![&correlation.value],
        AutomationWaitSpec::ExternalCondition { condition_key, .. } => vec![condition_key],
    }
}

fn wait_spec_issues(wait: &AutomationWaitSpec) -> Vec<(&'static str, &'static str)> {
    let mut issues = Vec::new();
    match wait {
        AutomationWaitSpec::Timer {
            delay_ms,
            wake_at,
            timeout,
        } => {
            let sources = usize::from(delay_ms.is_some()) + usize::from(wake_at.is_some());
            if sources != 1 {
                issues.push((
                    "timer_wake_conflict",
                    "timer waits require exactly one of delay_ms or wake_at",
                ));
            }
            if *delay_ms == Some(0) {
                issues.push((
                    "timer_delay_invalid",
                    "timer delay_ms must be greater than zero",
                ));
            }
            if let Some(OrchestrationValueBinding::Literal { value }) = wake_at {
                if value.as_u64().is_none_or(|value| value == 0) {
                    issues.push((
                        "timer_wake_at_invalid",
                        "literal timer wake_at must be a positive millisecond timestamp",
                    ));
                }
            }
            if timeout.as_ref().is_some_and(invalid_timeout) {
                issues.push(("wait_timeout_invalid", "wait timeout policy is invalid"));
            }
        }
        AutomationWaitSpec::Approval {
            decisions,
            expires_after_ms,
            timeout,
        } => {
            let normalized = decisions
                .iter()
                .map(|decision| decision.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            let unique = normalized.iter().collect::<HashSet<_>>();
            if normalized.is_empty()
                || normalized.iter().any(String::is_empty)
                || unique.len() != normalized.len()
            {
                issues.push((
                    "approval_decisions_invalid",
                    "approval waits require unique, non-empty decisions",
                ));
            }
            if *expires_after_ms == Some(0) {
                issues.push((
                    "approval_expiry_invalid",
                    "approval expires_after_ms must be greater than zero",
                ));
            }
            if expires_after_ms.is_some() && timeout.is_some() {
                issues.push((
                    "approval_timeout_conflict",
                    "approval waits cannot define both expires_after_ms and timeout",
                ));
            }
            if timeout.as_ref().is_some_and(invalid_timeout) {
                issues.push(("wait_timeout_invalid", "wait timeout policy is invalid"));
            }
            if timeout
                .as_ref()
                .is_some_and(|policy| policy.on_timeout == WaitTimeoutAction::Resume)
            {
                issues.push((
                    "approval_timeout_resume_forbidden",
                    "approval waits must fail closed on timeout and cannot resume execution",
                ));
            }
        }
        AutomationWaitSpec::Webhook {
            trigger_id,
            correlation,
            timeout,
            ..
        } => {
            if trigger_id.trim().is_empty() {
                issues.push((
                    "webhook_trigger_invalid",
                    "webhook waits require a non-empty trigger_id",
                ));
            }
            if invalid_binding(&correlation.value) {
                issues.push((
                    "webhook_correlation_invalid",
                    "webhook waits require a typed correlation constraint",
                ));
            }
            if invalid_timeout(timeout) {
                issues.push(("wait_timeout_invalid", "wait timeout policy is invalid"));
            }
        }
        AutomationWaitSpec::ExternalCondition {
            condition_key,
            timeout,
            ..
        } => {
            if invalid_binding(condition_key) {
                issues.push((
                    "external_condition_invalid",
                    "external-condition waits require a typed condition key",
                ));
            }
            if invalid_timeout(timeout) {
                issues.push(("wait_timeout_invalid", "wait timeout policy is invalid"));
            }
        }
    }
    issues
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalPolicy {
    #[serde(default = "default_max_hops")]
    pub max_hops: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_cost_usd: Option<f64>,
    #[serde(default)]
    pub on_limit: GoalLimitAction,
}

impl Default for GoalPolicy {
    fn default() -> Self {
        Self {
            max_hops: default_max_hops(),
            deadline_at_ms: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
            on_limit: GoalLimitAction::PauseForReview,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalLimitAction {
    #[default]
    PauseForReview,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LongRunningGoalStatus {
    Queued,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

impl LongRunningGoalStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Expired
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LongRunningGoal {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub goal_id: String,
    pub orchestration_id: String,
    pub orchestration_version: u64,
    pub objective: String,
    pub status: LongRunningGoalStatus,
    pub tenant_context: TenantContext,
    pub policy: GoalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(default)]
    pub hop_count: u32,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub total_cost_usd: f64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_artifact: Option<OrchestrationArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalPolicyLimit {
    HopLimit,
    Deadline,
    TokenBudget,
    CostBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalTransitionAdmission {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<GoalPolicyLimit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_status: Option<LongRunningGoalStatus>,
}

impl LongRunningGoal {
    /// Decide whether another cross-workflow hop may start without mutating the goal.
    pub fn admit_transition(
        &self,
        now_ms: u64,
        additional_tokens: u64,
        additional_cost_usd: f64,
    ) -> GoalTransitionAdmission {
        if self.status.is_terminal() || self.status == LongRunningGoalStatus::Paused {
            return GoalTransitionAdmission {
                allowed: false,
                limit: None,
                resulting_status: Some(self.status.clone()),
            };
        }

        let limit =
            if self
                .policy
                .deadline_at_ms
                .is_some_and(|deadline| now_ms >= deadline)
            {
                Some(GoalPolicyLimit::Deadline)
            } else if self.hop_count >= self.policy.max_hops {
                Some(GoalPolicyLimit::HopLimit)
            } else if self.policy.max_total_tokens.is_some_and(|maximum| {
                self.total_tokens.saturating_add(additional_tokens) > maximum
            }) {
                Some(GoalPolicyLimit::TokenBudget)
            } else if self
                .policy
                .max_total_cost_usd
                .is_some_and(|maximum| self.total_cost_usd + additional_cost_usd > maximum)
            {
                Some(GoalPolicyLimit::CostBudget)
            } else {
                None
            };

        let resulting_status = limit.as_ref().map(|limit| {
            if matches!(limit, GoalPolicyLimit::Deadline) {
                LongRunningGoalStatus::Expired
            } else {
                match self.policy.on_limit {
                    GoalLimitAction::PauseForReview => LongRunningGoalStatus::Paused,
                    GoalLimitAction::Fail => LongRunningGoalStatus::Failed,
                }
            }
        });
        GoalTransitionAdmission {
            allowed: limit.is_none(),
            limit,
            resulting_status,
        }
    }

    /// Move a goal to an explicit terminal or operator-paused state.
    pub fn apply_terminal_outcome(&mut self, outcome: GoalTerminalOutcome, now_ms: u64) {
        self.status = match outcome {
            GoalTerminalOutcome::Complete => LongRunningGoalStatus::Completed,
            GoalTerminalOutcome::Pause => LongRunningGoalStatus::Paused,
            GoalTerminalOutcome::Fail => LongRunningGoalStatus::Failed,
        };
        self.updated_at_ms = now_ms;
        if self.status.is_terminal() {
            self.finished_at_ms = Some(now_ms);
            self.active_run_id = None;
        }
    }

    /// Cancellation is idempotent and clears the active-run pointer so callers can
    /// propagate cancellation to that run exactly once using their durable outbox.
    pub fn cancel(&mut self, now_ms: u64) -> Option<String> {
        if self.status.is_terminal() {
            return None;
        }
        self.status = LongRunningGoalStatus::Cancelled;
        self.updated_at_ms = now_ms;
        self.finished_at_ms = Some(now_ms);
        self.active_run_id.take()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalRunLink {
    pub goal_id: String,
    pub run_id: String,
    pub orchestration_node_id: String,
    pub orchestration_version: u64,
    pub hop_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggering_handoff_id: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowHandoffStatus {
    PendingApproval,
    Approved,
    Rejected,
    Claimed,
    Consumed,
    DeadLettered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowHandoff {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub handoff_id: String,
    pub idempotency_key: String,
    pub goal_id: String,
    pub orchestration_id: String,
    pub orchestration_version: u64,
    pub tenant_context: TenantContext,
    pub edge_id: String,
    pub transition_key: String,
    pub source_automation_id: String,
    pub source_run_id: String,
    pub source_node_id: String,
    pub target_automation_id: String,
    pub target_node_id: String,
    pub artifact: OrchestrationArtifactRef,
    pub status: WorkflowHandoffStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_by_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestrationArtifactRef {
    pub artifact_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaitResolution {
    pub wait_id: String,
    pub idempotency_key: String,
    pub resolved_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrchestrationValidationIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrchestrationValidationReport {
    pub valid: bool,
    #[serde(default)]
    pub issues: Vec<OrchestrationValidationIssue>,
}

pub fn validate_orchestration_spec(spec: &OrchestrationSpec) -> OrchestrationValidationReport {
    let mut issues = Vec::new();
    if spec.schema_version != 1 {
        push_issue(
            &mut issues,
            "unsupported_schema_version",
            "Only orchestration schema version 1 is supported",
            None,
            None,
        );
    }
    if spec.version == 0 {
        push_issue(
            &mut issues,
            "invalid_version",
            "Orchestration versions start at 1",
            None,
            None,
        );
    }
    if matches!(spec.status, OrchestrationStatus::Published) && spec.published_at_ms.is_none() {
        push_issue(
            &mut issues,
            "missing_published_at",
            "Published orchestration versions require published_at_ms",
            None,
            None,
        );
    }
    let mut nodes = HashMap::new();
    for node in &spec.nodes {
        if node.node_id.trim().is_empty() {
            push_issue(
                &mut issues,
                "empty_node_id",
                "Node IDs cannot be empty",
                None,
                None,
            );
            continue;
        }
        if nodes.insert(node.node_id.as_str(), node).is_some() {
            push_issue(
                &mut issues,
                "duplicate_node_id",
                "Node IDs must be unique",
                Some(&node.node_id),
                None,
            );
        }
        if let OrchestrationNodeKind::Workflow {
            automation_id,
            pinned_definition_hash,
            allowed_transition_keys,
            ..
        } = &node.node
        {
            if automation_id.trim().is_empty() {
                push_issue(
                    &mut issues,
                    "missing_automation_id",
                    "Workflow nodes must reference an automation",
                    Some(&node.node_id),
                    None,
                );
            }
            if matches!(spec.status, OrchestrationStatus::Published)
                && pinned_definition_hash
                    .as_deref()
                    .is_none_or(|hash| hash.trim().is_empty())
            {
                push_issue(
                    &mut issues,
                    "unpinned_workflow",
                    "Published orchestration workflow nodes require a definition hash",
                    Some(&node.node_id),
                    None,
                );
            }
            let mut unique_keys = HashSet::new();
            if allowed_transition_keys
                .iter()
                .any(|key| key.trim().is_empty() || !unique_keys.insert(key.as_str()))
            {
                push_issue(
                    &mut issues,
                    "invalid_allowed_transition_key",
                    "Workflow transition keys must be non-empty and unique",
                    Some(&node.node_id),
                    None,
                );
            }
        }
        validate_wait_node(node, &mut issues);
    }

    if !nodes.contains_key(spec.root_node_id.as_str()) {
        push_issue(
            &mut issues,
            "missing_root",
            "The root node must reference an existing node",
            Some(&spec.root_node_id),
            None,
        );
    }
    if spec.goal_policy.max_hops == 0 {
        push_issue(
            &mut issues,
            "invalid_max_hops",
            "max_hops must be greater than zero",
            None,
            None,
        );
    }

    let mut outgoing: HashMap<&str, Vec<&OrchestrationEdgeSpec>> = HashMap::new();
    let mut incoming: HashMap<&str, Vec<&OrchestrationEdgeSpec>> = HashMap::new();
    let mut edge_ids = HashSet::new();
    let mut transition_keys = HashSet::new();
    for edge in &spec.edges {
        if !edge_ids.insert(edge.edge_id.as_str()) {
            push_issue(
                &mut issues,
                "duplicate_edge_id",
                "Edge IDs must be unique",
                None,
                Some(&edge.edge_id),
            );
        }
        let source = nodes.get(edge.from_node_id.as_str()).copied();
        let target = nodes.get(edge.to_node_id.as_str()).copied();
        if source.is_none() || target.is_none() {
            push_issue(
                &mut issues,
                "unknown_edge_node",
                "Edges must reference existing source and target nodes",
                None,
                Some(&edge.edge_id),
            );
            continue;
        }
        if edge.transition_key.trim().is_empty() {
            push_issue(
                &mut issues,
                "empty_transition_key",
                "Transition keys cannot be empty",
                None,
                Some(&edge.edge_id),
            );
        }
        if !transition_keys.insert((edge.from_node_id.as_str(), edge.transition_key.as_str())) {
            push_issue(
                &mut issues,
                "duplicate_transition_key",
                "Transition keys must be unique for each source node",
                Some(&edge.from_node_id),
                Some(&edge.edge_id),
            );
        }
        if source.is_some_and(|node| node.node.is_terminal()) {
            push_issue(
                &mut issues,
                "terminal_has_outgoing_edge",
                "Terminal nodes cannot have outgoing transitions",
                Some(&edge.from_node_id),
                Some(&edge.edge_id),
            );
        }
        if let Some(OrchestrationNodeSpec {
            node:
                OrchestrationNodeKind::Workflow {
                    allowed_transition_keys,
                    ..
                },
            ..
        }) = source
        {
            if !allowed_transition_keys
                .iter()
                .any(|key| key == &edge.transition_key)
            {
                push_issue(
                    &mut issues,
                    "unknown_transition_key",
                    "The edge transition key is not declared by its workflow node",
                    Some(&edge.from_node_id),
                    Some(&edge.edge_id),
                );
            }
        }
        validate_artifact_compatibility(source.unwrap(), target.unwrap(), edge, &mut issues);
        outgoing.entry(&edge.from_node_id).or_default().push(edge);
        incoming.entry(&edge.to_node_id).or_default().push(edge);
    }

    for node in spec.nodes.iter().filter(|node| !node.node.is_terminal()) {
        if outgoing
            .get(node.node_id.as_str())
            .is_none_or(Vec::is_empty)
        {
            push_issue(
                &mut issues,
                "missing_outgoing_transition",
                "Nonterminal nodes require at least one outgoing transition",
                Some(&node.node_id),
                None,
            );
        }
    }

    let reachable = reachable_from_root(&spec.root_node_id, &outgoing);
    for node in &spec.nodes {
        if !reachable.contains(node.node_id.as_str()) {
            push_issue(
                &mut issues,
                "unreachable_node",
                "Every node must be reachable from the root",
                Some(&node.node_id),
                None,
            );
        }
    }

    let terminal_ids = spec
        .nodes
        .iter()
        .filter(|node| node.node.is_terminal())
        .map(|node| node.node_id.as_str())
        .collect::<Vec<_>>();
    if terminal_ids.is_empty() {
        push_issue(
            &mut issues,
            "missing_terminal",
            "The graph requires at least one terminal node",
            None,
            None,
        );
    } else {
        let can_reach_terminal = reverse_reachable(&terminal_ids, &incoming);
        for node in spec
            .nodes
            .iter()
            .filter(|node| reachable.contains(node.node_id.as_str()))
        {
            if !can_reach_terminal.contains(node.node_id.as_str()) {
                push_issue(
                    &mut issues,
                    "no_terminal_path",
                    "Every reachable node must have a path to a terminal",
                    Some(&node.node_id),
                    None,
                );
            }
        }
    }

    OrchestrationValidationReport {
        valid: issues.is_empty(),
        issues,
    }
}

fn validate_wait_node(
    node: &OrchestrationNodeSpec,
    issues: &mut Vec<OrchestrationValidationIssue>,
) {
    let OrchestrationNodeKind::Wait { wait } = &node.node else {
        return;
    };
    if !wait_spec_issues(wait).is_empty() {
        push_issue(
            issues,
            "invalid_wait",
            "Wait nodes require a bounded, well-formed wake condition",
            Some(&node.node_id),
            None,
        );
    }
}

fn invalid_timeout(timeout: &WaitTimeoutPolicy) -> bool {
    timeout.expires_after_ms == 0
        || (timeout.on_timeout == WaitTimeoutAction::Escalate
            && timeout
                .escalate_to
                .as_deref()
                .is_none_or(|value| value.trim().is_empty()))
        || timeout.remind_every_ms == Some(0)
}

fn invalid_binding(binding: &OrchestrationValueBinding) -> bool {
    match binding {
        OrchestrationValueBinding::Literal { value } => value.is_null(),
        OrchestrationValueBinding::NodeOutput { node_id, .. } => node_id.trim().is_empty(),
    }
}

fn validate_artifact_compatibility(
    source: &OrchestrationNodeSpec,
    target: &OrchestrationNodeSpec,
    edge: &OrchestrationEdgeSpec,
    issues: &mut Vec<OrchestrationValidationIssue>,
) {
    let Some(contract) = edge.artifact_contract.as_ref() else {
        return;
    };
    if contract.artifact_type.trim().is_empty() {
        push_issue(
            issues,
            "empty_artifact_type",
            "Artifact types cannot be empty",
            None,
            Some(&edge.edge_id),
        );
        return;
    }
    if let OrchestrationNodeKind::Workflow {
        emits_artifact_types,
        ..
    } = &source.node
    {
        if !emits_artifact_types.is_empty()
            && !emits_artifact_types.contains(&contract.artifact_type)
        {
            push_issue(
                issues,
                "source_artifact_mismatch",
                "The source workflow does not emit the edge artifact type",
                Some(&source.node_id),
                Some(&edge.edge_id),
            );
        }
    }
    if let OrchestrationNodeKind::Workflow {
        accepts_artifact_types,
        ..
    } = &target.node
    {
        if !accepts_artifact_types.is_empty()
            && !accepts_artifact_types.contains(&contract.artifact_type)
        {
            push_issue(
                issues,
                "target_artifact_mismatch",
                "The target workflow does not accept the edge artifact type",
                Some(&target.node_id),
                Some(&edge.edge_id),
            );
        }
    }
}

fn reachable_from_root<'a>(
    root: &'a str,
    outgoing: &HashMap<&'a str, Vec<&'a OrchestrationEdgeSpec>>,
) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(node_id) = queue.pop_front() {
        if !seen.insert(node_id) {
            continue;
        }
        for edge in outgoing.get(node_id).into_iter().flatten() {
            queue.push_back(edge.to_node_id.as_str());
        }
    }
    seen
}

fn reverse_reachable<'a>(
    roots: &[&'a str],
    incoming: &HashMap<&'a str, Vec<&'a OrchestrationEdgeSpec>>,
) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from_iter(roots.iter().copied());
    while let Some(node_id) = queue.pop_front() {
        if !seen.insert(node_id) {
            continue;
        }
        for edge in incoming.get(node_id).into_iter().flatten() {
            queue.push_back(edge.from_node_id.as_str());
        }
    }
    seen
}

fn push_issue(
    issues: &mut Vec<OrchestrationValidationIssue>,
    code: &str,
    message: &str,
    node_id: Option<&str>,
    edge_id: Option<&str>,
) {
    issues.push(OrchestrationValidationIssue {
        code: code.to_string(),
        message: message.to_string(),
        node_id: node_id.map(ToOwned::to_owned),
        edge_id: edge_id.map(ToOwned::to_owned),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> TenantContext {
        TenantContext::explicit("org-a", "workspace-a", None)
    }

    fn workflow(node_id: &str, automation_id: &str) -> OrchestrationNodeSpec {
        OrchestrationNodeSpec {
            node_id: node_id.to_string(),
            name: node_id.to_string(),
            position: OrchestrationCanvasPosition::default(),
            node: OrchestrationNodeKind::Workflow {
                automation_id: automation_id.to_string(),
                pinned_definition_hash: Some(format!("sha256:{automation_id}")),
                allowed_transition_keys: vec![
                    "continue".to_string(),
                    "complete".to_string(),
                    "replan".to_string(),
                ],
                accepts_artifact_types: vec!["plan".to_string()],
                emits_artifact_types: vec!["plan".to_string()],
            },
        }
    }

    fn terminal(node_id: &str) -> OrchestrationNodeSpec {
        OrchestrationNodeSpec {
            node_id: node_id.to_string(),
            name: node_id.to_string(),
            position: OrchestrationCanvasPosition::default(),
            node: OrchestrationNodeKind::Terminal {
                outcome: GoalTerminalOutcome::Complete,
                final_artifact_type: Some("plan".to_string()),
            },
        }
    }

    fn edge(id: &str, from: &str, to: &str, key: &str) -> OrchestrationEdgeSpec {
        OrchestrationEdgeSpec {
            edge_id: id.to_string(),
            from_node_id: from.to_string(),
            to_node_id: to.to_string(),
            transition_key: key.to_string(),
            artifact_contract: Some(OrchestrationArtifactContract {
                artifact_type: "plan".to_string(),
                schema: None,
                required: true,
            }),
            approval: None,
            metadata: None,
        }
    }

    fn valid_loop() -> OrchestrationSpec {
        OrchestrationSpec {
            schema_version: 1,
            orchestration_id: "goal-loop".to_string(),
            name: "Goal loop".to_string(),
            description: None,
            status: OrchestrationStatus::Draft,
            version: 1,
            root_node_id: "plan".to_string(),
            nodes: vec![
                workflow("plan", "planner"),
                workflow("execute", "executor"),
                workflow("verify", "verifier"),
                terminal("complete"),
            ],
            edges: vec![
                edge("plan-execute", "plan", "execute", "continue"),
                edge("execute-verify", "execute", "verify", "continue"),
                edge("verify-plan", "verify", "plan", "replan"),
                edge("verify-complete", "verify", "complete", "complete"),
            ],
            goal_policy: GoalPolicy::default(),
            tenant_context: tenant(),
            created_at_ms: 1,
            updated_at_ms: 1,
            published_at_ms: None,
            metadata: None,
        }
    }

    #[test]
    fn validates_bounded_goal_loop_with_terminal_path() {
        let report = validate_orchestration_spec(&valid_loop());
        assert!(report.valid, "issues: {:?}", report.issues);
    }

    #[test]
    fn rejects_unreachable_node_and_unbounded_policy() {
        let mut spec = valid_loop();
        spec.goal_policy.max_hops = 0;
        spec.nodes.push(workflow("orphan", "orphan"));
        let report = validate_orchestration_spec(&spec);
        assert!(!report.valid);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_max_hops"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "unreachable_node"));
    }

    #[test]
    fn rejects_duplicate_transition_keys() {
        let mut spec = valid_loop();
        spec.edges
            .push(edge("duplicate", "verify", "complete", "complete"));
        let report = validate_orchestration_spec(&spec);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "duplicate_transition_key"));
    }

    #[test]
    fn rejects_transition_not_declared_by_workflow() {
        let mut spec = valid_loop();
        spec.edges[0].transition_key = "invented_by_agent".to_string();
        let report = validate_orchestration_spec(&spec);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "unknown_transition_key"));
    }

    #[test]
    fn rejects_unbounded_wait_configuration() {
        let mut spec = valid_loop();
        spec.nodes[1] = OrchestrationNodeSpec {
            node_id: "execute".to_string(),
            name: "wait".to_string(),
            position: OrchestrationCanvasPosition::default(),
            node: OrchestrationNodeKind::Wait {
                wait: AutomationWaitSpec::ExternalCondition {
                    condition_key: OrchestrationValueBinding::Literal { value: Value::Null },
                    timeout: WaitTimeoutPolicy {
                        expires_after_ms: 0,
                        on_timeout: WaitTimeoutAction::Resume,
                        escalate_to: None,
                        remind_every_ms: None,
                    },
                    payload_schema: None,
                },
            },
        };
        let report = validate_orchestration_spec(&spec);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_wait"));
    }

    #[test]
    fn goal_policy_pauses_before_an_over_budget_hop() {
        let mut goal = LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-1".to_string(),
            orchestration_id: "goal-loop".to_string(),
            orchestration_version: 1,
            objective: "Complete the project".to_string(),
            status: LongRunningGoalStatus::Active,
            tenant_context: tenant(),
            policy: GoalPolicy {
                max_hops: 3,
                deadline_at_ms: None,
                max_total_tokens: Some(100),
                max_total_cost_usd: None,
                on_limit: GoalLimitAction::PauseForReview,
            },
            active_run_id: Some("run-1".to_string()),
            current_node_id: Some("verify".to_string()),
            hop_count: 2,
            total_tokens: 90,
            total_cost_usd: 0.0,
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
            final_artifact: None,
            metadata: None,
        };

        let admission = goal.admit_transition(2, 11, 0.0);
        assert!(!admission.allowed);
        assert_eq!(admission.limit, Some(GoalPolicyLimit::TokenBudget));
        assert_eq!(
            admission.resulting_status,
            Some(LongRunningGoalStatus::Paused)
        );
        assert_eq!(goal.cancel(3), Some("run-1".to_string()));
        assert_eq!(goal.status, LongRunningGoalStatus::Cancelled);
        assert_eq!(goal.cancel(4), None);
    }

    #[test]
    fn deadline_expires_the_goal() {
        let mut goal = LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-2".to_string(),
            orchestration_id: "goal-loop".to_string(),
            orchestration_version: 1,
            objective: "Complete the project".to_string(),
            status: LongRunningGoalStatus::Waiting,
            tenant_context: tenant(),
            policy: GoalPolicy {
                max_hops: 3,
                deadline_at_ms: Some(50),
                max_total_tokens: None,
                max_total_cost_usd: None,
                on_limit: GoalLimitAction::PauseForReview,
            },
            active_run_id: None,
            current_node_id: Some("timer".to_string()),
            hop_count: 1,
            total_tokens: 0,
            total_cost_usd: 0.0,
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
            final_artifact: None,
            metadata: None,
        };
        let admission = goal.admit_transition(50, 0, 0.0);
        assert_eq!(admission.limit, Some(GoalPolicyLimit::Deadline));
        assert_eq!(
            admission.resulting_status,
            Some(LongRunningGoalStatus::Expired)
        );
        goal.apply_terminal_outcome(GoalTerminalOutcome::Pause, 51);
        assert_eq!(goal.status, LongRunningGoalStatus::Paused);
    }
}
