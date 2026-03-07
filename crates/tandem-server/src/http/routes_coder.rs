use axum::routing::{get, post};
use axum::Router;

use super::coder::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/coder/runs", post(coder_run_create).get(coder_run_list))
        .route("/coder/runs/{id}", get(coder_run_get))
        .route("/coder/runs/{id}/approve", post(coder_run_approve))
        .route("/coder/runs/{id}/cancel", post(coder_run_cancel))
        .route("/coder/runs/{id}/artifacts", get(coder_run_artifacts))
        .route("/coder/runs/{id}/memory-hits", get(coder_memory_hits_get))
        .route(
            "/coder/runs/{id}/triage-summary",
            post(coder_triage_summary_create),
        )
        .route(
            "/coder/runs/{id}/pr-review-summary",
            post(coder_pr_review_summary_create),
        )
        .route(
            "/coder/runs/{id}/merge-recommendation-summary",
            post(coder_merge_recommendation_summary_create),
        )
        .route(
            "/coder/runs/{id}/memory-candidates",
            get(coder_memory_candidate_list).post(coder_memory_candidate_create),
        )
        .route(
            "/coder/runs/{id}/memory-candidates/{candidate_id}/promote",
            post(coder_memory_candidate_promote),
        )
}
