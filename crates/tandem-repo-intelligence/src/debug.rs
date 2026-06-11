use crate::model::{
    RepoContextBundle, RepoContextBundleMetrics, RepoDebugExport, RepoIndexMetrics,
    RepoIndexSnapshot, RepoIntelligenceEvent,
};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn repo_index_metrics(snapshot: &RepoIndexSnapshot) -> RepoIndexMetrics {
    let graph_edges = snapshot.graph_edges();
    RepoIndexMetrics {
        indexed_unix_ms: snapshot.indexed_unix_ms,
        files_indexed: snapshot.manifest.len(),
        total_bytes: snapshot.manifest.iter().map(|entry| entry.size_bytes).sum(),
        symbols: snapshot.facts.symbols.len(),
        imports: snapshot.facts.imports.len(),
        config_references: snapshot.facts.config_references.len(),
        doc_headings: snapshot.facts.doc_headings.len(),
        graph_edges: graph_edges.len(),
        top_file_sources: top_file_sources(snapshot),
    }
}

pub fn repo_debug_export(snapshot: &RepoIndexSnapshot) -> RepoDebugExport {
    RepoDebugExport {
        root_label: snapshot.root_label.clone(),
        generated_unix_ms: now_unix_ms(),
        metrics: repo_index_metrics(snapshot),
        graph_edges: snapshot.graph_edges(),
    }
}

pub fn repo_context_bundle_metrics(bundle: &RepoContextBundle) -> RepoContextBundleMetrics {
    RepoContextBundleMetrics {
        task: bundle.task.clone(),
        estimated_chars: bundle.estimated_chars,
        budget_chars: bundle.budget_chars,
        likely_files: bundle.likely_files.len(),
        relevant_symbols: bundle.relevant_symbols.len(),
        graph_edges: bundle.graph_edges.len(),
        suggested_first_reads: bundle.suggested_first_reads.len(),
        test_targets: bundle.test_targets.len(),
        gaps: bundle.gaps.len(),
        top_sources: bundle
            .suggested_first_reads
            .iter()
            .take(8)
            .cloned()
            .collect(),
    }
}

pub fn repo_intelligence_event(
    event: impl Into<String>,
    repo_root: impl Into<String>,
    metrics: Option<RepoIndexMetrics>,
    error: Option<String>,
) -> RepoIntelligenceEvent {
    RepoIntelligenceEvent {
        event: event.into(),
        repo_root: repo_root.into(),
        unix_ms: now_unix_ms(),
        metrics,
        error,
    }
}

fn top_file_sources(snapshot: &RepoIndexSnapshot) -> Vec<String> {
    let mut scores: BTreeMap<String, usize> = BTreeMap::new();
    for symbol in &snapshot.facts.symbols {
        *scores.entry(symbol.file_path.clone()).or_default() += 3;
    }
    for import in &snapshot.facts.imports {
        *scores.entry(import.source_file.clone()).or_default() += 2;
    }
    for reference in &snapshot.facts.config_references {
        *scores.entry(reference.file_path.clone()).or_default() += 2;
    }
    for heading in &snapshot.facts.doc_headings {
        *scores.entry(heading.file_path.clone()).or_default() += 1;
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().map(|(path, _)| path).take(12).collect()
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
