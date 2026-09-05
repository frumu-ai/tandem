//! Ephemeral registry projection. Its sole owner is the accepted control-plane
//! snapshot; callers must never persist these rows into locally authored stores.
use super::{
    hosted_unit_principal, projection::permission, ValidatedHostedPolicy, HOSTED_TAXONOMY_ID,
};
use crate::{
    DataClass, OrganizationUnit, OrganizationUnitAccessGrant, OrganizationUnitKind,
    OrganizationUnitMembership, OrganizationUnitMembershipSource, OrganizationUnitState,
    PrincipalKind, PrincipalRef, TenantContext,
};

pub struct HostedRegistryProjection {
    pub units: Vec<OrganizationUnit>,
    pub memberships: Vec<OrganizationUnitMembership>,
    pub access_grants: Vec<OrganizationUnitAccessGrant>,
}

impl ValidatedHostedPolicy {
    pub fn registry_projection(
        &self,
        now_ms: u64,
    ) -> Result<HostedRegistryProjection, &'static str> {
        if now_ms >= self.expires_at_ms {
            return Err("hosted_policy_not_fresh");
        }
        let tenant = TenantContext::explicit_user_workspace(
            &self.bundle.organization_id,
            &self.bundle.deployment_id,
            Some(self.bundle.deployment_id.clone()),
            "hosted-policy-agent",
        );
        let generated = self.bundle.generated_at.timestamp_millis() as u64;
        let units = self
            .bundle
            .org_units
            .iter()
            .map(|row| {
                let kind = match row.kind.as_str() {
                    "department" => OrganizationUnitKind::Department,
                    "team" => OrganizationUnitKind::Team,
                    _ => OrganizationUnitKind::Custom,
                };
                OrganizationUnit::active(
                    &row.id,
                    tenant.clone(),
                    &row.display_name,
                    kind,
                    PrincipalRef::new(PrincipalKind::ServiceAccount, "hosted-policy-agent"),
                    generated,
                )
                .with_taxonomy_id(HOSTED_TAXONOMY_ID)
                .with_state(
                    if row.state == "active" {
                        OrganizationUnitState::Active
                    } else {
                        OrganizationUnitState::Disabled
                    },
                    generated,
                )
            })
            .collect();
        let memberships =
            self.bundle
                .org_unit_memberships
                .iter()
                .map(|row| {
                    let active = self.bundle.users.iter().any(|user| {
                        user.id == row.user_id && user.is_active && user.email_verified
                    }) && self
                        .bundle
                        .org_units
                        .iter()
                        .any(|unit| unit.id == row.unit_id && unit.state == "active");
                    let mut membership = OrganizationUnitMembership::active(
                        format!(
                            "hosted:{}:{}:{}",
                            self.bundle.deployment_id, row.unit_id, row.user_id
                        ),
                        tenant.clone(),
                        hosted_unit_principal(&row.unit_id),
                        PrincipalRef::human_user(&row.user_id),
                        OrganizationUnitMembershipSource::HostedControlPlane,
                        generated,
                    )
                    .with_expires_at_ms(self.expires_at_ms);
                    if !active {
                        membership.state = OrganizationUnitState::Disabled;
                    }
                    membership
                })
                .collect();
        let access_grants = self
            .bundle
            .deployment_grants
            .iter()
            .filter(|row| row.principal_kind == "org_unit")
            .map(|row| {
                OrganizationUnitAccessGrant::active(
                    format!("hosted:{}:grant:{}", self.bundle.deployment_id, row.id),
                    tenant.clone(),
                    hosted_unit_principal(&row.principal_id),
                    self.deployment_resource(),
                    generated,
                )
                .with_permissions(
                    row.permissions
                        .iter()
                        .filter_map(|p| permission(p))
                        .collect(),
                )
                .with_data_classes(vec![DataClass::Internal])
                .with_expires_at_ms(self.expires_at_ms)
            })
            .collect();
        Ok(HostedRegistryProjection {
            units,
            memberships,
            access_grants,
        })
    }
}
