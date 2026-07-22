// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::Extension;
use std::collections::HashSet;
use tandem_types::{ApprovalListFilter, RequestPrincipal, TenantContext, VerifiedTenantContext};

const INCIDENT_MONITOR_AUTHORITY_INVENTORY_SCHEMA_VERSION: u64 = 1;
const INCIDENT_MONITOR_AUTHORITY_INVENTORY_LIMIT: usize = 100;

pub(super) async fn get_incident_monitor_authority_inventory(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
) -> Response {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let actor = super::governance::resolve_governance_actor(
        &headers,
        &tenant_context,
        &request_principal,
    );
    Json(
        incident_monitor_authority_inventory_payload(
            &state,
            tenant_context,
            verified,
            Some(&actor),
        )
        .await,
    )
    .into_response()
}

async fn incident_monitor_authority_inventory_payload(
    state: &AppState,
    tenant_context: TenantContext,
    verified: Option<&VerifiedTenantContext>,
    approval_viewer: Option<&crate::automation_v2::governance::GovernanceActorRef>,
) -> Value {
    let config = state.incident_monitor_config().await;
    let destinations = config.effective_destinations();
    let routes = config.routes.clone();
    let workflows = state.list_workflows().await;
    let workflow_hooks = state.list_workflow_hooks(None).await;
    let automations = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter(|automation| super::tenant_matches(&tenant_context, &automation.tenant_context()))
        .filter(|automation| {
            super::routines_automations::automation_v2_visible_to_context(automation, verified)
        })
        .collect::<Vec<_>>();
    let intake_keys = state.list_incident_monitor_intake_keys().await;
    let mut governance_approvals = state
        .list_approval_requests_for_tenant(None, None, &tenant_context)
        .await;
    if let Some(actor) = approval_viewer {
        let standalone_local_owner = tenant_context.is_local_implicit()
            && actor.kind
                == crate::automation_v2::governance::GovernanceActorKind::System;
        let reviewer_wide = (actor.kind
            == crate::automation_v2::governance::GovernanceActorKind::Human
            || standalone_local_owner)
            && super::workflows::workflow_reviewer_is_eligible(&tenant_context, verified);
        if !reviewer_wide {
            let caller = actor
                .actor_id
                .as_deref()
                .or(actor.source.as_deref())
                .map(str::trim)
                .filter(|actor_id| !actor_id.is_empty());
            governance_approvals.retain(|approval| {
                caller.is_some_and(|caller| {
                    approval
                        .requested_by
                        .actor_id
                        .as_deref()
                        .or(approval.requested_by.source.as_deref())
                        .is_some_and(|requester| requester.eq_ignore_ascii_case(caller))
                })
            });
        }
    }
    let pending_approvals = crate::http::approvals::list_pending_approvals(
        &state,
        &ApprovalListFilter {
            org_id: Some(tenant_context.org_id.clone()),
            workspace_id: Some(tenant_context.workspace_id.clone()),
            source: None,
            limit: Some(INCIDENT_MONITOR_AUTHORITY_INVENTORY_LIMIT as u32),
        },
    )
    .await;
    let policy_decisions = state
        .list_policy_decisions(&tenant_context, INCIDENT_MONITOR_AUTHORITY_INVENTORY_LIMIT)
        .await;
    let external_actions = state
        .list_external_actions(INCIDENT_MONITOR_AUTHORITY_INVENTORY_LIMIT)
        .await;
    let mcp_snapshot = crate::http::mcp_inventory::mcp_inventory_snapshot(&state).await;
    let mcp_inventory = authority_mcp_inventory(mcp_snapshot);

    let log_watcher = state.incident_monitor_log_watcher_status.read().await.clone();
    let source_readiness = crate::incident_monitor::source_readiness::evaluate_source_readiness(
        &config,
        &log_watcher,
        crate::now_ms(),
    );
    let monitored_sources = incident_monitor_monitored_sources_inventory(&config, &source_readiness);

    // TAN-547: when the caller is tenant-scoped, drop config topology that
    // belongs to a *different* tenant so this inventory (and the assessment
    // report / deployment cards that reuse it) stays inside the tenant boundary,
    // matching the filtering already applied to automations and approvals.
    // Local/operator-implicit callers still see the full operator-global view.
    let (routes, intake_keys, monitored_sources, destinations) = if tenant_context
        .is_local_implicit()
    {
        (routes, intake_keys, monitored_sources, destinations)
    } else {
        let routes = routes
            .into_iter()
            .filter(|route| incident_monitor_route_visible_to_tenant(route, &tenant_context))
            .collect::<Vec<_>>();
        let intake_keys = intake_keys
            .into_iter()
            .filter(|key| {
                incident_monitor_intake_key_visible_to_tenant(key, &config, &tenant_context)
            })
            .collect::<Vec<_>>();
        let monitored_sources = monitored_sources
            .into_iter()
            .filter(|source| incident_monitor_inventory_value_visible_to_tenant(source, &tenant_context))
            .collect::<Vec<_>>();
        // Keep only destinations reachable from tenant-visible routes/projects
        // (plus the global defaults) so another tenant's destination targets
        // don't surface.
        let referenced = incident_monitor_tenant_referenced_destination_ids(
            &config,
            &routes,
            &tenant_context,
        );
        let destinations = destinations
            .into_iter()
            .filter(|destination| referenced.contains(&destination.destination_id))
            .collect::<Vec<_>>();
        (routes, intake_keys, monitored_sources, destinations)
    };

    let destination_inventory = destinations
        .iter()
        .map(incident_monitor_destination_inventory)
        .collect::<Vec<_>>();
    let route_inventory = routes
        .iter()
        .map(incident_monitor_route_inventory)
        .collect::<Vec<_>>();
    let automation_inventory = automations
        .iter()
        .map(incident_monitor_automation_inventory)
        .collect::<Vec<_>>();
    let workflow_inventory = workflows
        .iter()
        .map(|workflow| incident_monitor_workflow_inventory(workflow, &workflow_hooks))
        .collect::<Vec<_>>();

    json!({
        "schema_version": INCIDENT_MONITOR_AUTHORITY_INVENTORY_SCHEMA_VERSION,
        "generated_at_ms": crate::now_ms(),
        "scope": {
            "tenant_context": tenant_context,
            "source": "incident_monitor_authority_inventory",
            "read_only": true,
        },
        "inventory": {
            "workflows": workflow_inventory,
            "automation_specs": automation_inventory,
            "mcp": mcp_inventory,
            "incident_monitor": {
                "enabled": config.enabled,
                "paused": config.paused,
                "repo": config.repo,
                "workspace_root": config.workspace_root,
                "default_destination_ids": config.default_destination_ids,
                "model_policy_present": config.model_policy.is_some(),
                "provider_preference": config.provider_preference,
                "auto_create_new_issues": config.auto_create_new_issues,
                "require_approval_for_new_issues": config.require_approval_for_new_issues,
                "auto_comment_on_matched_open_issues": config.auto_comment_on_matched_open_issues,
                "safety_defaults": config.safety_defaults,
                "updated_at_ms": config.updated_at_ms,
            },
            "destinations": destination_inventory,
            "routes": route_inventory,
            "monitored_sources": monitored_sources,
            "scoped_intake_keys": intake_keys.iter().map(incident_monitor_intake_key_inventory).collect::<Vec<_>>(),
            "approval_rules": incident_monitor_approval_rules_inventory(&automations, &destinations, &routes, &config),
            "pending_approvals": pending_approvals.iter().map(incident_monitor_pending_approval_inventory).collect::<Vec<_>>(),
            "governance_approval_requests": governance_approvals.iter().map(incident_monitor_governance_approval_inventory).collect::<Vec<_>>(),
            "policy_decisions": policy_decisions.iter().map(incident_monitor_policy_decision_inventory).collect::<Vec<_>>(),
            "external_publish_surfaces": {
                "configured_destinations": destinations.iter().map(incident_monitor_external_publish_surface).collect::<Vec<_>>(),
                "recent_actions": external_actions.iter().map(incident_monitor_external_action_inventory).collect::<Vec<_>>(),
            },
        },
        "counts": {
            "workflows": workflows.len(),
            "automation_specs": automations.len(),
            "mcp_servers": mcp_inventory.get("servers").and_then(Value::as_array).map(|rows| rows.len()).unwrap_or(0),
            "destinations": destinations.len(),
            "routes": routes.len(),
            "monitored_sources": monitored_sources.len(),
            "scoped_intake_keys": intake_keys.len(),
            "pending_approvals": pending_approvals.len(),
            "governance_approval_requests": governance_approvals.len(),
            "policy_decisions": policy_decisions.len(),
            "external_actions": external_actions.len(),
        },
        "sensitive_values": {
            "policy": "redacted_or_summarized",
            "omitted_fields": [
                "incident_monitor_intake_key.key_hash",
                "incident_monitor_intake_key.raw_key",
                "destination.webhook_secret_ref_value",
                "destination.config_values",
                "workflow.action.with_values",
                "automation.model_policy_values",
                "automation.node.metadata_values",
                "source.data_readiness.allowed_use",
                "source.data_readiness.quality_notes",
                "source.data_readiness.legal_basis",
                "source.data_readiness.authorization_marker",
                "external_action.receipt",
                "external_action.metadata",
                "mcp.secret_refs",
                "mcp.credential_binding",
                "mcp.last_auth_challenge"
            ],
        },
    })
}

