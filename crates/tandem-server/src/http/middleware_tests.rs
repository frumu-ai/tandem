use super::*;
use axum::http::HeaderValue;
use tandem_types::{AuthorityChain, HumanActor, OrganizationUnitState, TenantSource};

#[test]
fn resolve_enterprise_request_context_defaults_to_local_tenant() {
    let headers = HeaderMap::new();
    let resolved = resolve_enterprise_request_context(&headers);
    let tenant_context = resolved.tenant_context;
    let principal = resolved.request_principal;
    assert_eq!(tenant_context.org_id, "local");
    assert_eq!(tenant_context.workspace_id, "local");
    assert!(tenant_context.actor_id.is_none());
    assert_eq!(principal.actor_id, None);
    assert_eq!(principal.source, "local_control_panel");
}

#[test]
fn resolve_enterprise_request_context_ignores_unsigned_tenant_headers_in_local_mode() {
    let mut headers = HeaderMap::new();
    headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
    headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
    headers.insert("x-user-id", HeaderValue::from_static("user-1"));
    let resolved = resolve_secure_local_enterprise_request_context(&headers);
    let tenant_context = resolved.tenant_context;
    let principal = resolved.request_principal;
    assert_eq!(tenant_context.org_id, "local");
    assert_eq!(tenant_context.workspace_id, "local");
    assert_eq!(tenant_context.actor_id, None);
    assert_eq!(principal.actor_id, None);
    assert_eq!(tenant_context.source, TenantSource::LocalImplicit);
}

#[test]
fn resolve_enterprise_request_context_uses_request_source_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-tandem-request-source",
        HeaderValue::from_static("control_panel"),
    );
    let resolved = resolve_enterprise_request_context(&headers);
    let principal = resolved.request_principal;
    assert_eq!(principal.source, "control_panel");
}

#[test]
fn local_mode_transport_token_remains_optional_when_unconfigured() {
    let headers = HeaderMap::new();

    assert!(request_transport_token_authorized(
        &headers,
        None,
        RuntimeAuthMode::LocalSingleTenant
    ));
}

#[test]
fn local_mode_rejects_missing_transport_token_when_configured() {
    let headers = HeaderMap::new();

    assert!(!request_transport_token_authorized(
        &headers,
        Some("tk_local"),
        RuntimeAuthMode::LocalSingleTenant
    ));
}

#[test]
fn hosted_mode_requires_configured_transport_token() {
    let headers = HeaderMap::new();

    assert!(!request_transport_token_authorized(
        &headers,
        None,
        RuntimeAuthMode::HostedSingleTenant
    ));
}

#[test]
fn hosted_mode_rejects_wrong_transport_token() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer wrong-token"),
    );

    assert!(!request_transport_token_authorized(
        &headers,
        Some("tk_hosted"),
        RuntimeAuthMode::HostedSingleTenant
    ));
}

#[test]
fn hosted_mode_accepts_matching_transport_token() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer tk_hosted"),
    );

    assert!(request_transport_token_authorized(
        &headers,
        Some("tk_hosted"),
        RuntimeAuthMode::HostedSingleTenant
    ));
}

#[test]
fn hosted_mode_rejects_unsigned_tenant_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
    headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));

    let err =
        resolve_enterprise_request_context_for_mode(&headers, RuntimeAuthMode::HostedSingleTenant)
            .expect_err("hosted mode must not trust raw tenant headers");

    assert_eq!(err, TenantContextIngressError::UnsignedTenantHeaders);
}

#[test]
fn hosted_mode_requires_verified_context_even_without_raw_headers() {
    let headers = HeaderMap::new();

    let err =
        resolve_enterprise_request_context_for_mode(&headers, RuntimeAuthMode::HostedSingleTenant)
            .expect_err("hosted mode requires signed context");

    assert_eq!(err, TenantContextIngressError::MissingVerifiedContext);
}

