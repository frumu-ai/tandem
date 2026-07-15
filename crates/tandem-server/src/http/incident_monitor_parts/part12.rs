// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

const INCIDENT_MONITOR_ASSESSMENT_REPORT_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct IncidentMonitorAssessmentReportInput {
    #[serde(default)]
    pub from_ms: Option<u64>,
    #[serde(default)]
    pub to_ms: Option<u64>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub min_severity: Option<String>,
    #[serde(default)]
    pub probes: Vec<String>,
    #[serde(default)]
    pub include_probe_results: Option<bool>,
    #[serde(default)]
    pub include_draft_suggestions: Option<bool>,
    #[serde(default)]
    pub persist_artifact: Option<bool>,
    #[serde(default)]
    pub route_destination_ids: Vec<String>,
    #[serde(default)]
    pub include_raw_payloads: Option<bool>,
}

pub(super) async fn generate_incident_monitor_security_assessment_report(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Json(input): Json<IncidentMonitorAssessmentReportInput>,
) -> Response {
    if incident_monitor_assessment_has_scoped_intake_key_header(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Incident Monitor assessment reports require an admin API token, not a scoped intake key",
                "code": "INCIDENT_MONITOR_ASSESSMENT_REPORT_ADMIN_REQUIRED",
            })),
        )
            .into_response();
    }
    if let Some(required_token) = state.api_token().await {
        if !incident_monitor_assessment_request_has_api_token(&headers, &required_token) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Incident Monitor assessment reports require the full admin API token",
                    "code": "INCIDENT_MONITOR_ASSESSMENT_REPORT_ADMIN_REQUIRED",
                })),
            )
                .into_response();
        }
    }

    let generated_at_ms = crate::now_ms();
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let mut report = incident_monitor_assessment_report_payload(
        &state,
        tenant_context.clone(),
        verified,
        &input,
        generated_at_ms,
    )
    .await;

    let evidence_pack = if input.persist_artifact.unwrap_or(true) {
        match incident_monitor_assessment_report_persist_evidence_pack(
            &state,
            tenant_context.clone(),
            generated_at_ms,
            &report,
        )
        .await
        {
            Ok(value) => value,
            Err(status_code) => {
                return (
                    status_code,
                    Json(json!({
                        "error": "Failed to persist Incident Monitor assessment report evidence pack",
                        "code": "INCIDENT_MONITOR_ASSESSMENT_REPORT_EVIDENCE_WRITE_FAILED",
                    })),
                )
                    .into_response();
            }
        }
    } else {
        json!({
            "persisted": false,
            "reason": "persist_artifact was false",
        })
    };
    report["evidence_pack"] = evidence_pack.clone();

    incident_monitor_assessment_report_emit_audit_event(
        &state,
        &tenant_context,
        "incident_monitor.assessment_report.generated",
        json!({
            "source_kind": "tandem_monitor",
            "schema_version": INCIDENT_MONITOR_ASSESSMENT_REPORT_SCHEMA_VERSION,
            "generated_at_ms": generated_at_ms,
            "counts": report["counts"].clone(),
            "evidence_pack": evidence_pack,
            "route_destination_ids": input.route_destination_ids,
            "raw_payloads_included": false,
        }),
    )
    .await;

    Json(report).into_response()
}

