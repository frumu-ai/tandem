use axum::routing::{get, post};
use axum::Router;

use super::routines_automations::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/routines", get(routines_list).post(routines_create))
        .route("/routines/events", get(routines_events))
        .route(
            "/routines/{id}",
            axum::routing::patch(routines_patch).delete(routines_delete),
        )
        .route("/routines/{id}/run_now", post(routines_run_now))
        .route("/routines/{id}/history", get(routines_history))
        .route("/routines/runs", get(routines_runs_all))
        .route("/routines/{id}/runs", get(routines_runs))
        .route("/routines/runs/{run_id}", get(routines_run_get))
        .route(
            "/routines/runs/{run_id}/approve",
            post(routines_run_approve),
        )
        .route("/routines/runs/{run_id}/deny", post(routines_run_deny))
        .route("/routines/runs/{run_id}/pause", post(routines_run_pause))
        .route("/routines/runs/{run_id}/resume", post(routines_run_resume))
        .route(
            "/routines/runs/{run_id}/artifacts",
            get(routines_run_artifacts).post(routines_run_artifact_add),
        )
        .route(
            "/automations",
            get(automations_list).post(automations_create),
        )
        .route("/automations/events", get(automations_events))
        .route(
            "/automations/{id}",
            axum::routing::patch(automations_patch).delete(automations_delete),
        )
        .route("/automations/{id}/run_now", post(automations_run_now))
        .route("/automations/{id}/history", get(automations_history))
        .route("/automations/runs", get(automations_runs_all))
        .route("/automations/{id}/runs", get(automations_runs))
        .route("/automations/runs/{run_id}", get(automations_run_get))
        .route(
            "/automations/runs/{run_id}/approve",
            post(automations_run_approve),
        )
        .route(
            "/automations/runs/{run_id}/deny",
            post(automations_run_deny),
        )
        .route(
            "/automations/runs/{run_id}/pause",
            post(automations_run_pause),
        )
        .route(
            "/automations/runs/{run_id}/resume",
            post(automations_run_resume),
        )
        .route(
            "/automations/runs/{run_id}/artifacts",
            get(automations_run_artifacts).post(automations_run_artifact_add),
        )
        .route(
            "/automations/v2",
            get(automations_v2_list).post(automations_v2_create),
        )
        .route("/automations/v2/events", get(automations_v2_events))
        .route(
            "/automations/v2/{id}",
            get(automations_v2_get)
                .patch(automations_v2_patch)
                .delete(automations_v2_delete),
        )
        .route("/automations/v2/{id}/run_now", post(automations_v2_run_now))
        .route("/automations/v2/{id}/pause", post(automations_v2_pause))
        .route("/automations/v2/{id}/resume", post(automations_v2_resume))
        .route("/automations/v2/{id}/runs", get(automations_v2_runs))
        .route("/automations/v2/runs/{run_id}", get(automations_v2_run_get))
        .route(
            "/automations/v2/runs/{run_id}/pause",
            post(automations_v2_run_pause),
        )
        .route(
            "/automations/v2/runs/{run_id}/resume",
            post(automations_v2_run_resume),
        )
        .route(
            "/automations/v2/runs/{run_id}/cancel",
            post(automations_v2_run_cancel),
        )
        .route(
            "/automations/v2/runs/{run_id}/recover",
            post(automations_v2_run_recover),
        )
        .route(
            "/automations/v2/runs/{run_id}/repair",
            post(automations_v2_run_repair),
        )
        .route(
            "/automations/v2/runs/{run_id}/gate",
            post(automations_v2_run_gate_decide),
        )
}