/// True unless the entry is explicitly bound to a *different* tenant. Unbound
/// (operator-global) entries and same-tenant entries stay visible; only another
/// tenant's clearly-attributed topology is hidden (TAN-547).
pub(crate) fn incident_monitor_entry_not_other_tenant(
    tenant_id: Option<&str>,
    workspace_id: Option<&str>,
    tenant_context: &TenantContext,
) -> bool {
    tenant_context.is_local_implicit()
        || (tenant_id.map_or(true, |value| value == tenant_context.org_id.as_str())
            && workspace_id.map_or(true, |value| value == tenant_context.workspace_id.as_str()))
}

pub(crate) fn incident_monitor_route_visible_to_tenant(
    route: &IncidentMonitorRouteConfig,
    tenant_context: &TenantContext,
) -> bool {
    if tenant_context.is_local_implicit() {
        return true;
    }
    let tenant_ok = route.match_tenant_ids.is_empty()
        || route
            .match_tenant_ids
            .iter()
            .any(|value| value == tenant_context.org_id.as_str());
    let workspace_ok = route.match_workspace_ids.is_empty()
        || route
            .match_workspace_ids
            .iter()
            .any(|value| value == tenant_context.workspace_id.as_str());
    tenant_ok && workspace_ok
}

fn incident_monitor_intake_key_visible_to_tenant(
    key: &crate::IncidentMonitorProjectIntakeKey,
    config: &IncidentMonitorConfig,
    tenant_context: &TenantContext,
) -> bool {
    match config
        .monitored_projects
        .iter()
        .find(|project| project.project_id == key.project_id)
    {
        Some(project) => incident_monitor_entry_not_other_tenant(
            project.tenant_id.as_deref(),
            project.workspace_id.as_deref(),
            tenant_context,
        ),
        // An intake key for an unknown/unbound project is not another tenant's.
        None => true,
    }
}

