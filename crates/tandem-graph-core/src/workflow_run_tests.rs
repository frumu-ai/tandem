use crate::{
    EdgeKind, GraphAuditEventType, GraphDomain, GraphQueryEnvelope, GraphScope,
    GraphStoragePartition, NodeKind, RunTraceEvent, RunTraceEventKind, RunTraceGraph,
    RunTraceGraphSpec, WorkflowBlockerKind, WorkflowGraph, WorkflowGraphSpec, WorkflowRuntimeState,
    WorkflowStepGraphNode, WorkflowTemplateGraphNode, WorkflowVersionGraphNode,
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
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::ObservedIn
            && edge.target.kind == NodeKind::WorkflowVersion.stable_id()
            && edge.target.scope.run_id.is_none()
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::ObservedIn
            && edge.target.kind == NodeKind::WorkflowStep.stable_id()
            && edge.target.scope.run_id.is_none()
    }));
    assert_eq!(
        graph.audit_event.event_type,
        GraphAuditEventType::RunTraceCaptured
    );
    assert_eq!(graph.audit_event.run_id.as_deref(), Some("run-a"));
    assert_eq!(graph.audit_event.metrics.denied, 0);
    assert!(!graph.audit_event.safe_details.contains_key("token"));
}

#[test]
fn workflow_preflight_blocks_missing_approval_and_denied_memory() {
    let graph = runtime_query_workflow();
    let mut envelope = runtime_envelope();
    envelope.allowed_tools = vec!["web.search".to_string(), "slack.send".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string()];

    let output = graph.workflow_preflight(&envelope);

    assert!(!output.value.allowed);
    assert!(output.audit.denied_count > 0);
    assert!(output.value.blockers.iter().any(|blocker| {
        blocker.step_id == "publish" && blocker.kind == WorkflowBlockerKind::ApprovalMissing
    }));
    assert!(output.value.blockers.iter().any(|blocker| {
        blocker.step_id == "publish" && blocker.kind == WorkflowBlockerKind::MemoryDenied
    }));
}

#[test]
fn workflow_preflight_denies_envelopes_outside_workflow_partition() {
    let graph = runtime_query_workflow();
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-b"), "agent-a");
    envelope.readable_paths = vec![".".to_string()];
    envelope.allowed_tools = vec!["web.search".to_string(), "slack.send".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string(), "private".to_string()];
    envelope.approvals = vec!["human-review".to_string()];

    let output = graph.workflow_preflight(&envelope);

    assert!(!output.value.allowed);
    assert!(output.value.blockers.iter().any(|blocker| {
        blocker.step_id.is_empty() && blocker.kind == WorkflowBlockerKind::ScopeMismatch
    }));
}

#[test]
fn workflow_tool_selection_prunes_denied_tools_before_prompting() {
    let graph = runtime_query_workflow();
    let mut envelope = runtime_envelope();
    envelope.allowed_tools = vec!["web.search".to_string()];

    let output = graph.workflow_tool_selection(&envelope, None);

    assert_eq!(output.value.metrics.candidate_tools, 2);
    assert_eq!(output.value.metrics.selected_tools, 1);
    assert_eq!(output.value.metrics.denied_tools, 1);
    assert_eq!(output.value.metrics.pruned_tools, 1);
    assert!(output
        .value
        .candidates
        .iter()
        .any(|tool| tool.tool_name == "web.search" && tool.selected));
    assert!(output
        .value
        .candidates
        .iter()
        .any(|tool| tool.tool_name == "slack.send" && !tool.selected));
}

#[test]
fn workflow_tool_selection_fails_closed_for_invalid_envelope() {
    let graph = runtime_query_workflow();
    let envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "");

    let output = graph.workflow_tool_selection(&envelope, None);

    assert!(output.value.candidates.is_empty());
    assert_eq!(output.value.metrics.candidate_tools, 0);
    assert!(output.audit.denied_count > 0);
}

#[test]
fn workflow_runtime_plan_returns_ready_blocked_and_critical_path() {
    let graph = runtime_query_workflow();
    let mut envelope = runtime_envelope();
    envelope.allowed_tools = vec!["web.search".to_string(), "slack.send".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string(), "private".to_string()];
    envelope.approvals = vec!["human-review".to_string()];
    let state = WorkflowRuntimeState::new().with_completed_steps(["collect"]);

    let output = graph.workflow_runtime_plan(&state, &envelope);

    assert_eq!(
        output
            .value
            .ready_nodes
            .iter()
            .map(|node| node.step_id.as_str())
            .collect::<Vec<_>>(),
        vec!["publish"]
    );
    assert!(output.value.blocked_nodes.is_empty());
    assert_eq!(output.value.parallel_groups.len(), 2);
    assert_eq!(
        output.value.critical_path,
        vec!["collect".to_string(), "publish".to_string()]
    );
}

#[test]
fn workflow_runtime_plan_blocks_invalid_envelope_instead_of_scheduling_roots() {
    let graph = runtime_query_workflow();
    let envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "");
    let state = WorkflowRuntimeState::new();

    let output = graph.workflow_runtime_plan(&state, &envelope);

    assert!(output.value.ready_nodes.is_empty());
    let collect = output
        .value
        .blocked_nodes
        .iter()
        .find(|node| node.step_id == "collect")
        .expect("collect should be blocked by invalid envelope");
    assert!(collect
        .blockers
        .iter()
        .any(|blocker| blocker.kind == WorkflowBlockerKind::EnvelopeInvalid));
}

#[test]
fn workflow_runtime_plan_explains_dependency_and_policy_blockers() {
    let graph = runtime_query_workflow();
    let mut envelope = runtime_envelope();
    envelope.allowed_tools = vec!["web.search".to_string()];
    envelope.allowed_memory_tiers = vec!["project".to_string()];
    let state = WorkflowRuntimeState::new();

    let output = graph.workflow_runtime_plan(&state, &envelope);

    assert!(output
        .value
        .ready_nodes
        .iter()
        .any(|node| node.step_id == "collect"));
    let publish = output
        .value
        .blocked_nodes
        .iter()
        .find(|node| node.step_id == "publish")
        .expect("publish blocked");
    assert!(publish
        .blockers
        .iter()
        .any(|blocker| blocker.kind == WorkflowBlockerKind::DependencyPending));
    assert!(publish
        .blockers
        .iter()
        .any(|blocker| blocker.kind == WorkflowBlockerKind::ToolDenied));
    assert!(publish
        .blockers
        .iter()
        .any(|blocker| blocker.kind == WorkflowBlockerKind::ApprovalMissing));
}

fn runtime_query_workflow() -> WorkflowGraph {
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

fn runtime_envelope() -> GraphQueryEnvelope {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "agent-a");
    envelope.readable_paths = vec![".".to_string()];
    envelope
}
