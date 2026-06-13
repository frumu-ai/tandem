use crate::model::{
    Confidence, FileManifestEntry, GraphEdge, GraphRelation, RepoIndexSnapshot, RepoRetrievalTrace,
    RepoSearchResult, SymbolKind,
};
use crate::query_text::{
    best_match, compare_match_score, match_text, MatchScore, NormalizedText, TokenMatchMode,
};

struct ScoredResult {
    result: RepoSearchResult,
    score: MatchScore,
    kind_rank: u8,
}

pub fn repo_file<'a>(snapshot: &'a RepoIndexSnapshot, path: &str) -> Option<&'a FileManifestEntry> {
    snapshot.manifest.iter().find(|entry| entry.path == path)
}

pub fn repo_symbol(
    snapshot: &RepoIndexSnapshot,
    query: &str,
    kind: Option<SymbolKind>,
    limit: usize,
) -> Vec<RepoSearchResult> {
    let query = NormalizedText::for_query(query);
    let mut results = Vec::new();
    for symbol in &snapshot.facts.symbols {
        if kind
            .as_ref()
            .is_some_and(|expected| expected != &symbol.kind)
        {
            continue;
        }
        let Some(score) = match_text(&query, &symbol.name, TokenMatchMode::RequireAll) else {
            continue;
        };
        results.push(ScoredResult {
            result: RepoSearchResult {
                file_path: symbol.file_path.clone(),
                line: symbol.line,
                kind: format!("{:?}", symbol.kind),
                label: symbol.name.clone(),
                reason: symbol_reason(score),
                confidence: symbol.confidence.clone(),
                trace: trace(
                    "symbol",
                    &query,
                    &[&symbol.file_path, &symbol.name],
                    None,
                    symbol.confidence.clone(),
                    score,
                    0,
                ),
            },
            score,
            kind_rank: 0,
        });
    }
    sort_scored_and_limit(results, limit)
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
    let query = NormalizedText::for_query(query);
    if query.is_empty() || snapshot.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    search_manifest(snapshot, &query, path_scope, &mut results);
    search_symbols(snapshot, &query, path_scope, &mut results);
    search_imports(snapshot, &query, path_scope, &mut results);
    search_config(snapshot, &query, path_scope, &mut results);
    search_docs(snapshot, &query, path_scope, &mut results);
    sort_scored_and_limit(results, limit)
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
    query: &NormalizedText,
    path_scope: Option<&str>,
    results: &mut Vec<ScoredResult>,
) {
    for file in &snapshot.manifest {
        if !in_scope(&file.path, path_scope) {
            continue;
        }
        let Some(score) = match_text(query, &file.path, TokenMatchMode::AllowPartial) else {
            continue;
        };
        results.push(scored_result(
            result(
                &file.path,
                1,
                "file",
                &file.path,
                search_reason(score, "file path"),
                Confidence::Extracted,
            ),
            score,
            "manifest",
            query,
            &[&file.path],
            path_scope,
        ));
    }
}

fn search_symbols(
    snapshot: &RepoIndexSnapshot,
    query: &NormalizedText,
    path_scope: Option<&str>,
    results: &mut Vec<ScoredResult>,
) {
    for symbol in &snapshot.facts.symbols {
        if !in_scope(&symbol.file_path, path_scope) {
            continue;
        }
        let name_score = match_text(query, &symbol.name, TokenMatchMode::AllowPartial);
        let path_score =
            match_text(query, &symbol.file_path, TokenMatchMode::AllowPartial).map(|mut score| {
                if name_score.is_none() {
                    score.tier = score.tier.max(5);
                }
                score
            });
        let Some(score) = best_score(name_score, path_score) else {
            continue;
        };
        results.push(scored_result(
            result(
                &symbol.file_path,
                symbol.line,
                &format!("{:?}", symbol.kind),
                &symbol.name,
                search_reason(score, "symbol name or file path"),
                symbol.confidence.clone(),
            ),
            score,
            "symbol",
            query,
            &[&symbol.file_path, &symbol.name],
            path_scope,
        ));
    }
}

fn search_imports(
    snapshot: &RepoIndexSnapshot,
    query: &NormalizedText,
    path_scope: Option<&str>,
    results: &mut Vec<ScoredResult>,
) {
    for import in &snapshot.facts.imports {
        if !in_scope(&import.source_file, path_scope) {
            continue;
        }
        let Some(score) = best_match(
            query,
            &[&import.target, &import.source_file],
            TokenMatchMode::AllowPartial,
        ) else {
            continue;
        };
        results.push(scored_result(
            result(
                &import.source_file,
                import.line,
                "import",
                &import.target,
                search_reason(score, "import target or source path"),
                import.confidence.clone(),
            ),
            score,
            "import",
            query,
            &[&import.source_file, &import.target],
            path_scope,
        ));
    }
}