fn incident_monitor_inventory_value_visible_to_tenant(
    value: &Value,
    tenant_context: &TenantContext,
) -> bool {
    incident_monitor_entry_not_other_tenant(
        value.get("tenant_id").and_then(Value::as_str),
        value.get("workspace_id").and_then(Value::as_str),
        tenant_context,
    )
}

/// Destination ids reachable from the tenant's visible routes and projects, plus
/// the global defaults — the set of destinations a tenant-scoped inventory may
/// surface.
pub(crate) fn incident_monitor_tenant_referenced_destination_ids(
    config: &IncidentMonitorConfig,
    visible_routes: &[IncidentMonitorRouteConfig],
    tenant_context: &TenantContext,
) -> HashSet<String> {
    let mut referenced: HashSet<String> = config.default_destination_ids.iter().cloned().collect();
    for route in visible_routes {
        referenced.extend(route.destination_ids.iter().cloned());
    }
    for project in config.monitored_projects.iter().filter(|project| {
        incident_monitor_entry_not_other_tenant(
            project.tenant_id.as_deref(),
            project.workspace_id.as_deref(),
            tenant_context,
        )
    }) {
        referenced.extend(project.allowed_destination_ids.iter().cloned());
        referenced.extend(project.default_destination_ids.iter().cloned());
        for source in &project.log_sources {
            referenced.extend(source.allowed_destination_ids.iter().cloned());
            referenced.extend(source.default_destination_ids.iter().cloned());
        }
    }
    referenced
}

