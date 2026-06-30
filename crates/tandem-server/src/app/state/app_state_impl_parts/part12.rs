// Bug Monitor draft submission helpers split from part02.rs for the file-size
// gate (same module via include!).

fn normalize_bug_monitor_submission_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn normalize_bug_monitor_safety_context_optional(value: Option<String>) -> Option<String> {
    normalize_bug_monitor_submission_optional(value)
        .map(|value| crate::bug_monitor::safety_context::redact_safety_context_text(&value))
        .filter(|value| !value.is_empty())
}

fn normalize_bug_monitor_submission_vec(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim().to_string();
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

fn normalize_bug_monitor_safety_context_vec(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = crate::bug_monitor::safety_context::redact_safety_context_text(&value);
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

fn merge_bug_monitor_missing_submission_values(
    existing: &mut Vec<String>,
    incoming: &[String],
) -> bool {
    let mut changed = false;
    for value in incoming {
        if !existing.iter().any(|row| row == value) {
            existing.push(value.clone());
            changed = true;
        }
    }
    changed
}

fn bug_monitor_submission_fingerprint(parts: &[&str]) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn bug_monitor_submission_title(submission: &BugMonitorSubmission) -> String {
    submission.title.clone().unwrap_or_else(|| {
        if let Some(event) = submission.event.as_ref() {
            format!("Failure detected in {event}")
        } else if let Some(component) = submission.component.as_ref() {
            format!("Failure detected in {component}")
        } else if let Some(process) = submission.process.as_ref() {
            format!("Failure detected in {process}")
        } else if let Some(source) = submission.source.as_ref() {
            format!("Failure report from {source}")
        } else {
            "Failure report".to_string()
        }
    })
}

fn bug_monitor_submission_detail(submission: &BugMonitorSubmission) -> Option<String> {
    let mut detail_lines = Vec::new();
    if let Some(source) = submission.source.as_ref() {
        detail_lines.push(format!("source: {source}"));
    }
    if let Some(file_name) = submission.file_name.as_ref() {
        detail_lines.push(format!("file: {file_name}"));
    }
    if let Some(level) = submission.level.as_ref() {
        detail_lines.push(format!("level: {level}"));
    }
    if let Some(process) = submission.process.as_ref() {
        detail_lines.push(format!("process: {process}"));
    }
    if let Some(component) = submission.component.as_ref() {
        detail_lines.push(format!("component: {component}"));
    }
    if let Some(event) = submission.event.as_ref() {
        detail_lines.push(format!("event: {event}"));
    }
    if let Some(confidence) = submission.confidence.as_ref() {
        detail_lines.push(format!("confidence: {confidence}"));
    }
    if let Some(risk_level) = submission.risk_level.as_ref() {
        detail_lines.push(format!("risk_level: {risk_level}"));
    }
    if let Some(risk_category) = submission.risk_category.as_ref() {
        detail_lines.push(format!("risk_category: {risk_category}"));
    }
    if let Some(actor) = submission.actor.as_ref() {
        detail_lines.push(format!("actor: {actor}"));
    }
    if let Some(model) = submission.model.as_ref() {
        detail_lines.push(format!("model: {model}"));
    }
    if let Some(tool_name) = submission.tool_name.as_ref() {
        detail_lines.push(format!("tool_name: {tool_name}"));
    }
    if let Some(action) = submission.action.as_ref() {
        detail_lines.push(format!("action: {action}"));
    }
    if let Some(policy) = submission.policy.as_ref() {
        detail_lines.push(format!("policy: {policy}"));
    }
    if let Some(approval_state) = submission.approval_state.as_ref() {
        detail_lines.push(format!("approval_state: {approval_state}"));
    }
    if let Some(blast_radius) = submission.blast_radius.as_ref() {
        detail_lines.push(format!("blast_radius: {blast_radius}"));
    }
    if let Some(expected_destination) = submission.expected_destination.as_ref() {
        detail_lines.push(format!("expected_destination: {expected_destination}"));
    }
    if let Some(source_kind) = submission.source_kind.as_ref() {
        detail_lines.push(format!("source_kind: {}", source_kind.as_str()));
    }
    if let Some(tenant_id) = submission.tenant_id.as_ref() {
        detail_lines.push(format!("tenant_id: {tenant_id}"));
    }
    if let Some(workspace_id) = submission.workspace_id.as_ref() {
        detail_lines.push(format!("workspace_id: {workspace_id}"));
    }
    if !submission.route_tags.is_empty() {
        detail_lines.push(format!("route_tags: {}", submission.route_tags.join(", ")));
    }
    if let Some(run_id) = submission.run_id.as_ref() {
        detail_lines.push(format!("run_id: {run_id}"));
    }
    if let Some(session_id) = submission.session_id.as_ref() {
        detail_lines.push(format!("session_id: {session_id}"));
    }
    if let Some(correlation_id) = submission.correlation_id.as_ref() {
        detail_lines.push(format!("correlation_id: {correlation_id}"));
    }
    if !submission.external_correlation_ids.is_empty() {
        detail_lines.push(format!(
            "external_correlation_ids: {}",
            submission.external_correlation_ids.join(", ")
        ));
    }
    if let Some(detail) = submission.detail.as_ref() {
        detail_lines.push(String::new());
        detail_lines.push(detail.clone());
    }
    if !submission.excerpt.is_empty() {
        if !detail_lines.is_empty() {
            detail_lines.push(String::new());
        }
        detail_lines.push("excerpt:".to_string());
        detail_lines.extend(submission.excerpt.iter().map(|line| format!("  {line}")));
    }
    if !submission.evidence_refs.is_empty() {
        if !detail_lines.is_empty() {
            detail_lines.push(String::new());
        }
        detail_lines.push("evidence_refs:".to_string());
        detail_lines.extend(
            submission
                .evidence_refs
                .iter()
                .map(|line| format!("  {line}")),
        );
    }
    if detail_lines.is_empty() {
        None
    } else {
        Some(detail_lines.join("\n"))
    }
}
