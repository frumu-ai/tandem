// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::claims::CLAIM_LEASE;
use super::*;
use std::time::Duration;

const MAX_CONCURRENT_RECOVERIES: usize = 32;
const UNSAFE_EXECUTION_RECOVERY: &str = "unsafe Slack execution recovery";

pub(super) async fn run_slack_event_recovery_worker(state: AppState, cancel: CancellationToken) {
    let mut scan = tokio::time::interval(CLAIM_RECOVERY_SCAN_INTERVAL);
    scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut jobs = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = scan.tick() => {
                while let Some(result) = jobs.try_join_next() {
                    if let Err(error) = result {
                        tracing::warn!(%error, "recovered Slack event task failed to join");
                    }
                }
                match compact_slack_event_claims(&state, crate::now_ms()).await {
                    Ok(removed) if removed > 0 => tracing::info!(removed, "compacted completed Slack event claims"),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "Slack event claim compaction failed"),
                }
                let capacity = MAX_CONCURRENT_RECOVERIES.saturating_sub(jobs.len());
                if capacity == 0 {
                    continue;
                }
                match recover_slack_event_claims(&state, crate::now_ms(), capacity).await {
                    Ok(recovered) => {
                        for recovered in recovered {
                            let job_state = state.clone();
                            let job_cancel = cancel.child_token();
                            jobs.spawn(async move {
                                resume_recovered_slack_event(job_state, recovered, job_cancel).await;
                            });
                        }
                    }
                    Err(error) => tracing::warn!(%error, "Slack event recovery scan failed"),
                }
            }
            result = jobs.join_next(), if !jobs.is_empty() => {
                if let Some(Err(error)) = result {
                    tracing::warn!(%error, "recovered Slack event task failed to join");
                }
            }
        }
    }

    while let Some(result) = jobs.join_next().await {
        if let Err(error) = result {
            tracing::warn!(%error, "recovered Slack event task failed during shutdown");
        }
    }
}

async fn resume_recovered_slack_event(
    state: AppState,
    recovered: RecoverableSlackEventClaim,
    cancel: CancellationToken,
) {
    let claim = recovered.claim;
    let prepared = prepare_recovered_slack_event(&state, recovered.recovery_payload).await;
    let (connection, event, installation, verified) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            tracing::warn!(
                target: "tandem_server::slack_events",
                event_key = %claim.key,
                %error,
                "recovered Slack event failed current binding validation"
            );
            let _ = retry_slack_event_claim(&claim, &error.to_string(), crate::now_ms()).await;
            return;
        }
    };
    if slack_event_fingerprint(&event, &installation) != claim.fingerprint {
        let error = "recovered Slack event fingerprint does not match durable claim";
        tracing::error!(target: "tandem_server::slack_events", event_key = %claim.key, error,);
        let _ = retry_slack_event_claim(&claim, error, crate::now_ms()).await;
        return;
    }
    if !verified.tenant_matches(&claim.tenant_context) {
        let error = "recovered Slack event tenant binding changed";
        tracing::warn!(target: "tandem_server::slack_events", event_key = %claim.key, error,);
        let _ = retry_slack_event_claim(&claim, error, crate::now_ms()).await;
        return;
    }
    run_claimed_slack_event(
        state,
        connection,
        event,
        installation,
        verified,
        claim,
        cancel,
    )
    .await;
}

async fn prepare_recovered_slack_event(
    state: &AppState,
    payload: Value,
) -> anyhow::Result<(
    ResolvedSlackConnection,
    SlackMessageEvent,
    SlackInstallationBinding,
    VerifiedTenantContext,
)> {
    let recovered: SlackEventRecoveryPayload =
        serde_json::from_value(payload).context("parse durable Slack recovery payload")?;
    let effective_config = state.config.get_effective_value().await;
    let connections = slack_connections_from_effective_config(&effective_config);
    anyhow::ensure!(
        connections
            .iter()
            .any(|connection| connection.events_enabled),
        "Slack events are no longer enabled"
    );
    anyhow::ensure!(
        connections
            .iter()
            .any(|connection| connection.team_id.as_deref()
                == Some(recovered.installation.team_id.as_str())),
        "recovered Slack team does not match current configuration"
    );
    let installation_connections = connections
        .iter()
        .filter(|connection| {
            connection.team_id.as_deref() == Some(recovered.installation.team_id.as_str())
                && connection.app_id.as_deref() == Some(recovered.installation.app_id.as_str())
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !installation_connections.is_empty(),
        "recovered Slack app does not match current configuration"
    );
    let connection = installation_connections
        .into_iter()
        .find(|connection| connection.channel_id == recovered.event.channel_id)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!("recovered Slack channel does not match current configuration")
        })?;
    anyhow::ensure!(
        connection.events_enabled,
        "Slack events are no longer enabled"
    );
    let principal = match resolve_slack_user_for_connection(
        &connection.allowed_users,
        &recovered.installation.team_id,
        &recovered.installation.app_id,
        &recovered.event.user_id,
    ) {
        ChannelIdentityResolution::Resolved(principal) => principal,
        ChannelIdentityResolution::Denied { .. } => {
            anyhow::bail!("recovered Slack user is no longer authorized")
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            anyhow::bail!("Slack channel is no longer configured")
        }
    };
    let verified = build_governed_slack_context(
        state,
        &effective_config,
        &connection,
        &recovered.event,
        &recovered.installation,
        principal,
    )
    .await
    .map_err(anyhow::Error::msg)?;
    Ok((
        connection,
        recovered.event,
        recovered.installation,
        verified,
    ))
}

