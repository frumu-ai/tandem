// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use super::workflows::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/workflows", get(workflows_list))
        .route("/workflows/validate", post(workflows_validate))
        .route("/workflows/simulate", post(workflows_simulate))
        .route("/workflows/events", get(workflow_events))
        .route("/workflows/runs", get(workflow_runs_list))
        .route("/workflows/runs/{id}", get(workflow_runs_get))
        .route("/workflows/runs/{id}/gate", post(workflow_run_gate_decide))
        .route("/workflows/{id}", get(workflows_get))
        .route("/workflows/{id}/run", post(workflows_run))
        .route("/workflow-hooks", get(workflow_hooks_list))
        .route(
            "/workflow-hooks/{id}",
            axum::routing::patch(workflow_hooks_patch),
        )
}
