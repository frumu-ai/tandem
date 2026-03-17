use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

#[derive(Debug, Deserialize, Default)]
pub(super) struct ExternalActionsListQuery {
    pub(super) limit: Option<usize>,
}

pub(super) async fn list_external_actions(
    State(state): State<AppState>,
    Query(query): Query<ExternalActionsListQuery>,
) -> impl IntoResponse {
    let actions = state.list_external_actions(query.limit.unwrap_or(50)).await;
    Json(json!({
        "count": actions.len(),
        "actions": actions,
    }))
}

pub(super) async fn get_external_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.get_external_action(&id).await {
        Some(action) => Json(json!({
            "action": action,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "External action not found",
                "code": "EXTERNAL_ACTION_NOT_FOUND",
                "action_id": id,
            })),
        )
            .into_response(),
    }
}
