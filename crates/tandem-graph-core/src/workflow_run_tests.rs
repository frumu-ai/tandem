use crate::{
    EdgeKind, GraphAuditEventType, GraphDomain, GraphScope, GraphStoragePartition, NodeKind,
    RunTraceEvent, RunTraceEventKind, RunTraceGraph, RunTraceGraphSpec, WorkflowGraph,
    WorkflowGraphSpec, WorkflowStepGraphNode, WorkflowTemplateGraphNode, WorkflowVersionGraphNode,
};

#[test]
fn workflow_spec_builds_dependency_dag_and_step_summary() {
    let graph = WorkflowGraph::from_spec(WorkflowGraphSpec {
        scope: GraphScope::new("tenant-a", "project-a"),
        template: WorkflowTemplateGraphNode {
            template_id: "template-a".to_string(),
            name: "Daily research".to_string(),
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
                artifact_refs: vec!["artifact://brief".to_string()],
            },
            WorkflowStepGraphNode {
                step_id: "summarize".to_string(),
                title: "Summarize".to_string(),
                kind: "synthesis".to_string(),
                depends_on: vec!["collect".to_string()],
                required_tools: vec!["docs.write".to_string()],
                memory_tiers: vec![],
                approval_gates: vec!["human-review".to_string()],
                policy_scopes: vec!["policy:publish".to_string()],
                artifact_refs: vec![],
            },
        ],
    })
    .expect("build workflow graph");

    assert_eq!(graph.partition.domain, GraphDomain::Workflow);
    assert!(graph
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::WorkflowVersion));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::DependsOn));

    let summary = graph
        .dependencies_for_step("summarize")
        .expect("step summary");
    assert_eq!(summary.depends_on, vec!["collect"]);
    assert_eq!(summary.required_tools, vec!["docs.write"]);
    assert_eq!(summary.approval_gates, vec!["human-review"]);
    assert_eq!(summary.policy_scopes, vec!["policy:publish"]);
}

#[test]
fn run_trace_spec_builds_redacted_run_graph_and_audit_marker() {
    let graph = RunTraceGraph::from_spec(
        RunTraceGraphSpec {
            scope: GraphScope::new("tenant-a", "project-a"),
            run_id: "run-a".to_string(),
            workflow_version_id: Some("version-a".to_string()),
            events: vec![
                RunTraceEvent {
                    event_id: "tool-1".to_string(),
                    kind: RunTraceEventKind::ToolCall,
                    workflow_step_id: Some("collect".to_string()),
                    tool_name: Some("web.search".to_string()),
                    memory_tier: None,
                    policy_scope: Some("policy:research".to_string()),
                    artifact_ref: Some("artifact://brief".to_string()),
                    safe_summary: Some("searched public docs".to_string()),
                    policy_denied: false,
                    latency_ms: Some(42),
                    cost_microunits: None,
                    occurred_at_unix_ms: Some(1_800_000_000_000),
                },
                RunTraceEvent {
                    event_id: "policy-1".to_string(),
                    kind: RunTraceEventKind::PolicyCheck,
                    workflow_step_id: Some("collect".to_string()),
                    tool_name: None,
                    memory_tier: None,
                    policy_scope: Some("policy:research".to_string()),
                    artifact_ref: None,
                    safe_summary: Some("allowed public read".to_string()),
                    policy_denied: false,
                    latency_ms: None,
                    cost_microunits: None,
                    occurred_at_unix_ms: None,
                },
            ],
        },
        "agent-a",
    )
    .expect("build run trace graph");

    assert_eq!(
        graph.partition.kind,
        GraphStoragePartition::run_ephemeral(
            GraphScope::new("tenant-a", "project-a").with_run("run-a"),
            graph.partition.retention.clone(),
        )
        .kind
    );
    assert!(graph.nodes.iter().all(|node| node.visibility.redacted));
    assert!(graph
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::ToolCall));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == EdgeKind::ObservedIn));
    assert_eq!(
        graph.audit_event.event_type,
        GraphAuditEventType::RunTraceCaptured
    );
    assert_eq!(graph.audit_event.run_id.as_deref(), Some("run-a"));
    assert_eq!(graph.audit_event.metrics.denied, 0);
    assert!(!graph.audit_event.safe_details.contains_key("token"));
}
