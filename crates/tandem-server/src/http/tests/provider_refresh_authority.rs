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
