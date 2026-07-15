// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(super) async fn workflow_start(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let (actor, verified) = mutation_actor(args, tenant, chat_session)?;
    let prompt = required_str(args, "prompt")?;
    let key = required_str(args, "idempotency_key")?;
    let workspace_root = if let Some(requested) = optional_str(args, "workspace_root") {
        crate::normalize_absolute_workspace_root(requested).map_err(anyhow::Error::msg)?
    } else {
        let session_root = chat_session
            .workspace_root
            .as_deref()
            .unwrap_or(&chat_session.directory);
        match crate::normalize_absolute_workspace_root(session_root) {
            Ok(root) => root,
            Err(_) => crate::normalize_absolute_workspace_root(
                &state.workspace_index.snapshot().await.root,
            )
            .map_err(anyhow::Error::msg)?,
        }
    };
    let project_slug = optional_str(args, "project_slug")
        .map(str::to_string)
        .or_else(|| chat_session.project_id.clone())
        .unwrap_or_else(|| "chat-authoring".to_string());
    let fingerprint = operator_args_fingerprint(args, "operator.workflow_plan_start");
    let planner_session_digest = crate::sha256_hex(&[
        &tenant.org_id,
        &tenant.workspace_id,
        tenant.deployment_id.as_deref().unwrap_or(""),
        "operator.workflow_plan_start",
        key,
        &fingerprint,
    ]);
    let planner_session_id = format!("wfplan-session-{}", &planner_session_digest[..32]);
    if let Some(replay) = reserve_idempotent(
        state,
        tenant,
        "operator.workflow_plan_start",
        key,
        &actor,
        &fingerprint,
    )
    .await?
    {
        if !idempotency_in_progress(&replay) {
            return Ok(replay);
        }
        if let Some(mut existing) = state
            .get_workflow_planner_session(&planner_session_id)
            .await
            .filter(|session| {
                tenant_matches(&session.tenant_context, tenant)
                    && session.linked_chat_session_id.as_deref() == Some(chat_session.id.as_str())
                    && session.draft.is_some()
            })
        {
            let revision = existing.draft.as_ref().map(|draft| draft.plan_revision);
            push_artifact_link(
                &mut existing,
                "planner_session",
                &planner_session_id,
                planner_url(&planner_session_id),
                revision,
                active_chat_run_id(state, &chat_session.id).await,
                tool_call_id(args),
                Some(key.to_string()),
                "workflow_plan_start",
            );
            let existing = state.put_workflow_planner_session(existing).await?;
            let details = planner_envelope("start_recovered", &existing, None);
            return complete_idempotent(
                state,
                tenant,
                "operator.workflow_plan_start",
                key,
                "workflow_planner_session",
                &planner_session_id,
                details,
            )
            .await;
        }
        return Ok(replay);
    }

    let now = crate::now_ms();
    let chat_run_id = active_chat_run_id(state, &chat_session.id).await;
    let planner_session = super::workflow_planner::WorkflowPlannerSessionRecord {
        session_id: planner_session_id.clone(),
        tenant_context: tenant.clone(),
        linked_chat_session_id: Some(chat_session.id.clone()),
        linked_chat_run_id: chat_run_id.clone(),
        last_referenced_at_ms: Some(now),
        artifact_links: Vec::new(),
        project_slug,
        title: optional_str(args, "title")
            .map(str::to_string)
            .unwrap_or_else(|| prompt.chars().take(72).collect()),
        workspace_root,
        source_kind: "agentic_chat".to_string(),
        source_bundle_digest: None,
        source_pack_id: None,
        source_pack_version: None,
        current_plan_id: None,
        draft: None,
        goal: prompt.to_string(),
        notes: String::new(),
        planner_provider: String::new(),
        planner_model: String::new(),
        plan_source: "agentic_chat".to_string(),
        allowed_mcp_servers: args
            .get("allowed_mcp_servers")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        operator_preferences: Some(json!({
            "source": "authenticated_chat",
            "actor_id": actor,
            "chat_session_id": chat_session.id,
            "chat_run_id": chat_run_id,
        })),
        planning: Some(
            super::workflow_planner::WorkflowPlannerSessionPlanningRecord {
                mode: "workflow_planning".to_string(),
                source_platform: "chat".to_string(),
                requesting_actor: Some(actor.clone()),
                created_by_agent: Some("tandem_operator".to_string()),
                linked_channel_session_id: Some(chat_session.id.clone()),
                started_at_ms: Some(now),
                updated_at_ms: Some(now),
                ..Default::default()
            },
        ),
        import_validation: None,
        import_transform_log: Vec::new(),
        import_scope_snapshot: None,
        operation: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    };
    if let Err(error) = state.put_workflow_planner_session(planner_session).await {
        let _ = release_idempotent(
            state,
            tenant,
            "operator.workflow_plan_start",
            key,
            &fingerprint,
        )
        .await;
        return Err(error);
    }
    let input = serde_json::from_value(json!({
        "prompt": prompt,
        "plan_source": "agentic_chat",
    }))?;
    let response = match super::workflow_planner::workflow_planner_session_start(
        State(state.clone()),
        Path(planner_session_id.clone()),
        Extension(tenant.clone()),
        verified.map(Extension),
        Json(input),
    )
    .await
    {
        Ok(response) => response,
        Err((_, payload)) => {
            let _ = release_idempotent(
                state,
                tenant,
                "operator.workflow_plan_start",
                key,
                &fingerprint,
            )
            .await;
            bail!(payload.0.to_string());
        }
    };
    let completed_session = response
        .0
        .get("session")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let mut completed_session = match completed_session {
        Some(session) => session,
        None => state
            .get_workflow_planner_session(&planner_session_id)
            .await
            .context("planner start did not return a session")?,
    };
    let completed_revision = completed_session
        .draft
        .as_ref()
        .map(|draft| draft.plan_revision);
    push_artifact_link(
        &mut completed_session,
        "planner_session",
        &planner_session_id,
        planner_url(&planner_session_id),
        completed_revision,
        active_chat_run_id(state, &chat_session.id).await,
        tool_call_id(args),
        Some(key.to_string()),
        "workflow_plan_start",
    );
    let completed_session = state
        .put_workflow_planner_session(completed_session)
        .await?;
    let details = json!({
        "ok": true,
        "action": "start",
        "status": "completed",
        "resource": {
            "kind": "workflow_planner_session",
            "id": planner_session_id,
            "url": planner_url(&planner_session_id),
        },
        "planner_session": completed_session,
        "blockers": [],
    });
    complete_idempotent(
        state,
        tenant,
        "operator.workflow_plan_start",
        key,
        "workflow_planner_session",
        &planner_session_id,
        details,
    )
    .await
}