pub(super) async fn build_governed_slack_context(
    state: &AppState,
    effective_config: &Value,
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    request_principal: RequestPrincipal,
) -> Result<VerifiedTenantContext, String> {
    let actor_id = request_principal
        .actor_id
        .clone()
        .ok_or_else(|| "resolved Slack principal is missing actor_id".to_string())?;
    let (org_id, workspace_id) = connection
        .bound_tenant()
        .ok_or_else(|| "slack channel must be bound to a governed tenant".to_string())?;
    let mut tenant_context =
        TenantContext::explicit(org_id.clone(), workspace_id.clone(), Some(actor_id.clone()));
    tenant_context.deployment_id = connection.tenant_deployment_id.clone();

    let principal = PrincipalRef::human_user(actor_id.clone());
    let now_ms = crate::now_ms();
    let graph = state
        .build_intra_tenant_authority_graph(&tenant_context, Vec::new())
        .await;
    let resolved_units = graph.resolved_unit_principals(&principal, now_ms);
    let active_units = graph
        .units
        .iter()
        .filter(|unit| unit.state.is_active() && resolved_units.contains(&unit.principal_ref()))
        .collect::<Vec<_>>();
    if active_units.is_empty() {
        return Err("Slack user has no active organization-unit membership".to_string());
    }

    // TAN-764: a department-bound connection narrows the run's authority to
    // the intersection of the user's active units and the channel's bound
    // units. The user's verified memberships stay authoritative — the channel
    // can only narrow, never widen — and an empty intersection fails closed.
    // Roles, grants, and capabilities below all derive from the intersected
    // set, so both memory reach and the tool surface narrow with it.
    let active_units = if !connection.binds_departments() {
        active_units
    } else {
        let user_unit_ids = active_units
            .iter()
            .map(|unit| unit.principal_ref().id)
            .collect::<Vec<_>>();
        let intersected = active_units
            .into_iter()
            .filter(|unit| connection.binds_org_unit(&unit.principal_ref().id, &unit.unit_id))
            .collect::<Vec<_>>();
        if intersected.is_empty() {
            return Err(format!(
                "Slack user's organization units do not intersect the channel's bound org units (user: [{}]; channel: [{}])",
                user_unit_ids.join(", "),
                connection.org_units.join(", ")
            ));
        }
        intersected
    };

    let active_unit_principals = active_units
        .iter()
        .map(|unit| unit.principal_ref())
        .collect::<Vec<_>>();
    let mut org_units = active_unit_principals
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<Vec<_>>();
    org_units.sort();
    org_units.dedup();
    let mut roles = active_units
        .iter()
        .filter(|unit| unit.kind == OrganizationUnitKind::RoleDomain)
        .map(|unit| unit.unit_id.clone())
        .collect::<Vec<_>>();
    roles.sort();
    roles.dedup();

    let mut grants = narrow_grants_to_channel_authority(
        graph.effective_grants(&principal, now_ms),
        &active_unit_principals,
        connection.binds_departments(),
    );
    grants.sort_by(|left, right| left.grant_id.cmp(&right.grant_id));
    let capabilities = capabilities_from_grants(&grants);
    let authority_chain = AuthorityChain::from_request(request_principal);
    let assertion_id = format!(
        "slack-event:{}:{}:{}",
        installation.team_id, installation.app_id, event.event_id
    );
    let expires_at_ms = now_ms.saturating_add(SLACK_CONTEXT_TTL_MS);
    let assertion = AssertionMetadata::new(
        SLACK_CONTEXT_ISSUER,
        SLACK_CONTEXT_AUDIENCE,
        now_ms,
        expires_at_ms,
        assertion_id.clone(),
    );
    let strict_projection = StrictTenantContext::new(
        tenant_context.clone(),
        principal,
        authority_chain.clone(),
        ResourceScope::root(ResourceRef::new(
            org_id,
            workspace_id.clone(),
            ResourceKind::Workspace,
            workspace_id,
        )),
        assertion,
    )
    .with_grants(grants)
    .with_data_boundary(DataBoundary::unrestricted());
    let mut verified = VerifiedTenantContext {
        tenant_context,
        human_actor: HumanActor {
            actor_id,
            provider: Some("slack".to_string()),
            issuer: Some(format!(
                "slack-events:{}:{}",
                installation.team_id, installation.app_id
            )),
            subject: Some(slack_installation_identity(
                &installation.team_id,
                &installation.app_id,
                &event.user_id,
            )),
            email: None,
        },
        authority_chain,
        roles,
        org_units,
        capabilities,
        policy_version: effective_config
            .pointer("/governance/policy_version")
            .and_then(Value::as_u64),
        strict_projection: Some(strict_projection),
        issuer: SLACK_CONTEXT_ISSUER.to_string(),
        audience: SLACK_CONTEXT_AUDIENCE.to_string(),
        issued_at_ms: now_ms,
        expires_at_ms,
        assertion_id,
        assertion_key_id: None,
    };
    super::super::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
        state,
        &mut verified,
    )
    .await;
    if let Some(strict) = verified.strict_projection.as_ref() {
        verified.capabilities = capabilities_from_grants(&strict.grants);
    }
    Ok(verified)
}

