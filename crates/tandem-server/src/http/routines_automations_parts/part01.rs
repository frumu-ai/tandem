// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    evaluate_routine_execution_policy, AppState, AutomationAgentMcpPolicy, AutomationAgentProfile,
    AutomationAgentToolPolicy, AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunStatus,
    AutomationV2Schedule, AutomationV2Spec, AutomationV2Status, RoutineExecutionDecision,
    RoutineHistoryEvent, RoutineMisfirePolicy, RoutineRunArtifact, RoutineRunRecord,
    RoutineRunStatus, RoutineSchedule, RoutineSpec, RoutineStatus, RoutineStoreError,
};
use tandem_plan_compiler::api as compiler_api;
use tandem_types::EngineEvent;

fn routine_run_with_context_links(run: &RoutineRunRecord) -> Value {
    let context_run_id = super::context_runs::routine_context_run_id(&run.run_id);
    let mut payload = serde_json::to_value(run).unwrap_or_else(|_| json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("contextRunID".to_string(), json!(context_run_id));
        obj.insert("linked_context_run_id".to_string(), json!(context_run_id));
    }
    payload
}

fn automation_v2_node_repair_guidance(output: &Value) -> Option<Value> {
    let validator_summary = output.get("validator_summary");
    let artifact_validation = output.get("artifact_validation");
    let required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .cloned()
        .unwrap_or_default();
    let validator_reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .cloned()
        .unwrap_or_default();
    let blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let validation_basis = artifact_validation
        .and_then(|value| value.get("validation_basis"))
        .cloned()
        .filter(|value| !value.is_null());
    let required_source_read_paths = validation_basis
        .as_ref()
        .and_then(|value| value.get("required_source_read_paths"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let missing_required_source_read_paths = validation_basis
        .as_ref()
        .and_then(|value| value.get("missing_required_source_read_paths"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let upstream_read_paths = validation_basis
        .as_ref()
        .and_then(|value| value.get("upstream_read_paths"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let knowledge_preflight = output
        .get("knowledge_preflight")
        .cloned()
        .filter(|value| !value.is_null());
    let runtime_pause = output
        .get("runtime_pause")
        .or_else(|| artifact_validation.and_then(|value| value.get("runtime_pause")))
        .cloned()
        .filter(|value| !value.is_null());
    let runtime_state = artifact_validation
        .and_then(|value| value.get("runtime_state"))
        .and_then(Value::as_str)
        .or_else(|| {
            runtime_pause
                .as_ref()
                .and_then(|value| value.get("runtime_state"))
                .and_then(Value::as_str)
        });

    if required_next_tool_actions.is_empty()
        && validator_reason.is_none()
        && unmet_requirements.is_empty()
        && blocking_classification.is_none()
        && validation_basis.is_none()
        && knowledge_preflight.is_none()
        && runtime_pause.is_none()
    {
        return None;
    }

    Some(json!({
        "status": output.get("status").and_then(Value::as_str),
        "failureKind": output.get("failure_kind").and_then(Value::as_str),
        "reason": validator_reason,
        "unmetRequirements": unmet_requirements,
        "blockingClassification": blocking_classification,
        "requiredNextToolActions": required_next_tool_actions,
        "validationBasis": validation_basis,
        "upstreamReadPaths": upstream_read_paths,
        "requiredSourceReadPaths": required_source_read_paths,
        "missingRequiredSourceReadPaths": missing_required_source_read_paths,
        "knowledgePreflight": knowledge_preflight,
        "runtimeState": runtime_state,
        "runtimePause": runtime_pause,
        "knowledgeReuseReason": knowledge_preflight.as_ref().and_then(|value| value.get("reuse_reason")).and_then(Value::as_str),
        "knowledgeSkipReason": knowledge_preflight.as_ref().and_then(|value| value.get("skip_reason")).and_then(Value::as_str),
        "knowledgeFreshnessReason": knowledge_preflight.as_ref().and_then(|value| value.get("freshness_reason")).and_then(Value::as_str),
        "repairAttempt": artifact_validation
            .and_then(|value| value.get("repair_attempt"))
            .and_then(Value::as_u64),
        "repairAttemptsRemaining": artifact_validation
            .and_then(|value| value.get("repair_attempts_remaining"))
            .and_then(Value::as_u64),
    }))
}

fn spawn_automation_v2_run_cleanup(
    state: AppState,
    session_ids: Vec<String>,
    instance_ids: Vec<String>,
    instance_cancel_reason: &'static str,
) {
    if session_ids.is_empty() && instance_ids.is_empty() {
        return;
    }
    tokio::spawn(async move {
        for session_id in session_ids {
            let _ = state.cancellations.cancel(&session_id).await;
        }
        for instance_id in instance_ids {
            let _ = state
                .agent_teams
                .cancel_instance(&state, &instance_id, instance_cancel_reason)
                .await;
        }
    });
}

async fn automation_v2_run_with_context_links(
    state: &crate::app::state::AppState,
    run: &crate::AutomationV2RunRecord,
) -> Value {
    let mut normalized_run = run.clone();
    let blocked_node_ids = automation_v2_blocked_node_ids(&normalized_run);
    let mut last_activity_at_ms =
        crate::app::state::automation::lifecycle::automation_last_activity_at_ms(&normalized_run);
    for session_id in &normalized_run.active_session_ids {
        if let Some(session) = state.storage.get_session(session_id).await {
            let updated_ms: i64 = session.time.updated.timestamp_millis().max(0);
            let updated_ms_u64: u64 = updated_ms.try_into().unwrap_or_default();
            if updated_ms_u64 > last_activity_at_ms {
                last_activity_at_ms = updated_ms_u64;
            }
        }
    }
    normalized_run.checkpoint.blocked_nodes = blocked_node_ids.clone();
    if let Some(automation) = normalized_run.automation_snapshot.clone() {
        for node in &automation.flow.nodes {
            if let Some(output) = normalized_run
                .checkpoint
                .node_outputs
                .get_mut(&node.node_id)
            {
                *output =
                    crate::app::state::enrich_automation_node_output_for_contract(node, output);
            }
        }
    }
    let mut node_repair_guidance = serde_json::Map::new();
    let mut node_attempt_evidence = serde_json::Map::new();
    let mut needs_repair_node_ids = Vec::new();
    for (node_id, output) in &normalized_run.checkpoint.node_outputs {
        if output
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("needs_repair"))
        {
            needs_repair_node_ids.push(node_id.clone());
        }
        if let Some(guidance) = automation_v2_node_repair_guidance(output) {
            node_repair_guidance.insert(node_id.clone(), guidance);
        }
        if let Some(attempt_evidence) = output
            .get("attempt_evidence")
            .cloned()
            .filter(|value| !value.is_null())
        {
            node_attempt_evidence.insert(node_id.clone(), attempt_evidence);
        }
    }
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    if let Some(derived_status) = automation_v2_projected_backlog_status_override(state, run) {
        normalized_run.status = derived_status;
    }
    let mut payload = serde_json::to_value(&normalized_run).unwrap_or_else(|_| json!({}));
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("contextRunID".to_string(), json!(context_run_id.clone()));
        obj.insert("linked_context_run_id".to_string(), json!(context_run_id));
        obj.insert("blockedNodeIDs".to_string(), json!(blocked_node_ids));
        if normalized_run.status != run.status {
            obj.insert("stored_status".to_string(), json!(run.status));
            obj.insert("storedStatus".to_string(), json!(run.status));
            obj.insert(
                "statusDerivedNote".to_string(),
                json!("derived from projected task board"),
            );
        }
        obj.insert(
            "last_activity_at_ms".to_string(),
            json!(last_activity_at_ms),
        );
        obj.insert("lastActivityAtMs".to_string(), json!(last_activity_at_ms));
        obj.insert(
            "nodeRepairGuidance".to_string(),
            Value::Object(node_repair_guidance),
        );
        obj.insert(
            "needsRepairNodeIDs".to_string(),
            json!(needs_repair_node_ids),
        );
        obj.insert(
            "nodeAttemptEvidence".to_string(),
            Value::Object(node_attempt_evidence),
        );
    }
    payload
}

fn automation_v2_projected_backlog_status_override(
    state: &crate::app::state::AppState,
    run: &crate::AutomationV2RunRecord,
) -> Option<AutomationRunStatus> {
    if !matches!(run.status, AutomationRunStatus::Completed) {
        return None;
    }
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    let blackboard = super::context_runs::load_projected_context_blackboard(state, &context_run_id);
    let projected_statuses = blackboard
        .tasks
        .into_iter()
        .filter(|task| task.task_type == "automation_backlog_item")
        .map(|task| task.status)
        .collect::<Vec<_>>();
    if projected_statuses.is_empty() {
        return None;
    }
    use crate::http::context_types::ContextBlackboardTaskStatus as TaskStatus;
    if projected_statuses
        .iter()
        .any(|status| matches!(status, TaskStatus::Failed))
    {
        return Some(AutomationRunStatus::Failed);
    }
    if projected_statuses
        .iter()
        .any(|status| matches!(status, TaskStatus::Blocked))
    {
        return Some(AutomationRunStatus::Blocked);
    }
    if projected_statuses.iter().any(|status| {
        matches!(
            status,
            TaskStatus::Pending | TaskStatus::Runnable | TaskStatus::InProgress
        )
    }) {
        return Some(AutomationRunStatus::Running);
    }
    None
}

fn automation_v2_with_manual_trigger_record(
    automation: &AutomationV2Spec,
    run_id: &str,
    dry_run: bool,
) -> Option<AutomationV2Spec> {
    let mut automation = automation.clone();
    let metadata = automation.metadata.as_mut()?.as_object_mut()?;
    let plan_package_value = metadata.get("plan_package")?.clone();
    let plan_package: compiler_api::PlanPackage =
        serde_json::from_value(plan_package_value).ok()?;
    let plan_package = compiler_api::with_manual_trigger_record(
        &plan_package,
        &format!("manual-trigger-{run_id}"),
        &automation.creator_id,
        if dry_run {
            compiler_api::ManualTriggerSource::DryRun
        } else {
            compiler_api::ManualTriggerSource::Api
        },
        dry_run,
        &chrono::Utc::now().to_rfc3339(),
        Some(run_id),
        None,
        Vec::new(),
        Some(if dry_run {
            "manual dry run triggered via API"
        } else {
            "manual run triggered via API"
        }),
    )?;
    metadata.insert(
        "plan_package".to_string(),
        serde_json::to_value(plan_package).ok()?,
    );
    Some(automation)
}

fn automation_v2_failed_node_ids(run: &crate::AutomationV2RunRecord) -> Vec<String> {
    let mut failed_nodes = run
        .checkpoint
        .node_outputs
        .iter()
        .filter_map(|(node_id, output)| {
            let status = output
                .get("status")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let failure_kind = output
                .get("failure_kind")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            (matches!(status.as_str(), "failed" | "verify_failed")
                || matches!(failure_kind.as_str(), "verification_failed" | "run_failed"))
            .then_some(node_id.clone())
        })
        .collect::<Vec<_>>();
    failed_nodes.sort();
    failed_nodes.dedup();
    failed_nodes
}

fn automation_v2_node_output_status(output: &Value) -> String {
    let direct_status = output
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !direct_status.is_empty() {
        return direct_status.to_ascii_lowercase();
    }
    output
        .get("content")
        .and_then(|content| content.get("status"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn automation_v2_blocked_node_ids(run: &crate::AutomationV2RunRecord) -> Vec<String> {
    let mut blocked_nodes = run.checkpoint.blocked_nodes.clone();
    blocked_nodes.extend(
        run.checkpoint
            .node_outputs
            .iter()
            .filter_map(|(node_id, output)| {
                (automation_v2_node_output_status(output) == "blocked").then_some(node_id.clone())
            }),
    );
    blocked_nodes.sort();
    blocked_nodes.dedup();
    blocked_nodes
}

async fn validate_shared_context_pack_bindings(
    state: &AppState,
    workspace_root: Option<&str>,
    metadata: Option<&Value>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let pack_ids = crate::http::context_packs::shared_context_pack_ids_from_metadata(metadata);
    if pack_ids.is_empty() {
        return Ok(());
    }
    let normalized_workspace_root = match workspace_root {
        Some(value) if !value.trim().is_empty() => Some(
            crate::normalize_absolute_workspace_root(value).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": error,
                        "code": "AUTOMATION_V2_SHARED_CONTEXT_INVALID",
                    })),
                )
            })?,
        ),
        _ => None,
    };
    let declared_project_key = metadata
        .and_then(Value::as_object)
        .and_then(|object| {
            object
                .get("shared_context_project_key")
                .or_else(|| object.get("sharedContextProjectKey"))
                .or_else(|| object.get("project_key"))
                .or_else(|| object.get("projectKey"))
                .or_else(|| {
                    object
                        .get("plan_package")
                        .and_then(Value::as_object)
                        .and_then(|value| {
                            value.get("project_key").or_else(|| value.get("projectKey"))
                        })
                })
                .and_then(Value::as_str)
                .map(str::trim)
                .map(ToString::to_string)
        })
        .filter(|value| !value.is_empty());
    for pack_id in pack_ids {
        let Some(pack) = state.get_context_pack(&pack_id).await else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "shared workflow context not found",
                    "code": "AUTOMATION_V2_SHARED_CONTEXT_PACK_NOT_FOUND",
                    "pack_id": pack_id,
                })),
            ));
        };
        if pack.state != crate::http::context_packs::ContextPackState::Published {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "shared workflow context is not published",
                    "code": "AUTOMATION_V2_SHARED_CONTEXT_PACK_INVALID",
                    "pack_id": pack.pack_id,
                    "state": pack.state,
                })),
            ));
        }
        if let Some(root) = normalized_workspace_root.as_deref() {
            if pack.workspace_root != root {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "shared workflow context workspace does not match",
                        "code": "AUTOMATION_V2_SHARED_CONTEXT_PACK_SCOPE_MISMATCH",
                        "pack_id": pack.pack_id,
                        "workspace_root": pack.workspace_root,
                    })),
                ));
            }
        }
        if !crate::http::context_packs::context_pack_allows_project(
            &pack,
            declared_project_key.as_deref(),
        ) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "shared workflow context project does not match",
                    "code": "AUTOMATION_V2_SHARED_CONTEXT_PACK_SCOPE_MISMATCH",
                    "pack_id": pack.pack_id,
                    "project_key": pack.project_key,
                    "allowed_project_keys": pack.allowed_project_keys,
                })),
            ));
        }
    }
    Ok(())
}

