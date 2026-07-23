// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{routing::get, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use uuid::Uuid;

fn make_jwt(payload: Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).expect("payload json"));
    format!("{header}.{payload}.signature")
}

fn test_codex_oauth(label: &str) -> tandem_core::OAuthProviderCredential {
    tandem_core::OAuthProviderCredential {
        provider_id: "openai-codex".to_string(),
        access_token: format!("access-{label}"),
        refresh_token: format!("refresh-{label}"),
        expires_at_ms: 2_000_000_000_000,
        account_id: Some(format!("account-{label}")),
        email: Some(format!("{label}@example.com")),
        display_name: None,
        managed_by: "tandem".to_string(),
        api_key: Some(format!("api-{label}")),
    }
}

fn provider_auth_security_dir(state: &AppState) -> std::path::PathBuf {
    state
        .memory_db_path
        .parent()
        .expect("memory db parent")
        .join("security")
}

async fn make_protected_audit_unwritable(state: &AppState) {
    tokio::fs::create_dir_all(&state.protected_audit_path)
        .await
        .expect("make protected audit path unwritable as a file");
    crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;
}

async fn runtime_provider_api_key(state: &AppState, provider_id: &str) -> Option<String> {
    state
        .config
        .get_layers_value()
        .await
        .get("runtime")
        .and_then(|runtime| runtime.get("providers"))
        .and_then(|providers| providers.get(provider_id))
        .and_then(|provider| provider.get("api_key"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn direct_loopback_request() -> axum::http::request::Builder {
    Request::builder().extension(axum::extract::ConnectInfo(
        "127.0.0.1:39731"
            .parse::<std::net::SocketAddr>()
            .expect("loopback socket address"),
    ))
}

fn verified_deployment_admin(
    tenant_context: tandem_types::TenantContext,
    actor_id: &str,
) -> tandem_types::VerifiedTenantContext {
    let now = crate::now_ms();
    tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user(actor_id),
        authority_chain: tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(actor_id, "provider-test"),
        ),
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: vec!["deployment.admin".to_string()],
        policy_version: None,
        strict_projection: None,
        issuer: "provider-test".to_string(),
        audience: "tandem".to_string(),
        issued_at_ms: now,
        expires_at_ms: now.saturating_add(60_000),
        assertion_id: format!("provider-test-{actor_id}-{now}"),
        assertion_key_id: None,
    }
}

fn tenant_request(
    method: &str,
    uri: &str,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let tenant_context =
        tandem_types::TenantContext::explicit_user_workspace(org_id, workspace_id, None, actor_id);
    let mut builder = Request::builder()
        .extension(verified_deployment_admin(tenant_context, actor_id))
        .method(method)
        .uri(uri)
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .expect("tenant request")
}

async fn json_body(resp: axum::response::Response) -> Value {
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    serde_json::from_slice(&body).expect("json")
}

fn provider_status<'a>(payload: &'a Value, provider_id: &str) -> &'a Value {
    payload
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(provider_id))
        .unwrap_or_else(|| panic!("{provider_id} provider auth"))
}

#[tokio::test]
async fn provider_route_returns_known_providers_without_synthetic_default_models() {
    let state = test_state().await;
    let app = app_router(state);
    let req = tenant_request("GET", "/provider", "org-a", "workspace-a", "user-a", None);
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let all = payload
        .get("all")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let openai = all
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        .cloned()
        .expect("openai entry");

    assert_eq!(
        openai
            .get("models")
            .and_then(Value::as_object)
            .map(|m| m.len()),
        Some(0)
    );
    assert_eq!(
        openai.get("catalog_source").and_then(Value::as_str),
        Some("empty")
    );
    assert_eq!(
        openai.get("catalog_status").and_then(Value::as_str),
        Some("unavailable")
    );

    let openai_codex = all
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai-codex"))
        .cloned()
        .expect("openai-codex entry");
    let codex_models = openai_codex
        .get("models")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(codex_models.contains_key("gpt-5.6-sol"));
    assert!(codex_models.contains_key("gpt-5.6-terra"));
    assert!(codex_models.contains_key("gpt-5.6-luna"));
    assert!(!codex_models.contains_key("gpt-5.6"));
    assert!(codex_models.contains_key("gpt-5.5"));
    assert!(codex_models.contains_key("gpt-5.4"));
    assert!(codex_models.contains_key("gpt-5.2-codex"));
    assert!(codex_models.contains_key("gpt-5.4-mini"));
    assert!(codex_models.contains_key("gpt-5.3-codex"));
    assert!(codex_models.contains_key("gpt-5.3-codex-spark"));
    assert!(codex_models.contains_key("gpt-5.1-codex-mini"));
    // Retired phantom model must not reappear in the catalog.
    assert!(!codex_models.contains_key("gpt-5.1-codex-max"));
    assert_eq!(
        openai_codex.get("catalog_source").and_then(Value::as_str),
        Some("static")
    );
    assert_eq!(
        openai_codex.get("catalog_status").and_then(Value::as_str),
        Some("ok")
    );
}

