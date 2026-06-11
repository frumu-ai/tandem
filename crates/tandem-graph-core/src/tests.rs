use crate::{
    stable_graph_hash, EdgeKind, Freshness, FreshnessSource, GraphDomain, GraphFact,
    GraphQueryEnvelope, GraphScope, NodeKind, Provenance,
};
use serde_json::json;

#[test]
fn node_and_edge_kinds_have_stable_ids() {
    assert_eq!(NodeKind::File.stable_id(), "repo.file");
    assert_eq!(NodeKind::WorkflowStep.stable_id(), "workflow.step");
    assert_eq!(NodeKind::ToolDefinition.stable_id(), "tool.definition");
    assert_eq!(NodeKind::MemoryCollection.stable_id(), "memory.collection");
    assert_eq!(NodeKind::PolicyScope.stable_id(), "policy.scope");
    assert_eq!(NodeKind::ModelCall.stable_id(), "run.model_call");

    assert_eq!(EdgeKind::Defines.stable_id(), "defines");
    assert_eq!(EdgeKind::Documents.stable_id(), "documents");
    assert_eq!(EdgeKind::RequiresApproval.stable_id(), "requires_approval");
}

#[test]
fn graph_scope_carries_governance_boundaries() {
    let scope = GraphScope::new("tenant-a", "project-a")
        .with_repo("repo-a")
        .with_run("run-a");

    assert_eq!(scope.tenant_id, "tenant-a");
    assert_eq!(scope.project_id, "project-a");
    assert_eq!(scope.repo_id.as_deref(), Some("repo-a"));
    assert_eq!(scope.run_id.as_deref(), Some("run-a"));
}

#[test]
fn stable_hash_is_repeatable_for_graph_facts() {
    let mut fact = GraphFact::new(
        GraphScope::new("tenant-a", "project-a").with_repo("repo-a"),
        GraphDomain::Repo,
        "src/lib.rs",
        "RepoIndexSnapshot",
        EdgeKind::Defines,
        Provenance::Extracted,
    );
    fact.freshness = Freshness::from_revision(FreshnessSource::Commit, "abc123");
    fact.evidence.insert("line".to_string(), "42".to_string());

    let first = stable_graph_hash(&fact).expect("hash graph fact");
    let second = stable_graph_hash(&fact).expect("hash graph fact again");

    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
}

#[test]
fn graph_fact_serializes_stable_taxonomy_ids() {
    let fact = GraphFact::new(
        GraphScope::new("tenant-a", "project-a").with_repo("repo-a"),
        GraphDomain::Repo,
        "src/lib.rs",
        "RepoIndexSnapshot",
        EdgeKind::Defines,
        Provenance::Extracted,
    );

    let value = serde_json::to_value(&fact).expect("serialize graph fact");

    assert_eq!(value["domain"], json!("repo"));
    assert_eq!(value["edge_kind"], json!("defines"));
    assert_eq!(value["provenance"], json!("extracted"));
    assert_eq!(value["freshness"]["source"], json!("unknown"));
    assert_eq!(value["policy"], json!("allowed"));
    assert_eq!(
        serde_json::to_value(NodeKind::File).expect("serialize node kind"),
        json!("repo.file")
    );
}

#[test]
fn graph_query_envelope_requires_scope_and_actor() {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("", "project-a"), "");
    envelope.readable_paths.push("src".to_string());

    let error = envelope.validate().expect_err("missing scope is denied");

    assert_eq!(error.missing, vec!["tenant_id", "actor_id"]);
}

#[test]
fn graph_query_envelope_checks_tools_and_paths() {
    let mut envelope = GraphQueryEnvelope::new(GraphScope::new("tenant-a", "project-a"), "agent-a");
    envelope.readable_paths.push("src".to_string());
    envelope.allowed_tools.push("repo.search".to_string());

    assert!(envelope.validate().is_ok());
    assert!(envelope.allows_tool("repo.search"));
    assert!(!envelope.allows_tool("repo.impact"));
    assert!(envelope.allows_path("src/lib.rs"));
    assert!(!envelope.allows_path("docs/private.md"));
}
