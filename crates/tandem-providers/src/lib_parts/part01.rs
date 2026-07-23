use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
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

use tandem_types::{ModelInfo, ProviderInfo, SamplingParams, TenantContext, ToolMode, ToolSchema};

struct HardenedProviderTarget {
    url: reqwest::Url,
    client: Client,
}

const PROVIDER_RESPONSE_MAX_BYTES: usize = 16 * 1024 * 1024;

async fn read_provider_response_bytes_limited(
    response: reqwest::Response,
) -> anyhow::Result<Vec<u8>> {
    read_provider_response_bytes_with_limit(response, PROVIDER_RESPONSE_MAX_BYTES).await
}

async fn read_provider_response_bytes_with_limit(
    mut response: reqwest::Response,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        anyhow::bail!("provider response exceeds {limit} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("provider response exceeds {limit} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn read_provider_response_text_limited(
    response: reqwest::Response,
) -> anyhow::Result<String> {
    let body = read_provider_response_bytes_limited(response).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn read_provider_response_json_limited(
    response: reqwest::Response,
) -> anyhow::Result<serde_json::Value> {
    let body = read_provider_response_bytes_limited(response).await?;
    Ok(serde_json::from_slice(&body)?)
}

async fn resolve_provider_request_target(
    raw: &str,
    provider_id: &str,
) -> anyhow::Result<HardenedProviderTarget> {
    let url = reqwest::Url::parse(raw)?;
    let local_provider = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
        .try_with(|allowed| *allowed)
        .unwrap_or(false)
        && matches!(
        provider_id.trim().to_ascii_lowercase().as_str(),
        "ollama" | "llama_cpp" | "llama.cpp"
    );
    let insecure_http = url.scheme() == "http";
    if url.scheme() != "https" && !(insecure_http && local_provider) {
        anyhow::bail!("provider endpoint must use https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("provider endpoint must not include URL credentials");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("provider endpoint host is missing"))?
        .trim_matches(['[', ']'])
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let (dns_override_host, dns_override_addrs) = if let Ok(ip) = host.parse::<IpAddr>() {
        let public = provider_ip_is_publicly_routable(ip);
        if !public && !local_provider {
            anyhow::bail!("provider endpoint resolves to a private or internal address");
        }
        if insecure_http && public {
            anyhow::bail!("insecure provider HTTP is limited to standalone local providers");
        }
        (None, Vec::new())
    } else {
        let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if !local_provider && (normalized == "localhost" || normalized.ends_with(".localhost")) {
            anyhow::bail!("provider endpoint points to localhost/private network");
        }
        let addrs = tokio::net::lookup_host((host.as_str(), port))
            .await?
            .collect::<Vec<SocketAddr>>();
        if addrs.is_empty() {
            anyhow::bail!("provider endpoint host did not resolve");
        }
        let any_private = addrs
            .iter()
            .any(|address| !provider_ip_is_publicly_routable(address.ip()));
        let any_public = addrs
            .iter()
            .any(|address| provider_ip_is_publicly_routable(address.ip()));
        if any_private && !local_provider {
            anyhow::bail!("provider endpoint resolves to a private or internal address");
        }
        if insecure_http && any_public {
            anyhow::bail!("insecure provider HTTP is limited to standalone local providers");
        }
        (Some(host), addrs)
    };
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10));
    if let Some(host) = dns_override_host.as_deref() {
        builder = builder.resolve_to_addrs(host, &dns_override_addrs);
    }
    Ok(HardenedProviderTarget {
        url,
        client: builder.build()?,
    })
}

fn provider_ip_is_publicly_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => provider_ipv4_is_publicly_routable(ip),
        IpAddr::V6(ip) => provider_ipv6_is_publicly_routable(ip),
    }
}

fn provider_ipv4_is_publicly_routable(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240)
}

fn provider_ipv6_is_publicly_routable(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return provider_ipv4_is_publicly_routable(mapped);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ((segments[0] & 0xfe00) == 0xfc00)
        || ((segments[0] & 0xffc0) == 0xfe80)
        || ((segments[0] & 0xffc0) == 0xfec0)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0]))
}