#[tokio::test]
async fn config_patch_preserves_saved_codex_default_over_runtime_provider() {
    let state = test_state().await;
    state
        .config
        .patch_runtime(json!({
            "providers": {
                "openai-codex": {
                    "url": "https://chatgpt.com/backend-api/codex",
                    "default_model": "gpt-5.4"
                }
            }
        }))
        .await
        .expect("runtime codex provider");

    let app = app_router(state);
    let req = direct_loopback_request()
        .method("PATCH")
        .uri("/config")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "default_provider": "openai-codex",
                "providers": {
                    "openai-codex": { "default_model": "gpt-5.5" }
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    assert_eq!(
        payload
            .get("effective")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("openai-codex"))
            .and_then(|v| v.get("default_model"))
            .and_then(Value::as_str),
        Some("gpt-5.5")
    );

    let req = direct_loopback_request()
        .method("GET")
        .uri("/config/providers")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    assert_eq!(
        payload
            .get("providers")
            .and_then(|v| v.get("openai-codex"))
            .and_then(|v| v.get("default_model"))
            .and_then(Value::as_str),
        Some("gpt-5.5")
    );
}

#[tokio::test]
async fn config_providers_heals_retired_codex_default_model() {
    let state = test_state().await;
    // Reproduce a deployment that persisted the retired phantom model as its
    // openai-codex default — the exact state that broke channel dispatches.
    state
        .config
        .patch_runtime(json!({
            "providers": {
                "openai-codex": {
                    "url": "https://chatgpt.com/backend-api/codex",
                    "default_model": "gpt-5.1-codex-max"
                }
            }
        }))
        .await
        .expect("runtime codex provider");

    let app = app_router(state);
    let req = direct_loopback_request()
        .method("GET")
        .uri("/config/providers")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    // Channel dispatchers read this endpoint fresh each run and dispatch the
    // returned default as the request model; the retired model must be healed
    // to the compiled default, not surfaced.
    assert_eq!(
        payload
            .get("providers")
            .and_then(|v| v.get("openai-codex"))
            .and_then(|v| v.get("default_model"))
            .and_then(Value::as_str),
        Some(tandem_providers::OPENAI_CODEX_DEFAULT_MODEL)
    );
}

#[tokio::test]
async fn provider_auth_set_writes_protected_audit_record() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = direct_loopback_request()
        .method("PUT")
        .uri("/auth/openai")
        .header("content-type", "application/json")
        .body(Body::from(json!({"token": "sk-test"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"provider.secret.updated\""));
    assert!(audit.contains("\"providerID\":\"openai\""));
}

#[tokio::test]
async fn provider_auth_mutation_rejects_missing_host_authority_without_leaking_secret() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let provider_id = format!("denied-provider-{}", Uuid::new_v4());
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/auth/{provider_id}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"token": "sk-must-never-appear-in-audit"}).to_string(),
        ))
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .unwrap_or_default();
    assert!(!audit.contains("sk-must-never-appear-in-audit"));
    assert!(tandem_core::load_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tandem_types::TenantContext::local_implicit(),
    )
    .get(&provider_id)
    .is_none());
}

