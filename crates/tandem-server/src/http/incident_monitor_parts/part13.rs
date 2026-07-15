// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

const INCIDENT_MONITOR_DEPLOYMENT_CARDS_SCHEMA_VERSION: u64 = 1;

const INCIDENT_MONITOR_DEPLOYMENT_CARD_REQUIRED_FIELDS: [&str; 8] = [
    "intended_purpose",
    "business_owner",
    "accountable_team",
    "autonomy_level",
    "data_classification",
    "approval_protocol",
    "escalation_protocol",
    "review_cadence_days",
];

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct IncidentMonitorDeploymentCardMetadata {
    #[serde(default, alias = "purpose")]
    pub intended_purpose: Option<String>,
    #[serde(default)]
    pub business_owner: Option<String>,
    #[serde(default)]
    pub accountable_team: Option<String>,
    #[serde(default)]
    pub autonomy_level: Option<String>,
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    #[serde(default)]
    pub prohibited_actions: Vec<String>,
    #[serde(default)]
    pub data_classification: Option<String>,
    #[serde(default)]
    pub approval_protocol: Option<String>,
    #[serde(default)]
    pub escalation_protocol: Option<String>,
    #[serde(default)]
    pub known_limitations: Vec<String>,
    #[serde(default)]
    pub residual_risks: Vec<String>,
    #[serde(default)]
    pub review_cadence_days: Option<u64>,
    #[serde(default)]
    pub next_reassessment_at_ms: Option<u64>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct IncidentMonitorDeploymentCardsInput {
    #[serde(default)]
    pub defaults: IncidentMonitorDeploymentCardMetadata,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, IncidentMonitorDeploymentCardMetadata>,
    #[serde(default)]
    pub include_markdown: Option<bool>,
    #[serde(default)]
    pub include_raw_inventory: Option<bool>,
}

struct IncidentMonitorDeploymentCardSeed {
    card_id: String,
    card_kind: String,
    target_kind: String,
    target_id: String,
    target_name: String,
    system_boundary: String,
    source_kind: Option<String>,
    fallback_purpose: Option<String>,
    allowed_actions: Vec<String>,
    prohibited_actions: Vec<String>,
    tools: Vec<String>,
    mcp_servers: Vec<String>,
    destinations: Vec<String>,
    routes: Vec<String>,
    data_sources: Vec<String>,
    tenant_scope: Value,
    evidence_refs: Vec<String>,
    authority: Value,
}

pub(super) async fn generate_incident_monitor_deployment_cards(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Json(input): Json<IncidentMonitorDeploymentCardsInput>,
) -> Response {
    if incident_monitor_assessment_has_scoped_intake_key_header(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Incident Monitor deployment cards require an admin API token, not a scoped intake key",
                "code": "INCIDENT_MONITOR_DEPLOYMENT_CARDS_ADMIN_REQUIRED",
            })),
        )
            .into_response();
    }
    if let Some(required_token) = state.api_token().await {
        if !incident_monitor_assessment_request_has_api_token(&headers, &required_token) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Incident Monitor deployment cards require the full admin API token",
                    "code": "INCIDENT_MONITOR_DEPLOYMENT_CARDS_ADMIN_REQUIRED",
                })),
            )
                .into_response();
        }
    }

    let generated_at_ms = crate::now_ms();
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let payload = incident_monitor_deployment_cards_payload(
        &state,
        tenant_context.clone(),
        verified,
        &input,
        generated_at_ms,
    )
    .await;

    incident_monitor_assessment_report_emit_audit_event(
        &state,
        &tenant_context,
        "incident_monitor.deployment_cards.generated",
        json!({
            "source_kind": "tandem_monitor",
            "schema_version": INCIDENT_MONITOR_DEPLOYMENT_CARDS_SCHEMA_VERSION,
            "generated_at_ms": generated_at_ms,
            "counts": payload["counts"].clone(),
            "raw_inventory_included": false,
        }),
    )
    .await;

    Json(payload).into_response()
}

