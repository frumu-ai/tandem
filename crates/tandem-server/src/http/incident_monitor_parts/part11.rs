// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

const INCIDENT_MONITOR_ASSESSMENT_PROBES_SCHEMA_VERSION: u64 = 1;

const INCIDENT_MONITOR_ASSESSMENT_PROBES: [&str; 6] = [
    "approval_required_tool_policy",
    "route_preview_high_risk_approval",
    "destination_readiness_fail_closed",
    "scoped_intake_restriction",
    "mcp_tool_allowlist",
    "webhook_url_policy",
];

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct IncidentMonitorAssessmentProbeRunInput {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub probes: Vec<String>,
    #[serde(default)]
    pub include_draft_suggestions: Option<bool>,
}

pub(super) async fn run_incident_monitor_security_assessment_probes(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Json(input): Json<IncidentMonitorAssessmentProbeRunInput>,
) -> Response {
    if incident_monitor_assessment_has_scoped_intake_key_header(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Incident Monitor assessment probes require an admin API token, not a scoped intake key",
                "code": "INCIDENT_MONITOR_ASSESSMENT_PROBES_ADMIN_REQUIRED",
            })),
        )
            .into_response();
    }
    if let Some(required_token) = state.api_token().await {
        if !incident_monitor_assessment_request_has_api_token(&headers, &required_token) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Incident Monitor assessment probes require the full admin API token",
                    "code": "INCIDENT_MONITOR_ASSESSMENT_PROBES_ADMIN_REQUIRED",
                })),
            )
                .into_response();
        }
    }

    let actor = super::governance::resolve_governance_actor(
        &headers,
        &tenant_context,
        &request_principal,
    );
    let requested_mode = input.mode.as_deref().unwrap_or("dry_run");
    let mode = incident_monitor_assessment_mode(Some(requested_mode));
    let selected_probe_ids = incident_monitor_assessment_selected_probe_ids(&input.probes);
    let generated_at_ms = crate::now_ms();
    let dry_run = mode != "disabled";
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let authority_inventory =
        incident_monitor_authority_inventory_payload(
            &state,
            tenant_context.clone(),
            verified,
            IncidentMonitorApprovalInventoryAccess::Caller(&actor),
        ).await;
    let status = state.incident_monitor_status_snapshot().await;
    let include_draft_suggestions = input.include_draft_suggestions.unwrap_or(true);

    let results = incident_monitor_security_assessment_probe_results(
        &selected_probe_ids,
        &mode,
        &authority_inventory,
        &status,
        include_draft_suggestions,
    );

    let counts = incident_monitor_assessment_counts(&results);
    let scope = json!({
        "source": "incident_monitor_security_assessment_probes",
        "read_only": true,
        "dry_run": dry_run,
        "mutates_external_systems": false,
        "admin_invoked": true,
        "full_token_required": true,
        "tenant_context": tenant_context,
        "authority_inventory_source": authority_inventory
            .get("scope")
            .and_then(|scope| scope.get("source"))
            .cloned()
            .unwrap_or(Value::Null),
    });
    let probe_policy = json!({
        "mode": mode,
        "requested_mode": requested_mode,
        "dry_run": dry_run,
        "supported_modes": ["dry_run", "disabled"],
        "selected_probe_ids": selected_probe_ids,
        "supported_probe_ids": INCIDENT_MONITOR_ASSESSMENT_PROBES,
        "include_draft_suggestions": include_draft_suggestions,
    });
    let redacted_destinations = status
        .destinations
        .iter()
        .map(incident_monitor_destination_inventory)
        .collect::<Vec<_>>();
    let mut persisted_payload = json!({
        "schema_version": INCIDENT_MONITOR_ASSESSMENT_PROBES_SCHEMA_VERSION,
        "generated_at_ms": generated_at_ms,
        "scope": scope,
        "probe_policy": probe_policy,
        "counts": counts,
        "results": results,
        "authority_inventory": authority_inventory,
        "status_snapshot": {
            "config": {
                "enabled": status.config.enabled,
                "paused": status.config.paused,
                "default_destination_ids": status.config.default_destination_ids,
                "safety_defaults": status.config.safety_defaults,
            },
            "destinations": redacted_destinations,
            "destination_readiness": status.destination_readiness,
        },
    });

    let evidence_pack = if dry_run {
        match incident_monitor_assessment_persist_evidence_pack(
            &state,
            tenant_context.clone(),
            generated_at_ms,
            &persisted_payload,
        )
        .await
        {
            Ok(value) => value,
            Err(status_code) => {
                return (
                    status_code,
                    Json(json!({
                        "error": "Failed to persist Incident Monitor assessment probe evidence pack",
                        "code": "INCIDENT_MONITOR_ASSESSMENT_PROBES_EVIDENCE_WRITE_FAILED",
                    })),
                )
                    .into_response();
            }
        }
    } else {
        json!({
            "persisted": false,
            "reason": "assessment probes are disabled",
        })
    };
    persisted_payload["evidence_pack"] = evidence_pack.clone();

    emit_incident_monitor_admin_audit_event(
        &state,
        "incident_monitor.assessment_probes.completed",
        json!({
            "mode": persisted_payload["probe_policy"]["mode"],
            "dry_run": persisted_payload["scope"]["dry_run"],
            "selected_probe_ids": persisted_payload["probe_policy"]["selected_probe_ids"],
            "counts": persisted_payload["counts"],
            "evidence_pack": evidence_pack,
        }),
    )
    .await;

    Json(json!({
        "schema_version": persisted_payload["schema_version"],
        "generated_at_ms": persisted_payload["generated_at_ms"],
        "scope": persisted_payload["scope"],
        "probe_policy": persisted_payload["probe_policy"],
        "counts": persisted_payload["counts"],
        "results": persisted_payload["results"],
        "authority_inventory": {
            "schema_version": persisted_payload["authority_inventory"]["schema_version"],
            "generated_at_ms": persisted_payload["authority_inventory"]["generated_at_ms"],
            "counts": persisted_payload["authority_inventory"]["counts"],
        },
        "evidence_pack": persisted_payload["evidence_pack"],
        "draft_conversion": {
            "available": true,
            "method": "submit result.incident_draft_suggestion through the normal Incident Monitor report or draft flow",
            "mutates_state": false,
        },
        "sensitive_values": persisted_payload["authority_inventory"]["sensitive_values"],
    }))
    .into_response()
}

