// Bug Monitor intake helpers split from part06.rs for the file-size gate
// (same module via include!).

fn apply_bug_monitor_report_source_approval_binding(
    config: &BugMonitorConfig,
    report: &mut BugMonitorSubmission,
) {
    let project_id = report
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(project_id) = project_id else {
        report.source_approval_policy = None;
        return;
    };
    let Some(project) = config
        .monitored_projects
        .iter()
        .find(|project| project.project_id == project_id)
    else {
        report.source_approval_policy = None;
        return;
    };
    let source_id = report
        .log_source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let configured_source = source_id.and_then(|source_id| {
        project
            .log_sources
            .iter()
            .find(|source| source.source_id == source_id)
    });
    if source_id.is_some() && configured_source.is_none() {
        report.source_approval_policy = None;
        return;
    }
    report.source_approval_policy = None;
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
