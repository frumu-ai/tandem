// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn hosted_preset_mutations_require_deployment_admin() {
    let mut state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    state.preset_registry = std::sync::Arc::new(crate::preset_registry::PresetRegistry::new(
        temp.path().join("packs"),
        temp.path().join("runtime"),
    ));
    let original = "id: shared\nversion: 1.0.0\n";
    let shared = state
        .preset_registry
        .save_override("agent_preset", "shared", original)
        .await
        .unwrap();
    let seed_content = "id: seed\nversion: 1.0.0\n";
    let seed = state
        .preset_registry
        .save_override("skill_module", "seed", seed_content)
        .await
        .unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[47; 32]);
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
    let app = crate::http::routes_presets::apply(Router::new())
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
    for (version, role, permissions) in [
        (1, "viewer", json!([])),
        (2, "member", json!([])),
        (3, "viewer", json!(["automation.write"])),
    ] {
        write_policy(&path, version, Some(role), now);
        let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        if version == 3 {
            policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
                "resource_id": "dep-a", "permissions": permissions}]);
        }
        std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        state.reload_hosted_policy().await.unwrap();
        for (method, route, body) in [
            (
                "PUT",
                "/presets/overrides/AGENT_PRESETS/shared",
                json!({"content": "changed"}),
            ),
            (
                "DELETE",
                "/presets/overrides/agent_preset/shared",
                json!({}),
            ),
            (
                "POST",
                "/presets/fork",
                json!({"kind": "agent_preset", "source_path": seed, "target_id": "shared"}),
            ),
            // Export is a filesystem write, even when its target is an existing preset.
            (
                "POST",
                "/presets/export_overrides",
                json!({"output_path": shared}),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(signed_request(method, route, body, role, version))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "{role}: {method} {route}"
            );
            assert_eq!(std::fs::read_to_string(&shared).unwrap(), original);
            assert_eq!(std::fs::read_to_string(&seed).unwrap(), seed_content);
        }
        assert_eq!(
            app.clone()
                .oneshot(signed_request(
                    "GET",
                    "/presets/index",
                    json!({}),
                    role,
                    version
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    // Delegated administration is permission-based, not a hard-coded role test.
    write_policy(&path, 4, Some("viewer"), now);
    let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
        "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
        "resource_id": "dep-a", "permissions": ["hosted.admin"]}]);
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    state.reload_hosted_policy().await.unwrap();
    let exported = temp.path().join("export.zip");
    for (method, route, body) in [
        (
            "PUT",
            "/presets/overrides/agent_preset/shared",
            json!({"content": "updated"}),
        ),
        (
            "POST",
            "/presets/export_overrides",
            json!({"output_path": exported}),
        ),
        (
            "DELETE",
            "/presets/overrides/AGENT_PRESETS/shared",
            json!({}),
        ),
        (
            "POST",
            "/presets/fork",
            json!({"kind": "agent_preset", "source_path": seed, "target_id": "shared"}),
        ),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(signed_request(method, route, body, "viewer", 4))
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "{method} {route}"
        );
    }
    assert_eq!(std::fs::read_to_string(&shared).unwrap(), seed_content);
    assert!(std::fs::metadata(exported).unwrap().len() > 0);
}
