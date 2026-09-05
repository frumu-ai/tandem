//! Signed HTTP ingress with the actual key verifier, durable replay store and
//! file-backed policy reload. Uses disposable state; no environment overrides.
use super::*;
use axum::{
    body::{to_bytes, Body},
    routing::get,
    Extension, Router,
};
use serde_json::Value;
use tandem_types::{AuthorityChain, HumanActor};
use tower::ServiceExt;

fn claims(actor: &str, role: &str, version: u64, now: u64) -> TenantContextAssertionClaims {
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        format!("assertion-{actor}-{version}-{role}"),
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
    claims
}

fn write_policy(path: &std::path::Path, version: u64, alice_role: Option<&str>, now: u64) {
    let users: Vec<_> = [("alice", alice_role), ("bob", Some("member"))]
        .into_iter()
        .filter_map(|(id, role)| {
            role.map(|role| json!({
            "id": id, "email": null, "username": null, "role": role,
            "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
            "is_active": true, "email_verified": true,
        }))
        })
        .collect();
    std::fs::write(path, serde_json::to_vec(&json!({
        "schema_version": 1, "policy_version": version, "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(), "users": users,
        "org_units": [], "org_unit_memberships": [], "deployment_grants": []
    })).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn probe(Extension(verified): Extension<VerifiedTenantContext>) -> Json<Value> {
    let strict = verified.strict_projection.unwrap();
    Json(
        json!({"hosted_admin": strict.has_permission(AccessPermission::HostedAdmin),
        "generic_admin": strict.has_permission(AccessPermission::Admin),
        "hosted_use": strict.has_permission(AccessPermission::HostedUse)}),
    )
}

async fn ingress(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    match attach_enterprise_request_context_for_mode(
        &state,
        &mut request,
        RuntimeAuthMode::HostedSingleTenant,
    )
    .await
    {
        Ok(true) => next.run(request).await,
        Ok(false) => StatusCode::FORBIDDEN.into_response(),
        Err(error) => panic!("required test denial receipt failed: {error}"),
    }
}

async fn request(app: &Router, assertion: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri("/probe")
                .header("x-tandem-context-assertion", assertion)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn hosted_policy_signed_http_downgrade_removal_and_unaffected_user_refresh() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[39; 32]);
    let raw = json!({"key-a": {"purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a", "allowed_audiences": ["tandem-runtime"], "status": "active"}}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(&raw, &temp.path().join("replay.json"));
    *state.context_assertion_security.write().unwrap() = Some(std::sync::Arc::new(security));
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    let app = Router::new()
        .route("/probe", get(probe))
        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress));
    let now = crate::now_ms();
    let sign = |actor, role, version| {
        super::tests::sign_test_context_assertion(&key, "key-a", claims(actor, role, version, now))
    };
    let alice = sign("alice", "admin", 1);
    assert_eq!(request(&app, &alice).await.status(), StatusCode::FORBIDDEN);
    write_policy(&path, 1, Some("admin"), now);
    state.reload_hosted_policy().await.unwrap();
    let response = request(&app, &alice).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"hosted_admin": true, "generic_admin": false, "hosted_use": true})
    );
    let bob = sign("bob", "member", 1);
    assert_eq!(request(&app, &bob).await.status(), StatusCode::OK);

    write_policy(&path, 2, Some("viewer"), now);
    state.reload_hosted_policy().await.unwrap();
    assert_eq!(request(&app, &alice).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(request(&app, &bob).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        request(&app, &sign("bob", "member", 2)).await.status(),
        StatusCode::OK
    );
    let viewer = request(&app, &sign("alice", "viewer", 2)).await;
    assert_eq!(viewer.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(viewer.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"hosted_admin": false, "generic_admin": false, "hosted_use": false})
    );

    write_policy(&path, 3, None, now);
    state.reload_hosted_policy().await.unwrap();
    assert_eq!(
        request(&app, &sign("alice", "viewer", 3)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, &sign("bob", "member", 3)).await.status(),
        StatusCode::OK
    );
    let wrong_key = ed25519_dalek::SigningKey::from_bytes(&[40; 32]);
    let forged = super::tests::sign_test_context_assertion(
        &wrong_key,
        "key-a",
        claims("bob", "member", 3, now),
    );
    assert_eq!(request(&app, &forged).await.status(), StatusCode::FORBIDDEN);
    assert!(temp.path().join("replay.json").is_file());
}
