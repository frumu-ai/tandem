// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
};

#[tokio::test]
#[serial_test::serial(data_boundary_env)]
async fn hosted_policy_direct_provider_rechecks_after_real_approval() {
    use crate::http::session_run_retry::{
        provider_auth_test_support::install_capturing_codex_provider,
        scope_provider_auth_for_tenant, PromptExecutionSurface,
    };
    use crate::provider_egress::{prepare_chat_messages, ServerProviderEgressKind};
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }
    let values = [
        ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
        ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
        (
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "openai-codex=approved_external",
        ),
        ("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES", "customer_data"),
    ];
    let _restore = Restore(
        values
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect(),
    );
    for (name, value) in values {
        std::env::set_var(name, value);
    }
    for revoked in [false, true] {
        let state = crate::test_support::test_state().await;
        let verified = identity(4);
        let tenant = verified.tenant_context.clone();
        let now = crate::now_ms();
        let policy = HostedPolicyBundle::from_json(&policy_json(4, now, true)).unwrap();
        *state.enterprise.hosted_policy.source.write().unwrap() = Some(PolicySource {
            organization_id: "org-a".into(),
            deployment_id: "dep-a".into(),
            path: PathBuf::from("unused"),
            started_at_ms: 0,
        });
        *state.enterprise.hosted_policy.snapshot.write().unwrap() = Some(Arc::new(
            policy.validate("org-a", "dep-a", now, None).unwrap(),
        ));
        let sends = install_capturing_codex_provider(
            &state,
            "synthetic output",
            &[(&tenant, "synthetic-access-token")],
        )
        .await;
        let mut events = state.event_bus.subscribe();
        let permissions = state.runtime.wait().permissions.clone();
        let reply_state = state.clone();
        let reply_tenant = tenant.clone();
        let responder = tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(5), async move {
                loop {
                    let event = events.recv().await.unwrap();
                    if event.event_type != "permission.asked" {
                        continue;
                    }
                    if revoked {
                        let changed =
                            HostedPolicyBundle::from_json(&policy_json(5, crate::now_ms(), false))
                                .unwrap();
                        *reply_state
                            .enterprise
                            .hosted_policy
                            .snapshot
                            .write()
                            .unwrap() = Some(Arc::new(
                            changed
                                .validate("org-a", "dep-a", crate::now_ms(), None)
                                .unwrap(),
                        ));
                    }
                    assert!(permissions
                        .reply_with_provenance_for_tenant(
                            &reply_tenant,
                            Some("session-direct"),
                            event.properties["requestID"].as_str().unwrap(),
                            "allow",
                            Some("independent-reviewer".into()),
                            Some("hosted-policy-test".into())
                        )
                        .await
                        .unwrap()
                        .is_some());
                    break;
                }
            })
            .await
            .unwrap();
        });
        let dispatch = async {
            let messages = [tandem_providers::ChatMessage {
                role: "user".into(),
                content: "ordinary mission content".into(),
                attachments: vec![],
            }];
            let prepared = prepare_chat_messages(
                &state,
                Some(&tenant),
                Some(&verified),
                Some("run-direct"),
                "session-direct",
                "operation-direct",
                "server.mission_builder",
                ServerProviderEgressKind::MissionBuilder,
                "openai-codex",
                Some("codex-test"),
                &messages,
            )
            .await
            .map_err(anyhow::Error::msg)?;
            state
                .providers
                .stream_with_egress_permit(
                    &prepared.permit,
                    Some("openai-codex"),
                    Some("codex-test"),
                    prepared.messages,
                    tandem_types::ToolMode::None,
                    None,
                    tandem_types::SamplingParams::default(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .map(|_| ())
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            scope_provider_auth_for_tenant(
                &state,
                &tenant,
                Some(&verified),
                PromptExecutionSurface::MissionBuilder,
                Some("session-direct"),
                Some("run-direct"),
                Some("openai-codex"),
                dispatch,
            ),
        )
        .await
        .unwrap();
        responder.await.unwrap();
        assert_eq!(result.is_ok(), !revoked, "{result:?}");
        assert_eq!(sends.lock().unwrap().len(), usize::from(!revoked));
    }
}

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
async fn hosted_policy_registry_view_replaces_human_memberships_without_persisting_imports() {
    use tandem_enterprise_contract::{
        hosted_policy::{hosted_unit_principal, HOSTED_TAXONOMY_ID},
        OrganizationUnit, OrganizationUnitAccessGrant, OrganizationUnitKind,
        OrganizationUnitMembership, OrganizationUnitMembershipSource, PrincipalKind, PrincipalRef,
        ResourceKind, ResourceRef,
    };
    let state = crate::test_support::test_state().await;
    let tenant = identity(4).tenant_context;
    let now = crate::now_ms();
    let local = OrganizationUnit::active(
        "local",
        tenant.clone(),
        "Local",
        OrganizationUnitKind::Team,
        PrincipalRef::human_user("operator"),
        now,
    );
    state
        .enterprise
        .org_units
        .write()
        .await
        .insert("local".into(), local.clone());
    for (id, member) in [
        ("old-human", PrincipalRef::human_user("alice")),
        (
            "service",
            PrincipalRef::new(PrincipalKind::ServiceAccount, "automation"),
        ),
    ] {
        state.enterprise.org_unit_memberships.write().await.insert(
            id.into(),
            OrganizationUnitMembership::active(
                id,
                tenant.clone(),
                local.principal_ref(),
                member,
                OrganizationUnitMembershipSource::Direct,
                now,
            ),
        );
    }
    let native_grant = OrganizationUnitAccessGrant::active(
        "data",
        tenant.clone(),
        hosted_unit_principal("eng"),
        ResourceRef::new("org-a", "dep-a", ResourceKind::Document, "engineering"),
        now,
    )
    .with_permissions(vec![AccessPermission::Read]);
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert("data".into(), native_grant.clone());
    assert_eq!(
        state
            .enterprise_org_unit_view(&tenant)
            .await
            .unwrap()
            .memberships
            .len(),
        2
    );
    *state.enterprise.hosted_policy.source.write().unwrap() = Some(PolicySource {
        organization_id: "org-a".into(),
        deployment_id: "dep-a".into(),
        path: PathBuf::from("unused"),
        started_at_ms: 0,
    });
    assert!(state.enterprise_org_unit_view(&tenant).await.is_err());
    let mut input = HostedPolicyBundle::from_json(&policy_json(4, now, true)).unwrap();
    input.org_units = serde_json::from_value(serde_json::json!([
        {"id":"eng", "slug":"eng", "display_name":"Engineering", "kind":"department", "state":"active"}])).unwrap();
    input.org_unit_memberships =
        serde_json::from_value(serde_json::json!([{"unit_id":"eng", "user_id":"alice"}])).unwrap();
    *state.enterprise.hosted_policy.snapshot.write().unwrap() = Some(Arc::new(
        input.clone().validate("org-a", "dep-a", now, None).unwrap(),
    ));
    let view = state.enterprise_org_unit_view(&tenant).await.unwrap();
    assert_eq!(view.hosted_policy_revision.unwrap().version, 4);
    assert_eq!(view.units.len(), 2);
    assert_eq!(view.memberships.len(), 2);
    assert!(!view
        .memberships
        .iter()
        .any(|row| row.membership_id == "old-human"));
    let hosted_member = view
        .memberships
        .iter()
        .find(|row| row.source == OrganizationUnitMembershipSource::HostedControlPlane)
        .unwrap();
    assert!(view.access_grants[0]
        .to_scoped_grant_for_membership(hosted_member, now)
        .is_some());
    input.policy_version = 5;
    input.org_unit_memberships.clear();
    *state.enterprise.hosted_policy.snapshot.write().unwrap() = Some(Arc::new(
        input.validate("org-a", "dep-a", now, None).unwrap(),
    ));
    let removed = state.enterprise_org_unit_view(&tenant).await.unwrap();
    assert_eq!(removed.hosted_policy_revision.unwrap().version, 5);
    assert_eq!(removed.memberships.len(), 1);
    assert_eq!(removed.memberships[0].membership_id, "service");
    assert_eq!(removed.access_grants, vec![native_grant]);
    assert_eq!(state.enterprise.org_units.read().await.len(), 1);
    assert_eq!(state.enterprise.org_unit_memberships.read().await.len(), 2);
    let other =
        TenantContext::explicit_user_workspace("other", "dep-a", Some("dep-a".into()), "alice");
    assert!(state.enterprise_org_unit_view(&other).await.is_err());
    state.enterprise.org_units.write().await.insert(
        "conflict".into(),
        local.with_taxonomy_id(HOSTED_TAXONOMY_ID),
    );
    assert!(matches!(
        state.enterprise_org_unit_view(&tenant).await,
        Err("hosted_registry_ownership_conflict")
    ));
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
