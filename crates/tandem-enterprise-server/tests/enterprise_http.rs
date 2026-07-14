#![allow(unused_imports, dead_code)]
// EAA-10 (TAN-35): enterprise HTTP integration tests, owned by the crate that
// owns the enterprise routes. The router under test is built from tandem-server's
// base router with `tandem_enterprise_server::apply_routes` mounted as a route
// extension — the same wiring `serve_with_route_extensions` uses in production.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use tandem_enterprise_server::apply_routes;
use tandem_memory::response_cache::ResponseCacheScope;
use tandem_memory::types::{
    MemoryChunk, MemoryTenantScope, MemoryTier, SourceObjectLifecycleRecord,
    SourceObjectLifecycleState, DEFAULT_EMBEDDING_DIMENSION,
};
use tandem_server::build_router_with_extensions;
use tandem_server::test_support::{next_event_of_type, test_state};

mod common;
use common::{connector_body, connector_credential_ref_body, source_binding_body};

#[tokio::test]
async fn enterprise_status_returns_public_safe_summary() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/status")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("mode").and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("absent")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("source"))
            .and_then(Value::as_str),
        Some("local_implicit")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("local")
    );
    assert!(payload
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|caps| !caps.is_empty()));
}

#[tokio::test]
async fn enterprise_policy_authoring_publishes_and_previews_argument_predicates() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let tenant_headers = |builder: axum::http::request::Builder| {
        builder
            .header("x-tandem-org-id", "acme")
            .header("x-tandem-workspace-id", "finance")
            .header("x-tandem-actor-id", "admin-user")
            .header("content-type", "application/json")
    };
    let rule = json!({
        "rule_id": "finance-small-payment",
        "policy_id": "finance-policy",
        "version": 1,
        "scope_level": "tenant",
        "tool_patterns": ["mcp.payments.create_payment"],
        "predicate": {
            "expression_version": "permission_predicates/v1",
            "condition": {
                "condition_id": "amount-threshold",
                "selector": "/amount/value",
                "value_type": "decimal",
                "operator": "less_than",
                "operand": "10000.00"
            }
        },
        "effect": "allow",
        "overridable": true,
        "reason_code": "finance_small_payment",
        "reason": "small payments are allowed",
        "updated_at_ms": 0
    });
    let response = app
        .clone()
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies"),
            )
            .body(Body::from(rule.to_string()))
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies/finance-policy/publish"),
            )
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let preview = |amount: &str| {
        json!({
            "input": {
                "tenant_context": {"org_id":"ignored","workspace_id":"ignored","source":"explicit"},
                "tool": "mcp.payments.create_payment",
                "arguments": {"amount":{"value":amount}}
            }
        })
    };
    let response = app
        .clone()
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies/preview"),
            )
            .body(Body::from(preview("9999.00").to_string()))
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload.pointer("/snapshot/effect").and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        payload.get("winning_rule_id").and_then(Value::as_str),
        Some("finance-small-payment")
    );

    let response = app
        .clone()
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies/preview"),
            )
            .body(Body::from(preview("15000.00").to_string()))
            .expect("request"),
        )
        .await
        .expect("response");
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload.pointer("/snapshot/effect").and_then(Value::as_str),
        Some("deny")
    );
    assert_eq!(
        payload.get("default_denied").and_then(Value::as_bool),
        Some(true)
    );
    let replacement = json!({
        "rule_id": "finance-small-payment:v2",
        "policy_id": "finance-policy",
        "version": 2,
        "scope_level": "tenant",
        "tool_patterns": ["mcp.payments.create_payment"],
        "effect": "deny",
        "reason_code": "finance_payments_paused",
        "reason": "payments are paused",
        "updated_at_ms": 0
    });
    let response = app
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies/finance-policy/supersede"),
            )
            .body(Body::from(json!({"rules":[replacement]}).to_string()))
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("superseded")
    );
    assert_eq!(
        payload.pointer("/rules/0/version").and_then(Value::as_u64),
        Some(2)
    );
    assert!(state.enterprise.policy_rules_path.exists());
    let tenant = tandem_enterprise_contract::TenantContext::explicit_user_workspace(
        "acme",
        "finance",
        None,
        "admin-user",
    );
    let audit_events =
        tandem_server::audit::load_protected_audit_events_for_tenant(&state, &tenant).await;
    for event_type in [
        "enterprise.policy.created",
        "enterprise.policy.published",
        "enterprise.policy.superseded",
    ] {
        assert!(audit_events
            .iter()
            .any(|event| event.event_type == event_type));
    }
}

