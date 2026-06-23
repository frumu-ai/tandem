use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use tandem_observability::redact_text;

use crate::dataset::{
    ArtifactStatus, AutomationSpecTest, EvalDataset, EvalExpectedOutput, EvalTestCase,
    MetricTolerance, TestNode,
};
use tandem_bug_monitor::types::BugMonitorIncidentRecord;

const DEFAULT_DATASET_NAME: &str = "dogfooding_regression_fixture";
const DEFAULT_DATASET_VERSION: &str = "1.0";

#[derive(Debug, Clone, Default)]
pub struct BugMonitorRegressionFixtureOptions {
    pub fixture_id: Option<String>,
    pub dataset_name: Option<String>,
    pub extra_tags: Vec<String>,
}

pub fn incident_to_regression_dataset(
    incident: &BugMonitorIncidentRecord,
    options: BugMonitorRegressionFixtureOptions,
) -> EvalDataset {
    let mut dataset = EvalDataset::new(
        options
            .dataset_name
            .clone()
            .unwrap_or_else(|| DEFAULT_DATASET_NAME.to_string()),
        DEFAULT_DATASET_VERSION,
    );
    dataset.description =
        "Dogfooding regression fixture scaffolded from a Bug Monitor incident".to_string();
    dataset.tags = vec![
        "dogfooding".to_string(),
        "regression".to_string(),
        "bug_monitor".to_string(),
    ];
    dataset.created_at = chrono::Utc::now().to_rfc3339();
    dataset
        .test_cases
        .push(incident_to_eval_test_case(incident, &options));
    dataset
}

pub fn incident_to_eval_test_case(
    incident: &BugMonitorIncidentRecord,
    options: &BugMonitorRegressionFixtureOptions,
) -> EvalTestCase {
    let case_id = options.fixture_id.clone().unwrap_or_else(|| {
        format!(
            "dogfood_{}",
            slugify(first_non_empty(&[
                &incident.incident_id,
                &incident.fingerprint
            ]))
        )
    });
    let title = clean_summary(&incident.title, 140);
    let node_id = incident
        .event_payload
        .as_ref()
        .and_then(|payload| first_string(payload, &["node_id", "nodeID", "task_id", "taskID"]))
        .unwrap_or_else(|| "incident_replay".to_string());

    let mut config = HashMap::new();
    config.insert("source".to_string(), json!("bug_monitor_incident"));
    config.insert(
        "incident_id".to_string(),
        json!(incident.incident_id.as_str()),
    );
    config.insert(
        "fingerprint".to_string(),
        json!(incident.fingerprint.as_str()),
    );
    config.insert(
        "event_type".to_string(),
        json!(incident.event_type.as_str()),
    );
    config.insert(
        "incident_status".to_string(),
        json!(incident.status.as_str()),
    );
    config.insert("repo".to_string(), json!(incident.repo.as_str()));
    config.insert(
        "workspace_root".to_string(),
        json!(redact_text(&incident.workspace_root)),
    );
    config.insert("title".to_string(), json!(title));
    if let Some(detail) = incident.detail.as_deref() {
        config.insert("detail_redacted".to_string(), json!(redact_text(detail)));
    }
    if let Some(component) = incident.component.as_deref() {
        config.insert("component".to_string(), json!(component));
    }
    if let Some(source) = incident.source.as_deref() {
        config.insert("source_ref".to_string(), json!(source));
    }
    if let Some(run_id) = incident.run_id.as_deref() {
        config.insert("run_id".to_string(), json!(run_id));
    }
    if let Some(session_id) = incident.session_id.as_deref() {
        config.insert("session_id".to_string(), json!(session_id));
    }
    if !incident.evidence_refs.is_empty() {
        config.insert("evidence_refs".to_string(), json!(&incident.evidence_refs));
    }
    if !incident.excerpt.is_empty() {
        config.insert(
            "excerpt_redacted".to_string(),
            json!(incident
                .excerpt
                .iter()
                .take(5)
                .map(|line| redact_text(line))
                .collect::<Vec<_>>()),
        );
    }
    if let Some(payload) = incident.event_payload.as_ref() {
        config.insert("event_payload".to_string(), sanitize_fixture_value(payload));
    }

    let mut tags = vec![
        "dogfooding_regression".to_string(),
        "bug_monitor".to_string(),
        "regression".to_string(),
        slugify(&incident.event_type),
    ];
    tags.extend(options.extra_tags.iter().cloned());
    tags.sort();
    tags.dedup();

    EvalTestCase {
        id: case_id,
        description: format!("Dogfooding regression replay for Bug Monitor incident: {title}"),
        priority: 1,
        automation_spec: AutomationSpecTest {
            name: format!("dogfooding_regression_{}", slugify(&incident.event_type)),
            nodes: vec![TestNode {
                id: slugify(&node_id),
                node_type: "workflow_policy".to_string(),
                objective: format!(
                    "Replay incident {} and assert the failure signature remains visible.",
                    incident.incident_id
                ),
                output_contract:
                    "JSON with incident_id, failure_signature_preserved, blocker_category, and required_next_actions"
                        .to_string(),
            }],
            validators: vec![
                "dogfooding_fixture_schema".to_string(),
                "failure_signature".to_string(),
                "regression_assertions".to_string(),
            ],
            config,
        },
        expected_output: EvalExpectedOutput {
            artifact_status: expected_artifact_status(&incident.status),
            required_validators: vec![
                "dogfooding_fixture_schema".to_string(),
                "failure_signature".to_string(),
            ],
            optional_validators: vec!["regression_assertions".to_string()],
            unmet_requirements_acceptable: true,
            max_repair_iterations: Some(3),
            output_format: "json".to_string(),
            quality_indicators: vec![
                "incident_id_present".to_string(),
                "failure_signature_preserved".to_string(),
                "required_next_actions_present".to_string(),
            ],
        },
        enabled: true,
        tags,
        metric_tolerance: MetricTolerance {
            repair_iterations_delta: Some(1),
            token_usage_tolerance_percent: Some(0.20),
            cost_tolerance_usd: None,
            acceptable_failure_rate: None,
        },
    }
}

