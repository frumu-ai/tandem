use crate::context::repo_impact;
use crate::model::{
    Confidence, GraphEdge, RepoContextBundle, RepoContextBundleOptions, RepoImpactItem,
    RepoIndexSnapshot, RepoSearchResult,
};
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

    let likely_files = ranked_results(likely_files, limit);
    let relevant_symbols = ranked_results(relevant_symbols, limit);
    let graph_edges = explanatory_edges(snapshot, &likely_files, limit);
    let suggested_first_reads = unique_paths(&likely_files, limit);
    let mut bundle = RepoContextBundle {
        task: task.to_string(),
        budget_chars: options.budget_chars,
        likely_files,
        relevant_symbols,
        graph_edges,
        suggested_first_reads,
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
            )
        })
        .collect()
}

fn task_terms(task: &str) -> Vec<String> {
    let mut terms: Vec<_> = task
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        .map(|term| term.trim_matches(['-', '_']).to_lowercase())
        .filter(|term| term.len() >= 3 && !STOP_WORDS.contains(&term.as_str()))
        .collect();
    terms.sort();
    terms.dedup();
    terms.truncate(8);
    terms
}

fn search_result(
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
            )
        })
        .collect()
}

fn ranked_results(results: Vec<RepoSearchResult>, limit: usize) -> Vec<RepoSearchResult> {
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
        file_rank(left)
            .cmp(&file_rank(right))
            .then(left.file_path.cmp(&right.file_path))
            .then(left.line.cmp(&right.line))
            .then(left.kind.cmp(&right.kind))
            .then(left.label.cmp(&right.label))
    });
    ranked.truncate(limit);
    ranked
}

fn file_rank(result: &RepoSearchResult) -> u8 {
    if result.reason.contains("required") {
        0
    } else if matches!(
        result.kind.as_str(),
        "Function" | "Struct" | "Class" | "Interface"
    ) {
        1
    } else if is_source_file(&result.file_path) {
        2
    } else if is_likely_test_file(&result.file_path) {
        3
    } else {
        4
    }
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
        || path.contains(".test.")
        || path.contains("_spec.")
        || path.contains(".spec.")
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
        bundle.estimated_chars = estimate_bundle_chars(bundle);
    }
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
        + bundle.test_targets.iter().map(String::len).sum::<usize>()
        + bundle.gaps.iter().map(String::len).sum::<usize>()
}

const STOP_WORDS: &[&str] = &[
    "and",
    "the",
    "for",
    "with",
    "from",
    "into",
    "this",
    "that",
    "task",
    "update",
    "change",
    "fix",
    "add",
    "implement",
];