/// TAN-764: which effective grants a channel-scoped run may carry.
/// Unit-sourced grants survive only when their source unit is in the
/// (possibly intersected) active set. A grant without a source unit is
/// DIRECT (personal) authority — a department-bound channel narrows the run
/// to the intersected units only, so a personal engineering grant must not
/// ride into a sales-bound channel; direct grants survive only on unbound
/// connections.
fn narrow_grants_to_channel_authority(
    grants: Vec<tandem_types::ScopedGrant>,
    active_unit_principals: &[PrincipalRef],
    binds_departments: bool,
) -> Vec<tandem_types::ScopedGrant> {
    grants
        .into_iter()
        .filter(|grant| match grant.source_principal.as_ref() {
            Some(source) => active_unit_principals.contains(source),
            None => !binds_departments,
        })
        .collect()
}

fn capabilities_from_grants(grants: &[tandem_types::ScopedGrant]) -> Vec<String> {
    let mut capabilities = grants
        .iter()
        .filter(|grant| grant.effect == AccessEffect::Allow)
        .flat_map(|grant| grant.tool_patterns.iter())
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

pub(super) async fn run_claimed_slack_event(
    state: AppState,
    connection: ResolvedSlackConnection,
    event: SlackMessageEvent,
    installation: SlackInstallationBinding,
    verified: VerifiedTenantContext,
    claim: SlackEventClaim,
    cancel: CancellationToken,
) {
    let processing = process_governed_slack_event(
        state.clone(),
        connection,
        event,
        installation,
        verified,
        &claim,
        cancel.clone(),
    );
    tokio::pin!(processing);
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + CLAIM_HEARTBEAT,
        CLAIM_HEARTBEAT,
    );
    let result = loop {
        tokio::select! {
            result = &mut processing => break result,
            _ = heartbeat.tick() => {
                match refresh_slack_event_claim(&claim, crate::now_ms()).await {
                    Ok(true) => {}
                    Ok(false) => {
                        cancel.cancel();
                        let _ = (&mut processing).await;
                        break Err(anyhow::anyhow!("lost ownership of durable Slack event claim"));
                    }
                    Err(error) => {
                        cancel.cancel();
                        let _ = (&mut processing).await;
                        break Err(error.context("refresh durable Slack event claim"));
                    }
                }
            }
        }
    };

    match result {
        Ok(session_id) => {
            match complete_slack_event_claim(&claim, &session_id, crate::now_ms()).await {
                Ok(true) => {}
                Ok(false) => tracing::error!(
                    target: "tandem_server::slack_events",
                    event_key = %claim.key,
                    "lost Slack event claim before durable completion"
                ),
                Err(error) => {
                    tracing::error!(
                        target: "tandem_server::slack_events",
                        event_key = %claim.key,
                        %error,
                        "failed to complete durable Slack event claim"
                    );
                    if let Err(retry_error) = retry_slack_event_claim(
                        &claim,
                        &format!("durable completion failed: {error}"),
                        crate::now_ms(),
                    )
                    .await
                    {
                        tracing::error!(
                            target: "tandem_server::slack_events",
                            event_key = %claim.key,
                            %retry_error,
                            "failed to make incomplete Slack event retryable"
                        );
                    }
                }
            }
        }
        Err(error) => {
            if error.to_string().contains(UNSAFE_EXECUTION_RECOVERY) {
                tracing::error!(
                    target: "tandem_server::slack_events",
                    event_key = %claim.key,
                    %error,
                    "governed Slack event cannot be safely replayed; quarantining claim"
                );
                if let Err(quarantine_error) =
                    quarantine_slack_event_claim(&claim, &error.to_string(), crate::now_ms()).await
                {
                    tracing::error!(
                        target: "tandem_server::slack_events",
                        event_key = %claim.key,
                        %quarantine_error,
                        "failed to quarantine unsafe Slack event claim"
                    );
                }
                return;
            }
            tracing::error!(
                target: "tandem_server::slack_events",
                event_key = %claim.key,
                %error,
                "governed Slack event processing failed; claim is retryable"
            );
            if let Err(retry_error) =
                retry_slack_event_claim(&claim, &error.to_string(), crate::now_ms()).await
            {
                tracing::error!(
                    target: "tandem_server::slack_events",
                    event_key = %claim.key,
                    %retry_error,
                    "failed to mark Slack event claim retryable"
                );
            }
        }
    }
}

