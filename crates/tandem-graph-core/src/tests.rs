use crate::{
    stable_graph_hash, ArtifactNode, ContextNodePayload, EdgeKind, Freshness, FreshnessSource,
    GraphDomain, GraphFact, GraphQueryEnvelope, GraphScope, NodeKind, PolicyDecision, Provenance,
    ToolCredentialNode, Visibility,
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
    assert!(value["freshness"].get("checked_at_unix_ms").is_none());
    assert!(value["freshness"].get("stale_after_unix_ms").is_none());
    assert_eq!(value["policy"], json!("allowed"));
    assert_eq!(
        serde_json::to_value(NodeKind::File).expect("serialize node kind"),
        json!("repo.file")
    );
}

#[test]
fn freshness_optional_metadata_does_not_change_default_serialization() {
    let freshness = Freshness::from_revision(FreshnessSource::Commit, "abc123");
    let value = serde_json::to_value(&freshness).expect("serialize freshness");

    assert_eq!(value["source"], json!("commit"));
    assert_eq!(value["revision"], json!("abc123"));
    assert!(value.get("checked_at_unix_ms").is_none());
    assert!(value.get("stale_after_unix_ms").is_none());
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

#[test]
fn context_node_payloads_are_display_safe() {
    let credential = ContextNodePayload::ToolCredential(ToolCredentialNode {
        provider: "linear".to_string(),
        credential_ref: "credential://linear/team-a".to_string(),
        status: "connected".to_string(),
        scopes: vec!["issues:read".to_string(), "issues:write".to_string()],
        expires_at_unix_ms: Some(1_800_000_000_000),
        secret_material_present: true,
    });

    assert_eq!(credential.node_kind(), NodeKind::Credential);
    let payload = credential.display_safe_payload();
    assert_eq!(
        payload.get("credential_ref").map(String::as_str),
        Some("credential://linear/team-a")
    );
    assert_eq!(
        payload.get("secret_material_present").map(String::as_str),
        Some("true")
    );
    assert!(!payload.contains_key("token"));
    assert!(!payload.contains_key("secret"));
    assert!(!payload.contains_key("refresh_token"));

    let artifact = ContextNodePayload::Artifact(ArtifactNode {
        artifact_id: "artifact-a".to_string(),
        artifact_type: "report".to_string(),
        display_name: "Run summary".to_string(),
        path_ref: Some("artifact://run-a/summary.md".to_string()),
        content_hash: Some("sha256:abc".to_string()),
        produced_by_run: Some("run-a".to_string()),
    });

    let payload = artifact.display_safe_payload();
    assert_eq!(
        payload.get("content_hash").map(String::as_str),
        Some("sha256:abc")
    );
    assert!(!payload.contains_key("content"));
}

#[test]
fn provenance_distinguishes_source_truth_from_agent_hints() {
    assert!(Provenance::Extracted.is_source_truth());
    assert!(Provenance::Configured.is_source_truth());
    assert!(Provenance::Observed.is_source_truth());
    assert!(!Provenance::Inferred.is_source_truth());
    assert!(!Provenance::Summarized.is_source_truth());
    assert!(Provenance::Inferred.requires_source_confirmation());
}

#[test]
fn freshness_and_visibility_report_staleness_and_scope() {
    let freshness = Freshness::from_revision(FreshnessSource::PolicyHash, "policy-a")
        .with_checked_at(1_000)
        .with_stale_after(2_000);

    assert!(!freshness.is_unknown());
    assert!(!freshness.is_stale_at(1_999));
    assert!(freshness.is_stale_at(2_000));

    let tenant_scope = GraphScope::new("tenant-a", "project-a").with_run("run-a");
    let other_scope = GraphScope::new("tenant-b", "project-a").with_run("run-a");
    let visibility = Visibility::for_scope(&tenant_scope)
        .with_readable_paths(["src", "docs"])
        .redacted();

    assert!(visibility.redacted);
    assert!(visibility.allows_scope(&tenant_scope));
    assert!(!visibility.allows_scope(&other_scope));
    assert!(!Visibility::default().allows_scope(&tenant_scope));
    assert_eq!(visibility.readable_paths, vec!["src", "docs"]);

    assert!(PolicyDecision::Allowed.is_allowed());
    assert!(PolicyDecision::Denied {
        reason: "path_denied".to_string()
    }
    .is_denied());
}
