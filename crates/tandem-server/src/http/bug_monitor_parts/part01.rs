use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path as FsPath, PathBuf};
use tandem_memory::{
    GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryPartition, MemoryPutRequest,
};
use uuid::Uuid;

use super::context_runs::{
    context_run_dir, context_run_tasks_create, ensure_context_run_dir, load_context_run_state,
    save_context_run_state,
};
use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardTaskStatus, ContextRunCreateInput, ContextRunState,
    ContextRunStatus, ContextTaskCreateBatchInput, ContextTaskCreateInput, ContextWorkspaceLease,
};

const DEFAULT_BUG_MONITOR_TEMPLATE: &str =
    include_str!("../../../resources/issue_templates/bug_report.md");

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorConfigInput {
    #[serde(default)]
    pub bug_monitor: Option<BugMonitorConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorDraftsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorIncidentsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorPostsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorBulkDeleteInput {
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorSubmissionInput {
    #[serde(default)]
    pub report: Option<BugMonitorSubmission>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorDecisionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorTriageSummaryInput {
    #[serde(default)]
    pub suggested_title: Option<String>,
    #[serde(default)]
    pub what_happened: Option<String>,
    #[serde(default)]
    pub why_it_likely_happened: Option<String>,
    #[serde(default)]
    pub root_cause_confidence: Option<String>,
    #[serde(default)]
    pub failure_type: Option<String>,
    #[serde(default)]
    pub affected_components: Vec<String>,
    #[serde(default)]
    pub likely_files_to_edit: Vec<String>,
    #[serde(default)]
    pub expected_behavior: Option<String>,
    #[serde(default)]
    pub steps_to_reproduce: Vec<String>,
    #[serde(default)]
    pub environment: Vec<String>,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub related_existing_issues: Vec<Value>,
    #[serde(default)]
    pub related_failure_patterns: Vec<Value>,
    #[serde(default)]
    pub research_sources: Vec<Value>,
    #[serde(default)]
    pub recommended_fix: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub verification_steps: Vec<String>,
    #[serde(default)]
    pub coder_ready: Option<bool>,
    #[serde(default)]
    pub risk_level: Option<String>,
    #[serde(default)]
    pub required_tool_scopes: Vec<String>,
    #[serde(default)]
    pub missing_tool_scopes: Vec<String>,
    #[serde(default)]
    pub permissions_available: Option<bool>,
    #[serde(default)]
    pub notes: Option<String>,
}

async fn write_bug_monitor_artifact(
    state: &AppState,
    linked_context_run_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    relative_path: &str,
    payload: &serde_json::Value,
) -> Result<(), StatusCode> {
    super::context_runs::append_json_artifact_to_context_run(
        state,
        linked_context_run_id,
        artifact_id,
        artifact_type,
        relative_path,
        payload,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

fn split_template_frontmatter(template: &str) -> (&str, &str) {
    let trimmed = template.trim();
    if !trimmed.starts_with("---\n") {
        return ("", trimmed);
    }
    let rest = &trimmed[4..];
    if let Some(index) = rest.find("\n---") {
        let end = index + 8;
        return (&trimmed[..end], trimmed[end..].trim_start());
    }
    ("", trimmed)
}

fn normalize_issue_draft_line(value: impl AsRef<str>) -> Option<String> {
    let line = value.as_ref().trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn bug_monitor_coder_ready_gate(
    requested_coder_ready: Option<bool>,
    root_cause_confidence: &str,
    likely_files_to_edit: &[String],
    affected_components: &[String],
    acceptance_criteria: &[String],
    verification_steps: &[String],
    risk_level: &str,
    duplicate_known: bool,
    required_tool_scopes: &[String],
    missing_tool_scopes: &[String],
    permissions_available: Option<bool>,
) -> (bool, Value) {
    let confidence_ok = matches!(root_cause_confidence, "high" | "medium");
    let scope_identified = !likely_files_to_edit.is_empty() || !affected_components.is_empty();
    let acceptance_clear = !acceptance_criteria.is_empty();
    let verification_clear = !verification_steps.is_empty();
    let risk_ok = matches!(risk_level, "low" | "medium");
    let duplicate_clear = !duplicate_known;
    let tool_scope_available =
        missing_tool_scopes.is_empty() && permissions_available != Some(false);
    let gates = vec![
        json!({
            "key": "confidence",
            "label": "Root cause confidence is high or medium",
            "passed": confidence_ok,
            "detail": root_cause_confidence,
        }),
        json!({
            "key": "scope_identified",
            "label": "Likely files or components are identified",
            "passed": scope_identified,
            "detail": {
                "likely_files_to_edit": likely_files_to_edit,
                "affected_components": affected_components,
            },
        }),
        json!({
            "key": "acceptance_criteria",
            "label": "Acceptance criteria are clear",
            "passed": acceptance_clear,
            "detail": acceptance_criteria,
        }),
        json!({
            "key": "verification_steps",
            "label": "Verification steps are clear",
            "passed": verification_clear,
            "detail": verification_steps,
        }),
        json!({
            "key": "risk_level",
            "label": "Risk is low or medium",
            "passed": risk_ok,
            "detail": risk_level,
        }),
        json!({
            "key": "duplicate_clear",
            "label": "Issue is not marked as duplicate",
            "passed": duplicate_clear,
        }),
        json!({
            "key": "tool_scope_available",
            "label": "Required permissions and tool scopes are available or not required",
            "passed": tool_scope_available,
            "detail": {
                "required_tool_scopes": required_tool_scopes,
                "missing_tool_scopes": missing_tool_scopes,
                "permissions_available": permissions_available,
            },
        }),
    ];
    let missing = gates
        .iter()
        .filter(|gate| !gate.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|gate| gate.get("key").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    let eligible = missing.is_empty();
    let requested = requested_coder_ready.unwrap_or(eligible);
    let coder_ready = requested && eligible;
    let passed_count = gates.len().saturating_sub(missing.len());
    (
        coder_ready,
        json!({
            "stage": "proposal_to_coder_ready",
            "status": if coder_ready { "passed" } else { "blocked" },
            "passed": coder_ready,
            "requested_coder_ready": requested_coder_ready,
            "passed_count": passed_count,
            "total_count": gates.len(),
            "missing": missing,
            "gates": gates,
            "blocked_reason": if coder_ready {
                Value::Null
            } else if requested_coder_ready == Some(false) {
                json!("triage summary explicitly marked coder_ready=false")
            } else {
                json!("coder-ready requirements were not satisfied")
            },
        }),
    )
}

fn bug_monitor_value_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn bug_monitor_summary_string_array(summary: Option<&Value>, key: &str) -> Vec<String> {
    bug_monitor_value_array(summary.and_then(|row| row.get(key)))
        .into_iter()
        .filter_map(|row| {
            row.as_str()
                .and_then(normalize_issue_draft_line)
                .or_else(|| {
                    (!row.is_null())
                        .then(|| row.to_string())
                        .and_then(normalize_issue_draft_line)
                })
        })
        .collect()
}

fn bug_monitor_summary_text(summary: Option<&Value>, key: &str) -> Option<String> {
    summary
        .and_then(|row| row.get(key))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
}

fn bug_monitor_proposal_quality_gate(
    state: &AppState,
    triage_run_id: &str,
    triage_summary: Option<&Value>,
) -> (bool, Value) {
    let has_summary = triage_summary.is_some();
    let inspection_artifact =
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_inspection").is_some();
    let research_artifact =
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_research").is_some();
    let validation_artifact =
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_validation").is_some();
    let fix_artifact =
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_fix_proposal").is_some();
    let durable_artifacts =
        inspection_artifact && research_artifact && validation_artifact && fix_artifact;

    let research_sources =
        bug_monitor_value_array(triage_summary.and_then(|row| row.get("research_sources")));
    let related_existing_issues =
        bug_monitor_value_array(triage_summary.and_then(|row| row.get("related_existing_issues")));
    let related_failure_patterns =
        bug_monitor_value_array(triage_summary.and_then(|row| row.get("related_failure_patterns")));
    let why_it_likely_happened =
        bug_monitor_summary_text(triage_summary, "why_it_likely_happened").unwrap_or_default();
    let research_performed = !research_sources.is_empty()
        || !related_existing_issues.is_empty()
        || !related_failure_patterns.is_empty()
        || (!why_it_likely_happened.is_empty()
            && !why_it_likely_happened
                .to_ascii_lowercase()
                .contains("pending"));

    let steps_to_reproduce = bug_monitor_summary_string_array(triage_summary, "steps_to_reproduce");
    let logs = bug_monitor_summary_string_array(triage_summary, "logs");
    let validation_confirmed = !steps_to_reproduce.is_empty() || !logs.is_empty();

    let root_cause_confidence =
        bug_monitor_summary_text(triage_summary, "root_cause_confidence").unwrap_or_default();
    let notes = bug_monitor_summary_text(triage_summary, "notes").unwrap_or_default();
    let uncertainty_explicit =
        matches!(root_cause_confidence.as_str(), "high" | "medium" | "low") || !notes.is_empty();

    let recommended_fix =
        bug_monitor_summary_text(triage_summary, "recommended_fix").unwrap_or_default();
    let acceptance_criteria =
        bug_monitor_summary_string_array(triage_summary, "acceptance_criteria");
    let bounded_action = !recommended_fix.is_empty()
        && !recommended_fix
            .to_ascii_lowercase()
            .contains("complete the bug monitor research")
        && !acceptance_criteria.is_empty();

    let verification_steps = bug_monitor_summary_string_array(triage_summary, "verification_steps");
    let verification_known = !verification_steps.is_empty();

    let gates = vec![
        json!({
            "key": "triage_summary",
            "label": "Triage summary artifact exists",
            "passed": has_summary,
        }),
        json!({
            "key": "durable_artifacts",
            "label": "Inspection, research, validation, and fix proposal artifacts exist",
            "passed": durable_artifacts,
            "detail": {
                "inspection": inspection_artifact,
                "research": research_artifact,
                "validation": validation_artifact,
                "fix_proposal": fix_artifact,
            },
        }),
        json!({
            "key": "research_performed",
            "label": "Research or related failure lookup has been performed",
            "passed": research_performed,
            "detail": {
                "research_sources": research_sources,
                "related_existing_issues": related_existing_issues,
                "related_failure_patterns": related_failure_patterns,
                "why_it_likely_happened": why_it_likely_happened,
            },
        }),
        json!({
            "key": "validation_scope",
            "label": "Validation has confirmed the failure scope",
            "passed": validation_confirmed,
            "detail": {
                "steps_to_reproduce": steps_to_reproduce,
                "logs": logs,
            },
        }),
        json!({
            "key": "uncertainty_explicit",
            "label": "Assumptions and uncertainty are explicit",
            "passed": uncertainty_explicit,
            "detail": {
                "root_cause_confidence": root_cause_confidence,
                "notes": notes,
            },
        }),
        json!({
            "key": "bounded_action",
            "label": "Proposed action is bounded and has acceptance criteria",
            "passed": bounded_action,
            "detail": {
                "recommended_fix": recommended_fix,
                "acceptance_criteria": acceptance_criteria,
            },
        }),
        json!({
            "key": "verification_steps",
            "label": "Verification steps are known",
            "passed": verification_known,
            "detail": verification_steps,
        }),
    ];
    let missing = gates
        .iter()
        .filter(|gate| !gate.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|gate| gate.get("key").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    let passed = missing.is_empty();
    (
        passed,
        json!({
            "stage": "draft_to_proposal",
            "status": if passed { "passed" } else { "blocked" },
            "passed": passed,
            "passed_count": gates.len().saturating_sub(missing.len()),
            "total_count": gates.len(),
            "missing": missing,
            "gates": gates,
            "blocked_reason": if passed {
                Value::Null
            } else {
                json!("draft-to-proposal requirements were not satisfied")
            },
        }),
    )
}

async fn persist_blocked_bug_monitor_report_observation(
    state: &AppState,
    report: &BugMonitorSubmission,
    repo: &str,
    detail: &str,
) -> Option<crate::BugMonitorIncidentRecord> {
    let repo = repo.trim();
    if repo.is_empty() {
        return None;
    }
    let config = state.bug_monitor_config().await;
    let workspace_root = match config.workspace_root.clone() {
        Some(root) => root,
        None => state.workspace_index.snapshot().await.root,
    };
    let mut submission = report.clone();
    submission.repo = Some(repo.to_string());
    if submission
        .source
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        submission.source = Some("manual".to_string());
    }
    if submission
        .event
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        submission.event = Some("manual.report".to_string());
    }
    if submission
        .confidence
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        submission.confidence = Some("medium".to_string());
    }
    if submission
        .risk_level
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        submission.risk_level = Some("medium".to_string());
    }
    if submission
        .expected_destination
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        submission.expected_destination = Some("bug_monitor_issue_draft".to_string());
    }
    submission.excerpt = submission
        .excerpt
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .take(50)
        .collect();
    submission.evidence_refs = submission
        .evidence_refs
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .take(50)
        .collect();
    let title = submission
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Blocked Bug Monitor signal".to_string());
    let fingerprint = submission.fingerprint.clone().unwrap_or_else(|| {
        crate::sha256_hex(&[
            repo,
            title.as_str(),
            submission.detail.as_deref().unwrap_or(""),
            submission.source.as_deref().unwrap_or(""),
            submission.run_id.as_deref().unwrap_or(""),
            submission.session_id.as_deref().unwrap_or(""),
            submission.correlation_id.as_deref().unwrap_or(""),
        ])
    });
    submission.fingerprint = Some(fingerprint.clone());
    let quality_gate =
        crate::bug_monitor::service::evaluate_bug_monitor_submission_quality(&submission);
    let now = crate::now_ms();
    let incident = crate::BugMonitorIncidentRecord {
        incident_id: format!("failure-incident-{}", Uuid::new_v4().simple()),
        fingerprint,
        event_type: submission
            .event
            .clone()
            .unwrap_or_else(|| "manual.report".to_string()),
        status: "quality_gate_blocked".to_string(),
        repo: repo.to_string(),
        workspace_root,
        title,
        detail: Some(format!(
            "Bug Monitor signal quality gate blocked draft creation.\n\nerror: {detail}"
        )),
        excerpt: submission.excerpt.clone(),
        source: submission.source.clone(),
        run_id: submission.run_id.clone(),
        session_id: submission.session_id.clone(),
        correlation_id: submission.correlation_id.clone(),
        component: submission.component.clone(),
        level: submission.level.clone(),
        occurrence_count: 1,
        created_at_ms: now,
        updated_at_ms: now,
        last_seen_at_ms: Some(now),
        draft_id: None,
        triage_run_id: None,
        last_error: Some(crate::truncate_text(detail, 500)),
        confidence: submission.confidence.clone(),
        risk_level: submission.risk_level.clone(),
        expected_destination: submission.expected_destination.clone(),
        evidence_refs: submission.evidence_refs.clone(),
        quality_gate: Some(quality_gate),
        duplicate_summary: None,
        duplicate_matches: None,
        event_payload: None,
    };
    state.put_bug_monitor_incident(incident).await.ok()
}

fn parse_existing_list(detail: Option<&str>, prefix: &str) -> Vec<String> {
    detail
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix(prefix)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .collect()
}

fn derive_expected_behavior(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> String {
    if let Some(event_type) = incident.map(|row| row.event_type.as_str()) {
        return format!("The affected flow should complete without triggering `{event_type}`.");
    }
    if let Some(title) = draft.title.as_deref().and_then(normalize_issue_draft_line) {
        return format!("The system should complete this flow without reproducing: {title}");
    }
    "The affected flow should complete without an error.".to_string()
}

fn derive_steps_to_reproduce(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> Vec<String> {
    let mut steps = parse_existing_list(draft.detail.as_deref(), "step:");
    if steps.is_empty() {
        if let Some(workspace_root) = incident
            .map(|row| row.workspace_root.trim())
            .filter(|value| !value.is_empty())
        {
            steps.push(format!("Open the workspace at `{workspace_root}`."));
        } else {
            steps.push("Open the affected Tandem workspace.".to_string());
        }
        if let Some(run_id) = incident
            .and_then(|row| row.run_id.as_deref())
            .and_then(normalize_issue_draft_line)
        {
            steps.push(format!(
                "Trigger the failing flow associated with run `{run_id}`."
            ));
        } else if let Some(event_type) = incident
            .map(|row| row.event_type.as_str())
            .and_then(normalize_issue_draft_line)
        {
            steps.push(format!("Trigger the flow that emits `{event_type}`."));
        } else {
            steps.push("Trigger the behavior described in the failure report.".to_string());
        }
        steps.push("Observe the error in the logs or Bug Monitor incident feed.".to_string());
    }
    steps.truncate(6);
    steps
}

fn derive_environment_lines(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("Repo: {}", draft.repo));
    if let Some(workspace_root) = incident
        .map(|row| row.workspace_root.trim())
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Workspace: {workspace_root}"));
    }
    if let Some(process) = draft
        .detail
        .as_deref()
        .and_then(|detail| {
            detail
                .lines()
                .find_map(|line| line.trim().strip_prefix("process:").map(str::trim))
        })
        .and_then(normalize_issue_draft_line)
    {
        lines.push(format!("Process: {process}"));
    } else {
        lines.push("Process: tandem-engine".to_string());
    }
    if let Some(component) = incident
        .and_then(|row| row.component.as_deref())
        .and_then(normalize_issue_draft_line)
    {
        lines.push(format!("Component: {component}"));
    }
    if let Some(run_id) = incident
        .and_then(|row| row.run_id.as_deref())
        .and_then(normalize_issue_draft_line)
    {
        lines.push(format!("Run ID: {run_id}"));
    }
    if let Some(session_id) = incident
        .and_then(|row| row.session_id.as_deref())
        .and_then(normalize_issue_draft_line)
    {
        lines.push(format!("Session ID: {session_id}"));
    }
    lines
}

fn derive_log_lines(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> Vec<String> {
    let mut lines = incident
        .map(|row| row.excerpt.clone())
        .unwrap_or_default()
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.extend(
            draft
                .detail
                .as_deref()
                .unwrap_or_default()
                .lines()
                .filter_map(normalize_issue_draft_line)
                .take(12),
        );
    }
    lines.truncate(12);
    lines
}

async fn load_bug_monitor_issue_template(config: &BugMonitorConfig) -> (String, Option<String>) {
    let mut candidates = Vec::<PathBuf>::new();
    if let Some(root) = config
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        candidates.push(FsPath::new(root).join(".github/ISSUE_TEMPLATE/bug_report.md"));
        candidates.push(FsPath::new(root).join(".github/ISSUE_TEMPLATE/bug-report.md"));
    }
    for candidate in candidates {
        if let Ok(raw) = tokio::fs::read_to_string(&candidate).await {
            if !raw.trim().is_empty() {
                return (raw, Some(candidate.to_string_lossy().to_string()));
            }
        }
    }
    (
        DEFAULT_BUG_MONITOR_TEMPLATE.to_string(),
        Some("builtin:.github/ISSUE_TEMPLATE/bug_report.md".to_string()),
    )
}

fn render_bug_monitor_template(
    template: &str,
    what_happened: &str,
    expected_behavior: &str,
    steps_to_reproduce: &[String],
    environment_lines: &[String],
    log_lines: &[String],
    extra_sections: &[(String, String)],
    hidden_markers: &[String],
) -> String {
    let (frontmatter, _) = split_template_frontmatter(template);
    let mut body = String::new();
    if !frontmatter.trim().is_empty() {
        body.push_str(frontmatter.trim());
        body.push_str("\n\n");
    }
    body.push_str("## What happened?\n\n");
    body.push_str(what_happened.trim());
    body.push_str("\n\n## What did you expect to happen?\n\n");
    body.push_str(expected_behavior.trim());
    body.push_str("\n\n## Steps to reproduce\n\n");
    for (index, step) in steps_to_reproduce.iter().enumerate() {
        body.push_str(&format!("{}. {}\n", index + 1, step.trim()));
    }
    body.push_str("\n## Environment\n\n");
    for line in environment_lines {
        body.push_str("- ");
        body.push_str(line.trim());
        body.push('\n');
    }
    body.push_str("\n## Logs / screenshots\n\n");
    if log_lines.is_empty() {
        body.push_str("Attach any relevant logs from `logs/` or screenshots.\n");
    } else {
        body.push_str("```text\n");
        for line in log_lines {
            body.push_str(line);
            body.push('\n');
        }
        body.push_str("```\n");
    }
    for (title, content) in extra_sections {
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        body.push_str("\n## ");
        body.push_str(title.trim());
        body.push_str("\n\n");
        body.push_str(content);
        body.push('\n');
    }
    if !hidden_markers.is_empty() {
        body.push('\n');
        for marker in hidden_markers {
            body.push_str(marker.trim());
            body.push('\n');
        }
    }
    body.trim().to_string()
}

fn derive_bug_monitor_failure_pattern_markers(
    summary: &str,
    notes: Option<&str>,
    logs: &[String],
) -> Vec<String> {
    let mut canonical_markers = summary
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| token.len() >= 5)
        .map(ToString::to_string)
        .take(5)
        .collect::<Vec<_>>();
    if let Some(note_text) = notes.map(str::trim).filter(|value| !value.is_empty()) {
        canonical_markers.push(note_text.to_string());
    }
    for line in logs.iter().map(String::as_str).take(3) {
        if let Some(value) = normalize_issue_draft_line(line) {
            canonical_markers.push(value);
        }
    }
    canonical_markers.sort();
    canonical_markers.dedup();
    canonical_markers.truncate(8);
    canonical_markers
}

async fn bug_monitor_failure_recurrence_count(
    state: &AppState,
    repo: &str,
    fingerprint: &str,
) -> u64 {
    state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.repo == repo && row.fingerprint == fingerprint)
        .map(|row| row.occurrence_count.max(1))
        .sum::<u64>()
        .max(1)
}

