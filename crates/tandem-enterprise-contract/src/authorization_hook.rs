//! EAA-01 (TAN-26): the unified enterprise authorization seam.
//!
//! The per-surface enforcement primitives already exist
//! ([`StrictTenantContext::evaluate_access`], delegation projection, approval
//! receipts, MCP discovery filtering). What was missing is a single trait the
//! four governed surfaces — resource access, context projection, tool
//! discovery, and delegation issuance — can all adopt so policy is applied
//! the same way everywhere and a hosted/enterprise outage fails closed while
//! local/single-tenant stays a no-op.
//!
//! [`EnterpriseAuthorizationHook`] is that seam. The default
//! [`StrictContextAuthorizationHook`] delegates to the existing primitives:
//! when a strict tenant projection is present every decision flows through the
//! fail-closed `evaluate_access`; when it is absent (local/single-tenant) the
//! hook allows, preserving today's behavior.

use crate::{
    AccessDecision, AccessPermission, DataClass, DelegationProjection, GrantEvaluation,
    ResourceRef, StrictTenantContext,
};

/// A tool's authorization requirements, expressed in primitives so this crate
/// need not depend on `tandem-types`' `ToolSecurityDescriptor`. The call site
/// (tandem-core / tandem-server) maps a descriptor into this and redacts the
/// tool from the provider schema when the hook denies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAccessTarget {
    /// The tool's resource reference (e.g. `ResourceKind::McpTool`).
    pub resource: ResourceRef,
    /// Permissions the tool requires; empty means "no special requirement".
    pub required_permissions: Vec<AccessPermission>,
    /// Data classes the tool touches; empty defaults to `Internal`.
    pub required_data_classes: Vec<DataClass>,
}

/// The unified authorization seam. Every method returns a [`GrantEvaluation`]
/// whose `decision` callers MUST treat fail-closed: only [`AccessDecision::Allow`]
/// permits the action; both `Deny` and `NotApplicable` are hard blocks.
pub trait EnterpriseAuthorizationHook: Send + Sync {
    /// May `permission` be exercised on `resource` at `data_class`?
    fn authorize_resource_access(
        &self,
        resource: &ResourceRef,
        permission: AccessPermission,
        data_class: DataClass,
        now_ms: u64,
    ) -> GrantEvaluation;

    /// May `resource` (at `data_class`) be projected into prompt context,
    /// semantic search, memory retrieval, or artifact assembly? Read intent.
    fn authorize_context_projection(
        &self,
        resource: &ResourceRef,
        data_class: DataClass,
        now_ms: u64,
    ) -> GrantEvaluation {
        self.authorize_resource_access(resource, AccessPermission::Read, data_class, now_ms)
    }

    /// May `tool` be surfaced to the model (discovery) / invoked? Allowed only
    /// when every required (permission × data class) on the tool resource is
    /// authorized.
    fn authorize_tool_discovery(&self, tool: &ToolAccessTarget, now_ms: u64) -> GrantEvaluation;

    /// May this delegation be issued — i.e. does it strictly narrow the
    /// delegator's authority? Allowed iff the projection validates against the
    /// delegator's strict context.
    fn authorize_delegation(
        &self,
        delegation: &DelegationProjection,
        now_ms: u64,
    ) -> GrantEvaluation;
}

/// Default seam backed by an optional strict tenant projection.
///
/// - `None` (local / single-tenant, no strict context): every surface allows,
///   preserving pre-enterprise behavior.
/// - `Some(strict)`: every surface flows through the strict context's
///   fail-closed primitives.
#[derive(Debug, Clone, Default)]
pub struct StrictContextAuthorizationHook {
    strict: Option<StrictTenantContext>,
}

impl StrictContextAuthorizationHook {
    /// Local/single-tenant hook: no strict context, so everything is allowed.
    pub fn local() -> Self {
        Self { strict: None }
    }

    /// Hook bound to a verified strict tenant projection.
    pub fn strict(context: StrictTenantContext) -> Self {
        Self {
            strict: Some(context),
        }
    }

    /// Build from an optional projection (e.g. a request's
    /// `verified_tenant_context.strict_projection`).
    pub fn from_optional(context: Option<StrictTenantContext>) -> Self {
        Self { strict: context }
    }

