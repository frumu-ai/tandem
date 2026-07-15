async fn evaluate_enterprise_authored_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> anyhow::Result<Option<ToolPolicyDecision>> {
    use tandem_enterprise_contract::{
        enterprise_scope_ids_match, AccessPermission, EnterprisePolicyEffect,
        EnterprisePolicyInput, EnterprisePolicyResolver, EnterprisePolicyRuleState,
    };

    let Some(tenant_context) = ctx.tenant_context.clone() else {
        return Ok(None);
    };
    state.ensure_enterprise_policy_rules_loaded().await?;
    let relevant_rules = state
        .enterprise
        .policy_rules
        .read()
        .await
        .values()
        .filter(|rule| {
            rule.state == EnterprisePolicyRuleState::Published
                && rule.tenant_context.as_ref().is_none_or(|tenant| {
                    enterprise_scope_ids_match(&tenant.org_id, &tenant_context.org_id)
                        && enterprise_scope_ids_match(
                            &tenant.workspace_id,
                            &tenant_context.workspace_id,
                        )
                        && match (
                            tenant.deployment_id.as_deref(),
                            tenant_context.deployment_id.as_deref(),
                        ) {
                            (Some(left), Some(right)) => enterprise_scope_ids_match(left, right),
                            (None, None) => true,
                            _ => false,
                        }
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if relevant_rules.is_empty() {
        return Ok(None);
    }

    let mut input = EnterprisePolicyInput::new(tenant_context.clone())
        .with_tool(tool)
        .with_permission(AccessPermission::Execute)
        .with_arguments(ctx.args.clone());
    if let Some(org_unit_id) = ctx
        .verified_tenant_context
        .as_ref()
        .and_then(|verified| verified.org_units.first())
    {
        input = input.with_org_unit_id(org_unit_id.clone());
    }
    if let Some((run_id, node_id)) = state
        .automation_v2_session_run_and_node(&ctx.session_id)
        .await
    {
        input = input.with_workflow_id(run_id);
        if let Some(node_id) = node_id {
            input = input.with_workflow_phase(node_id);
        }
    }

    let data_classes = registered_tool_security_descriptor(state, tool)
        .await
        .unwrap_or_else(|| tandem_core::tool_name_security_descriptor(tool))
        .data_classes;
    let resolved_at_ms = crate::now_ms();
    let resolver = EnterprisePolicyResolver::new(relevant_rules.clone());
    let (snapshot, resolved_input) = if data_classes.is_empty() {
        (resolver.resolve(&input, resolved_at_ms), input.clone())
    } else {
        data_classes
            .iter()
            .copied()
            .map(|data_class| {
                let resolved_input = input.clone().with_data_class(data_class);
                (
                    resolver.resolve(&resolved_input, resolved_at_ms),
                    resolved_input,
                )
            })
            .max_by_key(|(snapshot, _)| {
                let effect_priority = match snapshot.effect {
                    EnterprisePolicyEffect::Allow => 0,
                    EnterprisePolicyEffect::ApprovalRequired => 1,
                    EnterprisePolicyEffect::Deny => 2,
                };
                let source_priority = snapshot
                    .decision_source
                    .as_ref()
                    .map(|source| {
                        (
                            source.scope_level.inheritance_rank(),
                            source.version,
                            source.rule_id.clone(),
                        )
                    })
                    .unwrap_or_default();
                (effect_priority, source_priority)
            })
            .expect("tool data classes are non-empty")
    };
    let decision_id = format!("policy_decision_{}", Uuid::new_v4().simple());
    let evidence_rule = predicate_evidence_rule(
        &relevant_rules,
        &snapshot,
        &resolved_input,
        resolved_at_ms,
    );
    let mut predicate_evidence = match evidence_rule {
        Some(rule) => predicate_decision_evidence(
            rule,
            &ctx.args,
            &tenant_context,
            &snapshot.policy_version_id,
            &decision_id,
            None,
            None,
        )?,
        None => None,
    };
    let mut effect = PolicyDecisionEffect::from_enterprise_effect(snapshot.effect);
    let mut reason = snapshot.reason.clone();
    let mut reason_code = snapshot.reason_code.clone();
    let policy_id = snapshot
        .decision_source
        .as_ref()
        .map(|source| source.policy_id.clone())
        .unwrap_or_else(|| "enterprise_policy_resolver".to_string());
    let mut approval_receipt_id = None;
    let mut approval_state = None;
    let mut approval_error = None;
    let mut action_hash = None;
    let mut approval_consumed = false;
    let mut approval_run_id = None;
    let mut approval_automation_id = None;
    let mut approval_node_id = None;
    if snapshot.effect == EnterprisePolicyEffect::ApprovalRequired {
        let connector_generations =
            governed_connector_generation_bindings(state, tool, &tenant_context).await;
        let hash = governed_exact_action_binding(
            tool,
            &ctx.args,
            &tenant_context,
            &connector_generations,
        )?;
        action_hash = Some(hash.clone());
        if state.premium_governance_enabled() {
            let links = action_gate_runtime_links(state, &ctx.session_id).await;
            approval_run_id.clone_from(&links.run_id);
            approval_automation_id.clone_from(&links.automation_id);
            approval_node_id.clone_from(&links.node_id);
            let request = state
                .request_approval(
                    crate::automation_v2::governance::GovernanceApprovalRequestType::ElevatedCapability,
                    crate::automation_v2::governance::GovernanceActorRef::agent(
                        tenant_context.actor_id.clone(),
                        "enterprise_authored_policy",
                    ),
                    crate::automation_v2::governance::GovernanceResourceRef {
                        resource_type: "protected_action".to_string(),
                        id: hash.clone(),
                    },
                    format!("Review policy-gated tool action `{tool}`"),
                    json!({
                        "action_gate": {
                            "action_hash": hash,
                            "session_id": ctx.session_id,
                            "message_id": ctx.message_id,
                            "run_id": approval_run_id,
                            "automation_id": approval_automation_id,
                            "node_id": approval_node_id,
                            "tool": tool,
                            "policy_id": policy_id,
                            "policy_version_id": snapshot.policy_version_id,
                            "policy_decision_id": decision_id,
                            "approval_class": snapshot.approval_id,
                            "connector_generations": connector_generations,
                            "source": "enterprise_authored_policy",
                        }
                    }),
                    Some(crate::now_ms().saturating_add(tandem_types::DEFAULT_APPROVAL_TTL_MS)),
                    &tenant_context,
                )
                .await;
            match request {
                Ok(request) => {
                    approval_receipt_id = Some(request.approval_id.clone());
                    match state
                        .consume_action_gate_approval(&request.approval_id, &tenant_context)
                        .await
                    {
                        Ok(
                            crate::app::state::governance_action_gate::ActionGateApprovalState::ApprovedAndConsumed,
                        ) => {
                            approval_state = Some("approved_and_consumed");
                            approval_consumed = true;
                            effect = PolicyDecisionEffect::Allow;
                            reason_code = "matching_approval_receipt".to_string();
                            reason = "authored policy action approved by matching receipt".to_string();
                        }
                        Ok(
                            crate::app::state::governance_action_gate::ActionGateApprovalState::Pending,
                        ) => approval_state = Some("pending"),
                        Ok(
                            crate::app::state::governance_action_gate::ActionGateApprovalState::Denied,
                        ) => approval_state = Some("denied_or_expired"),
                        Err(error) => approval_error = Some(error.to_string()),
                    }
                }
                Err(error) => approval_error = Some(error.to_string()),
            }
        } else {
            approval_error =
                Some("premium governance approval requests are unavailable".to_string());
        }
    }
    if let Some(evidence) = predicate_evidence.as_mut() {
        evidence.approval_request_id.clone_from(&approval_receipt_id);
        evidence.action_binding.clone_from(&action_hash);
    }
    let record = PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: tenant_context.clone(),
        requester_context: ctx
            .verified_tenant_context
            .as_ref()
            .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        actor_id: tenant_context.actor_id.clone(),
        session_id: Some(ctx.session_id.clone()),
        message_id: Some(ctx.message_id.clone()),
        run_id: approval_run_id,
        automation_id: approval_automation_id,
        node_id: approval_node_id,
        tool: Some(tool.to_string()),
        resource: None,
        data_classes: data_classes.clone(),
        risk_tier: Some("enterprise_authored_policy".to_string()),
        decision: effect,
        reason_code: reason_code.clone(),
        reason: reason.clone(),
        policy_id: Some(policy_id.clone()),
        grant_id: None,
        approval_id: approval_receipt_id
            .clone()
            .or_else(|| snapshot.approval_id.clone()),
        audit_event_id: None,
        created_at_ms: resolved_at_ms,
        metadata: json!({
            "authoring_source": "enterprise_control_panel",
            "predicate_evidence": predicate_evidence.clone(),
            "evaluated_data_classes": data_classes,
            "approval_class": snapshot.approval_id,
            "approval_state": approval_state,
            "approval_error": approval_error,
            "action_hash": action_hash,
        }),
    }
    .with_effective_policy_snapshot(snapshot.clone());
    state.record_policy_decision(record).await?;
    crate::audit::append_protected_audit_event(
        state,
        "enterprise.policy.enforced",
        &tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "decision_id": decision_id.clone(),
            "tool": tool,
            "effect": effect,
            "policy_effect": snapshot.effect,
            "reason_code": reason_code,
            "policy_version_id": snapshot.policy_version_id,
            "approval_id": approval_receipt_id,
            "predicate_evidence": predicate_evidence,
        }),
    )
    .await?;

    Ok(match snapshot.effect {
        EnterprisePolicyEffect::Allow => Some(ToolPolicyDecision {
            allowed: true,
            reason: None,
            policy_decision_id: Some(decision_id.clone()),
            dispatch_decision: Some(tandem_tools::ToolDispatchDecision::allow_with_id(
                decision_id,
            )),
        }),
        EnterprisePolicyEffect::Deny => Some(ToolPolicyDecision {
            allowed: false,
            reason: Some(reason),
            policy_decision_id: Some(decision_id),
            dispatch_decision: None,
        }),
        EnterprisePolicyEffect::ApprovalRequired if approval_consumed => {
            Some(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: Some(decision_id.clone()),
                dispatch_decision: Some(tandem_tools::ToolDispatchDecision::allow_with_id(
                    decision_id,
                )),
            })
        }
        EnterprisePolicyEffect::ApprovalRequired => {
            let receipt = approval_receipt_id
                .as_deref()
                .map(|id| format!("; approval `{id}`"))
                .unwrap_or_default();
            let error = approval_error
                .as_deref()
                .map(|message| format!("; approval error: {message}"))
                .unwrap_or_default();
            let approval_reason =
                format!("{reason}; approval receipt required before execution{receipt}{error}");
            let source = snapshot.decision_source.as_ref().ok_or_else(|| {
                anyhow::anyhow!("approval-required policy decision has no winning rule")
            })?;
            let approval_class = snapshot.approval_id.clone().ok_or_else(|| {
                anyhow::anyhow!("approval-required policy decision has no approval class")
            })?;
            let action_binding = action_hash.clone().ok_or_else(|| {
                anyhow::anyhow!("approval-required policy decision has no action binding")
            })?;
            let dispatch_decision = tandem_tools::ToolDispatchDecision::approval_required(
                decision_id.clone(),
                approval_reason.clone(),
                tandem_tools::ToolDispatchApprovalRequirement {
                    approval_request_id: approval_receipt_id,
                    policy_id: source.policy_id.clone(),
                    policy_version_id: snapshot.policy_version_id.clone(),
                    rule_id: source.rule_id.clone(),
                    rule_version: source.version,
                    approval_class,
                    action_binding,
                },
            );
            Some(ToolPolicyDecision {
                allowed: false,
                reason: Some(approval_reason),
                policy_decision_id: Some(decision_id),
                dispatch_decision: Some(dispatch_decision),
            })
        }
    })
}