fn search_config(
    snapshot: &RepoIndexSnapshot,
    query: &NormalizedText,
    path_scope: Option<&str>,
    results: &mut Vec<ScoredResult>,
) {
    for reference in &snapshot.facts.config_references {
        if !in_scope(&reference.file_path, path_scope) {
            continue;
        }
        let Some(score) = best_match(
            query,
            &[&reference.key, &reference.value, &reference.file_path],
            TokenMatchMode::AllowPartial,
        ) else {
            continue;
        };
        results.push(scored_result(
            result(
                &reference.file_path,
                reference.line,
                "config",
                &reference.key,
                search_reason(score, "config key, value, or file path"),
                reference.confidence.clone(),
            ),
            score,
            "config",
            query,
            &[&reference.file_path, &reference.key],
            path_scope,
        ));
    }
}

fn search_docs(
    snapshot: &RepoIndexSnapshot,
    query: &NormalizedText,
    path_scope: Option<&str>,
    results: &mut Vec<ScoredResult>,
) {
    for heading in &snapshot.facts.doc_headings {
        if !in_scope(&heading.file_path, path_scope) {
            continue;
        }
        let Some(score) = best_match(
            query,
            &[&heading.title, &heading.excerpt, &heading.file_path],
            TokenMatchMode::AllowPartial,
        ) else {
            continue;
        };
        results.push(scored_result(
            result(
                &heading.file_path,
                heading.line,
                "doc",
                &heading.title,
                search_reason(score, "doc heading, excerpt, or file path"),
                heading.confidence.clone(),
            ),
            score,
            "doc",
            query,
            &[&heading.file_path, &heading.title],
            path_scope,
        ));
    }
}

fn result(
    file_path: &str,
    line: usize,
    kind: &str,
    label: &str,
    reason: impl Into<String>,
    confidence: Confidence,
) -> RepoSearchResult {
    RepoSearchResult {
        file_path: file_path.to_string(),
        line,
        kind: kind.to_string(),
        label: label.to_string(),
        reason: reason.into(),
        confidence,
        trace: Vec::new(),
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

fn scored_result(
    mut result: RepoSearchResult,
    score: MatchScore,
    retriever: &str,
    query: &NormalizedText,
    edge_path: &[&str],
    path_scope: Option<&str>,
) -> ScoredResult {
    let kind_rank = search_kind_rank(&result);
    result.trace = trace(
        retriever,
        query,
        edge_path,
        path_scope,
        result.confidence.clone(),
        score,
        kind_rank,
    );
    ScoredResult {
        result,
        score,
        kind_rank,
    }
}

fn best_score(left: Option<MatchScore>, right: Option<MatchScore>) -> Option<MatchScore> {
    match (left, right) {
        (Some(left), Some(right)) => {
            if compare_match_score(&left, &right).is_le() {
                Some(left)
            } else {
                Some(right)
            }
        }
        (Some(score), None) | (None, Some(score)) => Some(score),
        (None, None) => None,
    }
}

fn trace(
    retriever: &str,
    query: &NormalizedText,
    edge_path: &[&str],
    scope: Option<&str>,
    confidence: Confidence,
    score: MatchScore,
    kind_rank: u8,
) -> Vec<RepoRetrievalTrace> {
    vec![RepoRetrievalTrace {
        retriever: retriever.to_string(),
        matched_term: query.display(),
        edge_path: edge_path.iter().map(|value| value.to_string()).collect(),
        confidence,
        scope: scope.map(str::to_string),
        ranking: format!(
            "match_tier={},missed_tokens={},token_hits={},kind_rank={kind_rank}",
            score.tier, score.missed_tokens, score.token_hits
        ),
    }]
}

fn search_kind_rank(result: &RepoSearchResult) -> u8 {
    match result.kind.as_str() {
        "Function" | "Struct" | "Enum" | "Trait" | "Impl" | "Module" | "Class" | "Interface"
        | "TypeAlias" | "Constant" => 0,
        "file" => 1,
        "import" | "config" => 2,
        "doc" => 3,
        _ => 4,
    }
}

fn symbol_reason(score: MatchScore) -> String {
    if score.tier <= 1 {
        "symbol name exactly matched normalized query".to_string()
    } else if score.tier <= 3 {
        "symbol name contained normalized query".to_string()
    } else {
        "symbol name matched normalized query tokens".to_string()
    }
}

fn search_reason(score: MatchScore, target: &str) -> String {
    if score.tier <= 1 {
        format!("{target} exactly matched normalized query")
    } else if score.tier <= 3 {
        format!("{target} contained normalized query")
    } else {
        format!("{target} matched normalized query tokens")
    }
}

fn sort_scored_and_limit(mut results: Vec<ScoredResult>, limit: usize) -> Vec<RepoSearchResult> {
    results.sort_by(|left, right| {
        compare_match_score(&left.score, &right.score)
            .then(left.kind_rank.cmp(&right.kind_rank))
            .then(left.result.file_path.cmp(&right.result.file_path))
            .then(left.result.line.cmp(&right.result.line))
            .then(left.result.kind.cmp(&right.result.kind))
            .then(left.result.label.cmp(&right.result.label))
    });
    results.truncate(limit);
    results.into_iter().map(|scored| scored.result).collect()
}
