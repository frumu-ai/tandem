use std::{collections::HashSet, sync::OnceLock};

use futures::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_providers::{ChatMessage, StreamChunk};
use tandem_types::{MessagePart, MessageRole, ModelSpec, ToolMode};
use tokio_util::sync::CancellationToken;

use super::{sessions::truncate_text, AppState};

const STRICT_KB_FALLBACK: &str = "I do not see that in the connected knowledgebase.";
const STRICT_KB_FETCH_FALLBACK: &str = "I found a likely matching document, but could not retrieve enough content to answer safely from the knowledgebase.";
const MAX_SOURCE_LABELS: usize = 3;
const MAX_EVIDENCE_EXCERPTS: usize = 6;
const MAX_EVIDENCE_CHARS: usize = 700;
const MAX_FULL_DOCUMENT_CHARS: usize = 6_000;
const MAX_FULL_DOCUMENT_FETCHES: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub(super) struct StrictKbGroundingOutcome {
    pub support: String,
    pub sources: Vec<String>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone)]
struct KbEvidenceItem {
    excerpt: String,
    sources: Vec<String>,
    full_document: bool,
}

#[derive(Debug, Clone, Default)]
struct KbEvidenceBundle {
    items: Vec<KbEvidenceItem>,
    document_refs_found: bool,
    full_documents_fetched: usize,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct KbDocumentRef {
    server_name: String,
    doc_id: String,
    collection_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct StrictKbSynthesisResponse {
    #[serde(default)]
    kb_answer_support: String,
    #[serde(default)]
    supported_facts: Vec<String>,
    #[serde(default)]
    missing_facts: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    answer_text: String,
}

pub(super) async fn apply_strict_kb_grounding_after_run(
    state: &AppState,
    session_id: &str,
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
    model_override: Option<ModelSpec>,
) -> anyhow::Result<Option<StrictKbGroundingOutcome>> {
    if !policy.required || !policy.strict {
        return Ok(None);
    }
    let Some((mut session, user_idx, assistant_idx)) =
        wait_for_kb_turn_materialization(state, session_id, policy).await
    else {
        return Ok(None);
    };
    let user_text = message_text(&session.messages[user_idx]);
    if user_text.trim().is_empty() {
        return Ok(None);
    }
    let evidence_bundle = collect_kb_evidence(state, &session.messages[user_idx], policy).await;
    let evidence = evidence_bundle.items;
    let (support, mut answer_text, mut sources) = if evidence.is_empty() {
        (
            "unsupported".to_string(),
            STRICT_KB_FALLBACK.to_string(),
            Vec::new(),
        )
    } else if evidence_bundle.document_refs_found
        && evidence_bundle.full_documents_fetched == 0
        && !snippet_evidence_can_safely_answer(&user_text, &evidence)
    {
        (
            "unsupported".to_string(),
            STRICT_KB_FETCH_FALLBACK.to_string(),
            evidence
                .iter()
                .flat_map(|item| item.sources.iter().cloned())
                .collect::<Vec<_>>(),
        )
    } else if let Some((support, answer)) = deterministic_strict_kb_answer(&user_text, &evidence) {
        (
            support,
            answer,
            evidence
                .iter()
                .flat_map(|item| item.sources.iter().cloned())
                .collect::<Vec<_>>(),
        )
    } else if let Some(response) =
        synthesize_strict_kb_answer(state, &user_text, &evidence, model_override.as_ref()).await
    {
        let support = normalize_support_label(&response.kb_answer_support).to_string();
        let sources = merged_sources(
            response.sources.clone(),
            evidence
                .iter()
                .flat_map(|item| item.sources.iter().cloned())
                .collect(),
        );
        let mut answer_text = render_strict_kb_answer(&support, &response, &sources);
        if let Some(unsupported_tokens) = unsupported_strict_kb_fact_tokens(&answer_text, &evidence)
        {
            tracing::warn!(
                unsupported_tokens = ?unsupported_tokens,
                "strict KB answer contained unsupported numeric/date facts; using extractive fallback"
            );
            answer_text = extractive_strict_kb_answer(&user_text, &evidence)
                .unwrap_or_else(|| STRICT_KB_FALLBACK.to_string());
        }
        if strict_kb_answer_has_unsupported_advice(&answer_text, &evidence) {
            tracing::warn!(
                "strict KB answer contained unsupported procedural/policy advice; using extractive fallback"
            );
            answer_text = extractive_strict_kb_answer(&user_text, &evidence)
                .unwrap_or_else(|| STRICT_KB_FALLBACK.to_string());
        }
        (support, answer_text, sources)
    } else {
        (
            "unsupported".to_string(),
            STRICT_KB_FALLBACK.to_string(),
            evidence
                .iter()
                .flat_map(|item| item.sources.iter().cloned())
                .collect::<Vec<_>>(),
        )
    };
    sources = merged_sources(sources, Vec::new());
    answer_text = append_source_footer(answer_text, &sources);
    let assistant_id = session.messages[assistant_idx].id.clone();
    session.messages[assistant_idx].parts = vec![MessagePart::Text {
        text: answer_text.clone(),
    }];
    session.time.updated = chrono::Utc::now();
    state.storage.save_session(session).await?;
    let final_part = tandem_wire::WireMessagePart::text(
        session_id,
        &assistant_id,
        truncate_text(&answer_text, 16_000),
    );
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "message.part.updated",
        json!({ "part": final_part }),
    ));
    Ok(Some(StrictKbGroundingOutcome {
        support,
        sources,
        evidence_count: evidence.len(),
    }))
}