fn automation_v2_run_has_run_level_completion_assertion(
    run: &crate::AutomationV2RunRecord,
) -> bool {
    run.status == crate::AutomationRunStatus::Failed
        && run.detail.as_deref().is_some_and(|detail| {
            detail.starts_with("automation run deliverable `")
                || detail.starts_with("automation run missing required deliverable `")
        })
}

fn automation_v2_run_level_completion_recovery_node_ids(
    automation: &crate::AutomationV2Spec,
) -> std::collections::HashSet<String> {
    let output_capable = automation
        .flow
        .nodes
        .iter()
        .filter(|node| {
            crate::app::state::automation::automation_node_can_access_declared_output_targets(
                automation,
                node,
            )
        })
        .map(|node| node.node_id.clone())
        .collect::<std::collections::HashSet<_>>();
    if !output_capable.is_empty() {
        return output_capable;
    }

    let dependency_ids = automation
        .flow
        .nodes
        .iter()
        .flat_map(|node| node.depends_on.iter().cloned())
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !dependency_ids.contains(&node.node_id))
        .map(|node| node.node_id.clone())
        .collect()
}

fn automation_v2_recoverable_failure_node_id(run: &crate::AutomationV2RunRecord) -> Option<String> {
    run.checkpoint
        .last_failure
        .as_ref()
        .map(|failure| failure.node_id.clone())
        .or_else(|| automation_v2_failed_node_ids(run).into_iter().next())
        .or_else(|| {
            let detail = run.detail.as_deref()?;
            [
                "automation run missing successful external action receipt for outbound node `",
                "automation run missing successful email delivery receipt for node `",
            ]
            .iter()
            .find_map(|prefix| {
                detail
                    .strip_prefix(prefix)
                    .and_then(|suffix| suffix.split_once('`'))
                    .map(|(node_id, _)| node_id.trim().to_string())
                    .filter(|node_id| !node_id.is_empty())
            })
        })
        .or_else(|| {
            const STARTUP_RUNTIME_CONTEXT_MISSING: &str =
                "runtime context partition missing for automation run";
            (run.status == crate::AutomationRunStatus::Failed
                && run
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail == STARTUP_RUNTIME_CONTEXT_MISSING))
            .then_some("runtime_context".to_string())
        })
}