#[cfg(test)]
mod enterprise_authored_policy_tests {
    use super::*;
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyInput, EnterprisePolicyRule,
        EnterprisePolicyScopeLevel, PermissionPredicate, PredicateCondition, PredicateExpression,
        PredicateOperator, PredicateValueType,
    };
    use tandem_types::DataClass;

    #[tokio::test]
    async fn authored_tool_policy_normalizes_tenant_ids_before_prefiltering() {
        let state = crate::test_support::test_state().await;
        let stored_tenant = TenantContext::explicit_user_workspace(
            " Org-A ",
            " WORKSPACE-A ",
            Some(" Deployment-A ".to_string()),
            "admin-a",
        );
        let runtime_tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "admin-a",
        );
        let deny = EnterprisePolicyRule::new(
            "normalized-tenant-deny",
            "normalized-tenant-policy",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(stored_tenant)
        .with_tool_patterns(vec!["mcp.files.delete".to_string()]);
        state
            .enterprise
            .policy_rules
            .write()
            .await
            .insert(deny.rule_id.clone(), deny);

        let decision = evaluate_enterprise_authored_tool_policy(
            &state,
            &ToolPolicyContext {
                session_id: "session-normalized-tenant".to_string(),
                message_id: "message-normalized-tenant".to_string(),
                tenant_context: Some(runtime_tenant),
                verified_tenant_context: None,
                tool: "mcp.files.delete".to_string(),
                args: json!({"path":"/sensitive"}),
            },
            "mcp.files.delete",
        )
        .await
        .expect("normalized tenant evaluation")
        .expect("matching authored deny");
        assert!(!decision.allowed);
    }

    #[tokio::test]
    async fn authored_parameter_policy_matches_preview_and_records_runtime_decisions() {
        let state = crate::test_support::test_state().await;
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let predicate = PermissionPredicate {
            expression_version: "permission_predicates/v1".to_string(),
            expression: PredicateExpression::Condition {
                condition: PredicateCondition {
                    condition_id: Some("threshold".to_string()),
                    selector: "/amount".to_string(),
                    value_type: PredicateValueType::Decimal,
                    operator: PredicateOperator::LessThan,
                    operand: json!("10000"),
                },
            },
        };
        let rule = EnterprisePolicyRule::new(
            "small-payment",
            "finance-authored",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Allow,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["mcp.payments.create_payment".to_string()])
        .with_predicate(predicate);
        state
            .enterprise
            .policy_rules
            .write()
            .await
            .insert(rule.rule_id.clone(), rule);

        let context = |amount: &str| ToolPolicyContext {
            session_id: "session-authored-policy".to_string(),
            message_id: format!("message-{amount}"),
            tenant_context: Some(tenant.clone()),
            verified_tenant_context: None,
            tool: "mcp.payments.create_payment".to_string(),
            args: json!({"amount": amount}),
        };
        let allowed = evaluate_enterprise_authored_tool_policy(
            &state,
            &context("9999"),
            "mcp.payments.create_payment",
        )
        .await
        .expect("allow evaluation");
        let allowed = allowed.expect("matching authored allow decision");
        assert!(allowed.allowed);
        assert!(allowed.dispatch_decision.is_some());

        let denied = evaluate_enterprise_authored_tool_policy(
            &state,
            &context("15000"),
            "mcp.payments.create_payment",
        )
        .await
        .expect("deny evaluation")
        .expect("default deny decision");
        assert!(!denied.allowed);
        let recorded = state
            .get_policy_decision(denied.policy_decision_id.as_deref().expect("decision id"))
            .await
            .expect("recorded decision");
        assert_eq!(recorded.decision, PolicyDecisionEffect::Deny);
        let evidence = recorded
            .metadata
            .pointer("/predicate_evidence")
            .expect("structured predicate evidence");
        assert_eq!(evidence["expression_version"], "permission_predicates/v1");
        assert_eq!(evidence["result"], "no_match");
        assert_eq!(evidence["conditions"][0]["condition_id"], "threshold");
        assert_eq!(evidence["conditions"][0]["result"], "no_match");
        assert!(evidence["expression_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("hmac-sha256:")));
        assert!(evidence["conditions"][0]["selector_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("hmac-sha256:")));
        assert!(evidence["conditions"][0]["value_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("hmac-sha256:")));
        let metadata = recorded.metadata.to_string();
        assert!(!metadata.contains("15000"));
        assert!(!metadata.contains("10000"));
        assert!(!metadata.contains("/amount"));
        assert!(recorded.metadata.get("tool_arguments").is_none());

        let approval_rule = EnterprisePolicyRule::new(
            "large-refund",
            "finance-authored",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::ApprovalRequired,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["mcp.payments.refund".to_string()])
        .with_predicate(PermissionPredicate {
            expression_version: "permission_predicates/v1".to_string(),
            expression: PredicateExpression::Condition {
                condition: PredicateCondition {
                    condition_id: Some("refund-threshold".to_string()),
                    selector: "/amount".to_string(),
                    value_type: PredicateValueType::Decimal,
                    operator: PredicateOperator::GreaterThanOrEqual,
                    operand: json!("10000"),
                },
            },
        })
        .with_approval_id("finance-large-refund");
        state
            .enterprise
            .policy_rules
            .write()
            .await
            .insert(approval_rule.rule_id.clone(), approval_rule);
        let approval_context = ToolPolicyContext {
            session_id: "session-authored-policy".to_string(),
            message_id: "message-refund".to_string(),
            tenant_context: Some(tenant.clone()),
            verified_tenant_context: None,
            tool: "mcp.payments.refund".to_string(),
            args: json!({"amount": "15000"}),
        };
        let approval = evaluate_enterprise_authored_tool_policy(
            &state,
            &approval_context,
            "mcp.payments.refund",
        )
        .await
        .expect("approval evaluation")
        .expect("approval gate decision");
        assert!(!approval.allowed);
        let pending = approval
            .dispatch_decision
            .as_ref()
            .expect("first-class approval-required outcome");
        assert_eq!(
            pending.outcome,
            tandem_tools::ToolDispatchPolicyOutcome::ApprovalRequired
        );
        let requirement = pending
            .approval_requirement
            .as_ref()
            .expect("approval metadata");
        assert_eq!(requirement.policy_id, "finance-authored");
        assert_eq!(requirement.rule_id, "large-refund");
        assert_eq!(requirement.approval_class, "finance-large-refund");
        assert!(requirement.action_binding.starts_with("hmac-sha256:"));
        let recorded = state
            .get_policy_decision(approval.policy_decision_id.as_deref().expect("decision id"))
            .await
            .expect("approval decision record");
        assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
        if state.premium_governance_enabled() {
            assert!(recorded
                .approval_id
                .as_deref()
                .is_some_and(|id| id != "finance-large-refund"));
        } else {
            assert_eq!(
                recorded.approval_id.as_deref(),
                Some("finance-large-refund")
            );
        }
        let preview = state
            .resolve_enterprise_policy_input(
                &EnterprisePolicyInput::new(tenant)
                    .with_tool("mcp.payments.refund")
                    .with_arguments(json!({"amount": "15000"})),
                crate::now_ms(),
            )
            .await
            .expect("policy preview");
        assert_eq!(preview.effect, EnterprisePolicyEffect::ApprovalRequired);
        assert_eq!(preview.approval_id.as_deref(), Some("finance-large-refund"));
    }

    #[cfg(feature = "premium-governance")]
    #[tokio::test]
    async fn authored_approval_resumes_exactly_one_execution() {
        let state = crate::test_support::test_state().await;
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "agent-a");
        let rule = EnterprisePolicyRule::new(
            "approved-send",
            "communications-authored",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::ApprovalRequired,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["mcp.email.send".to_string()])
        .with_approval_id("external-communications");
        state
            .enterprise
            .policy_rules
            .write()
            .await
            .insert(rule.rule_id.clone(), rule);
        let context = ToolPolicyContext {
            session_id: "session-authored-approval".to_string(),
            message_id: "message-authored-approval".to_string(),
            tenant_context: Some(tenant.clone()),
            verified_tenant_context: None,
            tool: "mcp.email.send".to_string(),
            args: json!({"to":"customer@example.com","subject":"Hello"}),
        };

        let pending = evaluate_enterprise_authored_tool_policy(&state, &context, &context.tool)
            .await
            .expect("pending evaluation")
            .expect("approval gate decision");
        assert!(!pending.allowed);
        let approvals = state
            .list_approval_requests_for_tenant(
                Some(
                    crate::automation_v2::governance::GovernanceApprovalRequestType::ElevatedCapability,
                ),
                None,
                &tenant,
            )
            .await;
        assert_eq!(approvals.len(), 1);
        let approval = &approvals[0];
        let dispatch = pending
            .dispatch_decision
            .as_ref()
            .expect("first-class pending dispatch decision");
        assert_eq!(
            dispatch.outcome,
            tandem_tools::ToolDispatchPolicyOutcome::ApprovalRequired
        );
        let requirement = dispatch
            .approval_requirement
            .as_ref()
            .expect("approval requirement");
        assert_eq!(
            requirement.approval_request_id.as_deref(),
            Some(approval.approval_id.as_str())
        );
        assert_eq!(requirement.rule_id, "approved-send");
        assert_eq!(requirement.rule_version, 1);
        assert_eq!(requirement.approval_class, "external-communications");
        assert!(requirement.action_binding.starts_with("hmac-sha256:"));
        assert_ne!(
            requirement.action_binding,
            fintech_protected_action_hash(&context.tool, &context.args)
        );
        assert_eq!(
            approval
                .context
                .pointer("/action_gate/source")
                .and_then(Value::as_str),
            Some("enterprise_authored_policy")
        );
        assert_eq!(
            approval
                .context
                .pointer("/action_gate/approval_class")
                .and_then(Value::as_str),
            Some("external-communications")
        );
        state
            .decide_approval_request(
                &approval.approval_id,
                crate::automation_v2::governance::GovernanceActorRef::human(
                    Some("reviewer-a".to_string()),
                    "test",
                ),
                true,
                Some("approved in test".to_string()),
                &tenant,
            )
            .await
            .expect("approve request")
            .expect("approval exists");

        let first_retry = evaluate_enterprise_authored_tool_policy(&state, &context, &context.tool)
            .await
            .expect("approved retry");
        let second_retry =
            evaluate_enterprise_authored_tool_policy(&state, &context, &context.tool)
                .await
                .expect("consumed retry");
        assert!(
            first_retry.is_some_and(|decision| {
                decision.allowed && decision.dispatch_decision.is_some()
            }),
            "approved retry must preserve its allow receipt while lower guards continue"
        );
        assert!(
            second_retry.is_some_and(|decision| !decision.allowed),
            "the same receipt must not authorize a second execution"
        );

        let decisions = state.list_policy_decisions(&tenant, 20).await;
        assert!(decisions.iter().any(|decision| {
            decision.decision == PolicyDecisionEffect::Allow
                && decision.reason_code == "matching_approval_receipt"
                && decision.approval_id.as_deref() == Some(approval.approval_id.as_str())
        }));
    }

    #[tokio::test]
    async fn authored_tool_policy_evaluates_registered_tool_data_classes() {
        let state = crate::test_support::test_state().await;
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let broad_allow = EnterprisePolicyRule::new(
            "bash-broad-allow",
            "coding-authored",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Allow,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["bash".to_string()]);
        let source_code_deny = EnterprisePolicyRule::new(
            "bash-source-code-deny",
            "coding-authored",
            EnterprisePolicyScopeLevel::Tenant,
            EnterprisePolicyEffect::Deny,
        )
        .with_tenant_context(tenant.clone())
        .with_tool_patterns(vec!["bash".to_string()])
        .with_data_classes(vec![DataClass::SourceCode]);
        {
            let mut rules = state.enterprise.policy_rules.write().await;
            rules.insert(broad_allow.rule_id.clone(), broad_allow);
            rules.insert(source_code_deny.rule_id.clone(), source_code_deny);
        }

        let decision = evaluate_enterprise_authored_tool_policy(
            &state,
            &ToolPolicyContext {
                session_id: "session-data-class-policy".to_string(),
                message_id: "message-data-class-policy".to_string(),
                tenant_context: Some(tenant),
                verified_tenant_context: None,
                tool: "bash".to_string(),
                args: json!({"command":"git status"}),
            },
            "bash",
        )
        .await
        .expect("data-class policy evaluation")
        .expect("source-code deny decision");
        assert!(!decision.allowed);
        let recorded = state
            .get_policy_decision(decision.policy_decision_id.as_deref().expect("decision id"))
            .await
            .expect("recorded data-class decision");
        assert!(recorded.data_classes.contains(&DataClass::Internal));
        assert!(recorded.data_classes.contains(&DataClass::SourceCode));
        assert_eq!(
            recorded
                .effective_policy_snapshot()
                .as_ref()
                .and_then(|snapshot| snapshot.decision_source.as_ref())
                .map(|source| source.rule_id.as_str()),
            Some("bash-source-code-deny")
        );
    }
}
