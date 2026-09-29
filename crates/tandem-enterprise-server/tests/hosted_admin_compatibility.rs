// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Extension, Router,
};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    hosted_policy::{
        role_capabilities, HostedPolicyBundle, HostedPolicyGrant, HostedPolicyMembership,
        HostedPolicyUnit, HostedPolicyUser,
    },
    AccessEffect, AuthorityChain, HumanActor, IngestionJob, IngestionJobState, IngestionQuarantine,
    RequestPrincipal, TenantContext, VerifiedTenantContext,
};
use tandem_enterprise_server::apply_routes;
use tandem_server::{
    now_ms,
    test_support::{install_hosted_policy_snapshot, test_state},
    AppState,
};
use tower::ServiceExt;

// Exercise the enterprise handler boundary with the actual hosted projection.
// Signature verification is owned and tested by the upstream ingress module.
fn projected_context(
    role: &str,
    delegated_admin: bool,
) -> (VerifiedTenantContext, HostedPolicyBundle) {
    let now = now_ms();
    let capabilities: Vec<String> = role_capabilities(role)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let principal = RequestPrincipal::authenticated_user("alice", "tandem-web");
    let mut verified = VerifiedTenantContext {
        tenant_context: TenantContext::explicit_user_workspace(
            "org-a",
            "dep-a",
            Some("dep-a".into()),
            "alice",
        ),
        human_actor: HumanActor::tandem_user("alice"),
        authority_chain: AuthorityChain::from_request(principal),
        roles: vec![format!("hosted:role:{role}")],
        capabilities: capabilities.clone(),
        org_units: vec![],
        policy_version: Some(1),
        strict_projection: None,
        issuer: "tandem-web".into(),
        audience: "tandem-runtime".into(),
        issued_at_ms: now,
        expires_at_ms: now + 60_000,
        assertion_id: uuid::Uuid::new_v4().to_string(),
        assertion_key_id: None,
    };
    let grants = if delegated_admin {
        json!([{
            "id": "delegated-admin", "deployment_id": "dep-a", "principal_kind": "member", "principal_id": "alice",
            "resource_kind": "deployment", "resource_id": "dep-a", "permissions": ["hosted.admin"]
        }])
    } else {
        json!([])
    };
    let bundle: HostedPolicyBundle = serde_json::from_value(json!({
        "schema_version": 1, "policy_version": 1, "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
        "users": [{"id": "alice", "email": null, "username": null, "role": role,
            "capabilities": capabilities, "is_active": true, "email_verified": true}],
        "org_units": [], "org_unit_memberships": [], "deployment_grants": grants
    })).unwrap();
    verified.strict_projection = Some(
        bundle
            .clone()
            .validate("org-a", "dep-a", now, None)
            .unwrap()
            .project_identity(&verified, now)
            .unwrap(),
    );
    (verified, bundle)
}

fn revoked_admin_bundle(mut bundle: HostedPolicyBundle) -> HostedPolicyBundle {
    bundle.policy_version += 1;
    bundle.generated_at = chrono::DateTime::from_timestamp_millis(now_ms() as i64).unwrap();
    bundle.users[0].role = "member".into();
    bundle.users[0].capabilities = role_capabilities("member")
        .into_iter()
        .map(str::to_owned)
        .collect();
    bundle
}

fn with_org_unit_roster(mut bundle: HostedPolicyBundle) -> HostedPolicyBundle {
    bundle.users.push(HostedPolicyUser {
        id: "bob".into(),
        email: None,
        username: None,
        role: "member".into(),
        capabilities: role_capabilities("member")
            .into_iter()
            .map(str::to_owned)
            .collect(),
        is_active: true,
        email_verified: true,
    });
    bundle.org_units.push(HostedPolicyUnit {
        id: "eng".into(),
        slug: "eng".into(),
        display_name: "Engineering".into(),
        kind: "department".into(),
        state: "active".into(),
    });
    bundle.org_unit_memberships.push(HostedPolicyMembership {
        unit_id: "eng".into(),
        user_id: "bob".into(),
    });
    bundle.deployment_grants.push(HostedPolicyGrant {
        id: "eng-view".into(),
        deployment_id: Some("dep-a".into()),
        principal_kind: "org_unit".into(),
        principal_id: "eng".into(),
        resource_kind: "deployment".into(),
        resource_id: "dep-a".into(),
        permissions: vec!["hosted.view".into()],
    });
    bundle
}