async fn bug_monitor_max_occurrence_count_for_draft(
    state: &AppState,
    draft_id: &str,
) -> Option<u64> {
    state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.draft_id.as_deref() == Some(draft_id))
        .map(|row| row.occurrence_count.max(1))
        .max()
}

fn bug_monitor_linked_issue_numbers(draft: &BugMonitorDraftRecord) -> Vec<u64> {
    let mut linked = draft
        .issue_number
        .into_iter()
        .chain(draft.matched_issue_number)
        .collect::<Vec<_>>();
    linked.sort_unstable();
    linked.dedup();
    linked
}

async fn persist_bug_monitor_failure_pattern_memory(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    triage_run_id: &str,
    triage_summary: &Value,
    summary_artifact_path: &str,
) -> Result<Value, StatusCode> {
    let summary_text = triage_summary
        .get("what_happened")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            draft
                .title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Bug Monitor detected a failure that needs triage.")
        .to_string();
    let notes = triage_summary.get("notes").and_then(Value::as_str);
    let logs = triage_summary
        .get("logs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let canonical_markers = derive_bug_monitor_failure_pattern_markers(&summary_text, notes, &logs);
    let affected_components = vec![draft
        .repo
        .rsplit('/')
        .next()
        .unwrap_or(draft.repo.as_str())
        .to_string()];
    let fingerprint = super::coder::failure_pattern_fingerprint(
        &draft.repo,
        &summary_text,
        &affected_components,
        &canonical_markers,
    );
    let query_text = std::iter::once(summary_text.as_str())
        .chain(logs.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let duplicate_matches = super::coder::find_failure_pattern_duplicates(
        state,
        &draft.repo,
        None,
        &["bug_monitor".to_string(), "default".to_string()],
        &query_text,
        Some(&fingerprint),
        3,
    )
    .await?;
    let recurrence_count = if let Some(count) =
        bug_monitor_max_occurrence_count_for_draft(state, &draft.draft_id).await
    {
        count
    } else {
        bug_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await
    };
    let linked_issue_numbers = bug_monitor_linked_issue_numbers(draft);
    if duplicate_matches.iter().any(|row| {
        row.get("source").and_then(Value::as_str) == Some("governed_memory")
            && row.get("match_reason").and_then(Value::as_str) == Some("exact_fingerprint")
    }) {
        let duplicate_summary = build_bug_monitor_duplicate_summary(&duplicate_matches);
        return Ok(json!({
            "stored": false,
            "reason": "governed_failure_pattern_exists",
            "fingerprint": fingerprint,
            "duplicate_summary": duplicate_summary,
            "duplicate_matches": duplicate_matches,
        }));
    }

    let partition = MemoryPartition {
        org_id: draft.repo.clone(),
        workspace_id: draft.repo.clone(),
        project_id: draft.repo.clone(),
        tier: GovernedMemoryTier::Session,
    };
    let capability = Some(super::skills_memory::issue_run_memory_capability(
        triage_run_id,
        Some("bug_monitor"),
        &partition,
        super::skills_memory::RunMemoryCapabilityPolicy::CoderWorkflow,
    ));
    let metadata = json!({
        "kind": "failure_pattern",
        "repo_slug": draft.repo,
        "failure_pattern_fingerprint": fingerprint,
        "linked_issue_numbers": linked_issue_numbers,
        "recurrence_count": recurrence_count,
        "affected_components": affected_components,
        "artifact_refs": [summary_artifact_path],
        "canonical_markers": canonical_markers,
        "symptoms": [summary_text],
        "draft_id": draft.draft_id,
        "triage_run_id": triage_run_id,
        "source": "bug_monitor",
    });
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        state,
        &tenant_context,
        MemoryPutRequest {
            run_id: triage_run_id.to_string(),
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content: summary_text.clone(),
            artifact_refs: vec![summary_artifact_path.to_string()],
            classification: MemoryClassification::Internal,
            metadata: Some(metadata.clone()),
        },
        capability,
    )
    .await?;
    Ok(json!({
        "stored": true,
        "memory_id": put_response.id,
        "fingerprint": fingerprint,
        "content": summary_text,
        "duplicate_summary": build_bug_monitor_duplicate_summary(&duplicate_matches),
        "metadata": metadata,
        "partition": {
            "org_id": partition.org_id,
            "workspace_id": partition.workspace_id,
            "project_id": partition.project_id,
            "tier": partition.tier,
        },
        "duplicate_matches": duplicate_matches,
    }))
}

async fn persist_bug_monitor_regression_signal_memory(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    triage_run_id: &str,
    triage_summary: &Value,
    summary_artifact_path: &str,
) -> Result<Value, StatusCode> {
    let what_happened = triage_summary
        .get("what_happened")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Bug Monitor detected a post-failure regression signal.");
    let expected_behavior = triage_summary
        .get("expected_behavior")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("The affected flow should complete without the observed failure.");
    let steps_to_reproduce = triage_summary
        .get("steps_to_reproduce")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let environment = triage_summary
        .get("environment")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let logs = triage_summary
        .get("logs")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let content = format!("{what_happened}\nExpected: {expected_behavior}");
    let recurrence_count = if let Some(count) =
        bug_monitor_max_occurrence_count_for_draft(state, &draft.draft_id).await
    {
        count
    } else {
        bug_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await
    };
    let linked_issue_numbers = bug_monitor_linked_issue_numbers(draft);
    let partition = MemoryPartition {
        org_id: draft.repo.clone(),
        workspace_id: draft.repo.clone(),
        project_id: draft.repo.clone(),
        tier: GovernedMemoryTier::Session,
    };
    let capability = Some(super::skills_memory::issue_run_memory_capability(
        triage_run_id,
        Some("bug_monitor"),
        &partition,
        super::skills_memory::RunMemoryCapabilityPolicy::CoderWorkflow,
    ));
    let metadata = json!({
        "kind": "regression_signal",
        "repo_slug": draft.repo,
        "linked_issue_numbers": linked_issue_numbers,
        "recurrence_count": recurrence_count,
        "draft_id": draft.draft_id,
        "triage_run_id": triage_run_id,
        "source": "bug_monitor",
        "what_happened": what_happened,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment,
        "logs": logs,
        "artifact_refs": [summary_artifact_path],
    });
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        state,
        &tenant_context,
        MemoryPutRequest {
            run_id: triage_run_id.to_string(),
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content,
            artifact_refs: vec![summary_artifact_path.to_string()],
            classification: MemoryClassification::Internal,
            metadata: Some(metadata.clone()),
        },
        capability,
    )
    .await?;
    Ok(json!({
        "stored": true,
        "memory_id": put_response.id,
        "content": format!("{what_happened}\nExpected: {expected_behavior}"),
        "metadata": metadata,
        "partition": {
            "org_id": partition.org_id,
            "workspace_id": partition.workspace_id,
            "project_id": partition.project_id,
            "tier": partition.tier,
        },
    }))
}

