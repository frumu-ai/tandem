use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    apply_optimization_execution_override, freeze_optimization_artifact,
    load_optimization_phase1_config, now_ms, optimization_snapshot_hash,
    validate_phase1_workflow_target, AppState, OptimizationArtifactRefs,
    OptimizationCampaignRecord, OptimizationCampaignStatus, OptimizationExecutionOverride,
    OptimizationFrozenArtifacts, OptimizationTargetKind,
};

use super::{ErrorCode, ErrorEnvelope};

#[derive(Debug, Deserialize)]
pub(super) struct OptimizationCreateInput {
    #[serde(default)]
    pub optimization_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub source_workflow_id: String,
    pub artifacts: OptimizationArtifactRefs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_override: Option<OptimizationExecutionOverride>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OptimizationActionInput {
    pub action: String,
    #[serde(default)]
    pub experiment_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

fn optimization_error(
    status: StatusCode,
    error: impl Into<String>,
) -> (StatusCode, Json<ErrorEnvelope>) {
    let code = match status {
        StatusCode::BAD_REQUEST => ErrorCode::OptimizationValidationFailed,
        StatusCode::NOT_FOUND => ErrorCode::OptimizationNotFound,
        StatusCode::CONFLICT => ErrorCode::OptimizationConflict,
        StatusCode::INTERNAL_SERVER_ERROR => ErrorCode::InternalError,
        _ => ErrorCode::OptimizationValidationFailed,
    };
    (status, Json(ErrorEnvelope::new(error.into(), code)))
}

async fn optimization_payload(state: &AppState, campaign: &OptimizationCampaignRecord) -> Value {
    json!({
        "optimization": campaign,
        "experimentCount": state.count_optimization_experiments(&campaign.optimization_id).await,
    })
}

pub(super) async fn optimizations_list(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let optimizations = state.list_optimization_campaigns().await;
    Ok(Json(json!({
        "optimizations": optimizations,
        "count": optimizations.len(),
    })))
}

pub(super) async fn optimizations_create(
    State(state): State<AppState>,
    Json(input): Json<OptimizationCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let source_workflow_id = input.source_workflow_id.trim();
    if source_workflow_id.is_empty() {
        return Err(optimization_error(
            StatusCode::BAD_REQUEST,
            "source_workflow_id is required",
        ));
    }
    let Some(source_workflow) = state.get_automation_v2(source_workflow_id).await else {
        return Err(optimization_error(
            StatusCode::NOT_FOUND,
            "source workflow not found",
        ));
    };
    let workspace_root = source_workflow
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            optimization_error(
                StatusCode::BAD_REQUEST,
                "source workflow must declare an absolute workspace_root",
            )
        })?;
    if input.artifacts.objective_ref.trim().is_empty()
        || input.artifacts.eval_ref.trim().is_empty()
        || input.artifacts.mutation_policy_ref.trim().is_empty()
        || input.artifacts.scope_ref.trim().is_empty()
        || input.artifacts.budget_ref.trim().is_empty()
    {
        return Err(optimization_error(
            StatusCode::BAD_REQUEST,
            "all required optimization artifact refs must be provided",
        ));
    }
    let frozen_artifacts = OptimizationFrozenArtifacts {
        objective: freeze_optimization_artifact(workspace_root, &input.artifacts.objective_ref)
            .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?,
        eval: freeze_optimization_artifact(workspace_root, &input.artifacts.eval_ref)
            .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?,
        mutation_policy: freeze_optimization_artifact(
            workspace_root,
            &input.artifacts.mutation_policy_ref,
        )
        .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?,
        scope: freeze_optimization_artifact(workspace_root, &input.artifacts.scope_ref)
            .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?,
        budget: freeze_optimization_artifact(workspace_root, &input.artifacts.budget_ref)
            .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?,
    };
    let phase1 = load_optimization_phase1_config(&frozen_artifacts)
        .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?;
    validate_phase1_workflow_target(&source_workflow, &phase1)
        .map_err(|error| optimization_error(StatusCode::BAD_REQUEST, error))?;
    let execution_override =
        input
            .execution_override
            .as_ref()
            .map(|value| OptimizationExecutionOverride {
                provider_id: value.provider_id.trim().to_string(),
                model_id: value.model_id.trim().to_string(),
            });
    if execution_override
        .as_ref()
        .is_some_and(|value| value.provider_id.is_empty() || value.model_id.is_empty())
    {
        return Err(optimization_error(
            StatusCode::BAD_REQUEST,
            "execution_override requires both provider_id and model_id",
        ));
    }
    let baseline_snapshot = execution_override
        .as_ref()
        .map(|value| apply_optimization_execution_override(&source_workflow, value))
        .unwrap_or_else(|| source_workflow.clone());
    let optimization_id = input
        .optimization_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("opt-{}", Uuid::new_v4()));
    let source_hash = optimization_snapshot_hash(&source_workflow);
    let baseline_hash = optimization_snapshot_hash(&baseline_snapshot);
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Optimize {}", source_workflow.name));
    let campaign = OptimizationCampaignRecord {
        optimization_id,
        name,
        target_kind: OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
        status: OptimizationCampaignStatus::Draft,
        source_workflow_id: source_workflow.automation_id.clone(),
        source_workflow_name: source_workflow.name.clone(),
        source_workflow_snapshot: source_workflow.clone(),
        source_workflow_snapshot_hash: source_hash.clone(),
        baseline_snapshot,
        baseline_snapshot_hash: baseline_hash,
        execution_override,
        artifacts: input.artifacts,
        frozen_artifacts,
        phase1: Some(phase1),
        baseline_metrics: None,
        baseline_replays: Vec::new(),
        pending_baseline_run_ids: Vec::new(),
        pending_promotion_experiment_id: None,
        last_pause_reason: None,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
        metadata: input.metadata,
    };
    let stored = state
        .put_optimization_campaign(campaign)
        .await
        .map_err(|error| {
            optimization_error(
                StatusCode::BAD_REQUEST,
                format!("failed to store optimization campaign: {error}"),
            )
        })?;
    Ok(Json(optimization_payload(&state, &stored).await))
}

