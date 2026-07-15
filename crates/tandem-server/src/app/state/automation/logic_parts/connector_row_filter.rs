// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1


fn automation_node_expects_connector_row_filter(node: &AutomationFlowNode) -> bool {
    automation_node_metadata_string(node, "source_node_id").is_some()
        && node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.schema.as_ref())
            .is_some_and(|schema| {
                schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|properties| properties.contains_key("leads"))
                    || schema
                        .get("required")
                        .and_then(Value::as_array)
                        .is_some_and(|required| {
                            required.iter().any(|value| value.as_str() == Some("leads"))
                        })
            })
}

fn automation_filter_artifact_lead_count(text: &str) -> Option<usize> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value.get("leads").and_then(Value::as_array).map(Vec::len))
}

fn automation_filter_artifact_has_truncated_identity(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(leads) = value.get("leads").and_then(Value::as_array) else {
        return false;
    };
    leads.iter().any(|lead| {
        ["thread_link", "url", "permalink", "topic_thread_title", "title"]
            .iter()
            .filter_map(|key| lead.get(key).and_then(Value::as_str))
            .any(|text| text.contains("...") || text.contains('…'))
    })
}

fn automation_row_string(row: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| row.get(*key))
        .find_map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    value.as_i64().map(|number| number.to_string()).or_else(|| {
                        value.as_u64().map(|number| number.to_string()).or_else(|| {
                            value.as_f64().map(|number| number.to_string())
                        })
                    })
                })
        })
}

fn automation_row_text_for_filter(row: &Value) -> String {
    [
        "title",
        "topic_thread_title",
        "name",
        "headline",
        "selftext",
        "selftext_excerpt",
        "body",
        "description",
        "summary",
        "url",
        "permalink",
        "subreddit",
        "subreddit_prefixed",
    ]
    .iter()
    .filter_map(|key| row.get(*key).and_then(Value::as_str))
    .collect::<Vec<_>>()
    .join("\n")
}

fn automation_normalized_subreddit(row: &Value) -> String {
    let raw = automation_row_string(row, &["subreddit_prefixed", "subreddit"])
        .unwrap_or_else(|| "unknown".to_string());
    let trimmed = raw.trim();
    if trimmed.starts_with("r/") || trimmed == "unknown" {
        trimmed.to_string()
    } else {
        format!("r/{trimmed}")
    }
}

fn automation_normalized_thread_link(row: &Value) -> Option<String> {
    for key in ["permalink", "thread_link", "url", "href"] {
        let Some(value) = row.get(key).and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if value.is_empty() || value.contains("...") || value.contains('…') {
            continue;
        }
        if value.starts_with("https://www.reddit.com/")
            || value.starts_with("https://reddit.com/")
            || value.starts_with("http://www.reddit.com/")
            || value.starts_with("http://reddit.com/")
        {
            return Some(value.to_string());
        }
        if value.starts_with("/r/") {
            return Some(format!("https://www.reddit.com{value}"));
        }
    }
    for key in ["url", "href"] {
        let value = row.get(key).and_then(Value::as_str).map(str::trim)?;
        if value.starts_with("http") && !value.contains("...") && !value.contains('…') {
            return Some(value.to_string());
        }
    }
    None
}

fn automation_keyword_matches(text: &str, keywords: &[String]) -> Vec<String> {
    let lowered = text.to_ascii_lowercase();
    keywords
        .iter()
        .map(|keyword| keyword.trim())
        .filter(|keyword| !keyword.is_empty())
        .filter(|keyword| lowered.contains(&keyword.to_ascii_lowercase()))
        .map(str::to_string)
        .collect()
}

fn automation_connector_filter_lead_from_row(
    row: &Value,
    filter_keywords: &[String],
    reject_keywords: &[String],
) -> Option<Value> {
    let title = automation_row_string(
        row,
        &["topic_thread_title", "title", "name", "headline", "summary"],
    )?;
    if title.contains("...") || title.contains('…') {
        return None;
    }
    let thread_link = automation_normalized_thread_link(row)?;
    let text = automation_row_text_for_filter(row);
    if !automation_keyword_matches(&text, reject_keywords).is_empty() {
        return None;
    }
    let matched = automation_keyword_matches(&text, filter_keywords);
    if !filter_keywords.is_empty() && matched.is_empty() {
        return None;
    }
    let subreddit = automation_normalized_subreddit(row);
    let user_handle = automation_row_string(row, &["user_handle", "author", "username", "user"])
        .unwrap_or_else(|| "unknown".to_string());
    let matched_summary = if matched.is_empty() {
        "connector row matched the declared filter contract".to_string()
    } else {
        format!("matched keywords: {}", matched.join(", "))
    };
    Some(json!({
        "topic_thread_title": title,
        "subreddit": subreddit,
        "core_pain_point": format!(
            "{matched_summary}; review the source thread for infrastructure, security, governance, or production fit."
        ),
        "user_handle": user_handle,
        "thread_link": thread_link,
        "status": "New Lead",
    }))
}