async fn persist_bug_monitor_failure_pattern_from_approved_draft(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
) -> Result<Value, StatusCode> {
    let summary_text = draft
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            draft
                .detail
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Bug Monitor approved a failure draft without triage details.")
        .to_string();
    let detail = draft.detail.as_deref().unwrap_or_default();
    let canonical_markers = detail
        .lines()
        .filter_map(normalize_issue_draft_line)
        .take(5)
        .collect::<Vec<_>>();
    let duplicate_matches = super::coder::find_failure_pattern_duplicates(
        state,
        &draft.repo,
        None,
        &["bug_monitor".to_string(), "default".to_string()],
        &format!("{summary_text} {detail}"),
        Some(&draft.fingerprint),
        3,
    )
    .await?;
    let recurrence_count =
        bug_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await;
    let linked_issue_numbers = bug_monitor_linked_issue_numbers(draft);
    if duplicate_matches.iter().any(|row| {
        row.get("source").and_then(Value::as_str) == Some("governed_memory")
            && row.get("match_reason").and_then(Value::as_str) == Some("exact_fingerprint")
    }) {
        let duplicate_summary = build_bug_monitor_duplicate_summary(&duplicate_matches);
        return Ok(json!({
            "stored": false,
            "reason": "governed_failure_pattern_exists",
            "fingerprint": draft.fingerprint,
            "duplicate_summary": duplicate_summary,
            "duplicate_matches": duplicate_matches,
        }));
    }

    let run_id = format!("bug-monitor-approval-{}", draft.draft_id);
    ensure_context_run_dir(state, &run_id).await?;
    let approval_artifact_path =
        context_run_dir(state, &run_id).join("artifacts/bug_monitor.approval_failure_pattern.json");
    if let Some(parent) = approval_artifact_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let approval_artifact_payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "fingerprint": draft.fingerprint,
        "summary": summary_text.clone(),
        "detail": detail,
        "canonical_markers": canonical_markers.clone(),
        "created_at_ms": crate::now_ms(),
        "source": "bug_monitor_approval",
    });
    let approval_artifact_raw = serde_json::to_string_pretty(&approval_artifact_payload)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&approval_artifact_path, approval_artifact_raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let approval_artifact_path = approval_artifact_path.to_string_lossy().to_string();
    let partition = MemoryPartition {
        org_id: draft.repo.clone(),
        workspace_id: draft.repo.clone(),
        project_id: draft.repo.clone(),
        tier: GovernedMemoryTier::Session,
    };
    let capability = Some(super::skills_memory::issue_run_memory_capability(
        &run_id,
        Some("bug_monitor"),
        &partition,
        super::skills_memory::RunMemoryCapabilityPolicy::CoderWorkflow,
    ));
    let metadata = json!({
        "kind": "failure_pattern",
        "repo_slug": draft.repo,
        "failure_pattern_fingerprint": draft.fingerprint,
        "linked_issue_numbers": linked_issue_numbers,
        "recurrence_count": recurrence_count,
        "affected_components": [draft
            .repo
            .rsplit('/')
            .next()
            .unwrap_or(draft.repo.as_str())],
        "artifact_refs": [approval_artifact_path],
        "canonical_markers": canonical_markers,
        "symptoms": [summary_text],
        "draft_id": draft.draft_id,
        "source": "bug_monitor_approval",
    });
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        state,
        &tenant_context,
        MemoryPutRequest {
            run_id,
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content: summary_text.clone(),
            artifact_refs: vec![approval_artifact_path],
            classification: MemoryClassification::Internal,
            metadata: Some(metadata.clone()),
        },
        capability,
    )
    .await?;
    Ok(json!({
        "stored": true,
        "memory_id": put_response.id,
        "fingerprint": draft.fingerprint,
        "content": summary_text,
        "duplicate_summary": build_bug_monitor_duplicate_summary(&duplicate_matches),
        "metadata": metadata,
        "partition": {
            "org_id": partition.org_id,
            "workspace_id": partition.workspace_id,
            "project_id": partition.project_id,
            "tier": partition.tier,
        },
        "duplicate_matches": duplicate_matches,
    }))
}