#[test]
fn hosted_mode_rejects_context_assertion_without_configured_key() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-tandem-context-jws",
        HeaderValue::from_static("placeholder.assertion.signature"),
    );

    let err =
        resolve_enterprise_request_context_for_mode(&headers, RuntimeAuthMode::HostedSingleTenant)
            .expect_err("hosted mode must fail closed without verifier key config");

    assert_eq!(
        err,
        TenantContextIngressError::ContextAssertionKeyNotConfigured
    );
}

#[test]
fn local_mode_ignores_unsigned_tenant_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
    headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
    headers.insert("x-user-id", HeaderValue::from_static("user-1"));

    let resolved = resolve_secure_local_enterprise_request_context(&headers);
    let tenant_context = resolved.tenant_context;
    let principal = resolved.request_principal;

    assert_eq!(tenant_context.org_id, "local");
    assert_eq!(tenant_context.workspace_id, "local");
    assert_eq!(principal.actor_id, None);
}

#[test]
fn verifier_accepts_valid_tandem_context_assertion() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let assertion =
        sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));

    let verified = verifier
        .verify_at(&assertion, 1_500)
        .expect("signed assertion should verify");

    assert_eq!(verified.issuer, "tandem-web");
    assert_eq!(verified.audience, "tandem-runtime");
    assert_eq!(verified.human_actor.actor_id, "user-a");
    assert_eq!(verified.tenant_context.org_id, "org-a");
    assert_eq!(verified.tenant_context.workspace_id, "workspace-a");
    assert_eq!(
        verified.tenant_context.deployment_id.as_deref(),
        Some("dep-a")
    );
}

#[test]
fn verifier_accepts_signed_context_assertion_with_strict_projection() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let principal = PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a");
    let repo = ResourceRef::new("org-a", "workspace-a", ResourceKind::Repository, "tandem")
        .with_project_id("platform")
        .with_path_prefix("crates/tandem-enterprise-contract/");
    let grant = ScopedGrant::new(
        "grant-platform-read",
        principal.clone(),
        repo.clone(),
        GrantSource::Delegation,
    )
    .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
    .with_data_classes(vec![DataClass::SourceCode]);
    let claims = test_claims(1_000, 2_000).with_strict_projection(
        principal,
        ResourceScope {
            root: ResourceRef::new("org-a", "workspace-a", ResourceKind::Project, "platform"),
            allowed_resources: vec![repo],
            denied_resources: Vec::new(),
            max_depth: Some(4),
        },
        vec![grant],
        DataBoundary::allow(vec![DataClass::SourceCode]),
    );
    let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

    let verified = verifier
        .verify_at(&assertion, 1_500)
        .expect("signed scoped assertion should verify");

    assert_eq!(verified.issuer, "tandem-web");
    assert_eq!(verified.tenant_context.org_id, "org-a");
    assert_eq!(verified.human_actor.actor_id, "user-a");
}

