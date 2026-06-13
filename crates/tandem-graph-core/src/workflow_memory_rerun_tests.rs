use crate::{
    Freshness, FreshnessSource, GraphQueryEnvelope, GraphScope, Provenance, WorkflowGraph,
    WorkflowGraphSpec, WorkflowMemoryCandidate, WorkflowMemoryQuery, WorkflowRerunChange,
    WorkflowStepCacheKey, WorkflowStepGraphNode, WorkflowTemplateGraphNode,
    WorkflowVersionGraphNode,
};

#[test]
fn workflow_memory_bundle_filters_by_scope_policy_tier_and_freshness() {
    let graph = workflow_graph();
    let mut envelope = envelope();
    envelope.allowed_memory_tiers = vec!["private".to_string()];
    let output = graph.workflow_memory_bundle(
        &envelope,
        WorkflowMemoryQuery {
            step_id: "publish".to_string(),
            step_kind: Some("notification".to_string()),
            now_unix_ms: Some(200),
            include_stale: false,
        },
        &[
            memory("same-step", "private", "policy:external-send").without_graph_links(),
            memory("cross-project", "private", "policy:external-send")
                .with_scope(GraphScope::new("tenant-a", "project-b")),
            memory("stale", "private", "policy:external-send").with_stale_after(100),
            memory("wrong-policy", "private", "policy:research").without_graph_links(),
            memory("wrong-tier", "project", "policy:external-send"),
        ],
    );

    assert!(output.value.blockers.is_empty());
    assert_eq!(output.value.memories.len(), 1);
    assert_eq!(output.value.memories[0].memory_id, "same-step");
    assert!(output.value.memories[0]
        .reason
        .contains("policy scope required"));
    assert!(output.audit.denied_count >= 2);
}

#[test]
fn workflow_memory_bundle_falls_back_when_graph_has_no_memory_link() {
    let graph = workflow_graph();
    let mut envelope = envelope();
    envelope.allowed_memory_tiers = vec!["private".to_string()];

    let output = graph.workflow_memory_bundle(
        &envelope,
        WorkflowMemoryQuery {
            step_id: "publish".to_string(),
            step_kind: Some("notification".to_string()),
            now_unix_ms: None,
            include_stale: false,
        },
        &[memory("unrelated", "private", "policy:unrelated").without_graph_links()],
    );

    assert!(output.value.memories.is_empty());
    assert!(output.value.fallback_to_semantic_search);
}

#[test]
fn workflow_memory_bundle_rejects_memories_from_other_runs() {
    let graph = workflow_graph();
    let mut envelope = envelope();
    envelope.scope = envelope.scope.with_run("run-a");
    envelope.run_id = Some("run-a".to_string());
    envelope.allowed_memory_tiers = vec!["private".to_string()];

    let output = graph.workflow_memory_bundle(
        &envelope,
        WorkflowMemoryQuery {
            step_id: "publish".to_string(),
            step_kind: Some("notification".to_string()),
            now_unix_ms: None,
            include_stale: false,
        },
        &[
            memory("same-run", "private", "policy:external-send")
                .with_scope(GraphScope::new("tenant-a", "project-a").with_run("run-a")),
            memory("other-run", "private", "policy:external-send")
                .with_scope(GraphScope::new("tenant-a", "project-a").with_run("run-b")),
        ],
    );

    assert!(output.value.blockers.is_empty());
    assert_eq!(
        output
            .value
            .memories
            .iter()
            .map(|memory| memory.memory_id.as_str())
            .collect::<Vec<_>>(),
        vec!["same-run"]
    );
    assert!(output
        .audit
        .denied_reasons
        .iter()
        .any(|reason| reason.contains("outside the query run scope")));
}

#[test]
fn workflow_memory_bundle_preserves_memory_tier_governance() {
    let graph = workflow_graph();
    let mut envelope = envelope();
    envelope.allowed_memory_tiers = vec!["project".to_string()];
    let output = graph.workflow_memory_bundle(
        &envelope,
        WorkflowMemoryQuery {
            step_id: "publish".to_string(),
            step_kind: Some("notification".to_string()),
            now_unix_ms: None,
            include_stale: false,
        },
        &[memory("same-step", "private", "policy:external-send")],
    );

    assert!(output.value.memories.is_empty());
    assert!(output.value.fallback_to_semantic_search);
    assert!(output.audit.denied_count > 0);
}