fn latest_exchange_indexes(session: &tandem_types::Session) -> Option<(usize, usize)> {
    let assistant_idx = session
        .messages
        .iter()
        .rposition(|message| matches!(message.role, MessageRole::Assistant))?;
    let user_idx = session.messages[..assistant_idx]
        .iter()
        .rposition(|message| matches!(message.role, MessageRole::User))?;
    Some((user_idx, assistant_idx))
}

fn message_text(message: &tandem_types::Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text { text } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn collect_kb_evidence(
    state: &AppState,
    message: &tandem_types::Message,
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
) -> KbEvidenceBundle {
    let mut bundle = KbEvidenceBundle::default();
    let document_refs = collect_kb_document_refs(message, policy);
    bundle.document_refs_found = !document_refs.is_empty();
    for document_ref in document_refs.iter().take(MAX_FULL_DOCUMENT_FETCHES) {
        match fetch_kb_full_document(state, document_ref).await {
            Some(items) if !items.is_empty() => {
                bundle.full_documents_fetched += 1;
                for item in items {
                    bundle.items.push(item);
                    if bundle.items.len() >= MAX_EVIDENCE_EXCERPTS {
                        return bundle;
                    }
                }
            }
            _ => {
                tracing::warn!(
                    server = %document_ref.server_name,
                    doc_id = %document_ref.doc_id,
                    collection_id = ?document_ref.collection_id,
                    "strict KB grounding could not fetch full source document"
                );
            }
        }
    }
    for part in &message.parts {
        let MessagePart::ToolInvocation {
            tool,
            result: Some(result),
            ..
        } = part
        else {
            continue;
        };
        if !tool_matches_kb_policy(tool, policy) {
            continue;
        }
        let output = tool_result_text(result);
        if output.trim().is_empty() || looks_like_non_evidence_output(&output) {
            continue;
        }
        let excerpts = extract_kb_excerpts(&output, MAX_EVIDENCE_CHARS);
        if excerpts.is_empty() {
            continue;
        }
        let sources = extract_kb_source_labels(&output);
        for excerpt in excerpts {
            bundle.items.push(KbEvidenceItem {
                excerpt,
                sources: sources.clone(),
                full_document: false,
            });
            if bundle.items.len() >= MAX_EVIDENCE_EXCERPTS {
                return bundle;
            }
        }
    }
    bundle
}

async fn wait_for_kb_turn_materialization(
    state: &AppState,
    session_id: &str,
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
) -> Option<(tandem_types::Session, usize, usize)> {
    for attempt in 0..20 {
        let session = state.storage.get_session(session_id).await?;
        let (user_idx, assistant_idx) = latest_exchange_indexes(&session)?;
        let user_message = &session.messages[user_idx];
        let has_kb_result = user_message.parts.iter().any(|part| match part {
            MessagePart::ToolInvocation {
                tool,
                result: Some(_),
                ..
            } => tool_matches_kb_policy(tool, policy),
            _ => false,
        });
        if has_kb_result || attempt == 19 {
            return Some((session, user_idx, assistant_idx));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

fn tool_matches_kb_policy(
    tool_name: &str,
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
) -> bool {
    let normalized_tool = normalize_tool_name(tool_name);
    policy
        .tool_patterns
        .iter()
        .any(|pattern| tandem_core::tool_name_matches_policy(pattern, &normalized_tool))
}

fn tool_result_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn looks_like_non_evidence_output(output: &str) -> bool {
    let lower = output.trim().to_ascii_lowercase();
    lower.is_empty()
        || lower.contains("authorization required")
        || lower.contains("authorization pending")
        || lower.contains("call skipped")
        || lower.contains("tool denied")
}

async fn fetch_kb_full_document(
    state: &AppState,
    document_ref: &KbDocumentRef,
) -> Option<Vec<KbEvidenceItem>> {
    let mut args = json!({ "doc_id": document_ref.doc_id });
    if let Some(collection_id) = document_ref
        .collection_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args["collection_id"] = Value::String(collection_id.to_string());
    }
    let result = state
        .mcp
        .call_tool(&document_ref.server_name, "get_document", args)
        .await
        .ok()?;
    let result_value = result
        .metadata
        .get("result")
        .cloned()
        .unwrap_or(Value::Null);
    let output = if !result_value.is_null() {
        result_value.to_string()
    } else {
        result.output
    };
    if output.trim().is_empty() || looks_like_non_evidence_output(&output) {
        return None;
    }
    let excerpts = extract_kb_excerpts(&output, MAX_FULL_DOCUMENT_CHARS);
    if excerpts.is_empty() {
        return None;
    }
    let sources = extract_kb_source_labels(&output);
    Some(
        excerpts
            .into_iter()
            .map(|excerpt| KbEvidenceItem {
                excerpt,
                sources: sources.clone(),
                full_document: true,
            })
            .collect(),
    )
}

fn collect_kb_document_refs(
    message: &tandem_types::Message,
    policy: &tandem_core::KnowledgebaseGroundingPolicy,
) -> Vec<KbDocumentRef> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for part in &message.parts {
        let MessagePart::ToolInvocation {
            tool,
            result: Some(result),
            ..
        } = part
        else {
            continue;
        };
        if !tool_matches_kb_policy(tool, policy) {
            continue;
        }
        let Some(server_name) = mcp_server_name_from_tool(tool) else {
            continue;
        };
        let output = tool_result_text(result);
        let Ok(parsed) = serde_json::from_str::<Value>(output.trim()) else {
            continue;
        };
        collect_document_refs_from_value(&parsed, &server_name, None, &mut refs, &mut seen);
        if refs.len() >= MAX_FULL_DOCUMENT_FETCHES {
            break;
        }
    }
    refs
}

fn collect_document_refs_from_value(
    value: &Value,
    server_name: &str,
    inherited_collection_id: Option<&str>,
    refs: &mut Vec<KbDocumentRef>,
    seen: &mut HashSet<String>,
) {
    if refs.len() >= MAX_FULL_DOCUMENT_FETCHES {
        return;
    }
    match value {
        Value::Object(map) => {
            let local_collection_id = map
                .get("collection_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or(inherited_collection_id);
            if let Some(doc_id) = map
                .get("doc_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let collection_id = local_collection_id.map(ToOwned::to_owned);
                let key = format!("{server_name}:{doc_id}");
                if seen.insert(key) {
                    refs.push(KbDocumentRef {
                        server_name: server_name.to_string(),
                        doc_id: doc_id.to_string(),
                        collection_id,
                    });
                }
                if refs.len() >= MAX_FULL_DOCUMENT_FETCHES {
                    return;
                }
            }
            for nested in map.values() {
                if refs.len() >= MAX_FULL_DOCUMENT_FETCHES {
                    break;
                }
                collect_document_refs_from_value(
                    nested,
                    server_name,
                    local_collection_id,
                    refs,
                    seen,
                );
            }
        }
        Value::Array(items) => {
            for item in items {
                if refs.len() >= MAX_FULL_DOCUMENT_FETCHES {
                    break;
                }
                collect_document_refs_from_value(
                    item,
                    server_name,
                    inherited_collection_id,
                    refs,
                    seen,
                );
            }
        }
        _ => {}
    }
}

fn mcp_server_name_from_tool(tool_name: &str) -> Option<String> {
    let normalized = normalize_tool_name(tool_name);
    let mut parts = normalized.split('.');
    if parts.next()? != "mcp" {
        return None;
    }
    parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_kb_excerpts(output: &str, max_chars: usize) -> Vec<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        let mut excerpts = Vec::new();
        collect_value_excerpts(&parsed, &mut excerpts, max_chars);
        if !excerpts.is_empty() {
            return excerpts;
        }
        if structured_value_signals_no_hits(&parsed) {
            return Vec::new();
        }
    }
    vec![truncate_inline(trimmed, max_chars)]
}

fn collect_value_excerpts(value: &Value, excerpts: &mut Vec<String>, max_chars: usize) {
    if excerpts.len() >= MAX_EVIDENCE_EXCERPTS {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(docs) = map.get("documents").and_then(Value::as_array) {
                for doc in docs {
                    if excerpts.len() >= MAX_EVIDENCE_EXCERPTS {
                        break;
                    }
                    if let Some(excerpt) = render_document_excerpt(doc, max_chars) {
                        excerpts.push(excerpt);
                    }
                }
            }
            if excerpts.is_empty() {
                if let Some(excerpt) = render_document_excerpt(value, max_chars) {
                    excerpts.push(excerpt);
                }
            }
            for nested in map.values() {
                if excerpts.len() >= MAX_EVIDENCE_EXCERPTS {
                    break;
                }
                collect_value_excerpts(nested, excerpts, max_chars);
            }
        }
        Value::Array(items) => {
            for item in items {
                if excerpts.len() >= MAX_EVIDENCE_EXCERPTS {
                    break;
                }
                collect_value_excerpts(item, excerpts, max_chars);
            }
        }
        _ => {}
    }
}

fn render_document_excerpt(value: &Value, max_chars: usize) -> Option<String> {
    let Some(map) = value.as_object() else {
        return None;
    };
    let source = source_label_from_map(map);
    let body = [
        "snippet", "excerpt", "content", "text", "answer", "summary", "body",
    ]
    .iter()
    .find_map(|key| map.get(*key).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())?;
    Some(match source {
        Some(source) => format!("Source: {}\n{}", source, truncate_inline(body, max_chars)),
        None => truncate_inline(body, max_chars),
    })
}

fn source_label_from_map(map: &serde_json::Map<String, Value>) -> Option<String> {
    for key in ["title", "display_title", "display_name", "name"] {
        if let Some(label) = map
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(label.to_string());
        }
    }
    for key in ["relative_path", "source_path", "path", "doc_id", "slug"] {
        if let Some(label) = map
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(safe_source_label(label));
        }
    }
    None
}

