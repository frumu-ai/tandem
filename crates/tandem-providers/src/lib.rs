use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::{pin::Pin, str};

use async_stream::try_stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;

use tandem_types::{ModelInfo, ProviderInfo, ToolMode, ToolSchema};

fn provider_max_tokens_for(provider_id: &str) -> u32 {
    if provider_id.eq_ignore_ascii_case("openai-codex") {
        return std::env::var("TANDEM_PROVIDER_MAX_TOKENS_OPENAI_CODEX")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .filter(|value| *value >= 64)
            .unwrap_or(128_000);
    }
    let normalized = provider_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let provider_specific_env =
        (!normalized.is_empty()).then(|| format!("TANDEM_PROVIDER_MAX_TOKENS_{normalized}"));
    provider_specific_env
        .as_deref()
        .and_then(|name| std::env::var(name).ok())
        .or_else(|| std::env::var("TANDEM_PROVIDER_MAX_TOKENS").ok())
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|value| *value >= 64)
        .unwrap_or(16384)
}

fn parse_openrouter_affordable_max_tokens(detail: &str) -> Option<u32> {
    let marker = "can only afford";
    let start = detail.to_ascii_lowercase().find(marker)?;
    let suffix = detail.get(start + marker.len()..)?.trim_start();
    let digits = suffix
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u32>().ok().filter(|value| *value >= 64)
}

fn format_openai_error_response(status: reqwest::StatusCode, text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| extract_openai_error(&value))
        .unwrap_or_else(|| {
            format!(
                "provider request failed with status {}: {}",
                status,
                truncate_for_error(text, 500)
            )
        })
}

fn openrouter_affordability_retry_max_tokens(
    provider_id: &str,
    status: reqwest::StatusCode,
    detail: &str,
    current_max_tokens: u32,
) -> Option<u32> {
    if provider_id != "openrouter" || status != reqwest::StatusCode::PAYMENT_REQUIRED {
        return None;
    }
    parse_openrouter_affordable_max_tokens(detail)
        .filter(|affordable| *affordable < current_max_tokens)
}

fn protocol_title_header() -> String {
    std::env::var("AGENT_PROTOCOL_TITLE")
        .ok()
        .or_else(|| std::env::var("TANDEM_PROTOCOL_TITLE").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Tandem".to_string())
}

fn sanitize_openai_function_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "tool".to_string()
    } else {
        cleaned.to_string()
    }
}

fn build_openai_tool_aliases(
    tools: &[ToolSchema],
) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut original_to_alias = HashMap::new();
    let mut alias_to_original = HashMap::new();

    for tool in tools {
        let original = tool.name.trim();
        if original.is_empty() {
            continue;
        }
        let base = sanitize_openai_function_name(original);
        let mut alias = base.clone();
        let mut suffix = 2usize;
        while alias_to_original.contains_key(&alias) {
            alias = format!("{base}_{suffix}");
            suffix = suffix.saturating_add(1);
        }
        original_to_alias.insert(original.to_string(), alias.clone());
        alias_to_original.insert(alias, original.to_string());
    }

    (original_to_alias, alias_to_original)
}

fn normalize_openai_function_parameters(schema: serde_json::Value) -> serde_json::Value {
    let mut schema = match schema {
        serde_json::Value::Object(obj) => serde_json::Value::Object(obj),
        _ => json!({}),
    };

    normalize_openai_schema_node(&mut schema);

    let Some(obj) = schema.as_object_mut() else {
        return json!({"type":"object","properties":{}});
    };
    if obj.get("type").and_then(|v| v.as_str()) != Some("object") {
        obj.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
    }
    if !obj.contains_key("properties") || !obj["properties"].is_object() {
        obj.insert("properties".to_string(), json!({}));
    }

    schema
}

fn normalize_openai_schema_node(node: &mut serde_json::Value) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };

    if obj.get("type").and_then(|v| v.as_str()) == Some("object") || obj.contains_key("properties")
    {
        if !obj.contains_key("properties") || !obj["properties"].is_object() {
            obj.insert("properties".to_string(), json!({}));
        }
    }

    if obj.get("type").and_then(|v| v.as_str()) == Some("array") && !obj.contains_key("items") {
        obj.insert("items".to_string(), json!({}));
    }

    if let Some(items) = obj.get_mut("items") {
        normalize_openai_items_schema(items);
    }

    if let Some(additional) = obj.get_mut("additionalProperties") {
        normalize_openai_schema_or_bool(additional);
    }

    if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
        for property_schema in properties.values_mut() {
            normalize_openai_schema_or_bool(property_schema);
        }
    }

    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(variants) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            for variant in variants.iter_mut() {
                normalize_openai_schema_or_bool(variant);
            }
        }
    }
}

fn normalize_openai_schema_or_bool(node: &mut serde_json::Value) {
    match node {
        serde_json::Value::Object(_) => normalize_openai_schema_node(node),
        serde_json::Value::Bool(_) => {}
        _ => *node = json!({}),
    }
}

fn normalize_openai_items_schema(items: &mut serde_json::Value) {
    if let Some(tuple_items) = items.as_array_mut() {
        let replacement = tuple_items
            .iter()
            .find(|candidate| candidate.is_object() || candidate.is_boolean())
            .cloned()
            .unwrap_or_else(|| json!({}));
        *items = replacement;
    }
    normalize_openai_schema_or_bool(items);
}

fn openai_tool_choice(tool_mode: &ToolMode) -> &'static str {
    match tool_mode {
        ToolMode::Required => "required",
        ToolMode::Auto | ToolMode::None => "auto",
    }
}

fn openrouter_tool_choice_retry_supported(
    provider_id: &str,
    tool_mode: &ToolMode,
    detail: &str,
) -> bool {
    if provider_id != "openrouter" || !matches!(tool_mode, ToolMode::Required) {
        return false;
    }

    let normalized = detail.to_ascii_lowercase();
    normalized.contains("tool_choice")
        && (normalized.contains("no endpoints found that support")
            || normalized.contains("does not support the provided"))
}

#[derive(Debug, Clone)]
struct OpenAiToolCallChunk {
    id: String,
    name: String,
    args_delta: String,
    index: u64,
}

