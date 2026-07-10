use crate::types::{DistillationReport, DistilledFact, FactCategory, MemoryResult};
use crate::{
    provider_egress::{complete_memory_prompt, MemoryProviderEgressKind},
    MemoryProviderEgressContext,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use tandem_providers::ProviderRegistry;

const DISTILLATION_PROMPT: &str = r#"You are analyzing a conversation session to extract memories.

## Conversation History:
{conversation}

## Task:
Extract and summarize important information from this conversation. Return a JSON array of objects with this format:
[
  {
    "category": "user_preference|task_outcome|learning|fact",
    "content": "The extracted information",
    "importance": 0.0-1.0,
    "follow_up_needed": true|false
  }
]

Only include information that would be valuable for future sessions.
"#;

pub struct SessionDistiller {
    providers: Arc<ProviderRegistry>,
    importance_threshold: f64,
    provider_egress: Option<MemoryProviderEgressContext>,
}

#[derive(Debug, Clone, Default)]
pub struct DistillationMemoryWrite {
    pub stored: bool,
    pub deduped: bool,
    pub memory_id: Option<String>,
    pub candidate_id: Option<String>,
}

#[async_trait]
pub trait DistillationMemoryWriter: Send + Sync {
    async fn store_user_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<DistillationMemoryWrite>;

    async fn store_agent_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<DistillationMemoryWrite>;
}

struct NoopDistillationMemoryWriter;

#[async_trait]
impl DistillationMemoryWriter for NoopDistillationMemoryWriter {
    async fn store_user_fact(
        &self,
        _session_id: &str,
        _fact: &DistilledFact,
    ) -> MemoryResult<DistillationMemoryWrite> {
        Ok(DistillationMemoryWrite::default())
    }

    async fn store_agent_fact(
        &self,
        _session_id: &str,
        _fact: &DistilledFact,
    ) -> MemoryResult<DistillationMemoryWrite> {
        Ok(DistillationMemoryWrite::default())
    }
}

impl SessionDistiller {
    pub fn new(providers: Arc<ProviderRegistry>) -> Self {
        Self {
            providers,
            importance_threshold: 0.5,
            provider_egress: None,
        }
    }

    pub fn with_threshold(providers: Arc<ProviderRegistry>, importance_threshold: f64) -> Self {
        Self {
            providers,
            importance_threshold,
            provider_egress: None,
        }
    }

    pub fn with_provider_egress(mut self, provider_egress: MemoryProviderEgressContext) -> Self {
        self.provider_egress = Some(provider_egress);
        self
    }

    pub async fn distill(
        &self,
        session_id: &str,
        conversation: &[String],
    ) -> MemoryResult<DistillationReport> {
        self.distill_with_writer(session_id, conversation, &NoopDistillationMemoryWriter)
            .await
    }

    pub async fn distill_with_writer<W: DistillationMemoryWriter>(
        &self,
        session_id: &str,
        conversation: &[String],
        writer: &W,
    ) -> MemoryResult<DistillationReport> {
        let distillation_id = uuid::Uuid::new_v4().to_string();
        let full_text = conversation.join("\n\n---\n\n");
        let token_count = self.count_tokens(&full_text);

        if token_count < 50 {
            return Ok(DistillationReport {
                distillation_id,
                session_id: session_id.to_string(),
                distilled_at: Utc::now(),
                facts_extracted: 0,
                importance_threshold: self.importance_threshold,
                user_memory_updated: false,
                agent_memory_updated: false,
                stored_count: 0,
                deduped_count: 0,
                memory_ids: Vec::new(),
                candidate_ids: Vec::new(),
                status: "skipped_short_conversation".to_string(),
            });
        }

        let facts = self.extract_facts(&full_text, &distillation_id).await?;

        let filtered_facts: Vec<&DistilledFact> = facts
            .iter()
            .filter(|f| f.importance_score >= self.importance_threshold)
            .collect();

        Self::build_distillation_report(
            distillation_id,
            session_id,
            self.importance_threshold,
            &filtered_facts,
            writer,
        )
        .await
    }

    async fn build_distillation_report<W: DistillationMemoryWriter>(
        distillation_id: String,
        session_id: &str,
        importance_threshold: f64,
        filtered_facts: &[&DistilledFact],
        writer: &W,
    ) -> MemoryResult<DistillationReport> {
        if filtered_facts.is_empty() {
            return Ok(DistillationReport {
                distillation_id,
                session_id: session_id.to_string(),
                distilled_at: Utc::now(),
                facts_extracted: 0,
                importance_threshold,
                user_memory_updated: false,
                agent_memory_updated: false,
                stored_count: 0,
                deduped_count: 0,
                memory_ids: Vec::new(),
                candidate_ids: Vec::new(),
                status: "no_important_facts".to_string(),
            });
        }

        let user_results =
            Self::write_user_memory_facts(session_id, filtered_facts, writer).await?;
        let agent_results =
            Self::write_agent_memory_facts(session_id, filtered_facts, writer).await?;
        let all_results = user_results
            .iter()
            .chain(agent_results.iter())
            .cloned()
            .collect::<Vec<_>>();
        let stored_count = all_results.iter().filter(|row| row.stored).count();
        let deduped_count = all_results.iter().filter(|row| row.deduped).count();
        let memory_ids = all_results
            .iter()
            .filter_map(|row| row.memory_id.clone())
            .collect::<Vec<_>>();
        let candidate_ids = all_results
            .iter()
            .filter_map(|row| row.candidate_id.clone())
            .collect::<Vec<_>>();

        Ok(DistillationReport {
            distillation_id,
            session_id: session_id.to_string(),
            distilled_at: Utc::now(),
            facts_extracted: filtered_facts.len(),
            importance_threshold,
            user_memory_updated: !user_results.is_empty(),
            agent_memory_updated: !agent_results.is_empty(),
            stored_count,
            deduped_count,
            memory_ids,
            candidate_ids,
            status: if stored_count > 0 || deduped_count > 0 {
                "stored".to_string()
            } else {
                "facts_extracted_only".to_string()
            },
        })
    }

    async fn extract_facts(
        &self,
        conversation: &str,
        distillation_id: &str,
    ) -> MemoryResult<Vec<DistilledFact>> {
        let prompt = DISTILLATION_PROMPT.replace("{conversation}", conversation);

        let response = match complete_memory_prompt(
            &self.providers,
            &prompt,
            None,
            None,
            self.provider_egress.as_ref(),
            MemoryProviderEgressKind::Distillation,
            distillation_id,
            "memory.session_distillation",
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Distillation LLM failed: {}", e);
                return Ok(Vec::new());
            }
        };

        let extracted: Vec<ExtractedFact> = match serde_json::from_str(&response) {
            Ok(facts) => facts,
            Err(e) => {
                tracing::warn!("Failed to parse distillation response: {}", e);
                return Ok(Vec::new());
            }
        };

        let facts: Vec<DistilledFact> = extracted
            .into_iter()
            .map(|e| DistilledFact {
                id: uuid::Uuid::new_v4().to_string(),
                distillation_id: distillation_id.to_string(),
                content: e.content,
                category: parse_category(&e.category),
                importance_score: e.importance,
                source_message_ids: Vec::new(),
                contradicts_fact_id: None,
            })
            .collect();

        Ok(facts)
    }

    async fn write_user_memory_facts(
        session_id: &str,
        facts: &[&DistilledFact],
        writer: &impl DistillationMemoryWriter,
    ) -> MemoryResult<Vec<DistillationMemoryWrite>> {
        if facts.is_empty() {
            return Ok(Vec::new());
        }

        let user_facts: Vec<&DistilledFact> = facts
            .iter()
            .filter(|f| {
                matches!(
                    f.category,
                    FactCategory::UserPreference | FactCategory::Fact
                )
            })
            .cloned()
            .collect();

        if user_facts.is_empty() {
            return Ok(Vec::new());
        }

        let mut writes = Vec::new();
        for fact in user_facts {
            writes.push(writer.store_user_fact(session_id, fact).await?);
        }
        Ok(writes)
    }

    async fn write_agent_memory_facts(
        session_id: &str,
        facts: &[&DistilledFact],
        writer: &impl DistillationMemoryWriter,
    ) -> MemoryResult<Vec<DistillationMemoryWrite>> {
        if facts.is_empty() {
            return Ok(Vec::new());
        }

        let agent_facts: Vec<&DistilledFact> = facts
            .iter()
            .filter(|f| {
                matches!(
                    f.category,
                    FactCategory::TaskOutcome | FactCategory::Learning
                )
            })
            .cloned()
            .collect();

        if agent_facts.is_empty() {
            return Ok(Vec::new());
        }

        let mut writes = Vec::new();
        for fact in agent_facts {
            writes.push(writer.store_agent_fact(session_id, fact).await?);
        }
        Ok(writes)
    }

    fn count_tokens(&self, text: &str) -> i64 {
        tiktoken_rs::cl100k_base()
            .map(|bpe| bpe.encode_with_special_tokens(text).len() as i64)
            .unwrap_or((text.len() / 4) as i64)
    }
}