#[derive(Debug, Deserialize)]
pub(super) struct RoutineCreateInput {
    pub routine_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: String,
    pub args: Option<Value>,
    pub allowed_tools: Option<Vec<String>>,
    pub output_targets: Option<Vec<String>>,
    pub creator_type: Option<String>,
    pub creator_id: Option<String>,
    pub requires_approval: Option<bool>,
    pub external_integrations_allowed: Option<bool>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationMissionInput {
    pub objective: String,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationToolPolicyInput {
    #[serde(default)]
    pub run_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default)]
    pub orchestrator_only_tool_calls: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationApprovalPolicyInput {
    #[serde(default)]
    pub requires_approval: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationPolicyInput {
    #[serde(default)]
    pub tool: AutomationToolPolicyInput,
    #[serde(default)]
    pub approval: AutomationApprovalPolicyInput,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationCreateInput {
    pub automation_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub mission: AutomationMissionInput,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    pub creator_type: Option<String>,
    pub creator_id: Option<String>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationMissionPatchInput {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub success_criteria: Option<Vec<String>>,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationPatchInput {
    pub name: Option<String>,
    pub status: Option<RoutineStatus>,
    pub schedule: Option<RoutineSchedule>,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub mission: Option<AutomationMissionPatchInput>,
    #[serde(default)]
    pub policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationV2CreateInput {
    pub automation_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<AutomationV2Status>,
    pub schedule: AutomationV2Schedule,
    #[serde(default)]
    pub agents: Vec<AutomationAgentProfile>,
    pub flow: AutomationFlowSpec,
    #[serde(default)]
    pub execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub capabilities: Option<crate::automation_v2::governance::AutomationDeclaredCapabilities>,
    #[serde(default)]
    pub scope_policy: Option<crate::automation_v2::types::AutomationScopePolicy>,
    #[serde(default)]
    pub watch_conditions: Option<Vec<crate::automation_v2::types::WatchCondition>>,
    #[serde(default)]
    pub handoff_config: Option<crate::automation_v2::types::AutomationHandoffConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2PatchInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<AutomationV2Status>,
    #[serde(default)]
    pub schedule: Option<AutomationV2Schedule>,
    #[serde(default)]
    pub agents: Option<Vec<AutomationAgentProfile>>,
    #[serde(default)]
    pub flow: Option<AutomationFlowSpec>,
    #[serde(default)]
    pub execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub capabilities: Option<crate::automation_v2::governance::AutomationDeclaredCapabilities>,
    #[serde(default)]
    pub scope_policy: Option<crate::automation_v2::types::AutomationScopePolicy>,
    #[serde(default)]
    pub watch_conditions: Option<Vec<crate::automation_v2::types::WatchCondition>>,
    #[serde(default)]
    pub handoff_config: Option<crate::automation_v2::types::AutomationHandoffConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2ShareInput {
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub audience_principals: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunNowInput {
    #[serde(default)]
    pub dry_run: bool,
    /// Run-level override for execution profile. When set, supersedes the
    /// automation's saved policy for this run only.
    #[serde(default)]
    pub execution_profile: Option<crate::automation_v2::execution_profile::ExecutionProfile>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutinePatchInput {
    pub name: Option<String>,
    pub status: Option<RoutineStatus>,
    pub schedule: Option<RoutineSchedule>,
    pub timezone: Option<String>,
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: Option<String>,
    pub args: Option<Value>,
    pub allowed_tools: Option<Vec<String>>,
    pub output_targets: Option<Vec<String>>,
    pub requires_approval: Option<bool>,
    pub external_integrations_allowed: Option<bool>,
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunNowInput {
    pub run_count: Option<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineHistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunsQuery {
    pub routine_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineRunDecisionInput {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AutomationV2GateDecisionInput {
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "request_id")]
    pub approval_request_id: Option<String>,
    #[serde(default)]
    pub transition_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunRepairInput {
    pub node_id: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    #[serde(default)]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunTaskActionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2RunTaskDispositionInput {
    /// Canonical: "unmarked" | "accepted" | "rejected" | "re_ran_strict".
    /// Operator-friendly aliases are accepted (see `parse_human_disposition_str`).
    pub disposition: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2GraduationSummaryQuery {
    /// Rolling window in hours. Defaults to 168 (7 days). Capped at 720
    /// (30 days) to avoid scanning archived runs.
    #[serde(default)]
    pub window_hours: Option<u32>,
    /// Optional: scope the aggregate to a single automation.
    #[serde(default)]
    pub automation_id: Option<String>,
    /// Cap on number of recent runs scanned. Defaults to 200, max 500.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationV2BacklogClaimInput {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub lease_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RoutineRunArtifactInput {
    pub uri: String,
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct RoutineEventsQuery {
    pub routine_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AutomationEventsQuery {
    pub automation_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AutomationV2TaskResetPreview {
    pub run_id: String,
    pub node_id: String,
    pub reset_nodes: Vec<String>,
    pub cleared_outputs: Vec<String>,
    pub preserves_upstream_outputs: bool,
}

pub(super) fn routine_error_response(error: RoutineStoreError) -> (StatusCode, Json<Value>) {
    match error {
        RoutineStoreError::InvalidRoutineId { routine_id } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine id",
                "code": "INVALID_ROUTINE_ID",
                "routineID": routine_id,
            })),
        ),
        RoutineStoreError::InvalidSchedule { detail } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine schedule",
                "code": "INVALID_ROUTINE_SCHEDULE",
                "detail": detail,
            })),
        ),
        RoutineStoreError::PersistFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Routine persistence failed",
                "code": "ROUTINE_PERSIST_FAILED",
                "detail": message,
            })),
        ),
    }
}

fn routine_creation_audit_error_response(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(error = ?error, "routine creation rolled back after protected audit failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Routine creation was rolled back because its required audit record could not be persisted",
            "code": "PROTECTED_AUDIT_PERSISTENCE_FAILED",
            "retryable": true,
        })),
    )
}

/// GOV-B2b: routine mutations (create/run/approve/deny/pause/resume) require a
/// verified human actor. Routines have no per-routine governance/approval record,
/// so agent-authored routine work is refused here — an agent that needs governed
/// autonomous work must use Automations V2, which carries the capability/approval
/// flow. Resolving the actor from the verified principal also stops callers from
/// spoofing `creator_type`/`creator_id` via the request body.
fn ensure_routine_human_actor(
    headers: &HeaderMap,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    action: &str,
) -> Result<crate::automation_v2::governance::GovernanceActorRef, (StatusCode, Json<Value>)> {
    let actor =
        super::governance::resolve_governance_actor(headers, tenant_context, request_principal);
    if actor.kind != crate::automation_v2::governance::GovernanceActorKind::Human {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": format!(
                    "Routine {action} requires a human actor; agents must use automations v2"
                ),
                "code": "ROUTINE_REQUIRES_HUMAN",
            })),
        ));
    }
    Ok(actor)
}

