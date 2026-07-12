use crate::automation_v2::types::{AutomationEnterpriseScope, AutomationV2Spec};
use crate::util::time::now_ms;
use tandem_enterprise_contract::{
    canonical_enterprise_scope_id, AccessEffect, OrganizationUnitAccessGrant,
};
use tandem_types::{AccessPermission, PrincipalRef, TenantContext};

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
            let matching = grants.values().filter(|grant| {
                enterprise_delegation_grant_matches_scope(
                    grant,
                    &tenant_context,
                    &scope,
                    grant_id,
                    now_ms,
                )
            });
            validate_matching_delegation_grants(
                matching,
                grant_id,
                &automation.automation_id,
                AccessPermission::Execute,
            )?;
        }
        Ok(())
    }

    pub(crate) async fn validate_automation_enterprise_delegation_grants_for_author(
        &self,
        automation: &AutomationV2Spec,
        author: &PrincipalRef,
    ) -> anyhow::Result<()> {
        self.validate_automation_enterprise_delegation_grants(automation)
            .await?;

        let Some(scope) = automation.enterprise_scope() else {
            return Ok(());
        };
        if scope.delegation_grant_ids.is_empty() {
            return Ok(());
        }

        let tenant_context = automation.tenant_context();
        let graph = self
            .build_intra_tenant_authority_graph(&tenant_context, Vec::new())
            .await;
        let now_ms = now_ms();
        let author_units = graph.resolved_unit_principals(author, now_ms);

        for grant_id in &scope.delegation_grant_ids {
            for permission in [AccessPermission::View, AccessPermission::Execute] {
                let matching = graph.unit_access_grants.iter().filter(|grant| {
                    author_units.contains(&grant.unit)
                        && enterprise_delegation_grant_matches_scope(
                            grant,
                            &tenant_context,
                            &scope,
                            grant_id,
                            now_ms,
                        )
                });
                validate_matching_delegation_grants(
                    matching,
                    grant_id,
                    &automation.automation_id,
                    permission,
                )
                .map_err(|error| {
                    anyhow::anyhow!(
                        "author `{}` cannot inspect/use automation `{}`: {error}",
                        author.id,
                        automation.automation_id
                    )
                })?;
            }
        }
        Ok(())
    }
}

fn validate_matching_delegation_grants<'a>(
    grants: impl Iterator<Item = &'a OrganizationUnitAccessGrant>,
    grant_id: &str,
    automation_id: &str,
    permission: AccessPermission,
) -> anyhow::Result<()> {
    let matching = grants.collect::<Vec<_>>();
    if matching
        .iter()
        .any(|grant| grant.effect == AccessEffect::Deny && grant.permissions.contains(&permission))
    {
        anyhow::bail!(
            "delegation grant `{grant_id}` explicitly denies {permission:?} authority for automation `{automation_id}`"
        );
    }
    match matching.iter().find(|grant| {
        grant.effect == AccessEffect::Allow && grant.permissions.contains(&permission)
    }) {
        Some(_) => Ok(()),
        None if matching.is_empty() => {
            anyhow::bail!(
                "delegation grant `{grant_id}` is not active authority for automation `{automation_id}`"
            );
        }
        None => anyhow::bail!(
            "delegation grant `{grant_id}` does not include {permission:?} authority for automation `{automation_id}`"
        ),
    }
}

fn enterprise_delegation_grant_matches_scope(
    grant: &OrganizationUnitAccessGrant,
    tenant_context: &TenantContext,
    scope: &AutomationEnterpriseScope,
    grant_id: &str,
    now_ms: u64,
) -> bool {
    canonical_enterprise_scope_id(&grant.grant_id) == canonical_enterprise_scope_id(grant_id)
        && grant.tenant_context.org_id == tenant_context.org_id
        && grant.tenant_context.workspace_id == tenant_context.workspace_id
        && grant.tenant_context.deployment_id == tenant_context.deployment_id
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