fn canonical_openai_tool_name(
    raw_name: &str,
    alias_to_original: &HashMap<String, String>,
) -> String {
    alias_to_original
        .get(raw_name)
        .cloned()
        .unwrap_or_else(|| raw_name.to_string())
}

fn push_openai_text_fragments(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            if !text.is_empty() {
                out.push(text.to_string());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                push_openai_text_fragments(item, out);
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
            if let Some(text) = obj
                .get("text")
                .and_then(|v| v.as_object())
                .and_then(|nested| nested.get("value"))
                .and_then(|v| v.as_str())
            {
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
            if let Some(text) = obj.get("content").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
            if let Some(text) = obj.get("input_text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
        }
        _ => {}
    }
}

fn extract_openai_tool_call_chunk(
    call: &serde_json::Value,
    alias_to_original: &HashMap<String, String>,
    fallback_id: String,
) -> Option<OpenAiToolCallChunk> {
    let obj = call.as_object()?;
    let function = obj.get("function").cloned().unwrap_or_default();
    let index = obj
        .get("index")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let raw_name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("tool_name").and_then(|v| v.as_str()))
        .or_else(|| function.get("name").and_then(|v| v.as_str()))
        .or_else(|| {
            obj.get("call")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    let args_delta = obj
        .get("arguments")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            obj.get("args")
                .and_then(|v| (!v.is_null()).then(|| v.to_string()))
        })
        .or_else(|| {
            obj.get("input")
                .and_then(|v| (!v.is_null()).then(|| v.to_string()))
        })
        .or_else(|| {
            function
                .get("arguments")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .or_else(|| {
            function
                .get("arguments")
                .and_then(|v| (!v.is_null()).then(|| v.to_string()))
        })
        .unwrap_or_default();
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("tool_call_id").and_then(|v| v.as_str()))
        .map(ToString::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_id);
    let canonical_name = if raw_name.is_empty() {
        String::new()
    } else {
        canonical_openai_tool_name(&raw_name, alias_to_original)
    };
    if canonical_name.is_empty() && args_delta.is_empty() {
        return None;
    }
    Some(OpenAiToolCallChunk {
        id,
        name: canonical_name,
        args_delta,
        index,
    })
}

fn extract_openai_tool_call_chunks(
    choice: &serde_json::Value,
    alias_to_original: &HashMap<String, String>,
) -> Vec<OpenAiToolCallChunk> {
    let delta = choice.get("delta").cloned().unwrap_or_default();
    let message = choice.get("message").cloned().unwrap_or_default();
    let mut calls = Vec::new();
    let direct_lists = [
        delta.get("tool_calls").and_then(|v| v.as_array()),
        message.get("tool_calls").and_then(|v| v.as_array()),
        choice.get("tool_calls").and_then(|v| v.as_array()),
    ];
    for list in direct_lists.into_iter().flatten() {
        for (idx, call) in list.iter().enumerate() {
            let index = call
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(idx as u64);
            if let Some(chunk) = extract_openai_tool_call_chunk(
                call,
                alias_to_original,
                format!("tool_call_{index}"),
            ) {
                calls.push(chunk);
            }
        }
    }
    for content in [
        delta.get("content"),
        message.get("content"),
        choice.get("content"),
    ] {
        let Some(items) = content.and_then(|v| v.as_array()) else {
            continue;
        };
        for (idx, item) in items.iter().enumerate() {
            let index = item
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(idx as u64);
            let item_type = item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(
                item_type.as_str(),
                "tool_call" | "function_call" | "tool_use" | "output_tool_call"
            ) {
                if let Some(chunk) = extract_openai_tool_call_chunk(
                    item,
                    alias_to_original,
                    format!("content_tool_call_{index}"),
                ) {
                    calls.push(chunk);
                }
            }
        }
    }
    calls
}

fn is_openai_tool_call_fallback_id(id: &str) -> bool {
    id.starts_with("tool_call_") || id.starts_with("content_tool_call_")
}

fn resolve_openai_tool_call_stream_id(
    call: &OpenAiToolCallChunk,
    real_ids_by_index: &mut HashMap<u64, String>,
) -> String {
    if !is_openai_tool_call_fallback_id(&call.id) {
        real_ids_by_index.insert(call.index, call.id.clone());
        return call.id.clone();
    }

    real_ids_by_index
        .get(&call.index)
        .cloned()
        .unwrap_or_else(|| call.id.clone())
}

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

fn default_true() -> bool {
    true
}

/// Configuration for background memory consolidation via a cheap/free LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConsolidationConfig {
    /// Set to `true` to enable automatic channel memory archival when a session run ends.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Override the provider to use for consolidation.
    /// Defaults to cheapest available: ollama → groq → openrouter → mistral → openai → default.
    #[serde(default)]
    pub provider: Option<String>,
    /// Override the model to use for consolidation.
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for MemoryConsolidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone)]
pub enum ChatAttachment {
    ImageUrl { url: String },
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
        _tool_mode: ToolMode,
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

    /// Complete a prompt using the cheapest available configured provider.
    ///
    /// Tries providers in this cost order (first configured one wins):
    /// `ollama` (free/local) → `groq` (free tier) → `openrouter` (free models) →
    /// `mistral` ($0.10/1M) → `openai` ($0.15/1M) → `anthropic` → default provider.
    ///
    /// Optionally accepts an explicit `provider_override` and `model_override` from
    /// `MemoryConsolidationConfig` to let users pin a specific provider/model.
    pub async fn complete_cheapest(
        &self,
        prompt: &str,
        provider_override: Option<&str>,
        model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        // If the user has explicitly pinned a provider, use it directly.
        if let Some(pid) = provider_override {
            return self
                .complete_for_provider(Some(pid), prompt, model_override)
                .await;
        }

        let best_provider = self.select_cheapest_provider_id().await;
        let openrouter_free_model = "meta-llama/llama-3.3-70b-instruct:free";

        match best_provider {
            Some(pid @ "openrouter") if model_override.is_none() => {
                self.complete_for_provider(Some(pid), prompt, Some(openrouter_free_model))
                    .await
            }
            Some(pid) => {
                self.complete_for_provider(Some(pid), prompt, model_override)
                    .await
            }
            None => {
                // No known cheap provider configured — fall back to default.
                self.complete_for_provider(None, prompt, model_override)
                    .await
            }
        }
    }

