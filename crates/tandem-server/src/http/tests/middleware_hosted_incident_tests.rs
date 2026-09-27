// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn hosted_incident_triage_requires_execution_grant_and_preserves_approval() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[43; 32]);
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
    let app = crate::http::routes_incident_monitor::apply(Router::new())
        .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
        .with_state(state.clone());
    let mut config = state.incident_monitor_config().await;
    config.require_approval_for_new_issues = true;
    state.put_incident_monitor_config(config).await.unwrap();
    let draft = crate::IncidentMonitorDraftRecord {
        draft_id: "hosted-triage-draft".into(),
        fingerprint: "hosted-triage".into(),
        repo: "example/project".into(),
        status: "approval_required".into(),
        created_at_ms: crate::now_ms(),
        ..Default::default()
    };
    state
        .put_incident_monitor_draft(draft.clone())
        .await
        .unwrap();
    state
        .put_incident_monitor_incident(crate::IncidentMonitorIncidentRecord {
            incident_id: "hosted-incident".into(),
            draft_id: Some(draft.draft_id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    let now = crate::now_ms();
    for (version, permissions) in [(1, json!([])), (2, json!(["automation.write"]))] {
        write_policy(&path, version, Some("viewer"), now);
        let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
            "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
            "resource_id": "dep-a", "permissions": permissions}]);
        if version == 1 {
            policy["deployment_grants"] = json!([]);
        }
        std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        state.reload_hosted_policy().await.unwrap();
        for route in [
            "/incident-monitor/drafts/hosted-triage-draft/triage-run",
            "/incident-monitor/drafts/hosted-triage-draft/approve",
            "/incident-monitor/incidents/hosted-incident/replay",
            "/incident-monitor/log-sources/project/source/replay-latest",
        ] {
            let mut assertion = claims("alice", "viewer", version, now);
            assertion.assertion_id = uuid::Uuid::new_v4().to_string();
            let signed = super::super::tests::sign_test_context_assertion(&key, "key-a", assertion);
            assert_eq!(
                automation_request(&app, &signed, "POST", route)
                    .await
                    .status(),
                StatusCode::FORBIDDEN,
                "{route}"
            );
            assert_eq!(
                serde_json::to_value(
                    state
                        .get_incident_monitor_draft(&draft.draft_id)
                        .await
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(&draft).unwrap()
            );
            assert!(state.automations_v2.read().await.is_empty());
            assert!(state.automation_v2_runs.read().await.is_empty());
        }
    }
    write_policy(&path, 3, Some("viewer"), now);
    let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
        "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment",
        "resource_id": "dep-a", "permissions": ["automation.execute"]}]);
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    state.reload_hosted_policy().await.unwrap();
    for route in [
        "/incident-monitor/drafts/missing/triage-run",
        "/incident-monitor/drafts/missing/approve",
        "/incident-monitor/incidents/missing/replay",
        "/incident-monitor/log-sources/project/source/replay-latest",
        "/incident-monitor/drafts/hosted-triage-draft/triage-run",
    ] {
        let mut assertion = claims("alice", "viewer", 3, now);
        assertion.assertion_id = uuid::Uuid::new_v4().to_string();
        let signed = super::super::tests::sign_test_context_assertion(&key, "key-a", assertion);
        let expected = if route.contains("hosted-triage-draft") {
            StatusCode::CONFLICT
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(
            automation_request(&app, &signed, "POST", route)
                .await
                .status(),
            expected,
            "{route}"
        );
    }
    // Standalone routing retains its existing approval/error semantics.
    let local = crate::http::routes_incident_monitor::apply(Router::new()).with_state(state);
    assert_eq!(
        automation_request(
            &local,
            "unused",
            "POST",
            "/incident-monitor/drafts/hosted-triage-draft/triage-run"
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
}