async fn process_governed_slack_event(
    state: AppState,
    connection: ResolvedSlackConnection,
    event: SlackMessageEvent,
    installation: SlackInstallationBinding,
    verified: VerifiedTenantContext,
    claim: &SlackEventClaim,
    cancel: CancellationToken,
) -> anyhow::Result<String> {
    let lock_key = format!(
        "{}:{}:{}:{}",
        verified.tenant_context.org_id,
        verified.tenant_context.workspace_id,
        event.scope_id(&installation),
        event.user_id
    );
    let lock = slack_execution_lock(&lock_key).await;
    let _guard = tokio::select! {
        guard = lock.lock() => guard,
        _ = cancel.cancelled() => anyhow::bail!("Slack event cancelled before execution lock"),
    };

    if let (Some(session_id), Some(response)) = (
        claim.session_id.as_deref(),
        claim.pending_response.as_deref(),
    ) {
        let delivery_checkpoint_reused = claim.response_delivered_at_ms.is_some();
        if !delivery_checkpoint_reused {
            let delivery = tokio::select! {
                result = deliver_slack_reply(&connection, &event, &installation, response) => result,
                _ = cancel.cancelled() => anyhow::bail!("Slack event cancelled before response replay"),
            };
            if let Err(error) = delivery {
                let _ = emit_slack_tenant_audit(
                    &state,
                    &verified.tenant_context,
                    Some(verified.human_actor.actor_id.clone()),
                    "channel.slack.response.failed",
                    json!({
                        "error": error.to_string(),
                        "replayed_staged_response": true,
                        "dimensions": slack_audit_dimensions(&event, &installation, Some(session_id)),
                    }),
                )
                .await;
                return Err(error);
            }
            if !mark_slack_event_response_delivered(claim, crate::now_ms()).await? {
                anyhow::bail!("lost Slack event claim before delivery checkpoint");
            }
        }
        if claim.response_audited_at_ms.is_none() {
            emit_slack_tenant_audit(
                &state,
                &verified.tenant_context,
                Some(verified.human_actor.actor_id.clone()),
                "channel.slack.response.delivered",
                json!({
                    "response_sha256": crate::sha256_hex(&[response]),
                    "replayed_staged_response": true,
                    "delivery_checkpoint_reused": delivery_checkpoint_reused,
                    "dimensions": slack_audit_dimensions(&event, &installation, Some(session_id)),
                }),
            )
            .await?;
            if !mark_slack_event_response_audited(claim, crate::now_ms()).await? {
                anyhow::bail!("lost Slack event claim before response audit checkpoint");
            }
        }
        return Ok(session_id.to_string());
    }

    if claim.session_id.is_some() || claim.run_id.is_some() {
        let (session_id, response) =
            reconcile_checkpointed_slack_execution(&state, claim, &cancel).await?;
        return deliver_and_checkpoint_slack_response(
            &state,
            &connection,
            &event,
            &installation,
            &verified,
            claim,
            &session_id,
            &response,
            true,
            &cancel,
        )
        .await;
    }

    let session_id =
        get_or_create_governed_slack_session(&state, &connection, &event, &installation, &verified)
            .await?;
    let session_message_count = state
        .storage
        .get_session(&session_id)
        .await
        .map(|session| session.messages.len())
        .ok_or_else(|| anyhow::anyhow!("governed Slack session disappeared before execution"))?;
    let run_id = deterministic_slack_run_id(&claim.key);
    emit_slack_tenant_audit(
        &state,
        &verified.tenant_context,
        Some(verified.human_actor.actor_id.clone()),
        "channel.slack.run.started",
        json!({
            "attempt": claim.attempt,
            "roles": verified.roles,
            "org_units": verified.org_units,
            "channel_org_units": connection.org_units,
            "tool_capabilities": verified.capabilities,
            "grant_ids": verified.strict_projection.as_ref().map(|strict| {
                strict.grants.iter().map(|grant| grant.grant_id.clone()).collect::<Vec<_>>()
            }).unwrap_or_default(),
            "dimensions": slack_audit_dimensions(&event, &installation, Some(&session_id)),
            "run_id": &run_id,
        }),
    )
    .await?;

    let reply = run_governed_slack_prompt_in_session(
        &state,
        &connection,
        &event,
        &installation,
        &verified,
        claim,
        &session_id,
        &run_id,
        session_message_count,
        &cancel,
    )
    .await;
    let reply = match reply {
        Ok(reply) => reply,
        Err(error) => {
            let _ = emit_slack_tenant_audit(
                &state,
                &verified.tenant_context,
                Some(verified.human_actor.actor_id.clone()),
                "channel.slack.run.failed",
                json!({
                    "error": error.to_string(),
                    "dimensions": slack_audit_dimensions(&event, &installation, Some(&session_id)),
                }),
            )
            .await;
            return Err(error);
        }
    };
    emit_slack_tenant_audit(
        &state,
        &verified.tenant_context,
        Some(verified.human_actor.actor_id.clone()),
        "channel.slack.run.completed",
        json!({
            "response_sha256": crate::sha256_hex(&[&reply]),
            "dimensions": slack_audit_dimensions(&event, &installation, Some(&session_id)),
        }),
    )
    .await?;

    let response = redact_slack_response(&reply);
    deliver_and_checkpoint_slack_response(
        &state,
        &connection,
        &event,
        &installation,
        &verified,
        claim,
        &session_id,
        &response,
        false,
        &cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn deliver_and_checkpoint_slack_response(
    state: &AppState,
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    verified: &VerifiedTenantContext,
    claim: &SlackEventClaim,
    session_id: &str,
    response: &str,
    reconciled_execution: bool,
    cancel: &CancellationToken,
) -> anyhow::Result<String> {
    if !stage_slack_event_response(claim, session_id, response, crate::now_ms()).await? {
        anyhow::bail!("lost Slack event claim before staging response");
    }
    let delivery = tokio::select! {
        result = deliver_slack_reply(connection, event, installation, response) => result,
        _ = cancel.cancelled() => Err(anyhow::anyhow!("Slack event cancelled before response delivery")),
    };
    if let Err(error) = delivery {
        let _ = emit_slack_tenant_audit(
            state,
            &verified.tenant_context,
            Some(verified.human_actor.actor_id.clone()),
            "channel.slack.response.failed",
            json!({
                "error": error.to_string(),
                "replayed_staged_response": false,
                "reconciled_execution": reconciled_execution,
                "dimensions": slack_audit_dimensions(event, installation, Some(session_id)),
            }),
        )
        .await;
        return Err(error);
    }
    if !mark_slack_event_response_delivered(claim, crate::now_ms()).await? {
        anyhow::bail!("lost Slack event claim before delivery checkpoint");
    }
    emit_slack_tenant_audit(
        state,
        &verified.tenant_context,
        Some(verified.human_actor.actor_id.clone()),
        "channel.slack.response.delivered",
        json!({
            "response_sha256": crate::sha256_hex(&[&response]),
            "replayed_staged_response": false,
            "reconciled_execution": reconciled_execution,
            "dimensions": slack_audit_dimensions(event, installation, Some(session_id)),
        }),
    )
    .await?;
    if !mark_slack_event_response_audited(claim, crate::now_ms()).await? {
        anyhow::bail!("lost Slack event claim before response audit checkpoint");
    }
    Ok(session_id.to_string())
}

async fn run_governed_slack_prompt_in_session(
    state: &AppState,
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    verified: &VerifiedTenantContext,
    claim: &SlackEventClaim,
    session_id: &str,
    run_id: &str,
    prior_message_count: usize,
    cancel: &CancellationToken,
) -> anyhow::Result<String> {
    let model = slack_model_spec(connection);
    let memory_subject = format!(
        "{}:{}:{}",
        installation.team_id, installation.app_id, event.user_id
    );
    let client_id = channel_memory_subject_client_id("slack", &memory_subject);
    let request = SendMessageRequest {
        parts: vec![MessagePartInput::Text {
            text: event.text.clone(),
        }],
        model,
        agent: None,
        tool_mode: None,
        tool_allowlist: Some(verified.capabilities.clone()),
        strict_kb_grounding: connection.strict_kb_grounding,
        context_mode: None,
        write_required: None,
        prewrite_requirements: None,
        sampling: SamplingParams::default(),
    };

    state
        .run_registry
        .acquire(
            session_id,
            run_id.to_string(),
            client_id.clone(),
            None,
            None,
        )
        .await
        .map_err(|active| {
            anyhow::anyhow!(
                "Slack session already has active run {} while reserving {}",
                active.run_id,
                run_id
            )
        })?;
    let checkpointed = checkpoint_slack_event_execution(
        claim,
        session_id,
        run_id,
        prior_message_count,
        crate::now_ms(),
    )
    .await;
    match checkpointed {
        Ok(true) => {}
        Ok(false) => {
            let _ = state.run_registry.finish_if_match(session_id, run_id).await;
            anyhow::bail!("lost Slack event claim before execution checkpoint");
        }
        Err(error) => {
            let _ = state.run_registry.finish_if_match(session_id, run_id).await;
            return Err(error.context("persist Slack execution checkpoint"));
        }
    }

    super::super::sessions::publish_tenant_event(
        state,
        &verified.tenant_context,
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": run_id,
            "clientID": client_id,
            "correlationID": claim.key,
            "source": "slack_event_claim",
        }),
    );
    let execution = super::super::sessions::execute_run(
        state.clone(),
        session_id.to_string(),
        run_id.to_string(),
        request,
        Some(claim.key.clone()),
        client_id,
        verified.tenant_context.clone(),
    );
    tokio::pin!(execution);
    let execution_result = tokio::select! {
        result = &mut execution => result,
        _ = cancel.cancelled() => {
            let _ = state.cancellations.cancel(session_id).await;
            (&mut execution).await
        }
    };
    execution_result.map_err(|error| {
        anyhow::anyhow!(
            "{UNSAFE_EXECUTION_RECOVERY}: deterministic run {run_id} failed after execution checkpoint: {error}"
        )
    })?;
    let session = state
        .storage
        .get_session(session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("governed Slack session disappeared after prompt"))?;
    assistant_text_after(&session, prior_message_count)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{UNSAFE_EXECUTION_RECOVERY}: deterministic run {run_id} has no reconcilable assistant response"
            )
        })
}

