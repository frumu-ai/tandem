// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Per-automation authority for routes which operate on an automation indirectly.
//! A deployment operation grant alone must not confer access to another actor's
//! workflow, campaign, or learning evidence.

use tandem_types::{
    AccessDecision, AccessPermission, DataClass, GrantSource, PrincipalRef, ResourceKind,
    ResourceRef, TenantContext, VerifiedTenantContext,
};

use std::collections::HashMap;

use crate::{AppState, AutomationV2Spec};

#[derive(Clone, Copy)]
enum ObjectAccess {
    Read,
    Write,
    Execute,
}

enum GrantView<'a> {
    Live,
    Held {
        memberships: &'a HashMap<String, tandem_types::OrganizationUnitMembership>,
        access_grants: &'a HashMap<String, tandem_types::OrganizationUnitAccessGrant>,
        cross_tenant_grants: &'a HashMap<String, tandem_types::CrossTenantGrantRecord>,
    },
}

pub(super) fn can_read(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation: &AutomationV2Spec,
) -> bool {
    allowed(
        state,
        tenant,
        verified,
        automation,
        AccessPermission::HostedAutomationRead,
        ObjectAccess::Read,
        GrantView::Live,
    )
}

/// Evaluate a read with current grant registries held through frame creation.
/// A queued writer must not turn an already-held read guard into a denial by
/// causing a second `try_read` of the same registry to fail.
pub(super) fn can_read_with_held_grants(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation: &AutomationV2Spec,
    memberships: &HashMap<String, tandem_types::OrganizationUnitMembership>,
    access_grants: &HashMap<String, tandem_types::OrganizationUnitAccessGrant>,
    cross_tenant_grants: &HashMap<String, tandem_types::CrossTenantGrantRecord>,
) -> bool {
    allowed(
        state,
        tenant,
        verified,
        automation,
        AccessPermission::HostedAutomationRead,
        ObjectAccess::Read,
        GrantView::Held {
            memberships,
            access_grants,
            cross_tenant_grants,
        },
    )
}

pub(super) fn can_write(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation: &AutomationV2Spec,
) -> bool {
    allowed(
        state,
        tenant,
        verified,
        automation,
        AccessPermission::HostedAutomationWrite,
        ObjectAccess::Write,
        GrantView::Live,
    )
}

pub(super) fn can_execute(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation: &AutomationV2Spec,
) -> bool {
    allowed(
        state,
        tenant,
        verified,
        automation,
        AccessPermission::HostedAutomationExecute,
        ObjectAccess::Execute,
        GrantView::Live,
    )
}