fn incident_monitor_assessment_has_scoped_intake_key_header(headers: &HeaderMap) -> bool {
    headers
        .get("x-tandem-incident-monitor-intake-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn incident_monitor_security_assessment_probe_results(
    selected_probe_ids: &[String],
    mode: &str,
    authority_inventory: &Value,
    status: &crate::IncidentMonitorStatus,
    include_draft_suggestions: bool,
) -> Vec<Value> {
    let mut results = Vec::new();
    if mode == "disabled" {
        return results;
    }

    for probe_id in selected_probe_ids {
        match probe_id.as_str() {
            "approval_required_tool_policy" => incident_monitor_assessment_probe_posture_rule(
                authority_inventory,
                "approval_required_tool_policy",
                "high_risk_tool_without_approval",
                "Approval-required tools must have an agent approval policy or node approval gate.",
                "No unapproved approval-required tool policy gaps were found.",
                include_draft_suggestions,
                &mut results,
            ),
            "route_preview_high_risk_approval" => {
                incident_monitor_assessment_probe_route_preview_high_risk(
                    status,
                    include_draft_suggestions,
                    &mut results,
                )
            }
            "destination_readiness_fail_closed" => {
                incident_monitor_assessment_probe_destination_readiness(
                    status,
                    include_draft_suggestions,
                    &mut results,
                )
            }
            "scoped_intake_restriction" => incident_monitor_assessment_probe_scoped_intake(
                authority_inventory,
                include_draft_suggestions,
                &mut results,
            ),
            "mcp_tool_allowlist" => incident_monitor_assessment_probe_mcp_allowlist(
                authority_inventory,
                status,
                include_draft_suggestions,
                &mut results,
            ),
            "webhook_url_policy" => incident_monitor_assessment_probe_webhook_url_policy(
                status,
                include_draft_suggestions,
                &mut results,
            ),
            _ => {}
        }
    }
    results
}

fn incident_monitor_assessment_probe_posture_rule(
    authority_inventory: &Value,
    probe_id: &str,
    posture_rule_id: &str,
    expected_behavior: &str,
    pass_observed_behavior: &str,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    let findings = incident_monitor_assessment_posture_findings(authority_inventory, posture_rule_id);
    if findings.is_empty() {
        results.push(incident_monitor_assessment_result(
            probe_id,
            json!({
                "kind": "authority_inventory",
                "id": posture_rule_id,
            }),
            expected_behavior,
            pass_observed_behavior.to_string(),
            "pass",
            vec![incident_monitor_posture_evidence_ref(format!(
                "inventory.posture_rules[{posture_rule_id}]"
            ))],
            None,
            include_draft_suggestions,
        ));
        return;
    }

    for finding in findings {
        let target = finding
            .get("affected_objects")
            .and_then(Value::as_array)
            .and_then(|objects| objects.first())
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "kind": "posture_finding",
                    "id": finding
                        .get("finding_id")
                        .and_then(Value::as_str)
                        .unwrap_or(probe_id),
                })
            });
        let evidence_refs = finding
            .get("evidence_refs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        results.push(incident_monitor_assessment_result(
            probe_id,
            target,
            expected_behavior,
            finding
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("Posture rule reported a gap.")
                .to_string(),
            "fail",
            evidence_refs,
            Some(finding),
            include_draft_suggestions,
        ));
    }
}