pub(super) async fn routines_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<RoutineCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // GOV-B2b: enforce a verified human actor and derive the creator from it
    // rather than trusting client-supplied creator fields.
    let actor =
        ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "creation")?;
    let creator_id = actor
        .actor_id
        .clone()
        .or(input.creator_id)
        .unwrap_or_else(|| "unknown".to_string());

    let routine = RoutineSpec {
        routine_id: input
            .routine_id
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        tenant_context: tenant_context.clone(),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input.entrypoint,
        args: input.args.unwrap_or_else(|| json!({})),
        allowed_tools: input.allowed_tools.unwrap_or_default(),
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: "user".to_string(),
        creator_id,
        requires_approval: input.requires_approval.unwrap_or(true),
        external_integrations_allowed: input.external_integrations_allowed.unwrap_or(false),
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    };
    let previous = state
        .get_routine_for_tenant(&routine.routine_id, &tenant_context)
        .await;
    let stored = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    if let Err(audit_error) = crate::audit::append_protected_audit_event(
        &state,
        "routine.created",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "routineID": stored.routine_id,
            "name": stored.name,
            "entrypoint": stored.entrypoint,
            "actor": actor.clone(),
        }),
    )
    .await
    {
        let rollback = match previous {
            Some(previous) => state.put_routine(previous).await.map(|_| ()),
            None => state
                .delete_routine_for_tenant(&stored.routine_id, &tenant_context)
                .await
                .map(|_| ()),
        };
        if let Err(rollback_error) = rollback {
            return Err(super::protected_audit_error_response(anyhow::anyhow!(
                "routine creation audit failed ({audit_error:#}) and rollback failed ({rollback_error:?})"
            )));
        }
        return Err(routine_creation_audit_error_response(audit_error));
    }
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.created",
            &stored.tenant_context,
            json!({
                "routineID": stored.routine_id,
                "name": stored.name,
                "entrypoint": stored.entrypoint,
            }),
        ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

