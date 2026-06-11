use crate::{
    edges_by_relation, graph_scope_for_repo, repo_context_bundle, repo_file, repo_impact,
    repo_neighbors, repo_search, repo_symbol, symbols_by_kind, FileManifestEntry, GraphEdge,
    GraphRelation, RepoContextBundle, RepoContextBundleOptions, RepoGraphNeighbor, RepoImpactItem,
    RepoImpactSummary, RepoIndexSnapshot, RepoSearchResult, SymbolKind,
};
use tandem_graph_core::{GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput};

pub fn repo_file_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    path: &str,
) -> GraphQueryOutput<Option<FileManifestEntry>> {
    let mut audit = base_audit(envelope, snapshot, "repo.file");
    let value = if audit.allowed() && allow_path(envelope, path, &mut audit) {
        repo_file(snapshot, path).cloned()
    } else {
        None
    };
    GraphQueryOutput::new(value, audit)
}

pub fn repo_search_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    query: &str,
    limit: usize,
    path_scope: Option<&str>,
) -> GraphQueryOutput<Vec<RepoSearchResult>> {
    let mut audit = base_audit(envelope, snapshot, "repo.search");
    let value = if audit.allowed() {
        repo_search(snapshot, query, limit, path_scope)
            .into_iter()
            .filter(|result| allow_path(envelope, &result.file_path, &mut audit))
            .collect()
    } else {
        Vec::new()
    };
    GraphQueryOutput::new(value, audit)
}

pub fn repo_symbol_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    query: &str,
    kind: Option<SymbolKind>,
    limit: usize,
) -> GraphQueryOutput<Vec<RepoSearchResult>> {
    let mut audit = base_audit(envelope, snapshot, "repo.symbol");
    let value = if audit.allowed() {
        repo_symbol(snapshot, query, kind, limit)
            .into_iter()
            .filter(|result| allow_path(envelope, &result.file_path, &mut audit))
            .collect()
    } else {
        Vec::new()
    };
    GraphQueryOutput::new(value, audit)
}

pub fn symbols_by_kind_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    kind: SymbolKind,
    limit: usize,
) -> GraphQueryOutput<Vec<RepoSearchResult>> {
    let mut audit = base_audit(envelope, snapshot, "repo.symbol");
    let value = if audit.allowed() {
        symbols_by_kind(snapshot, kind, limit)
            .into_iter()
            .filter(|result| allow_path(envelope, &result.file_path, &mut audit))
            .collect()
    } else {
        Vec::new()
    };
    GraphQueryOutput::new(value, audit)
}

pub fn edges_by_relation_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    relation: GraphRelation,
) -> GraphQueryOutput<Vec<GraphEdge>> {
    let mut audit = base_audit(envelope, snapshot, "repo.edges");
    let value = if audit.allowed() {
        edges_by_relation(snapshot, relation)
            .into_iter()
            .filter(|edge| {
                allow_path(envelope, &edge.source, &mut audit)
                    && target_allowed_if_path(envelope, &edge.target, &mut audit)
            })
            .collect()
    } else {
        Vec::new()
    };
    GraphQueryOutput::new(value, audit)
}

pub fn repo_neighbors_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    node_or_path: &str,
    relation_filter: Option<GraphRelation>,
    depth: usize,
) -> GraphQueryOutput<Vec<RepoGraphNeighbor>> {
    let mut audit = base_audit(envelope, snapshot, "repo.neighbors");
    if audit.allowed() && looks_like_path(node_or_path) {
        allow_path(envelope, node_or_path, &mut audit);
    }
    let value = if audit.allowed() {
        repo_neighbors(snapshot, node_or_path, relation_filter, depth)
            .into_iter()
            .filter(|neighbor| {
                allow_path(envelope, &neighbor.edge.source, &mut audit)
                    && target_allowed_if_path(envelope, &neighbor.edge.target, &mut audit)
            })
            .collect()
    } else {
        Vec::new()
    };
    GraphQueryOutput::new(value, audit)
}