tokio::task_local! {
    static PROVIDER_TENANT_CONTEXT: TenantContext;
}

tokio::task_local! {
    static PROVIDER_AUTH_RECOVERY: ProviderAuthRecovery;
}

tokio::task_local! {
    static PROVIDER_PRIVATE_ENDPOINTS_ALLOWED: bool;
}

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

/// Upper bound for `temperature` accepted by a provider family. Most
/// OpenAI-compatible providers accept up to `2.0`; Anthropic caps at `1.0`.
fn provider_temperature_max(provider_id: &str) -> f32 {
    if provider_id.eq_ignore_ascii_case("anthropic") {
        1.0
    } else {
        2.0
    }
}

/// Some models only support the default temperature and reject an explicit
/// `temperature` field (OpenAI reasoning models such as the o-series and
/// gpt-5 reasoning variants). For these we drop the parameter with a warning
/// rather than hard-failing the run.
fn model_rejects_temperature(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m == "o1"
        || m == "o3"
        || m.starts_with("o1-")
        || m.starts_with("o3-")
        || m.starts_with("o4-")
        || m.starts_with("gpt-5")
}

/// Resolve a generic `temperature` into a value safe for `provider_id`,
/// clamping to the provider's accepted range. Returns `None` when the value
/// must be dropped (the model rejects an explicit temperature), emitting a
/// warning in that case so the run continues with the model default.
fn resolve_temperature(provider_id: &str, model: &str, temperature: f32) -> Option<f32> {
    if model_rejects_temperature(model) {
        tracing::warn!(
            provider = provider_id,
            model,
            "model rejects explicit `temperature`; dropping sampling temperature for this request"
        );
        return None;
    }
    Some(temperature.clamp(0.0, provider_temperature_max(provider_id)))
}

/// Apply generic sampling parameters onto an OpenAI-compatible Chat Completions
/// request body (`temperature`, `top_p`, `max_tokens`). Unset fields leave the
/// body untouched so omitting sampling preserves the existing request exactly.
fn apply_openai_chat_sampling(
    body: &mut serde_json::Value,
    provider_id: &str,
    model: &str,
    sampling: SamplingParams,
) {
    if let Some(temperature) = sampling.temperature {
        if let Some(resolved) = resolve_temperature(provider_id, model, temperature) {
            body["temperature"] = json!(resolved);
        }
    }
    if let Some(top_p) = sampling.top_p {
        body["top_p"] = json!(top_p.clamp(0.0, 1.0));
    }
    if let Some(max_tokens) = sampling.max_tokens {
        body["max_tokens"] = json!(max_tokens.max(1));
    }
}

/// Apply generic sampling parameters onto an OpenAI Responses API request body.
/// The Responses API uses `max_output_tokens` instead of `max_tokens`.
fn apply_openai_responses_sampling(
    body: &mut serde_json::Value,
    provider_id: &str,
    model: &str,
    sampling: SamplingParams,
) {
    if let Some(temperature) = sampling.temperature {
        if let Some(resolved) = resolve_temperature(provider_id, model, temperature) {
            body["temperature"] = json!(resolved);
        }
    }
    if let Some(top_p) = sampling.top_p {
        body["top_p"] = json!(top_p.clamp(0.0, 1.0));
    }
    if let Some(max_tokens) = sampling.max_tokens {
        body["max_output_tokens"] = json!(max_tokens.max(1));
    }
}

