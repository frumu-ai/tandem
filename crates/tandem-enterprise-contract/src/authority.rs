//! Intra-tenant authority graph (CT-18 / TAN-89).
//!
//! Cross-tenant isolation keeps one tenant's data away from another tenant. An
//! AI-first business also needs boundaries *inside* a single tenant: a junior
//! engineer should not read lead-engineer architecture docs, and an engineer
//! should not read finance books unless they were explicitly shared.
//!
//! This module operationalizes the existing authority primitives
//! ([`PrincipalRef`], [`ResourceRef`], [`ScopedGrant`], [`DataClass`],
//! [`OrganizationUnit`], [`OrganizationUnitMembership`],
//! [`OrganizationUnitAccessGrant`]) into a single graph that:
//!
//! * resolves a principal's *effective grants* from direct grants plus
//!   organization-unit memberships, honoring unit nesting (a unit can be a
//!   member of another unit) and parent-unit ancestry, and
//! * renders a **fail-closed** access decision: a request is allowed only when
//!   a matching allow grant exists, an explicit deny grant always wins, and the
//!   absence of any matching grant denies.
//!
//! Decisions are intentionally side-effect free so callers can attribute and
//! audit them (policy decision records + protected audit evidence) at the
//! enforcement site.

use serde::{Deserialize, Serialize};

use crate::{
    AccessEffect, AccessPermission, DataClass, GrantSource, OrganizationUnit,
    OrganizationUnitAccessGrant, OrganizationUnitMembership, PrincipalRef, ResourceRef,
    ScopedGrant, TenantContext,
};

/// Effect of an intra-tenant authority decision.
///
/// Fail-closed by construction: anything that is not an explicit allow is a
/// deny, so callers never have to treat "not applicable" as "permitted".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityEffect {
    Allow,
    Deny,
}

impl AuthorityEffect {
    pub fn is_allow(self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn is_deny(self) -> bool {
        matches!(self, Self::Deny)
    }
}

/// A resolved intra-tenant access decision.
///
/// `reason_code` is a stable machine token (suitable for a
/// `PolicyDecisionRecord.reason_code`); `reason` is a human-readable summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityDecision {
    pub effect: AuthorityEffect,
    pub reason_code: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    /// The unit / department / role-domain a deciding grant was derived from,
    /// when the grant came from an organization-unit membership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_principal: Option<PrincipalRef>,
}

impl AuthorityDecision {
    pub fn is_allow(&self) -> bool {
        self.effect.is_allow()
    }

    pub fn is_deny(&self) -> bool {
        self.effect.is_deny()
    }

    fn allow(grant: &ScopedGrant) -> Self {
        Self {
            effect: AuthorityEffect::Allow,
            reason_code: "matching_allow_grant".to_string(),
            reason: "principal holds a matching allow grant for the resource".to_string(),
            grant_id: Some(grant.grant_id.clone()),
            source_principal: grant.source_principal.clone(),
        }
    }

    fn deny_grant(grant: &ScopedGrant) -> Self {
        Self {
            effect: AuthorityEffect::Deny,
            reason_code: "matching_deny_grant".to_string(),
            reason: "an explicit deny grant blocks the principal from the resource".to_string(),
            grant_id: Some(grant.grant_id.clone()),
            source_principal: grant.source_principal.clone(),
        }
    }

    fn deny_no_grant() -> Self {
        Self {
            effect: AuthorityEffect::Deny,
            reason_code: "no_matching_grant".to_string(),
            reason: "principal holds no grant authorizing the resource (fail closed)".to_string(),
            grant_id: None,
            source_principal: None,
        }
    }

    fn deny_cross_tenant() -> Self {
        Self {
            effect: AuthorityEffect::Deny,
            reason_code: "resource_outside_tenant".to_string(),
            reason: "resource belongs to a different tenant than the authority graph".to_string(),
            grant_id: None,
            source_principal: None,
        }
    }
}

/// A single intra-tenant access request: "can `principal` do `permission` on
/// `resource` carrying `data_class`?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityAccessRequest {
    pub principal: PrincipalRef,
    pub resource: ResourceRef,
    pub permission: AccessPermission,
    pub data_class: DataClass,
}

impl AuthorityAccessRequest {
    pub fn new(
        principal: PrincipalRef,
        resource: ResourceRef,
        permission: AccessPermission,
        data_class: DataClass,
    ) -> Self {
        Self {
            principal,
            resource,
            permission,
            data_class,
        }
    }
}

