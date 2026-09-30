// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

mod session_permission_batch_tests {
    use super::*;

    #[tokio::test]
    async fn session_permission_rule_batch_denial_has_no_live_or_durable_prefix() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        let path = state
            .routines_path
            .parent()
            .unwrap()
            .join("permissions.json");
        let before_file = tokio::fs::read(&path).await.unwrap();
        let before_rules = serde_json::to_value(state.permissions.list_rules().await).unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let guard_calls = calls.clone();
        let result = apply_session_permission_rules_with_authority(
            &state,
            &tenant,
            "batch-denied",
            Some(vec![
                json!({"permission":"read", "pattern":"*", "action":"allow"}),
                json!({"permission":"write", "pattern":"*", "action":"allow"}),
            ]),
            move |_| {
                guard_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(anyhow::Error::new(SessionPermissionAuthorityDenied))
            },
        )
        .await;
        assert_eq!(result, Err(StatusCode::FORBIDDEN));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            serde_json::to_value(state.permissions.list_rules().await).unwrap(),
            before_rules
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
        let guard_calls = calls.clone();
        apply_session_permission_rules_with_authority(
            &state,
            &tenant,
            "batch-denied",
            Some(vec![
                json!({"permission":" read ", "pattern":" * ", "action":"always"}),
                json!({"permission":"read", "pattern":"*", "action":"allow"}),
                json!({"permission":"write", "pattern":"*", "action":"reject"}),
                json!({"permission":"invalid", "action":"never"}),
                json!({"permission":"", "action":"allow"}),
            ]),
            move |commit| {
                guard_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                commit()
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            state.permissions.list_rules().await.len(),
            before_rules.as_array().unwrap().len() + 2
        );
        let reloaded =
            tandem_core::PermissionManager::new_with_state_file(tandem_core::EventBus::new(), path)
                .await
                .unwrap();
        assert_eq!(
            serde_json::to_value(reloaded.list_rules().await).unwrap(),
            serde_json::to_value(state.permissions.list_rules().await).unwrap()
        );
    }

    #[tokio::test]
    async fn session_permission_rule_batch_persistence_failure_rolls_back_create_and_update() {
        for update in [false, true] {
            let state = crate::test_support::test_state().await;
            let tenant = TenantContext::local_implicit();
            let existing = Session::new(Some("unchanged".into()), None);
            let id = existing.id.clone();
            state.storage.save_session(existing).await.unwrap();
            let before_sessions = state
                .storage
                .list_sessions()
                .await
                .into_iter()
                .map(|session| session.id)
                .collect::<std::collections::BTreeSet<_>>();
            let before_rules = serde_json::to_value(state.permissions.list_rules().await).unwrap();
            let root = state.routines_path.parent().unwrap();
            let path = root.join("permissions.json");
            let before_file = tokio::fs::read(&path).await.unwrap();
            let blocked = root.join("permission-state-is-a-directory");
            tokio::fs::create_dir(&blocked).await.unwrap();
            // The failed load configures the destination but leaves live rules
            // unchanged, exercising the real file-publication failure path.
            assert!(state.permissions.load_state_file(&blocked).await.is_err());
            let permission = json!([
                {"permission":"read", "pattern":"*", "action":"allow"},
                {"permission":"write", "pattern":"*", "action":"allow"}
            ]);
            if update {
                let result = update_session(
                    State(state.clone()),
                    Extension(tenant.clone()),
                    None,
                    Path(id.clone()),
                    Json(
                        serde_json::from_value(json!({"title":"changed", "permission":permission}))
                            .unwrap(),
                    ),
                )
                .await;
                assert!(matches!(result, Err(StatusCode::INTERNAL_SERVER_ERROR)));
                assert_eq!(
                    state.storage.get_session(&id).await.unwrap().title,
                    "unchanged"
                );
            } else {
                let result = create_session(
                    State(state.clone()),
                    Extension(tenant.clone()),
                    None,
                    Json(serde_json::from_value(json!({"permission":permission})).unwrap()),
                )
                .await;
                assert!(matches!(
                    result,
                    Err((StatusCode::INTERNAL_SERVER_ERROR, _))
                ));
                let after_sessions = state
                    .storage
                    .list_sessions()
                    .await
                    .into_iter()
                    .map(|session| session.id)
                    .collect::<std::collections::BTreeSet<_>>();
                assert_eq!(after_sessions, before_sessions);
            }
            assert_eq!(
                serde_json::to_value(state.permissions.list_rules().await).unwrap(),
                before_rules
            );
            assert_eq!(tokio::fs::read(&path).await.unwrap(), before_file);
            assert!(
                blocked.is_dir(),
                "a failed publish must not remove its destination"
            );
            state.permissions.load_state_file(path).await.unwrap();
            apply_session_permission_rules_checked(
                &state,
                &tenant,
                &id,
                None,
                Some(vec![
                    json!({"permission":"read", "pattern":"*", "action":"allow"}),
                    json!({"permission":"write", "pattern":"*", "action":"allow"}),
                ]),
            )
            .await
            .unwrap();
            assert_eq!(
                state.permissions.list_rules().await.len(),
                before_rules.as_array().unwrap().len() + 2
            );
        }
    }

    #[tokio::test]
    async fn session_permission_rule_batch_absent_empty_and_invalid_inputs_are_noops() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        let before = serde_json::to_value(state.permissions.list_rules().await).unwrap();
        for rules in [
            None,
            Some(Vec::new()),
            Some(vec![json!({"action":"allow"}), json!("invalid")]),
        ] {
            apply_session_permission_rules_with_authority(
                &state,
                &tenant,
                "session",
                rules,
                |_| panic!("no valid rules must not invoke the commit guard"),
            )
            .await
            .unwrap();
        }
        assert_eq!(
            serde_json::to_value(state.permissions.list_rules().await).unwrap(),
            before
        );
    }
}
