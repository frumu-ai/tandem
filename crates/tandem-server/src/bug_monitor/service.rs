use anyhow::Result;
use serde_json::{Map, Value};

use crate::bug_monitor::types::BugMonitorIncidentRecord;
use crate::bug_monitor::types::{
    BugMonitorConfig, BugMonitorQualityGateReport, BugMonitorQualityGateResult,
    BugMonitorSubmission,
};
use crate::EngineEvent;
use crate::{
    app::state::{sha256_hex, truncate_text, AppState},
    now_ms,
};

fn bug_monitor_triage_timeout_deadline_ms(created_at_ms: u64, timeout_ms: u64) -> u64 {
    created_at_ms.saturating_add(timeout_ms)
}

/// Build a multi-line `last_post_error` describing why the triage run
/// missed its deadline. The first line is the original short message
/// (preserving backwards compat for any consumer that reads the first
/// line). Subsequent lines are the structured diagnostics, suitable
/// for embedding in the GitHub issue body's "Triage timeout details"
/// section. When diagnostics could not be loaded (run state missing
/// or corrupt), the message degrades gracefully to the single-line
/// pre-diagnostics format.
fn compose_triage_timeout_last_post_error(
    triage_run_id: &str,
    timeout_ms: u64,
    diagnostics: Option<&serde_json::Value>,
) -> String {
    let head =
        format!("triage run {triage_run_id} did not reach a terminal status within {timeout_ms}ms");
    match diagnostics {
        Some(value) => {
            let detail =
                crate::http::context_runs::format_bug_monitor_triage_timeout_diagnostics(value);
            if detail.trim().is_empty() {
                head
            } else {
                format!("{head}\n{detail}")
            }
        }
        None => head,
    }
}

async fn bug_monitor_incident_for_draft(
    state: &AppState,
    draft_id: &str,
    triage_run_id: &str,
) -> Option<String> {
    let incidents = state.bug_monitor_incidents.read().await;
    incidents
        .values()
        .find(|incident| {
            incident.draft_id.as_deref() == Some(draft_id)
                || incident.triage_run_id.as_deref() == Some(triage_run_id)
        })
        .map(|incident| incident.incident_id.clone())
}

pub async fn recover_overdue_bug_monitor_triage_runs(
    state: &AppState,
) -> anyhow::Result<Vec<(String, Option<String>)>> {
    let config = state.bug_monitor_config().await;
    let Some(timeout_ms) = config.triage_timeout_ms else {
        return Ok(Vec::new());
    };
    if !config.enabled || config.paused {
        return Ok(Vec::new());
    }

    let now = now_ms();
    let drafts = {
        let guard = state.bug_monitor_drafts.read().await;
        guard.values().cloned().collect::<Vec<_>>()
    };

    let mut recovered = Vec::new();
    for draft in drafts {
        let Some(triage_run_id) = draft.triage_run_id.clone() else {
            continue;
        };
        if draft.issue_number.is_some() || draft.github_issue_url.is_some() {
            continue;
        }
        if draft
            .github_status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("triage_timed_out"))
        {
            continue;
        }

        let run_created_at_ms =
            crate::http::bug_monitor::bug_monitor_triage_effective_started_at_ms(
                state,
                &triage_run_id,
            )
            .await
            .unwrap_or(draft.created_at_ms);
        if now < bug_monitor_triage_timeout_deadline_ms(run_created_at_ms, timeout_ms) {
            continue;
        }

        let diagnostics_value = crate::http::bug_monitor::bug_monitor_triage_timeout_diagnostics(
            state,
            &triage_run_id,
            timeout_ms,
        )
        .await;
        let last_post_error = compose_triage_timeout_last_post_error(
            &triage_run_id,
            timeout_ms,
            diagnostics_value.as_ref(),
        );
        // Atomic CAS: only the caller that actually flips github_status
        // to triage_timed_out continues into the publish path. A second
        // concurrent recover_overdue invocation reading the same
        // not-yet-timed-out draft will see `Ok(None)` here and skip the
        // publish — closing the race that produced duplicate GitHub
        // issues (#45 / #46) when two status pollers fire near
        // simultaneously. A persistence failure surfaces as `Err` and
        // is propagated via `?` so we don't publish without a durable
        // marker (which would re-publish on restart and create a
        // duplicate).
        let Some(current_draft) = state
            .try_mark_triage_timed_out(&draft.draft_id, last_post_error.clone())
            .await?
        else {
            continue;
        };

        if let Some(incident_id) =
            bug_monitor_incident_for_draft(state, &current_draft.draft_id, &triage_run_id).await
        {
            if let Some(mut incident) = state.get_bug_monitor_incident(&incident_id).await {
                incident.status = "triage_timed_out".to_string();
                incident.last_error = Some(last_post_error.clone());
                incident.updated_at_ms = now;
                state.put_bug_monitor_incident(incident.clone()).await?;
                let mut event_payload = serde_json::json!({
                    "incident_id": incident.incident_id,
                    "draft_id": current_draft.draft_id,
                    "triage_run_id": triage_run_id,
                    "timeout_ms": timeout_ms,
                });
                if let Some(diagnostics) = diagnostics_value.as_ref() {
                    if let Some(obj) = event_payload.as_object_mut() {
                        obj.insert("diagnostics".to_string(), diagnostics.clone());
                    }
                }
                state.event_bus.publish(EngineEvent::new(
                    "bug_monitor.incident.triage_timed_out",
                    event_payload,
                ));
            }
        }

        recovered.push((current_draft.draft_id.clone(), Some(triage_run_id)));
    }

    Ok(recovered)
}