fn deterministic_slack_run_id(claim_key: &str) -> String {
    format!("slack-{}", crate::sha256_hex(&[claim_key]))
}

async fn reconcile_checkpointed_slack_execution(
    state: &AppState,
    claim: &SlackEventClaim,
    cancel: &CancellationToken,
) -> anyhow::Result<(String, String)> {
    let session_id = claim.session_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!("{UNSAFE_EXECUTION_RECOVERY}: claim has a partial session checkpoint")
    })?;
    let run_id = claim.run_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!("{UNSAFE_EXECUTION_RECOVERY}: claim has a partial run checkpoint")
    })?;
    let message_count = claim.session_message_count.ok_or_else(|| {
        anyhow::anyhow!("{UNSAFE_EXECUTION_RECOVERY}: claim has no session message checkpoint")
    })?;

    let active_run_deadline = tokio::time::Instant::now() + CLAIM_LEASE;
    loop {
        match state.run_registry.get(session_id).await {
            Some(active) if active.run_id == run_id => {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    _ = tokio::time::sleep_until(active_run_deadline) => {
                        anyhow::bail!(
                            "{UNSAFE_EXECUTION_RECOVERY}: deterministic run {run_id} did not reconcile before the recovery deadline"
                        );
                    }
                    _ = cancel.cancelled() => {
                        anyhow::bail!(
                            "{UNSAFE_EXECUTION_RECOVERY}: recovery cancelled while deterministic run {run_id} remained active"
                        );
                    }
                }
            }
            Some(active) => {
                anyhow::bail!(
                    "{UNSAFE_EXECUTION_RECOVERY}: session {session_id} is occupied by run {} while reconciling {run_id}",
                    active.run_id
                );
            }
            None => break,
        }
    }

    let session = state.storage.get_session(session_id).await.ok_or_else(|| {
        anyhow::anyhow!(
            "{UNSAFE_EXECUTION_RECOVERY}: checkpointed session {session_id} no longer exists"
        )
    })?;
    anyhow::ensure!(
        claim.tenant_context.org_id == session.tenant_context.org_id
            && claim.tenant_context.workspace_id == session.tenant_context.workspace_id
            && claim.tenant_context.deployment_id == session.tenant_context.deployment_id,
        "{UNSAFE_EXECUTION_RECOVERY}: checkpointed session tenant binding changed"
    );
    let response = assistant_text_after(&session, message_count).ok_or_else(|| {
        anyhow::anyhow!(
            "{UNSAFE_EXECUTION_RECOVERY}: deterministic run {run_id} may have executed but has no durable assistant response"
        )
    })?;
    Ok((session_id.to_string(), redact_slack_response(&response)))
}

