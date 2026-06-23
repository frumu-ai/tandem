#[derive(Debug, Clone, Default)]
struct BugMonitorRoutePreviewContext {
    event_type: Option<String>,
    source: Option<String>,
    component: Option<String>,
    risk_level: Option<String>,
    confidence: Option<String>,
    expected_destination: Option<String>,
    project_id: Option<String>,
    log_source_id: Option<String>,
    route_tags: Vec<String>,
}

pub(super) async fn preview_bug_monitor_route(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorRoutePreviewInput>,
) -> Response {
    let draft = if let Some(draft_id) = trim_bug_monitor_preview_value(input.draft_id.as_deref()) {
        match state.get_bug_monitor_draft(&draft_id).await {
            Some(draft) => Some(draft),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "Bug monitor draft not found",
                        "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                        "draft_id": draft_id,
                    })),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let incident =
        if let Some(incident_id) = trim_bug_monitor_preview_value(input.incident_id.as_deref()) {
            match state.get_bug_monitor_incident(&incident_id).await {
                Some(incident) => Some(incident),
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": "Bug monitor incident not found",
                            "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                            "incident_id": incident_id,
                        })),
                    )
                        .into_response();
                }
            }
        } else {
            None
        };
    let status = state.bug_monitor_status_snapshot().await;
    let context = bug_monitor_route_preview_context(&input, draft.as_ref(), incident.as_ref());
    let preview = build_bug_monitor_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &context,
        &trim_bug_monitor_preview_values(&input.destination_ids),
    );
    Json(preview).into_response()
}

fn build_bug_monitor_route_preview(
    config: &BugMonitorConfig,
    destinations: &[BugMonitorDestinationConfig],
    readiness: &[BugMonitorDestinationReadiness],
    context: &BugMonitorRoutePreviewContext,
    requested_destination_ids: &[String],
) -> BugMonitorRoutePreviewResponse {
    let default_destination_ids = config.effective_default_destination_ids();
    let matches = if requested_destination_ids.is_empty() {
        bug_monitor_route_preview_matches(config, context, &default_destination_ids, destinations)
    } else {
        vec![BugMonitorRoutePreviewMatch {
            route_id: None,
            route_name: None,
            destination_ids: requested_destination_ids.to_vec(),
            approval_required: bug_monitor_route_preview_approval_required(
                None,
                context,
                config,
                destinations,
                requested_destination_ids,
            ),
            reason: Some("requested_destination_override".to_string()),
        }]
    };
    let mut effective_destination_ids = Vec::new();
    for preview_match in &matches {
        for destination_id in &preview_match.destination_ids {
            push_bug_monitor_preview_unique(&mut effective_destination_ids, destination_id);
        }
    }
    let selected_destinations =
        bug_monitor_preview_selected_destinations(destinations, &effective_destination_ids);
    let selected_readiness =
        bug_monitor_preview_selected_readiness(readiness, &effective_destination_ids);
    let mut blocked_reasons = Vec::new();
    if effective_destination_ids.is_empty() {
        blocked_reasons.push("No destination matched route preview".to_string());
    }
    for destination_id in &effective_destination_ids {
        if !destinations
            .iter()
            .any(|destination| destination.destination_id == *destination_id)
        {
            blocked_reasons.push(format!("Destination `{destination_id}` is not configured"));
            continue;
        }
        match readiness
            .iter()
            .find(|row| row.destination_id == *destination_id)
        {
            Some(row) if row.publish_ready => {}
            Some(row) => {
                let detail = if row.missing.is_empty() {
                    "readiness is false".to_string()
                } else {
                    row.missing.join(", ")
                };
                blocked_reasons.push(format!("Destination `{destination_id}` is not ready: {detail}"));
            }
            None => blocked_reasons.push(format!(
                "Destination `{destination_id}` has no readiness result"
            )),
        }
    }
    let approval_required = matches.iter().any(|row| row.approval_required);

    BugMonitorRoutePreviewResponse {
        matches,
        destinations: selected_destinations,
        readiness: selected_readiness,
        default_destination_ids,
        effective_destination_ids,
        approval_required,
        blocked: !blocked_reasons.is_empty(),
        blocked_reasons,
    }
}

