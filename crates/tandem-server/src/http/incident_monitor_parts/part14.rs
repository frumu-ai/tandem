// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Adversarial scenario pack endpoints (TAN-487), split into its own part file
// for the file-size gate; same module via include!.

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct IncidentMonitorScenarioRunInput {
    /// Optional custom pack to run instead of the built-in default pack.
    #[serde(default)]
    pub pack: Option<crate::IncidentMonitorScenarioPack>,
    /// Optional subset of scenario ids to run (defaults to the whole pack).
    #[serde(default)]
    pub scenario_ids: Vec<String>,
}

async fn incident_monitor_scenario_admin_guard(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<Response> {
    if incident_monitor_assessment_has_scoped_intake_key_header(headers) {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Incident Monitor scenario packs require an admin API token, not a scoped intake key",
                    "code": "INCIDENT_MONITOR_SCENARIO_PACKS_ADMIN_REQUIRED",
                })),
            )
                .into_response(),
        );
    }
    if state.api_token_required().await {
        if !incident_monitor_assessment_request_has_active_api_token(state, headers).await {
            return Some(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "Incident Monitor scenario packs require the full admin API token",
                        "code": "INCIDENT_MONITOR_SCENARIO_PACKS_ADMIN_REQUIRED",
                    })),
                )
                    .into_response(),
            );
        }
    }
    None
}

/// GET: describe the built-in adversarial scenario pack without executing it.
pub(super) async fn list_incident_monitor_scenario_packs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) =
        incident_monitor_scenario_admin_guard(&state, &headers).await
    {
        return denied;
    }
    let pack = crate::default_scenario_pack();
    Json(json!({
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_adversarial_scenario_packs",
            "read_only": true,
            "dry_run": true,
            "mutates_external_systems": false,
            "admin_invoked": true,
        },
        "packs": [pack],
        "note": "Scenario packs run only in dry-run/sandbox mode and never mutate external systems. Probes must be authorized and bounded to systems you operate.",
    }))
    .into_response()
}

/// POST: run the built-in (or a supplied) scenario pack in dry-run mode against
/// the live governance config. Never mutates external systems.
pub(super) async fn run_incident_monitor_scenario_packs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
    Json(input): Json<IncidentMonitorScenarioRunInput>,
) -> Response {
    if let Some(denied) =
        incident_monitor_scenario_admin_guard(&state, &headers).await
    {
        return denied;
    }

    let mut pack = input.pack.unwrap_or_else(crate::default_scenario_pack);
    if !input.scenario_ids.is_empty() {
        pack.scenarios
            .retain(|scenario| input.scenario_ids.contains(&scenario.scenario_id));
    }

    let status = state.incident_monitor_status_snapshot().await;
    let result = crate::incident_monitor_scenarios::run_incident_monitor_scenario_pack(
        &status,
        &tenant_context,
        &pack,
    );

    Json(json!({
        "schema_version": 1,
        "scope": {
            "source": "incident_monitor_adversarial_scenario_packs",
            "read_only": true,
            "dry_run": true,
            "mutates_external_systems": false,
            "admin_invoked": true,
            "tenant_context": tenant_context,
        },
        "pack": result,
    }))
    .into_response()
}