fn authority_mcp_inventory(snapshot: Value) -> Value {
    let mut compact = crate::http::mcp_inventory::compact_mcp_inventory_for_tool_output(&snapshot);
    strip_sensitive_mcp_inventory_keys(&mut compact);
    compact
}

fn strip_sensitive_mcp_inventory_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("last_auth_challenge");
            map.remove("credential_binding");
            map.remove("secret_refs");
            for child in map.values_mut() {
                strip_sensitive_mcp_inventory_keys(child);
            }
        }
        Value::Array(rows) => {
            for child in rows {
                strip_sensitive_mcp_inventory_keys(child);
            }
        }
        _ => {}
    }
}

fn incident_monitor_destination_inventory(destination: &IncidentMonitorDestinationConfig) -> Value {
    json!({
        "destination_id": destination.destination_id,
        "name": destination.name,
        "kind": destination.kind,
        "enabled": destination.enabled,
        "require_approval": destination.require_approval,
        "repo": destination.repo,
        "mcp_server": destination.mcp_server,
        "linear_team": destination.linear_team,
        "linear_project": destination.linear_project,
        "webhook_url_present": destination.webhook_url.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "webhook_secret_ref_present": destination.webhook_secret_ref.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "telemetry_path": destination.telemetry_path,
        "mcp_tool": destination.mcp_tool,
        "memory_category": destination.memory_category,
        "route_tags": destination.route_tags,
        "custom_config_keys": sorted_object_keys(destination.config.as_ref()),
        "allow_publish": destination.config.as_ref().and_then(|value| value.get("allow_publish")).and_then(Value::as_bool),
    })
}

fn incident_monitor_route_inventory(route: &IncidentMonitorRouteConfig) -> Value {
    json!({
        "route_id": route.route_id,
        "name": route.name,
        "enabled": route.enabled,
        "priority": route.priority,
        "destination_ids": route.destination_ids,
        "approval_policy": route.approval_policy,
        "match": {
            "event_types": route.match_event_types,
            "sources": route.match_sources,
            "components": route.match_components,
            "risk_levels": route.match_risk_levels,
            "risk_categories": route.match_risk_categories,
            "confidence": route.match_confidence,
            "expected_destinations": route.match_expected_destinations,
            "project_ids": route.match_project_ids,
            "log_source_ids": route.match_log_source_ids,
            "route_tags": route.match_route_tags,
            "source_kinds": route.match_source_kinds,
            "tenant_ids": route.match_tenant_ids,
            "workspace_ids": route.match_workspace_ids,
            "event_schema_versions": route.match_event_schema_versions,
        },
    })
}