    pub fn is_strict(&self) -> bool {
        self.strict.is_some()
    }
}

const LOCAL_ALLOW: &str = "local_no_strict_context";

impl EnterpriseAuthorizationHook for StrictContextAuthorizationHook {
    fn authorize_resource_access(
        &self,
        resource: &ResourceRef,
        permission: AccessPermission,
        data_class: DataClass,
        now_ms: u64,
    ) -> GrantEvaluation {
        match &self.strict {
            None => GrantEvaluation::allow(LOCAL_ALLOW),
            Some(strict) => strict.evaluate_access(resource, permission, data_class, now_ms),
        }
    }

    fn authorize_tool_discovery(&self, tool: &ToolAccessTarget, now_ms: u64) -> GrantEvaluation {
        let Some(strict) = &self.strict else {
            return GrantEvaluation::allow(LOCAL_ALLOW);
        };
        let permissions = if tool.required_permissions.is_empty() {
            // A tool with no declared permission requirement still needs to be
            // visible to a principal with at least View on the resource.
            vec![AccessPermission::View]
        } else {
            tool.required_permissions.clone()
        };
        let data_classes = if tool.required_data_classes.is_empty() {
            vec![DataClass::Internal]
        } else {
            tool.required_data_classes.clone()
        };
        // Fail closed: every required (permission × data class) must be allowed.
        for permission in &permissions {
            for data_class in &data_classes {
                let evaluation =
                    strict.evaluate_access(&tool.resource, *permission, *data_class, now_ms);
                if evaluation.decision != AccessDecision::Allow {
                    return evaluation;
                }
            }
        }
        GrantEvaluation::allow("tool_discovery_authorized")
    }