async fn latest_bug_monitor_incident_for_draft(
    state: &AppState,
    draft_id: &str,
) -> Option<crate::BugMonitorIncidentRecord> {
    state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.draft_id.as_deref() == Some(draft_id))
        .max_by_key(|row| row.updated_at_ms)
        .cloned()
}

fn latest_bug_monitor_artifact(
    state: &AppState,
    triage_run_id: &str,
    artifact_type: &str,
) -> Option<ContextBlackboardArtifact> {
    let blackboard = super::context_runs::load_context_blackboard(state, triage_run_id);
    blackboard
        .artifacts
        .iter()
        .filter(|row| row.artifact_type == artifact_type)
        .max_by_key(|row| row.ts_ms)
        .cloned()
}

async fn load_bug_monitor_artifact_payload(
    state: &AppState,
    triage_run_id: &str,
    artifact_type: &str,
) -> Option<(ContextBlackboardArtifact, Value)> {
    let artifact = latest_bug_monitor_artifact(state, triage_run_id, artifact_type)?;
    let raw = tokio::fs::read_to_string(&artifact.path).await.ok()?;
    let payload = serde_json::from_str::<Value>(&raw).ok()?;
    Some((artifact, payload))
}

pub(crate) async fn load_bug_monitor_triage_summary_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "bug_monitor_triage_summary")
        .await
        .map(|(_, payload)| payload)
}

