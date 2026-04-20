use super::*;
use axum::{routing::get, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tokio::net::TcpListener;
use uuid::Uuid;

fn make_jwt(payload: Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).expect("payload json"));
    format!("{header}.{payload}.signature")
}

#[tokio::test]
async fn provider_route_returns_known_providers_without_synthetic_default_models() {
    let state = test_state().await;
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
        "last_refresh": 456
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
            "redirect_uri=https%3A%2F%2Ft-999.hosted.tandem.ac%2Fprovider%2Fopenai-codex%2Foauth%2Fcallback"
        ),
        "expected hosted callback in authorization URL, got {authorization_url}"
    );
    assert!(
        !authorization_url.contains("localhost%3A1455"),
        "did not expect localhost callback in hosted authorization URL: {authorization_url}"
    );

    let sessions = state.provider_oauth_sessions.read().await;
    let session = sessions.get(&session_id).expect("stored oauth session");
    assert_eq!(session.provider_id, "openai-codex");
    assert_eq!(
        session.redirect_uri,
        "https://t-999.hosted.tandem.ac/provider/openai-codex/oauth/callback"
    );
    assert_eq!(session.status, "pending");
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