fn incident_monitor_assessment_probe_route_preview_high_risk(
    status: &crate::IncidentMonitorStatus,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    let context = crate::incident_monitor::router::build_route_context(
        Some("security.assessment.probe"),
        Some("incident_monitor_assessment"),
        Some("incident_router"),
        Some("high"),
        Some("security_gap"),
        Some("high"),
        Some("security_review"),
        None,
        None,
        &["security_assessment".to_string()],
        None,
        None,
        None,
    );
    let preview = crate::incident_monitor::router::build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &status.source_readiness,
        &context,
        &[],
    );
    if preview.effective_destination_ids.is_empty() {
        results.push(incident_monitor_assessment_result(
            "route_preview_high_risk_approval",
            json!({
                "kind": "incident_monitor_route_preview",
                "id": "high_risk_synthetic_event",
            }),
            "A high-risk route preview should resolve to an approved destination path or block safely.",
            "No destination matched the synthetic high-risk probe event.".to_string(),
            "blocked",
            vec![incident_monitor_posture_evidence_ref(
                "incident_monitor.route_preview[security.assessment.probe]",
            )],
            None,
            include_draft_suggestions,
        ));
        return;
    }

    let external_destinations = preview
        .destinations
        .iter()
        .filter(|destination| {
            matches!(
                &destination.kind,
                IncidentMonitorDestinationKind::GithubIssue
                    | IncidentMonitorDestinationKind::LinearIssue
                    | IncidentMonitorDestinationKind::Webhook
                    | IncidentMonitorDestinationKind::McpTool
            )
        })
        .collect::<Vec<_>>();
    let pass = preview.approval_required || external_destinations.is_empty();
    results.push(incident_monitor_assessment_result(
        "route_preview_high_risk_approval",
        json!({
            "kind": "incident_monitor_route_preview",
            "id": "high_risk_synthetic_event",
            "destination_ids": preview.effective_destination_ids,
        }),
        "High-risk route previews must require approval before publishing to external destinations.",
        if pass {
            "High-risk synthetic route preview requires approval or only selected non-external destinations."
                .to_string()
        } else {
            "High-risk synthetic route preview can publish to external destinations without approval."
                .to_string()
        },
        if pass { "pass" } else { "fail" },
        vec![
            incident_monitor_posture_evidence_ref(
                "incident_monitor.route_preview[security.assessment.probe].approval_required",
            ),
            incident_monitor_posture_evidence_ref(
                "inventory.incident_monitor.safety_defaults.require_approval_for_high_risk",
            ),
        ],
        None,
        include_draft_suggestions,
    ));
}

