use crate::{
    GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, NodeKind, WorkflowBlocker,
    WorkflowGraph, WorkflowImpactChange, WorkflowImpactQuery, WorkflowImpactReport,
    WorkflowImpactRiskGroup, WorkflowImpactRiskHint, WorkflowImpactStep, WorkflowImpactWorkflow,
    WorkflowStepDependencySummary,
};
use std::collections::{BTreeMap, BTreeSet};

impl WorkflowGraph {
    pub fn workflow_impact_analysis(
        &self,
        envelope: &GraphQueryEnvelope,
        query: WorkflowImpactQuery,
    ) -> GraphQueryOutput<WorkflowImpactReport> {
        let mut audit = GraphQueryAudit::default();
        let blockers = self.envelope_blockers(envelope);
        if !blockers.is_empty() {
            for blocker in &blockers {
                audit.deny(blocker.detail.clone());
            }
            return GraphQueryOutput::new(self.blocked_impact_report(blockers), audit);
        }

        let direct = directly_impacted_steps(&self.step_dependencies, &query.changes);
        let visible_direct =
            filter_visible_steps(direct, &self.step_dependencies, envelope, &mut audit);
        let impacted = downstream_closure(&self.step_dependencies, visible_direct.keys());
        let affected_steps = affected_steps(&self.step_dependencies, &visible_direct, &impacted);
        let risk_groups = risk_groups(&affected_steps, &query.risk_hints);
        let checks_to_run = collect_report_checks(&affected_steps, &risk_groups);

        GraphQueryOutput::new(
            WorkflowImpactReport {
                workflow_scope: self.partition.scope.clone(),
                affected_workflows: affected_workflows(self, !affected_steps.is_empty()),
                affected_steps,
                risk_groups,
                checks_to_run,
                blockers,
            },
            audit,
        )
    }

    fn blocked_impact_report(&self, blockers: Vec<WorkflowBlocker>) -> WorkflowImpactReport {
        WorkflowImpactReport {
            workflow_scope: self.partition.scope.clone(),
            affected_workflows: Vec::new(),
            affected_steps: Vec::new(),
            risk_groups: Vec::new(),
            checks_to_run: Vec::new(),
            blockers,
        }
    }
}

fn directly_impacted_steps(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    changes: &[WorkflowImpactChange],
) -> BTreeMap<String, Vec<String>> {
    let mut impacted = BTreeMap::<String, Vec<String>>::new();
    for (step_id, summary) in dependencies {
        for change in changes {
            if let Some(reason) = impact_reason(change, summary) {
                impacted.entry(step_id.clone()).or_default().push(reason);
            }
        }
    }
    impacted
}

fn filter_visible_steps(
    direct: BTreeMap<String, Vec<String>>,
    dependencies: &[(String, WorkflowStepDependencySummary)],
    envelope: &GraphQueryEnvelope,
    audit: &mut GraphQueryAudit,
) -> BTreeMap<String, Vec<String>> {
    direct
        .into_iter()
        .filter(|(step_id, _)| {
            let Some(summary) = dependencies_for_step(dependencies, step_id) else {
                return false;
            };
            let hidden_tools = summary
                .required_tools
                .iter()
                .filter(|tool| !envelope.allows_tool(tool))
                .cloned()
                .collect::<Vec<_>>();
            let hidden_memory = summary
                .memory_tiers
                .iter()
                .filter(|tier| !envelope.allows_memory_tier(tier))
                .cloned()
                .collect::<Vec<_>>();
            if hidden_tools.is_empty() && hidden_memory.is_empty() {
                return true;
            }
            for tool in hidden_tools {
                audit.deny(format!(
                    "impact for step `{step_id}` references tool `{tool}` outside the query envelope"
                ));
            }
            for tier in hidden_memory {
                audit.deny(format!(
                    "impact for step `{step_id}` references memory tier `{tier}` outside the query envelope"
                ));
            }
            false
        })
        .collect()
}