/// An intra-tenant authority graph: the units, memberships, unit access grants,
/// and direct grants that define who can reach what inside one tenant.
///
/// The graph is a pure value: build it from stored enterprise state (or a seed
/// fixture), then call [`IntraTenantAuthorityGraph::evaluate`] to render a
/// decision. It never mutates and never performs I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntraTenantAuthorityGraph {
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<OrganizationUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memberships: Vec<OrganizationUnitMembership>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unit_access_grants: Vec<OrganizationUnitAccessGrant>,
    /// Grants bound directly to a principal (e.g. from a verified context's
    /// strict projection or an explicit one-off share).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_grants: Vec<ScopedGrant>,
}

impl IntraTenantAuthorityGraph {
    pub fn new(tenant_context: TenantContext) -> Self {
        Self {
            tenant_context,
            units: Vec::new(),
            memberships: Vec::new(),
            unit_access_grants: Vec::new(),
            direct_grants: Vec::new(),
        }
    }

    pub fn with_unit(mut self, unit: OrganizationUnit) -> Self {
        self.units.push(unit);
        self
    }

    pub fn with_membership(mut self, membership: OrganizationUnitMembership) -> Self {
        self.memberships.push(membership);
        self
    }

    pub fn with_unit_access_grant(mut self, grant: OrganizationUnitAccessGrant) -> Self {
        self.unit_access_grants.push(grant);
        self
    }

    pub fn with_direct_grant(mut self, grant: ScopedGrant) -> Self {
        self.direct_grants.push(grant);
        self
    }

    pub fn extend_units(&mut self, units: impl IntoIterator<Item = OrganizationUnit>) {
        self.units.extend(units);
    }

    pub fn extend_memberships(
        &mut self,
        memberships: impl IntoIterator<Item = OrganizationUnitMembership>,
    ) {
        self.memberships.extend(memberships);
    }

    pub fn extend_unit_access_grants(
        &mut self,
        grants: impl IntoIterator<Item = OrganizationUnitAccessGrant>,
    ) {
        self.unit_access_grants.extend(grants);
    }

    pub fn extend_direct_grants(&mut self, grants: impl IntoIterator<Item = ScopedGrant>) {
        self.direct_grants.extend(grants);
    }

    fn tenant_matches(&self, tenant: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
    }

    fn membership_in_tenant(&self, membership: &OrganizationUnitMembership) -> bool {
        self.tenant_matches(&membership.tenant_context)
    }

    fn unit_grant_in_tenant(&self, grant: &OrganizationUnitAccessGrant) -> bool {
        self.tenant_matches(&grant.tenant_context)
    }

    fn unit_by_principal(&self, principal: &PrincipalRef) -> Option<&OrganizationUnit> {
        self.units
            .iter()
            .find(|unit| &unit.principal_ref() == principal)
    }

    /// Resolve every organization-unit principal the given principal belongs to,
    /// transitively. Expansion covers:
    ///
    /// * direct memberships (`member == principal`),
    /// * unit-in-unit nesting (a unit that is itself a member of another unit),
    /// * parent-unit ancestry (a unit inherits its ancestors' grants).
    ///
    /// Only memberships active at `now_ms` and within the graph's tenant are
    /// followed, so expired memberships never widen authority.
    pub fn resolved_unit_principals(
        &self,
        principal: &PrincipalRef,
        now_ms: u64,
    ) -> Vec<PrincipalRef> {
        let mut resolved: Vec<PrincipalRef> = Vec::new();
        // Seed the frontier with the principal itself so we pick up its direct
        // memberships, then expand outward through nesting and ancestry.
        let mut frontier: Vec<PrincipalRef> = vec![principal.clone()];
        let mut visited: Vec<PrincipalRef> = vec![principal.clone()];

        while let Some(current) = frontier.pop() {
            for membership in self.memberships.iter().filter(|membership| {
                self.membership_in_tenant(membership)
                    && membership.is_active_at(now_ms)
                    && membership.member == current
            }) {
                self.push_unit_with_ancestry(
                    &membership.unit,
                    &mut resolved,
                    &mut frontier,
                    &mut visited,
                );
            }
        }

        resolved
    }

