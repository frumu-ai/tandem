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

fn default_openai_responses_instructions() -> String {
    format!(
        "You are {}. Follow the system and user instructions carefully.",
        protocol_title_header()
    )
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

fn normalize_codex_function_parameters(schema: serde_json::Value) -> serde_json::Value {
    let mut schema = normalize_openai_function_parameters(schema);
    let Some(obj) = schema.as_object_mut() else {
        return json!({"type":"object","properties":{}});
    };

    // Codex rejects root-level combinators and root enum/not constraints even when
    // the nested property schemas are otherwise valid JSON Schema.
    for key in ["anyOf", "oneOf", "allOf", "enum", "not"] {
        obj.remove(key);
    }

    schema
}

fn normalize_openai_schema_node(node: &mut serde_json::Value) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };

    if (obj.get("type").and_then(|v| v.as_str()) == Some("object")
        || obj.contains_key("properties"))
        && (!obj.contains_key("properties") || !obj["properties"].is_object())
    {
        obj.insert("properties".to_string(), json!({}));
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
        serde_json::Value::String(text) if !text.is_empty() => {
            out.push(text.to_string());
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

#[allow(clippy::too_many_arguments)]
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
    let configured_default_model = entry
        .default_model
        .clone()
        .unwrap_or_else(|| default_model.to_string());
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
        default_model: configured_default_model.clone(),
        models: if id == "openai-codex" {
            codex_supported_models(context_window)
        } else {
            vec![ModelInfo {
                id: configured_default_model.clone(),
                provider_id: id.to_string(),
                display_name: configured_default_model.clone(),
                context_window,
            }]
        },
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
