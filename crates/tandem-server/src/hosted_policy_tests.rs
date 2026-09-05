use super::*;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
};

#[tokio::test]
async fn hosted_policy_execution_requires_current_use_grant_and_replaces_projection() {
    let state = crate::test_support::test_state().await;
    assert!(state
        .enterprise
        .hosted_policy
        .authorize_execution(None)
        .is_ok());
    *state.enterprise.hosted_policy.source.write().unwrap() = Some(PolicySource {
        organization_id: "org-a".into(),
        deployment_id: "dep-a".into(),
        path: PathBuf::from("unused"),
        started_at_ms: 0,
    });
    let runtime = &state.enterprise.hosted_policy;
    assert!(runtime.authorize_execution(Some(&identity(4))).is_err());
    let now = crate::now_ms();
    let policy = HostedPolicyBundle::from_json(&policy_json(4, now, true))
        .unwrap()
        .validate("org-a", "dep-a", now, None)
        .unwrap();
    *runtime.snapshot.write().unwrap() = Some(Arc::new(policy));
    let mut verified = identity(4);
    assert!(runtime.project(&mut verified).unwrap().unwrap().is_empty());
    assert!(verified.strict_projection.is_some());
    assert!(runtime.authorize_execution(Some(&verified)).is_ok());
    let mut changed = HostedPolicyBundle::from_json(&policy_json(5, now, true)).unwrap();
    changed.users[0].role = "viewer".into();
    changed.users[0].capabilities = vec!["hosted.view".into()];
    *runtime.snapshot.write().unwrap() = Some(Arc::new(
        changed.validate("org-a", "dep-a", now, None).unwrap(),
    ));
    assert!(runtime.authorize_execution(Some(&verified)).is_err());
    verified.policy_version = Some(5);
    verified.roles = vec!["hosted:role:viewer".into()];
    verified.capabilities = vec!["hosted.view".into()];
    assert!(runtime.authorize(Some(&verified)).is_ok());
    assert!(runtime.authorize_execution(Some(&verified)).is_err());
    runtime.project(&mut verified).unwrap();
    assert!(!verified
        .strict_projection
        .unwrap()
        .has_permission(AccessPermission::HostedUse));
}

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
    assert!(!runtime.is_ready());
}

#[tokio::test]
async fn hosted_policy_public_health_reports_unavailable_until_a_fresh_snapshot() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let state = crate::test_support::test_state().await;
    let app = crate::build_router_with_extensions(state.clone(), &[]);
    *state.enterprise.hosted_policy.source.write().unwrap() = Some(PolicySource {
        organization_id: "org-a".into(),
        deployment_id: "dep-a".into(),
        path: "/unused/policy.json".into(),
        started_at_ms: 0,
    });
    for fresh in [false, true, false] {
        if fresh {
            let now = crate::now_ms();
            let policy = HostedPolicyBundle::from_json(&policy_json(4, now, true))
                .unwrap()
                .validate("org-a", "dep-a", now, None)
                .unwrap();
            *state.enterprise.hosted_policy.snapshot.write().unwrap() = Some(Arc::new(policy));
        } else {
            *state.enterprise.hosted_policy.snapshot.write().unwrap() = None;
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/global/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value, serde_json::json!({"healthy": fresh, "ready": fresh}));
    }
}