fn bug_monitor_route_preview_matches(
    config: &BugMonitorConfig,
    context: &BugMonitorRoutePreviewContext,
    default_destination_ids: &[String],
    destinations: &[BugMonitorDestinationConfig],
) -> Vec<BugMonitorRoutePreviewMatch> {
    let mut routes = config
        .routes
        .iter()
        .filter(|route| route.enabled && bug_monitor_route_matches(route, context))
        .collect::<Vec<_>>();
    routes.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.route_id.cmp(&b.route_id))
    });
    let mut matches = routes
        .into_iter()
        .map(|route| {
            let destination_ids = if route.destination_ids.is_empty() {
                default_destination_ids.to_vec()
            } else {
                trim_bug_monitor_preview_values(&route.destination_ids)
            };
            BugMonitorRoutePreviewMatch {
                route_id: Some(route.route_id.clone()),
                route_name: Some(route.name.clone()),
                approval_required: bug_monitor_route_preview_approval_required(
                    Some(route),
                    context,
                    config,
                    destinations,
                    &destination_ids,
                ),
                destination_ids,
                reason: Some(bug_monitor_route_match_reason(route)),
            }
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches.push(BugMonitorRoutePreviewMatch {
            route_id: None,
            route_name: None,
            destination_ids: default_destination_ids.to_vec(),
            approval_required: bug_monitor_route_preview_approval_required(
                None,
                context,
                config,
                destinations,
                default_destination_ids,
            ),
            reason: Some("default_destination".to_string()),
        });
    }
    matches
}

fn bug_monitor_route_preview_context(
    input: &BugMonitorRoutePreviewInput,
    draft: Option<&BugMonitorDraftRecord>,
    incident: Option<&BugMonitorIncidentRecord>,
) -> BugMonitorRoutePreviewContext {
    let report = input.report.as_ref();
    BugMonitorRoutePreviewContext {
        event_type: first_bug_monitor_preview_value(&[
            input.event_type.as_deref(),
            report.and_then(|row| row.event.as_deref()),
            incident.map(|row| row.event_type.as_str()),
        ]),
        source: first_bug_monitor_preview_value(&[
            input.source.as_deref(),
            report.and_then(|row| row.source.as_deref()),
            incident.and_then(|row| row.source.as_deref()),
        ]),
        component: first_bug_monitor_preview_value(&[
            input.component.as_deref(),
            report.and_then(|row| row.component.as_deref()),
            incident.and_then(|row| row.component.as_deref()),
        ]),
        risk_level: first_bug_monitor_preview_value(&[
            input.risk_level.as_deref(),
            report.and_then(|row| row.risk_level.as_deref()),
            draft.and_then(|row| row.risk_level.as_deref()),
            incident.and_then(|row| row.risk_level.as_deref()),
        ]),
        confidence: first_bug_monitor_preview_value(&[
            input.confidence.as_deref(),
            report.and_then(|row| row.confidence.as_deref()),
            draft.and_then(|row| row.confidence.as_deref()),
            incident.and_then(|row| row.confidence.as_deref()),
        ]),
        expected_destination: first_bug_monitor_preview_value(&[
            input.expected_destination.as_deref(),
            report.and_then(|row| row.expected_destination.as_deref()),
            draft.and_then(|row| row.expected_destination.as_deref()),
            incident.and_then(|row| row.expected_destination.as_deref()),
        ]),
        project_id: first_bug_monitor_preview_value(&[
            input.project_id.as_deref(),
            report.and_then(|row| row.project_id.as_deref()),
        ]),
        log_source_id: first_bug_monitor_preview_value(&[
            input.log_source_id.as_deref(),
            report.and_then(|row| row.log_source_id.as_deref()),
        ]),
        route_tags: normalize_bug_monitor_preview_values(&input.route_tags),
    }
}

fn bug_monitor_route_matches(
    route: &BugMonitorRouteConfig,
    context: &BugMonitorRoutePreviewContext,
) -> bool {
    bug_monitor_route_value_matches(&route.match_event_types, context.event_type.as_deref())
        && bug_monitor_route_value_matches(&route.match_sources, context.source.as_deref())
        && bug_monitor_route_value_matches(&route.match_components, context.component.as_deref())
        && bug_monitor_route_value_matches(&route.match_risk_levels, context.risk_level.as_deref())
        && bug_monitor_route_value_matches(&route.match_confidence, context.confidence.as_deref())
        && bug_monitor_route_value_matches(
            &route.match_expected_destinations,
            context.expected_destination.as_deref(),
        )
        && bug_monitor_route_value_matches(&route.match_project_ids, context.project_id.as_deref())
        && bug_monitor_route_value_matches(
            &route.match_log_source_ids,
            context.log_source_id.as_deref(),
        )
        && bug_monitor_route_tags_match(&route.match_route_tags, &context.route_tags)
}

