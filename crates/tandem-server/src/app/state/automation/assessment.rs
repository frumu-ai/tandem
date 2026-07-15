// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

fn artifact_candidate_source_priority(source: &str) -> i64 {
    match source {
        "verified_output" => 3,
        "session_write" => 2,
        "preexisting_output" => 1,
        _ => 0,
    }
}

fn normalized_anchor_variants(value: &str) -> Vec<String> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut variants = HashSet::new();
    variants.insert(trimmed.clone());
    let collapsed = trimmed
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !collapsed.is_empty() {
        variants.insert(collapsed);
    }
    let compact = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if !compact.is_empty() {
        variants.insert(compact);
    }
    if trimmed.contains('/') {
        if let Some(file_name) = Path::new(&trimmed)
            .file_name()
            .and_then(|value| value.to_str())
        {
            variants.insert(file_name.to_ascii_lowercase());
            variants.extend(
                file_name
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
                    .collect::<String>()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            );
        }
        if let Some(stem) = Path::new(&trimmed)
            .file_stem()
            .and_then(|value| value.to_str())
        {
            variants.insert(stem.to_ascii_lowercase());
            variants.extend(
                stem.chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
                    .collect::<String>()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            );
        }
    }
    variants.into_iter().collect()
}

fn source_anchor_variants(source: &str) -> Vec<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut variants = normalized_anchor_variants(trimmed);
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    variants.extend(normalized_anchor_variants(host));
    if let Some(last_segment) = without_scheme.rsplit('/').next() {
        variants.extend(normalized_anchor_variants(last_segment));
    }
    variants.sort();
    variants.dedup();
    variants
}

