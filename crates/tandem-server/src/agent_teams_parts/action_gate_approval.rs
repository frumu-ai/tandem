struct ActionGateLinks {
    run_id: Option<String>,
    automation_id: Option<String>,
    node_id: Option<String>,
}

async fn action_gate_runtime_links(state: &AppState, session_id: &str) -> ActionGateLinks {
    if let Some((run_id, node_id)) = state
        .automation_v2_session_run_and_node(session_id)
        .await
    {
        let automation_id = state
            .automation_v2_runs
            .read()
            .await
            .get(&run_id)
            .map(|run| run.automation_id.clone());
        return ActionGateLinks {
            run_id: Some(run_id),
            automation_id,
            node_id,
        };
    }
    if let Some(policy) = state.routine_session_policy(session_id).await {
        return ActionGateLinks {
            run_id: Some(policy.run_id),
            automation_id: None,
            node_id: None,
        };
    }
    ActionGateLinks {
        run_id: state
            .agent_teams
            .instance_for_session(session_id)
            .await
            .and_then(|instance| instance.run_id),
        automation_id: None,
        node_id: None,
    }
}

async fn link_action_gate_policy_decision(
    state: &AppState,
    policy_decision_id: Option<&str>,
    ctx: &ToolPolicyContext,
    links: &ActionGateLinks,
    risk_tier: ToolRiskTier,
    action_hash: &str,
    approval_id: Option<&str>,
) -> anyhow::Result<()> {
    let policy_decision_id = policy_decision_id
        .ok_or_else(|| anyhow::anyhow!("action-gate policy decision was not persisted"))?;
    let mut record = state
        .get_policy_decision(policy_decision_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("action-gate policy decision not found"))?;
    record.session_id = Some(ctx.session_id.clone());
    record.message_id = Some(ctx.message_id.clone());
    record.run_id.clone_from(&links.run_id);
    record.automation_id.clone_from(&links.automation_id);
    record.node_id.clone_from(&links.node_id);
    record.approval_id = approval_id.map(str::to_string);
    record.metadata["action_gate"] = json!({
        "action_hash": action_hash,
        "policy_id": "approval_gate_matrix",
        "tool": ctx.tool,
        "risk_tier": risk_tier.as_str(),
        "approval_id": approval_id,
    });
    state.record_policy_decision(record).await?;
    Ok(())
}

/// Resolve protected tools against the approval matrix and durable governance
/// approval bridge. An approved receipt is consumed before this returns allow.
pub(crate) async fn evaluate_action_gate_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> Option<ToolPolicyDecision> {
    let descriptor = registered_tool_security_descriptor(state, tool)
        .await
        .unwrap_or_default();
    let risk_tier = tool_risk_tier_from_name_and_descriptor(tool, &descriptor);
    if !risk_tier.approval_required_by_default() {
        return None;
    }

    let tenant_context = ctx.tenant_context.clone().unwrap_or_default();
    let actor_id = tenant_context.actor_id.clone();
    let (outcome, policy_decision_id) = state
        .enforce_action_gate(
            &tenant_context,
            &GateRequest::new(Some(risk_tier), None),
            Some(tool.to_string()),
            actor_id.clone(),
            crate::now_ms(),
        )
        .await;
    if outcome.is_allowed() {
        return None;
    }

    let links = action_gate_runtime_links(state, &ctx.session_id).await;
    let action_hash = fintech_protected_action_hash(tool, &ctx.args);
    let mut approval_id = None;
    let mut approval_state =
        crate::app::state::governance_action_gate::ActionGateApprovalState::Denied;
    let mut approval_error = None;

    if matches!(outcome.effect, PolicyDecisionEffect::ApprovalRequired)
        && state.premium_governance_enabled()
    {
        let request = state
            .request_approval(
                crate::automation_v2::governance::GovernanceApprovalRequestType::ElevatedCapability,
                crate::automation_v2::governance::GovernanceActorRef::agent(
                    actor_id,
                    "action_gate_tool_policy",
                ),
                crate::automation_v2::governance::GovernanceResourceRef {
                    resource_type: "protected_action".to_string(),
                    id: action_hash.clone(),
                },
                format!("Review protected tool action `{tool}`"),
                json!({
                    "action_gate": {
                        "action_hash": action_hash,
                        "session_id": ctx.session_id,
                        "message_id": ctx.message_id,
                        "run_id": links.run_id,
                        "automation_id": links.automation_id,
                        "node_id": links.node_id,
                        "tool": tool,
                        "risk_tier": risk_tier.as_str(),
                        "policy_id": "approval_gate_matrix",
                        "policy_decision_id": policy_decision_id,
                    }
                }),
                Some(crate::now_ms().saturating_add(outcome.approval_ttl_ms)),
                &tenant_context,
            )
            .await;
        match request {
            Ok(request) => {
                approval_id = Some(request.approval_id.clone());
                match state
                    .consume_action_gate_approval(&request.approval_id, &tenant_context)
                    .await
                {
                    Ok(state) => approval_state = state,
                    Err(error) => approval_error = Some(error.to_string()),
                }
            }
            Err(error) => approval_error = Some(error.to_string()),
        }
    } else if matches!(outcome.effect, PolicyDecisionEffect::ApprovalRequired) {
        approval_error = Some("premium governance approval requests are unavailable".to_string());
    }

    if let Err(error) = link_action_gate_policy_decision(
        state,
        policy_decision_id.as_deref(),
        ctx,
        &links,
        risk_tier,
        &action_hash,
        approval_id.as_deref(),
    )
    .await
    {
        approval_error = Some(error.to_string());
    }

    if approval_error.is_none()
        && approval_state
            == crate::app::state::governance_action_gate::ActionGateApprovalState::ApprovedAndConsumed
    {
        return Some(ToolPolicyDecision {
            allowed: true,
            reason: None,
            policy_decision_id,
            dispatch_decision: None,
        });
    }

    let reason = if approval_state
        == crate::app::state::governance_action_gate::ActionGateApprovalState::Denied
        && approval_id.is_some()
    {
        format!("tool `{tool}` denied by runtime approval gate")
    } else {
        format!(
            "tool `{tool}` paused by runtime approval gate ({}): {}{}",
            outcome.reviewer_eligibility.as_str(),
            outcome.reason,
            approval_id
                .as_deref()
                .map(|id| format!(" (approval `{id}`)"))
                .unwrap_or_default()
        )
    };
    if state.is_ready() {
        state.event_bus.publish(EngineEvent::new(
            "approval.gate.tool.gated",
            json!({
                "sessionID": ctx.session_id,
                "messageID": ctx.message_id,
                "tool": tool,
                "riskTier": risk_tier.as_str(),
                "effect": outcome.effect,
                "reviewerEligibility": outcome.reviewer_eligibility.as_str(),
                "reasonCode": outcome.reason_code,
                "timestampMs": crate::now_ms(),
                "tenantContext": tenant_context,
            }),
        ));
    }
    Some(ToolPolicyDecision {
        allowed: false,
        reason: Some(reason),
        policy_decision_id,
        dispatch_decision: None,
    })
}