fn impact_reason(
    change: &WorkflowImpactChange,
    summary: &WorkflowStepDependencySummary,
) -> Option<String> {
    match change {
        WorkflowImpactChange::ToolSchemaChanged { tool_name } => {
            matches_any(tool_name, &summary.required_tools)
                .then(|| target_reason("tool schema changed", tool_name))
        }
        WorkflowImpactChange::McpServerChanged { tool_names, .. } => (tool_names.is_empty()
            || summary
                .required_tools
                .iter()
                .any(|tool| tool_names.iter().any(|changed| changed == tool)))
        .then(|| "MCP server changed for a required tool".to_string()),
        WorkflowImpactChange::CredentialChanged { tool_name, .. } => {
            matches_any(tool_name, &summary.required_tools)
                .then(|| target_reason("credential changed for required tool", tool_name))
        }
        WorkflowImpactChange::MemoryCollectionChanged {
            tier, policy_scope, ..
        } => (matches_any(tier, &summary.memory_tiers)
            || matches_any(policy_scope, &summary.policy_scopes))
        .then(|| "memory collection or policy changed".to_string()),
        WorkflowImpactChange::PolicyScopeChanged { policy_scope }
        | WorkflowImpactChange::BudgetChanged { policy_scope } => {
            matches_any(policy_scope, &summary.policy_scopes)
                .then(|| target_reason("policy or budget changed", policy_scope))
        }
        WorkflowImpactChange::ApprovalRuleChanged { approval_gate } => {
            matches_any(approval_gate, &summary.approval_gates)
                .then(|| target_reason("approval rule changed", approval_gate))
        }
        WorkflowImpactChange::WorkflowTemplateChanged { .. } => {
            Some("workflow template changed".to_string())
        }
    }
}

fn matches_any(target: &Option<String>, candidates: &[String]) -> bool {
    target
        .as_ref()
        .is_none_or(|target| candidates.iter().any(|candidate| candidate == target))
}

fn target_reason(prefix: &str, target: &Option<String>) -> String {
    target
        .as_ref()
        .map(|target| format!("{prefix}: `{target}`"))
        .unwrap_or_else(|| prefix.to_string())
}

fn downstream_closure<'a>(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    seeds: impl Iterator<Item = &'a String>,
) -> BTreeSet<String> {
    let mut impacted = seeds.cloned().collect::<BTreeSet<_>>();
    let mut changed = true;
    while changed {
        changed = false;
        for (step_id, summary) in dependencies {
            if impacted.contains(step_id) {
                continue;
            }
            if summary
                .depends_on
                .iter()
                .any(|upstream| impacted.contains(upstream))
            {
                impacted.insert(step_id.clone());
                changed = true;
            }
        }
    }
    impacted
}

fn affected_steps(
    dependencies: &[(String, WorkflowStepDependencySummary)],
    direct: &BTreeMap<String, Vec<String>>,
    impacted: &BTreeSet<String>,
) -> Vec<WorkflowImpactStep> {
    dependencies
        .iter()
        .filter(|(step_id, _)| impacted.contains(step_id))
        .map(|(step_id, summary)| {
            let direct_reasons = direct.get(step_id).cloned().unwrap_or_default();
            let direct = !direct_reasons.is_empty();
            WorkflowImpactStep {
                step_id: step_id.clone(),
                direct,
                reasons: if direct {
                    direct_reasons
                } else {
                    vec!["downstream of impacted workflow dependency".to_string()]
                },
                required_tools: summary.required_tools.clone(),
                memory_tiers: summary.memory_tiers.clone(),
                policy_scopes: summary.policy_scopes.clone(),
                approval_gates: summary.approval_gates.clone(),
                checks_to_run: checks_for_step(summary),
            }
        })
        .collect()
}