async fn incident_monitor_assessment_report_payload(
    state: &AppState,
    tenant_context: TenantContext,
    verified: Option<&VerifiedTenantContext>,
    input: &IncidentMonitorAssessmentReportInput,
    generated_at_ms: u64,
) -> Value {
    let authority_inventory =
        incident_monitor_authority_inventory_payload(state, tenant_context.clone(), verified).await;
    let status = state.incident_monitor_status_snapshot().await;
    let policy = incident_monitor_assessment_report_posture_policy(input);
    let findings = incident_monitor_security_posture_findings(&authority_inventory, &policy);
    let selected_probe_ids = incident_monitor_assessment_selected_probe_ids(&input.probes);
    let probe_results = if input.include_probe_results.unwrap_or(true) {
        incident_monitor_security_assessment_probe_results(
            &selected_probe_ids,
            "dry_run",
            &authority_inventory,
            &status,
            input.include_draft_suggestions.unwrap_or(false),
        )
    } else {
        Vec::new()
    };
    // TAN-487: run the built-in adversarial scenario pack in dry-run against the
    // live config and fold its results into the report.
    let scenario_pack = crate::incident_monitor_scenarios::run_incident_monitor_scenario_pack(
        &status,
        &tenant_context,
        &crate::default_scenario_pack(),
    );
    // TAN-488: compute governance maturity metrics + behavioral drift over the
    // report window (redacted, dry-run) and fold them in. from_ms/to_ms are
    // absolute timestamps, so the window is their difference.
    let metrics_to_ms = input.to_ms.unwrap_or(generated_at_ms);
    let metrics_window_ms = input
        .from_ms
        .map(|from_ms| metrics_to_ms.saturating_sub(from_ms));
    let governance_maturity =
        crate::incident_monitor_governance_metrics::compute_incident_monitor_governance_metrics(
            state,
            &tenant_context,
            metrics_to_ms,
            metrics_window_ms,
            &crate::IncidentMonitorGovernanceThresholds::default(),
        )
        .await;
    // Tenant filter is applied inside the listing, before the recency cap, so a
    // scoped report can't lose the caller tenant's incidents to newer incidents
    // from other tenants (TAN-546-style crowd-out).
    let incidents = state
        .list_incident_monitor_incidents_for_tenant(&tenant_context, 200)
        .await
        .into_iter()
        .filter(|incident| {
            incident_monitor_assessment_report_incident_matches(incident, &tenant_context, input)
        })
        .collect::<Vec<_>>();
    // Tenant filter is applied inside the listing, before the recency cap, so a
    // scoped report can't lose the caller tenant's receipts to newer receipts
    // from other tenants (TAN-546).
    let posts = state
        .list_incident_monitor_posts_for_tenant(&tenant_context, 200)
        .await
        .into_iter()
        .filter(|post| incident_monitor_assessment_report_time_matches(post.updated_at_ms, input))
        .collect::<Vec<_>>();
    let audit_events = crate::audit::load_protected_audit_events_for_tenant(state, &tenant_context)
        .await
        .into_iter()
        .filter(|event| incident_monitor_assessment_report_audit_event_matches(event, input))
        .collect::<Vec<_>>();

    let inventory = authority_inventory
        .get("inventory")
        .cloned()
        .unwrap_or(Value::Null);
    let monitored_sources =
        incident_monitor_posture_array(&inventory, &["monitored_sources"]).to_vec();
    let destinations = incident_monitor_posture_array(&inventory, &["destinations"]).to_vec();
    let routes = incident_monitor_posture_array(&inventory, &["routes"]).to_vec();
    let automation_specs =
        incident_monitor_posture_array(&inventory, &["automation_specs"]).to_vec();
    let approval_rules = incident_monitor_posture_array(&inventory, &["approval_rules"]).to_vec();
    let finding_counts = incident_monitor_assessment_report_findings_counts(&findings);
    let probe_counts = incident_monitor_assessment_counts(&probe_results);
    let incident_summaries = incidents
        .iter()
        .map(incident_monitor_assessment_report_incident_summary)
        .collect::<Vec<_>>();
    let post_summaries = posts
        .iter()
        .map(incident_monitor_assessment_report_post_summary)
        .collect::<Vec<_>>();
    let audit_export_rows = audit_events
        .iter()
        .map(incident_monitor_assessment_report_audit_row)
        .collect::<Vec<_>>();
    let self_monitoring = incident_monitor_assessment_report_self_monitoring(
        &incidents,
        &audit_events,
        &probe_results,
    );
    let route_preview = incident_monitor_assessment_report_route_preview(
        &status,
        &tenant_context,
        &input.route_destination_ids,
    );
    let recommendations =
        incident_monitor_assessment_report_recommendations(&findings, &probe_results, &status);
    let markdown = incident_monitor_assessment_report_markdown(
        generated_at_ms,
        &tenant_context,
        &finding_counts,
        &probe_counts,
        &self_monitoring,
        &recommendations,
    );

    json!({
        "schema_version": INCIDENT_MONITOR_ASSESSMENT_REPORT_SCHEMA_VERSION,
        "generated_at_ms": generated_at_ms,
        "scope": {
            "source": "incident_monitor_security_gap_assessment_report",
            "read_only": true,
            "dry_run": true,
            "tenant_context": tenant_context,
            "from_ms": input.from_ms,
            "to_ms": input.to_ms,
            "source_kind": input.source_kind.as_deref().map(str::trim).filter(|value| !value.is_empty()),
            "raw_payloads_included": false,
            "raw_payload_request_ignored": input.include_raw_payloads.unwrap_or(false),
        },
        "counts": {
            "monitored_sources": monitored_sources.len(),
            "source_readiness_findings": incident_monitor_assessment_report_source_readiness_finding_count(&monitored_sources),
            "automation_specs": automation_specs.len(),
            "destinations": destinations.len(),
            "routes": routes.len(),
            "findings": findings.len(),
            "probe_results": probe_results.len(),
            "adversarial_scenarios": scenario_pack.pointer("/counts/total").and_then(Value::as_u64).unwrap_or(0),
            "failed_adversarial_scenarios": scenario_pack.pointer("/counts/failed").and_then(Value::as_u64).unwrap_or(0),
            "governance_metric_breaches": governance_maturity.pointer("/counts/breached_metrics").and_then(Value::as_u64).unwrap_or(0),
            "behavioral_drift_flags": governance_maturity.pointer("/counts/drift_flags").and_then(Value::as_u64).unwrap_or(0),
            "incidents": incidents.len(),
            "destination_receipts": posts.len(),
            "protected_audit_events": audit_export_rows.len(),
        },
        "sections": {
            "assessment_scope": {
                "time_window": {
                    "from_ms": input.from_ms,
                    "to_ms": input.to_ms,
                },
                "source_kind": input.source_kind,
                "includes_authority_inventory": true,
                "includes_posture_checks": true,
                "includes_controlled_probes": input.include_probe_results.unwrap_or(true),
                "includes_protected_audit_export": true,
            },
            "monitored_systems_and_sources": {
                "source_kind_counts": incident_monitor_assessment_report_count_values(&monitored_sources, "source_kind"),
                "source_readiness": incident_monitor_assessment_report_source_readiness_summary(&monitored_sources),
                "sources": monitored_sources.iter()
                    .take(50)
                    .map(incident_monitor_assessment_report_source_summary)
                    .collect::<Vec<_>>(),
            },
            "authority_inventory": {
                "counts": authority_inventory.get("counts").cloned().unwrap_or(Value::Null),
                "destinations": destinations.iter()
                    .take(50)
                    .map(incident_monitor_assessment_report_destination_summary)
                    .collect::<Vec<_>>(),
                "routes": routes.iter()
                    .take(50)
                    .map(incident_monitor_assessment_report_route_summary)
                    .collect::<Vec<_>>(),
            },
            "approval_coverage": {
                "approval_rules": approval_rules.len(),
                "approval_required_rules": approval_rules.iter()
                    .filter(|rule| rule.get("requires_approval").and_then(Value::as_bool).unwrap_or(false))
                    .count(),
                "destinations_requiring_approval": destinations.iter()
                    .filter(|destination| destination.get("require_approval").and_then(Value::as_bool).unwrap_or(false))
                    .count(),
                "require_approval_for_high_risk": inventory
                    .get("incident_monitor")
                    .and_then(|value| value.get("safety_defaults"))
                    .and_then(|value| value.get("require_approval_for_high_risk"))
                    .cloned()
                    .unwrap_or(Value::Null),
            },
            "tenant_workspace_context_coverage": incident_monitor_assessment_report_context_coverage(&monitored_sources, &automation_specs),
            "detected_findings": {
                "counts": finding_counts,
                "findings": findings,
            },
            "controlled_probe_results": {
                "selected_probe_ids": selected_probe_ids,
                "counts": probe_counts,
                "results": probe_results,
            },
            "adversarial_scenario_packs": scenario_pack,
            "governance_maturity_metrics": governance_maturity,
            "incidents_and_evidence_refs": {
                "counts": {
                    "incidents": incident_summaries.len(),
                    "by_source_kind": incident_monitor_assessment_report_count_values(&incident_summaries, "source_kind"),
                },
                "incidents": incident_summaries,
            },
            "destination_receipts": {
                "counts": {
                    "receipts": post_summaries.len(),
                    "by_destination_kind": incident_monitor_assessment_report_count_values(&post_summaries, "destination_kind"),
                    "by_status": incident_monitor_assessment_report_count_values(&post_summaries, "status"),
                },
                "receipts": post_summaries,
            },
            "recommended_mitigations": recommendations,
            "residual_risk_and_follow_up": incident_monitor_assessment_report_residual_risk(&findings, &probe_results),
            "self_monitoring_boundary": self_monitoring,
            "external_audit_export": {
                "available": true,
                "source_kind": "tandem_monitor",
                "claim": "Tandem produces governed, exportable evidence of observed behavior, policy decisions, incidents, destination attempts, and monitor health; it does not prove itself safe.",
                "customer_owned_systems": ["siem", "database", "object_storage", "linear", "github", "webhook", "telemetry", "mcp_tool"],
                "existing_ndjson_endpoint": "/audit/ledger/export",
                "report_endpoint": "/incident-monitor/security/assessment-report",
                "route_preview": route_preview,
                "records": audit_export_rows,
                "sensitive_values": {
                    "payloads": "omitted_by_default",
                    "payload_keys": "included",
                    "record_hashes": "included",
                },
            },
        },
        "markdown_summary": markdown,
        "sensitive_values": {
            "policy": "redacted_or_summarized",
            "omitted_fields": [
                "scoped_intake_key.raw_key",
                "scoped_intake_key.key_hash",
                "authorization",
                "x-agent-token",
                "x-tandem-token",
                "destination.webhook_secret_ref_value",
                "protected_audit.payload",
                "destination.receipt.raw_payload"
            ],
        },
    })
}