fn structured_value_signals_no_hits(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    ["documents", "results", "hits", "matches", "items"]
        .iter()
        .any(|key| {
            map.get(*key)
                .and_then(Value::as_array)
                .is_some_and(|items| items.is_empty())
        })
}

fn extract_kb_source_labels(output: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(output.trim()) else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    collect_source_labels(&parsed, &mut labels);
    merged_sources(labels, Vec::new())
}

fn collect_source_labels(value: &Value, labels: &mut Vec<String>) {
    if labels.len() >= MAX_SOURCE_LABELS {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(label) = source_label_from_map(map) {
                labels.push(label);
                if labels.len() >= MAX_SOURCE_LABELS {
                    return;
                }
            }
            for nested in map.values() {
                if labels.len() >= MAX_SOURCE_LABELS {
                    break;
                }
                collect_source_labels(nested, labels);
            }
        }
        Value::Array(items) => {
            for item in items {
                if labels.len() >= MAX_SOURCE_LABELS {
                    break;
                }
                collect_source_labels(item, labels);
            }
        }
        _ => {}
    }
}

fn safe_source_label(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    let last_segment = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    let stem = last_segment
        .strip_suffix(".md")
        .or_else(|| last_segment.strip_suffix(".markdown"))
        .or_else(|| last_segment.strip_suffix(".txt"))
        .unwrap_or(last_segment);
    let cleaned = stem
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        return "Knowledgebase Source".to_string();
    }
    cleaned
        .split_whitespace()
        .map(|word| {
            if matches!(
                word.to_ascii_lowercase().as_str(),
                "faq" | "kb" | "mcp" | "api" | "ui" | "id" | "url"
            ) {
                return word.to_ascii_uppercase();
            }
            if word.chars().any(|ch| ch.is_ascii_alphabetic())
                && word
                    .chars()
                    .filter(|ch| ch.is_ascii_alphabetic())
                    .all(|ch| ch.is_ascii_uppercase())
            {
                return word.to_string();
            }
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!(
                    "{}{}",
                    first.to_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn synthesize_strict_kb_answer(
    state: &AppState,
    question: &str,
    evidence: &[KbEvidenceItem],
    model_override: Option<&ModelSpec>,
) -> Option<StrictKbSynthesisResponse> {
    let evidence_block = evidence
        .iter()
        .take(MAX_EVIDENCE_EXCERPTS)
        .enumerate()
        .map(|(index, item)| format!("{}. {}", index + 1, item.excerpt))
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: [
                "STRICT KNOWLEDGEBASE GROUNDING IS ENABLED.",
                "Answer only from the retrieved knowledgebase excerpts provided in the next message.",
                "Do not use general model knowledge, external product knowledge, inferred policy, likely owners, or best practices.",
                "If the excerpts do not answer the question, set `kb_answer_support` to `unsupported` and `answer_text` to exactly `I do not see that in the connected knowledgebase.`",
                "If the excerpts explicitly say the policy or answer is not defined, set `kb_answer_support` to `explicitly_undefined` and explain that only from the excerpts.",
                "If the answer is partial, set `kb_answer_support` to `partial`, answer only the supported part, and list the missing facts.",
                "Preserve exact times, dates, names, channels, amounts, paths, IDs, and owners from the excerpts.",
                "Never introduce dates, times, amounts, owners, or numeric facts that are not present in the excerpts.",
                "Never convert, round, or adjust times unless the excerpts explicitly instruct conversion.",
                "If an excerpt says `local venue time`, keep `local venue time`.",
                "Prefer short extractive answers for factual questions.",
                "Return only valid JSON with keys `kb_answer_support`, `supported_facts`, `missing_facts`, `sources`, and `answer_text`.",
            ]
            .join("\n"),
            attachments: Vec::new(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Question:\n{}\n\nRetrieved knowledgebase excerpts:\n{}\n",
                question.trim(),
                evidence_block
            ),
            attachments: Vec::new(),
        },
    ];
    let provider_id = model_override
        .map(|model| model.provider_id.as_str())
        .filter(|value| !value.trim().is_empty());
    let model_id = model_override
        .map(|model| model.model_id.as_str())
        .filter(|value| !value.trim().is_empty());
    let cancel = CancellationToken::new();
    let stream = state
        .providers
        .stream_for_provider(
            provider_id,
            model_id,
            messages,
            ToolMode::None,
            None,
            cancel,
        )
        .await
        .ok()?;
    tokio::pin!(stream);
    let mut completion = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk.ok()? {
            StreamChunk::TextDelta(delta) => {
                let delta = strip_model_control_markers(&delta);
                if !delta.trim().is_empty() {
                    completion.push_str(&delta);
                }
            }
            StreamChunk::Done { .. } => break,
            _ => {}
        }
    }
    parse_strict_synthesis_response(&completion)
}