#[tokio::test]
async fn provider_auth_set_authority_audit_failure_preserves_existing_credential() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    tandem_core::set_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tenant,
        "openai",
        "sk-before-audit-failure",
    )
    .expect("seed persisted provider auth");
    state
        .auth
        .write()
        .await
        .insert("openai".to_string(), "sk-before-audit-failure".to_string());
    state
        .config
        .patch_runtime(json!({
            "providers": {
                "openai": { "api_key": "sk-before-audit-failure" }
            }
        }))
        .await
        .expect("seed runtime provider auth");
    make_protected_audit_unwritable(&state).await;

    let app = app_router(state.clone());
    let req = direct_loopback_request()
        .method("PUT")
        .uri("/auth/openai")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"token": "sk-after-audit-failure"}).to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        tandem_core::load_provider_auth_for_tenant_in_dir(
            &provider_auth_security_dir(&state),
            &tenant,
        )
        .get("openai")
        .map(String::as_str),
        Some("sk-before-audit-failure")
    );
    assert_eq!(
        state.auth.read().await.get("openai").map(String::as_str),
        Some("sk-before-audit-failure")
    );
    assert_eq!(
        runtime_provider_api_key(&state, "openai").await.as_deref(),
        Some("sk-before-audit-failure")
    );
}

#[tokio::test]
async fn provider_auth_delete_persistence_failure_keeps_durable_and_runtime_key() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    tandem_core::set_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tenant,
        "openai",
        "sk-delete-must-rollback",
    )
    .expect("seed persisted provider auth");
    state
        .auth
        .write()
        .await
        .insert("openai".to_string(), "sk-delete-must-rollback".to_string());
    state
        .config
        .patch_runtime(json!({
            "providers": {
                "openai": { "api_key": "sk-delete-must-rollback" }
            }
        }))
        .await
        .expect("seed runtime provider auth");

    let index_path = provider_auth_security_dir(&state).join("provider_auth_index.json");
    tokio::fs::remove_file(&index_path)
        .await
        .expect("remove provider auth index");
    tokio::fs::create_dir(&index_path)
        .await
        .expect("replace provider auth index with a directory");

    let response = app_router(state.clone())
        .oneshot(
            direct_loopback_request()
                .method("DELETE")
                .uri("/auth/openai")
                .body(Body::empty())
                .expect("delete request"),
        )
        .await
        .expect("delete response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let payload = json_body(response).await;
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("PROVIDER_AUTH_PERSISTENCE_FAILED")
    );
    assert_eq!(
        tandem_core::load_provider_auth_for_tenant_in_dir(
            &provider_auth_security_dir(&state),
            &tenant,
        )
        .get("openai")
        .map(String::as_str),
        Some("sk-delete-must-rollback")
    );
    assert_eq!(
        state.auth.read().await.get("openai").map(String::as_str),
        Some("sk-delete-must-rollback")
    );
    assert_eq!(
        runtime_provider_api_key(&state, "openai").await.as_deref(),
        Some("sk-delete-must-rollback")
    );
    if let Ok(audit) = tokio::fs::read_to_string(&state.protected_audit_path).await {
        assert!(!audit.contains("provider.secret.deleted"));
    }
}

