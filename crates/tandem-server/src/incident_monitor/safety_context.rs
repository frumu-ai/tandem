// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use tandem_incident_monitor::{
    IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord, IncidentMonitorSubmission,
};

use crate::app::state::truncate_text;

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtractedSafetyContext {
    pub actor: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
    pub action: Option<String>,
    pub policy: Option<String>,
    pub approval_state: Option<String>,
    pub risk_category: Option<String>,
    pub blast_radius: Option<String>,
    pub external_correlation_ids: Vec<String>,
}

pub(crate) fn normalize_submission_safety_context(submission: &mut IncidentMonitorSubmission) {
    submission.actor = normalize_safety_context_optional(submission.actor.take());
    submission.model = normalize_safety_context_optional(submission.model.take());
    submission.tool_name = normalize_safety_context_optional(submission.tool_name.take());
    submission.action = normalize_safety_context_optional(submission.action.take());
    submission.policy = normalize_safety_context_optional(submission.policy.take());
    submission.approval_state = normalize_safety_context_optional(submission.approval_state.take());
    submission.risk_category = normalize_safety_context_optional(submission.risk_category.take());
    submission.blast_radius = normalize_safety_context_optional(submission.blast_radius.take());
    submission.external_correlation_ids =
        normalize_safety_context_vec(std::mem::take(&mut submission.external_correlation_ids), 50);
}

pub(crate) fn apply_submission_safety_context_to_draft(
    draft: &mut IncidentMonitorDraftRecord,
    submission: &IncidentMonitorSubmission,
) -> bool {
    let mut changed = false;
    changed |= fill_missing_option(&mut draft.actor, &submission.actor);
    changed |= fill_missing_option(&mut draft.model, &submission.model);
    changed |= fill_missing_option(&mut draft.tool_name, &submission.tool_name);
    changed |= fill_missing_option(&mut draft.action, &submission.action);
    changed |= fill_missing_option(&mut draft.policy, &submission.policy);
    changed |= fill_missing_option(&mut draft.approval_state, &submission.approval_state);
    changed |= fill_missing_option(&mut draft.risk_category, &submission.risk_category);
    changed |= fill_missing_option(&mut draft.blast_radius, &submission.blast_radius);
    changed |= merge_missing_values(
        &mut draft.external_correlation_ids,
        &submission.external_correlation_ids,
    );
    changed
}

pub(crate) fn apply_submission_safety_context_to_incident(
    incident: &mut IncidentMonitorIncidentRecord,
    submission: &IncidentMonitorSubmission,
) {
    fill_missing_option(&mut incident.actor, &submission.actor);
    fill_missing_option(&mut incident.model, &submission.model);
    fill_missing_option(&mut incident.tool_name, &submission.tool_name);
    fill_missing_option(&mut incident.action, &submission.action);
    fill_missing_option(&mut incident.policy, &submission.policy);
    fill_missing_option(&mut incident.approval_state, &submission.approval_state);
    fill_missing_option(&mut incident.risk_category, &submission.risk_category);
    fill_missing_option(&mut incident.blast_radius, &submission.blast_radius);
    merge_missing_values(
        &mut incident.external_correlation_ids,
        &submission.external_correlation_ids,
    );
}

pub(crate) fn draft_defaults_from_submission(
    submission: &IncidentMonitorSubmission,
) -> IncidentMonitorDraftRecord {
    IncidentMonitorDraftRecord {
        actor: submission.actor.clone(),
        model: submission.model.clone(),
        tool_name: submission.tool_name.clone(),
        action: submission.action.clone(),
        policy: submission.policy.clone(),
        approval_state: submission.approval_state.clone(),
        risk_category: submission.risk_category.clone(),
        blast_radius: submission.blast_radius.clone(),
        external_correlation_ids: submission.external_correlation_ids.clone(),
        ..IncidentMonitorDraftRecord::default()
    }
}

