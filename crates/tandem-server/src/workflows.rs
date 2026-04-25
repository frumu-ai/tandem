use anyhow::Context;
use serde_json::{json, Value};
use std::time::Duration;
use tandem_types::{EngineEvent, MessagePartInput, SendMessageRequest, Session, TenantContext};
use tandem_workflows::{
    WorkflowActionRunRecord, WorkflowActionRunStatus, WorkflowActionSpec, WorkflowHookBinding,
    WorkflowRunRecord, WorkflowRunStatus, WorkflowSimulationResult, WorkflowSpec,
};
use uuid::Uuid;

use crate::{now_ms, AppState, WorkflowSourceRef};

#[derive(Debug, Clone)]
struct PreparedWorkflowAction {
    action_id: String,
    spec: WorkflowActionSpec,
}

fn workflow_event_tenant_context_value(tenant_context: &TenantContext) -> Value {
    serde_json::to_value(tenant_context).unwrap_or_else(|_| json!(tenant_context))
}

fn workflow_event_with_tenant_context(mut payload: Value, tenant_context: &TenantContext) -> Value {
    if let Some(map) = payload.as_object_mut() {
        map.entry("tenantContext".to_string())
            .or_insert_with(|| workflow_event_tenant_context_value(tenant_context));
    }
    payload
}

fn event_tenant_context(event: &EngineEvent) -> TenantContext {
    event
        .properties
        .get("tenantContext")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_else(TenantContext::local_implicit)
}

fn workflow_action_objective(action: &str, with: Option<&Value>) -> String {
    match with {
        Some(with) if !with.is_null() => {
            format!("Execute workflow action `{action}` with payload {with}.")
        }
        _ => format!("Execute workflow action `{action}`."),
    }
}

fn workflow_manual_schedule() -> crate::AutomationV2Schedule {
    crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    }
}

fn workflow_execution_plan(
    workflow_id: &str,
    name: &str,
    description: Option<String>,
    actions: &[PreparedWorkflowAction],
    source_label: &str,
    source: Option<&WorkflowSourceRef>,
    trigger_event: Option<&str>,
) -> crate::WorkflowPlan {
    crate::WorkflowPlan {
        plan_id: format!("workflow-plan-{workflow_id}"),
        planner_version: "workflow_runtime_v1".to_string(),
        plan_source: source_label.to_string(),
        original_prompt: description.clone().unwrap_or_else(|| name.to_string()),
        normalized_prompt: description.clone().unwrap_or_else(|| name.to_string()),
        confidence: "high".to_string(),
        title: name.to_string(),
        description,
        schedule: workflow_manual_schedule(),
        execution_target: "automation_v2".to_string(),
        workspace_root: std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string(),
        steps: actions
            .iter()
            .map(|action| crate::WorkflowPlanStep {
                step_id: action.action_id.clone(),
                kind: "workflow_action".to_string(),
                objective: workflow_action_objective(
                    &action.spec.action,
                    action.spec.with.as_ref(),
                ),
                depends_on: Vec::new(),
                agent_role: "operator".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "generic_artifact".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    enforcement: None,
                    schema: None,
                    summary_guidance: None,
                }),
                metadata: None,
            })
            .collect(),
        requires_integrations: Vec::new(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "source": source_label,
            "tool_access_mode": "auto",
        })),
        save_options: json!({
            "origin": source_label,
            "workflow_source": source,
            "trigger_event": trigger_event,
        }),
    }
}

pub(crate) fn compile_workflow_spec_to_automation_preview(
    workflow: &WorkflowSpec,
) -> crate::AutomationV2Spec {
    let actions = workflow
        .steps
        .iter()
        .map(|step| PreparedWorkflowAction {
            action_id: step.step_id.clone(),
            spec: WorkflowActionSpec {
                action: step.action.clone(),
                with: step.with.clone(),
            },
        })
        .collect::<Vec<_>>();
    let mut automation = crate::http::compile_plan_to_automation_v2(
        &workflow_execution_plan(
            &workflow.workflow_id,
            &workflow.name,
            workflow.description.clone(),
            &actions,
            "workflow_registry",
            workflow.source.as_ref(),
            None,
        ),
        None,
        "workflow_registry",
    );
    if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
        metadata.insert("workflow_id".to_string(), json!(workflow.workflow_id));
        metadata.insert("workflow_name".to_string(), json!(workflow.name));
        metadata.insert("workflow_source".to_string(), json!(workflow.source));
        metadata.insert("workflow_enabled".to_string(), json!(workflow.enabled));
    }
    automation
}

