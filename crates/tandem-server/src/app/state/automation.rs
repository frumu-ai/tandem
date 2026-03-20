use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::{json, Value};
use tandem_types::{
    MessagePart, MessagePartInput, MessageRole, ModelSpec, PrewriteCoverageMode,
    PrewriteRequirements, SendMessageRequest, Session, ToolMode,
};

use super::*;
use crate::capability_resolver::{self};
use crate::config::{self};
use crate::util::time::now_ms;
pub async fn run_automation_v2_scheduler(state: AppState) {
    crate::app::tasks::run_automation_v2_scheduler(state).await
}

fn build_automation_v2_upstream_inputs(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> anyhow::Result<Vec<Value>> {
    let mut inputs = Vec::new();
    for input_ref in &node.input_refs {
        let Some(output) = run.checkpoint.node_outputs.get(&input_ref.from_step_id) else {
            anyhow::bail!(
                "missing upstream output for `{}` referenced by node `{}`",
                input_ref.from_step_id,
                node.node_id
            );
        };
        inputs.push(json!({
            "alias": input_ref.alias,
            "from_step_id": input_ref.from_step_id,
            "output": normalize_upstream_research_output_paths(workspace_root, output),
        }));
    }
    Ok(inputs)
}

pub(crate) fn is_automation_approval_node(node: &AutomationFlowNode) -> bool {
    matches!(node.stage_kind, Some(AutomationNodeStageKind::Approval))
        || node
            .gate
            .as_ref()
            .map(|gate| gate.required)
            .unwrap_or(false)
}

pub(crate) fn automation_guardrail_failure(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<String> {
    if let Some(max_runtime_ms) = automation.execution.max_total_runtime_ms {
        if let Some(started_at_ms) = run.started_at_ms {
            let elapsed = now_ms().saturating_sub(started_at_ms);
            if elapsed >= max_runtime_ms {
                return Some(format!(
                    "run exceeded max_total_runtime_ms ({elapsed}/{max_runtime_ms})"
                ));
            }
        }
    }
    if let Some(max_total_tokens) = automation.execution.max_total_tokens {
        if run.total_tokens >= max_total_tokens {
            return Some(format!(
                "run exceeded max_total_tokens ({}/{})",
                run.total_tokens, max_total_tokens
            ));
        }
    }
    if let Some(max_total_cost_usd) = automation.execution.max_total_cost_usd {
        if run.estimated_cost_usd >= max_total_cost_usd {
            return Some(format!(
                "run exceeded max_total_cost_usd ({:.4}/{:.4})",
                run.estimated_cost_usd, max_total_cost_usd
            ));
        }
    }
    None
}

pub fn record_automation_lifecycle_event(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
) {
    record_automation_lifecycle_event_with_metadata(run, event, reason, stop_kind, None);
}

pub fn record_automation_lifecycle_event_with_metadata(
    run: &mut AutomationV2RunRecord,
    event: impl Into<String>,
    reason: Option<String>,
    stop_kind: Option<AutomationStopKind>,
    metadata: Option<Value>,
) {
    run.checkpoint
        .lifecycle_history
        .push(AutomationLifecycleRecord {
            event: event.into(),
            recorded_at_ms: now_ms(),
            reason,
            stop_kind,
            metadata,
        });
}

pub fn automation_lifecycle_event_metadata_for_node(
    node_id: &str,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
    workflow_class: &str,
    phase: &str,
    status: &str,
    failure_kind: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert("node_id".to_string(), json!(node_id));
    map.insert("attempt".to_string(), json!(attempt));
    map.insert("summary".to_string(), json!(summary));
    map.insert("contract_kind".to_string(), json!(contract_kind));
    map.insert("workflow_class".to_string(), json!(workflow_class));
    map.insert("phase".to_string(), json!(phase));
    map.insert("status".to_string(), json!(status));
    map.insert("event_contract_version".to_string(), json!(1));
    if let Some(value) = session_id.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("session_id".to_string(), json!(value));
    }
    if let Some(value) = failure_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("failure_kind".to_string(), json!(value));
    }
    map
}

