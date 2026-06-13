use crate::{
    GraphNode, GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, NodeKind,
    RunCascadingFailure, RunFailureCausalityReport, RunFailureCause, RunFailureCauseKind,
    RunRepairContext, RunRepairEvidence, RunTraceGraph, WorkflowGraph,
};
use std::collections::BTreeSet;

impl RunTraceGraph {
    pub fn failure_causality_report(
        &self,
        envelope: &GraphQueryEnvelope,
        workflow: Option<&WorkflowGraph>,
    ) -> GraphQueryOutput<RunFailureCausalityReport> {
        let mut audit = GraphQueryAudit::default();
        if let Err(error) = envelope.validate() {
            audit.deny(error.to_string());
            return GraphQueryOutput::new(self.empty_failure_report(), audit);
        }
        if !self.is_visible_to_envelope(envelope) {
            audit.deny("graph query envelope scope is not visible to the run trace partition");
            return GraphQueryOutput::new(self.empty_failure_report(), audit);
        }

        let mut events = self
            .nodes
            .iter()
            .filter_map(TraceFailureEvent::from_node)
            .filter(TraceFailureEvent::is_failure_signal)
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            left.occurred_at_unix_ms
                .cmp(&right.occurred_at_unix_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });

        let mut root_steps = BTreeSet::new();
        let mut root_causes = Vec::new();
        let mut cascading_failures = Vec::new();

        for event in events {
            let upstream_root_steps = event
                .step_id
                .as_deref()
                .map(|step_id| upstream_root_steps(workflow, step_id, &root_steps))
                .unwrap_or_default();
            if upstream_root_steps.is_empty() {
                if let Some(step_id) = &event.step_id {
                    root_steps.insert(step_id.clone());
                }
                root_causes.push(event.to_root_cause());
            } else {
                cascading_failures.push(event.to_cascading_failure(upstream_root_steps));
            }
        }

        let repair_context = repair_context(&root_causes, &cascading_failures);

