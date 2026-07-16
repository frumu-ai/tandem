// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use futures::future::join_all;
use futures::FutureExt;
use serde_json::{json, Value};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use crate::app::state::AppState;
use crate::automation_v2::types::{AutomationRunStatus, AutomationStopKind};
use crate::util::time::now_ms;

include!("executor_helpers.rs");
include!("executor_recovery.rs");
include!("executor_retry_policy.rs");

fn normalized_output_contract_value(
    node: &crate::automation_v2::types::AutomationFlowNode,
) -> Option<Value> {
    let mut contract = node.output_contract.clone()?;
    contract.enforcement =
        Some(crate::app::state::automation::automation_node_output_enforcement(node));
    Some(serde_json::to_value(contract).unwrap_or(Value::Null))
}

pub(crate) fn publish_automation_v2_failure_event(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) {
    let actionable = matches!(
        run.status,
        AutomationRunStatus::Failed | AutomationRunStatus::Blocked
    );
    if !actionable {
        return;
    }

    let failure = run.checkpoint.last_failure.as_ref();
    let node_id = failure
        .map(|row| row.node_id.as_str())
        .or_else(|| run.checkpoint.blocked_nodes.first().map(String::as_str));
    let node = node_id.and_then(|id| automation.flow.nodes.iter().find(|row| row.node_id == id));
    let output = node_id.and_then(|id| run.checkpoint.node_outputs.get(id));
    let reason = failure
        .map(|row| row.reason.clone())
        .or_else(|| run.detail.clone())
        .or_else(|| {
            output
                .and_then(|row| {
                    row.get("blocked_reason")
                        .or_else(|| row.get("summary"))
                        .and_then(Value::as_str)
                })
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("automation run ended with status {:?}", run.status));
    let attempts = node_id
        .and_then(|id| run.checkpoint.node_attempts.get(id).copied())
        .unwrap_or(0);
    let max_attempts = node
        .map(crate::app::state::automation_node_max_attempts)
        .unwrap_or(1);
    let failure_kind = output
        .and_then(|row| row.get("failure_kind"))
        .and_then(Value::as_str);
    let status = output
        .and_then(|row| row.get("status"))
        .and_then(Value::as_str);
    let mut validation_errors = output
        .and_then(|row| row.pointer("/validator_summary/unmet_requirements"))
        .or_else(|| output.and_then(|row| row.pointer("/artifact_validation/unmet_requirements")))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_refs = output
        .and_then(|row| row.get("artifact_refs"))
        .cloned()
        .unwrap_or_else(|| {
            output
                .and_then(|row| row.get("output_path"))
                .cloned()
                .map(|path| json!([path]))
                .unwrap_or_else(|| json!([]))
        });
    let mut missing_workspace_files = output_missing_workspace_paths(output);
    let mut required_next_tool_actions = output_required_next_tool_actions(output);
    let final_error_text = [
        reason.as_str(),
        failure.map(|row| row.reason.as_str()).unwrap_or_default(),
        run.detail.as_deref().unwrap_or_default(),
    ]
    .join("\n");
    let attempt_verdict_chain = recent_node_attempt_verdicts(run, node_id);
    let attempt_review_chain = attempt_verdict_chain
        .iter()
        .filter_map(|verdict| {
            let review = verdict.get("attempt_review")?;
            Some(json!({
                "attempt": verdict.get("attempt").cloned().unwrap_or(Value::Null),
                "node_id": verdict.get("node_id").cloned().unwrap_or(Value::Null),
                "failure_class": verdict.get("failure_class").cloned().unwrap_or(Value::Null),
                "attempt_review": review,
            }))
        })
        .collect::<Vec<_>>();
    let should_include_recent_attempt_evidence =
        automation_failure_is_provider_stream_related(&final_error_text)
            || validation_errors
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
            || !missing_workspace_files.is_empty()
            || !required_next_tool_actions.is_empty()
            || !attempt_verdict_chain.is_empty();
    let recent_attempt_evidence = if should_include_recent_attempt_evidence {
        recent_node_attempt_evidence(run, node_id)
    } else {
        Vec::new()
    };
    if !recent_attempt_evidence.is_empty() {
        validation_errors =
            validation_errors_with_prior_evidence(validation_errors, &recent_attempt_evidence);
        missing_workspace_files.extend(evidence_string_array(
            &recent_attempt_evidence,
            "missing_workspace_files",
        ));
        required_next_tool_actions.extend(evidence_string_array(
            &recent_attempt_evidence,
            "required_next_tool_actions",
        ));
        dedupe_strings(&mut missing_workspace_files);
        dedupe_strings(&mut required_next_tool_actions);
    }
    let has_validation_errors = validation_errors
        .as_array()
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    let error_kind = if failure_kind == Some("verification_failed")
        || status == Some("verify_failed")
        || has_validation_errors
    {
        "validation_error"
    } else if matches!(run.status, AutomationRunStatus::Blocked) {
        "missing_config"
    } else {
        "unknown"
    };

    let mut payload = json!({
            "automation_id": run.automation_id,
            "automationID": run.automation_id,
            "workflow_id": run.automation_id,
            "workflowID": run.automation_id,
            "workflow_name": automation.name,
            "run_id": run.run_id,
            "runID": run.run_id,
            "session_id": run.latest_session_id,
            "sessionID": run.latest_session_id,
            "task_id": node_id,
            "taskID": node_id,
            "stage_id": node_id,
            "stage_name": node.map(|row| row.objective.as_str()),
            "node_id": node_id,
            "agent_id": node.map(|row| row.agent_id.as_str()),
            "agent_role": node.map(|row| row.agent_id.as_str()),
            "component": "automation_v2",
            "effective_execution_profile": run.effective_execution_profile.as_str(),
            "requested_execution_profile": run.requested_execution_profile.map(|p| p.as_str()),
            "attempt": attempts,
            "max_attempts": max_attempts,
            "retry_exhausted": attempts >= max_attempts,
            "status": match run.status {
                AutomationRunStatus::Blocked => "blocked",
                _ => "failed",
            },
            "reason": reason,
            "error": failure.map(|row| row.reason.as_str()).or(run.detail.as_deref()),
            "error_kind": error_kind,
            "expected_output": node.and_then(normalized_output_contract_value),
            "actual_output": output.and_then(|row| row.get("summary")).and_then(Value::as_str),
            "artifact_refs": artifact_refs,
            "missing_workspace_files": missing_workspace_files,
            "required_next_tool_actions": required_next_tool_actions,
            "input_refs": node.map(|row| serde_json::to_value(&row.input_refs).unwrap_or(Value::Null)).unwrap_or(Value::Null),
            "output_contract": node.and_then(normalized_output_contract_value),
            "validation_errors": validation_errors,
            "attempt_verdict_chain": attempt_verdict_chain,
            "attempt_review_chain": attempt_review_chain,
            "suggested_next_action": "Inspect the failing automation node output, fix the validation/config/tool failure, then rerun the automation.",
            "tenantContext": serde_json::to_value(&run.tenant_context).unwrap_or(Value::Null),
    });
    if !recent_attempt_evidence.is_empty() {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "recent_node_attempt_evidence".to_string(),
                Value::Array(recent_attempt_evidence),
            );
        }
    }

    state.event_bus.publish(tandem_types::EngineEvent::new(
        "automation_v2.run.failed",
        payload,
    ));
}

fn execution_error_validator_kind(
    node: &crate::automation_v2::types::AutomationFlowNode,
) -> &'static str {
    match crate::app::state::automation_output_validator_kind(node) {
        crate::AutomationOutputValidatorKind::CodePatch => "code_patch",
        crate::AutomationOutputValidatorKind::ResearchBrief => "research_brief",
        crate::AutomationOutputValidatorKind::ReviewDecision => "review_decision",
        crate::AutomationOutputValidatorKind::StructuredJson => "structured_json",
        crate::AutomationOutputValidatorKind::StandupUpdate => "standup_update",
        crate::AutomationOutputValidatorKind::GenericArtifact => "generic_artifact",
    }
}

pub(crate) fn build_node_execution_error_output_with_category(
    node: &crate::automation_v2::types::AutomationFlowNode,
    detail: &str,
    terminal: bool,
    blocker_category: &str,
) -> Value {
    let reason = normalize_execution_error_detail(detail);
    let status = if terminal { "failed" } else { "needs_repair" };
    let summary = if terminal {
        format!(
            "Node `{}` failed before producing a final response.",
            node.node_id
        )
    } else {
        format!(
            "Node `{}` failed before producing a final response and will be retried.",
            node.node_id
        )
    };
    let required_next_tool_actions = if terminal {
        Vec::new()
    } else if blocker_category == "tool_resolution_failed" {
        vec![
            "Retry this node only after the required tool capabilities are actually available."
                .to_string(),
            "Do not continue with a collapsed tool set that only exposes discovery helpers."
                .to_string(),
        ]
    } else if blocker_category == "stale_no_provider_activity" {
        vec![
            "Retry the same node execution after provider activity/heartbeat recovers.".to_string(),
            "Treat this as a stale provider/session heartbeat failure, not as a valid completed handoff."
                .to_string(),
        ]
    } else {
        vec![
            "Retry the same node execution after provider connectivity recovers.".to_string(),
            "Do not classify this attempt as a missing handoff unless a final response was actually returned."
                .to_string(),
        ]
    };
    json!({
        "status": status,
        "summary": summary,
        "blocked_reason": reason,
        "failure_kind": if terminal { "run_failed" } else { "execution_failed" },
        "blocker_category": blocker_category,
        "validator_summary": {
            "kind": execution_error_validator_kind(node),
            "outcome": status,
            "reason": reason,
            "unmet_requirements": [],
            "warning_count": 0,
        },
        "artifact_validation": {
            "blocking_classification": blocker_category,
            "required_next_tool_actions": required_next_tool_actions,
            "unmet_requirements": [],
        },
    })
}