fn incident_monitor_monitored_sources_inventory(
    config: &IncidentMonitorConfig,
    source_readiness: &[IncidentMonitorSourceReadiness],
) -> Vec<Value> {
    let mut sources = Vec::new();
    for project in &config.monitored_projects {
        let readiness =
            incident_monitor_source_readiness_for(source_readiness, &project.project_id, None);
        sources.push(json!({
            "project_id": project.project_id,
            "source_id": Value::Null,
            "name": project.name,
            "enabled": project.enabled,
            "paused": project.paused,
            "repo": project.repo,
            "workspace_root": project.workspace_root,
            "source_kind": project.source_kind,
            "mcp_server": project.mcp_server,
            "allowed_destination_ids": project.allowed_destination_ids,
            "default_destination_ids": project.default_destination_ids,
            "default_route_tags": project.default_route_tags,
            "tenant_id": project.tenant_id,
            "workspace_id": project.workspace_id,
            "event_schema_version": project.event_schema_version,
            "approval_policy": project.approval_policy,
            "redaction_profile": project.redaction_profile,
            "retention_profile": project.retention_profile,
            "auto_create_new_issues": project.auto_create_new_issues,
            "require_approval_for_new_issues": project.require_approval_for_new_issues,
            "auto_comment_on_matched_open_issues": project.auto_comment_on_matched_open_issues,
            "log_source_count": project.log_sources.len(),
            "data_readiness": incident_monitor_source_readiness_metadata_inventory(&project.data_readiness),
            "readiness": readiness,
        }));
        for source in &project.log_sources {
            let binding = project.source_binding(Some(source));
            let readiness = incident_monitor_source_readiness_for(
                source_readiness,
                &project.project_id,
                Some(&source.source_id),
            );
            sources.push(json!({
                "project_id": project.project_id,
                "source_id": source.source_id,
                "name": project.name,
                "enabled": source.enabled && project.enabled,
                "paused": source.paused || project.paused,
                "path": source.path,
                "source_kind": source.source_kind.as_ref().unwrap_or(&project.source_kind),
                "format": source.format,
                "minimum_level": source.minimum_level,
                "start_position": source.start_position,
                "watch_interval_seconds": source.watch_interval_seconds,
                "max_bytes_per_poll": source.max_bytes_per_poll,
                "max_candidates_per_poll": source.max_candidates_per_poll,
                "fingerprint_cooldown_ms": source.fingerprint_cooldown_ms,
                "allowed_destination_ids": binding.allowed_destination_ids,
                "default_destination_ids": binding.default_destination_ids,
                "default_route_tags": binding.default_route_tags,
                "tenant_id": binding.tenant_id,
                "workspace_id": binding.workspace_id,
                "event_schema_version": binding.event_schema_version,
                "approval_policy": binding.approval_policy,
                "redaction_profile": binding.redaction_profile,
                "retention_profile": binding.retention_profile,
                "data_readiness": incident_monitor_source_readiness_metadata_inventory(&binding.data_readiness),
                "readiness": readiness,
            }));
        }
    }
    sources
}

fn incident_monitor_source_readiness_for(
    readiness: &[IncidentMonitorSourceReadiness],
    project_id: &str,
    source_id: Option<&str>,
) -> Value {
    readiness
        .iter()
        .find(|row| {
            row.project_id == project_id
                && match source_id {
                    Some(source_id) => row.source_id.as_deref() == Some(source_id),
                    None => row.source_id.is_none(),
                }
        })
        .map(incident_monitor_source_readiness_inventory)
        .unwrap_or(Value::Null)
}

fn incident_monitor_source_readiness_inventory(
    readiness: &IncidentMonitorSourceReadiness,
) -> Value {
    json!({
        "project_id": readiness.project_id,
        "source_id": readiness.source_id,
        "source_kind": readiness.source_kind,
        "enabled": readiness.enabled,
        "ready": readiness.ready,
        "governance_ready": readiness.governance_ready,
        "lineage_ready": readiness.lineage_ready,
        "freshness_ready": readiness.freshness_ready,
        "schema_ready": readiness.schema_ready,
        "protection_ready": readiness.protection_ready,
        "missing": readiness.missing,
        "warnings": readiness.warnings,
        "findings": readiness.findings.iter().map(|finding| {
            json!({
                "finding_id": finding.finding_id,
                "rule_id": finding.rule_id,
                "category": finding.category,
                "severity": finding.severity,
                "title": finding.title,
                "detail": finding.detail,
                "evidence_refs": finding.evidence_refs,
                "recommendation": finding.recommendation,
            })
        }).collect::<Vec<_>>(),
        "last_observed_at_ms": readiness.last_observed_at_ms,
        "stale_after_ms": readiness.stale_after_ms,
        "detail": readiness.detail,
    })
}