/// Apply generic sampling parameters onto an Anthropic Messages request body.
/// Anthropic always requires `max_tokens`, so an unset value leaves the
/// existing default in place; a set value overrides it.
fn apply_anthropic_sampling(body: &mut serde_json::Value, model: &str, sampling: SamplingParams) {
    if let Some(temperature) = sampling.temperature {
        if let Some(resolved) = resolve_temperature("anthropic", model, temperature) {
            body["temperature"] = json!(resolved);
        }
    }
    if let Some(top_p) = sampling.top_p {
        body["top_p"] = json!(top_p.clamp(0.0, 1.0));
    }
    if let Some(max_tokens) = sampling.max_tokens {
        body["max_tokens"] = json!(max_tokens.max(1));
    }
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

fn openai_response_error(status: reqwest::StatusCode, text: &str) -> anyhow::Error {
    let detail = format_openai_error_response(status, text);
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        ProviderAuthenticationError::new(status.as_u16(), detail).into()
    } else {
        anyhow::anyhow!(detail)
    }
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
    async fn complete_with_auth_override(
        &self,
        prompt: &str,
        model_override: Option<&str>,
        auth_override: ProviderAuthOverride,
    ) -> anyhow::Result<String> {
        if !matches!(auth_override, ProviderAuthOverride::Inherit) {
            anyhow::bail!("provider does not support tenant-scoped authentication")
        }
        self.complete(prompt, model_override).await
    }
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: SamplingParams,
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

    #[allow(clippy::too_many_arguments)]
    async fn stream_with_auth_override(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        sampling: SamplingParams,
        cancel: CancellationToken,
        auth_override: ProviderAuthOverride,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        if !matches!(auth_override, ProviderAuthOverride::Inherit) {
            anyhow::bail!("provider does not support tenant-scoped authentication")
        }
        self.stream(messages, model_override, tool_mode, tools, sampling, cancel)
            .await
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ProviderAuthOverride {
    Inherit,
    Suppress,
    Bearer(String),
}

/// Typed provider-dispatch failure used to constrain OAuth recovery to the
/// request boundary. Callers must not infer retry safety from a later engine
/// error string because the engine may already have executed tools.
#[derive(Debug)]
pub struct ProviderAuthenticationError {
    status: u16,
    detail: String,
}

impl ProviderAuthenticationError {
    pub fn new(status: u16, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }
}

impl std::fmt::Display for ProviderAuthenticationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for ProviderAuthenticationError {}

type ProviderAuthRecoveryFuture =
    Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + 'static>>;

/// One-shot, task-scoped callback installed by the server around a tenant's
/// provider execution. The shared attempt flag enforces one refresh across all
/// provider dispatches in that execution scope.
#[derive(Clone)]
pub struct ProviderAuthRecovery {
    handler: Arc<dyn Fn(String) -> ProviderAuthRecoveryFuture + Send + Sync>,
    attempted: Arc<AtomicBool>,
}

impl ProviderAuthRecovery {
    pub fn new<Handler, HandlerFuture>(handler: Handler) -> Self
    where
        Handler: Fn(String) -> HandlerFuture + Send + Sync + 'static,
        HandlerFuture: std::future::Future<Output = anyhow::Result<bool>> + Send + 'static,
    {
        Self {
            handler: Arc::new(move |provider_id| Box::pin(handler(provider_id))),
            attempted: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn attempt(&self, provider_id: &str) -> anyhow::Result<bool> {
        if self.attempted.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        (self.handler)(provider_id.to_string()).await
    }
}

impl std::fmt::Debug for ProviderAuthRecovery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAuthRecovery")
            .field("attempted", &self.attempted.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ProviderAuthOverride {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inherit => formatter.write_str("Inherit"),
            Self::Suppress => formatter.write_str("Suppress"),
            Self::Bearer(_) => formatter.write_str("Bearer(<redacted>)"),
        }
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Arc<RwLock<Vec<Arc<dyn Provider>>>>,
    default_provider: Arc<RwLock<Option<String>>>,
    tenant_bearer_tokens: Arc<RwLock<HashMap<String, String>>>,
}

/// Provider/model route resolved before a guarded egress evaluation. Callers
/// evaluate this exact route and then dispatch with its explicit provider id,
/// preventing default-provider changes from invalidating classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProviderRoute {
    pub provider_id: String,
    pub model_id: Option<String>,
}

impl ProviderRegistry {
    pub fn new(config: AppConfig) -> Self {
        let providers = build_providers(&config);
        Self {
            providers: Arc::new(RwLock::new(providers)),
            default_provider: Arc::new(RwLock::new(config.default_provider)),
            tenant_bearer_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn scope_tenant_provider_auth<F>(
        &self,
        tenant_context: TenantContext,
        future: F,
    ) -> F::Output
    where
        F: std::future::Future,
    {
        PROVIDER_TENANT_CONTEXT.scope(tenant_context, future).await
    }

    pub async fn scope_tenant_provider_auth_with_recovery<F>(
        &self,
        tenant_context: TenantContext,
        recovery: ProviderAuthRecovery,
        allow_private_provider_endpoints: bool,
        future: F,
    ) -> F::Output
    where
        F: std::future::Future,
    {
        PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                allow_private_provider_endpoints,
                PROVIDER_TENANT_CONTEXT.scope(
                    tenant_context,
                    PROVIDER_AUTH_RECOVERY.scope(recovery, future),
                ),
            )
            .await
    }

    pub async fn set_tenant_provider_bearer_token(
        &self,
        tenant_context: &TenantContext,
        provider_id: &str,
        bearer_token: String,
    ) {
        let token = bearer_token.trim().to_string();
        if token.is_empty() {
            self.clear_tenant_provider_bearer_token(tenant_context, provider_id)
                .await;
            return;
        }
        self.tenant_bearer_tokens
            .write()
            .await
            .insert(tenant_provider_auth_key(tenant_context, provider_id), token);
    }

    pub async fn clear_tenant_provider_bearer_token(
        &self,
        tenant_context: &TenantContext,
        provider_id: &str,
    ) {
        self.tenant_bearer_tokens
            .write()
            .await
            .remove(&tenant_provider_auth_key(tenant_context, provider_id));
    }

    pub async fn tenant_provider_auth_is_loaded(
        &self,
        tenant_context: &TenantContext,
        provider_id: &str,
    ) -> bool {
        self.tenant_bearer_tokens
            .read()
            .await
            .contains_key(&tenant_provider_auth_key(tenant_context, provider_id))
    }

    async fn auth_override_for_provider(&self, provider_id: &str) -> ProviderAuthOverride {
        if !provider_id.eq_ignore_ascii_case("openai-codex") {
            return ProviderAuthOverride::Inherit;
        }
        let Some(tenant_context) = PROVIDER_TENANT_CONTEXT.try_with(Clone::clone).ok() else {
            return ProviderAuthOverride::Inherit;
        };
        if let Some(token) = self
            .tenant_bearer_tokens
            .read()
            .await
            .get(&tenant_provider_auth_key(&tenant_context, provider_id))
            .cloned()
        {
            ProviderAuthOverride::Bearer(token)
        } else if tenant_context.is_local_implicit() {
            ProviderAuthOverride::Inherit
        } else {
            // An explicit tenant must never inherit a local/global Codex token.
            ProviderAuthOverride::Suppress
        }
    }

    async fn recover_typed_auth_failure(&self, provider_id: &str, error: &anyhow::Error) -> bool {
        if !provider_id.eq_ignore_ascii_case("openai-codex")
            || error
                .downcast_ref::<ProviderAuthenticationError>()
                .is_none()
        {
            return false;
        }
        let Ok(recovery) = PROVIDER_AUTH_RECOVERY.try_with(Clone::clone) else {
            return false;
        };
        match recovery.attempt(provider_id).await {
            Ok(recovered) => recovered,
            Err(_) => {
                tracing::warn!(
                    provider_id,
                    "provider authentication recovery callback failed"
                );
                false
            }
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
        self.complete_for_provider(None, prompt, None).await
    }

    pub async fn complete_for_provider(
        &self,
        provider_id: Option<&str>,
        prompt: &str,
        model_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let provider = self.select_provider(provider_id).await?;
        let resolved_provider_id = provider.info().id;
        let auth_override = self
            .auth_override_for_provider(resolved_provider_id.as_str())
            .await;
        match provider
            .complete_with_auth_override(prompt, model_id, auth_override)
            .await
        {
            Ok(output) => Ok(output),
            Err(error)
                if self
                    .recover_typed_auth_failure(resolved_provider_id.as_str(), &error)
                    .await =>
            {
                let auth_override = self
                    .auth_override_for_provider(resolved_provider_id.as_str())
                    .await;
                provider
                    .complete_with_auth_override(prompt, model_id, auth_override)
                    .await
            }
            Err(error) => Err(error),
        }
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
        let route = self
            .resolve_cheapest_completion_route(provider_override, model_override)
            .await?;
        self.complete_for_provider(
            Some(route.provider_id.as_str()),
            prompt,
            route.model_id.as_deref(),
        )
        .await
    }

    /// Resolve the concrete provider selected by an optional provider id.
    pub async fn resolve_provider_route(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> anyhow::Result<ResolvedProviderRoute> {
        let provider = self.select_provider(provider_id).await?;
        Ok(ResolvedProviderRoute {
            provider_id: provider.info().id,
            model_id: model_id.map(str::to_string),
        })
    }

    /// Resolve the exact route used by `complete_cheapest`, including the
    /// OpenRouter free-model default.
    pub async fn resolve_cheapest_completion_route(
        &self,
        provider_override: Option<&str>,
        model_override: Option<&str>,
    ) -> anyhow::Result<ResolvedProviderRoute> {
        if provider_override.is_some() {
            return self
                .resolve_provider_route(provider_override, model_override)
                .await;
        }
        let best_provider = self.select_cheapest_provider_id().await;
        let model_id = if best_provider == Some("openrouter") && model_override.is_none() {
            Some("meta-llama/llama-3.3-70b-instruct:free")
        } else {
            model_override
        };
        self.resolve_provider_route(best_provider, model_id).await
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
        self.stream_for_provider(
            None,
            None,
            messages,
            tool_mode,
            tools,
            SamplingParams::default(),
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn stream_for_provider(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        messages: Vec<ChatMessage>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        sampling: SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let provider = self.select_provider(provider_id).await?;
        let resolved_provider_id = provider.info().id;
        let auth_override = self
            .auth_override_for_provider(resolved_provider_id.as_str())
            .await;
        let retry_messages = messages.clone();
        let retry_tools = tools.clone();
        let retry_cancel = cancel.clone();
        match provider
            .stream_with_auth_override(
                messages,
                model_id,
                tool_mode.clone(),
                tools,
                sampling,
                cancel,
                auth_override,
            )
            .await
        {
            Ok(stream) => Ok(stream),
            Err(error)
                if self
                    .recover_typed_auth_failure(resolved_provider_id.as_str(), &error)
                    .await =>
            {
                let auth_override = self
                    .auth_override_for_provider(resolved_provider_id.as_str())
                    .await;
                provider
                    .stream_with_auth_override(
                        retry_messages,
                        model_id,
                        tool_mode,
                        retry_tools,
                        sampling,
                        retry_cancel,
                        auth_override,
                    )
                    .await
            }
            Err(error) => Err(error),
        }
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

fn tenant_provider_auth_key(tenant_context: &TenantContext, provider_id: &str) -> String {
    serde_json::to_string(&(
        tenant_context.org_id.as_str(),
        tenant_context.workspace_id.as_str(),
        tenant_context.deployment_id.as_deref(),
        provider_id.trim().to_ascii_lowercase(),
    ))
    .expect("tenant provider auth key is serializable")
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
        OPENAI_CODEX_DEFAULT_MODEL,
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
    // Heal a persisted openai-codex default that has been retired from the
    // catalog so the registry built at startup/reload never serves an
    // unsupported model as the provider default.
    let configured_default_model = if id == "openai-codex" {
        openai_codex_effective_default_model(Some(&configured_default_model))
    } else {
        configured_default_model
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
}
