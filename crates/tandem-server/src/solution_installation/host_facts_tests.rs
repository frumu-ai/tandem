use super::*;
use tandem_enterprise_contract::{
    hosted_policy::HostedPolicyBundle, AuthorityChain, HumanActor, OrganizationUnitState,
    RequestPrincipal, TenantContext, TenantContextAssertionClaims,
};
use tandem_solutions::{
    parse_blueprint, parse_customer_config, prepare_customer_config, CustomerMemorySpace,
    MemorySpace,
};

#[test]
fn department_memory_requires_projected_membership_in_an_active_unit() {
    let now = 1500;
    let bundle = HostedPolicyBundle::from_json(&serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "policy_version": 4,
        "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(1000).unwrap(),
        "users": [{"id": "owner-a", "email": null, "username": null,
            "role": "admin", "capabilities": ["hosted.admin"],
            "is_active": true, "email_verified": true}],
        "org_units": [
            {"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"},
            {"id": "other", "slug": "other", "display_name": "Other", "kind": "department", "state": "active"},
            {"id": "inactive", "slug": "inactive", "display_name": "Inactive", "kind": "department", "state": "archived"}
        ],
        "org_unit_memberships": [{"unit_id": "eng", "user_id": "owner-a"}],
        "deployment_grants": []
    })).unwrap()).unwrap().validate("org-a", "dep-a", now, None).unwrap();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "issuer",
        "runtime",
        1000,
        2000,
        "assertion",
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "owner-a"),
        HumanActor::tandem_user("owner-a"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("owner-a", "fixture")),
        vec!["hosted:role:admin".into()],
    );
    claims.policy_version = Some(4);
    claims.capabilities = vec!["hosted.admin".into()];
    claims.org_units = vec!["eng".into()];
    let mut context: VerifiedTenantContext = claims.into();
    context.strict_projection = Some(bundle.project_identity(&context, now).unwrap());
    let mut units = bundle.registry_projection(now).unwrap().units;
    let mut blueprint = parse_blueprint(include_str!(
        "../../../tandem-solutions/fixtures/company-brain-text/solution.json"
    ))
    .unwrap();
    blueprint
        .memory_spaces
        .insert("shared".into(), MemorySpace::DepartmentShared);
    let mut config = parse_customer_config(include_str!(
        "../../../tandem-solutions/fixtures/company-brain-text/customer-a.yaml"
    ))
    .unwrap();
    config.scope.workspace_id = "dep-a".into();
    config.scope.deployment_id = "dep-a".into();
    let references = std::iter::once(config.profile_ref.clone())
        .chain(config.data_refs.values().cloned())
        .collect();
    let subjects = BTreeSet::from(["owner-a".into()]);
    let projects = BTreeSet::from(["project-a".into()]);
    let connectors = BTreeMap::new();
    for unit_id in ["eng", "other", "inactive"] {
        config.memory_spaces.insert(
            "shared".into(),
            CustomerMemorySpace::DepartmentShared {
                org_unit_id: unit_id.into(),
            },
        );
        let approved = approved_org_units(&context, &units);
        assert_eq!(approved, BTreeSet::from(["eng".into()]));
        let result = prepare_customer_config(
            &blueprint,
            &config,
            CustomerConfigInput {
                verified_context: &context,
                selected_scope: &config.scope,
                now_ms: now,
                current_revision: None,
                expected_revision: None,
                host_policy: &blueprint.constraints,
                approved_references: &references,
                approved_connectors: &connectors,
                approved_subjects: &subjects,
                approved_org_units: &approved,
                approved_projects: &projects,
            },
        );
        assert_eq!(result.is_ok(), unit_id == "eng", "{unit_id}");
    }
    // Even a still-claimed membership must not approve an inactive unit.
    units
        .iter_mut()
        .find(|unit| unit.unit_id == "eng")
        .unwrap()
        .state = OrganizationUnitState::Disabled;
    assert!(approved_org_units(&context, &units).is_empty());
}