fn incident_monitor_source_readiness_metadata_inventory(
    readiness: &crate::IncidentMonitorSourceReadinessConfig,
) -> Value {
    json!({
        "source_owner_present": readiness.source_owner.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "system_of_record_present": readiness.system_of_record.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "data_classification_present": readiness.data_classification.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "allowed_use_present": readiness.allowed_use.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "source_of_truth_present": readiness.source_of_truth.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "lineage_ref_present": readiness.lineage_ref.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "freshness_sla_ms": readiness.freshness_sla_ms,
        "last_observed_at_ms": readiness.last_observed_at_ms,
        "expected_schema_version_present": readiness.expected_schema_version.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "schema_drift_status": readiness.schema_drift_status,
        "quality_notes_present": readiness.quality_notes.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "legal_basis_present": readiness.legal_basis.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "authorization_marker_present": readiness.authorization_marker.as_ref().is_some_and(|value| !value.trim().is_empty()),
    })
}

fn incident_monitor_intake_key_inventory(key: &crate::IncidentMonitorProjectIntakeKey) -> Value {
    json!({
        "key_id": key.key_id,
        "project_id": key.project_id,
        "name": key.name,
        "enabled": key.enabled,
        "scopes": key.scopes,
        "created_at_ms": key.created_at_ms,
        "last_used_at_ms": key.last_used_at_ms,
        "key_hash_present": !key.key_hash.trim().is_empty(),
    })
}