    fn push_unit_with_ancestry(
        &self,
        unit: &PrincipalRef,
        resolved: &mut Vec<PrincipalRef>,
        frontier: &mut Vec<PrincipalRef>,
        visited: &mut Vec<PrincipalRef>,
    ) {
        if !visited.contains(unit) {
            visited.push(unit.clone());
            // A unit can itself be a member of another unit; keep expanding.
            frontier.push(unit.clone());
        }
        if !resolved.contains(unit) {
            resolved.push(unit.clone());
        }

        // Walk the structural parent chain so membership in a child unit
        // inherits the parent unit's grants (junior-eng ⊂ engineering).
        let mut cursor = self.unit_by_principal(unit).cloned();
        while let Some(node) = cursor {
            let Some(parent_id) = node.parent_unit_id.clone() else {
                break;
            };
            let Some(parent) = self.units.iter().find(|candidate| {
                candidate.unit_id == parent_id && candidate.taxonomy_id == node.taxonomy_id
            }) else {
                break;
            };
            let parent_principal = parent.principal_ref();
            if resolved.contains(&parent_principal) {
                break;
            }
            resolved.push(parent_principal.clone());
            if !visited.contains(&parent_principal) {
                visited.push(parent_principal.clone());
                frontier.push(parent_principal);
            }
            cursor = Some(parent.clone());
        }
    }

    /// Build the full set of grants that apply to `principal`: direct grants
    /// plus every unit access grant reachable through resolved unit
    /// memberships, each re-bound to the requesting principal.
    pub fn effective_grants(&self, principal: &PrincipalRef, now_ms: u64) -> Vec<ScopedGrant> {
        let mut grants: Vec<ScopedGrant> = self
            .direct_grants
            .iter()
            .filter(|grant| &grant.principal == principal && !grant.is_expired_at(now_ms))
            .cloned()
            .collect();

        let unit_principals = self.resolved_unit_principals(principal, now_ms);
        for unit in &unit_principals {
            for unit_grant in self.unit_access_grants.iter().filter(|grant| {
                self.unit_grant_in_tenant(grant)
                    && &grant.unit == unit
                    && grant.is_active_at(now_ms)
            }) {
                grants.push(self.bind_unit_grant_to_principal(unit_grant, principal, unit));
            }
        }

        grants
    }

    fn bind_unit_grant_to_principal(
        &self,
        unit_grant: &OrganizationUnitAccessGrant,
        principal: &PrincipalRef,
        unit: &PrincipalRef,
    ) -> ScopedGrant {
        let mut grant = ScopedGrant::new(
            format!("{}::{}", unit.id, unit_grant.grant_id),
            principal.clone(),
            unit_grant.resource.clone(),
            GrantSource::OrganizationUnitMembership,
        )
        .with_effect(unit_grant.effect)
        .with_permissions(unit_grant.permissions.clone())
        .with_data_classes(unit_grant.data_classes.clone())
        .with_tool_patterns(unit_grant.tool_patterns.clone())
        .with_source_principal(unit.clone());
        grant.expires_at_ms = unit_grant.expires_at_ms;
        grant
    }

    /// Render a fail-closed access decision for `request` as of `now_ms`.
    ///
    /// Order of precedence:
    /// 1. cross-tenant resource → deny;
    /// 2. any matching deny grant → deny (deny always wins);
    /// 3. any matching allow grant → allow;
    /// 4. otherwise → deny (no matching grant).
    pub fn evaluate(&self, request: &AuthorityAccessRequest, now_ms: u64) -> AuthorityDecision {
        if request.resource.organization_id != self.tenant_context.org_id {
            return AuthorityDecision::deny_cross_tenant();
        }

        let grants = self.effective_grants(&request.principal, now_ms);

        if let Some(grant) = grants.iter().find(|grant| {
            grant.effect == AccessEffect::Deny
                && grant.applies_to(
                    &request.resource,
                    request.permission,
                    request.data_class,
                    now_ms,
                )
        }) {
            return AuthorityDecision::deny_grant(grant);
        }

        if let Some(grant) = grants.iter().find(|grant| {
            grant.effect == AccessEffect::Allow
                && grant.applies_to(
                    &request.resource,
                    request.permission,
                    request.data_class,
                    now_ms,
                )
        }) {
            return AuthorityDecision::allow(grant);
        }

        AuthorityDecision::deny_no_grant()
    }
}

pub mod fixtures;

#[cfg(test)]
mod tests;