pub fn incident_json_to_regression_yaml(
    raw: &str,
    options: BugMonitorRegressionFixtureOptions,
) -> Result<String> {
    let incident: BugMonitorIncidentRecord =
        serde_json::from_str(raw).context("failed to parse Bug Monitor incident JSON")?;
    let dataset = incident_to_regression_dataset(&incident, options);
    serde_yaml::to_string(&dataset).context("failed to serialize dogfooding regression fixture")
}

pub fn write_incident_regression_fixture(
    incident_path: &Path,
    output_path: &Path,
    options: BugMonitorRegressionFixtureOptions,
) -> Result<()> {
    let raw = std::fs::read_to_string(incident_path).with_context(|| {
        format!(
            "failed to read Bug Monitor incident JSON from {}",
            incident_path.display()
        )
    })?;
    let yaml = incident_json_to_regression_yaml(&raw, options)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create regression fixture directory {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(output_path, yaml).with_context(|| {
        format!(
            "failed to write regression fixture to {}",
            output_path.display()
        )
    })?;
    Ok(())
}

fn expected_artifact_status(status: &str) -> ArtifactStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("blocked") || status.contains("timed_out") || status.contains("timeout") {
        ArtifactStatus::Blocked
    } else if status.contains("failed") || status.contains("error") {
        ArtifactStatus::Failed
    } else {
        ArtifactStatus::Completed
    }
}

fn sanitize_fixture_value(value: &Value) -> Value {
    sanitize_fixture_value_with_key(value, None)
}

fn sanitize_fixture_value_with_key(value: &Value, key: Option<&str>) -> Value {
    if key.is_some_and(should_redact_key) {
        return redact_value(value);
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        sanitize_fixture_value_with_key(value, Some(key)),
                    )
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(40)
                .map(|item| sanitize_fixture_value_with_key(item, key))
                .collect(),
        ),
        Value::String(text) => Value::String(clean_summary(text, 500)),
        _ => value.clone(),
    }
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text)),
        Value::Array(items) => Value::Array(items.iter().take(40).map(redact_value).collect()),
        Value::Object(_) => Value::String(redact_text(&value.to_string())),
        _ => Value::String("[redacted]".to_string()),
    }
}

fn should_redact_key(key: &str) -> bool {
    let normalized = normalize_redaction_key(key);
    let compact = normalized.replace('_', "");
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("authorization")
        || normalized.contains("api_key")
        || normalized.ends_with("_key")
        || compact == "key"
        || compact.ends_with("apikey")
        || compact.ends_with("accesskey")
        || compact.ends_with("clientkey")
        || compact.ends_with("privatekey")
        || compact.ends_with("secretkey")
        || normalized.contains("prompt")
        || normalized.contains("message")
        || normalized.contains("body")
        || normalized.contains("content")
        || normalized.contains("args")
        || normalized.contains("input")
        || normalized.contains("query")
}

