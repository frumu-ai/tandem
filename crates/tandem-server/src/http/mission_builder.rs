use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_workflows::MissionBlueprint;

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct MissionBuilderPreviewRequest {
    pub blueprint: MissionBlueprint,
    #[serde(default)]
    pub schedule: Option<crate::AutomationV2Schedule>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MissionBuilderApplyRequest {
    pub blueprint: MissionBlueprint,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub schedule: Option<crate::AutomationV2Schedule>,
}

pub(super) async fn mission_builder_preview(
    State(_state): State<AppState>,
    Json(input): Json<MissionBuilderPreviewRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let preview = compile_blueprint_preview(input.blueprint, input.schedule, "mission_builder")?;
    Ok(Json(
        serde_json::to_value(preview).unwrap_or_else(|_| json!({})),
    ))
}

pub(super) async fn mission_builder_apply(
    State(state): State<AppState>,
    Json(input): Json<MissionBuilderApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let creator_id = input
        .creator_id
        .as_deref()
        .unwrap_or("mission_builder")
        .to_string();
    let preview = compile_blueprint_preview(input.blueprint, input.schedule, &creator_id)?;
    let stored = state
        .put_automation_v2(preview.automation.clone())
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "MISSION_BUILDER_APPLY_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "automation": stored,
        "mission_spec": preview.mission_spec,
        "work_items": preview.work_items,
        "node_previews": preview.node_previews,
        "validation": preview.validation,
    })))
}

fn compile_blueprint_preview(
    blueprint: MissionBlueprint,
    schedule: Option<crate::AutomationV2Schedule>,
    creator_id: &str,
) -> Result<mission_builder_runtime::MissionCompilePreview, (StatusCode, Json<Value>)> {
    let preview = tandem_plan_compiler::api::compile_mission_blueprint_preview(blueprint.clone())
        .map_err(|validation| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "mission blueprint validation failed",
                "code": "MISSION_BLUEPRINT_INVALID",
                "validation": validation,
            })),
        )
    })?;
    let mut automation =
        mission_builder_runtime::compile_to_automation(blueprint.clone(), schedule, creator_id);
    automation.knowledge = preview.mission_spec.knowledge.clone();

    Ok(mission_builder_runtime::MissionCompilePreview {
        blueprint,
        automation,
        mission_spec: preview.mission_spec,
        work_items: preview.work_items,
        node_previews: preview.node_previews,
        validation: preview.validation,
    })
}
