use super::*;
use sha2::{Digest, Sha256};

pub(crate) fn workflow_learning_project_id(automation: &AutomationV2Spec) -> String {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("project_id")
                .or_else(|| metadata.get("project_slug"))
                .or_else(|| metadata.get("workspace_root"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            automation
                .workspace_root
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| automation.automation_id.clone())
}

pub(crate) fn workflow_learning_has_plan_bundle(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("plan_package_bundle")
                .or_else(|| metadata.get("plan_package"))
        })
        .is_some()
}

fn workflow_learning_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

fn workflow_learning_normalize_text(raw: &str) -> String {
    raw.to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn workflow_learning_validator_family(node: Option<&AutomationFlowNode>) -> Option<String> {
    node.map(|candidate| {
        format!("{:?}", automation_output_validator_kind(candidate)).to_ascii_lowercase()
    })
}

fn workflow_learning_node_kind(node: Option<&AutomationFlowNode>) -> Option<String> {
    node.and_then(|candidate| {
        candidate
            .stage_kind
            .as_ref()
            .map(|kind| format!("{kind:?}").to_ascii_lowercase())
    })
}

fn workflow_learning_run_artifact_refs(run: &AutomationV2RunRecord) -> Vec<String> {
    let mut refs = Vec::new();
    for output in run.checkpoint.node_outputs.values() {
        if let Some(path) = output.get("path").and_then(Value::as_str) {
            if !path.trim().is_empty() && !refs.iter().any(|existing| existing == path) {
                refs.push(path.to_string());
            }
        }
        if let Some(path) = output
            .get("artifact_validation")
            .and_then(Value::as_object)
            .and_then(|artifact| artifact.get("path"))
            .and_then(Value::as_str)
        {
            if !path.trim().is_empty() && !refs.iter().any(|existing| existing == path) {
                refs.push(path.to_string());
            }
        }
    }
    refs
}

fn workflow_learning_forensic_receipt_kind_for_path(path: &str) -> Option<&'static str> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else if trimmed.ends_with(".jsonl") {
        Some("receipt_ledger")
    } else if trimmed.ends_with(".json") {
        Some("forensic_record")
    } else {
        None
    }
}

fn workflow_learning_push_forensic_receipt_ref(
    refs: &mut Vec<Value>,
    kind: &str,
    path: &str,
    details: Option<&serde_json::Map<String, Value>>,
) {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return;
    }
    if refs.iter().any(|existing| {
        existing.get("kind").and_then(Value::as_str) == Some(kind)
            && existing.get("path").and_then(Value::as_str) == Some(trimmed)
    }) {
        return;
    }
    let mut entry = serde_json::Map::new();
    entry.insert("kind".to_string(), json!(kind));
    entry.insert("path".to_string(), json!(trimmed));
    if let Some(details) = details {
        for key in ["seq", "record_count", "attempt", "event_type"] {
            if let Some(value) = details.get(key).cloned() {
                entry.insert(key.to_string(), value);
            }
        }
    }
    refs.push(Value::Object(entry));
}

fn workflow_learning_forensic_receipt_refs(output: &Value) -> Vec<Value> {
    let mut refs = Vec::new();
    if let Some(attempt_evidence) = output.get("attempt_evidence").and_then(Value::as_object) {
        if let Some(ledger) = attempt_evidence
            .get("receipt_ledger")
            .and_then(Value::as_object)
        {
            if let Some(path) = ledger.get("path").and_then(Value::as_str) {
                workflow_learning_push_forensic_receipt_ref(
                    &mut refs,
                    "receipt_ledger",
                    path,
                    Some(ledger),
                );
            }
        }
        for (key, kind) in [
            ("forensic_record_path", "forensic_record"),
            ("standup_receipt_path", "standup_receipt"),
        ] {
            if let Some(path) = attempt_evidence.get(key).and_then(Value::as_str) {
                workflow_learning_push_forensic_receipt_ref(&mut refs, kind, path, None);
            }
        }
    }
    if let Some(timeline) = output.get("receipt_timeline").and_then(Value::as_array) {
        for item in timeline {
            let Some(object) = item.as_object() else {
                continue;
            };
            for key in [
                "receipt_path",
                "forensic_record_path",
                "standup_receipt_path",
                "path",
            ] {
                let Some(path) = object.get(key).and_then(Value::as_str) else {
                    continue;
                };
                let kind = match key {
                    "receipt_path" => Some("receipt_ledger"),
                    "forensic_record_path" => Some("forensic_record"),
                    "standup_receipt_path" => Some("standup_receipt"),
                    _ => workflow_learning_forensic_receipt_kind_for_path(path),
                };
                if let Some(kind) = kind {
                    workflow_learning_push_forensic_receipt_ref(
                        &mut refs,
                        kind,
                        path,
                        Some(object),
                    );
                }
            }
        }
    }
    refs
}

