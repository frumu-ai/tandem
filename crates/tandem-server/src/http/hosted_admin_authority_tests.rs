// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_enterprise_contract::hosted_policy::{role_capabilities, HostedPolicyBundle};
use tandem_types::{
    AccessEffect, AuthorityChain, HumanActor, RequestPrincipal, TenantContext,
    TenantContextAssertionClaims,
};

fn context(role: &str) -> VerifiedTenantContext {
    let now = crate::now_ms();
    let capabilities: Vec<String> = role_capabilities(role)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        "admin-boundary",
        tenant,
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec![format!("hosted:role:{role}")],
    );
    claims.policy_version = Some(1);
    claims.capabilities = capabilities.clone();
    let mut verified: VerifiedTenantContext = claims.into();
    let bundle: HostedPolicyBundle = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "policy_version": 1, "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
        "users": [{"id": "alice", "email": null, "username": null, "role": role,
            "capabilities": capabilities, "is_active": true, "email_verified": true}],
        "org_units": [], "org_unit_memberships": [], "deployment_grants": []
    })).unwrap();
    verified.strict_projection = Some(
        bundle
            .validate("org-a", "dep-a", now, None)
            .unwrap()
            .project_identity(&verified, now)
            .unwrap(),
    );
    verified
}

fn assert_guards(verified: &VerifiedTenantContext, expected: bool) {
    let tenant = &verified.tenant_context;
    assert_eq!(
        super::super::sessions::session_permission_rules_allowed(tenant, Some(verified)),
        expected,
        "session permissions"
    );
    assert_eq!(
        super::super::routes_governance::governance_mutation_admin_allowed(tenant, Some(verified)),
        expected,
        "governance mutations"
    );
    assert_eq!(
        super::super::workflows::workflow_reviewer_is_eligible(tenant, Some(verified)),
        expected,
        "workflow reviewer"
    );
}

#[test]
fn hosted_admin_guards_preserve_admin_and_legacy_controls() {
    for (role, expected) in [
        ("viewer", false),
        ("member", false),
        ("admin", true),
        ("owner", true),
    ] {
        assert_guards(&context(role), expected);
    }
    let mut legacy = context("member");
    legacy.policy_version = None;
    legacy.strict_projection = None;
    legacy.roles = vec!["admin".into()];
    assert_guards(&legacy, true);
    legacy.roles.clear();
    assert_guards(&legacy, false);
}

#[test]
fn hosted_admin_guards_require_matching_live_deployment_grant() {
    let admin = context("admin");
    let mut data_only = admin.clone();
    let strict = data_only.strict_projection.as_mut().unwrap();
    for grant in &mut strict.grants {
        grant.resource.resource_kind = ResourceKind::Document;
        grant.permissions = vec![AccessPermission::Admin, AccessPermission::Delegate];
    }
    assert_guards(&data_only, false);
    let mut wrong_deployment = admin.clone();
    for grant in &mut wrong_deployment.strict_projection.as_mut().unwrap().grants {
        grant.resource.resource_id = "other-deployment".into();
    }
    assert_guards(&wrong_deployment, false);
    let mut denied = admin.clone();
    let strict = denied.strict_projection.as_mut().unwrap();
    let mut deny = strict
        .grants
        .iter()
        .find(|g| g.permissions.contains(&AccessPermission::HostedAdmin))
        .unwrap()
        .clone();
    deny.grant_id = "explicit-deny".into();
    deny.effect = AccessEffect::Deny;
    strict.grants.push(deny);
    assert_guards(&denied, false);
    let mut expired = admin.clone();
    expired.expires_at_ms = crate::now_ms() - 1;
    assert_guards(&expired, false);
    let mut missing = admin;
    missing.strict_projection = None;
    missing.roles = vec!["admin".into()];
    missing.capabilities = vec![
        "governance.admin".into(),
        "approval.review".into(),
        "permission.admin".into(),
    ];
    assert_guards(&missing, false);
}

#[tokio::test]
async fn governance_admin_commit_rejects_a_revoked_ingress_projection() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().expect("hosted policy directory");
    let policy_path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", policy_path.clone());
    let now = crate::now_ms();
    let write_policy = |version: u64, role: &str| {
        let capabilities = role_capabilities(role);
        std::fs::write(
            &policy_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "policy_version": version,
                "organization_id": "org-a",
                "deployment_id": "dep-a",
                "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
                "users": [{
                    "id": "alice", "email": null, "username": null,
                    "role": role, "capabilities": capabilities,
                    "is_active": true, "email_verified": true
                }],
                "org_units": [], "org_unit_memberships": [], "deployment_grants": []
            }))
            .unwrap(),
        )
        .expect("write hosted policy");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&policy_path, std::fs::Permissions::from_mode(0o600))
                .expect("private hosted policy");
        }
    };
    write_policy(1, "admin");
    state
        .reload_hosted_policy()
        .await
        .expect("load admin policy");
    let ingress_admin = context("admin");
    let tenant = &ingress_admin.tenant_context;
    assert!(
        super::super::routes_governance::require_current_governance_admin(
            &state,
            tenant,
            Some(&ingress_admin)
        )
        .is_ok()
    );

    write_policy(2, "viewer");
    state
        .reload_hosted_policy()
        .await
        .expect("publish revoked policy");
    assert!(
        super::super::routes_governance::governance_mutation_admin_allowed(
            tenant,
            Some(&ingress_admin)
        ),
        "the request-local projection must remain stale for this regression"
    );
    assert!(
        super::super::routes_governance::require_current_governance_admin(
            &state,
            tenant,
            Some(&ingress_admin)
        )
        .is_err()
    );
}