fn incident_monitor_assessment_report_posture_policy(
    input: &IncidentMonitorAssessmentReportInput,
) -> IncidentMonitorPostureRulePolicy {
    IncidentMonitorPostureRulePolicy {
        mode: "dry_run".to_string(),
        enabled_rules: INCIDENT_MONITOR_POSTURE_RULES
            .iter()
            .map(|(rule_id, _, _)| (*rule_id).to_string())
            .collect::<HashSet<_>>(),
        disabled_rules: HashSet::new(),
        min_severity_rank: incident_monitor_posture_severity_rank(
            input.min_severity.as_deref().unwrap_or("low"),
        ),
    }
}

fn incident_monitor_assessment_report_incident_matches(
    incident: &IncidentMonitorIncidentRecord,
    tenant_context: &TenantContext,
    input: &IncidentMonitorAssessmentReportInput,
) -> bool {
    if !incident_monitor_assessment_report_incident_matches_tenant(incident, tenant_context) {
        return false;
    }
    if !incident_monitor_assessment_report_time_matches(incident.updated_at_ms, input) {
        return false;
    }
    let Some(source_kind) = input
        .source_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    incident
        .source_kind
        .as_ref()
        .map(IncidentMonitorSourceKind::as_str)
        .is_some_and(|value| value == source_kind)
}

