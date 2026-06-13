use crate::chunks::first_read_spans;
use crate::context::repo_impact;
use crate::model::{
    Confidence, GraphEdge, RepoContextBundle, RepoContextBundleOptions, RepoImpactItem,
    RepoIndexSnapshot, RepoRetrievalTrace, RepoSearchResult,
};
use crate::query_text::{best_match, query_terms, NormalizedText, TokenMatchMode};
use crate::{repo_file, repo_search, repo_symbol};
use std::collections::BTreeSet;

pub fn repo_context_bundle(
    snapshot: &RepoIndexSnapshot,
    task: &str,
    options: RepoContextBundleOptions,
) -> RepoContextBundle {
    let limit = options.result_limit.max(1);
    let path_scope = options.path_scope.as_deref();
    let terms = task_terms(task);
    let mut likely_files = required_file_results(snapshot, &options.required_files);
    let mut relevant_symbols = Vec::new();

    for term in &terms {
        likely_files.extend(repo_search(snapshot, term, limit, path_scope));
        relevant_symbols.extend(
            repo_symbol(snapshot, term, None, limit)
                .into_iter()
                .filter(|result| in_scope(&result.file_path, path_scope)),
        );
    }

    let impact = repo_impact(snapshot, &options.changed_files);
    likely_files.extend(impact_items_as_results(&impact.directly_affected));
    likely_files.extend(impact_items_as_results(&impact.import_neighbors));
    likely_files.extend(impact_items_as_results(&impact.config_or_docs));

    let rank_context = RankContext::new(&terms, path_scope);
    let likely_files = ranked_results(likely_files, limit, &rank_context);
    let relevant_symbols = ranked_results(relevant_symbols, limit, &rank_context);
    let graph_edges = explanatory_edges(snapshot, &likely_files, limit);
    let suggested_first_reads = unique_paths(&likely_files, limit);
    let first_read_spans = first_read_spans(snapshot, &likely_files, &relevant_symbols, limit);
    let mut bundle = RepoContextBundle {
        task: task.to_string(),
        budget_chars: options.budget_chars,
        likely_files,
        relevant_symbols,
        graph_edges,
        suggested_first_reads,
        first_read_spans,
        test_targets: impact
            .likely_test_targets
            .iter()
            .map(|item| item.file_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(limit)
            .collect(),
        gaps: bundle_gaps(snapshot, &terms),
        estimated_chars: 0,
    };
    trim_to_budget(&mut bundle);
    bundle
}

fn required_file_results(
    snapshot: &RepoIndexSnapshot,
    required_files: &[String],
) -> Vec<RepoSearchResult> {
    required_files
        .iter()
        .filter(|file| repo_file(snapshot, file).is_some())
        .map(|file| {
            search_result(
                file,
                1,
                "file",
                file,
                "required by task input",
                Confidence::Extracted,
                "required_file",
            )
        })
        .collect()
}

fn task_terms(task: &str) -> Vec<String> {
    query_terms(task, 12)
}

fn search_result(
    file_path: &str,
    line: usize,
    kind: &str,
    label: &str,
    reason: &str,
    confidence: Confidence,
    retriever: &str,
) -> RepoSearchResult {
    RepoSearchResult {
        file_path: file_path.to_string(),
        line,
        kind: kind.to_string(),
        label: label.to_string(),
        reason: reason.to_string(),
        confidence: confidence.clone(),
        trace: vec![RepoRetrievalTrace {
            retriever: retriever.to_string(),
            matched_term: label.to_string(),
            edge_path: vec![file_path.to_string(), label.to_string()],
            confidence,
            scope: None,
            ranking: reason.to_string(),
        }],
    }
}

fn impact_items_as_results(items: &[RepoImpactItem]) -> Vec<RepoSearchResult> {
    items
        .iter()
        .map(|item| {
            search_result(
                &item.file_path,
                item.line,
                &format!("{:?}", item.relation),
                &item.file_path,
                &item.reason,
                item.confidence.clone(),
                "graph_impact",
            )
        })
        .collect()
}

fn ranked_results(
    results: Vec<RepoSearchResult>,
    limit: usize,
    context: &RankContext<'_>,
) -> Vec<RepoSearchResult> {
    let mut seen = BTreeSet::new();
    let mut ranked = Vec::new();
    for result in results {
        let key = (
            result.file_path.clone(),
            result.line,
            result.kind.clone(),
            result.label.clone(),
        );
        if seen.insert(key) {
            ranked.push(result);
        }
    }
    ranked.sort_by(|left, right| {
        file_rank(left, context)
            .cmp(&file_rank(right, context))
            .then(relevance_rank(left, context).cmp(&relevance_rank(right, context)))
            .then(left.file_path.cmp(&right.file_path))
            .then(left.line.cmp(&right.line))
            .then(left.kind.cmp(&right.kind))
            .then(left.label.cmp(&right.label))
    });
    ranked.truncate(limit);
    ranked
}

struct RankContext<'a> {
    terms: &'a [String],
    path_scope: Option<&'a str>,
    implementation_task: bool,
    explicit_doc_task: bool,
    explicit_test_task: bool,
}