#[test]
fn signed_strict_context_projects_active_org_unit_membership_grants() {
    let principal = PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a");
    let patient_cases = ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::DataStore,
        "patient-cases",
    );
    let mut verified =
        VerifiedTenantContext::from(test_claims(1_000, 4_000).with_strict_projection(
            principal,
            ResourceScope::root(ResourceRef::new(
                "org-a",
                "workspace-a",
                ResourceKind::Workspace,
                "workspace-a",
            )),
            Vec::new(),
            DataBoundary::allow(vec![DataClass::Regulated, DataClass::CustomerData]),
        ));
    let tenant = verified.tenant_context.clone();
    let doctors = PrincipalRef::organization_unit("clinical_role/doctors");
    let membership = OrganizationUnitMembership::active(
        "membership-doctor-user",
        tenant.clone(),
        doctors.clone(),
        PrincipalRef::human_user("user-a"),
        tandem_types::OrganizationUnitMembershipSource::HostedControlPlane,
        1_000,
    );
    let disabled_membership = OrganizationUnitMembership {
        membership_id: "membership-disabled".to_string(),
        state: OrganizationUnitState::Disabled,
        member: PrincipalRef::human_user("user-b"),
        ..membership.clone()
    };
    let access_grant = OrganizationUnitAccessGrant::active(
        "grant-doctors-patient-cases",
        tenant,
        doctors,
        patient_cases.clone(),
        1_000,
    )
    .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
    .with_data_classes(vec![DataClass::Regulated, DataClass::CustomerData]);

    project_org_unit_grants_into_verified_context(
        &mut verified,
        [&membership, &disabled_membership].into_iter(),
        [&access_grant].into_iter(),
        1_500,
    );

    let strict = verified
        .strict_projection
        .as_ref()
        .expect("strict projection remains present");
    assert_eq!(strict.grants.len(), 1);
    assert_eq!(
        strict.grants[0].grant_source,
        GrantSource::OrganizationUnitMembership
    );
    assert_eq!(
        strict.grants[0].source_principal.as_ref(),
        Some(&access_grant.unit)
    );
    assert!(
        strict
            .evaluate_access(
                &patient_cases,
                AccessPermission::Read,
                DataClass::Regulated,
                1_500,
            )
            .decision
            == tandem_types::AccessDecision::Allow
    );
}

#[test]
fn org_unit_projection_does_not_create_strict_context_or_cross_tenants() {
    let mut verified = VerifiedTenantContext::from(test_claims(1_000, 4_000));
    let other_tenant = TenantContext::explicit_user_workspace(
        "other-org",
        "workspace-a",
        Some("dep-a".to_string()),
        "user-a",
    );
    let doctors = PrincipalRef::organization_unit("clinical_role/doctors");
    let membership = OrganizationUnitMembership::active(
        "membership-doctor-user",
        other_tenant.clone(),
        doctors.clone(),
        PrincipalRef::human_user("user-a"),
        tandem_types::OrganizationUnitMembershipSource::HostedControlPlane,
        1_000,
    );
    let access_grant = OrganizationUnitAccessGrant::active(
        "grant-doctors-patient-cases",
        other_tenant,
        doctors,
        ResourceRef::new(
            "other-org",
            "workspace-a",
            ResourceKind::DataStore,
            "patient-cases",
        ),
        1_000,
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::Regulated]);

    project_org_unit_grants_into_verified_context(
        &mut verified,
        [&membership].into_iter(),
        [&access_grant].into_iter(),
        1_500,
    );

    assert!(verified.strict_projection.is_none());
}

#[test]
fn hosted_mode_resolves_verified_context_as_tandem_web_principal() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let assertion =
        sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));
    let verified = verifier
        .verify_at(&assertion, 1_500)
        .expect("signed assertion should verify");

    let resolved = ResolvedEnterpriseRequestContext::verified(verified);

    assert_eq!(
        resolved.request_principal.actor_id.as_deref(),
        Some("user-a")
    );
    assert_eq!(resolved.request_principal.source, "tandem-web");
    assert_eq!(resolved.tenant_context.org_id, "org-a");
    assert_eq!(resolved.tenant_context.source, TenantSource::Explicit);
}

#[test]
fn verifier_rejects_tampered_tandem_context_assertion() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let assertion =
        sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));
    let parts = assertion.split('.').collect::<Vec<_>>();
    let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&test_claims(1_100, 2_100)).expect("claims json"));
    let assertion = format!("{}.{}.{}", parts[0], encoded_claims, parts[2]);

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("tampered assertion must not verify");

    assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
}

#[test]
fn verifier_rejects_expired_tandem_context_assertion() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let assertion =
        sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));

    let err = verifier
        .verify_at(&assertion, 2_000)
        .expect_err("expired assertion must fail closed");

    assert_eq!(err, TenantContextIngressError::ContextAssertionExpired);
}

#[test]
fn verifier_rejects_local_implicit_tenant_context_assertion() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let mut claims = test_claims(1_000, 2_000);
    claims.tenant_context = TenantContext::local_implicit();
    let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("hosted assertions must carry explicit deployment tenant context");

    assert_eq!(err, TenantContextIngressError::ContextAssertionMalformed);
}