fn incident_monitor_assessment_report_incident_matches_tenant(
    incident: &IncidentMonitorIncidentRecord,
    tenant_context: &TenantContext,
) -> bool {
    tenant_context.is_local_implicit()
        || (incident.tenant_id.as_deref() == Some(tenant_context.org_id.as_str())
            && incident.workspace_id.as_deref() == Some(tenant_context.workspace_id.as_str()))
}

fn incident_monitor_assessment_report_time_matches(
    timestamp_ms: u64,
    input: &IncidentMonitorAssessmentReportInput,
) -> bool {
    input
        .from_ms
        .map(|from_ms| timestamp_ms >= from_ms)
        .unwrap_or(true)
        && input
            .to_ms
            .map(|to_ms| timestamp_ms <= to_ms)
            .unwrap_or(true)
}

fn incident_monitor_assessment_report_audit_event_matches(
    event: &crate::audit::ProtectedAuditEnvelope,
    input: &IncidentMonitorAssessmentReportInput,
) -> bool {
    incident_monitor_assessment_report_time_matches(event.created_at_ms, input)
        && (event.event_type.starts_with("incident_monitor.")
            || event
                .payload
                .get("source_kind")
                .and_then(Value::as_str)
                .is_some_and(|source_kind| {
                    source_kind == "tandem_runtime" || source_kind == "tandem_monitor"
                }))
}

fn incident_monitor_assessment_report_findings_counts(findings: &[Value]) -> Value {
    json!({
        "findings": findings.len(),
        "by_severity": incident_monitor_posture_counts_by(findings, "severity"),
        "by_category": incident_monitor_posture_counts_by(findings, "category"),
        "by_rule": incident_monitor_posture_counts_by(findings, "rule_id"),
    })
}

fn incident_monitor_assessment_report_incident_summary(
    incident: &IncidentMonitorIncidentRecord,
) -> Value {
    json!({
        "incident_id": incident.incident_id,
        "title": incident.title,
        "status": incident.status,
        "event_type": incident.event_type,
        "fingerprint": incident.fingerprint,
        "source": incident.source,
        "source_kind": incident.source_kind.as_ref().map(IncidentMonitorSourceKind::as_str),
        "project_id": incident.project_id,
        "log_source_id": incident.log_source_id,
        "risk_level": incident.risk_level,
        "risk_category": incident.risk_category,
        "confidence": incident.confidence,
        "tenant_id": incident.tenant_id,
        "workspace_id": incident.workspace_id,
        "route_tags": incident.route_tags,
        "evidence_refs": incident.evidence_refs,
        "created_at_ms": incident.created_at_ms,
        "updated_at_ms": incident.updated_at_ms,
        "last_error_present": incident.last_error.as_ref().is_some_and(|value| !value.trim().is_empty()),
    })
}

