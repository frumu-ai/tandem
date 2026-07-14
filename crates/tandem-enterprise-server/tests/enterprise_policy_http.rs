#![allow(unused_imports, dead_code)]

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use tandem_enterprise_server::apply_routes;
use tandem_server::build_router_with_extensions;
use tandem_server::test_support::test_state;

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
