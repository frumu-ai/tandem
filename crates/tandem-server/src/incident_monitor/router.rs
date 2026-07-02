use std::collections::BTreeSet;

use anyhow::Context;
use serde_json::{json, Value};
use tandem_types::{EngineEvent, TenantContext};

use crate::{
    incident_monitor_github, incident_monitor_linear, incident_monitor_local, incident_monitor_mcp,
    incident_monitor_webhook, now_ms, AppState, IncidentMonitorApprovalPolicy,
    IncidentMonitorConfig, IncidentMonitorDestinationConfig, IncidentMonitorDestinationKind,
    IncidentMonitorDestinationReadiness, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord, IncidentMonitorRouteConfig, IncidentMonitorRoutePreviewMatch,
    IncidentMonitorRoutePreviewResponse, IncidentMonitorSourceReadiness, IncidentMonitorSubmission,
    INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID,
};

#[derive(Debug, Clone, Default)]
pub struct IncidentMonitorRouteContext {
    pub event_type: Option<String>,
    pub source: Option<String>,
    pub component: Option<String>,
    pub risk_level: Option<String>,
    pub risk_category: Option<String>,
    pub confidence: Option<String>,
    pub expected_destination: Option<String>,
    pub project_id: Option<String>,
    pub log_source_id: Option<String>,
    pub route_tags: Vec<String>,
    pub source_kind: Option<String>,
    pub tenant_id: Option<String>,
    pub workspace_id: Option<String>,
    pub event_schema_version: Option<String>,
    pub allowed_destination_ids: Vec<String>,
    pub destination_allowlist_enforced: bool,
    pub default_destination_ids: Vec<String>,
    pub source_approval_policy: Option<IncidentMonitorApprovalPolicy>,
    pub source_approval_policy_trusted: bool,
    pub source_binding_trusted: bool,
}

#[derive(Debug, Clone)]
pub struct IncidentMonitorPublishRequest {
    pub draft_id: String,
    pub incident_id: Option<String>,
    pub mode: incident_monitor_github::PublishMode,
    pub destination_ids: Vec<String>,
}

pub fn build_route_context(
    event_type: Option<&str>,
    source: Option<&str>,
    component: Option<&str>,
    risk_level: Option<&str>,
    risk_category: Option<&str>,
    confidence: Option<&str>,
    expected_destination: Option<&str>,
    project_id: Option<&str>,
    log_source_id: Option<&str>,
    route_tags: &[String],
    report: Option<&IncidentMonitorSubmission>,
    draft: Option<&IncidentMonitorDraftRecord>,
    incident: Option<&IncidentMonitorIncidentRecord>,
) -> IncidentMonitorRouteContext {
    let mut normalized_route_tags = normalize_route_values(route_tags);
    if let Some(report) = report {
        push_normalized_values(&mut normalized_route_tags, &report.route_tags);
    }
    if let Some(draft) = draft {
        push_normalized_values(&mut normalized_route_tags, &draft.route_tags);
    }
    if let Some(incident) = incident {
        push_normalized_values(&mut normalized_route_tags, &incident.route_tags);
    }
    let source_approval_policy = first_approval_policy(&[
        report.and_then(|row| row.source_approval_policy.as_ref()),
        draft.and_then(|row| row.source_approval_policy.as_ref()),
        incident.and_then(|row| row.source_approval_policy.as_ref()),
    ]);
    let has_only_unpersisted_report = report.is_some() && draft.is_none() && incident.is_none();
    let source_binding_trusted = source_approval_policy.is_some() || !has_only_unpersisted_report;

    let allowed_destination_ids = first_non_empty_destination_ids(&[
        report.map(|row| row.allowed_destination_ids.as_slice()),
        draft.map(|row| row.allowed_destination_ids.as_slice()),
        incident.map(|row| row.allowed_destination_ids.as_slice()),
    ]);

    IncidentMonitorRouteContext {
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
        risk_category: first_route_value(&[
            risk_category,
            report.and_then(|row| row.risk_category.as_deref()),
            draft.and_then(|row| row.risk_category.as_deref()),
            incident.and_then(|row| row.risk_category.as_deref()),
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
            incident.and_then(|row| row.project_id.as_deref()),
        ]),
        log_source_id: first_route_value(&[
            log_source_id,
            report.and_then(|row| row.log_source_id.as_deref()),
            draft.and_then(|row| row.log_source_id.as_deref()),
            incident.and_then(|row| row.log_source_id.as_deref()),
        ]),
        route_tags: normalized_route_tags,
        source_kind: first_route_value(&[
            report.and_then(|row| row.source_kind.as_ref().map(|kind| kind.as_str())),
            draft.and_then(|row| row.source_kind.as_ref().map(|kind| kind.as_str())),
            incident.and_then(|row| row.source_kind.as_ref().map(|kind| kind.as_str())),
        ]),
        tenant_id: first_route_value(&[
            report.and_then(|row| row.tenant_id.as_deref()),
            draft.and_then(|row| row.tenant_id.as_deref()),
            incident.and_then(|row| row.tenant_id.as_deref()),
        ]),
        workspace_id: first_route_value(&[
            report.and_then(|row| row.workspace_id.as_deref()),
            draft.and_then(|row| row.workspace_id.as_deref()),
            incident.and_then(|row| row.workspace_id.as_deref()),
        ]),
        event_schema_version: first_route_value(&[
            report.and_then(|row| row.event_schema_version.as_deref()),
            draft.and_then(|row| row.event_schema_version.as_deref()),
            incident.and_then(|row| row.event_schema_version.as_deref()),
        ]),
        destination_allowlist_enforced: !allowed_destination_ids.is_empty(),
        allowed_destination_ids,
        default_destination_ids: first_non_empty_destination_ids(&[
            report.map(|row| row.default_destination_ids.as_slice()),
            draft.map(|row| row.default_destination_ids.as_slice()),
            incident.map(|row| row.default_destination_ids.as_slice()),
        ]),
        source_approval_policy,
        source_approval_policy_trusted: false,
        source_binding_trusted,
    }
}