fn incident_monitor_assessment_probe_destination_readiness(
    status: &crate::IncidentMonitorStatus,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    let block_unready = status.config.safety_defaults.block_unready_destinations;
    let mut evaluated = 0usize;
    for readiness in &status.destination_readiness {
        if !readiness.enabled
            || !matches!(
                &readiness.kind,
                IncidentMonitorDestinationKind::GithubIssue
                    | IncidentMonitorDestinationKind::LinearIssue
                    | IncidentMonitorDestinationKind::Webhook
                    | IncidentMonitorDestinationKind::McpTool
            )
        {
            continue;
        }
        evaluated += 1;
        if readiness.publish_ready {
            results.push(incident_monitor_assessment_result(
                "destination_readiness_fail_closed",
                json!({
                    "kind": "incident_monitor_destination",
                    "id": readiness.destination_id,
                    "destination_kind": readiness.kind.clone(),
                }),
                "Destination publish readiness should be complete, or incomplete destinations must fail closed.",
                "Destination readiness is complete.".to_string(),
                "pass",
                vec![incident_monitor_posture_evidence_ref(format!(
                    "incident_monitor.status.destination_readiness[{}]",
                    readiness.destination_id
                ))],
                None,
                include_draft_suggestions,
            ));
            continue;
        }
        let pass = block_unready;
        results.push(incident_monitor_assessment_result(
            "destination_readiness_fail_closed",
            json!({
                "kind": "incident_monitor_destination",
                "id": readiness.destination_id,
                "destination_kind": readiness.kind.clone(),
            }),
            "Incomplete external destinations must be blocked by block_unready_destinations.",
            if pass {
                format!(
                    "Destination is not publish-ready, and block_unready_destinations is enabled: {}",
                    readiness.missing.join(", ")
                )
            } else {
                format!(
                    "Destination is not publish-ready while block_unready_destinations is disabled: {}",
                    readiness.missing.join(", ")
                )
            },
            if pass { "pass" } else { "fail" },
            vec![
                incident_monitor_posture_evidence_ref(format!(
                    "incident_monitor.status.destination_readiness[{}]",
                    readiness.destination_id
                )),
                incident_monitor_posture_evidence_ref(
                    "inventory.incident_monitor.safety_defaults.block_unready_destinations",
                ),
            ],
            None,
            include_draft_suggestions,
        ));
    }
    if evaluated == 0 {
        results.push(incident_monitor_assessment_result(
            "destination_readiness_fail_closed",
            json!({
                "kind": "incident_monitor_destinations",
                "id": "none",
            }),
            "External destinations should have publish readiness coverage.",
            "No enabled external destinations are configured.".to_string(),
            "blocked",
            vec![incident_monitor_posture_evidence_ref(
                "incident_monitor.status.destination_readiness",
            )],
            None,
            include_draft_suggestions,
        ));
    }
}

