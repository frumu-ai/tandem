// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn sync_incident_monitor_automation_node_artifact(
    state: &AppState,
    context_run_id: &str,
    automation: &crate::AutomationV2Spec,
    run: &crate::AutomationV2RunRecord,
    node: &crate::AutomationFlowNode,
    output: Option<&Value>,
) -> Result<(), StatusCode> {
    let is_incident_monitor_triage = automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("source"))
        .and_then(Value::as_str)
        == Some("incident_monitor");
    if !is_incident_monitor_triage {
        return Ok(());
    }
    let Some(output) = output else {
        return Ok(());
    };
    let status = output
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !status.eq_ignore_ascii_case("completed") {
        return Ok(());
    }
    let Some(artifact_type) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("incident_monitor"))
        .and_then(|metadata| metadata.get("artifact_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let artifact_id = format!("incident-monitor-v2-{}-{}", run.run_id, node.node_id);
    let blackboard = load_context_blackboard(state, context_run_id);
    if blackboard.artifacts.iter().any(|row| row.id == artifact_id) {
        return Ok(());
    }
    let payload = output
        .get("content")
        .and_then(|content| content.get("structured_handoff"))
        .cloned()
        .or_else(|| output.get("content").cloned())
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| output.clone());
    let relative_path = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("incident_monitor"))
        .and_then(|metadata| metadata.get("context_artifact_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            match artifact_type {
                "incident_monitor_inspection" => Some("artifacts/incident_monitor.inspection.json"),
                "incident_monitor_research" => Some("artifacts/incident_monitor.research.json"),
                "incident_monitor_validation" => Some("artifacts/incident_monitor.validation.json"),
                "incident_monitor_fix_proposal" => Some("artifacts/incident_monitor.fix_proposal.json"),
                _ => None,
            }
            .map(str::to_string)
        });
    let Some(relative_path) = relative_path else {
        return Ok(());
    };
    append_json_artifact_to_context_run(
        state,
        context_run_id,
        &artifact_id,
        artifact_type,
        &relative_path,
        &payload,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if artifact_type == "incident_monitor_fix_proposal" {
        if let Some(draft_id) = automation
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("draft_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            if let Err(error) =
                crate::http::incident_monitor::finalize_completed_incident_monitor_triage(state, draft_id)
                    .await
            {
                tracing::warn!(
                    draft_id = %draft_id,
                    run_id = %run.run_id,
                    error = %error,
                    "failed to finalize completed Incident Monitor triage after artifact sync",
                );
            }
        }
    }
    Ok(())
}
