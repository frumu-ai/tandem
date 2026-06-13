//! EAA-08 (TAN-33): down-scoped delegation projection tokens.
//!
//! External delegates and A2A workers must never receive a broad hosted
//! context assertion. Instead, a delegator holding a verified
//! [`StrictTenantContext`] issues a narrow signed projection: its own JWS
//! `typ` (so it can never pass an assertion verifier), an exact resource
//! scope, an allow-list of permissions/tools/data classes, an expiry/nonce/
//! purpose, and a parent-assertion binding. Projection can only narrow the
//! parent context — every delegated (resource × permission × data class)
//! triple must already be allowed by the parent, evaluated with the same
//! fail-closed `evaluate_access` used everywhere else.

use serde::{Deserialize, Serialize};

use crate::{
    AccessDecision, AccessPermission, CrossTenantGrantParty, DataClass, GrantSource, PrincipalRef,
    ResourceScope, ScopedGrant, StrictTenantContext,
};

/// JWS `typ` for delegation projection tokens. Distinct from the hosted
/// context assertion typ (`tandem-tenant-context+jws`) and the cross-tenant
/// grant typ, so a projection can never be replayed as either.
pub const DELEGATION_PROJECTION_TYP: &str = "tandem-delegation-projection+jws";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationProjectionHeader {
    pub alg: String,
    pub typ: String,
    pub kid: String,
}