pub(crate) async fn bug_monitor_failure_pattern_matches(
    state: &AppState,
    repo_slug: &str,
    fingerprint: &str,
    title: Option<&str>,
    detail: Option<&str>,
    excerpt: &[String],
    limit: usize,
) -> Vec<Value> {
    let mut rows = super::coder::query_failure_pattern_matches(
        state,
        repo_slug,
        fingerprint,
        title,
        detail,
        excerpt,
        limit,
    )
    .await
    .unwrap_or_default();
    for row in rows.iter_mut() {
        let source_missing = row.get("source").and_then(Value::as_str).is_none();
        let is_memory_candidate = row
            .get("candidate_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("memcand-"));
        if source_missing && is_memory_candidate {
            if let Some(object) = row.as_object_mut() {
                object.insert(
                    "source".to_string(),
                    Value::String("coder_candidate".to_string()),
                );
            }
        }
    }
    rows
}

pub(crate) fn build_bug_monitor_duplicate_summary(matches: &[Value]) -> Value {
    let normalized_matches = matches
        .iter()
        .map(|row| {
            let candidate_id = row.get("candidate_id").cloned().unwrap_or(Value::Null);
            let source = row.get("source").cloned().or_else(|| {
                candidate_id
                    .as_str()
                    .filter(|value| value.starts_with("memcand-"))
                    .map(|_| Value::String("coder_candidate".to_string()))
            });
            json!({
                "source": source.unwrap_or(Value::Null),
                "fingerprint": row.get("fingerprint").cloned().unwrap_or(Value::Null),
                "summary": row.get("summary").cloned().unwrap_or(Value::Null),
                "match_reason": row
                    .get("match_reason")
                    .cloned()
                    .or_else(|| {
                        row.get("match_reasons")
                            .and_then(Value::as_array)
                            .and_then(|reasons| reasons.first().cloned())
                    })
                    .unwrap_or(Value::Null),
                "score": row.get("score").cloned().unwrap_or(Value::Null),
                "recurrence_count": row.get("recurrence_count").cloned().unwrap_or_else(|| Value::from(1_u64)),
                "linked_issue_numbers": row.get("linked_issue_numbers").cloned().unwrap_or_else(|| json!([])),
                "run_id": row.get("run_id").cloned().unwrap_or(Value::Null),
                "memory_id": row.get("memory_id").cloned().unwrap_or(Value::Null),
                "artifact_refs": row.get("artifact_refs").cloned().unwrap_or_else(|| json!([])),
                "artifact_path": row.get("artifact_path").cloned().unwrap_or(Value::Null),
                "candidate_id": candidate_id,
                "linked_context_run_id": row
                    .get("linked_context_run_id")
                    .cloned()
                    .unwrap_or(Value::Null),
                "source_coder_run_id": row
                    .get("source_coder_run_id")
                    .cloned()
                    .unwrap_or_else(|| row.get("coder_run_id").cloned().unwrap_or(Value::Null)),
            })
        })
        .collect::<Vec<_>>();
    let best_match = normalized_matches.first().cloned().unwrap_or(Value::Null);
    let max_recurrence_count = normalized_matches
        .iter()
        .filter_map(|row| row.get("recurrence_count").and_then(Value::as_u64))
        .max()
        .unwrap_or(1);
    let mut linked_issue_numbers = normalized_matches
        .iter()
        .filter_map(|row| row.get("linked_issue_numbers").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_u64)
        .collect::<Vec<_>>();
    linked_issue_numbers.sort_unstable();
    linked_issue_numbers.dedup();
    json!({
        "reason": "duplicate_failure_pattern",
        "match_count": normalized_matches.len(),
        "max_recurrence_count": max_recurrence_count,
        "linked_issue_numbers": linked_issue_numbers,
        "best_match": best_match,
        "matches": normalized_matches,
    })
}

pub(crate) async fn load_bug_monitor_issue_draft_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "bug_monitor_issue_draft")
        .await
        .map(|(_, payload)| payload)
}

/// True if the triage run has reached a terminal status (`Failed` /
/// `Completed` / `Cancelled`). Returns `false` if the run record can
/// not be loaded — a missing/corrupt run is treated as non-terminal so
/// the deadline task proceeds to mark the draft `triage_timed_out` and
/// `publish_draft` falls through to the basic issue body. Returning
/// `true` here would short-circuit the deadline task before it sets
/// `triage_timed_out_at_ms`, leaving the draft stuck in
/// `triage_pending` indefinitely.
pub(crate) async fn bug_monitor_triage_run_is_terminal(state: &AppState, run_id: &str) -> bool {
    match load_context_run_state(state, run_id).await {
        Ok(run) => super::context_runs::context_run_is_terminal(&run.status),
        Err(_) => false,
    }
}

pub(crate) async fn load_bug_monitor_proposal_quality_gate_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "bug_monitor_proposal_quality_gate")
        .await
        .map(|(_, payload)| payload)
}

fn bug_monitor_triage_artifacts(
    state: &AppState,
    triage_run_id: Option<&str>,
) -> (
    Option<ContextBlackboardArtifact>,
    Option<ContextBlackboardArtifact>,
    Option<ContextBlackboardArtifact>,
) {
    let triage_summary_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_triage_summary")
    });
    let issue_draft_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_issue_draft")
    });
    let duplicate_matches_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "failure_duplicate_matches")
    });
    (
        triage_summary_artifact,
        issue_draft_artifact,
        duplicate_matches_artifact,
    )
}

async fn bug_monitor_duplicate_match_context(
    state: &AppState,
    triage_run_id: Option<&str>,
) -> (Option<Value>, Option<Value>) {
    let Some(triage_run_id) = triage_run_id else {
        return (None, None);
    };
    let duplicate_matches =
        load_bug_monitor_artifact_payload(state, triage_run_id, "failure_duplicate_matches")
            .await
            .and_then(|(_, payload)| {
                payload
                    .get("matches")
                    .and_then(Value::as_array)
                    .cloned()
                    .map(Value::Array)
            });
    let duplicate_rows = duplicate_matches
        .as_ref()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_summary = Some(build_bug_monitor_duplicate_summary(&duplicate_rows));
    (duplicate_summary, Some(Value::Array(duplicate_rows)))
}

async fn refresh_bug_monitor_duplicate_matches_artifact(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    triage_run_id: &str,
) -> Option<Vec<Value>> {
    if latest_bug_monitor_artifact(state, triage_run_id, "failure_duplicate_matches").is_some() {
        return None;
    }
    let duplicate_matches = bug_monitor_failure_pattern_matches(
        state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
    )
    .await;
    if duplicate_matches.is_empty() {
        return None;
    }
    write_bug_monitor_artifact(
        state,
        triage_run_id,
        "failure-duplicate-matches",
        "failure_duplicate_matches",
        "artifacts/failure_duplicate_matches.json",
        &json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "fingerprint": draft.fingerprint,
            "matches": duplicate_matches,
            "created_at_ms": crate::now_ms(),
        }),
    )
    .await
    .ok()?;
    Some(duplicate_matches)
}

