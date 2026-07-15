// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct PresetForkInput {
    pub kind: String,
    pub source_path: String,
    #[serde(default)]
    pub target_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PresetOverrideWriteInput {
    pub content: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct PresetOverridesExportInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
}

pub(super) async fn presets_index(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let index = state.preset_registry.index().await.map_err(|err| {
        tracing::warn!("presets index failed: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({ "index": index })))
}

pub(super) async fn presets_compose_preview(
    Json(input): Json<crate::preset_composer::PromptComposeInput>,
) -> Result<Json<Value>, StatusCode> {
    let out = crate::preset_composer::compose(input);
    Ok(Json(json!({ "composition": out })))
}

pub(super) async fn presets_fork(
    State(state): State<AppState>,
    Json(input): Json<PresetForkInput>,
) -> Result<Json<Value>, StatusCode> {
    let source_path = std::path::PathBuf::from(&input.source_path);
    let path = state
        .preset_registry
        .fork_to_override(&input.kind, &source_path, input.target_id.as_deref())
        .await
        .map_err(|err| {
            tracing::warn!("preset fork failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    state.event_bus.publish(EngineEvent::new(
        "registry.updated",
        json!({ "entity": "presets" }),
    ));
    Ok(Json(json!({
        "forked": true,
        "path": path.to_string_lossy(),
    })))
}

pub(super) async fn presets_override_put(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
    Json(input): Json<PresetOverrideWriteInput>,
) -> Result<Json<Value>, StatusCode> {
    let path = state
        .preset_registry
        .save_override(&kind, &id, &input.content)
        .await
        .map_err(|err| {
            tracing::warn!("preset override put failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    state.event_bus.publish(EngineEvent::new(
        "registry.updated",
        json!({ "entity": "presets" }),
    ));
    Ok(Json(json!({
        "saved": true,
        "path": path.to_string_lossy(),
    })))
}

pub(super) async fn presets_override_delete(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let removed = state
        .preset_registry
        .delete_override(&kind, &id)
        .await
        .map_err(|err| {
            tracing::warn!("preset override delete failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    if removed {
        state.event_bus.publish(EngineEvent::new(
            "registry.updated",
            json!({ "entity": "presets" }),
        ));
    }
    Ok(Json(json!({ "removed": removed })))
}

pub(super) async fn presets_capability_summary(
    Json(input): Json<crate::preset_summary::CapabilitySummaryInput>,
) -> Result<Json<Value>, StatusCode> {
    let summary = crate::preset_summary::summarize(input);
    Ok(Json(json!({ "summary": summary })))
}

pub(super) async fn presets_export_overrides(
    State(state): State<AppState>,
    Json(input): Json<PresetOverridesExportInput>,
) -> Result<Json<Value>, StatusCode> {
    let name = input.name.as_deref().unwrap_or("preset-overrides");
    let version = input.version.as_deref().unwrap_or("0.1.0");
    let exported = state
        .preset_registry
        .export_overrides(name, version, input.output_path.as_deref())
        .await
        .map_err(|err| {
            tracing::warn!("preset overrides export failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    Ok(Json(json!({ "exported": exported })))
}
