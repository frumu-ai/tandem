// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/permission", get(list_permissions))
        .route("/permission/{id}/reply", post(reply_permission))
        .route(
            "/sessions/{session_id}/tools/{tool_call_id}/approve",
            post(approve_tool_by_call),
        )
        .route(
            "/sessions/{session_id}/tools/{tool_call_id}/deny",
            post(deny_tool_by_call),
        )
        .route("/question", get(list_questions))
        .route("/question/{id}/reply", post(reply_question))
        .route("/question/{id}/reject", post(reject_question))
        .route(
            "/sessions/{session_id}/questions/{question_id}/answer",
            post(answer_question),
        )
}