fn parse_category(s: &str) -> FactCategory {
    match s.to_lowercase().as_str() {
        "user_preference" => FactCategory::UserPreference,
        "task_outcome" => FactCategory::TaskOutcome,
        "learning" => FactCategory::Learning,
        _ => FactCategory::Fact,
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct ExtractedFact {
    category: String,
    content: String,
    importance: f64,
    #[serde(default)]
    follow_up_needed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingWriter {
        user_calls: Arc<Mutex<Vec<String>>>,
        agent_calls: Arc<Mutex<Vec<String>>>,
        dedupe: bool,
    }

    #[async_trait]
    impl DistillationMemoryWriter for RecordingWriter {
        async fn store_user_fact(
            &self,
            _session_id: &str,
            fact: &DistilledFact,
        ) -> MemoryResult<DistillationMemoryWrite> {
            self.user_calls
                .lock()
                .expect("user calls")
                .push(fact.id.clone());
            Ok(DistillationMemoryWrite {
                stored: !self.dedupe,
                deduped: self.dedupe,
                memory_id: Some(format!("memory-{}", fact.id)),
                candidate_id: Some(format!("candidate-{}", fact.id)),
            })
        }

        async fn store_agent_fact(
            &self,
            _session_id: &str,
            fact: &DistilledFact,
        ) -> MemoryResult<DistillationMemoryWrite> {
            self.agent_calls
                .lock()
                .expect("agent calls")
                .push(fact.id.clone());
            Ok(DistillationMemoryWrite {
                stored: !self.dedupe,
                deduped: self.dedupe,
                memory_id: Some(format!("memory-{}", fact.id)),
                candidate_id: Some(format!("candidate-{}", fact.id)),
            })
        }
    }

    fn test_fact(id: &str, category: FactCategory) -> DistilledFact {
        DistilledFact {
            id: id.to_string(),
            distillation_id: "distill-test".to_string(),
            content: format!("content for {id}"),
            category,
            importance_score: 0.9,
            source_message_ids: Vec::new(),
            contradicts_fact_id: None,
        }
    }

    #[test]
    fn test_parse_category() {
        assert_eq!(
            parse_category("user_preference"),
            FactCategory::UserPreference
        );
        assert_eq!(parse_category("task_outcome"), FactCategory::TaskOutcome);
        assert_eq!(parse_category("learning"), FactCategory::Learning);
        assert_eq!(parse_category("unknown"), FactCategory::Fact);
    }

    #[tokio::test]
    async fn test_distiller_requires_conversation() {
        // This test would require ProviderRegistry mock
        // Placeholder for now
    }

    #[tokio::test]
    async fn build_distillation_report_routes_user_and_agent_facts_to_writer() {
        let writer = RecordingWriter::default();
        let facts = vec![
            test_fact("fact-user-preference", FactCategory::UserPreference),
            test_fact("fact-user-fact", FactCategory::Fact),
            test_fact("fact-agent-outcome", FactCategory::TaskOutcome),
            test_fact("fact-agent-learning", FactCategory::Learning),
        ];
        let fact_refs = facts.iter().collect::<Vec<_>>();

        let report = SessionDistiller::build_distillation_report(
            "distill-1".to_string(),
            "session-1",
            0.5,
            &fact_refs,
            &writer,
        )
        .await
        .expect("distillation report");

        assert_eq!(report.facts_extracted, 4);
        assert!(report.user_memory_updated);
        assert!(report.agent_memory_updated);
        assert_eq!(report.stored_count, 4);
        assert_eq!(report.deduped_count, 0);
        assert_eq!(report.memory_ids.len(), 4);
        assert_eq!(report.candidate_ids.len(), 4);
        assert_eq!(report.status, "stored");

        let user_calls = writer.user_calls.lock().expect("user calls").clone();
        let agent_calls = writer.agent_calls.lock().expect("agent calls").clone();
        assert_eq!(user_calls.len(), 2);
        assert_eq!(agent_calls.len(), 2);
        assert!(user_calls.iter().any(|id| id == "fact-user-preference"));
        assert!(user_calls.iter().any(|id| id == "fact-user-fact"));
        assert!(agent_calls.iter().any(|id| id == "fact-agent-outcome"));
        assert!(agent_calls.iter().any(|id| id == "fact-agent-learning"));
    }

    #[tokio::test]
    async fn build_distillation_report_counts_deduped_writes_as_stored_status() {
        let writer = RecordingWriter {
            dedupe: true,
            ..RecordingWriter::default()
        };
        let facts = vec![test_fact("fact-deduped", FactCategory::Fact)];
        let fact_refs = facts.iter().collect::<Vec<_>>();

        let report = SessionDistiller::build_distillation_report(
            "distill-2".to_string(),
            "session-2",
            0.5,
            &fact_refs,
            &writer,
        )
        .await
        .expect("distillation report");

        assert_eq!(report.facts_extracted, 1);
        assert_eq!(report.stored_count, 0);
        assert_eq!(report.deduped_count, 1);
        assert_eq!(report.status, "stored");
        assert_eq!(report.memory_ids, vec!["memory-fact-deduped".to_string()]);
        assert_eq!(
            report.candidate_ids,
            vec!["candidate-fact-deduped".to_string()]
        );
    }
}
