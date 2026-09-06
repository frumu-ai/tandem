// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_enterprise_contract::hosted_policy::HostedPolicyBundle;
use tandem_types::{AccessDecision, AuthorityChain, HumanActor};

#[tokio::test]
async fn hosted_policy_removed_membership_cannot_reappear_from_local_registry() {
    let state = crate::test_support::test_state().await;
    let now = crate::now_ms();
    let mut input = HostedPolicyBundle::from_json(serde_json::json!({
        "schema_version": 1, "policy_version": 1, "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
        "users": [{"id": "alice", "email": null, "username": null, "role": "member",
            "capabilities": ["hosted.use"], "is_active": true, "email_verified": true}],
        "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
        "org_unit_memberships": [{"unit_id": "eng", "user_id": "alice"}], "deployment_grants": []
    }).to_string().as_bytes()).unwrap();
    let policy = input.clone().validate("org-a", "dep-a", now, None).unwrap();
    let tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 300_000,
        "assertion-a",
        tenant.clone(),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec!["hosted:role:member".into()],
    );
    claims.policy_version = Some(1);
    claims.org_units = vec!["eng".into()];
    claims.capabilities = vec!["hosted.use".into()];
    let mut verified: VerifiedTenantContext = claims.into();
    let memberships = policy.memberships_for_identity(&verified, now).unwrap();
    state
        .enterprise
        .org_unit_memberships
        .write()
        .await
        .insert("retained-membership".into(), memberships[0].clone());
    let document = ResourceRef::new(
        "org-a",
        "dep-a",
        ResourceKind::Document,
        "engineering-document",
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(
            "grant-eng".into(),
            OrganizationUnitAccessGrant::active(
                "grant-eng",
                tenant,
                tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
                document.clone(),
                now,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal]),
        );
    verified.strict_projection = Some(policy.project_identity(&verified, now).unwrap());
    enrich_verified_context_with_org_unit_grants(&state, &mut verified, Some(memberships)).await;
    let access = |verified: &VerifiedTenantContext| {
        verified
            .strict_projection
            .as_ref()
            .unwrap()
            .evaluate_access(&document, AccessPermission::Read, DataClass::Internal, now)
            .decision
    };
    assert_eq!(access(&verified), AccessDecision::Allow);

    input.policy_version = 2;
    input.org_unit_memberships.clear();
    let removed = input
        .validate("org-a", "dep-a", now, Some(policy.revision()))
        .unwrap();
    verified.policy_version = Some(2);
    verified.org_units.clear();
    verified.strict_projection = Some(removed.project_identity(&verified, now).unwrap());
    let removed_memberships = removed.memberships_for_identity(&verified, now).unwrap();
    enrich_verified_context_with_org_unit_grants(&state, &mut verified, Some(removed_memberships))
        .await;
    assert_ne!(access(&verified), AccessDecision::Allow);
    assert_eq!(state.enterprise.org_unit_memberships.read().await.len(), 1);

    // Unconfigured local policy enrichment preserves its existing behavior.
    enrich_verified_context_with_org_unit_grants(&state, &mut verified, None).await;
    assert_eq!(access(&verified), AccessDecision::Allow);
}
