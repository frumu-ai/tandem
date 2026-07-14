async fn evaluate_enterprise_authored_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> anyhow::Result<Option<ToolPolicyDecision>> {
    use tandem_enterprise_contract::{
        AccessPermission, EnterprisePolicyEffect, EnterprisePolicyInput, EnterprisePolicyResolver,
        EnterprisePolicyRuleState,
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
                    tenant.org_id == tenant_context.org_id
                        && tenant.workspace_id == tenant_context.workspace_id
                        && tenant.deployment_id == tenant_context.deployment_id
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
    let resolver = EnterprisePolicyResolver::new(relevant_rules);
    let snapshot = if data_classes.is_empty() {
        resolver.resolve(&input, resolved_at_ms)
    } else {
        data_classes
            .iter()
            .copied()
            .map(|data_class| {
                resolver.resolve(&input.clone().with_data_class(data_class), resolved_at_ms)
            })
            .max_by_key(|snapshot| {
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
    let effect = PolicyDecisionEffect::from_enterprise_effect(snapshot.effect);
    let reason = snapshot.reason.clone();
    let reason_code = snapshot.reason_code.clone();
    let policy_id = snapshot
        .decision_source
        .as_ref()
        .map(|source| source.policy_id.clone())
        .unwrap_or_else(|| "enterprise_policy_resolver".to_string());
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
        run_id: None,
        automation_id: None,
        node_id: None,
        tool: Some(tool.to_string()),
        resource: None,
        data_classes: data_classes.clone(),
        risk_tier: Some("enterprise_authored_policy".to_string()),
        decision: effect,
        reason_code: reason_code.clone(),
        reason: reason.clone(),
        policy_id: Some(policy_id),
        grant_id: None,
        approval_id: snapshot.approval_id.clone(),
        audit_event_id: None,
        created_at_ms: resolved_at_ms,
        metadata: json!({
            "authoring_source": "enterprise_control_panel",
            "predicate_evidence": "redacted",
            "evaluated_data_classes": data_classes,
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
            "effect": snapshot.effect,
            "reason_code": reason_code,
            "policy_version_id": snapshot.policy_version_id,
        }),
    )
    .await?;

    Ok(match snapshot.effect {
        EnterprisePolicyEffect::Allow => None,
        EnterprisePolicyEffect::Deny => Some(ToolPolicyDecision {
            allowed: false,
            reason: Some(reason),
            policy_decision_id: Some(decision_id),
        }),
        EnterprisePolicyEffect::ApprovalRequired => Some(ToolPolicyDecision {
            allowed: false,
            reason: Some(format!(
                "{reason}; approval receipt required before execution"
            )),
            policy_decision_id: Some(decision_id),
        }),
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
        assert!(
            allowed.is_none(),
            "matching authored allow continues to lower guards"
        );

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
        assert_eq!(
            recorded
                .metadata
                .pointer("/predicate_evidence")
                .and_then(Value::as_str),
            Some("redacted")
        );
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
        let recorded = state
            .get_policy_decision(approval.policy_decision_id.as_deref().expect("decision id"))
            .await
            .expect("approval decision record");
        assert_eq!(recorded.decision, PolicyDecisionEffect::ApprovalRequired);
        assert_eq!(
            recorded.approval_id.as_deref(),
            Some("finance-large-refund")
        );
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