pub(super) async fn workflow_revise(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let (actor, verified) = mutation_actor(args, tenant, chat_session)?;
    let planner_session_id = required_str(args, "planner_session_id")?;
    let message = required_str(args, "message")?;
    let key = required_str(args, "idempotency_key")?;
    let session =
        scoped_planner_session(state, tenant, &chat_session.id, Some(planner_session_id)).await?;
    ensure_expected_revision(args, &session)?;
    if session.published_at_ms.is_some() {
        bail!("workflow planner session is published; create a new draft before revising it");
    }
    if session
        .operation
        .as_ref()
        .is_some_and(|operation| operation.status == "running")
    {
        bail!("workflow planner session already has an operation in progress");
    }
    let fingerprint = operator_args_fingerprint(args, "operator.workflow_plan_revise");
    if let Some(replay) = reserve_idempotent(
        state,
        tenant,
        "operator.workflow_plan_revise",
        key,
        &actor,
        &fingerprint,
    )
    .await?
    {
        if !idempotency_in_progress(&replay) {
            return Ok(replay);
        }
        let latest =
            scoped_planner_session(state, tenant, &chat_session.id, Some(planner_session_id))
                .await?;
        if latest
            .artifact_links
            .iter()
            .any(|link| link.idempotency_key.as_deref() == Some(key))
        {
            let details = planner_envelope("revise_recovered", &latest, None);
            return complete_idempotent(
                state,
                tenant,
                "operator.workflow_plan_revise",
                key,
                "workflow_planner_session",
                planner_session_id,
                details,
            )
            .await;
        }
        return Ok(replay);
    }
    let input = serde_json::from_value(json!({ "message": message }))?;
    let response = match super::workflow_planner::workflow_planner_session_message(
        State(state.clone()),
        Path(planner_session_id.to_string()),
        Extension(tenant.clone()),
        verified.map(Extension),
        Json(input),
    )
    .await
    {
        Ok(response) => response,
        Err((_, payload)) => {
            let _ = release_idempotent(
                state,
                tenant,
                "operator.workflow_plan_revise",
                key,
                &fingerprint,
            )
            .await;
            bail!(payload.0.to_string());
        }
    };
    let completed_session = response
        .0
        .get("session")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let mut completed_session = match completed_session {
        Some(session) => session,
        None => state
            .get_workflow_planner_session(planner_session_id)
            .await
            .context("planner revision did not return a session")?,
    };
    let completed_revision = completed_session
        .draft
        .as_ref()
        .map(|draft| draft.plan_revision);
    push_artifact_link(
        &mut completed_session,
        "revision",
        planner_session_id,
        planner_url(planner_session_id),
        completed_revision,
        active_chat_run_id(state, &chat_session.id).await,
        tool_call_id(args),
        Some(key.to_string()),
        "workflow_plan_revise",
    );
    let completed_session = state
        .put_workflow_planner_session(completed_session)
        .await?;
    let details = json!({
        "ok": true,
        "action": "revise",
        "status": "completed",
        "resource": {
            "kind": "workflow_planner_session",
            "id": planner_session_id,
            "url": planner_url(planner_session_id),
        },
        "planner_session": completed_session,
        "revision": completed_revision,
        "blockers": [],
    });
    complete_idempotent(
        state,
        tenant,
        "operator.workflow_plan_revise",
        key,
        "workflow_planner_session",
        planner_session_id,
        details,
    )
    .await
}
