use crate::{
    GraphQueryEnvelope, GraphScope, RunFailureCauseKind, RunTraceEvent, RunTraceEventKind,
    RunTraceGraph, RunTraceGraphSpec, WorkflowGraph, WorkflowGraphSpec, WorkflowStepGraphNode,
    WorkflowTemplateGraphNode, WorkflowVersionGraphNode,
};

#[test]
fn run_failure_causality_report_distinguishes_root_and_cascading_failures() {
    let workflow = workflow_graph();
    let trace = RunTraceGraph::from_spec(
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
                    artifact_ref: None,
                    safe_summary: Some("tool failed with a display-safe timeout".to_string()),
                    policy_denied: false,
                    latency_ms: Some(5000),
                    cost_microunits: None,
                    occurred_at_unix_ms: Some(10),
                },
                RunTraceEvent {
                    event_id: "error-2".to_string(),
                    kind: RunTraceEventKind::Error,
                    workflow_step_id: Some("publish".to_string()),
                    tool_name: None,
                    memory_tier: None,
                    policy_scope: None,
                    artifact_ref: Some("artifact://brief".to_string()),
                    safe_summary: Some(
                        "publish failed because collect output was missing".to_string(),
                    ),
                    policy_denied: false,
                    latency_ms: None,
                    cost_microunits: None,
                    occurred_at_unix_ms: Some(20),
                },
            ],
        },
        "agent-a",
    )
    .expect("build run trace graph");
    let mut envelope = GraphQueryEnvelope::new(
        GraphScope::new("tenant-a", "project-a").with_run("run-a"),
        "agent-a",
    );
    envelope.readable_paths = vec![".".to_string()];

    let output = trace.failure_causality_report(&envelope, Some(&workflow));

    assert!(output.audit.allowed());
    assert_eq!(output.value.run_id, "run-a");
    assert_eq!(output.value.root_causes.len(), 1);
    assert_eq!(
        output.value.root_causes[0].kind,
        RunFailureCauseKind::ToolFailure
    );
    assert_eq!(
        output.value.root_causes[0].target.as_deref(),
        Some("web.search")
    );
    assert_eq!(output.value.cascading_failures.len(), 1);
    assert_eq!(
        output.value.cascading_failures[0].upstream_root_steps,
        vec!["collect".to_string()]
    );
    assert_eq!(
        output.value.repair_context.related_steps,
        vec!["collect".to_string(), "publish".to_string()]
    );
    assert_eq!(
        output.value.repair_context.relevant_tools,
        vec!["web.search".to_string()]
    );
    assert!(output
        .value
        .repair_context
        .evidence
        .iter()
        .all(|evidence| !evidence.safe_summary.contains("raw")));
}

#[test]
fn run_failure_causality_report_fails_closed_for_run_scope_mismatch() {
    let trace = RunTraceGraph::from_spec(
        RunTraceGraphSpec {
            scope: GraphScope::new("tenant-a", "project-a"),
            run_id: "run-a".to_string(),
            workflow_version_id: None,
            events: vec![RunTraceEvent {
                event_id: "error-1".to_string(),
                kind: RunTraceEventKind::Error,
                workflow_step_id: Some("collect".to_string()),
                tool_name: None,
                memory_tier: None,
                policy_scope: None,
                artifact_ref: None,
                safe_summary: Some("failed safely".to_string()),
                policy_denied: false,
                latency_ms: None,
                cost_microunits: None,
                occurred_at_unix_ms: None,
            }],
        },
        "agent-a",
    )
    .expect("build run trace graph");
    let mut envelope = GraphQueryEnvelope::new(
        GraphScope::new("tenant-a", "project-a").with_run("run-b"),
        "agent-a",
    );
    envelope.readable_paths = vec![".".to_string()];

    let output = trace.failure_causality_report(&envelope, None);

    assert!(!output.audit.allowed());
    assert!(output.value.root_causes.is_empty());
    assert!(output
        .audit
        .denied_reasons
        .iter()
        .any(|reason| reason.contains("run trace partition")));
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
                artifact_refs: vec![],
            },
        ],
    })
    .expect("build workflow graph")
}
