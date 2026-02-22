use std::collections::HashMap;
use std::sync::Arc;
use std::{pin::Pin, str};

use async_stream::try_stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use tandem_types::{ModelInfo, ProviderInfo, ToolSchema};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub url: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    pub default_provider: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args_delta: String,
    },
    ToolCallEnd {
        id: String,
    },
    Done {
        finish_reason: String,
        usage: Option<TokenUsage>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String>;
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        _tools: Option<Vec<ToolSchema>>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        let response = self.complete(&prompt, model_override).await?;
        let stream = futures::stream::iter(vec![
            Ok(StreamChunk::TextDelta(response)),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ]);
        Ok(Box::pin(stream))
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Arc<RwLock<Vec<Arc<dyn Provider>>>>,
    default_provider: Arc<RwLock<Option<String>>>,
}

impl ProviderRegistry {
    pub fn new(config: AppConfig) -> Self {
        let providers = build_providers(&config);
        Self {
            providers: Arc::new(RwLock::new(providers)),
            default_provider: Arc::new(RwLock::new(config.default_provider)),
        }
    }

    pub async fn reload(&self, config: AppConfig) {
        let rebuilt = build_providers(&config);
        *self.providers.write().await = rebuilt;
        *self.default_provider.write().await = config.default_provider;
    }

    pub async fn list(&self) -> Vec<ProviderInfo> {
        self.providers
            .read()
            .await
            .iter()
            .map(|p| p.info())
            .collect()
    }

    pub async fn default_complete(&self, prompt: &str) -> anyhow::Result<String> {
        let provider = self.select_provider(None).await?;
        provider.complete(prompt, None).await
    }

    pub async fn complete_for_provider(
        &self,
        provider_id: Option<&str>,
        prompt: &str,
        model_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let provider = self.select_provider(provider_id).await?;
        provider.complete(prompt, model_id).await
    }

    pub async fn default_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        self.stream_for_provider(None, None, messages, tools, cancel)
            .await
    }

    pub async fn stream_for_provider(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let provider = self.select_provider(provider_id).await?;
        provider.stream(messages, model_id, tools, cancel).await
    }

    async fn select_provider(
        &self,
        provider_id: Option<&str>,
    ) -> anyhow::Result<Arc<dyn Provider>> {
        let providers = self.providers.read().await;
        let available = providers.iter().map(|p| p.info().id).collect::<Vec<_>>();

        if let Some(id) = provider_id {
            if let Some(provider) = providers.iter().find(|p| p.info().id == id) {
                return Ok(provider.clone());
            }
            anyhow::bail!(
                "provider `{}` is not configured. configured providers: {}",
                id,
                available.join(", ")
            );
        };

        let configured_default = self.default_provider.read().await.clone();
        if let Some(default_id) = configured_default {
            if let Some(provider) = providers.iter().find(|p| p.info().id == default_id) {
                return Ok(provider.clone());
            }
        };

        let Some(provider) = providers.first() else {
            anyhow::bail!("No provider configured.");
        };
        Ok(provider.clone())
    }
}

fn build_providers(config: &AppConfig) -> Vec<Arc<dyn Provider>> {
    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

    add_openai_provider(
        config,
        &mut providers,
        "ollama",
        "Ollama",
        "http://127.0.0.1:11434/v1",
        "llama3.1:8b",
        false,
    );
    add_openai_provider(
        config,
        &mut providers,
        "openai",
        "OpenAI",
        "https://api.openai.com/v1",
        "gpt-4o-mini",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "openai/gpt-4o-mini",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "groq",
        "Groq",
        "https://api.groq.com/openai/v1",
        "llama-3.1-8b-instant",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "mistral",
        "Mistral",
        "https://api.mistral.ai/v1",
        "mistral-small-latest",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "together",
        "Together",
        "https://api.together.xyz/v1",
        "meta-llama/Llama-3.1-8B-Instruct-Turbo",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "azure",
        "Azure OpenAI-Compatible",
        "https://example.openai.azure.com/openai/deployments/default",
        "gpt-4o-mini",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "bedrock",
        "Bedrock-Compatible",
        "https://bedrock-runtime.us-east-1.amazonaws.com",
        "anthropic.claude-3-5-sonnet-20240620-v1:0",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "vertex",
        "Vertex-Compatible",
        "https://aiplatform.googleapis.com/v1",
        "gemini-1.5-flash",
        true,
    );
    add_openai_provider(
        config,
        &mut providers,
        "copilot",
        "GitHub Copilot-Compatible",
        "https://api.githubcopilot.com",
        "gpt-4o-mini",
        true,
    );

    if let Some(anthropic) = config.providers.get("anthropic") {
        providers.push(Arc::new(AnthropicProvider {
            api_key: anthropic
                .api_key
                .as_deref()
                .filter(|key| !is_placeholder_api_key(key))
                .map(|key| key.to_string())
                .or_else(|| {
                    std::env::var("ANTHROPIC_API_KEY")
                        .ok()
                        .filter(|v| !v.trim().is_empty())
                }),
            default_model: anthropic
                .default_model
                .clone()
                .unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
            client: Client::new(),
        }));
    }
    if let Some(cohere) = config.providers.get("cohere") {
        providers.push(Arc::new(CohereProvider {
            api_key: cohere
                .api_key
                .as_deref()
                .filter(|key| !is_placeholder_api_key(key))
                .map(|key| key.to_string())
                .or_else(|| {
                    std::env::var("COHERE_API_KEY")
                        .ok()
                        .filter(|v| !v.trim().is_empty())
                }),
            base_url: normalize_plain_base(
                cohere.url.as_deref().unwrap_or("https://api.cohere.com/v2"),
            ),
            default_model: cohere
                .default_model
                .clone()
                .unwrap_or_else(|| "command-r-plus".to_string()),
            client: Client::new(),
        }));
    }

    if providers.is_empty() {
        providers.push(Arc::new(LocalEchoProvider));
    }

    providers
}

