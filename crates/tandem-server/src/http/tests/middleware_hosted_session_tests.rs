// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::super::*;
use axum::{body::Body, Router};
use tandem_types::{AuthorityChain, HumanActor};
use tower::ServiceExt;

async fn ingress(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    if attach_enterprise_request_context_for_mode(
        &state,
        &mut request,
        RuntimeAuthMode::HostedSingleTenant,
    )
    .await
    .expect("denial audit must succeed")
    {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn signed_request(
    key: &ed25519_dalek::SigningKey,
    actor: &str,
    role: &str,
    version: u64,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> Request {
    let now = crate::now_ms();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        uuid::Uuid::new_v4().to_string(),
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), actor),
        HumanActor::tandem_user(actor),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(actor, "tandem-web")),
        vec![format!("hosted:role:{role}")],
    );
    claims.policy_version = Some(version);
    claims.capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities(role)
        .iter()
        .map(|cap| cap.to_string())
        .collect();
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header(
            "x-tandem-context-assertion",
            super::sign_test_context_assertion(key, "key-a", claims),
        )
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn hosted_session_signed_mutations_enforce_current_use_and_preserve_ownership() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[73; 32]);
    let raw = json!({"key-a": {"purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a", "allowed_audiences": ["tandem-runtime"], "status": "active"}}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(&raw, &temp.path().join("replay.json"));
    *state.context_assertion_security.write().unwrap() = Some(std::sync::Arc::new(security));
    let policy_path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", policy_path.clone());
    let app = crate::http::routes_sessions::apply(Router::new())
        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
        .with_state(state.clone());
    let mut session = tandem_types::Session::new(Some("existing owned session".into()), None);
    session.tenant_context =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
    let id = session.id.clone();
    state.storage.save_session(session).await.unwrap();
    // Include a live downgrade after successful member writes.
    for (version, role, allowed) in [
        (1, "viewer", false),
        (2, "member", true),
        (3, "viewer", false),
    ] {
        let now = crate::now_ms();
        let users: Vec<_> = [("alice", role), ("bob", "member")].into_iter().map(|(actor, role)| json!({
            "id": actor, "email": null, "username": null, "role": role,
            "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
            "is_active": true, "email_verified": true
        })).collect();
        std::fs::write(&policy_path, serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": version, "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(), "users": users,
            "org_units": [], "org_unit_memberships": [], "deployment_grants": []
        })).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&policy_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        for prefix in ["/session", "/api/session"] {
            let before = state.storage.list_sessions().await.len();
            let response = app
                .clone()
                .oneshot(signed_request(
                    &key,
                    "alice",
                    role,
                    version,
                    "POST",
                    prefix,
                    json!({}),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            assert_eq!(
                state.storage.list_sessions().await.len(),
                before + usize::from(allowed)
            );
            let path = format!("{prefix}/{id}/message");
            let before = state.storage.get_session(&id).await.unwrap().messages.len();
            let body = json!({"parts": [{"type": "text", "text": "authorized append"}]});
            let response = app
                .clone()
                .oneshot(signed_request(
                    &key,
                    "alice",
                    role,
                    version,
                    "POST",
                    &path,
                    body.clone(),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            let after = state.storage.get_session(&id).await.unwrap().messages.len();
            assert_eq!(after, before + usize::from(allowed));
            let response = app
                .clone()
                .oneshot(signed_request(
                    &key, "bob", "member", version, "POST", &path, body,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(
                state.storage.get_session(&id).await.unwrap().messages.len(),
                after
            );
            let response = app
                .clone()
                .oneshot(signed_request(
                    &key,
                    "alice",
                    role,
                    version,
                    "GET",
                    &format!("{prefix}/{id}"),
                    json!({}),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