async fn incident_monitor_deployment_cards_payload(
    state: &AppState,
    tenant_context: TenantContext,
    verified: Option<&VerifiedTenantContext>,
    input: &IncidentMonitorDeploymentCardsInput,
    generated_at_ms: u64,
) -> Value {
    let authority_inventory =
        incident_monitor_authority_inventory_payload(state, tenant_context.clone(), verified).await;
    let policy = IncidentMonitorPostureRulePolicy {
        mode: "dry_run".to_string(),
        enabled_rules: INCIDENT_MONITOR_POSTURE_RULES
            .iter()
            .map(|(rule_id, _, _)| (*rule_id).to_string())
            .collect::<std::collections::HashSet<_>>(),
        disabled_rules: std::collections::HashSet::new(),
        min_severity_rank: incident_monitor_posture_severity_rank("low"),
    };
    let posture_findings = incident_monitor_security_posture_findings(&authority_inventory, &policy);
    let inventory = authority_inventory
        .get("inventory")
        .cloned()
        .unwrap_or(Value::Null);
    let mut findings = Vec::new();
    let mut cards = Vec::new();

    cards.push(incident_monitor_deployment_card_from_seed(
        incident_monitor_deployment_card_seed_for_monitor(&inventory, &tenant_context),
        input,
        &posture_findings,
        &mut findings,
    ));

    for automation in incident_monitor_posture_array(&inventory, &["automation_specs"]) {
        cards.push(incident_monitor_deployment_card_from_seed(
            incident_monitor_deployment_card_seed_for_automation(automation),
            input,
            &posture_findings,
            &mut findings,
        ));
    }
    for workflow in incident_monitor_posture_array(&inventory, &["workflows"]) {
        cards.push(incident_monitor_deployment_card_from_seed(
            incident_monitor_deployment_card_seed_for_workflow(workflow),
            input,
            &posture_findings,
            &mut findings,
        ));
    }
    for source in incident_monitor_posture_array(&inventory, &["monitored_sources"]) {
        cards.push(incident_monitor_deployment_card_from_seed(
            incident_monitor_deployment_card_seed_for_source(source),
            input,
            &posture_findings,
            &mut findings,
        ));
    }

    // TAN-490: attach live continuous-reassessment schedule status (last
    // completed / next due / overdue) per scope so operators can see it on the
    // deployment cards, not just as operator-declared metadata.
    let reassessment_schedule =
        crate::incident_monitor_reassessment::incident_monitor_reassessment_schedule_status(
            state,
            &tenant_context,
            generated_at_ms,
        )
        .await;

    let markdown_export = input.include_markdown.unwrap_or(true).then(|| {
        incident_monitor_deployment_cards_markdown(generated_at_ms, &tenant_context, &cards, &findings)
    });

    json!({
        "schema_version": INCIDENT_MONITOR_DEPLOYMENT_CARDS_SCHEMA_VERSION,
        "generated_at_ms": generated_at_ms,
        "scope": {
            "source": "incident_monitor_deployment_cards",
            "read_only": true,
            "dry_run": true,
            "mutates_external_systems": false,
            "tenant_context": tenant_context,
            "raw_inventory_included": false,
            "raw_inventory_request_ignored": input.include_raw_inventory.unwrap_or(false),
        },
        "card_policy": {
            "required_fields": INCIDENT_MONITOR_DEPLOYMENT_CARD_REQUIRED_FIELDS,
            "metadata_card_ids": input.metadata.keys().cloned().collect::<Vec<_>>(),
            "supports_operator_metadata": true,
            "metadata_keys": [
                "intended_purpose",
                "business_owner",
                "accountable_team",
                "autonomy_level",
                "allowed_actions",
                "prohibited_actions",
                "data_classification",
                "approval_protocol",
                "escalation_protocol",
                "known_limitations",
                "residual_risks",
                "review_cadence_days",
                "next_reassessment_at_ms",
                "evidence_refs"
            ],
        },
        "counts": {
            "cards": cards.len(),
            "by_card_kind": incident_monitor_assessment_report_count_values(&cards, "card_kind"),
            "findings": findings.len(),
            "missing_required_fields": findings.iter()
                .filter(|finding| finding.get("rule_id").and_then(Value::as_str) == Some("deployment_card_required_field_missing"))
                .count(),
            "posture_findings_linked": cards.iter()
                .filter_map(|card| card.pointer("/linked_evidence/posture_finding_refs").and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>(),
        },
        "reassessment": {
            "source": "incident_monitor_continuous_reassessment",
            "overdue_scopes": reassessment_schedule.iter().filter(|status| status.overdue).count(),
            "schedule": reassessment_schedule,
        },
        "cards": cards,
        "findings": findings,
        "markdown_export": markdown_export,
        "authority_inventory": {
            "schema_version": authority_inventory.get("schema_version").cloned().unwrap_or(Value::Null),
            "generated_at_ms": authority_inventory.get("generated_at_ms").cloned().unwrap_or(Value::Null),
            "counts": authority_inventory.get("counts").cloned().unwrap_or(Value::Null),
        },
        "sensitive_values": {
            "policy": "redacted_or_summarized",
            "raw_inventory": "omitted_by_default",
            "omitted_fields": [
                "incident_monitor_intake_key.key_hash",
                "incident_monitor_intake_key.raw_key",
                "destination.webhook_secret_ref_value",
                "destination.config_values",
                "workflow.action.with_values",
                "automation.model_policy_values",
                "automation.node.metadata_values",
                "mcp.secret_refs",
                "mcp.credential_binding",
                "mcp.last_auth_challenge",
                "protected_audit.payload"
            ],
        },
    })
}

fn incident_monitor_deployment_card_seed_for_monitor(
    inventory: &Value,
    tenant_context: &TenantContext,
) -> IncidentMonitorDeploymentCardSeed {
    let incident_monitor = inventory.get("incident_monitor").cloned().unwrap_or(Value::Null);
    let destinations = incident_monitor_posture_array(inventory, &["destinations"])
        .iter()
        .filter_map(|row| incident_monitor_deployment_card_value_text(row, "destination_id"))
        .collect::<Vec<_>>();
    let routes = incident_monitor_posture_array(inventory, &["routes"])
        .iter()
        .filter_map(|row| incident_monitor_deployment_card_value_text(row, "route_id"))
        .collect::<Vec<_>>();
    IncidentMonitorDeploymentCardSeed {
        card_id: "tandem:self_monitoring:incident_monitor".to_string(),
        card_kind: "tandem_self_monitoring".to_string(),
        target_kind: "incident_monitor".to_string(),
        target_id: "incident_monitor".to_string(),
        target_name: "Tandem Incident Monitor".to_string(),
        system_boundary: "tandem_self_monitoring".to_string(),
        source_kind: Some("tandem_monitor".to_string()),
        fallback_purpose: Some(
            "Monitor incident signals, route governed evidence, and preserve production governance records."
                .to_string(),
        ),
        allowed_actions: vec![
            "Observe configured incident and safety events".to_string(),
            "Preview and route incident evidence to configured destinations".to_string(),
            "Emit protected audit evidence for monitor activity".to_string(),
        ],
        prohibited_actions: vec![
            "Bypass configured approval, route, readiness, or redaction policy".to_string(),
        ],
        tools: Vec::new(),
        mcp_servers: Vec::new(),
        destinations,
        routes,
        data_sources: incident_monitor_posture_array(inventory, &["monitored_sources"])
            .iter()
            .filter_map(incident_monitor_deployment_card_source_ref)
            .collect::<Vec<_>>(),
        tenant_scope: json!({
            "org_id": tenant_context.org_id,
            "workspace_id": tenant_context.workspace_id,
            "deployment_id": tenant_context.deployment_id,
        }),
        evidence_refs: vec!["authority_inventory:incident_monitor".to_string()],
        authority: json!({
            "incident_monitor": incident_monitor,
            "configured_destinations": incident_monitor_posture_array(inventory, &["destinations"]).len(),
            "routes": incident_monitor_posture_array(inventory, &["routes"]).len(),
            "monitored_sources": incident_monitor_posture_array(inventory, &["monitored_sources"]).len(),
        }),
    }
}

fn incident_monitor_deployment_card_seed_for_automation(
    automation: &Value,
) -> IncidentMonitorDeploymentCardSeed {
    let automation_id = incident_monitor_deployment_card_value_text(automation, "automation_id")
        .unwrap_or_else(|| "unknown_automation".to_string());
    let target_name = incident_monitor_deployment_card_value_text(automation, "name")
        .unwrap_or_else(|| automation_id.clone());
    let mut tools = std::collections::BTreeSet::new();
    let mut mcp_servers = std::collections::BTreeSet::new();
    for agent in incident_monitor_posture_array(automation, &["agents"]) {
        incident_monitor_deployment_card_extend_text_set(
            &mut tools,
            agent
                .pointer("/tool_policy/allowlist")
                .and_then(Value::as_array),
        );
        incident_monitor_deployment_card_extend_text_set(
            &mut mcp_servers,
            agent
                .pointer("/mcp_policy/effective_allowed_servers")
                .and_then(Value::as_array),
        );
        incident_monitor_deployment_card_extend_text_set(
            &mut tools,
            agent
                .pointer("/mcp_policy/allowed_tools")
                .and_then(Value::as_array),
        );
    }
    for node in incident_monitor_posture_array(automation, &["nodes"]) {
        incident_monitor_deployment_card_extend_text_set(
            &mut tools,
            node.pointer("/tool_policy/allowlist")
                .and_then(Value::as_array),
        );
        incident_monitor_deployment_card_extend_text_set(
            &mut mcp_servers,
            node.pointer("/mcp_policy/effective_allowed_servers")
                .and_then(Value::as_array),
        );
    }
    let tools = tools.into_iter().collect::<Vec<_>>();
    let mcp_servers = mcp_servers.into_iter().collect::<Vec<_>>();
    let output_targets =
        incident_monitor_deployment_card_value_string_array(automation, "output_targets");
    IncidentMonitorDeploymentCardSeed {
        card_id: format!("automation:{automation_id}"),
        card_kind: "automation".to_string(),
        target_kind: "automation".to_string(),
        target_id: automation_id.clone(),
        target_name,
        system_boundary: "tandem_runtime".to_string(),
        source_kind: Some("agent_runtime".to_string()),
        fallback_purpose: incident_monitor_deployment_card_value_text(automation, "description"),
        allowed_actions: if output_targets.is_empty() && tools.is_empty() {
            vec!["Run configured automation flow".to_string()]
        } else {
            output_targets
                .iter()
                .map(|target| format!("Publish to output target `{target}`"))
                .chain(tools.iter().map(|tool| format!("Use tool `{tool}`")))
                .collect::<Vec<_>>()
        },
        prohibited_actions: vec![
            "Use tools, MCP servers, or destinations outside the automation policy".to_string(),
        ],
        tools,
        mcp_servers,
        destinations: output_targets,
        routes: Vec::new(),
        data_sources: Vec::new(),
        tenant_scope: automation
            .get("tenant_context")
            .cloned()
            .unwrap_or(Value::Null),
        evidence_refs: vec![
            format!("authority_inventory:automation:{automation_id}"),
            format!("automation:{automation_id}"),
        ],
        authority: json!({
            "status": automation.get("status").cloned().unwrap_or(Value::Null),
            "agents": automation.get("agents").cloned().unwrap_or_else(|| json!([])),
            "nodes": automation.get("nodes").cloned().unwrap_or_else(|| json!([])),
            "output_targets": automation.get("output_targets").cloned().unwrap_or_else(|| json!([])),
            "scope_policy_present": automation.get("scope_policy_present").cloned().unwrap_or(Value::Null),
        }),
    }
}

fn incident_monitor_deployment_card_seed_for_workflow(workflow: &Value) -> IncidentMonitorDeploymentCardSeed {
    let workflow_id = incident_monitor_deployment_card_value_text(workflow, "workflow_id")
        .unwrap_or_else(|| "unknown_workflow".to_string());
    let target_name = incident_monitor_deployment_card_value_text(workflow, "name")
        .unwrap_or_else(|| workflow_id.clone());
    let mut actions = std::collections::BTreeSet::new();
    for step in incident_monitor_posture_array(workflow, &["steps"]) {
        if let Some(action) = incident_monitor_deployment_card_value_text(step, "action") {
            actions.insert(action);
        }
    }
    for hook in incident_monitor_posture_array(workflow, &["hooks"]) {
        for action in incident_monitor_posture_array(hook, &["actions"]) {
            if let Some(action_name) = incident_monitor_deployment_card_value_text(action, "action") {
                actions.insert(action_name);
            }
        }
    }
    let actions = actions.into_iter().collect::<Vec<_>>();
    IncidentMonitorDeploymentCardSeed {
        card_id: format!("workflow:{workflow_id}"),
        card_kind: "workflow".to_string(),
        target_kind: "workflow".to_string(),
        target_id: workflow_id.clone(),
        target_name,
        system_boundary: "tandem_runtime".to_string(),
        source_kind: Some("tandem_runtime".to_string()),
        fallback_purpose: incident_monitor_deployment_card_value_text(workflow, "description"),
        allowed_actions: if actions.is_empty() {
            vec!["Run configured workflow steps".to_string()]
        } else {
            actions
                .iter()
                .map(|action| format!("Run workflow action `{action}`"))
                .collect()
        },
        prohibited_actions: vec![
            "Execute workflow actions outside configured step or hook definitions".to_string(),
        ],
        tools: actions,
        mcp_servers: Vec::new(),
        destinations: Vec::new(),
        routes: Vec::new(),
        data_sources: Vec::new(),
        tenant_scope: Value::Null,
        evidence_refs: vec![
            format!("authority_inventory:workflow:{workflow_id}"),
            format!("workflow:{workflow_id}"),
        ],
        authority: json!({
            "enabled": workflow.get("enabled").cloned().unwrap_or(Value::Null),
            "source": workflow.get("source").cloned().unwrap_or(Value::Null),
            "steps": workflow.get("steps").cloned().unwrap_or_else(|| json!([])),
            "hooks": workflow.get("hooks").cloned().unwrap_or_else(|| json!([])),
        }),
    }
}

fn incident_monitor_deployment_card_seed_for_source(source: &Value) -> IncidentMonitorDeploymentCardSeed {
    let source_ref = incident_monitor_deployment_card_source_ref(source)
        .unwrap_or_else(|| "monitored_source:unknown".to_string());
    let source_kind = incident_monitor_deployment_card_value_text(source, "source_kind");
    let name = incident_monitor_deployment_card_value_text(source, "name")
        .unwrap_or_else(|| source_ref.clone());
    let project_id = incident_monitor_deployment_card_value_text(source, "project_id");
    let source_id = incident_monitor_deployment_card_value_text(source, "source_id");
    let destinations =
        incident_monitor_deployment_card_value_string_array(source, "allowed_destination_ids")
            .into_iter()
            .chain(incident_monitor_deployment_card_value_string_array(
                source,
                "default_destination_ids",
            ))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
    IncidentMonitorDeploymentCardSeed {
        card_id: source_ref.clone(),
        card_kind: "monitored_source".to_string(),
        target_kind: "monitored_source".to_string(),
        target_id: source_ref.clone(),
        target_name: name.clone(),
        system_boundary: incident_monitor_deployment_card_boundary(source_kind.as_deref()).to_string(),
        source_kind: source_kind.clone(),
        fallback_purpose: Some(format!("Monitor incident and safety signals from {name}.")),
        allowed_actions: vec![
            "Accept scoped incident reports from this source".to_string(),
            "Route source evidence only to configured destinations".to_string(),
        ],
        prohibited_actions: vec![
            "Use source data outside tenant/workspace, redaction, retention, or route policy"
                .to_string(),
        ],
        tools: Vec::new(),
        mcp_servers: incident_monitor_deployment_card_value_text(source, "mcp_server")
            .into_iter()
            .collect(),
        destinations,
        routes: incident_monitor_deployment_card_value_string_array(source, "default_route_tags"),
        data_sources: vec![source_ref.clone()],
        tenant_scope: json!({
            "tenant_id": source.get("tenant_id").cloned().unwrap_or(Value::Null),
            "workspace_id": source.get("workspace_id").cloned().unwrap_or(Value::Null),
            "project_id": project_id,
            "source_id": source_id,
        }),
        evidence_refs: vec![format!("authority_inventory:{source_ref}")],
        authority: json!({
            "enabled": source.get("enabled").cloned().unwrap_or(Value::Null),
            "paused": source.get("paused").cloned().unwrap_or(Value::Null),
            "source_kind": source_kind,
            "approval_policy": source.get("approval_policy").cloned().unwrap_or(Value::Null),
            "redaction_profile_present": incident_monitor_deployment_card_value_has_text(source, "redaction_profile"),
            "retention_profile_present": incident_monitor_deployment_card_value_has_text(source, "retention_profile"),
            "event_schema_version": source.get("event_schema_version").cloned().unwrap_or(Value::Null),
            "data_readiness": source.get("data_readiness").cloned().unwrap_or(Value::Null),
            "source_readiness": source.get("readiness").cloned().unwrap_or(Value::Null),
        }),
    }
}

fn incident_monitor_deployment_card_from_seed(
    seed: IncidentMonitorDeploymentCardSeed,
    input: &IncidentMonitorDeploymentCardsInput,
    posture_findings: &[Value],
    findings: &mut Vec<Value>,
) -> Value {
    let metadata = input
        .metadata
        .get(&seed.card_id)
        .or_else(|| input.metadata.get(&seed.target_id));
    let purpose = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.intended_purpose,
        seed.fallback_purpose.clone(),
    );
    let business_owner = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.business_owner,
        None,
    );
    let accountable_team = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.accountable_team,
        None,
    );
    let autonomy_level = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.autonomy_level,
        None,
    );
    let data_classification = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.data_classification,
        None,
    );
    let approval_protocol = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.approval_protocol,
        None,
    );
    let escalation_protocol = incident_monitor_deployment_card_metadata_text(
        metadata,
        &input.defaults,
        |metadata| &metadata.escalation_protocol,
        None,
    );
    let review_cadence_days = metadata
        .and_then(|metadata| metadata.review_cadence_days)
        .or(input.defaults.review_cadence_days);
    let next_reassessment_at_ms = metadata
        .and_then(|metadata| metadata.next_reassessment_at_ms)
        .or(input.defaults.next_reassessment_at_ms);
    let posture_link_ids = incident_monitor_deployment_card_posture_link_ids(&seed);
    incident_monitor_deployment_card_missing_findings(
        &seed,
        &purpose,
        &business_owner,
        &accountable_team,
        &autonomy_level,
        &data_classification,
        &approval_protocol,
        &escalation_protocol,
        review_cadence_days,
        findings,
    );
    let allowed_actions = incident_monitor_deployment_card_metadata_list(
        metadata,
        &input.defaults,
        |metadata| &metadata.allowed_actions,
        seed.allowed_actions,
    );
    let prohibited_actions = incident_monitor_deployment_card_metadata_list(
        metadata,
        &input.defaults,
        |metadata| &metadata.prohibited_actions,
        seed.prohibited_actions,
    );
    let known_limitations = incident_monitor_deployment_card_metadata_list(
        metadata,
        &input.defaults,
        |metadata| &metadata.known_limitations,
        vec![
            "Generated from current Tandem authority inventory and operator metadata.".to_string(),
        ],
    );
    let residual_risks = incident_monitor_deployment_card_metadata_list(
        metadata,
        &input.defaults,
        |metadata| &metadata.residual_risks,
        vec!["Incomplete operator metadata reduces production governance confidence.".to_string()],
    );
    let operator_evidence_refs = incident_monitor_deployment_card_metadata_list(
        metadata,
        &input.defaults,
        |metadata| &metadata.evidence_refs,
        Vec::new(),
    );
    let posture_finding_refs =
        incident_monitor_deployment_card_linked_posture_findings(&posture_link_ids, posture_findings);

    json!({
        "card_id": seed.card_id,
        "card_kind": seed.card_kind,
        "target": {
            "kind": seed.target_kind,
            "id": seed.target_id,
            "name": seed.target_name,
        },
        "system_boundary": seed.system_boundary,
        "source_kind": seed.source_kind,
        "intended_purpose": purpose,
        "business_owner": business_owner,
        "accountable_team": accountable_team,
        "autonomy_level": autonomy_level,
        "allowed_actions": allowed_actions,
        "prohibited_actions": prohibited_actions,
        "authority": {
            "tools": seed.tools,
            "mcp_servers": seed.mcp_servers,
            "destinations": seed.destinations,
            "routes": seed.routes,
            "data_sources": seed.data_sources,
            "inventory": seed.authority,
        },
        "data": {
            "classification": data_classification,
            "tenant_scope": seed.tenant_scope,
        },
        "approval_and_escalation": {
            "approval_protocol": approval_protocol,
            "escalation_protocol": escalation_protocol,
        },
        "known_limitations": known_limitations,
        "residual_risks": residual_risks,
        "review": {
            "review_cadence_days": review_cadence_days,
            "next_reassessment_at_ms": next_reassessment_at_ms,
        },
        "linked_evidence": {
            "inventory_refs": seed.evidence_refs,
            "operator_refs": operator_evidence_refs,
            "posture_finding_refs": posture_finding_refs,
            "incident_refs": [],
            "evidence_pack_refs": [],
        },
    })
}

