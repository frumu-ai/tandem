// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tandem_enterprise_contract::hosted_policy::role_capabilities;
use tandem_types::{
    AuthorityChain, HumanActor, TenantContextAssertionClaims, VerifiedTenantContext,
};

async fn hosted_fixture(
    role: &str,
    directory: &std::path::Path,
) -> (AppState, VerifiedTenantContext) {
    let state = crate::test_support::test_state().await;
    let path = directory.join("policy.json");
    let now = crate::now_ms();
    std::fs::write(&path, serde_json::to_vec(&json!({
        "schema_version":1, "policy_version":1, "organization_id":"org-a", "deployment_id":"dep-a",
        "generated_at":chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
        "users":[{"id":"alice", "email":null, "username":null, "role":role,
            "capabilities":role_capabilities(role), "is_active":true, "email_verified":true}],
        "org_units":[], "org_unit_memberships":[], "deployment_grants":[]
    })).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path);
    state.reload_hosted_policy().await.unwrap();
    let tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        "channel-preferences",
        tenant,
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user("alice", "tandem-web")),
        vec![format!("hosted:role:{role}")],
    );
    claims.policy_version = Some(1);
    claims.capabilities = role_capabilities(role)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut verified = claims.into();
    state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .unwrap();
    (state, verified)
}

#[tokio::test]
async fn channel_preferences_require_admin_for_base_scoped_and_reset_writes() {
    for role in ["viewer", "admin"] {
        let directory = tempfile::tempdir().unwrap();
        let (state, verified) = hosted_fixture(role, directory.path()).await;
        let path = directory.path().join("preferences.json");
        let original = serde_json::to_vec(&json!({"slack":{"enabled_tools":["read"]}})).unwrap();
        for scope in [None, Some("thread-a")] {
            for reset in [false, true] {
                std::fs::write(&path, &original).unwrap();
                let result = channel_tool_preferences_put_at_path(
                    &state,
                    Some(&verified),
                    "slack".into(),
                    serde_json::from_value(json!({"scope_id":scope})).unwrap(),
                    serde_json::from_value(json!({"enabled_tools":["write"], "reset":reset}))
                        .unwrap(),
                    path.clone(),
                )
                .await;
                if role == "viewer" {
                    assert!(matches!(result, Err(StatusCode::FORBIDDEN)));
                    assert_eq!(std::fs::read(&path).unwrap(), original);
                } else {
                    let prefs = result.unwrap().0;
                    let saved = load_tool_preferences_map(&path).await;
                    let key = scope
                        .map(|scope| format!("slack:{scope}"))
                        .unwrap_or("slack".into());
                    assert_eq!(saved.get(&key), Some(&prefs));
                    if reset {
                        assert_eq!(prefs, ChannelToolPreferences::default());
                    } else {
                        assert_eq!(prefs.enabled_tools, vec!["write"]);
                    }
                    if scope.is_some() {
                        assert_eq!(saved["slack"].enabled_tools, vec!["read"]);
                    }
                }
            }
        }
        assert_eq!(
            authorize_tool_preferences_write(&state, None),
            Err(StatusCode::FORBIDDEN)
        );
    }
}

#[tokio::test]
async fn channel_preferences_get_is_read_only_and_local_updates_still_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let state = crate::test_support::test_state().await;
    let original = serde_json::to_vec(&json!({
        "slack":{"enabled_tools":["read", "read"]},
        "slack:thread-a":{"enabled_tools":["write", "write"]}
    }))
    .unwrap();
    std::fs::write(&path, &original).unwrap();
    for scope in [None, Some("thread-a")] {
        let prefs = channel_tool_preferences_get_at_path(
            &state,
            "slack".into(),
            serde_json::from_value(json!({"scope_id":scope})).unwrap(),
            &path,
        )
        .await
        .unwrap()
        .0;
        assert_eq!(
            prefs.enabled_tools,
            if scope.is_some() {
                vec!["read", "write"]
            } else {
                vec!["read"]
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let prefs = channel_tool_preferences_put_at_path(
        &state,
        None,
        "slack".into(),
        serde_json::from_value(json!({})).unwrap(),
        serde_json::from_value(json!({"enabled_tools":["websearch"]})).unwrap(),
        path.clone(),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(prefs.enabled_tools, vec!["websearch"]);
    assert_eq!(load_tool_preferences_map(&path).await["slack"], prefs);
}

#[test]
fn channel_preferences_recheck_after_blocking_queue_wait() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        std::fs::write(&path, b"original").unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        ready_rx.recv().unwrap();
        let allowed = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let checked_allowed = allowed.clone();
        let checked_calls = calls.clone();
        let map = ToolPreferencesMap::new();
        let save = save_tool_preferences_map(path.clone(), &map, move || {
            checked_calls.fetch_add(1, Ordering::SeqCst);
            if checked_allowed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        });
        tokio::pin!(save);
        let first = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(save.as_mut(), cx))
        })
        .await;
        // Release the worker even if a subsequent assertion fails.
        allowed.store(false, Ordering::SeqCst);
        release_tx.send(()).unwrap();
        assert!(first.is_pending());
        assert_eq!(save.await, Err(StatusCode::FORBIDDEN));
        blocker.await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    });
}

#[tokio::test]
async fn channel_preferences_recheck_before_write_and_report_io_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    std::fs::write(&path, b"original").unwrap();
    let calls = AtomicUsize::new(0);
    assert_eq!(
        save_tool_preferences_map(path.clone(), &ToolPreferencesMap::new(), move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        })
        .await,
        Err(StatusCode::FORBIDDEN)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(
        save_tool_preferences_map(
            directory.path().to_owned(),
            &ToolPreferencesMap::new(),
            || Ok(())
        )
        .await,
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    );
}
