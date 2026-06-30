use super::*;
use tandem_types::{
    AssertionMetadata, AuthorityChain, DataBoundary, GrantSource, HumanActor, ResourceKind,
    ResourceRef, ScopedGrant, StrictTenantContext,
};

fn verified_with_strict_grant(
    permissions: Vec<AccessPermission>,
    data_classes: Vec<DataClass>,
) -> (VerifiedTenantContext, ResourceScope) {
    let tenant_context =
        TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
    let principal = PrincipalRef::human_user("actor-a");
    let request_principal = RequestPrincipal::authenticated_user("actor-a", "tandem-web");
    let authority_chain = AuthorityChain::from_request(request_principal);
    let resource = ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::Project,
        "automation-project",
    );
    let scope = ResourceScope::root(resource.clone());
    let grant = ScopedGrant::new(
        "grant-webhook-scope",
        principal.clone(),
        resource,
        GrantSource::Delegation,
    )
    .with_permissions(permissions)
    .with_data_classes(data_classes.clone());
    let strict_projection = StrictTenantContext::new(
        tenant_context.clone(),
        principal,
        authority_chain.clone(),
        scope.clone(),
        AssertionMetadata::new(
            "tandem-web",
            "tandem-runtime",
            1_000,
            9_999_999_999_999,
            "assertion-webhook-scope",
        ),
    )
    .with_grants(vec![grant])
    .with_data_boundary(DataBoundary::allow(data_classes));
    let verified = VerifiedTenantContext {
        tenant_context,
        human_actor: HumanActor::tandem_user("actor-a"),
        authority_chain,
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: Some(strict_projection),
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: "assertion-webhook-scope".to_string(),
        assertion_key_id: None,
    };
    (verified, scope)
}

#[test]
fn strict_scope_allows_requires_matching_permission_grant() {
    let (viewer, scope) =
        verified_with_strict_grant(vec![AccessPermission::View], vec![DataClass::Internal]);
    assert!(strict_scope_allows(
        &viewer,
        &scope,
        AccessPermission::View,
        DataClass::Internal,
    ));
    assert!(!strict_scope_allows(
        &viewer,
        &scope,
        AccessPermission::Edit,
        DataClass::Internal,
    ));
    assert!(!strict_scope_allows(
        &viewer,
        &scope,
        AccessPermission::Admin,
        DataClass::Internal,
    ));

    let (admin, scope) =
        verified_with_strict_grant(vec![AccessPermission::Admin], vec![DataClass::Internal]);
    assert!(strict_scope_allows(
        &admin,
        &scope,
        AccessPermission::Edit,
        DataClass::Internal,
    ));
    assert!(strict_scope_allows(
        &admin,
        &scope,
        AccessPermission::Admin,
        DataClass::Internal,
    ));
}

#[test]
fn strict_scope_allows_requires_matching_data_class() {
    let (verified, scope) =
        verified_with_strict_grant(vec![AccessPermission::Admin], vec![DataClass::Internal]);
    assert!(!strict_scope_allows(
        &verified,
        &scope,
        AccessPermission::Admin,
        DataClass::Confidential,
    ));
}