fn incident_monitor_assessment_report_post_summary(post: &IncidentMonitorPostRecord) -> Value {
    json!({
        "post_id": post.post_id,
        "draft_id": post.draft_id,
        "incident_id": post.incident_id,
        "operation": post.operation,
        "status": post.status,
        "destination_id": post.destination_id,
        "destination_kind": post.destination_kind,
        "route_id": post.route_id,
        "route_match_reason": post.route_match_reason,
        "external_id": post.external_id,
        "external_url": post.external_url,
        "target_ref": post.target_ref,
        "receipt_present": post.receipt.is_some(),
        "evidence_digest": post.evidence_digest,
        "evidence_refs": post.evidence_refs,
        "idempotency_key_present": !post.idempotency_key.trim().is_empty(),
        "error_present": post.error.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "created_at_ms": post.created_at_ms,
        "updated_at_ms": post.updated_at_ms,
    })
}

fn incident_monitor_assessment_report_audit_row(
    event: &crate::audit::ProtectedAuditEnvelope,
) -> Value {
    let payload_json = serde_json::to_string(&event.payload).unwrap_or_default();
    json!({
        "event_id": event.event_id,
        "event_type": event.event_type,
        "created_at_ms": event.created_at_ms,
        "seq": event.seq,
        "prev_hash_present": event.prev_hash.is_some(),
        "record_hash": event.record_hash,
        "payload_hash": format!("sha256:{}", crate::sha256_hex(&[payload_json.as_str()])),
        "payload_keys": sorted_object_keys(Some(&event.payload)),
        "tenant_context": {
            "org_id": event.tenant_context.org_id,
            "workspace_id": event.tenant_context.workspace_id,
            "deployment_id": event.tenant_context.deployment_id,
        },
    })
}

fn incident_monitor_assessment_report_source_summary(source: &Value) -> Value {
    json!({
        "project_id": source.get("project_id").cloned().unwrap_or(Value::Null),
        "source_id": source.get("source_id").cloned().unwrap_or(Value::Null),
        "name": source.get("name").cloned().unwrap_or(Value::Null),
        "enabled": source.get("enabled").cloned().unwrap_or(Value::Null),
        "paused": source.get("paused").cloned().unwrap_or(Value::Null),
        "source_kind": source.get("source_kind").cloned().unwrap_or(Value::Null),
        "tenant_id_present": source.get("tenant_id").and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty()),
        "workspace_id_present": source.get("workspace_id").and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty()),
        "allowed_destination_ids": source.get("allowed_destination_ids").cloned().unwrap_or(Value::Array(Vec::new())),
        "default_destination_ids": source.get("default_destination_ids").cloned().unwrap_or(Value::Array(Vec::new())),
        "data_readiness": source.get("data_readiness").cloned().unwrap_or(Value::Null),
        "readiness": {
            "ready": source.pointer("/readiness/ready").cloned().unwrap_or(Value::Null),
            "governance_ready": source.pointer("/readiness/governance_ready").cloned().unwrap_or(Value::Null),
            "lineage_ready": source.pointer("/readiness/lineage_ready").cloned().unwrap_or(Value::Null),
            "freshness_ready": source.pointer("/readiness/freshness_ready").cloned().unwrap_or(Value::Null),
            "schema_ready": source.pointer("/readiness/schema_ready").cloned().unwrap_or(Value::Null),
            "protection_ready": source.pointer("/readiness/protection_ready").cloned().unwrap_or(Value::Null),
            "missing": source.pointer("/readiness/missing").cloned().unwrap_or(Value::Array(Vec::new())),
            "warnings": source.pointer("/readiness/warnings").cloned().unwrap_or(Value::Array(Vec::new())),
            "finding_count": source.pointer("/readiness/findings").and_then(Value::as_array).map(|rows| rows.len()).unwrap_or(0),
        },
    })
}

fn incident_monitor_assessment_report_destination_summary(destination: &Value) -> Value {
    json!({
        "destination_id": destination.get("destination_id").cloned().unwrap_or(Value::Null),
        "name": destination.get("name").cloned().unwrap_or(Value::Null),
        "kind": destination.get("kind").cloned().unwrap_or(Value::Null),
        "enabled": destination.get("enabled").cloned().unwrap_or(Value::Null),
        "require_approval": destination.get("require_approval").cloned().unwrap_or(Value::Null),
        "webhook_url_present": destination.get("webhook_url_present").cloned().unwrap_or(Value::Null),
        "webhook_secret_ref_present": destination.get("webhook_secret_ref_present").cloned().unwrap_or(Value::Null),
        "allow_publish": destination.get("allow_publish").cloned().unwrap_or(Value::Null),
        "custom_config_keys": destination.get("custom_config_keys").cloned().unwrap_or(Value::Array(Vec::new())),
    })
}

