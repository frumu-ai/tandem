// Governance maturity metrics + behavioral drift endpoint (TAN-488), split into
// its own part file for the file-size gate; same module via include!.

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct IncidentMonitorGovernanceMetricsInput {
    /// End of the current window (defaults to now).
    #[serde(default)]
    pub to_ms: Option<u64>,
    /// Window duration in ms (defaults to 7 days). The baseline window is the
    /// span immediately before the current one.
    #[serde(default)]
    pub window_ms: Option<u64>,
    /// Optional threshold overrides (defaults are production-safe).
    #[serde(default)]
    pub thresholds: Option<crate::IncidentMonitorGovernanceThresholds>,
}

/// POST: compute governance maturity metrics and behavioral drift in dry-run,
/// redacted mode. Admin-only; never mutates external systems.
pub(super) async fn compute_incident_monitor_governance_metrics_endpoint(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
    Json(input): Json<IncidentMonitorGovernanceMetricsInput>,
) -> Response {
    if let Some(denied) =
        incident_monitor_scenario_admin_guard(state.api_token().await.as_deref(), &headers)
    {
        return denied;
    }

    let to_ms = input.to_ms.unwrap_or_else(crate::now_ms);
    let thresholds = input.thresholds.unwrap_or_default();
    let metrics = crate::incident_monitor_governance_metrics::compute_incident_monitor_governance_metrics(
        &state,
        &tenant_context,
        to_ms,
        input.window_ms,
        &thresholds,
    )
    .await;

    Json(json!({
        "scope": {
            "source": "incident_monitor_governance_maturity_metrics",
            "read_only": true,
            "dry_run": true,
            "redacted": true,
            "mutates_external_systems": false,
            "admin_invoked": true,
            "tenant_context": tenant_context,
        },
        "governance_maturity": metrics,
    }))
    .into_response()
}