pub fn record_automation_workflow_state_events(
    run: &mut AutomationV2RunRecord,
    node_id: &str,
    output: &Value,
    attempt: u32,
    session_id: Option<&str>,
    summary: &str,
    contract_kind: &str,
) {
    let workflow_class = output
        .get("workflow_class")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("artifact");
    let phase = output
        .get("phase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let failure_kind = output
        .get("failure_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let artifact_validation = output.get("artifact_validation");
    let base_reason = output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        });

    let mut base_metadata = automation_lifecycle_event_metadata_for_node(
        node_id,
        attempt,
        session_id,
        summary,
        contract_kind,
        workflow_class,
        phase,
        status,
        failure_kind,
    );
    if let Some(classification) = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert("blocking_classification".to_string(), json!(classification));
    }
    if let Some(actions) = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        base_metadata.insert(
            "required_next_tool_actions".to_string(),
            Value::Array(actions.clone()),
        );
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "workflow_state_changed",
        base_reason.clone(),
        None,
        Some(Value::Object(base_metadata.clone())),
    );

    if let Some(candidates) = artifact_validation
        .and_then(|value| value.get("artifact_candidates"))
        .and_then(Value::as_array)
    {
        for candidate in candidates {
            let mut metadata = base_metadata.clone();
            metadata.insert("candidate".to_string(), candidate.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "artifact_candidate_written",
                None,
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(source) = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("accepted_candidate_source".to_string(), json!(source));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_accepted",
            None,
            None,
            Some(Value::Object(metadata)),
        );
    }

    if let Some(reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut metadata = base_metadata.clone();
        metadata.insert("rejected_artifact_reason".to_string(), json!(reason));
        record_automation_lifecycle_event_with_metadata(
            run,
            "artifact_rejected",
            Some(reason.to_string()),
            None,
            Some(Value::Object(metadata)),
        );
    }

    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if repair_attempted {
        let mut metadata = base_metadata.clone();
        metadata.insert("repair_attempt".to_string(), json!(repair_attempt));
        metadata.insert(
            "repair_attempts_remaining".to_string(),
            json!(repair_attempts_remaining),
        );
        metadata.insert("repair_succeeded".to_string(), json!(repair_succeeded));
        metadata.insert("repair_exhausted".to_string(), json!(repair_exhausted));
        record_automation_lifecycle_event_with_metadata(
            run,
            "repair_started",
            None,
            None,
            Some(Value::Object(metadata.clone())),
        );
        if !repair_succeeded {
            record_automation_lifecycle_event_with_metadata(
                run,
                "repair_exhausted",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(unmet_requirements) = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
    {
        if workflow_class == "research" {
            let mut metadata = base_metadata.clone();
            metadata.insert(
                "unmet_requirements".to_string(),
                Value::Array(unmet_requirements.clone()),
            );
            record_automation_lifecycle_event_with_metadata(
                run,
                "research_coverage_failed",
                base_reason.clone(),
                None,
                Some(Value::Object(metadata)),
            );
        }
    }

    if let Some(verification) = artifact_validation.and_then(|value| value.get("verification")) {
        let expected = verification
            .get("verification_expected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let ran = verification
            .get("verification_ran")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let failed = verification
            .get("verification_failed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if expected {
            let mut metadata = base_metadata.clone();
            metadata.insert("verification".to_string(), verification.clone());
            record_automation_lifecycle_event_with_metadata(
                run,
                "verification_started",
                None,
                None,
                Some(Value::Object(metadata.clone())),
            );
            if failed {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_failed",
                    base_reason.clone(),
                    None,
                    Some(Value::Object(metadata)),
                );
            } else if ran {
                record_automation_lifecycle_event_with_metadata(
                    run,
                    "verification_passed",
                    None,
                    None,
                    Some(Value::Object(metadata)),
                );
            }
        }
    }
}

pub(crate) fn automation_output_session_id(output: &Value) -> Option<String> {
    output
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| {
            content
                .get("session_id")
                .or_else(|| content.get("sessionId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn build_automation_pending_gate(
    node: &AutomationFlowNode,
) -> Option<AutomationPendingGate> {
    let gate = node.gate.as_ref()?;
    Some(AutomationPendingGate {
        node_id: node.node_id.clone(),
        title: node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(node.objective.as_str())
            .to_string(),
        instructions: gate.instructions.clone(),
        decisions: gate.decisions.clone(),
        rework_targets: gate.rework_targets.clone(),
        requested_at_ms: now_ms(),
        upstream_node_ids: node.depends_on.clone(),
    })
}

fn automation_node_builder_metadata(node: &AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn automation_node_research_stage(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "research_stage")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_is_research_finalize(node: &AutomationFlowNode) -> bool {
    automation_node_research_stage(node).as_deref() == Some("research_finalize")
}

fn automation_node_builder_priority(node: &AutomationFlowNode) -> i32 {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

fn automation_upstream_output_for_alias<'a>(
    upstream_inputs: &'a [Value],
    alias: &str,
) -> Option<&'a Value> {
    upstream_inputs
        .iter()
        .find(|input| input.get("alias").and_then(Value::as_str) == Some(alias))
        .and_then(|input| input.get("output"))
}

fn automation_upstream_structured_handoff<'a>(output: &'a Value) -> Option<&'a Value> {
    output
        .pointer("/content/structured_handoff")
        .or_else(|| output.get("structured_handoff"))
}

fn truncate_path_list_for_prompt(paths: Vec<String>, limit: usize) -> Vec<String> {
    let mut deduped = normalize_non_empty_list(paths);
    if deduped.len() > limit {
        deduped.truncate(limit);
    }
    deduped
}

fn value_object_path_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn path_looks_like_workspace_path(raw_path: &str) -> bool {
    let trimmed = raw_path.trim().trim_matches('`');
    !trimmed.is_empty()
        && !trimmed.starts_with("http://")
        && !trimmed.starts_with("https://")
        && (trimmed.contains('/') || trimmed.ends_with(".md") || trimmed.ends_with(".yaml"))
}

fn top_level_workspace_dir(path: &str) -> Option<String> {
    PathBuf::from(path)
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn workspace_relative_path_exists(workspace_root: &str, relative_path: &str) -> bool {
    let candidate = PathBuf::from(workspace_root).join(relative_path.trim_start_matches('/'));
    candidate.exists()
}

fn normalize_workspace_display_path_with_bases(
    workspace_root: &str,
    raw_path: &str,
    base_dirs: &[String],
) -> Option<String> {
    if let Some(normalized) = normalize_workspace_display_path(workspace_root, raw_path) {
        if workspace_relative_path_exists(workspace_root, &normalized) {
            return Some(normalized);
        }
    }
    if !path_looks_like_workspace_path(raw_path) {
        return None;
    }
    let trimmed = raw_path
        .trim()
        .trim_matches('`')
        .trim_start_matches("./")
        .trim_start_matches('/');
    let mut candidates = base_dirs
        .iter()
        .filter_map(|base_dir| {
            let candidate = format!("{}/{}", base_dir.trim_end_matches('/'), trimmed);
            normalize_workspace_display_path(workspace_root, &candidate)
                .filter(|normalized| workspace_relative_path_exists(workspace_root, normalized))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return normalize_workspace_display_path(workspace_root, raw_path);
    }
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

fn normalize_workspace_path_annotation(
    workspace_root: &str,
    raw_path: &str,
    base_dirs: &[String],
) -> Option<String> {
    if let Some((candidate, suffix)) = raw_path.split_once(" (") {
        return normalize_workspace_display_path_with_bases(workspace_root, candidate, base_dirs)
            .map(|normalized| format!("{normalized} ({suffix}"));
    }
    if let Some((candidate, suffix)) = raw_path.split_once(": ") {
        return normalize_workspace_display_path_with_bases(workspace_root, candidate, base_dirs)
            .map(|normalized| format!("{normalized}: {suffix}"));
    }
    normalize_workspace_display_path_with_bases(workspace_root, raw_path, base_dirs)
}

fn upstream_output_base_dirs(output: &Value, workspace_root: &str) -> Vec<String> {
    let mut bases = Vec::new();
    let path_arrays = [
        output
            .get("artifact_validation")
            .and_then(|value| value.get("read_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("current_node_read_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("discovered_relevant_paths")),
        output
            .get("artifact_validation")
            .and_then(|value| value.get("current_node_discovered_relevant_paths")),
    ];
    for rows in path_arrays.into_iter().flatten() {
        let Some(rows) = rows.as_array() else {
            continue;
        };
        for row in rows.iter().filter_map(Value::as_str) {
            let Some(normalized) = normalize_workspace_display_path(workspace_root, row) else {
                continue;
            };
            if let Some(parent) = PathBuf::from(&normalized)
                .parent()
                .and_then(|value| value.to_str())
            {
                let parent = parent.trim().trim_matches('/');
                if !parent.is_empty() {
                    bases.push(parent.to_string());
                }
            }
            if let Some(top_level) = top_level_workspace_dir(&normalized) {
                bases.push(top_level);
            }
        }
    }
    bases.sort();
    bases.dedup();
    bases
}

fn normalize_structured_handoff_field(
    workspace_root: &str,
    base_dirs: &[String],
    key: &str,
    value: &mut Value,
) {
    let Some(rows) = value.as_array_mut() else {
        return;
    };
    for row in rows {
        match row {
            Value::String(raw) => {
                let normalized = match key {
                    "files_not_reviewed" | "skipped_paths_initial" => {
                        normalize_workspace_path_annotation(workspace_root, raw, base_dirs)
                    }
                    _ => {
                        normalize_workspace_display_path_with_bases(workspace_root, raw, base_dirs)
                    }
                };
                if let Some(normalized) = normalized {
                    *raw = normalized;
                }
            }
            Value::Object(map) => {
                if let Some(Value::String(path)) = map.get_mut("path") {
                    if let Some(normalized) =
                        normalize_workspace_display_path_with_bases(workspace_root, path, base_dirs)
                    {
                        *path = normalized;
                    }
                }
                if matches!(
                    key,
                    "citations_local" | "citations_external" | "sources_reviewed"
                ) {
                    if let Some(Value::String(source)) = map.get_mut("source") {
                        if let Some(normalized) = normalize_workspace_display_path_with_bases(
                            workspace_root,
                            source,
                            base_dirs,
                        ) {
                            *source = normalized;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn normalize_upstream_research_output_paths(workspace_root: &str, output: &Value) -> Value {
    let mut normalized = output.clone();
    let base_dirs = upstream_output_base_dirs(&normalized, workspace_root);
    let Some(content) = normalized.get_mut("content").and_then(Value::as_object_mut) else {
        return normalized;
    };
    if let Some(handoff) = content
        .get_mut("structured_handoff")
        .and_then(Value::as_object_mut)
    {
        for key in [
            "discovered_paths",
            "priority_paths",
            "skipped_paths_initial",
            "read_paths",
            "files_reviewed",
            "files_not_reviewed",
            "citations_local",
            "citations_external",
            "sources_reviewed",
        ] {
            if let Some(value) = handoff.get_mut(key) {
                normalize_structured_handoff_field(workspace_root, &base_dirs, key, value);
            }
        }
    }
    if let Some(text) = content
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&text) {
            if let Some(map) = parsed.as_object_mut() {
                for key in [
                    "discovered_paths",
                    "priority_paths",
                    "skipped_paths_initial",
                    "read_paths",
                    "files_reviewed",
                    "files_not_reviewed",
                    "citations_local",
                    "citations_external",
                    "sources_reviewed",
                ] {
                    if let Some(value) = map.get_mut(key) {
                        normalize_structured_handoff_field(workspace_root, &base_dirs, key, value);
                    }
                }
            }
            content.insert("text".to_string(), json!(parsed.to_string()));
        }
    }
    normalized
}

fn render_research_finalize_upstream_summary(upstream_inputs: &[Value]) -> Option<String> {
    let source_inventory =
        automation_upstream_output_for_alias(upstream_inputs, "source_inventory")
            .and_then(automation_upstream_structured_handoff);
    let local_source_notes =
        automation_upstream_output_for_alias(upstream_inputs, "local_source_notes")
            .and_then(automation_upstream_structured_handoff);
    let external_research =
        automation_upstream_output_for_alias(upstream_inputs, "external_research")
            .and_then(automation_upstream_structured_handoff);

    let discovered_files = source_inventory
        .and_then(|handoff| handoff.get("discovered_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let priority_files = source_inventory
        .and_then(|handoff| handoff.get("priority_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let files_not_reviewed = local_source_notes
        .and_then(|handoff| handoff.get("files_not_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "path"),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let web_sources_reviewed = external_research
        .and_then(|handoff| handoff.get("sources_reviewed"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| match row {
                    Value::String(path) => Some(path.trim().to_string()),
                    Value::Object(_) => value_object_path_field(row, "url")
                        .or_else(|| value_object_path_field(row, "path")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let discovered_files = truncate_path_list_for_prompt(discovered_files, 12);
    let priority_files = truncate_path_list_for_prompt(priority_files, 12);
    let files_reviewed = truncate_path_list_for_prompt(files_reviewed, 12);
    let files_not_reviewed = truncate_path_list_for_prompt(files_not_reviewed, 12);
    let web_sources_reviewed = truncate_path_list_for_prompt(web_sources_reviewed, 8);

    if discovered_files.is_empty()
        && priority_files.is_empty()
        && files_reviewed.is_empty()
        && files_not_reviewed.is_empty()
        && web_sources_reviewed.is_empty()
    {
        return None;
    }

    let list_or_none = |items: &[String]| {
        if items.is_empty() {
            "none recorded".to_string()
        } else {
            items
                .iter()
                .map(|item| format!("- `{}`", item))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    Some(format!(
        "Research Coverage Summary:\nRelevant discovered files from upstream:\n{}\nPriority paths from upstream:\n{}\nUpstream files already reviewed:\n{}\nUpstream files already marked not reviewed:\n{}\nUpstream web sources reviewed:\n{}\nFinal brief rule: every relevant discovered file should appear in `Files reviewed` or `Files not reviewed`, and proof points must stay citation-backed.",
        list_or_none(&discovered_files),
        list_or_none(&priority_files),
        list_or_none(&files_reviewed),
        list_or_none(&files_not_reviewed),
        list_or_none(&web_sources_reviewed),
    ))
}

#[derive(Clone, Copy)]
struct SplitResearchTemplateConfig {
    template_id: &'static str,
    final_node_id: &'static str,
    final_agent_id: &'static str,
    discover_node_id: &'static str,
    discover_agent_id: &'static str,
    discover_title: &'static str,
    discover_objective: &'static str,
    discover_display_name: &'static str,
    local_node_id: &'static str,
    local_agent_id: &'static str,
    local_title: &'static str,
    local_objective: &'static str,
    local_display_name: &'static str,
    external_node_id: &'static str,
    external_agent_id: &'static str,
    external_title: &'static str,
    external_objective: &'static str,
    external_display_name: &'static str,
    final_title: &'static str,
    final_objective: &'static str,
}

fn split_research_template_config(template_id: &str) -> Option<SplitResearchTemplateConfig> {
    match template_id {
        "marketing-content-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "marketing-content-pipeline",
            final_node_id: "research-brief",
            final_agent_id: "research",
            discover_node_id: "research-discover-sources",
            discover_agent_id: "research-discover",
            discover_title: "Discover Sources",
            discover_objective: "Enumerate the workspace, identify the relevant source corpus, and prioritize which local files must be read for the marketing brief.",
            discover_display_name: "Research Discover",
            local_node_id: "research-local-sources",
            local_agent_id: "research-local-sources",
            local_title: "Read Local Sources",
            local_objective: "Read the prioritized local product and marketing files and produce source-backed notes for the brief.",
            local_display_name: "Research Local Sources",
            external_node_id: "research-external-research",
            external_agent_id: "research-external",
            external_title: "External Research",
            external_objective: "Perform targeted external research that complements the local source notes and record what web evidence was gathered or unavailable.",
            external_display_name: "Research External",
            final_title: "Research Brief",
            final_objective: "Write `marketing-brief.md` from the structured discovery, local source notes, and external research gathered earlier in the workflow.",
        }),
        "competitor-research-pipeline" => Some(SplitResearchTemplateConfig {
            template_id: "competitor-research-pipeline",
            final_node_id: "scan-market",
            final_agent_id: "market-scan",
            discover_node_id: "scan-market-discover",
            discover_agent_id: "market-discover",
            discover_title: "Discover Market Sources",
            discover_objective: "Identify the local source corpus and file inventory that should guide the competitor scan.",
            discover_display_name: "Market Discover",
            local_node_id: "scan-market-local-sources",
            local_agent_id: "market-local-sources",
            local_title: "Read Market Sources",
            local_objective: "Read the prioritized local competitor and strategy sources before external scanning.",
            local_display_name: "Market Local Sources",
            external_node_id: "scan-market-external-research",
            external_agent_id: "market-external",
            external_title: "Research Market",
            external_objective: "Gather current external competitor evidence guided by the local market context.",
            external_display_name: "Market External",
            final_title: "Scan Market",
            final_objective: "Synthesize the discovered local and external evidence into the final competitor scan.",
        }),
        "weekly-newsletter-builder" => Some(SplitResearchTemplateConfig {
            template_id: "weekly-newsletter-builder",
            final_node_id: "curate-issue",
            final_agent_id: "curator",
            discover_node_id: "curate-issue-discover",
            discover_agent_id: "curator-discover",
            discover_title: "Discover Issue Sources",
            discover_objective: "Identify the local source corpus and candidate files that should feed this week's issue.",
            discover_display_name: "Curator Discover",
            local_node_id: "curate-issue-local-sources",
            local_agent_id: "curator-local-sources",
            local_title: "Read Issue Sources",
            local_objective: "Read the prioritized local source files and extract the strongest issue candidates.",
            local_display_name: "Curator Local Sources",
            external_node_id: "curate-issue-external-research",
            external_agent_id: "curator-external",
            external_title: "Research Issue",
            external_objective: "Gather timely external signals that should influence this week's issue.",
            external_display_name: "Curator External",
            final_title: "Curate Issue",
            final_objective: "Curate the best items for this week's issue from the staged research handoffs.",
        }),
        "sales-prospecting-team" => Some(SplitResearchTemplateConfig {
            template_id: "sales-prospecting-team",
            final_node_id: "research-account",
            final_agent_id: "account-research",
            discover_node_id: "research-account-discover",
            discover_agent_id: "account-discover",
            discover_title: "Discover Account Sources",
            discover_objective: "Identify the source corpus that should guide account research.",
            discover_display_name: "Account Discover",
            local_node_id: "research-account-local-sources",
            local_agent_id: "account-local-sources",
            local_title: "Read Account Sources",
            local_objective: "Read the prioritized local account and ICP files before drafting the account brief.",
            local_display_name: "Account Local Sources",
            external_node_id: "research-account-external-research",
            external_agent_id: "account-external",
            external_title: "Research Account Externally",
            external_objective: "Gather targeted external account context and buying signals to support the brief.",
            external_display_name: "Account External",
            final_title: "Research Account",
            final_objective: "Prepare the final account brief from the staged discovery, local evidence, and external research.",
        }),
        _ => None,
    }
}

fn studio_template_id(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("studio"))
        .and_then(Value::as_object)
        .and_then(|studio| {
            studio
                .get("template_id")
                .or_else(|| studio.get("templateId"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn split_research_stage_metadata(
    title: &str,
    role: &str,
    prompt: String,
    research_stage: &str,
    output_path: Option<&str>,
    required_tools: &[&str],
    write_required: bool,
) -> Option<Value> {
    let mut builder = serde_json::Map::new();
    builder.insert("title".to_string(), json!(title));
    builder.insert("role".to_string(), json!(role));
    builder.insert("prompt".to_string(), json!(prompt));
    builder.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        builder.insert("output_path".to_string(), json!(path));
    }
    if !required_tools.is_empty() {
        builder.insert("required_tools".to_string(), json!(required_tools));
    }
    if write_required {
        builder.insert("write_required".to_string(), json!(true));
    }
    let mut studio = serde_json::Map::new();
    studio.insert("research_stage".to_string(), json!(research_stage));
    if let Some(path) = output_path {
        studio.insert("output_path".to_string(), json!(path));
    }
    Some(json!({
        "builder": Value::Object(builder),
        "studio": Value::Object(studio),
    }))
}

fn migrated_stage_agent(
    base: &AutomationAgentProfile,
    agent_id: &str,
    display_name: &str,
    allowlist: &[&str],
) -> AutomationAgentProfile {
    let mut agent = base.clone();
    agent.agent_id = agent_id.to_string();
    agent.display_name = display_name.to_string();
    agent.template_id = None;
    agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(
        allowlist.iter().map(|value| (*value).to_string()).collect(),
    );
    agent.tool_policy.denylist =
        config::channels::normalize_allowed_tools(agent.tool_policy.denylist.clone());
    agent
}

fn migrate_split_research_studio_metadata(metadata: &mut Value) {
    let Some(root) = metadata.as_object_mut() else {
        return;
    };
    let studio = root
        .entry("studio".to_string())
        .or_insert_with(|| json!({}));
    let Some(studio_obj) = studio.as_object_mut() else {
        return;
    };
    studio_obj.insert("version".to_string(), json!(2));
    studio_obj.insert("workflow_structure_version".to_string(), json!(2));
    studio_obj.remove("agent_drafts");
    studio_obj.remove("node_drafts");
    studio_obj.remove("node_layout");
}

pub(super) fn migrate_bundled_studio_research_split_automation(
    automation: &mut AutomationV2Spec,
) -> bool {
    let Some(template_id) = studio_template_id(automation) else {
        return false;
    };
    let Some(config) = split_research_template_config(&template_id) else {
        return false;
    };
    if automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == config.discover_node_id)
        || automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == config.final_node_id)
            .is_some_and(automation_node_is_research_finalize)
    {
        if let Some(metadata) = automation.metadata.as_mut() {
            migrate_split_research_studio_metadata(metadata);
        }
        return false;
    }
    let Some(final_node_index) = automation
        .flow
        .nodes
        .iter()
        .position(|node| node.node_id == config.final_node_id)
    else {
        return false;
    };
    let Some(base_agent) = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == config.final_agent_id)
        .cloned()
    else {
        return false;
    };
    let existing_final_node = automation.flow.nodes[final_node_index].clone();
    let output_path = automation_node_required_output_path(&existing_final_node);
    let final_contract_kind = existing_final_node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "artifact".to_string());
    let final_is_brief_like = final_contract_kind.trim().eq_ignore_ascii_case("brief");
    let final_summary_guidance = existing_final_node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.summary_guidance.clone());
    let discover_prompt = "Enumerate the workspace, identify the relevant source corpus, and return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`. If a curated source index such as `SOURCES.md` exists, read it first. Perform at least one concrete `read` before finishing, but read only enough to identify the corpus for the next stage. Do not write final workspace artifacts in this stage.".to_string();
    let local_prompt = "Use the upstream `source_inventory` handoff to decide which concrete local files to read. Perform concrete `read` calls, extract the product or market facts supported by those reads, and return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`. Do not invent facts from filenames alone.".to_string();
    let external_prompt = "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide targeted external research. Perform `websearch` and fetch result pages when snippets are not enough, then return `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`. If search is unavailable, record that limitation clearly instead of inventing evidence.".to_string();
    let final_prompt = match config.template_id {
        "marketing-content-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth. Read `marketing-brief.md` from disk only as a fallback or verification step. Synthesize the final marketing brief from those handoffs instead of repeating discovery or fresh web research in this stage. Include a workspace source audit, audience, positioning, proof points with citations, `Files reviewed`, `Files not reviewed`, and `Web sources reviewed`, and clearly note any research limitations. In source-audit sections, list only exact concrete workspace-relative file paths or exact reviewed URLs; do not use directory names, wildcard paths, or glob patterns.".to_string(),
        "competitor-research-pipeline" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final competitor scan. Separate observed evidence from inference, keep the scan current and signal-focused, and do not rerun discovery or fresh web research in this stage.".to_string(),
        "weekly-newsletter-builder" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs to curate the final issue. Turn them into the final shortlist and section order without repeating discovery or fresh web research in this stage.".to_string(),
        "sales-prospecting-team" => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final account brief. Separate observed facts from hypotheses and do not rerun discovery or fresh web research in this stage.".to_string(),
        _ => "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth and synthesize the final artifact without repeating discovery or fresh web research in this stage.".to_string(),
    };

    let discover_node = AutomationFlowNode {
        node_id: config.discover_node_id.to_string(),
        agent_id: config.discover_agent_id.to_string(),
        objective: config.discover_objective.to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff in the final response instead of writing workspace files."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.discover_title,
            "watcher",
            discover_prompt,
            "research_discover",
            None,
            &["glob", "read"],
            false,
        ),
    };
    let local_node = AutomationFlowNode {
        node_id: config.local_node_id.to_string(),
        agent_id: config.local_agent_id.to_string(),
        objective: config.local_objective.to_string(),
        depends_on: vec![config.discover_node_id.to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["concrete_reads".to_string()],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff backed by concrete local file reads.".to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.local_title,
            "watcher",
            local_prompt,
            "research_local_sources",
            None,
            &["read"],
            false,
        ),
    };
    let external_node = AutomationFlowNode {
        node_id: config.external_node_id.to_string(),
        agent_id: config.external_agent_id.to_string(),
        objective: config.external_objective.to_string(),
        depends_on: vec![
            config.discover_node_id.to_string(),
            config.local_node_id.to_string(),
        ],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: config.discover_node_id.to_string(),
                alias: "source_inventory".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: config.local_node_id.to_string(),
                alias: "local_source_notes".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                required_tools: vec!["websearch".to_string()],
                required_evidence: vec!["external_sources".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["successful_web_research".to_string()],
                retry_on_missing: vec![
                    "external_sources".to_string(),
                    "successful_web_research".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some(
                "Return a structured handoff describing external research findings or limitations."
                    .to_string(),
            ),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: split_research_stage_metadata(
            config.external_title,
            "watcher",
            external_prompt,
            "research_external_sources",
            None,
            &["websearch", "webfetch", "read"],
            false,
        ),
    };
    let mut final_node = existing_final_node.clone();
    final_node.objective = config.final_objective.to_string();
    final_node.depends_on = vec![
        config.discover_node_id.to_string(),
        config.local_node_id.to_string(),
        config.external_node_id.to_string(),
    ];
    final_node.input_refs = vec![
        AutomationFlowInputRef {
            from_step_id: config.discover_node_id.to_string(),
            alias: "source_inventory".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.local_node_id.to_string(),
            alias: "local_source_notes".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: config.external_node_id.to_string(),
            alias: "external_research".to_string(),
        },
    ];
    final_node.stage_kind = Some(AutomationNodeStageKind::Workstream);
    final_node.output_contract = Some(AutomationFlowOutputContract {
        kind: final_contract_kind,
        validator: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator)
            .or(if final_is_brief_like {
                Some(crate::AutomationOutputValidatorKind::ResearchBrief)
            } else {
                None
            }),
        enforcement: Some(crate::AutomationOutputEnforcement {
            required_tools: Vec::new(),
            required_evidence: vec![
                "local_source_reads".to_string(),
                "external_sources".to_string(),
            ],
            required_sections: if final_is_brief_like {
                vec![
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                    "web_sources_reviewed".to_string(),
                ]
            } else {
                Vec::new()
            },
            prewrite_gates: Vec::new(),
            retry_on_missing: if final_is_brief_like {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                    "web_sources_reviewed".to_string(),
                ]
            } else {
                vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                ]
            },
            terminal_on: vec![
                "tool_unavailable".to_string(),
                "repair_budget_exhausted".to_string(),
            ],
            repair_budget: Some(5),
            session_text_recovery: Some("require_prewrite_satisfied".to_string()),
        }),
        schema: existing_final_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.schema.clone()),
        summary_guidance: final_summary_guidance,
    });
    final_node.metadata = split_research_stage_metadata(
        config.final_title,
        "watcher",
        final_prompt,
        "research_finalize",
        output_path.as_deref(),
        &[],
        output_path.is_some(),
    );

    let mut new_nodes = Vec::with_capacity(automation.flow.nodes.len() + 3);
    let mut inserted = false;
    for node in automation.flow.nodes.clone() {
        if node.node_id == config.final_node_id {
            new_nodes.push(discover_node.clone());
            new_nodes.push(local_node.clone());
            new_nodes.push(external_node.clone());
            new_nodes.push(final_node.clone());
            inserted = true;
        } else if node.node_id != config.discover_node_id
            && node.node_id != config.local_node_id
            && node.node_id != config.external_node_id
        {
            new_nodes.push(node);
        }
    }
    if !inserted {
        return false;
    }
    automation.flow.nodes = new_nodes;

    for candidate in [
        migrated_stage_agent(
            &base_agent,
            config.discover_agent_id,
            config.discover_display_name,
            &["glob", "read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.local_agent_id,
            config.local_display_name,
            &["read"],
        ),
        migrated_stage_agent(
            &base_agent,
            config.external_agent_id,
            config.external_display_name,
            &["websearch", "webfetch", "read"],
        ),
    ] {
        if !automation
            .agents
            .iter()
            .any(|agent| agent.agent_id == candidate.agent_id)
        {
            automation.agents.push(candidate);
        }
    }
    if let Some(final_agent) = automation
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == config.final_agent_id)
    {
        final_agent.tool_policy.allowlist = config::channels::normalize_allowed_tools(vec![
            "read".to_string(),
            "write".to_string(),
        ]);
    }
    if let Some(metadata) = automation.metadata.as_mut() {
        migrate_split_research_studio_metadata(metadata);
    } else {
        automation.metadata = Some(json!({
            "studio": {
                "template_id": config.template_id,
                "version": 2,
                "workflow_structure_version": 2
            }
        }));
    }
    true
}

fn automation_phase_execution_mode_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter_map(|phase| {
                    let phase_id = phase.get("phase_id").and_then(Value::as_str)?.trim();
                    if phase_id.is_empty() {
                        return None;
                    }
                    let mode = phase
                        .get("execution_mode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("soft");
                    Some((phase_id.to_string(), mode.to_string()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_current_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(String, usize, String)> {
    let phase_rank = automation_phase_rank_map(automation);
    if phase_rank.is_empty() {
        return None;
    }
    let phase_modes = automation_phase_execution_mode_map(automation);
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !completed.contains(&node.node_id))
        .filter_map(|node| {
            automation_node_builder_metadata(node, "phase_id").and_then(|phase_id| {
                phase_rank
                    .get(&phase_id)
                    .copied()
                    .map(|rank| (phase_id, rank))
            })
        })
        .min_by_key(|(_, rank)| *rank)
        .map(|(phase_id, rank)| {
            let mode = phase_modes
                .get(&phase_id)
                .cloned()
                .unwrap_or_else(|| "soft".to_string());
            (phase_id, rank, mode)
        })
}

pub(crate) fn automation_phase_rank_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, usize> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .enumerate()
                .filter_map(|(index, phase)| {
                    phase
                        .get("phase_id")
                        .and_then(Value::as_str)
                        .map(|phase_id| (phase_id.to_string(), index))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_sort_key(
    node: &AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
    current_open_phase_rank: Option<usize>,
) -> (usize, usize, i32, String) {
    let phase_order = automation_node_builder_metadata(node, "phase_id")
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    let open_phase_bias = current_open_phase_rank
        .map(|open_rank| usize::from(phase_order != open_rank))
        .unwrap_or(0);
    (
        open_phase_bias,
        phase_order,
        -automation_node_builder_priority(node),
        node.node_id.clone(),
    )
}

pub(crate) fn automation_filter_runnable_by_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    let Some((_, open_rank, _)) = automation_current_open_phase(automation, run) else {
        return runnable;
    };
    let phase_rank = automation_phase_rank_map(automation);
    let in_open_phase = runnable
        .iter()
        .filter(|node| {
            automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied()
                == Some(open_rank)
        })
        .cloned()
        .collect::<Vec<_>>();
    if in_open_phase.is_empty() {
        runnable
    } else {
        in_open_phase
    }
}

fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
    let Some(scope) = scope else {
        return vec!["__repo__".to_string()];
    };
    let entries = scope
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        vec!["__repo__".to_string()]
    } else {
        entries
    }
}

fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a == "__repo__"
                || b == "__repo__"
                || a == b
                || a == "."
                || b == "."
                || a == "*"
                || b == "*"
                || a.starts_with(&format!("{}/", b))
                || b.starts_with(&format!("{}/", a))
        })
    })
}

pub(crate) fn automation_filter_runnable_by_write_scope_conflicts(
    runnable: Vec<AutomationFlowNode>,
    max_parallel: usize,
) -> Vec<AutomationFlowNode> {
    if max_parallel <= 1 {
        return runnable.into_iter().take(1).collect();
    }
    let mut selected = Vec::new();
    let mut selected_scopes = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        if conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

pub(crate) fn automation_blocked_nodes(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Vec<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let pending = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let phase_rank = automation_phase_rank_map(automation);
    let current_open_phase = automation_current_open_phase(automation, run);
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| pending.contains(&node.node_id))
        .filter_map(|node| {
            let missing_deps = node.depends_on.iter().any(|dep| !completed.contains(dep));
            if missing_deps {
                return Some(node.node_id.clone());
            }
            let Some((_, open_rank, mode)) = current_open_phase.as_ref() else {
                return None;
            };
            if mode != "barrier" {
                return None;
            }
            let node_phase_rank = automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied();
            if node_phase_rank.is_some_and(|rank| rank > *open_rank) {
                return Some(node.node_id.clone());
            }
            None
        })
        .collect::<Vec<_>>()
}

pub(crate) fn record_automation_open_phase_event(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    let Some((phase_id, phase_rank, execution_mode)) =
        automation_current_open_phase(automation, run)
    else {
        return;
    };
    let last_recorded = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|entry| entry.event == "phase_opened")
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|metadata| metadata.get("phase_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if last_recorded.as_deref() == Some(phase_id.as_str()) {
        return;
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "phase_opened",
        Some(format!("phase `{}` is now open", phase_id)),
        None,
        Some(json!({
            "phase_id": phase_id,
            "phase_rank": phase_rank,
            "execution_mode": execution_mode,
        })),
    );
}

pub fn refresh_automation_runtime_state(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    run.checkpoint.blocked_nodes = automation_blocked_nodes(automation, run);
    record_automation_open_phase_event(automation, run);
}

fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completed_mission_milestones(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> std::collections::HashSet<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation_mission_milestones(automation)
        .iter()
        .filter_map(|milestone| {
            let milestone_id = milestone
                .get("milestone_id")
                .and_then(Value::as_str)?
                .trim();
            if milestone_id.is_empty() {
                return None;
            }
            let required = milestone
                .get("required_stage_ids")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!required.is_empty()
                && required
                    .iter()
                    .all(|stage_id| completed.contains(*stage_id)))
            .then_some(milestone_id.to_string())
        })
        .collect()
}

pub(crate) fn record_milestone_promotions(
    automation: &AutomationV2Spec,
    row: &mut AutomationV2RunRecord,
    promoted_by_node_id: &str,
) {
    let already_recorded = row
        .checkpoint
        .lifecycle_history
        .iter()
        .filter(|entry| entry.event == "milestone_promoted")
        .filter_map(|entry| {
            entry.metadata.as_ref().and_then(|metadata| {
                metadata
                    .get("milestone_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let completed = completed_mission_milestones(automation, row);
    for milestone in automation_mission_milestones(automation) {
        let milestone_id = milestone
            .get("milestone_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if milestone_id.is_empty()
            || !completed.contains(milestone_id)
            || already_recorded.contains(milestone_id)
        {
            continue;
        }
        let title = milestone
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(milestone_id);
        let phase_id = milestone
            .get("phase_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let required_stage_ids = milestone
            .get("required_stage_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        record_automation_lifecycle_event_with_metadata(
            row,
            "milestone_promoted",
            Some(format!("milestone `{title}` promoted")),
            None,
            Some(json!({
                "milestone_id": milestone_id,
                "title": title,
                "phase_id": phase_id,
                "required_stage_ids": required_stage_ids,
                "promoted_by_node_id": promoted_by_node_id,
            })),
        );
    }
}

pub fn collect_automation_descendants(
    automation: &AutomationV2Spec,
    root_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut descendants = root_ids.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &automation.flow.nodes {
            if descendants.contains(&node.node_id) {
                continue;
            }
            if node.depends_on.iter().any(|dep| descendants.contains(dep)) {
                descendants.insert(node.node_id.clone());
                changed = true;
            }
        }
    }
    descendants
}

/// Returns all transitive ancestors of `node_id` (i.e. every node that
/// `node_id` directly or indirectly depends on), not including `node_id`
/// itself.
pub fn collect_automation_ancestors(
    automation: &AutomationV2Spec,
    node_id: &str,
) -> std::collections::HashSet<String> {
    let mut ancestors = std::collections::HashSet::new();
    let mut queue = vec![node_id.to_string()];
    while let Some(current_id) = queue.pop() {
        if let Some(node) = automation
            .flow
            .nodes
            .iter()
            .find(|n| n.node_id == current_id)
        {
            for dep in &node.depends_on {
                if ancestors.insert(dep.clone()) {
                    queue.push(dep.clone());
                }
            }
        }
    }
    ancestors
}

pub(crate) fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
) -> String {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let normalized_upstream_inputs = upstream_inputs
        .iter()
        .map(|input| {
            let mut normalized_input = input.clone();
            if let Some(output) = input.get("output") {
                if let Some(object) = normalized_input.as_object_mut() {
                    object.insert(
                        "output".to_string(),
                        normalize_upstream_research_output_paths(workspace_root, output),
                    );
                }
            }
            normalized_input
        })
        .collect::<Vec<_>>();
    let mut sections = Vec::new();
    if let Some(system_prompt) = template_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Template system prompt:\n{}", system_prompt));
    }
    if let Some(mission) = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("mission"))
    {
        let mission_title = mission
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(automation.name.as_str());
        let mission_goal = mission
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let success_criteria = mission
            .get("success_criteria")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|row| format!("- {}", row.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let shared_context = mission
            .get("shared_context")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Mission Brief:\nTitle: {mission_title}\nGoal: {mission_goal}\nShared context: {shared_context}\nSuccess criteria:\n{}",
            if success_criteria.is_empty() {
                "- none provided".to_string()
            } else {
                success_criteria
            }
        ));
    }
    sections.push(format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id, run_id, node.node_id, agent.display_name, node.objective, contract_kind
    ));
    if let Some(contract) = node.output_contract.as_ref() {
        let schema = contract
            .schema
            .as_ref()
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".to_string());
        let guidance = contract.summary_guidance.as_deref().unwrap_or("none");
        sections.push(format!(
            "Output Contract:\nKind: {}\nSummary guidance: {}\nSchema:\n{}",
            contract.kind, guidance, schema
        ));
    }
    if let Some(builder) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    {
        let local_title = builder
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(node.node_id.as_str());
        let local_prompt = builder
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let local_role = builder
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        sections.push(format!(
            "Local Assignment:\nTitle: {local_title}\nRole: {local_role}\nInstructions: {local_prompt}"
        ));
    }
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    sections.push(format!(
        "Execution Policy:\n- Mode: `{}`.\n- Use only declared workflow artifact paths.\n- Keep status and blocker notes in the response JSON, not as placeholder file contents.",
        execution_mode
    ));
    if automation_node_is_code_workflow(node) {
        let task_kind =
            automation_node_task_kind(node).unwrap_or_else(|| "code_change".to_string());
        let project_backlog_tasks = automation_node_projects_backlog_tasks(node);
        let task_id = automation_node_task_id(node).unwrap_or_else(|| "unassigned".to_string());
        let repo_root = automation_node_repo_root(node).unwrap_or_else(|| ".".to_string());
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        let acceptance_criteria = automation_node_acceptance_criteria(node)
            .unwrap_or_else(|| "satisfy the declared coding task acceptance criteria".to_string());
        let task_dependencies =
            automation_node_task_dependencies(node).unwrap_or_else(|| "none declared".to_string());
        let verification_state =
            automation_node_verification_state(node).unwrap_or_else(|| "pending".to_string());
        let task_owner =
            automation_node_task_owner(node).unwrap_or_else(|| "unclaimed".to_string());
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        sections.push(format!(
            "Coding Task Context:\n- Task id: `{}`.\n- Task kind: `{}`.\n- Repo root: `{}`.\n- Declared write scope: {}.\n- Acceptance criteria: {}.\n- Backlog dependencies: {}.\n- Verification state: {}.\n- Preferred owner: {}.\n- Verification expectation: {}.\n- Projects backlog tasks: {}.\n- Prefer repository edits plus a concise handoff artifact, not placeholder file rewrites.\n- Use `bash` for verification commands when tool access allows it.",
            task_id, task_kind, repo_root, write_scope, acceptance_criteria, task_dependencies, verification_state, task_owner, verification_command, if project_backlog_tasks { "yes" } else { "no" }
        ));
    }
    if let Some(output_path) = automation_node_required_output_path(node) {
        let output_rules = match execution_mode {
            "git_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `apply_patch` for multi-line source edits and `edit` for localized replacements.\n- Use `write` only for brand-new files or when patch/edit cannot express the change.\n- Do not replace an existing source file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            "filesystem_patch" => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `edit` for existing-file changes.\n- Use `write` for brand-new files or as a last resort when an edit cannot express the change.\n- Do not replace an existing file with a status note, preservation note, or placeholder summary.\n- Only write declared workflow artifact files.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
            _ => format!(
                "Required Workspace Output:\n- Create or update `{}` relative to the workspace root.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Use the `write` tool to create the full file contents.\n- Only write declared workflow artifact files; do not create auxiliary touch files, status files, marker files, or placeholder preservation notes.\n- Overwrite the declared output with the actual artifact contents for this run instead of preserving a prior placeholder.\n- Do not report success unless this file exists in the workspace when the stage ends.",
                output_path
            ),
        };
        sections.push(output_rules);
    }
    if automation_node_web_research_expected(node) {
        let requested_has_websearch = requested_tools.iter().any(|tool| tool == "websearch");
        let requested_has_webfetch = requested_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "webfetch" | "webfetch_html"));
        if requested_has_websearch {
            sections.push(
                "External Research Expectation:\n- Use `websearch` for current external evidence before finalizing the output file.\n- Use `webfetch` on concrete result URLs when search snippets are not enough.\n- Include only evidence you can support from local files or current web findings.\n- If `websearch` returns an authorization-required or unavailable result, treat external research as unavailable for this run, continue with local file reads, and note the web-research limitation instead of stopping."
                    .to_string(),
            );
        } else if requested_has_webfetch {
            sections.push(
                "External Research Expectation:\n- `websearch` is not available in this run.\n- Use `webfetch` only for concrete URLs already present in local sources or upstream handoffs.\n- If you cannot validate externally without search, record that limitation in the structured handoff and finish the node.\n- Do not ask the user for clarification or permission to continue; return the required JSON handoff for this run."
                    .to_string(),
            );
        } else {
            sections.push(
                "External Research Expectation:\n- No web research tool is available in this run.\n- Record the web-research limitation clearly in the structured handoff, continue with any allowed local reads, and finish without asking follow-up questions."
                    .to_string(),
            );
        }
    }
    let validator_kind = automation_output_validator_kind(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    if handoff_only_structured_json {
        sections.push(
            "Structured Handoff Expectation:\n- Return the requested structured JSON handoff in the final response body.\n- The final response body should contain JSON only: the handoff JSON, then the final compact JSON status object.\n- Do not include headings, bullets, markdown fences, prose explanations, or follow-up questions.\n- Do not stop after tool calls alone; include a machine-readable JSON object or array with the requested fields."
                .to_string(),
        );
    }
    let mut prompt = sections.join("\n\n");
    if !normalized_upstream_inputs.is_empty() {
        prompt.push_str("\n\nUpstream Inputs:");
        for input in &normalized_upstream_inputs {
            let alias = input
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or("input");
            let from_step_id = input
                .get("from_step_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let output = input.get("output").cloned().unwrap_or(Value::Null);
            let rendered =
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
            prompt.push_str(&format!(
                "\n- {}\n  from_step_id: {}\n  output:\n{}",
                alias,
                from_step_id,
                rendered
                    .lines()
                    .map(|line| format!("    {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    if automation_node_is_research_finalize(node) {
        if let Some(summary) =
            render_research_finalize_upstream_summary(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&summary);
        }
    }
    if node.node_id == "notify_user" || node.objective.to_ascii_lowercase().contains("email") {
        prompt.push_str(
            "\n\nDelivery rules:\n- Prefer inline email body delivery by default.\n- Only include an email attachment when upstream inputs contain a concrete attachment artifact with a non-empty s3key or upload result.\n- Never send an attachment parameter with an empty or null s3key.\n- If no attachment artifact exists, omit the attachment parameter entirely.",
        );
    }
    if let Some(report_path) = standup_report_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nStandup report path:\n- Write the final markdown report to `{}` relative to the workspace root.\n- Use the `write` tool for the report.\n- The report must remain inside the workspace.",
            report_path
        ));
    }
    if let Some(project_id) = memory_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nMemory search scope:\n- `memory_search` defaults to the current session, current project, and global memory.\n- Current project_id: `{}`.\n- Use `tier: \"project\"` when you need recall limited to this workspace.\n- Use workspace files via `glob`, `grep`, and `read` when memory is sparse or stale.",
            project_id
        ));
    }
    let enforce_completed_first_attempt = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
        || handoff_only_structured_json)
        && attempt <= 1;
    if enforce_completed_first_attempt {
        if automation_node_required_output_path(node).is_some() {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the write tool actually created the output file.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the required structured handoff was actually returned in the final response.",
            );
        }
    } else {
        if handoff_only_structured_json {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include the required structured handoff JSON in the response body before the final compact status object.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim success unless the required structured handoff was actually returned in the final response.\n- Do not claim semantic success if the output is blocked or not approved.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim semantic success if the output is blocked or not approved.",
            );
        }
    }
    prompt
}

pub(crate) fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
) -> Option<String> {
    if attempt <= 1 {
        return None;
    }
    let prior_output = prior_output?;
    if !automation_output_needs_repair(prior_output) {
        return None;
    }

    let validator_summary = prior_output.get("validator_summary");
    let artifact_validation = prior_output.get("artifact_validation");
    let tool_telemetry = prior_output.get("tool_telemetry");
    let validator_outcome = validator_summary
        .and_then(|value| value.get("outcome"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements_from_summary = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_upstream_passed = validator_outcome
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("passed"))
        && unmet_requirements_from_summary.is_empty();
    if is_upstream_passed {
        return None;
    }
    let reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("the previous attempt did not satisfy the runtime validator");
    let unmet_requirements = unmet_requirements_from_summary;
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_offered = tool_telemetry
        .and_then(|value| value.get("requested_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_executed = tool_telemetry
        .and_then(|value| value.get("executed_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unreviewed_relevant_paths = artifact_validation
        .and_then(|value| value.get("unreviewed_relevant_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(attempt.saturating_sub(1));
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| max_attempts.saturating_sub(attempt.saturating_sub(1)));

    let unmet_line = if unmet_requirements.is_empty() {
        "none recorded".to_string()
    } else {
        unmet_requirements.join(", ")
    };
    let tools_offered_line = if tools_offered.is_empty() {
        "none recorded".to_string()
    } else {
        tools_offered.join(", ")
    };
    let tools_executed_line = if tools_executed.is_empty() {
        "none recorded".to_string()
    } else {
        tools_executed.join(", ")
    };
    let unreviewed_line = if unreviewed_relevant_paths.is_empty() {
        "none recorded".to_string()
    } else {
        unreviewed_relevant_paths.join(", ")
    };
    let next_actions_line = if required_next_tool_actions.is_empty() {
        "none recorded".to_string()
    } else {
        required_next_tool_actions.join(" | ")
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.",
        node.node_id,
        reason,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
    ))
}

fn is_agent_standup_automation(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("feature"))
        .and_then(Value::as_str)
        .map(|value| value == "agent_standup")
        .unwrap_or(false)
}

fn resolve_standup_report_path_template(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("standup"))
        .and_then(|value| value.get("report_path_template"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_standup_report_path_for_run(
    automation: &AutomationV2Spec,
    started_at_ms: u64,
) -> Option<String> {
    let template = resolve_standup_report_path_template(automation)?;
    if !template.contains("{{date}}") {
        return Some(template);
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    Some(template.replace("{{date}}", &date))
}

fn automation_workspace_project_id(workspace_root: &str) -> String {
    tandem_core::workspace_project_id(workspace_root)
        .unwrap_or_else(|| "workspace-unknown".to_string())
}

fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    let mut allowlist = if agent.tool_policy.allowlist.is_empty() {
        template
            .map(|value| value.capabilities.tool_allowlist.clone())
            .unwrap_or_default()
    } else {
        agent.tool_policy.allowlist.clone()
    };
    allowlist.sort();
    allowlist.dedup();
    allowlist
}

fn automation_node_output_extension(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path(node)
        .as_deref()
        .and_then(|value| std::path::Path::new(value).extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_kind")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("project_backlog_tasks"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "repo_root")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "write_scope")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "acceptance_criteria")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_dependencies")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_verification_state(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_state")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_owner")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_node_verification_command(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "verification_command")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Debug)]
struct AutomationVerificationStep {
    kind: String,
    command: String,
}

fn infer_verification_kind(command: &str) -> String {
    let lowered = command.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return "verify".to_string();
    }
    if lowered.starts_with("build:")
        || lowered.contains(" cargo build")
        || lowered.starts_with("cargo build")
        || lowered.contains(" npm run build")
        || lowered.starts_with("npm run build")
        || lowered.contains(" pnpm build")
        || lowered.starts_with("pnpm build")
        || lowered.contains(" yarn build")
        || lowered.starts_with("yarn build")
        || lowered.contains(" tsc")
        || lowered.starts_with("tsc")
        || lowered.starts_with("cargo check")
        || lowered.contains(" cargo check")
    {
        return "build".to_string();
    }
    if lowered.starts_with("test:")
        || lowered.contains(" cargo test")
        || lowered.starts_with("cargo test")
        || lowered.contains(" pytest")
        || lowered.starts_with("pytest")
        || lowered.contains(" npm test")
        || lowered.starts_with("npm test")
        || lowered.contains(" pnpm test")
        || lowered.starts_with("pnpm test")
        || lowered.contains(" yarn test")
        || lowered.starts_with("yarn test")
        || lowered.contains(" go test")
        || lowered.starts_with("go test")
    {
        return "test".to_string();
    }
    if lowered.starts_with("lint:")
        || lowered.contains(" clippy")
        || lowered.starts_with("cargo clippy")
        || lowered.contains(" eslint")
        || lowered.starts_with("eslint")
        || lowered.contains(" ruff")
        || lowered.starts_with("ruff")
        || lowered.contains(" shellcheck")
        || lowered.starts_with("shellcheck")
        || lowered.contains(" fmt --check")
        || lowered.contains(" format")
        || lowered.contains(" lint")
    {
        return "lint".to_string();
    }
    "verify".to_string()
}

fn split_verification_commands(raw: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        for chunk in trimmed.split("&&") {
            for piece in chunk.split(';') {
                let candidate = piece.trim();
                if candidate.is_empty() {
                    continue;
                }
                commands.push(candidate.to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    commands
        .into_iter()
        .filter(|value| seen.insert(value.to_ascii_lowercase()))
        .collect()
}

fn automation_node_verification_plan(node: &AutomationFlowNode) -> Vec<AutomationVerificationStep> {
    if let Some(items) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("verification_plan"))
        .and_then(Value::as_array)
    {
        let mut plan = Vec::new();
        for item in items {
            let (kind, command) = if let Some(obj) = item.as_object() {
                let command = obj
                    .get("command")
                    .or_else(|| obj.get("value"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let kind = obj
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_ascii_lowercase);
                (kind, command)
            } else {
                (
                    None,
                    item.as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                )
            };
            let Some(command) = command else {
                continue;
            };
            plan.push(AutomationVerificationStep {
                kind: kind.unwrap_or_else(|| infer_verification_kind(&command)),
                command,
            });
        }
        if !plan.is_empty() {
            return plan;
        }
    }
    automation_node_verification_command(node)
        .map(|raw| {
            split_verification_commands(&raw)
                .into_iter()
                .map(|command| AutomationVerificationStep {
                    kind: infer_verification_kind(&command),
                    command,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    if automation_node_task_kind(node)
        .as_deref()
        .is_some_and(|kind| matches!(kind, "code_change" | "repo_fix" | "implementation"))
    {
        return true;
    }
    let Some(extension) = automation_node_output_extension(node) else {
        return false;
    };
    let code_extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
        "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh",
    ];
    code_extensions.contains(&extension.as_str())
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    if let Some(validator) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.validator)
    {
        return validator;
    }
    if automation_node_is_code_workflow(node) {
        return crate::AutomationOutputValidatorKind::CodePatch;
    }
    match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("brief") => crate::AutomationOutputValidatorKind::ResearchBrief,
        Some("review") => crate::AutomationOutputValidatorKind::ReviewDecision,
        Some("structured_json") => crate::AutomationOutputValidatorKind::StructuredJson,
        _ => crate::AutomationOutputValidatorKind::GenericArtifact,
    }
}

fn path_looks_like_source_file(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref().is_some_and(|extension| {
        [
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
            "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh", "toml", "yaml", "yml",
            "json",
        ]
        .contains(&extension)
    }) {
        return true;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "cargo.toml"
                    | "cargo.lock"
                    | "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "tsconfig.json"
                    | "deno.json"
                    | "deno.jsonc"
                    | "jest.config.js"
                    | "jest.config.ts"
                    | "vite.config.ts"
                    | "vite.config.js"
                    | "webpack.config.js"
                    | "webpack.config.ts"
                    | "next.config.js"
                    | "next.config.mjs"
                    | "pyproject.toml"
                    | "requirements.txt"
                    | "makefile"
                    | "dockerfile"
            )
        })
}

fn workspace_has_git_repo(workspace_root: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn automation_node_execution_mode(node: &AutomationFlowNode, workspace_root: &str) -> &'static str {
    if !automation_node_is_code_workflow(node) {
        return "artifact_write";
    }
    if workspace_has_git_repo(workspace_root) {
        "git_patch"
    } else {
        "filesystem_patch"
    }
}

fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let mut normalized = config::channels::normalize_allowed_tools(raw);
    if normalized.iter().any(|tool| tool == "*") {
        return vec!["*".to_string()];
    }
    match automation_node_execution_mode(node, workspace_root) {
        "git_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        "filesystem_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        _ => {
            if automation_node_required_output_path(node).is_some() {
                normalized.push("write".to_string());
            }
        }
    }
    let has_read = normalized.iter().any(|tool| tool == "read");
    let has_workspace_probe = normalized
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list"));
    if has_read && !has_workspace_probe {
        normalized.push("glob".to_string());
    }
    if automation_node_web_research_expected(node) {
        normalized.push("websearch".to_string());
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(crate) fn filter_requested_tools_to_available(
    requested_tools: Vec<String>,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    if requested_tools.iter().any(|tool| tool == "*") {
        return requested_tools;
    }
    requested_tools
        .into_iter()
        .filter(|tool| available_tool_names.contains(tool))
        .collect()
}

pub(crate) fn automation_node_prewrite_requirements(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    let write_required = automation_node_required_output_path(node).is_some();
    if !write_required {
        return None;
    }
    let enforcement = automation_node_output_enforcement(node);
    let required_tools = enforcement.required_tools.clone();
    let web_research_expected = enforcement_requires_external_sources(&enforcement);
    let workspace_inspection_required = requested_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list" | "read"));
    let web_research_required =
        web_research_expected && requested_tools.iter().any(|tool| tool == "websearch");
    let brief_research_node = enforcement
        .required_sections
        .iter()
        .any(|item| item == "files_reviewed");
    let research_finalize = automation_node_is_research_finalize(node);
    let has_required_read = required_tools.iter().any(|tool| tool == "read");
    let has_required_websearch = required_tools.iter().any(|tool| tool == "websearch");
    let has_any_required_tools = !required_tools.is_empty();
    let concrete_read_required = !research_finalize
        && (brief_research_node
            || has_required_read
            || enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "concrete_reads"))
        && requested_tools.iter().any(|tool| tool == "read");
    let successful_web_research_required = !research_finalize
        && (brief_research_node
            || has_required_websearch
            || enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "successful_web_research"))
        && web_research_expected
        && requested_tools.iter().any(|tool| tool == "websearch");
    Some(PrewriteRequirements {
        workspace_inspection_required: workspace_inspection_required && !research_finalize,
        web_research_required: web_research_required && !research_finalize,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node || has_any_required_tools,
        coverage_mode: if brief_research_node {
            PrewriteCoverageMode::ResearchCorpus
        } else {
            PrewriteCoverageMode::None
        },
    })
}

pub(crate) fn automation_node_output_enforcement(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputEnforcement {
    let mut enforcement = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.enforcement.clone())
        .unwrap_or_default();
    let validator_kind = automation_output_validator_kind(node);
    let legacy_required_tools = automation_node_legacy_required_tools(node);
    let legacy_web_research_expected = automation_node_legacy_web_research_expected(node);
    let is_research_contract =
        validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;

    if enforcement.required_tools.is_empty() {
        enforcement.required_tools = legacy_required_tools.clone();
        if is_research_contract && !enforcement.required_tools.iter().any(|tool| tool == "read") {
            enforcement.required_tools.push("read".to_string());
        }
        if legacy_web_research_expected
            && !enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "websearch")
        {
            enforcement.required_tools.push("websearch".to_string());
        }
    }

    if enforcement.required_evidence.is_empty() {
        if is_research_contract || enforcement.required_tools.iter().any(|tool| tool == "read") {
            enforcement
                .required_evidence
                .push("local_source_reads".to_string());
        }
        if legacy_web_research_expected
            || enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "websearch")
        {
            enforcement
                .required_evidence
                .push("external_sources".to_string());
        }
    }

    if enforcement.required_sections.is_empty() && is_research_contract {
        enforcement.required_sections.extend([
            "files_reviewed".to_string(),
            "files_not_reviewed".to_string(),
            "citations".to_string(),
        ]);
        if legacy_web_research_expected || enforcement_requires_external_sources(&enforcement) {
            enforcement
                .required_sections
                .push("web_sources_reviewed".to_string());
        }
    }

    if enforcement.prewrite_gates.is_empty() && automation_node_required_output_path(node).is_some()
    {
        enforcement
            .prewrite_gates
            .push("workspace_inspection".to_string());
        if enforcement
            .required_evidence
            .iter()
            .any(|item| item == "local_source_reads")
            || enforcement.required_tools.iter().any(|tool| tool == "read")
        {
            enforcement
                .prewrite_gates
                .push("concrete_reads".to_string());
        }
        if enforcement_requires_external_sources(&enforcement) {
            enforcement
                .prewrite_gates
                .push("successful_web_research".to_string());
        }
    }

    if enforcement.retry_on_missing.is_empty() {
        enforcement
            .retry_on_missing
            .extend(enforcement.required_evidence.iter().cloned());
        enforcement
            .retry_on_missing
            .extend(enforcement.required_sections.iter().cloned());
        enforcement
            .retry_on_missing
            .extend(enforcement.prewrite_gates.iter().cloned());
    }

    if enforcement.terminal_on.is_empty() && !enforcement.retry_on_missing.is_empty() {
        enforcement.terminal_on.extend([
            "tool_unavailable".to_string(),
            "repair_budget_exhausted".to_string(),
        ]);
    }

    if enforcement.repair_budget.is_none()
        && (!enforcement.retry_on_missing.is_empty() || !enforcement.required_tools.is_empty())
    {
        enforcement.repair_budget = Some(tandem_core::prewrite_repair_retry_max_attempts() as u32);
    }

    if enforcement.session_text_recovery.is_none() {
        enforcement.session_text_recovery = Some(
            if !enforcement.prewrite_gates.is_empty()
                || enforcement
                    .required_sections
                    .iter()
                    .any(|item| item == "files_reviewed")
            {
                "require_prewrite_satisfied".to_string()
            } else {
                "allow".to_string()
            },
        );
    }

    enforcement.required_tools = normalize_non_empty_list(enforcement.required_tools);
    enforcement.required_evidence = normalize_non_empty_list(enforcement.required_evidence);
    enforcement.required_sections = normalize_non_empty_list(enforcement.required_sections);
    enforcement.prewrite_gates = normalize_non_empty_list(enforcement.prewrite_gates);
    enforcement.retry_on_missing = normalize_non_empty_list(enforcement.retry_on_missing);
    enforcement.terminal_on = normalize_non_empty_list(enforcement.terminal_on);
    enforcement
}

fn enforcement_requires_external_sources(enforcement: &crate::AutomationOutputEnforcement) -> bool {
    enforcement
        .required_evidence
        .iter()
        .any(|item| item == "external_sources")
        || enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
        || enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
}

fn automation_node_legacy_builder(
    node: &AutomationFlowNode,
) -> Option<&serde_json::Map<String, Value>> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
}

fn resolve_automation_agent_model(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Option<ModelSpec> {
    if let Some(model) = agent
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }
    template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("output_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    enforcement_requires_external_sources(&automation_node_output_enforcement(node))
}

pub(crate) fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    automation_node_output_enforcement(node).required_tools
}

fn automation_node_legacy_web_research_expected(node: &AutomationFlowNode) -> bool {
    automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("web_research_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_legacy_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("required_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_execution_policy(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> Value {
    let output_path = automation_node_required_output_path(node);
    let code_workflow = automation_node_is_code_workflow(node);
    let git_backed = workspace_has_git_repo(workspace_root);
    let mode = automation_node_execution_mode(node, workspace_root);
    let workflow_class = automation_node_workflow_class(node);
    json!({
        "mode": mode,
        "workflow_class": workflow_class,
        "code_workflow": code_workflow,
        "git_backed": git_backed,
        "declared_output_path": output_path,
        "project_backlog_tasks": automation_node_projects_backlog_tasks(node),
        "task_id": automation_node_task_id(node),
        "task_kind": automation_node_task_kind(node),
        "repo_root": automation_node_repo_root(node),
        "write_scope": automation_node_write_scope(node),
        "acceptance_criteria": automation_node_acceptance_criteria(node),
        "task_dependencies": automation_node_task_dependencies(node),
        "verification_state": automation_node_verification_state(node),
        "task_owner": automation_node_task_owner(node),
        "verification_command": automation_node_verification_command(node),
    })
}

fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(workspace_root);
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    if !resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(resolved)
}

fn is_suspicious_automation_marker_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with(".tandem")
        || lowered == "_automation_touch.txt"
        || lowered.contains("stage-touch")
        || lowered.ends_with("-status.txt")
        || lowered.contains("touch.txt")
}

fn list_suspicious_automation_marker_files(workspace_root: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_suspicious_automation_marker_file(path))
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn remove_suspicious_automation_marker_files(workspace_root: &str) {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_suspicious_automation_marker_file(&path) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

pub(crate) fn automation_workspace_root_file_snapshot(
    workspace_root: &str,
) -> std::collections::BTreeSet<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut snapshot = std::collections::BTreeSet::new();
    let mut stack = vec![workspace.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let display = path
                .strip_prefix(&workspace)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            snapshot.insert(display);
        }
    }
    snapshot
}

pub(crate) fn placeholder_like_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // TODO(coding-hardening): Replace this phrase-based placeholder detection with
    // structural artifact validation. The long-term design should score artifact
    // substance from session mutation history + contract-kind-specific structure
    // (sections, length, citations, required headings) rather than hard-coded text
    // markers that are brittle across providers, prompts, and languages.
    if trimmed.len() <= 160 {
        let compact = trimmed.to_ascii_lowercase();
        let status_only_markers = [
            "completed",
            "written to",
            "already written",
            "no content change",
            "no content changes",
            "confirmed",
            "preserving existing artifact",
            "finalization",
            "write completion",
        ];
        if status_only_markers
            .iter()
            .any(|marker| compact.contains(marker))
            && !compact.contains("## ")
            && !compact.contains("\n## ")
            && !compact.contains("files reviewed")
            && !compact.contains("proof points")
        {
            return true;
        }
    }
    let lowered = trimmed
        .chars()
        .take(800)
        .collect::<String>()
        .to_ascii_lowercase();
    let strong_markers = [
        "completed previously in this run",
        "preserving file creation requirement",
        "preserving current workspace output state",
        "created/updated to satisfy workflow artifact requirement",
        "see existing workspace research already completed in this run",
        "already written in prior step",
        "no content changes needed",
        "placeholder preservation note",
        "touch file",
        "status note",
        "marker file",
    ];
    if strong_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }
    let status_markers = [
        "# status",
        "## status",
        "status: blocked",
        "status: completed",
        "status: pending",
        "blocked handoff",
        "blocked note",
        "not approved yet",
        "pending approval",
    ];
    status_markers.iter().any(|marker| lowered.contains(marker)) && trimmed.len() < 280
}

fn markdown_heading_count(text: &str) -> usize {
    text.lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count()
}

fn markdown_list_item_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() && trimmed.contains('.'))
        })
        .count()
}

fn paragraph_block_count(text: &str) -> usize {
    text.split("\n\n")
        .filter(|block| {
            let trimmed = block.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count()
}

fn structural_substantive_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 180 {
        return false;
    }
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    heading_count >= 2
        || (heading_count >= 1 && paragraph_count >= 3)
        || (paragraph_count >= 4)
        || (list_count >= 5)
}

fn substantive_artifact_text(text: &str) -> bool {
    structural_substantive_artifact_text(text)
}

#[derive(Debug, Clone)]
struct ArtifactCandidateAssessment {
    source: String,
    text: String,
    length: usize,
    score: i64,
    substantive: bool,
    placeholder_like: bool,
    heading_count: usize,
    list_count: usize,
    paragraph_count: usize,
    required_section_count: usize,
    files_reviewed_present: bool,
    reviewed_paths: Vec<String>,
    reviewed_paths_backed_by_read: Vec<String>,
    unreviewed_relevant_paths: Vec<String>,
    citation_count: usize,
    web_sources_reviewed_present: bool,
}

fn artifact_required_section_count(node: &AutomationFlowNode, text: &str) -> usize {
    let lowered = text.to_ascii_lowercase();
    let headings = if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        vec![
            "workspace source audit",
            "campaign goal",
            "target audience",
            "core pain points",
            "positioning angle",
            "competitor context",
            "proof points",
            "likely objections",
            "channel considerations",
            "recommended message hierarchy",
            "files reviewed",
            "files not reviewed",
            "web sources reviewed",
        ]
    } else {
        vec![
            "files reviewed",
            "review notes",
            "approved",
            "draft",
            "summary",
        ]
    };
    headings
        .iter()
        .filter(|heading| lowered.contains(**heading))
        .count()
}

fn artifact_candidate_source_priority(source: &str) -> i64 {
    match source {
        "verified_output" => 3,
        "session_write" => 2,
        "preexisting_output" => 1,
        _ => 0,
    }
}

fn assess_artifact_candidate(
    node: &AutomationFlowNode,
    workspace_root: &str,
    source: &str,
    text: &str,
    read_paths: &[String],
    discovered_relevant_paths: &[String],
) -> ArtifactCandidateAssessment {
    let trimmed = text.trim();
    let length = trimmed.len();
    let placeholder_like = placeholder_like_artifact_text(trimmed);
    let substantive = substantive_artifact_text(trimmed);
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    let required_section_count = artifact_required_section_count(node, trimmed);
    let reviewed_paths = extract_markdown_section_paths(trimmed, "Files reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let files_not_reviewed = extract_markdown_section_paths(trimmed, "Files not reviewed")
        .into_iter()
        .filter_map(|value| normalize_workspace_display_path(workspace_root, &value))
        .collect::<Vec<_>>();
    let reviewed_paths_backed_by_read = reviewed_paths
        .iter()
        .filter(|path| read_paths.iter().any(|read| read == *path))
        .cloned()
        .collect::<Vec<_>>();
    let files_reviewed_present = files_reviewed_section_lists_paths(trimmed);
    let citation_count = markdown_citation_count(trimmed);
    let web_sources_reviewed_present = web_sources_reviewed_section_lists_sources(trimmed);
    let effective_relevant_paths = if discovered_relevant_paths.is_empty() {
        reviewed_paths.clone()
    } else {
        discovered_relevant_paths.to_vec()
    };
    let unreviewed_relevant_paths = effective_relevant_paths
        .iter()
        .filter(|path| {
            !read_paths.iter().any(|read| read == *path)
                && !files_not_reviewed.iter().any(|skipped| skipped == *path)
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut score = 0i64;
    score += artifact_candidate_source_priority(source) * 25;
    score += (length.min(12_000) / 24) as i64;
    score += (heading_count as i64) * 60;
    score += (list_count as i64) * 18;
    score += (paragraph_count as i64) * 24;
    score += (required_section_count as i64) * 160;
    if substantive {
        score += 2_000;
    }
    if files_reviewed_present {
        score += 180;
    }
    score += (citation_count.min(8) as i64) * 45;
    if web_sources_reviewed_present {
        score += 140;
    }
    if !reviewed_paths.is_empty() && reviewed_paths.len() == reviewed_paths_backed_by_read.len() {
        score += 260;
    } else if !reviewed_paths_backed_by_read.is_empty() {
        score += 90;
    }
    score -= (unreviewed_relevant_paths.len() as i64) * 220;
    if placeholder_like {
        score -= 450;
    }
    if trimmed.is_empty() {
        score -= 2_000;
    }

    ArtifactCandidateAssessment {
        source: source.to_string(),
        text: text.to_string(),
        length,
        score,
        substantive,
        placeholder_like,
        heading_count,
        list_count,
        paragraph_count,
        required_section_count,
        files_reviewed_present,
        reviewed_paths,
        reviewed_paths_backed_by_read,
        unreviewed_relevant_paths,
        citation_count,
        web_sources_reviewed_present,
    }
}

fn best_artifact_candidate(
    candidates: &[ArtifactCandidateAssessment],
) -> Option<ArtifactCandidateAssessment> {
    candidates.iter().cloned().max_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then(left.substantive.cmp(&right.substantive))
            .then(
                left.required_section_count
                    .cmp(&right.required_section_count),
            )
            .then(left.heading_count.cmp(&right.heading_count))
            .then(left.length.cmp(&right.length))
            .then(
                artifact_candidate_source_priority(&left.source)
                    .cmp(&artifact_candidate_source_priority(&right.source)),
            )
    })
}

fn markdown_section_lists_entries(
    text: &str,
    heading: &str,
    entry_matches: impl Fn(&str) -> bool,
) -> bool {
    let lowered = text.to_ascii_lowercase();
    let Some(start) = lowered.find(&heading.to_ascii_lowercase()) else {
        return false;
    };
    let tail = &text[start..];
    tail.lines().skip(1).take(24).any(|line| {
        let trimmed = line.trim();
        let bullet_like = (trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            && entry_matches(trimmed);
        let table_like = trimmed.starts_with('|')
            && !trimmed
                .chars()
                .all(|ch| matches!(ch, '|' | '-' | ':' | ' ' | '\t'))
            && entry_matches(trimmed);
        bullet_like || table_like
    })
}

fn concrete_workspace_path_like(value: &str) -> bool {
    let trimmed = value.trim().trim_matches('`');
    !trimmed.is_empty()
        && !trimmed.contains('*')
        && !trimmed.contains('?')
        && !trimmed.ends_with('/')
}

fn path_contains_wildcard_or_directory_placeholder(path: &str) -> bool {
    let trimmed = path.trim().trim_matches('`');
    trimmed.contains('*') || trimmed.contains('?') || trimmed.ends_with('/')
}

fn validate_path_array_hygiene(paths: &[String]) -> Option<String> {
    for path in paths {
        if path_contains_wildcard_or_directory_placeholder(path) {
            return Some(format!("path array contains non-concrete path: {}", path));
        }
    }
    None
}

fn files_reviewed_section_lists_paths(text: &str) -> bool {
    markdown_section_lists_entries(text, "files reviewed", |trimmed| {
        concrete_workspace_path_like(trimmed)
            && (trimmed.contains('/')
                || trimmed.contains(".md")
                || trimmed.contains(".txt")
                || trimmed.contains(".yaml")
                || trimmed.contains("readme"))
    })
}

fn markdown_citation_count(text: &str) -> usize {
    let markdown_links = text.match_indices("](").count();
    let bare_urls = text
        .split_whitespace()
        .filter(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(ch, ')' | '(' | '[' | ']' | ',' | '.' | ';' | '"' | '\'')
            });
            trimmed.starts_with("http://") || trimmed.starts_with("https://")
        })
        .count();
    markdown_links.max(bare_urls)
}

fn web_sources_reviewed_section_lists_sources(text: &str) -> bool {
    markdown_section_lists_entries(text, "web sources reviewed", |trimmed| {
        trimmed.contains("http://") || trimmed.contains("https://") || trimmed.contains("](")
    })
}

fn extract_markdown_section_paths(text: &str, heading: &str) -> Vec<String> {
    let mut collecting = false;
    let mut paths = Vec::new();
    let heading_normalized = heading.trim().to_ascii_lowercase();
    for line in text.lines() {
        let trimmed = line.trim();
        let normalized = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
        if trimmed.starts_with('#') {
            collecting = normalized == heading_normalized;
            continue;
        }
        if !collecting {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let candidate = trimmed
            .trim_start_matches(|ch: char| {
                ch == '-' || ch == '*' || ch.is_ascii_digit() || ch == '.' || ch == ')'
            })
            .trim();
        let token = candidate.split(['`', '(', ')']).find_map(|part| {
            let value = part.trim();
            if value.contains('/')
                || value.ends_with(".md")
                || value.ends_with(".txt")
                || value.ends_with(".yaml")
                || value.to_ascii_lowercase().contains("readme")
            {
                concrete_workspace_path_like(value).then(|| value.to_string())
            } else {
                None
            }
        });
        if let Some(path) = token.filter(|value| !value.is_empty()) {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn normalize_workspace_display_path(workspace_root: &str, raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    resolve_automation_output_path(workspace_root, trimmed)
        .ok()
        .and_then(|resolved| {
            resolved
                .strip_prefix(PathBuf::from(workspace_root))
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
        })
        .filter(|value| !value.is_empty())
}

fn tool_args_object(args: &Value) -> Option<std::borrow::Cow<'_, serde_json::Map<String, Value>>> {
    match args {
        Value::Object(map) => Some(std::borrow::Cow::Borrowed(map)),
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|value| match value {
                    Value::Object(map) => Some(std::borrow::Cow::Owned(map)),
                    _ => None,
                })
        }
        _ => None,
    }
}

pub(crate) fn session_read_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("read")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                continue;
            };
            if let Some(normalized) = normalize_workspace_display_path(workspace_root, path) {
                paths.push(normalized);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutomationUpstreamEvidence {
    pub(crate) read_paths: Vec<String>,
    pub(crate) discovered_relevant_paths: Vec<String>,
    pub(crate) web_research_attempted: bool,
    pub(crate) web_research_succeeded: bool,
}

async fn collect_automation_upstream_research_evidence(
    state: &AppState,
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> AutomationUpstreamEvidence {
    let mut evidence = AutomationUpstreamEvidence::default();
    let mut upstream_node_ids = node
        .input_refs
        .iter()
        .map(|input| input.from_step_id.clone())
        .collect::<Vec<_>>();
    upstream_node_ids.extend(node.depends_on.clone());
    upstream_node_ids.sort();
    upstream_node_ids.dedup();
    let flow_nodes = automation
        .flow
        .nodes
        .iter()
        .map(|entry| (entry.node_id.as_str(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    for upstream_node_id in upstream_node_ids {
        let Some(output) = run.checkpoint.node_outputs.get(&upstream_node_id) else {
            continue;
        };
        if let Some(validation) = output.get("artifact_validation") {
            if let Some(rows) = validation.get("read_paths").and_then(Value::as_array) {
                evidence
                    .read_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            if let Some(rows) = validation
                .get("discovered_relevant_paths")
                .and_then(Value::as_array)
            {
                evidence
                    .discovered_relevant_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            evidence.web_research_attempted |= validation
                .get("web_research_attempted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= validation
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        if let Some(tool_telemetry) = output.get("tool_telemetry") {
            evidence.web_research_attempted |= tool_telemetry
                .get("web_research_used")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        if let Some(session_id) = automation_output_session_id(output) {
            if let Some(session) = state.storage.get_session(&session_id).await {
                evidence
                    .read_paths
                    .extend(session_read_paths(&session, workspace_root));
                evidence
                    .discovered_relevant_paths
                    .extend(session_discovered_relevant_paths(&session, workspace_root));
                if let Some(upstream_node) = flow_nodes.get(upstream_node_id.as_str()) {
                    let requested_tools = output
                        .get("tool_telemetry")
                        .and_then(|value| value.get("requested_tools"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let telemetry = summarize_automation_tool_activity(
                        upstream_node,
                        &session,
                        &requested_tools,
                    );
                    evidence.web_research_attempted |= telemetry
                        .get("web_research_used")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    evidence.web_research_succeeded |= telemetry
                        .get("web_research_succeeded")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                }
            }
        }
    }
    evidence.read_paths.sort();
    evidence.read_paths.dedup();
    evidence.discovered_relevant_paths.sort();
    evidence.discovered_relevant_paths.dedup();
    evidence
}

fn session_discovered_relevant_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                result,
                error,
                ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("glob")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let output = result
                .as_ref()
                .and_then(|value| value.get("output"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            for line in output.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let path = PathBuf::from(trimmed);
                let resolved = if path.is_absolute() {
                    path
                } else {
                    let Ok(resolved) = resolve_automation_output_path(workspace_root, trimmed)
                    else {
                        continue;
                    };
                    resolved
                };
                if !resolved.starts_with(&workspace) {
                    continue;
                }
                if !std::fs::metadata(&resolved)
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    continue;
                }
                let display = resolved
                    .strip_prefix(&workspace)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty());
                if let Some(display) = display {
                    paths.push(display);
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn session_write_candidates_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
) -> Vec<String> {
    let Ok(target_path) = resolve_automation_output_path(workspace_root, declared_output_path)
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) = resolve_automation_output_path(workspace_root, path) else {
                continue;
            };
            if candidate_path != target_path {
                continue;
            }
            let Some(content) = args.get("content").and_then(Value::as_str) else {
                continue;
            };
            if !content.trim().is_empty() {
                candidates.push(content.to_string());
            }
        }
    }
    candidates
}

fn artifact_candidate_summary(candidate: &ArtifactCandidateAssessment, accepted: bool) -> Value {
    json!({
        "source": candidate.source,
        "length": candidate.length,
        "score": candidate.score,
        "substantive": candidate.substantive,
        "placeholder_like": candidate.placeholder_like,
        "heading_count": candidate.heading_count,
        "list_count": candidate.list_count,
        "paragraph_count": candidate.paragraph_count,
        "required_section_count": candidate.required_section_count,
        "files_reviewed_present": candidate.files_reviewed_present,
        "reviewed_paths_backed_by_read": candidate.reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": candidate.unreviewed_relevant_paths,
        "citation_count": candidate.citation_count,
        "web_sources_reviewed_present": candidate.web_sources_reviewed_present,
        "accepted": accepted,
    })
}

pub(crate) fn session_file_mutation_summary(session: &Session, workspace_root: &str) -> Value {
    let mut touched_files = Vec::<String>::new();
    let mut mutation_tool_by_file = serde_json::Map::new();
    let workspace_root_path = PathBuf::from(workspace_root);
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                continue;
            }
            let tool_name = tool.trim().to_ascii_lowercase().replace('-', "_");
            let parsed_args = tool_args_object(args);
            let candidate_paths = if tool_name == "apply_patch" {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("patchText"))
                    .and_then(Value::as_str)
                    .map(|patch| {
                        patch
                            .lines()
                            .filter_map(|line| {
                                let trimmed = line.trim();
                                trimmed
                                    .strip_prefix("*** Add File: ")
                                    .or_else(|| trimmed.strip_prefix("*** Update File: "))
                                    .or_else(|| trimmed.strip_prefix("*** Delete File: "))
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("path"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.trim().to_string()])
                    .unwrap_or_default()
            };
            for candidate in candidate_paths {
                let Some(resolved) = resolve_automation_output_path(workspace_root, &candidate)
                    .ok()
                    .or_else(|| {
                        let path = PathBuf::from(candidate.trim());
                        if path.is_absolute()
                            && tandem_core::is_within_workspace_root(&path, &workspace_root_path)
                        {
                            Some(path)
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };
                let display = resolved
                    .strip_prefix(&workspace_root_path)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| resolved.to_string_lossy().to_string());
                if !touched_files.iter().any(|existing| existing == &display) {
                    touched_files.push(display.clone());
                }
                match mutation_tool_by_file.get_mut(&display) {
                    Some(Value::Array(values)) => {
                        if !values
                            .iter()
                            .any(|value| value.as_str() == Some(tool_name.as_str()))
                        {
                            values.push(json!(tool_name.clone()));
                        }
                    }
                    _ => {
                        mutation_tool_by_file.insert(display.clone(), json!([tool_name.clone()]));
                    }
                }
            }
        }
    }
    touched_files.sort();
    json!({
        "touched_files": touched_files,
        "mutation_tool_by_file": mutation_tool_by_file,
    })
}

fn session_verification_summary(node: &AutomationFlowNode, session: &Session) -> Value {
    let verification_plan = automation_node_verification_plan(node);
    let Some(expected_command) = automation_node_verification_command(node) else {
        return json!({
            "verification_expected": false,
            "verification_command": Value::Null,
            "verification_plan": [],
            "verification_results": [],
            "verification_outcome": Value::Null,
            "verification_total": 0,
            "verification_completed": 0,
            "verification_passed_count": 0,
            "verification_failed_count": 0,
            "verification_ran": false,
            "verification_failed": false,
            "latest_verification_command": Value::Null,
            "latest_verification_failure": Value::Null,
        });
    };
    let verification_plan = if verification_plan.is_empty() {
        vec![AutomationVerificationStep {
            kind: infer_verification_kind(&expected_command),
            command: expected_command.clone(),
        }]
    } else {
        verification_plan
    };
    let mut verification_results = verification_plan
        .iter()
        .map(|step| {
            json!({
                "kind": step.kind,
                "command": step.command,
                "ran": false,
                "failed": false,
                "failure": Value::Null,
                "latest_command": Value::Null,
            })
        })
        .collect::<Vec<_>>();
    let mut verification_ran = false;
    let mut verification_failed = false;
    let mut latest_verification_command = None::<String>;
    let mut latest_verification_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                args,
                result,
                error,
            } = part
            else {
                continue;
            };
            if tool.trim().to_ascii_lowercase().replace('-', "_") != "bash" {
                continue;
            }
            let Some(command) = args.get("command").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let command_normalized = command.to_ascii_lowercase();
            let failure = if let Some(error) = error
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(error.to_string())
            } else {
                let metadata = result
                    .as_ref()
                    .and_then(|value| value.get("metadata"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let exit_code = metadata.get("exit_code").and_then(Value::as_i64);
                let timed_out = metadata
                    .get("timeout")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let cancelled = metadata
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let stderr = metadata
                    .get("stderr")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                if timed_out {
                    Some(format!("verification command timed out: {}", command))
                } else if cancelled {
                    Some(format!("verification command was cancelled: {}", command))
                } else if exit_code.is_some_and(|code| code != 0) {
                    Some(
                        stderr
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    truncate_text(&value, 240)
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "verification command failed with exit code {}: {}",
                                    exit_code.unwrap_or_default(),
                                    command
                                )
                            }),
                    )
                } else {
                    None
                }
            };
            for result in &mut verification_results {
                let Some(expected) = result.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let expected_normalized = expected.trim().to_ascii_lowercase();
                if !command_normalized.contains(&expected_normalized) {
                    continue;
                }
                verification_ran = true;
                latest_verification_command = Some(command.to_string());
                if let Some(object) = result.as_object_mut() {
                    object.insert("ran".to_string(), json!(true));
                    object.insert("latest_command".to_string(), json!(command.to_string()));
                    if let Some(failure_text) = failure.clone() {
                        verification_failed = true;
                        latest_verification_failure = Some(failure_text.clone());
                        object.insert("failed".to_string(), json!(true));
                        object.insert("failure".to_string(), json!(failure_text));
                    }
                }
            }
        }
    }
    let verification_completed = verification_results
        .iter()
        .filter(|value| value.get("ran").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let verification_failed_count = verification_results
        .iter()
        .filter(|value| {
            value
                .get("failed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let verification_passed_count = verification_results
        .iter()
        .filter(|value| {
            value.get("ran").and_then(Value::as_bool).unwrap_or(false)
                && !value
                    .get("failed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .count();
    let verification_total = verification_results.len();
    let verification_outcome = if verification_total == 0 {
        None
    } else if verification_failed_count > 0 {
        Some("failed")
    } else if verification_completed == 0 {
        Some("missing")
    } else if verification_completed < verification_total {
        Some("partial")
    } else {
        Some("passed")
    };
    json!({
        "verification_expected": true,
        "verification_command": expected_command,
        "verification_plan": verification_plan
            .iter()
            .map(|step| json!({"kind": step.kind, "command": step.command}))
            .collect::<Vec<_>>(),
        "verification_results": verification_results,
        "verification_outcome": verification_outcome,
        "verification_total": verification_total,
        "verification_completed": verification_completed,
        "verification_passed_count": verification_passed_count,
        "verification_failed_count": verification_failed_count,
        "verification_ran": verification_ran,
        "verification_failed": verification_failed,
        "latest_verification_command": latest_verification_command,
        "latest_verification_failure": latest_verification_failure,
    })
}

fn git_diff_summary_for_paths(workspace_root: &str, paths: &[String]) -> Option<Value> {
    if paths.is_empty() || !workspace_has_git_repo(workspace_root) {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace_root)
        .arg("diff")
        .arg("--stat")
        .arg("--");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        None
    } else {
        Some(json!({
            "stat": summary
        }))
    }
}

#[cfg(test)]
pub(crate) fn validate_automation_artifact_output(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
) -> (Option<(String, String)>, Value, Option<String>) {
    validate_automation_artifact_output_with_upstream(
        node,
        session,
        workspace_root,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        None,
    )
}

pub(crate) fn validate_automation_artifact_output_with_upstream(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let suspicious_after = list_suspicious_automation_marker_files(workspace_root);
    let undeclared_files_created = suspicious_after
        .iter()
        .filter(|name| !workspace_snapshot_before.contains((*name).as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut auto_cleaned = false;
    if !suspicious_after.is_empty() {
        remove_suspicious_automation_marker_files(workspace_root);
        auto_cleaned = true;
    }

    let enforcement = automation_node_output_enforcement(node);
    let validator_kind = automation_output_validator_kind(node);
    let execution_policy = automation_node_execution_policy(node, workspace_root);
    let mutation_summary = session_file_mutation_summary(session, workspace_root);
    let verification_summary = session_verification_summary(node, session);
    let touched_files = mutation_summary
        .get("touched_files")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mutation_tool_by_file = mutation_summary
        .get("mutation_tool_by_file")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rejected_reason = if undeclared_files_created.is_empty() {
        None
    } else {
        Some(format!(
            "undeclared marker files created: {}",
            undeclared_files_created.join(", ")
        ))
    };
    let mut semantic_block_reason = None::<String>;
    let mut accepted_output = verified_output;
    let mut recovered_from_session_write = false;
    let current_read_paths = session_read_paths(session, workspace_root);
    let current_discovered_relevant_paths =
        session_discovered_relevant_paths(session, workspace_root);
    let use_upstream_evidence = automation_node_is_research_finalize(node);
    let mut read_paths = current_read_paths.clone();
    let mut discovered_relevant_paths = if use_upstream_evidence {
        let mut paths = Vec::new();
        if let Some(upstream) = upstream_evidence {
            read_paths.extend(upstream.read_paths.clone());
            paths.extend(upstream.discovered_relevant_paths.clone());
        }
        paths
    } else {
        current_discovered_relevant_paths.clone()
    };
    read_paths.sort();
    read_paths.dedup();
    discovered_relevant_paths.sort();
    discovered_relevant_paths.dedup();
    let mut reviewed_paths_backed_by_read = Vec::<String>::new();
    let mut unreviewed_relevant_paths = Vec::<String>::new();
    let mut unmet_requirements = Vec::<String>::new();
    let mut repair_attempted = false;
    let mut repair_succeeded = false;
    let mut citation_count = 0usize;
    let mut web_sources_reviewed_present = false;
    let mut heading_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut artifact_candidates = Vec::<Value>::new();
    let mut accepted_candidate_source = None::<String>;
    let mut blocked_handoff_cleanup_action = None::<String>;
    let execution_mode = execution_policy
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("artifact_write");
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let enforcement_requires_evidence = !enforcement.required_tools.is_empty()
        || !enforcement.required_evidence.is_empty()
        || !enforcement.required_sections.is_empty()
        || !enforcement.prewrite_gates.is_empty();
    let parsed_status = parse_status_json(session_text);
    let structured_handoff = if handoff_only_structured_json {
        extract_structured_handoff_json(session_text)
    } else {
        None
    };
    let repair_exhausted_hint = parsed_status
        .as_ref()
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if rejected_reason.is_none() && matches!(execution_mode, "git_patch" | "filesystem_patch") {
        let unsafe_raw_write_paths = touched_files
            .iter()
            .filter(|path| workspace_snapshot_before.contains((*path).as_str()))
            .filter(|path| path_looks_like_source_file(path))
            .filter(|path| {
                mutation_tool_by_file
                    .get(*path)
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        let used_write = tools.iter().any(|value| value.as_str() == Some("write"));
                        let used_safe_patch = tools.iter().any(|value| {
                            matches!(value.as_str(), Some("edit") | Some("apply_patch"))
                        });
                        used_write && !used_safe_patch
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if !unsafe_raw_write_paths.is_empty() {
            rejected_reason = Some(format!(
                "unsafe raw source rewrite rejected: {}",
                unsafe_raw_write_paths.join(", ")
            ));
        }
    }

    if let Some((path, text)) = accepted_output.clone() {
        let session_write_candidates =
            session_write_candidates_for_output(session, workspace_root, &path);
        let requested_tools_for_contract = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let requested_has_read = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let requested_has_websearch = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|value| value.as_str() == Some("websearch"))
            });
        let current_executed_has_read = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let upstream_has_read = use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| !evidence.read_paths.is_empty());
        let executed_has_read = current_executed_has_read || upstream_has_read;
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let web_research_backend_unavailable =
            web_research_unavailable(latest_web_research_failure);
        let web_research_unavailable = !requested_has_websearch || web_research_backend_unavailable;
        let web_research_expected =
            enforcement_requires_external_sources(&enforcement) && !web_research_unavailable;
        let current_web_research_succeeded = tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let web_research_succeeded = current_web_research_succeeded
            || (use_upstream_evidence
                && upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded));
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || (use_upstream_evidence && !discovered_relevant_paths.is_empty());
        let prewrite_requirements =
            automation_node_prewrite_requirements(node, &requested_tools_for_contract);
        let session_text_recovery_requires_prewrite =
            enforcement.session_text_recovery.as_deref() == Some("require_prewrite_satisfied");
        let session_text_recovery_allowed =
            prewrite_requirements.as_ref().is_none_or(|requirements| {
                !session_text_recovery_requires_prewrite
                    || repair_exhausted_hint
                    || ((!requirements.workspace_inspection_required
                        || workspace_inspection_satisfied)
                        && (!requirements.concrete_read_required || executed_has_read)
                        && (!requirements.successful_web_research_required
                            || web_research_succeeded))
            });
        let mut candidate_assessments = session_write_candidates
            .iter()
            .map(|candidate| {
                assess_artifact_candidate(
                    node,
                    workspace_root,
                    "session_write",
                    candidate,
                    &read_paths,
                    &discovered_relevant_paths,
                )
            })
            .collect::<Vec<_>>();
        if !text.trim().is_empty() {
            candidate_assessments.push(assess_artifact_candidate(
                node,
                workspace_root,
                "verified_output",
                &text,
                &read_paths,
                &discovered_relevant_paths,
            ));
        }
        let executed_tools_for_attempt = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let current_attempt_has_recorded_activity = !executed_tools_for_attempt.is_empty()
            || !session_write_candidates.is_empty()
            || (use_upstream_evidence && upstream_evidence.is_some());
        let allow_preexisting_candidate =
            !enforcement_requires_evidence || current_attempt_has_recorded_activity;
        if allow_preexisting_candidate {
            if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty()) {
                candidate_assessments.push(assess_artifact_candidate(
                    node,
                    workspace_root,
                    "preexisting_output",
                    previous,
                    &read_paths,
                    &discovered_relevant_paths,
                ));
            }
        }
        if !allow_preexisting_candidate {
            accepted_candidate_source = Some("current_attempt_missing_activity".to_string());
        }
        let best_candidate = best_artifact_candidate(&candidate_assessments);
        artifact_candidates = candidate_assessments
            .iter()
            .map(|candidate| {
                let accepted = best_candidate.as_ref().is_some_and(|best| {
                    best.source == candidate.source && best.text == candidate.text
                });
                artifact_candidate_summary(candidate, accepted)
            })
            .collect::<Vec<_>>();
        if let Some(best) = best_candidate.clone() {
            accepted_candidate_source = Some(best.source.clone());
            reviewed_paths_backed_by_read = best.reviewed_paths_backed_by_read.clone();
            citation_count = best.citation_count;
            web_sources_reviewed_present = best.web_sources_reviewed_present;
            heading_count = best.heading_count;
            paragraph_count = best.paragraph_count;
            if discovered_relevant_paths.is_empty() {
                discovered_relevant_paths = best.reviewed_paths.clone();
            }
            unreviewed_relevant_paths = best.unreviewed_relevant_paths.clone();
            let best_is_verified_output = best.source == "verified_output" && best.text == text;
            if !best_is_verified_output {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                    }
                }
                recovered_from_session_write =
                    session_text_recovery_allowed && best.source == "session_write";
            } else {
                accepted_output = Some((path.clone(), best.text.clone()));
            }
        }
        repair_attempted = session_write_candidates.len() > 1
            && (requested_has_read || web_research_expected)
            && (!reviewed_paths_backed_by_read.is_empty()
                || !read_paths.is_empty()
                || tool_telemetry
                    .get("tool_call_counts")
                    .and_then(|value| value.get("write"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 1);
        let selected_assessment = best_candidate.as_ref();
        let required_tools_for_node = enforcement.required_tools.clone();
        let has_required_tools = !required_tools_for_node.is_empty();
        let requires_local_source_reads = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "local_source_reads");
        let requires_external_sources = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "external_sources")
            && !web_research_unavailable;
        let requires_files_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_reviewed");
        let requires_files_not_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_not_reviewed");
        let requires_citations = enforcement
            .required_sections
            .iter()
            .any(|item| item == "citations");
        let requires_web_sources_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "web_sources_reviewed")
            && !web_research_unavailable;
        let has_research_contract = requires_local_source_reads
            || requires_external_sources
            || requires_files_reviewed
            || requires_files_not_reviewed
            || requires_citations
            || requires_web_sources_reviewed;
        let requires_read = required_tools_for_node.iter().any(|tool| tool == "read");
        let requires_websearch = required_tools_for_node
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        if has_research_contract && (requested_has_read || requires_local_source_reads) {
            let missing_concrete_reads = requires_local_source_reads && !executed_has_read;
            let files_reviewed_backed = selected_assessment.is_some_and(|assessment| {
                !assessment.reviewed_paths.is_empty()
                    && assessment.reviewed_paths.len()
                        == assessment.reviewed_paths_backed_by_read.len()
            });
            let missing_file_coverage = (requires_files_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.files_reviewed_present))
                || !files_reviewed_backed
                || (requires_files_not_reviewed && !unreviewed_relevant_paths.is_empty());
            let missing_web_research = requires_external_sources && !web_research_succeeded;
            let missing_citations = requires_citations
                && !selected_assessment.is_some_and(|assessment| assessment.citation_count > 0);
            let missing_web_sources_reviewed = requires_web_sources_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.web_sources_reviewed_present);
            unmet_requirements.clear();
            let path_hygiene_failure = selected_assessment.and_then(|assessment| {
                validate_path_array_hygiene(&assessment.reviewed_paths)
                    .or_else(|| validate_path_array_hygiene(&assessment.unreviewed_relevant_paths))
            });
            if path_hygiene_failure.is_some() {
                unmet_requirements.push("files_reviewed_contains_nonconcrete_paths".to_string());
            }
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_citations {
                unmet_requirements.push("citations_missing".to_string());
            }
            if requires_files_reviewed
                && !selected_assessment.is_some_and(|assessment| assessment.files_reviewed_present)
            {
                unmet_requirements.push("files_reviewed_missing".to_string());
            }
            if requires_files_reviewed && !files_reviewed_backed {
                unmet_requirements.push("files_reviewed_not_backed_by_read".to_string());
            }
            if requires_files_not_reviewed && !unreviewed_relevant_paths.is_empty() {
                unmet_requirements.push("relevant_files_not_reviewed_or_skipped".to_string());
            }
            if missing_web_sources_reviewed {
                unmet_requirements.push("web_sources_reviewed_missing".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            let has_path_hygiene_failure = path_hygiene_failure.is_some();
            if missing_concrete_reads
                || missing_citations
                || missing_file_coverage
                || missing_web_sources_reviewed
                || missing_web_research
                || has_path_hygiene_failure
            {
                semantic_block_reason = Some(if has_path_hygiene_failure {
                    "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                        .to_string()
                } else if missing_concrete_reads {
                    "research completed without concrete file reads or required source coverage"
                        .to_string()
                } else if missing_web_research {
                    "research completed without required current web research".to_string()
                } else if !unreviewed_relevant_paths.is_empty() {
                    "research completed without covering or explicitly skipping relevant discovered files".to_string()
                } else if missing_citations {
                    "research completed without citation-backed claims".to_string()
                } else if missing_web_sources_reviewed {
                    "research completed without a web sources reviewed section".to_string()
                } else {
                    "research completed without a source-backed files reviewed section".to_string()
                });
            }
        }
        if !has_research_contract && has_required_tools {
            let missing_concrete_reads = requires_read && !executed_has_read;
            let missing_web_research =
                requires_websearch && requires_external_sources && !web_research_succeeded;
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads || missing_web_research {
                semantic_block_reason =
                    Some("artifact finalized without using required tools".to_string());
            }
        }
        if validator_kind == crate::AutomationOutputValidatorKind::GenericArtifact {
            let contract_kind = node
                .output_contract
                .as_ref()
                .map(|contract| contract.kind.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let selected = selected_assessment.cloned();
            let missing_editorial_substance =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && !selected.as_ref().is_some_and(|assessment| {
                        !assessment.placeholder_like
                            && (assessment.substantive
                                || (assessment.length >= 120 && assessment.paragraph_count >= 1))
                    });
            let missing_markdown_structure = contract_kind == "report_markdown"
                && !selected.as_ref().is_some_and(|assessment| {
                    assessment.heading_count >= 1 && assessment.paragraph_count >= 2
                });
            if missing_editorial_substance {
                unmet_requirements.push("editorial_substance_missing".to_string());
            }
            if missing_markdown_structure {
                unmet_requirements.push("markdown_structure_missing".to_string());
            }
            if semantic_block_reason.is_none()
                && (missing_editorial_substance || missing_markdown_structure)
            {
                semantic_block_reason = Some(if missing_markdown_structure {
                    "editorial artifact is missing expected markdown structure".to_string()
                } else {
                    "editorial artifact is too weak or placeholder-like".to_string()
                });
            }
        }
        let writes_blocked_handoff_artifact = accepted_output
            .as_ref()
            .map(|(_, accepted_text)| accepted_text.to_ascii_lowercase())
            .is_some_and(|lowered| {
                (lowered.contains("status: blocked")
                    || lowered.contains("blocked pending")
                    || lowered.contains("node produced a blocked handoff artifact"))
                    && (lowered.contains("cannot be finalized")
                        || lowered.contains("required source reads")
                        || lowered.contains("web research")
                        || lowered.contains("toolset available"))
            });
        if has_research_contract
            && semantic_block_reason.is_some()
            && writes_blocked_handoff_artifact
        {
            if let Some((path, _)) = accepted_output.as_ref() {
                if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty())
                {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::write(&resolved, previous);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action =
                        Some("restored_preexisting_output".to_string());
                } else {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::remove_file(&resolved);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action = Some("removed_blocked_output".to_string());
                }
            }
        }
        if rejected_reason.is_none()
            && matches!(execution_mode, "git_patch" | "filesystem_patch")
            && preexisting_output.is_some()
            && path_looks_like_source_file(&path)
            && tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| {
                    tools.iter().any(|value| value.as_str() == Some("write"))
                        && !tools.iter().any(|value| value.as_str() == Some("edit"))
                        && !tools
                            .iter()
                            .any(|value| value.as_str() == Some("apply_patch"))
                })
        {
            rejected_reason =
                Some("code workflow used raw write without patch/edit safety".to_string());
        }
        if semantic_block_reason.is_some()
            && !recovered_from_session_write
            && selected_assessment.is_some_and(|assessment| !assessment.substantive)
        {
            // TODO(coding-hardening): Fold this recovery path into a single
            // artifact-finalization step that deterministically picks the best
            // candidate before node output is wrapped, instead of patching up the
            // final file after semantic validation fires.
            if let Some(best) = selected_assessment
                .filter(|assessment| assessment.substantive)
                .cloned()
            {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                        recovered_from_session_write = best.source == "session_write";
                        repair_succeeded = true;
                        accepted_candidate_source = Some(best.source.clone());
                    }
                }
            }
        }
        if repair_attempted && semantic_block_reason.is_none() {
            repair_succeeded = true;
        }
        if semantic_block_reason.is_some()
            && enforcement_requires_evidence
            && !current_attempt_has_recorded_activity
        {
            accepted_output = None;
        }
        if semantic_block_reason.is_some() {
            let output_is_substantive =
                accepted_output.as_ref().is_some_and(|(_, accepted_text)| {
                    let assessment = assess_artifact_candidate(
                        node,
                        workspace_root,
                        "accepted_output",
                        accepted_text,
                        &read_paths,
                        &discovered_relevant_paths,
                    );
                    assessment.substantive && !assessment.placeholder_like
                });
            if output_is_substantive {
                // Research artifacts may stay on disk for operator inspection, but unmet source
                // coverage should continue to block or repair the node until requirements are
                // actually satisfied. Only non-research artifacts are allowed to clear semantic
                // validation purely because the produced file looks substantive.
                let should_clear = !has_research_contract;
                if should_clear {
                    semantic_block_reason = None;
                }
            }
        }
    }
    if accepted_output.is_some() && accepted_candidate_source.is_none() {
        accepted_candidate_source = Some("verified_output".to_string());
    }
    if handoff_only_structured_json {
        let requested_tools = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let executed_tools = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let requested_has_websearch = requested_tools
            .iter()
            .any(|value| value.as_str() == Some("websearch"));
        let executed_has_read = executed_tools
            .iter()
            .any(|value| value.as_str() == Some("read"));
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let web_research_unavailable =
            !requested_has_websearch || web_research_unavailable(latest_web_research_failure);
        let web_research_succeeded = tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || !current_discovered_relevant_paths.is_empty();
        let requires_read = enforcement.required_tools.iter().any(|tool| tool == "read");
        let requires_websearch = enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        let requires_workspace_inspection = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "workspace_inspection");
        let requires_concrete_reads = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "concrete_reads");
        let requires_successful_web_research = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
            && !web_research_unavailable;

        if structured_handoff.is_none() {
            unmet_requirements.push("structured_handoff_missing".to_string());
        }
        if requires_workspace_inspection && !workspace_inspection_satisfied {
            unmet_requirements.push("workspace_inspection_required".to_string());
        }
        if (requires_read || requires_concrete_reads) && !executed_has_read {
            unmet_requirements.push("no_concrete_reads".to_string());
        }
        if requires_concrete_reads && !executed_has_read {
            unmet_requirements.push("concrete_read_required".to_string());
        }
        if (requires_websearch || requires_successful_web_research) && !web_research_succeeded {
            unmet_requirements.push("missing_successful_web_research".to_string());
        }
        unmet_requirements.sort();
        unmet_requirements.dedup();

        if semantic_block_reason.is_none() && !unmet_requirements.is_empty() {
            semantic_block_reason = Some(
                if unmet_requirements
                    .iter()
                    .any(|item| item == "structured_handoff_missing")
                {
                    "structured handoff was not returned in the final response".to_string()
                } else if unmet_requirements
                    .iter()
                    .any(|item| item == "workspace_inspection_required")
                {
                    "structured handoff completed without required workspace inspection".to_string()
                } else if unmet_requirements
                    .iter()
                    .any(|item| item == "missing_successful_web_research")
                {
                    "structured handoff completed without required current web research".to_string()
                } else {
                    "structured handoff completed without required concrete file reads".to_string()
                },
            );
        }
    }
    let (repair_attempt, repair_attempts_remaining, repair_exhausted) = infer_artifact_repair_state(
        parsed_status.as_ref(),
        repair_attempted,
        repair_succeeded,
        semantic_block_reason.as_deref(),
        tool_telemetry,
    );
    let has_required_tools = !enforcement.required_tools.is_empty();
    let contract_requires_repair = !enforcement.retry_on_missing.is_empty()
        || has_required_tools
        || handoff_only_structured_json;
    let validation_outcome = if contract_requires_repair && semantic_block_reason.is_some() {
        if repair_exhausted {
            "blocked"
        } else {
            "needs_repair"
        }
    } else if semantic_block_reason.is_some() {
        "blocked"
    } else {
        "passed"
    };
    let should_classify = contract_requires_repair;
    let latest_web_research_failure = tool_telemetry
        .get("latest_web_research_failure")
        .and_then(Value::as_str);
    let requested_has_websearch = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|value| value.as_str() == Some("websearch"))
        });
    let web_research_expected_for_classification =
        enforcement_requires_external_sources(&enforcement)
            && requested_has_websearch
            && !web_research_unavailable(latest_web_research_failure);
    let external_research_mode = if enforcement_requires_external_sources(&enforcement) {
        if !requested_has_websearch || web_research_unavailable(latest_web_research_failure) {
            "waived_unavailable"
        } else {
            "required"
        }
    } else {
        "not_required"
    };
    let blocking_classification = if should_classify {
        classify_research_validation_state(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            latest_web_research_failure,
            repair_exhausted,
        )
        .map(str::to_string)
    } else {
        None
    };
    let required_next_tool_actions = if should_classify {
        research_required_next_tool_actions(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            &unreviewed_relevant_paths,
            latest_web_research_failure,
        )
    } else {
        Vec::new()
    };

    let metadata = json!({
        "accepted_artifact_path": accepted_output.as_ref().map(|(path, _)| path.clone()),
        "accepted_candidate_source": accepted_candidate_source,
        "rejected_artifact_reason": rejected_reason,
        "semantic_block_reason": semantic_block_reason,
        "recovered_from_session_write": recovered_from_session_write,
        "undeclared_files_created": undeclared_files_created,
        "auto_cleaned": auto_cleaned,
        "execution_policy": execution_policy,
        "touched_files": touched_files,
        "mutation_tool_by_file": Value::Object(mutation_tool_by_file),
        "verification": verification_summary,
        "git_diff_summary": git_diff_summary_for_paths(workspace_root, &touched_files),
        "read_paths": read_paths,
        "current_node_read_paths": current_read_paths,
        "discovered_relevant_paths": discovered_relevant_paths,
        "current_node_discovered_relevant_paths": current_discovered_relevant_paths,
        "reviewed_paths_backed_by_read": reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": unreviewed_relevant_paths,
        "citation_count": citation_count,
        "web_sources_reviewed_present": web_sources_reviewed_present,
        "heading_count": heading_count,
        "paragraph_count": paragraph_count,
        "web_research_attempted": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_used").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_attempted))
        } else {
            tool_telemetry.get("web_research_used").cloned().unwrap_or(json!(false))
        },
        "web_research_succeeded": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_succeeded").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded))
        } else {
            tool_telemetry.get("web_research_succeeded").cloned().unwrap_or(json!(false))
        },
        "external_research_mode": external_research_mode,
        "upstream_evidence_applied": use_upstream_evidence,
        "blocked_handoff_cleanup_action": blocked_handoff_cleanup_action,
        "repair_attempted": repair_attempted,
        "repair_attempt": repair_attempt,
        "repair_attempts_remaining": repair_attempts_remaining,
        "repair_budget_spent": repair_attempt > 0,
        "repair_succeeded": repair_succeeded,
        "repair_exhausted": repair_exhausted,
        "validation_outcome": validation_outcome,
        "blocking_classification": blocking_classification,
        "required_next_tool_actions": required_next_tool_actions,
        "unmet_requirements": unmet_requirements,
        "artifact_candidates": artifact_candidates,
        "resolved_enforcement": enforcement,
        "structured_handoff_present": structured_handoff.is_some(),
    });
    let rejected = metadata
        .get("rejected_artifact_reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    (accepted_output, metadata, rejected)
}

