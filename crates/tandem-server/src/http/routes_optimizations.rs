// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use super::optimizations::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/optimizations",
            get(optimizations_list).post(optimizations_create),
        )
        .route("/optimizations/{id}", get(optimizations_get))
        .route("/optimizations/{id}/actions", post(optimizations_action))
        .route(
            "/optimizations/{id}/experiments",
            get(optimizations_experiments_list),
        )
        .route(
            "/optimizations/{id}/experiments/{experiment_id}",
            get(optimizations_experiment_get).post(optimizations_experiment_apply),
        )
}
