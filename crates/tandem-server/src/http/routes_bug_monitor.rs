use super::bug_monitor::*;
use crate::http::AppState;
use axum::routing::{delete, get, post, MethodRouter};
use axum::Router;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    let router = router.route(
        "/config/incident-monitor",
        get(get_bug_monitor_config).patch(patch_bug_monitor_config),
    );
    let router = apply_incident_monitor_routes(router, "/incident-monitor");

    // Temporary compatibility until the SDK/UI rename batches land.
    let router = router.route(
        "/config/bug-monitor",
        get(get_bug_monitor_config).patch(patch_bug_monitor_config),
    );
    apply_incident_monitor_routes(router, "/bug-monitor")
}

fn apply_incident_monitor_routes(router: Router<AppState>, prefix: &str) -> Router<AppState> {
    let router = route_prefixed(router, prefix, "/status", get(get_bug_monitor_status));
    let router = route_prefixed(
        router,
        prefix,
        "/status/recompute",
        post(recompute_bug_monitor_status),
    );
    let router = route_prefixed(router, prefix, "/pause", post(pause_bug_monitor));
    let router = route_prefixed(router, prefix, "/resume", post(resume_bug_monitor));
    let router = route_prefixed(router, prefix, "/debug", get(get_bug_monitor_debug));
    let router = route_prefixed(
        router,
        prefix,
        "/security/authority-inventory",
        get(get_bug_monitor_authority_inventory),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/security/posture-checks",
        get(get_bug_monitor_security_posture_checks),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/security/assessment-probes",
        post(run_bug_monitor_security_assessment_probes),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/security/assessment-report",
        post(generate_bug_monitor_security_assessment_report),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/security/deployment-cards",
        post(generate_bug_monitor_deployment_cards),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/route-preview",
        post(preview_bug_monitor_route),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/incidents",
        get(list_bug_monitor_incidents),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/incidents/bulk-delete",
        post(bulk_delete_bug_monitor_incidents),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/incidents/{id}",
        get(get_bug_monitor_incident).delete(delete_bug_monitor_incident),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/incidents/{id}/replay",
        post(replay_bug_monitor_incident),
    );
    let router = route_prefixed(router, prefix, "/drafts", get(list_bug_monitor_drafts));
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/bulk-delete",
        post(bulk_delete_bug_monitor_drafts),
    );
    let router = route_prefixed(router, prefix, "/posts", get(list_bug_monitor_posts));
    let router = route_prefixed(
        router,
        prefix,
        "/posts/bulk-delete",
        post(bulk_delete_bug_monitor_posts),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/posts/{id}",
        delete(delete_bug_monitor_post),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}",
        get(get_bug_monitor_draft).delete(delete_bug_monitor_draft),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/approve",
        post(approve_bug_monitor_draft),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/deny",
        post(deny_bug_monitor_draft),
    );
    let router = route_prefixed(router, prefix, "/report", post(report_bug_monitor_issue));
    let router = route_prefixed(
        router,
        prefix,
        "/intake/report",
        post(report_bug_monitor_intake),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/intake/keys",
        get(list_bug_monitor_intake_keys).post(create_bug_monitor_intake_key),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/intake/keys/{id}/disable",
        post(disable_bug_monitor_intake_key),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/log-sources/{project_id}/{source_id}/reset-offset",
        post(reset_bug_monitor_log_source_offset),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/log-sources/{project_id}/{source_id}/replay-latest",
        post(replay_latest_bug_monitor_log_source_candidate),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/triage-run",
        post(create_bug_monitor_triage_run),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/triage-summary",
        post(create_bug_monitor_triage_summary),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/issue-draft",
        post(draft_bug_monitor_issue),
    );
    let router = route_prefixed(
        router,
        prefix,
        "/drafts/{id}/publish",
        post(publish_bug_monitor_draft),
    );
    route_prefixed(
        router,
        prefix,
        "/drafts/{id}/recheck-match",
        post(recheck_bug_monitor_draft_match),
    )
}

fn route_prefixed(
    router: Router<AppState>,
    prefix: &str,
    suffix: &str,
    method_router: MethodRouter<AppState>,
) -> Router<AppState> {
    let path = format!("{prefix}{suffix}");
    router.route(&path, method_router)
}
