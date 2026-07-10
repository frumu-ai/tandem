use crate::types::{LayerType, MemoryError, MemoryResult};
use crate::{
    provider_egress::{complete_memory_prompt, MemoryProviderEgressKind},
    MemoryProviderEgressContext,
};
use std::sync::Arc;
use tandem_providers::ProviderRegistry;

const L0_SUMMARY_PROMPT: &str = r#"You are a text abstraction engine. Given the input text, generate a very brief abstract (~100 tokens or less) that captures the core essence.

Requirements:
- One sentence that summarizes the entire content
- Focus on what this is about, not details
- Be concise and direct
- Start with the main topic/action

Input text:
"#;

const L1_OVERVIEW_PROMPT: &str = r#"You are a text summarization engine. Given the input text, generate an overview (~2000 tokens or less) that captures the key information.

Requirements:
- Start with a brief introduction (1-2 sentences)
- Cover main topics, key decisions, important facts
- Omit conversational filler, greetings, and sign-offs
- Use bullet points for lists where appropriate
- End with any unresolved issues or follow-up items if relevant

Input text:
"#;

pub struct ContextLayerGenerator {
    providers: Arc<ProviderRegistry>,
    provider_egress: Option<MemoryProviderEgressContext>,
}

impl ContextLayerGenerator {
    pub fn new(providers: Arc<ProviderRegistry>) -> Self {
        Self {
            providers,
            provider_egress: None,
        }
    }

    pub fn with_provider_egress(mut self, provider_egress: MemoryProviderEgressContext) -> Self {
        self.provider_egress = Some(provider_egress);
        self
    }

    pub async fn generate_l0(&self, content: &str) -> MemoryResult<String> {
        let prompt = format!("{}{}", L0_SUMMARY_PROMPT, content);
        self.call_llm(&prompt, "summarize-l0").await
    }

    pub async fn generate_l1(&self, content: &str) -> MemoryResult<String> {
        let prompt = format!("{}{}", L1_OVERVIEW_PROMPT, content);
        self.call_llm(&prompt, "summarize-l1").await
    }

    pub async fn generate_layers(&self, l2_content: &str) -> MemoryResult<(String, String)> {
        let l0 = self.generate_l0(l2_content).await?;
        let l1 = self.generate_l1(l2_content).await?;
        Ok((l0, l1))
    }

    async fn call_llm(&self, prompt: &str, model_hint: &str) -> MemoryResult<String> {
        match complete_memory_prompt(
            &self.providers,
            prompt,
            None,
            None,
            self.provider_egress.as_ref(),
            MemoryProviderEgressKind::ContextLayer,
            model_hint,
            "memory.context_layer",
        )
        .await
        {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::warn!("LLM call failed for layer generation: {}", e);
                Err(MemoryError::Embedding(format!(
                    "failed to generate context layer: {}",
                    e
                )))
            }
        }
    }
}

pub fn layer_type_token_limit(layer_type: LayerType) -> usize {
    match layer_type {
        LayerType::L0 => 150,
        LayerType::L1 => 2500,
        LayerType::L2 => 8000,
    }
}

pub fn truncate_to_layer(content: &str, layer_type: LayerType) -> String {
    let limit = layer_type_token_limit(layer_type);
    let char_limit = limit * 4;

    if content.len() <= char_limit {
        return content.to_string();
    }

    let truncated = &content[..char_limit];
    if let Some(last_newline) = truncated.rfind('\n') {
        format!("{}...", truncated[..last_newline].trim())
    } else if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", truncated[..last_space].trim())
    } else {
        format!("{}...", truncated.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_token_limits() {
        assert_eq!(layer_type_token_limit(LayerType::L0), 150);
        assert_eq!(layer_type_token_limit(LayerType::L1), 2500);
        assert_eq!(layer_type_token_limit(LayerType::L2), 8000);
    }

    #[test]
    fn test_truncate_to_layer() {
        let long_text = "a".repeat(10000);
        let truncated = truncate_to_layer(&long_text, LayerType::L0);
        assert!(truncated.len() < 10000);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_truncate_preserves_short() {
        let short_text = "This is a short text.";
        let result = truncate_to_layer(short_text, LayerType::L1);
        assert_eq!(result, short_text);
    }
}