        GraphQueryOutput::new(
            RunFailureCausalityReport {
                run_id: self.run_id(),
                root_causes,
                cascading_failures,
                repair_context,
            },
            audit,
        )
    }

    fn is_visible_to_envelope(&self, envelope: &GraphQueryEnvelope) -> bool {
        let scope_run_id = envelope.scope.run_id.as_deref();
        let envelope_run_id = envelope.run_id.as_deref();
        if scope_run_id
            .zip(envelope_run_id)
            .is_some_and(|(scope_run_id, envelope_run_id)| scope_run_id != envelope_run_id)
        {
            return false;
        }

        let mut scope = envelope.scope.clone();
        if scope.run_id.is_none() {
            scope.run_id = envelope.run_id.clone();
        }
        self.partition.is_visible_to(&scope)
    }

    fn empty_failure_report(&self) -> RunFailureCausalityReport {
        RunFailureCausalityReport {
            run_id: self.run_id(),
            root_causes: Vec::new(),
            cascading_failures: Vec::new(),
            repair_context: RunRepairContext {
                summary: "failure causality query was not allowed".to_string(),
                ..RunRepairContext::default()
            },
        }
    }

    fn run_id(&self) -> String {
        self.partition
            .scope
            .run_id
            .clone()
            .or_else(|| {
                self.nodes
                    .iter()
                    .find(|node| node.kind == NodeKind::Run)
                    .and_then(|node| node.payload.get("run_id").cloned())
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
struct TraceFailureEvent {
    event_id: String,
    event_kind: String,
    node_kind: NodeKind,
    step_id: Option<String>,
    tool_name: Option<String>,
    memory_tier: Option<String>,
    policy_scope: Option<String>,
    artifact_ref: Option<String>,
    safe_summary: String,
    policy_denied: bool,
    occurred_at_unix_ms: Option<u64>,
}

impl TraceFailureEvent {
    fn from_node(node: &GraphNode) -> Option<Self> {
        let event_id = node.payload.get("event_id")?.clone();
        Some(Self {
            event_id,
            event_kind: node
                .payload
                .get("kind")
                .cloned()
                .unwrap_or_else(|| node.kind.stable_id().to_string()),
            node_kind: node.kind.clone(),
            step_id: node.payload.get("workflow_step_id").cloned(),
            tool_name: node.payload.get("tool_name").cloned(),
            memory_tier: node.payload.get("memory_tier").cloned(),
            policy_scope: node.payload.get("policy_scope").cloned(),
            artifact_ref: node.payload.get("artifact_ref").cloned(),
            safe_summary: node
                .payload
                .get("safe_summary")
                .cloned()
                .unwrap_or_else(|| "no display-safe summary captured".to_string()),
            policy_denied: node
                .payload
                .get("policy_denied")
                .is_some_and(|value| value == "true"),
            occurred_at_unix_ms: node
                .payload
                .get("occurred_at_unix_ms")
                .and_then(|value| value.parse().ok()),
        })
    }

    fn is_failure_signal(&self) -> bool {
        self.node_kind == NodeKind::Error
            || self.policy_denied
            || summary_indicates_failure(&self.safe_summary)
    }

    fn to_root_cause(&self) -> RunFailureCause {
        RunFailureCause {
            event_id: self.event_id.clone(),
            step_id: self.step_id.clone(),
            kind: self.cause_kind(),
            target: self.target(),
            summary: self.safe_summary.clone(),
            evidence: vec![self.evidence()],
        }
    }

    fn to_cascading_failure(&self, upstream_root_steps: Vec<String>) -> RunCascadingFailure {
        RunCascadingFailure {
            event_id: self.event_id.clone(),
            step_id: self.step_id.clone(),
            upstream_root_steps,
            summary: self.safe_summary.clone(),
            evidence: vec![self.evidence()],
        }
    }

    fn cause_kind(&self) -> RunFailureCauseKind {
        if self.node_kind == NodeKind::ApprovalGate {
            return RunFailureCauseKind::ApprovalInterruption;
        }
        if self.policy_denied
            || self.policy_scope.is_some() && self.node_kind == NodeKind::PolicyScope
        {
            return RunFailureCauseKind::PolicyDenied;
        }
        if self.tool_name.is_some() || self.node_kind == NodeKind::ToolCall {
            return RunFailureCauseKind::ToolFailure;
        }
        if self.memory_tier.is_some()
            || matches!(
                self.node_kind,
                NodeKind::RetrievedMemory | NodeKind::MemoryWriteCandidate
            )
        {
            return RunFailureCauseKind::MemoryFailure;
        }
        if self.node_kind == NodeKind::ModelCall {
            return RunFailureCauseKind::ModelFailure;
        }
        if self.artifact_ref.is_some() || self.node_kind == NodeKind::Artifact {
            return RunFailureCauseKind::ArtifactFailure;
        }
        RunFailureCauseKind::Error
    }

    fn evidence(&self) -> RunRepairEvidence {
        RunRepairEvidence {
            event_id: self.event_id.clone(),
            event_kind: self.event_kind.clone(),
            step_id: self.step_id.clone(),
            target: self.target(),
            safe_summary: self.safe_summary.clone(),
        }
    }

    fn target(&self) -> Option<String> {
        self.tool_name
            .clone()
            .or_else(|| self.memory_tier.clone())
            .or_else(|| self.policy_scope.clone())
            .or_else(|| self.artifact_ref.clone())
            .or_else(|| self.step_id.clone())
    }
}

fn summary_indicates_failure(summary: &str) -> bool {
    let summary = summary.to_ascii_lowercase();
    [
        "blocked", "denied", "empty", "error", "fail", "missing", "stale", "timeout",
    ]
    .iter()
    .any(|needle| summary.contains(needle))
}

fn upstream_root_steps(
    workflow: Option<&WorkflowGraph>,
    step_id: &str,
    root_steps: &BTreeSet<String>,
) -> Vec<String> {
    root_steps
        .iter()
        .filter(|root_step| {
            workflow.is_some_and(|workflow| step_depends_on(workflow, step_id, root_step))
        })
        .cloned()
        .collect()
}

fn step_depends_on(workflow: &WorkflowGraph, step_id: &str, upstream: &str) -> bool {
    let mut visited = BTreeSet::new();
    let mut stack = vec![step_id.to_string()];
    while let Some(candidate) = stack.pop() {
        if !visited.insert(candidate.clone()) {
            continue;
        }
        let Some(summary) = workflow.dependencies_for_step(&candidate) else {
            continue;
        };
        if summary
            .depends_on
            .iter()
            .any(|dependency| dependency == upstream)
        {
            return true;
        }
        stack.extend(summary.depends_on.iter().cloned());
    }
    false
}

fn repair_context(
    root_causes: &[RunFailureCause],
    cascading_failures: &[RunCascadingFailure],
) -> RunRepairContext {
    let mut related_steps = BTreeSet::new();
    let mut relevant_tools = BTreeSet::new();
    let mut memory_tiers = BTreeSet::new();
    let mut policy_scopes = BTreeSet::new();
    let mut artifact_refs = BTreeSet::new();
    let mut evidence = Vec::new();

    for cause in root_causes {
        add_cause_context(
            cause.step_id.as_deref(),
            &cause.kind,
            cause.target.as_deref(),
            &cause.evidence,
            &mut related_steps,
            &mut relevant_tools,
            &mut memory_tiers,
            &mut policy_scopes,
            &mut artifact_refs,
            &mut evidence,
        );
    }
    for failure in cascading_failures {
        if let Some(step_id) = &failure.step_id {
            related_steps.insert(step_id.clone());
        }
        for step_id in &failure.upstream_root_steps {
            related_steps.insert(step_id.clone());
        }
        evidence.extend(failure.evidence.iter().cloned());
    }

    RunRepairContext {
        summary: repair_summary(root_causes, cascading_failures),
        related_steps: related_steps.into_iter().collect(),
        relevant_tools: relevant_tools.into_iter().collect(),
        memory_tiers: memory_tiers.into_iter().collect(),
        policy_scopes: policy_scopes.into_iter().collect(),
        artifact_refs: artifact_refs.into_iter().collect(),
        evidence,
    }
}

#[allow(clippy::too_many_arguments)]
fn add_cause_context(
    step_id: Option<&str>,
    kind: &RunFailureCauseKind,
    target: Option<&str>,
    cause_evidence: &[RunRepairEvidence],
    related_steps: &mut BTreeSet<String>,
    relevant_tools: &mut BTreeSet<String>,
    memory_tiers: &mut BTreeSet<String>,
    policy_scopes: &mut BTreeSet<String>,
    artifact_refs: &mut BTreeSet<String>,
    evidence: &mut Vec<RunRepairEvidence>,
) {
    if let Some(step_id) = step_id {
        related_steps.insert(step_id.to_string());
    }
    match (kind, target) {
        (RunFailureCauseKind::ToolFailure, Some(target)) => {
            relevant_tools.insert(target.to_string());
        }
        (RunFailureCauseKind::MemoryFailure, Some(target)) => {
            memory_tiers.insert(target.to_string());
        }
        (RunFailureCauseKind::PolicyDenied, Some(target))
        | (RunFailureCauseKind::ApprovalInterruption, Some(target)) => {
            policy_scopes.insert(target.to_string());
        }
        (RunFailureCauseKind::ArtifactFailure, Some(target)) => {
            artifact_refs.insert(target.to_string());
        }
        _ => {}
    }
    evidence.extend(cause_evidence.iter().cloned());
}

fn repair_summary(
    root_causes: &[RunFailureCause],
    cascading_failures: &[RunCascadingFailure],
) -> String {
    match (root_causes.len(), cascading_failures.len()) {
        (0, 0) => "no failure signals found in display-safe run trace events".to_string(),
        (root_count, 0) => format!("{root_count} root-cause event(s) identified"),
        (root_count, cascading_count) => format!(
            "{root_count} root-cause event(s) and {cascading_count} cascading failure event(s) identified"
        ),
    }
}
