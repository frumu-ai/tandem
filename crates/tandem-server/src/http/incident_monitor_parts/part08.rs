async fn emit_incident_monitor_config_audit(state: &AppState, config: &IncidentMonitorConfig) {
    emit_incident_monitor_admin_audit_event(
        state,
        "incident_monitor.config.updated",
        json!({
            "enabled": config.enabled,
            "paused": config.paused,
            "destination_count": config.destinations.len(),
            "route_count": config.routes.len(),
            "default_destination_ids": &config.default_destination_ids,
            "destinations": config.destinations.iter().map(|destination| {
                json!({
                    "destination_id": destination.destination_id.as_str(),
                    "kind": format!("{:?}", &destination.kind),
                    "enabled": destination.enabled,
                    "require_approval": destination.require_approval,
                    "has_webhook_secret_ref": destination.webhook_secret_ref.is_some(),
                    "has_webhook_url": destination.webhook_url.is_some(),
                    "has_mcp_tool": destination.mcp_tool.is_some(),
                    "has_custom_config": destination.config.is_some(),
                })
            }).collect::<Vec<_>>(),
            "routes": config.routes.iter().map(|route| {
                json!({
                    "route_id": route.route_id.as_str(),
                    "priority": route.priority,
                    "destination_ids": &route.destination_ids,
                    "approval_policy": format!("{:?}", &route.approval_policy),
                    "match_source_kinds": &route.match_source_kinds,
                    "match_tenant_ids": &route.match_tenant_ids,
                    "match_workspace_ids": &route.match_workspace_ids,
                })
            }).collect::<Vec<_>>(),
            "monitored_project_ids": config.monitored_projects.iter()
                .map(|project| project.project_id.as_str())
                .collect::<Vec<_>>(),
            "safety_defaults": {
                "require_approval_for_high_risk": config.safety_defaults.require_approval_for_high_risk,
                "redact_secrets": config.safety_defaults.redact_secrets,
                "block_unready_destinations": config.safety_defaults.block_unready_destinations,
                "block_unready_sources": config.safety_defaults.block_unready_sources,
                "retention_days": config.safety_defaults.retention_days,
                "minimum_risk_level": config.safety_defaults.minimum_risk_level,
            },
        }),
    )
    .await;
}

async fn emit_incident_monitor_intake_key_audit(
    state: &AppState,
    event_type: &'static str,
    key: &crate::IncidentMonitorProjectIntakeKey,
) {
    emit_incident_monitor_admin_audit_event(
        state,
        event_type,
        json!({
            "key_id": key.key_id.as_str(),
            "project_id": key.project_id.as_str(),
            "name": key.name.as_str(),
            "enabled": key.enabled,
            "scopes": &key.scopes,
            "created_at_ms": key.created_at_ms,
            "last_used_at_ms": key.last_used_at_ms,
        }),
    )
    .await;
}

async fn emit_incident_monitor_admin_audit_event(
    state: &AppState,
    event_type: &'static str,
    payload: serde_json::Value,
) {
    state
        .event_bus
        .publish(tandem_types::EngineEvent::new(event_type, payload.clone()));
    crate::audit::append_protected_audit_event_best_effort(
        state,
        event_type,
        &tandem_types::TenantContext::local_implicit(),
        None,
        payload,
    )
    .await;
}