async fn get_or_create_governed_slack_session(
    state: &AppState,
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    verified: &VerifiedTenantContext,
) -> anyhow::Result<String> {
    let scope_id = event.scope_id(installation);
    let model = slack_model_spec(connection);
    if let Some(mut session) = state
        .storage
        .list_sessions()
        .await
        .into_iter()
        .find(|session| {
            session.source_kind.as_deref() == Some("channel")
                && session
                    .source_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("channel"))
                    .and_then(Value::as_str)
                    == Some("slack")
                && session
                    .source_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("user_id"))
                    .and_then(Value::as_str)
                    == Some(event.user_id.as_str())
                && session
                    .source_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("slack_team_id"))
                    .and_then(Value::as_str)
                    == Some(installation.team_id.as_str())
                && session
                    .source_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("slack_app_id"))
                    .and_then(Value::as_str)
                    == Some(installation.app_id.as_str())
                && session
                    .source_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("scope_id"))
                    .and_then(Value::as_str)
                    == Some(scope_id.as_str())
                && verified.tenant_matches(&session.tenant_context)
        })
    {
        session.tenant_context = verified.tenant_context.clone();
        session.verified_tenant_context = Some(verified.clone());
        session.model = model.clone();
        session.provider = model.as_ref().map(|model| model.provider_id.clone());
        session.source_metadata = Some(slack_session_metadata(event, installation));
        session.time.updated = chrono::Utc::now();
        let session_id = session.id.clone();
        state.storage.save_session(session).await?;
        return Ok(session_id);
    }

    let security_profile = connection.security_profile;
    let request = CreateSessionRequest {
        parent_id: None,
        title: Some(format!("slack - {} - {scope_id}", event.user_id)),
        directory: Some(".".to_string()),
        workspace_root: None,
        pinned_workspace_id: None,
        project_id: None,
        model: model.clone(),
        provider: model.as_ref().map(|model| model.provider_id.clone()),
        sampling: SamplingParams::default(),
        source_kind: Some("channel".to_string()),
        source_metadata: Some(slack_session_metadata(event, installation)),
        permission: None,
    };
    let created = super::super::sessions::create_session(
        State(state.clone()),
        Extension(verified.tenant_context.clone()),
        Some(Extension(verified.clone())),
        Json(request),
    )
    .await
    .map_err(|(status, _)| anyhow::anyhow!("session creation failed with {status}"))?;
    let session_id = created.0.id;
    // These rules are derived from the administrator-controlled channel
    // security profile, not the Slack caller. Apply them through the internal
    // seam after the public session route has rejected caller-authored hosted
    // rules, and bind them to this exact session.
    super::super::sessions::apply_session_permission_rules(
        state,
        &verified.tenant_context,
        &session_id,
        Some(build_channel_session_permissions(security_profile)),
    )
    .await;
    Ok(session_id)
}