    fn authorize_delegation(
        &self,
        delegation: &DelegationProjection,
        now_ms: u64,
    ) -> GrantEvaluation {
        let Some(strict) = &self.strict else {
            return GrantEvaluation::allow(LOCAL_ALLOW);
        };
        match delegation.project_into_strict_context(strict, now_ms) {
            Ok(_) => GrantEvaluation::allow("delegation_within_parent_scope"),
            Err(reason) => GrantEvaluation::deny(reason, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssertionMetadata, AuthorityChain, CrossTenantGrantParty, DelegationProjectionClaims,
        DelegationProjectionHeader, GrantSource, PrincipalKind, PrincipalRef, RequestPrincipal,
        ResourceKind, ResourceScope, ScopedGrant, TenantContext,
    };

    fn resource(id: &str) -> ResourceRef {
        ResourceRef::new("org-a", "workspace-a", ResourceKind::Document, id)
    }

    fn strict_context() -> StrictTenantContext {
        let principal = PrincipalRef::human_user("user-1");
        let allowed = resource("doc-1");
        let grant = ScopedGrant::new(
            "grant-1",
            principal.clone(),
            allowed.clone(),
            GrantSource::Direct,
        )
        .with_permissions(vec![AccessPermission::Read, AccessPermission::Delegate])
        .with_data_classes(vec![DataClass::Internal]);
        StrictTenantContext::new(
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-1"),
            principal.clone(),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                principal.id,
                "tandem-web",
            )),
            ResourceScope::root(allowed),
            AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999,
                "assert-1",
            ),
        )
        .with_grants(vec![grant])
    }

    #[test]
    fn local_hook_allows_every_surface() {
        let hook = StrictContextAuthorizationHook::local();
        assert!(!hook.is_strict());
        assert_eq!(
            hook.authorize_resource_access(
                &resource("anything"),
                AccessPermission::Admin,
                DataClass::Restricted,
                5_000
            )
            .decision,
            AccessDecision::Allow
        );
        assert_eq!(
            hook.authorize_context_projection(&resource("anything"), DataClass::Credential, 5_000)
                .decision,
            AccessDecision::Allow
        );
        let tool = ToolAccessTarget {
            resource: resource("tool"),
            required_permissions: vec![AccessPermission::Admin],
            required_data_classes: vec![DataClass::Credential],
        };
        assert_eq!(
            hook.authorize_tool_discovery(&tool, 5_000).decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn strict_hook_allows_granted_resource_and_blocks_others() {
        let hook = StrictContextAuthorizationHook::strict(strict_context());
        // Granted (resource × permission × data class).
        assert_eq!(
            hook.authorize_resource_access(
                &resource("doc-1"),
                AccessPermission::Read,
                DataClass::Internal,
                5_000
            )
            .decision,
            AccessDecision::Allow
        );
        // Resource outside scope.
        assert_ne!(
            hook.authorize_resource_access(
                &resource("doc-2"),
                AccessPermission::Read,
                DataClass::Internal,
                5_000
            )
            .decision,
            AccessDecision::Allow
        );
        // Permission not granted.
        assert_ne!(
            hook.authorize_resource_access(
                &resource("doc-1"),
                AccessPermission::Edit,
                DataClass::Internal,
                5_000
            )
            .decision,
            AccessDecision::Allow
        );
        // Data class not granted.
        assert_ne!(
            hook.authorize_resource_access(
                &resource("doc-1"),
                AccessPermission::Read,
                DataClass::Restricted,
                5_000
            )
            .decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn context_projection_uses_read_intent() {
        let hook = StrictContextAuthorizationHook::strict(strict_context());
        assert_eq!(
            hook.authorize_context_projection(&resource("doc-1"), DataClass::Internal, 5_000)
                .decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn tool_discovery_requires_all_permission_class_pairs() {
        let hook = StrictContextAuthorizationHook::strict(strict_context());
        // Read/Internal on doc-1 is granted.
        let visible = ToolAccessTarget {
            resource: resource("doc-1"),
            required_permissions: vec![AccessPermission::Read],
            required_data_classes: vec![DataClass::Internal],
        };
        assert_eq!(
            hook.authorize_tool_discovery(&visible, 5_000).decision,
            AccessDecision::Allow
        );
        // Admin is not granted → redacted from discovery.
        let hidden = ToolAccessTarget {
            resource: resource("doc-1"),
            required_permissions: vec![AccessPermission::Admin],
            required_data_classes: vec![DataClass::Internal],
        };
        assert_ne!(
            hook.authorize_tool_discovery(&hidden, 5_000).decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn expired_strict_context_fails_closed() {
        let hook = StrictContextAuthorizationHook::strict(strict_context());
        // now_ms past the assertion expiry.
        assert_eq!(
            hook.authorize_resource_access(
                &resource("doc-1"),
                AccessPermission::Read,
                DataClass::Internal,
                99_999_999_999
            )
            .decision,
            AccessDecision::Deny
        );
    }

    fn delegation_to(doc: &str, permissions: Vec<AccessPermission>) -> DelegationProjection {
        let claims = DelegationProjectionClaims {
            version: "v1".to_string(),
            delegation_id: "delegation-1".to_string(),
            parent_assertion_id: "assert-1".to_string(),
            tenant: CrossTenantGrantParty {
                organization_id: "org-a".to_string(),
                workspace_id: "workspace-a".to_string(),
                deployment_id: None,
            },
            delegator: PrincipalRef::human_user("user-1"),
            delegate: PrincipalRef::new(PrincipalKind::ExternalDelegate, "a2a-1"),
            resource_scope: ResourceScope::root(resource(doc)),
            permissions,
            data_classes: vec![DataClass::Internal],
            tool_patterns: vec![],
            purpose: "summarize".to_string(),
            nonce: "nonce-1".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 2_000,
            not_before_ms: 2_000,
            expires_at_ms: 5_000,
            max_delegation_depth: None,
        };
        DelegationProjection::new(DelegationProjectionHeader::ed25519("k"), claims, "sig")
    }

    #[test]
    fn delegation_authorized_only_when_it_narrows_parent() {
        let hook = StrictContextAuthorizationHook::strict(strict_context());
        // Narrower: doc-1 with Read (parent holds it).
        assert_eq!(
            hook.authorize_delegation(&delegation_to("doc-1", vec![AccessPermission::Read]), 3_000)
                .decision,
            AccessDecision::Allow
        );
        // Widening: a permission the parent lacks → denied.
        assert_eq!(
            hook.authorize_delegation(
                &delegation_to("doc-1", vec![AccessPermission::Admin]),
                3_000
            )
            .decision,
            AccessDecision::Deny
        );
    }
}