impl DelegationProjectionHeader {
    pub fn ed25519(key_id: impl Into<String>) -> Self {
        Self {
            alg: "EdDSA".to_string(),
            typ: DELEGATION_PROJECTION_TYP.to_string(),
            kid: key_id.into(),
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.alg == "EdDSA" && self.typ == DELEGATION_PROJECTION_TYP && !self.kid.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationProjectionClaims {
    pub version: String,
    pub delegation_id: String,
    /// `assertion_id` of the delegator's verified hosted context assertion.
    pub parent_assertion_id: String,
    pub tenant: CrossTenantGrantParty,
    pub delegator: PrincipalRef,
    pub delegate: PrincipalRef,
    /// Exact resource scope the delegate may see; never widened by projection.
    pub resource_scope: ResourceScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_patterns: Vec<String>,
    /// Explicit intent recorded with the token (e.g. "summarize-doc-42").
    pub purpose: String,
    /// Replay-prevention nonce; verifiers should reject reuse.
    pub nonce: String,
    /// Verifier service audience (mirrors assertion audience semantics).
    pub audience: String,
    pub issued_at_ms: u64,
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    /// Remaining re-delegation depth. `None` or `Some(0)` forbids
    /// re-delegation entirely: the projected context cannot carry the
    /// `Delegate` permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_delegation_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationProjection {
    pub header: DelegationProjectionHeader,
    pub claims: DelegationProjectionClaims,
    pub signature: String,
}

impl DelegationProjection {
    pub fn new(
        header: DelegationProjectionHeader,
        claims: DelegationProjectionClaims,
        signature: impl Into<String>,
    ) -> Self {
        Self {
            header,
            claims,
            signature: signature.into(),
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.header.is_well_formed()
            && !self.signature.trim().is_empty()
            && self.claims.version == "v1"
            && !self.claims.delegation_id.trim().is_empty()
            && !self.claims.parent_assertion_id.trim().is_empty()
            && !self.claims.nonce.trim().is_empty()
            && !self.claims.purpose.trim().is_empty()
            && !self.claims.permissions.is_empty()
            && !self.claims.data_classes.is_empty()
            && self.claims.expires_at_ms > self.claims.not_before_ms
    }

    /// Non-cryptographic verification policy (signature verification happens
    /// at the transport boundary with a `SigningKeyPurpose::DelegationProjection`
    /// key): typ/alg, version, audience, validity window, and parent binding.
    pub fn denial_reason(
        &self,
        expected_audience: &str,
        parent_assertion_id: &str,
        now_ms: u64,
    ) -> Option<&'static str> {
        if !self.is_well_formed() {
            return Some("delegation_projection_malformed");
        }
        if self.claims.audience != expected_audience {
            return Some("delegation_projection_audience_mismatch");
        }
        if self.claims.parent_assertion_id != parent_assertion_id {
            return Some("delegation_projection_parent_mismatch");
        }
        if now_ms < self.claims.not_before_ms {
            return Some("delegation_projection_not_yet_valid");
        }
        if now_ms >= self.claims.expires_at_ms {
            return Some("delegation_projection_expired");
        }
        None
    }

    /// Project this delegation into a [`StrictTenantContext`] for the
    /// delegate, narrowing the delegator's `parent` context. Fails closed:
    /// any delegated resource outside the parent scope, or any
    /// (resource × permission × data class) the parent cannot itself
    /// exercise, rejects the whole projection — a delegate can never hold
    /// authority the delegator lacks.
    pub fn project_into_strict_context(
        &self,
        parent: &StrictTenantContext,
        now_ms: u64,
    ) -> Result<StrictTenantContext, &'static str> {
        if let Some(reason) = self.denial_reason(
            &parent.assertion.audience,
            &parent.assertion.assertion_id,
            now_ms,
        ) {
            return Err(reason);
        }
        if !self
            .claims
            .tenant
            .matches_tenant_context(&parent.tenant_context)
        {
            return Err("delegation_projection_tenant_mismatch");
        }
        if self.claims.delegator != parent.principal {
            return Err("delegation_projection_delegator_mismatch");
        }
        if self.claims.delegate == self.claims.delegator {
            return Err("delegation_projection_self_delegation");
        }
        // Depth accounting across re-delegation chains: a context produced by
        // a projection carries `remaining_delegation_depth`; projecting FROM
        // such a context is itself a re-delegation and consumes one hop. The
        // child's remaining depth is the requested depth clamped by the
        // parent's remaining hops, so an intermediate delegate can never
        // mint more depth than it was given.
        let child_remaining = match parent.remaining_delegation_depth {
            None => self.claims.max_delegation_depth.unwrap_or(0),
            Some(0) => return Err("delegation_projection_depth_exhausted"),
            Some(parent_remaining) => self
                .claims
                .max_delegation_depth
                .unwrap_or(0)
                .min(parent_remaining - 1),
        };
        if child_remaining == 0
            && self
                .claims
                .permissions
                .contains(&AccessPermission::Delegate)
        {
            return Err("delegation_projection_redelegation_forbidden");
        }

        let mut delegated_resources = vec![self.claims.resource_scope.root.clone()];
        for resource in &self.claims.resource_scope.allowed_resources {
            if !delegated_resources.contains(resource) {
                delegated_resources.push(resource.clone());
            }
        }
        for resource in &delegated_resources {
            if !parent.resource_scope.contains(resource) {
                return Err("delegation_projection_widens_parent_scope");
            }
            for permission in &self.claims.permissions {
                for data_class in &self.claims.data_classes {
                    let evaluation =
                        parent.evaluate_access(resource, *permission, *data_class, now_ms);
                    if evaluation.decision != AccessDecision::Allow {
                        return Err("delegation_projection_exceeds_parent_authority");
                    }
                }
            }
        }

        let grants = delegated_resources
            .iter()
            .map(|resource| {
                ScopedGrant::new(
                    self.claims.delegation_id.clone(),
                    self.claims.delegate.clone(),
                    resource.clone(),
                    GrantSource::Delegation,
                )
                .with_permissions(self.claims.permissions.clone())
                .with_data_classes(self.claims.data_classes.clone())
                .with_tool_patterns(self.claims.tool_patterns.clone())
                .with_expires_at_ms(self.claims.expires_at_ms)
                .with_delegation_id(self.claims.delegation_id.clone())
            })
            .collect::<Vec<_>>();

        // Carry the parent's explicit denials so a broad delegated scope
        // (e.g. a whole project) cannot bypass narrower deny rules: the
        // parent's deny-effect grants and scope-level denied resources keep
        // applying inside the delegate's context.
        let mut grants = grants;
        for deny in parent
            .grants
            .iter()
            .filter(|grant| grant.effect == crate::AccessEffect::Deny)
        {
            grants.push(deny.clone());
        }
        let mut scope = self.claims.resource_scope.clone();
        for denied in &parent.resource_scope.denied_resources {
            if !scope.denied_resources.contains(denied) {
                scope.denied_resources.push(denied.clone());
            }
        }

        let assertion = crate::AssertionMetadata::new(
            parent.assertion.issuer.clone(),
            self.claims.audience.clone(),
            self.claims.issued_at_ms,
            self.claims
                .expires_at_ms
                .min(parent.assertion.expires_at_ms),
            self.claims.delegation_id.clone(),
        );

        Ok(StrictTenantContext::new(
            parent.tenant_context.clone(),
            self.claims.delegate.clone(),
            parent.authority_chain.clone(),
            scope,
            assertion,
        )
        .with_grants(grants)
        .with_data_boundary(parent.data_boundary.clone())
        .with_remaining_delegation_depth(child_remaining))
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::{
        AssertionMetadata, AuthorityChain, RequestPrincipal, ResourceKind, ResourceRef,
        TenantContext,
    };

    pub(crate) fn resource(id: &str) -> ResourceRef {
        ResourceRef::new("org-a", "workspace-a", ResourceKind::Document, id)
    }

    pub(crate) fn parent_context() -> StrictTenantContext {
        let delegator = PrincipalRef::human_user("lead-1");
        let project_root =
            ResourceRef::new("org-a", "workspace-a", ResourceKind::Project, "proj-1");
        let mut scope = ResourceScope::root(project_root.clone());
        scope.allowed_resources = vec![resource("doc-1"), resource("doc-2")];
        let grants = vec![
            ScopedGrant::new(
                "grant-root",
                delegator.clone(),
                project_root,
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal]),
            ScopedGrant::new(
                "grant-doc-1",
                delegator.clone(),
                resource("doc-1"),
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal]),
            ScopedGrant::new(
                "grant-doc-2",
                delegator.clone(),
                resource("doc-2"),
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal]),
        ];
        StrictTenantContext::new(
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "lead-1"),
            delegator.clone(),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                delegator.id,
                "tandem-web",
            )),
            scope,
            AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999,
                "assertion-parent",
            ),
        )
        .with_grants(grants)
    }

    pub(crate) fn delegation_to(doc: &str) -> DelegationProjection {
        let claims = DelegationProjectionClaims {
            version: "v1".to_string(),
            delegation_id: "delegation-1".to_string(),
            parent_assertion_id: "assertion-parent".to_string(),
            tenant: CrossTenantGrantParty {
                organization_id: "org-a".to_string(),
                workspace_id: "workspace-a".to_string(),
                deployment_id: None,
            },
            delegator: PrincipalRef::human_user("lead-1"),
            delegate: PrincipalRef::new(crate::PrincipalKind::ExternalDelegate, "a2a-worker-1"),
            resource_scope: ResourceScope::root(resource(doc)),
            permissions: vec![AccessPermission::Read],
            data_classes: vec![DataClass::Internal],
            tool_patterns: vec!["mcp:google-drive:files.get".to_string()],
            purpose: "summarize-doc".to_string(),
            nonce: "nonce-1".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 2_000,
            not_before_ms: 2_000,
            expires_at_ms: 5_000,
            max_delegation_depth: None,
        };
        DelegationProjection::new(
            DelegationProjectionHeader::ed25519("delegation-key-1"),
            claims,
            "signature",
        )
    }

    #[test]
    fn projection_typ_is_not_a_hosted_context_assertion() {
        // A projection token can never pass as a broad hosted context
        // assertion: the JWS typ lanes are disjoint and each header
        // validator rejects the other's typ.
        assert_ne!(DELEGATION_PROJECTION_TYP, "tandem-tenant-context+jws");
        let mut header = DelegationProjectionHeader::ed25519("key-1");
        header.typ = "tandem-tenant-context+jws".to_string();
        assert!(!header.is_well_formed());
    }

    #[test]
    fn delegate_sees_only_the_delegated_resource_graph() {
        let parent = parent_context();
        let delegated = delegation_to("doc-1")
            .project_into_strict_context(&parent, 3_000)
            .expect("projection");

        assert_eq!(delegated.principal.id, "a2a-worker-1");
        assert_eq!(
            delegated
                .evaluate_access(
                    &resource("doc-1"),
                    AccessPermission::Read,
                    DataClass::Internal,
                    3_000
                )
                .decision,
            AccessDecision::Allow
        );
        // Sibling resource inside the PARENT scope is invisible to the delegate.
        assert_ne!(
            delegated
                .evaluate_access(
                    &resource("doc-2"),
                    AccessPermission::Read,
                    DataClass::Internal,
                    3_000
                )
                .decision,
            AccessDecision::Allow
        );
        // Permission not delegated is denied even on the delegated resource.
        assert_ne!(
            delegated
                .evaluate_access(
                    &resource("doc-1"),
                    AccessPermission::Edit,
                    DataClass::Internal,
                    3_000
                )
                .decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn delegation_cannot_widen_parent_scope_or_authority() {
        let parent = parent_context();

        // Resource outside the parent scope.
        let mut outside = delegation_to("doc-1");
        outside.claims.resource_scope = ResourceScope::root(ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Project,
            "proj-other",
        ));
        assert_eq!(
            outside.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_widens_parent_scope")
        );

        // Permission the parent itself does not hold.
        let mut escalated = delegation_to("doc-1");
        escalated.claims.permissions = vec![AccessPermission::Admin];
        assert_eq!(
            escalated.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_exceeds_parent_authority")
        );

        // Data class the parent cannot read.
        let mut reclassified = delegation_to("doc-1");
        reclassified.claims.data_classes = vec![DataClass::Restricted];
        assert_eq!(
            reclassified.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_exceeds_parent_authority")
        );
    }

    #[test]
    fn verifier_checks_audience_expiry_and_parent_binding() {
        let parent = parent_context();

        let expired = delegation_to("doc-1");
        assert_eq!(
            expired.project_into_strict_context(&parent, 6_000),
            Err("delegation_projection_expired")
        );

        let mut wrong_audience = delegation_to("doc-1");
        wrong_audience.claims.audience = "other-service".to_string();
        assert_eq!(
            wrong_audience.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_audience_mismatch")
        );

        let mut wrong_parent = delegation_to("doc-1");
        wrong_parent.claims.parent_assertion_id = "assertion-other".to_string();
        assert_eq!(
            wrong_parent.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_parent_mismatch")
        );

        let mut wrong_delegator = delegation_to("doc-1");
        wrong_delegator.claims.delegator = PrincipalRef::human_user("someone-else");
        assert_eq!(
            wrong_delegator.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_delegator_mismatch")
        );
    }

    #[test]
    fn redelegation_is_forbidden_unless_depth_granted() {
        let parent = parent_context();
        let mut redelegate = delegation_to("doc-1");
        redelegate.claims.permissions = vec![AccessPermission::Read, AccessPermission::Delegate];
        assert_eq!(
            redelegate.project_into_strict_context(&parent, 3_000),
            Err("delegation_projection_redelegation_forbidden")
        );
    }
}