pub(crate) async fn ensure_bug_monitor_issue_draft(
    state: AppState,
    draft_id: &str,
    force: bool,
) -> anyhow::Result<Value> {
    let config = state.bug_monitor_config().await;
    let mut draft = state
        .get_bug_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;
    let triage_run_id = draft.triage_run_id.clone().ok_or_else(|| {
        anyhow::anyhow!("Bug Monitor draft needs a triage run before issue drafting")
    })?;
    let triage_summary = load_bug_monitor_triage_summary_artifact(&state, &triage_run_id).await;
    let (proposal_gate_passed, proposal_quality_gate) =
        bug_monitor_proposal_quality_gate(&state, &triage_run_id, triage_summary.as_ref());
    if !proposal_gate_passed {
        let _ = write_bug_monitor_artifact(
            &state,
            &triage_run_id,
            "bug-monitor-proposal-quality-gate",
            "bug_monitor_proposal_quality_gate",
            "artifacts/bug_monitor.proposal_quality_gate.json",
            &proposal_quality_gate,
        )
        .await;
        draft.github_status = Some("proposal_blocked".to_string());
        draft.last_post_error = Some(
            "Bug Monitor draft-to-proposal quality gate blocked issue draft generation".to_string(),
        );
        let _ = state.put_bug_monitor_draft(draft).await;
        anyhow::bail!(
            "Bug Monitor draft-to-proposal quality gate blocked issue draft generation: {}",
            proposal_quality_gate
        );
    }
    if !force {
        let existing_issue_draft =
            load_bug_monitor_artifact_payload(&state, &triage_run_id, "bug_monitor_issue_draft")
                .await;
        let triage_summary =
            load_bug_monitor_artifact_payload(&state, &triage_run_id, "bug_monitor_triage_summary")
                .await;
        if let Some((issue_artifact, issue_payload)) = existing_issue_draft {
            let triage_newer = triage_summary
                .as_ref()
                .map(|(summary_artifact, _)| summary_artifact.ts_ms > issue_artifact.ts_ms)
                .unwrap_or(false);
            if !triage_newer {
                return Ok(issue_payload);
            }
        }
    }

    let incident = latest_bug_monitor_incident_for_draft(&state, draft_id).await;
    let (template, template_source) = load_bug_monitor_issue_template(&config).await;
    let what_happened = triage_summary
        .as_ref()
        .and_then(|row| row.get("what_happened"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .or_else(|| {
            draft
                .detail
                .as_deref()
                .and_then(normalize_issue_draft_line)
                .or_else(|| {
                    incident
                        .as_ref()
                        .and_then(|row| normalize_issue_draft_line(&row.title))
                })
                .or_else(|| draft.title.as_deref().and_then(normalize_issue_draft_line))
        })
        .unwrap_or_else(|| "Bug Monitor detected a failure that needs triage.".to_string());
    let expected_behavior = triage_summary
        .as_ref()
        .and_then(|row| row.get("expected_behavior"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_else(|| derive_expected_behavior(&draft, incident.as_ref()));
    let steps_to_reproduce = triage_summary
        .as_ref()
        .and_then(|row| row.get("steps_to_reproduce"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_steps_to_reproduce(&draft, incident.as_ref()));
    let environment_lines = triage_summary
        .as_ref()
        .and_then(|row| row.get("environment"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_environment_lines(&draft, incident.as_ref()));
    let log_lines = triage_summary
        .as_ref()
        .and_then(|row| row.get("logs"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_log_lines(&draft, incident.as_ref()));
    let string_array = |key: &str| -> Vec<String> {
        triage_summary
            .as_ref()
            .and_then(|row| row.get(key))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .filter_map(normalize_issue_draft_line)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let likely_files_to_edit = string_array("likely_files_to_edit");
    let affected_components = string_array("affected_components");
    let acceptance_criteria = string_array("acceptance_criteria");
    let verification_steps = string_array("verification_steps");
    let research_sources = triage_summary
        .as_ref()
        .and_then(|row| row.get("research_sources"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let related_existing_issues = triage_summary
        .as_ref()
        .and_then(|row| row.get("related_existing_issues"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_failure_patterns =
        load_bug_monitor_artifact_payload(&state, &triage_run_id, "failure_duplicate_matches")
            .await
            .and_then(|(_, payload)| payload.get("matches").and_then(Value::as_array).cloned())
            .unwrap_or_default();
    let related_failure_patterns = triage_summary
        .as_ref()
        .and_then(|row| row.get("related_failure_patterns"))
        .and_then(Value::as_array)
        .cloned()
        .filter(|rows| !rows.is_empty())
        .or_else(|| {
            if duplicate_failure_patterns.is_empty() {
                None
            } else {
                Some(duplicate_failure_patterns)
            }
        })
        .unwrap_or_default();
    let why_it_likely_happened = triage_summary
        .as_ref()
        .and_then(|row| row.get("why_it_likely_happened"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_default();
    let recommended_fix = triage_summary
        .as_ref()
        .and_then(|row| row.get("recommended_fix"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_default();
    let failure_type = triage_summary
        .as_ref()
        .and_then(|row| row.get("failure_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let root_cause_confidence = triage_summary
        .as_ref()
        .and_then(|row| row.get("root_cause_confidence"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("low")
        .to_string();
    let risk_level = triage_summary
        .as_ref()
        .and_then(|row| row.get("risk_level"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("medium")
        .to_string();
    let required_tool_scopes = string_array("required_tool_scopes");
    let missing_tool_scopes = string_array("missing_tool_scopes");
    let permissions_available = triage_summary
        .as_ref()
        .and_then(|row| row.get("permissions_available"))
        .and_then(Value::as_bool);
    let requested_coder_ready = triage_summary
        .as_ref()
        .and_then(|row| row.get("coder_ready"))
        .and_then(Value::as_bool);
    let duplicate_known = triage_summary
        .as_ref()
        .and_then(|row| row.get("duplicate"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (coder_ready, coder_ready_gate) = bug_monitor_coder_ready_gate(
        requested_coder_ready,
        &root_cause_confidence,
        &likely_files_to_edit,
        &affected_components,
        &acceptance_criteria,
        &verification_steps,
        &risk_level,
        duplicate_known,
        &required_tool_scopes,
        &missing_tool_scopes,
        permissions_available,
    );
    let list_section = |items: &[String]| -> String {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let json_list_section = |items: &[Value]| -> String {
        items
            .iter()
            .map(|item| {
                let text = item
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| item.to_string());
                format!("- {text}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let extra_sections = vec![
        (
            "Suspected root cause".to_string(),
            why_it_likely_happened.clone(),
        ),
        ("Recommended fix".to_string(), recommended_fix.clone()),
        (
            "Files likely involved".to_string(),
            list_section(&likely_files_to_edit),
        ),
        (
            "Affected components".to_string(),
            list_section(&affected_components),
        ),
        (
            "Acceptance criteria".to_string(),
            list_section(&acceptance_criteria),
        ),
        (
            "Verification steps".to_string(),
            list_section(&verification_steps),
        ),
        (
            "Related issues and failure patterns".to_string(),
            [
                json_list_section(&related_existing_issues),
                json_list_section(&related_failure_patterns),
            ]
            .into_iter()
            .filter(|row| !row.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        ),
        (
            "Research sources".to_string(),
            json_list_section(&research_sources),
        ),
    ];
    let handoff = json!({
        "handoff_type": "tandem_autonomous_coder_issue",
        "source": "bug_monitor",
        "repo": draft.repo.clone(),
        "triage_run_id": triage_run_id.clone(),
        "workflow_run_id": incident.as_ref().and_then(|row| row.run_id.clone()),
        "incident_id": incident.as_ref().map(|row| row.incident_id.clone()),
        "draft_id": draft.draft_id.clone(),
        "failure_type": failure_type.clone(),
        "likely_files_to_edit": likely_files_to_edit.clone(),
        "acceptance_criteria": acceptance_criteria.clone(),
        "verification_steps": verification_steps.clone(),
        "risk_level": risk_level.clone(),
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate.clone(),
        "required_tool_scopes": required_tool_scopes.clone(),
        "missing_tool_scopes": missing_tool_scopes.clone(),
        "permissions_available": permissions_available,
    });
    let mut hidden_markers = vec![
        format!("<!-- tandem:fingerprint:v1:{} -->", draft.fingerprint),
        format!("<!-- tandem:triage_run_id:v1:{} -->", triage_run_id),
    ];
    if coder_ready {
        hidden_markers.push(format!(
            "<!-- tandem:coder_handoff:v1\n{}\n-->",
            serde_json::to_string_pretty(&handoff).unwrap_or_else(|_| "{}".to_string())
        ));
    }
    let rendered_body = render_bug_monitor_template(
        &template,
        &what_happened,
        &expected_behavior,
        &steps_to_reproduce,
        &environment_lines,
        &log_lines,
        &extra_sections,
        &hidden_markers,
    );
    let payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "template_source": template_source,
        "suggested_title": triage_summary
            .as_ref()
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str)
            .and_then(normalize_issue_draft_line)
            .or_else(|| draft.title.clone())
            .unwrap_or_else(|| "Bug Monitor issue".to_string()),
        "what_happened": what_happened,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment_lines,
        "logs": log_lines,
        "why_it_likely_happened": why_it_likely_happened,
        "root_cause_confidence": root_cause_confidence,
        "failure_type": failure_type,
        "affected_components": affected_components,
        "likely_files_to_edit": likely_files_to_edit,
        "related_existing_issues": related_existing_issues,
        "related_failure_patterns": related_failure_patterns,
        "research_sources": research_sources,
        "recommended_fix": recommended_fix,
        "acceptance_criteria": acceptance_criteria,
        "verification_steps": verification_steps,
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate,
        "proposal_quality_gate": proposal_quality_gate,
        "risk_level": risk_level,
        "required_tool_scopes": required_tool_scopes,
        "missing_tool_scopes": missing_tool_scopes,
        "permissions_available": permissions_available,
        "coder_handoff": handoff,
        "triage_summary": triage_summary,
        "rendered_body": rendered_body,
        "created_at_ms": crate::now_ms(),
    });
    let artifact_id = format!("bug-monitor-issue-draft-{}", Uuid::new_v4().simple());
    write_bug_monitor_artifact(
        &state,
        &triage_run_id,
        &artifact_id,
        "bug_monitor_issue_draft",
        "artifacts/bug_monitor.issue_draft.json",
        &payload,
    )
    .await
    .map_err(|status| anyhow::anyhow!("Failed to write issue draft artifact: HTTP {status}"))?;

    draft.github_status = Some("issue_draft_ready".to_string());
    if draft.status.eq_ignore_ascii_case("triage_queued") {
        draft.status = "draft_ready".to_string();
    }
    let _ = state.put_bug_monitor_draft(draft).await?;
    Ok(payload)
}

pub(super) async fn create_bug_monitor_triage_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorTriageSummaryInput>,
) -> Response {
    let mut draft = match state.get_bug_monitor_draft(&id).await {
        Some(draft) => draft,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Bug Monitor draft not found",
                    "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                    "draft_id": id,
                })),
            )
                .into_response();
        }
    };
    let Some(triage_run_id) = draft.triage_run_id.clone() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug Monitor draft needs a triage run before a triage summary can be written",
                "code": "BUG_MONITOR_TRIAGE_SUMMARY_REQUIRES_RUN",
                "draft_id": id,
            })),
        )
            .into_response();
    };
    let what_happened = input
        .what_happened
        .as_deref()
        .and_then(normalize_issue_draft_line)
        .or_else(|| draft.title.as_deref().and_then(normalize_issue_draft_line))
        .unwrap_or_else(|| "Bug Monitor detected a failure that needs triage.".to_string());
    let expected_behavior = input
        .expected_behavior
        .as_deref()
        .and_then(normalize_issue_draft_line)
        .unwrap_or_else(|| "The failing flow should complete without an error.".to_string());
    let steps_to_reproduce = input
        .steps_to_reproduce
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(8)
        .collect::<Vec<_>>();
    let environment = input
        .environment
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(12)
        .collect::<Vec<_>>();
    let logs = input
        .logs
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let affected_components = input
        .affected_components
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let likely_files_to_edit = input
        .likely_files_to_edit
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(30)
        .collect::<Vec<_>>();
    let acceptance_criteria = input
        .acceptance_criteria
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let verification_steps = input
        .verification_steps
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let required_tool_scopes = input
        .required_tool_scopes
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let missing_tool_scopes = input
        .missing_tool_scopes
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let confidence = input
        .root_cause_confidence
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "high" | "medium" | "low"))
        .unwrap_or_else(|| "low".to_string());
    let failure_type = input
        .failure_type
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| {
            matches!(
                value.as_str(),
                "code_defect"
                    | "missing_config"
                    | "missing_capability"
                    | "model_error"
                    | "tool_error"
                    | "validation_error"
                    | "timeout"
                    | "external_dependency"
                    | "unknown"
            )
        })
        .unwrap_or_else(|| "unknown".to_string());
    let risk_level = input
        .risk_level
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "low" | "medium" | "high"))
        .unwrap_or_else(|| "medium".to_string());
    let (coder_ready, coder_ready_gate) = bug_monitor_coder_ready_gate(
        input.coder_ready,
        &confidence,
        &likely_files_to_edit,
        &affected_components,
        &acceptance_criteria,
        &verification_steps,
        &risk_level,
        false,
        &required_tool_scopes,
        &missing_tool_scopes,
        input.permissions_available,
    );
    let payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "suggested_title": input.suggested_title.as_deref().and_then(normalize_issue_draft_line),
        "what_happened": what_happened,
        "why_it_likely_happened": input.why_it_likely_happened.as_deref().and_then(normalize_issue_draft_line),
        "root_cause_confidence": confidence,
        "failure_type": failure_type,
        "affected_components": affected_components,
        "likely_files_to_edit": likely_files_to_edit,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment,
        "logs": logs,
        "related_existing_issues": input.related_existing_issues,
        "related_failure_patterns": input.related_failure_patterns,
        "research_sources": input.research_sources,
        "recommended_fix": input.recommended_fix.as_deref().and_then(normalize_issue_draft_line),
        "acceptance_criteria": acceptance_criteria,
        "verification_steps": verification_steps,
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate,
        "risk_level": risk_level,
        "required_tool_scopes": required_tool_scopes,
        "missing_tool_scopes": missing_tool_scopes,
        "permissions_available": input.permissions_available,
        "notes": input.notes.as_deref().and_then(normalize_issue_draft_line),
        "created_at_ms": crate::now_ms(),
    });
    let artifact_id = format!("bug-monitor-triage-summary-{}", Uuid::new_v4().simple());
    match write_bug_monitor_artifact(
        &state,
        &triage_run_id,
        &artifact_id,
        "bug_monitor_triage_summary",
        "artifacts/bug_monitor.triage_summary.json",
        &payload,
    )
    .await
    {
        Ok(()) => {}
        Err(status) => {
            return (
                status,
                Json(json!({
                    "error": "Failed to write Bug Monitor triage summary",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_WRITE_FAILED",
                    "draft_id": id,
                })),
            )
                .into_response();
        }
    }

    let summary_artifact_path = context_run_dir(&state, &triage_run_id)
        .join("artifacts/bug_monitor.triage_summary.json")
        .to_string_lossy()
        .to_string();
    let failure_pattern_memory = match persist_bug_monitor_failure_pattern_memory(
        &state,
        &draft,
        &triage_run_id,
        &payload,
        &summary_artifact_path,
    )
    .await
    {
        Ok(memory) => {
            if memory
                .get("stored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let memory_artifact_id = format!(
                    "bug-monitor-failure-pattern-memory-{}",
                    Uuid::new_v4().simple()
                );
                let _ = write_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    &memory_artifact_id,
                    "bug_monitor_failure_pattern_memory",
                    "artifacts/bug_monitor.failure_pattern_memory.json",
                    &memory,
                )
                .await;
            }
            Some(memory)
        }
        Err(_) => None,
    };
    let regression_signal_memory = match persist_bug_monitor_regression_signal_memory(
        &state,
        &draft,
        &triage_run_id,
        &payload,
        &summary_artifact_path,
    )
    .await
    {
        Ok(memory) => {
            if memory
                .get("stored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let memory_artifact_id = format!(
                    "bug-monitor-regression-signal-memory-{}",
                    Uuid::new_v4().simple()
                );
                let _ = write_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    &memory_artifact_id,
                    "bug_monitor_regression_signal_memory",
                    "artifacts/bug_monitor.regression_signal_memory.json",
                    &memory,
                )
                .await;
            }
            Some(memory)
        }
        Err(_) => None,
    };

    draft.github_status = Some("triage_summary_ready".to_string());
    if draft.status.eq_ignore_ascii_case("triage_queued") {
        draft.status = "draft_ready".to_string();
    }
    let draft = match state.put_bug_monitor_draft(draft).await {
        Ok(draft) => draft,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to update Bug Monitor draft after triage summary",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_DRAFT_UPDATE_FAILED",
                    "draft_id": id,
                    "detail": error.to_string(),
                })),
            )
                .into_response();
        }
    };
    let (triage_summary_artifact, _issue_draft_artifact, duplicate_matches_artifact) =
        bug_monitor_triage_artifacts(&state, Some(&triage_run_id));
    match ensure_bug_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => {
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(&triage_run_id));
            Json(json!({
                "ok": true,
                "draft": draft,
                "triage_summary": payload,
                "triage_summary_artifact": triage_summary_artifact,
                "failure_pattern_memory": failure_pattern_memory,
                "regression_signal_memory": regression_signal_memory,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            {
                let proposal_quality_gate =
                    load_bug_monitor_proposal_quality_gate_artifact(&state, &triage_run_id).await;
                let proposal_quality_gate_artifact = latest_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    "bug_monitor_proposal_quality_gate",
                );
                Json(json!({
                    "error": "Bug Monitor triage summary was written, but issue draft regeneration failed",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_ISSUE_DRAFT_FAILED",
                    "draft": draft,
                    "triage_summary": payload,
                    "triage_summary_artifact": triage_summary_artifact,
                    "failure_pattern_memory": failure_pattern_memory,
                    "regression_signal_memory": regression_signal_memory,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "proposal_quality_gate": proposal_quality_gate,
                    "proposal_quality_gate_artifact": proposal_quality_gate_artifact,
                    "detail": error.to_string(),
                }))
            },
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.bug_monitor_config().await;
    Json(json!({
        "bug_monitor": config
    }))
}

pub(super) async fn patch_bug_monitor_config(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorConfigInput>,
) -> Response {
    let Some(config) = input.bug_monitor else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "bug_monitor object is required",
                "code": "BUG_MONITOR_CONFIG_REQUIRED",
            })),
        )
            .into_response();
    };
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid bug monitor config",
                "code": "BUG_MONITOR_CONFIG_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn recompute_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn get_bug_monitor_debug(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    let selected_server_tools = if let Some(server_name) = status.config.mcp_server.as_deref() {
        state.mcp.server_tools(server_name).await
    } else {
        Vec::new()
    };
    let canonicalized_discovered_tools = selected_server_tools
        .iter()
        .map(|tool| {
            json!({
                "server_name": tool.server_name,
                "tool_name": tool.tool_name,
                "namespaced_name": tool.namespaced_name,
                "canonical_name": canonicalize_tool_name(&tool.namespaced_name),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "status": status,
        "selected_server_tools": selected_server_tools,
        "canonicalized_discovered_tools": canonicalized_discovered_tools,
    }))
}

