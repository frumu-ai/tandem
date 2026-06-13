use crate::{
    GraphQueryEnvelope, GraphScope, WorkflowApprovalPosture, WorkflowExecutionHintsQuery,
    WorkflowGraph, WorkflowGraphSpec, WorkflowModelTier, WorkflowRiskTier,
    WorkflowStepFailureHistory, WorkflowStepGraphNode, WorkflowTemplateGraphNode,
    WorkflowToolRiskHint, WorkflowVersionGraphNode,
};

#[test]
fn workflow_execution_hints_raise_review_for_external_side_effects() {
    let graph = execution_hints_workflow();
    let output = graph.workflow_execution_hints(
        &execution_hints_envelope(),
        WorkflowExecutionHintsQuery {
            tool_risk_hints: vec![WorkflowToolRiskHint {
                tool_name: "slack.send".to_string(),
                authority_level: "admin".to_string(),
                side_effects: true,
                data_classes: vec!["pii".to_string()],
                approval_required: true,
            }],
            failure_history: vec![],
            default_budget_tokens: Some(5_000),
        },
    );

    assert!(output.audit.allowed());
    let publish = output
        .value
        .step_hints
        .iter()
        .find(|hint| hint.step_id == "publish")
        .expect("publish step hint");
    assert_eq!(publish.risk_tier, WorkflowRiskTier::High);
    assert_eq!(publish.model_tier, WorkflowModelTier::Large);
    assert_eq!(
        publish.approval_posture,
        WorkflowApprovalPosture::StrongReview
    );
    assert_eq!(publish.max_retries, 1);
    assert!(publish.budget_tokens > 5_000);
    assert_eq!(output.value.metrics.high_risk_steps, 1);
    assert_eq!(output.value.metrics.approval_required_steps, 1);
    assert!(
        output.value.metrics.recommended_budget_tokens
            > output.value.metrics.baseline_budget_tokens
    );
}

#[test]
fn workflow_execution_hints_use_failure_history_for_routing_and_metrics() {
    let graph = low_risk_workflow();
    let output = graph.workflow_execution_hints(
        &low_risk_envelope(),
        WorkflowExecutionHintsQuery {
            failure_history: vec![WorkflowStepFailureHistory {
                step_id: "draft".to_string(),
                failure_count: 2,
                recent_failure_rate_bps: Some(1_200),
                last_failure_kind: Some("tool_timeout".to_string()),
            }],
            default_budget_tokens: Some(4_000),
            ..WorkflowExecutionHintsQuery::default()
        },
    );

    let draft = output
        .value
        .step_hints
        .iter()
        .find(|hint| hint.step_id == "draft")
        .expect("draft step hint");
    assert_eq!(draft.risk_tier, WorkflowRiskTier::Medium);
    assert_eq!(draft.model_tier, WorkflowModelTier::Standard);
    assert_eq!(draft.max_retries, 2);
    assert_eq!(output.value.metrics.historical_failure_steps, 1);
    assert_eq!(
        output.value.metrics.historical_failure_rate_bps,
        Some(1_200)
    );
    assert!(output.value.metrics.graph_guided_failure_rate_bps < Some(1_200));
}

#[test]
fn workflow_execution_hints_omit_steps_outside_the_query_envelope() {
    let graph = execution_hints_workflow();
    let mut envelope = execution_hints_envelope();
    envelope.allowed_tools = vec!["web.search".to_string()];

    let output = graph.workflow_execution_hints(&envelope, WorkflowExecutionHintsQuery::default());

    assert!(output.audit.denied_count > 0);
    assert!(output
        .audit
        .denied_reasons
        .iter()
        .any(|reason| reason.contains("slack.send")));
    assert!(output
        .value
        .step_hints
        .iter()
        .any(|hint| hint.step_id == "collect"));
    assert!(!output
        .value
        .step_hints
        .iter()
        .any(|hint| hint.step_id == "publish"));
}

fn execution_hints_workflow() -> WorkflowGraph {
    WorkflowGraph::from_spec(WorkflowGraphSpec {
        scope: GraphScope::new("tenant-a", "project-a"),
        template: WorkflowTemplateGraphNode {
            template_id: "template-a".to_string(),
            name: "Research and publish".to_string(),
            owner_id: "owner-a".to_string(),
            template_hash: Some("template-hash".to_string()),
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
                artifact_refs: vec![],
            },
            WorkflowStepGraphNode {
                step_id: "publish".to_string(),
                title: "Publish update".to_string(),
                kind: "external_publish".to_string(),
                depends_on: vec!["collect".to_string()],
                required_tools: vec!["slack.send".to_string()],
                memory_tiers: vec!["private".to_string()],
                approval_gates: vec!["human-review".to_string()],
                policy_scopes: vec!["policy:external-send".to_string()],
                artifact_refs: vec![],
            },
        ],
    })
    .expect("build workflow graph")
}

fn low_risk_workflow() -> WorkflowGraph {
    WorkflowGraph::from_spec(WorkflowGraphSpec {
        scope: GraphScope::new("tenant-a", "project-a"),
        template: WorkflowTemplateGraphNode {
            template_id: "template-a".to_string(),
            name: "Draft note".to_string(),
            owner_id: "owner-a".to_string(),
            template_hash: None,
        },
        version: WorkflowVersionGraphNode {
            version_id: "version-a".to_string(),
            workflow_hash: "workflow-hash".to_string(),
            policy_hash: None,
            prompt_hash: None,
            tool_schema_hash: None,
        },
        steps: vec![WorkflowStepGraphNode {
            step_id: "draft".to_string(),
            title: "Draft locally".to_string(),
            kind: "draft".to_string(),
            depends_on: vec![],
            required_tools: vec![],
            memory_tiers: vec![],
            approval_gates: vec![],
            policy_scopes: vec![],
            artifact_refs: vec![],
        }],
    })
    .expect("build workflow graph")
}

fn execution_hints_envelope() -> GraphQueryEnvelope {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "agent-a");
    envelope.readable_paths = vec![".".to_string()];
    envelope.allowed_tools = vec!["web.search".to_string(), "slack.send".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string(), "private".to_string()];
    envelope.approvals = vec!["human-review".to_string()];
    envelope
}

fn low_risk_envelope() -> GraphQueryEnvelope {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "agent-a");
    envelope.readable_paths = vec![".".to_string()];
    envelope
}
