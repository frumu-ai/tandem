//! Translate hosted operation grants without manufacturing data or admin ACLs.
use super::{hosted_unit_principal, ValidatedHostedPolicy};
use crate::{
    AccessPermission, AssertionMetadata, DataClass, GrantSource, OrganizationUnitMembership,
    OrganizationUnitMembershipSource, PrincipalRef, ResourceKind, ResourceRef, ResourceScope,
    ScopedGrant, StrictTenantContext, VerifiedTenantContext,
};

pub(super) fn permission(value: &str) -> Option<AccessPermission> {
    Some(match value {
        "hosted.view" => AccessPermission::HostedView,
        "hosted.use" => AccessPermission::HostedUse,
        "hosted.admin" => AccessPermission::HostedAdmin,
        "automation.read" => AccessPermission::HostedAutomationRead,
        "automation.execute" => AccessPermission::HostedAutomationExecute,
        "automation.write" => AccessPermission::HostedAutomationWrite,
        "automation.share" => AccessPermission::HostedAutomationShare,
        "workflow.read" => AccessPermission::HostedWorkflowRead,
        "workflow.share" => AccessPermission::HostedWorkflowShare,
        _ => return None,
    })
}

impl ValidatedHostedPolicy {
    pub fn deployment_resource(&self) -> ResourceRef {
        ResourceRef::new(
            &self.bundle.organization_id,
            &self.bundle.deployment_id,
            ResourceKind::HostedDeployment,
            &self.bundle.deployment_id,
        )
    }

    /// Memberships come from this immutable revision, never a retained local
    /// copy. Signed unit claims may narrow this set but cannot expand it.
    pub fn memberships_for_identity(
        &self,
        verified: &VerifiedTenantContext,
        now_ms: u64,
    ) -> Result<Vec<OrganizationUnitMembership>, &'static str> {
        self.authorize_identity(verified, now_ms)?;
        let expires = self.expires_at_ms.min(verified.expires_at_ms);
        Ok(self
            .bundle
            .org_unit_memberships
            .iter()
            .filter(|row| {
                row.user_id == verified.human_actor.actor_id
                    && verified.org_units.contains(&row.unit_id)
                    && self
                        .bundle
                        .org_units
                        .iter()
                        .any(|unit| unit.id == row.unit_id && unit.state == "active")
            })
            .map(|row| {
                OrganizationUnitMembership::active(
                    format!(
                        "hosted:{}:{}:{}",
                        self.bundle.deployment_id, row.unit_id, row.user_id
                    ),
                    verified.tenant_context.clone(),
                    hosted_unit_principal(&row.unit_id),
                    PrincipalRef::human_user(&row.user_id),
                    OrganizationUnitMembershipSource::HostedControlPlane,
                    self.bundle.generated_at.timestamp_millis() as u64,
                )
                .with_expires_at_ms(expires)
            })
            .collect())
    }

    pub fn project_identity(
        &self,
        verified: &VerifiedTenantContext,
        now_ms: u64,
    ) -> Result<StrictTenantContext, &'static str> {
        let memberships = self.memberships_for_identity(verified, now_ms)?;
        let principal = PrincipalRef::human_user(&verified.human_actor.actor_id);
        let resource = self.deployment_resource();
        let expires = self.expires_at_ms.min(verified.expires_at_ms);
        // The control plane signs identity/role capabilities. It does not
        // supply arbitrary strict grants; those must come from policy stores.
        let mut grants = vec![ScopedGrant::new(
            format!("hosted:{}:role:{}", self.bundle.deployment_id, principal.id),
            principal.clone(),
            resource.clone(),
            GrantSource::Direct,
        )
        .with_permissions(
            verified
                .capabilities
                .iter()
                .filter_map(|cap| permission(cap))
                .collect(),
        )
        .with_data_classes(vec![DataClass::Internal])
        .with_expires_at_ms(expires)];
        for grant in &self.bundle.deployment_grants {
            let source = match grant.principal_kind.as_str() {
                "member" if grant.principal_id == principal.id => None,
                "org_unit"
                    if memberships
                        .iter()
                        .any(|row| row.unit == hosted_unit_principal(&grant.principal_id)) =>
                {
                    Some(hosted_unit_principal(&grant.principal_id))
                }
                _ => continue,
            };
            let mut projected = ScopedGrant::new(
                format!("hosted:{}:grant:{}", self.bundle.deployment_id, grant.id),
                principal.clone(),
                resource.clone(),
                if source.is_some() {
                    GrantSource::OrganizationUnitMembership
                } else {
                    GrantSource::Direct
                },
            )
            .with_permissions(
                grant
                    .permissions
                    .iter()
                    .filter_map(|p| permission(p))
                    .collect(),
            )
            .with_data_classes(vec![DataClass::Internal])
            .with_expires_at_ms(expires);
            projected.source_principal = source;
            grants.push(projected);
        }
        let mut assertion = AssertionMetadata::from(verified);
        assertion.expires_at_ms = expires;
        Ok(StrictTenantContext::new(
            verified.tenant_context.clone(),
            principal,
            verified.authority_chain.clone(),
            ResourceScope::root(ResourceRef::new(
                &self.bundle.organization_id,
                &self.bundle.deployment_id,
                ResourceKind::Workspace,
                &self.bundle.deployment_id,
            )),
            assertion,
        )
        .with_grants(grants))
    }
}
