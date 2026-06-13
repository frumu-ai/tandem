use crate::model::{RepoChunk, RepoHybridCandidate};
use std::collections::BTreeMap;

pub trait RepoHybridRetriever {
    fn retrieve(&self, query: &str, chunks: &[RepoChunk]) -> Vec<RepoHybridCandidate>;
}

pub fn merge_hybrid_candidates(
    graph_candidates: Vec<RepoHybridCandidate>,
    vector_candidates: Vec<RepoHybridCandidate>,
    limit: usize,
) -> Vec<RepoHybridCandidate> {
    let mut merged: BTreeMap<String, RepoHybridCandidate> = BTreeMap::new();
    for candidate in graph_candidates.into_iter().chain(vector_candidates) {
        merged
            .entry(candidate.chunk_id.clone())
            .and_modify(|existing| {
                existing.score = existing.score.saturating_add(candidate.score);
                for retriever in &candidate.retrievers {
                    if !existing.retrievers.contains(retriever) {
                        existing.retrievers.push(retriever.clone());
                    }
                }
                existing.provenance.push_str("; ");
                existing.provenance.push_str(&candidate.provenance);
            })
            .or_insert(candidate);
    }

    let mut ranked = merged.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(left.file_path.cmp(&right.file_path))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    ranked.truncate(limit.max(1));
    ranked
}
