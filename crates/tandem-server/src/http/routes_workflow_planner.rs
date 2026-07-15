// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{delete, get, patch, post};
use axum::Router;

use super::workflow_planner::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/workflow-plans/sessions",
            get(workflow_planner_session_list),
        )
        .route(
            "/workflow-plans/sessions",
            post(workflow_planner_session_create),
        )
        .route(
            "/workflow-plans/sessions/{session_id}",
            get(workflow_planner_session_get),
        )
        .route(
            "/workflow-plans/sessions/{session_id}",
            patch(workflow_planner_session_patch),
        )
        .route(
            "/workflow-plans/sessions/{session_id}",
            delete(workflow_planner_session_delete),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/duplicate",
            post(workflow_planner_session_duplicate),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/start",
            post(workflow_planner_session_start),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/start-async",
            post(workflow_planner_session_start_async),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/message",
            post(workflow_planner_session_message),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/message-async",
            post(workflow_planner_session_message_async),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/reset",
            post(workflow_planner_session_reset),
        )
        .route("/workflow-plans/preview", post(workflow_plan_preview))
        .route("/workflow-plans/apply", post(workflow_plan_apply))
        .route("/workflow-plans/chat/start", post(workflow_plan_chat_start))
        .route(
            "/workflow-plans/chat/message",
            post(workflow_plan_chat_message),
        )
        .route("/workflow-plans/chat/reset", post(workflow_plan_chat_reset))
        .route(
            "/workflow-plans/export/pack",
            post(workflow_plan_export_pack),
        )
        .route(
            "/workflow-plans/export/pack/download",
            get(workflow_plan_export_pack_download),
        )
        .route(
            "/workflow-plans/import/pack/preview",
            post(workflow_plan_import_pack_preview),
        )
        .route(
            "/workflow-plans/import/pack",
            post(workflow_plan_import_pack),
        )
        .route(
            "/workflow-plans/import/preview",
            post(workflow_plan_import_preview),
        )
        .route("/workflow-plans/import", post(workflow_plan_import))
        .route("/workflow-plans/{plan_id}", get(workflow_plan_get))
}
