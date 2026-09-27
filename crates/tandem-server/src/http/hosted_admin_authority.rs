// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use tandem_types::{
    AccessDecision, AccessPermission, DataClass, PrincipalRef, ResourceKind, ResourceRef,
    VerifiedTenantContext,
};

#[cfg(test)]
#[path = "hosted_admin_authority_tests.rs"]
mod tests;

// Hosted operation authority must not be inferred from Admin or Delegate on
// an unrelated data resource. The ingress projection owns deployment grants.
pub(super) fn allowed(verified: &VerifiedTenantContext) -> bool {
    let now = crate::now_ms();
    let Some(deployment_id) = verified.tenant_context.deployment_id.as_deref() else {
        return false;
    };
    let Some(strict) = verified.strict_projection.as_ref() else {
        return false;
    };
    if verified.is_expired_at(now)
        || strict.tenant_context != verified.tenant_context
        || strict.principal != PrincipalRef::human_user(&verified.human_actor.actor_id)
    {
        return false;
    }
    let resource = ResourceRef::new(
        &verified.tenant_context.org_id,
        &verified.tenant_context.workspace_id,
        ResourceKind::HostedDeployment,
        deployment_id,
    );
    strict
        .evaluate_access(
            &resource,
            AccessPermission::HostedAdmin,
            DataClass::Internal,
            now,
        )
        .decision
        == AccessDecision::Allow
}