pub async fn collect_bug_monitor_excerpt(state: &AppState, properties: &Value) -> Vec<String> {
    let mut excerpt = Vec::new();
    if let Some(reason) = first_string(properties, &["reason", "error", "detail", "message"]) {
        excerpt.push(reason);
    }
    if let Some(title) = first_string(properties, &["title", "task"]) {
        if !excerpt.iter().any(|row| row == &title) {
            excerpt.push(title);
        }
    }
    let logs = state.logs.read().await;
    for entry in logs.iter().rev().take(3) {
        if let Some(message) = entry.get("message").and_then(|row| row.as_str()) {
            excerpt.push(truncate_text(message, 240));
        }
    }
    excerpt.truncate(8);
    excerpt
}

fn is_non_empty(value: &Option<String>) -> bool {
    value
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn event_is_routine_noise(event: Option<&str>) -> bool {
    let normalized = event.unwrap_or_default().trim().to_ascii_lowercase();
    !normalized.is_empty()
        && [
            "progress",
            "heartbeat",
            "started",
            "queued",
            "retrying",
            "attempt.started",
            "minor_retry",
        ]
        .iter()
        .any(|term| normalized.contains(term))
}

pub fn evaluate_bug_monitor_submission_quality(
    submission: &BugMonitorSubmission,
) -> BugMonitorQualityGateReport {
    let source_known = is_non_empty(&submission.source)
        || is_non_empty(&submission.component)
        || is_non_empty(&submission.process)
        || is_non_empty(&submission.event);
    let type_classified = is_non_empty(&submission.event) || is_non_empty(&submission.level);
    let confidence_recorded = is_non_empty(&submission.confidence);
    let dedupe_checked = is_non_empty(&submission.fingerprint);
    let evidence_exists = !submission.evidence_refs.is_empty()
        || !submission.excerpt.is_empty()
        || is_non_empty(&submission.detail)
        || is_non_empty(&submission.file_name);
    let destination_clear = is_non_empty(&submission.expected_destination);
    let risk_known = is_non_empty(&submission.risk_level);
    let not_routine_noise = !event_is_routine_noise(submission.event.as_deref());

    let gate_specs = [
        (
            "source_known",
            "Source known",
            source_known,
            submission
                .source
                .clone()
                .or_else(|| submission.component.clone())
                .or_else(|| submission.process.clone())
                .or_else(|| submission.event.clone()),
        ),
        (
            "type_classified",
            "Signal type classified",
            type_classified,
            submission
                .event
                .clone()
                .or_else(|| submission.level.clone()),
        ),
        (
            "confidence_recorded",
            "Confidence recorded",
            confidence_recorded,
            submission.confidence.clone(),
        ),
        (
            "dedupe_checked",
            "Dedupe/fingerprint checked",
            dedupe_checked,
            submission.fingerprint.clone(),
        ),
        (
            "evidence_present",
            "Evidence or artifact refs present",
            evidence_exists,
            submission
                .evidence_refs
                .first()
                .cloned()
                .or_else(|| submission.excerpt.first().cloned())
                .or_else(|| submission.file_name.clone()),
        ),
        (
            "destination_clear",
            "Expected destination clear",
            destination_clear,
            submission.expected_destination.clone(),
        ),
        (
            "risk_known",
            "Risk level known",
            risk_known,
            submission.risk_level.clone(),
        ),
        (
            "not_routine_noise",
            "Not routine progress or minor retry",
            not_routine_noise,
            submission.event.clone(),
        ),
    ];

    let gates = gate_specs
        .into_iter()
        .map(|(key, label, passed, detail)| BugMonitorQualityGateResult {
            key: key.to_string(),
            label: label.to_string(),
            passed,
            detail,
        })
        .collect::<Vec<_>>();
    let passed_count = gates.iter().filter(|gate| gate.passed).count();
    let missing = gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.key.clone())
        .collect::<Vec<_>>();
    let passed = passed_count == gates.len();
    BugMonitorQualityGateReport {
        stage: "intake_to_draft".to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        passed,
        passed_count,
        total_count: gates.len(),
        blocked_reason: if passed {
            None
        } else {
            Some(format!("missing quality gates: {}", missing.join(", ")))
        },
        gates,
        missing,
    }
}

