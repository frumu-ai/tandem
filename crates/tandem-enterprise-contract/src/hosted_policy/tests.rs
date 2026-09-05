use super::*;
use crate::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
};

const NOW: u64 = 1_800_000_000_000;

fn bundle() -> HostedPolicyBundle {
    HostedPolicyBundle::from_json(serde_json::json!({
        "schema_version": 1, "policy_version": 4,
        "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": DateTime::from_timestamp_millis(NOW as i64).unwrap(),
        "users": [{"id": "alice", "email": "alice@example.com", "username": "alice",
            "role": "member", "is_active": true, "email_verified": true,
            "capabilities": ["hosted.panel", "hosted.view", "hosted.use"]},
            {"id": "bob", "email": null, "username": null, "role": "viewer",
            "is_active": true, "email_verified": true, "capabilities": ["hosted.panel", "hosted.view"]}],
        "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
        "org_unit_memberships": [{"unit_id": "eng", "user_id": "alice"}],
        "deployment_grants": [{"id": "grant-a", "deployment_id": "dep-a", "principal_kind": "org_unit",
            "principal_id": "eng", "resource_kind": "deployment", "resource_id": "dep-a", "permissions": ["hosted.use"]}]
    }).to_string().as_bytes()).unwrap()
}

fn identity() -> VerifiedTenantContext {
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        NOW,
        NOW + 300_000,
        "assertion-a",
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice"),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec!["hosted:role:member".into(), "hosted:use".into()],
    );
    claims.policy_version = Some(4);
    claims.org_units = vec!["eng".into()];
    claims.capabilities = vec!["hosted.use".into()];
    claims.into()
}

#[test]
fn validates_real_shaped_bundle_and_current_identity() {
    let policy = bundle().validate("org-a", "dep-a", NOW, None).unwrap();
    assert_eq!(policy.revision().version, 4);
    assert_eq!(policy.expires_at_ms(), NOW + MAX_POLICY_AGE_MS);
    assert_eq!(policy.authorize_identity(&identity(), NOW), Ok(()));
    assert!(policy
        .authorize_identity(&identity(), NOW + MAX_POLICY_AGE_MS)
        .is_err());
}

#[test]
fn rejects_wrong_scope_schema_and_freshness() {
    assert!(bundle().validate("org-b", "dep-a", NOW, None).is_err());
    assert!(bundle().validate("org-a", "dep-b", NOW, None).is_err());
    assert!(bundle()
        .validate("org-a", "dep-a", NOW + MAX_POLICY_AGE_MS, None)
        .is_err());
    assert!(bundle()
        .validate("org-a", "dep-a", NOW - FUTURE_SKEW_MS - 1, None)
        .is_err());
    let mut value = bundle();
    value.schema_version = 2;
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
    let mut value = bundle();
    value.policy_version = 0;
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
    assert!(HostedPolicyBundle::from_json(&vec![b' '; MAX_POLICY_BYTES + 1]).is_err());
}

#[test]
fn rejects_rollback_and_conflicting_equal_revision_but_accepts_fresh_refetch() {
    let previous = bundle().validate("org-a", "dep-a", NOW, None).unwrap();
    let mut next = bundle();
    next.generated_at += chrono::Duration::seconds(20);
    next.users.reverse();
    next.users[0].email = Some("changed-profile@example.com".into());
    assert_eq!(
        next.validate("org-a", "dep-a", NOW + 20_000, Some(previous.revision()))
            .unwrap()
            .revision(),
        previous.revision()
    );
    let mut next = bundle();
    next.policy_version = 3;
    assert_eq!(
        next.validate("org-a", "dep-a", NOW, Some(previous.revision()))
            .unwrap_err(),
        "hosted_policy_rollback"
    );
    let mut next = bundle();
    next.users[0].role = "admin".into();
    assert_eq!(
        next.validate("org-a", "dep-a", NOW, Some(previous.revision()))
            .unwrap_err(),
        "hosted_policy_revision_conflict"
    );
}

#[test]
fn removing_and_reinviting_cannot_restore_an_old_assertion() {
    let old_identity = identity();
    let mut removed = bundle();
    removed.policy_version += 1;
    removed.users.remove(0);
    removed.org_unit_memberships.clear();
    let policy = removed.validate("org-a", "dep-a", NOW, None).unwrap();
    assert!(policy.authorize_identity(&old_identity, NOW).is_err());
    let mut reinvited = bundle();
    reinvited.policy_version += 2;
    let policy = reinvited
        .validate("org-a", "dep-a", NOW, Some(policy.revision()))
        .unwrap();
    assert!(policy.authorize_identity(&old_identity, NOW).is_err());
    let mut fresh = old_identity;
    fresh.policy_version = Some(6);
    assert_eq!(policy.authorize_identity(&fresh, NOW), Ok(()));
}

#[test]
fn rejects_elevated_role_capability_and_archived_unit() {
    let policy = bundle().validate("org-a", "dep-a", NOW, None).unwrap();
    let mut claims = identity();
    claims.roles.push("hosted:admin".into());
    assert!(policy.authorize_identity(&claims, NOW).is_err());
    let mut claims = identity();
    claims.capabilities.push("deployment.admin".into());
    assert!(policy.authorize_identity(&claims, NOW).is_err());
    let mut value = bundle();
    value.org_units[0].state = "archived".into();
    assert!(value
        .validate("org-a", "dep-a", NOW, None)
        .unwrap()
        .authorize_identity(&identity(), NOW)
        .is_err());
    let mut value = bundle();
    value.users[0].capabilities.push("deployment.admin".into());
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
}

#[test]
fn rejects_inactive_unverified_missing_or_cross_tenant_human() {
    for unverified in [false, true] {
        let mut value = bundle();
        if unverified {
            value.users[0].email_verified = false;
        } else {
            value.users[0].is_active = false;
        }
        assert!(value
            .validate("org-a", "dep-a", NOW, None)
            .unwrap()
            .authorize_identity(&identity(), NOW)
            .is_err());
    }
    let policy = bundle().validate("org-a", "dep-a", NOW, None).unwrap();
    let mut claims = identity();
    claims.human_actor.actor_id = "bob".into();
    assert!(policy.authorize_identity(&claims, NOW).is_err());
    let mut claims = identity();
    claims.tenant_context.org_id = "org-b".into();
    assert!(policy.authorize_identity(&claims, NOW).is_err());
    let mut claims = identity();
    claims.policy_version = None;
    assert!(policy.authorize_identity(&claims, NOW).is_err());
}

#[test]
fn rejects_orphan_duplicate_and_cross_deployment_authority() {
    let mut value = bundle();
    value.users.push(value.users[0].clone());
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
    let mut value = bundle();
    value.org_unit_memberships[0].user_id = "missing".into();
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
    let mut value = bundle();
    value.deployment_grants[0].deployment_id = Some("dep-b".into());
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
    let mut value = bundle();
    value.deployment_grants[0].permissions = vec!["root".into()];
    assert!(value.validate("org-a", "dep-a", NOW, None).is_err());
}