fn compile_workflow_run_automation(
    workflow_id: &str,
    workflow_name: Option<&str>,
    workflow_description: Option<&str>,
    binding_id: Option<&str>,
    actions: &[PreparedWorkflowAction],
    source: Option<&WorkflowSourceRef>,
    trigger_event: Option<&str>,
) -> crate::AutomationV2Spec {
    let automation_id = binding_id
        .map(|binding| format!("workflow-hook-automation-{workflow_id}-{binding}"))
        .unwrap_or_else(|| format!("workflow-automation-{workflow_id}"));
    let title = binding_id
        .map(|binding| {
            workflow_name
                .map(|name| format!("{name} hook {binding}"))
                .unwrap_or_else(|| format!("Workflow Hook {workflow_id}:{binding}"))
        })
        .unwrap_or_else(|| {
            workflow_name
                .map(|name| format!("{name} execution"))
                .unwrap_or_else(|| format!("Workflow {workflow_id}"))
        });
    let mut automation = crate::http::compile_plan_to_automation_v2(
        &workflow_execution_plan(
            &automation_id,
            &title,
            Some(
                workflow_description
                    .map(|description| description.to_string())
                    .unwrap_or_else(|| format!("Mirrored workflow execution for `{workflow_id}`.")),
            ),
            actions,
            "workflow_runtime",
            source,
            trigger_event,
        ),
        None,
        "workflow_runtime",
    );
    automation.automation_id = automation_id.clone();
    automation.name = title;
    automation.metadata = Some(json!({
        "workflow_id": workflow_id,
        "binding_id": binding_id,
        "workflow_source": source,
        "trigger_event": trigger_event,
        "origin": "workflow_runtime_mirror",
    }));
    automation
}

async fn sync_workflow_automation_run_start(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run_id: &str,
) -> anyhow::Result<crate::AutomationV2RunRecord> {
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.status = crate::AutomationRunStatus::Running;
            run.started_at_ms.get_or_insert_with(now_ms);
            crate::app::state::automation::lifecycle::record_automation_lifecycle_event(
                run,
                "workflow_run_started",
                Some("workflow runtime mirror started".to_string()),
                None,
            );
            crate::app::state::refresh_automation_runtime_state(automation, run);
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{run_id}` not found"))?;
    crate::http::context_runs::sync_automation_v2_run_blackboard(state, automation, &updated)
        .await
        .map_err(|status| anyhow::anyhow!("failed to sync workflow automation run: {status}"))?;
    Ok(updated)
}

async fn sync_workflow_automation_action_started(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run_id: &str,
    action_id: &str,
) -> anyhow::Result<crate::AutomationV2RunRecord> {
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            let next_attempt = run
                .checkpoint
                .node_attempts
                .get(action_id)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            run.checkpoint
                .node_attempts
                .insert(action_id.to_string(), next_attempt);
            run.detail = Some(format!("Running workflow action `{action_id}`"));
            crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                run,
                "workflow_action_started",
                Some(format!("workflow action `{action_id}` started")),
                None,
                Some(json!({
                    "action_id": action_id,
                    "attempt": next_attempt,
                })),
            );
            crate::app::state::refresh_automation_runtime_state(automation, run);
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{run_id}` not found"))?;
    crate::http::context_runs::sync_automation_v2_run_blackboard(state, automation, &updated)
        .await
        .map_err(|status| anyhow::anyhow!("failed to sync workflow automation run: {status}"))?;
    Ok(updated)
}

