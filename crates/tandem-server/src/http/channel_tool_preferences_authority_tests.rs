// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_enterprise_contract::hosted_policy::{role_capabilities, HostedPolicyBundle};
use tandem_types::{
    AuthorityChain, HumanActor, TenantContextAssertionClaims, VerifiedTenantContext,
};

fn hosted_bundle(role: &str, version: u64) -> HostedPolicyBundle {
    serde_json::from_value(json!({
        "schema_version":1, "policy_version":version,
        "organization_id":"org-a", "deployment_id":"dep-a",
        "generated_at":chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
        "users":[{"id":"alice", "email":null, "username":null, "role":role,
            "capabilities":role_capabilities(role), "is_active":true, "email_verified":true}],
        "org_units":[], "org_unit_memberships":[], "deployment_grants":[]
    }))
    .unwrap()
}

fn write_hosted_bundle(path: &std::path::Path, bundle: &HostedPolicyBundle) {
    std::fs::write(path, serde_json::to_vec(bundle).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn hosted_fixture(
    role: &str,
    directory: &std::path::Path,
) -> (AppState, VerifiedTenantContext) {
    let state = crate::test_support::test_state().await;
    let path = directory.join("policy.json");
    let now = crate::now_ms();
    write_hosted_bundle(&path, &hosted_bundle(role, 1));
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
        let (state, verified) = hosted_fixture("admin", directory.path()).await;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        ready_rx.recv().unwrap();
        let map = ToolPreferencesMap::new();
        let save = save_tool_preferences_map(path.clone(), &map, state.clone(), Some(verified));
        tokio::pin!(save);
        let first = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(save.as_mut(), cx))
        })
        .await;
        // Publication occurs while the sole blocking worker is occupied. The
        // queued preference commit must see this newer, non-admin snapshot.
        state
            .enterprise
            .hosted_policy
            .install_test_bundle(hosted_bundle("viewer", 2))
            .unwrap();
        release_tx.send(()).unwrap();
        assert!(first.is_pending());
        assert_eq!(save.await, Err(StatusCode::FORBIDDEN));
        blocker.await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channel_preferences_hold_policy_through_cancelled_caller_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let policy_path = directory.path().join("policy.json");
    let (state, verified) = hosted_fixture("admin", directory.path()).await;
    let mut map = ToolPreferencesMap::new();
    map.insert("slack".into(), ChannelToolPreferences::default());
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let guarded_state = state.clone();
    let write_path = path.clone();
    let write_state = state.clone();
    let write_map = map.clone();
    let write = tokio::spawn(async move {
        save_tool_preferences_map_with_hook(
            write_path,
            &write_map,
            write_state,
            Some(verified),
            move || {
                let guarded = guarded_state
                    .enterprise
                    .hosted_policy
                    .publication_write_blocked_for_test();
                let _ = entered_tx.send(guarded);
                release_rx.recv().unwrap();
            },
        )
        .await
    });
    let guarded = tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    write.abort();
    assert!(write.await.unwrap_err().is_cancelled());
    write_hosted_bundle(&policy_path, &hosted_bundle("viewer", 2));
    let reload_state = state.clone();
    let mut reload = tokio::spawn(async move { reload_state.reload_hosted_policy().await });
    let reload_was_pending =
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut reload)
            .await
            .is_err();
    release_tx.send(()).unwrap();
    reload.await.unwrap().unwrap();
    assert!(guarded, "commit must hold the policy snapshot read guard");
    assert!(
        reload_was_pending,
        "revocation must wait for the durable write"
    );
    assert_eq!(load_tool_preferences_map(&path).await, map);
    assert_eq!(
        authorize_tool_preferences_write(&state, None),
        Err(StatusCode::FORBIDDEN)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channel_preferences_recheck_claim_expiry_after_write_pause() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    std::fs::write(&path, b"original").unwrap();
    let (state, mut verified) = hosted_fixture("admin", directory.path()).await;
    let expires_at_ms = crate::now_ms() + 3_000;
    verified.expires_at_ms = expires_at_ms;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let write_path = path.clone();
    let write = tokio::spawn(async move {
        save_tool_preferences_map_with_hook(
            write_path,
            &ToolPreferencesMap::new(),
            state,
            Some(verified),
            move || {
                let _ = entered_tx.send(());
                release_rx.recv().unwrap();
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .expect("initial admin check did not reach the write pause")
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(
        expires_at_ms.saturating_sub(crate::now_ms()) + 1,
    ))
    .await;
    let expired = crate::now_ms() >= expires_at_ms;
    release_tx.send(()).unwrap();
    assert!(expired, "the claim must expire before the final check");
    assert_eq!(write.await.unwrap(), Err(StatusCode::FORBIDDEN));
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
}

#[tokio::test]
async fn channel_preferences_report_io_errors() {
    let directory = tempfile::tempdir().unwrap();
    let state = crate::test_support::test_state().await;
    assert_eq!(
        save_tool_preferences_map(
            directory.path().to_owned(),
            &ToolPreferencesMap::new(),
            state,
            None,
        )
        .await,
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    );
}