pub fn build_route_preview(
    config: &IncidentMonitorConfig,
    destinations: &[IncidentMonitorDestinationConfig],
    readiness: &[IncidentMonitorDestinationReadiness],
    source_readiness: &[IncidentMonitorSourceReadiness],
    context: &IncidentMonitorRouteContext,
    requested_destination_ids: &[String],
) -> IncidentMonitorRoutePreviewResponse {
    let context = enrich_route_context_from_sources(config, context);
    let default_destination_ids = if context.default_destination_ids.is_empty() {
        config.effective_default_destination_ids()
    } else {
        trim_route_values(&context.default_destination_ids)
    };
    let matches = if requested_destination_ids.is_empty() {
        route_preview_matches(config, &context, &default_destination_ids, destinations)
    } else {
        vec![IncidentMonitorRoutePreviewMatch {
            route_id: None,
            route_name: None,
            destination_ids: trim_route_values(requested_destination_ids),
            approval_required: route_preview_approval_required(
                None,
                &context,
                config,
                destinations,
                requested_destination_ids,
            ),
            reason: Some("requested_destination_override".to_string()),
        }]
    };
    // Route precedence: matches are sorted by priority (highest first), so the
    // top match is the winning route. Only its destinations are effective — we
    // must not union destinations across lower-priority routes, which previously
    // produced multiple effective destinations and hard-failed every publish for
    // any event that matched more than one route.
    let mut effective_destination_ids = Vec::new();
    if let Some(winning_match) = matches.first() {
        for destination_id in &winning_match.destination_ids {
            push_unique(&mut effective_destination_ids, destination_id);
        }
    }
    let selected_destinations = selected_destinations(destinations, &effective_destination_ids);
    let selected_readiness = selected_readiness(readiness, &effective_destination_ids);
    let selected_source_readiness = selected_source_readiness(source_readiness, &context);
    let source_readiness_warnings = selected_source_readiness
        .iter()
        .flat_map(|row| row.warnings.clone())
        .collect::<Vec<_>>();
    let mut blocked_reasons = Vec::new();
    if effective_destination_ids.is_empty() {
        blocked_reasons.push("No destination matched route preview".to_string());
    }
    for destination_id in &effective_destination_ids {
        if !destination_allowed_by_source(
            context.destination_allowlist_enforced,
            &context.allowed_destination_ids,
            destination_id,
        ) {
            blocked_reasons.push(format!(
                "Destination `{destination_id}` is not allowed by source binding"
            ));
            continue;
        }
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

    IncidentMonitorRoutePreviewResponse {
        matches,
        destinations: selected_destinations,
        readiness: selected_readiness,
        source_readiness: selected_source_readiness,
        source_readiness_warnings,
        default_destination_ids,
        effective_destination_ids,
        approval_required,
        blocked: !blocked_reasons.is_empty(),
        blocked_reasons,
    }
}

pub async fn publish_draft(
    state: &AppState,
    request: IncidentMonitorPublishRequest,
) -> anyhow::Result<incident_monitor_github::PublishOutcome> {
    let mut draft = state
        .get_incident_monitor_draft(&request.draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor draft not found"))?;
    let incident = match request.incident_id.as_deref() {
        Some(incident_id) => state.get_incident_monitor_incident(incident_id).await,
        None => None,
    };
    let status = state.incident_monitor_status_snapshot().await;
    let context = build_route_context(
        None,
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
    let context = enrich_route_context_from_sources(&status.config, &context);
    let requested_destination_ids = trim_route_values(&request.destination_ids);
    let preview = build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &status.source_readiness,
        &context,
        &requested_destination_ids,
    );
    let router_approval_required =
        route_publish_approval_required(&status.config, &preview, &context, &status.destinations);
    let audit_tenant_context = publish_audit_tenant_context(&context, &draft);

    let audit_payload = publish_audit_payload(
        &request,
        &draft,
        incident.as_ref(),
        &context,
        &preview,
        None,
        None,
        router_approval_required,
    );
    emit_publish_audit_event(
        state,
        "incident_monitor.publish.attempted",
        audit_payload.clone(),
        &audit_tenant_context,
    )
    .await;

    if let Err(error) = validate_publish_plan(&status.config, &preview, request.mode) {
        emit_publish_audit_event(
            state,
            "incident_monitor.publish.failed",
            publish_audit_payload(
                &request,
                &draft,
                incident.as_ref(),
                &context,
                &preview,
                Some("validation_failed"),
                Some(&crate::truncate_text(&error.to_string(), 500)),
                router_approval_required,
            ),
            &audit_tenant_context,
        )
        .await;
        return Err(error);
    }
    let selected_destination = selected_publish_destination(&preview)?;
    if request.mode != incident_monitor_github::PublishMode::RecheckOnly
        && router_approval_required
        && !draft.status.eq_ignore_ascii_case("denied")
        && !draft_satisfies_route_approval(&draft)
    {
        draft.status = "approval_required".to_string();
        draft.github_status = Some("approval_required".to_string());
        let draft = state.put_incident_monitor_draft(draft).await?;
        emit_publish_audit_event(
            state,
            "incident_monitor.publish.completed",
            publish_audit_payload(
                &request,
                &draft,
                incident.as_ref(),
                &context,
                &preview,
                Some("approval_required"),
                None,
                router_approval_required,
            ),
            &audit_tenant_context,
        )
        .await;
        return Ok(incident_monitor_github::PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }

    let outcome = match &selected_destination.kind {
        IncidentMonitorDestinationKind::GithubIssue => {
            incident_monitor_github::publish_draft(
                state,
                &request.draft_id,
                request.incident_id.as_deref(),
                request.mode,
                github_destination_context(&preview)?,
            )
            .await
        }
        IncidentMonitorDestinationKind::LinearIssue => {
            incident_monitor_linear::publish_draft(
                state,
                &request.draft_id,
                request.incident_id.as_deref(),
                request.mode,
                linear_destination_context(&preview)?,
            )
            .await
        }
        IncidentMonitorDestinationKind::Webhook => {
            incident_monitor_webhook::publish_draft(
                state,
                &request.draft_id,
                request.incident_id.as_deref(),
                request.mode,
                webhook_destination_context(&preview)?,
            )
            .await
        }
        IncidentMonitorDestinationKind::Telemetry
        | IncidentMonitorDestinationKind::InternalMemory => {
            incident_monitor_local::publish_draft(
                state,
                &request.draft_id,
                request.incident_id.as_deref(),
                request.mode,
                local_destination_context(&preview)?,
            )
            .await
        }
        IncidentMonitorDestinationKind::McpTool => {
            incident_monitor_mcp::publish_draft(
                state,
                &request.draft_id,
                request.incident_id.as_deref(),
                request.mode,
                mcp_tool_destination_context(&preview)?,
            )
            .await
        }
    };

    match outcome {
        Ok(outcome) => {
            emit_publish_audit_event(
                state,
                "incident_monitor.publish.completed",
                publish_audit_payload(
                    &request,
                    &outcome.draft,
                    incident.as_ref(),
                    &context,
                    &preview,
                    Some(outcome.action.as_str()),
                    None,
                    router_approval_required,
                ),
                &audit_tenant_context,
            )
            .await;
            Ok(outcome)
        }
        Err(error) => {
            emit_publish_audit_event(
                state,
                "incident_monitor.publish.failed",
                publish_audit_payload(
                    &request,
                    &draft,
                    incident.as_ref(),
                    &context,
                    &preview,
                    Some("adapter_failed"),
                    Some(&crate::truncate_text(&format!("{error:#}"), 500)),
                    router_approval_required,
                ),
                &audit_tenant_context,
            )
            .await;
            Err(error).context("publish Incident Monitor draft through destination router")
        }
    }
}

pub async fn record_publish_failure(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    incident_id: Option<&str>,
    operation: &str,
    evidence_digest: Option<&str>,
    error: &str,
) -> anyhow::Result<IncidentMonitorPostRecord> {
    incident_monitor_github::record_post_failure(
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
    config: &IncidentMonitorConfig,
    preview: &IncidentMonitorRoutePreviewResponse,
    mode: incident_monitor_github::PublishMode,
) -> anyhow::Result<()> {
    if preview.effective_destination_ids.is_empty() {
        anyhow::bail!("Incident Monitor destination router found no destination");
    }
    for blocked in &preview.blocked_reasons {
        if blocked.contains("not configured") || blocked.contains("not allowed by source binding") {
            anyhow::bail!("{blocked}");
        }
    }
    if preview.destinations.len() != 1 {
        anyhow::bail!(
            "Incident Monitor destination router supports one issue destination per publish in this phase"
        );
    }
    let destination = &preview.destinations[0];
    if !destination.enabled {
        anyhow::bail!("Destination `{}` is disabled", destination.destination_id);
    }
    if !matches!(
        destination.kind,
        IncidentMonitorDestinationKind::GithubIssue
            | IncidentMonitorDestinationKind::LinearIssue
            | IncidentMonitorDestinationKind::Webhook
            | IncidentMonitorDestinationKind::Telemetry
            | IncidentMonitorDestinationKind::McpTool
            | IncidentMonitorDestinationKind::InternalMemory
    ) {
        anyhow::bail!(
            "Destination `{}` uses {:?}, which is not available in this phase",
            destination.destination_id,
            destination.kind
        );
    }
    if config.safety_defaults.block_unready_destinations
        && mode != incident_monitor_github::PublishMode::RecheckOnly
        && preview.blocked
    {
        anyhow::bail!("{}", preview.blocked_reasons.join("; "));
    }
    Ok(())
}

fn selected_publish_destination(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<&IncidentMonitorDestinationConfig> {
    preview
        .destinations
        .first()
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor destination router found no destination"))
}

fn publish_audit_payload(
    request: &IncidentMonitorPublishRequest,
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    context: &IncidentMonitorRouteContext,
    preview: &IncidentMonitorRoutePreviewResponse,
    action: Option<&str>,
    detail: Option<&str>,
    approval_required: bool,
) -> Value {
    let selected_destination = preview.destinations.first();
    json!({
        "draft_id": request.draft_id.as_str(),
        "incident_id": request.incident_id.as_deref().or_else(|| incident.map(|row| row.incident_id.as_str())),
        "mode": format!("{:?}", &request.mode),
        "requested_destination_ids": &request.destination_ids,
        "effective_destination_ids": &preview.effective_destination_ids,
        "selected_destination_id": selected_destination.map(|row| row.destination_id.as_str()),
        "selected_destination_kind": selected_destination.map(|row| format!("{:?}", &row.kind)),
        "route_ids": preview.matches.iter().filter_map(|row| row.route_id.clone()).collect::<Vec<_>>(),
        "blocked": preview.blocked,
        "blocked_reasons": &preview.blocked_reasons,
        "approval_required": approval_required,
        "action": action,
        "detail": detail,
        "repo": draft.repo.as_str(),
        "fingerprint": draft.fingerprint.as_str(),
        "status": draft.status.as_str(),
        "risk_level": context.risk_level.as_deref().or(draft.risk_level.as_deref()),
        "risk_category": context.risk_category.as_deref().or(draft.risk_category.as_deref()),
        "confidence": context.confidence.as_deref().or(draft.confidence.as_deref()),
        "project_id": context.project_id.as_deref().or(draft.project_id.as_deref()),
        "log_source_id": context.log_source_id.as_deref().or(draft.log_source_id.as_deref()),
        "source_kind": context.source_kind.as_deref().or_else(|| {
            draft
                .source_kind
                .as_ref()
                .map(|source_kind| source_kind.as_str())
        }),
        "tenant_id": context.tenant_id.as_deref().or(draft.tenant_id.as_deref()),
        "workspace_id": context.workspace_id.as_deref().or(draft.workspace_id.as_deref()),
        "event_schema_version": context.event_schema_version.as_deref().or(draft.event_schema_version.as_deref()),
        "source_allowlist_enforced": context.destination_allowlist_enforced,
    })
}

fn publish_audit_tenant_context(
    context: &IncidentMonitorRouteContext,
    draft: &IncidentMonitorDraftRecord,
) -> TenantContext {
    let tenant_id = context
        .tenant_id
        .as_deref()
        .or(draft.tenant_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let workspace_id = context
        .workspace_id
        .as_deref()
        .or(draft.workspace_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (tenant_id, workspace_id) {
        (Some(tenant_id), Some(workspace_id)) => {
            TenantContext::explicit(tenant_id, workspace_id, None)
        }
        _ => TenantContext::local_implicit(),
    }
}

async fn emit_publish_audit_event(
    state: &AppState,
    event_type: &'static str,
    payload: Value,
    tenant_context: &TenantContext,
) {
    state
        .event_bus
        .publish(EngineEvent::new(event_type, payload.clone()));
    let _ = crate::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        None,
        payload,
    )
    .await;
}

fn github_destination_context(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<incident_monitor_github::GithubDestinationContext> {
    let destination = preview.destinations.first().ok_or_else(|| {
        anyhow::anyhow!("Incident Monitor destination router found no destination")
    })?;
    let preview_match = preview.matches.iter().find(|preview_match| {
        preview_match
            .destination_ids
            .iter()
            .any(|destination_id| destination_id == &destination.destination_id)
    });
    Ok(incident_monitor_github::GithubDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: preview_match.and_then(|row| row.route_id.clone()),
        route_match_reason: preview_match
            .and_then(|row| row.reason.clone())
            .or_else(|| Some("destination_router".to_string())),
        repo: destination.repo.clone(),
        mcp_server: destination.mcp_server.clone(),
    })
}

fn linear_destination_context(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<incident_monitor_linear::LinearDestinationContext> {
    let destination = selected_publish_destination(preview)?;
    let preview_match = preview.matches.iter().find(|preview_match| {
        preview_match
            .destination_ids
            .iter()
            .any(|destination_id| destination_id == &destination.destination_id)
    });
    Ok(incident_monitor_linear::LinearDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: preview_match.and_then(|row| row.route_id.clone()),
        route_match_reason: preview_match
            .and_then(|row| row.reason.clone())
            .or_else(|| Some("destination_router".to_string())),
        mcp_server: destination.mcp_server.clone(),
        linear_team: destination.linear_team.clone(),
        linear_project: destination.linear_project.clone(),
    })
}

fn webhook_destination_context(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<incident_monitor_webhook::WebhookDestinationContext> {
    let destination = selected_publish_destination(preview)?;
    let preview_match = preview.matches.iter().find(|preview_match| {
        preview_match
            .destination_ids
            .iter()
            .any(|destination_id| destination_id == &destination.destination_id)
    });
    Ok(incident_monitor_webhook::WebhookDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: preview_match.and_then(|row| row.route_id.clone()),
        route_match_reason: preview_match
            .and_then(|row| row.reason.clone())
            .or_else(|| Some("destination_router".to_string())),
        webhook_url: destination.webhook_url.clone(),
        webhook_secret_ref: destination.webhook_secret_ref.clone(),
        config: destination.config.clone(),
    })
}

fn local_destination_context(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<incident_monitor_local::LocalDestinationContext> {
    let destination = selected_publish_destination(preview)?;
    let preview_match = preview.matches.iter().find(|preview_match| {
        preview_match
            .destination_ids
            .iter()
            .any(|destination_id| destination_id == &destination.destination_id)
    });
    Ok(incident_monitor_local::LocalDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: preview_match.and_then(|row| row.route_id.clone()),
        route_match_reason: preview_match
            .and_then(|row| row.reason.clone())
            .or_else(|| Some("destination_router".to_string())),
        kind: destination.kind.clone(),
        telemetry_path: destination.telemetry_path.clone(),
        memory_category: destination.memory_category.clone(),
        config: destination.config.clone(),
    })
}

fn mcp_tool_destination_context(
    preview: &IncidentMonitorRoutePreviewResponse,
) -> anyhow::Result<incident_monitor_mcp::McpToolDestinationContext> {
    let destination = selected_publish_destination(preview)?;
    let preview_match = preview.matches.iter().find(|preview_match| {
        preview_match
            .destination_ids
            .iter()
            .any(|destination_id| destination_id == &destination.destination_id)
    });
    Ok(incident_monitor_mcp::McpToolDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: preview_match.and_then(|row| row.route_id.clone()),
        route_match_reason: preview_match
            .and_then(|row| row.reason.clone())
            .or_else(|| Some("destination_router".to_string())),
        mcp_server: destination.mcp_server.clone(),
        mcp_tool: destination.mcp_tool.clone(),
        config: destination.config.clone(),
    })
}

fn draft_satisfies_route_approval(draft: &IncidentMonitorDraftRecord) -> bool {
    draft.approval_granted_at_ms.is_some()
}

fn route_publish_approval_required(
    config: &IncidentMonitorConfig,
    preview: &IncidentMonitorRoutePreviewResponse,
    context: &IncidentMonitorRouteContext,
    destinations: &[IncidentMonitorDestinationConfig],
) -> bool {
    preview.matches.iter().any(|preview_match| {
        let route = preview_match.route_id.as_deref().and_then(|route_id| {
            config
                .routes
                .iter()
                .find(|route| route.route_id == route_id)
        });
        route_publish_match_approval_required(
            config,
            route,
            context,
            destinations,
            &preview_match.destination_ids,
        )
    })
}

fn route_publish_match_approval_required(
    config: &IncidentMonitorConfig,
    route: Option<&IncidentMonitorRouteConfig>,
    context: &IncidentMonitorRouteContext,
    destinations: &[IncidentMonitorDestinationConfig],
    destination_ids: &[String],
) -> bool {
    let destination_requires_approval = destination_ids.iter().any(|destination_id| {
        destinations
            .iter()
            .find(|destination| destination.destination_id == *destination_id)
            .map(|destination| {
                destination.require_approval
                    && !is_synthesized_legacy_github_destination(config, destination)
            })
            .unwrap_or(false)
    });
    let high_risk = is_high_risk(context.risk_level.as_deref());
    match explicit_approval_policy(route, context) {
        Some(IncidentMonitorApprovalPolicy::Always) => true,
        Some(IncidentMonitorApprovalPolicy::Never) => false,
        Some(IncidentMonitorApprovalPolicy::HighRisk) => destination_requires_approval || high_risk,
        Some(IncidentMonitorApprovalPolicy::Inherit) | None => {
            preview_inherited_approval_required(config, context, destination_requires_approval)
        }
    }
}

fn explicit_approval_policy<'a>(
    route: Option<&'a IncidentMonitorRouteConfig>,
    context: &'a IncidentMonitorRouteContext,
) -> Option<&'a IncidentMonitorApprovalPolicy> {
    if let Some(policy) = route
        .map(|row| &row.approval_policy)
        .filter(|policy| **policy != IncidentMonitorApprovalPolicy::Inherit)
    {
        return Some(policy);
    }
    context
        .source_approval_policy
        .as_ref()
        .filter(|_| context.source_approval_policy_trusted)
        .filter(|policy| **policy != IncidentMonitorApprovalPolicy::Inherit)
}

fn preview_inherited_approval_required(
    config: &IncidentMonitorConfig,
    context: &IncidentMonitorRouteContext,
    destination_requires_approval: bool,
) -> bool {
    let high_risk = is_high_risk(context.risk_level.as_deref());
    config.require_approval_for_new_issues
        || destination_requires_approval
        || (config.safety_defaults.require_approval_for_high_risk && high_risk)
}

fn approval_policy_requires(
    policy: &IncidentMonitorApprovalPolicy,
    context: &IncidentMonitorRouteContext,
    destination_requires_approval: bool,
) -> bool {
    let high_risk = is_high_risk(context.risk_level.as_deref());
    match policy {
        IncidentMonitorApprovalPolicy::Always => true,
        IncidentMonitorApprovalPolicy::Never => false,
        IncidentMonitorApprovalPolicy::HighRisk => destination_requires_approval || high_risk,
        IncidentMonitorApprovalPolicy::Inherit => destination_requires_approval,
    }
}

fn is_synthesized_legacy_github_destination(
    config: &IncidentMonitorConfig,
    destination: &IncidentMonitorDestinationConfig,
) -> bool {
    config.destinations.is_empty()
        && destination.destination_id == INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID
        && destination.kind == IncidentMonitorDestinationKind::GithubIssue
}

fn enrich_route_context_from_sources(
    config: &IncidentMonitorConfig,
    context: &IncidentMonitorRouteContext,
) -> IncidentMonitorRouteContext {
    let mut out = context.clone();
    let Some(project_id) = out.project_id.as_deref() else {
        return out;
    };
    let Some(project) = config
        .monitored_projects
        .iter()
        .find(|project| normalized_equals(&project.project_id, project_id))
    else {
        return out;
    };
    let source = out.log_source_id.as_deref().and_then(|source_id| {
        project
            .log_sources
            .iter()
            .find(|source| normalized_equals(&source.source_id, source_id))
    });
    let binding = project.source_binding(source);

    if !out.source_binding_trusted {
        out.project_id = None;
        out.log_source_id = None;
        out.source_kind = None;
        out.tenant_id = None;
        out.workspace_id = None;
        out.event_schema_version = None;
        out.route_tags.clear();
        out.allowed_destination_ids.clear();
        out.destination_allowlist_enforced = false;
        out.default_destination_ids.clear();
        out.source_approval_policy = None;
        out.source_approval_policy_trusted = false;
        return out;
    }

    out.source_kind = Some(binding.source_kind.as_str().to_string());
    out.tenant_id = binding
        .tenant_id
        .as_deref()
        .and_then(|value| normalize_route_value(Some(value)));
    out.workspace_id = binding
        .workspace_id
        .as_deref()
        .and_then(|value| normalize_route_value(Some(value)));
    out.event_schema_version = binding
        .event_schema_version
        .as_deref()
        .and_then(|value| normalize_route_value(Some(value)));
    push_normalized_values(&mut out.route_tags, &binding.default_route_tags);

    let existing_allowed_destination_ids = std::mem::take(&mut out.allowed_destination_ids);
    let existing_allowlist_enforced = out.destination_allowlist_enforced;
    let binding_allowlist_enforced = !normalize_route_values(&project.allowed_destination_ids)
        .is_empty()
        || source.is_some_and(|source| {
            !normalize_route_values(&source.allowed_destination_ids).is_empty()
        });
    out.allowed_destination_ids = match (existing_allowlist_enforced, binding_allowlist_enforced) {
        (false, false) => Vec::new(),
        (false, true) => binding.allowed_destination_ids.clone(),
        (true, false) => existing_allowed_destination_ids,
        (true, true) => intersect_destination_ids(
            &existing_allowed_destination_ids,
            &binding.allowed_destination_ids,
        ),
    };
    out.destination_allowlist_enforced = existing_allowlist_enforced || binding_allowlist_enforced;
    out.default_destination_ids = binding.default_destination_ids.clone();
    let existing_policy_matches_binding = out
        .source_approval_policy
        .as_ref()
        .is_some_and(|policy| policy == &binding.approval_policy);
    // Source IDs can fail closed with Always, but downgrading policies must come
    // from a trusted intake/log-watcher path that persisted the exact binding.
    let trusted_source_policy = existing_policy_matches_binding
        || binding.approval_policy == IncidentMonitorApprovalPolicy::Always;
    if !trusted_source_policy {
        out.source_approval_policy = None;
        out.source_approval_policy_trusted = false;
    } else {
        out.source_approval_policy = Some(binding.approval_policy);
        out.source_approval_policy_trusted = true;
    }
    out
}

fn normalized_equals(left: &str, right: &str) -> bool {
    normalize_route_value(Some(left)) == normalize_route_value(Some(right))
}

fn intersect_destination_ids(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|destination_id| right.iter().any(|allowed| allowed == *destination_id))
        .cloned()
        .collect()
}

fn destination_allowed_by_source(
    allowlist_enforced: bool,
    allowed_destination_ids: &[String],
    destination_id: &str,
) -> bool {
    !allowlist_enforced
        || allowed_destination_ids
            .iter()
            .any(|allowed| allowed == destination_id)
}

fn route_preview_matches(
    config: &IncidentMonitorConfig,
    context: &IncidentMonitorRouteContext,
    default_destination_ids: &[String],
    destinations: &[IncidentMonitorDestinationConfig],
) -> Vec<IncidentMonitorRoutePreviewMatch> {
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
            IncidentMonitorRoutePreviewMatch {
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
        matches.push(IncidentMonitorRoutePreviewMatch {
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

fn route_matches(
    route: &IncidentMonitorRouteConfig,
    context: &IncidentMonitorRouteContext,
) -> bool {
    route_value_matches(&route.match_event_types, context.event_type.as_deref())
        && route_value_matches(&route.match_sources, context.source.as_deref())
        && route_value_matches(&route.match_components, context.component.as_deref())
        && route_value_matches(&route.match_risk_levels, context.risk_level.as_deref())
        && route_value_matches(
            &route.match_risk_categories,
            context.risk_category.as_deref(),
        )
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
        && route_value_matches(&route.match_source_kinds, context.source_kind.as_deref())
        && route_value_matches(&route.match_tenant_ids, context.tenant_id.as_deref())
        && route_value_matches(&route.match_workspace_ids, context.workspace_id.as_deref())
        && route_value_matches(
            &route.match_event_schema_versions,
            context.event_schema_version.as_deref(),
        )
        && route_tags_match(&route.match_route_tags, &context.route_tags)
}

fn route_preview_approval_required(
    route: Option<&IncidentMonitorRouteConfig>,
    context: &IncidentMonitorRouteContext,
    config: &IncidentMonitorConfig,
    destinations: &[IncidentMonitorDestinationConfig],
    destination_ids: &[String],
) -> bool {
    let destination_requires_approval = destination_ids.iter().any(|destination_id| {
        destinations
            .iter()
            .find(|destination| destination.destination_id == *destination_id)
            .map(|destination| destination.require_approval)
            .unwrap_or(false)
    });
    if let Some(policy) = explicit_approval_policy(route, context) {
        return approval_policy_requires(policy, context, destination_requires_approval);
    }
    preview_inherited_approval_required(config, context, destination_requires_approval)
}

fn route_match_reason(route: &IncidentMonitorRouteConfig) -> String {
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
    if !route.match_risk_categories.is_empty() {
        parts.push("risk_category");
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
    if !route.match_source_kinds.is_empty() {
        parts.push("source_kind");
    }
    if !route.match_tenant_ids.is_empty() {
        parts.push("tenant_id");
    }
    if !route.match_workspace_ids.is_empty() {
        parts.push("workspace_id");
    }
    if !route.match_event_schema_versions.is_empty() {
        parts.push("event_schema_version");
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
    destinations: &[IncidentMonitorDestinationConfig],
    destination_ids: &[String],
) -> Vec<IncidentMonitorDestinationConfig> {
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
    readiness: &[IncidentMonitorDestinationReadiness],
    destination_ids: &[String],
) -> Vec<IncidentMonitorDestinationReadiness> {
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

fn selected_source_readiness(
    readiness: &[IncidentMonitorSourceReadiness],
    context: &IncidentMonitorRouteContext,
) -> Vec<IncidentMonitorSourceReadiness> {
    let Some(project_id) = context.project_id.as_deref() else {
        return Vec::new();
    };
    let log_source_id = context.log_source_id.as_deref();

    readiness
        .iter()
        .filter(|row| {
            normalized_equals(&row.project_id, project_id)
                && match log_source_id {
                    Some(source_id) => row
                        .source_id
                        .as_deref()
                        .is_some_and(|row_source_id| normalized_equals(row_source_id, source_id)),
                    None => row.source_id.is_none(),
                }
        })
        .cloned()
        .collect()
}

fn normalize_route_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    push_normalized_values(&mut out, values);
    out
}

fn push_normalized_values(out: &mut Vec<String>, values: &[String]) {
    for value in values {
        if let Some(value) = normalize_route_value(Some(value)) {
            push_unique(out, &value);
        }
    }
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

fn first_non_empty_destination_ids(values: &[Option<&[String]>]) -> Vec<String> {
    values
        .iter()
        .find_map(|value| {
            let value = (*value)?;
            let trimmed = trim_route_values(value);
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_default()
}

fn first_approval_policy(
    values: &[Option<&IncidentMonitorApprovalPolicy>],
) -> Option<IncidentMonitorApprovalPolicy> {
    values.iter().find_map(|value| value.map(Clone::clone))
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IncidentMonitorLogSource, IncidentMonitorMonitoredProject, IncidentMonitorSourceKind,
    };

    fn source_bound_config() -> IncidentMonitorConfig {
        IncidentMonitorConfig {
            monitored_projects: vec![IncidentMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: "/tmp/payments".to_string(),
                source_kind: IncidentMonitorSourceKind::ExternalApp,
                allowed_destination_ids: vec!["triage".to_string(), "pager".to_string()],
                default_destination_ids: vec!["triage".to_string()],
                default_route_tags: vec!["payments".to_string()],
                tenant_id: Some("tenant-payments".to_string()),
                workspace_id: Some("workspace-project".to_string()),
                event_schema_version: Some("project-v1".to_string()),
                log_sources: vec![IncidentMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    source_kind: Some(IncidentMonitorSourceKind::Ci),
                    allowed_destination_ids: vec!["pager".to_string()],
                    default_destination_ids: vec!["pager".to_string()],
                    default_route_tags: vec!["ci".to_string()],
                    tenant_id: Some("tenant-ci".to_string()),
                    workspace_id: Some("workspace-ci".to_string()),
                    event_schema_version: Some("ci-v1".to_string()),
                    approval_policy: IncidentMonitorApprovalPolicy::Never,
                    ..IncidentMonitorLogSource::default()
                }],
                ..IncidentMonitorMonitoredProject::default()
            }],
            ..IncidentMonitorConfig::default()
        }
    }

    #[test]
    fn untrusted_report_source_ids_do_not_inherit_source_binding_routes() {
        let report = IncidentMonitorSubmission {
            project_id: Some("payments".to_string()),
            log_source_id: Some("ci".to_string()),
            source_kind: Some(IncidentMonitorSourceKind::ExternalApp),
            route_tags: vec!["forged".to_string()],
            allowed_destination_ids: vec!["pager".to_string()],
            default_destination_ids: vec!["pager".to_string()],
            tenant_id: Some("tenant-forged".to_string()),
            workspace_id: Some("workspace-forged".to_string()),
            event_schema_version: Some("forged-v1".to_string()),
            source_approval_policy: None,
            ..IncidentMonitorSubmission::default()
        };

        let context = build_route_context(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            Some(&report),
            None,
            None,
        );
        let enriched = enrich_route_context_from_sources(&source_bound_config(), &context);

        assert_eq!(enriched.project_id, None);
        assert_eq!(enriched.log_source_id, None);
        assert_eq!(enriched.source_kind, None);
        assert_eq!(enriched.tenant_id, None);
        assert_eq!(enriched.workspace_id, None);
        assert_eq!(enriched.event_schema_version, None);
        assert!(enriched.route_tags.is_empty());
        assert!(enriched.allowed_destination_ids.is_empty());
        assert!(enriched.default_destination_ids.is_empty());
        assert!(!enriched.source_approval_policy_trusted);
    }

    #[test]
    fn legacy_persisted_draft_source_ids_inherit_fail_closed_source_binding() {
        let mut config = source_bound_config();
        config.monitored_projects[0].log_sources[0].approval_policy =
            IncidentMonitorApprovalPolicy::Always;
        let draft = IncidentMonitorDraftRecord {
            project_id: Some("payments".to_string()),
            log_source_id: Some("ci".to_string()),
            source_approval_policy: None,
            ..IncidentMonitorDraftRecord::default()
        };

        let context = build_route_context(
            None,
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
            None,
        );
        let enriched = enrich_route_context_from_sources(&config, &context);

        assert_eq!(enriched.project_id.as_deref(), Some("payments"));
        assert_eq!(enriched.log_source_id.as_deref(), Some("ci"));
        assert_eq!(enriched.source_kind.as_deref(), Some("ci"));
        assert_eq!(enriched.default_destination_ids, vec!["pager"]);
        assert_eq!(
            enriched.source_approval_policy,
            Some(IncidentMonitorApprovalPolicy::Always)
        );
        assert!(enriched.source_approval_policy_trusted);
    }

    #[test]
    fn trusted_report_source_binding_overrides_forged_source_fields() {
        let report = IncidentMonitorSubmission {
            project_id: Some("payments".to_string()),
            log_source_id: Some("ci".to_string()),
            source_kind: Some(IncidentMonitorSourceKind::ExternalApp),
            route_tags: vec!["candidate".to_string()],
            allowed_destination_ids: vec!["triage".to_string(), "pager".to_string()],
            default_destination_ids: vec!["triage".to_string()],
            tenant_id: Some("tenant-forged".to_string()),
            workspace_id: Some("workspace-forged".to_string()),
            event_schema_version: Some("forged-v1".to_string()),
            source_approval_policy: Some(IncidentMonitorApprovalPolicy::Never),
            ..IncidentMonitorSubmission::default()
        };

        let context = build_route_context(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            Some(&report),
            None,
            None,
        );
        let enriched = enrich_route_context_from_sources(&source_bound_config(), &context);

        assert_eq!(enriched.project_id.as_deref(), Some("payments"));
        assert_eq!(enriched.log_source_id.as_deref(), Some("ci"));
        assert_eq!(enriched.source_kind.as_deref(), Some("ci"));
        assert_eq!(enriched.tenant_id.as_deref(), Some("tenant-ci"));
        assert_eq!(enriched.workspace_id.as_deref(), Some("workspace-ci"));
        assert_eq!(enriched.event_schema_version.as_deref(), Some("ci-v1"));
        assert_eq!(enriched.route_tags, vec!["candidate", "payments", "ci"]);
        assert_eq!(enriched.allowed_destination_ids, vec!["pager"]);
        assert_eq!(enriched.default_destination_ids, vec!["pager"]);
        assert_eq!(
            enriched.source_approval_policy,
            Some(IncidentMonitorApprovalPolicy::Never)
        );
        assert!(enriched.source_approval_policy_trusted);
    }

    #[test]
    fn publish_audit_tenant_context_uses_source_bound_draft_scope() {
        let draft = IncidentMonitorDraftRecord {
            tenant_id: Some("tenant-draft".to_string()),
            workspace_id: Some("workspace-draft".to_string()),
            ..IncidentMonitorDraftRecord::default()
        };
        let context = IncidentMonitorRouteContext {
            tenant_id: Some("tenant-ci".to_string()),
            workspace_id: Some("workspace-ci".to_string()),
            ..IncidentMonitorRouteContext::default()
        };

        let tenant_context = publish_audit_tenant_context(&context, &draft);

        assert_eq!(tenant_context.org_id, "tenant-ci");
        assert_eq!(tenant_context.workspace_id, "workspace-ci");
        assert_eq!(tenant_context.actor_id, None);
        assert_eq!(tenant_context.source, tandem_types::TenantSource::Explicit);
    }

    #[test]
    fn publish_audit_tenant_context_falls_back_to_local_without_complete_scope() {
        let draft = IncidentMonitorDraftRecord {
            tenant_id: Some("tenant-draft".to_string()),
            ..IncidentMonitorDraftRecord::default()
        };
        let context = IncidentMonitorRouteContext::default();

        let tenant_context = publish_audit_tenant_context(&context, &draft);

        assert!(tenant_context.is_local_implicit());
    }

    #[test]
    fn route_preview_matches_risk_category() {
        let config = IncidentMonitorConfig {
            destinations: vec![IncidentMonitorDestinationConfig {
                destination_id: "security-linear".to_string(),
                name: "Security Linear".to_string(),
                kind: IncidentMonitorDestinationKind::LinearIssue,
                ..IncidentMonitorDestinationConfig::default()
            }],
            routes: vec![IncidentMonitorRouteConfig {
                route_id: "security-risk".to_string(),
                name: "Security Risk".to_string(),
                destination_ids: vec!["security-linear".to_string()],
                match_risk_categories: vec!["data_exfiltration".to_string()],
                ..IncidentMonitorRouteConfig::default()
            }],
            default_destination_ids: vec!["legacy-github".to_string()],
            ..IncidentMonitorConfig::default()
        };
        let context = build_route_context(
            None,
            None,
            None,
            None,
            Some("data_exfiltration"),
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            None,
        );
        let destinations = config.effective_destinations();
        let preview = build_route_preview(&config, &destinations, &[], &[], &context, &[]);

        assert_eq!(
            preview.effective_destination_ids,
            vec!["security-linear".to_string()]
        );
        assert_eq!(
            preview
                .matches
                .first()
                .and_then(|row| row.reason.as_deref()),
            Some("matched_risk_category")
        );
    }

    #[test]
    fn route_preview_prefers_highest_priority_route_over_overlapping_catch_all() {
        // TAN-543: when more than one route matches, the highest-priority route
        // wins and its destination is the only effective one. Previously the
        // matcher unioned destinations across all matching routes, which then
        // hard-failed every publish for any event that matched an overlapping
        // catch-all route.
        let config = IncidentMonitorConfig {
            destinations: vec![
                IncidentMonitorDestinationConfig {
                    destination_id: "security-linear".to_string(),
                    name: "Security Linear".to_string(),
                    kind: IncidentMonitorDestinationKind::LinearIssue,
                    ..IncidentMonitorDestinationConfig::default()
                },
                IncidentMonitorDestinationConfig {
                    destination_id: "legacy-github".to_string(),
                    name: "Legacy GitHub".to_string(),
                    kind: IncidentMonitorDestinationKind::GithubIssue,
                    ..IncidentMonitorDestinationConfig::default()
                },
            ],
            routes: vec![
                IncidentMonitorRouteConfig {
                    route_id: "catch-all".to_string(),
                    name: "Catch all".to_string(),
                    enabled: true,
                    priority: 0,
                    destination_ids: vec!["legacy-github".to_string()],
                    ..IncidentMonitorRouteConfig::default()
                },
                IncidentMonitorRouteConfig {
                    route_id: "security-risk".to_string(),
                    name: "Security Risk".to_string(),
                    enabled: true,
                    priority: 100,
                    destination_ids: vec!["security-linear".to_string()],
                    match_risk_categories: vec!["data_exfiltration".to_string()],
                    ..IncidentMonitorRouteConfig::default()
                },
            ],
            default_destination_ids: vec!["legacy-github".to_string()],
            ..IncidentMonitorConfig::default()
        };
        let context = build_route_context(
            None,
            None,
            None,
            None,
            Some("data_exfiltration"),
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            None,
        );
        let destinations = config.effective_destinations();
        let preview = build_route_preview(&config, &destinations, &[], &[], &context, &[]);

        // Both routes match...
        assert!(
            preview.matches.len() >= 2,
            "both routes should appear as matches: {:?}",
            preview.matches
        );
        // ...but only the higher-priority route's destination is effective.
        assert_eq!(
            preview.effective_destination_ids,
            vec!["security-linear".to_string()]
        );
        // A single effective destination satisfies the one-destination phase limit,
        // so validate_publish_plan no longer hard-fails on overlapping routes.
        assert_eq!(preview.destinations.len(), 1);
    }
}