fn incident_monitor_assessment_probe_scoped_intake(
    authority_inventory: &Value,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    let keys = incident_monitor_posture_array(
        authority_inventory
            .get("inventory")
            .unwrap_or(&Value::Null),
        &["scoped_intake_keys"],
    );
    let mut evaluated = 0usize;
    for key in keys {
        if !key.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        evaluated += 1;
        let key_id = incident_monitor_posture_str(key, "key_id").unwrap_or("unknown_intake_key");
        let project_id = incident_monitor_posture_str(key, "project_id").unwrap_or("unknown_project");
        let scopes = incident_monitor_posture_string_array(key, &["scopes"]);
        let report_only =
            !scopes.is_empty() && scopes.iter().all(|scope| scope == "incident_monitor:report");
        results.push(incident_monitor_assessment_result(
            "scoped_intake_restriction",
            json!({
                "kind": "incident_monitor_intake_key",
                "id": key_id,
                "project_id": project_id,
            }),
            "Scoped intake keys must be report-only and unable to invoke privileged Incident Monitor routes.",
            if report_only {
                "Intake key is scoped to incident_monitor:report; privileged probe routes reject intake-key authentication."
                    .to_string()
            } else {
                format!(
                    "Intake key has non-report scopes or missing report scope: {}",
                    scopes.join(", ")
                )
            },
            if report_only { "pass" } else { "fail" },
            vec![
                incident_monitor_posture_evidence_ref(format!(
                    "inventory.scoped_intake_keys[{key_id}].scopes"
                )),
                incident_monitor_posture_evidence_ref(
                    "incident_monitor.security.assessment_probes.intake_key_rejection",
                ),
            ],
            None,
            include_draft_suggestions,
        ));
    }
    if evaluated == 0 {
        results.push(incident_monitor_assessment_result(
            "scoped_intake_restriction",
            json!({
                "kind": "incident_monitor_intake_keys",
                "id": "none",
            }),
            "Scoped intake keys should be report-only when configured.",
            "No enabled scoped intake keys are configured.".to_string(),
            "blocked",
            vec![incident_monitor_posture_evidence_ref(
                "inventory.scoped_intake_keys",
            )],
            None,
            include_draft_suggestions,
        ));
    }
}

fn incident_monitor_assessment_probe_mcp_allowlist(
    authority_inventory: &Value,
    status: &crate::IncidentMonitorStatus,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    incident_monitor_assessment_probe_posture_rule(
        authority_inventory,
        "mcp_tool_allowlist",
        "mcp_server_without_tool_allowlist",
        "MCP server access must be pinned to explicit tool names.",
        "No automation agents with MCP server access and missing allowed_tools were found.",
        include_draft_suggestions,
        results,
    );

    let mut evaluated_destinations = 0usize;
    for readiness in &status.destination_readiness {
        if !matches!(&readiness.kind, IncidentMonitorDestinationKind::McpTool) || !readiness.enabled {
            continue;
        }
        evaluated_destinations += 1;
        results.push(incident_monitor_assessment_result(
            "mcp_tool_allowlist",
            json!({
                "kind": "incident_monitor_destination",
                "id": readiness.destination_id,
                "destination_kind": readiness.kind.clone(),
            }),
            "MCP destinations must target one configured tool, require explicit allow_publish, and have payload mapping evidence.",
            if readiness.publish_ready {
                "MCP destination readiness confirms a single configured publish tool and explicit allow_publish policy."
                    .to_string()
            } else {
                format!(
                    "MCP destination readiness is incomplete: {}",
                    readiness.missing.join(", ")
                )
            },
            if readiness.publish_ready { "pass" } else { "fail" },
            vec![incident_monitor_posture_evidence_ref(format!(
                "incident_monitor.status.destination_readiness[{}]",
                readiness.destination_id
            ))],
            None,
            include_draft_suggestions,
        ));
    }
    if evaluated_destinations == 0 {
        results.push(incident_monitor_assessment_result(
            "mcp_tool_allowlist",
            json!({
                "kind": "incident_monitor_destination",
                "id": "no_mcp_destinations",
            }),
            "MCP destinations should be explicitly allowlisted when configured.",
            "No enabled MCP tool destinations are configured.".to_string(),
            "blocked",
            vec![incident_monitor_posture_evidence_ref(
                "incident_monitor.status.destination_readiness[kind=mcp_tool]",
            )],
            None,
            include_draft_suggestions,
        ));
    }
}