fn incident_monitor_assessment_report_route_summary(route: &Value) -> Value {
    json!({
        "route_id": route.get("route_id").cloned().unwrap_or(Value::Null),
        "name": route.get("name").cloned().unwrap_or(Value::Null),
        "enabled": route.get("enabled").cloned().unwrap_or(Value::Null),
        "priority": route.get("priority").cloned().unwrap_or(Value::Null),
        "destination_ids": route.get("destination_ids").cloned().unwrap_or(Value::Array(Vec::new())),
        "approval_policy": route.get("approval_policy").cloned().unwrap_or(Value::Null),
        "match": route.get("match").cloned().unwrap_or(Value::Null),
    })
}

fn incident_monitor_assessment_report_context_coverage(
    monitored_sources: &[Value],
    automation_specs: &[Value],
) -> Value {
    let sources_with_context = monitored_sources
        .iter()
        .filter(|source| {
            incident_monitor_assessment_report_value_has_text(source, "tenant_id")
                && incident_monitor_assessment_report_value_has_text(source, "workspace_id")
        })
        .count();
    let automations_with_context = automation_specs
        .iter()
        .filter(|automation| {
            let context = automation.get("tenant_context").unwrap_or(&Value::Null);
            incident_monitor_assessment_report_value_has_text(context, "org_id")
                && incident_monitor_assessment_report_value_has_text(context, "workspace_id")
                && context
                    .get("source")
                    .and_then(Value::as_str)
                    .is_some_and(|source| source != "local_implicit")
        })
        .count();
    json!({
        "monitored_sources": monitored_sources.len(),
        "monitored_sources_with_tenant_workspace": sources_with_context,
        "monitored_sources_missing_tenant_workspace": monitored_sources.len().saturating_sub(sources_with_context),
        "automation_specs": automation_specs.len(),
        "automation_specs_with_explicit_context": automations_with_context,
        "automation_specs_missing_explicit_context": automation_specs.len().saturating_sub(automations_with_context),
    })
}