#[tokio::test]
async fn tenant_a_cannot_list_update_or_delete_tenant_b_provider_api_key() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_b = tenant_request(
        "PUT",
        "/auth/openai",
        "org-b",
        "workspace-b",
        "user-b",
        Some(json!({"token": "sk-tenant-b"})),
    );
    let resp = app.clone().oneshot(put_b).await.expect("put b");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        json_body(resp).await.get("ok").and_then(Value::as_bool),
        Some(true)
    );

    assert!(
        state.auth.read().await.get("openai").is_none(),
        "explicit tenant provider auth must not populate shared runtime auth"
    );

    let auth_a = tenant_request(
        "GET",
        "/provider/auth",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let resp = app.clone().oneshot(auth_a).await.expect("auth a");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    assert!(
        payload
            .get("providers")
            .and_then(Value::as_object)
            .and_then(|providers| providers.get("openai"))
            .is_none_or(|openai| openai.get("has_key").and_then(Value::as_bool) == Some(false)),
        "tenant A must not see tenant B provider credential"
    );

    let auth_b = tenant_request(
        "GET",
        "/provider/auth",
        "org-b",
        "workspace-b",
        "user-b",
        None,
    );
    let resp = app.clone().oneshot(auth_b).await.expect("auth b");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    let openai = provider_status(&payload, "openai");
    assert_eq!(openai.get("has_key").and_then(Value::as_bool), Some(true));
    assert_eq!(
        openai.get("source").and_then(Value::as_str),
        Some("persisted")
    );

    let put_a = tenant_request(
        "PUT",
        "/auth/openai",
        "org-a",
        "workspace-a",
        "user-a",
        Some(json!({"token": "sk-tenant-a"})),
    );
    let resp = app.clone().oneshot(put_a).await.expect("put a");
    assert_eq!(resp.status(), StatusCode::OK);

    let tenant_a =
        tandem_types::TenantContext::explicit("org-a", "workspace-a", Some("user-a".to_string()));
    let tenant_b =
        tandem_types::TenantContext::explicit("org-b", "workspace-b", Some("user-b".to_string()));
    let provider_auth_security_dir = state
        .memory_db_path
        .parent()
        .expect("memory db parent")
        .join("security");
    assert_eq!(
        tandem_core::load_provider_auth_for_tenant_in_dir(&provider_auth_security_dir, &tenant_a)
            .get("openai")
            .map(String::as_str),
        Some("sk-tenant-a")
    );
    assert_eq!(
        tandem_core::load_provider_auth_for_tenant_in_dir(&provider_auth_security_dir, &tenant_b)
            .get("openai")
            .map(String::as_str),
        Some("sk-tenant-b")
    );

    let delete_a = tenant_request(
        "DELETE",
        "/auth/openai",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let resp = app.clone().oneshot(delete_a).await.expect("delete a");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        tandem_core::load_provider_auth_for_tenant_in_dir(&provider_auth_security_dir, &tenant_a)
            .get("openai")
            .is_none(),
        "tenant A delete only removes tenant A provider credential"
    );
    assert_eq!(
        tandem_core::load_provider_auth_for_tenant_in_dir(&provider_auth_security_dir, &tenant_b)
            .get("openai")
            .map(String::as_str),
        Some("sk-tenant-b"),
        "tenant B provider credential survives tenant A delete"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn provider_oauth_session_import_persists_codex_auth_and_reports_connected_status() {
    let codex_home_path = std::env::temp_dir()
        .join(format!("tandem-codex-home-{}", Uuid::new_v4()))
        .join(".codex");
    // Serialized test: restore CODEX_HOME on exit so parallel non-serial tests
    // reading codex-cli credentials never observe this test's temp home.
    struct CodexHomeGuard(Option<std::ffi::OsString>);
    impl Drop for CodexHomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("CODEX_HOME", previous),
                None => std::env::remove_var("CODEX_HOME"),
            }
        }
    }
    let _codex_home_guard = CodexHomeGuard(std::env::var_os("CODEX_HOME"));
    std::env::set_var("CODEX_HOME", &codex_home_path);

    let state = test_state().await;
    let app = app_router(state.clone());
    let access_token = make_jwt(json!({
        "exp": 2_000_000_000,
        "email": "hosted@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_456"
        }
    }));
    let auth_json = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": access_token,
            "refresh_token": "refresh-token-456",
            "account_id": "acct_456"
        },
        "last_refresh": "2026-04-23T08:15:30.000Z"
    });
    let req = direct_loopback_request()
        .method("POST")
        .uri("/provider/openai-codex/oauth/session/import")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "auth_json": auth_json.to_string()
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        payload.get("provider_id").and_then(Value::as_str),
        Some("openai-codex")
    );
    assert_eq!(
        payload.get("managed_by").and_then(Value::as_str),
        Some("codex-upload")
    );
    assert_eq!(
        payload.get("email").and_then(Value::as_str),
        Some("hosted@example.com")
    );

    let auth_path = codex_home_path.join("auth.json");
    let persisted = tokio::fs::read_to_string(auth_path.as_path())
        .await
        .expect("persisted codex auth");
    assert!(persisted.contains("refresh-token-456"));
    assert!(persisted.contains("\"auth_mode\": \"chatgpt\""));

    let status_req = direct_loopback_request()
        .method("GET")
        .uri("/provider/openai-codex/oauth/status")
        .body(Body::empty())
        .expect("status request");
    let status_resp = app
        .clone()
        .oneshot(status_req)
        .await
        .expect("status response");
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body = to_bytes(status_resp.into_body(), usize::MAX)
        .await
        .expect("status body");
    let status_payload: Value = serde_json::from_slice(&status_body).expect("status json");
    assert_eq!(
        status_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status_payload.get("status").and_then(Value::as_str),
        Some("connected")
    );
    assert_eq!(
        status_payload.get("managed_by").and_then(Value::as_str),
        Some("codex-upload")
    );
    assert_eq!(
        status_payload.get("connected").and_then(Value::as_bool),
        Some(true)
    );

    let auth_req = direct_loopback_request()
        .method("GET")
        .uri("/provider/auth")
        .body(Body::empty())
        .expect("auth request");
    let auth_resp = app.clone().oneshot(auth_req).await.expect("auth response");
    assert_eq!(auth_resp.status(), StatusCode::OK);
    let auth_body = to_bytes(auth_resp.into_body(), usize::MAX)
        .await
        .expect("auth body");
    let auth_payload: Value = serde_json::from_slice(&auth_body).expect("auth json");
    let codex = auth_payload
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get("openai-codex"))
        .cloned()
        .expect("openai-codex provider auth");
    assert_eq!(
        codex.get("status").and_then(Value::as_str),
        Some("connected")
    );
    assert_eq!(
        codex.get("managed_by").and_then(Value::as_str),
        Some("codex-upload")
    );
    assert_eq!(
        codex
            .get("local_session_available")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn hosted_tenant_cannot_import_local_codex_session() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = tenant_request(
        "POST",
        "/provider/openai-codex/oauth/session/local",
        "hosted-org",
        "hosted-workspace",
        "hosted-user",
        None,
    );

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let payload = json_body(resp).await;
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("LOCAL_PROVIDER_SESSION_REQUIRES_LOCAL_IMPLICIT_TENANT")
    );

    let tenant = tandem_types::TenantContext::explicit(
        "hosted-org",
        "hosted-workspace",
        Some("hosted-user".to_string()),
    );
    let provider_auth_security_dir = state
        .memory_db_path
        .parent()
        .expect("memory db parent")
        .join("security");
    assert!(
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &provider_auth_security_dir,
            &tenant,
            "openai-codex",
        )
        .is_none()
    );
}

