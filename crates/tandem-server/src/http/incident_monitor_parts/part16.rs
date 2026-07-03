// Continuous reassessment scheduler endpoints (TAN-490), split into its own
// part file for the file-size gate; same module via include!.

/// Internal wrapper exposing the dry-run assessment posture payload to the
/// reassessment scheduler (which lives outside the http module). Uses default
/// report inputs so the reassessment sees the full current posture.
pub(crate) async fn incident_monitor_reassessment_posture_payload(
    state: &AppState,
    tenant_context: &TenantContext,
    generated_at_ms: u64,
) -> Value {
    incident_monitor_assessment_report_payload(
        state,
        tenant_context.clone(),
        None,
        &IncidentMonitorAssessmentReportInput::default(),
        generated_at_ms,
    )
    .await
}

fn incident_monitor_reassessment_records_for_tenant(
    records: &std::collections::HashMap<String, crate::ReassessmentRecord>,
    tenant_context: &TenantContext,
    limit: usize,
) -> Vec<crate::ReassessmentRecord> {
    let prefix = format!("{}/{}/", tenant_context.org_id, tenant_context.workspace_id);
    let mut rows = records
        .values()
        .filter(|record| record.scope_key.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    // Most recent first (by generation time, then version).
    rows.sort_by(|a, b| {
        b.generated_at_ms
            .cmp(&a.generated_at_ms)
            .then(b.version.cmp(&a.version))
    });
    rows.truncate(limit);
    rows
}

/// POST: run a continuous reassessment now for the caller's tenant deployment
/// scope, in dry-run/redacted mode. Admin-only; never mutates external systems.
pub(super) async fn run_incident_monitor_reassessment_endpoint(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) =
        incident_monitor_scenario_admin_guard(state.api_token().await.as_deref(), &headers)
    {
        return denied;
    }

    let now_ms = crate::now_ms();
    let record = match crate::incident_monitor_reassessment::compute_incident_monitor_reassessment(
        &state,
        &tenant_context,
        crate::ReassessmentTrigger::Manual,
        Some("admin-invoked reassessment".to_string()),
        now_ms,
    )
    .await
    {
        Ok(record) => record,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("reassessment failed: {error}") })),
            )
                .into_response();
        }
    };
    let schedule = crate::incident_monitor_reassessment::incident_monitor_reassessment_schedule_status(
        &state,
        &tenant_context,
        now_ms,
    )
    .await;

    Json(json!({
        "scope": {
            "source": "incident_monitor_continuous_reassessment",
            "read_only": true,
            "dry_run": true,
            "redacted": true,
            "mutates_external_systems": false,
            "admin_invoked": true,
            "tenant_context": tenant_context,
        },
        "reassessment": record,
        "schedule": schedule,
    }))
    .into_response()
}

/// GET: list recent reassessment records and the per-scope schedule status
/// (last completed / next due / overdue) for the caller's tenant. Admin-only.
pub(super) async fn list_incident_monitor_reassessments_endpoint(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) =
        incident_monitor_scenario_admin_guard(state.api_token().await.as_deref(), &headers)
    {
        return denied;
    }

    let now_ms = crate::now_ms();
    let records = {
        let guard = state.incident_monitor_reassessments.read().await;
        incident_monitor_reassessment_records_for_tenant(&guard, &tenant_context, 50)
    };
    let schedule = crate::incident_monitor_reassessment::incident_monitor_reassessment_schedule_status(
        &state,
        &tenant_context,
        now_ms,
    )
    .await;

    Json(json!({
        "scope": {
            "source": "incident_monitor_continuous_reassessment",
            "read_only": true,
            "dry_run": true,
            "redacted": true,
            "mutates_external_systems": false,
            "admin_invoked": true,
            "tenant_context": tenant_context,
        },
        "counts": {
            "records": records.len(),
            "overdue_scopes": schedule.iter().filter(|status| status.overdue).count(),
        },
        "schedule": schedule,
        "records": records,
    }))
    .into_response()
}