#[test]
fn verifier_rejects_context_assertion_without_deployment_scope() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let mut claims = test_claims(1_000, 2_000);
    claims.tenant_context.deployment_id = None;
    let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("hosted assertions must bind to a deployment audience");

    assert_eq!(err, TenantContextIngressError::ContextAssertionMalformed);
}

#[test]
fn verifier_rejects_context_assertion_with_mismatched_authority_actor() {
    let (signing_key, verifier) = test_signing_key_and_verifier();
    let mut claims = test_claims(1_000, 2_000);
    claims.authority_chain =
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("user-b", "tandem-web"));
    let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("hosted assertions must bind authority to the human actor");

    assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
}

#[test]
fn verifier_selects_context_assertion_key_by_kid() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let other_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::from([
            (
                "old-key".to_string(),
                ContextAssertionPublicKey::legacy(other_key.verifying_key().to_bytes()),
            ),
            (
                "active-key".to_string(),
                ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes()),
            ),
        ]),
        legacy_public_key: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    let assertion =
        sign_test_context_assertion(&signing_key, "active-key", test_claims(1_000, 2_000));

    let verified = verifier
        .verify_at(&assertion, 1_500)
        .expect("kid-selected key should verify");

    assert_eq!(verified.assertion_id, "assertion-a");
}

#[test]
fn verifier_rejects_unknown_context_assertion_kid_when_keyring_is_configured() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let other_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::from([(
            "old-key".to_string(),
            ContextAssertionPublicKey::legacy(other_key.verifying_key().to_bytes()),
        )]),
        legacy_public_key: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    let assertion =
        sign_test_context_assertion(&signing_key, "active-key", test_claims(1_000, 2_000));

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("unknown kid should not use the wrong key");

    assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
}

#[test]
fn parse_context_public_keyring_accepts_json_and_delimited_forms() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let encoded =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing_key.verifying_key());
    let json_keyring = format!(r#"{{"active-key":"{encoded}"}}"#);
    let delimited_keyring = format!("active-key={encoded};next-key={encoded}");

    assert_eq!(
        parse_context_public_keyring(&json_keyring)
            .expect("json keyring")
            .get("active-key")
            .map(|key| key.public_key),
        Some(signing_key.verifying_key().to_bytes())
    );
    assert_eq!(
        parse_context_public_keyring(&delimited_keyring)
            .expect("delimited keyring")
            .len(),
        2
    );
}

#[test]
fn parse_context_public_keyring_accepts_metadata_objects() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let encoded =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing_key.verifying_key());
    let json_keyring = format!(
        r#"{{
                "active-key": {{
                    "publicKey": "{encoded}",
                    "purpose": "context_assertion",
                    "organizationId": "org-a",
                    "deploymentId": "dep-a",
                    "allowedAudiences": ["tandem-runtime"],
                    "allowedResourceScopePrefixes": ["org/org-a/workspace/workspace-a/project/platform"],
                    "notBeforeMs": 1,
                    "notAfterMs": 5000,
                    "status": "active"
                }}
            }}"#
    );

    let keyring = parse_context_public_keyring(&json_keyring).expect("metadata keyring");
    let key = keyring.get("active-key").expect("active key metadata");

    assert_eq!(key.public_key, signing_key.verifying_key().to_bytes());
    assert_eq!(key.purpose, Some(SigningKeyPurpose::ContextAssertion));
    assert_eq!(key.organization_id.as_deref(), Some("org-a"));
    assert_eq!(key.deployment_id.as_deref(), Some("dep-a"));
    assert_eq!(key.allowed_audiences, vec!["tandem-runtime"]);
    assert_eq!(
        key.allowed_resource_scope_prefixes,
        vec!["org/org-a/workspace/workspace-a/project/platform"]
    );
    assert_eq!(key.not_before_ms, Some(1));
    assert_eq!(key.not_after_ms, Some(5000));
    assert_eq!(key.status.as_deref(), Some("active"));
}