#[tokio::test]
async fn oauth_upload_authority_audit_failure_preserves_persisted_and_runtime_credential() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::explicit(
        "org-audit-upload",
        "workspace-audit-upload",
        Some("user-audit-upload".to_string()),
    );
    let prior = test_codex_oauth("prior-upload");
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tenant,
        "openai-codex",
        prior.clone(),
    )
    .expect("seed prior OAuth credential");
    crate::http::config_providers::load_openai_codex_oauth_into_runtime(&state, &tenant)
        .await
        .expect("load prior runtime credential");
    make_protected_audit_unwritable(&state).await;

    let uploaded_access = make_jwt(json!({
        "exp": 2_000_000_000u64,
        "email": "uploaded@example.com"
    }));
    let app = app_router(state.clone());
    let response = app
        .oneshot(tenant_request(
            "POST",
            "/provider/openai-codex/oauth/session/import",
            "org-audit-upload",
            "workspace-audit-upload",
            "user-audit-upload",
            Some(json!({
                "auth_json": json!({
                    "auth_mode": "chatgpt",
                    "tokens": {
                        "access_token": uploaded_access,
                        "refresh_token": "refresh-uploaded",
                        "account_id": "account-uploaded"
                    }
                }).to_string()
            })),
        ))
        .await
        .expect("upload response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &provider_auth_security_dir(&state),
            &tenant,
            "openai-codex",
        ),
        Some(prior)
    );
    assert!(
        state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await,
        "the prior runtime credential must be restored"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn oauth_local_import_audit_failure_restores_persisted_and_runtime_credential() {
    struct CodexHomeGuard(Option<std::ffi::OsString>);
    impl Drop for CodexHomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("CODEX_HOME", previous),
                None => std::env::remove_var("CODEX_HOME"),
            }
        }
    }

    let codex_home = std::env::temp_dir().join(format!("tandem-codex-home-{}", Uuid::new_v4()));
    let _codex_home_guard = CodexHomeGuard(std::env::var_os("CODEX_HOME"));
    std::env::set_var("CODEX_HOME", &codex_home);
    let incoming_access = make_jwt(json!({
        "exp": 2_000_000_000u64,
        "email": "incoming-local@example.com"
    }));
    tandem_core::write_openai_codex_cli_auth_json(&json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": incoming_access,
            "refresh_token": "refresh-incoming-local",
            "account_id": "account-incoming-local"
        }
    }))
    .expect("write local Codex CLI credential");

    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    let prior = test_codex_oauth("prior-local-import");
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tenant,
        "openai-codex",
        prior.clone(),
    )
    .expect("seed prior OAuth credential");
    state.auth.write().await.insert(
        "openai-codex".to_string(),
        "api-prior-local-import".to_string(),
    );
    crate::http::config_providers::load_openai_codex_oauth_into_runtime(&state, &tenant)
        .await
        .expect("load prior runtime credential");
    make_protected_audit_unwritable(&state).await;

    let response = app_router(state.clone())
        .oneshot(
            direct_loopback_request()
                .method("POST")
                .uri("/provider/openai-codex/oauth/session/local")
                .body(Body::empty())
                .expect("local import request"),
        )
        .await
        .expect("local import response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &provider_auth_security_dir(&state),
            &tenant,
            "openai-codex",
        ),
        Some(prior)
    );
    assert_eq!(
        state
            .auth
            .read()
            .await
            .get("openai-codex")
            .map(String::as_str),
        Some("api-prior-local-import")
    );
    assert!(
        state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await,
        "the prior local runtime credential must be restored"
    );
}