fn workflow_learning_validator_failure_summary(output: &Value) -> Option<Value> {
    let validator_summary = output.get("validator_summary").and_then(Value::as_object)?;
    let mut summary = serde_json::Map::new();
    for key in [
        "outcome",
        "reason",
        "verification_outcome",
        "accepted_candidate_source",
    ] {
        if let Some(value) = validator_summary.get(key).cloned() {
            summary.insert(key.to_string(), value);
        }
    }
    for key in [
        "unmet_requirements",
        "warning_requirements",
        "validation_basis",
    ] {
        if let Some(value) = validator_summary.get(key).cloned() {
            summary.insert(key.to_string(), value);
        }
    }
    if let Some(value) = validator_summary.get("warning_count").cloned() {
        summary.insert("warning_count".to_string(), value);
    }
    if summary.is_empty() {
        None
    } else {
        Some(Value::Object(summary))
    }
}

fn workflow_learning_repair_state(output: &Value) -> Option<Value> {
    let validator_summary = output.get("validator_summary").and_then(Value::as_object);
    let artifact_validation = output.get("artifact_validation").and_then(Value::as_object);
    let mut state = serde_json::Map::new();
    for key in [
        "repair_attempted",
        "repair_attempt",
        "repair_attempts_remaining",
        "repair_succeeded",
        "repair_exhausted",
    ] {
        if let Some(value) = validator_summary
            .and_then(|summary| summary.get(key))
            .cloned()
            .or_else(|| {
                artifact_validation
                    .and_then(|summary| summary.get(key))
                    .cloned()
            })
        {
            state.insert(key.to_string(), value);
        }
    }
    if let Some(value) = artifact_validation
        .and_then(|artifact| artifact.get("rejected_artifact_reason"))
        .cloned()
    {
        state.insert("rejected_artifact_reason".to_string(), value);
    }
    if let Some(value) = validator_summary
        .and_then(|summary| summary.get("verification_outcome"))
        .cloned()
        .or_else(|| {
            artifact_validation
                .and_then(|summary| summary.get("verification_outcome"))
                .cloned()
        })
    {
        state.insert("verification_outcome".to_string(), value);
    }
    if state.is_empty() {
        None
    } else {
        Some(Value::Object(state))
    }
}

fn workflow_learning_artifact_validation_summary(output: &Value) -> Option<Value> {
    let artifact_validation = output
        .get("artifact_validation")
        .and_then(Value::as_object)?;
    let mut summary = serde_json::Map::new();
    for key in [
        "path",
        "status",
        "rejected_artifact_reason",
        "verification_outcome",
        "verification_expected",
        "verification_ran",
        "verification_failed",
    ] {
        if let Some(value) = artifact_validation.get(key).cloned() {
            summary.insert(key.to_string(), value);
        }
    }
    for key in ["unmet_requirements", "warning_requirements"] {
        if let Some(value) = artifact_validation.get(key).cloned() {
            summary.insert(key.to_string(), value);
        }
    }
    if let Some(value) = artifact_validation.get("warning_count").cloned() {
        summary.insert("warning_count".to_string(), value);
    }
    if summary.is_empty() {
        None
    } else {
        Some(Value::Object(summary))
    }
}

