use super::bug_monitor::*;
use crate::http::AppState;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/config/bug-monitor",
            get(get_bug_monitor_config).patch(patch_bug_monitor_config),
        )
        .route(
            "/config/failure-reporter",
            get(get_bug_monitor_config).patch(patch_bug_monitor_config),
        )
        .route("/bug-monitor/status", get(get_bug_monitor_status))
        .route("/failure-reporter/status", get(get_bug_monitor_status))
        .route(
            "/bug-monitor/status/recompute",
            post(recompute_bug_monitor_status),
        )
        .route(
            "/failure-reporter/status/recompute",
            post(recompute_bug_monitor_status),
        )
        .route("/bug-monitor/pause", post(pause_bug_monitor))
        .route("/failure-reporter/pause", post(pause_bug_monitor))
        .route("/bug-monitor/resume", post(resume_bug_monitor))
        .route("/failure-reporter/resume", post(resume_bug_monitor))
        .route("/bug-monitor/debug", get(get_bug_monitor_debug))
        .route("/failure-reporter/debug", get(get_bug_monitor_debug))
        .route("/bug-monitor/incidents", get(list_bug_monitor_incidents))
        .route(
            "/failure-reporter/incidents",
            get(list_bug_monitor_incidents),
        )
        .route("/bug-monitor/incidents/{id}", get(get_bug_monitor_incident))
        .route(
            "/failure-reporter/incidents/{id}",
            get(get_bug_monitor_incident),
        )
        .route(
            "/bug-monitor/incidents/{id}/replay",
            post(replay_bug_monitor_incident),
        )
        .route(
            "/failure-reporter/incidents/{id}/replay",
            post(replay_bug_monitor_incident),
        )
        .route("/bug-monitor/drafts", get(list_bug_monitor_drafts))
        .route("/failure-reporter/drafts", get(list_bug_monitor_drafts))
        .route("/bug-monitor/posts", get(list_bug_monitor_posts))
        .route("/failure-reporter/posts", get(list_bug_monitor_posts))
        .route("/bug-monitor/drafts/{id}", get(get_bug_monitor_draft))
        .route("/failure-reporter/drafts/{id}", get(get_bug_monitor_draft))
        .route(
            "/bug-monitor/drafts/{id}/approve",
            post(approve_bug_monitor_draft),
        )
        .route(
            "/failure-reporter/drafts/{id}/approve",
            post(approve_bug_monitor_draft),
        )
        .route(
            "/bug-monitor/drafts/{id}/deny",
            post(deny_bug_monitor_draft),
        )
        .route(
            "/failure-reporter/drafts/{id}/deny",
            post(deny_bug_monitor_draft),
        )
        .route("/bug-monitor/report", post(report_bug_monitor_issue))
        .route("/failure-reporter/report", post(report_bug_monitor_issue))
        .route(
            "/bug-monitor/drafts/{id}/triage-run",
            post(create_bug_monitor_triage_run),
        )
        .route(
            "/failure-reporter/drafts/{id}/triage-run",
            post(create_bug_monitor_triage_run),
        )
        .route(
            "/bug-monitor/drafts/{id}/issue-draft",
            post(draft_bug_monitor_issue),
        )
        .route(
            "/failure-reporter/drafts/{id}/issue-draft",
            post(draft_bug_monitor_issue),
        )
        .route(
            "/bug-monitor/drafts/{id}/publish",
            post(publish_bug_monitor_draft),
        )
        .route(
            "/failure-reporter/drafts/{id}/publish",
            post(publish_bug_monitor_draft),
        )
        .route(
            "/bug-monitor/drafts/{id}/recheck-match",
            post(recheck_bug_monitor_draft_match),
        )
        .route(
            "/failure-reporter/drafts/{id}/recheck-match",
            post(recheck_bug_monitor_draft_match),
        )
}