pub(super) fn incident_monitor_workflow_inventory(
    workflow: &tandem_workflows::WorkflowSpec,
    hooks: &[tandem_workflows::WorkflowHookBinding],
) -> Value {
    let mut seen_hook_ids = HashSet::new();
    let workflow_hooks = hooks
        .iter()
        .filter(|hook| hook.workflow_id == workflow.workflow_id)
        .chain(workflow.hooks.iter())
        .filter(|hook| seen_hook_ids.insert(hook.binding_id.clone()))
        .map(|hook| {
            json!({
                "binding_id": hook.binding_id,
                "event": hook.event,
                "enabled": hook.enabled,
                "action_count": hook.actions.len(),
                "actions": hook.actions.iter().map(|action| {
                    json!({
                        "action": action.action,
                        "has_parameters": action.with.is_some(),
                        "parameter_keys": sorted_object_keys(action.with.as_ref()),
                    })
                }).collect::<Vec<_>>(),
                "source": hook.source,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "workflow_id": workflow.workflow_id,
        "name": workflow.name,
        "description": workflow.description,
        "enabled": workflow.enabled,
        "source": workflow.source,
        "steps": workflow.steps.iter().map(|step| {
            json!({
                "step_id": step.step_id,
                "action": step.action,
                "has_parameters": step.with.is_some(),
                "parameter_keys": sorted_object_keys(step.with.as_ref()),
            })
        }).collect::<Vec<_>>(),
        "hooks": workflow_hooks,
    })
}

fn incident_monitor_automation_inventory(automation: &crate::AutomationV2Spec) -> Value {
    json!({
        "automation_id": automation.automation_id,
        "name": automation.name,
        "description": automation.description,
        "status": automation.status,
        "creator_id": automation.creator_id,
        "workspace_root": automation.workspace_root,
        "schedule": automation.schedule,
        "output_targets": automation.output_targets,
        "scope_policy_present": automation.scope_policy.is_some(),
        "watch_condition_count": automation.watch_conditions.len(),
        "execution": automation.execution,
        "tenant_context": automation.tenant_context(),
        "agents": automation.agents.iter().map(incident_monitor_agent_inventory).collect::<Vec<_>>(),
        "nodes": automation.flow.nodes.iter().map(incident_monitor_automation_node_inventory).collect::<Vec<_>>(),
        "created_at_ms": automation.created_at_ms,
        "updated_at_ms": automation.updated_at_ms,
    })
}

fn incident_monitor_agent_inventory(agent: &crate::AutomationAgentProfile) -> Value {
    json!({
        "agent_id": agent.agent_id,
        "template_id": agent.template_id,
        "display_name": agent.display_name,
        "model_policy_present": agent.model_policy.is_some(),
        "skills": agent.skills,
        "tool_policy": {
            "allowlist": agent.tool_policy.allowlist,
            "denylist": agent.tool_policy.denylist,
        },
        "mcp_policy": incident_monitor_mcp_policy_inventory(&agent.mcp_policy),
        "approval_policy": agent.approval_policy,
    })
}

fn incident_monitor_automation_node_inventory(node: &crate::AutomationFlowNode) -> Value {
    json!({
        "node_id": node.node_id,
        "agent_id": node.agent_id,
        "depends_on": node.depends_on,
        "input_ref_count": node.input_refs.len(),
        "output_contract_kind": node.output_contract.as_ref().map(|contract| contract.kind.clone()),
        "tool_policy": node.tool_policy.as_ref().map(|policy| json!({
            "allowlist": policy.allowlist,
            "denylist": policy.denylist,
        })),
        "mcp_policy": node.mcp_policy.as_ref().map(incident_monitor_mcp_policy_inventory),
        "timeout_ms": node.timeout_ms,
        "max_tool_calls": node.max_tool_calls,
        "stage_kind": node.stage_kind,
        "gate": node.gate.as_ref().map(|gate| json!({
            "required": gate.required,
            "decisions": gate.decisions,
            "rework_targets": gate.rework_targets,
            "instructions_present": gate.instructions.is_some(),
            "expiry_policy": gate.expiry_policy,
        })),
        "metadata_keys": sorted_object_keys(node.metadata.as_ref()),
    })
}

fn incident_monitor_mcp_policy_inventory(policy: &crate::AutomationAgentMcpPolicy) -> Value {
    json!({
        "allowed_servers": policy.allowed_servers,
        "effective_allowed_servers": policy.effective_allowed_servers(),
        "allowed_tools": policy.allowed_tools,
        "allowed_connection_grants": policy.allowed_connections.iter().map(|grant| {
            json!({
                "server": grant.server,
                "connection_id_present": grant.connection_id.as_ref().is_some_and(|value| !value.trim().is_empty()),
                "run_as": grant.run_as.as_ref().map(|run_as| match run_as {
                    crate::AutomationMcpRunAs::CurrentActor => "current_actor",
                    crate::AutomationMcpRunAs::ServicePrincipal { .. } => "service_principal",
                    crate::AutomationMcpRunAs::AutomationPrincipal { .. } => "automation_principal",
                    crate::AutomationMcpRunAs::SharedConnection { .. } => "shared_connection",
                }),
            })
        }).collect::<Vec<_>>(),
    })
}

fn incident_monitor_approval_rules_inventory(
    automations: &[crate::AutomationV2Spec],
    destinations: &[IncidentMonitorDestinationConfig],
    routes: &[IncidentMonitorRouteConfig],
    config: &IncidentMonitorConfig,
) -> Vec<Value> {
    let mut rules = Vec::new();
    rules.push(json!({
        "source": "incident_monitor_config",
        "rule_id": "incident_monitor.high_risk",
        "requires_approval": config.safety_defaults.require_approval_for_high_risk,
        "policy": "require_approval_for_high_risk",
    }));
    if config.require_approval_for_new_issues {
        rules.push(json!({
            "source": "incident_monitor_config",
            "rule_id": "incident_monitor.new_issues",
            "requires_approval": true,
            "policy": "require_approval_for_new_issues",
        }));
    }
    for destination in destinations {
        rules.push(json!({
            "source": "incident_monitor_destination",
            "rule_id": format!("destination:{}", destination.destination_id),
            "destination_id": destination.destination_id,
            "requires_approval": destination.require_approval,
            "policy": "destination.require_approval",
        }));
    }
    for route in routes {
        rules.push(json!({
            "source": "incident_monitor_route",
            "rule_id": format!("route:{}", route.route_id),
            "route_id": route.route_id,
            "destination_ids": route.destination_ids,
            "approval_policy": route.approval_policy,
        }));
    }
    for automation in automations {
        for agent in &automation.agents {
            if let Some(policy) = agent.approval_policy.as_ref() {
                rules.push(json!({
                    "source": "automation_agent",
                    "rule_id": format!("automation:{}:agent:{}", automation.automation_id, agent.agent_id),
                    "automation_id": automation.automation_id,
                    "agent_id": agent.agent_id,
                    "approval_policy": policy,
                }));
            }
        }
        for node in &automation.flow.nodes {
            if let Some(gate) = node.gate.as_ref() {
                rules.push(json!({
                    "source": "automation_node_gate",
                    "rule_id": format!("automation:{}:node:{}", automation.automation_id, node.node_id),
                    "automation_id": automation.automation_id,
                    "node_id": node.node_id,
                    "required": gate.required,
                    "decisions": gate.decisions,
                    "rework_targets": gate.rework_targets,
                    "expiry_policy": gate.expiry_policy,
                }));
            }
        }
    }
    rules
}

fn incident_monitor_pending_approval_inventory(approval: &tandem_types::ApprovalRequest) -> Value {
    json!({
        "request_id": approval.request_id,
        "source": approval.source,
        "tenant": approval.tenant,
        "run_id": approval.run_id,
        "node_id": approval.node_id,
        "workflow_name": approval.workflow_name,
        "action_kind": approval.action_kind,
        "requested_at_ms": approval.requested_at_ms,
        "expires_at_ms": approval.expires_at_ms,
        "decisions": approval.decisions,
        "rework_targets": approval.rework_targets,
        "instructions_present": approval.instructions.is_some(),
        "approval_wait": approval.approval_wait,
    })
}

fn incident_monitor_governance_approval_inventory(
    approval: &crate::automation_v2::governance::GovernanceApprovalRequest,
) -> Value {
    json!({
        "approval_id": approval.approval_id,
        "request_type": approval.request_type,
        "target_resource": approval.target_resource,
        "status": approval.status,
        "expires_at_ms": approval.expires_at_ms,
        "created_at_ms": approval.created_at_ms,
        "updated_at_ms": approval.updated_at_ms,
        "reviewed_at_ms": approval.reviewed_at_ms,
    })
}

fn incident_monitor_policy_decision_inventory(decision: &tandem_types::PolicyDecisionRecord) -> Value {
    json!({
        "decision_id": decision.decision_id,
        "tenant_context": decision.tenant_context,
        "actor_id": decision.actor_id,
        "session_id": decision.session_id,
        "run_id": decision.run_id,
        "automation_id": decision.automation_id,
        "node_id": decision.node_id,
        "tool": decision.tool,
        "resource": decision.resource,
        "data_classes": decision.data_classes,
        "risk_tier": decision.risk_tier,
        "decision": decision.decision,
        "reason_code": decision.reason_code,
        "policy_id": decision.policy_id,
        "grant_id": decision.grant_id,
        "approval_id": decision.approval_id,
        "created_at_ms": decision.created_at_ms,
        "metadata_keys": sorted_object_keys(Some(&decision.metadata)),
    })
}

fn incident_monitor_external_publish_surface(destination: &IncidentMonitorDestinationConfig) -> Value {
    json!({
        "source": "incident_monitor_destination",
        "surface_id": destination.destination_id,
        "name": destination.name,
        "kind": destination.kind,
        "enabled": destination.enabled,
        "require_approval": destination.require_approval,
        "target": {
            "repo": destination.repo,
            "linear_team": destination.linear_team,
            "linear_project": destination.linear_project,
            "webhook_url_present": destination.webhook_url.as_ref().is_some_and(|value| !value.trim().is_empty()),
            "telemetry_path": destination.telemetry_path,
            "mcp_server": destination.mcp_server,
            "mcp_tool": destination.mcp_tool,
            "memory_category": destination.memory_category,
        },
        "config_keys": sorted_object_keys(destination.config.as_ref()),
    })
}

fn incident_monitor_external_action_inventory(action: &crate::ExternalActionRecord) -> Value {
    json!({
        "action_id": action.action_id,
        "operation": action.operation,
        "status": action.status,
        "source_kind": action.source_kind,
        "source_id": action.source_id,
        "routine_run_id": action.routine_run_id,
        "context_run_id": action.context_run_id,
        "capability_id": action.capability_id,
        "provider": action.provider,
        "target": action.target,
        "approval_state": action.approval_state,
        "idempotency_key_present": action.idempotency_key.as_ref().is_some_and(|value| !value.trim().is_empty()),
        "receipt_present": action.receipt.is_some(),
        "error": action.error,
        "metadata_keys": sorted_object_keys(action.metadata.as_ref()),
        "created_at_ms": action.created_at_ms,
        "updated_at_ms": action.updated_at_ms,
    })
}

fn sorted_object_keys(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    let mut keys = map.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys
}