fn extract_session_text_output(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, MessageRole::Assistant))
        .map(|message| {
            message
                .parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text { text } | MessagePart::Reasoning { text } => {
                        Some(text.as_str())
                    }
                    MessagePart::ToolInvocation { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

pub(crate) fn parse_status_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return Some(value);
        }
    }
    for (idx, ch) in trimmed.char_indices().rev() {
        if ch != '{' {
            continue;
        }
        let candidate = trimmed[idx..].trim();
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            return Some(value);
        }
    }
    None
}

fn extract_markdown_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = text;
    while let Some(start) = remainder.find("```") {
        remainder = &remainder[start + 3..];
        let Some(line_end) = remainder.find('\n') else {
            break;
        };
        let lang = remainder[..line_end].trim().to_ascii_lowercase();
        remainder = &remainder[line_end + 1..];
        let Some(end) = remainder.find("```") else {
            break;
        };
        let block = remainder[..end].trim();
        if !block.is_empty() && (lang.is_empty() || lang == "json" || lang == "javascript") {
            blocks.push(block.to_string());
        }
        remainder = &remainder[end + 3..];
    }
    blocks
}

fn extract_loose_json_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut start = None::<usize>;
    let mut stack = Vec::<char>::new();
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if stack.is_empty() {
                    start = Some(idx);
                }
                stack.push('}');
            }
            '[' => {
                if stack.is_empty() {
                    start = Some(idx);
                }
                stack.push(']');
            }
            '}' | ']' => {
                let Some(expected) = stack.pop() else {
                    continue;
                };
                if ch != expected {
                    stack.clear();
                    start = None;
                    continue;
                }
                if stack.is_empty() {
                    if let Some(begin) = start.take() {
                        if let Some(block) = text.get(begin..=idx) {
                            blocks.push(block.trim().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    blocks
}

fn automation_session_text_is_tool_summary_fallback(raw: &str) -> bool {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.contains("model returned no final narrative text")
        || lowered.contains("tool result summary:")
}

fn automation_json_looks_like_status_payload(value: &Value) -> bool {
    let Value::Object(map) = value else {
        return false;
    };
    if !map.contains_key("status") {
        return false;
    }
    map.keys().all(|key| {
        matches!(
            key.as_str(),
            "status"
                | "approved"
                | "reason"
                | "summary"
                | "failureCode"
                | "failure_code"
                | "repairAttempt"
                | "repairAttemptsRemaining"
                | "repairExhausted"
                | "unmetRequirements"
                | "phase"
        )
    })
}

pub(crate) fn extract_structured_handoff_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || automation_session_text_is_tool_summary_fallback(trimmed) {
        return None;
    }

    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut candidates = Vec::<String>::new();

    for candidate in std::iter::once(trimmed.to_string())
        .chain(extract_markdown_json_blocks(trimmed))
        .chain(extract_loose_json_blocks(trimmed))
    {
        let normalized = candidate.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        candidates.push(normalized);
    }

    candidates.into_iter().find_map(|candidate| {
        let value = serde_json::from_str::<Value>(&candidate).ok()?;
        if automation_json_looks_like_status_payload(&value) {
            None
        } else {
            Some(value)
        }
    })
}

fn parsed_status_u32(status: Option<&Value>, key: &str) -> Option<u32> {
    status
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn infer_artifact_repair_state(
    parsed_status: Option<&Value>,
    repair_attempted: bool,
    repair_succeeded: bool,
    semantic_block_reason: Option<&str>,
    tool_telemetry: &Value,
) -> (u32, u32, bool) {
    let default_budget = tandem_core::prewrite_repair_retry_max_attempts() as u32;
    let inferred_attempt = tool_telemetry
        .get("tool_call_counts")
        .and_then(|value| value.get("write"))
        .and_then(Value::as_u64)
        .and_then(|count| count.checked_sub(1))
        .map(|count| count.min(default_budget as u64) as u32)
        .unwrap_or(0);
    let repair_attempt = parsed_status_u32(parsed_status, "repairAttempt").unwrap_or_else(|| {
        if repair_attempted {
            inferred_attempt.max(1)
        } else {
            0
        }
    });
    let repair_attempts_remaining = parsed_status_u32(parsed_status, "repairAttemptsRemaining")
        .unwrap_or_else(|| default_budget.saturating_sub(repair_attempt.min(default_budget)));
    let repair_exhausted = parsed_status
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            repair_attempted
                && !repair_succeeded
                && semantic_block_reason.is_some()
                && repair_attempt >= default_budget
        });
    (repair_attempt, repair_attempts_remaining, repair_exhausted)
}

pub(crate) fn summarize_automation_tool_activity(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
) -> Value {
    let mut executed_tools = Vec::new();
    let mut counts = serde_json::Map::new();
    let mut workspace_inspection_used = false;
    let mut web_research_used = false;
    let mut web_research_succeeded = false;
    let mut latest_web_research_failure = None::<String>;
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                error,
                result,
                ..
            } = part
            else {
                continue;
            };
            let normalized = tool.trim().to_ascii_lowercase().replace('-', "_");
            let is_workspace_tool = matches!(
                normalized.as_str(),
                "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
            );
            let is_web_tool = matches!(
                normalized.as_str(),
                "websearch" | "webfetch" | "webfetch_html"
            );
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                if !executed_tools.iter().any(|entry| entry == &normalized) {
                    executed_tools.push(normalized.clone());
                }
                let next_count = counts
                    .get(&normalized)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .saturating_add(1);
                counts.insert(normalized.clone(), json!(next_count));
                if is_workspace_tool {
                    workspace_inspection_used = true;
                }
                if is_web_tool {
                    web_research_used = true;
                }
                if is_web_tool {
                    latest_web_research_failure = error
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(normalize_web_research_failure_label);
                }
                continue;
            }
            if !executed_tools.iter().any(|entry| entry == &normalized) {
                executed_tools.push(normalized.clone());
            }
            let next_count = counts
                .get(&normalized)
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_add(1);
            counts.insert(normalized.clone(), json!(next_count));
            if is_workspace_tool {
                workspace_inspection_used = true;
            }
            if is_web_tool {
                web_research_used = true;
                let metadata = result
                    .as_ref()
                    .and_then(|value| value.get("metadata"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let output = result
                    .as_ref()
                    .and_then(|value| value.get("output"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase();
                let result_error = metadata
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let timed_out = result_error
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("timeout"))
                    || output.contains("search timed out")
                    || output.contains("no results received")
                    || output.contains("timed out");
                let unavailable = result_error
                    .as_deref()
                    .is_some_and(web_research_unavailable_failure)
                    || web_research_unavailable_failure(&output);
                if result_error.is_none() && !timed_out && !unavailable && !output.is_empty() {
                    web_research_succeeded = true;
                    latest_web_research_failure = None;
                } else if latest_web_research_failure.is_none() {
                    latest_web_research_failure = result_error
                        .map(|value| normalize_web_research_failure_label(&value))
                        .or_else(|| {
                            if timed_out {
                                Some("web research timed out".to_string())
                            } else if unavailable {
                                Some(normalize_web_research_failure_label(&output))
                            } else if output.is_empty() {
                                Some("web research returned no usable output".to_string())
                            } else {
                                Some("web research returned an unusable result".to_string())
                            }
                        });
                }
            }
        }
    }
    if executed_tools.is_empty() {
        for message in &session.messages {
            for part in &message.parts {
                let MessagePart::Text { text } = part else {
                    continue;
                };
                if !text.contains("Tool result summary:") {
                    continue;
                }
                let mut current_tool = None::<String>;
                let mut current_block = String::new();
                let flush_summary_block =
                    |tool_name: &Option<String>,
                     block: &str,
                     executed_tools: &mut Vec<String>,
                     counts: &mut serde_json::Map<String, Value>,
                     workspace_inspection_used: &mut bool,
                     web_research_used: &mut bool,
                     web_research_succeeded: &mut bool,
                     latest_web_research_failure: &mut Option<String>| {
                        let Some(tool_name) = tool_name.as_ref() else {
                            return;
                        };
                        let normalized = tool_name.trim().to_ascii_lowercase().replace('-', "_");
                        if !executed_tools.iter().any(|entry| entry == &normalized) {
                            executed_tools.push(normalized.clone());
                        }
                        let next_count = counts
                            .get(&normalized)
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            .saturating_add(1);
                        counts.insert(normalized.clone(), json!(next_count));
                        if matches!(
                            normalized.as_str(),
                            "glob" | "read" | "grep" | "search" | "codesearch" | "ls" | "list"
                        ) {
                            *workspace_inspection_used = true;
                        }
                        if matches!(
                            normalized.as_str(),
                            "websearch" | "webfetch" | "webfetch_html"
                        ) {
                            *web_research_used = true;
                            let lowered = block.to_ascii_lowercase();
                            if lowered.contains("timed out")
                                || lowered.contains("no results received")
                            {
                                *latest_web_research_failure =
                                    Some("web research timed out".to_string());
                            } else if web_research_unavailable_failure(&lowered) {
                                *latest_web_research_failure =
                                    Some(normalize_web_research_failure_label(&lowered));
                            } else if !block.trim().is_empty() {
                                *web_research_succeeded = true;
                                *latest_web_research_failure = None;
                            }
                        }
                    };
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("Tool `") && trimmed.ends_with("` result:") {
                        flush_summary_block(
                            &current_tool,
                            &current_block,
                            &mut executed_tools,
                            &mut counts,
                            &mut workspace_inspection_used,
                            &mut web_research_used,
                            &mut web_research_succeeded,
                            &mut latest_web_research_failure,
                        );
                        current_block.clear();
                        current_tool = trimmed
                            .strip_prefix("Tool `")
                            .and_then(|value| value.strip_suffix("` result:"))
                            .map(str::to_string);
                        continue;
                    }
                    if current_tool.is_some() {
                        if !current_block.is_empty() {
                            current_block.push('\n');
                        }
                        current_block.push_str(trimmed);
                    }
                }
                flush_summary_block(
                    &current_tool,
                    &current_block,
                    &mut executed_tools,
                    &mut counts,
                    &mut workspace_inspection_used,
                    &mut web_research_used,
                    &mut web_research_succeeded,
                    &mut latest_web_research_failure,
                );
            }
        }
    }
    let verification = session_verification_summary(node, session);
    json!({
        "requested_tools": requested_tools,
        "executed_tools": executed_tools,
        "tool_call_counts": counts,
        "workspace_inspection_used": workspace_inspection_used,
        "web_research_used": web_research_used,
        "web_research_succeeded": web_research_succeeded,
        "latest_web_research_failure": latest_web_research_failure,
        "verification_expected": verification.get("verification_expected").cloned().unwrap_or(json!(false)),
        "verification_command": verification.get("verification_command").cloned().unwrap_or(Value::Null),
        "verification_plan": verification.get("verification_plan").cloned().unwrap_or(json!([])),
        "verification_results": verification.get("verification_results").cloned().unwrap_or(json!([])),
        "verification_outcome": verification.get("verification_outcome").cloned().unwrap_or(Value::Null),
        "verification_total": verification.get("verification_total").cloned().unwrap_or(json!(0)),
        "verification_completed": verification.get("verification_completed").cloned().unwrap_or(json!(0)),
        "verification_passed_count": verification.get("verification_passed_count").cloned().unwrap_or(json!(0)),
        "verification_failed_count": verification.get("verification_failed_count").cloned().unwrap_or(json!(0)),
        "verification_ran": verification.get("verification_ran").cloned().unwrap_or(json!(false)),
        "verification_failed": verification.get("verification_failed").cloned().unwrap_or(json!(false)),
        "latest_verification_command": verification.get("latest_verification_command").cloned().unwrap_or(Value::Null),
        "latest_verification_failure": verification.get("latest_verification_failure").cloned().unwrap_or(Value::Null),
    })
}

