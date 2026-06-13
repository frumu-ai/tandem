use crate::workflow_runtime_topology::{critical_path, parallel_groups};
use crate::{
    GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, PolicyDecision, WorkflowGraph,
    WorkflowStepDependencySummary,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRuntimeState {
    pub completed_steps: Vec<String>,
    pub failed_steps: Vec<String>,
}

impl WorkflowRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_completed_steps(
        mut self,
        steps: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.completed_steps = steps.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_failed_steps(mut self, steps: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.failed_steps = steps.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPreflightReport {
    pub allowed: bool,
    pub checked_steps: Vec<String>,
    pub blockers: Vec<WorkflowBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBlocker {
    pub step_id: String,
    pub kind: WorkflowBlockerKind,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowBlockerKind {
    #[serde(rename = "envelope_invalid")]
    EnvelopeInvalid,
    #[serde(rename = "scope_mismatch")]
    ScopeMismatch,
    #[serde(rename = "tool_denied")]
    ToolDenied,
    #[serde(rename = "memory_denied")]
    MemoryDenied,
    #[serde(rename = "approval_missing")]
    ApprovalMissing,
    #[serde(rename = "policy_denied")]
    PolicyDenied,
    #[serde(rename = "approval_required")]
    ApprovalRequired,
    #[serde(rename = "dependency_pending")]
    DependencyPending,
    #[serde(rename = "dependency_failed")]
    DependencyFailed,
    #[serde(rename = "step_failed")]
    StepFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowToolSelection {
    pub step_id: Option<String>,
    pub candidates: Vec<WorkflowToolCandidate>,
    pub policy_notes: Vec<String>,
    pub metrics: WorkflowPromptPruningMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowToolCandidate {
    pub tool_name: String,
    pub selected: bool,
    pub reason: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPromptPruningMetrics {
    pub candidate_tools: usize,
    pub selected_tools: usize,
    pub denied_tools: usize,
    pub pruned_tools: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRuntimePlan {
    pub ready_nodes: Vec<WorkflowReadyNode>,
    pub blocked_nodes: Vec<WorkflowBlockedNode>,
    pub parallel_groups: Vec<Vec<String>>,
    pub critical_path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowReadyNode {
    pub step_id: String,
    pub runnable_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBlockedNode {
    pub step_id: String,
    pub blockers: Vec<WorkflowBlocker>,
}

impl WorkflowGraph {
    pub fn workflow_preflight(
        &self,
        envelope: &GraphQueryEnvelope,
    ) -> GraphQueryOutput<WorkflowPreflightReport> {
        let mut audit = GraphQueryAudit::default();
        let mut blockers = self.envelope_blockers(envelope);

        for (step_id, summary) in &self.step_dependencies {
            append_governance_blockers(step_id, summary, envelope, &mut blockers);
            append_policy_blockers(self, step_id, &mut blockers);
        }

        for blocker in &blockers {
            audit.deny(blocker.detail.clone());
        }

        GraphQueryOutput::new(
            WorkflowPreflightReport {
                allowed: blockers.is_empty(),
                checked_steps: self.step_ids(),
                blockers,
            },
            audit,
        )
    }

    pub fn workflow_tool_selection(
        &self,
        envelope: &GraphQueryEnvelope,
        step_id: Option<&str>,
    ) -> GraphQueryOutput<WorkflowToolSelection> {
        let mut audit = GraphQueryAudit::default();
        let envelope_blockers = self.envelope_blockers(envelope);
        if !envelope_blockers.is_empty() {
            for blocker in envelope_blockers {
                audit.deny(blocker.detail);
            }
            return GraphQueryOutput::new(
                WorkflowToolSelection {
                    step_id: step_id.map(str::to_string),
                    candidates: Vec::new(),
                    policy_notes: Vec::new(),
                    metrics: WorkflowPromptPruningMetrics::default(),
                },
                audit,
            );
        }

        let mut tools = BTreeSet::new();
        let mut policy_notes = BTreeSet::new();

        for (candidate_step_id, summary) in &self.step_dependencies {
            if step_id.is_none_or(|selected| selected == candidate_step_id) {
                tools.extend(summary.required_tools.iter().cloned());
                policy_notes.extend(summary.policy_scopes.iter().cloned());
            }
        }

        let candidates: Vec<_> = tools
            .into_iter()
            .map(|tool_name| {
                let selected = envelope.allows_tool(&tool_name);
                if !selected {
                    audit.deny(format!(
                        "tool `{tool_name}` is not allowed by graph query envelope"
                    ));
                }
                WorkflowToolCandidate {
                    reason: tool_selection_reason(selected, &tool_name),
                    provenance: "workflow_graph.required_tools".to_string(),
                    tool_name,
                    selected,
                }
            })
            .collect();
        let selected_tools = candidates.iter().filter(|tool| tool.selected).count();
        let denied_tools = candidates.len() - selected_tools;

        GraphQueryOutput::new(
            WorkflowToolSelection {
                step_id: step_id.map(str::to_string),
                policy_notes: policy_notes.into_iter().collect(),
                metrics: WorkflowPromptPruningMetrics {
                    candidate_tools: candidates.len(),
                    selected_tools,
                    denied_tools,
                    pruned_tools: denied_tools,
                },
                candidates,
            },
            audit,
        )
    }

    pub fn workflow_runtime_plan(
        &self,
        state: &WorkflowRuntimeState,
        envelope: &GraphQueryEnvelope,
    ) -> GraphQueryOutput<WorkflowRuntimePlan> {
        let mut audit = GraphQueryAudit::default();
        let envelope_blockers = self.envelope_blockers(envelope);
        let completed: BTreeSet<_> = state.completed_steps.iter().cloned().collect();
        let failed: BTreeSet<_> = state.failed_steps.iter().cloned().collect();
        let mut ready_nodes = Vec::new();
        let mut blocked_nodes = Vec::new();

        for (step_id, summary) in &self.step_dependencies {
            if completed.contains(step_id) {
                continue;
            }
            let mut blockers = envelope_blockers
                .iter()
                .map(|blocker| blocker.for_step(step_id))
                .collect::<Vec<_>>();
            if blockers.is_empty() {
                blockers = runtime_blockers(self, step_id, summary, envelope, &completed, &failed);
            }
            if blockers.is_empty() {
                ready_nodes.push(WorkflowReadyNode {
                    step_id: step_id.clone(),
                    runnable_reason: "all dependencies and governance preflight checks passed"
                        .to_string(),
                });
            } else {
                for blocker in &blockers {
                    audit.deny(blocker.detail.clone());
                }
                blocked_nodes.push(WorkflowBlockedNode {
                    step_id: step_id.clone(),
                    blockers,
                });
            }
        }

        GraphQueryOutput::new(
            WorkflowRuntimePlan {
                ready_nodes,
                blocked_nodes,
                parallel_groups: parallel_groups(&self.step_dependencies),
                critical_path: critical_path(&self.step_dependencies),
            },
            audit,
        )
    }

    fn step_ids(&self) -> Vec<String> {
        self.step_dependencies
            .iter()
            .map(|(step_id, _)| step_id.clone())
            .collect()
    }

    pub(crate) fn envelope_blockers(&self, envelope: &GraphQueryEnvelope) -> Vec<WorkflowBlocker> {
        let mut blockers = Vec::new();
        if let Err(error) = envelope.validate() {
            blockers.push(WorkflowBlocker::new(
                "",
                WorkflowBlockerKind::EnvelopeInvalid,
                error.missing.join(","),
                error.to_string(),
            ));
        }
        if !self.partition.is_visible_to(&envelope.scope) {
            blockers.push(WorkflowBlocker::new(
                "",
                WorkflowBlockerKind::ScopeMismatch,
                self.partition.key(),
                "graph query envelope scope is not visible to the workflow partition",
            ));
        }
        blockers
    }
}

impl WorkflowBlocker {
    pub(crate) fn new(
        step_id: impl Into<String>,
        kind: WorkflowBlockerKind,
        target: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            step_id: step_id.into(),
            kind,
            target: target.into(),
            detail: detail.into(),
        }
    }

    pub(crate) fn for_step(&self, step_id: impl Into<String>) -> Self {
        Self {
            step_id: step_id.into(),
            kind: self.kind.clone(),
            target: self.target.clone(),
            detail: self.detail.clone(),
        }
    }
}

fn append_governance_blockers(
    step_id: &str,
    summary: &WorkflowStepDependencySummary,
    envelope: &GraphQueryEnvelope,
    blockers: &mut Vec<WorkflowBlocker>,
) {
    for tool in &summary.required_tools {
        if !envelope.allows_tool(tool) {
            blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::ToolDenied,
                tool,
                format!("tool `{tool}` is not allowed or available"),
            ));
        }
    }
    for tier in &summary.memory_tiers {
        if !envelope.allows_memory_tier(tier) {
            blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::MemoryDenied,
                tier,
                format!("memory tier `{tier}` is not allowed"),
            ));
        }
    }
    for gate in &summary.approval_gates {
        if !envelope.has_approval(gate) {
            blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::ApprovalMissing,
                gate,
                format!("approval gate `{gate}` has not been satisfied"),
            ));
        }
    }
}

fn append_policy_blockers(
    graph: &WorkflowGraph,
    step_id: &str,
    blockers: &mut Vec<WorkflowBlocker>,
) {
    for edge in graph
        .edges
        .iter()
        .filter(|edge| edge.source.key == step_id || edge.target.key == step_id)
    {
        match &edge.policy {
            PolicyDecision::Allowed => {}
            PolicyDecision::Denied { reason } => blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::PolicyDenied,
                edge.kind.stable_id(),
                reason,
            )),
            PolicyDecision::RequiresApproval { approval_gate } => {
                blockers.push(WorkflowBlocker::new(
                    step_id,
                    WorkflowBlockerKind::ApprovalRequired,
                    approval_gate,
                    format!("policy requires approval gate `{approval_gate}`"),
                ))
            }
        }
    }
}

fn runtime_blockers(
    graph: &WorkflowGraph,
    step_id: &str,
    summary: &WorkflowStepDependencySummary,
    envelope: &GraphQueryEnvelope,
    completed: &BTreeSet<String>,
    failed: &BTreeSet<String>,
) -> Vec<WorkflowBlocker> {
    let mut blockers = Vec::new();
    if failed.contains(step_id) {
        blockers.push(WorkflowBlocker::new(
            step_id,
            WorkflowBlockerKind::StepFailed,
            step_id,
            "step has already failed",
        ));
    }
    for upstream in &summary.depends_on {
        if failed.contains(upstream) {
            blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::DependencyFailed,
                upstream,
                format!("dependency `{upstream}` failed"),
            ));
        } else if !completed.contains(upstream) {
            blockers.push(WorkflowBlocker::new(
                step_id,
                WorkflowBlockerKind::DependencyPending,
                upstream,
                format!("dependency `{upstream}` has not completed"),
            ));
        }
    }
    append_governance_blockers(step_id, summary, envelope, &mut blockers);
    append_policy_blockers(graph, step_id, &mut blockers);
    blockers
}

fn tool_selection_reason(selected: bool, tool_name: &str) -> String {
    if selected {
        format!("tool `{tool_name}` is required by the workflow graph and allowed")
    } else {
        format!("tool `{tool_name}` is required by the workflow graph but denied")
    }
}