#[tokio::test]
async fn oauth_disconnect_audit_failure_restores_persisted_and_runtime_credential() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::explicit(
        "org-audit-disconnect",
        "workspace-audit-disconnect",
        Some("user-audit-disconnect".to_string()),
    );
    let prior = test_codex_oauth("prior-disconnect");
    tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
        &provider_auth_security_dir(&state),
        &tenant,
        "openai-codex",
        prior.clone(),
    )
    .expect("seed prior OAuth credential");
    crate::http::config_providers::load_openai_codex_oauth_into_runtime(&state, &tenant)
        .await
        .expect("load prior runtime credential");
    make_protected_audit_unwritable(&state).await;

    let response = app_router(state.clone())
        .oneshot(tenant_request(
            "DELETE",
            "/provider/openai-codex/oauth/session",
            "org-audit-disconnect",
            "workspace-audit-disconnect",
            "user-audit-disconnect",
            None,
        ))
        .await
        .expect("disconnect response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &provider_auth_security_dir(&state),
            &tenant,
            "openai-codex",
        ),
        Some(prior)
    );
    assert!(
        state
            .providers
            .tenant_provider_auth_is_loaded(&tenant, "openai-codex")
            .await,
        "the disconnected runtime credential must be restored"
    );
}

#[tokio::test]
async fn tenant_a_cannot_refresh_or_delete_tenant_b_oauth_credential() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let access_token = make_jwt(json!({
        "exp": 2_000_000_000,
        "email": "tenant-b@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_user_id": "acct_tenant_b"
        }
    }));
    let auth_json = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "access_token": access_token,
            "refresh_token": "refresh-token-tenant-b",
            "account_id": "acct_tenant_b"
        }
    });

    let import_b = tenant_request(
        "POST",
        "/provider/openai-codex/oauth/session/import",
        "org-b",
        "workspace-b",
        "user-b",
        Some(json!({ "auth_json": auth_json.to_string() })),
    );
    let resp = app.clone().oneshot(import_b).await.expect("import b");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        json_body(resp).await.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        state.auth.read().await.get("openai-codex").is_none(),
        "explicit tenant OAuth import must not populate shared runtime auth"
    );

    let status_a = tenant_request(
        "GET",
        "/provider/openai-codex/oauth/status",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let resp = app.clone().oneshot(status_a).await.expect("status a");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("missing")
    );
    assert_eq!(
        payload.get("connected").and_then(Value::as_bool),
        Some(false)
    );

    let delete_a = tenant_request(
        "DELETE",
        "/provider/openai-codex/oauth/session",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let resp = app.clone().oneshot(delete_a).await.expect("delete a");
    assert_eq!(resp.status(), StatusCode::OK);

    let status_b = tenant_request(
        "GET",
        "/provider/openai-codex/oauth/status",
        "org-b",
        "workspace-b",
        "user-b",
        None,
    );
    let resp = app.clone().oneshot(status_b).await.expect("status b");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("connected")
    );
    assert_eq!(
        payload.get("connected").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("email").and_then(Value::as_str),
        Some("tenant-b@example.com")
    );
}