fn normalize_web_research_failure_label(raw: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    if lowered.contains("authorization required")
        || lowered.contains("authorization_required")
        || lowered.contains("authorize")
    {
        "web research authorization required".to_string()
    } else if lowered.contains("backend unavailable")
        || lowered.contains("backend_unavailable")
        || lowered.contains("web research unavailable")
        || lowered.contains("service unavailable")
        || lowered.contains("currently unavailable")
        || lowered.contains("connection refused")
        || lowered.contains("dns error")
        || lowered.contains("network error")
        || lowered.contains("temporarily unavailable")
    {
        "web research unavailable".to_string()
    } else if lowered.contains("timed out") || lowered.contains("timeout") {
        "web research timed out".to_string()
    } else {
        raw.trim().to_string()
    }
}

fn web_research_unavailable_failure(raw: &str) -> bool {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.contains("authorization required")
        || lowered.contains("authorization_required")
        || lowered.contains("authorize")
        || lowered.contains("backend unavailable")
        || lowered.contains("backend_unavailable")
        || lowered.contains("web research unavailable")
        || lowered.contains("service unavailable")
        || lowered.contains("currently unavailable")
        || lowered.contains("temporarily unavailable")
        || lowered.contains("timed out")
        || lowered.contains("timeout")
}

fn web_research_unavailable(latest_web_research_failure: Option<&str>) -> bool {
    latest_web_research_failure.is_some_and(web_research_unavailable_failure)
}

