use super::*;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
};

fn policy_json(version: u64, generated_at_ms: u64, active: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "policy_version": version,
        "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(generated_at_ms as i64).unwrap(),
        "users": [{"id": "alice", "email": null, "username": null,
            "role": "member", "capabilities": ["hosted.use"], "is_active": active, "email_verified": true}],
        "org_units": [], "org_unit_memberships": [], "deployment_grants": [],
    })).unwrap()
}

fn identity(version: u64) -> VerifiedTenantContext {
    let now = crate::now_ms();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 300_000,
        "assertion-a",
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice"),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec!["hosted:role:member".into()],
    );
    claims.policy_version = Some(version);
    claims.capabilities = vec!["hosted.use".into()];
    claims.into()
}

fn write_input(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[tokio::test]
async fn hosted_policy_persists_high_water_and_requires_fresh_restart_fetch() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.json");
    let source = PolicySource {
        organization_id: "org-a".into(),
        deployment_id: "dep-a".into(),
        path: path.clone(),
        started_at_ms: 0,
    };
    *state.enterprise.hosted_policy.source.write().unwrap() = Some(source.clone());
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(4)))
        .is_err());
    write_input(&path, &policy_json(4, crate::now_ms(), true));
    state.reload_hosted_policy().await.unwrap();
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(4)))
        .is_ok());
    write_input(&path, &policy_json(3, crate::now_ms(), true));
    assert!(state
        .reload_hosted_policy()
        .await
        .unwrap_err()
        .to_string()
        .contains("rollback"));
    assert_eq!(
        state
            .enterprise
            .hosted_policy
            .revision()
            .unwrap()
            .unwrap()
            .version,
        4
    );
    write_input(&path, &policy_json(4, crate::now_ms(), false));
    assert!(state
        .reload_hosted_policy()
        .await
        .unwrap_err()
        .to_string()
        .contains("conflict"));
    write_input(&path, &policy_json(5, crate::now_ms(), false));
    state.reload_hosted_policy().await.unwrap();
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(4)))
        .is_err());
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(5)))
        .is_err());

    // Simulate a fresh process with the same persisted store and input file.
    *state.enterprise.hosted_policy.snapshot.write().unwrap() = None;
    let boot = crate::now_ms() + 1;
    *state.enterprise.hosted_policy.source.write().unwrap() = Some(PolicySource {
        started_at_ms: boot,
        ..source
    });
    assert!(state
        .reload_hosted_policy()
        .await
        .unwrap_err()
        .to_string()
        .contains("after engine startup"));
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(5)))
        .is_err());
    write_input(&path, &policy_json(6, boot, true));
    state.reload_hosted_policy().await.unwrap();
    assert!(state
        .enterprise
        .hosted_policy
        .authorize(Some(&identity(6)))
        .is_ok());
}

#[test]
fn snapshot_expiry_is_checked_at_effect_time_without_polling() {
    let runtime = HostedPolicyRuntime::default();
    *runtime.source.write().unwrap() = Some(PolicySource {
        organization_id: "org-a".into(),
        deployment_id: "dep-a".into(),
        path: "/unused/policy.json".into(),
        started_at_ms: 0,
    });
    let now = crate::now_ms();
    let policy = HostedPolicyBundle::from_json(&policy_json(4, now - 120_000, true))
        .unwrap()
        .validate("org-a", "dep-a", now - 120_000, None)
        .unwrap();
    *runtime.snapshot.write().unwrap() = Some(Arc::new(policy));
    assert!(runtime.authorize(Some(&identity(4))).is_err());
    assert!(runtime.authorize(None).is_err());
}
