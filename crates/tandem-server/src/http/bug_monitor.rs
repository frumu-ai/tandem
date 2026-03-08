use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{bug_monitor_github, BugMonitorConfig, BugMonitorDraftRecord, BugMonitorSubmission};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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
    include_str!("../../../../.github/ISSUE_TEMPLATE/bug_report.md");

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
    pub expected_behavior: Option<String>,
    #[serde(default)]
    pub steps_to_reproduce: Vec<String>,
    #[serde(default)]
    pub environment: Vec<String>,
    #[serde(default)]
    pub logs: Vec<String>,
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
    let path =
        super::context_runs::context_run_dir(state, linked_context_run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let raw =
        serde_json::to_string_pretty(payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact = ContextBlackboardArtifact {
        id: artifact_id.to_string(),
        ts_ms: crate::now_ms(),
        path: path.to_string_lossy().to_string(),
        artifact_type: artifact_type.to_string(),
        step_id: None,
        source_event_id: None,
    };
    super::context_runs::context_run_engine()
        .commit_blackboard_patch(
            state,
            linked_context_run_id,
            super::context_types::ContextBlackboardPatchOp::AddArtifact,
            serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await?;
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
    let put_response = super::skills_memory::memory_put_impl(
        state,
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
    let put_response = super::skills_memory::memory_put_impl(
        state,
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
    super::coder::query_failure_pattern_matches(
        state,
        repo_slug,
        fingerprint,
        title,
        detail,
        excerpt,
        limit,
    )
    .await
    .unwrap_or_default()
}

pub(crate) fn build_bug_monitor_duplicate_summary(matches: &[Value]) -> Value {
    let normalized_matches = matches
        .iter()
        .map(|row| {
            json!({
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
                "candidate_id": row.get("candidate_id").cloned().unwrap_or(Value::Null),
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
    let duplicate_summary = duplicate_matches
        .as_ref()
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .map(|rows| build_bug_monitor_duplicate_summary(rows));
    (duplicate_summary, duplicate_matches)
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
    let triage_summary = load_bug_monitor_triage_summary_artifact(&state, &triage_run_id).await;
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
    let hidden_markers = vec![
        format!("<!-- tandem:fingerprint:v1:{} -->", draft.fingerprint),
        format!("<!-- tandem:triage_run_id:v1:{} -->", triage_run_id),
    ];
    let rendered_body = render_bug_monitor_template(
        &template,
        &what_happened,
        &expected_behavior,
        &steps_to_reproduce,
        &environment_lines,
        &log_lines,
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
    let payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "suggested_title": input.suggested_title.as_deref().and_then(normalize_issue_draft_line),
        "what_happened": what_happened,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment,
        "logs": logs,
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
    let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
        bug_monitor_triage_artifacts(&state, Some(&triage_run_id));
    match ensure_bug_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => Json(json!({
            "ok": true,
            "draft": draft,
            "triage_summary": payload,
            "triage_summary_artifact": triage_summary_artifact,
            "failure_pattern_memory": failure_pattern_memory,
            "issue_draft": issue_draft,
            "issue_draft_artifact": issue_draft_artifact,
            "duplicate_matches_artifact": duplicate_matches_artifact,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Bug Monitor triage summary was written, but issue draft regeneration failed",
                "code": "BUG_MONITOR_TRIAGE_SUMMARY_ISSUE_DRAFT_FAILED",
                "draft": draft,
                "triage_summary": payload,
                "triage_summary_artifact": triage_summary_artifact,
                "failure_pattern_memory": failure_pattern_memory,
                "duplicate_matches_artifact": duplicate_matches_artifact,
                "detail": error.to_string(),
            })),
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
    match state.submit_bug_monitor_draft(report).await {
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
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to create Bug Monitor draft",
                "code": "BUG_MONITOR_REPORT_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
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
            let approved_draft = draft.clone();
            let approval_failure_pattern_memory = if approved_draft.triage_run_id.is_none() {
                persist_bug_monitor_failure_pattern_from_approved_draft(&state, &approved_draft)
                    .await
                    .ok()
            } else {
                None
            };
            let issue_draft = ensure_bug_monitor_issue_draft(state.clone(), &draft.draft_id, true)
                .await
                .ok();
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, draft.triage_run_id.as_deref());
            match bug_monitor_github::publish_draft(
                &state,
                &draft.draft_id,
                None,
                bug_monitor_github::PublishMode::Auto,
            )
            .await
            {
                Ok(outcome) => Json(json!({
                    "ok": true,
                    "draft": outcome.draft,
                    "action": outcome.action,
                    "failure_pattern_memory": approval_failure_pattern_memory,
                    "issue_draft": issue_draft,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "post": outcome.post,
                }))
                .into_response(),
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
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, triage_run_id).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, triage_run_id);
            Json(json!({
                "ok": true,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to generate Bug Monitor issue draft",
                "code": "BUG_MONITOR_ISSUE_DRAFT_FAILED",
                "draft_id": id,
                "detail": error.to_string(),
            })),
        )
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

pub(super) async fn publish_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let existing_draft = state.get_bug_monitor_draft(&id).await;
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::ManualPublish,
    )
    .await
    {
        Ok(outcome) => {
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_bug_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            Json(json!({
                "ok": true,
                "draft": outcome.draft,
                "action": outcome.action,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
                "post": outcome.post,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_bug_monitor_draft(&id).await.or(existing_draft);
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_bug_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to publish Bug Monitor draft to GitHub",
                    "code": "BUG_MONITOR_DRAFT_PUBLISH_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": error.to_string(),
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn recheck_bug_monitor_draft_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let existing_draft = state.get_bug_monitor_draft(&id).await;
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::RecheckOnly,
    )
    .await
    {
        Ok(outcome) => {
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_bug_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            Json(json!({
                "ok": true,
                "draft": outcome.draft,
                "action": outcome.action,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
                "post": outcome.post,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_bug_monitor_draft(&id).await.or(existing_draft);
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_bug_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to recheck Bug Monitor draft against GitHub",
                    "code": "BUG_MONITOR_DRAFT_RECHECK_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": error.to_string(),
                })),
            )
                .into_response()
        }
    }
}

pub(crate) async fn ensure_bug_monitor_triage_run(
    state: AppState,
    id: &str,
    bypass_approval_gate: bool,
) -> anyhow::Result<(BugMonitorDraftRecord, String, bool)> {
    let config = state.bug_monitor_config().await;
    let draft = state
        .get_bug_monitor_draft(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;

    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Denied Bug Monitor drafts cannot create triage runs");
    }
    if !bypass_approval_gate
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        anyhow::bail!("Bug Monitor draft must be approved before triage run creation");
    }

    if let Some(existing_run_id) = draft.triage_run_id.clone() {
        match load_context_run_state(&state, &existing_run_id).await {
            Ok(_) => return Ok((draft, existing_run_id, true)),
            Err(_) => {}
        }
    }

    let run_id = format!("failure-triage-{}", Uuid::new_v4().simple());
    let objective = format!(
        "Triage bug monitor draft {} for {}: {}",
        draft.draft_id,
        draft.repo,
        draft
            .title
            .clone()
            .unwrap_or_else(|| "Untitled failure".to_string())
    );
    let workspace = config
        .workspace_root
        .as_ref()
        .map(|root| ContextWorkspaceLease {
            workspace_id: root.clone(),
            canonical_path: root.clone(),
            lease_epoch: crate::now_ms(),
        });
    let model_provider = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("provider_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let model_id = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("model_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let mcp_servers = config
        .mcp_server
        .as_ref()
        .map(|row| vec![row.clone()])
        .filter(|row| !row.is_empty());

    let duplicate_matches = super::coder::query_failure_pattern_matches(
        &state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
    )
    .await
    .map_err(|status| {
        anyhow::anyhow!("Failed to query duplicate failure patterns: HTTP {status}")
    })?;

    let create_input = ContextRunCreateInput {
        run_id: Some(run_id.clone()),
        objective,
        run_type: Some("bug_monitor_triage".to_string()),
        workspace,
        source_client: Some("bug_monitor".to_string()),
        model_provider,
        model_id,
        mcp_servers,
    };
    let created_run =
        match super::context_runs::context_run_create(State(state.clone()), Json(create_input))
            .await
        {
            Ok(Json(payload)) => match serde_json::from_value::<ContextRunState>(
                payload.get("run").cloned().unwrap_or_default(),
            ) {
                Ok(run) => run,
                Err(_) => anyhow::bail!("Failed to deserialize triage context run"),
            },
            Err(status) => anyhow::bail!("Failed to create triage context run: HTTP {status}"),
        };

    let inspect_task_id = format!("triage-inspect-{}", Uuid::new_v4().simple());
    let validate_task_id = format!("triage-validate-{}", Uuid::new_v4().simple());
    let tasks_input = ContextTaskCreateBatchInput {
        tasks: vec![
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:inspect")),
                id: Some(inspect_task_id.clone()),
                task_type: "inspection".to_string(),
                payload: json!({
                    "task_kind": "inspection",
                    "title": "Inspect failure report and affected area",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "summary": draft.title,
                    "detail": draft.detail,
                    "duplicate_matches": duplicate_matches,
                }),
                status: Some(ContextBlackboardTaskStatus::Runnable),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("inspect_failure_report".to_string()),
                parent_task_id: None,
                depends_on_task_ids: Vec::new(),
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(10),
                max_attempts: Some(2),
            },
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:validate")),
                id: Some(validate_task_id.clone()),
                task_type: "validation".to_string(),
                payload: json!({
                    "task_kind": "validation",
                    "title": "Reproduce or validate failure scope",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "depends_on": inspect_task_id,
                }),
                status: Some(ContextBlackboardTaskStatus::Pending),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("validate_failure_scope".to_string()),
                parent_task_id: None,
                depends_on_task_ids: vec![inspect_task_id.clone()],
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(5),
                max_attempts: Some(2),
            },
        ],
    };
    let tasks_response = context_run_tasks_create(
        State(state.clone()),
        Path(run_id.clone()),
        Json(tasks_input),
    )
    .await;
    if tasks_response.is_err() {
        anyhow::bail!("Failed to seed triage tasks");
    }

    if !duplicate_matches.is_empty() {
        write_bug_monitor_artifact(
            &state,
            &run_id,
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
        .map_err(|status| {
            anyhow::anyhow!("Failed to write duplicate matches artifact: HTTP {status}")
        })?;
    }

    let mut updated_draft = draft.clone();
    updated_draft.triage_run_id = Some(run_id.clone());
    updated_draft.status = "triage_queued".to_string();
    {
        let mut drafts = state.bug_monitor_drafts.write().await;
        drafts.insert(updated_draft.draft_id.clone(), updated_draft.clone());
    }
    state.persist_bug_monitor_drafts().await?;

    let mut run = match load_context_run_state(&state, &run_id).await {
        Ok(row) => row,
        Err(_) => created_run,
    };
    run.status = ContextRunStatus::Planning;
    run.why_next_step =
        Some("Inspect the failure report, then validate the failure scope.".to_string());
    ensure_context_run_dir(&state, &run_id)
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to finalize triage run workspace: HTTP {status}")
        })?;
    save_context_run_state(&state, &run)
        .await
        .map_err(|status| anyhow::anyhow!("Failed to finalize triage run state: HTTP {status}"))?;
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "bug_monitor.triage_run.created",
        json!({
            "draft_id": updated_draft.draft_id,
            "run_id": run.run_id,
            "repo": updated_draft.repo,
        }),
    ));

    Ok((updated_draft, run.run_id, false))
}
