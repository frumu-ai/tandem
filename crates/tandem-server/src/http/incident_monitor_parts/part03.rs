pub(super) async fn preview_incident_monitor_route(
    State(state): State<AppState>,
    Json(input): Json<IncidentMonitorRoutePreviewInput>,
) -> Response {
    let draft = if let Some(draft_id) = trim_incident_monitor_preview_value(input.draft_id.as_deref()) {
        match state.get_incident_monitor_draft(&draft_id).await {
            Some(draft) => Some(draft),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "Incident monitor draft not found",
                        "code": "INCIDENT_MONITOR_DRAFT_NOT_FOUND",
                        "draft_id": draft_id,
                    })),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let incident =
        if let Some(incident_id) = trim_incident_monitor_preview_value(input.incident_id.as_deref()) {
            match state.get_incident_monitor_incident(&incident_id).await {
                Some(incident) => Some(incident),
                None => {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": "Incident monitor incident not found",
                            "code": "INCIDENT_MONITOR_INCIDENT_NOT_FOUND",
                            "incident_id": incident_id,
                        })),
                    )
                        .into_response();
                }
            }
        } else {
            None
        };
    let status = state.incident_monitor_status_snapshot().await;
    let context = crate::incident_monitor::router::build_route_context(
        input.event_type.as_deref(),
        input.source.as_deref(),
        input.component.as_deref(),
        input.risk_level.as_deref(),
        input.risk_category.as_deref(),
        input.confidence.as_deref(),
        input.expected_destination.as_deref(),
        input.project_id.as_deref(),
        input.log_source_id.as_deref(),
        &input.route_tags,
        input.report.as_ref(),
        draft.as_ref(),
        incident.as_ref(),
    );
    let preview = crate::incident_monitor::router::build_route_preview(
        &status.config,
        &status.destinations,
        &status.destination_readiness,
        &status.source_readiness,
        &context,
        &input.destination_ids,
    );
    Json(preview).into_response()
}

fn trim_incident_monitor_preview_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
