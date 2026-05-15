/// Scripted Provider for Eval Engine Mode
///
/// Implements the `Provider` trait with deterministic, pattern-based responses so the
/// eval-runner can exercise the full Tandem engine path (`create_automation_v2_run`,
/// repair loops, validator chokepoints) without making real AI calls.
///
/// Pattern matching is substring-based against the assembled prompt. Tests register
/// patterns up-front; if no pattern matches, a kitchen-sink JSON response is returned
/// that includes the fields most output-contract validators look for (summary, content,
/// citations, web_sources, code, status). Test cases that need different shapes register
/// a tailored response via `.with_pattern(...)`.
///
/// Used by `EngineExecutor` in `--engine-mode stub` runs of the eval-runner CLI.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use tandem_providers::{ChatMessage, Provider, StreamChunk, TokenUsage};
use tandem_types::{ModelInfo, ProviderInfo, ToolMode, ToolSchema};

pub const SCRIPTED_PROVIDER_ID: &str = "scripted-eval";
pub const SCRIPTED_MODEL_ID: &str = "scripted-eval-1";
pub const SCRIPTED_MODEL_CONTEXT_WINDOW: usize = 32_768;

/// A scripted response shape.
#[derive(Debug, Clone)]
pub enum ScriptedResponse {
    /// Return a plain text body.
    Text(String),
    /// Return a JSON value, serialized to a string.
    Json(serde_json::Value),
}

impl ScriptedResponse {
    fn to_body(&self) -> String {
        match self {
            ScriptedResponse::Text(s) => s.clone(),
            ScriptedResponse::Json(v) => v.to_string(),
        }
    }
}

/// Deterministic provider used by the eval-runner stub engine mode.
pub struct ScriptedEvalProvider {
    /// (substring_to_match_in_prompt, response). Patterns are checked in registration order.
    patterns: Vec<(String, ScriptedResponse)>,
    /// Fallback response when no pattern matches.
    default_response: ScriptedResponse,
    /// Captured prompts, for test inspection.
    call_log: Arc<Mutex<Vec<String>>>,
}

impl Default for ScriptedEvalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedEvalProvider {
    /// Construct with a kitchen-sink default response. Tests should add patterns for
    /// scenarios that need specific output shapes.
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            default_response: ScriptedResponse::Json(default_completion_json()),
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a substring pattern. The first pattern whose substring is found in the
    /// assembled prompt wins.
    pub fn with_pattern(
        mut self,
        contains: impl Into<String>,
        response: ScriptedResponse,
    ) -> Self {
        self.patterns.push((contains.into(), response));
        self
    }

    /// Replace the default (fallback) response.
    pub fn with_default(mut self, response: ScriptedResponse) -> Self {
        self.default_response = response;
        self
    }

    /// Returns a snapshot of every prompt this provider has observed.
    pub async fn call_log(&self) -> Vec<String> {
        self.call_log.lock().await.clone()
    }

    fn match_response(&self, prompt: &str) -> ScriptedResponse {
        for (needle, response) in &self.patterns {
            if prompt.contains(needle) {
                return response.clone();
            }
        }
        self.default_response.clone()
    }
}

/// Rough token estimate: 1 token per 4 chars. Stable and good enough for the simulated
/// cost/budget metrics the eval-runner reports — we don't need real tokenization here.
fn estimate_tokens(text: &str) -> u64 {
    let len = text.len() as u64;
    if len == 0 {
        0
    } else {
        len.div_ceil(4)
    }
}

fn assemble_prompt(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The kitchen-sink default response — contains the fields most output-contract validators
/// look for so it can satisfy a wide range of test cases without per-case configuration.
fn default_completion_json() -> serde_json::Value {
    json!({
        "status": "completed",
        "summary": "Scripted eval response: task completed by the deterministic stub provider.",
        "content": "Scripted eval response body. Use ScriptedEvalProvider::with_pattern to customize.",
        "citations": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2",
            "https://example.com/scripted-eval/source-3"
        ],
        "web_sources": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "web_sources_reviewed": [
            "https://example.com/scripted-eval/source-1",
            "https://example.com/scripted-eval/source-2"
        ],
        "code": "// scripted eval stub\nfn stub() -> bool { true }",
        "bullets": [
            "Point one from the scripted provider.",
            "Point two from the scripted provider."
        ]
    })
}