fn parse_strict_synthesis_response(raw: &str) -> Option<StrictKbSynthesisResponse> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<StrictKbSynthesisResponse>(trimmed)
        .ok()
        .or_else(|| extract_first_json_object(trimmed))
}

fn extract_first_json_object(raw: &str) -> Option<StrictKbSynthesisResponse> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<StrictKbSynthesisResponse>(&raw[start..=end]).ok()
}

fn render_strict_kb_answer(
    support: &str,
    response: &StrictKbSynthesisResponse,
    sources: &[String],
) -> String {
    match support {
        "unsupported" => STRICT_KB_FALLBACK.to_string(),
        "explicitly_undefined" => non_empty_text(&response.answer_text)
            .or_else(|| response.supported_facts.first().cloned())
            .unwrap_or_else(|| STRICT_KB_FALLBACK.to_string()),
        "partial" | "supported" => non_empty_text(&response.answer_text)
            .or_else(|| render_fact_fallback(response, support == "partial"))
            .unwrap_or_else(|| {
                if sources.is_empty() {
                    STRICT_KB_FALLBACK.to_string()
                } else {
                    response
                        .supported_facts
                        .first()
                        .cloned()
                        .unwrap_or_else(|| STRICT_KB_FALLBACK.to_string())
                }
            }),
        _ => STRICT_KB_FALLBACK.to_string(),
    }
}

