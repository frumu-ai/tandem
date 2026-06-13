use crate::model::{Confidence, RepoChunk, RepoContextSpan, RepoIndexSnapshot, RepoSearchResult};
use std::collections::{BTreeMap, BTreeSet};

pub fn repo_chunks(snapshot: &RepoIndexSnapshot) -> Vec<RepoChunk> {
    let hashes = file_hashes(snapshot);
    let mut chunks = Vec::new();
    for symbol in &snapshot.facts.symbols {
        let hash = hashes.get(&symbol.file_path).cloned().unwrap_or_default();
        chunks.push(chunk(
            &symbol.file_path,
            symbol.line,
            symbol.line,
            "source_symbol",
            &symbol.name,
            &hash,
            symbol.confidence.clone(),
        ));
    }

    let mut headings = snapshot.facts.doc_headings.clone();
    headings.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.line.cmp(&right.line))
    });
    for (index, heading) in headings.iter().enumerate() {
        let end_line = headings
            .iter()
            .skip(index + 1)
            .find(|candidate| candidate.file_path == heading.file_path)
            .map(|candidate| candidate.line.saturating_sub(1))
            .unwrap_or(heading.line);
        let hash = hashes.get(&heading.file_path).cloned().unwrap_or_default();
        chunks.push(chunk(
            &heading.file_path,
            heading.line,
            end_line.max(heading.line),
            "doc_section",
            &heading.title,
            &hash,
            heading.confidence.clone(),
        ));
    }

    chunks.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.start_line.cmp(&right.start_line))
            .then(left.kind.cmp(&right.kind))
            .then(left.label.cmp(&right.label))
    });
    chunks
}

pub fn first_read_spans(
    snapshot: &RepoIndexSnapshot,
    likely_files: &[RepoSearchResult],
    relevant_symbols: &[RepoSearchResult],
    limit: usize,
) -> Vec<RepoContextSpan> {
    let chunks = repo_chunks(snapshot);
    let mut seen = BTreeSet::new();
    let mut spans = Vec::new();
    for result in likely_files.iter().chain(relevant_symbols.iter()) {
        if let Some(chunk) = best_chunk_for_result(&chunks, result) {
            if seen.insert(chunk.id.clone()) {
                spans.push(span(chunk));
            }
        }
        if spans.len() >= limit {
            break;
        }
    }
    spans
}

fn best_chunk_for_result<'a>(
    chunks: &'a [RepoChunk],
    result: &RepoSearchResult,
) -> Option<&'a RepoChunk> {
    chunks
        .iter()
        .filter(|chunk| chunk.file_path == result.file_path)
        .find(|chunk| {
            (chunk.start_line <= result.line && result.line <= chunk.end_line)
                || chunk.label == result.label
        })
        .or_else(|| {
            chunks
                .iter()
                .find(|chunk| chunk.file_path == result.file_path)
        })
}

fn span(chunk: &RepoChunk) -> RepoContextSpan {
    RepoContextSpan {
        chunk_id: chunk.id.clone(),
        file_path: chunk.file_path.clone(),
        start_line: chunk.start_line,
        end_line: chunk.end_line,
        kind: chunk.kind.clone(),
        label: chunk.label.clone(),
        confidence: chunk.confidence.clone(),
    }
}

fn chunk(
    file_path: &str,
    start_line: usize,
    end_line: usize,
    kind: &str,
    label: &str,
    sha256: &str,
    confidence: Confidence,
) -> RepoChunk {
    RepoChunk {
        id: format!(
            "{}:{}-{}:{}",
            sha256,
            start_line,
            end_line,
            stable_label(label)
        ),
        file_path: file_path.to_string(),
        start_line,
        end_line,
        kind: kind.to_string(),
        label: label.to_string(),
        sha256: sha256.to_string(),
        confidence,
    }
}

fn file_hashes(snapshot: &RepoIndexSnapshot) -> BTreeMap<String, String> {
    snapshot
        .manifest
        .iter()
        .map(|entry| (entry.path.clone(), entry.sha256.clone()))
        .collect()
}

fn stable_label(label: &str) -> String {
    label
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .take(48)
        .collect()
}