pub async fn process_event(
    state: &AppState,
    event: &EngineEvent,
    config: &BugMonitorConfig,
) -> anyhow::Result<BugMonitorIncidentRecord> {
    let submission = build_bug_monitor_submission_from_event(state, config, event).await?;
    let duplicate_matches = crate::http::bug_monitor::bug_monitor_failure_pattern_matches(
        state,
        submission.repo.as_deref().unwrap_or_default(),
        submission.fingerprint.as_deref().unwrap_or_default(),
        submission.title.as_deref(),
        submission.detail.as_deref(),
        &submission.excerpt,
        3,
    )
    .await;
    let fingerprint = submission
        .fingerprint
        .clone()
        .ok_or_else(|| anyhow::anyhow!("bug monitor submission fingerprint missing"))?;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .unwrap_or(default_workspace_root);
    let now = crate::util::time::now_ms();
    let quality_gate = evaluate_bug_monitor_submission_quality(&submission);

    let existing = state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .find(|row| row.fingerprint == fingerprint)
        .cloned();

    let mut incident = if let Some(mut row) = existing {
        row.occurrence_count = row.occurrence_count.saturating_add(1);
        row.updated_at_ms = now;
        row.last_seen_at_ms = Some(now);
        if row.excerpt.is_empty() {
            row.excerpt = submission.excerpt.clone();
        }
        if row.confidence.is_none() {
            row.confidence = submission.confidence.clone();
        }
        if row.risk_level.is_none() {
            row.risk_level = submission.risk_level.clone();
        }
        if row.expected_destination.is_none() {
            row.expected_destination = submission.expected_destination.clone();
        }
        row.quality_gate = Some(quality_gate.clone());
        for evidence_ref in &submission.evidence_refs {
            if !row
                .evidence_refs
                .iter()
                .any(|existing| existing == evidence_ref)
            {
                row.evidence_refs.push(evidence_ref.clone());
            }
        }
        row
    } else {
        BugMonitorIncidentRecord {
            incident_id: format!("failure-incident-{}", uuid::Uuid::new_v4().simple()),
            fingerprint: fingerprint.clone(),
            event_type: event.event_type.clone(),
            status: "queued".to_string(),
            repo: submission.repo.clone().unwrap_or_default(),
            workspace_root,
            title: submission
                .title
                .clone()
                .unwrap_or_else(|| format!("Failure detected in {}", event.event_type)),
            detail: submission.detail.clone(),
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
            last_error: None,
            confidence: submission.confidence.clone(),
            risk_level: submission.risk_level.clone(),
            expected_destination: submission.expected_destination.clone(),
            evidence_refs: submission.evidence_refs.clone(),
            quality_gate: Some(quality_gate.clone()),
            duplicate_summary: None,
            duplicate_matches: None,
            event_payload: Some(event.properties.clone()),
        }
    };
    state.put_bug_monitor_incident(incident.clone()).await?;

    if !duplicate_matches.is_empty() {
        incident.status = "duplicate_suppressed".to_string();
        let duplicate_summary =
            crate::http::bug_monitor::build_bug_monitor_duplicate_summary(&duplicate_matches);
        incident.duplicate_summary = Some(duplicate_summary.clone());
        incident.duplicate_matches = Some(duplicate_matches.clone());
        incident.updated_at_ms = crate::util::time::now_ms();
        state.put_bug_monitor_incident(incident.clone()).await?;
        state.event_bus.publish(EngineEvent::new(
            "bug_monitor.incident.duplicate_suppressed",
            serde_json::json!({
                "incident_id": incident.incident_id,
                "fingerprint": incident.fingerprint,
                "eventType": incident.event_type,
                "status": incident.status,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
            }),
        ));
        return Ok(incident);
    }

    let draft = match state.submit_bug_monitor_draft(submission).await {
        Ok(draft) => draft,
        Err(error) => {
            incident.status = "draft_failed".to_string();
            incident.last_error = Some(truncate_text(&error.to_string(), 500));
            incident.updated_at_ms = crate::util::time::now_ms();
            state.put_bug_monitor_incident(incident.clone()).await?;
            state.event_bus.publish(EngineEvent::new(
                "bug_monitor.incident.detected",
                serde_json::json!({
                    "incident_id": incident.incident_id,
                    "fingerprint": incident.fingerprint,
                    "eventType": incident.event_type,
                    "draft_id": incident.draft_id,
                    "triage_run_id": incident.triage_run_id,
                    "status": incident.status,
                    "detail": incident.last_error,
                }),
            ));
            return Ok(incident);
        }
    };
    incident.draft_id = Some(draft.draft_id.clone());
    incident.status = "draft_created".to_string();
    state.put_bug_monitor_incident(incident.clone()).await?;

    match crate::http::bug_monitor::ensure_bug_monitor_triage_run(
        state.clone(),
        &draft.draft_id,
        true,
    )
    .await
    {
        Ok((updated_draft, _run_id, _deduped)) => {
            incident.triage_run_id = updated_draft.triage_run_id.clone();
            if incident.triage_run_id.is_some() {
                incident.status = "triage_queued".to_string();
            }
            incident.last_error = None;
        }
        Err(error) => {
            incident.status = "draft_created".to_string();
            incident.last_error = Some(truncate_text(&error.to_string(), 500));
        }
    }

    if let Some(draft_id) = incident.draft_id.clone() {
        let latest_draft = state
            .get_bug_monitor_draft(&draft_id)
            .await
            .unwrap_or(draft.clone());
        match crate::bug_monitor_github::publish_draft(
            state,
            &draft_id,
            Some(&incident.incident_id),
            crate::bug_monitor_github::PublishMode::Auto,
        )
        .await
        {
            Ok(outcome) => {
                incident.status = outcome.action;
                incident.last_error = None;
            }
            Err(error) => {
                let detail = truncate_text(&error.to_string(), 500);
                incident.last_error = Some(detail.clone());
                let mut failed_draft = latest_draft;
                failed_draft.status = "github_post_failed".to_string();
                failed_draft.github_status = Some("github_post_failed".to_string());
                failed_draft.last_post_error = Some(detail.clone());
                let evidence_digest = failed_draft.evidence_digest.clone();
                if let Err(persist_err) = state.put_bug_monitor_draft(failed_draft.clone()).await {
                    tracing::warn!(
                        incident_id = %incident.incident_id,
                        draft_id = %failed_draft.draft_id,
                        error = %persist_err,
                        "failed to persist bug monitor draft after auto-post failure",
                    );
                }
                if let Err(record_err) = crate::bug_monitor_github::record_post_failure(
                    state,
                    &failed_draft,
                    Some(&incident.incident_id),
                    "auto_post",
                    evidence_digest.as_deref(),
                    &detail,
                )
                .await
                {
                    tracing::warn!(
                        incident_id = %incident.incident_id,
                        draft_id = %failed_draft.draft_id,
                        error = %record_err,
                        "failed to record bug monitor post failure",
                    );
                }
            }
        }

        if let Some(triage_run_id) = incident.triage_run_id.clone() {
            if let Some(timeout_ms) = config.triage_timeout_ms {
                spawn_triage_deadline_task(
                    state.clone(),
                    incident.incident_id.clone(),
                    draft_id.clone(),
                    triage_run_id,
                    timeout_ms,
                );
            }
        }
    }

    incident.updated_at_ms = crate::util::time::now_ms();
    state.put_bug_monitor_incident(incident.clone()).await?;
    state.event_bus.publish(EngineEvent::new(
        "bug_monitor.incident.detected",
        serde_json::json!({
            "incident_id": incident.incident_id,
            "fingerprint": incident.fingerprint,
            "eventType": incident.event_type,
            "draft_id": incident.draft_id,
            "triage_run_id": incident.triage_run_id,
            "status": incident.status,
        }),
    ));
    Ok(incident)
}
pub fn first_string(properties: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = properties.get(*key).and_then(|row| row.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
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

fn first_value<'a>(properties: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| get_path_value(properties, key))
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

fn first_u64(properties: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(value) = get_path_value(properties, key) {
            if let Some(number) = value.as_u64() {
                return Some(number);
            }
            if let Some(text) = value.as_str() {
                if let Ok(number) = text.trim().parse::<u64>() {
                    return Some(number);
                }
            }
        }
    }
    None
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
                        if item.is_object() || item.is_array() {
                            Some(truncate_text(&sanitize_json_value(item).to_string(), 300))
                        } else {
                            None
                        }
                    })
            })
            .collect::<Vec<_>>(),
        Some(Value::String(text)) => text
            .lines()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        Some(value) if value.is_object() => {
            vec![truncate_text(&sanitize_json_value(value).to_string(), 300)]
        }
        _ => Vec::new(),
    };
    rows.truncate(max_items);
    rows
}