fn workflow_learning_output_evidence(output: &Value) -> Value {
    let mut evidence = serde_json::Map::new();
    for key in [
        "status",
        "summary",
        "text",
        "path",
        "blocked_reason",
        "failure_kind",
        "blocker_category",
        "phase",
    ] {
        if let Some(value) = output.get(key).cloned() {
            evidence.insert(key.to_string(), value);
        }
    }
    if let Some(value) = output.get("attempt_evidence").cloned() {
        evidence.insert("attempt_evidence".to_string(), value);
    }
    if let Some(value) = output.get("receipt_timeline").cloned() {
        evidence.insert("receipt_timeline".to_string(), value);
    }
    if let Some(value) = output.get("validator_summary").cloned() {
        evidence.insert("validator_summary".to_string(), value);
    }
    if let Some(value) = output.get("artifact_validation").cloned() {
        evidence.insert("artifact_validation".to_string(), value);
    }
    if let Some(value) = workflow_learning_validator_failure_summary(output) {
        evidence.insert("validator_failure".to_string(), value);
    }
    if let Some(value) = workflow_learning_repair_state(output) {
        evidence.insert("repair_state".to_string(), value);
    }
    if let Some(value) = workflow_learning_artifact_validation_summary(output) {
        evidence.insert("artifact_validation_summary".to_string(), value);
    }
    let forensic_receipts = workflow_learning_forensic_receipt_refs(output);
    if !forensic_receipts.is_empty() {
        evidence.insert(
            "forensic_receipts".to_string(),
            Value::Array(forensic_receipts),
        );
    }
    Value::Object(evidence)
}

fn workflow_learning_run_duration_ms(run: &AutomationV2RunRecord) -> u64 {
    match (run.started_at_ms, run.finished_at_ms) {
        (Some(started), Some(finished)) if finished >= started => finished - started,
        _ => 0,
    }
}

fn workflow_learning_validation_pass(run: &AutomationV2RunRecord) -> bool {
    if !matches!(run.status, AutomationRunStatus::Completed) {
        return false;
    }
    !run.checkpoint.node_outputs.values().any(|output| {
        output
            .get("validator_summary")
            .and_then(Value::as_object)
            .and_then(|summary| summary.get("outcome"))
            .and_then(Value::as_str)
            .map(|value| matches!(value.to_ascii_lowercase().as_str(), "failed" | "blocked"))
            .unwrap_or(false)
    })
}