pub(super) async fn list_bug_monitor_incidents(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorIncidentsQuery>,
) -> Json<serde_json::Value> {
    let incidents = state
        .list_bug_monitor_incidents(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "incidents": incidents,
        "count": incidents.len(),
    }))
}

pub(super) async fn get_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.get_bug_monitor_incident(&id).await {
        Some(incident) => Json(json!({ "incident": incident })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response(),
    }
}

pub(super) async fn list_bug_monitor_drafts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorDraftsQuery>,
) -> Json<serde_json::Value> {
    let drafts = state
        .list_bug_monitor_drafts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "drafts": drafts,
        "count": drafts.len(),
    }))
}

pub(super) async fn list_bug_monitor_posts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorPostsQuery>,
) -> Json<serde_json::Value> {
    let posts = state
        .list_bug_monitor_posts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "posts": posts,
        "count": posts.len(),
    }))
}

pub(super) async fn delete_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_incidents(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor incident",
                "code": "BUG_MONITOR_INCIDENT_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_incidents(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_incidents().await
    } else {
        state.delete_bug_monitor_incidents(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor incidents",
                "code": "BUG_MONITOR_INCIDENTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn delete_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_drafts(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                "draft_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor draft",
                "code": "BUG_MONITOR_DRAFT_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_drafts(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_drafts().await
    } else {
        state.delete_bug_monitor_drafts(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor drafts",
                "code": "BUG_MONITOR_DRAFTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn delete_bug_monitor_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_posts(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor post not found",
                "code": "BUG_MONITOR_POST_NOT_FOUND",
                "post_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor post",
                "code": "BUG_MONITOR_POST_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_posts(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_posts().await
    } else {
        state.delete_bug_monitor_posts(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor posts",
                "code": "BUG_MONITOR_POSTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn pause_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = true;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to pause Bug Monitor",
                "code": "BUG_MONITOR_PAUSE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn resume_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = false;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to resume Bug Monitor",
                "code": "BUG_MONITOR_RESUME_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn replay_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(incident) = state.get_bug_monitor_incident(&id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    let Some(draft_id) = incident.draft_id.as_deref() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug monitor incident has no associated draft",
                "code": "BUG_MONITOR_INCIDENT_NO_DRAFT",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    match ensure_bug_monitor_triage_run(state.clone(), draft_id, true).await {
        Ok((draft, run, deduped)) => {
            let triage_run_id = draft.triage_run_id.as_deref().unwrap_or(run.as_str());
            refresh_bug_monitor_duplicate_matches_artifact(&state, &draft, triage_run_id).await;
            let run = load_context_run_state(&state, triage_run_id).await.ok();
            let triage_summary =
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await;
            let issue_draft = ensure_bug_monitor_issue_draft(state.clone(), draft_id, true)
                .await
                .ok();
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, Some(triage_run_id)).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(triage_run_id));
            Json(json!({
                "ok": true,
                "incident": incident,
                "draft": draft,
                "run": run,
                "deduped": deduped,
                "triage_summary": triage_summary,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to replay Bug Monitor incident",
                "code": "BUG_MONITOR_INCIDENT_REPLAY_FAILED",
                "incident_id": id,
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let draft = state.get_bug_monitor_draft(&id).await;
    match draft {
        Some(draft) => Json(json!({ "draft": draft })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
            })),
        )
            .into_response(),
    }
}

fn map_bug_monitor_draft_update_error(
    draft_id: String,
    error: anyhow::Error,
) -> (StatusCode, Json<serde_json::Value>) {
    let detail = error.to_string();
    if detail.contains("not found") {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                "draft_id": draft_id,
            })),
        )
    } else if detail.contains("not waiting for approval") {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug Monitor draft is not waiting for approval",
                "code": "BUG_MONITOR_DRAFT_NOT_PENDING_APPROVAL",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to update Bug Monitor draft",
                "code": "BUG_MONITOR_DRAFT_UPDATE_FAILED",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    }
}

