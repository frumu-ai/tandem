// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Debug, Clone, PartialEq, Eq)]
enum DerivedTerminalRunState {
    Completed,
    Blocked {
        blocked_nodes: Vec<String>,
        detail: String,
    },
    Failed {
        failed_nodes: Vec<String>,
        blocked_nodes: Vec<String>,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionDeliverableRepair {
    node_id: String,
    path: String,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionDeliverableState {
    Satisfied,
    Repair(CompletionDeliverableRepair),
    Failed { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionDeliverableRequirement {
    path: String,
    owner_node_id: Option<String>,
}

fn completion_output_target_looks_like_file(path: &str) -> bool {
    let path = path.trim();
    let explicit_file_uri = path.starts_with("file://");
    let trimmed = path
        .strip_prefix("file://")
        .unwrap_or(path)
        .trim();
    if trimmed.is_empty() || trimmed.contains("://") {
        return false;
    }
    if explicit_file_uri {
        return true;
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return true;
    }
    let extension = std::path::Path::new(trimmed)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    matches!(
        extension.as_deref(),
        Some(
            "md" | "markdown"
                | "txt"
                | "json"
                | "jsonl"
                | "yaml"
                | "yml"
                | "csv"
                | "tsv"
                | "toml"
                | "ini"
                | "cfg"
                | "conf"
                | "env"
                | "xml"
                | "html"
                | "htm"
                | "sql"
                | "rs"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "swift"
                | "rb"
                | "php"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "hpp"
                | "cs"
                | "sh"
                | "css"
                | "scss"
                | "vue"
                | "svelte"
                | "pdf"
                | "docx"
                | "xlsx"
                | "pptx"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "svg"
                | "webp"
                | "zip"
                | "tar"
                | "gz"
        )
    )
}

fn automation_run_needs_initial_output_cleanup(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> bool {
    run.checkpoint.node_attempts.is_empty()
        && run.checkpoint.node_outputs.is_empty()
        && run.checkpoint.completed_nodes.is_empty()
        && run.checkpoint.gate_history.is_empty()
}

fn completion_artifact_is_substantive(path: &str, text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("artifact is empty".to_string());
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "json" => {
            let parsed: Value = serde_json::from_str(trimmed)
                .map_err(|error| format!("artifact is not valid JSON: {error}"))?;
            let substantive = match &parsed {
                Value::Object(map) => !map.is_empty(),
                Value::Array(items) => !items.is_empty(),
                Value::String(value) => !value.trim().is_empty(),
                Value::Number(_) | Value::Bool(_) => true,
                Value::Null => false,
            };
            if substantive {
                Ok(())
            } else {
                Err("JSON artifact has no substantive value".to_string())
            }
        }
        "md" | "markdown" | "html" | "htm" | "txt" => {
            if crate::app::state::automation::structural_substantive_artifact_text(trimmed) {
                Ok(())
            } else {
                Err("artifact is not substantive enough for completion".to_string())
            }
        }
        _ => Ok(()),
    }
}

fn completion_node_was_skipped(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: &str,
) -> bool {
    run.checkpoint
        .node_outputs
        .get(node_id)
        .and_then(|output| output.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("skipped"))
}

fn completion_required_deliverables(
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> Vec<CompletionDeliverableRequirement> {
    let started_at_ms = run.started_at_ms.unwrap_or(run.created_at_ms);
    let runtime_values = crate::app::state::automation::automation_prompt_runtime_values(Some(
        started_at_ms,
    ));
    let mut requirements = Vec::new();
    let mut owner_by_path = std::collections::HashMap::<String, String>::new();
    for node in &automation.flow.nodes {
        if let Some(path) = crate::app::state::automation::automation_effective_required_output_path_for_run(
            automation,
            node,
            &run.run_id,
            started_at_ms,
        ) {
            owner_by_path.insert(
                normalize_completion_path_for_compare(&path),
                node.node_id.clone(),
            );
            if !completion_node_was_skipped(run, &node.node_id) {
                requirements.push(CompletionDeliverableRequirement {
                    path,
                    owner_node_id: Some(node.node_id.clone()),
                });
            }
        }
        if let Some(path) = node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.pointer("/artifact/path"))
            .and_then(Value::as_str)
            .map(|path| {
                crate::app::state::automation::automation_runtime_placeholder_replace(
                    path,
                    Some(&runtime_values),
                )
            })
        {
            let normalized = normalize_completion_path_for_compare(&path);
            if !normalized.is_empty() {
                owner_by_path.insert(normalized, node.node_id.clone());
            }
        }
    }
    for target in &automation.output_targets {
        let path = crate::app::state::automation::automation_runtime_placeholder_replace(
            target,
            Some(&runtime_values),
        );
        let path = path.trim().to_string();
        if path.is_empty() || !completion_output_target_looks_like_file(&path) {
            continue;
        }
        let path = path
            .strip_prefix("file://")
            .unwrap_or(&path)
            .trim()
            .to_string();
        let owner_node_id = owner_by_path
            .get(&normalize_completion_path_for_compare(&path))
            .cloned();
        if owner_node_id
            .as_deref()
            .is_some_and(|node_id| completion_node_was_skipped(run, node_id))
        {
            continue;
        }
        requirements.push(CompletionDeliverableRequirement {
            path,
            owner_node_id,
        });
    }
    let mut seen = std::collections::HashSet::new();
    requirements
        .into_iter()
        .filter(|requirement| seen.insert(requirement.path.clone()))
        .collect()
}

fn repairable_completion_owner(
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    owner_node_id: Option<&str>,
) -> Option<String> {
    let owner_node_id = owner_node_id?;
    let node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == owner_node_id)?;
    let attempts = run
        .checkpoint
        .node_attempts
        .get(owner_node_id)
        .copied()
        .unwrap_or(0);
    if attempts < crate::app::state::automation_node_max_attempts(node) {
        Some(owner_node_id.to_string())
    } else {
        None
    }
}

fn completion_node_tool_telemetry<'a>(output: &'a Value) -> Option<&'a Value> {
    output
        .get("tool_telemetry")
        .or_else(|| output.pointer("/attempt_evidence/tool_telemetry"))
}

fn completion_node_email_delivery_succeeded(output: &Value) -> bool {
    let telemetry = completion_node_tool_telemetry(output);
    telemetry
        .and_then(|value| value.get("email_delivery_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || telemetry
            .and_then(|value| value.get("delivery_status"))
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status.trim().to_ascii_lowercase().as_str(), "sent" | "drafted" | "succeeded" | "success"))
        || output
            .pointer("/attempt_evidence/delivery/status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.trim().eq_ignore_ascii_case("succeeded"))
}

fn completion_external_action_status_succeeded(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "posted" | "sent" | "succeeded" | "success" | "executed" | "completed"
    )
}

fn completion_node_external_action_succeeded(output: &Value) -> bool {
    let Some(actions) = output.get("external_actions").and_then(Value::as_array) else {
        return false;
    };
    actions.iter().any(|action| {
        let status_ok = action
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(completion_external_action_status_succeeded);
        let approval_ok = action
            .get("approval_state")
            .and_then(Value::as_str)
            .map(|state| state.trim().eq_ignore_ascii_case("executed"))
            .unwrap_or(true);
        let has_error = action.get("error").is_some_and(|error| {
            !error.is_null()
                && error
                    .as_str()
                    .map(str::trim)
                    .map(|value| !value.is_empty())
                    .unwrap_or(true)
        });
        status_ok && approval_ok && !has_error
    })
}

fn completion_side_effect_missing_state(
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node: &crate::AutomationFlowNode,
    path: impl Into<String>,
    detail: String,
) -> CompletionDeliverableState {
    if let Some(node_id) = repairable_completion_owner(automation, run, Some(&node.node_id)) {
        return CompletionDeliverableState::Repair(CompletionDeliverableRepair {
            node_id,
            path: path.into(),
            detail,
        });
    }
    CompletionDeliverableState::Failed { detail }
}

fn assert_completion_side_effects(
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> CompletionDeliverableState {
    for node in &automation.flow.nodes {
        let Some(output) = run.checkpoint.node_outputs.get(&node.node_id) else {
            continue;
        };
        if crate::app::state::automation_node_requires_email_delivery(node) {
            if completion_node_email_delivery_succeeded(output) {
                continue;
            }
            let target = crate::app::state::automation_node_delivery_target(node)
                .unwrap_or_else(|| "requested recipient".to_string());
            let detail = format!(
                "automation run missing successful email delivery receipt for node `{}` to `{}`",
                node.node_id, target
            );
            return completion_side_effect_missing_state(
                automation,
                run,
                node,
                "email_delivery",
                detail,
            );
        }
        if !crate::app::state::automation_node_is_outbound_action(node) {
            continue;
        }
        if completion_node_external_action_succeeded(output) {
            continue;
        }
        let detail = format!(
            "automation run missing successful external action receipt for outbound node `{}`",
            node.node_id
        );
        return completion_side_effect_missing_state(
            automation,
            run,
            node,
            "external_action_receipt",
            detail,
        );
    }
    CompletionDeliverableState::Satisfied
}

fn normalize_completion_path_for_compare(path: &str) -> String {
    let trimmed = path.trim().strip_prefix("file://").unwrap_or(path.trim());
    crate::app::state::automation::normalize_automation_path_text(trimmed)
        .unwrap_or_else(|| trimmed.to_string())
        .trim_start_matches("./")
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn completion_output_value_references_path(value: &Value, target: &str) -> bool {
    if let Some(text) = value.as_str() {
        return normalize_completion_path_for_compare(text) == target;
    }
    if let Some(object) = value.as_object() {
        for key in ["path", "output_path", "source_artifact_path", "published_path", "target_path"] {
            if object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|path| normalize_completion_path_for_compare(path) == target)
            {
                return true;
            }
        }
        return object
            .values()
            .any(|value| completion_output_value_references_path(value, target));
    }
    if let Some(array) = value.as_array() {
        return array
            .iter()
            .any(|value| completion_output_value_references_path(value, target));
    }
    false
}

fn completion_unowned_output_target_has_run_evidence(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    path: &str,
) -> bool {
    let target = normalize_completion_path_for_compare(path);
    run.checkpoint
        .node_outputs
        .values()
        .any(|output| completion_output_value_references_path(output, &target))
}

fn assert_completion_deliverables(
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    workspace_root: &str,
) -> CompletionDeliverableState {
    match assert_completion_side_effects(automation, run) {
        CompletionDeliverableState::Satisfied => {}
        state => return state,
    }
    for requirement in completion_required_deliverables(automation, run) {
        if requirement.owner_node_id.is_none()
            && !completion_unowned_output_target_has_run_evidence(run, &requirement.path)
        {
            return CompletionDeliverableState::Failed {
                detail: format!(
                    "automation run deliverable `{}` lacks current-run output evidence",
                    requirement.path
                ),
            };
        }
        let resolved = match crate::app::state::automation::resolve_automation_output_path(
            workspace_root,
            &requirement.path,
        ) {
            Ok(path) => path,
            Err(error) => {
                return CompletionDeliverableState::Failed {
                    detail: format!(
                        "automation run missing required deliverable `{}`: {}",
                        requirement.path, error
                    ),
                };
            }
        };
        let text = match std::fs::read_to_string(&resolved) {
            Ok(text) => text,
            Err(_) => {
                let detail = format!(
                    "automation run missing required deliverable `{}`",
                    requirement.path
                );
                if let Some(node_id) = repairable_completion_owner(
                    automation,
                    run,
                    requirement.owner_node_id.as_deref(),
                ) {
                    return CompletionDeliverableState::Repair(CompletionDeliverableRepair {
                        node_id,
                        path: requirement.path,
                        detail,
                    });
                }
                return CompletionDeliverableState::Failed { detail };
            }
        };
        if let Err(reason) = completion_artifact_is_substantive(&requirement.path, &text) {
            let detail = format!(
                "automation run deliverable `{}` failed completion assertion: {}",
                requirement.path, reason
            );
            if let Some(node_id) = repairable_completion_owner(
                automation,
                run,
                requirement.owner_node_id.as_deref(),
            ) {
                return CompletionDeliverableState::Repair(CompletionDeliverableRepair {
                    node_id,
                    path: requirement.path,
                    detail,
                });
            }
            return CompletionDeliverableState::Failed { detail };
        }
    }
    CompletionDeliverableState::Satisfied
}

fn requeue_completion_deliverable_repair(
    row: &mut crate::automation_v2::types::AutomationV2RunRecord,
    repair: &CompletionDeliverableRepair,
) {
    row.checkpoint.completed_nodes.retain(|id| id != &repair.node_id);
    row.checkpoint.blocked_nodes.retain(|id| id != &repair.node_id);
    if !row
        .checkpoint
        .pending_nodes
        .iter()
        .any(|id| id == &repair.node_id)
    {
        row.checkpoint.pending_nodes.push(repair.node_id.clone());
    }
    row.checkpoint.node_outputs.insert(
        repair.node_id.clone(),
        json!({
            "status": "needs_repair",
            "failure_kind": "missing_required_artifact_path",
            "summary": repair.detail,
            "blocked_reason": repair.detail,
            "artifact_validation": {
                "validation_outcome": "needs_repair",
                "rejected_artifact_reason": repair.detail,
                "missing_workspace_files": [repair.path],
                "required_next_tool_actions": [format!("Write `{}` before completing the automation run.", repair.path)],
                "repair_exhausted": false
            }
        }),
    );
    row.checkpoint.last_failure = Some(crate::automation_v2::types::AutomationFailureRecord {
        node_id: repair.node_id.clone(),
        reason: repair.detail.clone(),
        failed_at_ms: now_ms(),
        failure_kind: Some("missing_required_artifact_path".to_string()),
        metadata: None,
    });
    row.status = AutomationRunStatus::Running;
    row.detail = Some(repair.detail.clone());
    crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
        row,
        "run_completion_repair_requested",
        Some(repair.detail.clone()),
        None,
        Some(json!({
            "node_id": repair.node_id,
            "path": repair.path,
            "reason": repair.detail,
        })),
    );
}

fn node_output_status(value: &Value) -> String {
    value
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn node_output_failure_kind(value: &Value) -> String {
    value
        .get("failure_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn output_only_failed_for_missing_materialized_artifact(value: &Value) -> bool {
    let unmet_requirements = value
        .pointer("/artifact_validation/unmet_requirements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let unmet_is_missing_output_only = unmet_requirements.is_empty()
        || unmet_requirements.iter().all(|item| {
            matches!(
                item.as_str(),
                Some("current_attempt_output_missing") | Some("structured_handoff_missing")
            )
        });
    if !unmet_is_missing_output_only {
        return false;
    }
    let blocked_reason = value
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let rejected_reason = value
        .pointer("/artifact_validation/rejected_artifact_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    blocked_reason.contains("explicit status or validated output")
        || blocked_reason.contains("required output `")
        || rejected_reason.contains("required output `")
}

fn run_node_is_settled_completed(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: &str,
) -> bool {
    run.checkpoint
        .completed_nodes
        .iter()
        .any(|id| id == node_id)
        || run
            .checkpoint
            .node_outputs
            .get(node_id)
            .is_some_and(|output| {
                let status = node_output_status(output);
                !matches!(
                    status.as_str(),
                    "needs_repair" | "blocked" | "failed" | "verify_failed"
                ) && crate::app::state::automation_node_has_passing_artifact(
                    node_id,
                    &run.checkpoint,
                )
            })
}

fn automation_failure_is_provider_stream_related(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    lowered.contains("provider stream chunk error")
        || lowered.contains("stream chunk error")
        || lowered.contains("error decoding response body")
        || lowered.contains("unexpected eof")
        || lowered.contains("incomplete streamed response")
}

fn strings_from_json_array(value: Option<&Value>, max_items: usize) -> Vec<String> {
    let mut rows = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    rows.truncate(max_items);
    rows
}

fn dedupe_strings(rows: &mut Vec<String>) {
    rows.sort();
    rows.dedup();
}

fn lifecycle_missing_workspace_paths(metadata: &Value) -> Vec<String> {
    metadata
        .get("must_write_file_statuses")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|item| {
                    item.get("materialized_by_current_attempt")
                        .and_then(Value::as_bool)
                        != Some(true)
                })
                .filter_map(|item| item.get("path").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn output_missing_workspace_paths(output: Option<&Value>) -> Vec<String> {
    let Some(output) = output else {
        return Vec::new();
    };
    let mut paths = strings_from_json_array(
        output
            .pointer("/artifact_validation/missing_workspace_files")
            .or_else(|| output.pointer("/validator_summary/missing_workspace_files")),
        20,
    );
    paths.extend(lifecycle_missing_workspace_paths(
        output.get("artifact_validation").unwrap_or(&Value::Null),
    ));
    dedupe_strings(&mut paths);
    paths
}

fn output_required_next_tool_actions(output: Option<&Value>) -> Vec<String> {
    let mut actions = strings_from_json_array(
        output.and_then(|row| {
            row.pointer("/artifact_validation/required_next_tool_actions")
                .or_else(|| row.pointer("/validator_summary/required_next_tool_actions"))
        }),
        20,
    );
    dedupe_strings(&mut actions);
    actions
}

fn evidence_string_array(evidence: &[Value], field: &str) -> Vec<String> {
    let mut rows = Vec::new();
    for item in evidence {
        rows.extend(strings_from_json_array(item.get(field), 20));
    }
    dedupe_strings(&mut rows);
    rows
}

fn recent_node_attempt_evidence(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: Option<&str>,
) -> Vec<Value> {
    let Some(node_id) = node_id else {
        return Vec::new();
    };
    let mut evidence = Vec::new();
    for record in run.checkpoint.lifecycle_history.iter().rev() {
        let Some(metadata) = record.metadata.as_ref() else {
            continue;
        };
        if metadata.get("node_id").and_then(Value::as_str) != Some(node_id) {
            continue;
        }
        let unmet_requirements = metadata
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let missing_workspace_files = lifecycle_missing_workspace_paths(metadata);
        let required_next_tool_actions = metadata
            .get("required_next_tool_actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let rejected_artifact_reason = metadata
            .get("rejected_artifact_reason")
            .and_then(Value::as_str);
        let useful = !unmet_requirements.is_empty()
            || !missing_workspace_files.is_empty()
            || !required_next_tool_actions.is_empty()
            || rejected_artifact_reason.is_some();
        if !useful {
            continue;
        }
        evidence.push(json!({
            "event": record.event,
            "recorded_at_ms": record.recorded_at_ms,
            "reason": record.reason,
            "attempt": metadata.get("attempt").cloned().unwrap_or(Value::Null),
            "unmet_requirements": unmet_requirements,
            "missing_workspace_files": missing_workspace_files,
            "required_next_tool_actions": required_next_tool_actions,
            "rejected_artifact_reason": rejected_artifact_reason,
            "summary": metadata.get("summary").cloned().unwrap_or(Value::Null),
        }));
        if evidence.len() >= 5 {
            break;
        }
    }
    evidence.reverse();
    evidence
}

fn recent_node_attempt_verdicts(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: Option<&str>,
) -> Vec<Value> {
    let Some(node_id) = node_id else {
        return Vec::new();
    };
    let mut verdicts = run
        .checkpoint
        .node_attempt_verdicts
        .get(node_id)
        .cloned()
        .unwrap_or_default();
    if let Some(latest) = run
        .checkpoint
        .node_outputs
        .get(node_id)
        .and_then(|output| output.get("attempt_verdict"))
        .cloned()
    {
        verdicts.push(latest);
    }
    verdicts.sort_by(|left, right| {
        let left_attempt = left.get("attempt").and_then(Value::as_u64).unwrap_or(0);
        let right_attempt = right.get("attempt").and_then(Value::as_u64).unwrap_or(0);
        left_attempt.cmp(&right_attempt)
    });
    verdicts.dedup_by(|left, right| {
        left.get("attempt") == right.get("attempt")
            && left.get("failure_class") == right.get("failure_class")
            && left.get("validation_reason") == right.get("validation_reason")
    });
    let keep_from = verdicts.len().saturating_sub(5);
    verdicts.into_iter().skip(keep_from).collect()
}

fn validation_errors_with_prior_evidence(current: Value, evidence: &[Value]) -> Value {
    let mut rows = current.as_array().cloned().unwrap_or_default();
    for item in evidence {
        if let Some(unmet) = item.get("unmet_requirements").and_then(Value::as_array) {
            rows.extend(unmet.iter().cloned());
        }
        if let Some(paths) = item
            .get("missing_workspace_files")
            .and_then(Value::as_array)
        {
            for path in paths.iter().filter_map(Value::as_str) {
                rows.push(json!(format!(
                    "required workspace file `{}` was not written in a prior attempt",
                    path
                )));
            }
        }
    }
    rows.sort_by(|left, right| left.to_string().cmp(&right.to_string()));
    rows.dedup();
    Value::Array(rows)
}