pub(super) async fn routines_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let routines = state.list_routines_for_tenant(&tenant_context).await;
    Json(json!({
        "routines": routines,
        "count": routines.len(),
    }))
}

pub(super) async fn routines_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(input): Json<RoutinePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let stored = state
        .update_routine_for_tenant(&id, &tenant_context, move |routine| {
            if let Some(name) = input.name {
                routine.name = name;
            }
            if let Some(status) = input.status {
                routine.status = status;
            }
            if let Some(schedule) = input.schedule {
                routine.schedule = schedule;
            }
            if let Some(timezone) = input.timezone {
                routine.timezone = timezone;
            }
            if let Some(misfire_policy) = input.misfire_policy {
                routine.misfire_policy = misfire_policy;
            }
            if let Some(entrypoint) = input.entrypoint {
                routine.entrypoint = entrypoint;
            }
            if let Some(args) = input.args {
                routine.args = args;
            }
            if let Some(allowed_tools) = input.allowed_tools {
                routine.allowed_tools = allowed_tools;
            }
            if let Some(output_targets) = input.output_targets {
                routine.output_targets = output_targets;
            }
            if let Some(requires_approval) = input.requires_approval {
                routine.requires_approval = requires_approval;
            }
            if let Some(external_integrations_allowed) = input.external_integrations_allowed {
                routine.external_integrations_allowed = external_integrations_allowed;
            }
            if let Some(next_fire_at_ms) = input.next_fire_at_ms {
                routine.next_fire_at_ms = Some(next_fire_at_ms);
            }
        })
        .await
        .map_err(routine_error_response)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Routine not found",
                    "code": "ROUTINE_NOT_FOUND",
                    "routineID": id,
                })),
            )
        })?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.updated",
            &stored.tenant_context,
            json!({
                "routineID": stored.routine_id,
                "status": stored.status,
                "nextFireAtMs": stored.next_fire_at_ms,
            }),
        ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