async fn sync_workflow_automation_action_completed(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run_id: &str,
    action_id: &str,
    output: &Value,
) -> anyhow::Result<crate::AutomationV2RunRecord> {
    let action_count = automation.flow.nodes.len();
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.checkpoint.pending_nodes.retain(|id| id != action_id);
            if !run
                .checkpoint
                .completed_nodes
                .iter()
                .any(|id| id == action_id)
            {
                run.checkpoint.completed_nodes.push(action_id.to_string());
            }
            run.checkpoint.node_outputs.insert(
                action_id.to_string(),
                json!(crate::AutomationNodeOutput {
                    contract_kind: "generic_artifact".to_string(),
                    validator_kind: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    validator_summary: Some(crate::AutomationValidatorSummary {
                        kind: crate::AutomationOutputValidatorKind::GenericArtifact,
                        outcome: "accepted".to_string(),
                        reason: Some("workflow action completed".to_string()),
                        unmet_requirements: Vec::new(),
                        warning_requirements: Vec::new(),
                        warning_count: 0,
                        accepted_candidate_source: Some("workflow_runtime".to_string()),
                        verification_outcome: Some("not_applicable".to_string()),
                        validation_basis: None,
                        repair_attempted: false,
                        repair_attempt: 0,
                        repair_attempts_remaining: tandem_core::prewrite_repair_retry_max_attempts()
                            as u32,
                        repair_succeeded: false,
                        repair_exhausted: false,
                    }),
                    summary: format!("Workflow action `{action_id}` completed"),
                    content: json!({
                        "action_id": action_id,
                        "output": output,
                    }),
                    created_at_ms: now_ms(),
                    node_id: action_id.to_string(),
                    status: Some("completed".to_string()),
                    blocked_reason: None,
                    approved: None,
                    workflow_class: Some("workflow_action".to_string()),
                    phase: Some("execution".to_string()),
                    failure_kind: None,
                    tool_telemetry: None,
                    preflight: None,
                    knowledge_preflight: None,
                    capability_resolution: None,
                    attempt_evidence: None,
                    blocker_category: None,
                    fallback_used: None,
                    artifact_validation: None,
                    receipt_timeline: None,
                    quality_mode: Some("strict_research_v1".to_string()),
                    requested_quality_mode: None,
                    emergency_rollback_enabled: Some(false),
                    provenance: Some(crate::AutomationNodeOutputProvenance {
                        session_id: format!("workflow-runtime-{run_id}"),
                        node_id: action_id.to_string(),
                        run_id: Some(run_id.to_string()),
                        output_path: None,
                        content_digest: None,
                        accepted_candidate_source: Some("workflow_runtime".to_string()),
                        validation_outcome: Some("not_applicable".to_string()),
                        repair_attempt: Some(0),
                        repair_succeeded: Some(false),
                        reuse_allowed: Some(false),
                        freshness: crate::AutomationNodeOutputFreshness {
                            current_run: true,
                            current_attempt: true,
                        },
                    }),
                }),
            );
            if run.checkpoint.completed_nodes.len() >= action_count {
                run.status = crate::AutomationRunStatus::Completed;
                run.detail = Some("workflow runtime mirror completed".to_string());
            }
            crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                run,
                "workflow_action_completed",
                Some(format!("workflow action `{action_id}` completed")),
                None,
                Some(json!({
                    "action_id": action_id,
                })),
            );
            crate::app::state::refresh_automation_runtime_state(automation, run);
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{run_id}` not found"))?;
    crate::http::context_runs::sync_automation_v2_run_blackboard(state, automation, &updated)
        .await
        .map_err(|status| anyhow::anyhow!("failed to sync workflow automation run: {status}"))?;
    Ok(updated)
}

async fn sync_workflow_automation_action_failed(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run_id: &str,
    action_id: &str,
    error: &str,
) -> anyhow::Result<crate::AutomationV2RunRecord> {
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.status = crate::AutomationRunStatus::Failed;
            run.detail = Some(format!("Workflow action `{action_id}` failed"));
            run.checkpoint.pending_nodes.retain(|id| id != action_id);
            run.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                node_id: action_id.to_string(),
                reason: error.to_string(),
                failed_at_ms: now_ms(),
            });
            run.checkpoint.node_outputs.insert(
                action_id.to_string(),
                json!(crate::AutomationNodeOutput {
                    contract_kind: "generic_artifact".to_string(),
                    validator_kind: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    validator_summary: Some(crate::AutomationValidatorSummary {
                        kind: crate::AutomationOutputValidatorKind::GenericArtifact,
                        outcome: "rejected".to_string(),
                        reason: Some(error.to_string()),
                        unmet_requirements: vec![error.to_string()],
                        warning_requirements: Vec::new(),
                        warning_count: 0,
                        accepted_candidate_source: None,
                        verification_outcome: Some("failed".to_string()),
                        validation_basis: None,
                        repair_attempted: false,
                        repair_attempt: 0,
                        repair_attempts_remaining: tandem_core::prewrite_repair_retry_max_attempts()
                            as u32,
                        repair_succeeded: false,
                        repair_exhausted: false,
                    }),
                    summary: format!("Workflow action `{action_id}` failed"),
                    content: json!({
                        "action_id": action_id,
                        "error": error,
                    }),
                    created_at_ms: now_ms(),
                    node_id: action_id.to_string(),
                    status: Some("failed".to_string()),
                    blocked_reason: Some(error.to_string()),
                    approved: None,
                    workflow_class: Some("workflow_action".to_string()),
                    phase: Some("execution".to_string()),
                    failure_kind: Some("workflow_action_failed".to_string()),
                    tool_telemetry: None,
                    preflight: None,
                    knowledge_preflight: None,
                    capability_resolution: None,
                    attempt_evidence: None,
                    blocker_category: Some("tool_result_unusable".to_string()),
                    fallback_used: None,
                    artifact_validation: None,
                    receipt_timeline: None,
                    quality_mode: Some("strict_research_v1".to_string()),
                    requested_quality_mode: None,
                    emergency_rollback_enabled: Some(false),
                    provenance: Some(crate::AutomationNodeOutputProvenance {
                        session_id: format!("workflow-runtime-{run_id}"),
                        node_id: action_id.to_string(),
                        run_id: Some(run_id.to_string()),
                        output_path: None,
                        content_digest: None,
                        accepted_candidate_source: None,
                        validation_outcome: Some("failed".to_string()),
                        repair_attempt: Some(0),
                        repair_succeeded: Some(false),
                        reuse_allowed: Some(false),
                        freshness: crate::AutomationNodeOutputFreshness {
                            current_run: true,
                            current_attempt: true,
                        },
                    }),
                }),
            );
            crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
                run,
                "workflow_action_failed",
                Some(format!("workflow action `{action_id}` failed")),
                None,
                Some(json!({
                    "action_id": action_id,
                    "error": error,
                })),
            );
            crate::app::state::refresh_automation_runtime_state(automation, run);
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{run_id}` not found"))?;
    crate::http::context_runs::sync_automation_v2_run_blackboard(state, automation, &updated)
        .await
        .map_err(|status| anyhow::anyhow!("failed to sync workflow automation run: {status}"))?;
    Ok(updated)
}

