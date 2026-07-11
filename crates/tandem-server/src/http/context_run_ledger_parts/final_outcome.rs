fn governance_evidence_final_outcome(
    context_run: &ContextRunState,
    automation_run: Option<&crate::automation_v2::types::AutomationV2RunRecord>,
    slack_visible_response: Option<&str>,
) -> Value {
    json!({
        "context_status": context_run.status,
        "automation_status": automation_run.map(|run| serde_json::to_value(&run.status).unwrap_or(Value::Null)),
        "completed_nodes": automation_run.map(|run| run.checkpoint.completed_nodes.clone()).unwrap_or_default(),
        "pending_nodes": automation_run.map(|run| run.checkpoint.pending_nodes.clone()).unwrap_or_default(),
        "blocked_nodes": automation_run.map(|run| run.checkpoint.blocked_nodes.clone()).unwrap_or_default(),
        "last_error": redacted_text_ref(context_run.last_error.as_deref()),
        "detail": redacted_text_ref(automation_run.and_then(|run| run.detail.as_deref())),
        "stop_kind": automation_run.and_then(|run| run.stop_kind.as_ref()).map(|kind| serde_json::to_value(kind).unwrap_or(Value::Null)),
        "stop_reason": redacted_text_ref(automation_run.and_then(|run| run.stop_reason.as_deref())),
        "slack_visible_response": slack_visible_response,
    })
}

async fn governance_evidence_slack_response(
    state: &AppState,
    context_run: &ContextRunState,
) -> Option<String> {
    if context_run.source_client.as_deref() != Some("channel:slack") {
        return None;
    }
    let session_id = context_run.run_id.strip_prefix("session-")?;
    let session = state.storage.get_session(session_id).await?;
    let response = session.messages.iter().rev().find_map(|message| {
        if !matches!(&message.role, MessageRole::Assistant) {
            return None;
        }
        let text = message
            .parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        (!text.trim().is_empty()).then_some(text)
    })?;
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    Some(tandem_channels::redaction::redact_outbound(
        &response,
        &workspace_root,
    ))
}