pub(crate) fn source_evidence_anchor_target(read_paths: &[String], citations: &[String]) -> usize {
    let unique_sources = read_paths
        .iter()
        .chain(citations.iter())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    match unique_sources.len() {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

pub(crate) fn evidence_anchor_count(
    text: &str,
    read_paths: &[String],
    citations: &[String],
) -> usize {
    let lowered = text.to_ascii_lowercase();
    let mut matched = HashSet::new();
    for source in read_paths.iter().chain(citations.iter()) {
        let source = source.trim();
        if source.is_empty() {
            continue;
        }
        let matched_source = source_anchor_variants(source)
            .into_iter()
            .any(|variant| !variant.is_empty() && lowered.contains(&variant));
        if matched_source {
            matched.insert(source.to_ascii_lowercase());
        }
    }
    matched.len()
}

pub(crate) fn artifact_text_contradicts_successful_web_research(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "unavailable_in_current_tooling",
        "no websearch/webfetch tools were available",
        "no websearch or webfetch tools were available",
        "no websearch tools were available",
        "no web research tools were available",
        "no external urls could be fetched",
        "external web research could not be performed",
        "web research could not be performed",
        "websearch unavailable",
        "web research unavailable",
        "current external web research could not be performed",
        "no dated market or technical sources were captured",
        "no external sources were captured",
        "no web sources were captured",
        "web research returned no usable output",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

pub(crate) fn assess_artifact_candidate(
    node: &AutomationFlowNode,
    workspace_root: &str,
    source: &str,
    text: &str,
    read_paths: &[String],
    discovered_relevant_paths: &[String],
    upstream_read_paths: &[String],
    upstream_citations: &[String],
) -> ArtifactCandidateAssessment {
    let trimmed = text.trim();
    let length = trimmed.len();
    let placeholder_like = placeholder_like_artifact_text(trimmed);
    let substantive = substantive_artifact_text(trimmed);
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    let required_section_count = artifact_required_section_count(node, trimmed);
    let reviewed_paths = extract_markdown_section_paths(trimmed, "Files reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let files_not_reviewed = extract_markdown_section_paths(trimmed, "Files not reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let reviewed_paths_backed_by_read = reviewed_paths
        .iter()
        .filter(|path| read_paths.iter().any(|read| read == *path))
        .cloned()
        .collect::<Vec<_>>();
    let files_reviewed_present = files_reviewed_section_lists_paths(trimmed);
    let citation_count = markdown_citation_count(trimmed);
    let web_sources_reviewed_present = web_sources_reviewed_section_lists_sources(trimmed);
    let effective_relevant_paths = if discovered_relevant_paths.is_empty() {
        reviewed_paths.clone()
    } else {
        discovered_relevant_paths.to_vec()
    };
    let evidence_anchor_count =
        evidence_anchor_count(trimmed, upstream_read_paths, upstream_citations);
    let unreviewed_relevant_paths = effective_relevant_paths
        .iter()
        .filter(|path| {
            !read_paths.iter().any(|read| read == *path)
                && !files_not_reviewed.iter().any(|skipped| skipped == *path)
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut score = 0i64;
    score += artifact_candidate_source_priority(source) * 25;
    score += (length.min(12_000) / 24) as i64;
    score += (heading_count as i64) * 60;
    score += (list_count as i64) * 18;
    score += (paragraph_count as i64) * 24;
    score += (required_section_count as i64) * 160;
    score += (evidence_anchor_count.min(5) as i64) * 120;
    if substantive {
        score += 2_000;
    }
    if files_reviewed_present {
        score += 180;
    }
    score += (citation_count.min(8) as i64) * 45;
    if web_sources_reviewed_present {
        score += 140;
    }
    if !reviewed_paths.is_empty() && reviewed_paths.len() == reviewed_paths_backed_by_read.len() {
        score += 260;
    } else if !reviewed_paths_backed_by_read.is_empty() {
        score += 90;
    }
    score -= (unreviewed_relevant_paths.len() as i64) * 220;
    if placeholder_like {
        score -= 450;
    }
    if trimmed.is_empty() {
        score -= 2_000;
    }

    ArtifactCandidateAssessment {
        source: source.to_string(),
        text: text.to_string(),
        length,
        score,
        substantive,
        placeholder_like,
        heading_count,
        list_count,
        paragraph_count,
        required_section_count,
        files_reviewed_present,
        reviewed_paths,
        reviewed_paths_backed_by_read,
        unreviewed_relevant_paths,
        citation_count,
        web_sources_reviewed_present,
        evidence_anchor_count,
    }
}

pub(crate) fn artifact_text_contains_required_tool_mode_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("tool_mode_required_not_satisfied")
        || lower.contains("write_required_not_satisfied")
        || lower.contains("tool choice 'required' must be specified with 'tools' parameter")
        || lower.contains("tool choice `required` must be specified with `tools` parameter")
}

fn value_has_nonempty_key(value: &Value, keys: &[&str]) -> bool {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if keys
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                {
                    match child {
                        Value::Null => {}
                        Value::Array(items) if items.is_empty() => {}
                        Value::Object(items) if items.is_empty() => {}
                        Value::String(text) if text.trim().is_empty() => {}
                        _ => return true,
                    }
                }
                if value_has_nonempty_key(child, keys) {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => items
            .iter()
            .any(|child| value_has_nonempty_key(child, keys)),
        _ => false,
    }
}

pub(crate) fn artifact_text_has_connector_source_evidence_or_limitation(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        const EVIDENCE_KEYS: &[&str] = &[
            "posts",
            "items",
            "findings",
            "signals",
            "source_url",
            "permalink",
            "selftext",
            "citations",
            "citations_external",
            "sources_reviewed",
            "web_sources_reviewed",
            "tool_evidence",
            "tool_results",
            "search_queries_used",
            "result_excerpt",
        ];
        const LIMITATION_KEYS: &[&str] = &[
            "limitations",
            "source_limitations",
            "connector_limitations",
            "tool_limitations",
        ];
        return value_has_nonempty_key(&value, EVIDENCE_KEYS)
            || value_has_nonempty_key(&value, LIMITATION_KEYS);
    }
    let lower = trimmed.to_ascii_lowercase();
    [
        "https://www.reddit.com/",
        "permalink",
        "source_url",
        "source url",
        "citations",
        "sources reviewed",
        "connector limitation",
        "source limitation",
        "tool limitation",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(crate) fn artifact_text_has_receipt_backed_connector_source_evidence(
    text: &str,
    assessment: Option<&ArtifactCandidateAssessment>,
    executed_concrete_mcp_tools: &[String],
    selected_mcp_server_names: &[String],
) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if artifact_text_has_connector_source_evidence_or_limitation(trimmed) {
        return true;
    }
    let Some(assessment) = assessment else {
        return false;
    };

    let lowered = trimmed.to_ascii_lowercase();
    let has_receipt_anchor = executed_concrete_mcp_tools
        .iter()
        .chain(selected_mcp_server_names.iter())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .any(|value| lowered.contains(&value));
    let has_structural_substance = assessment.substantive
        || assessment.length >= 120
        || assessment.heading_count > 0
        || assessment.list_count >= 2
        || assessment.paragraph_count >= 2;

    if has_receipt_anchor && (has_structural_substance || assessment.length >= 80) {
        return true;
    }
    !assessment.placeholder_like && has_structural_substance
}

pub(crate) fn artifact_text_is_mcp_inventory_only(text: &str) -> bool {
    let trimmed = text.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let inventory_keys = [
        "connected_server_names",
        "enabled_server_names",
        "inventory_version",
        "registered_tools",
        "remote_tools",
        "servers",
    ];
    let has_inventory_shape = inventory_keys
        .iter()
        .filter(|key| object.contains_key(**key))
        .count()
        >= 3;
    has_inventory_shape && !artifact_text_has_connector_source_evidence_or_limitation(trimmed)
}

pub(crate) fn best_artifact_candidate(
    candidates: &[ArtifactCandidateAssessment],
) -> Option<ArtifactCandidateAssessment> {
    candidates.iter().cloned().max_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then(left.substantive.cmp(&right.substantive))
            .then(
                left.required_section_count
                    .cmp(&right.required_section_count),
            )
            .then(left.evidence_anchor_count.cmp(&right.evidence_anchor_count))
            .then(left.heading_count.cmp(&right.heading_count))
            .then(left.length.cmp(&right.length))
            .then(
                artifact_candidate_source_priority(&left.source)
                    .cmp(&artifact_candidate_source_priority(&right.source)),
            )
    })
}

pub(crate) fn artifact_candidate_summary(
    candidate: &ArtifactCandidateAssessment,
    accepted: bool,
) -> Value {
    json!({
        "source": candidate.source,
        "length": candidate.length,
        "score": candidate.score,
        "substantive": candidate.substantive,
        "placeholder_like": candidate.placeholder_like,
        "heading_count": candidate.heading_count,
        "list_count": candidate.list_count,
        "paragraph_count": candidate.paragraph_count,
        "required_section_count": candidate.required_section_count,
        "files_reviewed_present": candidate.files_reviewed_present,
        "reviewed_paths_backed_by_read": candidate.reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": candidate.unreviewed_relevant_paths,
        "citation_count": candidate.citation_count,
        "web_sources_reviewed_present": candidate.web_sources_reviewed_present,
        "evidence_anchor_count": candidate.evidence_anchor_count,
        "accepted": accepted,
    })
}