#[derive(Debug, Clone)]
pub enum ParsedWorkflowAction {
    EventEmit { event_type: String },
    ResourcePut { key: String },
    ResourcePatch { key: String },
    ResourceDelete { key: String },
    Tool { tool_name: String },
    Capability { capability_id: String },
    Workflow { workflow_id: String },
    Agent { agent_id: String },
}

pub fn parse_workflow_action(action: &str) -> ParsedWorkflowAction {
    let trimmed = action.trim();
    if let Some(rest) = trimmed.strip_prefix("event:") {
        return ParsedWorkflowAction::EventEmit {
            event_type: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("resource:put:") {
        return ParsedWorkflowAction::ResourcePut {
            key: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("resource:patch:") {
        return ParsedWorkflowAction::ResourcePatch {
            key: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("resource:delete:") {
        return ParsedWorkflowAction::ResourceDelete {
            key: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("tool:") {
        return ParsedWorkflowAction::Tool {
            tool_name: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("capability:") {
        return ParsedWorkflowAction::Capability {
            capability_id: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("workflow:") {
        return ParsedWorkflowAction::Workflow {
            workflow_id: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix("agent:") {
        return ParsedWorkflowAction::Agent {
            agent_id: rest.trim().to_string(),
        };
    }
    ParsedWorkflowAction::Capability {
        capability_id: trimmed.to_string(),
    }
}

pub fn canonical_workflow_event_names(event: &EngineEvent) -> Vec<String> {
    let mut names = vec![event.event_type.clone(), event.event_type.replace('.', "_")];
    match event.event_type.as_str() {
        "context.task.created" => names.push("task_created".to_string()),
        "context.task.started" => names.push("task_started".to_string()),
        "context.task.completed" => names.push("task_completed".to_string()),
        "context.task.failed" => names.push("task_failed".to_string()),
        "workflow.run.started" | "routine.run.created" => {
            names.push("workflow_started".to_string())
        }
        "workflow.run.completed" | "routine.run.completed" => {
            names.push("workflow_completed".to_string())
        }
        "workflow.run.failed" | "routine.run.failed" => names.push("task_failed".to_string()),
        _ => {}
    }
    names.sort();
    names.dedup();
    names
}

pub async fn simulate_workflow_event(
    state: &AppState,
    event: &EngineEvent,
) -> WorkflowSimulationResult {
    let registry = state.workflow_registry().await;
    let canonical = canonical_workflow_event_names(event);
    let matched_bindings = registry
        .hooks
        .into_iter()
        .filter(|hook| {
            hook.enabled
                && canonical
                    .iter()
                    .any(|name| event_name_matches(&hook.event, name))
        })
        .collect::<Vec<_>>();
    let planned_actions = matched_bindings
        .iter()
        .flat_map(|hook| hook.actions.clone())
        .collect::<Vec<_>>();
    WorkflowSimulationResult {
        matched_bindings,
        planned_actions,
        canonical_events: canonical,
    }
}

pub async fn dispatch_workflow_event(state: &AppState, event: &EngineEvent) {
    let simulation = simulate_workflow_event(state, event).await;
    if simulation.matched_bindings.is_empty() {
        return;
    }
    for hook in simulation.matched_bindings {
        let source_event_id = source_event_id(event);
        let task_id = task_id_from_event(event);
        let tenant_context = event_tenant_context(event);
        let dedupe_key = format!("{}::{source_event_id}", hook.binding_id);
        {
            let mut seen = state.workflow_dispatch_seen.write().await;
            if seen.contains_key(&dedupe_key) {
                continue;
            }
            seen.insert(dedupe_key, now_ms());
        }
        let _ = execute_hook_binding(
            state,
            &hook,
            tenant_context.clone(),
            Some(event.event_type.clone()),
            Some(source_event_id),
            task_id,
            false,
        )
        .await;
    }
}

pub async fn run_workflow_dispatcher(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        let startup = state.startup_snapshot().await;
        tracing::warn!(
            component = "workflow_dispatcher",
            startup_status = ?startup.status,
            startup_phase = %startup.phase,
            attempt_id = %startup.attempt_id,
            "workflow dispatcher exiting before runtime access because startup did not become ready"
        );
        return;
    }
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => dispatch_workflow_event(&state, &event).await,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub async fn execute_workflow(
    state: &AppState,
    workflow: &WorkflowSpec,
    tenant_context: TenantContext,
    trigger_event: Option<String>,
    source_event_id: Option<String>,
    task_id: Option<String>,
    dry_run: bool,
) -> anyhow::Result<WorkflowRunRecord> {
    let actions = workflow
        .steps
        .iter()
        .map(|step| PreparedWorkflowAction {
            action_id: step.step_id.clone(),
            spec: WorkflowActionSpec {
                action: step.action.clone(),
                with: step.with.clone(),
            },
        })
        .collect::<Vec<_>>();
    execute_actions(
        state,
        &workflow.workflow_id,
        None,
        actions,
        Some(workflow.name.clone()),
        workflow.description.clone(),
        workflow.source.clone(),
        tenant_context,
        trigger_event,
        source_event_id,
        task_id,
        dry_run,
    )
    .await
}

pub async fn execute_hook_binding(
    state: &AppState,
    hook: &WorkflowHookBinding,
    tenant_context: TenantContext,
    trigger_event: Option<String>,
    source_event_id: Option<String>,
    task_id: Option<String>,
    dry_run: bool,
) -> anyhow::Result<WorkflowRunRecord> {
    let workflow = state
        .get_workflow(&hook.workflow_id)
        .await
        .with_context(|| format!("unknown workflow `{}`", hook.workflow_id))?;
    execute_actions(
        state,
        &hook.workflow_id,
        Some(hook.binding_id.clone()),
        hook.actions
            .iter()
            .enumerate()
            .map(|(idx, action)| PreparedWorkflowAction {
                action_id: format!("action_{}", idx + 1),
                spec: action.clone(),
            })
            .collect(),
        None,
        None,
        workflow.source,
        tenant_context,
        trigger_event,
        source_event_id,
        task_id,
        dry_run,
    )
    .await
}

async fn execute_actions(
    state: &AppState,
    workflow_id: &str,
    binding_id: Option<String>,
    actions: Vec<PreparedWorkflowAction>,
    workflow_name: Option<String>,
    workflow_description: Option<String>,
    source: Option<WorkflowSourceRef>,
    tenant_context: TenantContext,
    trigger_event: Option<String>,
    source_event_id: Option<String>,
    task_id: Option<String>,
    dry_run: bool,
) -> anyhow::Result<WorkflowRunRecord> {
    let run_id = format!("workflow-run-{}", Uuid::new_v4());
    let now = now_ms();
    let automation = compile_workflow_run_automation(
        workflow_id,
        workflow_name.as_deref(),
        workflow_description.as_deref(),
        binding_id.as_deref(),
        &actions,
        source.as_ref(),
        trigger_event.as_deref(),
    );
    let automation = state.put_automation_v2(automation).await?;
    let automation_run = state
        .create_automation_v2_run(&automation, trigger_event.as_deref().unwrap_or("workflow"))
        .await?;
    let automation_run =
        sync_workflow_automation_run_start(state, &automation, &automation_run.run_id).await?;
    let mut run = WorkflowRunRecord {
        run_id: run_id.clone(),
        workflow_id: workflow_id.to_string(),
        tenant_context: tenant_context.clone(),
        automation_id: Some(automation.automation_id.clone()),
        automation_run_id: Some(automation_run.run_id.clone()),
        binding_id,
        trigger_event: trigger_event.clone(),
        source_event_id: source_event_id.clone(),
        task_id: task_id.clone(),
        status: if dry_run {
            WorkflowRunStatus::DryRun
        } else {
            WorkflowRunStatus::Running
        },
        created_at_ms: now,
        updated_at_ms: now,
        finished_at_ms: if dry_run { Some(now) } else { None },
        actions: actions
            .iter()
            .map(|action| WorkflowActionRunRecord {
                action_id: action.action_id.clone(),
                action: action.spec.action.clone(),
                task_id: task_id.clone(),
                status: if dry_run {
                    WorkflowActionRunStatus::Skipped
                } else {
                    WorkflowActionRunStatus::Pending
                },
                detail: None,
                output: None,
                updated_at_ms: now,
            })
            .collect(),
        source,
    };
    state.put_workflow_run(run.clone()).await?;
    let _ = crate::http::sync_workflow_run_blackboard(state, &run).await;
    state.event_bus.publish(EngineEvent::new(
        "workflow.run.started",
        workflow_event_with_tenant_context(
            json!({
            "runID": run.run_id,
            "workflowID": run.workflow_id,
            "bindingID": run.binding_id,
            "triggerEvent": trigger_event,
            "sourceEventID": source_event_id,
            "taskID": task_id,
            "dryRun": dry_run,
            }),
            &run.tenant_context,
        ),
    ));
    if dry_run {
        return Ok(run);
    }
    for (action_row, action_spec) in run.actions.iter_mut().zip(actions.iter()) {
        action_row.status = WorkflowActionRunStatus::Running;
        action_row.updated_at_ms = now_ms();
        let action_name = action_row.action.clone();
        let _ = sync_workflow_automation_action_started(
            state,
            &automation,
            automation_run.run_id.as_str(),
            &action_row.action_id,
        )
        .await;
        state
            .update_workflow_run(&run.run_id, |row| {
                if let Some(target) = row
                    .actions
                    .iter_mut()
                    .find(|item| item.action_id == action_row.action_id)
                {
                    *target = action_row.clone();
                }
            })
            .await;
        if let Some(latest) = state.get_workflow_run(&run.run_id).await {
            let _ = crate::http::sync_workflow_run_blackboard(state, &latest).await;
        }
        state.event_bus.publish(EngineEvent::new(
            "workflow.action.started",
            workflow_event_with_tenant_context(
                json!({
                "runID": run.run_id,
                "workflowID": run.workflow_id,
                "actionID": action_row.action_id,
                "action": action_name,
                "taskID": run.task_id,
                }),
                &run.tenant_context,
            ),
        ));
        match execute_action(
            state,
            &run.run_id,
            workflow_id,
            &action_spec.spec,
            action_row,
            &run.tenant_context,
            trigger_event.clone(),
        )
        .await
        {
            Ok(output) => {
                action_row.status = WorkflowActionRunStatus::Completed;
                action_row.output = Some(output.clone());
                action_row.updated_at_ms = now_ms();
                state
                    .update_workflow_run(&run.run_id, |row| {
                        if let Some(target) = row
                            .actions
                            .iter_mut()
                            .find(|item| item.action_id == action_row.action_id)
                        {
                            *target = action_row.clone();
                        }
                    })
                    .await;
                let _ = sync_workflow_automation_action_completed(
                    state,
                    &automation,
                    automation_run.run_id.as_str(),
                    &action_row.action_id,
                    &output,
                )
                .await;
                if let Some(latest) = state.get_workflow_run(&run.run_id).await {
                    let _ = crate::http::sync_workflow_run_blackboard(state, &latest).await;
                }
                state.event_bus.publish(EngineEvent::new(
                    "workflow.action.completed",
                    workflow_event_with_tenant_context(
                        json!({
                        "runID": run.run_id,
                        "workflowID": run.workflow_id,
                        "actionID": action_row.action_id,
                        "action": action_name,
                        "taskID": run.task_id,
                        "output": output,
                        }),
                        &run.tenant_context,
                    ),
                ));
            }
            Err(error) => {
                let detail = error.to_string();
                action_row.status = WorkflowActionRunStatus::Failed;
                action_row.detail = Some(detail.clone());
                action_row.updated_at_ms = now_ms();
                run.status = WorkflowRunStatus::Failed;
                run.finished_at_ms = Some(now_ms());
                state
                    .update_workflow_run(&run.run_id, |row| {
                        row.status = WorkflowRunStatus::Failed;
                        row.finished_at_ms = Some(now_ms());
                        if let Some(target) = row
                            .actions
                            .iter_mut()
                            .find(|item| item.action_id == action_row.action_id)
                        {
                            *target = action_row.clone();
                        }
                    })
                    .await;
                let _ = sync_workflow_automation_action_failed(
                    state,
                    &automation,
                    automation_run.run_id.as_str(),
                    &action_row.action_id,
                    &detail,
                )
                .await;
                if let Some(latest) = state.get_workflow_run(&run.run_id).await {
                    let _ = crate::http::sync_workflow_run_blackboard(state, &latest).await;
                }
                state.event_bus.publish(EngineEvent::new(
                    "workflow.action.failed",
                    workflow_event_with_tenant_context(
                        json!({
                        "runID": run.run_id,
                        "workflowID": run.workflow_id,
                        "actionID": action_row.action_id,
                        "action": action_name,
                        "taskID": run.task_id,
                        "error": detail,
                        }),
                        &run.tenant_context,
                    ),
                ));
                state.event_bus.publish(EngineEvent::new(
                    "workflow.run.failed",
                    workflow_event_with_tenant_context(
                        json!({
                        "runID": run.run_id,
                        "workflowID": run.workflow_id,
                        "actionID": action_row.action_id,
                        "taskID": run.task_id,
                        "error": action_row.detail,
                        }),
                        &run.tenant_context,
                    ),
                ));
                return state.get_workflow_run(&run.run_id).await.with_context(|| {
                    format!("workflow run `{}` missing after failure", run.run_id)
                });
            }
        }
    }
    run.status = WorkflowRunStatus::Completed;
    run.finished_at_ms = Some(now_ms());
    let final_run = state
        .update_workflow_run(&run.run_id, |row| {
            row.status = WorkflowRunStatus::Completed;
            row.finished_at_ms = Some(now_ms());
        })
        .await
        .with_context(|| format!("workflow run `{}` missing on completion", run.run_id))?;
    let _ = crate::http::sync_workflow_run_blackboard(state, &final_run).await;
    state.event_bus.publish(EngineEvent::new(
        "workflow.run.completed",
        workflow_event_with_tenant_context(
            json!({
            "runID": final_run.run_id,
            "workflowID": final_run.workflow_id,
            "bindingID": final_run.binding_id,
            "taskID": final_run.task_id,
            }),
            &final_run.tenant_context,
        ),
    ));
    Ok(final_run)
}

async fn execute_action(
    state: &AppState,
    run_id: &str,
    workflow_id: &str,
    action_spec: &WorkflowActionSpec,
    action_row: &WorkflowActionRunRecord,
    tenant_context: &TenantContext,
    trigger_event: Option<String>,
) -> anyhow::Result<Value> {
    let action_name = action_spec.action.as_str();
    let parsed = parse_workflow_action(action_name);
    match parsed {
        ParsedWorkflowAction::EventEmit { event_type } => {
            let payload = action_payload(action_spec, action_row);
            state.event_bus.publish(EngineEvent::new(
                event_type.clone(),
                workflow_event_with_tenant_context(
                    json!({
                    "workflowID": workflow_id,
                    "actionID": action_row.action_id,
                    "triggerEvent": trigger_event,
                    "payload": payload,
                    }),
                    tenant_context,
                ),
            ));
            Ok(json!({ "eventType": event_type }))
        }
        ParsedWorkflowAction::ResourcePut { key } => {
            let record = state
                .put_shared_resource(
                    key.clone(),
                    action_payload(action_spec, action_row),
                    None,
                    "workflow".to_string(),
                    None,
                )
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))?;
            Ok(json!({ "key": record.key, "rev": record.rev }))
        }
        ParsedWorkflowAction::ResourcePatch { key } => {
            let current = state.get_shared_resource(&key).await;
            let next_rev = current.as_ref().map(|row| row.rev);
            let record = state
                .put_shared_resource(
                    key.clone(),
                    merge_object(
                        current.map(|row| row.value).unwrap_or_else(|| json!({})),
                        action_payload(action_spec, action_row),
                    ),
                    next_rev,
                    "workflow".to_string(),
                    None,
                )
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))?;
            Ok(json!({ "key": record.key, "rev": record.rev }))
        }
        ParsedWorkflowAction::ResourceDelete { key } => {
            let deleted = state
                .delete_shared_resource(&key, None)
                .await
                .map_err(|err| anyhow::anyhow!("{err:?}"))?;
            Ok(json!({ "key": key, "deleted": deleted.is_some() }))
        }
        ParsedWorkflowAction::Tool { tool_name } => {
            let payload = action_payload(action_spec, action_row);
            let result = state.tools.execute(&tool_name, payload.clone()).await?;
            let mut response = json!({
                "tool": tool_name,
                "output": result.output,
                "metadata": result.metadata,
            });
            if let Some(external_action) = record_workflow_external_action(
                state,
                run_id,
                workflow_id,
                action_row,
                trigger_event.clone(),
                WorkflowExternalActionExecution::Tool {
                    tool_name: tool_name.clone(),
                },
                &payload,
                &response,
            )
            .await?
            {
                if let Some(obj) = response.as_object_mut() {
                    obj.insert("external_action".to_string(), external_action);
                }
            }
            Ok(response)
        }
        ParsedWorkflowAction::Capability { capability_id } => {
            let bindings = state.capability_resolver.list_bindings().await?;
            let tool_name = bindings
                .bindings
                .iter()
                .find(|binding| binding.capability_id == capability_id)
                .map(|binding| binding.tool_name.clone())
                .unwrap_or_else(|| capability_id.clone());
            let payload = action_payload(action_spec, action_row);
            let result = state.tools.execute(&tool_name, payload.clone()).await?;
            let mut response = json!({
                "capability": capability_id,
                "tool": tool_name,
                "output": result.output,
                "metadata": result.metadata,
            });
            if let Some(external_action) = record_workflow_external_action(
                state,
                run_id,
                workflow_id,
                action_row,
                trigger_event.clone(),
                WorkflowExternalActionExecution::Capability {
                    capability_id: capability_id.clone(),
                    tool_name: tool_name.clone(),
                },
                &payload,
                &response,
            )
            .await?
            {
                if let Some(obj) = response.as_object_mut() {
                    obj.insert("external_action".to_string(), external_action);
                }
            }
            Ok(response)
        }
        ParsedWorkflowAction::Workflow { workflow_id } => {
            anyhow::bail!("nested workflow action `{workflow_id}` is not supported in this slice")
        }
        ParsedWorkflowAction::Agent { agent_id } => {
            let workspace_root = state.workspace_index.snapshot().await.root;
            let session = Session::new(
                Some(format!("Workflow {} / {}", workflow_id, agent_id)),
                Some(workspace_root.clone()),
            );
            let session_id = session.id.clone();
            state.storage.save_session(session).await?;
            let prompt = action_spec
                .with
                .as_ref()
                .and_then(|v| v.get("prompt"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("Execute workflow action `{}`", action_name));
            let request = SendMessageRequest {
                parts: vec![MessagePartInput::Text { text: prompt }],
                model: None,
                agent: Some(agent_id.clone()),
                tool_mode: None,
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
            };
            state
                .engine_loop
                .run_prompt_async_with_context(
                    session_id.clone(),
                    request,
                    Some(format!("workflow:{workflow_id}")),
                )
                .await?;
            Ok(json!({ "agentID": agent_id, "sessionID": session_id }))
        }
    }
}

enum WorkflowExternalActionExecution {
    Tool {
        tool_name: String,
    },
    Capability {
        capability_id: String,
        tool_name: String,
    },
}

async fn record_workflow_external_action(
    state: &AppState,
    run_id: &str,
    workflow_id: &str,
    action_row: &WorkflowActionRunRecord,
    trigger_event: Option<String>,
    execution: WorkflowExternalActionExecution,
    payload: &Value,
    result: &Value,
) -> anyhow::Result<Option<Value>> {
    let bindings = state.capability_resolver.list_bindings().await?;
    let binding = match execution {
        WorkflowExternalActionExecution::Tool { ref tool_name } => bindings
            .bindings
            .iter()
            .find(|binding| workflow_binding_matches_tool_name(binding, tool_name)),
        WorkflowExternalActionExecution::Capability {
            ref capability_id,
            ref tool_name,
        } => bindings.bindings.iter().find(|binding| {
            binding.capability_id == *capability_id
                && workflow_binding_matches_tool_name(binding, tool_name)
        }),
    };
    let Some(binding) = binding else {
        return Ok(None);
    };

    let target = workflow_external_action_target(payload, result);
    let source_id = format!("{run_id}:{}", action_row.action_id);
    let idempotency_key = crate::sha256_hex(&[
        workflow_id,
        run_id,
        &action_row.action_id,
        &action_row.action,
        &payload.to_string(),
    ]);
    let action = crate::ExternalActionRecord {
        action_id: format!("workflow-external-{}", &idempotency_key[..16]),
        operation: binding.capability_id.clone(),
        status: "posted".to_string(),
        source_kind: Some("workflow".to_string()),
        source_id: Some(source_id.clone()),
        routine_run_id: None,
        context_run_id: Some(crate::http::context_runs::workflow_context_run_id(run_id)),
        capability_id: Some(binding.capability_id.clone()),
        provider: Some(binding.provider.clone()),
        target,
        approval_state: Some("executed".to_string()),
        idempotency_key: Some(idempotency_key),
        receipt: Some(result.clone()),
        error: None,
        metadata: Some(json!({
            "workflowID": workflow_id,
            "workflowRunID": run_id,
            "actionID": action_row.action_id,
            "action": action_row.action,
            "taskID": action_row.task_id,
            "triggerEvent": trigger_event,
            "tool": binding.tool_name,
            "provider": binding.provider,
            "input": payload,
        })),
        created_at_ms: action_row.updated_at_ms,
        updated_at_ms: action_row.updated_at_ms,
    };
    let recorded = state.record_external_action(action).await?;
    Ok(Some(serde_json::to_value(&recorded)?))
}

fn workflow_binding_matches_tool_name(
    binding: &crate::capability_resolver::CapabilityBinding,
    tool_name: &str,
) -> bool {
    binding.tool_name.eq_ignore_ascii_case(tool_name)
        || binding
            .tool_name_aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(tool_name))
}

fn workflow_external_action_target(payload: &Value, result: &Value) -> Option<String> {
    for candidate in [
        payload.pointer("/owner_repo").and_then(Value::as_str),
        payload.pointer("/repo").and_then(Value::as_str),
        payload.pointer("/repository").and_then(Value::as_str),
        payload.pointer("/channel").and_then(Value::as_str),
        payload.pointer("/channel_id").and_then(Value::as_str),
        payload.pointer("/thread_ts").and_then(Value::as_str),
        result.pointer("/metadata/channel").and_then(Value::as_str),
        result.pointer("/metadata/repo").and_then(Value::as_str),
    ] {
        let trimmed = candidate.map(str::trim).unwrap_or_default();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn action_payload(action_spec: &WorkflowActionSpec, action_row: &WorkflowActionRunRecord) -> Value {
    action_spec
        .with
        .clone()
        .unwrap_or_else(|| json!({ "action_id": action_row.action_id }))
}

fn merge_object(current: Value, patch: Value) -> Value {
    if let (Some(mut current_obj), Some(patch_obj)) =
        (current.as_object().cloned(), patch.as_object())
    {
        for (key, value) in patch_obj {
            current_obj.insert(key.clone(), value.clone());
        }
        Value::Object(current_obj)
    } else {
        patch
    }
}

fn source_event_id(event: &EngineEvent) -> String {
    if let Some(id) = event.properties.get("event_id").and_then(|v| v.as_str()) {
        return id.to_string();
    }
    for key in ["runID", "runId", "task_id", "taskID", "sessionID"] {
        if let Some(id) = event.properties.get(key).and_then(|v| v.as_str()) {
            return format!("{}:{id}", event.event_type);
        }
    }
    format!("{}:{}", event.event_type, event.properties)
}

fn task_id_from_event(event: &EngineEvent) -> Option<String> {
    for key in ["task_id", "taskID", "step_id", "stepID"] {
        if let Some(id) = event.properties.get(key).and_then(|v| v.as_str()) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn event_name_matches(expected: &str, actual: &str) -> bool {
    expected.trim().eq_ignore_ascii_case(actual.trim())
}