fn incident_monitor_assessment_probe_webhook_url_policy(
    status: &crate::IncidentMonitorStatus,
    include_draft_suggestions: bool,
    results: &mut Vec<Value>,
) {
    let mut evaluated = 0usize;
    for readiness in &status.destination_readiness {
        if !matches!(&readiness.kind, IncidentMonitorDestinationKind::Webhook) || !readiness.enabled {
            continue;
        }
        evaluated += 1;
        results.push(incident_monitor_assessment_result(
            "webhook_url_policy",
            json!({
                "kind": "incident_monitor_destination",
                "id": readiness.destination_id,
                "destination_kind": readiness.kind.clone(),
            }),
            "Webhook destinations must use public HTTPS URLs, reject local/private hosts, and have an env-backed signing secret.",
            if readiness.publish_ready {
                "Webhook readiness confirms public URL policy and signing secret availability.".to_string()
            } else {
                format!(
                    "Webhook readiness is incomplete: {}",
                    readiness.missing.join(", ")
                )
            },
            if readiness.publish_ready { "pass" } else { "fail" },
            vec![incident_monitor_posture_evidence_ref(format!(
                "incident_monitor.status.destination_readiness[{}]",
                readiness.destination_id
            ))],
            None,
            include_draft_suggestions,
        ));
    }
    if evaluated == 0 {
        results.push(incident_monitor_assessment_result(
            "webhook_url_policy",
            json!({
                "kind": "incident_monitor_destination",
                "id": "no_webhook_destinations",
            }),
            "Webhook URL policy should be enforced when webhook destinations are configured.",
            "No enabled webhook destinations are configured.".to_string(),
            "blocked",
            vec![incident_monitor_posture_evidence_ref(
                "incident_monitor.status.destination_readiness[kind=webhook]",
            )],
            None,
            include_draft_suggestions,
        ));
    }
}