#[tokio::test]
async fn provider_oauth_authorize_uses_hosted_public_callback_for_codex() {
    let state = test_state().await;
    state.set_server_base_url("http://127.0.0.1:39731".to_string());
    state
        .config
        .patch_project(json!({
            "hosted": {
                "managed": true,
                "public_url": "https://t-999.hosted.tandem.ac"
            }
        }))
        .await
        .expect("patch hosted config");

    let app = app_router(state.clone());
    let req = tenant_request(
        "POST",
        "/provider/openai-codex/oauth/authorize",
        "org-a",
        "workspace-a",
        "admin-a",
        None,
    );
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));

    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .expect("session_id")
        .to_string();
    let authorization_url = payload
        .get("authorizationUrl")
        .and_then(Value::as_str)
        .expect("authorizationUrl");
    assert!(
        authorization_url.contains(
            "redirect_uri=https%3A%2F%2Ft-999.hosted.tandem.ac%2Fapi%2Fengine%2Fprovider%2Fopenai-codex%2Foauth%2Fcallback"
        ),
        "expected hosted callback in authorization URL, got {authorization_url}"
    );
    assert!(
        !authorization_url.contains("localhost%3A1455"),
        "did not expect localhost callback in hosted authorization URL: {authorization_url}"
    );

    let sessions = state.oauth.provider_sessions().read().await;
    let session = sessions.get(&session_id).expect("stored oauth session");
    assert_eq!(session.provider_id, "openai-codex");
    assert_eq!(
        session.redirect_uri,
        "https://t-999.hosted.tandem.ac/api/engine/provider/openai-codex/oauth/callback"
    );
    assert_eq!(session.status, "pending");
}

#[tokio::test]
async fn provider_oauth_authorize_uses_forwarded_panel_origin_for_codex() {
    let state = test_state().await;
    state.set_server_base_url("http://127.0.0.1:39731".to_string());

    let app = app_router(state.clone());
    let mut req = tenant_request(
        "POST",
        "/provider/openai-codex/oauth/authorize",
        "org-a",
        "workspace-a",
        "admin-a",
        None,
    );
    req.headers_mut().insert(
        "x-forwarded-proto",
        axum::http::HeaderValue::from_static("https"),
    );
    req.headers_mut().insert(
        "x-forwarded-host",
        axum::http::HeaderValue::from_static("panel.example.com"),
    );
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));

    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .expect("session_id")
        .to_string();
    let authorization_url = payload
        .get("authorizationUrl")
        .and_then(Value::as_str)
        .expect("authorizationUrl");
    assert!(
        authorization_url.contains(
            "redirect_uri=https%3A%2F%2Fpanel.example.com%2Fapi%2Fengine%2Fprovider%2Fopenai-codex%2Foauth%2Fcallback"
        ),
        "expected forwarded panel callback in authorization URL, got {authorization_url}"
    );
    assert!(
        !authorization_url.contains("localhost%3A1455"),
        "did not expect localhost callback in panel authorization URL: {authorization_url}"
    );

    let sessions = state.oauth.provider_sessions().read().await;
    let session = sessions.get(&session_id).expect("stored oauth session");
    assert_eq!(
        session.redirect_uri,
        "https://panel.example.com/api/engine/provider/openai-codex/oauth/callback"
    );
}