fn incident_monitor_deployment_card_missing_findings(
    seed: &IncidentMonitorDeploymentCardSeed,
    purpose: &Option<String>,
    business_owner: &Option<String>,
    accountable_team: &Option<String>,
    autonomy_level: &Option<String>,
    data_classification: &Option<String>,
    approval_protocol: &Option<String>,
    escalation_protocol: &Option<String>,
    review_cadence_days: Option<u64>,
    findings: &mut Vec<Value>,
) {
    let required = [
        ("intended_purpose", purpose.is_some()),
        ("business_owner", business_owner.is_some()),
        ("accountable_team", accountable_team.is_some()),
        ("autonomy_level", autonomy_level.is_some()),
        ("data_classification", data_classification.is_some()),
        ("approval_protocol", approval_protocol.is_some()),
        ("escalation_protocol", escalation_protocol.is_some()),
        ("review_cadence_days", review_cadence_days.is_some()),
    ];
    for (field, present) in required {
        if present {
            continue;
        }
        let severity = match field {
            "business_owner" | "accountable_team" | "escalation_protocol" => "high",
            _ => "medium",
        };
        findings.push(json!({
            "finding_id": format!("deployment_card_missing:{}:{}", seed.card_id, field),
            "fingerprint": format!("deployment_card:{}:missing:{}", seed.card_id, field),
            "rule_id": "deployment_card_required_field_missing",
            "category": "production_governance",
            "severity": severity,
            "title": format!("Deployment card missing `{field}`"),
            "detail": "Production deployment cards require explicit operator metadata before promotion.",
            "affected_objects": [{
                "kind": seed.target_kind,
                "id": seed.target_id,
                "card_id": seed.card_id,
                "field": field,
            }],
            "evidence_refs": [{
                "kind": "deployment_card",
                "card_id": seed.card_id,
            }],
            "recommendation": format!("Add `{field}` metadata for `{}` before production promotion.", seed.card_id),
            "dry_run": true,
        }));
    }
}

