use serde::{Deserialize, Serialize};

use crate::{
    AccessEffect, AccessPermission, DataClass, GrantSource, PrincipalRef, ResourceRef,
    ResourceScope, ScopedGrant, StrictTenantContext, TenantContext,
};

pub const CROSS_TENANT_GRANT_TYP: &str = "tandem-cross-tenant-grant+jws";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrantParty {
    pub organization_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
}

impl CrossTenantGrantParty {
    pub fn from_tenant_context(tenant_context: &TenantContext) -> Self {
        Self {
            organization_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        }
    }

    pub fn matches_tenant_context(&self, tenant_context: &TenantContext) -> bool {
        self.organization_id == tenant_context.org_id
            && self.workspace_id == tenant_context.workspace_id
            && self.deployment_id == tenant_context.deployment_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrantHeader {
    pub alg: String,
    pub typ: String,
    pub kid: String,
}

impl CrossTenantGrantHeader {
    pub fn ed25519(key_id: impl Into<String>) -> Self {
        Self {
            alg: "EdDSA".to_string(),
            typ: CROSS_TENANT_GRANT_TYP.to_string(),
            kid: key_id.into(),
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.alg == "EdDSA" && self.typ == CROSS_TENANT_GRANT_TYP && !self.kid.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrantClaims {
    pub version: String,
    pub grant_id: String,
    pub issuer: CrossTenantGrantParty,
    pub audience: CrossTenantGrantParty,
    pub subject: PrincipalRef,
    pub resource_scope: ResourceScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_patterns: Vec<String>,
    pub issued_at_ms: u64,
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    pub issued_by: PrincipalRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_audit_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

impl CrossTenantGrantClaims {
    #[allow(clippy::too_many_arguments)]
    pub fn new_v1(
        grant_id: impl Into<String>,
        issuer: CrossTenantGrantParty,
        audience: CrossTenantGrantParty,
        subject: PrincipalRef,
        resource_scope: ResourceScope,
        permissions: Vec<AccessPermission>,
        data_classes: Vec<DataClass>,
        issued_at_ms: u64,
        expires_at_ms: u64,
        issued_by: PrincipalRef,
    ) -> Self {
        Self {
            version: "v1".to_string(),
            grant_id: grant_id.into(),
            issuer,
            audience,
            subject,
            resource_scope,
            permissions,
            data_classes,
            tool_patterns: Vec::new(),
            issued_at_ms,
            not_before_ms: issued_at_ms,
            expires_at_ms,
            issued_by,
            source_policy_decision_id: None,
            source_audit_event_id: None,
            approval_id: None,
        }
    }

    pub fn issuer_owns_scope(&self) -> bool {
        self.issuer_owns_resource(&self.resource_scope.root)
            && self
                .resource_scope
                .allowed_resources
                .iter()
                .all(|resource| self.issuer_owns_resource(resource))
            && self
                .resource_scope
                .denied_resources
                .iter()
                .all(|resource| self.issuer_owns_resource(resource))
    }

    fn issuer_owns_resource(&self, resource: &ResourceRef) -> bool {
        resource.organization_id == self.issuer.organization_id
            && (resource.workspace_id == self.issuer.workspace_id || resource.workspace_id == "*")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrant {
    pub header: CrossTenantGrantHeader,
    pub claims: CrossTenantGrantClaims,
    pub signature: String,
}

impl CrossTenantGrant {
    pub fn new(
        header: CrossTenantGrantHeader,
        claims: CrossTenantGrantClaims,
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
            && !self.claims.grant_id.trim().is_empty()
            && self.claims.expires_at_ms > self.claims.not_before_ms
            && !self.claims.permissions.is_empty()
            && !self.claims.data_classes.is_empty()
            && self.claims.issuer_owns_scope()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrossTenantGrantState {
    #[default]
    Active,
    Suspended,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrantRevocation {
    pub revoked_at_ms: u64,
    pub revoked_by: PrincipalRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_audit_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossTenantGrantRecord {
    pub grant: CrossTenantGrant,
    #[serde(default)]
    pub state: CrossTenantGrantState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation: Option<CrossTenantGrantRevocation>,
}

impl CrossTenantGrantRecord {
    pub fn active(grant: CrossTenantGrant, now_ms: u64) -> Self {
        Self {
            grant,
            state: CrossTenantGrantState::Active,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            revocation: None,
        }
    }

    pub fn revoke(
        &mut self,
        revoked_at_ms: u64,
        revoked_by: PrincipalRef,
        reason: Option<String>,
        source_policy_decision_id: Option<String>,
        source_audit_event_id: Option<String>,
    ) {
        self.state = CrossTenantGrantState::Revoked;
        self.updated_at_ms = revoked_at_ms;
        self.revocation = Some(CrossTenantGrantRevocation {
            revoked_at_ms,
            revoked_by,
            reason,
            source_policy_decision_id,
            source_audit_event_id,
        });
    }

    pub fn denial_reason_for_audience(
        &self,
        audience_tenant: &TenantContext,
        subject: &PrincipalRef,
        now_ms: u64,
    ) -> Option<&'static str> {
        if !self.grant.is_well_formed() {
            return Some("cross_tenant_grant_malformed");
        }
        if self.state != CrossTenantGrantState::Active {
            return Some("cross_tenant_grant_inactive");
        }
        if !self
            .grant
            .claims
            .audience
            .matches_tenant_context(audience_tenant)
        {
            return Some("cross_tenant_grant_audience_mismatch");
        }
        if !principal_matches(&self.grant.claims.subject, subject) {
            return Some("cross_tenant_grant_subject_mismatch");
        }
        if now_ms < self.grant.claims.not_before_ms {
            return Some("cross_tenant_grant_not_yet_valid");
        }
        if now_ms >= self.grant.claims.expires_at_ms {
            return Some("cross_tenant_grant_expired");
        }
        None
    }

    pub fn project_scoped_grants_for_audience(
        &self,
        audience_tenant: &TenantContext,
        subject: &PrincipalRef,
        now_ms: u64,
    ) -> Vec<ScopedGrant> {
        if self
            .denial_reason_for_audience(audience_tenant, subject, now_ms)
            .is_some()
        {
            return Vec::new();
        }

        let mut resources = vec![self.grant.claims.resource_scope.root.clone()];
        for resource in &self.grant.claims.resource_scope.allowed_resources {
            if !resources.contains(resource) {
                resources.push(resource.clone());
            }
        }

        resources
            .into_iter()
            .enumerate()
            .map(|(idx, resource)| {
                let grant_id = if idx == 0 {
                    self.grant.claims.grant_id.clone()
                } else {
                    format!("{}::scope-{idx}", self.grant.claims.grant_id)
                };
                ScopedGrant::new(
                    grant_id,
                    self.grant.claims.subject.clone(),
                    resource,
                    GrantSource::CrossTenantGrant,
                )
                .with_effect(AccessEffect::Allow)
                .with_permissions(self.grant.claims.permissions.clone())
                .with_data_classes(self.grant.claims.data_classes.clone())
                .with_tool_patterns(self.grant.claims.tool_patterns.clone())
                .with_source_principal(self.grant.claims.issued_by.clone())
                .with_expires_at_ms(self.grant.claims.expires_at_ms)
                .with_delegation_id(self.grant.claims.grant_id.clone())
            })
            .collect()
    }

    pub fn project_into_strict_context(
        &self,
        strict_context: &mut StrictTenantContext,
        now_ms: u64,
    ) -> bool {
        let grants = self.project_scoped_grants_for_audience(
            &strict_context.tenant_context,
            &strict_context.principal,
            now_ms,
        );
        if grants.is_empty() {
            return false;
        }

        merge_cross_tenant_resource_scope(
            &mut strict_context.resource_scope,
            &self.grant.claims.resource_scope,
        );
        for grant in grants {
            if !strict_context
                .grants
                .iter()
                .any(|existing| existing.grant_id == grant.grant_id)
            {
                strict_context.grants.push(grant);
            }
        }
        true
    }
}

pub fn merge_cross_tenant_resource_scope(target: &mut ResourceScope, inbound: &ResourceScope) {
    push_unique_resource(&mut target.allowed_resources, inbound.root.clone());
    for resource in &inbound.allowed_resources {
        push_unique_resource(&mut target.allowed_resources, resource.clone());
    }
    for resource in &inbound.denied_resources {
        push_unique_resource(&mut target.denied_resources, resource.clone());
    }
}

fn push_unique_resource(resources: &mut Vec<ResourceRef>, resource: ResourceRef) {
    if !resources.contains(&resource) {
        resources.push(resource);
    }
}

fn principal_matches(expected: &PrincipalRef, actual: &PrincipalRef) -> bool {
    expected.kind == actual.kind && expected.id == actual.id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessDecision, AssertionMetadata, AuthorityChain, DataBoundary, RequestPrincipal,
        ResourceKind,
    };

    #[test]
    fn active_inbound_grant_projects_into_strict_context() {
        let issuer =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let audience =
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b");
        let subject = PrincipalRef::human_user("user-b");
        let resource = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::DocumentCollection,
            "finance-drive",
        );
        let claims = CrossTenantGrantClaims::new_v1(
            "grant-finance",
            CrossTenantGrantParty::from_tenant_context(&issuer),
            CrossTenantGrantParty::from_tenant_context(&audience),
            subject.clone(),
            ResourceScope::root(resource.clone()),
            vec![AccessPermission::Read],
            vec![DataClass::FinancialRecord],
            1_000,
            5_000,
            PrincipalRef::human_user("admin-a"),
        );
        let record = CrossTenantGrantRecord::active(
            CrossTenantGrant::new(
                CrossTenantGrantHeader::ed25519("grant-key"),
                claims,
                "signature-bytes",
            ),
            1_000,
        );
        let request_principal = RequestPrincipal::authenticated_user("user-b", "test");
        let mut strict = StrictTenantContext::new(
            audience,
            subject,
            AuthorityChain::from_request(request_principal),
            ResourceScope::root(ResourceRef::new(
                "org-b",
                "workspace-b",
                ResourceKind::Workspace,
                "workspace-b",
            )),
            AssertionMetadata::new("test", "runtime", 1_000, 5_000, "assertion-b"),
        )
        .with_data_boundary(DataBoundary::allow(vec![DataClass::FinancialRecord]));

        assert!(record.project_into_strict_context(&mut strict, 2_000));
        let decision = strict.evaluate_access(
            &resource,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            2_000,
        );

        assert_eq!(decision.decision, AccessDecision::Allow);
        assert_eq!(decision.grant_id.as_deref(), Some("grant-finance"));
        assert!(strict
            .grants
            .iter()
            .any(|grant| grant.grant_source == GrantSource::CrossTenantGrant));
    }

    #[test]
    fn revoked_or_wrong_audience_grants_fail_closed() {
        let issuer =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let audience =
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b");
        let resource = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::DocumentCollection,
            "finance-drive",
        );
        let claims = CrossTenantGrantClaims::new_v1(
            "grant-finance",
            CrossTenantGrantParty::from_tenant_context(&issuer),
            CrossTenantGrantParty::from_tenant_context(&audience),
            PrincipalRef::human_user("user-b"),
            ResourceScope::root(resource),
            vec![AccessPermission::Read],
            vec![DataClass::FinancialRecord],
            1_000,
            5_000,
            PrincipalRef::human_user("admin-a"),
        );
        let mut record = CrossTenantGrantRecord::active(
            CrossTenantGrant::new(
                CrossTenantGrantHeader::ed25519("grant-key"),
                claims,
                "signature-bytes",
            ),
            1_000,
        );

        record.revoke(2_000, PrincipalRef::human_user("admin-a"), None, None, None);

        assert_eq!(
            record.denial_reason_for_audience(
                &audience,
                &PrincipalRef::human_user("user-b"),
                2_500
            ),
            Some("cross_tenant_grant_inactive")
        );
        assert_eq!(
            record.denial_reason_for_audience(
                &TenantContext::explicit_user_workspace("org-c", "workspace-c", None, "user-b"),
                &PrincipalRef::human_user("user-b"),
                1_500
            ),
            Some("cross_tenant_grant_inactive")
        );
    }
}
