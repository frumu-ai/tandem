// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn context_run_matches_source(run: &ContextRunState, source: Option<&str>) -> bool {
    let Some(source) = source.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    run.source_client
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case(source))
}

fn session_context_run_channel_source(
    session: &tandem_types::Session,
) -> Option<(String, Value)> {
    if session.source_kind.as_deref() != Some("channel") {
        return None;
    }
    let metadata = session.source_metadata.clone()?;
    let channel = metadata.get("channel")?.as_str()?.trim();
    if channel.is_empty() {
        return None;
    }
    Some((format!("channel:{channel}"), metadata))
}

async fn backfill_session_context_run_source(
    state: &AppState,
    run: &mut ContextRunState,
    channel_source: Option<&(String, Value)>,
) -> Result<(), StatusCode> {
    let Some((source_client, source_metadata)) = channel_source else {
        return Ok(());
    };
    if run.source_client.as_ref() == Some(source_client)
        && run.source_metadata.as_ref() == Some(source_metadata)
    {
        return Ok(());
    }
    run.source_client = Some(source_client.clone());
    run.source_metadata = Some(source_metadata.clone());
    run.revision = run.revision.saturating_add(1);
    run.updated_at_ms = crate::now_ms();
    save_context_run_state(state, run).await
}