pub(crate) fn workflow_learning_metrics_snapshot(
    runs: &[AutomationV2RunRecord],
) -> WorkflowLearningMetricsSnapshot {
    let terminal_runs = runs
        .iter()
        .filter(|run| {
            matches!(
                run.status,
                AutomationRunStatus::Completed
                    | AutomationRunStatus::Blocked
                    | AutomationRunStatus::Failed
                    | AutomationRunStatus::Cancelled
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if terminal_runs.is_empty() {
        return WorkflowLearningMetricsSnapshot {
            computed_at_ms: now_ms(),
            ..WorkflowLearningMetricsSnapshot::default()
        };
    }

    let sample_size = terminal_runs.len();
    let completed = terminal_runs
        .iter()
        .filter(|run| run.status == AutomationRunStatus::Completed)
        .count();
    let validation_passes = terminal_runs
        .iter()
        .filter(|run| workflow_learning_validation_pass(run))
        .count();
    let repairable_failures = terminal_runs
        .iter()
        .filter(|run| {
            run.checkpoint.node_outputs.values().any(|output| {
                output
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status.eq_ignore_ascii_case("needs_repair"))
            })
        })
        .count();
    let total_attempts = terminal_runs
        .iter()
        .map(|run| {
            run.checkpoint
                .node_attempts
                .values()
                .copied()
                .map(u64::from)
                .sum::<u64>()
        })
        .sum::<u64>();
    let total_nodes = terminal_runs
        .iter()
        .map(|run| run.checkpoint.node_attempts.len() as u64)
        .sum::<u64>()
        .max(1);
    let human_intervention_count = terminal_runs
        .iter()
        .filter(|run| {
            !run.checkpoint.gate_history.is_empty()
                || matches!(run.stop_kind, Some(AutomationStopKind::OperatorStopped))
        })
        .count() as u64;
    let mut durations = terminal_runs
        .iter()
        .map(workflow_learning_run_duration_ms)
        .collect::<Vec<_>>();
    durations.sort_unstable();
    let median_wall_clock_ms = durations[durations.len() / 2];

    WorkflowLearningMetricsSnapshot {
        sample_size,
        completion_rate: completed as f64 / sample_size as f64,
        validation_pass_rate: validation_passes as f64 / sample_size as f64,
        mean_attempts_per_node: total_attempts as f64 / total_nodes as f64,
        repairable_failure_rate: repairable_failures as f64 / sample_size as f64,
        median_wall_clock_ms,
        human_intervention_count,
        computed_at_ms: now_ms(),
    }
}

fn workflow_learning_completed_summary(run: &AutomationV2RunRecord) -> Option<String> {
    let mut parts = run
        .checkpoint
        .node_outputs
        .iter()
        .filter_map(|(node_id, output)| {
            let status = output.get("status").and_then(Value::as_str)?;
            if !status.eq_ignore_ascii_case("completed") {
                return None;
            }
            let summary = output
                .get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    output
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| value.len() > 24)
                })?;
            Some(format!("{node_id}: {summary}"))
        })
        .take(2)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        if let Some(detail) = run
            .detail
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            parts.push(detail.to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

fn workflow_learning_failure_signature(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<Value>,
)> {
    let failure = run.checkpoint.last_failure.as_ref()?;
    let node = automation
        .flow
        .nodes
        .iter()
        .find(|candidate| candidate.node_id == failure.node_id);
    let normalized_reason = workflow_learning_normalize_text(&failure.reason);
    if normalized_reason.is_empty() {
        return None;
    }
    let validator_family = workflow_learning_validator_family(node);
    let node_kind = workflow_learning_node_kind(node);
    let fingerprint = workflow_learning_hash(&[
        &automation.automation_id,
        failure.node_id.as_str(),
        validator_family.as_deref().unwrap_or("unknown"),
        normalized_reason.as_str(),
    ]);
    let summary = format!(
        "Repeated failure at `{}`: {}",
        failure.node_id,
        failure.reason.trim()
    );
    let evidence = vec![json!({
        "run_id": run.run_id,
        "run_status": run.status,
        "detail": run.detail,
        "node_id": failure.node_id,
        "reason": failure.reason,
        "failed_at_ms": failure.failed_at_ms,
        "node_attempts": run
            .checkpoint
            .node_attempts
            .get(&failure.node_id)
            .copied()
            .unwrap_or(0),
        "validator_family": validator_family,
        "node_kind": node_kind,
        "node_output": run
            .checkpoint
            .node_outputs
            .get(&failure.node_id)
            .map(workflow_learning_output_evidence)
            .unwrap_or(Value::Null),
        "artifact_refs": workflow_learning_run_artifact_refs(run),
    })];
    Some((
        fingerprint,
        summary,
        Some(failure.node_id.clone()),
        node_kind,
        validator_family,
        evidence,
    ))
}

pub(crate) fn workflow_learning_context_for_candidates(
    candidates: &[WorkflowLearningCandidate],
) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    lines.push("<learning_context>".to_string());
    lines.push("Apply the following approved workflow learnings when useful:".to_string());
    for candidate in candidates.iter().take(6) {
        lines.push(format!(
            "- [{}] {}",
            match candidate.kind {
                WorkflowLearningCandidateKind::MemoryFact => "memory_fact",
                WorkflowLearningCandidateKind::RepairHint => "repair_hint",
                WorkflowLearningCandidateKind::PromptPatch => "prompt_patch",
                WorkflowLearningCandidateKind::GraphPatch => "graph_patch",
            },
            candidate.summary.trim()
        ));
    }
    lines.push("</learning_context>".to_string());
    Some(lines.join("\n"))
}