fn classify_research_validation_state(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    latest_web_research_failure: Option<&str>,
    repair_exhausted: bool,
) -> Option<&'static str> {
    if unmet_requirements.is_empty() {
        return None;
    }
    if unmet_requirements
        .iter()
        .any(|value| value == "structured_handoff_missing")
    {
        return Some("handoff_missing");
    }
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    if repair_exhausted {
        return Some("coverage_incomplete_after_retry");
    }
    if web_research_expected && web_research_unavailable(latest_web_research_failure) {
        return Some("tool_unavailable");
    }
    if (!requested_has_read
        && unmet_requirements.iter().any(|value| {
            matches!(
                value.as_str(),
                "no_concrete_reads" | "concrete_read_required"
            )
        }))
        || (web_research_expected
            && !requested_has_websearch
            && unmet_requirements
                .iter()
                .any(|value| value == "missing_successful_web_research"))
    {
        return Some("tool_unavailable");
    }
    if (requested_has_read && !executed_has_read)
        || (web_research_expected && requested_has_websearch && !executed_has_websearch)
    {
        return Some("tool_available_but_not_used");
    }
    Some("tool_attempted_but_failed")
}

pub(crate) fn research_required_next_tool_actions(
    requested_tools: &[Value],
    executed_tools: &[Value],
    web_research_expected: bool,
    unmet_requirements: &[String],
    unreviewed_relevant_paths: &[String],
    latest_web_research_failure: Option<&str>,
) -> Vec<String> {
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let requested_has_websearch = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_websearch = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("websearch"));
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);

    let mut actions = Vec::new();
    if has_unmet("structured_handoff_missing") {
        actions.push(
            "Return the required structured JSON handoff in the final response instead of ending after tool calls or tool summaries."
                .to_string(),
        );
    }
    if requested_has_read
        && (!executed_has_read
            || has_unmet("no_concrete_reads")
            || has_unmet("files_reviewed_not_backed_by_read"))
    {
        if unreviewed_relevant_paths.is_empty() {
            actions.push(
                "Use `read` on concrete workspace files before finalizing the brief.".to_string(),
            );
        } else {
            actions.push(format!(
                "Use `read` on the remaining relevant workspace files: {}.",
                unreviewed_relevant_paths.join(", ")
            ));
        }
    }
    if requested_has_websearch
        && web_research_expected
        && (!executed_has_websearch
            || has_unmet("missing_successful_web_research")
            || has_unmet("web_sources_reviewed_missing"))
    {
        if web_research_unavailable(latest_web_research_failure) {
            actions.push(
                "Skip `websearch` for this run because external research is unavailable. Continue with local file reads and note that web research could not be completed."
                    .to_string(),
            );
        } else {
            actions.push(
                "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
                    .to_string(),
            );
        }
    }
    if has_unmet("citations_missing") {
        actions.push(
            "Add citation-backed proof points instead of unsupported claims before writing the final brief."
                .to_string(),
        );
    }
    if has_unmet("files_reviewed_missing") {
        actions.push(
            "Include a `Files reviewed` section that lists the exact local paths you actually read in this run."
                .to_string(),
        );
    }
    if has_unmet("relevant_files_not_reviewed_or_skipped") {
        actions.push(
            "Move every discovered relevant file into either `Files reviewed` after `read`, or `Files not reviewed` with a reason. Use only exact concrete workspace-relative file paths; do not use directories or glob patterns."
                .to_string(),
        );
    }
    actions
}