#[async_trait]
impl Provider for ScriptedEvalProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: SCRIPTED_PROVIDER_ID.to_string(),
            name: "Scripted Eval".to_string(),
            models: vec![ModelInfo {
                id: SCRIPTED_MODEL_ID.to_string(),
                provider_id: SCRIPTED_PROVIDER_ID.to_string(),
                display_name: "Scripted Eval Stub".to_string(),
                context_window: SCRIPTED_MODEL_CONTEXT_WINDOW,
            }],
        }
    }

    async fn complete(
        &self,
        prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        self.call_log.lock().await.push(prompt.to_string());
        Ok(self.match_response(prompt).to_body())
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt = assemble_prompt(&messages);
        self.call_log.lock().await.push(prompt.clone());

        let body = self.match_response(&prompt).to_body();
        let prompt_tokens = estimate_tokens(&prompt);
        let completion_tokens = estimate_tokens(&body);

        let chunks: Vec<anyhow::Result<StreamChunk>> = vec![
            Ok(StreamChunk::TextDelta(body)),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: Some(TokenUsage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                }),
            }),
        ];

        Ok(Box::pin(futures::stream::iter(chunks)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn drain_stream_text(chunks: Vec<StreamChunk>) -> (String, Option<TokenUsage>) {
        let mut text = String::new();
        let mut usage = None;
        for chunk in chunks {
            match chunk {
                StreamChunk::TextDelta(s) => text.push_str(&s),
                StreamChunk::Done { usage: u, .. } => usage = u,
                _ => {}
            }
        }
        (text, usage)
    }

    async fn collect_stream(
        provider: &ScriptedEvalProvider,
        prompt: &str,
    ) -> (String, Option<TokenUsage>) {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            attachments: Vec::new(),
        }];
        let stream = provider
            .stream(messages, None, ToolMode::Auto, None, CancellationToken::new())
            .await
            .expect("stream");
        let chunks: Vec<StreamChunk> = stream
            .map(|r| r.expect("ok chunk"))
            .collect::<Vec<_>>()
            .await;
        drain_stream_text(chunks)
    }

    #[test]
    fn info_advertises_scripted_provider() {
        let provider = ScriptedEvalProvider::new();
        let info = provider.info();
        assert_eq!(info.id, SCRIPTED_PROVIDER_ID);
        assert_eq!(info.models.len(), 1);
        assert_eq!(info.models[0].id, SCRIPTED_MODEL_ID);
    }

    #[tokio::test]
    async fn complete_returns_default_when_no_pattern_matches() {
        let provider = ScriptedEvalProvider::new();
        let body = provider.complete("anything", None).await.expect("ok");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("default is json");
        assert_eq!(parsed["status"], "completed");
        assert!(parsed["citations"].as_array().expect("citations").len() >= 3);
    }

    #[tokio::test]
    async fn first_matching_pattern_wins() {
        let provider = ScriptedEvalProvider::new()
            .with_pattern("research", ScriptedResponse::Text("R".to_string()))
            .with_pattern("research and cite", ScriptedResponse::Text("RC".to_string()));

        let body = provider
            .complete("Research and cite recent AI safety papers", None)
            .await
            .expect("ok");
        assert_eq!(body, "R");
    }

    #[tokio::test]
    async fn complete_is_deterministic_across_calls() {
        let provider = ScriptedEvalProvider::new();
        let a = provider.complete("same prompt", None).await.expect("ok");
        let b = provider.complete("same prompt", None).await.expect("ok");
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn stream_emits_text_delta_and_done_with_usage() {
        let provider = ScriptedEvalProvider::new()
            .with_default(ScriptedResponse::Text("hello world".to_string()));
        let (text, usage) = collect_stream(&provider, "user prompt").await;

        assert_eq!(text, "hello world");
        let usage = usage.expect("done chunk should carry usage");
        assert!(usage.prompt_tokens > 0);
        assert!(usage.completion_tokens > 0);
        assert_eq!(
            usage.total_tokens,
            usage.prompt_tokens + usage.completion_tokens
        );
    }

    #[tokio::test]
    async fn token_counts_are_stable_for_identical_prompts() {
        let provider = ScriptedEvalProvider::new()
            .with_default(ScriptedResponse::Text("response body".to_string()));
        let (_, usage_a) = collect_stream(&provider, "prompt one").await;
        let (_, usage_b) = collect_stream(&provider, "prompt one").await;
        assert_eq!(usage_a.unwrap().total_tokens, usage_b.unwrap().total_tokens);
    }

    #[tokio::test]
    async fn call_log_captures_each_prompt() {
        let provider = ScriptedEvalProvider::new();
        let _ = provider.complete("first", None).await.expect("ok");
        let _ = provider.complete("second", None).await.expect("ok");

        let log = provider.call_log().await;
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], "first");
        assert_eq!(log[1], "second");
    }

    #[tokio::test]
    async fn json_response_serializes_to_string_body() {
        let provider = ScriptedEvalProvider::new().with_pattern(
            "needs json",
            ScriptedResponse::Json(json!({"answer": 42})),
        );
        let body = provider
            .complete("this prompt needs json", None)
            .await
            .expect("ok");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(parsed["answer"], 42);
    }

    #[test]
    fn estimate_tokens_handles_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 1); // div_ceil(3/4) = 1
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