fn app(state: AppState, verified: VerifiedTenantContext) -> Router {
    apply_routes(Router::new())
        .layer(Extension(verified.tenant_context.clone()))
        .layer(Extension(RequestPrincipal::authenticated_user(
            "alice",
            "tandem-web",
        )))
        .layer(Extension(verified))
        .with_state(state)
}

fn request(method: &str, path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn unit(taxonomy: &str) -> Value {
    json!({"unit_id": "hr", "taxonomy_id": taxonomy, "display_name": "Human Resources", "kind": "department"})
}

const ORG_UNIT_READ_PATHS: [&str; 4] = [
    "/enterprise/org-units",
    "/enterprise/org-unit-memberships",
    "/enterprise/org-unit-access-grants",
    "/enterprise/org-unit-access-grants/effective?member_kind=human_user&member_id=bob",
];

#[tokio::test]
async fn hosted_org_unit_roster_and_grants_require_current_admin() {
    for (role, delegated_admin, allowed) in [
        ("viewer", false, false),
        ("member", false, false),
        ("admin", false, true),
        ("owner", false, true),
        ("viewer", true, true),
    ] {
        let state = test_state().await;
        let (verified, bundle) = projected_context(role, delegated_admin);
        install_hosted_policy_snapshot(&state, with_org_unit_roster(bundle)).unwrap();
        let app = app(state, verified);
        for path in ORG_UNIT_READ_PATHS {
            let response = app
                .clone()
                .oneshot(request("GET", path, json!({})))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
                "{role}, delegated={delegated_admin}: GET {path}"
            );
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.unwrap())
                    .unwrap();
            if allowed {
                assert_eq!(body["count"], 1, "authorized GET {path}: {body}");
                if path == "/enterprise/org-unit-memberships" {
                    assert_eq!(body["memberships"][0]["member"]["id"], "bob");
                }
            } else {
                assert_eq!(body["code"], "ENTERPRISE_ADMIN_REQUIRED");
                assert!(body.get("memberships").is_none());
            }
        }
    }
}

#[tokio::test]
async fn hosted_org_unit_roster_read_rechecks_admin_after_registry_wait() {
    let state = test_state().await;
    let (verified, bundle) = projected_context("admin", false);
    let bundle = with_org_unit_roster(bundle);
    install_hosted_policy_snapshot(&state, bundle.clone()).unwrap();
    let held_registry = state.enterprise.org_units.write().await;
    let app = app(state.clone(), verified);
    let request_task = tokio::spawn(async move {
        app.oneshot(request(
            "GET",
            "/enterprise/org-unit-memberships",
            json!({}),
        ))
        .await
        .unwrap()
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !request_task.is_finished(),
        "read should wait for the registry"
    );
    install_hosted_policy_snapshot(&state, revoked_admin_bundle(bundle)).unwrap();
    drop(held_registry);

    let response = request_task.await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "ENTERPRISE_ADMIN_REQUIRED");
    assert!(body.get("memberships").is_none());
}

