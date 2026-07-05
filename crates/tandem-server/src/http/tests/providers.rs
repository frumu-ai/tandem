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

fn tenant_request(
    method: &str,
    uri: &str,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
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
    assert!(codex_models.contains_key("gpt-5.5"));
    assert!(codex_models.contains_key("gpt-5.4"));
    assert!(codex_models.contains_key("gpt-5.2-codex"));
    assert!(codex_models.contains_key("gpt-5.1-codex-max"));
    assert!(codex_models.contains_key("gpt-5.4-mini"));
    assert!(codex_models.contains_key("gpt-5.3-codex"));
    assert!(codex_models.contains_key("gpt-5.3-codex-spark"));
    assert!(codex_models.contains_key("gpt-5.1-codex-mini"));
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
    let req = Request::builder()
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

    let req = Request::builder()
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
async fn provider_auth_set_writes_protected_audit_record() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = Request::builder()
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
async fn provider_oauth_session_import_persists_codex_auth_and_reports_connected_status() {
    let codex_home_path = std::env::temp_dir()
        .join(format!("tandem-codex-home-{}", Uuid::new_v4()))
        .join(".codex");
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
    let req = Request::builder()
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

    let status_req = Request::builder()
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

    let auth_req = Request::builder()
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
    let req = Request::builder()
        .method("POST")
        .uri("/provider/openai-codex/oauth/authorize")
        .body(Body::empty())
        .expect("request");
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
    let req = Request::builder()
        .method("POST")
        .uri("/provider/openai-codex/oauth/authorize")
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-host", "panel.example.com")
        .body(Body::empty())
        .expect("request");
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
    let req = Request::builder()
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
    let req = Request::builder()
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
async fn provider_route_uses_only_request_tenant_persisted_auth_for_remote_catalog_fetch() {
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
        openai.get("catalog_source").and_then(Value::as_str),
        Some("remote")
    );

    let seen = seen_auth.lock().expect("seen auth lock").clone();
    assert_eq!(seen, vec!["Bearer sk-tenant-b".to_string()]);
}
