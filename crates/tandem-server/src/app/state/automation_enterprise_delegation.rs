use crate::automation_v2::types::{AutomationEnterpriseScope, AutomationV2Spec};
use crate::util::time::now_ms;
use tandem_enterprise_contract::{
    canonical_enterprise_scope_id, AccessEffect, OrganizationUnitAccessGrant,
};
use tandem_types::{AccessPermission, TenantContext};

use super::AppState;

impl AppState {
    pub(crate) async fn validate_automation_enterprise_delegation_grants(
        &self,
        automation: &AutomationV2Spec,
    ) -> anyhow::Result<()> {
        let Some(scope) = automation.enterprise_scope() else {
            return Ok(());
        };
        if scope.delegation_grant_ids.is_empty() {
            return Ok(());
        }
        let tenant_context = automation.tenant_context();
        let grants = self.enterprise.org_unit_access_grants.read().await;
        let now_ms = now_ms();
        for grant_id in &scope.delegation_grant_ids {
            let Some(grant) = grants.values().find(|grant| {
                enterprise_delegation_grant_matches_scope(
                    grant,
                    &tenant_context,
                    &scope,
                    grant_id,
                    now_ms,
                )
            }) else {
                anyhow::bail!(
                    "delegation grant `{grant_id}` is not active authority for automation `{}`",
                    automation.automation_id
                );
            };
            if !grant.permissions.contains(&AccessPermission::Execute) {
                anyhow::bail!(
                    "delegation grant `{}` does not include execute authority for automation `{}`",
                    grant.grant_id,
                    automation.automation_id
                );
            }
        }
        Ok(())
    }
}

fn enterprise_delegation_grant_matches_scope(
    grant: &OrganizationUnitAccessGrant,
    tenant_context: &TenantContext,
    scope: &AutomationEnterpriseScope,
    grant_id: &str,
    now_ms: u64,
) -> bool {
    grant.grant_id.trim() == grant_id.trim()
        && grant.tenant_context.org_id == tenant_context.org_id
        && grant.tenant_context.workspace_id == tenant_context.workspace_id
        && grant.tenant_context.deployment_id == tenant_context.deployment_id
        && grant.effect == AccessEffect::Allow
        && grant.is_active_at(now_ms)
        && enterprise_delegation_grant_org_unit_matches(grant, scope)
        && enterprise_delegation_grant_resource_matches(grant, scope)
        && enterprise_delegation_grant_data_classes_match(grant, scope)
}

fn enterprise_delegation_grant_org_unit_matches(
    grant: &OrganizationUnitAccessGrant,
    scope: &AutomationEnterpriseScope,
) -> bool {
    let Some(expected) = scope.owning_org_unit_id.as_deref() else {
        return true;
    };
    let Some(expected) = canonical_enterprise_scope_id(expected) else {
        return false;
    };
    let Some(actual) = canonical_enterprise_scope_id(&grant.unit.id) else {
        return false;
    };
    actual == expected || actual.ends_with(&format!("/{expected}"))
}

fn enterprise_delegation_grant_resource_matches(
    grant: &OrganizationUnitAccessGrant,
    scope: &AutomationEnterpriseScope,
) -> bool {
    let Some(resource_scope) = scope.resource_scope.as_ref() else {
        return true;
    };
    grant.resource.applies_to(&resource_scope.root)
        && resource_scope
            .allowed_resources
            .iter()
            .all(|resource| grant.resource.applies_to(resource))
}

fn enterprise_delegation_grant_data_classes_match(
    grant: &OrganizationUnitAccessGrant,
    scope: &AutomationEnterpriseScope,
) -> bool {
    scope.data_classes.is_empty()
        || scope
            .data_classes
            .iter()
            .all(|data_class| grant.data_classes.contains(data_class))
}
