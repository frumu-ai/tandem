// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::legacy_routine_authority::{hosted_state, tenant, verified};
use super::*;
use axum::{routing::get, Extension, Router};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
#[serial_test::serial]
async fn request_triggered_oauth_refresh_stops_after_hosted_use_revocation() {
    let (state, policy_dir) = hosted_state().await;
    let tenant = tenant("alice");
    let mut identity = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted identity");

    let security_dir = super::super::config_providers::provider_auth_security_dir_for_state(&state);
    let original = tandem_core::OAuthProviderCredential {
        provider_id: "openai-codex".to_string(),
        access_token: "original-access".to_string(),
        refresh_token: "original-refresh".to_string(),
        expires_at_ms: 1,
        account_id: Some("alice-account".to_string()),
        email: None,
        display_name: None,
        managed_by: "tandem".to_string(),
        api_key: Some("original-api-key".to_string()),
    };
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        &tenant,
        "openai-codex",
        original.clone(),
    )
    .expect("save original OAuth credential");

    let policy_path = policy_dir.path().join("policy.json");
    let reload_state = state.clone();
    let refresh_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&refresh_calls);
    let result = super::super::config_providers::refresh_openai_codex_oauth_for_request_with(
        &state,
        &tenant,
        &identity,
        move |mut credential| async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let mut policy: Value =
                serde_json::from_slice(&std::fs::read(&policy_path).expect("read hosted policy"))
                    .expect("policy JSON");
            policy["policy_version"] = json!(2);
            let alice = policy["users"]
                .as_array_mut()
                .expect("policy users")
                .iter_mut()
                .find(|user| user["id"] == "alice")
                .expect("alice policy entry");
            alice["is_active"] = json!(false);
            std::fs::write(
                &policy_path,
                serde_json::to_vec(&policy).expect("updated policy JSON"),
            )
            .expect("write revoked policy");
            reload_state
                .reload_hosted_policy()
                .await
                .expect("reload revoked policy");
            credential.access_token = "revoked-access".to_string();
            credential.api_key = Some("revoked-api-key".to_string());
            credential.expires_at_ms = crate::now_ms().saturating_add(60_000);
            Ok(credential)
        },
    )
    .await;
    assert!(result.is_err(), "revoked refresh must not commit");
    assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &security_dir,
            &tenant,
            "openai-codex",
        ),
        Some(original),
    );

    let denied_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&denied_calls);
    let denied = super::super::config_providers::refresh_openai_codex_oauth_for_request_with(
        &state,
        &tenant,
        &identity,
        move |credential| async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(credential)
        },
    )
    .await;
    assert!(
        denied.is_err(),
        "stale identity must be rejected before network"
    );
    assert_eq!(denied_calls.load(Ordering::SeqCst), 0);

    let status_app = Router::new()
        .route(
            "/provider/auth",
            get(super::super::config_providers::provider_auth),
        )
        .route(
            "/provider/{id}/oauth/status",
            get(super::super::config_providers::provider_oauth_status),
        )
        .layer(Extension(tenant))
        .layer(Extension(identity))
        .with_state(state);
    for path in ["/provider/auth", "/provider/openai-codex/oauth/status"] {
        let response = status_app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .expect("revoked provider status response");
        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn revoked_policy_cannot_publish_bearer_after_registry_lock_wait() {
    let (state, policy_dir) = hosted_state().await;
    let tenant = tenant("alice");
    let mut identity = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted identity");
    super::super::config_providers::publish_openai_codex_runtime_token_for_request_test(
        &state,
        &tenant,
        &identity,
        "authorized-token".into(),
    )
    .await
    .expect("initial authorized publication");
    let initial = state
        .providers
        .tenant_provider_bearer_token_snapshot(&tenant, "openai-codex")
        .await;

    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let registry = state.providers.clone();
    let held_tenant = tenant.clone();
    let handle = tokio::runtime::Handle::current();
    let holder = tokio::task::spawn_blocking(move || {
        handle.block_on(registry.set_tenant_provider_bearer_token_guarded(
            &held_tenant,
            "openai-codex",
            "unused-token".into(),
            move |_commit| {
                locked_tx.send(()).expect("signal held registry lock");
                release_rx.recv().expect("release registry lock");
                anyhow::bail!("lock-holder intentionally skipped publication")
            },
        ))
    });
    locked_rx.await.expect("registry lock acquired");

    let publish_state = state.clone();
    let publish_tenant = tenant.clone();
    let publish_identity = identity.clone();
    let pending = tokio::spawn(async move {
        super::super::config_providers::publish_openai_codex_runtime_token_for_request_test(
            &publish_state,
            &publish_tenant,
            &publish_identity,
            "revoked-token".into(),
        )
        .await
    });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished(), "publication waits on registry lock");

    let policy_path = policy_dir.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("read hosted policy"))
            .expect("policy JSON");
    policy["policy_version"] = json!(2);
    let alice = policy["users"]
        .as_array_mut()
        .expect("policy users")
        .iter_mut()
        .find(|user| user["id"] == "alice")
        .expect("alice policy entry");
    alice["is_active"] = json!(false);
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&policy).expect("updated policy JSON"),
    )
    .expect("write revoked policy");
    state
        .reload_hosted_policy()
        .await
        .expect("reload revoked policy");

    release_tx.send(()).expect("release registry lock");
    assert!(holder.await.expect("holder task").is_err());
    assert!(
        pending.await.expect("publisher task").is_err(),
        "revoked use must reject publication inside the registry write lock"
    );
    assert!(
        state
            .providers
            .clear_tenant_provider_bearer_token_if_unchanged(&tenant, "openai-codex", &initial)
            .await,
        "denied publication must retain the previously authorized bearer"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn revoked_execution_does_not_hydrate_the_tenant_bearer_cache() {
    let (state, policy_dir) = hosted_state().await;
    let tenant = tenant("alice");
    let mut identity = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted identity");
    let security_dir = super::super::config_providers::provider_auth_security_dir_for_state(&state);
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        &tenant,
        "openai-codex",
        tandem_core::OAuthProviderCredential {
            provider_id: "openai-codex".into(),
            access_token: "stored-token".into(),
            refresh_token: "stored-refresh".into(),
            expires_at_ms: crate::now_ms().saturating_add(60_000),
            account_id: None,
            email: None,
            display_name: None,
            managed_by: "tandem".into(),
            api_key: None,
        },
    )
    .expect("store tenant credential");

    let policy_path = policy_dir.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("read hosted policy"))
            .expect("policy JSON");
    policy["policy_version"] = json!(2);
    let alice = policy["users"]
        .as_array_mut()
        .expect("policy users")
        .iter_mut()
        .find(|user| user["id"] == "alice")
        .expect("alice policy entry");
    alice["is_active"] = json!(false);
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&policy).expect("updated policy JSON"),
    )
    .expect("write revoked policy");
    state
        .reload_hosted_policy()
        .await
        .expect("reload revoked policy");

    crate::http::session_run_retry::scope_provider_auth_for_tenant(
        &state,
        &tenant,
        Some(&identity),
        crate::http::session_run_retry::PromptExecutionSurface::MissionBuilder,
        None,
        None,
        Some("openai-codex"),
        async {},
    )
    .await;
    assert!(
        !state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await,
        "the request-scoped loader must not use Internal authority"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn loaded_oauth_credential_must_still_be_persisted_when_bearer_is_published() {
    let (state, _policy_dir) = hosted_state().await;
    let tenant = tenant("alice");
    let mut identity = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted identity");
    let security_dir = super::super::config_providers::provider_auth_security_dir_for_state(&state);
    let original = tandem_core::OAuthProviderCredential {
        provider_id: "openai-codex".into(),
        access_token: "original-access".into(),
        refresh_token: "original-refresh".into(),
        expires_at_ms: crate::now_ms().saturating_add(60_000),
        account_id: None,
        email: None,
        display_name: None,
        managed_by: "tandem".into(),
        api_key: None,
    };
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        &tenant,
        "openai-codex",
        original.clone(),
    )
    .expect("save original OAuth credential");

    assert!(
        super::super::config_providers::publish_loaded_openai_codex_runtime_token_for_request_test(
            &state,
            &tenant,
            &identity,
            &original,
            original.access_token.clone(),
        )
        .await
        .expect("current credential publication"),
        "the unchanged credential must still publish normally"
    );
    assert!(
        state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await
    );

    tandem_core::delete_provider_credential_for_tenant_in_dir_serialized(
        &security_dir,
        &tenant,
        "openai-codex",
    )
    .await
    .expect("delete persisted OAuth credential");
    state
        .providers
        .clear_tenant_provider_bearer_token(&tenant, "openai-codex")
        .await;
    assert!(
        !super::super::config_providers::publish_loaded_openai_codex_runtime_token_for_request_test(
            &state,
            &tenant,
            &identity,
            &original,
            original.access_token.clone(),
        )
        .await
        .expect("deleted credential must be a no-op"),
        "a deleted credential must not be republished by an in-flight load"
    );
    assert!(
        !state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await
    );

    let mut replacement = original.clone();
    replacement.refresh_token = "replacement-refresh".into();
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        &tenant,
        "openai-codex",
        replacement.clone(),
    )
    .expect("save replacement OAuth credential");
    assert!(
        !super::super::config_providers::publish_loaded_openai_codex_runtime_token_for_request_test(
            &state,
            &tenant,
            &identity,
            &original,
            original.access_token.clone(),
        )
        .await
        .expect("replaced credential must be a no-op"),
        "a replaced credential must not be republished even if the bearer value is unchanged"
    );
    assert!(
        super::super::config_providers::publish_loaded_openai_codex_runtime_token_for_request_test(
            &state,
            &tenant,
            &identity,
            &replacement,
            replacement.access_token.clone(),
        )
        .await
        .expect("replacement credential publication"),
        "the current replacement credential must still publish normally"
    );
}