#[tokio::test]
async fn provider_route_marks_config_models_as_config_catalogs() {
    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "providers": {
                "openai": {
                    "url": "https://api.openai.com/v1",
                    "models": {
                        "gpt-4.1-mini": {
                            "name": "GPT 4.1 Mini",
                            "context_length": 128000
                        }
                    }
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state);
    let req = direct_loopback_request()
        .method("GET")
        .uri("/provider")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let all = payload
        .get("all")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let openai = all
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        .cloned()
        .expect("openai entry");

    assert_eq!(
        openai.get("catalog_source").and_then(Value::as_str),
        Some("config")
    );
    assert_eq!(
        openai.get("catalog_status").and_then(Value::as_str),
        Some("ok")
    );
    assert!(
        openai
            .get("models")
            .and_then(Value::as_object)
            .and_then(|models| models.get("gpt-4.1-mini"))
            .is_some(),
        "expected configured model to appear in catalog"
    );
}

#[tokio::test]
async fn provider_route_uses_runtime_auth_for_remote_catalog_fetch() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let app = Router::new().route(
        "/v1/models",
        get(|| async {
            Json(json!({
                "data": [
                    { "id": "gpt-4.1-mini", "name": "GPT 4.1 Mini", "context_length": 128000 }
                ]
            }))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve test provider");
    });

    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "providers": {
                "openai": {
                    "url": format!("http://{addr}/v1")
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .auth
        .write()
        .await
        .insert("openai".to_string(), "test-key".to_string());
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state);
    let req = direct_loopback_request()
        .method("GET")
        .uri("/provider")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    server.abort();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let all = payload
        .get("all")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let openai = all
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        .cloned()
        .expect("openai entry");

    assert_eq!(
        openai.get("catalog_source").and_then(Value::as_str),
        Some("remote")
    );
    assert!(
        openai
            .get("models")
            .and_then(Value::as_object)
            .and_then(|models| models.get("gpt-4.1-mini"))
            .is_some(),
        "expected runtime-auth-backed remote catalog"
    );
}

#[tokio::test]
async fn explicit_tenant_provider_route_ignores_shared_config_api_key_for_remote_catalog_fetch() {
    let seen_auth = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let seen_auth_for_handler = seen_auth.clone();
    let app = Router::new().route(
        "/v1/models",
        get(move |headers: axum::http::HeaderMap| {
            let seen_auth = seen_auth_for_handler.clone();
            async move {
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                seen_auth.lock().expect("seen auth lock").push(auth);
                Json(json!({
                    "data": [
                        { "id": "gpt-4.1-mini", "name": "GPT 4.1 Mini", "context_length": 128000 }
                    ]
                }))
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve test provider");
    });

    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "providers": {
                "openai": {
                    "url": format!("http://{addr}/v1"),
                    "api_key": "sk-shared-config"
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state);
    let req = tenant_request("GET", "/provider", "org-a", "workspace-a", "user-a", None);
    let resp = app.oneshot(req).await.expect("response");
    server.abort();

    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    let openai = payload
        .get("all")
        .and_then(Value::as_array)
        .and_then(|providers| {
            providers
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        })
        .expect("openai entry");
    assert_eq!(
        openai.get("catalog_status").and_then(Value::as_str),
        Some("unavailable"),
        "explicit tenant must not use shared config provider key for discovery"
    );
    assert!(
        seen_auth.lock().expect("seen auth lock").is_empty(),
        "shared config key should not trigger a remote catalog request for explicit tenants"
    );
}

#[tokio::test]
async fn explicit_tenant_catalog_blocks_private_origin_without_cross_tenant_auth_leakage() {
    let seen_auth = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let seen_auth_for_handler = seen_auth.clone();
    let app = Router::new().route(
        "/v1/models",
        get(move |headers: axum::http::HeaderMap| {
            let seen_auth = seen_auth_for_handler.clone();
            async move {
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                seen_auth.lock().expect("seen auth lock").push(auth);
                Json(json!({
                    "data": [
                        { "id": "gpt-4.1-mini", "name": "GPT 4.1 Mini", "context_length": 128000 }
                    ]
                }))
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve test provider");
    });

    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "providers": {
                "openai": {
                    "url": format!("http://{addr}/v1")
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state);
    let put_b = tenant_request(
        "PUT",
        "/auth/openai",
        "org-b",
        "workspace-b",
        "user-b",
        Some(json!({"token": "sk-tenant-b"})),
    );
    let resp = app.clone().oneshot(put_b).await.expect("put b");
    assert_eq!(resp.status(), StatusCode::OK);

    let provider_a = tenant_request("GET", "/provider", "org-a", "workspace-a", "user-a", None);
    let resp = app.clone().oneshot(provider_a).await.expect("provider a");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    let openai = payload
        .get("all")
        .and_then(Value::as_array)
        .and_then(|providers| {
            providers
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        })
        .expect("openai entry for tenant a");
    assert_eq!(
        openai.get("catalog_status").and_then(Value::as_str),
        Some("unavailable"),
        "tenant A must not use tenant B provider credential for discovery"
    );

    let provider_b = tenant_request("GET", "/provider", "org-b", "workspace-b", "user-b", None);
    let resp = app.clone().oneshot(provider_b).await.expect("provider b");
    server.abort();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload = json_body(resp).await;
    let openai = payload
        .get("all")
        .and_then(Value::as_array)
        .and_then(|providers| {
            providers
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some("openai"))
        })
        .expect("openai entry for tenant b");
    assert_eq!(
        openai.get("catalog_status").and_then(Value::as_str),
        Some("error"),
        "hosted tenant discovery must reject a private provider origin"
    );

    let seen = seen_auth.lock().expect("seen auth lock").clone();
    assert!(
        seen.is_empty(),
        "neither tenant credential may be sent to a private provider origin"
    );
}
