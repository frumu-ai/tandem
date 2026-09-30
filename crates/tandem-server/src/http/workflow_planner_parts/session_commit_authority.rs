// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// The caller holds the planner authority and session write locks. Keep the
// current policy, automation, and revocable grant reads alive through `commit`
// so a queued PATCH cannot commit after its write authority is withdrawn.
pub(crate) fn with_current_planner_session_write_authority<R>(
    state: &AppState,
    binding: &WorkflowPlanDraftAccessBinding,
    tenant: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    policy: Option<&tandem_enterprise_contract::hosted_policy::ValidatedHostedPolicy>,
    commit: impl FnOnce() -> R,
) -> Result<R, ()> {
    use tandem_types::{AccessDecision, AccessPermission, DataClass, GrantSource};

    let verified = verified.ok_or(())?;
    let now = crate::now_ms();
    let actor_id = verified.human_actor.actor_id.trim();
    if actor_id.is_empty()
        || verified.is_expired_at(now)
        || !super::tenant_matches(tenant, &verified.tenant_context)
        || tenant.actor_id.as_deref() != Some(actor_id)
    {
        return Err(());
    }

    // Do not call HostedPolicyRuntime::project/authorize_permission here: the
    // caller already holds its snapshot read lock through with_current_policy.
    let mut current = verified.clone();
    let hosted_memberships = if let Some(policy) = policy {
        let memberships = policy
            .memberships_for_identity(&current, now)
            .map_err(|_| ())?;
        current.strict_projection = Some(policy.project_identity(&current, now).map_err(|_| ())?);
        let projection = current.strict_projection.as_ref().ok_or(())?;
        if projection
            .evaluate_access(
                &policy.deployment_resource(),
                AccessPermission::HostedAutomationWrite,
                DataClass::Internal,
                now,
            )
            .decision
            != AccessDecision::Allow
        {
            return Err(());
        }
        Some(memberships)
    } else {
        None
    };

    match binding {
        WorkflowPlanDraftAccessBinding::Actor(owner) => {
            if !super::tenant_matches(tenant, owner) || owner.actor_id.as_deref() != Some(actor_id)
            {
                return Err(());
            }
            Ok(commit())
        }
        WorkflowPlanDraftAccessBinding::Workflow(source) => {
            // try_read is deliberate: awaiting an automation writer while
            // holding planner locks can invert another writer's lock order.
            let automations = state.automations_v2.try_read().map_err(|_| ())?;
            let automation = automations.get(&source.workflow_id).ok_or(())?;
            let source_tenant = automation.tenant_context();
            if automation.created_at_ms == 0
                || source.binding
                    != crate::WorkflowLearningCandidateSourceBinding::workflow(automation)
                || !super::tenant_matches(tenant, &source_tenant)
            {
                return Err(());
            }

            let current_admin = if current.policy_version.is_some() {
                policy.is_some_and(|policy| {
                    current
                        .strict_projection
                        .as_ref()
                        .is_some_and(|projection| {
                            projection
                                .evaluate_access(
                                    &policy.deployment_resource(),
                                    AccessPermission::HostedAdmin,
                                    DataClass::Internal,
                                    now,
                                )
                                .decision
                                == AccessDecision::Allow
                        })
                })
            } else {
                current.roles.iter().any(|role| {
                    matches!(
                        role.as_str(),
                        "owner"
                            | "admin"
                            | "hosted:owner"
                            | "hosted:admin"
                            | "enterprise:admin"
                            | "workspace:admin"
                            | "organization:admin"
                    )
                })
            };
            if current_admin {
                return Ok(commit());
            }

            let access = automation
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("resource_access"))
                .and_then(serde_json::Value::as_object);
            let owner = access
                .and_then(|access| access.get("owner_principal"))
                .and_then(serde_json::Value::as_object)
                .filter(|owner| {
                    owner
                        .get("kind")
                        .is_none_or(|kind| kind.as_str() == Some("human_user"))
                })
                .and_then(|owner| owner.get("id"))
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    if access.is_none() {
                        source_tenant.actor_id.as_deref()
                    } else {
                        None
                    }
                });
            if owner == Some(actor_id) {
                return Ok(commit());
            }

            let strict = current.strict_projection.as_mut().ok_or(())?;
            if hosted_memberships.is_none() {
                // Middleware-enriched grants are snapshots. Rebuild only from
                // stores whose read locks remain held through the insert.
                strict.grants.retain(|grant| {
                    !matches!(
                        grant.grant_source,
                        GrantSource::OrganizationUnitMembership | GrantSource::CrossTenantGrant
                    )
                });
            }
            let local_memberships = if hosted_memberships.is_none() {
                Some(
                    state
                        .enterprise
                        .org_unit_memberships
                        .try_read()
                        .map_err(|_| ())?,
                )
            } else {
                None
            };
            let access_grants = state
                .enterprise
                .org_unit_access_grants
                .try_read()
                .map_err(|_| ())?;
            let memberships = hosted_memberships.unwrap_or_else(|| {
                local_memberships
                    .as_ref()
                    .map(|guard| guard.values().cloned().collect())
                    .unwrap_or_default()
            });
            let hosted = policy.is_some();
            super::middleware::project_org_unit_grants_into_verified_context(
                &mut current,
                memberships.iter(),
                access_grants
                    .values()
                    .filter(|grant| !hosted || super::middleware::local_hosted_data_grant(grant)),
                now,
            );
            let cross_tenant_grants = state
                .enterprise
                .cross_tenant_grants
                .try_read()
                .map_err(|_| ())?;
            super::cross_tenant_grants::project_inbound_cross_tenant_grants(
                &mut current,
                cross_tenant_grants.values(),
                now,
            );
            let strict = current.strict_projection.as_ref().ok_or(())?;
            if strict.tenant_context != current.tenant_context
                || strict.principal != tandem_types::PrincipalRef::human_user(actor_id)
            {
                return Err(());
            }
            let resource = tandem_types::ResourceRef::new(
                &tenant.org_id,
                &tenant.workspace_id,
                tandem_types::ResourceKind::Automation,
                &automation.automation_id,
            );
            if [AccessPermission::Edit, AccessPermission::Admin]
                .iter()
                .all(|permission| {
                    strict
                        .evaluate_access(&resource, *permission, DataClass::Internal, now)
                        .decision
                        != AccessDecision::Allow
                })
            {
                return Err(());
            }
            Ok(commit())
        }
    }
}
