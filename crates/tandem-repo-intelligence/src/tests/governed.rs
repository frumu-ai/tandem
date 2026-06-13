use crate::{
    repo_context_bundle_governed, repo_impact_governed, repo_search_governed, repo_symbol_governed,
    RepoContextBundleOptions, RepoIndexSnapshot, SymbolKind,
};
use tandem_graph_core::{GraphQueryEnvelope, GraphScope};

#[test]
fn governed_search_filters_denied_paths() {
    let snapshot = fixture_snapshot();
    let envelope = envelope_for(&snapshot, "repo.search", &["src"]);

    let output = repo_search_governed(&envelope, &snapshot, "login", 10, None);

    assert!(!output.value.is_empty());
    assert!(output
        .value
        .iter()
        .all(|result| result.file_path == "src/login.rs"));
    assert!(output.audit.denied_count > 0);
    assert!(output
        .audit
        .denied_reasons
        .contains(&"path_denied".to_string()));
}

#[test]
fn governed_query_denies_cross_tenant_and_project_scope() {
    let snapshot = fixture_snapshot();
    let mut envelope = envelope_for(&snapshot, "repo.search", &["."]);
    envelope.scope.tenant_id = "other-tenant".to_string();
    envelope.scope.project_id = "other-project".to_string();

    let output = repo_search_governed(&envelope, &snapshot, "login", 10, None);

    assert!(output.value.is_empty());
    assert!(output
        .audit
        .denied_reasons
        .contains(&"tenant_scope_mismatch".to_string()));
    assert!(output
        .audit
        .denied_reasons
        .contains(&"project_scope_mismatch".to_string()));
}

#[test]
fn governed_query_denies_unlisted_tool() {
    let snapshot = fixture_snapshot();
    let envelope = envelope_for(&snapshot, "repo.search", &["."]);

    let output = repo_symbol_governed(
        &envelope,
        &snapshot,
        "login",
        Some(SymbolKind::Function),
        10,
    );

    assert!(output.value.is_empty());
    assert!(output
        .audit
        .denied_reasons
        .contains(&"tool_denied".to_string()));
}

#[test]
fn governed_impact_blocks_base_scope_mismatch_even_for_allowed_paths() {
    let snapshot = fixture_snapshot();
    let mut envelope = envelope_for(&snapshot, "repo.impact", &["."]);
    envelope.scope.repo_id = Some("other-repo".to_string());

    let output = repo_impact_governed(&envelope, &snapshot, &[String::from("src/login.rs")]);

    assert!(output.value.changed_files.is_empty());
    assert!(output
        .audit
        .denied_reasons
        .contains(&"repo_scope_mismatch".to_string()));
}

#[test]
fn governed_context_bundle_blocks_unlisted_tool() {
    let snapshot = fixture_snapshot();
    let envelope = envelope_for(&snapshot, "repo.search", &["."]);

    let output = repo_context_bundle_governed(
        &envelope,
        &snapshot,
        "login flow",
        RepoContextBundleOptions::default(),
    );

    assert!(output.value.suggested_first_reads.is_empty());
    assert!(output
        .audit
        .denied_reasons
        .contains(&"tool_denied".to_string()));
}

#[test]
fn governed_context_bundle_does_not_leak_denied_required_files() {
    let snapshot = fixture_snapshot();
    let envelope = envelope_for(&snapshot, "repo.context_bundle", &["src"]);

    let output = repo_context_bundle_governed(
        &envelope,
        &snapshot,
        "login flow",
        RepoContextBundleOptions {
            required_files: vec!["private/plan.md".to_string()],
            result_limit: 10,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(!output
        .value
        .suggested_first_reads
        .contains(&"private/plan.md".to_string()));
    assert!(output
        .value
        .suggested_first_reads
        .iter()
        .all(|path| path.starts_with("src/")));
    assert!(output.audit.denied_count > 0);
}

#[test]
fn governed_context_bundle_reports_denied_scope_without_leaking_paths() {
    let snapshot = fixture_snapshot();
    let envelope = envelope_for(&snapshot, "repo.context_bundle", &[]);

    let output = repo_context_bundle_governed(
        &envelope,
        &snapshot,
        "login flow private plan",
        RepoContextBundleOptions {
            required_files: vec!["private/plan.md".to_string()],
            result_limit: 10,
            ..RepoContextBundleOptions::default()
        },
    );

    assert!(output.value.suggested_first_reads.is_empty());
    assert!(output
        .value
        .gaps
        .iter()
        .any(|gap| gap.contains("readable_paths")));
    assert!(output
        .value
        .gaps
        .iter()
        .any(|gap| gap.contains("path_scope:\".\"")));
    assert!(!output
        .value
        .gaps
        .iter()
        .any(|gap| gap.contains("private/plan.md")));
}

fn envelope_for(
    snapshot: &RepoIndexSnapshot,
    tool: &str,
    readable_paths: &[&str],
) -> GraphQueryEnvelope {
    let mut envelope = GraphQueryEnvelope::new(
        GraphScope::new("local", "repo-intelligence").with_repo(&snapshot.root_label),
        "agent-a",
    );
    envelope.allowed_tools.push(tool.to_string());
    envelope.readable_paths = readable_paths.iter().map(|path| path.to_string()).collect();
    envelope
}

fn fixture_snapshot() -> RepoIndexSnapshot {
    use crate::{Confidence, ExtractedFacts, ExtractedSymbol, ImportEdge};

    RepoIndexSnapshot {
        root_label: "repo-a".to_string(),
        indexed_unix_ms: 1,
        manifest: vec![
            file("src/login.rs"),
            file("private/plan.md"),
            file("tests/login_test.rs"),
        ],
        facts: ExtractedFacts {
            symbols: vec![
                ExtractedSymbol {
                    file_path: "src/login.rs".to_string(),
                    line: 1,
                    name: "login_flow".to_string(),
                    kind: SymbolKind::Function,
                    confidence: Confidence::Extracted,
                },
                ExtractedSymbol {
                    file_path: "private/plan.md".to_string(),
                    line: 1,
                    name: "login_plan".to_string(),
                    kind: SymbolKind::Module,
                    confidence: Confidence::Summary,
                },
            ],
            imports: vec![ImportEdge {
                source_file: "tests/login_test.rs".to_string(),
                line: 1,
                target: "login_flow".to_string(),
                confidence: Confidence::Extracted,
            }],
            ..ExtractedFacts::default()
        },
    }
}

fn file(path: &str) -> crate::FileManifestEntry {
    crate::FileManifestEntry {
        path: path.to_string(),
        size_bytes: 1,
        modified_unix_ms: 1,
        sha256: "hash".to_string(),
    }
}