pub(crate) fn detect_automation_node_status(
    node: &AutomationFlowNode,
    session_text: &str,
    verified_output: Option<&(String, String)>,
    tool_telemetry: &Value,
    artifact_validation: Option<&Value>,
) -> (String, Option<String>, Option<bool>) {
    let research_repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let validator_kind = automation_output_validator_kind(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let validation_repairable = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || has_required_tools
        || handoff_only_structured_json)
        && !research_repair_exhausted;
    let parsed = parse_status_json(session_text);
    let approved = parsed
        .as_ref()
        .and_then(|value| value.get("approved"))
        .and_then(Value::as_bool);
    let explicit_reason = parsed
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("verify_failed"))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if parsed
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("blocked"))
    {
        let has_actionable_validation = artifact_validation
            .and_then(|value| {
                value
                    .get("rejected_artifact_reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        value
                            .get("semantic_block_reason")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                    })
            })
            .is_some();
        if !validation_repairable || !has_actionable_validation {
            return ("blocked".to_string(), explicit_reason, approved);
        }
    }
    if approved == Some(false) {
        return (
            "blocked".to_string(),
            explicit_reason
                .or_else(|| Some("upstream review did not approve the output".to_string())),
            approved,
        );
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("rejected_artifact_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return ("blocked".to_string(), Some(reason), approved);
    }
    if let Some(reason) = artifact_validation.and_then(|value| {
        value
            .get("semantic_block_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }) {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(reason),
            approved,
        );
    }
    let output_text = verified_output
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let lowered = output_text
        .chars()
        .take(1600)
        .collect::<String>()
        .to_ascii_lowercase();
    // TODO(coding-hardening): Replace these content markers with structured node
    // status signals from the runtime/session wrapper. Prompt text should not be the
    // primary source of truth for blocked vs completed vs verify_failed decisions.
    let blocked_markers = [
        "status: blocked",
        "status blocked",
        "## status blocked",
        "blocked pending",
        "this brief is blocked",
        "brief is blocked",
        "partially blocked",
        "provisional",
        "path-level evidence",
        "based on filenames not content",
        "could not be confirmed from file contents",
        "could not safely cite exact file-derived claims",
        "not approved",
        "approval has not happened",
        "publication is blocked",
        "i’m blocked",
        "i'm blocked",
    ];
    // TODO(coding-hardening): Same here for verification failures. We should rely on
    // explicit verification result metadata and command outcomes, not phrase matching.
    let verify_failed_markers = [
        "status: verify_failed",
        "status verify_failed",
        "verification failed",
        "tests failed",
        "build failed",
        "lint failed",
        "verify failed",
    ];
    if verify_failed_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return (
            "verify_failed".to_string(),
            explicit_reason.or_else(|| Some("verification command failed".to_string())),
            approved,
        );
    }
    if blocked_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        let reason = explicit_reason.or_else(|| {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ReviewDecision
            {
                Some("review output was not approved".to_string())
            } else {
                Some("node produced a blocked handoff artifact".to_string())
            }
        });
        return ("blocked".to_string(), reason, approved);
    }
    let requested_tools = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let executed_tools = tool_telemetry
        .get("executed_tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let requested_has_read = requested_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let executed_has_read = executed_tools
        .iter()
        .any(|value| value.as_str() == Some("read"));
    let is_brief_contract = validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;
    let requires_read = automation_node_required_tools(node)
        .iter()
        .any(|value| value == "read");
    let verification_expected = tool_telemetry
        .get("verification_expected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_ran = tool_telemetry
        .get("verification_ran")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_failed = tool_telemetry
        .get("verification_failed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verification_outcome = tool_telemetry
        .get("verification_outcome")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let verification_completed = tool_telemetry
        .get("verification_completed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_total = tool_telemetry
        .get("verification_total")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let verification_failure_reason = tool_telemetry
        .get("latest_verification_failure")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if verification_expected && verification_failed {
        return (
            "verify_failed".to_string(),
            explicit_reason.or(verification_failure_reason),
            approved,
        );
    }
    if automation_node_is_code_workflow(node)
        && verification_expected
        && verification_outcome.as_deref() == Some("partial")
    {
        return (
            "blocked".to_string(),
            Some(format!(
                "coding task completed with only {} of {} declared verification commands run",
                verification_completed, verification_total
            )),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) && verification_expected && !verification_ran {
        return (
            "blocked".to_string(),
            Some(
                "coding task completed without running the declared verification command"
                    .to_string(),
            ),
            approved,
        );
    }
    let mentions_missing_file_evidence = lowered.contains("file contents were not")
        || lowered.contains("could not safely cite exact file-derived claims")
        || lowered.contains("could not be confirmed from file contents")
        || lowered.contains("path-level evidence")
        || lowered.contains("based on filenames not content")
        || lowered.contains("partially blocked")
        || lowered.contains("provisional")
        || lowered.contains("this brief is blocked")
        || lowered.contains("brief is blocked");
    let artifact_semantic_block = artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if ((is_brief_contract && requested_has_read && !executed_has_read)
        || (requires_read && requested_has_read && !executed_has_read))
        && (artifact_semantic_block || verified_output.is_none())
    {
        return (
            if validation_repairable {
                "needs_repair".to_string()
            } else {
                "blocked".to_string()
            },
            Some(if mentions_missing_file_evidence {
                if is_brief_contract {
                    "research brief did not read concrete workspace files, so source-backed validation is incomplete".to_string()
                } else {
                    "node did not use required read tool calls before finalizing the artifact"
                        .to_string()
                }
            } else {
                if is_brief_contract {
                    "research brief cited workspace sources without using read, so source-backed validation is incomplete".to_string()
                } else {
                    "node finalized its artifact without required concrete file reads".to_string()
                }
            }),
            approved,
        );
    }
    if automation_node_is_code_workflow(node) {
        return ("done".to_string(), explicit_reason, approved);
    }
    ("completed".to_string(), explicit_reason, approved)
}

fn automation_node_workflow_class(node: &AutomationFlowNode) -> String {
    if automation_node_is_code_workflow(node) {
        "code".to_string()
    } else if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        "research".to_string()
    } else {
        "artifact".to_string()
    }
}

pub(crate) fn detect_automation_node_failure_kind(
    node: &AutomationFlowNode,
    status: &str,
    approved: Option<bool>,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> Option<String> {
    let normalized_status = status.trim().to_ascii_lowercase();
    let reason = blocked_reason
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_unmet = |needle: &str| {
        unmet_requirements
            .iter()
            .any(|value| value.as_str() == Some(needle))
    };
    let has_required_tools = !automation_node_required_tools(node).is_empty();
    let handoff_only_structured_json = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let research_requirements_blocked = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research")
            || has_unmet("citations_missing")
            || has_unmet("web_sources_reviewed_missing")
            || has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode"));
    let required_tools_blocked = has_required_tools
        && (has_unmet("no_concrete_reads")
            || has_unmet("concrete_read_required")
            || has_unmet("missing_successful_web_research"));
    let editorial_requirements_blocked = has_unmet("editorial_substance_missing")
        || has_unmet("markdown_structure_missing")
        || has_unmet("editorial_clearance_required");
    let verification_failed = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| value.get("verification_failed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if verification_failed || normalized_status == "verify_failed" {
        return Some("verification_failed".to_string());
    }
    if let Some(rejected_reason) = artifact_validation
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if rejected_reason.contains("placeholder") {
            return Some("placeholder_overwrite_rejected".to_string());
        }
        if rejected_reason.contains("unsafe raw source rewrite")
            || rejected_reason.contains("raw write without patch/edit")
        {
            return Some("unsafe_raw_write_rejected".to_string());
        }
        return Some("artifact_rejected".to_string());
    }
    if artifact_validation
        .and_then(|value| value.get("semantic_block_reason"))
        .and_then(Value::as_str)
        .is_some()
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::ResearchBrief
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && research_requirements_blocked)
        || (has_required_tools
            && matches!(normalized_status.as_str(), "blocked" | "needs_repair")
            && required_tools_blocked)
        || (automation_output_validator_kind(node)
            == crate::AutomationOutputValidatorKind::GenericArtifact
            && normalized_status == "blocked"
            && editorial_requirements_blocked)
    {
        let repair_exhausted = artifact_validation
            .and_then(|value| value.get("repair_exhausted"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if repair_exhausted && research_requirements_blocked {
            return Some("research_retry_exhausted".to_string());
        }
        if handoff_only_structured_json && has_unmet("structured_handoff_missing") {
            return Some("structured_handoff_missing".to_string());
        }
        if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_reads".to_string());
            }
            return Some("required_tool_unused_read".to_string());
        }
        if has_unmet("missing_successful_web_research") {
            if automation_output_validator_kind(node)
                == crate::AutomationOutputValidatorKind::ResearchBrief
            {
                return Some("research_missing_web_research".to_string());
            }
            return Some("required_tool_unused_websearch".to_string());
        }
        if has_unmet("citations_missing") || has_unmet("web_sources_reviewed_missing") {
            return Some("research_citations_missing".to_string());
        }
        if has_unmet("files_reviewed_missing")
            || has_unmet("files_reviewed_not_backed_by_read")
            || has_unmet("relevant_files_not_reviewed_or_skipped")
            || has_unmet("coverage_mode")
        {
            return Some("research_coverage_failed".to_string());
        }
        if editorial_requirements_blocked {
            return Some("editorial_quality_failed".to_string());
        }
        return Some("semantic_blocked".to_string());
    }
    if normalized_status == "blocked" && approved == Some(false) {
        return Some("review_not_approved".to_string());
    }
    if normalized_status == "blocked" && reason.contains("upstream review did not approve") {
        return Some("upstream_not_approved".to_string());
    }
    if normalized_status == "failed" {
        return Some("run_failed".to_string());
    }
    if automation_node_is_code_workflow(node) && normalized_status == "done" {
        return Some("verification_passed".to_string());
    }
    None
}

pub(crate) fn build_automation_validator_summary(
    validator_kind: crate::AutomationOutputValidatorKind,
    status: &str,
    blocked_reason: Option<&str>,
    artifact_validation: Option<&Value>,
) -> crate::AutomationValidatorSummary {
    let normalized_status = status.trim().to_ascii_lowercase();
    let verification_outcome = artifact_validation
        .and_then(|value| value.get("verification"))
        .and_then(|value| {
            value
                .get("verification_outcome")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    if value
                        .get("verification_failed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("failed".to_string())
                    } else if value
                        .get("verification_ran")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        Some("passed".to_string())
                    } else {
                        None
                    }
                })
        });
    let unmet_requirements = artifact_validation
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let accepted_candidate_source = artifact_validation
        .and_then(|value| value.get("accepted_candidate_source"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let repair_attempted = artifact_validation
        .and_then(|value| value.get("repair_attempted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| tandem_core::prewrite_repair_retry_max_attempts() as u32);
    let repair_succeeded = artifact_validation
        .and_then(|value| value.get("repair_succeeded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repair_exhausted = artifact_validation
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = blocked_reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("rejected_artifact_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let outcome = match normalized_status.as_str() {
        "completed" | "done" => "passed",
        "verify_failed" => "verify_failed",
        "blocked" => "blocked",
        "failed" => "failed",
        other => other,
    }
    .to_string();
    crate::AutomationValidatorSummary {
        kind: validator_kind,
        outcome,
        reason,
        unmet_requirements,
        accepted_candidate_source,
        verification_outcome,
        repair_attempted,
        repair_attempt,
        repair_attempts_remaining,
        repair_succeeded,
        repair_exhausted,
    }
}

pub(crate) fn enrich_automation_node_output_for_contract(
    node: &AutomationFlowNode,
    output: &Value,
) -> Value {
    let Some(mut object) = output.as_object().cloned() else {
        return output.clone();
    };
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
        .to_string();
    let blocked_reason = object
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let approved = object
        .get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let artifact_validation = object.get("artifact_validation").cloned();
    let validator_kind = automation_output_validator_kind(node);

    object.insert(
        "contract_kind".to_string(),
        json!(node
            .output_contract
            .as_ref()
            .map(|row| row.kind.clone())
            .unwrap_or_else(|| "structured_json".to_string())),
    );
    object.insert("validator_kind".to_string(), json!(validator_kind));
    object.insert(
        "workflow_class".to_string(),
        json!(automation_node_workflow_class(node)),
    );
    object.insert(
        "phase".to_string(),
        json!(detect_automation_node_phase(
            node,
            &status,
            artifact_validation.as_ref()
        )),
    );
    object.insert(
        "failure_kind".to_string(),
        detect_automation_node_failure_kind(
            node,
            &status,
            Some(approved),
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )
        .map(Value::String)
        .unwrap_or(Value::Null),
    );
    object.insert(
        "validator_summary".to_string(),
        json!(build_automation_validator_summary(
            validator_kind,
            &status,
            blocked_reason.as_deref(),
            artifact_validation.as_ref(),
        )),
    );
    Value::Object(object)
}

pub(crate) fn detect_automation_node_phase(
    node: &AutomationFlowNode,
    status: &str,
    artifact_validation: Option<&Value>,
) -> String {
    let workflow_class = automation_node_workflow_class(node);
    let normalized_status = status.trim().to_ascii_lowercase();
    match workflow_class.as_str() {
        "research" => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let research_validation_blocked = artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .is_some()
                || (automation_output_validator_kind(node)
                    == crate::AutomationOutputValidatorKind::ResearchBrief
                    && normalized_status == "blocked"
                    && (has_unmet("no_concrete_reads")
                        || has_unmet("concrete_read_required")
                        || has_unmet("missing_successful_web_research")
                        || has_unmet("citations_missing")
                        || has_unmet("web_sources_reviewed_missing")
                        || has_unmet("files_reviewed_missing")
                        || has_unmet("files_reviewed_not_backed_by_read")
                        || has_unmet("relevant_files_not_reviewed_or_skipped")
                        || has_unmet("coverage_mode")));
            if research_validation_blocked {
                "research_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "research".to_string()
            }
        }
        "code" => {
            let verification_expected = artifact_validation
                .and_then(|value| value.get("verification"))
                .and_then(|value| value.get("verification_expected"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if verification_expected {
                if normalized_status == "done" {
                    "completed".to_string()
                } else {
                    "verification".to_string()
                }
            } else if normalized_status == "done" {
                "completed".to_string()
            } else {
                "implementation".to_string()
            }
        }
        _ => {
            let unmet_requirements = artifact_validation
                .and_then(|value| value.get("unmet_requirements"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_unmet = |needle: &str| {
                unmet_requirements
                    .iter()
                    .any(|value| value.as_str() == Some(needle))
            };
            let editorial_validation_blocked = (has_unmet("editorial_substance_missing")
                || has_unmet("markdown_structure_missing")
                || has_unmet("editorial_clearance_required"))
                && (artifact_validation
                    .and_then(|value| value.get("semantic_block_reason"))
                    .and_then(Value::as_str)
                    .is_some()
                    || normalized_status == "blocked");
            if editorial_validation_blocked {
                "editorial_validation".to_string()
            } else if normalized_status == "completed" {
                "completed".to_string()
            } else {
                "artifact_write".to_string()
            }
        }
    }
}

pub(crate) fn wrap_automation_node_output(
    node: &AutomationFlowNode,
    session: &Session,
    requested_tools: &[String],
    session_id: &str,
    session_text: &str,
    verified_output: Option<(String, String)>,
    artifact_validation: Option<Value>,
) -> Value {
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.clone())
        .unwrap_or_else(|| "structured_json".to_string());
    let summary = if let Some((path, _)) = verified_output.as_ref() {
        format!(
            "Verified workspace output `{}` for node `{}`.",
            path, node.node_id
        )
    } else if let Some(reason) = artifact_validation
        .as_ref()
        .and_then(|value| value.get("rejected_artifact_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        format!(
            "Artifact validation rejected node `{}` output: {}.",
            node.node_id, reason
        )
    } else if session_text.trim().is_empty() {
        format!("Node `{}` completed successfully.", node.node_id)
    } else {
        truncate_text(session_text.trim(), 240)
    };
    let primary_text = verified_output
        .as_ref()
        .map(|(_, text)| text.as_str())
        .unwrap_or_else(|| session_text.trim());
    let validator_kind = automation_output_validator_kind(node);
    let structured_handoff = if validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && verified_output.is_none()
    {
        extract_structured_handoff_json(session_text)
    } else {
        None
    };
    let structured_primary_text = structured_handoff
        .as_ref()
        .and_then(|value| serde_json::to_string_pretty(value).ok());
    let tool_telemetry = summarize_automation_tool_activity(node, session, requested_tools);
    let (status, blocked_reason, approved) = detect_automation_node_status(
        node,
        session_text,
        verified_output.as_ref(),
        &tool_telemetry,
        artifact_validation.as_ref(),
    );
    let workflow_class = automation_node_workflow_class(node);
    let phase = detect_automation_node_phase(node, &status, artifact_validation.as_ref());
    let failure_kind = detect_automation_node_failure_kind(
        node,
        &status,
        approved,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let validator_summary = build_automation_validator_summary(
        validator_kind,
        &status,
        blocked_reason.as_deref(),
        artifact_validation.as_ref(),
    );
    let content = match contract_kind.as_str() {
        "report_markdown" | "text_summary" => {
            json!({
                "text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        "urls" => json!({
            "items": [],
            "raw_text": primary_text,
            "path": verified_output.as_ref().map(|(path, _)| path.clone()),
            "raw_assistant_text": session_text.trim(),
            "session_id": session_id
        }),
        "citations" => {
            json!({
                "items": [],
                "raw_text": primary_text,
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            })
        }
        _ => {
            let mut content = json!({
                "text": structured_primary_text
                    .as_deref()
                    .unwrap_or(primary_text),
                "path": verified_output.as_ref().map(|(path, _)| path.clone()),
                "raw_assistant_text": session_text.trim(),
                "session_id": session_id
            });
            if let Some(handoff) = structured_handoff {
                if let Some(object) = content.as_object_mut() {
                    object.insert("structured_handoff".to_string(), handoff);
                }
            }
            content
        }
    };
    json!(AutomationNodeOutput {
        contract_kind,
        validator_kind: Some(validator_kind),
        validator_summary: Some(validator_summary),
        summary,
        content,
        created_at_ms: now_ms(),
        node_id: node.node_id.clone(),
        status: Some(status),
        blocked_reason,
        approved,
        workflow_class: Some(workflow_class),
        phase: Some(phase),
        failure_kind,
        tool_telemetry: Some(tool_telemetry),
        artifact_validation,
    })
}

async fn record_automation_external_actions_for_session(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> anyhow::Result<Vec<ExternalActionRecord>> {
    let actions = collect_automation_external_action_receipts(
        &state.capability_resolver.list_bindings().await?,
        run_id,
        automation,
        node,
        attempt,
        session_id,
        session,
    );
    let mut recorded = Vec::with_capacity(actions.len());
    for action in actions {
        recorded.push(state.record_external_action(action).await?);
    }
    Ok(recorded)
}

pub(crate) fn collect_automation_external_action_receipts(
    bindings: &capability_resolver::CapabilityBindingsFile,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    attempt: u32,
    session_id: &str,
    session: &Session,
) -> Vec<ExternalActionRecord> {
    if !automation_node_is_outbound_action(node) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (call_index, part) in session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .enumerate()
    {
        let MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } = part
        else {
            continue;
        };
        if error.as_ref().is_some_and(|value| !value.trim().is_empty()) || result.is_none() {
            continue;
        }
        let Some(binding) = bindings
            .bindings
            .iter()
            .find(|binding| automation_binding_matches_tool_name(binding, tool))
        else {
            continue;
        };
        let idempotency_key = crate::sha256_hex(&[
            "automation_v2",
            &automation.automation_id,
            run_id,
            &node.node_id,
            &attempt.to_string(),
            tool,
            &args.to_string(),
            &call_index.to_string(),
        ]);
        if !seen.insert(idempotency_key.clone()) {
            continue;
        }
        let source_id = format!("{run_id}:{}:{attempt}:{call_index}", node.node_id);
        let created_at_ms = now_ms();
        out.push(ExternalActionRecord {
            action_id: format!("automation-external-{}", &idempotency_key[..16]),
            operation: binding.capability_id.clone(),
            status: "posted".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some(source_id),
            routine_run_id: None,
            context_run_id: Some(format!("automation-v2-{run_id}")),
            capability_id: Some(binding.capability_id.clone()),
            provider: Some(binding.provider.clone()),
            target: automation_external_action_target(args, result.as_ref()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some(idempotency_key),
            receipt: Some(json!({
                "tool": tool,
                "args": args,
                "result": result,
            })),
            error: None,
            metadata: Some(json!({
                "automationID": automation.automation_id,
                "automationRunID": run_id,
                "nodeID": node.node_id,
                "attempt": attempt,
                "nodeObjective": node.objective,
                "sessionID": session_id,
                "tool": tool,
                "provider": binding.provider,
            })),
            created_at_ms,
            updated_at_ms: created_at_ms,
        });
    }
    out
}

fn automation_node_is_outbound_action(node: &AutomationFlowNode) -> bool {
    if node
        .metadata
        .as_ref()
        .and_then(|value| value.pointer("/builder/role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("publisher"))
    {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    [
        "publish", "post ", "send ", "notify", "deliver", "submit", "share",
    ]
    .iter()
    .any(|needle| objective.contains(needle))
}

pub(crate) fn automation_publish_editorial_block_reason(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> Option<String> {
    if !automation_node_is_outbound_action(node) {
        return None;
    }
    let mut upstream_ids = node.depends_on.clone();
    for input in &node.input_refs {
        if !upstream_ids
            .iter()
            .any(|value| value == &input.from_step_id)
        {
            upstream_ids.push(input.from_step_id.clone());
        }
    }
    let blocked_upstreams = upstream_ids
        .into_iter()
        .filter(|node_id| {
            let Some(output) = run.checkpoint.node_outputs.get(node_id) else {
                return false;
            };
            output
                .get("failure_kind")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "editorial_quality_failed")
                || output
                    .get("phase")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "editorial_validation")
                || output
                    .get("validator_summary")
                    .and_then(|value| value.get("unmet_requirements"))
                    .and_then(Value::as_array)
                    .is_some_and(|requirements| {
                        requirements.iter().any(|value| {
                            matches!(
                                value.as_str(),
                                Some("editorial_substance_missing")
                                    | Some("markdown_structure_missing")
                                    | Some("editorial_clearance_required")
                            )
                        })
                    })
        })
        .collect::<Vec<_>>();
    if blocked_upstreams.is_empty() {
        None
    } else {
        Some(format!(
            "publish step blocked until upstream editorial issues are resolved: {}",
            blocked_upstreams.join(", ")
        ))
    }
}

fn automation_binding_matches_tool_name(
    binding: &capability_resolver::CapabilityBinding,
    tool_name: &str,
) -> bool {
    binding.tool_name.eq_ignore_ascii_case(tool_name)
        || binding
            .tool_name_aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(tool_name))
}

fn automation_external_action_target(args: &Value, result: Option<&Value>) -> Option<String> {
    for candidate in [
        args.pointer("/owner_repo").and_then(Value::as_str),
        args.pointer("/repo").and_then(Value::as_str),
        args.pointer("/repository").and_then(Value::as_str),
        args.pointer("/channel").and_then(Value::as_str),
        args.pointer("/channel_id").and_then(Value::as_str),
        args.pointer("/thread_ts").and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/channel"))
            .and_then(Value::as_str),
        result
            .and_then(|value| value.pointer("/metadata/repo"))
            .and_then(Value::as_str),
    ] {
        let trimmed = candidate.map(str::trim).unwrap_or_default();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn automation_node_max_attempts(node: &AutomationFlowNode) -> u32 {
    let explicit = node
        .retry_policy
        .as_ref()
        .and_then(|value| value.get("max_attempts"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 10) as u32);
    if let Some(value) = explicit {
        return value;
    }
    if automation_output_validator_kind(node) == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
    {
        5
    } else {
        3
    }
}

pub(crate) fn automation_output_is_blocked(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("blocked"))
}

pub(crate) fn automation_output_is_verify_failed(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("verify_failed"))
}

pub(crate) fn automation_output_needs_repair(output: &Value) -> bool {
    output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("needs_repair"))
}

pub(crate) fn automation_output_repair_exhausted(output: &Value) -> bool {
    output
        .get("artifact_validation")
        .and_then(|value| value.get("repair_exhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn automation_output_failure_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_output_blocked_reason(output: &Value) -> Option<String> {
    output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_output_is_passing(output: &Value) -> bool {
    output
        .get("validator_summary")
        .and_then(|v| v.get("outcome"))
        .and_then(Value::as_str)
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("passed"))
        && output
            .get("validator_summary")
            .and_then(|v| v.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|reqs| reqs.is_empty())
            .unwrap_or(false)
}

pub(crate) fn automation_node_has_passing_artifact(
    node_id: &str,
    checkpoint: &crate::automation_v2::types::AutomationRunCheckpoint,
) -> bool {
    checkpoint
        .node_outputs
        .get(node_id)
        .map(automation_output_is_passing)
        .unwrap_or(false)
}

async fn resolve_automation_v2_workspace_root(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> String {
    if let Some(workspace_root) = automation
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    if let Some(workspace_root) = automation
        .metadata
        .as_ref()
        .and_then(|row| row.get("workspace_root"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return workspace_root;
    }
    state.workspace_index.snapshot().await.root
}

fn automation_declared_output_paths(automation: &AutomationV2Spec) -> Vec<String> {
    let mut paths = Vec::new();
    for target in &automation.output_targets {
        let trimmed = target.trim();
        if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
            paths.push(trimmed.to_string());
        }
    }
    for node in &automation.flow.nodes {
        if let Some(path) = automation_node_required_output_path(node) {
            let trimmed = path.trim();
            if !trimmed.is_empty() && !paths.iter().any(|existing| existing == trimmed) {
                paths.push(trimmed.to_string());
            }
        }
    }
    paths
}

pub(crate) async fn clear_automation_declared_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
) -> anyhow::Result<()> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    // Preserve existing declared outputs across fresh runs so a failed retry does not
    // wipe the user's last substantive artifacts. Descendant retry/requeue paths still
    // clear subtree outputs explicitly when we know which node is being reset.
    let _ = automation_declared_output_paths(automation);
    remove_suspicious_automation_marker_files(&workspace_root);
    Ok(())
}

pub async fn clear_automation_subtree_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    node_ids: &std::collections::HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let mut cleared = Vec::new();
    for node in &automation.flow.nodes {
        if !node_ids.contains(&node.node_id) {
            continue;
        }
        let Some(output_path) = automation_node_required_output_path(node) else {
            continue;
        };
        let resolved = resolve_automation_output_path(&workspace_root, &output_path)?;
        if resolved.exists() && resolved.is_file() {
            std::fs::remove_file(&resolved).map_err(|error| {
                anyhow::anyhow!(
                    "failed to clear subtree output `{}` for automation `{}`: {}",
                    output_path,
                    automation.automation_id,
                    error
                )
            })?;
            cleared.push(output_path);
        }
    }
    let had_markers = !list_suspicious_automation_marker_files(&workspace_root).is_empty();
    if had_markers {
        remove_suspicious_automation_marker_files(&workspace_root);
    }
    cleared.sort();
    cleared.dedup();
    Ok(cleared)
}

pub(crate) async fn execute_automation_v2_node(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    agent: &AutomationAgentProfile,
) -> anyhow::Result<Value> {
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{}` not found", run_id))?;
    let attempt = run
        .checkpoint
        .node_attempts
        .get(&node.node_id)
        .copied()
        .unwrap_or(1);
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let upstream_inputs = build_automation_v2_upstream_inputs(&run, node, &workspace_root)?;
    let workspace_path = PathBuf::from(&workspace_root);
    if !workspace_path.exists() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` does not exist",
            workspace_root,
            automation.automation_id
        );
    }
    if !workspace_path.is_dir() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` is not a directory",
            workspace_root,
            automation.automation_id
        );
    }
    let template = if let Some(template_id) = agent.template_id.as_deref().map(str::trim) {
        if template_id.is_empty() {
            None
        } else {
            state
                .agent_teams
                .get_template_for_workspace(&workspace_root, template_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("agent template `{}` not found", template_id))
                .map(Some)?
        }
    } else {
        None
    };
    let mut session = Session::new(
        Some(format!(
            "Automation {} / {}",
            automation.automation_id, node.node_id
        )),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    let project_id = automation_workspace_project_id(&workspace_root);
    session.project_id = Some(project_id.clone());
    session.workspace_root = Some(workspace_root.clone());
    state.storage.save_session(session).await?;

    state.add_automation_v2_session(run_id, &session_id).await;

    let mut allowlist = merge_automation_agent_allowlist(agent, template.as_ref());
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        allowlist.extend(mcp_tools.clone());
    }
    let available_tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<HashSet<_>>();
    let requested_tools = filter_requested_tools_to_available(
        normalize_automation_requested_tools(node, &workspace_root, allowlist.clone()),
        &available_tool_names,
    );
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, requested_tools.clone())
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = resolve_automation_agent_model(agent, template.as_ref());
    let required_output_path = automation_node_required_output_path(node);
    let preexisting_output = required_output_path
        .as_deref()
        .and_then(|output_path| resolve_automation_output_path(&workspace_root, output_path).ok())
        .and_then(|resolved| std::fs::read_to_string(resolved).ok());
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(&workspace_root);
    let standup_report_path = if is_agent_standup_automation(automation)
        && node.node_id == "standup_synthesis"
    {
        resolve_standup_report_path_for_run(automation, run.started_at_ms.unwrap_or_else(now_ms))
    } else {
        None
    };
    let max_attempts = automation_node_max_attempts(node);
    let mut prompt = render_automation_v2_prompt(
        automation,
        &workspace_root,
        run_id,
        node,
        attempt,
        agent,
        &upstream_inputs,
        &requested_tools,
        template
            .as_ref()
            .and_then(|value| value.system_prompt.as_deref()),
        standup_report_path.as_deref(),
        if is_agent_standup_automation(automation) {
            Some(project_id.as_str())
        } else {
            None
        },
    );
    if let Some(repair_brief) = render_automation_repair_brief(
        node,
        run.checkpoint.node_outputs.get(&node.node_id),
        attempt,
        max_attempts,
    ) {
        prompt.push_str("\n\n");
        prompt.push_str(&repair_brief);
    }
    let req = SendMessageRequest {
        parts: vec![MessagePartInput::Text { text: prompt }],
        model,
        agent: None,
        tool_mode: Some(ToolMode::Required),
        tool_allowlist: Some(requested_tools.clone()),
        context_mode: None,
        write_required: required_output_path.as_ref().map(|_| true),
        prewrite_requirements: automation_node_prewrite_requirements(node, &requested_tools),
    };
    let result = state
        .engine_loop
        .run_prompt_async_with_context(
            session_id.clone(),
            req,
            Some(format!("automation-v2:{run_id}")),
        )
        .await;

    state
        .engine_loop
        .clear_session_allowed_tools(&session_id)
        .await;
    state
        .engine_loop
        .clear_session_auto_approve_permissions(&session_id)
        .await;
    state.clear_automation_v2_session(run_id, &session_id).await;

    result?;
    let session = state
        .storage
        .get_session(&session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation session `{}` missing after run", session_id))?;
    let session_text = extract_session_text_output(&session);
    let verified_output = if let Some(output_path) = required_output_path.as_deref() {
        let resolved = resolve_automation_output_path(&workspace_root, output_path)?;
        if !resolved.exists() {
            anyhow::bail!(
                "required output `{}` was not created for node `{}`",
                output_path,
                node.node_id
            );
        }
        if !resolved.is_file() {
            anyhow::bail!(
                "required output `{}` for node `{}` is not a file",
                output_path,
                node.node_id
            );
        }
        let file_text = std::fs::read_to_string(&resolved).map_err(|error| {
            anyhow::anyhow!(
                "required output `{}` for node `{}` could not be read: {}",
                output_path,
                node.node_id,
                error
            )
        })?;
        Some((output_path.to_string(), file_text))
    } else {
        None
    };
    let tool_telemetry = summarize_automation_tool_activity(node, &session, &requested_tools);
    let upstream_evidence = if automation_node_is_research_finalize(node) {
        Some(
            collect_automation_upstream_research_evidence(
                state,
                automation,
                &run,
                node,
                &workspace_root,
            )
            .await,
        )
    } else {
        None
    };
    let (verified_output, mut artifact_validation, artifact_rejected_reason) =
        validate_automation_artifact_output_with_upstream(
            node,
            &session,
            &workspace_root,
            &session_text,
            &tool_telemetry,
            preexisting_output.as_deref(),
            verified_output,
            &workspace_snapshot_before,
            upstream_evidence.as_ref(),
        );
    let _ = artifact_rejected_reason;
    let editorial_publish_block_reason = state
        .get_automation_v2_run(run_id)
        .await
        .and_then(|run| automation_publish_editorial_block_reason(&run, node));
    if let Some(reason) = editorial_publish_block_reason.as_ref() {
        if let Some(object) = artifact_validation.as_object_mut() {
            let unmet = object
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("editorial_clearance_required"))
                {
                    rows.push(json!("editorial_clearance_required"));
                }
            }
            object
                .entry("semantic_block_reason".to_string())
                .or_insert_with(|| Value::String(reason.clone()));
        }
    }
    let external_actions = if editorial_publish_block_reason.is_some() {
        Vec::new()
    } else {
        record_automation_external_actions_for_session(
            state,
            run_id,
            automation,
            node,
            attempt,
            &session_id,
            &session,
        )
        .await?
    };
    let mut output = wrap_automation_node_output(
        node,
        &session,
        &requested_tools,
        &session_id,
        &session_text,
        verified_output,
        Some(artifact_validation),
    );
    if !external_actions.is_empty() {
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "external_actions".to_string(),
                serde_json::to_value(&external_actions)?,
            );
        }
    }
    Ok(output)
}

pub async fn run_automation_v2_executor(state: AppState) {
    crate::automation_v2::executor::run_automation_v2_executor(state).await
}
