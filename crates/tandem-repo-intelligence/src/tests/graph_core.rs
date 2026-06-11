use crate::{
    confidence_provenance, graph_fact_for_edge, graph_scope_for_repo, relation_edge_kind,
    Confidence, ExtractedFacts, ExtractedSymbol, GraphRelation, RepoIndexSnapshot, SymbolKind,
};
use tandem_graph_core::{stable_graph_hash, EdgeKind, FreshnessSource, GraphDomain, Provenance};

#[test]
fn repo_scope_uses_graph_core_without_runtime_dependencies() {
    let scope = graph_scope_for_repo("frumu-ai/tandem");

    assert_eq!(scope.tenant_id, "local");
    assert_eq!(scope.project_id, "repo-intelligence");
    assert_eq!(scope.repo_id.as_deref(), Some("frumu-ai/tandem"));
}

#[test]
fn repo_relations_map_to_shared_edge_taxonomy() {
    assert_eq!(
        relation_edge_kind(&GraphRelation::Defines),
        EdgeKind::Defines
    );
    assert_eq!(
        relation_edge_kind(&GraphRelation::Imports),
        EdgeKind::Imports
    );
    assert_eq!(
        relation_edge_kind(&GraphRelation::Configures),
        EdgeKind::Configures
    );
    assert_eq!(
        relation_edge_kind(&GraphRelation::Documents),
        EdgeKind::Documents
    );

    assert_eq!(
        confidence_provenance(&Confidence::Extracted),
        Provenance::Extracted
    );
    assert_eq!(
        confidence_provenance(&Confidence::Summary),
        Provenance::Summarized
    );
}

#[test]
fn repo_edge_can_be_promoted_to_stable_context_graph_fact() {
    let snapshot = RepoIndexSnapshot {
        root_label: "frumu-ai/tandem".to_string(),
        indexed_unix_ms: 1_700_000_000_000,
        manifest: Vec::new(),
        facts: ExtractedFacts {
            symbols: vec![ExtractedSymbol {
                file_path: "src/lib.rs".to_string(),
                line: 7,
                name: "RepoIndexSnapshot".to_string(),
                kind: SymbolKind::Struct,
                confidence: Confidence::Extracted,
            }],
            ..ExtractedFacts::default()
        },
    };
    let edge = snapshot.graph_edges().remove(0);
    let fact = graph_fact_for_edge(&snapshot, &edge);

    assert_eq!(fact.domain, GraphDomain::Repo);
    assert_eq!(fact.source_key, "src/lib.rs");
    assert_eq!(fact.target_key, "RepoIndexSnapshot");
    assert_eq!(fact.edge_kind, EdgeKind::Defines);
    assert_eq!(fact.provenance, Provenance::Extracted);
    assert_eq!(fact.freshness.source, FreshnessSource::IndexRevision);
    assert_eq!(fact.evidence.get("line").map(String::as_str), Some("7"));
    assert_eq!(stable_graph_hash(&fact).expect("hash graph fact").len(), 64);
}