#[tokio::test]
async fn hosted_admin_projection_admits_enterprise_management_but_not_global_authority() {
    for (role, delegated, allowed) in [
        ("viewer", false, false),
        ("member", false, false),
        ("admin", false, true),
        ("owner", false, true),
        ("viewer", true, true),
    ] {
        let state = test_state().await;
        let (verified, bundle) = projected_context(role, delegated);
        install_hosted_policy_snapshot(&state, bundle).unwrap();
        let app = app(state.clone(), verified);
        for (method, path, body) in [
            ("GET", "/enterprise/readiness", json!({})),
            ("POST", "/enterprise/onboarding-plans/preview", json!({})),
            ("POST", "/enterprise/org-units", unit("department")),
        ] {
            let response = app
                .clone()
                .oneshot(request(method, path, body))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
                "{role}, delegated={delegated}: {method} {path}"
            );
        }
        assert_eq!(
            state.enterprise.org_units.read().await.len(),
            usize::from(allowed)
        );
        if allowed {
            for (path, body) in [
                ("/enterprise/org-units", unit("hosted-control-plane")),
                (
                    "/enterprise/org-unit-memberships",
                    json!({"membership_id": "forged", "taxonomy_id": "department",
                    "unit_id": "hr", "member_kind": "human_user", "member_id": "alice", "source": "hosted_control_plane"}),
                ),
            ] {
                let response = app
                    .clone()
                    .oneshot(request("POST", path, body))
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
                let body: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                        .unwrap();
                assert_eq!(body["code"], "ENTERPRISE_HOSTED_REGISTRY_READ_ONLY");
            }
            let response = app.clone().oneshot(request("POST", "/enterprise/policies", json!({
                "rule_id": "global-rule", "policy_id": "global-policy", "version": 1,
                "scope_level": "enterprise", "effect": "deny", "tool_patterns": ["mcp.secrets.export"],
                "reason_code": "test", "reason": "test", "updated_at_ms": 1
            }))).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "hosted admin is not global enterprise admin"
            );
            assert!(state.enterprise.policy_rules.read().await.is_empty());
            assert!(state
                .enterprise
                .org_unit_memberships
                .read()
                .await
                .is_empty());
        }
    }
}

#[tokio::test]
async fn hosted_admin_projection_denials_do_not_fall_back_to_roles() {
    for fault in ["missing", "expired", "wrong-deployment", "deny"] {
        let state = test_state().await;
        let (mut verified, bundle) = projected_context("admin", false);
        install_hosted_policy_snapshot(&state, bundle).unwrap();
        verified.roles.push("hosted:admin".into());
        match fault {
            "missing" => verified.strict_projection = None,
            "expired" => {
                verified
                    .strict_projection
                    .as_mut()
                    .unwrap()
                    .assertion
                    .expires_at_ms = now_ms() - 1
            }
            "wrong-deployment" => {
                for grant in &mut verified.strict_projection.as_mut().unwrap().grants {
                    grant.resource.resource_id = "another-deployment".into();
                }
            }
            _ => {
                let strict = verified.strict_projection.as_mut().unwrap();
                let mut deny = strict.grants[0].clone();
                deny.effect = AccessEffect::Deny;
                strict.grants.push(deny);
            }
        }
        let app = app(state.clone(), verified);
        for (method, path, body) in [
            ("GET", "/enterprise/readiness", json!({})),
            ("POST", "/enterprise/onboarding-plans/preview", json!({})),
            ("POST", "/enterprise/org-units", unit("department")),
        ] {
            let response = app
                .clone()
                .oneshot(request(method, path, body))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{fault}: {path}");
        }
        assert!(state.enterprise.org_units.read().await.is_empty());
    }
}