pub(super) async fn routines_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine_for_tenant(&id, &tenant_context)
        .await
        .map_err(routine_error_response)?;
    if let Some(routine) = deleted {
        state
            .event_bus
            .publish(crate::routines::types::tenant_scoped_engine_event(
                "routine.deleted",
                &routine.tenant_context,
                json!({
                    "routineID": routine.routine_id,
                }),
            ));
        Ok(Json(json!({
            "deleted": true,
            "routineID": id,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        ))
    }
}

pub(super) async fn routines_run_now(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let _actor =
        ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "execution")?;
    let routine = state
        .get_routine_for_tenant(&id, &tenant_context)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Routine not found",
                    "code": "ROUTINE_NOT_FOUND",
                    "routineID": id,
                })),
            )
        })?;
    let run_count = input.run_count.unwrap_or(1).clamp(1, 20);
    let now = crate::now_ms();
    let trigger_type = "manual";
    match evaluate_routine_execution_policy(&routine, trigger_type) {
        RoutineExecutionDecision::Allowed => {
            let _ = state
                .mark_routine_fired_for_tenant(&routine.routine_id, &tenant_context, now)
                .await;
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::Queued,
                    input.reason.clone(),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    tenant_context: routine.tenant_context.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "queued".to_string(),
                    detail: input.reason,
                })
                .await;
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.fired",
                    &routine.tenant_context,
                    json!({
                        "routineID": routine.routine_id,
                        "runID": run.run_id,
                        "runCount": run_count,
                        "triggerType": trigger_type,
                        "firedAtMs": now,
                    }),
                ));
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.run.created",
                    &routine.tenant_context,
                    json!({
                        "run": run,
                    }),
                ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Ok(Json(json!({
                "ok": true,
                "status": "queued",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
                "firedAtMs": now,
                "contextRunID": context_run_id,
                "linked_context_run_id": context_run_id,
            })))
        }
        RoutineExecutionDecision::RequiresApproval { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::PendingApproval,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    tenant_context: routine.tenant_context.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "pending_approval".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.approval_required",
                    &routine.tenant_context,
                    json!({
                        "routineID": routine.routine_id,
                        "runID": run.run_id,
                        "runCount": run_count,
                        "triggerType": trigger_type,
                        "reason": reason,
                    }),
                ));
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.run.created",
                    &routine.tenant_context,
                    json!({
                        "run": run,
                    }),
                ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Ok(Json(json!({
                "ok": true,
                "status": "pending_approval",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
                "contextRunID": context_run_id,
                "linked_context_run_id": context_run_id,
            })))
        }
        RoutineExecutionDecision::Blocked { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::BlockedPolicy,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    tenant_context: routine.tenant_context.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "blocked_policy".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.blocked",
                    &routine.tenant_context,
                    json!({
                        "routineID": routine.routine_id,
                        "runID": run.run_id,
                        "runCount": run_count,
                        "triggerType": trigger_type,
                        "reason": reason,
                    }),
                ));
            state
                .event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "routine.run.created",
                    &routine.tenant_context,
                    json!({
                        "run": run,
                    }),
                ));
            let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
                .await
                .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Routine blocked by policy",
                    "code": "ROUTINE_POLICY_BLOCKED",
                    "routineID": id,
                    "runID": run.run_id,
                    "reason": reason,
                    "contextRunID": context_run_id,
                    "linked_context_run_id": context_run_id,
                })),
            ))
        }
    }
}