#[test]
fn verifier_enforces_context_assertion_key_metadata() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::from([(
            "active-key".to_string(),
            ContextAssertionPublicKey {
                public_key: signing_key.verifying_key().to_bytes(),
                purpose: Some(SigningKeyPurpose::ContextAssertion),
                organization_id: Some("org-a".to_string()),
                deployment_id: Some("dep-a".to_string()),
                allowed_audiences: vec!["tandem-runtime".to_string()],
                allowed_resource_scope_prefixes: vec![
                    "org/org-a/workspace/workspace-a/project/platform".to_string(),
                ],
                not_before_ms: Some(1_000),
                not_after_ms: Some(2_000),
                status: Some("active".to_string()),
            },
        )]),
        legacy_public_key: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    let claims = test_claims(1_000, 2_000).with_strict_projection(
        PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a"),
        ResourceScope::root(ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Project,
            "platform",
        )),
        Vec::new(),
        DataBoundary::allow(vec![DataClass::SourceCode]),
    );
    let assertion = sign_test_context_assertion(&signing_key, "active-key", claims);

    let verified = verifier
        .verify_at(&assertion, 1_500)
        .expect("key metadata should match assertion scope");

    assert_eq!(verified.tenant_context.org_id, "org-a");
}

#[test]
fn verifier_rejects_context_assertion_key_with_wrong_purpose() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::from([(
            "approval-key".to_string(),
            ContextAssertionPublicKey {
                public_key: signing_key.verifying_key().to_bytes(),
                purpose: Some(SigningKeyPurpose::ApprovalReceipt),
                ..ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes())
            },
        )]),
        legacy_public_key: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    let assertion =
        sign_test_context_assertion(&signing_key, "approval-key", test_claims(1_000, 2_000));

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("approval keys must not verify context assertions");

    assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
}

#[test]
fn verifier_rejects_context_assertion_outside_key_scope() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::from([(
            "active-key".to_string(),
            ContextAssertionPublicKey {
                public_key: signing_key.verifying_key().to_bytes(),
                purpose: Some(SigningKeyPurpose::ContextAssertion),
                allowed_resource_scope_prefixes: vec![
                    "org/org-a/workspace/workspace-a/project/finance".to_string(),
                ],
                ..ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes())
            },
        )]),
        legacy_public_key: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    let claims = test_claims(1_000, 2_000).with_strict_projection(
        PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a"),
        ResourceScope::root(ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Project,
            "platform",
        )),
        Vec::new(),
        DataBoundary::allow(vec![DataClass::SourceCode]),
    );
    let assertion = sign_test_context_assertion(&signing_key, "active-key", claims);

    let err = verifier
        .verify_at(&assertion, 1_500)
        .expect_err("key scope must constrain signed projection scope");

    assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
}

#[test]
fn replay_guard_bound_mode_allows_identical_assertion_reuse() {
    let guard = AssertionReplayGuard::new();
    let fingerprint = assertion_fingerprint("assertion-bytes");

    for _ in 0..3 {
        guard
            .check_and_record(
                AssertionReplayMode::Bound,
                "assertion-a",
                fingerprint,
                2_000,
                1_500,
            )
            .expect("identical assertion reuse is allowed in bound mode");
    }
}

#[test]
fn replay_guard_bound_mode_rejects_same_id_with_different_bytes() {
    let guard = AssertionReplayGuard::new();
    guard
        .check_and_record(
            AssertionReplayMode::Bound,
            "assertion-a",
            assertion_fingerprint("original-bytes"),
            2_000,
            1_500,
        )
        .expect("first use binds the assertion id");

    let err = guard
        .check_and_record(
            AssertionReplayMode::Bound,
            "assertion-a",
            assertion_fingerprint("different-bytes"),
            2_500,
            1_600,
        )
        .expect_err("same assertion id with different bytes is a replay/substitution");

    assert_eq!(err, TenantContextIngressError::ContextAssertionReplayed);
}