fn incident_monitor_assessment_report_source_readiness_summary(
    monitored_sources: &[Value],
) -> Value {
    let ready_sources = monitored_sources
        .iter()
        .filter(|source| {
            source
                .pointer("/readiness/ready")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let enabled_sources = monitored_sources
        .iter()
        .filter(|source| {
            source
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .count();
    let findings = monitored_sources
        .iter()
        .flat_map(|source| {
            source
                .pointer("/readiness/findings")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    json!({
        "sources": monitored_sources.len(),
        "enabled_sources": enabled_sources,
        "ready_sources": ready_sources,
        "unready_enabled_sources": enabled_sources.saturating_sub(ready_sources),
        "findings": findings.len(),
        "findings_by_severity": incident_monitor_assessment_report_count_values(&findings, "severity"),
        "findings_by_category": incident_monitor_assessment_report_count_values(&findings, "category"),
    })
}

fn incident_monitor_assessment_report_source_readiness_finding_count(
    monitored_sources: &[Value],
) -> usize {
    monitored_sources
        .iter()
        .map(|source| {
            source
                .pointer("/readiness/findings")
                .and_then(Value::as_array)
                .map(|findings| findings.len())
                .unwrap_or(0)
        })
        .sum()
}

fn incident_monitor_assessment_report_value_has_text(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn incident_monitor_assessment_report_self_monitoring(
    incidents: &[IncidentMonitorIncidentRecord],
    audit_events: &[crate::audit::ProtectedAuditEnvelope],
    probe_results: &[Value],
) -> Value {
    let self_incidents = incidents
        .iter()
        .filter(|incident| {
            incident
                .source_kind
                .as_ref()
                .map(IncidentMonitorSourceKind::as_str)
                .is_some_and(|source_kind| {
                    source_kind == "tandem_runtime" || source_kind == "tandem_monitor"
                })
        })
        .count();
    let external_incidents = incidents.len().saturating_sub(self_incidents);
    let failed_probes = probe_results
        .iter()
        .filter(|probe| probe.get("status").and_then(Value::as_str) == Some("fail"))
        .count();
    json!({
        "source_kinds": ["tandem_runtime", "tandem_monitor"],
        "self_observed_incidents": self_incidents,
        "external_system_incidents": external_incidents,
        "protected_monitor_audit_events": audit_events.len(),
        "failed_controlled_probes": failed_probes,
        "evidence_claim": "Tandem self-monitoring records what the runtime/control layer observed and enforced.",
        "non_claim": "Tandem self-monitoring does not independently prove the monitor is safe or complete.",
        "external_export_required_for_high_assurance": true,
    })
}

fn incident_monitor_assessment_report_route_preview(
    status: &crate::IncidentMonitorStatus,
    tenant_context: &TenantContext,
    route_destination_ids: &[String],
) -> Value {
    let mut context = crate::incident_monitor::router::build_route_context(
        Some("security.assessment.report"),
        Some("incident_monitor_assessment_report"),
        Some("incident_monitor"),
        Some("high"),
        Some("security_gap"),
        Some("high"),
        Some("security_report"),
        None,
        None,
        &["security_assessment_report".to_string()],
        None,
        None,
        None,
    );
    context.source_kind = Some("tandem_monitor".to_string());
    context.tenant_id = Some(tenant_context.org_id.clone());
    context.workspace_id = Some(tenant_context.workspace_id.clone());
    let preview = crate::incident_monitor::router::build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &status.source_readiness,
        &context,
        route_destination_ids,
    );
    json!({
        "requested_destination_ids": route_destination_ids,
        "effective_destination_ids": preview.effective_destination_ids,
        "approval_required": preview.approval_required,
        "blocked": preview.blocked,
        "blocked_reasons": preview.blocked_reasons,
        "matches": preview.matches,
        "destinations": preview.destinations.iter()
            .map(incident_monitor_destination_inventory)
            .collect::<Vec<_>>(),
        "mutates_external_systems": false,
        "publish_method": "persist the report artifact, then submit or approve it through the normal Incident Monitor destination workflow",
    })
}

fn incident_monitor_assessment_report_recommendations(
    findings: &[Value],
    probe_results: &[Value],
    status: &crate::IncidentMonitorStatus,
) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for finding in findings {
        if let Some(recommendation) = finding.get("recommendation").and_then(Value::as_str) {
            if seen.insert(recommendation.to_string()) {
                rows.push(json!({
                    "source": "posture_finding",
                    "recommendation": recommendation,
                    "severity": finding.get("severity").cloned().unwrap_or(Value::Null),
                    "rule_id": finding.get("rule_id").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }
    for probe in probe_results {
        if probe.get("status").and_then(Value::as_str) != Some("fail") {
            continue;
        }
        let recommendation = format!(
            "Repair control gap observed by probe `{}` before production deployment.",
            probe
                .get("probe_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown_probe")
        );
        if seen.insert(recommendation.clone()) {
            rows.push(json!({
                "source": "assessment_probe",
                "recommendation": recommendation,
                "probe_id": probe.get("probe_id").cloned().unwrap_or(Value::Null),
                "target": probe.get("target").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    if !status.config.safety_defaults.require_approval_for_high_risk
        && seen.insert("Enable approval for high-risk Incident Monitor publishing.".to_string())
    {
        rows.push(json!({
            "source": "safety_defaults",
            "recommendation": "Enable approval for high-risk Incident Monitor publishing.",
            "policy": "require_approval_for_high_risk",
        }));
    }
    rows
}

fn incident_monitor_assessment_report_residual_risk(
    findings: &[Value],
    probe_results: &[Value],
) -> Value {
    let critical_or_high_findings = findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.get("severity").and_then(Value::as_str),
                Some("critical") | Some("high")
            )
        })
        .count();
    let failed_probes = probe_results
        .iter()
        .filter(|probe| probe.get("status").and_then(Value::as_str) == Some("fail"))
        .count();
    let blocked_probes = probe_results
        .iter()
        .filter(|probe| probe.get("status").and_then(Value::as_str) == Some("blocked"))
        .count();
    let risk = if critical_or_high_findings > 0 || failed_probes > 0 {
        "high"
    } else if blocked_probes > 0 {
        "medium"
    } else {
        "low"
    };
    json!({
        "risk": risk,
        "critical_or_high_findings": critical_or_high_findings,
        "failed_probes": failed_probes,
        "blocked_probes": blocked_probes,
        "follow_up_actions": [
            "Review high and critical posture findings.",
            "Re-run controlled probes after mitigation.",
            "Export protected audit evidence to a customer-owned system of record.",
            "Attach the report artifact to the production readiness review."
        ],
    })
}

fn incident_monitor_assessment_report_count_values(rows: &[Value], key: &str) -> Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        if let Some(value) = row.get(key).and_then(Value::as_str) {
            *counts.entry(value.to_string()).or_insert(0) += 1;
        }
    }
    json!(counts)
}

fn incident_monitor_assessment_report_markdown(
    generated_at_ms: u64,
    tenant_context: &TenantContext,
    finding_counts: &Value,
    probe_counts: &Value,
    self_monitoring: &Value,
    recommendations: &[Value],
) -> String {
    let mut lines = Vec::new();
    lines.push("# Incident Monitor Security Gap Assessment".to_string());
    lines.push(String::new());
    lines.push(format!("Generated at: `{generated_at_ms}`"));
    lines.push(format!(
        "Tenant/workspace: `{}` / `{}`",
        tenant_context.org_id, tenant_context.workspace_id
    ));
    lines.push(String::new());
    lines.push("## Summary".to_string());
    lines.push(format!(
        "- Findings: {}",
        finding_counts
            .get("findings")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    ));
    lines.push(format!(
        "- Probe results: {} pass, {} fail, {} blocked",
        probe_counts
            .get("pass")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        probe_counts
            .get("fail")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        probe_counts
            .get("blocked")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    ));
    lines.push(format!(
        "- Tandem self-observed incidents: {}",
        self_monitoring
            .get("self_observed_incidents")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    ));
    lines.push(format!(
        "- External system incidents: {}",
        self_monitoring
            .get("external_system_incidents")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    ));
    lines.push(String::new());
    lines.push("## Self-Monitoring Boundary".to_string());
    lines.push(
        "Tandem produces governed, exportable evidence about observed agent/workflow behavior, policy decisions, incidents, destination attempts, and monitor health."
            .to_string(),
    );
    lines.push(
        "This report does not claim that Tandem independently proves itself safe; high-assurance deployments should export the evidence to a customer-owned system of record."
            .to_string(),
    );
    lines.push(String::new());
    lines.push("## Recommended Mitigations".to_string());
    if recommendations.is_empty() {
        lines.push(
            "- No immediate mitigations were generated from the selected checks.".to_string(),
        );
    } else {
        for recommendation in recommendations.iter().take(10) {
            if let Some(text) = recommendation.get("recommendation").and_then(Value::as_str) {
                lines.push(format!("- {text}"));
            }
        }
    }
    lines.join("\n")
}

async fn incident_monitor_assessment_report_persist_evidence_pack(
    state: &AppState,
    tenant_context: TenantContext,
    generated_at_ms: u64,
    payload: &Value,
) -> Result<Value, StatusCode> {
    let run_id = format!(
        "incident-monitor-assessment-report-{}-{}",
        generated_at_ms,
        Uuid::new_v4().simple()
    );
    let artifact_id = format!(
        "incident-monitor-assessment-report-{}",
        Uuid::new_v4().simple()
    );
    super::context_runs::context_run_create_impl(
        state.clone(),
        tenant_context,
        ContextRunCreateInput {
            run_id: Some(run_id.clone()),
            objective:
                "Generate Incident Monitor security gap assessment report and evidence pack."
                    .to_string(),
            run_type: Some("incident_monitor_assessment_report".to_string()),
            workspace: Some(ContextWorkspaceLease::default()),
            source_client: Some("incident_monitor".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: None,
        },
    )
    .await?;
    write_incident_monitor_artifact(
        state,
        &run_id,
        &artifact_id,
        "incident_monitor_assessment_report",
        "artifacts/incident_monitor.assessment_report.json",
        payload,
    )
    .await?;
    let now = crate::now_ms();
    if let Ok(mut run) = load_context_run_state(state, &run_id).await {
        run.status = ContextRunStatus::Completed;
        run.started_at_ms = Some(generated_at_ms);
        run.ended_at_ms = Some(now);
        run.updated_at_ms = now;
        save_context_run_state(state, &run).await?;
    }
    Ok(json!({
        "persisted": true,
        "kind": "context_run_artifact",
        "context_run_id": run_id,
        "artifact_id": artifact_id,
        "artifact_type": "incident_monitor_assessment_report",
        "path": context_run_dir(state, &run_id)
            .join("artifacts/incident_monitor.assessment_report.json")
            .to_string_lossy()
            .to_string(),
    }))
}

async fn incident_monitor_assessment_report_emit_audit_event(
    state: &AppState,
    tenant_context: &TenantContext,
    event_type: &'static str,
    payload: Value,
) {
    state
        .event_bus
        .publish(tandem_types::EngineEvent::new(event_type, payload.clone()));
    crate::audit::append_protected_audit_event_best_effort(
        state,
        event_type,
        tenant_context,
        None,
        payload,
    )
    .await;
}
