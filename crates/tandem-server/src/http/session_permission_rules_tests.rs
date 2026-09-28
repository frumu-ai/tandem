// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

mod session_permission_rule_tests {
    use super::*;
    use tandem_enterprise_contract::hosted_policy::role_capabilities;
    use tandem_types::{AuthorityChain, HumanActor, TenantContextAssertionClaims};

    fn write_policy(path: &std::path::Path, version: u64, role: &str, now: u64) {
        std::fs::write(path, serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": version,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [{"id": "alice", "email": null, "username": null, "role": role,
                "capabilities": role_capabilities(role), "is_active": true, "email_verified": true}],
            "org_units": [], "org_unit_memberships": [], "deployment_grants": []
        })).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[tokio::test]
    async fn session_rules_reject_revoked_admin_on_create_and_update() {
        for update in [false, true] {
            let state = crate::test_support::test_state().await;
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("policy.json");
            state
                .enterprise
                .hosted_policy
                .configure_test_source("org-a", "dep-a", path.clone());
            let now = crate::now_ms();
            write_policy(&path, 1, "admin", now);
            state.reload_hosted_policy().await.unwrap();
            let tenant = TenantContext::explicit_user_workspace(
                "org-a",
                "dep-a",
                Some("dep-a".into()),
                "alice",
            );
            let mut claims = TenantContextAssertionClaims::new_v1(
                "tandem-web",
                "tandem-runtime",
                now,
                now + 60_000,
                "session-rules",
                tenant.clone(),
                HumanActor::tandem_user("alice"),
                AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                    "alice",
                    "tandem-web",
                )),
                vec!["hosted:role:admin".into()],
            );
            claims.policy_version = Some(1);
            claims.capabilities = role_capabilities("admin")
                .into_iter()
                .map(str::to_owned)
                .collect();
            let mut verified: VerifiedTenantContext = claims.into();
            state
                .enterprise
                .hosted_policy
                .project(&mut verified)
                .unwrap();
            assert!(session_permission_rules_allowed(&tenant, Some(&verified)));
            let mut session = Session::new(Some("unchanged".into()), None);
            session.tenant_context = tenant.clone();
            let id = session.id.clone();
            state.storage.save_session(session).await.unwrap();
            let sessions_before = state
                .storage
                .list_sessions()
                .await
                .into_iter()
                .map(|session| session.id)
                .collect::<std::collections::BTreeSet<_>>();
            let before = serde_json::to_value(state.permissions.list_rules().await).unwrap();
            write_policy(&path, 2, "member", now);
            state.reload_hosted_policy().await.unwrap();
            // A fresh member admission retains ordinary execution authority;
            // the in-flight admin admission remains bound to the old revision.
            let mut member = verified.clone();
            member.policy_version = Some(2);
            member.roles = vec!["hosted:role:member".into()];
            member.capabilities = role_capabilities("member")
                .into_iter()
                .map(str::to_owned)
                .collect();
            state.enterprise.hosted_policy.project(&mut member).unwrap();
            state
                .enterprise
                .hosted_policy
                .authorize_execution(Some(&member))
                .unwrap();
            let permission = json!([{"permission": "read", "pattern": "*", "action": "allow"}]);
            if update {
                let result = update_session(
                    State(state.clone()),
                    Extension(tenant.clone()),
                    Some(Extension(verified.clone())),
                    Path(id.clone()),
                    Json(
                        serde_json::from_value(json!({"title":"changed", "permission":permission}))
                            .unwrap(),
                    ),
                )
                .await;
                assert!(matches!(result, Err(StatusCode::FORBIDDEN)));
                assert_eq!(
                    state.storage.get_session(&id).await.unwrap().title,
                    "unchanged"
                );
            } else {
                let result = create_session(
                    State(state.clone()),
                    Extension(tenant.clone()),
                    Some(Extension(verified.clone())),
                    Json(serde_json::from_value(json!({"permission":permission})).unwrap()),
                )
                .await;
                assert!(matches!(result, Err((StatusCode::FORBIDDEN, _))));
                let sessions_after = state
                    .storage
                    .list_sessions()
                    .await
                    .into_iter()
                    .map(|session| session.id)
                    .collect::<std::collections::BTreeSet<_>>();
                assert_eq!(
                    sessions_after, sessions_before,
                    "a denied create must not leave an unreported session"
                );
            }
            assert_eq!(
                serde_json::to_value(state.permissions.list_rules().await).unwrap(),
                before
            );
            write_policy(&path, 3, "admin", now);
            state.reload_hosted_policy().await.unwrap();
            verified.policy_version = Some(3);
            state
                .enterprise
                .hosted_policy
                .project(&mut verified)
                .unwrap();
            apply_session_permission_rules_checked(
                &state,
                &tenant,
                &id,
                Some(&verified),
                Some(vec![
                    json!({"permission":"read", "pattern":"*", "action":"allow"}),
                ]),
            )
            .await
            .unwrap();
            assert_eq!(
                state.permissions.list_rules().await.len(),
                before.as_array().unwrap().len() + 1
            );
        }
    }
}