fn automation_run_output_artifact_path(
    run: &AutomationV2RunRecord,
    node_id: &str,
) -> Option<String> {
    let output = run.checkpoint.node_outputs.get(node_id)?;
    node_output::automation_output_validated_artifact(output)
        .map(|(path, _)| path)
        .or_else(|| {
            output
                .pointer("/artifact_validation/accepted_artifact_path")
                .or_else(|| output.get("output_path"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            output
                .get("artifact_refs")
                .and_then(Value::as_array)
                .and_then(|refs| refs.first())
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub(crate) fn automation_build_connector_row_filter_artifact(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> anyhow::Result<Option<Value>> {
    if !automation_node_expects_connector_row_filter(node) {
        return Ok(None);
    }
    let Some(source_node_id) = automation_node_metadata_string(node, "source_node_id") else {
        return Ok(None);
    };
    let Some(source_path) = automation_run_output_artifact_path(run, &source_node_id) else {
        return Ok(None);
    };
    let resolved = resolve_automation_output_path(workspace_root, &source_path)?;
    let text = std::fs::read_to_string(resolved)?;
    let source_artifact = serde_json::from_str::<Value>(&text)?;
    let mut rows = Vec::new();
    automation_collect_connector_source_rows_from_value(&source_artifact, &mut rows, 0);
    if rows.is_empty() {
        return Ok(None);
    }
    let filter_keywords = automation_node_metadata_string_array(node, "filter_keywords");
    let reject_keywords = automation_node_metadata_string_array(node, "reject_keywords");
    let mut seen = std::collections::BTreeSet::new();
    let leads = rows
        .iter()
        .filter_map(|row| {
            let lead =
                automation_connector_filter_lead_from_row(row, &filter_keywords, &reject_keywords)?;
            let key = lead
                .get("thread_link")
                .or_else(|| lead.get("user_handle"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if key.is_empty() || !seen.insert(key) {
                return None;
            }
            Some(lead)
        })
        .collect::<Vec<_>>();
    if leads.is_empty() {
        return Ok(None);
    }
    let source_alias = automation_node_metadata_string(node, "source_alias")
        .unwrap_or_else(|| source_node_id.clone());
    let mut source_counts = serde_json::Map::new();
    source_counts.insert(source_alias, json!(rows.len()));
    let lead_count = leads.len();
    Ok(Some(json!({
        "status": "completed",
        "leads": leads,
        "rejected_count": rows.len().saturating_sub(lead_count),
        "source_counts": Value::Object(source_counts),
        "notes": [
            "Deterministically materialized from the validated upstream connector source artifact using this node's filter_keywords and reject_keywords metadata."
        ],
        "deterministic_filter_materialization": {
            "source": "connector_row_filter_materializer",
            "source_node_id": source_node_id,
            "source_artifact_path": source_path,
            "source_row_count": rows.len(),
            "lead_count": lead_count,
        }
    })))
}

fn automation_try_materialize_connector_row_filter_artifact(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
    output_path: &str,
    verified_output: Option<&(String, String, AutomationVerifiedOutputResolution)>,
) -> anyhow::Result<Option<(String, String, AutomationVerifiedOutputResolution)>> {
    let should_materialize = verified_output
        .map(|(_, text, _)| {
            automation_filter_artifact_lead_count(text).unwrap_or(0) == 0
                || automation_filter_artifact_has_truncated_identity(text)
        })
        .unwrap_or(true);
    if !should_materialize {
        return Ok(None);
    }
    let Some(artifact) =
        automation_build_connector_row_filter_artifact(run, node, workspace_root)?
    else {
        return Ok(None);
    };
    let resolved = resolve_automation_output_path(workspace_root, output_path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&artifact)?;
    std::fs::write(&resolved, format!("{text}\n"))?;
    let display_path = resolved
        .strip_prefix(workspace_root)
        .ok()
        .and_then(|value| value.to_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| output_path.to_string());
    Ok(Some((
        display_path,
        format!("{text}\n"),
        AutomationVerifiedOutputResolution {
            path: resolved,
            legacy_workspace_artifact_promoted_from: None,
            materialized_by_current_attempt: true,
            resolution_kind: AutomationVerifiedOutputResolutionKind::Direct,
        },
    )))
}