#[test]
fn replay_guard_one_shot_mode_rejects_second_use() {
    let guard = AssertionReplayGuard::new();
    let fingerprint = assertion_fingerprint("assertion-bytes");
    guard
        .check_and_record(
            AssertionReplayMode::OneShot,
            "assertion-a",
            fingerprint,
            2_000,
            1_500,
        )
        .expect("first use is accepted");

    let err = guard
        .check_and_record(
            AssertionReplayMode::OneShot,
            "assertion-a",
            fingerprint,
            2_000,
            1_600,
        )
        .expect_err("one-shot mode accepts an assertion id exactly once");

    assert_eq!(err, TenantContextIngressError::ContextAssertionReplayed);
}

#[test]
fn replay_guard_releases_expired_assertion_ids() {
    let guard = AssertionReplayGuard::new();
    guard
        .check_and_record(
            AssertionReplayMode::OneShot,
            "assertion-a",
            assertion_fingerprint("old-bytes"),
            2_000,
            1_500,
        )
        .expect("first use is accepted");

    let past_retention = 2_000 + ASSERTION_REPLAY_RETENTION_GRACE_MS + 1;
    guard
        .check_and_record(
            AssertionReplayMode::OneShot,
            "assertion-a",
            assertion_fingerprint("new-bytes"),
            past_retention + 1_000,
            past_retention,
        )
        .expect("expired entries no longer block the assertion id");
}

#[test]
fn replay_guard_sweeps_expired_entries_to_bound_memory() {
    let guard = AssertionReplayGuard::new();
    let fingerprint = assertion_fingerprint("assertion-bytes");
    for index in 0..ASSERTION_REPLAY_SWEEP_THRESHOLD {
        guard
            .check_and_record(
                AssertionReplayMode::Bound,
                &format!("assertion-{index}"),
                fingerprint,
                2_000,
                1_500,
            )
            .expect("inserts succeed");
    }
    assert_eq!(guard.len(), ASSERTION_REPLAY_SWEEP_THRESHOLD);

    let past_retention = 2_000 + ASSERTION_REPLAY_RETENTION_GRACE_MS + 1;
    guard
        .check_and_record(
            AssertionReplayMode::Bound,
            "assertion-fresh",
            fingerprint,
            past_retention + 10_000,
            past_retention,
        )
        .expect("insert after sweep succeeds");

    assert_eq!(guard.len(), 1, "expired entries are swept at threshold");
}

#[test]
fn replay_mode_defaults_to_bound() {
    assert_eq!(resolve_assertion_replay_mode(), AssertionReplayMode::Bound);
}

fn test_signing_key_and_verifier() -> (ed25519_dalek::SigningKey, TenantContextAssertionVerifier) {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let verifier = TenantContextAssertionVerifier {
        public_keys_by_id: BTreeMap::new(),
        legacy_public_key: Some(ContextAssertionPublicKey::legacy(
            signing_key.verifying_key().to_bytes(),
        )),
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        max_future_skew_ms: 60_000,
    };
    (signing_key, verifier)
}

fn test_claims(issued_at_ms: u64, expires_at_ms: u64) -> TenantContextAssertionClaims {
    let tenant_context = TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("dep-a".to_string()),
        "user-a",
    );
    let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
    TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        issued_at_ms,
        expires_at_ms,
        "assertion-a",
        tenant_context,
        HumanActor::tandem_user("user-a"),
        AuthorityChain::from_request(principal),
        vec!["workspace:admin".to_string()],
    )
}

fn sign_test_context_assertion(
    signing_key: &ed25519_dalek::SigningKey,
    kid: &str,
    claims: TenantContextAssertionClaims,
) -> String {
    use ed25519_dalek::Signer;

    let header = TenantContextAssertionHeader::ed25519(kid);
    let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("header json"));
    let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("claims json"));
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    let encoded_signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());
    format!("{signing_input}.{encoded_signature}")
}