pub(super) async fn optimizations_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(campaign) = state.get_optimization_campaign(&id).await else {
        return Err(optimization_error(
            StatusCode::NOT_FOUND,
            "optimization not found",
        ));
    };
    Ok(Json(optimization_payload(&state, &campaign).await))
}

pub(super) async fn optimizations_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<OptimizationActionInput>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let updated = state
        .apply_optimization_action(
            &id,
            &input.action,
            input.experiment_id.as_deref(),
            input.run_id.as_deref(),
            input.reason.as_deref(),
        )
        .await
        .map_err(|error| {
            let status = if error.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            optimization_error(status, error)
        })?;
    Ok(Json(optimization_payload(&state, &updated).await))
}

pub(super) async fn optimizations_experiment_get(
    State(state): State<AppState>,
    Path((id, experiment_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(campaign) = state.get_optimization_campaign(&id).await else {
        return Err(optimization_error(
            StatusCode::NOT_FOUND,
            "optimization not found",
        ));
    };
    let Some(experiment) = state.get_optimization_experiment(&id, &experiment_id).await else {
        return Err(optimization_error(
            StatusCode::NOT_FOUND,
            "optimization experiment not found",
        ));
    };
    Ok(Json(json!({
        "optimization": campaign,
        "experiment": experiment,
    })))
}

pub(super) async fn optimizations_experiment_apply(
    State(state): State<AppState>,
    Path((id, experiment_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let (campaign, experiment, automation) = state
        .apply_optimization_winner(&id, &experiment_id)
        .await
        .map_err(|error| {
            let status = if error.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            optimization_error(status, error)
        })?;
    Ok(Json(json!({
        "optimization": campaign,
        "experiment": experiment,
        "automation": automation,
    })))
}

pub(super) async fn optimizations_experiments_list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(campaign) = state.get_optimization_campaign(&id).await else {
        return Err(optimization_error(
            StatusCode::NOT_FOUND,
            "optimization not found",
        ));
    };
    let experiments = state.list_optimization_experiments(&id).await;
    Ok(Json(json!({
        "optimization": campaign,
        "experiments": experiments,
        "count": experiments.len(),
    })))
}