fn redacted_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("authorization")
        || normalized == "api_key"
        || normalized.ends_with("_key")
}

fn sanitize_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    if redacted_key(key) {
                        (key.clone(), Value::String("[redacted]".to_string()))
                    } else {
                        (key.clone(), sanitize_json_value(value))
                    }
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => {
            Value::Array(items.iter().take(40).map(sanitize_json_value).collect())
        }
        Value::String(text) => Value::String(truncate_text(text, 1_000)),
        _ => value.clone(),
    }
}

fn field_line(label: &str, value: Option<String>) -> String {
    format!("{label}: {}", value.unwrap_or_default())
}

pub async fn build_bug_monitor_submission_from_event(
    state: &AppState,
    config: &BugMonitorConfig,
    event: &EngineEvent,
) -> Result<BugMonitorSubmission> {
    let repo = config
        .repo
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor repo is not configured"))?;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .unwrap_or(default_workspace_root);
    let reason = first_string_deep(
        &event.properties,
        &[
            "reason",
            "error",
            "detail",
            "message",
            "summary",
            "task.last_error",
            "task.payload.reason",
            "task.payload.error",
        ],
    );
    let workflow_id = first_string_deep(&event.properties, &["workflow_id", "workflowID"]);
    let workflow_name = first_string_deep(&event.properties, &["workflow_name", "workflowName"]);
    let run_id = first_string_deep(&event.properties, &["run_id", "runID"]);
    let session_id = first_string_deep(&event.properties, &["session_id", "sessionID"]);
    let task_id = first_string_deep(&event.properties, &["task_id", "taskID", "task.id"]);
    let stage_id = first_string_deep(&event.properties, &["stage_id", "stageID", "actionID"]);
    let node_id = first_string_deep(&event.properties, &["node_id", "nodeID"]);
    let automation_id = first_string_deep(&event.properties, &["automation_id", "automationID"]);
    let routine_id = first_string_deep(&event.properties, &["routine_id", "routineID"]);
    let agent_role = first_string_deep(&event.properties, &["agent_role", "agentRole"]);
    let error_kind = first_string_deep(
        &event.properties,
        &["error_kind", "errorKind", "failure_kind", "failureKind"],
    );
    let tool_name = first_string_deep(&event.properties, &["tool_name", "toolName", "tool"]);
    let suggested_next_action = first_string_deep(
        &event.properties,
        &["suggested_next_action", "suggestedNextAction"],
    );
    let expected_output =
        first_string_deep(&event.properties, &["expected_output", "expectedOutput"]).or_else(
            || {
                first_value(&event.properties, &["output_contract", "outputContract"])
                    .map(|value| truncate_text(&sanitize_json_value(value).to_string(), 800))
            },
        );
    let actual_output = first_string_deep(&event.properties, &["actual_output", "actualOutput"]);
    let tool_args_summary =
        first_value(&event.properties, &["tool_args_summary", "toolArgsSummary"])
            .map(|value| truncate_text(&sanitize_json_value(value).to_string(), 800));
    let tool_result_excerpt = first_string_deep(
        &event.properties,
        &["tool_result_excerpt", "toolResultExcerpt"],
    );
    let attempt = first_u64(&event.properties, &["attempt", "task.attempt"]);
    let max_attempts = first_u64(
        &event.properties,
        &["max_attempts", "maxAttempts", "task.max_attempts"],
    );
    let retry_exhausted = first_value(&event.properties, &["retry_exhausted", "retryExhausted"])
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            attempt
                .zip(max_attempts)
                .map(|(attempt, max)| max > 0 && attempt >= max)
                .unwrap_or(false)
        });
    let files_touched = strings_from_value(
        first_value(
            &event.properties,
            &["files_touched", "filesTouched", "changed_files"],
        ),
        20,
    );
    let artifact_refs = strings_from_value(
        first_value(
            &event.properties,
            &["artifact_refs", "artifactRefs", "artifacts"],
        ),
        20,
    );
    let mut evidence_refs = artifact_refs.clone();
    for evidence_ref in strings_from_value(
        first_value(&event.properties, &["evidence_refs", "evidenceRefs"]),
        20,
    ) {
        if !evidence_refs
            .iter()
            .any(|existing| existing == &evidence_ref)
        {
            evidence_refs.push(evidence_ref);
        }
    }
    let validation_errors = strings_from_value(
        first_value(
            &event.properties,
            &["validation_errors", "validationErrors"],
        ),
        12,
    );
    let correlation_id = first_string_deep(
        &event.properties,
        &[
            "correlationID",
            "correlation_id",
            "commandID",
            "command_id",
            "eventID",
        ],
    );
    let component = first_string_deep(
        &event.properties,
        &[
            "component",
            "routine_id",
            "routineID",
            "workflow_id",
            "workflowID",
            "automation_id",
            "automationID",
            "node_id",
            "nodeID",
            "stage_id",
            "task",
            "title",
        ],
    );
    let confidence = first_string_deep(
        &event.properties,
        &["confidence", "signal_confidence", "signalConfidence"],
    )
    .map(|value| truncate_text(&value, 80))
    .or_else(|| Some("high".to_string()));
    let risk_level = first_string_deep(&event.properties, &["risk_level", "riskLevel", "risk"])
        .map(|value| truncate_text(&value, 80))
        .or_else(|| Some("medium".to_string()));
    let expected_destination = first_string_deep(
        &event.properties,
        &["expected_destination", "expectedDestination"],
    )
    .map(|value| truncate_text(&value, 120))
    .or_else(|| Some("bug_monitor_issue_draft".to_string()));
    let mut excerpt = collect_bug_monitor_excerpt(state, &event.properties).await;
    if excerpt.is_empty() {
        if let Some(reason) = reason.as_ref() {
            excerpt.push(reason.clone());
        }
    }
    let sanitized_properties = sanitize_json_value(&event.properties);
    let serialized = serde_json::to_string(&sanitized_properties).unwrap_or_default();
    let fingerprint = sha256_hex(&[
        repo.as_str(),
        workspace_root.as_str(),
        event.event_type.as_str(),
        reason.as_deref().unwrap_or(""),
        workflow_id.as_deref().unwrap_or(""),
        task_id.as_deref().unwrap_or(""),
        stage_id.as_deref().unwrap_or(""),
        node_id.as_deref().unwrap_or(""),
        run_id.as_deref().unwrap_or(""),
        session_id.as_deref().unwrap_or(""),
        correlation_id.as_deref().unwrap_or(""),
        component.as_deref().unwrap_or(""),
    ]);
    let failure_place = stage_id
        .as_ref()
        .or(node_id.as_ref())
        .or(task_id.as_ref())
        .or(component.as_ref());
    let title_reason = reason
        .as_deref()
        .map(|row| truncate_text(row, 120))
        .unwrap_or_else(|| event.event_type.clone());
    let title = if let Some(workflow_id) = workflow_id.as_ref().or(automation_id.as_ref()) {
        if let Some(place) = failure_place {
            format!("Workflow {workflow_id} failed at {place}: {title_reason}")
        } else {
            format!("Workflow {workflow_id} failed: {title_reason}")
        }
    } else if let Some(routine_id) = routine_id.as_ref() {
        format!("Routine {routine_id} failed: {title_reason}")
    } else if let Some(component) = component.as_ref() {
        format!(
            "{} failure in {}: {}",
            event.event_type, component, title_reason
        )
    } else {
        format!("{}: {}", event.event_type, title_reason)
    };
    let mut detail_lines = vec![
        format!("event_type: {}", event.event_type),
        format!("repo: {}", repo),
        format!("workspace_root: {}", workspace_root),
        field_line("workflow_id", workflow_id.clone().or(automation_id.clone())),
        field_line("workflow_name", workflow_name.clone()),
        field_line("run_id", run_id.clone()),
        field_line("session_id", session_id.clone()),
        field_line("task_id", task_id.clone()),
        field_line("stage_id", stage_id.clone()),
        field_line("node_id", node_id.clone()),
        field_line("component", component.clone()),
        field_line("agent_role", agent_role.clone()),
        field_line("attempt", attempt.map(|value| value.to_string())),
        field_line("max_attempts", max_attempts.map(|value| value.to_string())),
        format!("retry_exhausted: {retry_exhausted}"),
        field_line("confidence", confidence.clone()),
        field_line("risk_level", risk_level.clone()),
        field_line("expected_destination", expected_destination.clone()),
        field_line("error_kind", error_kind.clone()),
        field_line("reason", reason.clone()),
        String::new(),
        "expected_output:".to_string(),
        expected_output.unwrap_or_default(),
        String::new(),
        "actual_output:".to_string(),
        actual_output.unwrap_or_default(),
        String::new(),
        field_line("tool", tool_name.clone()),
        "tool_args_summary:".to_string(),
        tool_args_summary.unwrap_or_default(),
        "tool_result_excerpt:".to_string(),
        tool_result_excerpt.unwrap_or_default(),
        String::new(),
        "artifact_refs:".to_string(),
        if artifact_refs.is_empty() {
            String::new()
        } else {
            artifact_refs.join("\n")
        },
        "files_touched:".to_string(),
        if files_touched.is_empty() {
            String::new()
        } else {
            files_touched.join("\n")
        },
        "validation_errors:".to_string(),
        if validation_errors.is_empty() {
            String::new()
        } else {
            validation_errors.join("\n")
        },
        String::new(),
        "suggested_next_action:".to_string(),
        suggested_next_action.unwrap_or_default(),
    ];
    if !serialized.trim().is_empty() {
        detail_lines.push(String::new());
        detail_lines.push("payload:".to_string());
        detail_lines.push(truncate_text(&serialized, 4_000));
    }

    Ok(BugMonitorSubmission {
        repo: Some(repo),
        title: Some(title),
        detail: Some(detail_lines.join("\n")),
        source: Some(
            first_string_deep(&event.properties, &["source"]).unwrap_or_else(|| {
                match event.event_type.as_str() {
                    "automation_v2.run.failed" | "automation.run.failed" => "automation_v2",
                    "automation_v2.run.paused_stale_no_provider_activity" => "automation_v2",
                    "workflow.run.failed" | "workflow.validation.failed" => "autonomous_workflow",
                    "routine.run.failed" => "routine",
                    "context.task.failed" | "context.task.blocked" | "context.run.failed" => {
                        "context_run"
                    }
                    "coder.run.failed" => "coder",
                    _ => "tandem_events",
                }
                .to_string()
            }),
        ),
        run_id,
        session_id,
        correlation_id,
        file_name: files_touched.first().cloned(),
        process: Some("tandem-engine".to_string()),
        component,
        event: Some(event.event_type.clone()),
        level: Some("error".to_string()),
        excerpt,
        fingerprint: Some(fingerprint),
        confidence,
        risk_level,
        expected_destination,
        evidence_refs,
    })
}