fn normalize_redaction_key(key: &str) -> String {
    let chars = key.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut prev_was_alnum = false;
    let mut prev_was_lower_or_digit = false;
    let mut last_was_sep = false;

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_alphanumeric() {
            let next_is_lower = chars
                .get(idx + 1)
                .is_some_and(|next| next.is_ascii_lowercase());
            if ch.is_ascii_uppercase()
                && prev_was_alnum
                && (prev_was_lower_or_digit || next_is_lower)
                && !out.ends_with('_')
            {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_was_alnum = true;
            prev_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
            last_was_sep = false;
        } else {
            if !last_was_sep && !out.is_empty() {
                out.push('_');
                last_was_sep = true;
            }
            prev_was_alnum = false;
            prev_was_lower_or_digit = false;
        }
    }

    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(found) = value.get(*key) else {
            continue;
        };
        if let Some(text) = found
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }
    }
    None
}

fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .copied()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("incident")
}

fn clean_summary(value: &str, max_len: usize) -> String {
    let mut out = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if out.chars().count() > max_len {
        out = out.chars().take(max_len.saturating_sub(3)).collect();
        out.push_str("...");
    }
    out
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.trim().chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep && !out.is_empty() {
            out.push('_');
            last_was_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "incident".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_incident() -> BugMonitorIncidentRecord {
        let mut event_payload = json!({
            "node_id": "send_email",
            "message": "user prompt should not leak",
            "token": "sk-live-secret",
            "reason": "timed out"
        });
        if let Some(object) = event_payload.as_object_mut() {
            object.insert(
                format!("api{}", "Key"),
                json!("fixture-camel-redaction-value"),
            );
            object.insert(
                format!("private{}", "Key"),
                json!("fixture-private-redaction-value"),
            );
        }

        BugMonitorIncidentRecord {
            incident_id: "failure-incident-abc".to_string(),
            fingerprint: "fp-secret-case".to_string(),
            event_type: "automation.node.failed".to_string(),
            status: "triage_failed".to_string(),
            repo: "frumu-ai/tandem".to_string(),
            workspace_root: "C:/Users/example/work/tandem".to_string(),
            title: "Provider timeout in approval workflow".to_string(),
            detail: Some("prompt included token=sk-live-secret".to_string()),
            excerpt: vec!["Authorization: Bearer hidden-token".to_string()],
            source: Some("bug_monitor".to_string()),
            run_id: Some("automation-v2-run-123".to_string()),
            session_id: None,
            correlation_id: None,
            component: Some("automation_v2".to_string()),
            level: Some("error".to_string()),
            occurrence_count: 1,
            created_at_ms: 1,
            updated_at_ms: 2,
            last_seen_at_ms: Some(2),
            draft_id: Some("draft-1".to_string()),
            triage_run_id: Some("triage-1".to_string()),
            last_error: None,
            confidence: Some("high".to_string()),
            risk_level: Some("medium".to_string()),
            expected_destination: None,
            evidence_refs: vec!["tandem://bug-monitor/evidence/1".to_string()],
            quality_gate: None,
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: Some(event_payload),
            ..Default::default()
        }
    }

    #[test]
    fn incident_fixture_scaffold_redacts_prompt_and_secret_fields() {
        let yaml = serde_yaml::to_string(&incident_to_regression_dataset(
            &sample_incident(),
            BugMonitorRegressionFixtureOptions::default(),
        ))
        .expect("fixture yaml");

        assert!(!yaml.contains("sk-live-secret"));
        assert!(!yaml.contains("fixture-camel-redaction-value"));
        assert!(!yaml.contains("fixture-private-redaction-value"));
        assert!(!yaml.contains("user prompt should not leak"));
        assert!(yaml.contains("[redacted len="));
        assert!(yaml.contains("dogfooding_regression"));

        let parsed: EvalDataset = serde_yaml::from_str(&yaml).expect("parse fixture dataset");
        assert_eq!(parsed.test_cases.len(), 1);
        let case = &parsed.test_cases[0];
        assert_eq!(case.automation_spec.nodes[0].id, "send_email");
        assert!(case.tags.iter().any(|tag| tag == "dogfooding_regression"));
        assert_eq!(
            case.automation_spec.config.get("source"),
            Some(&json!("bug_monitor_incident"))
        );
    }

    #[test]
    fn redaction_key_matching_handles_camel_case_credentials() {
        for key in [
            "apiKey",
            "APIKey",
            "privateKey",
            "x-api-key",
            "clientAccessKey",
            "secret_key",
        ] {
            assert!(should_redact_key(key), "{key} should be redacted");
        }

        assert_eq!(normalize_redaction_key("privateKey"), "private_key");
        assert_eq!(normalize_redaction_key("APIKey"), "api_key");
    }
}