fn build_node_execution_error_output(
    node: &crate::automation_v2::types::AutomationFlowNode,
    detail: &str,
    terminal: bool,
) -> Value {
    build_node_execution_error_output_with_category(
        node,
        detail,
        terminal,
        execution_error_blocker_category(detail),
    )
}

fn blocked_failure_kind(kind: &str) -> bool {
    matches!(
        kind,
        "research_retry_exhausted"
            | "editorial_quality_failed"
            | "semantic_blocked"
            | "review_not_approved"
            | "upstream_not_approved"
            | "artifact_rejected"
            | "unsafe_raw_write_rejected"
            | "placeholder_overwrite_rejected"
    )
}

fn approval_rejection_rollback_roots(
    automation: &crate::automation_v2::types::AutomationV2Spec,
    node_id: &str,
    checkpoint: &crate::automation_v2::types::AutomationRunCheckpoint,
) -> Vec<String> {
    let Some(node) = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)
    else {
        return Vec::new();
    };
    let flow_nodes = automation
        .flow
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<std::collections::HashMap<_, _>>();
    let is_resettable = |candidate_id: &str| {
        checkpoint
            .completed_nodes
            .iter()
            .any(|completed| completed == candidate_id)
            && flow_nodes.get(candidate_id).is_some_and(|candidate| {
                let used = checkpoint
                    .node_attempts
                    .get(candidate_id)
                    .copied()
                    .unwrap_or(0);
                let max_attempts = crate::app::state::automation_node_max_attempts(candidate);
                used < max_attempts
            })
    };

    let direct_derived_ids = node
        .depends_on
        .iter()
        .filter(|dep_id| {
            flow_nodes
                .get(dep_id.as_str())
                .is_some_and(|dep| !dep.depends_on.is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut direct_derived = direct_derived_ids
        .iter()
        .filter(|dep_id| is_resettable(dep_id))
        .cloned()
        .collect::<Vec<_>>();
    direct_derived.sort();
    direct_derived.dedup();
    if !direct_derived.is_empty() {
        return direct_derived;
    }
    if !direct_derived_ids.is_empty() {
        return Vec::new();
    }

    let mut direct_resettable = node
        .depends_on
        .iter()
        .filter(|dep_id| is_resettable(dep_id))
        .cloned()
        .collect::<Vec<_>>();
    direct_resettable.sort();
    direct_resettable.dedup();
    if !direct_resettable.is_empty() {
        return direct_resettable;
    }

    Vec::new()
}

fn relax_yolo_review_output(
    output: &mut Value,
    profile: crate::automation_v2::execution_profile::ExecutionProfile,
    validator_kind: Option<crate::AutomationOutputValidatorKind>,
) -> bool {
    if profile != crate::automation_v2::execution_profile::ExecutionProfile::Yolo
        || validator_kind != Some(crate::AutomationOutputValidatorKind::ReviewDecision)
    {
        return false;
    }

    let status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !matches!(
        status.to_ascii_lowercase().as_str(),
        "blocked" | "needs_repair" | "verify_failed" | "failed"
    ) && output.get("approved").and_then(Value::as_bool) != Some(false)
    {
        return false;
    }

    let object = match output.as_object_mut() {
        Some(object) => object,
        None => return false,
    };
    let original_status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let original_approved = object.get("approved").and_then(Value::as_bool);
    let original_reason = object
        .get("blocked_reason")
        .or_else(|| object.get("reason"))
        .or_else(|| object.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    object.insert("status".to_string(), json!("completed"));
    object.insert("approved".to_string(), json!(true));
    object.insert("failure_kind".to_string(), Value::Null);
    object.insert("blocked_reason".to_string(), Value::Null);
    object.insert("yolo_review_relaxed".to_string(), json!(true));
    if let Some(status) = original_status {
        object.insert("original_status".to_string(), json!(status));
    }
    if let Some(approved) = original_approved {
        object.insert("original_approved".to_string(), json!(approved));
    }
    if let Some(reason) = &original_reason {
        object.insert("yolo_relaxed_reason".to_string(), json!(reason));
    }

    let artifact_validation = object
        .entry("artifact_validation".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(validation) = artifact_validation.as_object_mut() {
        validation.insert("execution_profile".to_string(), json!("yolo"));
        validation.insert("effective_outcome".to_string(), json!("experimental"));
        validation.insert("original_validator_outcome".to_string(), json!("blocked"));
        validation.insert("experimental".to_string(), json!(true));
        validation.insert("warning_count".to_string(), json!(1));
        validation.insert(
            "warning_requirements".to_string(),
            json!(["yolo_review_decision_relaxed"]),
        );
        validation.insert(
            "relaxed_validator_classes".to_string(),
            json!([{
                "class": "validator_kind_specific_soft_check",
                "detail": "review decision accepted as advisory under yolo profile",
                "original_outcome": "blocked",
                "effective_outcome": "experimental"
            }]),
        );
        if let Some(reason) = original_reason.clone() {
            validation.insert("original_blocked_reason".to_string(), json!(reason));
        }
    }

    let validator_summary = object
        .entry("validator_summary".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(summary) = validator_summary.as_object_mut() {
        summary.insert("outcome".to_string(), json!("experimental"));
        summary.insert("warning_count".to_string(), json!(1));
        summary.insert(
            "warning_requirements".to_string(),
            json!(["yolo_review_decision_relaxed"]),
        );
        summary.insert(
            "reason".to_string(),
            json!("Lenient execution profile accepted this review decision as advisory."),
        );
    }

    true
}

fn yolo_relaxable_blocker_category(category: &str) -> bool {
    !category.trim().is_empty()
}

fn yolo_must_not_relax_required_source_reads(output: &Value) -> bool {
    output
        .pointer("/artifact_validation/unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str().map(str::trim).is_some_and(|raw| {
                    raw == "required_source_paths_not_read"
                        || raw == "external_mutation_failed"
                        || raw == "mcp_required_tool_failed"
                })
            })
        })
}

fn yolo_safety_blocker_category(category: &str) -> bool {
    matches!(
        category,
        "provider_auth"
            | "unauthorized_workspace"
            | "secret_access_denied"
            | "destructive_action_requires_approval"
            | "external_publish_requires_approval"
            | "tenant_policy_denied"
            | "tool_unauthorized"
            | "budget_exceeded"
            | "kill_switch_engaged"
            | "engine_lease_expired"
            | "invalid_api_token"
            | "deterministic_verification_failed"
            | "unsafe_raw_write_rejected"
            | "placeholder_overwrite_rejected"
            | "external_mutation_failed"
            | "mcp_required_tool_failed"
    )
}