fn incident_monitor_deployment_card_posture_link_ids(
    seed: &IncidentMonitorDeploymentCardSeed,
) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    incident_monitor_deployment_card_insert_link_id(&mut ids, &seed.target_id);

    if seed.target_kind == "monitored_source" {
        if let Some(project_id) =
            incident_monitor_deployment_card_value_text(&seed.tenant_scope, "project_id")
        {
            incident_monitor_deployment_card_insert_link_id(&mut ids, &project_id);
        }
        if let Some(source_id) =
            incident_monitor_deployment_card_value_text(&seed.tenant_scope, "source_id")
        {
            incident_monitor_deployment_card_insert_link_id(&mut ids, &source_id);
        }
        if let Some(source_ref) = seed.target_id.strip_prefix("monitored_source:") {
            for part in source_ref.split(':') {
                incident_monitor_deployment_card_insert_link_id(&mut ids, part);
            }
        }
    }

    ids.into_iter().collect()
}

fn incident_monitor_deployment_card_insert_link_id(
    ids: &mut std::collections::BTreeSet<String>,
    value: &str,
) {
    let value = value.trim();
    if !value.is_empty() {
        ids.insert(value.to_string());
    }
}

fn incident_monitor_deployment_card_linked_posture_findings(
    target_ids: &[String],
    posture_findings: &[Value],
) -> Vec<String> {
    posture_findings
        .iter()
        .filter(|finding| {
            target_ids.iter().any(|target_id| {
                incident_monitor_deployment_card_value_contains_text(finding, target_id)
            })
        })
        .filter_map(|finding| incident_monitor_deployment_card_value_text(finding, "finding_id"))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn incident_monitor_deployment_card_metadata_text<F>(
    metadata: Option<&IncidentMonitorDeploymentCardMetadata>,
    defaults: &IncidentMonitorDeploymentCardMetadata,
    pick: F,
    fallback: Option<String>,
) -> Option<String>
where
    F: Fn(&IncidentMonitorDeploymentCardMetadata) -> &Option<String>,
{
    metadata
        .and_then(|metadata| incident_monitor_deployment_card_clean_text(pick(metadata)))
        .or_else(|| incident_monitor_deployment_card_clean_text(pick(defaults)))
        .or(fallback)
}

fn incident_monitor_deployment_card_metadata_list<F>(
    metadata: Option<&IncidentMonitorDeploymentCardMetadata>,
    defaults: &IncidentMonitorDeploymentCardMetadata,
    pick: F,
    fallback: Vec<String>,
) -> Vec<String>
where
    F: Fn(&IncidentMonitorDeploymentCardMetadata) -> &Vec<String>,
{
    if let Some(metadata) = metadata {
        let values = incident_monitor_deployment_card_clean_list(pick(metadata));
        if !values.is_empty() {
            return values;
        }
    }
    let values = incident_monitor_deployment_card_clean_list(pick(defaults));
    if values.is_empty() {
        fallback
    } else {
        values
    }
}

fn incident_monitor_deployment_card_clean_text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn incident_monitor_deployment_card_clean_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn incident_monitor_deployment_card_value_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn incident_monitor_deployment_card_value_has_text(value: &Value, key: &str) -> bool {
    incident_monitor_deployment_card_value_text(value, key).is_some()
}

fn incident_monitor_deployment_card_value_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn incident_monitor_deployment_card_source_ref(source: &Value) -> Option<String> {
    let project_id = incident_monitor_deployment_card_value_text(source, "project_id")?;
    let source_id = incident_monitor_deployment_card_value_text(source, "source_id")
        .unwrap_or_else(|| "project".to_string());
    Some(format!("monitored_source:{project_id}:{source_id}"))
}

fn incident_monitor_deployment_card_extend_text_set(
    set: &mut std::collections::BTreeSet<String>,
    values: Option<&Vec<Value>>,
) {
    if let Some(values) = values {
        for value in values {
            if let Some(text) = value
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                set.insert(text.to_string());
            }
        }
    }
}