pub(crate) fn incident_defaults_from_submission(
    submission: &IncidentMonitorSubmission,
) -> IncidentMonitorIncidentRecord {
    IncidentMonitorIncidentRecord {
        actor: submission.actor.clone(),
        model: submission.model.clone(),
        tool_name: submission.tool_name.clone(),
        action: submission.action.clone(),
        policy: submission.policy.clone(),
        approval_state: submission.approval_state.clone(),
        risk_category: submission.risk_category.clone(),
        blast_radius: submission.blast_radius.clone(),
        external_correlation_ids: submission.external_correlation_ids.clone(),
        ..IncidentMonitorIncidentRecord::default()
    }
}

pub(crate) fn submission_defaults_from_extracted(
    context: &ExtractedSafetyContext,
) -> IncidentMonitorSubmission {
    IncidentMonitorSubmission {
        actor: context.actor.clone(),
        model: context.model.clone(),
        tool_name: context.tool_name.clone(),
        action: context.action.clone(),
        policy: context.policy.clone(),
        approval_state: context.approval_state.clone(),
        risk_category: context.risk_category.clone(),
        blast_radius: context.blast_radius.clone(),
        external_correlation_ids: context.external_correlation_ids.clone(),
        ..IncidentMonitorSubmission::default()
    }
}

pub(crate) fn redact_safety_context_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    static KEY_VALUE_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    static BEARER_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    let key_value = KEY_VALUE_SECRET_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(api[_-]?key|token|password|secret|credential|authorization)\s*[:=]\s*\S+",
        )
        .expect("valid incident monitor safety-context secret regex")
    });
    let bearer = BEARER_SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/\-]+=*")
            .expect("valid incident monitor bearer redaction regex")
    });

    let out = bearer.replace_all(trimmed, "Bearer [redacted]");
    key_value
        .replace_all(&out, |caps: &regex::Captures<'_>| {
            format!("{}=[redacted]", &caps[1])
        })
        .to_string()
}

/// Redact secrets from the free-text incident fields that are published
/// verbatim to destinations (GitHub/Linear/webhook/MCP) and embedded in
/// assessment reports. Safety-context metadata is scrubbed separately by
/// `normalize_submission_safety_context`; this covers the reporter/event-supplied
/// `title`, `detail`, `excerpt`, and `evidence_refs`. Gated by the caller on
/// `safety_defaults.redact_secrets`.
pub(crate) fn redact_incident_monitor_submission_secrets(
    submission: &mut IncidentMonitorSubmission,
) {
    if let Some(title) = submission.title.as_ref() {
        let redacted = redact_safety_context_text(title);
        submission.title = (!redacted.is_empty()).then_some(redacted);
    }
    if let Some(detail) = submission.detail.as_ref() {
        let redacted = redact_safety_context_text(detail);
        submission.detail = (!redacted.is_empty()).then_some(redacted);
    }
    for line in submission.excerpt.iter_mut() {
        *line = redact_safety_context_text(line);
    }
    for line in submission.evidence_refs.iter_mut() {
        *line = redact_safety_context_text(line);
    }
}

fn normalize_safety_context_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| redact_safety_context_text(&value))
        .filter(|value| !value.is_empty())
}

