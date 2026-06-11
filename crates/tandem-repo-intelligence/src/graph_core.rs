use crate::{Confidence, GraphEdge, GraphRelation, RepoIndexSnapshot};
use tandem_graph_core::{
    EdgeKind, Freshness, FreshnessSource, GraphDomain, GraphFact, GraphScope, Provenance,
};

pub fn graph_scope_for_repo(root_label: impl Into<String>) -> GraphScope {
    let root_label = root_label.into();
    GraphScope::new("local", "repo-intelligence").with_repo(root_label)
}

pub fn confidence_provenance(confidence: &Confidence) -> Provenance {
    match confidence {
        Confidence::Extracted => Provenance::Extracted,
        Confidence::Inferred => Provenance::Inferred,
        Confidence::Summary => Provenance::Summarized,
        Confidence::Ambiguous => Provenance::Ambiguous,
    }
}

pub fn relation_edge_kind(relation: &GraphRelation) -> EdgeKind {
    match relation {
        GraphRelation::Defines => EdgeKind::Defines,
        GraphRelation::Imports => EdgeKind::Imports,
        GraphRelation::Configures => EdgeKind::Configures,
        GraphRelation::Documents => EdgeKind::Documents,
    }
}

pub fn snapshot_freshness(snapshot: &RepoIndexSnapshot) -> Freshness {
    Freshness::from_revision(
        FreshnessSource::IndexRevision,
        snapshot.indexed_unix_ms.to_string(),
    )
}

pub fn graph_fact_for_edge(snapshot: &RepoIndexSnapshot, edge: &GraphEdge) -> GraphFact {
    let mut fact = GraphFact::new(
        graph_scope_for_repo(&snapshot.root_label),
        GraphDomain::Repo,
        &edge.source,
        &edge.target,
        relation_edge_kind(&edge.relation),
        confidence_provenance(&edge.confidence),
    );
    fact.freshness = snapshot_freshness(snapshot);
    fact.evidence
        .insert("line".to_string(), edge.line.to_string());
    fact.evidence
        .insert("relation".to_string(), format!("{:?}", edge.relation));
    fact
}
