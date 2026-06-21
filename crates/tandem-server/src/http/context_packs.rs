use crate::now_ms;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextPackState {
    #[default]
    Published,
    Superseded,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextPackVisibilityScope {
    #[default]
    SameProject,
    ProjectAllowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ContextPackManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plan_package: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) approved_plan_materialization: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_context: Option<Value>,
    #[serde(default)]
    pub(crate) context_object_refs: Vec<String>,
    #[serde(default)]
    pub(crate) artifact_refs: Vec<String>,
    #[serde(default)]
    pub(crate) governed_memory_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextPackBindingRecord {
    pub(crate) binding_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) consumer_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) consumer_project_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) consumer_workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) alias: Option<String>,
    #[serde(default)]
    pub(crate) required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) actor_metadata: Option<Value>,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextPackRecord {
    pub(crate) pack_id: String,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_key: Option<String>,
    #[serde(default)]
    pub(crate) allowed_project_keys: Vec<String>,
    pub(crate) workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_context_run_id: Option<String>,
    #[serde(default)]
    pub(crate) visibility_scope: ContextPackVisibilityScope,
    pub(crate) state: ContextPackState,
    #[serde(default)]
    pub(crate) manifest: ContextPackManifest,
    #[serde(default)]
    pub(crate) bindings: Vec<ContextPackBindingRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) freshness_window_hours: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) published_actor_metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revoked_actor_metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) superseded_actor_metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) superseded_by_pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) published_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revoked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) superseded_at_ms: Option<u64>,
    pub(crate) created_at_ms: u64,
    pub(crate) updated_at_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackListQuery {
    #[serde(default)]
    pub(super) project_key: Option<String>,
    #[serde(default)]
    pub(super) workspace_root: Option<String>,
    #[serde(default)]
    pub(super) state: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackPublishRequest {
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) project_key: Option<String>,
    #[serde(default)]
    pub(super) allowed_project_keys: Vec<String>,
    pub(super) workspace_root: String,
    #[serde(default)]
    pub(super) source_plan_id: Option<String>,
    #[serde(default)]
    pub(super) source_automation_id: Option<String>,
    #[serde(default)]
    pub(super) source_run_id: Option<String>,
    #[serde(default)]
    pub(super) source_context_run_id: Option<String>,
    #[serde(default)]
    pub(super) plan_package: Option<Value>,
    #[serde(default)]
    pub(super) approved_plan_materialization: Option<Value>,
    #[serde(default)]
    pub(super) runtime_context: Option<Value>,
    #[serde(default)]
    pub(super) context_object_refs: Vec<String>,
    #[serde(default)]
    pub(super) artifact_refs: Vec<String>,
    #[serde(default)]
    pub(super) governed_memory_refs: Vec<String>,
    #[serde(default)]
    pub(super) freshness_window_hours: Option<u32>,
    #[serde(default)]
    pub(super) actor_metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackBindRequest {
    #[serde(default)]
    pub(super) consumer_plan_id: Option<String>,
    #[serde(default)]
    pub(super) consumer_project_key: Option<String>,
    #[serde(default)]
    pub(super) consumer_workspace_root: Option<String>,
    #[serde(default)]
    pub(super) alias: Option<String>,
    #[serde(default)]
    pub(super) required: Option<bool>,
    #[serde(default)]
    pub(super) actor_metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackSupersedeRequest {
    pub(super) superseded_by_pack_id: String,
    #[serde(default)]
    pub(super) actor_metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackRevokeRequest {
    #[serde(default)]
    pub(super) actor_metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextPackPath {
    pub(super) pack_id: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ContextPackReadQuery {
    #[serde(default)]
    pub(super) project_key: Option<String>,
    #[serde(default)]
    pub(super) workspace_root: Option<String>,
}

fn infer_pack_title(request: &ContextPackPublishRequest) -> String {
    request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            request
                .plan_package
                .as_ref()
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            request
                .plan_package
                .as_ref()
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            request
                .source_plan_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("Shared context for {value}"))
        })
        .unwrap_or_else(|| "Shared workflow context".to_string())
}

fn infer_context_object_refs(request: &ContextPackPublishRequest) -> Vec<String> {
    if !request.context_object_refs.is_empty() {
        return request.context_object_refs.clone();
    }
    request
        .plan_package
        .as_ref()
        .and_then(|value| value.get("context_objects"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    row.get("context_object_id")
                        .or_else(|| row.get("contextObjectId"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn normalize_project_keys(values: Vec<String>) -> Vec<String> {
    let mut rows = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    rows
}

pub(crate) fn context_pack_allows_project(
    pack: &ContextPackRecord,
    project_key: Option<&str>,
) -> bool {
    let Some(project_key) = project_key.map(str::trim).filter(|value| !value.is_empty()) else {
        return matches!(
            pack.visibility_scope,
            ContextPackVisibilityScope::SameProject
        );
    };
    pack.project_key.as_deref() == Some(project_key)
        || pack
            .allowed_project_keys
            .iter()
            .any(|candidate| candidate == project_key)
}

pub(crate) fn shared_context_pack_ids_from_metadata(metadata: Option<&Value>) -> Vec<String> {
    tandem_automation::shared_context_pack_ids_from_metadata(metadata)
}

fn emit_context_pack_policy_hook(
    state: &AppState,
    action: &str,
    pack: &ContextPackRecord,
    actor_metadata: Option<Value>,
    related_pack_id: Option<String>,
) {
    state.event_bus.publish(EngineEvent::new(
        "context.pack.policy_hook",
        json!({
            "action": action,
            "pack_id": pack.pack_id,
            "title": pack.title,
            "workspace_root": pack.workspace_root,
            "project_key": pack.project_key,
            "state": pack.state,
            "related_pack_id": related_pack_id,
            "actor_present": actor_metadata.is_some(),
            "actor_metadata": actor_metadata,
            "timestamp_ms": now_ms(),
        }),
    ));
}

pub(super) async fn context_pack_publish(
    State(state): State<AppState>,
    Json(input): Json<ContextPackPublishRequest>,
) -> Result<Json<Value>, StatusCode> {
    let workspace_root = crate::normalize_absolute_workspace_root(&input.workspace_root)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let allowed_project_keys = normalize_project_keys(input.allowed_project_keys.clone());
    let pack_id = format!("context-pack-{}", Uuid::new_v4());
    let pack = ContextPackRecord {
        pack_id: pack_id.clone(),
        title: infer_pack_title(&input),
        summary: input
            .summary
            .clone()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        project_key: input
            .project_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        allowed_project_keys: allowed_project_keys.clone(),
        workspace_root: workspace_root.clone(),
        source_plan_id: input.source_plan_id.clone(),
        source_automation_id: input.source_automation_id.clone(),
        source_run_id: input.source_run_id.clone(),
        source_context_run_id: input.source_context_run_id.clone(),
        visibility_scope: if allowed_project_keys.is_empty() {
            ContextPackVisibilityScope::SameProject
        } else {
            ContextPackVisibilityScope::ProjectAllowlist
        },
        state: ContextPackState::Published,
        manifest: ContextPackManifest {
            plan_package: input.plan_package.clone(),
            approved_plan_materialization: input.approved_plan_materialization.clone(),
            runtime_context: input.runtime_context.clone(),
            context_object_refs: infer_context_object_refs(&input),
            artifact_refs: input
                .artifact_refs
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
            governed_memory_refs: input
                .governed_memory_refs
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
        },
        bindings: Vec::new(),
        freshness_window_hours: input.freshness_window_hours,
        published_actor_metadata: input.actor_metadata,
        revoked_actor_metadata: None,
        superseded_actor_metadata: None,
        superseded_by_pack_id: None,
        published_at_ms: Some(now_ms()),
        revoked_at_ms: None,
        superseded_at_ms: None,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
    };
    emit_context_pack_policy_hook(
        &state,
        "publish",
        &pack,
        pack.published_actor_metadata.clone(),
        None,
    );
    let stored = state.put_context_pack(pack).await.map_err(|error| {
        tracing::warn!("shared workflow context publish failed: {}", error);
        StatusCode::BAD_REQUEST
    })?;
    state.event_bus.publish(EngineEvent::new(
        "context.pack.published",
        json!({
            "pack_id": stored.pack_id,
            "title": stored.title,
            "workspace_root": stored.workspace_root,
            "project_key": stored.project_key,
            "source_plan_id": stored.source_plan_id,
        }),
    ));
    Ok(Json(json!({
        "context_pack": stored,
    })))
}

pub(super) async fn context_pack_list(
    State(state): State<AppState>,
    Query(query): Query<ContextPackListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let workspace_root = query
        .workspace_root
        .as_deref()
        .map(crate::normalize_absolute_workspace_root)
        .transpose()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let packs = state
        .list_context_packs(query.project_key.as_deref(), workspace_root.as_deref())
        .await
        .into_iter()
        .filter(|pack| {
            query
                .state
                .as_deref()
                .map(|state| {
                    let expected = state.trim().to_lowercase();
                    let actual = serde_json::to_string(&pack.state)
                        .ok()
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_lowercase();
                    actual == expected
                })
                .unwrap_or(true)
        })
        .filter(|pack| context_pack_allows_project(pack, query.project_key.as_deref()))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "context_packs": packs })))
}

pub(super) async fn context_pack_get(
    State(state): State<AppState>,
    Path(ContextPackPath { pack_id }): Path<ContextPackPath>,
    Query(query): Query<ContextPackReadQuery>,
) -> Result<Json<Value>, StatusCode> {
    let pack = state
        .get_context_pack(&pack_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    if let Some(project_key) = query.project_key.as_deref() {
        if !context_pack_allows_project(&pack, Some(project_key.trim())) {
            return Err(StatusCode::FORBIDDEN);
        }
    } else if !context_pack_allows_project(&pack, None) {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(workspace_root) = query.workspace_root.as_deref() {
        let normalized = crate::normalize_absolute_workspace_root(workspace_root)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if normalized != pack.workspace_root {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(Json(json!({ "context_pack": pack })))
}

pub(super) async fn context_pack_bind(
    State(state): State<AppState>,
    Path(ContextPackPath { pack_id }): Path<ContextPackPath>,
    Json(input): Json<ContextPackBindRequest>,
) -> Result<Json<Value>, StatusCode> {
    let pack = state
        .get_context_pack(&pack_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    if matches!(pack.state, ContextPackState::Revoked) {
        return Err(StatusCode::CONFLICT);
    }
    if !context_pack_allows_project(&pack, input.consumer_project_key.as_deref()) {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(workspace_root) = input.consumer_workspace_root.as_deref() {
        let normalized = crate::normalize_absolute_workspace_root(workspace_root)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if normalized != pack.workspace_root {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let binding = ContextPackBindingRecord {
        binding_id: format!("context-pack-binding-{}", Uuid::new_v4()),
        consumer_plan_id: input.consumer_plan_id.clone(),
        consumer_project_key: input.consumer_project_key.clone(),
        consumer_workspace_root: input.consumer_workspace_root.clone(),
        alias: input.alias.clone(),
        required: input.required.unwrap_or(true),
        actor_metadata: input.actor_metadata,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
    };
    emit_context_pack_policy_hook(&state, "bind", &pack, binding.actor_metadata.clone(), None);
    let stored = state
        .bind_context_pack(&pack_id, binding)
        .await
        .map_err(|error| {
            tracing::warn!("shared workflow context bind failed: {}", error);
            StatusCode::BAD_REQUEST
        })?;
    state.event_bus.publish(EngineEvent::new(
        "context.pack.bound",
        json!({
            "pack_id": stored.pack_id,
            "title": stored.title,
            "workspace_root": stored.workspace_root,
            "binding_count": stored.bindings.len(),
        }),
    ));
    Ok(Json(json!({
        "context_pack": stored,
    })))
}

pub(super) async fn context_pack_revoke(
    State(state): State<AppState>,
    Path(ContextPackPath { pack_id }): Path<ContextPackPath>,
    Json(input): Json<ContextPackRevokeRequest>,
) -> Result<Json<Value>, StatusCode> {
    let stored = state
        .get_context_pack(&pack_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    emit_context_pack_policy_hook(
        &state,
        "revoke",
        &stored,
        input.actor_metadata.clone(),
        None,
    );
    let stored = state
        .revoke_context_pack(&pack_id, input.actor_metadata)
        .await
        .map_err(|error| {
            if error.to_string().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                tracing::warn!("shared workflow context revoke failed: {}", error);
                StatusCode::BAD_REQUEST
            }
        })?;
    state.event_bus.publish(EngineEvent::new(
        "context.pack.revoked",
        json!({
            "pack_id": stored.pack_id,
            "title": stored.title,
            "workspace_root": stored.workspace_root,
        }),
    ));
    Ok(Json(json!({
        "context_pack": stored,
    })))
}

pub(super) async fn context_pack_supersede(
    State(state): State<AppState>,
    Path(ContextPackPath { pack_id }): Path<ContextPackPath>,
    Json(input): Json<ContextPackSupersedeRequest>,
) -> Result<Json<Value>, StatusCode> {
    if input.superseded_by_pack_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let target = state
        .get_context_pack(&input.superseded_by_pack_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let current = state
        .get_context_pack(&pack_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    emit_context_pack_policy_hook(
        &state,
        "supersede",
        &current,
        input.actor_metadata.clone(),
        Some(target.pack_id.clone()),
    );
    let stored = state
        .supersede_context_pack(&pack_id, target.pack_id.clone(), input.actor_metadata)
        .await
        .map_err(|error| {
            if error.to_string().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                tracing::warn!("shared workflow context supersede failed: {}", error);
                StatusCode::BAD_REQUEST
            }
        })?;
    state.event_bus.publish(EngineEvent::new(
        "context.pack.superseded",
        json!({
            "pack_id": stored.pack_id,
            "superseded_by_pack_id": stored.superseded_by_pack_id,
            "title": stored.title,
            "workspace_root": stored.workspace_root,
        }),
    ));
    Ok(Json(json!({
        "context_pack": stored,
    })))
}
