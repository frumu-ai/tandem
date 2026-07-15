// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn evaluate_automation_phase_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> anyhow::Result<Option<ToolPolicyDecision>> {
    let Some((run_id, mapped_node_id)) = state
        .automation_v2_session_run_and_node(&ctx.session_id)
        .await
    else {
        return Ok(None);
    };
    if !state.is_ready() {
        return Ok(None);
    }
    let Some(run) = state.get_automation_v2_run(&run_id).await else {
        return Ok(None);
    };
    let allowed_tools = state
        .engine_loop
        .get_session_allowed_tools(&ctx.session_id)
        .await;
    let allowed_patterns = allowed_tools
        .iter()
        .map(|name| normalize_tool_name(name))
        .collect::<Vec<_>>();
    if !allowed_patterns.is_empty() && any_policy_matches(&allowed_patterns, tool) {
        return Ok(None);
    }

    let node_id = mapped_node_id.or_else(|| phase_policy_single_active_node_id(&run));
    let phase = node_id
        .as_deref()
        .and_then(|id| phase_policy_node_label(&run, id))
        .unwrap_or_else(|| "unknown".to_string());
    let empty_allowlist = allowed_patterns.is_empty();
    let reason = if empty_allowlist {
        format!(
            "tool `{tool}` is denied because workflow phase `{phase}` for automation run `{}` has no allowed tools",
            run.run_id
        )
    } else {
        format!(
            "tool `{tool}` is not allowed during workflow phase `{phase}` for automation run `{}`",
            run.run_id
        )
    };
    let decision_id = record_automation_phase_tool_policy_decision(
        state,
        &run,
        ctx,
        node_id.as_deref(),
        &phase,
        tool,
        &allowed_patterns,
        empty_allowlist,
        &reason,
    )
    .await?;

    if state.is_ready() {
        state.event_bus.publish(EngineEvent::new(
            "automation.phase_tool.denied",
            json!({
                "sessionID": ctx.session_id.clone(),
                "messageID": ctx.message_id.clone(),
                "runID": run.run_id.clone(),
                "automationID": run.automation_id.clone(),
                "nodeID": node_id.clone(),
                "phase": phase.clone(),
                "tool": tool,
                "allowedTools": allowed_patterns.clone(),
                "policyDecisionID": decision_id.clone(),
                "reason": reason.clone(),
                "timestampMs": crate::now_ms(),
            }),
        ));
    }

    Ok(Some(ToolPolicyDecision {
        allowed: false,
        reason: Some(reason),
        policy_decision_id: Some(decision_id),
        dispatch_decision: None,
    }))
}

async fn session_allowlist_would_deny_non_automation_tool(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> bool {
    if state
        .automation_v2_session_run_and_node(&ctx.session_id)
        .await
        .is_some()
    {
        return false;
    }

    let allowed_tools = state
        .engine_loop
        .get_session_allowed_tools(&ctx.session_id)
        .await;
    if allowed_tools.is_empty() {
        return false;
    }

    let allowed_patterns = allowed_tools
        .iter()
        .map(|name| normalize_tool_name(name))
        .collect::<Vec<_>>();
    !any_policy_matches(&allowed_patterns, tool)
}

fn phase_policy_single_active_node_id(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> Option<String> {
    if run.checkpoint.pending_nodes.len() == 1 {
        return run.checkpoint.pending_nodes.first().cloned();
    }
    if run.checkpoint.blocked_nodes.len() == 1 {
        return run.checkpoint.blocked_nodes.first().cloned();
    }
    None
}

fn phase_policy_node_label(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: &str,
) -> Option<String> {
    let node = run
        .automation_snapshot
        .as_ref()?
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == node_id)?;
    node.metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("phase")
                .or_else(|| metadata.pointer("/builder/phase"))
                .or_else(|| metadata.pointer("/runtime/phase"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|phase| !phase.is_empty())
        .map(str::to_string)
        .or_else(|| {
            node.stage_kind.as_ref().and_then(|stage| {
                serde_json::to_value(stage)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
            })
        })
}

async fn record_automation_phase_tool_policy_decision(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    ctx: &ToolPolicyContext,
    node_id: Option<&str>,
    phase: &str,
    tool: &str,
    allowed_tools: &[String],
    empty_allowlist: bool,
    reason: &str,
) -> anyhow::Result<String> {
    let decision_id = format!("policy_decision_{}", Uuid::new_v4().simple());
    let metadata = json!({
        "phase_tool_authority": {
            "phase": phase,
            "allowed_tools": allowed_tools,
            "requested_tool": tool,
            "rule": "workflow_phase_tool_allowlist",
            "source": "automation_session_allowed_tools",
        }
    });
    let record = PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: run.tenant_context.clone(),
        requester_context: ctx
            .verified_tenant_context
            .as_ref()
            .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        actor_id: run.tenant_context.actor_id.clone(),
        session_id: Some(ctx.session_id.clone()),
        message_id: Some(ctx.message_id.clone()),
        run_id: Some(run.run_id.clone()),
        automation_id: Some(run.automation_id.clone()),
        node_id: node_id.map(str::to_string),
        tool: Some(tool.to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier: Some("workflow_phase_tool_scope".to_string()),
        decision: PolicyDecisionEffect::Deny,
        reason_code: if empty_allowlist {
            "phase_tool_authority_empty_allowlist".to_string()
        } else {
            "phase_tool_not_allowed".to_string()
        },
        reason: reason.to_string(),
        policy_id: Some("workflow_phase_tool_authority".to_string()),
        grant_id: None,
        approval_id: None,
        audit_event_id: None,
        created_at_ms: crate::now_ms(),
        metadata: metadata.clone(),
    };
    let recorded = state.record_policy_decision(record).await?.decision_id;

    crate::audit::append_protected_audit_event(
        state,
        "automation.phase_tool.denied",
        &run.tenant_context,
        run.tenant_context.actor_id.clone(),
        json!({
            "decision_id": recorded.clone(),
            "session_id": ctx.session_id.clone(),
            "message_id": ctx.message_id.clone(),
            "run_id": run.run_id.clone(),
            "automation_id": run.automation_id.clone(),
            "node_id": node_id,
            "phase": phase,
            "tool": tool,
            "allowed_tools": allowed_tools,
            "reason_code": "phase_tool_not_allowed",
            "requester_context": ctx
                .verified_tenant_context
                .as_ref()
                .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        }),
    )
    .await?;

    Ok(recorded)
}
