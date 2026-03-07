use axum::routing::{get, post};
use axum::Router;

use super::coder::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/coder/runs", post(coder_run_create).get(coder_run_list))
        .route("/coder/runs/{id}", get(coder_run_get))
        .route("/coder/runs/{id}/artifacts", get(coder_run_artifacts))
        .route(
            "/coder/runs/{id}/memory-candidates",
            post(coder_memory_candidate_create),
        )
}