#[tokio::test]
async fn enterprise_policy_mutations_reject_hosted_members_without_admin_role() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policies")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "engineering-member")
                .header("x-tandem-request-source", "tandem-web")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "rule_id": "member-rule",
                        "policy_id": "member-policy",
                        "version": 1,
                        "scope_level": "tenant",
                        "effect": "deny",
                        "tool_patterns": ["mcp.github.*"],
                        "reason_code": "member_rule",
                        "reason": "must not be accepted",
                        "updated_at_ms": 1
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn enterprise_policy_validation_returns_actionable_scope_and_predicate_errors() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policies/validate")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "rule_id": "invalid-resource-rule",
                        "policy_id": "invalid-policy",
                        "version": 1,
                        "scope_level": "resource",
                        "effect": "allow",
                        "predicate": {
                            "expression_version": "permission_predicates/v1",
                            "condition": {
                                "selector": "/amount",
                                "value_type": "decimal",
                                "operator": "is_subdomain_of",
                                "operand": "example.com"
                            }
                        },
                        "reason_code": "invalid_policy",
                        "reason": "validation fixture",
                        "updated_at_ms": 1
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(payload.get("valid").and_then(Value::as_bool), Some(false));
    let messages = payload
        .get("errors")
        .and_then(Value::as_array)
        .expect("validation errors")
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(messages.contains("resource scope requires resource"));
    assert!(messages.contains("is not valid for value type"));
}

#[tokio::test]
async fn enterprise_policy_templates_instantiate_bounded_overrides_as_drafts() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/enterprise/policy-templates")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload
            .get("templates")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policy-templates/coding-agent/instantiate")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "instance_id": "coding-production",
                        "version": 1,
                        "overrides": [{
                            "rule_id": "approved-repository",
                            "predicate_operands": {"repository":"frumu-ai/tandem"}
                        }]
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload
            .pointer("/instantiation/template_version")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(payload
        .pointer("/instantiation/rules")
        .and_then(Value::as_array)
        .is_some_and(|rules| rules
            .iter()
            .all(|rule| rule.get("state").and_then(Value::as_str) == Some("draft"))));
    assert_eq!(
        payload
            .pointer("/instantiation/overrides_applied/0")
            .and_then(Value::as_str),
        Some("approved-repository.predicate_operands.repository")
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policy-templates/finance-agent/instantiate")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "instance_id": "finance-production",
                        "version": 1,
                        "overrides": []
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policy-templates/finance-agent/upgrade")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "instance_id": "finance-production",
                        "version": 2,
                        "overrides": []
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload
            .pointer("/instantiation/template_version")
            .and_then(Value::as_u64),
        Some(2)
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policy-templates/finance-agent/rollback")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "instance_id": "finance-production",
                        "version": 1,
                        "overrides": []
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert!(payload
        .pointer("/instantiation/rules")
        .and_then(Value::as_array)
        .is_some_and(|rules| rules
            .iter()
            .all(|rule| rule.get("state").and_then(Value::as_str) == Some("published"))));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/enterprise/policies")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    let finance_rules = payload
        .pointer("/policy_rules")
        .and_then(Value::as_array)
        .expect("policy rules")
        .iter()
        .filter(|rule| rule.get("policy_id").and_then(Value::as_str) == Some("finance-production"))
        .collect::<Vec<_>>();
    assert!(finance_rules.iter().any(|rule| {
        rule.get("state").and_then(Value::as_str) == Some("superseded")
            && rule.get("template_version").and_then(Value::as_u64) == Some(2)
    }));
    assert!(finance_rules.iter().any(|rule| {
        rule.get("state").and_then(Value::as_str) == Some("published")
            && rule.get("template_version").and_then(Value::as_u64) == Some(1)
    }));
}

#[tokio::test]
async fn enterprise_org_units_storage_threads_request_tenant() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "admin-user")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("ok"));
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("storage_backed")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("clinic-co")
    );
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("workspace_id"))
            .and_then(Value::as_str),
        Some("care-delivery")
    );
    assert_eq!(
        payload
            .get("request_principal")
            .and_then(|row| row.get("actor_id"))
            .and_then(Value::as_str),
        Some("admin-user")
    );
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn enterprise_org_units_create_persists_under_request_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise.org_units_path.clone();
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "unit_id": "hr",
                "taxonomy_id": "department",
                "display_name": "Human Resources",
                "kind": "department",
                "labels": ["people", "benefits"]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .and_then(|units| units.first())
            .and_then(|unit| unit.get("tenant_context"))
            .and_then(|tenant| tenant.get("org_id"))
            .and_then(Value::as_str),
        Some("clinic-co")
    );
    assert_eq!(
        payload
            .get("org_units")
            .and_then(Value::as_array)
            .and_then(|units| units.first())
            .and_then(|unit| unit.get("taxonomy_id"))
            .and_then(Value::as_str),
        Some("department")
    );
}

