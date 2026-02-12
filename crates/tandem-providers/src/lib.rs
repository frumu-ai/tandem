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
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_delta: String },
    ToolCallEnd { id: String },
    Done { finish_reason: String },
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    async fn complete(&self, prompt: &str) -> anyhow::Result<String>;
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Option<Vec<ToolSchema>>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        let response = self.complete(&prompt).await?;
        let stream = futures::stream::iter(vec![
            Ok(StreamChunk::TextDelta(response)),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
            }),
        ]);
        Ok(Box::pin(stream))
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Arc<RwLock<Vec<Arc<dyn Provider>>>>,
}

impl ProviderRegistry {
    pub fn new(config: AppConfig) -> Self {
        let providers = build_providers(&config);
        Self {
            providers: Arc::new(RwLock::new(providers)),
        }
    }

    pub async fn reload(&self, config: AppConfig) {
        let rebuilt = build_providers(&config);
        *self.providers.write().await = rebuilt;
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
        let providers = self.providers.read().await;
        let Some(provider) = providers.first() else {
            return Ok("No provider configured.".to_string());
        };
        provider.complete(prompt).await
    }

    pub async fn default_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let providers = self.providers.read().await;
        let Some(provider) = providers.first() else {
            let stream = futures::stream::iter(vec![
                Ok(StreamChunk::TextDelta(
                    "No provider configured.".to_string(),
                )),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                }),
            ]);
            return Ok(Box::pin(stream));
        };
        provider.stream(messages, tools, cancel).await
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
            api_key: anthropic.api_key.clone(),
            default_model: anthropic
                .default_model
                .clone()
                .unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
            client: Client::new(),
        }));
    }
    if let Some(cohere) = config.providers.get("cohere") {
        providers.push(Arc::new(CohereProvider {
            api_key: cohere.api_key.clone(),
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
            entry.api_key.clone()
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

    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
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

    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self.client.post(url).json(&json!({
            "model": self.default_model,
            "messages": [{"role":"user","content": prompt}],
            "stream": false,
        }));
        if let Some(api_key) = &self.api_key {
            req = req.bearer_auth(api_key);
        }
        let value: serde_json::Value = req.send().await?.json().await?;
        let text = value["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No completion content.")
            .to_string();
        Ok(text)
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

    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": self.default_model,
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
        _tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": self.default_model,
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
                    yield StreamChunk::Done { finish_reason: "cancelled".to_string() };
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
                            yield StreamChunk::Done { finish_reason: "stop".to_string() };
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
                                yield StreamChunk::Done { finish_reason: "stop".to_string() };
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

    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let mut req = self
            .client
            .post(format!("{}/chat", self.base_url))
            .json(&json!({
                "model": self.default_model,
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
