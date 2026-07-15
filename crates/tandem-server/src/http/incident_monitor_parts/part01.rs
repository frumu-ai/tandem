// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
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

const DEFAULT_INCIDENT_MONITOR_TEMPLATE: &str =
    include_str!("../../../resources/issue_templates/bug_report.md");

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorConfigInput {
    #[serde(default)]
    pub incident_monitor: Option<IncidentMonitorConfig>,
}
#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorDraftsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorIncidentsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorPostsQuery {
    pub limit: Option<usize>,
    pub destination_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorBulkDeleteInput {
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorSubmissionInput {
    #[serde(default)]
    pub report: Option<IncidentMonitorSubmission>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorRoutePreviewInput {
    #[serde(default)]
    pub draft_id: Option<String>,
    #[serde(default)]
    pub incident_id: Option<String>,
    #[serde(default)]
    pub report: Option<IncidentMonitorSubmission>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default)]
    pub risk_level: Option<String>,
    #[serde(default)]
    pub risk_category: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub log_source_id: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub destination_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorIntakeReportInput {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub report: Option<IncidentMonitorSubmission>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorCreateIntakeKeyInput {
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorDecisionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorPublishInput {
    #[serde(default)]
    pub destination_id: Option<String>,
    #[serde(default)]
    pub destination_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct IncidentMonitorTriageSummaryInput {
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
    pub file_references: Vec<Value>,
    #[serde(default)]
    pub fix_points: Vec<Value>,
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

async fn write_incident_monitor_artifact(
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

fn incident_monitor_coder_ready_gate(
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

fn incident_monitor_value_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn incident_monitor_summary_string_array(summary: Option<&Value>, key: &str) -> Vec<String> {
    incident_monitor_value_array(summary.and_then(|row| row.get(key)))
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

fn incident_monitor_summary_text(summary: Option<&Value>, key: &str) -> Option<String> {
    summary
        .and_then(|row| row.get(key))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
}

fn incident_monitor_triage_summary_input_has_substance(input: &IncidentMonitorTriageSummaryInput) -> bool {
    input
        .suggested_title
        .as_deref()
        .is_some_and(|row| !row.trim().is_empty())
        || input
            .what_happened
            .as_deref()
            .is_some_and(|row| !row.trim().is_empty())
        || input
            .why_it_likely_happened
            .as_deref()
            .is_some_and(|row| !row.trim().is_empty())
        || input
            .recommended_fix
            .as_deref()
            .is_some_and(|row| !row.trim().is_empty())
        || !input.affected_components.is_empty()
        || !input.likely_files_to_edit.is_empty()
        || !input.steps_to_reproduce.is_empty()
        || !input.logs.is_empty()
        || !input.related_existing_issues.is_empty()
        || !input.related_failure_patterns.is_empty()
        || !input.research_sources.is_empty()
        || !input.file_references.is_empty()
        || !input.fix_points.is_empty()
        || !input.acceptance_criteria.is_empty()
        || !input.verification_steps.is_empty()
}

fn incident_monitor_value_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .and_then(normalize_issue_draft_line)
}

fn incident_monitor_value_strings(payload: &Value, keys: &[&str], limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for key in keys {
        match payload.get(*key) {
            Some(Value::Array(rows)) => {
                for row in rows {
                    if out.len() >= limit {
                        return out;
                    }
                    if let Some(value) = row
                        .as_str()
                        .and_then(normalize_issue_draft_line)
                        .or_else(|| normalize_issue_draft_line(row.to_string()))
                    {
                        if !out.iter().any(|existing| existing == &value) {
                            out.push(value);
                        }
                    }
                }
            }
            Some(Value::String(row)) => {
                if let Some(value) = normalize_issue_draft_line(row) {
                    if !out.iter().any(|existing| existing == &value) {
                        out.push(value);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn incident_monitor_push_unique(rows: &mut Vec<String>, value: impl AsRef<str>, limit: usize) {
    if rows.len() >= limit {
        return;
    }
    let Some(value) = normalize_issue_draft_line(value.as_ref()) else {
        return;
    };
    if !rows.iter().any(|existing| existing == &value) {
        rows.push(value);
    }
}

fn incident_monitor_search_term_is_useful(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() < 6 || normalized.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    !matches!(
        normalized.as_str(),
        "workflow"
            | "failed"
            | "failure"
            | "automation"
            | "automations"
            | "blocked"
            | "upstream"
            | "outcome"
            | "required"
            | "output"
            | "created"
            | "create"
            | "error"
            | "errors"
            | "issue"
            | "issues"
            | "provider"
            | "activity"
            | "without"
            | "process"
            | "tandem"
            | "engine"
    )
}

fn incident_monitor_failure_type(reason: &str, event_type: &str) -> String {
    let haystack = format!("{reason}\n{event_type}").to_ascii_lowercase();
    if haystack.contains("timeout") || haystack.contains("timed out") {
        "timeout"
    } else if haystack.contains("required output")
        || haystack.contains("artifact")
        || haystack.contains("validation")
        || haystack.contains("prewrite")
    {
        "validation_error"
    } else if haystack.contains("mcp")
        || haystack.contains("tool")
        || haystack.contains("github")
        || haystack.contains("provider stream")
    {
        "tool_error"
    } else if haystack.contains("permission")
        || haystack.contains("auth")
        || haystack.contains("unauthorized")
    {
        "missing_capability"
    } else {
        "unknown"
    }
    .to_string()
}

fn incident_monitor_candidate_search_terms(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
    incident_payload: &Value,
) -> Vec<String> {
    let mut terms = Vec::new();
    for candidate in [
        draft.title.as_deref(),
        draft.detail.as_deref(),
        incident.map(|row| row.title.as_str()),
        incident.and_then(|row| row.last_error.as_deref()),
    ] {
        if let Some(candidate) = candidate {
            for part in candidate
                .split(|ch: char| {
                    !(ch.is_ascii_alphanumeric()
                        || ch == '_'
                        || ch == '-'
                        || ch == '.'
                        || ch == '/')
                })
                .map(str::trim)
                .filter(|part| part.len() <= 120 && incident_monitor_search_term_is_useful(part))
            {
                incident_monitor_push_unique(&mut terms, part, 16);
            }
        }
    }
    for candidate in [
        incident_monitor_value_string(
            incident_payload,
            &["reason", "error", "failureCode", "blockedReasonCode"],
        ),
        incident_monitor_value_string(
            incident_payload,
            &["task_id", "taskID", "stage_id", "node_id"],
        ),
    ]
    .into_iter()
    .flatten()
    {
        for part in candidate
            .split(|ch: char| {
                !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '/')
            })
            .map(str::trim)
            .filter(|part| part.len() <= 120 && incident_monitor_search_term_is_useful(part))
        {
            incident_monitor_push_unique(&mut terms, part, 16);
        }
    }
    for key in [
        "task_id",
        "taskID",
        "stage_id",
        "stageID",
        "node_id",
        "nodeID",
        "component",
        "tool_name",
        "toolName",
        "error_kind",
        "errorKind",
    ] {
        if let Some(value) = incident_payload.get(key).and_then(Value::as_str) {
            if incident_monitor_search_term_is_useful(value) {
                incident_monitor_push_unique(&mut terms, value, 16);
            }
        }
    }
    terms
}

fn incident_monitor_path_is_researchable(path: &FsPath) -> bool {
    let Some(name) = path.file_name().and_then(|row| row.to_str()) else {
        return false;
    };
    if matches!(
        name,
        "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
    ) {
        return false;
    }
    let path_text = path.to_string_lossy();
    if path_text.contains("/docs/")
        || path_text.contains("/agent-templates/")
        || path_text.contains("/target/")
        || path_text.contains("/node_modules/")
        || path_text.contains("/.tandem/")
    {
        return false;
    }
    matches!(
        path.extension().and_then(|row| row.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx")
    )
}

fn incident_monitor_search_repo_file_references(workspace_root: &str, terms: &[String]) -> Vec<Value> {
    let root = FsPath::new(workspace_root);
    if !root.is_dir() || terms.is_empty() {
        return Vec::new();
    }
    let lowered_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in WalkBuilder::new(root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .build()
        .flatten()
    {
        if refs.len() >= 12 {
            break;
        }
        let path = entry.path();
        if !path.is_file() || !incident_monitor_path_is_researchable(path) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        if raw.len() > 400_000 {
            continue;
        }
        for (idx, line) in raw.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            let Some(term) = lowered_terms
                .iter()
                .find(|term| lower.contains(term.as_str()))
            else {
                continue;
            };
            let display_path = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            let key = format!("{}:{}", display_path, idx + 1);
            if !seen.insert(key) {
                continue;
            }
            refs.push(json!({
                "path": display_path,
                "line": idx + 1,
                "excerpt": crate::truncate_text(line.trim(), 240),
                "matched_term": term,
                "reason": "Local repository search matched failure evidence from the Incident Monitor draft.",
                "confidence": "medium",
            }));
            break;
        }
    }
    refs
}

fn incident_monitor_fallback_file_references(reason: &str) -> Vec<Value> {
    let lower = reason.to_ascii_lowercase();
    let mut refs = Vec::new();
    let mut push = |path: &str, reason: &str| {
        refs.push(json!({
            "path": path,
            "line": Value::Null,
            "excerpt": Value::Null,
            "reason": reason,
            "confidence": "medium",
        }));
    };
    if lower.contains("mcp") || lower.contains("github") || lower.contains("tool") {
        push(
            "crates/tandem-server/src/incident_monitor_github.rs",
            "GitHub/MCP issue publishing and lookup flow lives here.",
        );
        push(
            "crates/tandem-runtime/src/mcp_ready.rs",
            "MCP readiness and reconnect behavior lives here.",
        );
    }
    if lower.contains("required output")
        || lower.contains("artifact")
        || lower.contains("in_progress")
        || lower.contains("prewrite")
        || lower.contains("upstream node outcome")
    {
        push(
            "crates/tandem-server/src/app/state/automation/logic_parts/part04.rs",
            "Automation artifact validation rejects missing or non-terminal required outputs.",
        );
        push(
            "crates/tandem-server/src/app/state/automation/prompting_impl.rs",
            "Required artifact instructions are generated here.",
        );
        push(
            "crates/tandem-core/src/engine_loop/prewrite_mode.rs",
            "Tool-mode repair and required-write enforcement lives here.",
        );
    }
    if lower.contains("no provider activity")
        || lower.contains("stale")
        || lower.contains("paused after")
    {
        push(
            "crates/tandem-server/src/automation_v2/executor.rs",
            "Automation V2 stale/no-provider-activity handling is reported from the executor.",
        );
        push(
            "crates/tandem-server/src/app/state/automation/workflow_learning.rs",
            "Automation failure learning and recurring workflow blockers are summarized here.",
        );
    }
    if lower.contains("read-only source") || lower.contains("modify read-only") {
        push(
            "crates/tandem-core/src/engine_loop/write_targets.rs",
            "Write-target derivation decides whether a tool call writes source files.",
        );
    }
    refs
}

fn incident_monitor_proposal_quality_gate(
    state: &AppState,
    triage_run_id: &str,
    triage_summary: Option<&Value>,
) -> (bool, Value) {
    let has_summary = triage_summary.is_some();
    let inspection_artifact =
        incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_inspection");
    let research_artifact =
        incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_research");
    let validation_artifact =
        incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_validation");
    let fix_artifact = incident_monitor_completed_phase_artifact_exists(
        state,
        triage_run_id,
        "incident_monitor_fix_proposal",
    );
    let durable_artifacts =
        inspection_artifact && research_artifact && validation_artifact && fix_artifact;

    let research_sources =
        incident_monitor_value_array(triage_summary.and_then(|row| row.get("research_sources")));
    let related_existing_issues =
        incident_monitor_value_array(triage_summary.and_then(|row| row.get("related_existing_issues")));
    let related_failure_patterns =
        incident_monitor_value_array(triage_summary.and_then(|row| row.get("related_failure_patterns")));
    let why_it_likely_happened =
        incident_monitor_summary_text(triage_summary, "why_it_likely_happened").unwrap_or_default();
    let research_performed = !research_sources.is_empty()
        || !related_existing_issues.is_empty()
        || !related_failure_patterns.is_empty()
        || (!why_it_likely_happened.is_empty()
            && !why_it_likely_happened
                .to_ascii_lowercase()
                .contains("pending"));

    let steps_to_reproduce = incident_monitor_summary_string_array(triage_summary, "steps_to_reproduce");
    let logs = incident_monitor_summary_string_array(triage_summary, "logs");
    let validation_confirmed = !steps_to_reproduce.is_empty() || !logs.is_empty();

    let root_cause_confidence =
        incident_monitor_summary_text(triage_summary, "root_cause_confidence").unwrap_or_default();
    let notes = incident_monitor_summary_text(triage_summary, "notes").unwrap_or_default();
    let uncertainty_explicit =
        matches!(root_cause_confidence.as_str(), "high" | "medium" | "low") || !notes.is_empty();

    let recommended_fix =
        incident_monitor_summary_text(triage_summary, "recommended_fix").unwrap_or_default();
    let acceptance_criteria =
        incident_monitor_summary_string_array(triage_summary, "acceptance_criteria");
    let bounded_action = !recommended_fix.is_empty()
        && !recommended_fix
            .to_ascii_lowercase()
            .contains("complete the incident monitor research")
        && !acceptance_criteria.is_empty();

    let verification_steps = incident_monitor_summary_string_array(triage_summary, "verification_steps");
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

fn incident_monitor_completed_phase_artifact_exists(
    state: &AppState,
    triage_run_id: &str,
    artifact_type: &str,
) -> bool {
    let Some(artifact) = latest_incident_monitor_artifact(state, triage_run_id, artifact_type) else {
        return false;
    };
    let Ok(raw) = std::fs::read_to_string(&artifact.path) else {
        return false;
    };
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    incident_monitor_phase_artifact_payload_candidates(&payload, artifact_type)
        .into_iter()
        .any(|candidate| incident_monitor_completed_phase_payload_exists(candidate, artifact_type))
}

fn incident_monitor_completed_phase_payload_exists(payload: &Value, artifact_type: &str) -> bool {
    if incident_monitor_artifact_is_task_spec_placeholder(payload) {
        return false;
    }
    match artifact_type {
        "incident_monitor_research" => {
            !incident_monitor_value_array(payload.get("research_sources")).is_empty()
                || !incident_monitor_value_array(payload.get("related_existing_issues")).is_empty()
                || !incident_monitor_value_array(payload.get("related_failure_patterns")).is_empty()
                || payload
                    .get("findings")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| !rows.is_empty())
                || payload
                    .get("summary")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
                || !incident_monitor_value_array(payload.get("search_queries_used")).is_empty()
                || payload.get("tool_evidence").is_some()
        }
        "incident_monitor_validation" => {
            !incident_monitor_value_array(payload.get("evidence")).is_empty()
                || !incident_monitor_value_array(payload.get("validation_errors")).is_empty()
                || !incident_monitor_value_array(payload.get("steps_to_reproduce")).is_empty()
                || payload
                    .get("failure_scope")
                    .is_some_and(incident_monitor_non_empty_value)
                || payload
                    .get("summary")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
                || payload.get("validation_decision").is_some()
                || !incident_monitor_value_array(payload.get("file_references")).is_empty()
                || payload.get("tool_evidence").is_some()
        }
        "incident_monitor_fix_proposal" => {
            payload
                .get("recommended_fix")
                .is_some_and(incident_monitor_non_empty_value)
                && (!incident_monitor_value_array(payload.get("acceptance_criteria")).is_empty()
                    || !incident_monitor_value_array(payload.get("verification_steps")).is_empty()
                    || !incident_monitor_value_array(payload.get("smoke_test_steps")).is_empty())
        }
        "incident_monitor_inspection" => {
            payload.get("detail").is_some()
                || payload.get("incident").is_some()
                || payload.get("incident_payload").is_some()
                || payload.get("original_failure").is_some()
                || !incident_monitor_value_array(payload.get("search_queries_used")).is_empty()
                || payload.get("tool_evidence").is_some()
        }
        _ => true,
    }
}

fn incident_monitor_non_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(rows) => !rows.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

fn incident_monitor_phase_artifact_payload_candidates<'a>(
    payload: &'a Value,
    artifact_type: &str,
) -> Vec<&'a Value> {
    let mut candidates = Vec::new();
    incident_monitor_collect_phase_artifact_payload_candidates(payload, artifact_type, &mut candidates);
    candidates
}

fn incident_monitor_collect_phase_artifact_payload_candidates<'a>(
    payload: &'a Value,
    artifact_type: &str,
    candidates: &mut Vec<&'a Value>,
) {
    match payload {
        Value::Array(rows) => {
            for row in rows {
                incident_monitor_collect_phase_artifact_payload_candidates(
                    row,
                    artifact_type,
                    candidates,
                );
            }
        }
        Value::Object(map) => {
            if !incident_monitor_artifact_is_task_spec_placeholder(payload) {
                candidates.push(payload);
            }
            for key in [
                artifact_type,
                "incident_monitor_inspection",
                "incident_monitor_research",
                "incident_monitor_validation",
                "incident_monitor_fix_proposal",
                "structured_handoff",
                "content",
            ] {
                if let Some(child) = map.get(key) {
                    incident_monitor_collect_phase_artifact_payload_candidates(
                        child,
                        artifact_type,
                        candidates,
                    );
                }
            }
        }
        _ => {}
    }
}

async fn ensure_incident_monitor_phase_artifacts_from_summary(
    state: &AppState,
    triage_run_id: &str,
    payload: &Value,
) -> Result<(), StatusCode> {
    if !incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_inspection")
    {
        write_incident_monitor_artifact(
            state,
            triage_run_id,
            &format!("incident-monitor-inspection-summary-{}", Uuid::new_v4().simple()),
            "incident_monitor_inspection",
            "artifacts/incident_monitor.inspection.json",
            &json!({
                "status": "completed",
                "detail": payload.get("what_happened").cloned().unwrap_or(Value::Null),
                "incident": {
                    "draft_id": payload.get("draft_id").cloned().unwrap_or(Value::Null),
                    "repo": payload.get("repo").cloned().unwrap_or(Value::Null),
                },
                "incident_payload": payload,
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await?;
    }
    if !incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_research") {
        write_incident_monitor_artifact(
            state,
            triage_run_id,
            &format!("incident-monitor-research-summary-{}", Uuid::new_v4().simple()),
            "incident_monitor_research",
            "artifacts/incident_monitor.research.json",
            &json!({
                "status": "completed",
                "summary": payload.get("why_it_likely_happened").cloned().unwrap_or(Value::Null),
                "research_sources": payload.get("research_sources").cloned().unwrap_or_else(|| json!([])),
                "related_existing_issues": payload.get("related_existing_issues").cloned().unwrap_or_else(|| json!([])),
                "related_failure_patterns": payload.get("related_failure_patterns").cloned().unwrap_or_else(|| json!([])),
                "file_references": payload.get("file_references").cloned().unwrap_or_else(|| json!([])),
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await?;
    }
    if !incident_monitor_completed_phase_artifact_exists(state, triage_run_id, "incident_monitor_validation")
    {
        write_incident_monitor_artifact(
            state,
            triage_run_id,
            &format!(
                "incident-monitor-validation-summary-{}",
                Uuid::new_v4().simple()
            ),
            "incident_monitor_validation",
            "artifacts/incident_monitor.validation.json",
            &json!({
                "status": "completed",
                "summary": payload.get("what_happened").cloned().unwrap_or(Value::Null),
                "failure_scope": payload.get("failure_type").cloned().unwrap_or(Value::Null),
                "steps_to_reproduce": payload.get("steps_to_reproduce").cloned().unwrap_or_else(|| json!([])),
                "logs": payload.get("logs").cloned().unwrap_or_else(|| json!([])),
                "evidence": payload.get("file_references").cloned().unwrap_or_else(|| json!([])),
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await?;
    }
    if !incident_monitor_completed_phase_artifact_exists(
        state,
        triage_run_id,
        "incident_monitor_fix_proposal",
    ) {
        write_incident_monitor_artifact(
            state,
            triage_run_id,
            &format!(
                "incident-monitor-fix-proposal-summary-{}",
                Uuid::new_v4().simple()
            ),
            "incident_monitor_fix_proposal",
            "artifacts/incident_monitor.fix_proposal.json",
            &json!({
                "status": "completed",
                "recommended_fix": payload.get("recommended_fix").cloned().unwrap_or(Value::Null),
                "acceptance_criteria": payload.get("acceptance_criteria").cloned().unwrap_or_else(|| json!([])),
                "verification_steps": payload.get("verification_steps").cloned().unwrap_or_else(|| json!([])),
                "risk_level": payload.get("risk_level").cloned().unwrap_or(Value::Null),
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await?;
    }
    Ok(())
}

async fn ensure_incident_monitor_approval_triage_summary_artifact(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
) -> Result<(), StatusCode> {
    let Some(triage_run_id) = draft.triage_run_id.as_deref() else {
        return Ok(());
    };
    if latest_incident_monitor_artifact(state, triage_run_id, "incident_monitor_triage_summary").is_some() {
        return Ok(());
    }
    write_incident_monitor_artifact(
        state,
        triage_run_id,
        &format!(
            "incident-monitor-approval-triage-summary-{}",
            Uuid::new_v4().simple()
        ),
        "incident_monitor_triage_summary",
        "artifacts/incident_monitor.triage_summary.json",
        &json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "triage_run_id": triage_run_id,
            "what_happened": draft.title,
            "notes": draft.detail,
            "created_at_ms": crate::now_ms(),
            "source": "incident_monitor_approval",
        }),
    )
    .await
}

fn incident_monitor_artifact_is_task_spec_placeholder(payload: &Value) -> bool {
    payload.get("expected_artifact").is_some()
        || payload.get("research_requirements").is_some()
        || payload.get("validation_requirements").is_some()
        || payload.get("proposal_requirements").is_some()
}

async fn persist_blocked_incident_monitor_report_observation(
    state: &AppState,
    report: &IncidentMonitorSubmission,
    repo: &str,
    detail: &str,
) -> Option<crate::IncidentMonitorIncidentRecord> {
    let repo = repo.trim();
    if repo.is_empty() {
        return None;
    }
    let config = state.incident_monitor_config().await;
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
        submission.expected_destination = Some("incident_monitor_issue_draft".to_string());
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
        .unwrap_or_else(|| "Blocked Incident Monitor signal".to_string());
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
        crate::incident_monitor::service::evaluate_incident_monitor_submission_quality(&submission);
    let now = crate::now_ms();
    let incident = crate::IncidentMonitorIncidentRecord {
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
            "Incident Monitor signal quality gate blocked draft creation.\n\nerror: {detail}"
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
        actor: submission.actor.clone(),
        model: submission.model.clone(),
        tool_name: submission.tool_name.clone(),
        action: submission.action.clone(),
        policy: submission.policy.clone(),
        approval_state: submission.approval_state.clone(),
        risk_category: submission.risk_category.clone(),
        blast_radius: submission.blast_radius.clone(),
        external_correlation_ids: submission.external_correlation_ids.clone(),
        expected_destination: submission.expected_destination.clone(),
        route_tags: submission.route_tags.clone(),
        allowed_destination_ids: submission.allowed_destination_ids.clone(),
        default_destination_ids: submission.default_destination_ids.clone(),
        tenant_id: submission.tenant_id.clone(),
        workspace_id: submission.workspace_id.clone(),
        event_schema_version: submission.event_schema_version.clone(),
        source_approval_policy: submission.source_approval_policy.clone(),
        redaction_profile: submission.redaction_profile.clone(),
        retention_profile: submission.retention_profile.clone(),
        evidence_refs: submission.evidence_refs.clone(),
        quality_gate: Some(quality_gate),
        duplicate_summary: None,
        duplicate_matches: None,
        event_payload: None,
        ..crate::IncidentMonitorIncidentRecord::default()
    };
    state.put_incident_monitor_incident(incident).await.ok()
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
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
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
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
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
        steps.push("Observe the error in the logs or Incident Monitor incident feed.".to_string());
    }
    steps.truncate(6);
    steps
}

fn derive_environment_lines(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
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
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
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

async fn load_incident_monitor_issue_template(config: &IncidentMonitorConfig) -> (String, Option<String>) {
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
        DEFAULT_INCIDENT_MONITOR_TEMPLATE.to_string(),
        Some("builtin:.github/ISSUE_TEMPLATE/bug_report.md".to_string()),
    )
}

fn render_incident_monitor_template(
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

fn derive_incident_monitor_failure_pattern_markers(
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

async fn incident_monitor_failure_recurrence_count(
    state: &AppState,
    repo: &str,
    fingerprint: &str,
) -> u64 {
    state
        .incident_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.repo == repo && row.fingerprint == fingerprint)
        .map(|row| row.occurrence_count.max(1))
        .sum::<u64>()
        .max(1)
}

async fn incident_monitor_max_occurrence_count_for_draft(
    state: &AppState,
    draft_id: &str,
) -> Option<u64> {
    state
        .incident_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.draft_id.as_deref() == Some(draft_id))
        .map(|row| row.occurrence_count.max(1))
        .max()
}

fn incident_monitor_linked_issue_numbers(draft: &IncidentMonitorDraftRecord) -> Vec<u64> {
    let mut linked = draft
        .issue_number
        .into_iter()
        .chain(draft.matched_issue_number)
        .collect::<Vec<_>>();
    linked.sort_unstable();
    linked.dedup();
    linked
}

async fn persist_incident_monitor_failure_pattern_memory(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
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
        .unwrap_or("Incident Monitor detected a failure that needs triage.")
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
    let canonical_markers = derive_incident_monitor_failure_pattern_markers(&summary_text, notes, &logs);
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
        &["incident_monitor".to_string(), "default".to_string()],
        &query_text,
        Some(&fingerprint),
        3,
        // Incident monitor is local/system-scoped (see local_implicit tenant).
        None,
    )
    .await?;
    let recurrence_count = if let Some(count) =
        incident_monitor_max_occurrence_count_for_draft(state, &draft.draft_id).await
    {
        count
    } else {
        incident_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await
    };
    let linked_issue_numbers = incident_monitor_linked_issue_numbers(draft);
    if duplicate_matches.iter().any(|row| {
        row.get("source").and_then(Value::as_str) == Some("governed_memory")
            && row.get("match_reason").and_then(Value::as_str) == Some("exact_fingerprint")
    }) {
        let duplicate_summary = build_incident_monitor_duplicate_summary(&duplicate_matches);
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
        Some("incident_monitor"),
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
        "source": "incident_monitor",
    });
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        state,
        &tenant_context,
        MemoryPutRequest {
            private: false,
            run_id: triage_run_id.to_string(),
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content: summary_text.clone(),
            artifact_refs: vec![summary_artifact_path.to_string()],
            classification: MemoryClassification::Internal,
            authority_job_context: None,
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
        "duplicate_summary": build_incident_monitor_duplicate_summary(&duplicate_matches),
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

async fn persist_incident_monitor_regression_signal_memory(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    triage_run_id: &str,
    triage_summary: &Value,
    summary_artifact_path: &str,
) -> Result<Value, StatusCode> {
    let what_happened = triage_summary
        .get("what_happened")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Incident Monitor detected a post-failure regression signal.");
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
        incident_monitor_max_occurrence_count_for_draft(state, &draft.draft_id).await
    {
        count
    } else {
        incident_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await
    };
    let linked_issue_numbers = incident_monitor_linked_issue_numbers(draft);
    let partition = MemoryPartition {
        org_id: draft.repo.clone(),
        workspace_id: draft.repo.clone(),
        project_id: draft.repo.clone(),
        tier: GovernedMemoryTier::Session,
    };
    let capability = Some(super::skills_memory::issue_run_memory_capability(
        triage_run_id,
        Some("incident_monitor"),
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
        "source": "incident_monitor",
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
            private: false,
            run_id: triage_run_id.to_string(),
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content,
            artifact_refs: vec![summary_artifact_path.to_string()],
            classification: MemoryClassification::Internal,
            authority_job_context: None,
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

#[cfg(test)]
mod incident_monitor_phase_artifact_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn completed_phase_payload_accepts_wrapped_inspection_handoff() {
        let payload = json!({
            "incident_monitor_inspection": {
                "status": "completed",
                "search_queries_used": [{"tool": "grep", "query": "stale_no_provider_activity"}],
                "tool_evidence": {"grep": "no exact match"}
            },
            "status": "completed"
        });

        assert!(
            incident_monitor_phase_artifact_payload_candidates(&payload, "incident_monitor_inspection")
                .into_iter()
                .any(|candidate| incident_monitor_completed_phase_payload_exists(
                    candidate,
                    "incident_monitor_inspection"
                ))
        );
    }

    #[test]
    fn completed_phase_payload_accepts_array_validation_handoff() {
        let payload = json!([
            {
                "artifact_kind": "incident_monitor_validation",
                "status": "completed",
                "failure_scope": {
                    "confirmed": true,
                    "summary": "structured handoff was missing"
                },
                "validation_decision": {
                    "scope_confirmed": true
                }
            },
            {
                "status": "completed",
                "approved": true
            }
        ]);

        assert!(
            incident_monitor_phase_artifact_payload_candidates(&payload, "incident_monitor_validation")
                .into_iter()
                .any(|candidate| incident_monitor_completed_phase_payload_exists(
                    candidate,
                    "incident_monitor_validation"
                ))
        );
    }

    #[test]
    fn completed_phase_payload_rejects_task_spec_placeholder() {
        let payload = json!({
            "expected_artifact": "incident_monitor_fix_proposal",
            "proposal_requirements": {
                "recommended_fix": true
            }
        });

        assert!(!incident_monitor_phase_artifact_payload_candidates(
            &payload,
            "incident_monitor_fix_proposal"
        )
        .into_iter()
        .any(|candidate| incident_monitor_completed_phase_payload_exists(
            candidate,
            "incident_monitor_fix_proposal"
        )));
    }
}