/// Spawns a deadline task that fires after `timeout_ms`. If the triage
/// run has not reached a terminal status (`Failed` / `Completed` /
/// `Cancelled`) by the deadline, the task marks the draft as
/// `triage_timed_out`, persists it, then re-runs `publish_draft` in
/// `Auto` mode. The triage_run_id is preserved on the draft so the UI
/// can still link to the abandoned run.
fn spawn_triage_deadline_task(
    state: AppState,
    incident_id: String,
    draft_id: String,
    triage_run_id: String,
    timeout_ms: u64,
) {
    tokio::spawn(async move {
        if timeout_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
        }
        if crate::http::bug_monitor::bug_monitor_triage_run_is_terminal(&state, &triage_run_id)
            .await
        {
            return;
        }
        let now = crate::util::time::now_ms();
        let Some(mut draft) = state.get_bug_monitor_draft(&draft_id).await else {
            return;
        };
        let already_marked = draft
            .github_status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("triage_timed_out"));
        if already_marked {
            return;
        }
        if draft.github_issue_url.is_some() {
            return;
        }
        draft.github_status = Some("triage_timed_out".to_string());
        let diagnostics_value = crate::http::bug_monitor::bug_monitor_triage_timeout_diagnostics(
            &state,
            &triage_run_id,
            timeout_ms,
        )
        .await;
        let last_post_error = compose_triage_timeout_last_post_error(
            &triage_run_id,
            timeout_ms,
            diagnostics_value.as_ref(),
        );
        draft.last_post_error = Some(last_post_error.clone());
        if let Err(error) = state.put_bug_monitor_draft(draft.clone()).await {
            tracing::warn!(
                incident_id = %incident_id,
                draft_id = %draft_id,
                error = %error,
                "failed to persist bug monitor draft after triage deadline",
            );
            return;
        }
        if let Some(mut incident) = state.get_bug_monitor_incident(&incident_id).await {
            incident.status = "triage_timed_out".to_string();
            incident.last_error = Some(last_post_error.clone());
            incident.updated_at_ms = now;
            if let Err(error) = state.put_bug_monitor_incident(incident.clone()).await {
                tracing::warn!(
                    incident_id = %incident_id,
                    error = %error,
                    "failed to persist bug monitor incident after triage deadline",
                );
            }
            let mut event_payload = serde_json::json!({
                "incident_id": incident_id,
                "draft_id": draft_id,
                "triage_run_id": triage_run_id,
                "timeout_ms": timeout_ms,
            });
            if let Some(diagnostics) = diagnostics_value.as_ref() {
                if let Some(obj) = event_payload.as_object_mut() {
                    obj.insert("diagnostics".to_string(), diagnostics.clone());
                }
            }
            state.event_bus.publish(EngineEvent::new(
                "bug_monitor.incident.triage_timed_out",
                event_payload,
            ));
        }
        if let Err(error) = crate::bug_monitor_github::publish_draft(
            &state,
            &draft_id,
            Some(&incident_id),
            crate::bug_monitor_github::PublishMode::Recovery,
        )
        .await
        {
            tracing::warn!(
                incident_id = %incident_id,
                draft_id = %draft_id,
                triage_run_id = %triage_run_id,
                error = %error,
                "fallback publish after triage deadline failed",
            );
        }
    });
}
