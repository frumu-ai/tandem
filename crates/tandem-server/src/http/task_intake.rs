use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use tandem_orchestrator::TaskIntakeRequest;

use super::*;

fn validate_task_intake(task: &TaskIntakeRequest) -> Result<(), (StatusCode, Json<Value>)> {
    if task.task_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "task_id is required",
                "code": "TASK_INTAKE_INVALID",
            })),
        ));
    }
    if task.title.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "title is required",
                "code": "TASK_INTAKE_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = task.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "TASK_INTAKE_INVALID",
                })),
            )
        })?;
    }
    Ok(())
}

pub(super) async fn task_intake_preview(
    State(_state): State<AppState>,
    Json(task): Json<TaskIntakeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_task_intake(&task)?;
    let preview = task.preview();
    Ok(Json(json!({
        "task": task,
        "preview": preview,
        "grouping_signals": task.grouping_signals(),
    })))
}
