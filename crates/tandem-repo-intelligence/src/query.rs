use crate::model::{
    Confidence, FileManifestEntry, GraphEdge, GraphRelation, RepoIndexSnapshot, RepoSearchResult,
    SymbolKind,
};

pub fn repo_file<'a>(snapshot: &'a RepoIndexSnapshot, path: &str) -> Option<&'a FileManifestEntry> {
    snapshot.manifest.iter().find(|entry| entry.path == path)
}

pub fn repo_symbol(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    kind: Option<SymbolKind>,
    limit: usize,
) -> Vec<RepoSearchResult> {
    let query = query.to_lowercase();
    let mut results = Vec::new();
    for symbol in &snapshot.facts.symbols {
        if kind
            .as_ref()
            .is_some_and(|expected| expected != &symbol.kind)
        {
            continue;
        }
        if !symbol.name.to_lowercase().contains(&query) {
            continue;
        }
        results.push(RepoSearchResult {
            file_path: symbol.file_path.clone(),
            line: symbol.line,
            kind: format!("{:?}", symbol.kind),
            label: symbol.name.clone(),
            reason: "symbol name matched query".to_string(),
            confidence: symbol.confidence.clone(),
        });
    }
    sort_and_limit(results, limit)
}

pub fn symbols_by_kind(
    snapshot: &RepoIndexSnapshot,
    kind: SymbolKind,
    limit: usize,
) -> Vec<RepoSearchResult> {
    repo_symbol(snapshot, "", Some(kind), limit)
}

pub fn repo_search(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    limit: usize,
    path_scope: Option<&str>,
) -> Vec<RepoSearchResult> {
    let query = query.to_lowercase();
    if query.trim().is_empty() || snapshot.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    search_manifest(snapshot, &query, path_scope, &mut results);
    search_symbols(snapshot, &query, path_scope, &mut results);
    search_imports(snapshot, &query, path_scope, &mut results);
    search_config(snapshot, &query, path_scope, &mut results);
    search_docs(snapshot, &query, path_scope, &mut results);
    sort_and_limit(results, limit)
}

pub fn edges_by_relation(snapshot: &RepoIndexSnapshot, relation: GraphRelation) -> Vec<GraphEdge> {
    snapshot
        .graph_edges()
        .into_iter()
        .filter(|edge| edge.relation == relation)
        .collect()
}

fn search_manifest(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    path_scope: Option<&str>,
    results: &mut Vec<RepoSearchResult>,
) {
    for file in &snapshot.manifest {
        if !in_scope(&file.path, path_scope) || !file.path.to_lowercase().contains(query) {
            continue;
        }
        results.push(result(
            &file.path,
            1,
            "file",
            &file.path,
            "file path matched query",
            Confidence::Extracted,
        ));
    }
}

fn search_symbols(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    path_scope: Option<&str>,
    results: &mut Vec<RepoSearchResult>,
) {
    for symbol in &snapshot.facts.symbols {
        if !in_scope(&symbol.file_path, path_scope) || !symbol.name.to_lowercase().contains(query) {
            continue;
        }
        results.push(result(
            &symbol.file_path,
            symbol.line,
            &format!("{:?}", symbol.kind),
            &symbol.name,
            "symbol name matched query",
            symbol.confidence.clone(),
        ));
    }
}

fn search_imports(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    path_scope: Option<&str>,
    results: &mut Vec<RepoSearchResult>,
) {
    for import in &snapshot.facts.imports {
        if !in_scope(&import.source_file, path_scope)
            || !import.target.to_lowercase().contains(query)
        {
            continue;
        }
        results.push(result(
            &import.source_file,
            import.line,
            "import",
            &import.target,
            "import target matched query",
            import.confidence.clone(),
        ));
    }
}

fn search_config(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    path_scope: Option<&str>,
    results: &mut Vec<RepoSearchResult>,
) {
    for reference in &snapshot.facts.config_references {
        if !in_scope(&reference.file_path, path_scope)
            || !(reference.key.to_lowercase().contains(query)
                || reference.value.to_lowercase().contains(query))
        {
            continue;
        }
        results.push(result(
            &reference.file_path,
            reference.line,
            "config",
            &reference.key,
            "config key or value matched query",
            reference.confidence.clone(),
        ));
    }
}

fn search_docs(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    path_scope: Option<&str>,
    results: &mut Vec<RepoSearchResult>,
) {
    for heading in &snapshot.facts.doc_headings {
        if !in_scope(&heading.file_path, path_scope)
            || !(heading.title.to_lowercase().contains(query)
                || heading.excerpt.to_lowercase().contains(query))
        {
            continue;
        }
        results.push(result(
            &heading.file_path,
            heading.line,
            "doc",
            &heading.title,
            "doc heading or excerpt matched query",
            heading.confidence.clone(),
        ));
    }
}

fn result(
    file_path: &str,
    line: usize,
    kind: &str,
    label: &str,
    reason: &str,
    confidence: Confidence,
) -> RepoSearchResult {
    RepoSearchResult {
        file_path: file_path.to_string(),
        line,
        kind: kind.to_string(),
        label: label.to_string(),
        reason: reason.to_string(),
        confidence,
    }
}

fn in_scope(path: &str, path_scope: Option<&str>) -> bool {
    let Some(scope) = path_scope else {
        return true;
    };
    let scope = scope.trim_matches('/');
    if scope.is_empty() || scope == "." {
        return true;
    }
    path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn sort_and_limit(mut results: Vec<RepoSearchResult>, limit: usize) -> Vec<RepoSearchResult> {
    results.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.line.cmp(&right.line))
            .then(left.kind.cmp(&right.kind))
            .then(left.label.cmp(&right.label))
    });
    results.truncate(limit);
    results
}