fn incident_monitor_deployment_card_boundary(source_kind: Option<&str>) -> &'static str {
    match source_kind {
        Some("tandem_monitor") => "tandem_self_monitoring",
        Some("tandem_runtime") | Some("agent_runtime") | Some("mcp_gateway") => {
            "tandem_control_plane"
        }
        Some("external_app") | Some("ci") | Some("customer_system") => "customer_owned_system",
        _ => "unknown_or_operator_supplied",
    }
}

fn incident_monitor_deployment_card_value_contains_text(value: &Value, needle: &str) -> bool {
    if needle.trim().is_empty() {
        return false;
    }
    match value {
        Value::String(text) => text == needle,
        Value::Array(rows) => rows
            .iter()
            .any(|row| incident_monitor_deployment_card_value_contains_text(row, needle)),
        Value::Object(map) => map
            .values()
            .any(|row| incident_monitor_deployment_card_value_contains_text(row, needle)),
        _ => false,
    }
}

fn incident_monitor_deployment_cards_markdown(
    generated_at_ms: u64,
    tenant_context: &TenantContext,
    cards: &[Value],
    findings: &[Value],
) -> String {
    let mut lines = Vec::new();
    lines.push("# Incident Monitor Deployment Cards".to_string());
    lines.push(String::new());
    lines.push(format!("- Generated at: `{generated_at_ms}`"));
    lines.push(format!(
        "- Tenant: `{}/{}`",
        tenant_context.org_id, tenant_context.workspace_id
    ));
    lines.push(format!("- Cards: `{}`", cards.len()));
    lines.push(format!("- Findings: `{}`", findings.len()));
    lines.push(String::new());
    for card in cards {
        let card_id = card
            .get("card_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let target_name = card
            .pointer("/target/name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        lines.push(format!("## {target_name}"));
        lines.push(String::new());
        lines.push(format!("- Card ID: `{card_id}`"));
        lines.push(format!(
            "- Kind: `{}`",
            card.get("card_kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
        lines.push(format!(
            "- Boundary: `{}`",
            card.get("system_boundary")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
        if let Some(purpose) = card.get("intended_purpose").and_then(Value::as_str) {
            lines.push(format!("- Purpose: {purpose}"));
        }
        if let Some(owner) = card.get("business_owner").and_then(Value::as_str) {
            lines.push(format!("- Business owner: {owner}"));
        }
        if let Some(team) = card.get("accountable_team").and_then(Value::as_str) {
            lines.push(format!("- Accountable team: {team}"));
        }
        if let Some(escalation) = card
            .pointer("/approval_and_escalation/escalation_protocol")
            .and_then(Value::as_str)
        {
            lines.push(format!("- Escalation: {escalation}"));
        }
        lines.push(String::new());
    }
    if !findings.is_empty() {
        lines.push("## Required Metadata Findings".to_string());
        lines.push(String::new());
        for finding in findings.iter().take(50) {
            let title = finding
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Deployment card finding");
            let severity = finding
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("medium");
            lines.push(format!("- `{severity}` {title}"));
        }
    }
    lines.join("\n")
}