pub fn repo_impact_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    changed_files: &[String],
) -> GraphQueryOutput<RepoImpactSummary> {
    let mut audit = base_audit(envelope, snapshot, "repo.impact");
    let base_allowed = audit.allowed();
    let allowed_changed_files = if base_allowed {
        changed_files
            .iter()
            .filter(|path| allow_path(envelope, path, &mut audit))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut value = if base_allowed {
        repo_impact(snapshot, &allowed_changed_files)
    } else {
        RepoImpactSummary {
            changed_files: Vec::new(),
            directly_affected: Vec::new(),
            import_neighbors: Vec::new(),
            config_or_docs: Vec::new(),
            likely_test_targets: Vec::new(),
        }
    };
    filter_impact(&mut value, envelope, &mut audit);
    GraphQueryOutput::new(value, audit)
}

pub fn repo_context_bundle_governed(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    task: &str,
    mut options: RepoContextBundleOptions,
) -> GraphQueryOutput<RepoContextBundle> {
    let mut audit = base_audit(envelope, snapshot, "repo.context_bundle");
    let base_allowed = audit.allowed();
    options.required_files = if base_allowed {
        options
            .required_files
            .into_iter()
            .filter(|path| allow_path(envelope, path, &mut audit))
            .collect()
    } else {
        Vec::new()
    };
    options.changed_files = if base_allowed {
        options
            .changed_files
            .into_iter()
            .filter(|path| allow_path(envelope, path, &mut audit))
            .collect()
    } else {
        Vec::new()
    };
    if options.path_scope.is_none() {
        options.path_scope = narrowest_readable_scope(envelope);
    }
    let mut value = if base_allowed {
        repo_context_bundle(snapshot, task, options)
    } else {
        RepoContextBundle {
            task: task.to_string(),
            budget_chars: options.budget_chars,
            likely_files: Vec::new(),
            relevant_symbols: Vec::new(),
            graph_edges: Vec::new(),
            suggested_first_reads: Vec::new(),
            test_targets: Vec::new(),
            gaps: vec!["graph query denied by governance envelope".to_string()],
            estimated_chars: 0,
        }
    };
    filter_bundle(&mut value, envelope, &mut audit);
    GraphQueryOutput::new(value, audit)
}

fn base_audit(
    envelope: &GraphQueryEnvelope,
    snapshot: &RepoIndexSnapshot,
    tool: &str,
) -> GraphQueryAudit {
    let mut audit = GraphQueryAudit::default();
    if let Err(error) = envelope.validate() {
        audit.deny(format!("invalid_envelope:{}", error.missing.join(",")));
    }
    if !envelope.allows_tool(tool) {
        audit.deny("tool_denied");
    }
    let expected = graph_scope_for_repo(&snapshot.root_label);
    if envelope.scope.tenant_id != expected.tenant_id {
        audit.deny("tenant_scope_mismatch");
    }
    if envelope.scope.project_id != expected.project_id {
        audit.deny("project_scope_mismatch");
    }
    if envelope.scope.repo_id != expected.repo_id {
        audit.deny("repo_scope_mismatch");
    }
    audit
}

fn allow_path(envelope: &GraphQueryEnvelope, path: &str, audit: &mut GraphQueryAudit) -> bool {
    if envelope.allows_path(path) {
        true
    } else {
        audit.deny("path_denied");
        false
    }
}

fn target_allowed_if_path(
    envelope: &GraphQueryEnvelope,
    target: &str,
    audit: &mut GraphQueryAudit,
) -> bool {
    !looks_like_path(target) || allow_path(envelope, target, audit)
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.contains('.')
}

fn filter_impact(
    impact: &mut RepoImpactSummary,
    envelope: &GraphQueryEnvelope,
    audit: &mut GraphQueryAudit,
) {
    impact
        .changed_files
        .retain(|path| allow_path(envelope, path, audit));
    filter_items(&mut impact.directly_affected, envelope, audit);
    filter_items(&mut impact.import_neighbors, envelope, audit);
    filter_items(&mut impact.config_or_docs, envelope, audit);
    filter_items(&mut impact.likely_test_targets, envelope, audit);
}

fn filter_items(
    items: &mut Vec<RepoImpactItem>,
    envelope: &GraphQueryEnvelope,
    audit: &mut GraphQueryAudit,
) {
    items.retain(|item| allow_path(envelope, &item.file_path, audit));
}

fn filter_bundle(
    bundle: &mut RepoContextBundle,
    envelope: &GraphQueryEnvelope,
    audit: &mut GraphQueryAudit,
) {
    bundle
        .likely_files
        .retain(|result| allow_path(envelope, &result.file_path, audit));
    bundle
        .relevant_symbols
        .retain(|result| allow_path(envelope, &result.file_path, audit));
    bundle.graph_edges.retain(|edge| {
        allow_path(envelope, &edge.source, audit)
            && target_allowed_if_path(envelope, &edge.target, audit)
    });
    bundle
        .suggested_first_reads
        .retain(|path| allow_path(envelope, path, audit));
    bundle
        .test_targets
        .retain(|path| allow_path(envelope, path, audit));
}

fn narrowest_readable_scope(envelope: &GraphQueryEnvelope) -> Option<String> {
    envelope
        .readable_paths
        .iter()
        .filter(|path| {
            let trimmed = path.trim_matches('/');
            !trimmed.is_empty() && trimmed != "."
        })
        .min_by_key(|path| path.len())
        .cloned()
}
