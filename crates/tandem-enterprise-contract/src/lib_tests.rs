#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ref_validation_rejects_cross_tenant_access() {
        let secret_ref = SecretRef {
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            provider: "mcp_header".to_string(),
            secret_id: "secret-a".to_string(),
            name: "authorization".to_string(),
        };
        let tenant = TenantContext::explicit("org-a", "workspace-a", None);
        assert!(secret_ref.validate_for_tenant(&tenant).is_ok());

        let wrong_workspace = TenantContext::explicit("org-a", "workspace-b", None);
        assert!(matches!(
            secret_ref.validate_for_tenant(&wrong_workspace),
            Err(SecretRefError::WorkspaceMismatch)
        ));
    }

    #[test]
    fn explicit_user_workspace_preserves_actor_and_deployment() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );

        assert_eq!(tenant.org_id, "org-a");
        assert_eq!(tenant.workspace_id, "workspace-a");
        assert_eq!(tenant.deployment_id.as_deref(), Some("deployment-a"));
        assert_eq!(tenant.actor_id.as_deref(), Some("user-a"));
        assert_eq!(tenant.source, TenantSource::Explicit);
        assert!(!tenant.is_local_implicit());
    }

    #[test]
    fn authority_chain_from_request_executes_as_same_actor() {
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let chain = AuthorityChain::from_request(principal.clone());

        assert_eq!(chain.initiated_by, principal);
        assert!(chain.owned_by.is_none());
        assert!(chain.approved_by.is_none());
        assert_eq!(chain.executed_as, ExecutionPrincipal::Request(principal));
    }

    #[test]
    fn verified_tenant_context_checks_expiry_and_tenant_match() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );
        let actor = HumanActor::tandem_user("user-a");
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let verified = VerifiedTenantContext {
            tenant_context: tenant.clone(),
            human_actor: actor,
            authority_chain: AuthorityChain::from_request(principal),
            roles: vec!["owner".to_string()],
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 100,
            expires_at_ms: 200,
            assertion_id: "assertion-1".to_string(),
        };

        assert!(!verified.is_expired_at(199));
        assert!(verified.is_expired_at(200));
        assert!(verified.tenant_matches(&tenant));
        assert!(!verified.tenant_matches(&TenantContext::explicit(
            "org-b",
            "workspace-a",
            Some("user-a".to_string()),
        )));
    }

    #[test]
    fn runtime_auth_mode_parses_operator_aliases() {
        assert_eq!(
            RuntimeAuthMode::parse("local"),
            Ok(RuntimeAuthMode::LocalSingleTenant)
        );
        assert_eq!(
            RuntimeAuthMode::parse("hosted-single-tenant"),
            Ok(RuntimeAuthMode::HostedSingleTenant)
        );
        assert_eq!(
            RuntimeAuthMode::parse("enterprise_required"),
            Ok(RuntimeAuthMode::EnterpriseRequired)
        );
        assert!(RuntimeAuthMode::parse("definitely-not-a-mode").is_err());
        assert_eq!(
            RuntimeAuthMode::EnterpriseRequired.to_string(),
            "enterprise_required"
        );
    }

    #[test]
    fn tenant_context_assertion_claims_convert_to_verified_context() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );
        let actor = HumanActor::tandem_user("user-a");
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem_web");
        let chain = AuthorityChain::from_request(principal);
        let claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            100,
            200,
            "assertion-1",
            tenant.clone(),
            actor.clone(),
            chain.clone(),
            vec!["operator".to_string(), "approver".to_string()],
        );

        assert_eq!(claims.version, "v1");
        assert!(!claims.is_expired_at(199));
        assert!(claims.is_expired_at(200));

        let verified = VerifiedTenantContext::from(claims);
        assert_eq!(verified.tenant_context, tenant);
        assert_eq!(verified.human_actor, actor);
        assert_eq!(verified.authority_chain, chain);
        assert_eq!(verified.roles, vec!["operator", "approver"]);
        assert_eq!(verified.issuer, "tandem-web");
        assert_eq!(verified.audience, "tandem-runtime");
        assert_eq!(verified.assertion_id, "assertion-1");
    }

    #[test]
    fn tenant_context_assertion_claims_can_carry_strict_projection() {
        let tenant = TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            Some("deployment-prod".to_string()),
            "user-eng",
        );
        let actor = HumanActor::tandem_user("user-eng");
        let request_principal = RequestPrincipal::authenticated_user("user-eng", "tandem-web");
        let authority_chain = AuthorityChain::from_request(request_principal);
        let principal =
            PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-eng");
        let project = ResourceRef::new("acme", "engineering", ResourceKind::Project, "platform");
        let repo = ResourceRef::new("acme", "engineering", ResourceKind::Repository, "tandem")
            .with_project_id("platform")
            .with_path_prefix("crates/tandem-enterprise-contract/");
        let grant = ScopedGrant::new(
            "grant-platform-read",
            principal.clone(),
            repo.clone(),
            GrantSource::Delegation,
        )
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::SourceCode])
        .with_delegation_id("delegation-platform");

        let claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            1_000,
            2_000,
            "assertion-platform",
            tenant.clone(),
            actor,
            authority_chain,
            vec![],
        )
        .with_strict_projection(
            principal.clone(),
            ResourceScope {
                root: project,
                allowed_resources: vec![repo.clone()],
                denied_resources: Vec::new(),
                max_depth: Some(4),
            },
            vec![grant],
            DataBoundary::allow(vec![DataClass::SourceCode]),
        );

        assert!(claims.has_strict_projection());
        let encoded = serde_json::to_value(&claims).expect("serialize projected claims");
        assert_eq!(encoded["principal"]["kind"], "agent_worker");
        assert_eq!(
            encoded["resource_scope"]["allowed_resources"][0]["resource_kind"],
            "repository"
        );
        assert_eq!(encoded["grants"][0]["delegation_id"], "delegation-platform");

        let decoded: TenantContextAssertionClaims =
            serde_json::from_value(encoded).expect("deserialize projected claims");
        let strict = decoded
            .strict_projection()
            .expect("strict projection should be present");
        assert_eq!(strict.tenant_context, tenant);
        assert_eq!(strict.principal, principal);
        assert_eq!(strict.grants[0].grant_id, "grant-platform-read");
        assert_eq!(strict.assertion.assertion_id, "assertion-platform");
        assert!(strict.allows_data_class(DataClass::SourceCode));
        assert!(!strict.allows_data_class(DataClass::Executive));
    }

    #[test]
    fn tenant_context_assertion_claims_remain_backward_compatible_without_projection() {
        let legacy = serde_json::json!({
            "version": "v1",
            "issuer": "tandem-web",
            "audience": "tandem-runtime",
            "issued_at_ms": 1000,
            "expires_at_ms": 2000,
            "assertion_id": "assertion-legacy",
            "tenant_context": {
                "org_id": "acme",
                "workspace_id": "engineering",
                "deployment_id": "deployment-prod",
                "actor_id": "user-eng",
                "source": "explicit"
            },
            "human_actor": {
                "actor_id": "user-eng",
                "provider": "tandem"
            },
            "authority_chain": {
                "initiated_by": {
                    "actor_id": "user-eng",
                    "source": "tandem-web"
                },
                "executed_as": {
                    "kind": "request",
                    "actor_id": "user-eng",
                    "source": "tandem-web"
                }
            }
        });

        let claims: TenantContextAssertionClaims =
            serde_json::from_value(legacy).expect("legacy claims should deserialize");
        assert!(!claims.has_strict_projection());
        assert!(claims.strict_projection().is_none());
        assert!(claims.grants.is_empty());
        assert!(claims.data_boundary.is_none());
    }

    #[test]
    fn tenant_context_assertion_header_uses_eddsa_jws_typ() {
        let header = TenantContextAssertionHeader::ed25519("key-1");
        assert_eq!(header.alg, "EdDSA");
        assert_eq!(header.typ, "tandem-tenant-context+jws");
        assert_eq!(header.kid, "key-1");
    }

    #[test]
    fn header_resolver_defaults_to_local_tenant() {
        let resolver = HeaderTenantContextResolver;
        let tenant = resolver.resolve_tenant_context(None, None, None);
        assert!(tenant.is_local_implicit());
    }

    #[test]
    fn request_authorization_hook_is_noop_by_default() {
        let hook = NoopRequestAuthorizationHook;
        let principal = RequestPrincipal::anonymous();
        let tenant = TenantContext::local_implicit();
        assert!(hook.authorize(&principal, &tenant));
    }

    #[test]
    fn resource_ref_round_trips_finance_workspace_data_store() {
        let resource =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger")
                .with_parent_path(vec![
                    ResourcePathSegment::named(ResourceKind::Department, "finance", "Finance"),
                    ResourcePathSegment::named(
                        ResourceKind::SharedDrive,
                        "finance-drive",
                        "Finance",
                    ),
                ]);

        let encoded = serde_json::to_string(&resource).expect("serialize resource ref");
        assert!(encoded.contains("\"resource_kind\":\"data_store\""));

        let decoded: ResourceRef =
            serde_json::from_str(&encoded).expect("deserialize resource ref");
        assert_eq!(decoded, resource);
        assert_eq!(decoded.organization_id, "acme");
        assert_eq!(decoded.workspace_id, "finance");
        assert_eq!(decoded.resource_kind, ResourceKind::DataStore);
    }

    #[test]
    fn resource_scope_models_engineering_repo_path_scope() {
        let repository =
            ResourceRef::new("acme", "engineering", ResourceKind::Repository, "tandem")
                .with_project_id("platform")
                .with_branch_id("main")
                .with_path_prefix("crates/tandem-enterprise-contract/");

        let scope = ResourceScope {
            root: ResourceRef::new("acme", "engineering", ResourceKind::Project, "platform"),
            allowed_resources: vec![repository.clone()],
            denied_resources: vec![ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Directory,
                "secrets",
            )
            .with_project_id("platform")
            .with_path_prefix("crates/tandem-enterprise-contract/secrets/")],
            max_depth: Some(4),
        };

        let encoded = serde_json::to_value(&scope).expect("serialize resource scope");
        assert_eq!(
            encoded["allowed_resources"][0]["resource_kind"],
            "repository"
        );
        assert_eq!(
            encoded["allowed_resources"][0]["path_prefix"],
            "crates/tandem-enterprise-contract/"
        );

        let decoded: ResourceScope =
            serde_json::from_value(encoded).expect("deserialize resource scope");
        assert_eq!(decoded, scope);
        assert_eq!(decoded.allowed_resources, vec![repository]);
    }

    #[test]
    fn resource_scope_models_ceo_org_wide_executive_access() {
        let principal = PrincipalRef::human_user("ceo-user")
            .with_tenant_actor_id("user-ceo")
            .with_issuer_subject("https://idp.acme.example", "00uceo");
        let scope = ResourceScope::root(ResourceRef::new(
            "acme",
            "*",
            ResourceKind::Organization,
            "acme",
        ));

        assert_eq!(principal.kind, PrincipalKind::HumanUser);
        assert_eq!(principal.tenant_actor_id.as_deref(), Some("user-ceo"));
        assert_eq!(scope.root.resource_kind, ResourceKind::Organization);
        assert_eq!(scope.root.workspace_id, "*");
        assert!(scope.allowed_resources.is_empty());

        let encoded = serde_json::to_string(&DataClass::Executive).expect("serialize data class");
        assert_eq!(encoded, "\"executive\"");
    }

    #[test]
    fn mcp_tool_resource_target_and_permissions_are_transport_safe() {
        let tool = ResourceRef::new(
            "acme",
            "security",
            ResourceKind::McpTool,
            "mcp:google-drive:files.export",
        )
        .with_parent_path(vec![
            ResourcePathSegment::new(ResourceKind::McpServer, "google-drive"),
            ResourcePathSegment::new(ResourceKind::DataStore, "security-drive"),
        ]);
        let permissions = vec![AccessPermission::View, AccessPermission::Execute];
        let data_classes = vec![DataClass::Confidential, DataClass::Credential];
        let worker = PrincipalRef::agent_worker("agent-security-export");

        let payload = serde_json::json!({
            "principal": worker,
            "resource": tool,
            "permissions": permissions,
            "data_classes": data_classes,
        });

        assert_eq!(payload["principal"]["kind"], "agent_worker");
        assert_eq!(payload["resource"]["resource_kind"], "mcp_tool");
        assert_eq!(payload["permissions"][1], "execute");
        assert_eq!(payload["data_classes"][1], "credential");
    }

    #[test]
    fn scoped_grant_models_department_membership_data_access() {
        let finance_department = PrincipalRef::new(PrincipalKind::Department, "finance");
        let finance_user =
            PrincipalRef::human_user("user-finance").with_tenant_actor_id("actor-finance");
        let finance_store =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger");
        let grant = ScopedGrant::new(
            "grant-finance-ledger-read",
            finance_user,
            finance_store,
            GrantSource::DepartmentMembership,
        )
        .with_source_principal(finance_department)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord, DataClass::Confidential]);

        assert_eq!(grant.grant_source, GrantSource::DepartmentMembership);
        assert!(grant.has_permission(AccessPermission::Read));
        assert!(!grant.has_permission(AccessPermission::Edit));
        assert!(grant.allows_data_class(DataClass::FinancialRecord));
        assert!(!grant.allows_data_class(DataClass::Executive));

        let encoded = serde_json::to_value(&grant).expect("serialize department grant");
        assert_eq!(encoded["grant_source"], "department_membership");
        assert_eq!(encoded["source_principal"]["kind"], "department");
    }

    #[test]
    fn scoped_grant_models_cross_functional_group_access() {
        let launch_group = PrincipalRef::new(PrincipalKind::Group, "launch-team");
        let marketer = PrincipalRef::human_user("user-marketing");
        let launch_room = ResourceRef::new("acme", "gtm", ResourceKind::DataRoom, "q4-launch-room");
        let grant = ScopedGrant::new(
            "grant-launch-room-edit",
            marketer,
            launch_room,
            GrantSource::GroupMembership,
        )
        .with_source_principal(launch_group)
        .with_permissions(vec![
            AccessPermission::View,
            AccessPermission::Read,
            AccessPermission::Edit,
        ])
        .with_data_classes(vec![DataClass::Internal, DataClass::CustomerData]);

        let decoded: ScopedGrant =
            serde_json::from_value(serde_json::to_value(&grant).expect("serialize group grant"))
                .expect("deserialize group grant");
        assert_eq!(decoded, grant);
        assert_eq!(decoded.grant_source, GrantSource::GroupMembership);
        assert!(decoded.has_permission(AccessPermission::Edit));
        assert!(decoded.allows_data_class(DataClass::CustomerData));
    }

    #[test]
    fn organization_unit_taxonomy_models_company_specific_domains() {
        let tenant = TenantContext::explicit_user_workspace(
            "clinic-co",
            "care-delivery",
            Some("deployment-prod".to_string()),
            "admin-user",
        );
        let admin = PrincipalRef::human_user("admin-user");
        let doctors = OrganizationUnit::active(
            "doctors",
            tenant.clone(),
            "Doctors",
            OrganizationUnitKind::ClinicalGroup,
            admin.clone(),
            1_000,
        )
        .with_taxonomy_id("clinical_role")
        .with_parent_unit_id("clinical");
        let consultants = OrganizationUnit::active(
            "consultants",
            tenant.clone(),
            "Consultants",
            OrganizationUnitKind::ContractorGroup,
            admin,
            1_000,
        );

        assert_eq!(
            doctors.principal_ref().kind,
            PrincipalKind::OrganizationUnit
        );
        assert_eq!(doctors.principal_ref().id, "clinical_role/doctors");
        assert_eq!(
            doctors.resource_ref().resource_kind,
            ResourceKind::OrganizationUnit
        );
        assert_eq!(doctors.resource_ref().resource_id, "clinical_role/doctors");
        assert_eq!(doctors.parent_unit_id.as_deref(), Some("clinical"));
        assert_eq!(consultants.kind, OrganizationUnitKind::ContractorGroup);

        let encoded = serde_json::to_value(&doctors).expect("serialize organization unit");
        assert_eq!(encoded["taxonomy_id"], "clinical_role");
        assert_eq!(encoded["kind"], "clinical_group");
        assert_eq!(encoded["state"], "active");
        assert_eq!(encoded["unit_id"], "doctors");

        let decoded: OrganizationUnit =
            serde_json::from_value(encoded).expect("deserialize organization unit");
        assert_eq!(decoded, doctors);
    }

    #[test]
    fn organization_unit_membership_feeds_scoped_grants_without_hardcoded_roles() {
        let tenant = TenantContext::explicit_user_workspace(
            "clinic-co",
            "care-delivery",
            Some("deployment-prod".to_string()),
            "doctor-user",
        );
        let doctors = PrincipalRef::organization_unit("clinical_role/doctors");
        let doctor = PrincipalRef::human_user("doctor-user");
        let membership = OrganizationUnitMembership::active(
            "membership-doctor-user",
            tenant,
            doctors.clone(),
            doctor.clone(),
            OrganizationUnitMembershipSource::HostedControlPlane,
            1_000,
        )
        .with_expires_at_ms(2_000);
        let patient_cases = ResourceRef::new(
            "clinic-co",
            "care-delivery",
            ResourceKind::DataStore,
            "patient-cases",
        );
        let grant = ScopedGrant::new(
            "grant-doctors-patient-cases",
            doctor,
            patient_cases.clone(),
            GrantSource::OrganizationUnitMembership,
        )
        .with_source_principal(doctors)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Regulated, DataClass::CustomerData]);

        assert!(membership.is_active_at(1_999));
        assert!(!membership.is_active_at(2_000));
        assert_eq!(grant.grant_source, GrantSource::OrganizationUnitMembership);
        assert_eq!(
            grant.source_principal.as_ref().map(|source| source.kind),
            Some(PrincipalKind::OrganizationUnit)
        );
        assert!(grant.applies_to(
            &patient_cases,
            AccessPermission::Read,
            DataClass::Regulated,
            1_500
        ));

        let encoded = serde_json::to_value(&grant).expect("serialize org unit grant");
        assert_eq!(encoded["grant_source"], "organization_unit_membership");
        assert_eq!(encoded["source_principal"]["kind"], "organization_unit");
    }

    #[test]
    fn scoped_grant_models_explicit_executive_global_access() {
        let ceo = PrincipalRef::human_user("ceo-user").with_tenant_actor_id("actor-ceo");
        let org = ResourceRef::new("acme", "*", ResourceKind::Organization, "acme");
        let grant = ScopedGrant::new("grant-ceo-global", ceo, org, GrantSource::ExecutiveGlobal)
            .with_permissions(vec![
                AccessPermission::View,
                AccessPermission::Read,
                AccessPermission::Admin,
            ])
            .with_data_classes(vec![
                DataClass::Internal,
                DataClass::Confidential,
                DataClass::Restricted,
                DataClass::Executive,
                DataClass::FinancialRecord,
            ]);

        assert_eq!(grant.grant_source, GrantSource::ExecutiveGlobal);
        assert_eq!(grant.resource.resource_kind, ResourceKind::Organization);
        assert_eq!(grant.resource.workspace_id, "*");
        assert!(grant.has_permission(AccessPermission::Admin));
        assert!(grant.allows_data_class(DataClass::Executive));
    }

    #[test]
    fn scoped_grant_models_down_scoped_delegation_with_expiry() {
        let delegate = PrincipalRef::new(PrincipalKind::ExternalDelegate, "vendor-agent")
            .with_issuer_subject("a2a://vendor.example", "vendor-agent-7");
        let delegator = PrincipalRef::human_user("user-legal");
        let contract_branch =
            ResourceRef::new("acme", "legal", ResourceKind::Document, "vendor-contract")
                .with_project_id("vendor-review")
                .with_path_prefix("/contracts/vendor-a/");
        let grant = ScopedGrant::new(
            "grant-vendor-contract-read",
            delegate,
            contract_branch,
            GrantSource::Delegation,
        )
        .with_source_principal(delegator)
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Confidential])
        .with_tool_patterns(vec!["mcp:google-drive:files.get".to_string()])
        .with_delegation_id("delegation-123")
        .with_expires_at_ms(2_000);

        assert_eq!(grant.grant_source, GrantSource::Delegation);
        assert_eq!(grant.delegation_id.as_deref(), Some("delegation-123"));
        assert!(!grant.is_expired_at(1_999));
        assert!(grant.is_expired_at(2_000));
        assert_eq!(grant.tool_patterns, vec!["mcp:google-drive:files.get"]);

        let encoded = serde_json::to_value(&grant).expect("serialize delegation grant");
        assert_eq!(encoded["principal"]["kind"], "external_delegate");
        assert_eq!(encoded["grant_source"], "delegation");
        assert_eq!(encoded["delegation_id"], "delegation-123");
    }

    #[test]
    fn assertion_metadata_derives_from_verified_tenant_context() {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "user-a",
        );
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        let verified = VerifiedTenantContext {
            tenant_context: tenant,
            human_actor: HumanActor::tandem_user("user-a"),
            authority_chain: AuthorityChain::from_request(principal),
            roles: vec!["enterprise:admin".to_string()],
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            assertion_id: "assertion-123".to_string(),
        };

        let metadata = AssertionMetadata::from(&verified);

        assert_eq!(metadata.issuer, "tandem-web");
        assert_eq!(metadata.audience, "tandem-runtime");
        assert_eq!(metadata.assertion_id, "assertion-123");
        assert_eq!(metadata.purpose, Some(SigningKeyPurpose::ContextAssertion));
        assert!(!metadata.is_expired_at(1_999));
        assert!(metadata.is_expired_at(2_000));
    }

    #[test]
    fn signing_key_purpose_defines_enterprise_signing_lanes() {
        let purposes = vec![
            SigningKeyPurpose::ContextAssertion,
            SigningKeyPurpose::ApprovalReceipt,
            SigningKeyPurpose::DelegationProjection,
            SigningKeyPurpose::A2aPeerAssertion,
            SigningKeyPurpose::BreakGlassAdminAssertion,
        ];

        let encoded = serde_json::to_value(&purposes).expect("serialize signing key purposes");

        assert_eq!(
            encoded,
            serde_json::json!([
                "context_assertion",
                "approval_receipt",
                "delegation_projection",
                "a2a_peer_assertion",
                "break_glass_admin_assertion"
            ])
        );
        assert_eq!(
            SigningKeyPurpose::parse("break-glass-admin"),
            Ok(SigningKeyPurpose::BreakGlassAdminAssertion)
        );
        assert!(SigningKeyPurpose::parse("arbitrary_header_key").is_err());
    }

    #[test]
    fn data_boundary_denies_explicitly_blocked_classes() {
        let boundary = DataBoundary {
            allowed_data_classes: vec![DataClass::Internal, DataClass::Executive],
            denied_data_classes: vec![DataClass::Executive],
        };

        assert!(boundary.allows(DataClass::Internal));
        assert!(!boundary.allows(DataClass::Executive));
        assert!(!boundary.allows(DataClass::FinancialRecord));
    }

    #[test]
    fn strict_tenant_context_round_trips_project_scoped_agent_projection() {
        let tenant_context = TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            Some("deployment-prod".to_string()),
            "user-eng",
        );
        let request_principal = RequestPrincipal::authenticated_user("user-eng", "tandem-web");
        let authority_chain = AuthorityChain::from_request(request_principal);
        let agent =
            PrincipalRef::agent_worker("agent-platform-fix").with_tenant_actor_id("user-eng");
        let project = ResourceRef::new("acme", "engineering", ResourceKind::Project, "platform");
        let repository =
            ResourceRef::new("acme", "engineering", ResourceKind::Repository, "tandem")
                .with_project_id("platform")
                .with_path_prefix("crates/tandem-enterprise-contract/");
        let resource_scope = ResourceScope {
            root: project,
            allowed_resources: vec![repository.clone()],
            denied_resources: vec![ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Directory,
                "restricted",
            )
            .with_project_id("platform")
            .with_path_prefix("crates/tandem-enterprise-contract/restricted/")],
            max_depth: Some(5),
        };
        let grant = ScopedGrant::new(
            "grant-agent-platform-edit",
            agent.clone(),
            repository,
            GrantSource::Delegation,
        )
        .with_permissions(vec![
            AccessPermission::View,
            AccessPermission::Read,
            AccessPermission::Edit,
        ])
        .with_data_classes(vec![DataClass::SourceCode, DataClass::Internal])
        .with_delegation_id("delegation-platform-fix")
        .with_expires_at_ms(2_000);
        let context = StrictTenantContext::new(
            tenant_context,
            agent,
            authority_chain,
            resource_scope,
            AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                2_000,
                "assertion-platform-fix",
            )
            .with_key_id("deployment-prod-ctx-2026-05-01")
            .with_purpose(SigningKeyPurpose::ContextAssertion),
        )
        .with_grants(vec![grant])
        .with_data_boundary(DataBoundary::allow(vec![
            DataClass::SourceCode,
            DataClass::Internal,
        ]));

        assert!(context.has_permission(AccessPermission::Edit));
        assert!(!context.has_permission(AccessPermission::Execute));
        assert!(context.allows_data_class(DataClass::SourceCode));
        assert!(!context.allows_data_class(DataClass::Executive));
        assert!(!context.is_expired_at(1_999));
        assert!(context.is_expired_at(2_000));

        let decoded: StrictTenantContext = serde_json::from_value(
            serde_json::to_value(&context).expect("serialize strict context"),
        )
        .expect("deserialize strict context");
        assert_eq!(decoded, context);
        assert_eq!(
            decoded.grants[0].delegation_id.as_deref(),
            Some("delegation-platform-fix")
        );
        assert_eq!(
            decoded.assertion.key_id.as_deref(),
            Some("deployment-prod-ctx-2026-05-01")
        );
    }

    #[test]
    fn grant_evaluation_allows_department_membership_data_access() {
        let finance_store =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger");
        let principal = PrincipalRef::human_user("user-finance");
        let grant = ScopedGrant::new(
            "grant-finance-read",
            principal.clone(),
            ResourceRef::new("acme", "finance", ResourceKind::Department, "finance"),
            GrantSource::DepartmentMembership,
        )
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord]);
        let context = test_strict_context(
            "finance",
            principal,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "finance",
                ResourceKind::Department,
                "finance",
            )),
            vec![grant],
        );

        let evaluation = context.evaluate_access(
            &finance_store,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            1_500,
        );

        assert_eq!(evaluation.decision, AccessDecision::Allow);
        assert_eq!(evaluation.grant_id.as_deref(), Some("grant-finance-read"));
    }

    #[test]
    fn grant_evaluation_deny_wins_over_org_wide_allow() {
        let hr_document =
            ResourceRef::new("acme", "hr", ResourceKind::Document, "compensation-plan");
        let principal = PrincipalRef::human_user("user-exec");
        let org_allow = ScopedGrant::new(
            "grant-org-read",
            principal.clone(),
            ResourceRef::new("acme", "*", ResourceKind::Organization, "acme"),
            GrantSource::ExecutiveGlobal,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Executive]);
        let hr_deny = ScopedGrant::new(
            "deny-hr-comp",
            principal.clone(),
            ResourceRef::new("acme", "hr", ResourceKind::Document, "compensation-plan"),
            GrantSource::Direct,
        )
        .with_effect(AccessEffect::Deny)
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Executive]);
        let context = test_strict_context(
            "*",
            principal,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "*",
                ResourceKind::Organization,
                "acme",
            )),
            vec![org_allow, hr_deny],
        )
        .with_data_boundary(DataBoundary::allow(vec![DataClass::Executive]));

        let evaluation = context.evaluate_access(
            &hr_document,
            AccessPermission::Read,
            DataClass::Executive,
            1_500,
        );

        assert_eq!(evaluation.decision, AccessDecision::Deny);
        assert_eq!(evaluation.grant_id.as_deref(), Some("deny-hr-comp"));
        assert_eq!(evaluation.reason, "matching_deny_grant");
    }

    #[test]
    fn grant_evaluation_project_grant_applies_to_file_path() {
        let principal = PrincipalRef::agent_worker("agent-platform");
        let file = ResourceRef::new(
            "acme",
            "engineering",
            ResourceKind::File,
            "crates/tandem-enterprise-contract/src/lib.rs",
        )
        .with_project_id("platform")
        .with_path_prefix("crates/tandem-enterprise-contract/src/lib.rs");
        let grant = ScopedGrant::new(
            "grant-platform-source-edit",
            principal.clone(),
            ResourceRef::new("acme", "engineering", ResourceKind::Project, "platform"),
            GrantSource::Delegation,
        )
        .with_permissions(vec![AccessPermission::Read, AccessPermission::Edit])
        .with_data_classes(vec![DataClass::SourceCode]);
        let context = test_strict_context(
            "engineering",
            principal,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Project,
                "platform",
            )),
            vec![grant],
        );

        let evaluation =
            context.evaluate_access(&file, AccessPermission::Edit, DataClass::SourceCode, 1_500);

        assert_eq!(evaluation.decision, AccessDecision::Allow);
        assert_eq!(
            evaluation.grant_id.as_deref(),
            Some("grant-platform-source-edit")
        );
    }

    #[test]
    fn grant_evaluation_expired_grant_does_not_apply() {
        let principal = PrincipalRef::human_user("user-finance");
        let finance_store =
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-ledger");
        let grant = ScopedGrant::new(
            "grant-expired-finance",
            principal.clone(),
            finance_store.clone(),
            GrantSource::Direct,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord])
        .with_expires_at_ms(1_400);
        let context = test_strict_context(
            "finance",
            principal,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "finance",
                ResourceKind::Workspace,
                "finance",
            )),
            vec![grant],
        );

        let evaluation = context.evaluate_access(
            &finance_store,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            1_500,
        );

        assert_eq!(evaluation.decision, AccessDecision::NotApplicable);
        assert_eq!(evaluation.reason, "no_matching_allow_grant");
    }

    #[test]
    fn grant_evaluation_delegated_grant_stays_narrower_than_parent_scope() {
        let principal = PrincipalRef::new(PrincipalKind::ExternalDelegate, "vendor-agent");
        let allowed_doc =
            ResourceRef::new("acme", "legal", ResourceKind::Document, "vendor-contract")
                .with_project_id("vendor-review")
                .with_path_prefix("/contracts/vendor-a/");
        let other_doc = ResourceRef::new("acme", "legal", ResourceKind::Document, "board-minutes")
            .with_project_id("vendor-review")
            .with_path_prefix("/executive/board-minutes/");
        let grant = ScopedGrant::new(
            "grant-vendor-contract",
            principal.clone(),
            allowed_doc.clone(),
            GrantSource::Delegation,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Confidential])
        .with_delegation_id("delegation-123");
        let context = test_strict_context(
            "legal",
            principal,
            ResourceScope {
                root: ResourceRef::new("acme", "legal", ResourceKind::Project, "vendor-review"),
                allowed_resources: vec![allowed_doc.clone()],
                denied_resources: vec![ResourceRef::new(
                    "acme",
                    "legal",
                    ResourceKind::Document,
                    "board-minutes",
                )],
                max_depth: Some(2),
            },
            vec![grant],
        );

        let allowed = context.evaluate_access(
            &allowed_doc,
            AccessPermission::Read,
            DataClass::Confidential,
            1_500,
        );
        let denied = context.evaluate_access(
            &other_doc,
            AccessPermission::Read,
            DataClass::Confidential,
            1_500,
        );

        assert_eq!(allowed.decision, AccessDecision::Allow);
        assert_eq!(denied.decision, AccessDecision::Deny);
        assert_eq!(denied.reason, "resource_explicitly_denied_by_scope");
    }

    #[test]
    fn department_grants_do_not_cross_resource_or_data_class_boundaries() {
        let finance_user = PrincipalRef::human_user("user-finance");
        let finance_grant = ScopedGrant::new(
            "grant-finance-ledger-read",
            finance_user.clone(),
            ResourceRef::new("acme", "finance", ResourceKind::Department, "finance"),
            GrantSource::DepartmentMembership,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord]);
        let finance_context = test_strict_context(
            "finance",
            finance_user,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "finance",
                ResourceKind::Department,
                "finance",
            )),
            vec![finance_grant],
        );
        let engineering_repo = ResourceRef::new(
            "acme",
            "engineering",
            ResourceKind::Repository,
            "product-api",
        );
        let finance_denied_engineering = finance_context.evaluate_access(
            &engineering_repo,
            AccessPermission::Read,
            DataClass::SourceCode,
            1_500,
        );

        assert_eq!(
            finance_denied_engineering.decision,
            AccessDecision::NotApplicable
        );
        assert_eq!(
            finance_denied_engineering.reason,
            "resource_outside_projected_scope"
        );

        let engineering_user = PrincipalRef::human_user("user-engineering");
        let engineering_grant = ScopedGrant::new(
            "grant-engineering-source-read",
            engineering_user.clone(),
            ResourceRef::new("acme", "engineering", ResourceKind::Project, "product-api"),
            GrantSource::DepartmentMembership,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::SourceCode]);
        let engineering_context = test_strict_context(
            "engineering",
            engineering_user,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Project,
                "product-api",
            )),
            vec![engineering_grant],
        );
        let hr_compensation =
            ResourceRef::new("acme", "hr", ResourceKind::Document, "compensation-bands");
        let engineering_denied_hr = engineering_context.evaluate_access(
            &hr_compensation,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            1_500,
        );

        assert_eq!(
            engineering_denied_hr.decision,
            AccessDecision::NotApplicable
        );
        assert_eq!(
            engineering_denied_hr.reason,
            "resource_outside_projected_scope"
        );
    }

    #[test]
    fn executive_global_access_is_explicit_and_not_inherited_by_agents() {
        let ceo = PrincipalRef::human_user("ceo-user");
        let executive_grant = ScopedGrant::new(
            "grant-ceo-org-read",
            ceo.clone(),
            ResourceRef::new("acme", "*", ResourceKind::Organization, "acme"),
            GrantSource::ExecutiveGlobal,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![
            DataClass::Executive,
            DataClass::FinancialRecord,
            DataClass::SourceCode,
        ]);
        let ceo_context = test_strict_context(
            "*",
            ceo,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "*",
                ResourceKind::Organization,
                "acme",
            )),
            vec![executive_grant],
        );
        let hr_compensation =
            ResourceRef::new("acme", "hr", ResourceKind::Document, "compensation-bands");
        let engineering_repo = ResourceRef::new(
            "acme",
            "engineering",
            ResourceKind::Repository,
            "product-api",
        );

        assert_eq!(
            ceo_context
                .evaluate_access(
                    &hr_compensation,
                    AccessPermission::Read,
                    DataClass::FinancialRecord,
                    1_500,
                )
                .decision,
            AccessDecision::Allow
        );
        assert_eq!(
            ceo_context
                .evaluate_access(
                    &engineering_repo,
                    AccessPermission::Read,
                    DataClass::SourceCode,
                    1_500,
                )
                .decision,
            AccessDecision::Allow
        );

        let ceo_agent =
            PrincipalRef::agent_worker("agent-ceo-summary").with_tenant_actor_id("ceo-user");
        let narrow_agent_grant = ScopedGrant::new(
            "grant-agent-product-read",
            ceo_agent.clone(),
            ResourceRef::new("acme", "engineering", ResourceKind::Project, "product-api"),
            GrantSource::Delegation,
        )
        .with_source_principal(PrincipalRef::human_user("ceo-user"))
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::SourceCode]);
        let agent_context = test_strict_context(
            "engineering",
            ceo_agent.clone(),
            ResourceScope::root(ResourceRef::new(
                "acme",
                "engineering",
                ResourceKind::Project,
                "product-api",
            )),
            vec![narrow_agent_grant],
        );

        let agent_denied_hr = agent_context.evaluate_access(
            &hr_compensation,
            AccessPermission::Read,
            DataClass::FinancialRecord,
            1_500,
        );
        assert_eq!(agent_denied_hr.decision, AccessDecision::NotApplicable);
        assert_eq!(agent_denied_hr.reason, "resource_outside_projected_scope");

        let projected_agent_grant = ScopedGrant::new(
            "grant-agent-executive-projection",
            ceo_agent.clone(),
            ResourceRef::new("acme", "*", ResourceKind::Organization, "acme"),
            GrantSource::Delegation,
        )
        .with_source_principal(PrincipalRef::human_user("ceo-user"))
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord])
        .with_delegation_id("delegation-ceo-summary");
        let projected_agent_context = test_strict_context(
            "*",
            ceo_agent,
            ResourceScope::root(ResourceRef::new(
                "acme",
                "*",
                ResourceKind::Organization,
                "acme",
            )),
            vec![projected_agent_grant],
        );

        assert_eq!(
            projected_agent_context
                .evaluate_access(
                    &hr_compensation,
                    AccessPermission::Read,
                    DataClass::FinancialRecord,
                    1_500,
                )
                .decision,
            AccessDecision::Allow
        );
    }

    #[test]
    fn connector_credential_ref_defaults_to_read_only_secret_reference() {
        let tenant = TenantContext::explicit_user_workspace(
            "acme",
            "finance",
            Some("deployment-prod".to_string()),
            "user-admin",
        );
        let credential = ConnectorCredentialRef::read_only(
            "acme",
            "finance",
            "google-drive-finance",
            "credential-readonly",
            SecretRef {
                org_id: "acme".to_string(),
                workspace_id: "finance".to_string(),
                provider: "google_kms".to_string(),
                secret_id: "secret://connectors/google-drive-finance/read".to_string(),
                name: "Google Drive read token".to_string(),
            },
            1_000,
        )
        .with_source_bound_resource(ResourceRef::new(
            "acme",
            "finance",
            ResourceKind::SharedDrive,
            "finance-drive",
        ));

        assert_eq!(
            credential.credential_class,
            ConnectorCredentialClass::ReadOnly
        );
        assert!(credential.validate_for_tenant(&tenant).is_ok());

        let encoded = serde_json::to_value(&credential).expect("serialize credential ref");
        assert_eq!(encoded["credential_class"], "read_only");
        assert_eq!(
            encoded["secret_ref"]["secret_id"],
            credential.secret_ref.secret_id
        );
        assert!(encoded.get("credential_value").is_none());
        assert!(encoded.get("access_token").is_none());
        assert_eq!(
            encoded["source_bound_resource"]["resource_kind"],
            "shared_drive"
        );

        let wrong_tenant = TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            Some("deployment-prod".to_string()),
            "user-admin",
        );
        assert!(matches!(
            credential.validate_for_tenant(&wrong_tenant),
            Err(SecretRefError::WorkspaceMismatch)
        ));
    }

    #[test]
    fn source_binding_blocks_ingestion_when_connector_or_binding_is_not_active() {
        let tenant = TenantContext::explicit_user_workspace(
            "acme",
            "finance",
            Some("deployment-prod".to_string()),
            "user-admin",
        );
        let admin = PrincipalRef::human_user("user-admin");
        let connector = ConnectorInstance::active(
            "google-drive-finance",
            tenant.clone(),
            "google_drive",
            admin.clone(),
            1_000,
        );
        let binding = SourceBinding::enabled(
            "binding-finance-drive",
            tenant.clone(),
            "google-drive-finance",
            "google_drive_shared_drive",
            "drive-finance",
            ResourceRef::new("acme", "finance", ResourceKind::DataStore, "finance-docs"),
            DataClass::FinancialRecord,
            admin,
            1_000,
        );

        assert!(binding.can_ingest_with(&connector));

        let paused_connector = connector
            .clone()
            .with_state(ConnectorLifecycleState::Paused, 1_100);
        assert!(!binding.can_ingest_with(&paused_connector));

        let revoked_connector = connector
            .clone()
            .with_state(ConnectorLifecycleState::Revoked, 1_200);
        assert!(!binding.can_ingest_with(&revoked_connector));

        let quarantined_connector = connector
            .clone()
            .with_state(ConnectorLifecycleState::Quarantined, 1_300);
        assert!(!binding.can_ingest_with(&quarantined_connector));

        let disabled_binding = binding
            .clone()
            .with_state(SourceBindingState::Disabled, 1_400);
        assert!(!disabled_binding.can_ingest_with(&connector));

        let review_only_binding = binding.with_ingestion_policy(IngestionPolicy {
            allow_indexing: false,
            allow_prompt_context: false,
            require_review: true,
            max_depth: Some(2),
        });
        assert!(!review_only_binding.can_ingest_with(&connector));
    }

    #[test]
    fn source_objects_and_memory_chunks_carry_resource_and_data_class_scope() {
        let tenant = TenantContext::explicit_user_workspace(
            "acme",
            "finance",
            Some("deployment-prod".to_string()),
            "user-admin",
        );
        let resource = ResourceRef::new("acme", "finance", ResourceKind::Document, "board-report")
            .with_parent_path(vec![ResourcePathSegment::new(
                ResourceKind::SharedDrive,
                "finance-drive",
            )]);
        let object = SourceObject {
            source_object_id: "source-object-1".to_string(),
            tenant_context: tenant.clone(),
            binding_id: "binding-finance-drive".to_string(),
            connector_id: "google-drive-finance".to_string(),
            native_object_id: "drive-file-123".to_string(),
            resource_ref: resource.clone(),
            data_class: DataClass::FinancialRecord,
            lifecycle_state: SourceObjectLifecycleState::Active,
            native_object_path: Some("/finance/board-report.md".to_string()),
            content_hash: Some("content-sha256:abc".to_string()),
            source_hash: Some("sha256:abc".to_string()),
            parent_source_object_id: None,
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            last_seen_at_ms: Some(1_000),
            lifecycle_changed_at_ms: None,
            superseded_by_source_object_id: None,
        };
        let chunk = ScopedMemoryChunkRef {
            chunk_id: "chunk-1".to_string(),
            tenant_context: tenant.clone(),
            source_object_id: object.source_object_id.clone(),
            resource_ref: resource,
            data_class: object.data_class,
            source_hash: object.source_hash.clone(),
        };

        assert!(object.dedupe_scope_key().contains("acme:finance"));
        assert!(object.dedupe_scope_key().contains("binding-finance-drive"));
        assert!(object.tenant_matches(&tenant));
        assert!(object.is_active());
        assert!(object.allows_prompt_context());
        assert!(object
            .lifecycle_identity_key()
            .contains("binding-finance-drive"));
        assert!(!object
            .clone()
            .with_lifecycle_state(SourceObjectLifecycleState::Tombstoned, 2_000)
            .allows_prompt_context());
        assert_eq!(chunk.tenant_context, tenant);
        assert_eq!(chunk.source_object_id, "source-object-1");
        assert_eq!(chunk.data_class, DataClass::FinancialRecord);

        let encoded = serde_json::to_value(&chunk).expect("serialize memory chunk ref");
        assert_eq!(encoded["source_object_id"], "source-object-1");
        assert_eq!(encoded["resource_ref"]["resource_kind"], "document");
        assert_eq!(encoded["data_class"], "financial_record");
    }

    #[test]
    fn ingestion_quarantine_tracks_review_without_making_output_searchable() {
        let tenant = TenantContext::explicit_user_workspace(
            "acme",
            "legal",
            Some("deployment-prod".to_string()),
            "user-legal",
        );
        let quarantine = IngestionQuarantine {
            quarantine_id: "quarantine-1".to_string(),
            tenant_context: tenant,
            connector_id: "notion-legal".to_string(),
            binding_id: "binding-legal-notion".to_string(),
            source_object_ids: vec!["source-object-legal-1".to_string()],
            reason: "high_risk_data_class_requires_review".to_string(),
            created_at_ms: 1_000,
            reviewed_by: Some(PrincipalRef::human_user("legal-admin")),
            reviewed_at_ms: Some(1_500),
            disposition: Some(QuarantineDisposition::Delete),
        };
        let job = IngestionJob {
            job_id: "ingestion-job-1".to_string(),
            tenant_context: quarantine.tenant_context.clone(),
            connector_id: quarantine.connector_id.clone(),
            binding_id: quarantine.binding_id.clone(),
            state: IngestionJobState::Quarantined,
            source_object_ids: quarantine.source_object_ids.clone(),
            started_at_ms: Some(900),
            finished_at_ms: Some(1_000),
            quarantine_id: Some(quarantine.quarantine_id.clone()),
        };

        assert_eq!(job.state, IngestionJobState::Quarantined);
        assert_eq!(job.quarantine_id.as_deref(), Some("quarantine-1"));
        assert_eq!(quarantine.disposition, Some(QuarantineDisposition::Delete));

        let encoded = serde_json::to_value(&quarantine).expect("serialize quarantine");
        assert_eq!(encoded["disposition"], "delete");
        assert_eq!(encoded["reason"], "high_risk_data_class_requires_review");
    }

    fn test_strict_context(
        workspace_id: &str,
        principal: PrincipalRef,
        resource_scope: ResourceScope,
        grants: Vec<ScopedGrant>,
    ) -> StrictTenantContext {
        StrictTenantContext::new(
            TenantContext::explicit_user_workspace(
                "acme",
                workspace_id,
                Some("deployment-test".to_string()),
                principal.id.clone(),
            ),
            principal,
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "user-test",
                "tandem-web",
            )),
            resource_scope,
            AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                2_000,
                "assertion-test",
            ),
        )
        .with_grants(grants)
        .with_data_boundary(DataBoundary::allow(vec![
            DataClass::Internal,
            DataClass::Confidential,
            DataClass::Executive,
            DataClass::FinancialRecord,
            DataClass::SourceCode,
        ]))
    }
}
