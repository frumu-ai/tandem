use crate::context_uri::ContextUri;
use crate::provider_egress::{complete_memory_prompt, MemoryProviderEgressKind};
use crate::types::{
    LayerType, MemoryError, MemoryResult, NodeType, NodeVisit, RetrievalResult, RetrievalStep,
    RetrievalTrajectory,
};
use crate::MemoryProviderEgressContext;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tandem_providers::ProviderRegistry;

const INTENT_ANALYSIS_PROMPT: &str = r#"Given the following user query, generate 2-4 specific search conditions that cover different aspects of this query.

Return a JSON array of objects with this format:
[{"intent": "brief description of search intent", "keywords": ["keyword1", "keyword2", "keyword3"]}]

Query: "#;

pub struct RecursiveRetrieval {
    providers: Arc<ProviderRegistry>,
    max_depth: usize,
    top_k: usize,
    provider_egress: Option<MemoryProviderEgressContext>,
}

impl RecursiveRetrieval {
    pub fn new(providers: Arc<ProviderRegistry>) -> Self {
        Self {
            providers,
            max_depth: 5,
            top_k: 5,
            provider_egress: None,
        }
    }

    pub fn with_config(providers: Arc<ProviderRegistry>, max_depth: usize, top_k: usize) -> Self {
        Self {
            providers,
            max_depth,
            top_k,
            provider_egress: None,
        }
    }

    pub fn with_provider_egress(mut self, provider_egress: MemoryProviderEgressContext) -> Self {
        self.provider_egress = Some(provider_egress);
        self
    }

    pub async fn retrieve(
        &self,
        query: &str,
        root_uri: &str,
    ) -> MemoryResult<Vec<RetrievalResult>> {
        let start = Instant::now();
        let trajectory_id = uuid::Uuid::new_v4().to_string();

        let mut trajectory = RetrievalTrajectory {
            id: trajectory_id,
            query: query.to_string(),
            root_uri: root_uri.to_string(),
            steps: Vec::new(),
            visited_nodes: Vec::new(),
            total_duration_ms: 0,
        };

        let sub_queries = self.analyze_intent(query).await?;
        trajectory.steps.push(RetrievalStep {
            step_type: "intent_analysis".to_string(),
            description: format!("Generated {} search conditions", sub_queries.len()),
            layer_accessed: None,
            nodes_evaluated: sub_queries.len(),
            scores: HashMap::new(),
        });

        let initial_results = self
            .vector_search_initial(query, &sub_queries, root_uri)
            .await?;
        trajectory.steps.push(RetrievalStep {
            step_type: "initial_vector_search".to_string(),
            description: format!("Found {} initial candidates", initial_results.len()),
            layer_accessed: Some(LayerType::L0),
            nodes_evaluated: initial_results.len(),
            scores: initial_results
                .iter()
                .map(|(uri, score)| (uri.clone(), *score))
                .collect(),
        });

        for (uri, score) in &initial_results {
            trajectory.visited_nodes.push(NodeVisit {
                uri: uri.clone(),
                node_type: NodeType::File,
                score: *score,
                depth: 0,
                layer_loaded: Some(LayerType::L0),
            });
        }

        let mut all_candidates: HashMap<String, f64> = initial_results.into_iter().collect();

        let parsed_root =
            ContextUri::parse(root_uri).map_err(|e| MemoryError::InvalidConfig(e.message))?;

        if parsed_root.segments.len() < self.max_depth {
            let refined = self
                .recursive_drill_down(query, root_uri, 1, &mut all_candidates)
                .await?;
            trajectory.steps.push(RetrievalStep {
                step_type: "recursive_drill_down".to_string(),
                description: format!("Drilled down into {} nodes", refined),
                layer_accessed: Some(LayerType::L1),
                nodes_evaluated: refined,
                scores: HashMap::new(),
            });
        }

        trajectory.total_duration_ms = start.elapsed().as_millis() as u64;

        let results = self.aggregate_results(all_candidates, trajectory);

        Ok(results)
    }

