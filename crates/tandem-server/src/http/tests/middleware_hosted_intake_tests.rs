// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

async fn intake_fixture() -> (AppState, tempfile::TempDir, ed25519_dalek::SigningKey) {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[67; 32]);
    let raw = json!({"key-a": {"purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
        "organization_id": "org-a", "deployment_id": "dep-a", "allowed_audiences": ["tandem-runtime"], "status": "active"}}).to_string();
    let security = crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(&raw, &temp.path().join("replay.json"));
    *state.context_assertion_security.write().unwrap() = Some(std::sync::Arc::new(security));
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            monitored_projects: vec![crate::IncidentMonitorMonitoredProject {
                project_id: "payments".into(),
                name: "Payments".into(),
                repo: "acme/payments".into(),
                workspace_root: temp.path().display().to_string(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    state
        .put_incident_monitor_intake_key(crate::IncidentMonitorProjectIntakeKey {
            key_id: "existing".into(),
            project_id: "payments".into(),
            name: "Existing".into(),
            key_hash: crate::sha256_hex(&["existing-raw"]),
            enabled: true,
            scopes: vec!["incident_monitor:report".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    (state, temp, key)
}

fn management_request(method: &str, route: &str) -> Request {
    Request::builder()
        .method(method)
        .uri(route)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"project_id": "payments", "name": "Created"}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn hosted_intake_key_management_requires_current_admin() {
    let (state, temp, key) = intake_fixture().await;
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    for (version, role, permissions, allowed) in [
        (1, "viewer", json!([]), false),
        (2, "member", json!([]), false),
        (
            3,
            "viewer",
            json!(["automation.write", "automation.execute"]),
            false,
        ),
        (4, "admin", json!([]), true),
        (5, "owner", json!([]), true),
        (6, "viewer", json!(["hosted.admin"]), true),
    ] {
        let now = crate::now_ms();
        write_policy(&path, version, Some(role), now);
        if version == 3 || version == 6 {
            let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
                "resource_id": "dep-a", "permissions": permissions}]);
            std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        }
        state.reload_hosted_policy().await.unwrap();
        for through_ingress in [true, false] {
            for (method, route) in [
                ("GET", "/incident-monitor/intake/keys"),
                ("HEAD", "/incident-monitor/intake/keys"),
                ("POST", "/incident-monitor/intake/keys"),
                ("POST", "/incident-monitor/intake/keys/existing/disable"),
            ] {
                let before =
                    serde_json::to_value(state.list_incident_monitor_intake_keys().await).unwrap();
                let persisted = std::fs::read(&state.incident_monitor_intake_keys_path).unwrap();
                let mut assertion = claims("alice", role, version, now);
                assertion.assertion_id = uuid::Uuid::new_v4().to_string();
                let router = crate::http::routes_incident_monitor::apply(Router::new());
                let app = if through_ingress {
                    router
                        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
                        .with_state(state.clone())
                } else {
                    router
                        .layer(Extension(VerifiedTenantContext::from(assertion.clone())))
                        .with_state(state.clone())
                };
                let mut request = management_request(method, route);
                request.headers_mut().insert(
                    "x-tandem-context-assertion",
                    crate::http::middleware::tests::sign_test_context_assertion(
                        &key, "key-a", assertion,
                    )
                    .parse()
                    .unwrap(),
                );
                let response = app.oneshot(request).await.unwrap();
                assert_eq!(
                    response.status(),
                    if allowed {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                    "{role}: {method} {route}, ingress={through_ingress}"
                );
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                if !allowed {
                    assert_eq!(
                        serde_json::to_value(state.list_incident_monitor_intake_keys().await)
                            .unwrap(),
                        before
                    );
                    assert_eq!(
                        std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
                        persisted
                    );
                    assert!(!String::from_utf8_lossy(&bytes).contains("tim_intake_"));
                } else if method != "HEAD" {
                    let payload: Value = serde_json::from_slice(&bytes).unwrap();
                    if method == "GET" {
                        assert!(payload["keys"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|row| row["key_hash"] == "[redacted]"));
                    } else if route.ends_with("/disable") {
                        assert_eq!(payload["key"]["enabled"], false);
                        assert!(state
                            .validate_incident_monitor_intake_key(
                                "existing-raw",
                                "payments",
                                "incident_monitor:report"
                            )
                            .await
                            .is_none());
                    } else {
                        assert_eq!(payload["key"]["key_hash"], "[redacted]");
                        assert!(state
                            .validate_incident_monitor_intake_key(
                                payload["raw_key"].as_str().unwrap(),
                                "payments",
                                "incident_monitor:report"
                            )
                            .await
                            .is_some());
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn local_intake_key_management_remains_available() {
    let (state, _temp, _key) = intake_fixture().await;
    let app = crate::http::routes_incident_monitor::apply(Router::new()).with_state(state);
    for (method, route) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("HEAD", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/existing/disable"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(management_request(method, route))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn hosted_intake_key_management_rechecks_authority_after_lock_wait() {
    let (state, temp, _key) = intake_fixture().await;
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    for (index, (method, route)) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/existing/disable"),
    ]
    .into_iter()
    .enumerate()
    {
        let version = 1 + 2 * index as u64;
        let now = crate::now_ms();
        write_policy(&path, version, Some("admin"), now);
        state.reload_hosted_policy().await.unwrap();
        let verified: VerifiedTenantContext = claims("alice", "admin", version, now).into();
        state
            .enterprise
            .hosted_policy
            .authorize_permission(Some(&verified), AccessPermission::HostedAdmin)
            .unwrap();
        let app = crate::http::routes_incident_monitor::apply(Router::new())
            .layer(Extension(verified))
            .with_state(state.clone());
        let held = state.incident_monitor_intake_keys.write().await;
        let before = serde_json::to_value(&*held).unwrap();
        let persisted = std::fs::read(&state.incident_monitor_intake_keys_path).unwrap();
        let pending = app.oneshot(management_request(method, route));
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        write_policy(&path, version + 1, Some("viewer"), now);
        state.reload_hosted_policy().await.unwrap();
        drop(held);
        assert_eq!(
            pending.await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "revoked {method} {route}"
        );
        assert_eq!(
            serde_json::to_value(&*state.incident_monitor_intake_keys.read().await).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read(&state.incident_monitor_intake_keys_path).unwrap(),
            persisted
        );
    }
    // A direct handler must fail closed without verified identity, even for a
    // nonexistent key, rather than revealing existence before authorization.
    let app = crate::http::routes_incident_monitor::apply(Router::new()).with_state(state);
    for (method, route) in [
        ("GET", "/incident-monitor/intake/keys"),
        ("HEAD", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys"),
        ("POST", "/incident-monitor/intake/keys/missing/disable"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(management_request(method, route))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