fn incident_monitor_assessment_posture_findings(
    authority_inventory: &Value,
    posture_rule_id: &str,
) -> Vec<Value> {
    let enabled_rules = [posture_rule_id.to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let policy = IncidentMonitorPostureRulePolicy {
        mode: "dry_run".to_string(),
        enabled_rules,
        disabled_rules: std::collections::HashSet::new(),
        min_severity_rank: incident_monitor_posture_severity_rank("low"),
    };
    incident_monitor_security_posture_findings(authority_inventory, &policy)
        .into_iter()
        .filter(|finding| finding.get("rule_id").and_then(Value::as_str) == Some(posture_rule_id))
        .collect()
}

fn incident_monitor_assessment_result(
    probe_id: &str,
    target: Value,
    expected_behavior: &str,
    observed_behavior: String,
    status: &str,
    evidence_refs: Vec<Value>,
    source_finding: Option<Value>,
    include_draft_suggestions: bool,
) -> Value {
    let target_id = target
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown_target");
    let hash = crate::sha256_hex(&[probe_id, target_id, status, observed_behavior.as_str()]);
    let fingerprint = format!("sha256:{hash}");
    let finding_id = (status == "fail").then(|| {
        format!(
            "bap_{}",
            hash.get(..16).unwrap_or(hash.as_str())
        )
    });
    let evidence_excerpt = evidence_refs
        .iter()
        .filter_map(|evidence| evidence.get("path").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let incident_draft_suggestion = if status == "fail" && include_draft_suggestions {
        Some(json!({
            "source": "security_assessment_probe",
            "event_type": "security.assessment_probe.failed",
            "title": format!("Security assessment probe `{probe_id}` failed for `{target_id}`"),
            "fingerprint": fingerprint,
            "detail": format!(
                "Expected behavior: {expected_behavior}\n\nObserved behavior: {observed_behavior}"
            ),
            "risk_level": "high",
            "risk_category": "security_gap",
            "confidence": "high",
            "route_tags": ["security_assessment", probe_id],
            "excerpt": evidence_excerpt,
        }))
    } else {
        None
    };

    json!({
        "probe_id": probe_id,
        "target": target,
        "expected_behavior": expected_behavior,
        "observed_behavior": observed_behavior,
        "status": status,
        "passed": status == "pass",
        "blocked": status == "blocked",
        "fingerprint": fingerprint,
        "finding_id": finding_id,
        "evidence_refs": evidence_refs,
        "source_finding": source_finding,
        "incident_draft_suggestion": incident_draft_suggestion,
        "dry_run": true,
    })
}

fn incident_monitor_assessment_counts(results: &[Value]) -> Value {
    let mut status_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut draft_suggestions = 0usize;
    for result in results {
        if let Some(status) = result.get("status").and_then(Value::as_str) {
            *status_counts.entry(status.to_string()).or_insert(0) += 1;
        }
        if result
            .get("incident_draft_suggestion")
            .is_some_and(|value| !value.is_null())
        {
            draft_suggestions += 1;
        }
    }
    json!({
        "results": results.len(),
        "pass": status_counts.get("pass").copied().unwrap_or(0),
        "fail": status_counts.get("fail").copied().unwrap_or(0),
        "blocked": status_counts.get("blocked").copied().unwrap_or(0),
        "by_status": status_counts,
        "draft_suggestions": draft_suggestions,
    })
}

fn incident_monitor_assessment_selected_probe_ids(requested: &[String]) -> Vec<String> {
    let mut selected = if requested.is_empty() {
        INCIDENT_MONITOR_ASSESSMENT_PROBES
            .iter()
            .map(|probe_id| (*probe_id).to_string())
            .collect::<Vec<_>>()
    } else {
        requested
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| INCIDENT_MONITOR_ASSESSMENT_PROBES.iter().any(|probe| probe == value))
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    selected.sort();
    selected.dedup();
    selected
}

fn incident_monitor_assessment_mode(mode: Option<&str>) -> String {
    match mode
        .unwrap_or("dry_run")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "disabled" | "disable" | "off" => "disabled".to_string(),
        _ => "dry_run".to_string(),
    }
}

async fn incident_monitor_assessment_persist_evidence_pack(
    state: &AppState,
    tenant_context: TenantContext,
    generated_at_ms: u64,
    payload: &Value,
) -> Result<Value, StatusCode> {
    let run_id = format!(
        "incident-monitor-assessment-probes-{}-{}",
        generated_at_ms,
        Uuid::new_v4().simple()
    );
    let artifact_id = format!("incident-monitor-assessment-probes-{}", Uuid::new_v4().simple());
    super::context_runs::context_run_create_impl(
        state.clone(),
        tenant_context,
        ContextRunCreateInput {
            run_id: Some(run_id.clone()),
            objective: "Run Incident Monitor security assessment probes and persist evidence.".to_string(),
            run_type: Some("incident_monitor_assessment_probes".to_string()),
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
        "incident_monitor_assessment_probe_results",
        "artifacts/incident_monitor.assessment_probes.json",
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
        "artifact_type": "incident_monitor_assessment_probe_results",
        "path": context_run_dir(state, &run_id)
            .join("artifacts/incident_monitor.assessment_probes.json")
            .to_string_lossy()
            .to_string(),
    }))
}

fn incident_monitor_assessment_request_has_api_token(headers: &HeaderMap, required: &str) -> bool {
    if headers
        .get("x-agent-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .is_some_and(|value| crate::constant_time_str_eq(value, required))
    {
        return true;
    }
    if headers
        .get("x-tandem-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .is_some_and(|value| crate::constant_time_str_eq(value, required))
    {
        return true;
    }
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(str::trim)
        .is_some_and(|value| crate::constant_time_str_eq(value, required))
}