impl<'a> RankContext<'a> {
    fn new(terms: &'a [String], path_scope: Option<&'a str>) -> Self {
        Self {
            terms,
            path_scope,
            implementation_task: terms
                .iter()
                .any(|term| IMPLEMENTATION_TERMS.contains(&term.as_str())),
            explicit_doc_task: terms.iter().any(|term| DOC_TERMS.contains(&term.as_str())),
            explicit_test_task: terms.iter().any(|term| TEST_TERMS.contains(&term.as_str())),
        }
    }
}

fn file_rank(result: &RepoSearchResult, context: &RankContext<'_>) -> u16 {
    if result.reason.contains("required") {
        0
    } else {
        let mut rank: u16 = if matches!(
            result.kind.as_str(),
            "Function" | "Struct" | "Class" | "Interface"
        ) {
            10
        } else if is_source_file(&result.file_path) {
            20
        } else if is_likely_test_file(&result.file_path) {
            30
        } else {
            50
        };
        if context.implementation_task && is_implementation_path(&result.file_path) {
            rank = rank.saturating_sub(8);
        }
        if context.explicit_test_task && is_likely_test_file(&result.file_path) {
            rank = rank.saturating_sub(20);
        }
        if is_broad_meta_doc(&result.file_path) && !context.explicit_doc_task {
            rank += 35;
        }
        if is_hidden_or_support_path(&result.file_path)
            && !context
                .path_scope
                .is_some_and(|scope| scope.trim_matches('/').starts_with('.'))
        {
            rank += 40;
        }
        rank
    }
}

fn relevance_rank(result: &RepoSearchResult, context: &RankContext<'_>) -> u16 {
    if context.terms.is_empty() {
        return 0;
    }
    context
        .terms
        .iter()
        .filter_map(|term| {
            let query = NormalizedText::for_query(term);
            best_match(
                &query,
                &[
                    &result.label,
                    &result.file_path,
                    &result.kind,
                    &result.reason,
                ],
                TokenMatchMode::AllowPartial,
            )
            .map(|score| (score.tier as u16 * 10) + score.missed_tokens as u16)
        })
        .min()
        .unwrap_or(200)
}

fn is_source_file(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or(""),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py"
    )
}

fn is_likely_test_file(path: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.contains("_test.")
        || path.contains("_tests.")
        || path.contains(".test.")
        || path.contains(".tests.")
        || path.contains("_spec.")
        || path.contains(".spec.")
}

fn is_implementation_path(path: &str) -> bool {
    is_source_file(path)
        && (path.starts_with("src/")
            || path.contains("/src/")
            || path.starts_with("crates/")
            || path.starts_with("engine/"))
}

fn is_broad_meta_doc(path: &str) -> bool {
    path == "README.md"
        || path == "AGENTS.md"
        || path.ends_with("/README.md")
        || path.ends_with("/AGENTS.md")
        || path.contains("template")
        || path.contains("example")
}

fn is_hidden_or_support_path(path: &str) -> bool {
    path.starts_with('.')
        || path.contains("/.")
        || path.starts_with("docs/")
        || path.starts_with("guide/")
        || path.contains("/fixtures/")
}

