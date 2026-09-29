// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Included by part04.rs so the gate's authorization and state transition share
// the run write lock without growing the handler file further.

fn pending_gate_for_run(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    automation: &AutomationV2Spec,
) -> Option<crate::AutomationPendingGate> {
    if run.status != AutomationRunStatus::AwaitingApproval {
        return None;
    }
    run.checkpoint.awaiting_gate.clone().or_else(|| {
        let pending_nodes = run
            .checkpoint
            .pending_nodes
            .iter()
            .collect::<std::collections::HashSet<_>>();
        automation
            .flow
            .nodes
            .iter()
            .find(|node| {
                pending_nodes.contains(&node.node_id)
                    && !crate::app::state::automation_gate_has_settled_decision(run, &node.node_id)
                    && crate::app::state::is_automation_approval_node(node)
            })
            .and_then(crate::app::state::build_automation_pending_gate)
            .map(|mut gate| {
                gate.requested_at_ms = run.updated_at_ms.max(run.created_at_ms);
                gate
            })
    })
}

pub(crate) enum GateCommitDenial {
    Authority(&'static str, &'static str),
    Changed,
    Expired,
    TransitionGuard(crate::app::state::AutomationGateTransitionGuardDenial),
}

impl GateCommitDenial {
    fn response(&self) -> (StatusCode, &'static str, &str) {
        match self {
            Self::Authority(code, detail) => (StatusCode::FORBIDDEN, code, detail),
            Self::Changed => (
                StatusCode::CONFLICT,
                "AUTOMATION_V2_GATE_CHANGED",
                "Approval gate changed before this decision was committed",
            ),
            Self::Expired => (
                StatusCode::CONFLICT,
                "AUTOMATION_V2_GATE_EXPIRED",
                "Approval gate expired before this decision was committed",
            ),
            Self::TransitionGuard(denial) => (StatusCode::CONFLICT, denial.code, &denial.detail),
        }
    }
}