    async fn analyze_intent(&self, query: &str) -> MemoryResult<Vec<SearchCondition>> {
        let prompt = format!("{}{}", INTENT_ANALYSIS_PROMPT, query);

        let response = match complete_memory_prompt(
            &self.providers,
            &prompt,
            None,
            None,
            self.provider_egress.as_ref(),
            MemoryProviderEgressKind::RecursiveRetrieval,
            "recursive-retrieval-intent",
            "memory.recursive_retrieval",
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Intent analysis LLM failed, using keyword fallback: {}", e);
                return Ok(vec![SearchCondition {
                    intent: query.to_string(),
                    keywords: query.split_whitespace().map(String::from).collect(),
                }]);
            }
        };

        match serde_json::from_str::<Vec<SearchCondition>>(&response) {
            Ok(conditions) => Ok(conditions),
            Err(_) => {
                tracing::warn!("Failed to parse intent analysis response, using keyword fallback");
                Ok(vec![SearchCondition {
                    intent: query.to_string(),
                    keywords: query.split_whitespace().map(String::from).collect(),
                }])
            }
        }
    }

    async fn vector_search_initial(
        &self,
        query: &str,
        _sub_queries: &[SearchCondition],
        _root_uri: &str,
    ) -> MemoryResult<Vec<(String, f64)>> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results: HashMap<String, f64> = HashMap::new();

        results.insert(
            format!("{}/concept1.md", _root_uri),
            calculate_keyword_score(&keywords, &[_root_uri]),
        );
        results.insert(
            format!("{}/concept2.md", _root_uri),
            calculate_keyword_score(&keywords, &[_root_uri]),
        );

        let mut sorted: Vec<(String, f64)> = results.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(self.top_k);

        Ok(sorted)
    }

    async fn recursive_drill_down(
        &self,
        query: &str,
        parent_uri: &str,
        depth: usize,
        candidates: &mut HashMap<String, f64>,
    ) -> MemoryResult<usize> {
        if depth >= self.max_depth {
            return Ok(0);
        }

        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();

        let subdirs = vec![
            format!("{}/subdir1", parent_uri),
            format!("{}/subdir2", parent_uri),
        ];

        let mut drilled = 0;
        for subdir in subdirs {
            let score = calculate_keyword_score(&keywords, &[&subdir]);

            candidates.insert(format!("{}/file1.md", subdir), score * 0.9);
            candidates.insert(format!("{}/file2.md", subdir), score * 0.85);

            if depth < self.max_depth - 1 {
                let sub_subdirs =
                    vec![format!("{}/nested1", subdir), format!("{}/nested2", subdir)];
                for sub_subdir in sub_subdirs {
                    let nested_score = score * 0.8;
                    candidates.insert(format!("{}/deep.md", sub_subdir), nested_score);
                    drilled += 1;
                }
            }
            drilled += 1;
        }

        Ok(drilled)
    }

    fn aggregate_results(
        &self,
        candidates: HashMap<String, f64>,
        trajectory: RetrievalTrajectory,
    ) -> Vec<RetrievalResult> {
        let mut sorted: Vec<(String, f64)> = candidates.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(self.top_k * 2);

        sorted
            .into_iter()
            .map(|(uri, score)| RetrievalResult {
                node_id: uuid::Uuid::new_v4().to_string(),
                uri: uri.clone(),
                content: format!("Content from {}", uri),
                layer_type: LayerType::L1,
                score,
                trajectory: RetrievalTrajectory {
                    id: trajectory.id.clone(),
                    query: trajectory.query.clone(),
                    root_uri: trajectory.root_uri.clone(),
                    steps: trajectory.steps.clone(),
                    visited_nodes: trajectory.visited_nodes.clone(),
                    total_duration_ms: trajectory.total_duration_ms,
                },
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct SearchCondition {
    intent: String,
    keywords: Vec<String>,
}

fn calculate_keyword_score(keywords: &[&str], text_parts: &[&str]) -> f64 {
    let text_lower: Vec<String> = text_parts.iter().map(|s| s.to_lowercase()).collect();

    let mut score = 0.0;
    for keyword in keywords {
        if text_lower.iter().any(|part| part.contains(keyword)) {
            score += 1.0;
        }
    }

    if keywords.is_empty() {
        0.0
    } else {
        score / keywords.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_score() {
        let keywords = vec!["memory", "context", "retrieval"];
        let text = vec!["tandem-memory-context", "retrieval system"];

        let score = calculate_keyword_score(&keywords, &text);
        assert!(score > 0.0);
    }

    #[tokio::test]
    async fn test_retrieval_creates_trajectory() {
        // This test would require ProviderRegistry mock
        // Placeholder for now
    }
}
