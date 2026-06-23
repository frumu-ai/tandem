use std::collections::BTreeSet;

use anyhow::Context;

use crate::{
    bug_monitor_github, now_ms, AppState, BugMonitorApprovalPolicy, BugMonitorConfig,
    BugMonitorDestinationConfig, BugMonitorDestinationKind, BugMonitorDestinationReadiness,
    BugMonitorDraftRecord, BugMonitorIncidentRecord, BugMonitorPostRecord, BugMonitorRouteConfig,
    BugMonitorRoutePreviewMatch, BugMonitorRoutePreviewResponse, BugMonitorSubmission,
    BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID,
};

#[derive(Debug, Clone, Default)]
pub struct BugMonitorRouteContext {
    pub event_type: Option<String>,
    pub source: Option<String>,
    pub component: Option<String>,
    pub risk_level: Option<String>,
    pub confidence: Option<String>,
    pub expected_destination: Option<String>,
    pub project_id: Option<String>,
    pub log_source_id: Option<String>,
    pub route_tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BugMonitorPublishRequest {
    pub draft_id: String,
    pub incident_id: Option<String>,
    pub mode: bug_monitor_github::PublishMode,
    pub destination_ids: Vec<String>,
}

pub fn build_route_context(
    event_type: Option<&str>,
    source: Option<&str>,
    component: Option<&str>,
    risk_level: Option<&str>,
    confidence: Option<&str>,
    expected_destination: Option<&str>,
    project_id: Option<&str>,
    log_source_id: Option<&str>,
    route_tags: &[String],
    report: Option<&BugMonitorSubmission>,
    draft: Option<&BugMonitorDraftRecord>,
    incident: Option<&BugMonitorIncidentRecord>,
) -> BugMonitorRouteContext {
    BugMonitorRouteContext {
        event_type: first_route_value(&[
            event_type,
            report.and_then(|row| row.event.as_deref()),
            incident.map(|row| row.event_type.as_str()),
        ]),
        source: first_route_value(&[
            source,
            report.and_then(|row| row.source.as_deref()),
            incident.and_then(|row| row.source.as_deref()),
        ]),
        component: first_route_value(&[
            component,
            report.and_then(|row| row.component.as_deref()),
            incident.and_then(|row| row.component.as_deref()),
        ]),
        risk_level: first_route_value(&[
            risk_level,
            report.and_then(|row| row.risk_level.as_deref()),
            draft.and_then(|row| row.risk_level.as_deref()),
            incident.and_then(|row| row.risk_level.as_deref()),
        ]),
        confidence: first_route_value(&[
            confidence,
            report.and_then(|row| row.confidence.as_deref()),
            draft.and_then(|row| row.confidence.as_deref()),
            incident.and_then(|row| row.confidence.as_deref()),
        ]),
        expected_destination: first_route_value(&[
            expected_destination,
            report.and_then(|row| row.expected_destination.as_deref()),
            draft.and_then(|row| row.expected_destination.as_deref()),
            incident.and_then(|row| row.expected_destination.as_deref()),
        ]),
        project_id: first_route_value(&[
            project_id,
            report.and_then(|row| row.project_id.as_deref()),
            draft.and_then(|row| row.project_id.as_deref()),
        ]),
        log_source_id: first_route_value(&[
            log_source_id,
            report.and_then(|row| row.log_source_id.as_deref()),
            draft.and_then(|row| row.log_source_id.as_deref()),
        ]),
        route_tags: normalize_route_values(route_tags),
    }
}

pub fn build_route_preview(
    config: &BugMonitorConfig,
    destinations: &[BugMonitorDestinationConfig],
    readiness: &[BugMonitorDestinationReadiness],
    context: &BugMonitorRouteContext,
    requested_destination_ids: &[String],
) -> BugMonitorRoutePreviewResponse {
    let default_destination_ids = config.effective_default_destination_ids();
    let matches = if requested_destination_ids.is_empty() {
        route_preview_matches(config, context, &default_destination_ids, destinations)
    } else {
        vec![BugMonitorRoutePreviewMatch {
            route_id: None,
            route_name: None,
            destination_ids: trim_route_values(requested_destination_ids),
            approval_required: route_preview_approval_required(
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
            push_unique(&mut effective_destination_ids, destination_id);
        }
    }
    let selected_destinations = selected_destinations(destinations, &effective_destination_ids);
    let selected_readiness = selected_readiness(readiness, &effective_destination_ids);
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
                blocked_reasons.push(format!(
                    "Destination `{destination_id}` is not ready: {detail}"
                ));
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

pub async fn publish_draft(
    state: &AppState,
    request: BugMonitorPublishRequest,
) -> anyhow::Result<bug_monitor_github::PublishOutcome> {
    let mut draft = state
        .get_bug_monitor_draft(&request.draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;
    let incident = match request.incident_id.as_deref() {
        Some(incident_id) => state.get_bug_monitor_incident(incident_id).await,
        None => None,
    };
    let status = state.bug_monitor_status_snapshot().await;
    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        Some(&draft),
        incident.as_ref(),
    );
    let requested_destination_ids = trim_route_values(&request.destination_ids);
    let preview = build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &context,
        &requested_destination_ids,
    );

    validate_publish_plan(&status.config, &preview, request.mode)?;
    if request.mode != bug_monitor_github::PublishMode::RecheckOnly
        && preview.approval_required
        && !draft.status.eq_ignore_ascii_case("denied")
        && !draft_satisfies_route_approval(&draft)
    {
        draft.status = "approval_required".to_string();
        draft.github_status = Some("approval_required".to_string());
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(bug_monitor_github::PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }

    bug_monitor_github::publish_draft(
        state,
        &request.draft_id,
        request.incident_id.as_deref(),
        request.mode,
    )
    .await
    .context("publish Bug Monitor draft through destination router")
}

pub async fn record_publish_failure(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    incident_id: Option<&str>,
    operation: &str,
    evidence_digest: Option<&str>,
    error: &str,
) -> anyhow::Result<BugMonitorPostRecord> {
    bug_monitor_github::record_post_failure(
        state,
        draft,
        incident_id,
        operation,
        evidence_digest,
        error,
    )
    .await
}

pub fn is_high_risk(value: Option<&str>) -> bool {
    matches!(
        normalize_route_value(value).unwrap_or_default().as_str(),
        "high" | "critical" | "urgent" | "severe"
    )
}

fn validate_publish_plan(
    config: &BugMonitorConfig,
    preview: &BugMonitorRoutePreviewResponse,
    mode: bug_monitor_github::PublishMode,
) -> anyhow::Result<()> {
    if preview.effective_destination_ids.is_empty() {
        anyhow::bail!("Bug Monitor destination router found no destination");
    }
    for blocked in &preview.blocked_reasons {
        if blocked.contains("not configured") {
            anyhow::bail!("{blocked}");
        }
    }
    if preview.destinations.len() != 1 {
        anyhow::bail!(
            "Bug Monitor destination router supports one legacy GitHub destination in this phase"
        );
    }
    let destination = &preview.destinations[0];
    if !destination.enabled {
        anyhow::bail!("Destination `{}` is disabled", destination.destination_id);
    }
    if destination.kind != BugMonitorDestinationKind::GithubIssue {
        anyhow::bail!(
            "Destination `{}` uses {:?}, which is not available in this phase",
            destination.destination_id,
            destination.kind
        );
    }
    if destination.destination_id != BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID {
        anyhow::bail!(
            "GitHub destination `{}` is configured but only the legacy GitHub destination can publish before GitHub adapter parity lands",
            destination.destination_id
        );
    }
    if config.safety_defaults.block_unready_destinations
        && mode != bug_monitor_github::PublishMode::RecheckOnly
        && preview.blocked
    {
        anyhow::bail!("{}", preview.blocked_reasons.join("; "));
    }
    Ok(())
}

fn draft_satisfies_route_approval(draft: &BugMonitorDraftRecord) -> bool {
    draft.status.eq_ignore_ascii_case("draft_ready") && draft.approval_granted_at_ms.is_some()
}

fn route_preview_matches(
    config: &BugMonitorConfig,
    context: &BugMonitorRouteContext,
    default_destination_ids: &[String],
    destinations: &[BugMonitorDestinationConfig],
) -> Vec<BugMonitorRoutePreviewMatch> {
    let mut routes = config
        .routes
        .iter()
        .filter(|route| route.enabled && route_matches(route, context))
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
                trim_route_values(&route.destination_ids)
            };
            BugMonitorRoutePreviewMatch {
                route_id: Some(route.route_id.clone()),
                route_name: Some(route.name.clone()),
                approval_required: route_preview_approval_required(
                    Some(route),
                    context,
                    config,
                    destinations,
                    &destination_ids,
                ),
                destination_ids,
                reason: Some(route_match_reason(route)),
            }
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches.push(BugMonitorRoutePreviewMatch {
            route_id: None,
            route_name: None,
            destination_ids: default_destination_ids.to_vec(),
            approval_required: route_preview_approval_required(
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

fn route_matches(route: &BugMonitorRouteConfig, context: &BugMonitorRouteContext) -> bool {
    route_value_matches(&route.match_event_types, context.event_type.as_deref())
        && route_value_matches(&route.match_sources, context.source.as_deref())
        && route_value_matches(&route.match_components, context.component.as_deref())
        && route_value_matches(&route.match_risk_levels, context.risk_level.as_deref())
        && route_value_matches(&route.match_confidence, context.confidence.as_deref())
        && route_value_matches(
            &route.match_expected_destinations,
            context.expected_destination.as_deref(),
        )
        && route_value_matches(&route.match_project_ids, context.project_id.as_deref())
        && route_value_matches(
            &route.match_log_source_ids,
            context.log_source_id.as_deref(),
        )
        && route_tags_match(&route.match_route_tags, &context.route_tags)
}

fn route_preview_approval_required(
    route: Option<&BugMonitorRouteConfig>,
    context: &BugMonitorRouteContext,
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
    let high_risk = is_high_risk(context.risk_level.as_deref());
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

fn route_match_reason(route: &BugMonitorRouteConfig) -> String {
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

fn route_value_matches(filters: &[String], candidate: Option<&str>) -> bool {
    if filters.is_empty() {
        return true;
    }
    let Some(candidate) = normalize_route_value(candidate) else {
        return false;
    };
    filters
        .iter()
        .filter_map(|value| normalize_route_value(Some(value)))
        .any(|value| value == candidate)
}

fn route_tags_match(filters: &[String], candidates: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let candidates = candidates
        .iter()
        .filter_map(|value| normalize_route_value(Some(value)))
        .collect::<BTreeSet<_>>();
    filters
        .iter()
        .filter_map(|value| normalize_route_value(Some(value)))
        .any(|value| candidates.contains(&value))
}

fn selected_destinations(
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

fn selected_readiness(
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

fn normalize_route_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(value) = normalize_route_value(Some(value)) {
            push_unique(&mut out, &value);
        }
    }
    out
}

fn trim_route_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() {
            push_unique(&mut out, value);
        }
    }
    out
}

fn normalize_route_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn first_route_value(values: &[Option<&str>]) -> Option<String> {
    values
        .iter()
        .find_map(|value| normalize_route_value(*value))
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}
