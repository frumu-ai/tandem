// Bug Monitor intake helpers split from part06.rs for the file-size gate
// (same module via include!).

fn apply_bug_monitor_report_source_approval_binding(
    _config: &BugMonitorConfig,
    report: &mut BugMonitorSubmission,
) {
    clear_bug_monitor_raw_source_routing_fields(report);
}

fn clear_bug_monitor_raw_source_routing_fields(report: &mut BugMonitorSubmission) {
    report.project_id = None;
    report.source_kind = None;
    report.log_source_id = None;
    report.route_tags.clear();
    report.allowed_destination_ids.clear();
    report.default_destination_ids.clear();
    report.tenant_id = None;
    report.workspace_id = None;
    report.event_schema_version = None;
    report.source_approval_policy = None;
    report.redaction_profile = None;
    report.retention_profile = None;
}

fn bug_monitor_intake_key_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get("x-tandem-bug-monitor-intake-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?
        .trim();
    let token = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))?
        .trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}
