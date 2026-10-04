// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn hosted_skill_mutations_require_deployment_admin() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[49; 32]);
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
    let app = crate::http::routes_skills_memory::apply(Router::new())
        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
        .with_state(state.clone());
    let now = crate::now_ms();
    let signed_request = |method: &str, route: &str, body: Value, role: &str, version: u64| {
        let mut assertion = claims("alice", role, version, now);
        assertion.assertion_id = uuid::Uuid::new_v4().to_string();
        let signed = super::super::tests::sign_test_context_assertion(&key, "key-a", assertion);
        Request::builder()
            .method(method)
            .uri(route)
            .header("x-tandem-context-assertion", signed)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let missing_name = format!("hosted-authz-absent-{}", uuid::Uuid::new_v4());
    let missing_source = temp.path().join("absent-skill.zip");
    // Invalid content and absent names exercise native handlers without writing
    // skills. File imports are denial-only: even an empty scan creates its base
    // directory. Permission denials must precede validation/filesystem lookup.
    for (version, role, grant) in [
        (1, "viewer", None),
        (2, "member", None),
        (3, "viewer", Some("automation.write")),
        (4, "viewer", Some("hosted.admin")),
    ] {
        write_policy(&path, version, Some(role), now);
        if let Some(permission) = grant {
            let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
                "resource_id": "dep-a", "permissions": [permission]}]);
            std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        for location in ["project", "global"] {
            let template_route = format!("/skills/templates/{missing_name}/install");
            let delete_route = format!("/skills/{missing_name}?location={location}");
            for (method, route, body, allowed_status) in [
                (
                    "POST",
                    "/skills",
                    json!({"content": "", "location": location}),
                    StatusCode::BAD_REQUEST,
                ),
                (
                    "POST",
                    "/skills/import",
                    json!({"content": "", "location": location}),
                    StatusCode::BAD_REQUEST,
                ),
                (
                    "POST",
                    "/skills",
                    json!({"file_or_path": missing_source, "location": location, "conflict_policy": "overwrite"}),
                    StatusCode::BAD_REQUEST,
                ),
                (
                    "POST",
                    "/skills/import",
                    json!({"file_or_path": missing_source, "location": location, "conflict_policy": "overwrite"}),
                    StatusCode::BAD_REQUEST,
                ),
                (
                    "POST",
                    "/skills/generate/install",
                    json!({"artifacts": {"SKILL.md": ""}, "location": location}),
                    StatusCode::BAD_REQUEST,
                ),
                (
                    "POST",
                    template_route.as_str(),
                    json!({"location": location}),
                    StatusCode::BAD_REQUEST,
                ),
                ("DELETE", delete_route.as_str(), json!({}), StatusCode::OK),
            ] {
                if version == 4 && body.get("file_or_path").is_some() {
                    continue;
                }
                let response = app
                    .clone()
                    .oneshot(signed_request(method, route, body.clone(), role, version))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if version == 4 {
                        allowed_status
                    } else {
                        StatusCode::FORBIDDEN
                    },
                    "{role}/{grant:?}: {method} {route} ({location}), {body}"
                );
            }
        }
        for route in ["/skills", "/skills/catalog", "/skills/templates"] {
            assert_eq!(
                app.clone()
                    .oneshot(signed_request("GET", route, json!({}), role, version))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK,
                "discovery {route}"
            );
        }
        // Generating a scaffold is distinct from installing it in the registry.
        assert_eq!(
            app.clone()
                .oneshot(signed_request(
                    "POST",
                    "/skills/generate",
                    json!({"prompt": ""}),
                    role,
                    version
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
