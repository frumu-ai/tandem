use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_plan_compiler::api as compiler_api;
use uuid::Uuid;

use super::*;

#[allow(unused_imports)]
pub(crate) use compiler_api::planner_model_spec;

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanPreviewRequest {
    pub prompt: String,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlanApplyRequest {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub plan: Option<crate::WorkflowPlan>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub pack_builder_export: Option<WorkflowPlanPackBuilderExportRequest>,
    #[serde(default)]
    pub overlap_decision: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanImportRequest {
    pub bundle: compiler_api::PlanPackageImportBundle,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub project_slug: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(super) struct WorkflowPlanPackBuilderExportRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub thread_key: Option<String>,
    #[serde(default)]
    pub auto_apply: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatStartRequest {
    pub prompt: String,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatMessageRequest {
    pub plan_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatResetRequest {
    pub plan_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPlannerSessionRecord {
    pub session_id: String,
    pub project_slug: String,
    pub title: String,
    pub workspace_root: String,
    #[serde(default = "default_workflow_planner_source_kind")]
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<crate::WorkflowPlanDraftRecord>,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub planner_provider: String,
    #[serde(default)]
    pub planner_model: String,
    #[serde(default)]
    pub plan_source: String,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_preferences: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_validation: Option<compiler_api::PlanReplayReport>,
    #[serde(default)]
    pub import_transform_log: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_scope_snapshot: Option<compiler_api::PlanScopeSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at_ms: Option<u64>,
    #[serde(default)]
    pub published_tasks: Vec<Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionListQuery {
    #[serde(default)]
    pub project_slug: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionCreateRequest {
    pub project_slug: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub planner_provider: Option<String>,
    #[serde(default)]
    pub planner_model: Option<String>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub plan: Option<crate::WorkflowPlan>,
    #[serde(default)]
    pub conversation: Option<crate::WorkflowPlanConversation>,
    #[serde(default)]
    pub planner_diagnostics: Option<Value>,
    #[serde(default)]
    pub plan_revision: Option<u32>,
    #[serde(default)]
    pub last_success_materialization: Option<Value>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionPatchRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub planner_provider: Option<String>,
    #[serde(default)]
    pub planner_model: Option<String>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Option<Vec<String>>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
    #[serde(default)]
    pub current_plan_id: Option<String>,
    #[serde(default)]
    pub draft: Option<crate::WorkflowPlanDraftRecord>,
    #[serde(default)]
    pub published_at_ms: Option<u64>,
    #[serde(default)]
    pub published_tasks: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionStartRequest {
    pub prompt: String,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionMessageRequest {
    pub message: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlannerSessionDuplicateRequest {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowPlannerSessionListItem {
    pub session_id: String,
    pub title: String,
    pub project_slug: String,
    pub workspace_root: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_plan_id: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_model: Option<String>,
}

fn workflow_plan_import_summary(plan_package: &compiler_api::PlanPackage) -> serde_json::Value {
    json!({
        "plan_id": plan_package.plan_id,
        "plan_revision": plan_package.plan_revision,
        "routine_count": plan_package.routine_graph.len(),
        "context_object_count": plan_package.context_objects.len(),
        "credential_envelope_count": plan_package.credential_envelopes.len(),
    })
}

fn default_workflow_planner_source_kind() -> String {
    "planner".to_string()
}

fn workflow_planner_session_fork_source_kind(source_kind: &str) -> String {
    let normalized = source_kind.trim();
    let stripped = normalized.strip_prefix("forked_").unwrap_or(normalized);
    format!("forked_{}", stripped)
}

fn workflow_plan_import_title(goal: &str, fallback_digest: &str) -> String {
    let goal = goal.trim();
    if !goal.is_empty() {
        let clipped = if goal.chars().count() > 40 {
            let mut clipped = goal.chars().take(39).collect::<String>();
            clipped.push('…');
            clipped
        } else {
            goal.to_string()
        };
        return format!("Imported {clipped}");
    }
    let digest = fallback_digest.chars().take(12).collect::<String>();
    format!("Imported workflow {digest}")
}

fn workflow_plan_import_agent_role(kind: &str) -> String {
    match kind.trim().to_ascii_lowercase().as_str() {
        "research" => "researcher",
        "monitoring" => "watcher",
        "drafting" => "writer",
        "review" => "reviewer",
        "execution" => "worker",
        "sync" => "worker",
        "reporting" => "reporter",
        "publication" => "publisher",
        "remediation" => "repairer",
        "triage" => "triager",
        "orchestration" => "orchestrator",
        _ => "worker",
    }
    .to_string()
}

fn workflow_plan_import_step_id(routine_id: &str, step_id: &str) -> String {
    let routine_id = routine_id.trim();
    let step_id = step_id.trim();
    if step_id.contains("::") {
        step_id.to_string()
    } else {
        format!("{routine_id}::{step_id}")
    }
}

fn workflow_plan_import_output_contract(
    step_notes: Option<&String>,
) -> crate::AutomationFlowOutputContract {
    crate::AutomationFlowOutputContract {
        kind: "generic_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: step_notes
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    }
}

fn workflow_plan_import_input_refs(depends_on: &[String]) -> Vec<crate::AutomationFlowInputRef> {
    depends_on
        .iter()
        .map(|from_step_id| crate::AutomationFlowInputRef {
            from_step_id: from_step_id.clone(),
            alias: from_step_id
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>(),
        })
        .collect()
}

fn workflow_plan_import_steps(
    plan_package: &compiler_api::PlanPackage,
    source_bundle_digest: &str,
) -> Vec<crate::WorkflowPlanStep> {
    let mut steps = Vec::new();
    for routine in &plan_package.routine_graph {
        let routine_kind = serde_json::to_value(&routine.semantic_kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "mixed".to_string());
        if routine.steps.is_empty() {
            let step_id = workflow_plan_import_step_id(&routine.routine_id, "routine");
            let mut metadata = json!({
                "imported": true,
                "source_bundle_digest": source_bundle_digest,
                "source_plan_id": plan_package.plan_id,
                "source_routine_id": routine.routine_id,
                "source_routine_kind": routine_kind,
                "source_step_kind": "routine",
                "source_step_label": routine.routine_id,
            });
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "source_step_dependencies".to_string(),
                    serde_json::to_value(&routine.dependencies).unwrap_or(Value::Null),
                );
            }
            steps.push(crate::WorkflowPlanStep {
                step_id,
                kind: routine_kind.clone(),
                objective: format!("Continue imported routine `{}`.", routine.routine_id),
                depends_on: routine
                    .dependencies
                    .iter()
                    .map(|dep| workflow_plan_import_step_id(&dep.routine_id, "routine"))
                    .collect(),
                agent_role: workflow_plan_import_agent_role(&routine_kind),
                input_refs: workflow_plan_import_input_refs(&[]),
                output_contract: Some(workflow_plan_import_output_contract(None)),
                metadata: Some(metadata),
            });
            continue;
        }

        for step in &routine.steps {
            let step_id = workflow_plan_import_step_id(&routine.routine_id, &step.step_id);
            let depends_on = step
                .dependencies
                .iter()
                .map(|dependency| workflow_plan_import_step_id(&routine.routine_id, dependency))
                .collect::<Vec<_>>();
            let objective = step
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    let label = step.label.trim();
                    if label.is_empty() {
                        None
                    } else {
                        Some(label.to_string())
                    }
                })
                .unwrap_or_else(|| format!("Continue imported step `{}`.", step.step_id));
            let mut metadata = json!({
                "imported": true,
                "source_bundle_digest": source_bundle_digest,
                "source_plan_id": plan_package.plan_id,
                "source_routine_id": routine.routine_id,
                "source_routine_kind": routine_kind,
                "source_step_id": step.step_id,
                "source_step_kind": step.kind,
                "source_step_label": step.label,
                "source_step_action": step.action,
                "source_step_notes": step.notes,
            });
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "source_step_dependencies".to_string(),
                    serde_json::to_value(&step.dependencies).unwrap_or(Value::Null),
                );
                object.insert(
                    "source_step_outputs".to_string(),
                    serde_json::to_value(&step.outputs).unwrap_or(Value::Null),
                );
                object.insert(
                    "source_step_context_reads".to_string(),
                    serde_json::to_value(&step.context_reads).unwrap_or(Value::Null),
                );
                object.insert(
                    "source_step_context_writes".to_string(),
                    serde_json::to_value(&step.context_writes).unwrap_or(Value::Null),
                );
            }
            steps.push(crate::WorkflowPlanStep {
                step_id,
                kind: step.kind.clone(),
                objective,
                depends_on: depends_on.clone(),
                agent_role: workflow_plan_import_agent_role(&routine_kind),
                input_refs: workflow_plan_import_input_refs(&depends_on),
                output_contract: Some(workflow_plan_import_output_contract(step.notes.as_ref())),
                metadata: Some(metadata),
            });
        }
    }

    if steps.is_empty() {
        steps.push(crate::WorkflowPlanStep {
            step_id: "import_bundle".to_string(),
            kind: "import_bundle".to_string(),
            objective: format!(
                "Review imported workflow bundle `{}`.",
                source_bundle_digest.chars().take(12).collect::<String>()
            ),
            depends_on: Vec::new(),
            agent_role: "reviewer".to_string(),
            input_refs: Vec::new(),
            output_contract: Some(workflow_plan_import_output_contract(None)),
            metadata: Some(json!({
                "imported": true,
                "source_bundle_digest": source_bundle_digest,
                "source_plan_id": plan_package.plan_id,
                "source_step_kind": "bundle_summary",
            })),
        });
    }

    steps
}

fn workflow_plan_from_import_preview(
    preview: &compiler_api::PlanPackageImportPreview,
    workspace_root: &str,
) -> crate::WorkflowPlan {
    let mission_goal = preview.plan_package.mission.goal.clone();
    let source_bundle_digest = preview.source_bundle_digest.clone();
    let import_transform_log = preview.import_transform_log.clone();
    let steps = workflow_plan_import_steps(&preview.plan_package, &source_bundle_digest);
    let allowed_mcp_servers = preview
        .plan_package
        .connector_bindings
        .iter()
        .filter(|binding| {
            binding
                .binding_type
                .trim()
                .to_ascii_lowercase()
                .contains("mcp")
        })
        .map(|binding| binding.binding_id.clone())
        .collect::<Vec<_>>();
    let requires_integrations = preview
        .plan_package
        .connector_intents
        .iter()
        .map(|intent| intent.capability.clone())
        .collect::<Vec<_>>();
    let original_prompt = if mission_goal.trim().is_empty() {
        format!(
            "Imported workflow bundle {}",
            source_bundle_digest.chars().take(12).collect::<String>()
        )
    } else {
        mission_goal.clone()
    };
    crate::WorkflowPlan {
        plan_id: preview.plan_package.plan_id.clone(),
        planner_version: "workflow_plan_import_v1".to_string(),
        plan_source: "workflow_plan_import".to_string(),
        original_prompt: original_prompt.clone(),
        normalized_prompt: original_prompt.clone(),
        confidence: "imported".to_string(),
        title: workflow_plan_import_title(&mission_goal, &source_bundle_digest),
        description: preview.plan_package.mission.summary.clone(),
        schedule: compiler_api::manual_schedule(
            "UTC".to_string(),
            crate::RoutineMisfirePolicy::RunOnce,
        ),
        execution_target: "automation_v2".to_string(),
        workspace_root: workspace_root.to_string(),
        steps,
        requires_integrations,
        allowed_mcp_servers,
        operator_preferences: Some(json!({
            "source_kind": "imported_bundle",
            "source_bundle_digest": source_bundle_digest,
            "source_plan_id": preview.plan_package.plan_id,
        })),
        save_options: json!({
            "origin": "workflow_plan_import",
            "source_kind": "imported_bundle",
            "source_bundle_digest": source_bundle_digest.clone(),
            "source_plan_id": preview.plan_package.plan_id.clone(),
            "import_transform_log": import_transform_log.clone(),
        }),
    }
}

fn workflow_plan_import_draft(
    preview: &compiler_api::PlanPackageImportPreview,
    workspace_root: &str,
) -> crate::WorkflowPlanDraftRecord {
    let plan = workflow_plan_from_import_preview(preview, workspace_root);
    let now = crate::now_ms();
    let source_bundle_digest = preview.source_bundle_digest.clone();
    let import_transform_log = preview.import_transform_log.clone();
    let derived_scope_snapshot = preview.derived_scope_snapshot.clone();
    let conversation = crate::WorkflowPlanConversation {
        conversation_id: format!("wfchat-{}", Uuid::new_v4()),
        plan_id: plan.plan_id.clone(),
        created_at_ms: now,
        updated_at_ms: now,
        messages: vec![crate::WorkflowPlanChatMessage {
            role: "system".to_string(),
            text: format!(
                "Imported workflow bundle `{}` from plan `{}`. Review and revise before applying.",
                source_bundle_digest, preview.plan_package.plan_id
            ),
            created_at_ms: now,
        }],
    };
    crate::WorkflowPlanDraftRecord {
        initial_plan: plan.clone(),
        current_plan: plan,
        plan_revision: 1,
        conversation,
        planner_diagnostics: Some(json!({
            "source_kind": "imported_bundle",
            "source_bundle_digest": source_bundle_digest.clone(),
            "import_transform_log": import_transform_log.clone(),
            "derived_scope_snapshot": derived_scope_snapshot.clone(),
        })),
        last_success_materialization: None,
    }
}

async fn compile_preview_plan_overlap(
    state: &AppState,
    plan_package: &compiler_api::PlanPackage,
) -> compiler_api::OverlapComparison {
    let prior_plans = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter_map(|automation| {
            automation
                .metadata
                .as_ref()
                .and_then(|metadata| {
                    metadata
                        .get("plan_package")
                        .or_else(|| metadata.get("planPackage"))
                })
                .cloned()
        })
        .filter_map(|value| serde_json::from_value::<compiler_api::PlanPackage>(value).ok())
        .filter(|prior| prior.plan_id != plan_package.plan_id)
        .collect::<Vec<_>>();
    compiler_api::analyze_plan_overlap(plan_package, &prior_plans)
}

fn parse_overlap_decision(
    decision: Option<&str>,
) -> Result<Option<compiler_api::OverlapDecision>, String> {
    let Some(decision) = decision.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match decision.to_ascii_lowercase().as_str() {
        "reuse" => Ok(Some(compiler_api::OverlapDecision::Reuse)),
        "merge" => Ok(Some(compiler_api::OverlapDecision::Merge)),
        "fork" => Ok(Some(compiler_api::OverlapDecision::Fork)),
        "new" => Ok(Some(compiler_api::OverlapDecision::New)),
        _ => Err(format!("unsupported overlap decision `{decision}`")),
    }
}

fn teaching_library_summary() -> Value {
    serde_json::to_value(compiler_api::planner_teaching_library_summary())
        .unwrap_or_else(|_| json!({}))
}

fn planner_session_default_title(goal: &str, fallback_time_ms: u64) -> String {
    let goal = goal.trim();
    if !goal.is_empty() {
        return if goal.chars().count() > 48 {
            let mut clipped = goal.chars().take(47).collect::<String>();
            clipped.push('…');
            clipped
        } else {
            goal.to_string()
        };
    }
    format!(
        "Plan {}",
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(fallback_time_ms as i64)
            .map(|value| value.format("%-H:%M:%S").to_string())
            .unwrap_or_else(|| "now".to_string())
    )
}

fn workflow_planner_session_list_item(
    session: &WorkflowPlannerSessionRecord,
) -> WorkflowPlannerSessionListItem {
    WorkflowPlannerSessionListItem {
        session_id: session.session_id.clone(),
        title: session.title.clone(),
        project_slug: session.project_slug.clone(),
        workspace_root: session.workspace_root.clone(),
        source_kind: session.source_kind.clone(),
        source_bundle_digest: session.source_bundle_digest.clone(),
        current_plan_id: session.current_plan_id.clone(),
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        goal: if session.goal.trim().is_empty() {
            None
        } else {
            Some(session.goal.clone())
        },
        planner_provider: if session.planner_provider.trim().is_empty() {
            None
        } else {
            Some(session.planner_provider.clone())
        },
        planner_model: if session.planner_model.trim().is_empty() {
            None
        } else {
            Some(session.planner_model.clone())
        },
    }
}

fn retag_workflow_plan_draft(
    draft: &crate::WorkflowPlanDraftRecord,
    new_plan_id: &str,
) -> Result<crate::WorkflowPlanDraftRecord, String> {
    let mut value = serde_json::to_value(draft).map_err(|error| error.to_string())?;
    if let Some(object) = value.as_object_mut() {
        if let Some(plan) = object
            .get_mut("current_plan")
            .and_then(Value::as_object_mut)
        {
            plan.insert(
                "plan_id".to_string(),
                Value::String(new_plan_id.to_string()),
            );
        }
        if let Some(plan) = object
            .get_mut("initial_plan")
            .and_then(Value::as_object_mut)
        {
            plan.insert(
                "plan_id".to_string(),
                Value::String(new_plan_id.to_string()),
            );
        }
        if let Some(conversation) = object
            .get_mut("conversation")
            .and_then(Value::as_object_mut)
        {
            conversation.insert(
                "plan_id".to_string(),
                Value::String(new_plan_id.to_string()),
            );
            if let Some(conversation_id) = conversation.get_mut("conversation_id") {
                *conversation_id = Value::String(format!("wfchat-{}", Uuid::new_v4()));
            }
            if let Some(messages) = conversation
                .get_mut("messages")
                .and_then(Value::as_array_mut)
            {
                for message in messages {
                    if let Some(message_obj) = message.as_object_mut() {
                        if let Some(role) = message_obj.get("role").and_then(Value::as_str) {
                            if role.is_empty() {
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

pub(super) async fn workflow_plan_preview(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPreviewRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "prompt is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let build = workflow_planner_host::build_workflow_plan(
        &state,
        prompt,
        input.schedule.as_ref(),
        input.plan_source.as_deref().unwrap_or("unknown"),
        input.allowed_mcp_servers,
        input.workspace_root.as_deref(),
        input.operator_preferences,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan = build.plan;
    let planner_diagnostics = build.planner_diagnostics.clone();
    let teaching_library = teaching_library_summary();
    let plan_json = compiler_api::workflow_plan_to_json(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        1,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let host = workflow_planner_host::WorkflowPlannerHost { state: &state };
    compiler_api::store_preview_draft::<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
        _,
    >(&host, plan.clone(), planner_diagnostics.clone())
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("{error:?}"),
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    Ok(Json(json!({
        "plan": plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "plan_package_validation": plan_package_validation,
        "overlap_analysis": overlap_analysis,
        "teaching_library": teaching_library,
        "clarifier": build.clarifier,
        "planner_diagnostics": planner_diagnostics,
        "assistant_message": build.assistant_text.map(|text| json!({
            "role": "assistant",
            "text": text,
        })),
    })))
}

pub(super) async fn workflow_plan_chat_start(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatStartRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "prompt is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let build = workflow_planner_host::build_workflow_plan(
        &state,
        prompt,
        input.schedule.as_ref(),
        input.plan_source.as_deref().unwrap_or("unknown"),
        input.allowed_mcp_servers,
        input.workspace_root.as_deref(),
        input.operator_preferences,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan = build.plan;
    let host = workflow_planner_host::WorkflowPlannerHost { state: &state };
    let draft = compiler_api::store_chat_start_draft::<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
        _,
    >(
        &host,
        plan.clone(),
        build.planner_diagnostics.clone(),
        build.assistant_text.clone(),
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("{error:?}"),
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan_json = compiler_api::workflow_plan_to_json(&draft.current_plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        draft.plan_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let teaching_library = teaching_library_summary();
    Ok(Json(json!({
        "plan": draft.current_plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "plan_package_validation": plan_package_validation,
        "overlap_analysis": overlap_analysis,
        "teaching_library": teaching_library,
        "conversation": draft.conversation,
        "planner_diagnostics": draft.planner_diagnostics,
        "clarifier": build.clarifier,
        "assistant_message": build.assistant_text.map(|text| json!({
            "role": "assistant",
            "text": text,
        })),
    })))
}

pub(super) async fn workflow_plan_get(
    State(state): State<AppState>,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let host = workflow_planner_host::WorkflowPlannerHost { state: &state };
    let draft = compiler_api::load_workflow_plan_draft::<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
        _,
    >(&host, &plan_id)
    .await
    .map_err(|error| match error {
        compiler_api::PlannerDraftError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(compiler_api::draft_not_found_response(&plan_id)),
        ),
        compiler_api::PlannerDraftError::InvalidState(error)
        | compiler_api::PlannerDraftError::Store(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ),
    })?;
    let plan_json = compiler_api::workflow_plan_to_json(&draft.current_plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        draft.plan_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    let initial_plan_json =
        compiler_api::workflow_plan_to_json(&draft.initial_plan).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    let plan_package_replay = compiler_api::compare_workflow_plan_preview_replay_with_revision(
        &plan_json,
        draft.plan_revision,
        &initial_plan_json,
        1,
    );
    let teaching_library = teaching_library_summary();
    Ok(Json(json!({
        "plan": draft.current_plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "plan_package_validation": plan_package_validation,
        "overlap_analysis": overlap_analysis,
        "plan_package_replay": plan_package_replay,
        "teaching_library": teaching_library,
        "conversation": draft.conversation,
        "planner_diagnostics": draft.planner_diagnostics,
    })))
}

pub(super) async fn workflow_plan_chat_message(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatMessageRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input.plan_id.trim();
    let message = input.message.trim();
    if plan_id.is_empty() || message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan_id and message are required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    let host = workflow_planner_host::WorkflowPlannerHost { state: &state };
    let mut revision = compiler_api::revise_workflow_plan_draft::<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
        _,
    >(
        &host,
        plan_id,
        message,
        compiler_api::PlannerLoopConfig {
            session_title: "Workflow Planner Revision".to_string(),
            timeout_ms: super::workflow_planner_policy::planner_revision_timeout_ms(),
            override_env: "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE".to_string(),
        },
        workflow_planner_host::normalize_workflow_step_metadata,
    )
    .await
    .map_err(|error| match error {
        compiler_api::PlannerDraftError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(compiler_api::draft_not_found_response(plan_id)),
        ),
        compiler_api::PlannerDraftError::InvalidState(error)
        | compiler_api::PlannerDraftError::Store(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ),
    })?;
    workflow_planner_host::normalize_workflow_plan_file_contracts(&mut revision.draft.current_plan);
    let plan_json =
        compiler_api::workflow_plan_to_json(&revision.draft.current_plan).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        revision.draft.plan_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let initial_plan_json = compiler_api::workflow_plan_to_json(&revision.draft.initial_plan)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    let plan_package_replay = compiler_api::compare_workflow_plan_preview_replay_with_revision(
        &plan_json,
        revision.draft.plan_revision,
        &initial_plan_json,
        1,
    );
    let teaching_library = teaching_library_summary();
    Ok(Json(json!({
        "plan": revision.draft.current_plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "plan_package_validation": plan_package_validation,
        "overlap_analysis": overlap_analysis,
        "plan_package_replay": plan_package_replay,
        "teaching_library": teaching_library,
        "conversation": revision.draft.conversation,
        "assistant_message": {
            "role": "assistant",
            "text": revision.assistant_text,
        },
        "change_summary": revision.change_summary,
        "clarifier": revision.clarifier,
        "planner_diagnostics": revision.draft.planner_diagnostics,
    })))
}

pub(super) async fn workflow_plan_chat_reset(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatResetRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input.plan_id.trim();
    if plan_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan_id is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    let host = workflow_planner_host::WorkflowPlannerHost { state: &state };
    let draft = compiler_api::reset_workflow_plan_draft::<
        crate::routines::types::RoutineMisfirePolicy,
        crate::AutomationFlowInputRef,
        crate::AutomationFlowOutputContract,
        _,
    >(&host, plan_id)
    .await
    .map_err(|error| match error {
        compiler_api::PlannerDraftError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(compiler_api::draft_not_found_response(plan_id)),
        ),
        compiler_api::PlannerDraftError::InvalidState(error)
        | compiler_api::PlannerDraftError::Store(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ),
    })?;
    let plan_json = compiler_api::workflow_plan_to_json(&draft.current_plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        draft.plan_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    let initial_plan_json =
        compiler_api::workflow_plan_to_json(&draft.initial_plan).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    let plan_package_replay = compiler_api::compare_workflow_plan_preview_replay_with_revision(
        &plan_json,
        draft.plan_revision,
        &initial_plan_json,
        1,
    );
    let teaching_library = teaching_library_summary();
    Ok(Json(json!({
        "plan": draft.current_plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "plan_package_validation": plan_package_validation,
        "overlap_analysis": overlap_analysis,
        "plan_package_replay": plan_package_replay,
        "teaching_library": teaching_library,
        "conversation": draft.conversation,
        "planner_diagnostics": draft.planner_diagnostics,
    })))
}

fn planner_session_title_from_record(session: &WorkflowPlannerSessionRecord) -> String {
    let title = session.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    planner_session_default_title(&session.goal, session.created_at_ms)
}

async fn workflow_planner_session_response(
    state: &AppState,
    session: &WorkflowPlannerSessionRecord,
    response: Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan = response.0.get("plan").cloned().unwrap_or(Value::Null);
    let conversation = response
        .0
        .get("conversation")
        .cloned()
        .unwrap_or(Value::Null);
    let planner_diagnostics = response
        .0
        .get("planner_diagnostics")
        .or_else(|| response.0.get("plannerDiagnostics"))
        .cloned()
        .unwrap_or(Value::Null);
    let change_summary = response
        .0
        .get("change_summary")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let clarifier = response
        .0
        .get("clarifier")
        .cloned()
        .unwrap_or_else(|| json!({"status":"none"}));
    let draft = state
        .get_workflow_plan_draft(session.current_plan_id.as_deref().unwrap_or_default())
        .await;
    let mut next_session = session.clone();
    if let Some(draft) = draft {
        next_session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        next_session.draft = Some(draft.clone());
        next_session.updated_at_ms = crate::now_ms();
        if next_session.title.trim().is_empty()
            || next_session.title.starts_with("Plan ")
            || next_session.title.starts_with("Untitled")
        {
            next_session.title = draft.current_plan.title.clone();
        }
        if next_session.title.trim().is_empty() {
            next_session.title = planner_session_title_from_record(&next_session);
        }
        let _ = state
            .put_workflow_planner_session(next_session.clone())
            .await;
    }
    Ok(Json(json!({
        "session": next_session,
        "plan": plan,
        "conversation": conversation,
        "change_summary": change_summary,
        "planner_diagnostics": planner_diagnostics,
        "clarifier": clarifier,
    })))
}

pub(super) async fn workflow_planner_session_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowPlannerSessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sessions = state
        .list_workflow_planner_sessions(query.project_slug.as_deref())
        .await;
    let items = sessions
        .iter()
        .map(workflow_planner_session_list_item)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "sessions": items,
        "count": items.len(),
    })))
}

pub(super) async fn workflow_planner_session_create(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlannerSessionCreateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_slug = input.project_slug.trim();
    if project_slug.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "project_slug is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let now = crate::now_ms();
    let session = WorkflowPlannerSessionRecord {
        session_id: format!("wfplan-session-{}", Uuid::new_v4()),
        project_slug: project_slug.to_string(),
        title: input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                planner_session_default_title(input.goal.as_deref().unwrap_or(""), now)
            }),
        workspace_root: input
            .workspace_root
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string(),
        source_kind: default_workflow_planner_source_kind(),
        source_bundle_digest: None,
        current_plan_id: None,
        draft: None,
        goal: input.goal.unwrap_or_default(),
        notes: input.notes.unwrap_or_default(),
        planner_provider: input.planner_provider.unwrap_or_default(),
        planner_model: input.planner_model.unwrap_or_default(),
        plan_source: input
            .plan_source
            .unwrap_or_else(|| "coding_task_planning".to_string()),
        allowed_mcp_servers: input.allowed_mcp_servers,
        operator_preferences: input.operator_preferences,
        import_validation: None,
        import_transform_log: Vec::new(),
        import_scope_snapshot: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let mut session = session;
    if let Some(plan) = input.plan {
        let conversation = input
            .conversation
            .unwrap_or_else(|| crate::WorkflowPlanConversation {
                conversation_id: format!("wfchat-{}", Uuid::new_v4()),
                plan_id: plan.plan_id.clone(),
                created_at_ms: now,
                updated_at_ms: now,
                messages: Vec::new(),
            });
        let draft = crate::WorkflowPlanDraftRecord {
            initial_plan: plan.clone(),
            current_plan: plan,
            plan_revision: input.plan_revision.unwrap_or(1),
            conversation,
            planner_diagnostics: input.planner_diagnostics,
            last_success_materialization: input.last_success_materialization,
        };
        session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        session.draft = Some(draft);
    }
    let stored = state
        .put_workflow_planner_session(session.clone())
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_get(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    Ok(Json(json!({
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_patch(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionPatchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if let Some(title) = input.title.as_deref() {
        let title = title.trim();
        if title.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "title cannot be empty",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
        session.title = title.to_string();
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
        session.workspace_root = workspace_root.trim().to_string();
    }
    if let Some(goal) = input.goal {
        session.goal = goal;
    }
    if let Some(notes) = input.notes {
        session.notes = notes;
    }
    if let Some(provider) = input.planner_provider {
        session.planner_provider = provider;
    }
    if let Some(model) = input.planner_model {
        session.planner_model = model;
    }
    if let Some(plan_source) = input.plan_source {
        session.plan_source = plan_source;
    }
    if let Some(allowed) = input.allowed_mcp_servers {
        session.allowed_mcp_servers = allowed;
    }
    if let Some(preferences) = input.operator_preferences {
        session.operator_preferences = Some(preferences);
    }
    if let Some(current_plan_id) = input.current_plan_id {
        let current_plan_id = current_plan_id.trim();
        session.current_plan_id = if current_plan_id.is_empty() {
            None
        } else {
            Some(current_plan_id.to_string())
        };
    }
    if let Some(draft) = input.draft {
        session.current_plan_id = Some(draft.current_plan.plan_id.clone());
        session.draft = Some(draft);
    }
    if let Some(published_at_ms) = input.published_at_ms {
        session.published_at_ms = Some(published_at_ms);
    }
    if let Some(published_tasks) = input.published_tasks {
        session.published_tasks = published_tasks;
    }
    let stored = state
        .put_workflow_planner_session(session)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_delete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(session) = state.delete_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    Ok(Json(json!({
        "ok": true,
        "session": session,
    })))
}

pub(super) async fn workflow_planner_session_duplicate(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionDuplicateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(source) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    let now = crate::now_ms();
    let mut next = source.clone();
    next.session_id = format!("wfplan-session-{}", Uuid::new_v4());
    next.title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Copy of {}", source.title));
    next.source_kind = workflow_planner_session_fork_source_kind(&source.source_kind);
    next.created_at_ms = now;
    next.updated_at_ms = now;
    if let Some(draft) = source.draft.as_ref() {
        let new_plan_id = format!("wfplan-{}", Uuid::new_v4());
        let duplicated = retag_workflow_plan_draft(draft, &new_plan_id).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
        next.current_plan_id = Some(new_plan_id);
        next.draft = Some(duplicated);
    }
    let stored = state
        .put_workflow_planner_session(next)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "session": stored,
    })))
}

pub(super) async fn workflow_planner_session_start(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionStartRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    let chat_start = WorkflowPlanChatStartRequest {
        prompt: input.prompt,
        schedule: input.schedule,
        plan_source: input
            .plan_source
            .or_else(|| Some(session.plan_source.clone())),
        allowed_mcp_servers: if input.allowed_mcp_servers.is_empty() {
            session.allowed_mcp_servers.clone()
        } else {
            input.allowed_mcp_servers
        },
        workspace_root: input
            .workspace_root
            .or_else(|| Some(session.workspace_root.clone())),
        operator_preferences: input
            .operator_preferences
            .or(session.operator_preferences.clone()),
    };
    let response = workflow_plan_chat_start(State(state.clone()), Json(chat_start)).await?;
    if let Some(plan) = response.0.get("plan").cloned() {
        if let Ok(plan) = serde_json::from_value::<crate::WorkflowPlan>(plan) {
            session.current_plan_id = Some(plan.plan_id.clone());
            session.goal = response
                .0
                .get("plan")
                .and_then(|plan| plan.get("title"))
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| session.goal.clone());
            session.title = if session.title.trim().is_empty()
                || session.title.starts_with("Plan ")
                || session.title.starts_with("Untitled")
            {
                plan.title.clone()
            } else {
                session.title.clone()
            };
        }
    }
    if let Some(plan_id) = response
        .0
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
    {
        if let Some(draft) = state.get_workflow_plan_draft(plan_id).await {
            session.current_plan_id = Some(plan_id.to_string());
            session.draft = Some(draft.clone());
            session.updated_at_ms = crate::now_ms();
            let _ = state.put_workflow_planner_session(session.clone()).await;
            return workflow_planner_session_response(&state, &session, response).await;
        }
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_planner_session_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(input): Json<WorkflowPlannerSessionMessageRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if session.current_plan_id.is_none() {
        if let Some(draft) = session.draft.clone() {
            session.current_plan_id = Some(draft.current_plan.plan_id.clone());
            let _ = state.put_workflow_plan_draft(draft).await;
        }
    }
    let Some(plan_id) = session.current_plan_id.clone() else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session has no active plan",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if state.get_workflow_plan_draft(&plan_id).await.is_none() {
        if let Some(draft) = session.draft.clone() {
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let response = workflow_plan_chat_message(
        State(state.clone()),
        Json(WorkflowPlanChatMessageRequest {
            plan_id: plan_id.clone(),
            message: input.message,
        }),
    )
    .await?;
    if let Some(draft) = state.get_workflow_plan_draft(&plan_id).await {
        session.draft = Some(draft.clone());
        session.updated_at_ms = crate::now_ms();
        if session.title.trim().is_empty()
            || session.title.starts_with("Plan ")
            || session.title.starts_with("Untitled")
        {
            session.title = draft.current_plan.title.clone();
        }
        let _ = state.put_workflow_planner_session(session.clone()).await;
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_planner_session_reset(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut session) = state.get_workflow_planner_session(&session_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session not found",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if session.current_plan_id.is_none() {
        if let Some(draft) = session.draft.clone() {
            session.current_plan_id = Some(draft.current_plan.plan_id.clone());
            let _ = state.put_workflow_plan_draft(draft).await;
        }
    }
    let Some(plan_id) = session.current_plan_id.clone() else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "planner session has no active plan",
                "code": "WORKFLOW_PLAN_SESSION_NOT_FOUND",
                "session_id": session_id,
            })),
        ));
    };
    if state.get_workflow_plan_draft(&plan_id).await.is_none() {
        if let Some(draft) = session.draft.clone() {
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let response = workflow_plan_chat_reset(
        State(state.clone()),
        Json(WorkflowPlanChatResetRequest {
            plan_id: plan_id.clone(),
        }),
    )
    .await?;
    if let Some(draft) = state.get_workflow_plan_draft(&plan_id).await {
        session.draft = Some(draft.clone());
        session.updated_at_ms = crate::now_ms();
        if session.title.trim().is_empty()
            || session.title.starts_with("Plan ")
            || session.title.starts_with("Untitled")
        {
            session.title = draft.current_plan.title.clone();
        }
        let _ = state.put_workflow_planner_session(session.clone()).await;
    }
    workflow_planner_session_response(&state, &session, response).await
}

pub(super) async fn workflow_plan_apply(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input
        .plan_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let plan = match (input.plan, plan_id.as_deref()) {
        (Some(plan), _) => plan,
        (None, Some(plan_id)) => state.get_workflow_plan(plan_id).await.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "workflow plan not found",
                    "code": "WORKFLOW_PLAN_NOT_FOUND",
                    "plan_id": plan_id,
                })),
            )
        })?,
        (None, None) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "plan or plan_id is required",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
    };
    compiler_api::validate_workflow_plan(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let draft_context = if let Some(plan_id) = plan_id.as_deref() {
        state.get_workflow_plan_draft(plan_id).await
    } else {
        None
    };
    let apply_revision = draft_context
        .as_ref()
        .map(|draft| draft.plan_revision)
        .unwrap_or(1);
    let planner_diagnostics = draft_context
        .as_ref()
        .and_then(|draft| draft.planner_diagnostics.clone());
    let plan_json = compiler_api::workflow_plan_to_json(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let mut plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        apply_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    let mut overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    if plan_package_validation.blocker_count > 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan package validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
            })),
        ));
    }
    let requested_overlap_decision = parse_overlap_decision(input.overlap_decision.as_deref())
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    if overlap_analysis.requires_user_confirmation && requested_overlap_decision.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "overlap confirmation is required before apply",
                "code": "WORKFLOW_PLAN_OVERLAP_CONFIRMATION_REQUIRED",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
                "overlap_analysis": overlap_analysis,
            })),
        ));
    }
    if overlap_analysis.matched_plan_id.is_none() && requested_overlap_decision.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "overlap_decision was provided but no prior overlap was detected",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(decision) = requested_overlap_decision {
        overlap_analysis.decision = decision;
        overlap_analysis.requires_user_confirmation = false;
    }
    if let Some(entry) = compiler_api::overlap_log_entry_from_analysis(
        &overlap_analysis,
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
        &chrono::Utc::now().to_rfc3339(),
    ) {
        plan_package
            .overlap_policy
            .get_or_insert_with(Default::default)
            .overlap_log
            .push(entry);
    }

    let mut automation = compile_plan_to_automation_v2(
        &plan,
        Some(&plan_package),
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
    );
    let approved_plan_materialization = compiler_api::approved_plan_materialization(&plan_package);
    let approved_plan_success_memory =
        compiler_api::approved_plan_success_memory_value(&plan_package);
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
        metadata.insert(
            "plan_source".to_string(),
            serde_json::to_value(&plan.plan_source).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package".to_string(),
            serde_json::to_value(&plan_package).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_bundle".to_string(),
            serde_json::to_value(&plan_package_bundle).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_validation".to_string(),
            serde_json::to_value(&plan_package_validation).unwrap_or(Value::Null),
        );
        metadata.insert(
            "overlap_analysis".to_string(),
            serde_json::to_value(&overlap_analysis).unwrap_or(Value::Null),
        );
        metadata.insert(
            "approved_plan_materialization".to_string(),
            approved_plan_success_memory.clone(),
        );
        metadata.insert(
            "planner_diagnostics".to_string(),
            planner_diagnostics.clone().unwrap_or(Value::Null),
        );
    } else {
        automation.metadata = Some(json!({
            "plan_package": plan_package,
            "plan_package_bundle": plan_package_bundle.clone(),
            "plan_package_validation": plan_package_validation,
            "overlap_analysis": overlap_analysis,
            "approved_plan_materialization": approved_plan_success_memory.clone(),
            "planner_diagnostics": planner_diagnostics,
        }));
    }
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "WORKFLOW_PLAN_APPLY_FAILED",
            })),
        )
    })?;
    if let Some(plan_id) = plan_id.as_deref() {
        if let Some(mut draft) = state.get_workflow_plan_draft(plan_id).await {
            draft.last_success_materialization = Some(approved_plan_success_memory);
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let pack_builder_export = match input.pack_builder_export {
        Some(export) if export.enabled.unwrap_or(true) => {
            Some(export_workflow_plan_to_pack_builder(&state, &plan, &export).await)
        }
        _ => None,
    };
    Ok(Json(json!({
        "ok": true,
        "plan": plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "overlap_analysis": overlap_analysis,
        "approved_plan_materialization": approved_plan_materialization,
        "automation": stored,
        "pack_builder_export": pack_builder_export,
    })))
}

async fn workflow_plan_import_inner(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
    persist: bool,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let report = compiler_api::validate_plan_package_bundle(&input.bundle);
    if !report.compatible {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan bundle import validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "bundle": input.bundle,
                "import_validation": report,
            })),
        ));
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    let import_preview = compiler_api::preview_plan_package_import_bundle(
        &input.bundle,
        &workspace_root,
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
    );
    let plan_package_validation = compiler_api::validate_plan_package(&import_preview.plan_package);
    let plan_package_preview = import_preview.plan_package.clone();
    let source_bundle_digest = import_preview.source_bundle_digest.clone();
    let derived_scope_snapshot = import_preview.derived_scope_snapshot.clone();
    let import_transform_log = import_preview.import_transform_log.clone();
    let source_plan_id = import_preview.plan_package.plan_id.clone();
    let imported_goal = import_preview.plan_package.mission.goal.clone();
    let summary = workflow_plan_import_summary(&plan_package_preview);
    if !persist {
        return Ok(Json(json!({
            "ok": true,
            "persisted": false,
            "bundle": input.bundle,
            "import_validation": report,
            "plan_package_preview": plan_package_preview,
            "plan_package_validation": plan_package_validation,
            "derived_scope_snapshot": derived_scope_snapshot,
            "summary": summary,
            "import_transform_log": import_transform_log,
            "import_source_bundle_digest": source_bundle_digest,
        })));
    }

    let project_slug = input
        .project_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("workflow-imports")
        .to_string();
    let draft = workflow_plan_import_draft(&import_preview, &workspace_root);
    let session = WorkflowPlannerSessionRecord {
        session_id: format!("wfplan-session-{}", Uuid::new_v4()),
        project_slug,
        title: input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| workflow_plan_import_title(&imported_goal, &source_bundle_digest)),
        workspace_root,
        source_kind: "imported_bundle".to_string(),
        source_bundle_digest: Some(source_bundle_digest.clone()),
        current_plan_id: Some(draft.current_plan.plan_id.clone()),
        draft: Some(draft),
        goal: imported_goal.clone(),
        notes: import_transform_log.join("\n"),
        planner_provider: String::new(),
        planner_model: String::new(),
        plan_source: "workflow_plan_import".to_string(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "source_kind": "imported_bundle",
            "source_bundle_digest": source_bundle_digest.clone(),
            "source_plan_id": source_plan_id.clone(),
        })),
        import_validation: Some(report.clone()),
        import_transform_log: import_transform_log.clone(),
        import_scope_snapshot: Some(derived_scope_snapshot.clone()),
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
    };
    let stored = state
        .put_workflow_planner_session(session)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "persisted": true,
        "bundle": input.bundle,
        "import_validation": report,
        "plan_package_preview": plan_package_preview,
        "plan_package_validation": plan_package_validation,
        "derived_scope_snapshot": derived_scope_snapshot,
        "summary": summary,
        "import_transform_log": import_transform_log,
        "import_source_bundle_digest": source_bundle_digest,
        "session": stored,
    })))
}

pub(super) async fn workflow_plan_import(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_inner(State(state), Json(input), true).await
}

pub(super) async fn workflow_plan_import_preview(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    workflow_plan_import_inner(State(state), Json(input), false).await
}

async fn export_workflow_plan_to_pack_builder(
    state: &AppState,
    plan: &crate::WorkflowPlan,
    export: &WorkflowPlanPackBuilderExportRequest,
) -> Value {
    let args = compiler_api::pack_builder_export_args(
        plan,
        &compiler_api::PackBuilderExportOptions {
            session_id: export.session_id.clone(),
            thread_key: export.thread_key.clone(),
            auto_apply: export.auto_apply.unwrap_or(false),
        },
    );
    match super::pack_builder::run_pack_builder_tool(state, args).await {
        Ok(payload) => payload,
        Err(code) => json!({
            "status": "export_failed",
            "error": "pack_builder_export_failed",
            "http_status": code.as_u16(),
        }),
    }
}