fn render_fact_fallback(response: &StrictKbSynthesisResponse, partial: bool) -> Option<String> {
    let supported = response
        .supported_facts
        .iter()
        .map(|fact| fact.trim())
        .filter(|fact| !fact.is_empty())
        .collect::<Vec<_>>();
    if supported.is_empty() {
        return None;
    }
    let mut text = supported.join(" ");
    if partial {
        let missing = response
            .missing_facts
            .iter()
            .map(|fact| fact.trim())
            .filter(|fact| !fact.is_empty())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            text.push_str(" Missing from the connected knowledgebase: ");
            text.push_str(&missing.join(", "));
            text.push('.');
        }
    }
    Some(text)
}

fn deterministic_strict_kb_answer(
    question: &str,
    evidence: &[KbEvidenceItem],
) -> Option<(String, String)> {
    let question_lower = question.trim().to_ascii_lowercase();
    let evidence_text = evidence_plain_text(evidence);
    let evidence_lower = evidence_text.to_ascii_lowercase();

    if question_lower.contains("policy")
        && (evidence_lower.contains("does not define policy")
            || evidence_lower.contains("does not define a policy")
            || evidence_lower.contains("no policy is available")
            || evidence_lower.contains("no policy exists"))
    {
        let subject = if question_lower.contains("crypto prize payout")
            || question_lower.contains("crypto prize payouts")
        {
            "a crypto prize payout policy"
        } else {
            "this policy"
        };
        let support_sentence = evidence_sentences_from_text(&evidence_text)
            .into_iter()
            .find(|sentence| {
                let lower = sentence.to_ascii_lowercase();
                lower.contains("does not define") || lower.contains("no policy")
            })
            .unwrap_or_else(|| {
                "The connected knowledgebase does not define this policy.".to_string()
            });
        return Some((
            "explicitly_undefined".to_string(),
            format!("I do not see {subject} in the connected knowledgebase. {support_sentence}"),
        ));
    }

    if asks_for_external_moderation_action(&question_lower)
        && (evidence_lower.contains("must not ban")
            || evidence_lower.contains("must not ban, timeout, delete, or moderate")
            || evidence_lower.contains("must not moderate"))
    {
        return extractive_strict_kb_answer(question, evidence)
            .map(|answer| ("supported".to_string(), answer));
    }

    if asks_for_phone_number(&question_lower)
        && (evidence_lower.contains("phone numbers are not included")
            || evidence_lower.contains("private phone numbers are not included")
            || evidence_lower.contains("does not contain real private phone numbers")
            || evidence_lower.contains("demo email"))
    {
        let person = first_name_in_question(question).unwrap_or_else(|| "that person".to_string());
        let sentences = evidence_sentences_from_text(&evidence_text);
        let mut selected = sentences
            .iter()
            .filter(|sentence| sentence.to_ascii_lowercase().contains("phone"))
            .cloned()
            .collect::<Vec<_>>();
        selected.extend(
            sentences
                .iter()
                .filter(|sentence| sentence.to_ascii_lowercase().contains("demo email"))
                .cloned()
                .take(1),
        );
        selected.extend(
            sentences
                .into_iter()
                .filter(|sentence| {
                    let lower = sentence.to_ascii_lowercase();
                    !lower.contains("phone")
                        && !lower.contains("demo email")
                        && (lower.contains("role")
                            || lower.contains("responsibil")
                            || sentence.contains(&person))
                })
                .take(1),
        );
        selected.truncate(3);
        let detail = if selected.is_empty() {
            "The connected knowledgebase does not include that private contact detail.".to_string()
        } else {
            selected.join(" ")
        };
        return Some((
            "partial".to_string(),
            format!("I do not see a phone number for {person} in the knowledgebase. {detail}"),
        ));
    }

    None
}