fn slack_session_metadata(
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
) -> Value {
    json!({
        "channel": "slack",
        "user_id": event.user_id,
        "scope_kind": "thread",
        "scope_id": event.scope_id(installation),
        "slack_team_id": installation.team_id,
        "slack_app_id": installation.app_id,
        "slack_channel_id": event.channel_id,
        "slack_thread_ts": event.thread_anchor(),
        "last_event_id": event.event_id,
    })
}

fn slack_model_spec(connection: &ResolvedSlackConnection) -> Option<ModelSpec> {
    Some(ModelSpec {
        provider_id: connection.model_provider_id.clone()?,
        model_id: connection.model_id.clone()?,
    })
}

fn assistant_text_after(session: &tandem_types::Session, start: usize) -> Option<String> {
    session
        .messages
        .get(start..)?
        .iter()
        .rev()
        .find_map(|message| {
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
        })
}

fn redact_slack_response(reply: &str) -> String {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    redact_outbound(reply, &workspace_root)
}

async fn deliver_slack_reply(
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    reply: &str,
) -> anyhow::Result<()> {
    verify_slack_bot_binding(connection, event, installation).await?;
    let bot_token = connection
        .bot_token
        .clone()
        .ok_or_else(|| anyhow::anyhow!("slack bot token not configured"))?;
    let channel_id = if connection.channel_id.is_empty() {
        event.channel_id.clone()
    } else {
        connection.channel_id.clone()
    };
    let slack_config = SlackConfig {
        bot_token,
        channel_id,
        allowed_users: connection.allowed_users.clone(),
        mention_only: connection.mention_only,
        security_profile: connection.security_profile,
    };
    let channel = match connection.api_base_url.clone() {
        Some(api_base_url) => SlackChannel::new_with_api_base_url(slack_config, api_base_url),
        None => SlackChannel::new(slack_config),
    };
    channel
        .send_thread_reply(&ThreadReply {
            content: reply.to_string(),
            recipient: event.channel_id.clone(),
            thread_id: event.thread_anchor().to_string(),
        })
        .await
}

