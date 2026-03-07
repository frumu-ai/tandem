use super::failure_reporter::*;
use crate::http::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/config/failure-reporter",
            get(get_failure_reporter_config).patch(patch_failure_reporter_config),
        )
        .route("/failure-reporter/status", get(get_failure_reporter_status))
        .route(
            "/failure-reporter/status/recompute",
            post(recompute_failure_reporter_status),
        )
        .route("/failure-reporter/pause", post(pause_failure_reporter))
        .route("/failure-reporter/resume", post(resume_failure_reporter))
        .route("/failure-reporter/debug", get(get_failure_reporter_debug))
        .route(
            "/failure-reporter/incidents",
            get(list_failure_reporter_incidents),
        )
        .route(
            "/failure-reporter/incidents/{id}",
            get(get_failure_reporter_incident),
        )
        .route(
            "/failure-reporter/incidents/{id}/replay",
            post(replay_failure_reporter_incident),
        )
        .route(
            "/failure-reporter/drafts",
            get(list_failure_reporter_drafts),
        )
        .route(
            "/failure-reporter/drafts/{id}",
            get(get_failure_reporter_draft),
        )
        .route(
            "/failure-reporter/drafts/{id}/approve",
            post(approve_failure_reporter_draft),
        )
        .route(
            "/failure-reporter/drafts/{id}/deny",
            post(deny_failure_reporter_draft),
        )
        .route(
            "/failure-reporter/report",
            post(report_failure_reporter_issue),
        )
        .route(
            "/failure-reporter/drafts/{id}/triage-run",
            post(create_failure_reporter_triage_run),
        )
}
