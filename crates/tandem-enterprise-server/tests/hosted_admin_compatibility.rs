// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Extension, Router,
};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    hosted_policy::{role_capabilities, HostedPolicyBundle},
    AccessEffect, AuthorityChain, HumanActor, RequestPrincipal, TenantContext,
    VerifiedTenantContext,
};
use tandem_enterprise_server::apply_routes;
use tandem_server::{now_ms, test_support::test_state, AppState};
use tower::ServiceExt;

// Exercise the enterprise handler boundary with the actual hosted projection.
// Signature verification is owned and tested by the upstream ingress module.
fn projected_context(role: &str, delegated_admin: bool) -> VerifiedTenantContext {
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
            .validate("org-a", "dep-a", now, None)
            .unwrap()
            .project_identity(&verified, now)
            .unwrap(),
    );
    verified
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
        let app = app(state.clone(), projected_context(role, delegated));
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
        let mut verified = projected_context("admin", false);
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
async fn legacy_enterprise_admin_read_and_mutation_remain_available() {
    let state = test_state().await;
    let mut verified = projected_context("admin", false);
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
    assert_eq!(state.enterprise.org_units.read().await.len(), 1);
}