pub(crate) fn workflow_learning_candidates_for_terminal_run(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    prior_runs: &[AutomationV2RunRecord],
    existing_candidates: &[WorkflowLearningCandidate],
) -> Vec<WorkflowLearningCandidate> {
    let project_id = workflow_learning_project_id(automation);
    let artifact_refs = workflow_learning_run_artifact_refs(run);
    let mut candidates = Vec::new();
    let now = now_ms();

    if run.status == AutomationRunStatus::Completed {
        if let Some(summary) = workflow_learning_completed_summary(run) {
            let normalized = workflow_learning_normalize_text(&summary);
            if !normalized.is_empty() {
                candidates.push(WorkflowLearningCandidate {
                    candidate_id: format!("wflearn-{}", uuid::Uuid::new_v4()),
                    workflow_id: automation.automation_id.clone(),
                    project_id: project_id.clone(),
                    source_run_id: run.run_id.clone(),
                    kind: WorkflowLearningCandidateKind::MemoryFact,
                    status: WorkflowLearningCandidateStatus::Proposed,
                    confidence: 0.65,
                    summary: summary.clone(),
                    fingerprint: workflow_learning_hash(&[
                        &automation.automation_id,
                        "memory_fact",
                        &normalized,
                    ]),
                    node_id: None,
                    node_kind: None,
                    validator_family: None,
                    evidence_refs: vec![json!({
                        "run_id": run.run_id,
                        "status": run.status,
                        "detail": run.detail,
                        "completed_nodes": run.checkpoint.completed_nodes,
                        "artifact_refs": artifact_refs,
                    })],
                    artifact_refs: artifact_refs.clone(),
                    proposed_memory_payload: Some(json!({
                        "content": summary,
                        "classification": "working_memory",
                    })),
                    proposed_revision_prompt: None,
                    source_memory_id: None,
                    promoted_memory_id: None,
                    needs_plan_bundle: false,
                    baseline_before: None,
                    latest_observed_metrics: None,
                    last_revision_session_id: None,
                    run_ids: vec![run.run_id.clone()],
                    created_at_ms: now,
                    updated_at_ms: now,
                });
            }
        }
    }

    if let Some((fingerprint, summary, node_id, node_kind, validator_family, evidence_refs)) =
        workflow_learning_failure_signature(automation, run)
    {
        let matching_failure_runs = prior_runs
            .iter()
            .filter_map(|candidate_run| {
                workflow_learning_failure_signature(automation, candidate_run).map(
                    |(candidate_fingerprint, _, _, _, _, _)| {
                        (candidate_run.run_id.clone(), candidate_fingerprint)
                    },
                )
            })
            .filter(|(_, candidate_fingerprint)| candidate_fingerprint == &fingerprint)
            .map(|(run_id, _)| run_id)
            .collect::<Vec<_>>();
        let failure_run_count = prior_runs
            .iter()
            .filter_map(|candidate_run| {
                workflow_learning_failure_signature(automation, candidate_run)
            })
            .filter(|(candidate_fingerprint, _, _, _, _, _)| candidate_fingerprint == &fingerprint)
            .count();
        if failure_run_count >= 2 {
            candidates.push(WorkflowLearningCandidate {
                candidate_id: format!("wflearn-{}", uuid::Uuid::new_v4()),
                workflow_id: automation.automation_id.clone(),
                project_id: project_id.clone(),
                source_run_id: run.run_id.clone(),
                kind: WorkflowLearningCandidateKind::RepairHint,
                status: WorkflowLearningCandidateStatus::Proposed,
                confidence: 0.7,
                summary: format!("{summary}. Prefer tighter repair instructions before retrying."),
                fingerprint: workflow_learning_hash(&[fingerprint.as_str(), "repair_hint"]),
                node_id: node_id.clone(),
                node_kind: node_kind.clone(),
                validator_family: validator_family.clone(),
                evidence_refs: evidence_refs.clone(),
                artifact_refs: artifact_refs.clone(),
                proposed_memory_payload: Some(json!({
                    "content": summary,
                    "kind": "repair_hint",
                })),
                proposed_revision_prompt: None,
                source_memory_id: None,
                promoted_memory_id: None,
                needs_plan_bundle: false,
                baseline_before: None,
                latest_observed_metrics: None,
                last_revision_session_id: None,
                run_ids: vec![run.run_id.clone()],
                created_at_ms: now,
                updated_at_ms: now,
            });
            candidates.push(WorkflowLearningCandidate {
                candidate_id: format!("wflearn-{}", uuid::Uuid::new_v4()),
                workflow_id: automation.automation_id.clone(),
                project_id: project_id.clone(),
                source_run_id: run.run_id.clone(),
                kind: WorkflowLearningCandidateKind::PromptPatch,
                status: WorkflowLearningCandidateStatus::Proposed,
                confidence: 0.75,
                summary: format!("{summary}. Revise the node prompt and repair guidance."),
                fingerprint: workflow_learning_hash(&[fingerprint.as_str(), "prompt_patch"]),
                node_id: node_id.clone(),
                node_kind: node_kind.clone(),
                validator_family: validator_family.clone(),
                evidence_refs: evidence_refs.clone(),
                artifact_refs: artifact_refs.clone(),
                proposed_memory_payload: None,
                proposed_revision_prompt: Some(format!(
                    "Repeated failure fingerprint `{}` for workflow `{}`.\n\
                     Requested change type: prompt_patch.\n\
                     Preserve validated parts of the existing workflow.\n\
                     Last affected run IDs: {}.\n\
                     Evidence: {}.",
                    fingerprint,
                    automation.automation_id,
                    matching_failure_runs.join(", "),
                    serde_json::to_string(&evidence_refs).unwrap_or_default()
                )),
                source_memory_id: None,
                promoted_memory_id: None,
                needs_plan_bundle: false,
                baseline_before: None,
                latest_observed_metrics: None,
                last_revision_session_id: None,
                run_ids: vec![run.run_id.clone()],
                created_at_ms: now,
                updated_at_ms: now,
            });
        }
        if failure_run_count >= 2 {
            let needs_plan_bundle = !workflow_learning_has_plan_bundle(automation);
            let prompt_patch_approved_and_still_failing =
                existing_candidates.iter().any(|candidate| {
                    candidate.kind == WorkflowLearningCandidateKind::PromptPatch
                        && candidate.status == WorkflowLearningCandidateStatus::Approved
                        && candidate.workflow_id == automation.automation_id
                        && candidate.node_id.as_deref() == node_id.as_deref()
                });
            if failure_run_count >= 3
                || (failure_run_count >= 2 && prompt_patch_approved_and_still_failing)
            {
                candidates.push(WorkflowLearningCandidate {
                    candidate_id: format!("wflearn-{}", uuid::Uuid::new_v4()),
                    workflow_id: automation.automation_id.clone(),
                    project_id,
                    source_run_id: run.run_id.clone(),
                    kind: WorkflowLearningCandidateKind::GraphPatch,
                    status: WorkflowLearningCandidateStatus::Proposed,
                    confidence: 0.8,
                    summary: format!("{summary}. This may need a workflow graph change."),
                    fingerprint: workflow_learning_hash(&[fingerprint.as_str(), "graph_patch"]),
                    node_id,
                    node_kind,
                    validator_family,
                    evidence_refs,
                    artifact_refs,
                    proposed_memory_payload: None,
                    proposed_revision_prompt: Some(format!(
                        "Repeated failure fingerprint `{}` for workflow `{}`.\n\
                         Requested change type: graph_patch.\n\
                         Preserve validated parts of the existing workflow.\n\
                         Consider restructuring node dependencies, splitting the node, or changing gating/validation boundaries.",
                        fingerprint,
                        automation.automation_id
                    )),
                    source_memory_id: None,
                    promoted_memory_id: None,
                    needs_plan_bundle,
                    baseline_before: None,
                    latest_observed_metrics: None,
                    last_revision_session_id: None,
                    run_ids: vec![run.run_id.clone()],
                    created_at_ms: now,
                    updated_at_ms: now,
                });
            }
        }
    }

    candidates
}