#[test]
fn workflow_rerun_plan_marks_failed_step_and_downstream_dirty() {
    let graph = workflow_graph();
    let output = graph.workflow_rerun_plan(
        &envelope(),
        &[WorkflowRerunChange::StepFailed {
            step_id: "collect".to_string(),
        }],
        &[cache_key("collect"), cache_key("publish")],
    );

    assert_eq!(
        output
            .value
            .dirty_steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>(),
        vec!["collect", "publish"]
    );
    assert!(output.value.reusable_steps.is_empty());
    assert!(output
        .value
        .dirty_steps
        .iter()
        .all(|step| step.cache_key.is_some()));
}

#[test]
fn workflow_rerun_plan_keeps_unchanged_upstream_reusable() {
    let graph = workflow_graph();
    let output = graph.workflow_rerun_plan(
        &envelope(),
        &[WorkflowRerunChange::PromptHashChanged {
            step_id: Some("publish".to_string()),
            old_hash: "old-prompt".to_string(),
            new_hash: "new-prompt".to_string(),
        }],
        &[cache_key("collect"), cache_key("publish")],
    );

    assert_eq!(output.value.dirty_steps[0].step_id, "publish");
    assert_eq!(output.value.reusable_steps, vec!["collect"]);
}

#[test]
fn workflow_rerun_plan_targets_policy_tool_and_memory_changes() {
    let graph = workflow_graph();

    let policy = graph.workflow_rerun_plan(
        &envelope(),
        &[WorkflowRerunChange::PolicyHashChanged {
            policy_scope: Some("policy:research".to_string()),
            old_hash: "old-policy".to_string(),
            new_hash: "new-policy".to_string(),
        }],
        &[],
    );
    assert_eq!(
        policy
            .value
            .dirty_steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>(),
        vec!["collect", "publish"]
    );

    let tool = graph.workflow_rerun_plan(
        &envelope(),
        &[WorkflowRerunChange::ToolSchemaChanged {
            tool_name: Some("slack.send".to_string()),
            old_hash: "old-tool".to_string(),
            new_hash: "new-tool".to_string(),
        }],
        &[],
    );
    assert_eq!(tool.value.dirty_steps[0].step_id, "publish");

    let memory = graph.workflow_rerun_plan(
        &envelope(),
        &[WorkflowRerunChange::MemorySnapshotChanged {
            tier: Some("private".to_string()),
            old_hash: "old-memory".to_string(),
            new_hash: "new-memory".to_string(),
        }],
        &[],
    );
    assert_eq!(memory.value.dirty_steps[0].step_id, "publish");
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

fn memory(id: &str, tier: &str, policy_scope: &str) -> WorkflowMemoryCandidate {
    WorkflowMemoryCandidate {
        memory_id: id.to_string(),
        collection_id: format!("collection-{tier}"),
        tier: tier.to_string(),
        policy_scope: Some(policy_scope.to_string()),
        workflow_template_id: Some("template-a".to_string()),
        workflow_step_id: None,
        step_kind: Some("notification".to_string()),
        artifact_refs: vec!["artifact://brief".to_string()],
        scope: GraphScope::new("tenant-a", "project-a"),
        summary: format!("memory {id}"),
        provenance: Provenance::Observed,
        freshness: Freshness::from_revision(FreshnessSource::MemorySnapshot, "memory-snapshot"),
        score: Some("0.9".to_string()),
    }
}

trait MemoryCandidateExt {
    fn with_scope(self, scope: GraphScope) -> Self;
    fn with_stale_after(self, stale_after_unix_ms: u64) -> Self;
    fn without_graph_links(self) -> Self;
}

impl MemoryCandidateExt for WorkflowMemoryCandidate {
    fn with_scope(mut self, scope: GraphScope) -> Self {
        self.scope = scope;
        self
    }

    fn with_stale_after(mut self, stale_after_unix_ms: u64) -> Self {
        self.freshness = self.freshness.with_stale_after(stale_after_unix_ms);
        self
    }

    fn without_graph_links(mut self) -> Self {
        self.workflow_template_id = None;
        self.workflow_step_id = None;
        self.step_kind = None;
        self.artifact_refs.clear();
        self
    }
}

fn cache_key(step_id: &str) -> WorkflowStepCacheKey {
    WorkflowStepCacheKey {
        step_id: step_id.to_string(),
        input_hash: "input".to_string(),
        tool_schema_hash: "tool-schema".to_string(),
        policy_hash: "policy".to_string(),
        memory_snapshot_hash: "memory".to_string(),
        model_id: "model".to_string(),
        prompt_hash: "prompt".to_string(),
    }
}
