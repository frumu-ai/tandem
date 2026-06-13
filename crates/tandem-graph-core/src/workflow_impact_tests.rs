use crate::{
    GraphQueryEnvelope, GraphScope, WorkflowGraph, WorkflowGraphSpec, WorkflowImpactChange,
    WorkflowImpactQuery, WorkflowImpactRiskHint, WorkflowStepGraphNode, WorkflowTemplateGraphNode,
    WorkflowVersionGraphNode,
};

#[test]
fn workflow_impact_analysis_propagates_tool_changes_downstream() {
    let graph = workflow_graph();
    let output = graph.workflow_impact_analysis(
        &envelope(),
        WorkflowImpactQuery {
            changes: vec![WorkflowImpactChange::ToolSchemaChanged {
                tool_name: Some("web.search".to_string()),
            }],
            risk_hints: vec![WorkflowImpactRiskHint {
                target: "web.search".to_string(),
                authority_level: "read".to_string(),
                side_effect_boundary: "external_network".to_string(),
                checks_to_run: vec!["tool_regression".to_string()],
            }],
        },
    );

    assert!(output.audit.allowed());
    assert_eq!(output.value.affected_workflows.len(), 1);
    assert_eq!(
        output
            .value
            .affected_steps
            .iter()
            .map(|step| (step.step_id.as_str(), step.direct))
            .collect::<Vec<_>>(),
        vec![("collect", true), ("publish", false)]
    );
    assert!(output
        .value
        .affected_steps
        .iter()
        .find(|step| step.step_id == "publish")
        .expect("publish impact")
        .reasons
        .iter()
        .any(|reason| reason.contains("downstream")));
    let hinted_group = output
        .value
        .risk_groups
        .iter()
        .find(|group| {
            group.authority_level == "read" && group.side_effect_boundary == "external_network"
        })
        .expect("hinted risk group");
    assert_eq!(hinted_group.affected_steps, vec!["collect"]);
    assert!(output
        .value
        .checks_to_run
        .iter()
        .any(|check| check == "tool_regression"));
}

#[test]
fn workflow_impact_analysis_groups_policy_and_approval_risk() {
    let graph = workflow_graph();
    let output = graph.workflow_impact_analysis(
        &envelope(),
        WorkflowImpactQuery {
            changes: vec![WorkflowImpactChange::ApprovalRuleChanged {
                approval_gate: Some("human-review".to_string()),
            }],
            risk_hints: vec![WorkflowImpactRiskHint {
                target: "human-review".to_string(),
                authority_level: "elevated".to_string(),
                side_effect_boundary: "human_approval".to_string(),
                checks_to_run: vec!["approval_policy_review".to_string()],
            }],
        },
    );

    assert!(output.audit.allowed());
    assert_eq!(
        output
            .value
            .affected_steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>(),
        vec!["publish"]
    );
    assert_eq!(output.value.risk_groups[0].authority_level, "elevated");
    assert!(output
        .value
        .risk_groups
        .iter()
        .flat_map(|group| group.checks_to_run.iter())
        .any(|check| check == "approval_policy_review"));
}

#[test]
fn workflow_impact_analysis_filters_steps_outside_query_governance() {
    let graph = workflow_graph();
    let mut envelope = envelope();
    envelope.allowed_tools = vec!["slack.send".to_string()];

    let output = graph.workflow_impact_analysis(
        &envelope,
        WorkflowImpactQuery {
            changes: vec![WorkflowImpactChange::ToolSchemaChanged {
                tool_name: Some("web.search".to_string()),
            }],
            risk_hints: vec![],
        },
    );

    assert!(output.value.affected_steps.is_empty());
    assert!(output.audit.denied_count > 0);
    assert!(output
        .audit
        .denied_reasons
        .iter()
        .any(|reason| reason.contains("outside the query envelope")));
}

fn workflow_graph() -> WorkflowGraph {
    WorkflowGraph::from_spec(WorkflowGraphSpec {
        scope: GraphScope::new("tenant-a", "project-a"),
        template: WorkflowTemplateGraphNode {
            template_id: "template-a".to_string(),
            name: "Notify operators".to_string(),
            owner_id: "owner-a".to_string(),
            template_hash: None,
        },
        version: WorkflowVersionGraphNode {
            version_id: "version-a".to_string(),
            workflow_hash: "workflow-hash".to_string(),
            policy_hash: Some("policy-hash".to_string()),
            prompt_hash: Some("prompt-hash".to_string()),
            tool_schema_hash: Some("tool-schema-hash".to_string()),
        },
        steps: vec![
            WorkflowStepGraphNode {
                step_id: "collect".to_string(),
                title: "Collect evidence".to_string(),
                kind: "research".to_string(),
                depends_on: vec![],
                required_tools: vec!["web.search".to_string()],
                memory_tiers: vec!["project".to_string()],
                approval_gates: vec![],
                policy_scopes: vec!["policy:research".to_string()],
                artifact_refs: vec!["artifact://brief".to_string()],
            },
            WorkflowStepGraphNode {
                step_id: "publish".to_string(),
                title: "Send operator update".to_string(),
                kind: "notification".to_string(),
                depends_on: vec!["collect".to_string()],
                required_tools: vec!["slack.send".to_string()],
                memory_tiers: vec!["private".to_string()],
                approval_gates: vec!["human-review".to_string()],
                policy_scopes: vec!["policy:external-send".to_string()],
                artifact_refs: vec!["artifact://brief".to_string()],
            },
        ],
    })
    .expect("build workflow graph")
}

fn envelope() -> GraphQueryEnvelope {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "agent-a");
    envelope.readable_paths = vec![".".to_string()];
    envelope.allowed_tools = vec!["web.search".to_string(), "slack.send".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string(), "private".to_string()];
    envelope.approvals = vec!["human-review".to_string()];
    envelope
}
