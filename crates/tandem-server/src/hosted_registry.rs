//! Read-through view over local authoring and one immutable hosted revision.
use crate::AppState;
use tandem_enterprise_contract::{
    hosted_policy::{HostedPolicyRevision, HOSTED_TAXONOMY_ID},
    OrganizationUnit, OrganizationUnitAccessGrant, OrganizationUnitMembership,
    OrganizationUnitMembershipSource, PrincipalKind, TenantContext,
};

pub struct EnterpriseOrgUnitView {
    pub units: Vec<OrganizationUnit>,
    pub memberships: Vec<OrganizationUnitMembership>,
    pub access_grants: Vec<OrganizationUnitAccessGrant>,
    pub hosted_policy_revision: Option<HostedPolicyRevision>,
}

fn same_scope(a: &TenantContext, b: &TenantContext) -> bool {
    a.org_id == b.org_id && a.workspace_id == b.workspace_id && a.deployment_id == b.deployment_id
}

impl AppState {
    pub async fn enterprise_org_unit_view(
        &self,
        tenant: &TenantContext,
    ) -> Result<EnterpriseOrgUnitView, &'static str> {
        let policy = self.enterprise.hosted_policy.current()?;
        let mut units: Vec<_> = self
            .enterprise
            .org_units
            .read()
            .await
            .values()
            .filter(|row| same_scope(&row.tenant_context, tenant))
            .cloned()
            .collect();
        let mut memberships: Vec<_> = self
            .enterprise
            .org_unit_memberships
            .read()
            .await
            .values()
            .filter(|row| same_scope(&row.tenant_context, tenant))
            .cloned()
            .collect();
        let mut access_grants: Vec<_> = self
            .enterprise
            .org_unit_access_grants
            .read()
            .await
            .values()
            .filter(|row| same_scope(&row.tenant_context, tenant))
            .cloned()
            .collect();
        let hosted_policy_revision = if let Some(policy) = policy {
            if tenant.org_id != policy.bundle().organization_id
                || tenant.workspace_id != policy.bundle().deployment_id
                || tenant.deployment_id.as_deref() != Some(policy.bundle().deployment_id.as_str())
            {
                return Err("hosted_policy_scope_mismatch");
            }
            if units
                .iter()
                .any(|unit| unit.taxonomy_id == HOSTED_TAXONOMY_ID)
            {
                return Err("hosted_registry_ownership_conflict");
            }
            let projected = policy.registry_projection(crate::now_ms())?;
            units.extend(projected.units);
            // Hosted human memberships have a single owner. Native service and
            // automation memberships retain their independently authored state.
            memberships.retain(|row| {
                row.member.kind != PrincipalKind::HumanUser
                    && row.source != OrganizationUnitMembershipSource::HostedControlPlane
                    && !row.unit.id.starts_with(&format!("{HOSTED_TAXONOMY_ID}/"))
            });
            memberships.extend(projected.memberships);
            access_grants.extend(projected.access_grants);
            Some(policy.revision().clone())
        } else {
            None
        };
        Ok(EnterpriseOrgUnitView {
            units,
            memberships,
            access_grants,
            hosted_policy_revision,
        })
    }
}