fn in_scope(path: &str, path_scope: Option<&str>) -> bool {
    let Some(scope) = path_scope else {
        return true;
    };
    let scope = scope.trim_matches('/');
    if scope.is_empty() {
        return true;
    }
    path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn explanatory_edges(
    snapshot: &RepoIndexSnapshot,
    likely_files: &[RepoSearchResult],
    limit: usize,
) -> Vec<GraphEdge> {
    let files: BTreeSet<_> = likely_files
        .iter()
        .map(|result| result.file_path.as_str())
        .collect();
    let mut edges: Vec<_> = snapshot
        .graph_edges()
        .into_iter()
        .filter(|edge| files.contains(edge.source.as_str()) || files.contains(edge.target.as_str()))
        .collect();
    edges.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then(left.target.cmp(&right.target))
            .then(left.relation.cmp(&right.relation))
    });
    edges.truncate(limit);
    edges
}

fn unique_paths(results: &[RepoSearchResult], limit: usize) -> Vec<String> {
    results
        .iter()
        .map(|result| result.file_path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(limit)
        .collect()
}

fn bundle_gaps(snapshot: &RepoIndexSnapshot, terms: &[String]) -> Vec<String> {
    let mut gaps = Vec::new();
    if snapshot.is_empty() {
        gaps.push("repo index is empty; fall back to direct file reads".to_string());
    }
    if terms.is_empty() {
        gaps.push("task did not include searchable repo terms".to_string());
    }
    gaps
}

fn trim_to_budget(bundle: &mut RepoContextBundle) {
    bundle.estimated_chars = estimate_bundle_chars(bundle);
    while bundle.estimated_chars > bundle.budget_chars && !bundle.graph_edges.is_empty() {
        bundle.graph_edges.pop();
        bundle.estimated_chars = estimate_bundle_chars(bundle);
    }
    while bundle.estimated_chars > bundle.budget_chars && !bundle.relevant_symbols.is_empty() {
        bundle.relevant_symbols.pop();
        bundle.estimated_chars = estimate_bundle_chars(bundle);
    }
    while bundle.estimated_chars > bundle.budget_chars && bundle.likely_files.len() > 1 {
        bundle.likely_files.pop();
        bundle.suggested_first_reads =
            unique_paths(&bundle.likely_files, bundle.suggested_first_reads.len());
        sync_first_read_spans(bundle);
        bundle.estimated_chars = estimate_bundle_chars(bundle);
    }
}

fn sync_first_read_spans(bundle: &mut RepoContextBundle) {
    let paths = bundle
        .likely_files
        .iter()
        .chain(bundle.relevant_symbols.iter())
        .map(|result| result.file_path.as_str())
        .collect::<BTreeSet<_>>();
    bundle
        .first_read_spans
        .retain(|span| paths.contains(span.file_path.as_str()));
}

fn estimate_bundle_chars(bundle: &RepoContextBundle) -> usize {
    bundle.task.len()
        + bundle
            .likely_files
            .iter()
            .map(|result| result.file_path.len() + result.label.len() + result.reason.len() + 32)
            .sum::<usize>()
        + bundle
            .relevant_symbols
            .iter()
            .map(|result| result.file_path.len() + result.label.len() + result.reason.len() + 32)
            .sum::<usize>()
        + bundle
            .graph_edges
            .iter()
            .map(|edge| edge.source.len() + edge.target.len() + 32)
            .sum::<usize>()
        + bundle
            .suggested_first_reads
            .iter()
            .map(String::len)
            .sum::<usize>()
        + bundle
            .first_read_spans
            .iter()
            .map(|span| span.file_path.len() + span.label.len() + span.chunk_id.len() + 32)
            .sum::<usize>()
        + bundle.test_targets.iter().map(String::len).sum::<usize>()
        + bundle.gaps.iter().map(String::len).sum::<usize>()
}

const IMPLEMENTATION_TERMS: &[&str] = &[
    "api", "bundle", "context", "crate", "crates", "engine", "index", "repo", "search", "symbol",
    "tool", "tools",
];

const DOC_TERMS: &[&str] = &["doc", "docs", "documentation", "guide", "readme"];

const TEST_TERMS: &[&str] = &["test", "tests", "tested", "testing", "coverage"];