fn add_openai_provider(
    config: &AppConfig,
    providers: &mut Vec<Arc<dyn Provider>>,
    id: &str,
    name: &str,
    default_url: &str,
    default_model: &str,
    use_api_key: bool,
) {
    let Some(entry) = config.providers.get(id) else {
        return;
    };
    providers.push(Arc::new(OpenAICompatibleProvider {
        id: id.to_string(),
        name: name.to_string(),
        base_url: normalize_base(entry.url.as_deref().unwrap_or(default_url)),
        api_key: if use_api_key {
            entry
                .api_key
                .as_deref()
                .filter(|key| !is_placeholder_api_key(key))
                .map(|key| key.to_string())
                .or_else(|| env_api_key_for_provider(id))
        } else {
            None
        },
        default_model: entry
            .default_model
            .clone()
            .unwrap_or_else(|| default_model.to_string()),
        client: Client::new(),
    }));
}

fn is_placeholder_api_key(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("x")
        || trimmed.eq_ignore_ascii_case("placeholder")
}

fn env_api_key_for_provider(id: &str) -> Option<String> {
    let env_name = match id {
        "openai" => Some("OPENAI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "together" => Some("TOGETHER_API_KEY"),
        "copilot" => Some("GITHUB_TOKEN"),
        _ => None,
    }?;
    std::env::var(env_name)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

fn provider_api_key_env_hint(id: &str) -> &'static str {
    match id {
        "openrouter" => "OPENROUTER_API_KEY",
        "opencode" => "OPENCODE_ZEN_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "cohere" => "COHERE_API_KEY",
        _ => "provider API key",
    }
}

struct LocalEchoProvider;

#[async_trait]
impl Provider for LocalEchoProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "local".to_string(),
            name: "Local Echo".to_string(),
            models: vec![ModelInfo {
                id: "echo-1".to_string(),
                provider_id: "local".to_string(),
                display_name: "Echo Model".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok(format!("Echo: {prompt}"))
    }
}

struct OpenAICompatibleProvider {
    id: String,
    name: String,
    base_url: String,
    api_key: Option<String>,
    default_model: String,
    client: Client,
}

#[async_trait]
impl Provider for OpenAICompatibleProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: self.id.clone(),
                display_name: self.default_model.clone(),
                context_window: 128_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.client.post(url).json(&json!({
            "model": model,
            "messages": [{"role":"user","content": prompt}],
            "stream": false,
        }));
        if self.id == "openrouter" {
            req = req
                .header("HTTP-Referer", "https://tandem.frumu.ai")
                .header("X-Title", "Tandem");
        }
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        let response = req.send().await?;
        let status = response.status();
        let value: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let detail = extract_openai_error(&value)
                .unwrap_or_else(|| format!("provider request failed with status {}", status));
            anyhow::bail!(detail);
        }

        if let Some(detail) = extract_openai_error(&value) {
            anyhow::bail!(detail);
        }

        if let Some(text) = extract_openai_text(&value) {
            return Ok(text);
        }

        let body_preview = truncate_for_error(&value.to_string(), 500);
        anyhow::bail!(
            "provider returned no completion content for model `{}` (response: {})",
            model,
            body_preview
        );
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/chat/completions", self.base_url);
        let wire_messages = messages
            .into_iter()
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect::<Vec<_>>();

        let wire_tools = tools
            .unwrap_or_default()
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": model,
            "messages": wire_messages,
            "stream": true,
        });
        if !wire_tools.is_empty() {
            body["tools"] = serde_json::Value::Array(wire_tools);
            body["tool_choice"] = json!("auto");
        }

        let mut req = self.client.post(url).json(&body);
        if self.id == "openrouter" {
            req = req
                .header("HTTP-Referer", "https://tandem.frumu.ai")
                .header("X-Title", "Tandem");
        }
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            if text.contains("Failed to authenticate request with Clerk") {
                let key_hint = provider_api_key_env_hint(&self.id);
                anyhow::bail!(
                    "provider authentication failed ({}) for `{}`. Verify the provider API key (set `{}` or configure the key in Settings) and retry.",
                    status,
                    self.id,
                    key_hint
                );
            }
            anyhow::bail!(
                "provider stream request failed with status {}: {}",
                status,
                truncate_for_error(&text, 500)
            );
        }

        let mut bytes = resp.bytes_stream();
        let stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    yield StreamChunk::Done {
                        finish_reason: "cancelled".to_string(),
                        usage: None,
                    };
                    break;
                }

                let chunk = chunk?;
                buffer.push_str(str::from_utf8(&chunk).unwrap_or_default());

                while let Some(pos) = buffer.find("\n\n") {
                    let frame = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();
                    for line in frame.lines() {
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data: ").trim();
                        if payload == "[DONE]" {
                            yield StreamChunk::Done {
                                finish_reason: "stop".to_string(),
                                usage: None,
                            };
                            continue;
                        }

                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };

                        if let Some(detail) = extract_openai_error(&value) {
                            Err(anyhow::anyhow!(detail))?;
                        }

                        let choices = value
                            .get("choices")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        for choice in choices {
                            let delta = choice.get("delta").cloned().unwrap_or_default();

                            if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    yield StreamChunk::TextDelta(text.to_string());
                                }
                            }

                            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                for call in tool_calls {
                                    let id = call
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    let function = call.get("function").cloned().unwrap_or_default();
                                    let name = function
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    let args_delta = function
                                        .get("arguments")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string();

                                    if !id.is_empty() && !name.is_empty() {
                                        yield StreamChunk::ToolCallStart {
                                            id: id.clone(),
                                            name,
                                        };
                                    }
                                    if !id.is_empty() && !args_delta.is_empty() {
                                        yield StreamChunk::ToolCallDelta {
                                            id: id.clone(),
                                            args_delta,
                                        };
                                    }
                                    if !id.is_empty() {
                                        yield StreamChunk::ToolCallEnd { id };
                                    }
                                }
                            }

                            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                if !reason.is_empty() {
                                    let usage = extract_usage(&value);
                                    yield StreamChunk::Done {
                                        finish_reason: reason.to_string(),
                                        usage,
                                    };
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

struct AnthropicProvider {
    api_key: Option<String>,
    default_model: String,
    client: Client,
}

struct CohereProvider {
    api_key: Option<String>,
    base_url: String,
    default_model: String,
    client: Client,
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: "anthropic".to_string(),
                display_name: self.default_model.clone(),
                context_window: 200_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": 1024,
                "messages": [{"role":"user","content": prompt}],
            }));
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let value: serde_json::Value = req.send().await?.json().await?;
        let text = value["content"][0]["text"]
            .as_str()
            .unwrap_or("No completion content.")
            .to_string();
        Ok(text)
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        _tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": 1024,
                "stream": true,
                "messages": messages
                    .into_iter()
                    .map(|m| json!({"role": m.role, "content": m.content}))
                    .collect::<Vec<_>>(),
            }));
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await?;
        let mut bytes = resp.bytes_stream();
        let stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    yield StreamChunk::Done {
                        finish_reason: "cancelled".to_string(),
                        usage: None,
                    };
                    break;
                }
                let chunk = chunk?;
                buffer.push_str(str::from_utf8(&chunk).unwrap_or_default());

                while let Some(pos) = buffer.find("\n\n") {
                    let frame = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();
                    for line in frame.lines() {
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data: ").trim();
                        if payload == "[DONE]" {
                            yield StreamChunk::Done {
                                finish_reason: "stop".to_string(),
                                usage: None,
                            };
                            continue;
                        }
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };
                        match value.get("type").and_then(|v| v.as_str()).unwrap_or_default() {
                            "content_block_delta" => {
                                if let Some(delta) = value.get("delta").and_then(|v| v.get("text")).and_then(|v| v.as_str()) {
                                    yield StreamChunk::TextDelta(delta.to_string());
                                }
                                if let Some(reasoning) = value.get("delta").and_then(|v| v.get("thinking")).and_then(|v| v.as_str()) {
                                    yield StreamChunk::ReasoningDelta(reasoning.to_string());
                                }
                            }
                            "message_stop" => {
                                yield StreamChunk::Done {
                                    finish_reason: "stop".to_string(),
                                    usage: None,
                                };
                            }
                            _ => {}
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl Provider for CohereProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "cohere".to_string(),
            name: "Cohere".to_string(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: "cohere".to_string(),
                display_name: self.default_model.clone(),
                context_window: 128_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post(format!("{}/chat", self.base_url))
            .json(&json!({
                "model": model,
                "messages": [{"role":"user","content": prompt}],
            }));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let value: serde_json::Value = req.send().await?.json().await?;
        let text = value["message"]["content"][0]["text"]
            .as_str()
            .or_else(|| value["text"].as_str())
            .unwrap_or("No completion content.")
            .to_string();
        Ok(text)
    }
}

fn normalize_base(input: &str) -> String {
    if input.ends_with("/v1") {
        input.trim_end_matches('/').to_string()
    } else {
        format!("{}/v1", input.trim_end_matches('/'))
    }
}

fn normalize_plain_base(input: &str) -> String {
    input.trim_end_matches('/').to_string()
}

fn truncate_for_error(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        input.to_string()
    } else {
        format!("{}...", &input[..max_len])
    }
}