    /// Returns the string ID of the cheapest available configured provider.
    pub async fn select_cheapest_provider_id(&self) -> Option<&'static str> {
        let providers = self.providers.read().await;
        let configured_ids: Vec<String> = providers.iter().map(|p| p.info().id).collect();
        drop(providers);

        // Cost-ordered priority: local/free first, paid last.
        let priority_order = [
            "ollama",
            "groq",
            "openrouter",
            "together",
            "mistral",
            "openai",
            "anthropic",
            "cohere",
        ];

        priority_order
            .iter()
            .find(|id| configured_ids.iter().any(|c| c == **id))
            .copied()
    }

    pub async fn default_stream(
        &self,
        messages: Vec<ChatMessage>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        self.stream_for_provider(None, None, messages, tool_mode, tools, cancel)
            .await
    }

    pub async fn stream_for_provider(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        messages: Vec<ChatMessage>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let provider = self.select_provider(provider_id).await?;
        provider
            .stream(messages, model_id, tool_mode, tools, cancel)
            .await
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

    pub async fn replace_for_test(
        &self,
        providers: Vec<Arc<dyn Provider>>,
        default_provider: Option<String>,
    ) {
        *self.providers.write().await = providers;
        *self.default_provider.write().await = default_provider;
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
        "gpt-5.2",
        true,
    );
    add_openai_responses_provider(
        config,
        &mut providers,
        "openai-codex",
        "OpenAI Codex",
        "https://chatgpt.com/backend-api/codex",
        "gpt-5.4",
        true,
        272_000,
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
        "llama_cpp",
        "llama.cpp",
        "http://127.0.0.1:8080/v1",
        "llm",
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
                .unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
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

    for (id, entry) in &config.providers {
        if is_known_provider_id(id) {
            continue;
        }

        let provider_id = id.trim();
        if provider_id.is_empty() {
            continue;
        }

        providers.push(Arc::new(OpenAICompatibleProvider {
            id: provider_id.to_string(),
            name: humanize_provider_name(provider_id),
            base_url: normalize_base(entry.url.as_deref().unwrap_or("https://api.openai.com/v1")),
            api_key: entry
                .api_key
                .as_deref()
                .filter(|key| !is_placeholder_api_key(key))
                .map(|key| key.to_string())
                .or_else(|| env_api_key_for_provider(provider_id)),
            default_model: entry
                .default_model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string()),
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
    let Some(entry) = provider_config_entry(config, id) else {
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

fn add_openai_responses_provider(
    config: &AppConfig,
    providers: &mut Vec<Arc<dyn Provider>>,
    id: &str,
    name: &str,
    default_url: &str,
    default_model: &str,
    use_api_key: bool,
    context_window: usize,
) {
    let Some(entry) = provider_config_entry(config, id) else {
        return;
    };
    providers.push(Arc::new(OpenAIResponsesProvider {
        id: id.to_string(),
        name: name.to_string(),
        base_url: default_url.to_string(),
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
        context_window,
        client: Client::new(),
    }));
}

fn provider_config_entry<'a>(config: &'a AppConfig, id: &str) -> Option<&'a ProviderConfig> {
    config
        .providers
        .get(id)
        .or_else(|| provider_id_aliases(id).find_map(|alias| config.providers.get(alias)))
}

fn provider_id_aliases(id: &str) -> impl Iterator<Item = &'static str> {
    match id.trim().to_ascii_lowercase().as_str() {
        "llama_cpp" => vec!["llama.cpp"].into_iter(),
        "llama.cpp" => vec!["llama_cpp"].into_iter(),
        _ => Vec::new().into_iter(),
    }
}

fn is_placeholder_api_key(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("x")
        || trimmed.eq_ignore_ascii_case("placeholder")
}

fn is_known_provider_id(id: &str) -> bool {
    matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "ollama"
            | "openai"
            | "openai-codex"
            | "openrouter"
            | "llama_cpp"
            | "llama.cpp"
            | "groq"
            | "mistral"
            | "together"
            | "azure"
            | "bedrock"
            | "vertex"
            | "copilot"
            | "anthropic"
            | "cohere"
    )
}

fn humanize_provider_name(id: &str) -> String {
    if matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "llama_cpp" | "llama.cpp"
    ) {
        return "llama.cpp".to_string();
    }
    let mut words = Vec::new();
    for segment in id.split(['_', '-']) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            let mut word = first.to_uppercase().collect::<String>();
            word.push_str(chars.as_str());
            words.push(word);
        }
    }
    if words.is_empty() {
        "Custom Provider".to_string()
    } else {
        words.join(" ")
    }
}