fn relax_yolo_non_safety_blocker_output(
    output: &mut Value,
    profile: crate::automation_v2::execution_profile::ExecutionProfile,
) -> bool {
    if profile != crate::automation_v2::execution_profile::ExecutionProfile::Yolo {
        return false;
    }

    let status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        status.as_str(),
        "blocked" | "needs_repair" | "verify_failed" | "failed"
    ) {
        return false;
    }

    // Explicit human/approval rejections are handled by the review-specific
    // relaxer above. Do not silently override non-review safety decisions here.
    if output.get("approved").and_then(Value::as_bool) == Some(false) {
        return false;
    }

    let category = output
        .get("blocker_category")
        .or_else(|| output.pointer("/artifact_validation/blocking_classification"))
        .or_else(|| output.get("failure_kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if yolo_must_not_relax_required_source_reads(output) {
        return false;
    }
    if yolo_safety_blocker_category(&category) || !yolo_relaxable_blocker_category(&category) {
        return false;
    }

    let object = match output.as_object_mut() {
        Some(object) => object,
        None => return false,
    };
    let original_status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let original_reason = object
        .get("blocked_reason")
        .or_else(|| object.get("reason"))
        .or_else(|| object.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    object.insert("status".to_string(), json!("completed"));
    object.insert("failure_kind".to_string(), Value::Null);
    object.insert("blocked_reason".to_string(), Value::Null);
    object.insert("yolo_non_safety_blocker_relaxed".to_string(), json!(true));
    object.insert("original_blocker_category".to_string(), json!(category));
    if let Some(status) = original_status.clone() {
        object.insert("original_status".to_string(), json!(status));
    }
    if let Some(reason) = &original_reason {
        object.insert("yolo_relaxed_reason".to_string(), json!(reason));
    }
    object.entry("content".to_string()).or_insert_with(|| {
        json!({
            "status": "completed",
            "experimental": true,
            "limitations": [
                "Lenient execution accepted this node with degraded evidence because a non-safety blocker prevented a normal artifact."
            ]
        })
    });

    let artifact_validation = object
        .entry("artifact_validation".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(validation) = artifact_validation.as_object_mut() {
        validation.insert("execution_profile".to_string(), json!("yolo"));
        validation.insert("effective_outcome".to_string(), json!("experimental"));
        validation.insert("original_validator_outcome".to_string(), json!("blocked"));
        validation.insert("experimental".to_string(), json!(true));
        validation.insert("warning_count".to_string(), json!(1));
        validation.insert(
            "warning_requirements".to_string(),
            json!(["yolo_non_safety_blocker_relaxed"]),
        );
        validation.insert(
            "relaxed_validator_classes".to_string(),
            json!([{
                "class": "validator_kind_specific_soft_check",
                "detail": "non-safety blocker accepted as degraded output under yolo profile",
                "original_outcome": "blocked",
                "effective_outcome": "experimental"
            }]),
        );
        if let Some(status) = original_status {
            validation.insert("original_status".to_string(), json!(status));
        }
        if let Some(reason) = original_reason.clone() {
            validation.insert("original_blocked_reason".to_string(), json!(reason));
        }
    }

    let validator_summary = object
        .entry("validator_summary".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(summary) = validator_summary.as_object_mut() {
        summary.insert("outcome".to_string(), json!("experimental"));
        summary.insert("warning_count".to_string(), json!(1));
        summary.insert(
            "warning_requirements".to_string(),
            json!(["yolo_non_safety_blocker_relaxed"]),
        );
        summary.insert(
            "reason".to_string(),
            json!("Lenient execution profile accepted this non-safety blocker as degraded output."),
        );
    }

    true
}

/// Returns `true` when a node should be skipped entirely because an upstream
/// node that is marked as a triage gate found no work.
///
/// The triage node signals this by having `metadata.triage_gate == true` in
/// the automation spec, and outputting `has_work:false` either directly in
/// `content`, in the structured artifact handoff under
/// `content.structured_handoff`, or in structured JSON embedded in the
/// captured assistant text.
/// When skipped, downstream nodes are also unconditionally skipped via the
/// same check (`should_skip_due_to_triage_gate` is called for every pending
/// node each loop iteration after the triage output lands).
fn triage_value_has_work(value: &serde_json::Value) -> Option<bool> {
    value
        .get("has_work")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            value
                .pointer("/structured_handoff/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/structured_handoff/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/data/has_work")
                .and_then(serde_json::Value::as_bool)
        })
}

fn triage_output_has_work(output: &serde_json::Value) -> Option<bool> {
    triage_value_has_work(output).or_else(|| {
        [
            output.pointer("/content/text"),
            output.pointer("/content/raw_assistant_text"),
            output.pointer("/content/raw_text"),
            output.get("text"),
        ]
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .find_map(|text| {
            crate::app::state::automation::extraction::extract_structured_handoff_json(text)
                .as_ref()
                .and_then(triage_value_has_work)
        })
    })
}

fn should_skip_due_to_triage_gate(
    node: &crate::automation_v2::types::AutomationFlowNode,
    node_outputs: &std::collections::HashMap<String, serde_json::Value>,
    flow_nodes: &[crate::automation_v2::types::AutomationFlowNode],
) -> bool {
    let mut has_no_work_triage_parent = false;
    let mut has_real_work_parent = false;
    for dep_id in &node.depends_on {
        let dep_output = node_outputs.get(dep_id);
        // Only apply the skip when the dependency is itself a triage gate node.
        let dep_is_triage = flow_nodes
            .iter()
            .find(|n| &n.node_id == dep_id)
            .and_then(|n| n.metadata.as_ref())
            .and_then(|m| m.get("triage_gate"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !dep_is_triage {
            // Propagate: if the *dependency* was itself skipped due to triage,
            // its skip output also carries triage_skipped:true so this node
            // picks it up in the next iteration.
            let dep_triage_skipped = dep_output
                .and_then(|o| o.get("triage_skipped"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if dep_triage_skipped {
                has_no_work_triage_parent = true;
                continue;
            }
            if dep_output.is_some() {
                has_real_work_parent = true;
            }
            continue;
        }
        let has_work = dep_output.and_then(triage_output_has_work).unwrap_or(true); // default: proceed (don't skip) if field is absent
        if has_work {
            has_real_work_parent = true;
        } else {
            has_no_work_triage_parent = true;
        }
    }
    has_no_work_triage_parent && !has_real_work_parent
}

fn automation_activation_validation_failure(
    automation: &crate::automation_v2::types::AutomationV2Spec,
) -> Option<String> {
    let report = automation.plan_package_validation_report()?;
    if report.ready_for_activation {
        return None;
    }
    report
        .issues
        .iter()
        .find(|issue| issue.blocking)
        .map(|issue| {
            format!(
                "plan package not ready for activation: {} ({})",
                issue.message, issue.code
            )
        })
        .or_else(|| Some("plan package not ready for activation".to_string()))
}

fn reconcile_pending_nodes_after_node_output(
    checkpoint: &mut crate::automation_v2::types::AutomationRunCheckpoint,
    node_id: &str,
    needs_repair: bool,
    terminal_repair_block: bool,
    blocked_descendants: &std::collections::HashSet<String>,
) {
    checkpoint.pending_nodes.retain(|pending_id| {
        if pending_id == node_id {
            needs_repair && !terminal_repair_block
        } else {
            !blocked_descendants.contains(pending_id)
        }
    });
    if needs_repair
        && !terminal_repair_block
        && !checkpoint
            .pending_nodes
            .iter()
            .any(|pending_id| pending_id == node_id)
    {
        checkpoint.pending_nodes.push(node_id.to_string());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePartialFailureMode {
    ContinueIndependent,
    PauseDownstreamOnly,
    PauseAll,
}

fn automation_node_partial_failure_mode(
    node: &crate::automation_v2::types::AutomationFlowNode,
) -> RuntimePartialFailureMode {
    fn parse(value: &str) -> Option<RuntimePartialFailureMode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "continue_independent" | "continue-independent" | "continueindependent" => {
                Some(RuntimePartialFailureMode::ContinueIndependent)
            }
            "pause_downstream_only" | "pause-downstream-only" | "pausedownstreamonly" => {
                Some(RuntimePartialFailureMode::PauseDownstreamOnly)
            }
            "pause_all" | "pause-all" | "pauseall" => Some(RuntimePartialFailureMode::PauseAll),
            _ => None,
        }
    }

    node.metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("partial_failure_mode")
                .or_else(|| metadata.pointer("/builder/partial_failure_mode"))
                .or_else(|| metadata.pointer("/runtime/partial_failure_mode"))
        })
        .and_then(Value::as_str)
        .and_then(parse)
        .unwrap_or(RuntimePartialFailureMode::PauseDownstreamOnly)
}

fn blocked_nodes_for_partial_failure_mode(
    automation: &crate::automation_v2::types::AutomationV2Spec,
    checkpoint: &crate::automation_v2::types::AutomationRunCheckpoint,
    failed_node_id: &str,
) -> std::collections::HashSet<String> {
    let mode = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == failed_node_id)
        .map(automation_node_partial_failure_mode)
        .unwrap_or(RuntimePartialFailureMode::PauseDownstreamOnly);
    match mode {
        RuntimePartialFailureMode::ContinueIndependent
        | RuntimePartialFailureMode::PauseDownstreamOnly => {
            crate::app::state::collect_automation_descendants(
                automation,
                &std::iter::once(failed_node_id.to_string()).collect(),
            )
        }
        RuntimePartialFailureMode::PauseAll => checkpoint
            .pending_nodes
            .iter()
            .cloned()
            .chain(std::iter::once(failed_node_id.to_string()))
            .collect(),
    }
}

fn node_output_is_terminal_success(output: &Value) -> bool {
    let status = node_output_status(output);
    let failure_kind = node_output_failure_kind(output);
    matches!(
        status.as_str(),
        "completed" | "done" | "passed" | "accepted_with_warnings" | "skipped"
    ) && !blocked_failure_kind(&failure_kind)
        && failure_kind != "verification_failed"
        && failure_kind != "run_failed"
}

fn derive_terminal_run_state(
    automation: &crate::automation_v2::types::AutomationV2Spec,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    deadlock: bool,
) -> DerivedTerminalRunState {
    let mut blocked_nodes = run.checkpoint.blocked_nodes.clone();
    blocked_nodes.extend(crate::app::state::automation_blocked_nodes(automation, run));
    let mut failed_nodes = Vec::new();
    let mut incomplete_nodes = Vec::new();
    let pending_nodes = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let completed_nodes = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    for completed_node in &completed_nodes {
        if !run.checkpoint.node_outputs.contains_key(completed_node) {
            incomplete_nodes.push(completed_node.clone());
        }
    }
    for node in &automation.flow.nodes {
        let attempts = run
            .checkpoint
            .node_attempts
            .get(&node.node_id)
            .copied()
            .unwrap_or(0);
        let output = run.checkpoint.node_outputs.get(&node.node_id);
        let max_attempts = automation_node_max_attempts_for_recorded_output(node, output);
        if pending_nodes.contains(&node.node_id) && attempts >= max_attempts {
            // Don't flag a node as failed if its latest attempt is still
            // mid-execution. The attempt counter bumps on node_started; until
            // the outcome lands, pending + attempts>=max is an in-flight run,
            // not an exhaustion. A true exhaustion leaves a terminal-status
            // outcome in node_outputs (handled below).
            let has_outcome = run.checkpoint.node_outputs.contains_key(&node.node_id);
            if !has_outcome && !deadlock {
                continue;
            }
            failed_nodes.push(node.node_id.clone());
        }
        if pending_nodes.contains(&node.node_id)
            || blocked_nodes.iter().any(|id| id == &node.node_id)
        {
            continue;
        }
        match run.checkpoint.node_outputs.get(&node.node_id) {
            Some(output) if node_output_is_terminal_success(output) => {}
            Some(output) => {
                let status = node_output_status(output);
                let failure_kind = node_output_failure_kind(output);
                if matches!(status.as_str(), "failed" | "verify_failed")
                    || failure_kind == "verification_failed"
                    || failure_kind == "run_failed"
                {
                    failed_nodes.push(node.node_id.clone());
                } else if status == "blocked" || blocked_failure_kind(&failure_kind) {
                    blocked_nodes.push(node.node_id.clone());
                } else {
                    incomplete_nodes.push(node.node_id.clone());
                }
            }
            None => incomplete_nodes.push(node.node_id.clone()),
        }
    }
    for (node_id, output) in &run.checkpoint.node_outputs {
        let status = node_output_status(output);
        let failure_kind = node_output_failure_kind(output);
        let retryable_pending_output = automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == *node_id)
            .is_some_and(|node| {
                pending_nodes.contains(node_id)
                    && run
                        .checkpoint
                        .node_attempts
                        .get(node_id)
                        .copied()
                        .unwrap_or(0)
                        < automation_node_max_attempts_for_recorded_output(node, Some(output))
            });
        if retryable_pending_output
            && (status == "needs_repair"
                || status == "verify_failed"
                || failure_kind == "verification_failed")
        {
            continue;
        }
        if matches!(status.as_str(), "failed" | "verify_failed")
            || failure_kind == "verification_failed"
            || failure_kind == "run_failed"
        {
            failed_nodes.push(node_id.clone());
            continue;
        }
        if status == "blocked" || blocked_failure_kind(&failure_kind) {
            blocked_nodes.push(node_id.clone());
        }
    }
    blocked_nodes.sort();
    blocked_nodes.dedup();
    failed_nodes.sort();
    failed_nodes.dedup();
    incomplete_nodes.sort();
    incomplete_nodes.dedup();
    if !failed_nodes.is_empty() {
        let detail = format!(
            "automation run failed from node outcomes: {}",
            failed_nodes.join(", ")
        );
        return DerivedTerminalRunState::Failed {
            failed_nodes,
            blocked_nodes,
            detail,
        };
    }
    if !blocked_nodes.is_empty() {
        return DerivedTerminalRunState::Blocked {
            blocked_nodes,
            detail: if deadlock {
                "automation run blocked: no runnable nodes remain".to_string()
            } else {
                "automation run blocked by upstream node outcome".to_string()
            },
        };
    }
    if !incomplete_nodes.is_empty() {
        return DerivedTerminalRunState::Failed {
            failed_nodes: incomplete_nodes.clone(),
            blocked_nodes: Vec::new(),
            detail: format!(
                "automation run incomplete: terminal accounting missing for node(s): {}",
                incomplete_nodes.join(", ")
            ),
        };
    }
    if deadlock {
        DerivedTerminalRunState::Failed {
            failed_nodes: Vec::new(),
            blocked_nodes: Vec::new(),
            detail: "flow deadlock: no runnable nodes".to_string(),
        }
    } else {
        DerivedTerminalRunState::Completed
    }
}

fn apply_terminal_run_state(
    row: &mut crate::automation_v2::types::AutomationV2RunRecord,
    terminal_state: &DerivedTerminalRunState,
) {
    let finished_at_ms = now_ms();
    match terminal_state {
        DerivedTerminalRunState::Completed => {
            row.status = AutomationRunStatus::Completed;
            row.detail = Some("automation run completed".to_string());
        }
        DerivedTerminalRunState::Blocked {
            blocked_nodes,
            detail,
        } => {
            row.checkpoint.blocked_nodes = blocked_nodes.clone();
            row.status = AutomationRunStatus::Blocked;
            row.detail = Some(detail.clone());
        }
        DerivedTerminalRunState::Failed {
            failed_nodes,
            blocked_nodes,
            detail,
        } => {
            row.checkpoint.blocked_nodes = blocked_nodes.clone();
            if let Some(node_id) = failed_nodes.first() {
                row.checkpoint.last_failure =
                    Some(crate::automation_v2::types::AutomationFailureRecord {
                        node_id: node_id.clone(),
                        reason: detail.clone(),
                        failed_at_ms: finished_at_ms,
                        failure_kind: None,
                        metadata: None,
                    });
            }
            row.status = AutomationRunStatus::Failed;
            row.detail = Some(detail.clone());
        }
    }
    row.finished_at_ms = Some(row.finished_at_ms.unwrap_or(finished_at_ms));
    row.active_session_ids.clear();
    row.latest_session_id = None;
    row.active_instance_ids.clear();
}

fn finalize_existing_terminal_run(row: &mut crate::automation_v2::types::AutomationV2RunRecord) {
    row.finished_at_ms = Some(row.finished_at_ms.unwrap_or_else(now_ms));
    row.active_session_ids.clear();
    row.latest_session_id = None;
    row.active_instance_ids.clear();
}

pub async fn run_automation_v2_run(
    state: AppState,
    run: crate::automation_v2::types::AutomationV2RunRecord,
) {
    let run_id = run.run_id.clone();
    let run_claim_epoch = run.execution_claim_epoch;
    let automation = state
        .get_automation_v2(&run.automation_id)
        .await
        .or_else(|| run.automation_snapshot.clone());
    let Some(automation) = automation else {
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some("automation not found".to_string());
            })
            .await;
        return;
    };
    if automation.requires_runtime_context() && run.runtime_context.as_ref().is_none() {
        let detail = "runtime context partition missing for automation run".to_string();
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
            })
            .await;
        return;
    }
    if let Some(detail) = automation_activation_validation_failure(&automation) {
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                row.stop_reason = Some(detail.clone());
                crate::app::state::automation::lifecycle::record_automation_lifecycle_event(
                    row,
                    "run_failed_activation_validation",
                    Some(detail.clone()),
                    Some(AutomationStopKind::GuardrailStopped),
                );
            })
            .await;
        return;
    }
    let workspace_root =
        crate::app::state::resolve_automation_v2_workspace_root(&state, &automation).await;
    let workspace_path = PathBuf::from(&workspace_root);
    if !workspace_path.exists() {
        let detail = format!(
            "workspace_root `{}` for automation `{}` does not exist",
            workspace_root, automation.automation_id
        );
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                row.stop_reason = Some(detail.clone());
            })
            .await;
        return;
    }
    if !workspace_path.is_dir() {
        let detail = format!(
            "workspace_root `{}` for automation `{}` is not a directory",
            workspace_root, automation.automation_id
        );
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                row.stop_reason = Some(detail.clone());
            })
            .await;
        return;
    }
    if let Err(error) =
        crate::app::state::clear_automation_declared_outputs(&state, &automation, &run.run_id).await
    {
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(error.to_string());
            })
            .await;
        return;
    }
    let max_parallel = automation
        .execution
        .max_parallel_agents
        .unwrap_or(1)
        .clamp(1, 16) as usize;

    loop {
        let Some(latest) = state.get_automation_v2_run(&run_id).await else {
            break;
        };
        if latest.checkpoint.awaiting_gate.is_none() {
            let blocked_nodes = crate::app::state::automation_blocked_nodes(&automation, &latest);
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    row.checkpoint.blocked_nodes = blocked_nodes.clone();
                    crate::app::state::record_automation_open_phase_event(&automation, row);
                })
                .await;
        }
        if let Some(detail) = crate::app::state::automation_guardrail_failure(&automation, &latest)
        {
            let session_ids = latest.active_session_ids.clone();
            for session_id in &session_ids {
                let _ = state.cancellations.cancel(&session_id).await;
            }
            state.forget_automation_v2_sessions(&session_ids).await;
            let instance_ids = latest.active_instance_ids.clone();
            for instance_id in instance_ids {
                let _ = state
                    .agent_teams
                    .cancel_instance(&state, &instance_id, "paused by budget guardrail")
                    .await;
            }
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    row.status = AutomationRunStatus::Paused;
                    row.detail = Some(detail.clone());
                    row.pause_reason = Some(detail.clone());
                    row.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                    row.stop_reason = Some(detail.clone());
                    row.active_session_ids.clear();
                    row.active_instance_ids.clear();
                    crate::app::state::automation::lifecycle::record_automation_lifecycle_event(
                        row,
                        "run_paused",
                        Some(detail.clone()),
                        Some(AutomationStopKind::GuardrailStopped),
                    );
                })
                .await;
            break;
        }
        if matches!(
            latest.status,
            AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Completed
        ) {
            let _ = state
                .update_automation_v2_run(&run_id, finalize_existing_terminal_run)
                .await;
            break;
        }
        if matches!(
            latest.status,
            AutomationRunStatus::Paused
                | AutomationRunStatus::Pausing
                | AutomationRunStatus::AwaitingApproval
                | AutomationRunStatus::Cancelled
        ) {
            break;
        }
        if latest.checkpoint.pending_nodes.is_empty() {
            let mut terminal_state = derive_terminal_run_state(&automation, &latest, false);
            if matches!(terminal_state, DerivedTerminalRunState::Completed) {
                match assert_completion_deliverables(&automation, &latest, &workspace_root) {
                    CompletionDeliverableState::Satisfied => {}
                    CompletionDeliverableState::Repair(repair) => {
                        let _ = state
                            .update_automation_v2_run(&run_id, |row| {
                                requeue_completion_deliverable_repair(row, &repair);
                            })
                            .await;
                        continue;
                    }
                    CompletionDeliverableState::Failed { detail } => {
                        terminal_state = DerivedTerminalRunState::Failed {
                            failed_nodes: Vec::new(),
                            blocked_nodes: Vec::new(),
                            detail,
                        };
                    }
                }
            }
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    apply_terminal_run_state(row, &terminal_state);
                })
                .await;
            break;
        }

        let completed = latest
            .checkpoint
            .completed_nodes
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let pending = latest.checkpoint.pending_nodes.clone();
        let mut triage_skipped_node_ids: Vec<String> = Vec::new();
        let mut runnable = pending
            .iter()
            .filter_map(|node_id| {
                let node = automation
                    .flow
                    .nodes
                    .iter()
                    .find(|n| n.node_id == *node_id)?;
                if automation_node_recorded_attempts_exhausted(&latest, node_id, node) {
                    return None;
                }
                // Dependency check: all deps must be completed.
                if !node.depends_on.iter().all(|dep| completed.contains(dep)) {
                    return None;
                }
                // Triage gate: skip if an upstream triage node found no work.
                if should_skip_due_to_triage_gate(
                    node,
                    &latest.checkpoint.node_outputs,
                    &automation.flow.nodes,
                ) {
                    triage_skipped_node_ids.push(node_id.clone());
                    return None;
                }
                Some(node.clone())
            })
            .collect::<Vec<_>>();
        // Apply triage skips before proceeding to phase/routine filtering.
        if !triage_skipped_node_ids.is_empty() {
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    for node_id in &triage_skipped_node_ids {
                        row.checkpoint.pending_nodes.retain(|id| id != node_id);
                        if !row.checkpoint.completed_nodes.iter().any(|id| id == node_id) {
                            row.checkpoint.completed_nodes.push(node_id.clone());
                        }
                        row.checkpoint.node_outputs.insert(
                            node_id.clone(),
                            json!({
                                "status": "skipped",
                                "summary": "Skipped: upstream triage found no work.",
                                "triage_skipped": true,
                                "contract_kind": "text_summary",
                            }),
                        );
                        crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                            row,
                            "node_skipped_no_work",
                            Some(format!("node `{node_id}` skipped: upstream triage found no work")),
                            None,
                            Some(json!({ "node_id": node_id, "triage_skipped": true })),
                        );
                    }
                })
                .await;
            // Re-enter the loop so newly-depending nodes can be evaluated for skip.
            continue;
        }
        runnable = crate::app::state::automation_filter_runnable_by_open_phase(
            &automation,
            &latest,
            runnable,
        );
        runnable = crate::app::state::automation_filter_runnable_by_routine_dependencies(
            &automation,
            &latest,
            runnable,
        );
        let phase_rank = crate::app::state::automation_phase_rank_map(&automation);
        let current_open_phase_rank =
            crate::app::state::automation_current_open_phase(&automation, &latest)
                .map(|(_, rank, _)| rank);
        runnable.sort_by(|a, b| {
            crate::app::state::automation_node_sort_key(a, &phase_rank, current_open_phase_rank)
                .cmp(&crate::app::state::automation_node_sort_key(
                    b,
                    &phase_rank,
                    current_open_phase_rank,
                ))
        });
        let runnable = crate::app::state::automation_filter_runnable_by_write_scope_conflicts(
            runnable,
            max_parallel,
        );

        if runnable.is_empty() {
            let mut terminal_state = derive_terminal_run_state(&automation, &latest, true);
            if matches!(terminal_state, DerivedTerminalRunState::Completed) {
                match assert_completion_deliverables(&automation, &latest, &workspace_root) {
                    CompletionDeliverableState::Satisfied => {}
                    CompletionDeliverableState::Repair(repair) => {
                        let _ = state
                            .update_automation_v2_run(&run_id, |row| {
                                requeue_completion_deliverable_repair(row, &repair);
                            })
                            .await;
                        continue;
                    }
                    CompletionDeliverableState::Failed { detail } => {
                        terminal_state = DerivedTerminalRunState::Failed {
                            failed_nodes: Vec::new(),
                            blocked_nodes: Vec::new(),
                            detail,
                        };
                    }
                }
            }
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    apply_terminal_run_state(row, &terminal_state);
                })
                .await;
            break;
        }

        if state
            .park_first_runnable_automation_v2_wait(&latest, &runnable)
            .await
        {
            break;
        }

        let executable = runnable
            .iter()
            .filter(|node| !crate::app::state::is_automation_approval_node(node))
            .cloned()
            .collect::<Vec<_>>();
        if executable.is_empty() {
            if let Some(gate_node) = runnable
                .iter()
                .find(|node| crate::app::state::is_automation_approval_node(node))
            {
                let blocked_nodes = crate::app::state::collect_automation_descendants(
                    &automation,
                    &std::iter::once(gate_node.node_id.clone()).collect(),
                )
                .into_iter()
                .filter(|node_id| node_id != &gate_node.node_id)
                .collect::<Vec<_>>();
                let Some(gate) = crate::app::state::build_automation_pending_gate(gate_node) else {
                    let _ = state
                        .update_automation_v2_run(&run_id, |row| {
                            row.status = AutomationRunStatus::Failed;
                            row.detail = Some("approval node missing gate config".to_string());
                        })
                        .await;
                    break;
                };
                let _ = state
                    .update_automation_v2_run(&run_id, |row| {
                        crate::app::state::pause_automation_run_for_gate(
                            row,
                            gate.clone(),
                            blocked_nodes.clone(),
                        );
                    })
                    .await;
            }
            break;
        }

        let runnable_node_ids = executable
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<Vec<_>>();
        let _ = state
            .update_automation_v2_run(&run_id, |row| {
                for node_id in &runnable_node_ids {
                    let attempts = row.checkpoint.node_attempts.entry(node_id.clone()).or_insert(0);
                    *attempts += 1;
                }
                for node in &executable {
                    let attempt = row
                        .checkpoint
                        .node_attempts
                        .get(&node.node_id)
                        .copied()
                        .unwrap_or(0);
                    crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                        row,
                        "node_started",
                        Some(format!("node `{}` started", node.node_id)),
                        None,
                        Some(json!({
                            "node_id": node.node_id,
                            "agent_id": node.agent_id,
                            "objective": node.objective,
                            "attempt": attempt,
                        })),
                    );
                }
            })
            .await;

        let tasks = executable
            .iter()
            .map(|node| {
                let Some(agent) = automation
                    .agents
                    .iter()
                    .find(|a| a.agent_id == node.agent_id)
                    .cloned()
                else {
                    return futures::future::ready((
                        node.node_id.clone(),
                        node.clone(),
                        Err(anyhow::anyhow!("agent not found")),
                    ))
                    .boxed();
                };
                let state = state.clone();
                let run_id = run_id.clone();
                let automation = automation.clone();
                let node = node.clone();
                async move {
                    let result = AssertUnwindSafe(crate::app::state::execute_automation_v2_node(
                        &state,
                        &run_id,
                        &automation,
                        &node,
                        &agent,
                    ))
                    .catch_unwind()
                    .await
                    .map_err(|panic_payload| {
                        let detail = if let Some(message) = panic_payload.downcast_ref::<&str>() {
                            (*message).to_string()
                        } else if let Some(message) = panic_payload.downcast_ref::<String>() {
                            message.clone()
                        } else {
                            "unknown panic".to_string()
                        };
                        anyhow::anyhow!("node execution panicked: {}", detail)
                    })
                    .and_then(|result| result);
                    (node.node_id.clone(), node, result)
                }
                .boxed()
            })
            .collect::<Vec<_>>();
        let outcomes = join_all(tasks).await;

        let mut terminal_failure = None::<String>;
        let mut pending_retry_backoff = None;
        let run_started_at_ms = state
            .get_automation_v2_run(&run_id)
            .await
            .and_then(|row| row.started_at_ms)
            .unwrap_or_else(crate::now_ms);
        let runtime_values = crate::app::state::automation::automation_prompt_runtime_values(Some(
            run_started_at_ms,
        ));
        let latest_attempts = state
            .get_automation_v2_run(&run_id)
            .await
            .map(|row| row.checkpoint.node_attempts)
            .unwrap_or_default();
        for (node_id, node, result) in outcomes {
            match result {
                Ok(output) => {
                    let mut output = output;
                    if let Some(output_path) =
                        crate::app::state::automation::automation_node_required_output_path(&node)
                    {
                        let workspace_root =
                            crate::app::state::resolve_automation_v2_workspace_root(
                                &state,
                                &automation,
                            )
                            .await;
                        let required_output_path =
                            crate::app::state::automation::automation_node_required_output_path_with_runtime_for_run(
                                &node,
                                Some(&run_id),
                                Some(&runtime_values),
                            )
                            .unwrap_or_else(|| {
                                crate::app::state::automation::automation_runtime_placeholder_replace(
                                    &output_path,
                                    Some(&runtime_values),
                                )
                            });
                        if let Ok(resolved) =
                            crate::app::state::automation::resolve_automation_output_path_with_runtime_for_run(
                                &workspace_root,
                                &run_id,
                                &output_path,
                                Some(&runtime_values),
                            )
                        {
                            let mut observed_artifact_text =
                                if resolved.exists() && resolved.is_file() {
                                    std::fs::read_to_string(&resolved)
                                        .ok()
                                        .filter(|text| !text.trim().is_empty())
                                } else {
                                    None
                                };
                            let mut recovery_source = None::<&str>;
                            if observed_artifact_text.is_none() {
                                if let Some(session_id) =
                                    crate::app::state::automation_output_session_id(&output)
                                {
                                    if let Some(runtime) = state.runtime.get() {
                                        if let Some(session) =
                                            runtime.storage.get_session(&session_id).await
                                        {
                                            if let Some(payload) = crate::app::state::automation::extraction::extract_recoverable_json_from_session(&session) {
                                                if let Some(parent) = resolved.parent() {
                                                    let _ = std::fs::create_dir_all(parent);
                                                }
                                                if let Ok(serialized) = serde_json::to_string_pretty(&payload) {
                                                    if std::fs::write(&resolved, &serialized).is_ok() {
                                                        observed_artifact_text = Some(serialized);
                                                        recovery_source = Some("session_text_salvage");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(artifact_text) = observed_artifact_text.as_deref() {
                                promote_materialized_output(
                                    &mut output,
                                    &node,
                                    &required_output_path,
                                    artifact_text,
                                    recovery_source,
                                );
                            }
                        }
                    }
                    if let Some((path, content_digest)) =
                        crate::app::state::automation::node_output::automation_output_validated_artifact(
                            &output,
                        )
                    {
                        let mut scheduler = state.automation_scheduler.write().await;
                        scheduler.preexisting_registry.register_validated(
                            &run_id,
                            &node_id,
                            crate::app::state::automation::scheduler::ValidatedArtifact {
                                path,
                                content_digest,
                            },
                        );
                    }
                    let run_for_decision = state.get_automation_v2_run(&run_id).await;
                    let can_accept = run_for_decision
                        .as_ref()
                        .map(|row| {
                            if run_node_is_settled_completed(row, &node_id) {
                                return false;
                            }
                            matches!(
                                row.status,
                                AutomationRunStatus::Running
                                    | AutomationRunStatus::Queued
                                    | AutomationRunStatus::Failed
                            )
                        })
                        .unwrap_or(false);
                    if !can_accept {
                        continue;
                    }
                    if let Some(row) = run_for_decision.as_ref() {
                        // Profile chokepoint: when every unmet_requirement on
                        // this output is relaxable under the active profile,
                        // write structured relaxation metadata onto
                        // artifact_validation AND downgrade the executor's
                        // blocking signals so the run continues.
                        // - Guided: status -> completed_with_warnings.
                        // - Lenient: status -> completed (with experimental).
                        // Strict, critical-class outputs, and outputs whose
                        // unmet_requirements include a not-yet-classified
                        // string are returned untouched. See
                        // docs/internal/execution-profiles/PROPOSAL.md
                        // "Executor Chokepoint Invariant".
                        let tenant_denylist =
                            crate::automation_v2::execution_profile::tenant_relaxation_denylist_from_env();
                        crate::automation_v2::execution_profile::augment_output_with_profile_relaxation(
                            &mut output,
                            row.effective_execution_profile,
                            row.requested_execution_profile,
                            &tenant_denylist,
                        );
                        let validator_kind = automation
                            .flow
                            .nodes
                            .iter()
                            .find(|n| n.node_id == node_id)
                            .and_then(|node| node.output_contract.as_ref())
                            .and_then(|contract| contract.validator);
                        relax_yolo_review_output(
                            &mut output,
                            row.effective_execution_profile,
                            validator_kind,
                        );
                        relax_yolo_non_safety_blocker_output(
                            &mut output,
                            row.effective_execution_profile,
                        );
                        // Experimental-input propagation: if any upstream
                        // dependency of this node already produced an
                        // experimental output, mark this output's
                        // artifact_validation experimental too and record
                        // which upstreams tainted it. Pure metadata; does
                        // not change status. See PROPOSAL.md
                        // "Experimental Propagation".
                        if let Some(node_def) =
                            automation.flow.nodes.iter().find(|n| n.node_id == node_id)
                        {
                            let upstream: Vec<(&str, &Value)> = node_def
                                .depends_on
                                .iter()
                                .filter_map(|dep| {
                                    row.checkpoint
                                        .node_outputs
                                        .get(dep)
                                        .map(|value| (dep.as_str(), value))
                                })
                                .collect();
                            crate::automation_v2::execution_profile::propagate_experimental_input_taint(
                                &mut output,
                                upstream,
                            );
                        }
                    }
                    let session_id = crate::app::state::automation_output_session_id(&output);
                    let summary = output
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    let contract_kind = output
                        .get("contract_kind")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    let blocked = crate::app::state::automation_output_is_blocked(&output);
                    let verify_failed =
                        crate::app::state::automation_output_is_verify_failed(&output);
                    let needs_repair = crate::app::state::automation_output_needs_repair(&output);
                    let has_warnings = crate::app::state::automation_output_has_warnings(&output);
                    let repair_exhausted =
                        crate::app::state::automation_output_repair_exhausted(&output);
                    let mut blocked_reason =
                        crate::app::state::automation_output_blocked_reason(&output);
                    let mut failure_reason =
                        crate::app::state::automation_output_failure_reason(&output);
                    let attempt = latest_attempts.get(&node_id).copied().unwrap_or(1);
                    let max_attempts = automation
                        .flow
                        .nodes
                        .iter()
                        .find(|row| row.node_id == node_id)
                        .map(crate::app::state::automation_node_max_attempts)
                        .unwrap_or(1);
                    let terminal_repair_block = needs_repair && repair_exhausted;
                    let terminal_verify_failed = verify_failed && attempt >= max_attempts;
                    let retryable_verify_failed = verify_failed && !terminal_verify_failed;
                    if terminal_repair_block {
                        if let Some(object) = output.as_object_mut() {
                            object.insert("status".to_string(), json!("blocked"));
                            if object
                                .get("blocked_reason")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .unwrap_or_default()
                                .is_empty()
                            {
                                object.insert(
                                    "blocked_reason".to_string(),
                                    json!(failure_reason.clone().unwrap_or_else(|| {
                                        format!(
                                            "node `{}` exhausted repair attempts without satisfying validation",
                                            node_id
                                        )
                                    })),
                                );
                            }
                        }
                        blocked_reason =
                            crate::app::state::automation_output_blocked_reason(&output);
                        failure_reason =
                            crate::app::state::automation_output_failure_reason(&output);
                    }
                    let blocked = blocked || terminal_repair_block;
                    let is_approval_rejected = blocked
                        && !verify_failed
                        && output.get("approved").and_then(serde_json::Value::as_bool)
                            == Some(false);
                    let _ = state
                        .update_automation_v2_run(&run_id, |row| {
                            if run_node_is_settled_completed(row, &node_id) {
                                return;
                            }
                            if is_approval_rejected {
                                let rollback_roots = approval_rejection_rollback_roots(
                                    &automation,
                                    &node_id,
                                    &row.checkpoint,
                                );
                                if !rollback_roots.is_empty() {
                                    let reset_roots: std::collections::HashSet<String> =
                                        rollback_roots.iter().cloned().collect();
                                    let nodes_to_reset =
                                        crate::app::state::collect_automation_descendants(
                                            &automation,
                                            &reset_roots,
                                        );
                                    let mut reset_list =
                                        nodes_to_reset.iter().cloned().collect::<Vec<_>>();
                                    reset_list.sort();
                                    for reset_id in &nodes_to_reset {
                                        row.checkpoint.node_outputs.remove(reset_id);
                                        row.checkpoint.completed_nodes.retain(|id| id != reset_id);
                                        row.checkpoint.blocked_nodes.retain(|id| id != reset_id);
                                        if !row.checkpoint.pending_nodes.iter().any(|id| id == reset_id) {
                                            row.checkpoint.pending_nodes.push(reset_id.clone());
                                        }
                                        // Reset the attempt counter for rolled-back nodes so
                                        // they have a fresh budget to re-run. Without this,
                                        // an ancestor that previously completed at attempt N
                                        // would enter re-execution with attempts==N; while
                                        // its next attempt is mid-flight, derive_terminal_run_state
                                        // can see (pending + attempts>=max) and falsely mark
                                        // the run as failed.
                                        row.checkpoint.node_attempts.remove(reset_id);
                                    }
                                    row.status = AutomationRunStatus::Queued;
                                    row.checkpoint.last_failure = None;
                                    crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                                        row,
                                        "node_approval_rollback",
                                        Some(format!(
                                            "node `{}` rejected upstream output; rolling back and re-queuing: {}",
                                            node_id,
                                            reset_list.join(", ")
                                        )),
                                        None,
                                        Some(json!({
                                            "node_id": node_id,
                                            "attempt": attempt,
                                            "session_id": session_id,
                                            "reset_nodes": reset_list,
                                            "blocked_reason": blocked_reason,
                                        })),
                                    );
                                    crate::app::state::refresh_automation_runtime_state(&automation, row);
                                    return;
                                }
                            }
                            let blocked_descendants = if blocked || terminal_verify_failed {
                                blocked_nodes_for_partial_failure_mode(
                                    &automation,
                                    &row.checkpoint,
                                    &node_id,
                                )
                            } else {
                                std::collections::HashSet::new()
                            };
                            reconcile_pending_nodes_after_node_output(
                                &mut row.checkpoint,
                                &node_id,
                                needs_repair || retryable_verify_failed,
                                terminal_repair_block || terminal_verify_failed,
                                &blocked_descendants,
                            );
                            if !blocked
                                && !needs_repair
                                && !verify_failed
                                && !row.checkpoint.completed_nodes.iter().any(|id| id == &node_id)
                            {
                                row.checkpoint.completed_nodes.push(node_id.clone());
                            }
                            if blocked {
                                if !row.checkpoint.blocked_nodes.iter().any(|id| id == &node_id) {
                                    row.checkpoint.blocked_nodes.push(node_id.clone());
                                }
                                for blocked_node in &blocked_descendants {
                                    if !row.checkpoint.blocked_nodes.iter().any(|id| id == blocked_node) {
                                        row.checkpoint.blocked_nodes.push(blocked_node.clone());
                                    }
                                }
                            }
                            row.checkpoint.node_outputs.insert(node_id.clone(), output.clone());
                            if let Some(verdict) = output.get("attempt_verdict").cloned() {
                                row.checkpoint
                                    .node_attempt_verdicts
                                    .entry(node_id.clone())
                                    .or_default()
                                    .push(verdict);
                            }
                            if !verify_failed
                                && row
                                    .checkpoint
                                    .last_failure
                                    .as_ref()
                                    .is_some_and(|failure| failure.node_id == node_id)
                            {
                                row.checkpoint.last_failure = None;
                            }
                            if terminal_verify_failed {
                                row.checkpoint.last_failure = Some(
                                    crate::automation_v2::types::AutomationFailureRecord {
                                        node_id: node_id.clone(),
                                        reason: failure_reason.clone().unwrap_or_else(|| {
                                            "verification failed".to_string()
                                        }),
                                        failed_at_ms: now_ms(),
                                        failure_kind: Some("verification_failed".to_string()),
                                        metadata: None,
                                    },
                                );
                            }
                            if needs_repair && !terminal_repair_block && !verify_failed {
                                if let Some(node) = automation
                                    .flow
                                    .nodes
                                    .iter()
                                    .find(|node| node.node_id == node_id)
                                {
                                    crate::automation_v2::schema_validation_pause::apply_schema_validation_pause(
                                        row,
                                        &automation,
                                        node,
                                        &output,
                                        attempt,
                                        max_attempts,
                                    );
                                }
                            }
                            crate::app::state::automation::lifecycle::record_automation_workflow_state_events(
                                row,
                                &node_id,
                                &output,
                                attempt,
                                session_id.as_deref(),
                                &summary,
                                &contract_kind,
                            );
                            // Surface profile relaxation as a top-level
                            // lifecycle event so it appears in run history,
                            // failure events, and Incident Monitor receipts —
                            // not only inside output["artifact_validation"].
                            if let Some(relaxed_classes) = output
                                .pointer("/artifact_validation/relaxed_validator_classes")
                                .and_then(Value::as_array)
                                .filter(|arr| !arr.is_empty())
                                .cloned()
                            {
                                let original_outcome = output
                                    .pointer("/artifact_validation/original_validator_outcome")
                                    .and_then(Value::as_str)
                                    .unwrap_or("blocked")
                                    .to_string();
                                let effective_outcome = output
                                    .pointer("/artifact_validation/effective_outcome")
                                    .and_then(Value::as_str)
                                    .unwrap_or("warning")
                                    .to_string();
                                let original_status = output
                                    .pointer("/artifact_validation/original_status")
                                    .and_then(Value::as_str)
                                    .map(str::to_string);
                                let experimental = output
                                    .pointer("/artifact_validation/experimental")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                let class_count = relaxed_classes.len();
                                let metadata = json!({
                                    "node_id": node_id,
                                    "attempt": attempt,
                                    "session_id": session_id,
                                    "relaxed_validator_classes": relaxed_classes,
                                    "original_validator_outcome": original_outcome,
                                    "effective_outcome": effective_outcome,
                                    "original_status": original_status,
                                    "experimental": experimental,
                                });
                                crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                                    row,
                                    "node_relaxed",
                                    Some(format!(
                                        "node `{}` accepted under relaxed profile ({} → {}; {} class{} relaxed)",
                                        node_id,
                                        original_outcome,
                                        effective_outcome,
                                        class_count,
                                        if class_count == 1 { "" } else { "es" },
                                    )),
                                    None,
                                    Some(metadata),
                                );
                            }
                            crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                                row,
                                if verify_failed {
                                    "node_verify_failed"
                                } else if needs_repair && !terminal_repair_block {
                                    "node_repair_requested"
                                } else if has_warnings {
                                    "node_completed_with_warnings"
                                } else if blocked {
                                    "node_blocked"
                                } else {
                                    "node_completed"
                                },
                                Some(if verify_failed {
                                    format!("node `{}` failed verification", node_id)
                                } else if needs_repair && !terminal_repair_block {
                                    format!("node `{}` requested another repair attempt", node_id)
                                } else if has_warnings {
                                    format!("node `{}` completed with warnings", node_id)
                                } else if blocked {
                                    format!("node `{}` blocked downstream execution", node_id)
                                } else {
                                    format!("node `{}` completed", node_id)
                                }),
                                None,
                                Some(json!({
                                    "node_id": node_id,
                                    "attempt": attempt,
                                    "session_id": session_id,
                                    "summary": summary,
                                    "contract_kind": contract_kind,
                                    "status": if verify_failed {
                                        "verify_failed"
                                    } else if needs_repair && !terminal_repair_block {
                                        "needs_repair"
                                    } else if has_warnings {
                                        "completed_with_warnings"
                                    } else if blocked {
                                        "blocked"
                                    } else {
                                        "completed"
                                    },
                                    "warning_count": output
                                        .get("artifact_validation")
                                        .and_then(|value| value.get("warning_count"))
                                        .and_then(Value::as_u64),
                                    "max_attempts": max_attempts,
                                    "blocked_reason": blocked_reason,
                                    "failure_reason": failure_reason,
                                    "blocked_descendants": blocked_descendants,
                                })),
                            );
                            if !blocked && !needs_repair && !verify_failed {
                                crate::app::state::record_milestone_promotions(&automation, row, &node_id);
                            }
                            // Rescue: if this node just succeeded but the run is Failed because
                            // of this node (concurrent batch-mate set it before our Ok landed),
                            // clear the failure and resume so downstream nodes can still run.
                            if matches!(row.status, AutomationRunStatus::Failed)
                                && !blocked
                                && !needs_repair
                                && !verify_failed
                                && row
                                    .checkpoint
                                    .last_failure
                                    .as_ref()
                                    .is_some_and(|f| f.node_id == node_id)
                            {
                                row.checkpoint.last_failure = None;
                                row.status = AutomationRunStatus::Running;
                                row.detail = Some(format!(
                                    "recovered: node `{}` succeeded after execution error",
                                    node_id
                                ));
                                crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                                    row,
                                    "node_recovered",
                                    Some(format!("node `{}` recovered from execution error; run resuming", node_id)),
                                    None,
                                    Some(json!({
                                        "node_id": node_id,
                                        "attempt": attempt,
                                        "session_id": session_id,
                                    })),
                                );
                            }
                            crate::app::state::refresh_automation_runtime_state(&automation, row);
                        })
                        .await;
                    if terminal_verify_failed {
                        terminal_failure =
                            Some(failure_reason.unwrap_or_else(|| {
                                format!("node `{}` failed verification", node_id)
                            }));
                        let _ = state
                            .update_automation_v2_run(&run_id, |row| {
                                row.status = AutomationRunStatus::Failed;
                                row.detail = terminal_failure.clone();
                            })
                            .await;
                        break;
                    }
                }
                Err(error) => {
                    let should_ignore = state
                        .get_automation_v2_run(&run_id)
                        .await
                        .map(|row| {
                            if run_node_is_settled_completed(&row, &node_id) {
                                return true;
                            }
                            matches!(
                                row.status,
                                AutomationRunStatus::Paused
                                    | AutomationRunStatus::Pausing
                                    | AutomationRunStatus::AwaitingApproval
                                    | AutomationRunStatus::Cancelled
                                    | AutomationRunStatus::Blocked
                                    | AutomationRunStatus::Failed
                                    | AutomationRunStatus::Completed
                            )
                        })
                        .unwrap_or(false);
                    if should_ignore {
                        continue;
                    }
                    let detail = normalize_execution_error_detail(
                        &crate::app::state::truncate_text(&error.to_string(), 500),
                    );
                    let attempts = latest_attempts.get(&node_id).copied().unwrap_or(1);
                    let blocker_category = execution_error_blocker_category(&detail);
                    let max_attempts = automation_node_execution_error_max_attempts(
                        &node,
                        &detail,
                        blocker_category,
                    );
                    let failure_class = retry_failure_class_from_blocker_category(blocker_category);
                    let retry_decision = automation_node_retry_decision(
                        &node,
                        &detail,
                        attempts,
                        max_attempts,
                        failure_class,
                        now_ms(),
                    );
                    let retry_decision_value =
                        serde_json::to_value(&retry_decision).unwrap_or(Value::Null);
                    let terminal = retry_decision.terminal;

                    let artifact_recovered = if let Some(output_path) =
                        crate::app::state::automation::automation_node_required_output_path(&node)
                    {
                        let workspace_root =
                            crate::app::state::resolve_automation_v2_workspace_root(
                                &state,
                                &automation,
                            )
                            .await;
                        let run_session_id = state
                            .get_automation_v2_run(&run_id)
                            .await
                            .and_then(|row| row.latest_session_id.clone())
                            .unwrap_or_default();
                        let session = if let Some(runtime) = state.runtime.get() {
                            runtime.storage.get_session(&run_session_id).await
                        } else {
                            None
                        };
                        match (session.as_ref(), Some(output_path.as_str())) {
                            (Some(session), Some(output_path)) => {
                                let session_text =
                                    crate::app::state::automation::extract_session_text_output(
                                        session,
                                    );
                                let recovered = crate::app::state::automation::extract_recoverable_json_artifact(&session_text)
                                    .and_then(|payload| {
                                        crate::app::state::automation::resolve_automation_output_path_with_runtime_for_run(
                                            &workspace_root,
                                            &run_id,
                                            output_path,
                                            Some(&runtime_values),
                                        )
                                        .ok()
                                        .map(|resolved| (payload, resolved))
                                    })
                                    .map(|(payload, resolved)| {
                                        if let Some(parent) = resolved.parent() {
                                            let _ = std::fs::create_dir_all(parent);
                                        }
                                        serde_json::to_string_pretty(&payload)
                                            .ok()
                                            .and_then(|serialized| {
                                                std::fs::write(&resolved, serialized).ok()?;
                                                Some(())
                                            })
                                    })
                                    .is_some();
                                recovered
                            }
                            _ => false,
                        }
                    } else {
                        false
                    };

                    let mut failure_output = build_node_execution_error_output_with_category(
                        &node,
                        &detail,
                        terminal,
                        blocker_category,
                    );
                    let required_next_actions = failure_output
                        .pointer("/artifact_validation/required_next_tool_actions")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let provider_related = failure_class == "provider_transient"
                        || failure_class == "provider_terminal";
                    let attempt_review = crate::automation_v2::types::AutomationAttemptReview {
                        tone: "calm_teammate_v1".to_string(),
                        progress_label: if failure_class == "provider_transient" {
                            "partial".to_string()
                        } else {
                            "none".to_string()
                        },
                        progress_score: if failure_class == "provider_transient" {
                            10
                        } else {
                            0
                        },
                        completed_correctly: Vec::new(),
                        still_needed: if failure_class == "provider_transient" {
                            vec!["Retry the same plan after provider activity recovers.".to_string()]
                        } else {
                            vec![format!(
                                "Address the recorded execution blocker: {}.",
                                crate::app::state::truncate_text(&detail, 220)
                            )]
                        },
                        why_it_matters: if failure_class == "provider_transient" {
                            vec![
                                "Provider interruptions are infrastructure failures, not evidence that the workflow contract was satisfied."
                                    .to_string(),
                            ]
                        } else {
                            vec![
                                "The next attempt needs one concrete blocker it can satisfy and verify."
                                    .to_string(),
                            ]
                        },
                        next_moves: if required_next_actions.is_empty() {
                            vec![
                                "Use the execution error and expected contract to repair the next attempt."
                                    .to_string(),
                            ]
                        } else {
                            required_next_actions.iter().take(3).cloned().collect()
                        },
                    };
                    let attempt_verdict = json!(
                        crate::automation_v2::types::AutomationAttemptVerdict {
                            version: 1,
                            node_id: node.node_id.clone(),
                            attempt: attempts,
                            session_id: None,
                            outcome: if terminal {
                                "failed".to_string()
                            } else {
                                "needs_repair".to_string()
                            },
                            failure_class: Some(failure_class.to_string()),
                            consumes_model_attempt_budget: failure_class != "provider_transient",
                            consumes_repair_budget: failure_class == "contract_miss",
                            expected: json!({
                                "output_contract": normalized_output_contract_value(&node),
                                "required_output_path": crate::app::state::automation::automation_node_required_output_path(&node),
                            }),
                            observed: json!({
                                "status": if terminal { "failed" } else { "needs_repair" },
                                "blocker_category": blocker_category,
                                "blocked_reason": detail,
                                "retry_decision": retry_decision_value.clone(),
                            }),
                            unmet_requirements: Vec::new(),
                            required_next_actions,
                            provider_error: provider_related.then(|| detail.clone()),
                            validation_reason: Some(detail.clone()),
                            attempt_review,
                            created_at_ms: now_ms(),
                        }
                    );
                    if let Some(object) = failure_output.as_object_mut() {
                        object.insert("attempt_verdict".to_string(), attempt_verdict.clone());
                        object.insert("retry_decision".to_string(), retry_decision_value.clone());
                    }
                    if artifact_recovered {
                        if let Some(obj) = failure_output.as_object_mut() {
                            obj.insert("artifact_recovered_from_session".to_string(), json!(true));
                            obj.get_mut("validator_summary")
                                .and_then(|v| v.as_object_mut())
                                .map(|v| {
                                    v.insert(
                                        "artifact_recovered_from_session".to_string(),
                                        json!(true),
                                    );
                                });
                            obj.get_mut("artifact_validation")
                                .and_then(|v| v.as_object_mut())
                                .map(|v| {
                                    v.insert(
                                        "artifact_recovered_from_session".to_string(),
                                        json!(true),
                                    );
                                });
                        }
                    }
                    let _ = state
                        .update_automation_v2_run(&run_id, |row| {
                            row.checkpoint
                                .node_outputs
                                .insert(node_id.clone(), failure_output.clone());
                            row.checkpoint
                                .node_attempt_verdicts
                                .entry(node_id.clone())
                                .or_default()
                                .push(attempt_verdict.clone());
                            crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                                row,
                                "node_failed",
                                Some(format!("node `{}` failed", node_id)),
                                None,
                                Some(json!({
                                    "node_id": node_id,
                                    "attempt": attempts,
                                    "max_attempts": max_attempts,
                                    "reason": detail,
                                    "terminal": terminal,
                                    "retry_decision": retry_decision_value.clone(),
                                    "artifact_recovered_from_session": artifact_recovered,
                                    "attempt_verdict": attempt_verdict,
                                })),
                            );
                        })
                        .await;
                    if terminal {
                        terminal_failure = Some(format!(
                            "node `{}` failed after {}/{} attempts: {}",
                            node_id, attempts, max_attempts, detail
                        ));
                        let _ = state
                            .update_automation_v2_run(&run_id, |row| {
                                row.checkpoint.last_failure =
                                    Some(crate::automation_v2::types::AutomationFailureRecord {
                                        node_id: node_id.clone(),
                                        reason: detail.clone(),
                                        failed_at_ms: now_ms(),
                                        failure_kind: Some(failure_class.to_string()),
                                        metadata: None,
                                    });
                            })
                            .await;
                        // Don't break early — continue processing remaining outcomes so
                        // sibling nodes that succeeded in the same batch still get recorded.
                        continue;
                    }
                    let _ = state
                        .update_automation_v2_run(&run_id, |row| {
                            row.detail = Some(format!(
                                "retrying node `{}` after attempt {}/{} failed: {}",
                                node_id, attempts, max_attempts, detail
                            ));
                        })
                        .await;
                    if let Some(backoff_ms) = retry_decision.backoff_ms {
                        crate::automation_v2::retry_backoff_queue::record_pending_retry_backoff(
                            &mut pending_retry_backoff,
                            &node_id,
                            attempts,
                            max_attempts,
                            &retry_decision,
                            &detail,
                            backoff_ms,
                        );
                    }
                }
            }
        }
        if let Some(detail) = terminal_failure {
            let _ = state
                .update_automation_v2_run(&run_id, |row| {
                    row.status = AutomationRunStatus::Failed;
                    row.detail = Some(detail);
                })
                .await;
            break;
        }
        if crate::automation_v2::retry_backoff_queue::queue_pending_retry_backoff(
            &state,
            &run_id,
            run_claim_epoch,
            pending_retry_backoff,
        )
        .await
        {
            return;
        }
    }
    let final_run = state.get_automation_v2_run(&run_id).await;
    if let Some(run) = final_run {
        let elapsed_ms = run
            .started_at_ms
            .map(|started| now_ms().saturating_sub(started));
        let completed_count = run.checkpoint.completed_nodes.len();
        let pending_count = run.checkpoint.pending_nodes.len();
        let blocked_count = run.checkpoint.blocked_nodes.len();
        let node_count = automation.flow.nodes.len();
        let failed_count = run
            .checkpoint
            .node_outputs
            .iter()
            .filter(|(_, output)| {
                output
                    .get("failure_kind")
                    .and_then(Value::as_str)
                    .is_some_and(|k| k == "run_failed" || k == "verification_failed")
            })
            .count();
        tracing::info!(
            run_id = %run_id,
            automation_id = %run.automation_id,
            final_status = ?run.status,
            elapsed_ms = elapsed_ms,
            completed_nodes = completed_count,
            pending_nodes = pending_count,
            blocked_nodes = blocked_count,
            total_nodes = node_count,
            failed_nodes = failed_count,
            total_tokens = run.total_tokens,
            estimated_cost_usd = run.estimated_cost_usd,
            "automation run finished"
        );
        publish_automation_v2_failure_event(&state, &automation, &run);
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
