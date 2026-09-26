// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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

fn private_automation(owner: &str) -> crate::AutomationV2Spec {
    let mut automation = crate::AutomationV2Spec {
        automation_id: format!("private-{owner}"),
        name: format!("Private {owner} automation"),
        description: None,
        status: crate::AutomationV2Status::Paused,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".into(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: owner.into(),
        workspace_root: None,
        metadata: Some(
            json!({"resource_access": {"visibility": "private", "owner_principal": {"kind": "human_user", "id": owner}}}),
        ),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    automation.set_tenant_context(&TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        owner,
    ));
    automation
}

async fn automation_request(app: &Router, assertion: &str, method: &str, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("x-tandem-context-assertion", assertion)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
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
    let app = super::super::routes_automation_webhook_management::apply(
        super::super::routes_channel_automation_drafts::apply(
            super::super::routes_workflows::apply(
                super::super::routes_routines_automations::apply(Router::new()),
            ),
        ),
    )
    .route("/probe", get(probe))
    .layer(axum::middleware::from_fn_with_state(state.clone(), ingress))
    .with_state(state.clone());
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
    for path in ["/workflows", "/automations", "/routines"] {
        assert_eq!(
            automation_request(&app, &bob, "GET", path).await.status(),
            StatusCode::OK
        );
    }

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

    for owner in ["alice", "bob"] {
        state
            .put_automation_v2(private_automation(owner))
            .await
            .unwrap();
    }
    write_policy(&path, 4, Some("viewer"), now);
    state.reload_hosted_policy().await.unwrap();
    assert_eq!(
        automation_request(&app, &sign("alice", "viewer", 4), "GET", "/automations/v2")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // Ownership and default trigger scope must not bypass hosted operation grants.
    for (method, path) in [
        ("GET", "/automations/channel-drafts/pending"),
        ("HEAD", "/automations/channel-drafts/pending"),
        ("POST", "/automations/channel-drafts"),
        ("POST", "/automations/channel-drafts/draft/answer"),
        ("POST", "/automations/channel-drafts/draft/confirm"),
        ("POST", "/automations/channel-drafts/draft/cancel"),
        ("GET", "/workflows"),
        ("HEAD", "/workflows"),
        ("GET", "/workflows/missing"),
        ("POST", "/workflows/missing/run"),
        ("POST", "/workflows/validate"),
        ("POST", "/workflows/simulate"),
        ("POST", "/workflows/runs/missing/gate"),
        ("GET", "/workflow-hooks"),
        ("PATCH", "/workflow-hooks/missing"),
        ("GET", "/automations/v2/private-alice/webhook-triggers"),
        ("HEAD", "/automations/v2/private-alice/webhook-triggers"),
        ("POST", "/automations/v2/private-alice/webhook-triggers"),
        (
            "GET",
            "/automations/v2/private-alice/webhook-triggers/trigger",
        ),
        (
            "PATCH",
            "/automations/v2/private-alice/webhook-triggers/trigger",
        ),
        (
            "DELETE",
            "/automations/v2/private-alice/webhook-triggers/trigger",
        ),
        (
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/disable",
        ),
        (
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/rotate-secret",
        ),
        (
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/reveal-verification-token",
        ),
        (
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/reset-verification",
        ),
        (
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/import-secret",
        ),
        (
            "GET",
            "/automations/v2/private-alice/webhook-triggers/trigger/deliveries",
        ),
        (
            "GET",
            "/automations/v2/private-alice/webhook-triggers/trigger/deliveries/delivery",
        ),
        ("GET", "/automations/v2/webhook-events"),
        (
            "GET",
            "/automations/v2/webhook-events/event?includePayload=true",
        ),
        ("GET", "/automations/v2/runs/run/webhook-events"),
    ] {
        assert_eq!(
            automation_request(&app, &sign("alice", "viewer", 4), method, path)
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "{method} {path}",
        );
    }

    for prefix in ["/automations", "/routines"] {
        for (method, suffix) in [
            ("GET", ""),
            ("HEAD", ""),
            ("POST", ""),
            ("PATCH", "/missing"),
            ("DELETE", "/missing"),
            ("GET", "/events"),
            ("GET", "/missing/history"),
            ("GET", "/runs"),
            ("GET", "/missing/runs"),
            ("GET", "/runs/missing"),
            ("POST", "/missing/run_now"),
            ("POST", "/runs/missing/approve"),
            ("POST", "/runs/missing/deny"),
            ("POST", "/runs/missing/pause"),
            ("POST", "/runs/missing/resume"),
            ("GET", "/runs/missing/artifacts"),
            ("POST", "/runs/missing/artifacts"),
        ] {
            let path = format!("{prefix}{suffix}");
            assert_eq!(
                automation_request(&app, &sign("alice", "viewer", 4), method, &path)
                    .await
                    .status(),
                StatusCode::FORBIDDEN,
                "{method} {path}"
            );
        }
    }

    // Explicit operation grants unlock only the corresponding surface. The
    // existing per-resource ownership check must still hide Bob's private row.
    write_policy(&path, 5, Some("viewer"), now);
    let mut policy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    policy["deployment_grants"] = json!([{"id": "grant-alice", "deployment_id": "dep-a",
        "principal_kind": "member", "principal_id": "alice", "resource_kind": "deployment", "resource_id": "dep-a",
        "permissions": ["automation.read"]}]);
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    state.reload_hosted_policy().await.unwrap();
    let fresh = sign("alice", "viewer", 5);
    assert_ne!(
        automation_request(&app, &fresh, "GET", "/automations/channel-drafts/pending")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        automation_request(
            &app,
            &fresh,
            "POST",
            "/automations/channel-drafts/draft/confirm"
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        automation_request(
            &app,
            &fresh,
            "GET",
            "/automations/v2/private-alice/webhook-triggers"
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        automation_request(
            &app,
            &fresh,
            "GET",
            "/automations/v2/private-bob/webhook-triggers"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        automation_request(
            &app,
            &fresh,
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/disable"
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    policy["policy_version"] = json!(6);
    policy["deployment_grants"][0]["permissions"] = json!(["automation.read", "automation.write"]);
    std::fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
    state.reload_hosted_policy().await.unwrap();
    let fresh = sign("alice", "viewer", 6);
    assert_ne!(
        automation_request(
            &app,
            &fresh,
            "POST",
            "/automations/channel-drafts/draft/confirm"
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    // With write authority, the handler runs and still requires a real trigger.
    assert_eq!(
        automation_request(
            &app,
            &fresh,
            "POST",
            "/automations/v2/private-alice/webhook-triggers/trigger/disable"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let response = automation_request(&app, &fresh, "GET", "/automations/v2").await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(value["count"], 1);
    assert_eq!(value["automations"][0]["automation_id"], "private-alice");
    assert_eq!(
        automation_request(&app, &fresh, "GET", "/automations/v2/private-bob")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        automation_request(&app, &fresh, "DELETE", "/automations/v2/private-bob")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    for action in ["run_now", "share"] {
        assert_eq!(
            automation_request(
                &app,
                &fresh,
                "POST",
                &format!("/automations/v2/private-alice/{action}")
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        automation_request(&app, &fresh, "DELETE", "/automations/v2/private-alice")
            .await
            .status(),
        StatusCode::OK
    );
    assert!(state.get_automation_v2("private-alice").await.is_none());
    assert!(state.get_automation_v2("private-bob").await.is_some());
}

#[tokio::test]
async fn hosted_policy_worker_readiness_waits_for_snapshot() {
    let state = crate::test_support::test_state().await;
    assert!(state.wait_until_ready_or_failed(0, 0).await);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    assert!(!state.is_ready());
    assert!(!state.wait_until_ready_or_failed(0, 0).await);
    assert!(!state.wait_until_ready_or_failed(1, 0).await);
    write_policy(&path, 1, Some("admin"), crate::now_ms());
    state.reload_hosted_policy().await.unwrap();
    assert!(state.is_ready());
    assert!(state.wait_until_ready_or_failed(1, 0).await);
    state.mark_failed("test", "expected failure").await;
    assert!(!state.wait_until_ready_or_failed(1, 0).await);
}

#[tokio::test]
async fn hosted_policy_admin_can_review_without_generic_admin() {
    let state = crate::test_support::test_state().await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.json");
    state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path.clone());
    let now = crate::now_ms();
    for (version, role) in [(1, "viewer"), (2, "member"), (3, "admin"), (4, "owner")] {
        write_policy(&path, version, Some(role), now);
        state.reload_hosted_policy().await.unwrap();
        let mut verified: VerifiedTenantContext = claims("alice", role, version, now).into();
        state
            .enterprise
            .hosted_policy
            .project(&mut verified)
            .unwrap();
        assert!(!verified
            .strict_projection
            .as_ref()
            .unwrap()
            .has_permission(AccessPermission::Admin));
        let tenant = verified.tenant_context.clone();
        assert_eq!(
            super::super::workflows::workflow_reviewer_is_eligible(&tenant, Some(&verified)),
            matches!(role, "admin" | "owner"),
            "{role}"
        );
        verified.expires_at_ms = now - 1;
        assert!(!super::super::workflows::workflow_reviewer_is_eligible(
            &tenant,
            Some(&verified)
        ));
    }
}