fn checks_for_step(summary: &WorkflowStepDependencySummary) -> Vec<String> {
    let mut checks = BTreeSet::from([
        "workflow_preflight".to_string(),
        "workflow_runtime_plan".to_string(),
        "workflow_rerun_plan".to_string(),
    ]);
    if !summary.required_tools.is_empty() {
        checks.insert("tool_schema_contract".to_string());
    }
    if !summary.memory_tiers.is_empty() {
        checks.insert("memory_governance".to_string());
    }
    if !summary.policy_scopes.is_empty() || !summary.approval_gates.is_empty() {
        checks.insert("policy_governance".to_string());
    }
    checks.into_iter().collect()
}

fn risk_groups(
    affected_steps: &[WorkflowImpactStep],
    hints: &[WorkflowImpactRiskHint],
) -> Vec<WorkflowImpactRiskGroup> {
    let mut groups = BTreeMap::<(String, String), (BTreeSet<String>, BTreeSet<String>)>::new();
    for step in affected_steps {
        for hint in risk_hints_for_step(step, hints) {
            let key = (hint.authority_level, hint.side_effect_boundary);
            let entry = groups.entry(key).or_default();
            entry.0.insert(step.step_id.clone());
            entry.1.extend(step.checks_to_run.iter().cloned());
            entry.1.extend(hint.checks_to_run);
        }
    }
    groups
        .into_iter()
        .map(
            |((authority_level, side_effect_boundary), (steps, checks))| WorkflowImpactRiskGroup {
                authority_level,
                side_effect_boundary,
                affected_steps: steps.into_iter().collect(),
                checks_to_run: checks.into_iter().collect(),
            },
        )
        .collect()
}

fn risk_hints_for_step(
    step: &WorkflowImpactStep,
    hints: &[WorkflowImpactRiskHint],
) -> Vec<WorkflowImpactRiskHint> {
    let mut matches = step
        .required_tools
        .iter()
        .chain(step.memory_tiers.iter())
        .chain(step.policy_scopes.iter())
        .chain(step.approval_gates.iter())
        .filter_map(|target| hints.iter().find(|hint| &hint.target == target).cloned())
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches.push(default_risk_hint_for_step(step));
    }
    matches
}

fn default_risk_hint_for_step(step: &WorkflowImpactStep) -> WorkflowImpactRiskHint {
    WorkflowImpactRiskHint {
        target: step.step_id.clone(),
        authority_level: if step.approval_gates.is_empty() {
            "standard".to_string()
        } else {
            "elevated".to_string()
        },
        side_effect_boundary: if step.required_tools.is_empty() {
            "read_only".to_string()
        } else {
            "tool_execution".to_string()
        },
        checks_to_run: Vec::new(),
    }
}

fn collect_report_checks(
    affected_steps: &[WorkflowImpactStep],
    risk_groups: &[WorkflowImpactRiskGroup],
) -> Vec<String> {
    let mut checks = BTreeSet::new();
    for step in affected_steps {
        checks.extend(step.checks_to_run.iter().cloned());
    }
    for group in risk_groups {
        checks.extend(group.checks_to_run.iter().cloned());
    }
    checks.into_iter().collect()
}

fn affected_workflows(graph: &WorkflowGraph, affected: bool) -> Vec<WorkflowImpactWorkflow> {
    if !affected {
        return Vec::new();
    }
    vec![WorkflowImpactWorkflow {
        workflow_template_id: graph
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::WorkflowTemplate)
            .map(|node| {
                node.payload
                    .get("template_id")
                    .cloned()
                    .unwrap_or_else(|| node.id.key.clone())
            }),
        workflow_version_id: graph
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::WorkflowVersion)
            .map(|node| node.label.clone()),
        reason: "one or more workflow steps depend on changed graph context".to_string(),
    }]
}

fn dependencies_for_step<'a>(
    dependencies: &'a [(String, WorkflowStepDependencySummary)],
    step_id: &str,
) -> Option<&'a WorkflowStepDependencySummary> {
    dependencies
        .iter()
        .find_map(|(candidate, summary)| (candidate == step_id).then_some(summary))
}