fn bug_monitor_route_preview_approval_required(
    route: Option<&BugMonitorRouteConfig>,
    context: &BugMonitorRoutePreviewContext,
    config: &BugMonitorConfig,
    destinations: &[BugMonitorDestinationConfig],
    destination_ids: &[String],
) -> bool {
    let destination_requires_approval = destination_ids.iter().any(|destination_id| {
        destinations
            .iter()
            .find(|destination| destination.destination_id == *destination_id)
            .map(|destination| destination.require_approval)
            .unwrap_or(false)
    });
    let high_risk = bug_monitor_route_preview_high_risk(context.risk_level.as_deref());
    match route
        .map(|row| &row.approval_policy)
        .unwrap_or(&BugMonitorApprovalPolicy::Inherit)
    {
        BugMonitorApprovalPolicy::Always => true,
        BugMonitorApprovalPolicy::Never => false,
        BugMonitorApprovalPolicy::HighRisk => destination_requires_approval || high_risk,
        BugMonitorApprovalPolicy::Inherit => {
            config.require_approval_for_new_issues
                || destination_requires_approval
                || (config.safety_defaults.require_approval_for_high_risk && high_risk)
        }
    }
}

fn bug_monitor_route_preview_high_risk(value: Option<&str>) -> bool {
    matches!(
        normalize_bug_monitor_preview_value(value)
            .unwrap_or_default()
            .as_str(),
        "high" | "critical" | "urgent" | "severe"
    )
}

fn bug_monitor_route_match_reason(route: &BugMonitorRouteConfig) -> String {
    let mut parts = Vec::new();
    if !route.match_event_types.is_empty() {
        parts.push("event_type");
    }
    if !route.match_sources.is_empty() {
        parts.push("source");
    }
    if !route.match_components.is_empty() {
        parts.push("component");
    }
    if !route.match_risk_levels.is_empty() {
        parts.push("risk_level");
    }
    if !route.match_confidence.is_empty() {
        parts.push("confidence");
    }
    if !route.match_expected_destinations.is_empty() {
        parts.push("expected_destination");
    }
    if !route.match_project_ids.is_empty() {
        parts.push("project_id");
    }
    if !route.match_log_source_ids.is_empty() {
        parts.push("log_source_id");
    }
    if !route.match_route_tags.is_empty() {
        parts.push("route_tags");
    }
    if parts.is_empty() {
        "catch_all_route".to_string()
    } else {
        format!("matched_{}", parts.join("_"))
    }
}

fn bug_monitor_route_value_matches(filters: &[String], candidate: Option<&str>) -> bool {
    if filters.is_empty() {
        return true;
    }
    let Some(candidate) = normalize_bug_monitor_preview_value(candidate) else {
        return false;
    };
    filters
        .iter()
        .filter_map(|value| normalize_bug_monitor_preview_value(Some(value)))
        .any(|value| value == candidate)
}

fn bug_monitor_route_tags_match(filters: &[String], candidates: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let candidates = candidates
        .iter()
        .filter_map(|value| normalize_bug_monitor_preview_value(Some(value)))
        .collect::<BTreeSet<_>>();
    filters
        .iter()
        .filter_map(|value| normalize_bug_monitor_preview_value(Some(value)))
        .any(|value| candidates.contains(&value))
}

fn bug_monitor_preview_selected_destinations(
    destinations: &[BugMonitorDestinationConfig],
    destination_ids: &[String],
) -> Vec<BugMonitorDestinationConfig> {
    destination_ids
        .iter()
        .filter_map(|destination_id| {
            destinations
                .iter()
                .find(|destination| destination.destination_id == *destination_id)
                .cloned()
        })
        .collect()
}

fn bug_monitor_preview_selected_readiness(
    readiness: &[BugMonitorDestinationReadiness],
    destination_ids: &[String],
) -> Vec<BugMonitorDestinationReadiness> {
    destination_ids
        .iter()
        .filter_map(|destination_id| {
            readiness
                .iter()
                .find(|row| row.destination_id == *destination_id)
                .cloned()
        })
        .collect()
}

fn normalize_bug_monitor_preview_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(value) = normalize_bug_monitor_preview_value(Some(value)) {
            push_bug_monitor_preview_unique(&mut out, &value);
        }
    }
    out
}

fn trim_bug_monitor_preview_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() {
            push_bug_monitor_preview_unique(&mut out, value);
        }
    }
    out
}

fn trim_bug_monitor_preview_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_bug_monitor_preview_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn first_bug_monitor_preview_value(values: &[Option<&str>]) -> Option<String> {
    values
        .iter()
        .find_map(|value| normalize_bug_monitor_preview_value(*value))
}

fn push_bug_monitor_preview_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}
