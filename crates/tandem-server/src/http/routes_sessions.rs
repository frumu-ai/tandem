// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/session", post(create_session).get(list_sessions))
        .route("/api/session", post(create_session).get(list_sessions))
        .route("/session/status", get(session_status_handler))
        .route(
            "/session/{id}",
            get(get_session)
                .delete(delete_session)
                .patch(update_session),
        )
        .route("/session/{id}/attach", post(attach_session))
        .route(
            "/session/{id}/workspace/override",
            post(grant_workspace_override),
        )
        .route(
            "/api/session/{id}",
            get(get_session)
                .delete(delete_session)
                .patch(update_session),
        )
        .route("/api/session/{id}/attach", post(attach_session))
        .route(
            "/api/session/{id}/workspace/override",
            post(grant_workspace_override),
        )
        .route(
            "/session/{id}/message",
            get(session_messages).post(post_session_message_append),
        )
        .route(
            "/api/session/{id}/message",
            get(session_messages).post(post_session_message_append),
        )
        .route("/session/{id}/todo", get(session_todos))
        .route("/api/session/{id}/todo", get(session_todos))
        .route("/session/{id}/prompt_async", post(prompt_async))
        .route("/api/session/{id}/prompt_async", post(prompt_async))
        .route("/session/{id}/prompt_sync", post(prompt_sync))
        .route("/api/session/{id}/prompt_sync", post(prompt_sync))
        .route("/session/{id}/run", get(get_active_run))
        .route("/api/session/{id}/run", get(get_active_run))
        .route("/session/{id}/abort", post(abort_session))
        .route("/session/{id}/cancel", post(abort_session))
        .route("/api/session/{id}/cancel", post(abort_session))
        .route("/session/{id}/run/{run_id}/cancel", post(cancel_run_by_id))
        .route(
            "/api/session/{id}/run/{run_id}/cancel",
            post(cancel_run_by_id),
        )
        .route("/session/{id}/fork", post(fork_session))
        .route("/session/{id}/revert", post(revert_session))
        .route("/session/{id}/unrevert", post(unrevert_session))
        .route(
            "/session/{id}/share",
            post(share_session).delete(unshare_session),
        )
        .route("/session/{id}/summarize", post(summarize_session))
        .route("/session/{id}/diff", get(session_diff))
        .route("/session/{id}/children", get(session_children))
        .route("/session/{id}/init", post(init_session))
}