async fn verify_slack_bot_binding(
    connection: &ResolvedSlackConnection,
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        connection.team_id.as_deref() == Some(installation.team_id.as_str()),
        "Slack outbound team binding changed"
    );
    anyhow::ensure!(
        connection.app_id.as_deref() == Some(installation.app_id.as_str()),
        "Slack outbound app binding changed"
    );
    anyhow::ensure!(
        connection.channel_id == event.channel_id,
        "Slack outbound channel binding changed"
    );
    let bot_token = connection
        .bot_token
        .clone()
        .context("slack bot token not configured")?;
    let api_base_url = connection
        .api_base_url
        .clone()
        .unwrap_or_else(|| "https://slack.com/api".to_string());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let response = client
        .get(format!("{}/auth.test", api_base_url.trim_end_matches('/')))
        .bearer_auth(&bot_token)
        .send()
        .await
        .context("Slack auth.test request failed")?;
    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .context("Slack auth.test response was not JSON")?;
    anyhow::ensure!(status.is_success(), "Slack auth.test failed with {status}");
    anyhow::ensure!(
        body.get("ok") == Some(&Value::Bool(true)),
        "Slack auth.test rejected bot token: {}",
        body.get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    anyhow::ensure!(
        config_string(&body, "/team_id").as_deref() == Some(installation.team_id.as_str()),
        "Slack bot token belongs to a different team"
    );
    let bot_id =
        config_string(&body, "/bot_id").context("Slack auth.test token is not a bot identity")?;
    anyhow::ensure!(
        config_string(&body, "/user_id").is_some(),
        "Slack auth.test response is missing bot user identity"
    );
    let response = client
        .get(format!("{}/bots.info", api_base_url.trim_end_matches('/')))
        .bearer_auth(&bot_token)
        .query(&[("bot", bot_id.as_str())])
        .send()
        .await
        .context("Slack bots.info request failed")?;
    let status = response.status();
    let body = response
        .json::<Value>()
        .await
        .context("Slack bots.info response was not JSON")?;
    anyhow::ensure!(status.is_success(), "Slack bots.info failed with {status}");
    anyhow::ensure!(
        body.get("ok") == Some(&Value::Bool(true)),
        "Slack bots.info rejected bot identity: {}",
        body.get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    anyhow::ensure!(
        config_string(&body, "/bot/id").as_deref() == Some(bot_id.as_str()),
        "Slack bots.info returned a different bot identity"
    );
    anyhow::ensure!(
        config_string(&body, "/bot/app_id").as_deref() == Some(installation.app_id.as_str()),
        "Slack bot token belongs to a different app"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::{GrantSource, ScopedGrant};

    fn unit_grant(id: &str, unit: &PrincipalRef) -> ScopedGrant {
        ScopedGrant::new(
            id,
            PrincipalRef::human_user("channel:slack:T1:A1:U1"),
            ResourceRef::new("org", "ws", ResourceKind::Workspace, "ws"),
            GrantSource::OrganizationUnitMembership,
        )
        .with_source_principal(unit.clone())
    }

    fn direct_grant(id: &str) -> ScopedGrant {
        ScopedGrant::new(
            id,
            PrincipalRef::human_user("channel:slack:T1:A1:U1"),
            ResourceRef::new("org", "ws", ResourceKind::Workspace, "ws"),
            GrantSource::Direct,
        )
    }

    #[test]
    fn department_bound_channels_drop_direct_grants() {
        let sales = PrincipalRef::organization_unit("department/sales");
        let engineering = PrincipalRef::organization_unit("department/engineering");
        let intersected = vec![sales.clone()];
        let grants = vec![
            unit_grant("sales-grant", &sales),
            unit_grant("eng-grant", &engineering),
            direct_grant("personal-grant"),
        ];

        let narrowed = narrow_grants_to_channel_authority(grants.clone(), &intersected, true);
        assert_eq!(
            narrowed
                .iter()
                .map(|grant| grant.grant_id.as_str())
                .collect::<Vec<_>>(),
            vec!["sales-grant"],
            "a department-bound channel must carry only grants sourced from \
             the intersected units — never direct (personal) grants"
        );

        // Unbound connections keep the legacy behavior: direct grants stay,
        // unit grants still require an active source unit.
        let unbound = narrow_grants_to_channel_authority(grants, &intersected, false);
        assert_eq!(
            unbound
                .iter()
                .map(|grant| grant.grant_id.as_str())
                .collect::<Vec<_>>(),
            vec!["sales-grant", "personal-grant"]
        );
    }
}