#[tokio::test]
async fn enterprise_org_units_do_not_cross_tenant_boundaries() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::from(
            json!({
                "unit_id": "executive",
                "display_name": "Executive",
                "kind": "executive_group"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-units")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn enterprise_org_unit_memberships_create_update_and_filter_by_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise.org_unit_memberships_path.clone();
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "unit_id": "doctors",
                "taxonomy_id": "clinical_role",
                "display_name": "Doctors",
                "kind": "clinical_group"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-unit-memberships")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "membership_id": "membership-doctor-user",
                "taxonomy_id": "clinical_role",
                "unit_id": "doctors",
                "member_kind": "human_user",
                "member_id": "doctor.user@example.com",
                "source": "hosted_control_plane"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("memberships")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|membership| membership.get("unit"))
            .and_then(|unit| unit.get("id"))
            .and_then(Value::as_str),
        Some("clinical_role/doctors")
    );
    assert_eq!(
        payload
            .get("memberships")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|membership| membership.get("member"))
            .and_then(|member| member.get("id"))
            .and_then(Value::as_str),
        Some("doctor.user@example.com")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/org-unit-memberships/membership-doctor-user")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(json!({"state": "disabled"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-unit-memberships")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("memberships")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|membership| membership.get("state"))
            .and_then(Value::as_str),
        Some("disabled")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-unit-memberships")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn enterprise_org_unit_memberships_require_existing_unit() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-unit-memberships")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "taxonomy_id": "department",
                "unit_id": "hr",
                "member_kind": "human_user",
                "member_id": "hr.user@example.com"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enterprise_org_unit_access_grants_project_effective_scoped_grants() {
    let state = test_state().await;
    let storage_path = state.enterprise.org_unit_access_grants_path.clone();
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-units")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "unit_id": "doctors",
                "taxonomy_id": "clinical_role",
                "display_name": "Doctors",
                "kind": "clinical_group"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-unit-memberships")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "membership_id": "membership-doctor-user",
                "taxonomy_id": "clinical_role",
                "unit_id": "doctors",
                "member_kind": "human_user",
                "member_id": "doctor.user@example.com",
                "source": "hosted_control_plane"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/org-unit-access-grants")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(
            json!({
                "grant_id": "grant-doctors-patient-cases",
                "taxonomy_id": "clinical_role",
                "unit_id": "doctors",
                "resource_kind": "data_store",
                "resource_id": "patient-cases",
                "permissions": ["view", "read"],
                "data_classes": ["regulated", "customer_data"]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-unit-access-grants/effective?member_kind=human_user&member_id=doctor.user%40example.com")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("grants")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|grant| grant.get("grant_source"))
            .and_then(Value::as_str),
        Some("organization_unit_membership")
    );
    assert_eq!(
        payload
            .get("grants")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|grant| grant.get("resource"))
            .and_then(|resource| resource.get("resource_id"))
            .and_then(Value::as_str),
        Some("patient-cases")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/org-unit-access-grants/grant-doctors-patient-cases")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .header("x-tandem-actor-id", "owner-user")
        .body(Body::from(json!({"state": "disabled"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/org-unit-access-grants/effective?member_kind=human_user&member_id=doctor.user%40example.com")
        .header("x-tandem-org-id", "clinic-co")
        .header("x-tandem-workspace-id", "care-delivery")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn enterprise_source_bindings_storage_threads_request_tenant() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("ok"));
    assert_eq!(
        payload.get("bridge_state").and_then(Value::as_str),
        Some("storage_backed")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload
            .get("tenant_context")
            .and_then(|row| row.get("org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );
    assert_eq!(
        payload
            .get("source_bindings")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn enterprise_readiness_returns_onboarding_counts_without_mutation() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/readiness")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("overall_status").and_then(Value::as_str),
        Some("attention")
    );
    assert_eq!(
        payload.pointer("/counts/org_units").and_then(Value::as_u64),
        Some(0)
    );
    assert!(payload
        .get("checks")
        .and_then(Value::as_array)
        .is_some_and(|checks| checks
            .iter()
            .any(|check| check.get("id").and_then(Value::as_str) == Some("governance_skeleton"))));
    let checks = payload
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks");
    let memory_policy = checks
        .iter()
        .find(|check| check.get("id").and_then(Value::as_str) == Some("memory_context_policy"))
        .expect("memory policy readiness check");
    assert_eq!(
        memory_policy.get("status").and_then(Value::as_str),
        Some("ready")
    );
    assert!(memory_policy
        .get("summary")
        .and_then(Value::as_str)
        .is_some_and(|details| details.contains("memory_policy_mode")));
    assert_eq!(state.enterprise.org_units.read().await.len(), 0);
}

#[tokio::test]
async fn enterprise_readiness_rejects_hosted_member_without_admin_role() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/readiness")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-member")
        .header("x-tandem-request-source", "tandem-web")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_READ_ACCESS_REQUIRED")
    );
}

#[tokio::test]
async fn enterprise_onboarding_preview_returns_operations_and_does_not_persist() {
    let state = test_state().await;
    let org_units_path = state.enterprise.org_units_path.clone();
    let connectors_path = state.enterprise.connectors_path.clone();
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/onboarding-plans/preview")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(
            json!({
                "org_units": [{
                    "unit_id": "finance",
                    "taxonomy_id": "department",
                    "display_name": "Finance"
                }],
                "connectors": [{
                    "connector_id": "google_drive",
                    "provider": "google_drive"
                }],
                "credential_refs": [{
                    "connector_id": "google_drive",
                    "credential_id": "readonly",
                    "credential_class": "read_only",
                    "secret_ref": {
                        "org_id": "acme",
                        "workspace_id": "finance",
                        "provider": "google_kms",
                        "secret_id": "finance-drive-readonly",
                        "name": "Finance Drive readonly"
                    },
                    "source_bound_resource": {
                        "organization_id": "acme",
                        "workspace_id": "finance",
                        "resource_kind": "document_collection",
                        "resource_id": "finance-drive"
                    }
                }],
                "source_bindings": [{
                    "binding_id": "finance-drive",
                    "connector_id": "google_drive",
                    "source_type": "google_drive",
                    "native_source_id": "drive-folder-123",
                    "resource_ref": {
                        "organization_id": "acme",
                        "workspace_id": "finance",
                        "resource_kind": "document_collection",
                        "resource_id": "finance-drive"
                    },
                    "data_class": "confidential",
                    "credential_ref_id": "readonly",
                    "ingestion_policy": {
                        "allow_indexing": true,
                        "allow_prompt_context": true,
                        "require_review": true
                    }
                }],
                "mcp_requirements": [{
                    "name": "pilot-slack",
                    "required_tools": ["post_message"]
                }]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("valid").and_then(Value::as_bool), Some(true));
    assert!(payload
        .get("operations")
        .and_then(Value::as_array)
        .is_some_and(
            |ops| ops.iter().any(|op| op.get("kind").and_then(Value::as_str)
                == Some("source_binding")
                && op.get("status").and_then(Value::as_str) == Some("would_create"))
        ));
    assert_eq!(state.enterprise.org_units.read().await.len(), 0);
    assert_eq!(state.enterprise.connectors.read().await.len(), 0);
    assert!(!org_units_path.exists());
    assert!(!connectors_path.exists());
}

#[tokio::test]
async fn enterprise_onboarding_preview_blocks_raw_credentials_and_destructive_actions() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/onboarding-plans/preview")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(
            json!({
                "org_units": [{
                    "unit_id": "finance",
                    "display_name": "Finance",
                    "action": "delete"
                }],
                "connectors": [{
                    "connector_id": "google_drive",
                    "provider": "google_drive"
                }],
                "credential_refs": [{
                    "connector_id": "google_drive",
                    "credential_id": "readonly",
                    "credential_class": "read_only",
                    "credential_value": {"token": "secret"},
                    "secret_ref": {
                        "org_id": "acme",
                        "workspace_id": "finance",
                        "provider": "google_kms",
                        "secret_id": "finance-drive-readonly",
                        "name": "Finance Drive readonly"
                    }
                }]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("valid").and_then(Value::as_bool), Some(false));
    let errors = payload
        .get("blocking_errors")
        .and_then(Value::as_array)
        .expect("errors");
    assert!(errors
        .iter()
        .any(|error| error.get("code").and_then(Value::as_str)
            == Some("ENTERPRISE_CONNECTOR_CREDENTIAL_VALUE_NOT_ALLOWED")));
    assert!(errors
        .iter()
        .any(|error| error.get("code").and_then(Value::as_str)
            == Some("ENTERPRISE_PREVIEW_DESTRUCTIVE_ACTION_NOT_ALLOWED")));
    assert_eq!(state.enterprise.org_units.read().await.len(), 0);
    assert_eq!(state.enterprise.connectors.read().await.len(), 0);
}

#[tokio::test]
async fn enterprise_connector_providers_expose_google_drive_read_only_constraints() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/connector-providers")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let provider = payload
        .get("providers")
        .and_then(Value::as_array)
        .and_then(|providers| providers.first())
        .expect("provider");
    assert_eq!(
        provider.get("provider").and_then(Value::as_str),
        Some("google_drive")
    );
    assert_eq!(
        provider
            .get("default_credential_class")
            .and_then(Value::as_str),
        Some("read_only")
    );
    assert_eq!(
        provider.get("acl_sync").and_then(Value::as_str),
        Some("not_synced_v1")
    );
    assert!(provider
        .get("supported_credential_classes")
        .and_then(Value::as_array)
        .is_some_and(|classes| classes.len() == 1
            && classes.first().and_then(Value::as_str) == Some("read_only")));
}

#[tokio::test]
async fn enterprise_connectors_reject_hosted_member_without_admin_role() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-member")
        .header("x-tandem-request-source", "tandem-web")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_ADMIN_REQUIRED")
    );
}

#[tokio::test]
async fn enterprise_connectors_create_and_update_persist_under_request_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise.connectors_path.clone();
    let mut rx = state.event_bus.subscribe();
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());
    let create_event =
        next_event_of_type(&mut rx, "enterprise.connector.cache_invalidation_required").await;
    assert_eq!(
        create_event
            .properties
            .get("connector_id")
            .and_then(Value::as_str),
        Some("google_drive")
    );
    assert_eq!(
        create_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("connector_created")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/connectors/google_drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(json!({"state": "paused"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let update_event =
        next_event_of_type(&mut rx, "enterprise.connector.cache_invalidation_required").await;
    assert_eq!(
        update_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("connector_updated")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/connectors")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("connectors")
            .and_then(Value::as_array)
            .and_then(|connectors| connectors.first())
            .and_then(|connector| connector.get("state"))
            .and_then(Value::as_str),
        Some("paused")
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/connectors")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/connectors/google_drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(json!({"state": "active"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enterprise_connector_credentials_are_secret_refs_and_rotate_by_tenant() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = build_router_with_extensions(state, &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = next_event_of_type(&mut rx, "enterprise.connector.cache_invalidation_required").await;

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "credential_id": "readonly",
                "credential_value": "raw-token-never-accepted",
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "google_kms",
                    "secret_id": "kms://finance/raw",
                    "name": "Raw value attempt"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors/google_drive/credential-refs")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_credential_ref_body(
            "readonly",
            "kms://finance/readonly-v1",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let create_event =
        next_event_of_type(&mut rx, "enterprise.connector.cache_invalidation_required").await;
    assert_eq!(
        create_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("connector_credential_ref_created")
    );
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let credential = payload
        .get("connectors")
        .and_then(Value::as_array)
        .and_then(|connectors| connectors.first())
        .and_then(|connector| connector.get("credential_refs"))
        .and_then(Value::as_array)
        .and_then(|credentials| credentials.first())
        .expect("credential ref");
    assert!(credential.get("credential_value").is_none());
    assert_eq!(
        credential.get("credential_class").and_then(Value::as_str),
        Some("read_only")
    );
    assert_eq!(
        credential
            .get("secret_ref")
            .and_then(|secret| secret.get("secret_id"))
            .and_then(Value::as_str),
        Some("kms://finance/readonly-v1")
    );

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/connectors/google_drive/credential-refs/readonly/rotate")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "secret_ref": {
                    "org_id": "acme",
                    "workspace_id": "finance",
                    "provider": "google_kms",
                    "secret_id": "kms://finance/readonly-v2",
                    "name": "Finance Drive read-only secret v2"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let rotate_event =
        next_event_of_type(&mut rx, "enterprise.connector.cache_invalidation_required").await;
    assert_eq!(
        rotate_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("connector_credential_ref_rotated")
    );
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let credential = payload
        .get("connectors")
        .and_then(Value::as_array)
        .and_then(|connectors| connectors.first())
        .and_then(|connector| connector.get("credential_refs"))
        .and_then(Value::as_array)
        .and_then(|credentials| credentials.first())
        .expect("credential ref");
    assert_eq!(
        credential
            .get("secret_ref")
            .and_then(|secret| secret.get("secret_id"))
            .and_then(Value::as_str),
        Some("kms://finance/readonly-v2")
    );
    assert!(credential
        .get("rotated_at_ms")
        .and_then(Value::as_u64)
        .is_some());

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/connectors/google_drive/credential-refs/readonly/rotate")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(
            json!({
                "secret_ref": {
                    "org_id": "other-co",
                    "workspace_id": "finance",
                    "provider": "google_kms",
                    "secret_id": "kms://other/readonly-v3",
                    "name": "Wrong tenant"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial_test::serial]
async fn enterprise_connector_impact_summarizes_revoke_rotate_scope() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/connectors")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(connector_body("google_drive", "google_drive")))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            "finance-drive-impact",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let tenant_scope = MemoryTenantScope {
        org_id: "acme".to_string(),
        workspace_id: "finance".to_string(),
        deployment_id: None,
    };
    let db = tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory db");
    db.upsert_source_object_active_for_tenant(&SourceObjectLifecycleRecord {
        source_object_id: "source-object-finance-impact".to_string(),
        tenant_scope,
        source_binding_id: "finance-drive-impact".to_string(),
        connector_id: "google_drive".to_string(),
        state: SourceObjectLifecycleState::Active,
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        import_namespace: "impact-test".to_string(),
        indexed_path: "impact-test/note.md".to_string(),
        native_object_id: "impact-test/note.md".to_string(),
        resource_ref: json!({
            "organization_id": "acme",
            "workspace_id": "finance",
            "resource_kind": "document_collection",
            "resource_id": "finance-drive-impact"
        }),
        data_class: "financial_record".to_string(),
        content_hash: Some("content-impact".to_string()),
        source_hash: Some("source-impact".to_string()),
        first_seen_at_ms: 500,
        last_seen_at_ms: 2_500,
        tombstoned_at_ms: None,
        metadata: None,
    })
    .await
    .expect("seed source object");

    state.enterprise.ingestion_jobs.write().await.insert(
        "acme::finance::local::job-impact".to_string(),
        tandem_enterprise_contract::IngestionJob {
            job_id: "job-impact".to_string(),
            tenant_context: tandem_types::TenantContext::explicit("acme", "finance", None),
            connector_id: "google_drive".to_string(),
            binding_id: "finance-drive-impact".to_string(),
            state: tandem_enterprise_contract::IngestionJobState::Completed,
            source_object_ids: vec!["source-object-finance-impact".to_string()],
            started_at_ms: Some(1_000),
            finished_at_ms: Some(2_000),
            quarantine_id: None,
        },
    );
    state.enterprise.ingestion_quarantines.write().await.insert(
        "acme::finance::local::quarantine-impact".to_string(),
        tandem_enterprise_contract::IngestionQuarantine {
            quarantine_id: "quarantine-impact".to_string(),
            tenant_context: tandem_types::TenantContext::explicit("acme", "finance", None),
            connector_id: "google_drive".to_string(),
            binding_id: "finance-drive-impact".to_string(),
            source_object_ids: vec!["source-object-finance-impact".to_string()],
            reason: "impact test".to_string(),
            created_at_ms: 1_500,
            reviewed_by: None,
            reviewed_at_ms: None,
            disposition: None,
        },
    );

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/connectors/google_drive/impact")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-request-source", "local_control_panel")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("connector_id").and_then(Value::as_str),
        Some("google_drive")
    );
    assert_eq!(
        payload
            .get("affected_bindings")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        payload
            .get("affected_source_objects")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        payload
            .get("affected_ingestion_jobs")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        payload
            .get("affected_quarantines")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        payload
            .get("cache_invalidation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("compromise_window_started_at_ms")
            .and_then(Value::as_u64),
        Some(500)
    );
    assert_eq!(
        payload
            .get("compromise_window_finished_at_ms")
            .and_then(Value::as_u64),
        Some(2_500)
    );
    assert!(payload
        .get("recommended_actions")
        .and_then(Value::as_array)
        .is_some_and(|actions| actions
            .iter()
            .any(|action| action.as_str() == Some("rotate_connector_credential"))));

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/connectors/google_drive/impact")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-request-source", "local_control_panel")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enterprise_source_bindings_create_and_update_persist_under_request_tenant() {
    let state = test_state().await;
    let storage_path = state.enterprise.source_bindings_path.clone();
    let mut rx = state.event_bus.subscribe();
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(storage_path.exists());
    let create_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        create_event
            .properties
            .get("binding_id")
            .and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        create_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_binding_created")
    );

    let cache_dir = state.memory_db_path.parent().expect("memory parent");
    let response_cache = tandem_memory::ResponseCache::new(cache_dir, 60, 1000)
        .await
        .expect("response cache");
    let finance_scope = ResponseCacheScope::tenant("acme", "finance", None)
        .with_source_bindings(vec!["finance-drive".to_string()]);
    let hr_scope = ResponseCacheScope::tenant("acme", "finance", None)
        .with_source_bindings(vec!["hr-drive".to_string()]);
    let finance_cache_key = tandem_memory::ResponseCache::cache_key_scoped(
        "test-model",
        None,
        "finance source-bound prompt",
        &finance_scope,
    );
    let hr_cache_key = tandem_memory::ResponseCache::cache_key_scoped(
        "test-model",
        None,
        "hr source-bound prompt",
        &hr_scope,
    );
    response_cache
        .put_scoped(
            &finance_cache_key,
            "test-model",
            "finance cached response",
            12,
            &finance_scope,
        )
        .await
        .expect("seed finance cache");
    response_cache
        .put_scoped(
            &hr_cache_key,
            "test-model",
            "hr cached response",
            12,
            &hr_scope,
        )
        .await
        .expect("seed hr cache");

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(
            json!({
                "state": "disabled",
                "ingestion_policy": {
                    "allow_indexing": false,
                    "allow_prompt_context": false,
                    "require_review": true
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let update_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        update_event
            .properties
            .get("binding_id")
            .and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        update_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_binding_updated")
    );
    assert_eq!(
        update_event
            .properties
            .get("cache_scope")
            .and_then(|scope| scope.get("tenant_org_id"))
            .and_then(Value::as_str),
        Some("acme")
    );
    assert!(response_cache
        .get(&finance_cache_key)
        .await
        .expect("read finance cache")
        .is_none());
    assert!(response_cache
        .get(&hr_cache_key)
        .await
        .expect("read hr cache")
        .is_some());

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    let binding = payload
        .get("source_bindings")
        .and_then(Value::as_array)
        .and_then(|bindings| bindings.first())
        .expect("binding");
    assert_eq!(
        binding.get("binding_id").and_then(Value::as_str),
        Some("finance-drive")
    );
    assert_eq!(
        binding.get("state").and_then(Value::as_str),
        Some("disabled")
    );
    assert_eq!(
        binding
            .get("ingestion_policy")
            .and_then(|policy| policy.get("allow_indexing"))
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn enterprise_source_bindings_reject_cross_tenant_resource_ref() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(source_binding_body(
            "wrong-tenant",
            "other-co",
            "finance",
        )))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH")
    );
}

#[tokio::test]
async fn enterprise_source_bindings_do_not_cross_tenant_boundaries() {
    let state = test_state().await;
    let app = build_router_with_extensions(state, &[apply_routes]);
    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let req = Request::builder()
        .method("GET")
        .uri("/enterprise/source-bindings")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(json!({"state": "disabled"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enterprise_source_object_lifecycle_actions_are_admin_gated_and_tenant_scoped() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let binding_id = "finance-drive-lifecycle";
    let source_object_id = "source-object-finance-note-lifecycle";
    let indexed_path = "import-test-lifecycle/note.md";

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            binding_id, "acme", "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;

    let tenant_scope = MemoryTenantScope {
        org_id: "acme".to_string(),
        workspace_id: "finance".to_string(),
        deployment_id: None,
    };
    let db = tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory db");
    db.upsert_source_object_active_for_tenant(&SourceObjectLifecycleRecord {
        source_object_id: source_object_id.to_string(),
        tenant_scope: tenant_scope.clone(),
        source_binding_id: binding_id.to_string(),
        connector_id: "manual-upload".to_string(),
        state: SourceObjectLifecycleState::Active,
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        import_namespace: "import-test-lifecycle".to_string(),
        indexed_path: indexed_path.to_string(),
        native_object_id: indexed_path.to_string(),
        resource_ref: json!({
            "organization_id": "acme",
            "workspace_id": "finance",
            "resource_kind": "document_collection",
            "resource_id": binding_id
        }),
        data_class: "financial_record".to_string(),
        content_hash: Some("content-1".to_string()),
        source_hash: Some("source-1".to_string()),
        first_seen_at_ms: 1_000,
        last_seen_at_ms: 1_000,
        tombstoned_at_ms: None,
        metadata: None,
    })
    .await
    .expect("seed lifecycle");
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));

    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}"
        ))
        .header("x-tandem-org-id", "other-co")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/reindex"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("reindex_requested")
    );
    let reindex_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        reindex_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_object_reindex_requested")
    );

    db.store_chunk(
        &MemoryChunk {
            id: "chunk-finance-note-lifecycle".to_string(),
            content: "finance note stale content must be purged on rescope".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: Some(indexed_path.to_string()),
            source_mtime: None,
            source_size: Some(57),
            source_hash: Some("source-1".to_string()),
            tenant_scope: tenant_scope.clone(),
            subject: None,
            created_at: chrono::Utc::now(),
            token_count: 8,
            metadata: Some(json!({
                "enterprise_source_binding": {
                    "binding_id": binding_id,
                    "connector_id": "manual-upload",
                    "resource_ref": {
                        "organization_id": "acme",
                        "workspace_id": "finance",
                        "resource_kind": "document_collection",
                        "resource_id": binding_id
                    },
                    "data_class": "financial_record",
                    "source_object_id": source_object_id,
                    "native_object_id": indexed_path,
                    "content_hash": "content-1"
                }
            })),
        },
        &vec![0.1; DEFAULT_EMBEDDING_DIMENSION],
    )
    .await
    .expect("seed indexed chunk before rescope");
    assert!(db
        .get_global_chunks_for_tenant(&tenant_scope, 10)
        .await
        .expect("global chunks before rescope")
        .iter()
        .any(|chunk| chunk.id == "chunk-finance-note-lifecycle"));

    let req = Request::builder()
        .method("PATCH")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/scope"
        ))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(
            json!({
                "resource_ref": {
                    "organization_id": "acme",
                    "workspace_id": "finance",
                    "resource_kind": "document_collection",
                    "resource_id": "finance-archive"
                },
                "data_class": "internal"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("rescoped")
    );
    assert_eq!(
        payload.get("chunks_deleted").and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        payload
            .get("source_object")
            .and_then(|object| object.get("state"))
            .and_then(Value::as_str),
        Some("rescoped")
    );
    assert_eq!(
        payload
            .get("source_object")
            .and_then(|object| object.get("data_class"))
            .and_then(Value::as_str),
        Some("internal")
    );
    let rescope_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        rescope_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_object_rescoped")
    );
    assert!(
        !db.get_global_chunks_for_tenant(&tenant_scope, 10)
            .await
            .expect("global chunks after rescope")
            .iter()
            .any(|chunk| chunk.id == "chunk-finance-note-lifecycle"
                || chunk
                    .content
                    .contains("finance note stale content must be purged")),
        "re-scope must purge old indexed chunks so stale resource grants cannot retrieve them"
    );

    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("deleted")
    );

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/enterprise/source-bindings/{binding_id}/source-objects"
        ))
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(0));
}