fn env_api_key_for_provider(id: &str) -> Option<String> {
    let explicit = match id {
        "openai" => Some("OPENAI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "together" => Some("TOGETHER_API_KEY"),
        "copilot" => Some("GITHUB_TOKEN"),
        _ => None,
    };
    if let Some(name) = explicit {
        if let Some(value) = std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
            return Some(value);
        }
    }

    let normalized = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        return None;
    }
    let env_name = format!("{}_API_KEY", normalized);
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
        let mut response_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        let mut max_tokens = provider_max_tokens_for(&self.id);
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&json!({
                "model": model,
                "messages": [{"role":"user","content": prompt}],
                "stream": false,
                "max_tokens": max_tokens,
            }));
            if self.id == "openrouter" {
                req = req
                    .header("HTTP-Referer", "https://tandem.ac")
                    .header("X-Title", protocol_title_header());
            }
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        if let Some(affordable_max) = openrouter_affordability_retry_max_tokens(
                            &self.id, status, &text, max_tokens,
                        ) {
                            max_tokens = affordable_max;
                            if attempt < 2 {
                                continue;
                            }
                        }
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let response = if let Some(resp) = response_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };
        let value: serde_json::Value = response.json().await?;

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
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/chat/completions", self.base_url);
        let has_image_inputs = messages.iter().any(|m| !m.attachments.is_empty());
        if has_image_inputs && !model_supports_vision_input(model) {
            anyhow::bail!(
                "selected model `{}` does not appear to support image input. choose a vision-capable model.",
                model
            );
        }

        let wire_messages = normalize_openai_messages(messages)
            .into_iter()
            .map(chat_message_to_openai_wire)
            .collect::<Vec<_>>();

        let tools = tools.unwrap_or_default();
        let (original_to_alias, alias_to_original) = build_openai_tool_aliases(&tools);
        let wire_tools = tools
            .into_iter()
            .map(|tool| {
                let safe_name = original_to_alias
                    .get(tool.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| sanitize_openai_function_name(&tool.name));
                json!({
                    "type": "function",
                    "function": {
                        "name": safe_name,
                        "description": tool.description,
                        "parameters": normalize_openai_function_parameters(tool.input_schema),
                    }
                })
            })
            .collect::<Vec<_>>();
        let has_tools = !wire_tools.is_empty();

        let mut max_tokens = provider_max_tokens_for(&self.id);
        let mut body = json!({
            "model": model,
            "messages": wire_messages,
            "stream": true,
            "max_tokens": max_tokens,
        });
        if has_tools {
            body["tools"] = serde_json::Value::Array(wire_tools);
            body["tool_choice"] = json!(openai_tool_choice(&tool_mode));
        }

        let mut resp_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        let mut downgraded_openrouter_tool_choice = false;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if self.id == "openrouter" {
                req = req
                    .header("HTTP-Referer", "https://tandem.ac")
                    .header("X-Title", protocol_title_header());
            }
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        if has_tools
                            && !downgraded_openrouter_tool_choice
                            && openrouter_tool_choice_retry_supported(&self.id, &tool_mode, &text)
                        {
                            body["tool_choice"] = json!("auto");
                            downgraded_openrouter_tool_choice = true;
                            if attempt < 2 {
                                continue;
                            }
                        }
                        if let Some(affordable_max) = openrouter_affordability_retry_max_tokens(
                            &self.id, status, &text, max_tokens,
                        ) {
                            max_tokens = affordable_max;
                            body["max_tokens"] = json!(max_tokens);
                            if attempt < 2 {
                                continue;
                            }
                        }
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    resp_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let resp = if let Some(resp) = resp_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };
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
        let alias_to_original = alias_to_original.clone();
        let stream = try_stream! {
            let mut buffer = String::new();
            let mut tool_call_real_ids = HashMap::new();
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
                            let message = choice.get("message").cloned().unwrap_or_default();

                            let mut emitted_text = false;
                            let mut text_fragments = Vec::new();
                            push_openai_text_fragments(&delta.get("content").cloned().unwrap_or_default(), &mut text_fragments);
                            if text_fragments.is_empty() {
                                push_openai_text_fragments(&message.get("content").cloned().unwrap_or_default(), &mut text_fragments);
                            }
                            for text in text_fragments {
                                if !text.is_empty() {
                                    emitted_text = true;
                                    yield StreamChunk::TextDelta(text);
                                }
                            }

                            if !emitted_text {
                                if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        yield StreamChunk::TextDelta(text.to_string());
                                    }
                                }
                            }

                            for call in extract_openai_tool_call_chunks(&choice, &alias_to_original) {
                                let effective_id =
                                    resolve_openai_tool_call_stream_id(&call, &mut tool_call_real_ids);
                                if !effective_id.is_empty() && !call.name.is_empty() {
                                    yield StreamChunk::ToolCallStart {
                                        id: effective_id.clone(),
                                        name: call.name.clone(),
                                    };
                                }
                                if !effective_id.is_empty() && !call.args_delta.is_empty() {
                                    yield StreamChunk::ToolCallDelta {
                                        id: effective_id.clone(),
                                        args_delta: call.args_delta.clone(),
                                    };
                                }
                                if !effective_id.is_empty() {
                                    yield StreamChunk::ToolCallEnd { id: effective_id };
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

struct OpenAIResponsesProvider {
    id: String,
    name: String,
    base_url: String,
    api_key: Option<String>,
    default_model: String,
    context_window: usize,
    client: Client,
}

#[async_trait]
impl Provider for OpenAIResponsesProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: self.id.clone(),
                display_name: self.default_model.clone(),
                context_window: self.context_window,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/responses", self.base_url);
        let mut body = json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "stream": false,
            "max_output_tokens": provider_max_tokens_for(&self.id),
        });
        if should_default_openai_reasoning(model) {
            body["reasoning"] = json!({
                "effort": "high",
                "summary": "auto"
            });
            body["include"] = json!(["reasoning.encrypted_content"]);
        }

        let mut response_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let response = if let Some(resp) = response_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };

        let value: serde_json::Value = response.json().await?;
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
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/responses", self.base_url);
        let has_image_inputs = messages.iter().any(|m| !m.attachments.is_empty());
        if has_image_inputs && !model_supports_vision_input(model) {
            anyhow::bail!(
                "selected model `{}` does not appear to support image input. choose a vision-capable model.",
                model
            );
        }

        let wire_messages = normalize_openai_messages(messages)
            .into_iter()
            .map(chat_message_to_openai_responses_wire)
            .collect::<Vec<_>>();

        let tools = tools.unwrap_or_default();
        let (original_to_alias, alias_to_original) = build_openai_tool_aliases(&tools);
        let wire_tools = tools
            .into_iter()
            .map(|tool| {
                let safe_name = original_to_alias
                    .get(tool.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| sanitize_openai_function_name(&tool.name));
                json!({
                    "type": "function",
                    "name": safe_name,
                    "description": tool.description,
                    "parameters": normalize_openai_function_parameters(tool.input_schema),
                })
            })
            .collect::<Vec<_>>();
        let has_tools = !wire_tools.is_empty();

        let max_output_tokens = provider_max_tokens_for(&self.id);
        let mut body = json!({
            "model": model,
            "input": wire_messages,
            "stream": true,
            "max_output_tokens": max_output_tokens,
        });
        if has_tools {
            body["tools"] = serde_json::Value::Array(wire_tools);
            body["tool_choice"] = json!(openai_tool_choice(&tool_mode));
        }
        if should_default_openai_reasoning(model) {
            body["reasoning"] = json!({
                "effort": "high",
                "summary": "auto"
            });
            body["include"] = json!(["reasoning.encrypted_content"]);
        }

        let mut resp_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    resp_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let resp = if let Some(resp) = resp_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };

        let mut bytes = resp.bytes_stream();
        let alias_to_original = alias_to_original.clone();
        let stream = try_stream! {
            let mut buffer = String::new();
            let mut tool_call_names: HashMap<String, String> = HashMap::new();
            let mut started_tool_calls: HashSet<String> = HashSet::new();
            let mut ended_tool_calls: HashSet<String> = HashSet::new();
            let mut emitted_message_text: HashSet<String> = HashSet::new();
            let mut emitted_reasoning_text: HashSet<String> = HashSet::new();
            let mut saw_completion = false;
            let mut saw_tool_calls = false;

            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    if !saw_completion {
                        for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                            if ended_tool_calls.insert(call_id.clone()) {
                                yield StreamChunk::ToolCallEnd { id: call_id };
                            }
                        }
                        saw_completion = true;
                        yield StreamChunk::Done {
                            finish_reason: "cancelled".to_string(),
                            usage: None,
                        };
                    }
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
                            if !saw_completion {
                                for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                                    if ended_tool_calls.insert(call_id.clone()) {
                                        yield StreamChunk::ToolCallEnd { id: call_id };
                                    }
                                }
                                yield StreamChunk::Done {
                                    finish_reason: if saw_tool_calls {
                                        "toolUse".to_string()
                                    } else {
                                        "stop".to_string()
                                    },
                                    usage: None,
                                };
                                saw_completion = true;
                            }
                            continue;
                        }

                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };

                        if let Some(detail) = extract_openai_error(&value) {
                            Err(anyhow::anyhow!(detail))?;
                        }

                        let event_type = value
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        match event_type {
                            "response.output_item.added" => {
                                if let Some(item) = value.get("item").and_then(|v| v.as_object()) {
                                    let item_type = item
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    if item_type == "function_call" {
                                        saw_tool_calls = true;
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let call_key = responses_tool_call_key(
                                            call_id,
                                            item_id,
                                            value.get("output_index").and_then(|v| v.as_u64()),
                                        );
                                        if !call_key.is_empty() {
                                            let name = responses_tool_call_name(item, &alias_to_original);
                                            if !name.is_empty() {
                                                tool_call_names.insert(call_key.clone(), name.clone());
                                            }
                                            if started_tool_calls.insert(call_key.clone()) {
                                                yield StreamChunk::ToolCallStart {
                                                    id: call_key.clone(),
                                                    name: if name.is_empty() {
                                                        "tool".to_string()
                                                    } else {
                                                        name
                                                    },
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                            "response.reasoning_summary_text.delta" => {
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                if !item_id.is_empty() {
                                    emitted_reasoning_text.insert(item_id.clone());
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::ReasoningDelta(delta);
                                }
                            }
                            "response.output_text.delta" | "response.refusal.delta" => {
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                if !item_id.is_empty() {
                                    emitted_message_text.insert(item_id.clone());
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::TextDelta(delta);
                                }
                            }
                            "response.function_call_arguments.delta" => {
                                saw_tool_calls = true;
                                let call_id = value
                                    .get("call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let call_key = responses_tool_call_key(
                                    call_id,
                                    item_id,
                                    value.get("output_index").and_then(|v| v.as_u64()),
                                );
                                if call_key.is_empty() {
                                    continue;
                                }
                                if started_tool_calls.insert(call_key.clone()) {
                                    let name = tool_call_names
                                        .get(&call_key)
                                        .cloned()
                                        .unwrap_or_else(|| "tool".to_string());
                                    yield StreamChunk::ToolCallStart {
                                        id: call_key.clone(),
                                        name,
                                    };
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::ToolCallDelta {
                                        id: call_key,
                                        args_delta: delta,
                                    };
                                }
                            }
                            "response.function_call_arguments.done" => {
                                saw_tool_calls = true;
                                let call_id = value
                                    .get("call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let call_key = responses_tool_call_key(
                                    call_id,
                                    item_id,
                                    value.get("output_index").and_then(|v| v.as_u64()),
                                );
                                if !call_key.is_empty() && ended_tool_calls.insert(call_key.clone()) {
                                    yield StreamChunk::ToolCallEnd { id: call_key };
                                }
                            }
                            "response.output_item.done" => {
                                if let Some(item) = value.get("item").and_then(|v| v.as_object()) {
                                    let item_type = item
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    if item_type == "message" {
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        if !item_id.is_empty() && !emitted_message_text.contains(&item_id) {
                                            if let Some(text) = extract_openai_text(&serde_json::Value::Object(item.clone())) {
                                                emitted_message_text.insert(item_id);
                                                yield StreamChunk::TextDelta(text);
                                            }
                                        }
                                    } else if item_type == "reasoning" {
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        if !item_id.is_empty() && !emitted_reasoning_text.contains(&item_id) {
                                            if let Some(summary) = extract_responses_reasoning_summary(item) {
                                                emitted_reasoning_text.insert(item_id);
                                                yield StreamChunk::ReasoningDelta(summary);
                                            }
                                        }
                                    } else if item_type == "function_call" {
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let call_key = responses_tool_call_key(
                                            call_id,
                                            item_id,
                                            value.get("output_index").and_then(|v| v.as_u64()),
                                        );
                                        if !call_key.is_empty() && ended_tool_calls.insert(call_key.clone()) {
                                            yield StreamChunk::ToolCallEnd { id: call_key };
                                        }
                                    }
                                }
                            }
                            "response.completed" => {
                                let response = value
                                    .get("response")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();
                                let usage = extract_responses_usage(&response);
                                let mut finish_reason = map_responses_stop_reason(
                                    response.get("status").and_then(|v| v.as_str()),
                                    response
                                        .get("incomplete_details")
                                        .and_then(|v| v.as_object())
                                        .and_then(|obj| obj.get("reason"))
                                        .and_then(|v| v.as_str()),
                                );
                                if finish_reason == "stop" && saw_tool_calls {
                                    finish_reason = "toolUse".to_string();
                                }
                                if !saw_completion {
                                    for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                                        if ended_tool_calls.insert(call_id.clone()) {
                                            yield StreamChunk::ToolCallEnd { id: call_id };
                                        }
                                    }
                                    yield StreamChunk::Done {
                                        finish_reason,
                                        usage,
                                    };
                                    saw_completion = true;
                                }
                            }
                            "error" => {
                                let code = value
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let message = value
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown error");
                                Err(anyhow::anyhow!("Error Code {}: {}", code, message))?;
                            }
                            "response.failed" => {
                                let response = value
                                    .get("response")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();
                                let detail = response
                                    .get("error")
                                    .and_then(|v| v.as_object())
                                    .map(|err| {
                                        let code = err
                                            .get("code")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        let message = err
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("no message");
                                        format!("{code}: {message}")
                                    })
                                    .or_else(|| {
                                        response
                                            .get("incomplete_details")
                                            .and_then(|v| v.as_object())
                                            .and_then(|obj| obj.get("reason"))
                                            .and_then(|v| v.as_str())
                                            .map(|reason| format!("incomplete: {reason}"))
                                    })
                                    .unwrap_or_else(|| "Unknown error (no error details in response)".to_string());
                                Err(anyhow::anyhow!(detail))?;
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !saw_completion {
                for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                    if ended_tool_calls.insert(call_id.clone()) {
                        yield StreamChunk::ToolCallEnd { id: call_id };
                    }
                }
                yield StreamChunk::Done {
                    finish_reason: if saw_tool_calls {
                        "toolUse".to_string()
                    } else {
                        "stop".to_string()
                    },
                    usage: None,
                };
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
        _tool_mode: ToolMode,
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

fn chat_message_to_openai_wire(message: ChatMessage) -> serde_json::Value {
    if message.attachments.is_empty() {
        return json!({
            "role": message.role,
            "content": message.content
        });
    }

    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(json!({
            "type": "text",
            "text": message.content
        }));
    }

    for attachment in message.attachments {
        match attachment {
            ChatAttachment::ImageUrl { url } => content.push(json!({
                "type": "image_url",
                "image_url": { "url": url }
            })),
        }
    }

    if content.is_empty() {
        content.push(json!({"type": "text", "text": ""}));
    }

    json!({
        "role": message.role,
        "content": content
    })
}

fn chat_message_to_openai_responses_wire(message: ChatMessage) -> serde_json::Value {
    let role = message.role.trim().to_ascii_lowercase();
    if role == "system" {
        return json!({
            "role": "developer",
            "content": message.content
        });
    }
    if role == "assistant" {
        return json!({
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": message.content,
                "annotations": []
            }]
        });
    }

    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(json!({
            "type": "input_text",
            "text": message.content
        }));
    }
    for attachment in message.attachments {
        match attachment {
            ChatAttachment::ImageUrl { url } => content.push(json!({
                "type": "input_image",
                "detail": "auto",
                "image_url": url
            })),
        }
    }
    if content.is_empty() {
        content.push(json!({
            "type": "input_text",
            "text": ""
        }));
    }

    json!({
        "role": "user",
        "content": content
    })
}

fn normalize_openai_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut merged_system: Option<ChatMessage> = None;
    let mut out = Vec::with_capacity(messages.len());

    for message in messages {
        if message.role.eq_ignore_ascii_case("system") {
            let entry = merged_system.get_or_insert_with(|| ChatMessage {
                role: "system".to_string(),
                content: String::new(),
                attachments: Vec::new(),
            });
            let next_content = message.content.trim();
            if !next_content.is_empty() {
                if !entry.content.is_empty() {
                    entry.content.push_str("\n\n");
                }
                entry.content.push_str(next_content);
            }
            entry.attachments.extend(message.attachments);
            continue;
        }
        out.push(message);
    }

    if let Some(system) = merged_system {
        out.insert(0, system);
    }

    out
}

fn responses_tool_call_key(call_id: &str, item_id: &str, output_index: Option<u64>) -> String {
    let call_id = call_id.trim();
    let item_id = item_id.trim();
    if !call_id.is_empty() && !item_id.is_empty() {
        return format!("{call_id}|{item_id}");
    }
    if !call_id.is_empty() {
        return call_id.to_string();
    }
    if !item_id.is_empty() {
        return item_id.to_string();
    }
    format!("fc_{}", output_index.unwrap_or_default())
}

fn responses_tool_call_name(
    item: &serde_json::Map<String, serde_json::Value>,
    alias_to_original: &HashMap<String, String>,
) -> String {
    let raw_name = item
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("tool_name").and_then(|v| v.as_str()))
        .or_else(|| {
            item.get("function")
                .and_then(|v| v.as_object())
                .and_then(|function| function.get("name"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_name.is_empty() {
        return String::new();
    }
    canonical_openai_tool_name(&raw_name, alias_to_original)
}

fn extract_responses_reasoning_summary(
    item: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let summary = item.get("summary")?;
    let mut out = String::new();
    match summary {
        serde_json::Value::String(text) => out.push_str(text),
        serde_json::Value::Array(parts) => {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(text);
                }
            }
        }
        _ => {}
    }
    (!out.trim().is_empty()).then_some(out)
}

fn extract_responses_usage(
    response: &serde_json::Map<String, serde_json::Value>,
) -> Option<TokenUsage> {
    let usage = response.get("usage")?.as_object()?;
    let cached_tokens = usage
        .get("input_tokens_details")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let prompt_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .saturating_sub(cached_tokens);
    let completion_tokens = usage
        .get("output_tokens")
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

fn map_responses_stop_reason(status: Option<&str>, incomplete_reason: Option<&str>) -> String {
    match status.map(|value| value.trim().to_ascii_lowercase()) {
        Some(ref value) if value == "completed" => "stop".to_string(),
        Some(ref value) if value == "incomplete" => "length".to_string(),
        Some(ref value) if value == "failed" || value == "cancelled" => "error".to_string(),
        Some(ref value) if value == "in_progress" || value == "queued" => "stop".to_string(),
        Some(_) | None => {
            if incomplete_reason
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                "length".to_string()
            } else {
                "stop".to_string()
            }
        }
    }
}

fn model_supports_vision_input(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    [
        "vision", "gpt-4o", "gpt-4.1", "gpt-5", "omni", "gemini", "claude-3", "llava", "qwen-vl",
        "pixtral",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn should_default_openai_reasoning(model: &str) -> bool {
    let lower = model.trim().to_ascii_lowercase();
    lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}

fn normalize_base(input: &str) -> String {
    // Accept base URLs with common OpenAI-compatible suffixes and normalize to `.../v1`.
    // This prevents accidental double suffixes like `/v1/v1`.
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }

    // Self-heal legacy malformed values that accidentally ended up with repeated `/v1`.
    while let Some(prefix) = base.strip_suffix("/v1") {
        if prefix.ends_with("/v1") {
            base = prefix.to_string();
            continue;
        }
        break;
    }

    if base.ends_with("/v1") {
        base
    } else {
        format!("{}/v1", base.trim_end_matches('/'))
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
    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

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

    #[tokio::test]
    async fn custom_provider_id_is_supported_from_config() {
        let registry = ProviderRegistry::new(cfg(&["custom"], Some("custom"), false));
        let provider = registry
            .select_provider(Some("custom"))
            .await
            .expect("provider");
        assert_eq!(provider.info().id, "custom");
    }

    #[test]
    fn normalize_base_handles_common_openai_compatible_inputs() {
        assert_eq!(
            normalize_base("http://localhost:8080"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/chat/completions"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/models"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/v1"),
            "http://localhost:8080/v1"
        );
    }

    #[test]
    fn normalize_openai_messages_merges_system_messages_to_front() {
        let normalized = normalize_openai_messages(vec![
            ChatMessage {
                role: "system".to_string(),
                content: "base instructions".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "system".to_string(),
                content: "memory scope".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
            },
        ]);

        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized[0].role, "system");
        assert_eq!(normalized[0].content, "base instructions\n\nmemory scope");
        assert_eq!(normalized[1].role, "user");
        assert_eq!(normalized[2].role, "assistant");
    }

    #[test]
    fn normalize_openai_messages_leaves_non_system_order_unchanged() {
        let normalized = normalize_openai_messages(vec![
            ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].role, "user");
        assert_eq!(normalized[1].role, "assistant");
    }

    #[tokio::test]
    async fn complete_cheapest_picks_ollama_first() {
        // Test priority parsing logic
        let registry = ProviderRegistry::new(cfg(&["openai", "groq", "ollama"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, Some("ollama"));

        let registry = ProviderRegistry::new(cfg(&["openai", "openai", "openrouter"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, Some("openrouter"));

        let registry = ProviderRegistry::new(cfg(&["unknown_provider"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, None);
    }

    #[test]
    fn sanitize_openai_function_name_rewrites_invalid_chars() {
        assert_eq!(
            sanitize_openai_function_name("mcp.arcade.gmail_sendemail"),
            "mcp_arcade_gmail_sendemail"
        );
        assert_eq!(sanitize_openai_function_name("  "), "tool");
        assert_eq!(
            sanitize_openai_function_name("clickup-getSpaces"),
            "clickup-getSpaces"
        );
    }

    #[test]
    fn build_openai_tool_aliases_preserves_roundtrip_and_uniqueness() {
        let tools = vec![
            ToolSchema::new("mcp.arcade.gmail.send", "a", json!({"type":"object"})),
            ToolSchema::new("mcp_arcade_gmail_send", "b", json!({"type":"object"})),
        ];
        let (forward, reverse) = build_openai_tool_aliases(&tools);
        let alias_a = forward
            .get("mcp.arcade.gmail.send")
            .expect("alias for dotted name");
        let alias_b = forward
            .get("mcp_arcade_gmail_send")
            .expect("alias for underscore name");
        assert_ne!(alias_a, alias_b, "aliases must be unique");
        assert_eq!(
            reverse.get(alias_a).map(String::as_str),
            Some("mcp.arcade.gmail.send")
        );
        assert_eq!(
            reverse.get(alias_b).map(String::as_str),
            Some("mcp_arcade_gmail_send")
        );
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    async fn read_single_http_request(
        socket: &mut tokio::net::TcpStream,
    ) -> (String, String, String) {
        let mut buffer = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = socket.read(&mut chunk).await.expect("read request");
            assert!(
                read > 0,
                "connection closed before request headers were read"
            );
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(pos) = find_subsequence(&buffer, b"\r\n\r\n") {
                break pos + 4;
            }
        };

        let headers = String::from_utf8(buffer[..header_end].to_vec()).expect("utf8 headers");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .or_else(|| line.strip_prefix("content-length: "))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let mut chunk = [0u8; 1024];
            let read = socket.read(&mut chunk).await.expect("read request body");
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }

        let request_line = headers.lines().next().unwrap_or("").to_string();
        let body = String::from_utf8(body).expect("utf8 body");
        (request_line, headers, body)
    }

    #[tokio::test]
    async fn openai_codex_stream_uses_responses_transport() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, rx) = oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let request = read_single_http_request(&mut socket).await;
            let response_body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"Hello\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":7,\"total_tokens\":12}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.as_bytes().len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
            tx.send(request).expect("send request");
        });

        let provider = OpenAIResponsesProvider {
            id: "openai-codex".to_string(),
            name: "OpenAI Codex".to_string(),
            base_url: format!("http://{}/codex", addr),
            api_key: Some("codex-test-token".to_string()),
            default_model: "gpt-5.4".to_string(),
            context_window: 272_000,
            client: Client::new(),
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
            attachments: Vec::new(),
        }];
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(messages, None, ToolMode::None, None, cancel)
            .await
            .expect("stream");

        let mut chunks = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            let is_done = matches!(chunk, StreamChunk::Done { .. });
            chunks.push(chunk);
            if is_done {
                break;
            }
        }

        let (request_line, headers, body) = rx.await.expect("request");
        server.await.expect("server task");

        assert_eq!(request_line, "POST /codex/responses HTTP/1.1");
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer codex-test-token"));
        assert!(body.contains("\"input\""));
        assert!(body.contains("\"max_output_tokens\""));
        assert!(body.contains("\"gpt-5.4\""));

        let text_deltas = chunks
            .iter()
            .filter_map(|chunk| match chunk {
                StreamChunk::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["Hello"]);

        let done_chunks = chunks
            .iter()
            .filter(|chunk| matches!(chunk, StreamChunk::Done { .. }))
            .count();
        assert_eq!(done_chunks, 1);

        let done = chunks
            .iter()
            .find_map(|chunk| match chunk {
                StreamChunk::Done {
                    finish_reason,
                    usage,
                } => Some((
                    finish_reason.as_str(),
                    usage.as_ref().map(|usage| usage.total_tokens),
                )),
                _ => None,
            })
            .expect("done chunk");
        assert_eq!(done.0, "stop");
        assert_eq!(done.1, Some(12));
    }

    #[test]
    fn extract_openai_tool_call_chunks_supports_content_array_tool_calls() {
        let mut alias_to_original = HashMap::new();
        alias_to_original.insert("write_alias".to_string(), "write".to_string());
        let choice = json!({
            "message": {
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call-1",
                        "function": {
                            "name": "write_alias",
                            "arguments": "{\"path\":\"README.md\",\"content\":\"hi\"}"
                        }
                    }
                ]
            }
        });
        let calls = extract_openai_tool_call_chunks(&choice, &alias_to_original);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "write");
        assert!(calls[0].args_delta.contains("\"README.md\""));
    }

    #[test]
    fn resolve_openai_tool_call_stream_id_keeps_multichunk_write_args_on_same_id() {
        let mut alias_to_original = HashMap::new();
        alias_to_original.insert("write_alias".to_string(), "write".to_string());

        let first_choice = json!({
            "delta": {
                "tool_calls": [
                    {
                        "index": 2,
                        "id": "call_ghi",
                        "function": {
                            "name": "write_alias",
                            "arguments": ""
                        }
                    }
                ]
            }
        });
        let continuation_choice = json!({
            "delta": {
                "tool_calls": [
                    {
                        "index": 2,
                        "function": {
                            "arguments": "{\"path\":\"game.html\",\"content\":\"hi\"}"
                        }
                    }
                ]
            }
        });

        let first_calls = extract_openai_tool_call_chunks(&first_choice, &alias_to_original);
        let continuation_calls =
            extract_openai_tool_call_chunks(&continuation_choice, &alias_to_original);

        assert_eq!(first_calls.len(), 1);
        assert_eq!(first_calls[0].id, "call_ghi");
        assert_eq!(first_calls[0].name, "write");
        assert_eq!(first_calls[0].index, 2);

        assert_eq!(continuation_calls.len(), 1);
        assert_eq!(continuation_calls[0].id, "tool_call_2");
        assert_eq!(continuation_calls[0].name, "");
        assert_eq!(continuation_calls[0].index, 2);

        let mut real_ids_by_index = HashMap::new();
        let mut args_by_id = HashMap::<String, String>::new();
        for call in first_calls.into_iter().chain(continuation_calls) {
            let effective_id = resolve_openai_tool_call_stream_id(&call, &mut real_ids_by_index);
            args_by_id
                .entry(effective_id)
                .or_default()
                .push_str(&call.args_delta);
        }

        assert_eq!(
            real_ids_by_index.get(&2).map(String::as_str),
            Some("call_ghi")
        );
        assert_eq!(
            args_by_id.get("call_ghi").map(String::as_str),
            Some("{\"path\":\"game.html\",\"content\":\"hi\"}")
        );
        assert!(!args_by_id.contains_key("tool_call_2"));
    }

    #[test]
    fn push_openai_text_fragments_reads_nested_text_parts() {
        let value = json!([
            {"type":"text","text":"first"},
            {"type":"output_text","text":{"value":"second"}},
            {"type":"text","content":"third"}
        ]);
        let mut fragments = Vec::new();
        push_openai_text_fragments(&value, &mut fragments);
        assert_eq!(fragments, vec!["first", "second", "third"]);
    }

    #[test]
    fn normalize_openai_function_parameters_adds_missing_properties() {
        let normalized = normalize_openai_function_parameters(json!({"type":"object"}));
        assert_eq!(
            normalized
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            "object"
        );
        assert!(
            normalized
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some(),
            "properties object should exist"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_recovers_non_object_schema() {
        let normalized = normalize_openai_function_parameters(json!("bad"));
        assert_eq!(
            normalized
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            "object"
        );
        assert!(
            normalized
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some(),
            "properties object should exist"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_rewrites_tuple_array_items() {
        let normalized = normalize_openai_function_parameters(json!({
            "type": "object",
            "properties": {
                "fieldIds": {
                    "type": "array",
                    "items": [
                        { "$ref": "#/properties/fieldIds/items" }
                    ]
                }
            }
        }));
        assert!(
            normalized["properties"]["fieldIds"]["items"].is_object(),
            "array items should be object/bool for OpenAI-compatible tools"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_adds_nested_object_properties() {
        let normalized = normalize_openai_function_parameters(json!({
            "type": "object",
            "properties": {
                "filters": {
                    "type": "object"
                }
            }
        }));
        assert!(
            normalized["properties"]["filters"]["properties"].is_object(),
            "nested object schemas should include properties for OpenAI validation"
        );
    }

    #[test]
    fn openrouter_affordability_retry_uses_affordable_cap() {
        let detail = r#"{"error":{"message":"This request requires more credits, or fewer max_tokens. You requested up to 16384 tokens, but can only afford 14605."}}"#;
        assert_eq!(
            openrouter_affordability_retry_max_tokens(
                "openrouter",
                reqwest::StatusCode::PAYMENT_REQUIRED,
                detail,
                16_384,
            ),
            Some(14_605)
        );
    }

    #[test]
    fn openrouter_tool_choice_retry_detects_unsupported_required_mode() {
        assert!(openrouter_tool_choice_retry_supported(
            "openrouter",
            &ToolMode::Required,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
        assert!(!openrouter_tool_choice_retry_supported(
            "openrouter",
            &ToolMode::Auto,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
        assert!(!openrouter_tool_choice_retry_supported(
            "openai",
            &ToolMode::Required,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
    }

    #[test]
    fn provider_specific_max_tokens_override_is_respected() {
        std::env::remove_var("TANDEM_PROVIDER_MAX_TOKENS");
        std::env::set_var("TANDEM_PROVIDER_MAX_TOKENS_OPENROUTER", "24576");
        assert_eq!(provider_max_tokens_for("openrouter"), 24_576);
        std::env::remove_var("TANDEM_PROVIDER_MAX_TOKENS_OPENROUTER");
        assert_eq!(provider_max_tokens_for("openrouter"), 16_384);
    }
}