pub(super) async fn report_bug_monitor_issue(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorSubmissionInput>,
) -> Response {
    let Some(report) = input.report else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "report object is required",
                "code": "BUG_MONITOR_REPORT_REQUIRED",
            })),
        )
            .into_response();
    };
    let config = state.bug_monitor_config().await;
    let effective_repo = report
        .repo
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(config.repo.as_deref())
        .unwrap_or_default();
    let duplicate_matches = bug_monitor_failure_pattern_matches(
        &state,
        effective_repo,
        report.fingerprint.as_deref().unwrap_or_default(),
        report.title.as_deref(),
        report.detail.as_deref(),
        &report.excerpt,
        3,
    )
    .await;
    if !duplicate_matches.is_empty() {
        let duplicate_summary = build_bug_monitor_duplicate_summary(&duplicate_matches);
        return Json(json!({
            "suppressed": true,
            "reason": "duplicate_failure_pattern",
            "duplicate_summary": duplicate_summary,
            "duplicate_matches": duplicate_matches,
        }))
        .into_response();
    }
    let report_excerpt = report.excerpt.clone();
    match state.submit_bug_monitor_draft(report.clone()).await {
        Ok(draft) => {
            let duplicate_matches = bug_monitor_failure_pattern_matches(
                &state,
                &draft.repo,
                &draft.fingerprint,
                draft.title.as_deref(),
                draft.detail.as_deref(),
                &report_excerpt,
                3,
            )
            .await;
            Json(json!({
                "draft": draft,
                "duplicate_summary": build_bug_monitor_duplicate_summary(&duplicate_matches),
                "duplicate_matches": duplicate_matches,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let blocked_incident = if detail.contains("signal quality gate") {
                persist_blocked_bug_monitor_report_observation(
                    &state,
                    &report,
                    effective_repo,
                    &detail,
                )
                .await
            } else {
                None
            };
            let quality_gate = blocked_incident
                .as_ref()
                .and_then(|incident| incident.quality_gate.clone());
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to create Bug Monitor draft",
                    "code": "BUG_MONITOR_REPORT_INVALID",
                    "detail": detail,
                    "incident": blocked_incident,
                    "quality_gate": quality_gate,
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn approve_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "draft_ready", input.reason.as_deref())
        .await
    {
        Ok(draft) => {
            let had_triage_run = draft.triage_run_id.is_some();
            let approved_draft = if draft.triage_run_id.is_none() {
                ensure_bug_monitor_triage_run(state.clone(), &draft.draft_id, true)
                    .await
                    .map(|(draft, _, _)| draft)
                    .unwrap_or(draft)
            } else {
                draft
            };
            let approval_failure_pattern_memory = if !had_triage_run {
                persist_bug_monitor_failure_pattern_from_approved_draft(&state, &approved_draft)
                    .await
                    .ok()
            } else {
                None
            };
            let issue_draft =
                ensure_bug_monitor_issue_draft(state.clone(), &approved_draft.draft_id, true)
                    .await
                    .ok();
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                approved_draft.triage_run_id.as_deref(),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, approved_draft.triage_run_id.as_deref());
            match bug_monitor_github::publish_draft(
                &state,
                &approved_draft.draft_id,
                None,
                bug_monitor_github::PublishMode::Auto,
            )
            .await
            {
                Ok(outcome) => {
                    let external_action = match outcome.post.as_ref() {
                        Some(post) => state.get_external_action(&post.post_id).await,
                        None => None,
                    };
                    Json(json!({
                        "ok": true,
                        "draft": outcome.draft,
                        "action": outcome.action,
                        "failure_pattern_memory": approval_failure_pattern_memory,
                        "issue_draft": issue_draft,
                        "duplicate_summary": duplicate_summary,
                        "duplicate_matches": duplicate_matches,
                        "triage_summary_artifact": triage_summary_artifact,
                        "issue_draft_artifact": issue_draft_artifact,
                        "duplicate_matches_artifact": duplicate_matches_artifact,
                        "post": outcome.post,
                        "external_action": external_action,
                    }))
                    .into_response()
                }
                Err(error) => {
                    let detail = error.to_string();
                    let mut updated_draft = state
                        .get_bug_monitor_draft(&approved_draft.draft_id)
                        .await
                        .unwrap_or(approved_draft);
                    updated_draft.last_post_error = Some(detail.clone());
                    updated_draft
                        .github_status
                        .get_or_insert_with(|| "publish_blocked".to_string());
                    let updated_draft = state
                        .put_bug_monitor_draft(updated_draft.clone())
                        .await
                        .unwrap_or(updated_draft);
                    Json(json!({
                        "ok": true,
                        "draft": updated_draft,
                        "action": "approved",
                        "failure_pattern_memory": approval_failure_pattern_memory,
                        "issue_draft": issue_draft,
                        "duplicate_summary": duplicate_summary,
                        "duplicate_matches": duplicate_matches,
                        "triage_summary_artifact": triage_summary_artifact,
                        "issue_draft_artifact": issue_draft_artifact,
                        "duplicate_matches_artifact": duplicate_matches_artifact,
                        "publish_error": detail,
                    }))
                    .into_response()
                }
            }
        }
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn draft_bug_monitor_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_bug_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => {
            let triage_run_id = issue_draft.get("triage_run_id").and_then(Value::as_str);
            let draft = state.get_bug_monitor_draft(&id).await;
            let triage_summary = triage_run_id.map(|run_id| async {
                load_bug_monitor_triage_summary_artifact(&state, run_id).await
            });
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, triage_run_id).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, triage_run_id);
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
            Json(json!({
                "ok": true,
                "draft": draft,
                "triage_summary": triage_summary,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (StatusCode::BAD_REQUEST, {
            let draft = state.get_bug_monitor_draft(&id).await;
            let triage_run_id = draft.as_ref().and_then(|row| row.triage_run_id.clone());
            let proposal_quality_gate = match triage_run_id.as_deref() {
                Some(run_id) => {
                    load_bug_monitor_proposal_quality_gate_artifact(&state, run_id).await
                }
                None => None,
            };
            let proposal_quality_gate_artifact = triage_run_id.as_deref().and_then(|run_id| {
                latest_bug_monitor_artifact(&state, run_id, "bug_monitor_proposal_quality_gate")
            });
            Json(json!({
                "error": "Failed to generate Bug Monitor issue draft",
                "code": "BUG_MONITOR_ISSUE_DRAFT_FAILED",
                "draft_id": id,
                "draft": draft,
                "proposal_quality_gate": proposal_quality_gate,
                "proposal_quality_gate_artifact": proposal_quality_gate_artifact,
                "detail": error.to_string(),
            }))
        })
            .into_response(),
    }
}

pub(super) async fn deny_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "denied", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn create_bug_monitor_triage_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_bug_monitor_triage_run(state.clone(), &id, false).await {
        Ok((draft, run_id, deduped)) => {
            let triage_run_id = draft.triage_run_id.as_deref().unwrap_or(run_id.as_str());
            let run = load_context_run_state(&state, triage_run_id).await.ok();
            let triage_summary =
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await;
            let issue_draft = ensure_bug_monitor_issue_draft(state.clone(), &id, true)
                .await
                .ok();
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, Some(triage_run_id)).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(triage_run_id));
            Json(json!({
                "ok": true,
                "draft": draft,
                "run": run,
                "deduped": deduped,
                "triage_summary": triage_summary,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let status = if detail.contains("not found") {
                StatusCode::NOT_FOUND
            } else if detail.contains("approved") || detail.contains("Denied") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(json!({
                    "error": "Failed to create Bug Monitor triage run",
                    "code": "BUG_MONITOR_TRIAGE_RUN_CREATE_FAILED",
                    "draft_id": id,
                    "detail": detail,
                })),
            )
                .into_response()
        }
    }
}