fn snippet_evidence_can_safely_answer(question: &str, evidence: &[KbEvidenceItem]) -> bool {
    evidence.iter().any(|item| item.full_document)
        || deterministic_strict_kb_answer(question, evidence).is_some()
}

fn evidence_plain_text(evidence: &[KbEvidenceItem]) -> String {
    evidence
        .iter()
        .map(|item| strip_evidence_source_prefix(&item.excerpt))
        .collect::<Vec<_>>()
        .join("\n")
}

fn asks_for_external_moderation_action(question_lower: &str) -> bool {
    (question_lower.starts_with("can ") || question_lower.starts_with("could "))
        && (question_lower.contains("ban")
            || question_lower.contains("timeout")
            || question_lower.contains("delete")
            || question_lower.contains("moderate"))
}

fn asks_for_phone_number(question_lower: &str) -> bool {
    question_lower.contains("phone number")
        || question_lower.contains("phone")
        || question_lower.contains("mobile")
        || question_lower.contains("cell number")
}

fn first_name_in_question(question: &str) -> Option<String> {
    name_token_regex()
        .find(question)
        .map(|mat| mat.as_str().trim().to_string())
}

fn strict_kb_answer_has_unsupported_advice(answer: &str, evidence: &[KbEvidenceItem]) -> bool {
    let answer_lower = answer.to_ascii_lowercase();
    let evidence_lower = evidence_plain_text(evidence).to_ascii_lowercase();
    [
        "right-click",
        "right click",
        "select ban",
        "ban user",
        "delete message history",
        "approved standard channels",
        "standard channels",
        "declined/escalated",
        "decline and escalate",
        "internal event ops procedures",
        "finance review",
        "operations owner",
        "wallet",
        "private key",
        "not visible in the available result snippet",
        "not visible in snippet",
        "full phone number visible",
    ]
    .iter()
    .any(|phrase| answer_lower.contains(phrase) && !evidence_lower.contains(phrase))
}

fn unsupported_strict_kb_fact_tokens(
    answer: &str,
    evidence: &[KbEvidenceItem],
) -> Option<Vec<String>> {
    let answer_tokens = strict_kb_fact_tokens(answer);
    if answer_tokens.is_empty() {
        return None;
    }
    let evidence_text = evidence
        .iter()
        .map(|item| item.excerpt.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_tokens = strict_kb_fact_tokens(&evidence_text);
    let unsupported = answer_tokens
        .into_iter()
        .filter(|token| !evidence_tokens.contains(token))
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        None
    } else {
        Some(unsupported)
    }
}