fn normalize_safety_context_vec(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = redact_safety_context_text(&value);
        if value.is_empty() || out.iter().any(|existing| existing == &value) {
            continue;
        }
        out.push(value);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn fill_missing_option(target: &mut Option<String>, source: &Option<String>) -> bool {
    if target.is_none() && source.is_some() {
        *target = source.clone();
        true
    } else {
        false
    }
}

fn merge_missing_values(existing: &mut Vec<String>, incoming: &[String]) -> bool {
    let mut changed = false;
    for value in incoming {
        if !existing.iter().any(|row| row == value) {
            existing.push(value.clone());
            changed = true;
        }
    }
    changed
}

pub(crate) fn extract_from_event_properties(properties: &Value) -> ExtractedSafetyContext {
    let mut external_correlation_ids = safety_context_strings_from_value(
        first_value(
            properties,
            &[
                "external_correlation_ids",
                "externalCorrelationIds",
                "external_ids",
                "externalIds",
                "external.correlation_ids",
                "external.correlationIds",
            ],
        ),
        20,
        160,
    );
    if let Some(external_correlation_id) = first_safety_context_string(
        properties,
        &[
            "external_correlation_id",
            "externalCorrelationId",
            "external_id",
            "externalId",
            "external.correlation_id",
        ],
        160,
    ) {
        if !external_correlation_ids
            .iter()
            .any(|row| row == &external_correlation_id)
        {
            external_correlation_ids.push(external_correlation_id);
        }
    }

    ExtractedSafetyContext {
        actor: first_safety_context_string(
            properties,
            &[
                "actor",
                "actor_id",
                "actorID",
                "principal",
                "principal_id",
                "agent",
                "agent_id",
                "agentID",
                "user_id",
                "userID",
            ],
            160,
        ),
        model: first_safety_context_string(
            properties,
            &["model", "model_id", "modelID", "model.name", "llm.model"],
            160,
        ),
        tool_name: first_safety_context_string(
            properties,
            &[
                "tool",
                "tool_name",
                "toolName",
                "mcp_tool",
                "mcpTool",
                "action.tool",
            ],
            160,
        ),
        action: first_safety_context_string(
            properties,
            &["action", "operation", "tool_action", "toolAction", "intent"],
            160,
        ),
        policy: first_safety_context_string(
            properties,
            &[
                "policy",
                "policy_id",
                "policyID",
                "policy.name",
                "approval_policy",
                "approvalPolicy",
                "approval.policy",
            ],
            160,
        ),
        approval_state: first_safety_context_string(
            properties,
            &[
                "approval_state",
                "approvalState",
                "approval_status",
                "approvalStatus",
                "approval.status",
            ],
            120,
        ),
        risk_category: first_safety_context_string(
            properties,
            &[
                "risk_category",
                "riskCategory",
                "risk_type",
                "riskType",
                "risk.category",
                "category",
            ],
            120,
        ),
        blast_radius: first_safety_context_string(
            properties,
            &[
                "blast_radius",
                "blastRadius",
                "blast.radius",
                "impact",
                "scope",
            ],
            240,
        ),
        external_correlation_ids,
    }
}

fn first_safety_context_string(
    properties: &Value,
    keys: &[&str],
    max_len: usize,
) -> Option<String> {
    first_string_deep(properties, keys)
        .map(|value| redact_safety_context_text(&value))
        .map(|value| truncate_text(&value, max_len))
        .filter(|value| !value.trim().is_empty())
}

fn first_value<'a>(properties: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| get_path_value(properties, key))
}

fn get_path_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if key.contains('.') {
        let mut current = value;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        Some(current)
    } else {
        value.get(key)
    }
}

fn first_string_deep(properties: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = get_path_value(properties, key) {
            if let Some(text) = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(text.to_string());
            }
            if value.is_number() || value.is_boolean() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn safety_context_strings_from_value(
    value: Option<&Value>,
    max_items: usize,
    max_len: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    for value in strings_from_value(value, max_items.saturating_mul(2)) {
        let value = redact_safety_context_text(&value);
        let value = truncate_text(&value, max_len);
        if value.trim().is_empty() || out.iter().any(|row| row == &value) {
            continue;
        }
        out.push(value);
        if out.len() >= max_items {
            break;
        }
    }
    out
}

fn strings_from_value(value: Option<&Value>, max_items: usize) -> Vec<String> {
    let mut rows = match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        if item.is_number() || item.is_boolean() {
                            Some(item.to_string())
                        } else {
                            None
                        }
                    })
            })
            .collect::<Vec<_>>(),
        Some(Value::String(text)) => text
            .lines()
            .flat_map(|line| line.split(','))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        Some(value) if value.is_number() || value.is_boolean() => vec![value.to_string()],
        _ => Vec::new(),
    };
    rows.truncate(max_items);
    rows
}

#[cfg(test)]
mod tests {
    use super::redact_safety_context_text;

    #[test]
    fn redacts_authorization_bearer_token_suffix() {
        let redacted = redact_safety_context_text("Authorization: Bearer SECRET_TOKEN_123");

        assert!(!redacted.contains("SECRET_TOKEN_123"));
        assert!(redacted.contains("[redacted]"));
    }
}