fn allowed(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation: &AutomationV2Spec,
    operation: AccessPermission,
    object_access: ObjectAccess,
    grant_view: GrantView<'_>,
) -> bool {
    let source_tenant = automation.tenant_context();
    if !super::tenant_matches(tenant, &source_tenant) {
        return false;
    }
    if tenant.is_local_implicit() {
        return true;
    }
    let Some(verified) = verified else {
        return false;
    };
    let now = crate::now_ms();
    let actor_id = verified.human_actor.actor_id.trim();
    if actor_id.is_empty()
        || verified.is_expired_at(now)
        || !super::tenant_matches(tenant, &verified.tenant_context)
        || tenant.actor_id.as_deref() != Some(actor_id)
    {
        return false;
    }

    // Reproject from the current policy rather than trusting a snapshot kept
    // on the request while its handler awaited campaign/workflow storage.
    let mut current = verified.clone();
    let current_memberships = match state.enterprise.hosted_policy.project(&mut current) {
        Ok(memberships) => memberships,
        Err(_) => return false,
    };
    if state
        .enterprise
        .hosted_policy
        .authorize_permission(Some(&current), operation)
        .is_err()
    {
        return false;
    }
    let current_admin = if current.policy_version.is_some() {
        state.authorize_current_hosted_admin(&current).is_ok()
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
        return true;
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
        return true;
    }
    // Hosted projection deliberately drops ingress-enriched object grants.
    // Without hosted policy, remove only the revocable grants middleware
    // appended to the signed assertion before rebuilding from live stores.
    if current_memberships.is_none() {
        if let Some(strict) = current.strict_projection.as_mut() {
            strict.grants.retain(|grant| {
                !matches!(
                    grant.grant_source,
                    GrantSource::OrganizationUnitMembership | GrantSource::CrossTenantGrant
                )
            });
        }
    }
    let grant_ready = match grant_view {
        GrantView::Live => {
            try_enrich_current_org_unit_grants(state, &mut current, current_memberships.clone())
                && super::cross_tenant_grants::try_enrich_verified_context_with_inbound_cross_tenant_grants(
                    state,
                    &mut current,
                )
        }
        GrantView::Held {
            memberships,
            access_grants,
            cross_tenant_grants,
        } => {
            if current.strict_projection.is_some() {
                let hosted = current_memberships.is_some();
                let memberships = current_memberships
                    .clone()
                    .unwrap_or_else(|| memberships.values().cloned().collect());
                super::middleware::project_org_unit_grants_into_verified_context(
                    &mut current,
                    memberships.iter(),
                    access_grants
                        .values()
                        .filter(|grant| !hosted || super::middleware::local_hosted_data_grant(grant)),
                    crate::now_ms(),
                );
                if !current.tenant_context.is_local_implicit() {
                    super::cross_tenant_grants::project_inbound_cross_tenant_grants(
                        &mut current,
                        cross_tenant_grants.values(),
                        crate::now_ms(),
                    );
                }
            }
            true
        }
    };
    let current_grant =
        grant_ready && scoped_grant(&current, tenant, automation, object_access, now);
    if current_grant {
        return true;
    }
    if !matches!(object_access, ObjectAccess::Read) {
        return false;
    }
    match access
        .and_then(|access| access.get("visibility"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("private")
    {
        "org" => true,
        "group" => {
            let audience = access
                .and_then(|access| access.get("audience_principals"))
                .and_then(serde_json::Value::as_array);
            let Some(audience) = audience else {
                return false;
            };
            if let Some(memberships) = current_memberships {
                memberships.iter().any(|membership| {
                    membership.is_active_at(now)
                        && audience
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .any(|unit_id| {
                                membership.unit
                                    == tandem_enterprise_contract::hosted_policy::hosted_unit_principal(
                                        unit_id,
                                    )
                            })
                })
            } else {
                current.org_units.iter().any(|unit| {
                    audience
                        .iter()
                        .any(|entry| entry.as_str() == Some(unit.as_str()))
                })
            }
        }
        _ => false,
    }
}

/// Checked optimization mutations call this synchronously. Read current
/// stores without blocking an async executor, and deny only grant-dependent
/// access if a writer currently holds either store.
pub(super) fn try_enrich_current_org_unit_grants(
    state: &AppState,
    verified: &mut VerifiedTenantContext,
    hosted_memberships: Option<Vec<tandem_types::OrganizationUnitMembership>>,
) -> bool {
    if verified.strict_projection.is_none() {
        return true;
    }
    let hosted = hosted_memberships.is_some();
    let memberships = match hosted_memberships {
        Some(memberships) => memberships,
        None => {
            let Ok(memberships) = state.enterprise.org_unit_memberships.try_read() else {
                return false;
            };
            memberships.values().cloned().collect()
        }
    };
    let Ok(access_grants) = state.enterprise.org_unit_access_grants.try_read() else {
        return false;
    };
    super::middleware::project_org_unit_grants_into_verified_context(
        verified,
        memberships.iter(),
        access_grants
            .values()
            .filter(|grant| !hosted || super::middleware::local_hosted_data_grant(grant)),
        crate::now_ms(),
    );
    true
}

fn scoped_grant(
    verified: &VerifiedTenantContext,
    tenant: &TenantContext,
    automation: &AutomationV2Spec,
    object_access: ObjectAccess,
    now: u64,
) -> bool {
    let Some(strict) = verified.strict_projection.as_ref() else {
        return false;
    };
    if strict.tenant_context != verified.tenant_context
        || strict.principal != PrincipalRef::human_user(&verified.human_actor.actor_id)
    {
        return false;
    }
    let resource = ResourceRef::new(
        &tenant.org_id,
        &tenant.workspace_id,
        ResourceKind::Automation,
        &automation.automation_id,
    );
    let permissions: &[AccessPermission] = match object_access {
        ObjectAccess::Read => &[
            AccessPermission::Read,
            AccessPermission::View,
            AccessPermission::Edit,
            AccessPermission::Admin,
        ],
        ObjectAccess::Write => &[AccessPermission::Edit, AccessPermission::Admin],
        ObjectAccess::Execute => &[AccessPermission::Execute, AccessPermission::Admin],
    };
    permissions.iter().any(|permission| {
        strict
            .evaluate_access(&resource, *permission, DataClass::Internal, now)
            .decision
            == AccessDecision::Allow
    })
}