fn strict_kb_fact_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    for mat in name_token_regex().find_iter(text) {
        tokens.insert(normalize_fact_token(&mat.as_str().to_ascii_lowercase()));
    }
    let normalized = text.to_ascii_lowercase();
    for regex in [
        time_token_regex(),
        month_date_token_regex(),
        amount_token_regex(),
    ] {
        for mat in regex.find_iter(&normalized) {
            tokens.insert(normalize_fact_token(mat.as_str()));
        }
    }
    tokens
}

fn normalize_fact_token(token: &str) -> String {
    token
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | ')' | '('))
        .to_string()
}

fn time_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\b\d{1,2}:\d{2}\s*(?:am|pm)?\b").expect("time regex"))
}

fn month_date_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"\b(?:jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\s+\d{1,2}(?:st|nd|rd|th)?\b",
        )
        .expect("month date regex")
    })
}

fn amount_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?:[$€£]\s?\d[\d,]*(?:\.\d+)?|\b\d[\d,]*(?:\.\d+)?\s?(?:usd|eur|gbp)\b)")
            .expect("amount regex")
    })
}

fn name_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)+\b").expect("name token regex")
    })
}

fn extractive_strict_kb_answer(question: &str, evidence: &[KbEvidenceItem]) -> Option<String> {
    let question_keywords = strict_kb_question_keywords(question);
    let evidence_text = evidence
        .iter()
        .map(|item| item.excerpt.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let mut selected = Vec::new();
    for sentence in evidence
        .iter()
        .flat_map(|item| evidence_sentences(&item.excerpt))
        .filter(|sentence| !sentence.trim().is_empty())
    {
        let sentence_lower = sentence.to_ascii_lowercase();
        let has_fact_token = !strict_kb_fact_tokens(&sentence).is_empty();
        let matches_question = question_keywords
            .iter()
            .any(|keyword| sentence_lower.contains(keyword));
        if has_fact_token || matches_question {
            selected.push(sentence.trim().to_string());
        }
        if selected.len() >= 3 {
            break;
        }
    }
    let mut answer = if selected.is_empty() {
        evidence
            .first()
            .map(|item| strip_evidence_source_prefix(&item.excerpt))
            .map(|text| truncate_inline(&text, MAX_EVIDENCE_CHARS))
            .filter(|text| !text.trim().is_empty())
    } else {
        Some(selected.join(" "))
    }?;
    if question.trim().to_ascii_lowercase().starts_with("can ")
        && evidence_text.contains("must not ban")
        && !answer.to_ascii_lowercase().starts_with("i cannot ban")
    {
        answer = format!("I cannot ban users from here. {answer}");
    }
    Some(answer)
}

fn strict_kb_question_keywords(question: &str) -> Vec<String> {
    question
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|word| word.len() >= 4)
        .filter(|word| {
            !matches!(
                *word,
                "what"
                    | "when"
                    | "where"
                    | "which"
                    | "does"
                    | "should"
                    | "must"
                    | "with"
                    | "from"
                    | "that"
                    | "this"
                    | "time"
            )
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn evidence_sentences(excerpt: &str) -> Vec<String> {
    evidence_sentences_from_text(&strip_evidence_source_prefix(excerpt))
}

fn evidence_sentences_from_text(text: &str) -> Vec<String> {
    text.split_inclusive(['.', '?', '!'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn strip_evidence_source_prefix(excerpt: &str) -> String {
    excerpt
        .lines()
        .filter(|line| !line.trim_start().starts_with("Source:"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn append_source_footer(answer: String, sources: &[String]) -> String {
    let sources = merged_sources(sources.to_vec(), Vec::new());
    if sources.is_empty() {
        return answer.trim().to_string();
    }
    if sources.len() == 1 {
        format!("{}\n\nSource: {}", answer.trim(), sources[0])
    } else {
        format!("{}\n\nSources: {}", answer.trim(), sources.join(", "))
    }
}

fn merged_sources(primary: Vec<String>, fallback: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    primary
        .into_iter()
        .chain(fallback)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| safe_source_label(&value))
        .filter(|value| seen.insert(value.clone()))
        .take(MAX_SOURCE_LABELS)
        .collect()
}

fn normalize_support_label(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "supported" => "supported",
        "partial" => "partial",
        "explicitly_undefined" => "explicitly_undefined",
        _ => "unsupported",
    }
}

fn non_empty_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn truncate_inline(value: &str, max_len: usize) -> String {
    truncate_text(&value.replace('\n', " ").replace('\r', " "), max_len)
}

fn normalize_tool_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

fn strip_model_control_markers(input: &str) -> String {
    let mut cleaned = input.to_string();
    for marker in ["<|eom|>", "<|eot_id|>", "<|im_end|>", "<|end|>"] {
        if cleaned.contains(marker) {
            cleaned = cleaned.replace(marker, "");
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_label_extraction_reads_nested_document_paths() {
        let labels = extract_kb_source_labels(
            r#"{"documents":[{"relative_path":"refund-and-billing-policy.md"},{"doc_id":"staff-roles-and-contacts.md"}]}"#,
        );
        assert_eq!(
            labels,
            vec![
                "Refund And Billing Policy".to_string(),
                "Staff Roles And Contacts".to_string()
            ]
        );
    }

    #[test]
    fn structured_empty_hits_do_not_count_as_evidence() {
        let excerpts = extract_kb_excerpts(r#"{"documents":[]}"#, MAX_EVIDENCE_CHARS);
        assert!(excerpts.is_empty());
    }

    #[test]
    fn source_label_extraction_prefers_safe_display_titles() {
        let labels = extract_kb_source_labels(
            r#"{"document":{"title":"Discord Community Rules","doc_id":"northstar-events/discord-community-rules","source_path":"/workspace/kb-data/northstar-events/discord-community-rules.md"}}"#,
        );
        assert_eq!(labels, vec!["Discord Community Rules".to_string()]);
    }

    #[test]
    fn source_label_extraction_does_not_expose_storage_paths() {
        let labels = extract_kb_source_labels(
            r#"{"results":[{"doc_id":"northstar-events/company-overview","source_path":"/workspace/kb-data/northstar-events/company-overview.md"}]}"#,
        );
        assert_eq!(labels, vec!["Company Overview".to_string()]);
    }

    #[test]
    fn document_refs_are_collected_from_kb_search_results() {
        let policy = tandem_core::KnowledgebaseGroundingPolicy {
            required: true,
            strict: true,
            server_names: vec!["kb".to_string()],
            tool_patterns: vec!["mcp.kb.*".to_string()],
        };
        let message = tandem_types::Message::new(
            MessageRole::User,
            vec![MessagePart::ToolInvocation {
                tool: "mcp.kb.search_docs".to_string(),
                args: json!({"query": "crypto prize payouts"}),
                result: Some(json!({
                    "collection_id": "northstar-events",
                    "results": [{
                        "doc_id": "northstar-events/company-overview",
                        "source_path": "company-overview.md",
                        "excerpt": "Important internal note"
                    }]
                })),
                error: None,
            }],
        );
        let refs = collect_kb_document_refs(&message, &policy);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].server_name, "kb");
        assert_eq!(refs[0].doc_id, "northstar-events/company-overview");
        assert_eq!(refs[0].collection_id.as_deref(), Some("northstar-events"));
    }

    #[test]
    fn full_document_evidence_supports_explicitly_undefined_policy() {
        let evidence = vec![KbEvidenceItem {
            excerpt: "Source: Company Overview\nThe knowledgebase does not define policy for crypto prize payouts, token rewards, or blockchain-based giveaways. The correct response is that no policy is available in the current knowledgebase.".to_string(),
            sources: vec!["Company Overview".to_string()],
            full_document: true,
        }];
        let (_, answer) = deterministic_strict_kb_answer(
            "What is the policy for crypto prize payouts?",
            &evidence,
        )
        .expect("deterministic answer");
        assert!(answer.contains("I do not see a crypto prize payout policy"));
        assert!(answer.contains("does not define policy for crypto prize payouts"));
        assert!(!answer.contains("approved standard channels"));
        assert!(!answer.contains("wallet"));
    }

    #[test]
    fn full_document_evidence_supports_missing_private_contact_info() {
        let evidence = vec![KbEvidenceItem {
            excerpt: "Source: Staff Roles and Contacts\nMira Kovac is the event director. Responsibilities include final escalation decisions. Demo email: mira@example.test. This demo knowledgebase does not contain real private phone numbers.".to_string(),
            sources: vec!["Staff Roles and Contacts".to_string()],
            full_document: true,
        }];
        let (_, answer) =
            deterministic_strict_kb_answer("What is Mira Kovac's phone number?", &evidence)
                .expect("deterministic answer");
        assert!(answer.contains("I do not see a phone number for Mira Kovac"));
        assert!(answer.contains("does not contain real private phone numbers"));
        assert!(!answer.contains("not visible in snippet"));
    }
}