pub(crate) fn apply_gate_decision_with_current_authority(
    state: &AppState,
    run: &mut crate::automation_v2::types::AutomationV2RunRecord,
    submitted_automation: &AutomationV2Spec,
    automation: &AutomationV2Spec,
    submitted_gate: &crate::AutomationPendingGate,
    live_gate: &crate::AutomationPendingGate,
    decision: &str,
    reason: Option<String>,
    decider: &crate::automation_v2::governance::GovernanceActorRef,
    request_tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    requested_approval_request_id: Option<&str>,
    requested_transition_id: Option<&str>,
) -> Result<crate::app::state::AutomationGateDecisionOutcome, GateCommitDenial> {
    // A reissued same-node gate is a different approval. Reminder metadata may
    // change within an epoch, but reviewer scope and expiry policy may not be
    // silently substituted underneath a submitted decision.
    if !super::tenant_matches(request_tenant, &run.tenant_context)
        || !super::tenant_matches(&run.tenant_context, &automation.tenant_context())
        || run.automation_id != automation.automation_id
        || submitted_gate.node_id != live_gate.node_id
        || submitted_gate.requested_at_ms != live_gate.requested_at_ms
        || GateReviewerPolicy::from_gate(submitted_gate, submitted_automation)
            != GateReviewerPolicy::from_gate(live_gate, automation)
        || submitted_gate.expiry_policy != live_gate.expiry_policy
    {
        return Err(GateCommitDenial::Changed);
    }

    // The run lock has already been acquired by update_automation_v2_run. Keep
    // policy publication and revocable grant writers outside the short,
    // synchronous authorize-and-apply section so revocation and approval have
    // an unambiguous order.
    let requires_reviewer_authority =
        GateReviewerPolicy::from_gate(live_gate, automation).requires_reviewer_authority();
    state
        .enterprise
        .hosted_policy
        .with_current_policy(|policy| {
            let mut current = verified.cloned();
            let mut local_membership_guard = None;
            let mut access_grant_guard = None;
            let mut cross_tenant_grant_guard = None;
            let mut hosted_memberships = None;

            if let Some(current_verified) = current.as_mut() {
                let projection_time = crate::now_ms();
                if let Some(policy) = policy {
                    hosted_memberships = Some(
                        policy
                            .memberships_for_identity(current_verified, projection_time)
                            .map_err(|_| {
                                GateCommitDenial::Authority(
                                    "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED",
                                    "Reviewer authority changed before this approval",
                                )
                            })?,
                    );
                    current_verified.strict_projection = Some(
                        policy
                            .project_identity(current_verified, projection_time)
                            .map_err(|_| {
                                GateCommitDenial::Authority(
                                    "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED",
                                    "Reviewer authority changed before this approval",
                                )
                            })?,
                    );
                } else if let Some(strict) = current_verified.strict_projection.as_mut() {
                    // Middleware-enriched grants are revocable; preserve only
                    // signed direct grants before rebuilding the live sources.
                    strict.grants.retain(|grant| {
                        !matches!(
                            grant.grant_source,
                            tandem_types::GrantSource::OrganizationUnitMembership
                                | tandem_types::GrantSource::CrossTenantGrant
                        )
                    });
                }

                if requires_reviewer_authority && current_verified.strict_projection.is_some() {
                    if hosted_memberships.is_none() {
                        local_membership_guard =
                            Some(state.enterprise.org_unit_memberships.try_read().map_err(
                                |_| {
                                    GateCommitDenial::Authority(
                                    "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
                                    "Reviewer authority could not be verified for this approval",
                                )
                                },
                            )?);
                    }
                    access_grant_guard = Some(
                        state
                            .enterprise
                            .org_unit_access_grants
                            .try_read()
                            .map_err(|_| {
                                GateCommitDenial::Authority(
                                    "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
                                    "Reviewer authority could not be verified for this approval",
                                )
                            })?,
                    );
                    let memberships = hosted_memberships.clone().unwrap_or_else(|| {
                        local_membership_guard
                            .as_ref()
                            .map(|guard| guard.values().cloned().collect())
                            .unwrap_or_default()
                    });
                    let hosted = hosted_memberships.is_some();
                    super::middleware::project_org_unit_grants_into_verified_context(
                        current_verified,
                        memberships.iter(),
                        access_grant_guard
                            .as_ref()
                            .expect("grant guard")
                            .values()
                            .filter(|grant| {
                                !hosted || super::middleware::local_hosted_data_grant(grant)
                            }),
                        projection_time,
                    );
                    cross_tenant_grant_guard = Some(
                        state
                            .enterprise
                            .cross_tenant_grants
                            .try_read()
                            .map_err(|_| {
                                GateCommitDenial::Authority(
                                    "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
                                    "Reviewer authority could not be verified for this approval",
                                )
                            })?,
                    );
                    super::cross_tenant_grants::project_inbound_cross_tenant_grants(
                        current_verified,
                        cross_tenant_grant_guard
                            .as_ref()
                            .expect("cross-tenant guard")
                            .values(),
                        projection_time,
                    );
                }
            }

            let commit_time = crate::now_ms();
            if crate::app::state::automation_gate_rejects_late_human_decision(
                live_gate,
                commit_time,
            ) {
                return Err(GateCommitDenial::Expired);
            }
            if let Some(current_verified) = current.as_ref() {
                if current_verified.is_expired_at(commit_time) {
                    return Err(GateCommitDenial::Authority(
                        "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED",
                        "Reviewer authority changed before this approval",
                    ));
                }
                if let Some(policy) = policy {
                    let strict = current_verified.strict_projection.as_ref().ok_or(
                        GateCommitDenial::Authority(
                            "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
                            "Reviewer authority could not be verified for this approval",
                        ),
                    )?;
                    let deployment = policy.deployment_resource();
                    for permission in [
                        tandem_types::AccessPermission::HostedUse,
                        tandem_types::AccessPermission::HostedAutomationExecute,
                    ] {
                        if strict
                            .evaluate_access(
                                &deployment,
                                permission,
                                tandem_types::DataClass::Internal,
                                commit_time,
                            )
                            .decision
                            != tandem_types::AccessDecision::Allow
                        {
                            return Err(GateCommitDenial::Authority(
                                "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_DENIED",
                                "Reviewer authority changed before this approval",
                            ));
                        }
                    }
                }
            }
            if !requires_reviewer_authority
                && ensure_automation_v2_owner_or_admin(automation, current.as_ref()).is_err()
            {
                return Err(GateCommitDenial::Authority(
                    "AUTOMATION_V2_ACCESS_DENIED",
                    "Automation access denied",
                ));
            }
            authorize_gate_decider(
                run,
                automation,
                live_gate,
                decision,
                decider,
                current.as_ref(),
                commit_time,
            )
            .map_err(|(code, detail)| GateCommitDenial::Authority(code, detail))?;
            crate::app::state::apply_automation_gate_decision_with_transition_guard(
                run,
                automation,
                live_gate,
                decision,
                reason,
                Some(decider.clone()),
                requested_approval_request_id,
                requested_transition_id,
            )
            .map_err(GateCommitDenial::TransitionGuard)
        })
        .map_err(|_| {
            GateCommitDenial::Authority(
                "AUTOMATION_V2_GATE_REVIEWER_AUTHORITY_REQUIRED",
                "Reviewer authority could not be verified for this approval",
            )
        })?
}