fn extract_usage(value: &serde_json::Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt_tokens.saturating_add(completion_tokens));
    Some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

fn collect_text_fragments(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_text_fragments(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(text) = map.get("output_text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(content) = map.get("content") {
                collect_text_fragments(content, out);
            }
            if let Some(delta) = map.get("delta") {
                collect_text_fragments(delta, out);
            }
            if let Some(message) = map.get("message") {
                collect_text_fragments(message, out);
            }
        }
        _ => {}
    }
}

fn extract_openai_text(value: &serde_json::Value) -> Option<String> {
    let mut out = String::new();

    if let Some(choice) = value.get("choices").and_then(|v| v.get(0)) {
        collect_text_fragments(choice, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(text) = value
        .get("choices")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
    {
        return Some(text.to_string());
    }

    if let Some(output) = value.get("output") {
        collect_text_fragments(output, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(content) = value.get("content") {
        collect_text_fragments(content, &mut out);
        if !out.trim().is_empty() {
            return Some(out);
        }
    }

    if let Some(text) = value.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    None
}

fn extract_openai_error(value: &serde_json::Value) -> Option<String> {
    value
        .get("error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            value
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(
        provider_ids: &[&str],
        default_provider: Option<&str>,
        include_openai_key: bool,
    ) -> AppConfig {
        let mut providers = HashMap::new();
        for id in provider_ids {
            let api_key = if *id == "openai" && include_openai_key {
                Some("sk-test".to_string())
            } else {
                None
            };
            providers.insert(
                (*id).to_string(),
                ProviderConfig {
                    api_key,
                    url: None,
                    default_model: Some(format!("{id}-model")),
                },
            );
        }
        AppConfig {
            providers,
            default_provider: default_provider.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn explicit_provider_wins_over_default_provider() {
        let registry = ProviderRegistry::new(cfg(&["openai", "openrouter"], Some("openai"), true));
        let provider = registry
            .select_provider(Some("openrouter"))
            .await
            .expect("provider");
        assert_eq!(provider.info().id, "openrouter");
    }

    #[tokio::test]
    async fn uses_default_provider_when_explicit_provider_missing() {
        let registry =
            ProviderRegistry::new(cfg(&["openai", "openrouter"], Some("openrouter"), true));
        let provider = registry.select_provider(None).await.expect("provider");
        assert_eq!(provider.info().id, "openrouter");
    }

    #[tokio::test]
    async fn falls_back_to_first_provider_when_default_provider_missing() {
        let registry = ProviderRegistry::new(cfg(&["openai"], Some("anthropic"), true));
        let provider = registry.select_provider(None).await.expect("provider");
        assert_eq!(provider.info().id, "openai");
    }

    #[tokio::test]
    async fn explicit_unknown_provider_errors() {
        let registry = ProviderRegistry::new(cfg(&["openai"], None, true));
        let err = registry
            .select_provider(Some("openruter"))
            .await
            .err()
            .expect("expected error");
        assert!(err
            .to_string()
            .contains("provider `openruter` is not configured"));
    }
}