pub(super) async fn routines_history(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if routine_for_tenant(&state, &id, &tenant_context)
        .await
        .is_none()
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"Routine not found","code":"ROUTINE_NOT_FOUND","routineID":id})),
        ));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let events = state
        .list_routine_history_for_tenant(&id, &tenant_context, limit)
        .await;
    Ok(Json(json!({
        "routineID": id,
        "events": events,
        "count": events.len(),
    })))
}

pub(super) async fn routines_runs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if routine_for_tenant(&state, &id, &tenant_context)
        .await
        .is_none()
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"Routine not found","code":"ROUTINE_NOT_FOUND","routineID":id})),
        ));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let runs = state
        .list_routine_runs_for_tenant(Some(&id), &tenant_context, limit)
        .await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    Ok(Json(json!({
        "routineID": id,
        "runs": runs.iter().map(routine_run_with_context_links).collect::<Vec<_>>(),
        "count": runs.len(),
    })))
}

pub(super) async fn routines_runs_all(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let runs = state
        .list_routine_runs_for_tenant(query.routine_id.as_deref(), &tenant_context, limit)
        .await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    Json(json!({
        "runs": runs.iter().map(routine_run_with_context_links).collect::<Vec<_>>(),
        "count": runs.len(),
    }))
}

pub(super) async fn routines_run_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "run": routine_run_with_context_links(&run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) fn reason_or_default(input: Option<String>, fallback: &str) -> String {
    input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn clear_automation_run_execution_handles(run: &mut crate::AutomationV2RunRecord) {
    run.active_session_ids.clear();
    run.latest_session_id = None;
    run.active_instance_ids.clear();
}

pub(super) async fn routines_run_approve(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor =
        ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "approval")?;
    let Some(current) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "approved by operator");
    let updated = state
        .update_routine_run_status_for_tenant(
            &run_id,
            &tenant_context,
            RoutineRunStatus::Queued,
            Some(reason.clone()),
        )
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    crate::audit::append_protected_audit_event(
        &state,
        "routine.run.approved",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "actor": actor.clone(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.run.approved",
            &updated.tenant_context,
            json!({
                "runID": run_id,
                "routineID": updated.routine_id,
                "reason": reason,
            }),
        ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_deny(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor =
        ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "denial")?;
    let Some(current) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "denied by operator");
    let updated = state
        .update_routine_run_status_for_tenant(
            &run_id,
            &tenant_context,
            RoutineRunStatus::Denied,
            Some(reason.clone()),
        )
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    crate::audit::append_protected_audit_event(
        &state,
        "routine.run.denied",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "actor": actor.clone(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.run.denied",
            &updated.tenant_context,
            json!({
                "runID": run_id,
                "routineID": updated.routine_id,
                "reason": reason,
            }),
        ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_pause(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor = ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "pause")?;
    let Some(current) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if !matches!(
        current.status,
        RoutineRunStatus::Queued | RoutineRunStatus::Running
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not pausable",
                "code": "ROUTINE_RUN_NOT_PAUSABLE",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let mut cancelled_sessions = Vec::new();
    if current.status == RoutineRunStatus::Running {
        for session_id in &current.active_session_ids {
            if state.cancellations.cancel(session_id).await {
                let _ = state.close_browser_sessions_for_owner(session_id).await;
                cancelled_sessions.push(session_id.clone());
            }
        }
    }
    let updated = state
        .update_routine_run_status_for_tenant(
            &run_id,
            &tenant_context,
            RoutineRunStatus::Paused,
            Some(reason.clone()),
        )
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    crate::audit::append_protected_audit_event(
        &state,
        "routine.run.paused",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "cancelledSessionIDs": cancelled_sessions,
            "actor": actor.clone(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.run.paused",
            &updated.tenant_context,
            json!({
                "runID": run_id,
                "routineID": updated.routine_id,
                "reason": reason,
                "cancelledSessionIDs": cancelled_sessions,
            }),
        ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "cancelledSessionIDs": cancelled_sessions,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_resume(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let actor =
        ensure_routine_human_actor(&headers, &tenant_context, &request_principal, "resume")?;
    let Some(current) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not paused",
                "code": "ROUTINE_RUN_NOT_PAUSED",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_routine_run_status_for_tenant(
            &run_id,
            &tenant_context,
            RoutineRunStatus::Queued,
            Some(reason.clone()),
        )
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    crate::audit::append_protected_audit_event(
        &state,
        "routine.run.resumed",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "actor": actor.clone(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_response)?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.run.resumed",
            &updated.tenant_context,
            json!({
                "runID": run_id,
                "routineID": updated.routine_id,
                "reason": reason,
            }),
        ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}