#[tokio::test]
async fn enterprise_source_binding_disable_purges_indexed_source_objects() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);

    let req = Request::builder()
        .method("POST")
        .uri("/enterprise/source-bindings")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .header("x-tandem-actor-id", "finance-admin")
        .body(Body::from(source_binding_body(
            "finance-drive",
            "acme",
            "finance",
        )))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;

    let tenant_scope = MemoryTenantScope {
        org_id: "acme".to_string(),
        workspace_id: "finance".to_string(),
        deployment_id: None,
    };
    let db = tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory db");
    db.upsert_source_object_active_for_tenant(&SourceObjectLifecycleRecord {
        source_object_id: "source-object-binding-disable".to_string(),
        tenant_scope: tenant_scope.clone(),
        source_binding_id: "finance-drive".to_string(),
        connector_id: "manual-upload".to_string(),
        state: SourceObjectLifecycleState::Active,
        tier: MemoryTier::Global,
        session_id: None,
        project_id: None,
        import_namespace: "binding-disable".to_string(),
        indexed_path: "binding-disable/note.md".to_string(),
        native_object_id: "binding-disable/note.md".to_string(),
        resource_ref: json!({
            "organization_id": "acme",
            "workspace_id": "finance",
            "resource_kind": "document_collection",
            "resource_id": "finance-drive"
        }),
        data_class: "financial_record".to_string(),
        content_hash: Some("content-binding-disable".to_string()),
        source_hash: Some("source-binding-disable".to_string()),
        first_seen_at_ms: 1_000,
        last_seen_at_ms: 1_000,
        tombstoned_at_ms: None,
        metadata: None,
    })
    .await
    .expect("seed lifecycle");
    db.store_chunk(
        &MemoryChunk {
            id: "chunk-binding-disable".to_string(),
            content: "binding disable stale content must be purged".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "file".to_string(),
            source_path: Some("binding-disable/note.md".to_string()),
            source_mtime: None,
            source_size: Some(45),
            source_hash: Some("source-binding-disable".to_string()),
            tenant_scope: tenant_scope.clone(),
            subject: None,
            created_at: chrono::Utc::now(),
            token_count: 7,
            metadata: Some(json!({
                "enterprise_source_binding": {
                    "binding_id": "finance-drive",
                    "connector_id": "manual-upload",
                    "resource_ref": {
                        "organization_id": "acme",
                        "workspace_id": "finance",
                        "resource_kind": "document_collection",
                        "resource_id": "finance-drive"
                    },
                    "data_class": "financial_record",
                    "source_object_id": "source-object-binding-disable",
                    "native_object_id": "binding-disable/note.md",
                    "content_hash": "content-binding-disable"
                }
            })),
        },
        &vec![0.1; DEFAULT_EMBEDDING_DIMENSION],
    )
    .await
    .expect("seed indexed chunk before binding disable");
    assert!(db
        .get_global_chunks_for_tenant(&tenant_scope, 10)
        .await
        .expect("global chunks before binding disable")
        .iter()
        .any(|chunk| chunk.id == "chunk-binding-disable"));

    let req = Request::builder()
        .method("PATCH")
        .uri("/enterprise/source-bindings/finance-drive")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "acme")
        .header("x-tandem-workspace-id", "finance")
        .body(Body::from(json!({"state": "disabled"}).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let update_event = next_event_of_type(
        &mut rx,
        "enterprise.source_binding.cache_invalidation_required",
    )
    .await;
    assert_eq!(
        update_event
            .properties
            .get("reason")
            .and_then(Value::as_str),
        Some("source_binding_updated")
    );

    assert!(
        !db.get_global_chunks_for_tenant(&tenant_scope, 10)
            .await
            .expect("global chunks after binding disable")
            .iter()
            .any(|chunk| chunk.id == "chunk-binding-disable"
                || chunk.content.contains("binding disable stale content")),
        "binding disable must purge indexed chunks so stale source grants cannot retrieve them"
    );
    let source_object = db
        .get_source_object_lifecycle_by_id_for_tenant(
            &tenant_scope,
            "finance-drive",
            "source-object-binding-disable",
        )
        .await
        .expect("source object lookup")
        .expect("source object after binding disable");
    assert_eq!(source_object.state, SourceObjectLifecycleState::Tombstoned);
    assert!(source_object.tombstoned_at_ms.is_some());
}
