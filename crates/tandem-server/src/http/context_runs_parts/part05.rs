// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) fn automation_v2_context_run_id(run_id: &str) -> String {
    format!("automation-v2-{run_id}")
}

pub(crate) fn routine_context_run_id(run_id: &str) -> String {
    format!("routine-{run_id}")
}

pub(crate) fn session_context_run_id(session_id: &str) -> String {
    format!("session-{session_id}")
}

pub(crate) fn session_run_status_to_context(status: &str) -> ContextRunStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" => ContextRunStatus::Completed,
        "cancelled" => ContextRunStatus::Cancelled,
        "timeout" | "error" | "failed" => ContextRunStatus::Failed,
        _ => ContextRunStatus::Running,
    }
}

pub(crate) async fn ensure_session_context_run(
    state: &AppState,
    session: &tandem_types::Session,
) -> Result<String, StatusCode> {
    let run_id = session_context_run_id(&session.id);
    let channel_source = session_context_run_channel_source(session);
    if let Ok(mut existing) = load_context_run_state(state, &run_id).await {
        backfill_session_context_run_source(state, &mut existing, channel_source.as_ref()).await?;
        return Ok(run_id);
    }
    let now = crate::now_ms();
    let workspace = session
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|path| ContextWorkspaceLease {
            workspace_id: session
                .project_id
                .clone()
                .unwrap_or_else(|| session.id.clone()),
            canonical_path: path.to_string(),
            lease_epoch: 0,
        })
        .unwrap_or_default();
    let run = ContextRunState {
        run_id: run_id.clone(),
        run_type: "session".to_string(),
        tenant_context: session.tenant_context.clone(),
        source_client: Some(
            channel_source
                .as_ref()
                .map(|(source_client, _)| source_client.clone())
                .unwrap_or_else(|| "session_api".to_string()),
        ),
        source_metadata: channel_source.map(|(_, metadata)| metadata),
        model_provider: session.provider.clone(),
        model_id: session.model.as_ref().map(|model| model.model_id.clone()),
        mcp_servers: Vec::new(),
        status: ContextRunStatus::Queued,
        objective: {
            let title = session.title.trim();
            if title.is_empty() {
                format!("Interactive session {}", session.id)
            } else {
                format!("Interactive session: {title}")
            }
        },
        workspace,
        steps: vec![ContextRunStep {
            step_id: "session-run".to_string(),
            title: "Execute interactive session work".to_string(),
            status: ContextStepStatus::Pending,
        }],
        tasks: Vec::new(),
        why_next_step: Some("waiting for session run activity".to_string()),
        revision: 1,
        last_event_seq: 0,
        created_at_ms: now,
        started_at_ms: None,
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(state, &run).await?;
    Ok(run_id)
}

pub(crate) fn workflow_context_run_id(run_id: &str) -> String {
    format!("workflow-{run_id}")
}

pub(super) fn automation_run_status_to_context(
    status: &crate::AutomationRunStatus,
) -> ContextRunStatus {
    match status {
        crate::AutomationRunStatus::Queued => ContextRunStatus::Queued,
        crate::AutomationRunStatus::Running
        | crate::AutomationRunStatus::Pausing
        | crate::AutomationRunStatus::Paused
        | crate::AutomationRunStatus::AwaitingApproval => ContextRunStatus::Running,
        crate::AutomationRunStatus::Completed => ContextRunStatus::Completed,
        crate::AutomationRunStatus::Blocked => ContextRunStatus::Blocked,
        crate::AutomationRunStatus::Failed => ContextRunStatus::Failed,
        crate::AutomationRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}

fn routine_run_status_to_context(status: &crate::RoutineRunStatus) -> ContextRunStatus {
    match status {
        crate::RoutineRunStatus::Queued => ContextRunStatus::Queued,
        crate::RoutineRunStatus::PendingApproval
        | crate::RoutineRunStatus::Running
        | crate::RoutineRunStatus::Paused => ContextRunStatus::Running,
        crate::RoutineRunStatus::BlockedPolicy => ContextRunStatus::Blocked,
        crate::RoutineRunStatus::Denied => ContextRunStatus::Cancelled,
        crate::RoutineRunStatus::Completed => ContextRunStatus::Completed,
        crate::RoutineRunStatus::Failed => ContextRunStatus::Failed,
        crate::RoutineRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}