#[cfg(test)]
mod review_fix_tests {
    use super::tests_support::*;
    use super::*;
    use crate::{AccessEffect, ResourceKind, ResourceRef};

    #[test]
    fn broad_delegation_carries_parent_denies() {
        // Parent: project-wide allow + a document-specific deny. Delegating
        // the whole project must NOT launder away the deny (Codex P1).
        let mut parent = parent_context();
        let doc_denied = ResourceRef::new("org-a", "workspace-a", ResourceKind::Document, "doc-2")
            .with_project_id("proj-1");
        let doc_allowed = ResourceRef::new("org-a", "workspace-a", ResourceKind::Document, "doc-1")
            .with_project_id("proj-1");
        let project = ResourceRef::new("org-a", "workspace-a", ResourceKind::Project, "proj-1");
        parent.grants.push(
            ScopedGrant::new(
                "deny-doc-2",
                parent.principal.clone(),
                doc_denied.clone(),
                GrantSource::Direct,
            )
            .with_effect(AccessEffect::Deny)
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal]),
        );

        let mut projection = delegation_to("doc-1");
        projection.claims.resource_scope = ResourceScope::root(project);
        let delegated = projection
            .project_into_strict_context(&parent, 3_000)
            .expect("broad projection");

        assert_eq!(
            delegated
                .evaluate_access(
                    &doc_allowed,
                    AccessPermission::Read,
                    DataClass::Internal,
                    3_000
                )
                .decision,
            AccessDecision::Allow
        );
        assert_eq!(
            delegated
                .evaluate_access(
                    &doc_denied,
                    AccessPermission::Read,
                    DataClass::Internal,
                    3_000
                )
                .decision,
            AccessDecision::Deny,
            "parent's document-specific deny must survive a broad delegation"
        );
    }

    #[test]
    fn delegation_depth_is_decremented_across_the_chain() {
        // Root delegates with one re-delegation hop; the chain cannot mint
        // more depth for itself (Codex P1).
        let mut parent = parent_context();
        // The root delegator itself holds the Delegate permission on doc-1.
        parent.grants.push(
            ScopedGrant::new(
                "grant-delegate",
                parent.principal.clone(),
                resource("doc-1"),
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Delegate])
            .with_data_classes(vec![DataClass::Internal]),
        );
        let mut first = delegation_to("doc-1");
        first.claims.max_delegation_depth = Some(1);
        first.claims.permissions = vec![AccessPermission::Read, AccessPermission::Delegate];
        let delegate_a = first
            .project_into_strict_context(&parent, 3_000)
            .expect("first hop");
        assert_eq!(delegate_a.remaining_delegation_depth, Some(1));

        // Second hop: requesting a huge depth is clamped to 0, so granting
        // the Delegate permission onward is rejected...
        let mut greedy = delegation_to("doc-1");
        greedy.claims.delegation_id = "delegation-2".to_string();
        greedy.claims.parent_assertion_id = "delegation-1".to_string();
        greedy.claims.delegator = delegate_a.principal.clone();
        greedy.claims.delegate =
            PrincipalRef::new(crate::PrincipalKind::ExternalDelegate, "a2a-worker-2");
        greedy.claims.max_delegation_depth = Some(5);
        greedy.claims.permissions = vec![AccessPermission::Read, AccessPermission::Delegate];
        assert_eq!(
            greedy.project_into_strict_context(&delegate_a, 3_000),
            Err("delegation_projection_redelegation_forbidden")
        );

        // ...while a read-only second hop succeeds with zero remaining depth.
        let mut second = greedy.clone();
        second.claims.permissions = vec![AccessPermission::Read];
        let delegate_b = second
            .project_into_strict_context(&delegate_a, 3_000)
            .expect("second hop");
        assert_eq!(delegate_b.remaining_delegation_depth, Some(0));

        // A third hop from the exhausted context is rejected outright.
        let mut third = second.clone();
        third.claims.delegation_id = "delegation-3".to_string();
        third.claims.parent_assertion_id = "delegation-2".to_string();
        third.claims.delegator = delegate_b.principal.clone();
        third.claims.delegate =
            PrincipalRef::new(crate::PrincipalKind::ExternalDelegate, "a2a-worker-3");
        assert_eq!(
            third.project_into_strict_context(&delegate_b, 3_000),
            Err("delegation_projection_depth_exhausted")
        );
    }
}
