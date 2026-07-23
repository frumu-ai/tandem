// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use crate::AppState;

use super::system_api::run_shell;
use super::system_api_hardened::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    let host_routes = Router::<AppState>::new()
        .route("/find", get(find_text))
        .route("/find/file", get(find_file))
        .route("/find/symbol", get(find_symbol))
        .route("/file", get(file_list))
        .route("/file/content", get(file_content))
        .route("/file/status", get(file_status))
        .route("/vcs", get(vcs))
        .route("/lsp", get(lsp_status))
        .route("/formatter", get(formatter_status))
        .route("/command", get(command_list))
        .route("/session/{id}/command", post(run_command))
        .route("/session/{id}/shell", post(run_shell))
        .route("/path", get(path_info))
        .route("/scheduler/metrics", get(scheduler_metrics))
        .route_layer(axum::middleware::from_fn(
            super::host_authority::require_direct_loopback_request,
        ));
    router.merge(host_routes)
}