#[tokio::test]
async fn hosted_admin_revoked_while_connector_lock_is_held_cannot_create() {
    let state = test_state().await;
    let (verified, bundle) = projected_context("admin", false);
    install_hosted_policy_snapshot(&state, bundle.clone()).unwrap();
    let app = app(state.clone(), verified);

    let allowed = app
        .clone()
        .oneshot(request(
            "POST",
            "/enterprise/connectors",
            json!({"connector_id": "before-revocation", "provider": "google_drive"}),
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let persisted_before = tokio::fs::read(&state.enterprise.connectors_path)
        .await
        .unwrap();

    let held = state.enterprise.connectors.read().await;
    let request_task = tokio::spawn(async move {
        app.oneshot(request(
            "POST",
            "/enterprise/connectors",
            json!({"connector_id": "after-revocation", "provider": "google_drive"}),
        ))
        .await
        .unwrap()
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !request_task.is_finished(),
        "request should wait for connector lock"
    );

    install_hosted_policy_snapshot(&state, revoked_admin_bundle(bundle)).unwrap();
    drop(held);

    let response = request_task.await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let connectors = state.enterprise.connectors.read().await;
    assert_eq!(connectors.len(), 1);
    assert!(connectors
        .values()
        .all(|connector| connector.connector_id != "after-revocation"));
    assert_eq!(
        tokio::fs::read(&state.enterprise.connectors_path)
            .await
            .unwrap(),
        persisted_before
    );
}

#[tokio::test]
async fn hosted_admin_revoked_while_quarantine_job_lock_is_held_changes_neither_registry() {
    let state = test_state().await;
    let (verified, bundle) = projected_context("admin", false);
    install_hosted_policy_snapshot(&state, bundle.clone()).unwrap();
    let tenant = verified.tenant_context.clone();
    state.enterprise.ingestion_jobs.write().await.insert(
        "job-review".into(),
        IngestionJob {
            job_id: "job-review".into(),
            tenant_context: tenant.clone(),
            connector_id: "manual_upload".into(),
            binding_id: "review-binding".into(),
            state: IngestionJobState::Quarantined,
            source_object_ids: vec![],
            started_at_ms: Some(1_000),
            finished_at_ms: None,
            quarantine_id: Some("quarantine-review".into()),
        },
    );
    state.enterprise.ingestion_quarantines.write().await.insert(
        "quarantine-review".into(),
        IngestionQuarantine {
            quarantine_id: "quarantine-review".into(),
            tenant_context: tenant,
            connector_id: "manual_upload".into(),
            binding_id: "review-binding".into(),
            source_object_ids: vec![],
            reason: "review required".into(),
            created_at_ms: 1_000,
            reviewed_by: None,
            reviewed_at_ms: None,
            disposition: None,
        },
    );
    let held = state.enterprise.ingestion_jobs.read().await;
    let app = app(state.clone(), verified);
    let request_task = tokio::spawn(async move {
        app.oneshot(request(
            "PATCH",
            "/enterprise/ingestion-quarantines/quarantine-review/review",
            json!({"disposition": "delete"}),
        ))
        .await
        .unwrap()
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !request_task.is_finished(),
        "request should wait for job lock"
    );
    install_hosted_policy_snapshot(&state, revoked_admin_bundle(bundle)).unwrap();
    drop(held);

    assert_eq!(request_task.await.unwrap().status(), StatusCode::FORBIDDEN);
    let quarantines = state.enterprise.ingestion_quarantines.read().await;
    assert_eq!(quarantines["quarantine-review"].disposition, None);
    assert_eq!(quarantines["quarantine-review"].reviewed_at_ms, None);
    let jobs = state.enterprise.ingestion_jobs.read().await;
    assert_eq!(jobs["job-review"].state, IngestionJobState::Quarantined);
}

#[tokio::test]
async fn legacy_enterprise_admin_read_and_mutation_remain_available() {
    let state = test_state().await;
    let (mut verified, _) = projected_context("admin", false);
    verified.policy_version = None;
    verified.strict_projection = None;
    verified.roles = vec!["workspace:admin".into()];
    verified.capabilities.clear();
    let app = app(state.clone(), verified);
    for (method, path, body) in [
        ("GET", "/enterprise/readiness", json!({})),
        ("POST", "/enterprise/onboarding-plans/preview", json!({})),
        ("POST", "/enterprise/org-units", unit("department")),
    ] {
        let response = app
            .clone()
            .oneshot(request(method, path, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "legacy admin: {path}");
    }
    for path in ORG_UNIT_READ_PATHS {
        let response = app
            .clone()
            .oneshot(request("GET", path, json!({})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "legacy admin: {path}");
    }
    assert_eq!(state.enterprise.org_units.read().await.len(), 1);
}
