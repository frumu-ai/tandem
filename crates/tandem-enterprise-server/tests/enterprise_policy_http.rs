#![allow(unused_imports, dead_code)]

use axum::body::{to_bytes, Body};
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use tandem_enterprise_server::apply_routes;
use tandem_server::build_router_with_extensions;
use tandem_server::test_support::test_state;

fn verified_admin_context(roles: &[&str]) -> tandem_enterprise_contract::VerifiedTenantContext {
    use tandem_enterprise_contract::{
        AuthorityChain, HumanActor, RequestPrincipal, TenantContext, VerifiedTenantContext,
    };

    let principal = RequestPrincipal::authenticated_user("admin-user", "tandem-web");
    VerifiedTenantContext {
        tenant_context: TenantContext::explicit_user_workspace(
            "acme",
            "engineering",
            None,
            "admin-user",
        ),
        human_actor: HumanActor::tandem_user("admin-user"),
        authority_chain: AuthorityChain::from_request(principal),
        roles: roles.iter().map(|role| (*role).to_string()).collect(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: u64::MAX,
        assertion_id: "policy-authoring-test".to_string(),
        assertion_key_id: None,
    }
}

fn global_policy_rule(rule_id: &str, policy_id: &str) -> Value {
    json!({
        "rule_id": rule_id,
        "policy_id": policy_id,
        "version": 1,
        "scope_level": "enterprise",
        "effect": "deny",
        "tool_patterns": ["mcp.secrets.export"],
        "reason_code": "global_secret_export_denied",
        "reason": "secret export is denied globally",
        "updated_at_ms": 1
    })
}

#[tokio::test]
async fn enterprise_global_policy_creation_requires_global_admin_and_preserves_scope() {
    let state = test_state().await;
    let workspace_app = build_router_with_extensions(state.clone(), &[apply_routes])
        .layer(Extension(verified_admin_context(&["workspace:admin"])));
    let request = |rule: Value| {
        Request::builder()
            .method("POST")
            .uri("/enterprise/policies")
            .header("x-tandem-org-id", "acme")
            .header("x-tandem-workspace-id", "engineering")
            .header("x-tandem-actor-id", "admin-user")
            .header("x-tandem-request-source", "tandem-web")
            .header("content-type", "application/json")
            .body(Body::from(rule.to_string()))
            .expect("request")
    };

    let response = workspace_app
        .oneshot(request(global_policy_rule(
            "workspace-global-rule",
            "workspace-global-policy",
        )))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(state.enterprise.policy_rules.read().await.is_empty());

    let enterprise_app = build_router_with_extensions(state.clone(), &[apply_routes])
        .layer(Extension(verified_admin_context(&["enterprise:admin"])));
    let response = enterprise_app
        .clone()
        .oneshot(request(global_policy_rule(
            "enterprise-global-rule",
            "enterprise-global-policy",
        )))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert!(payload.pointer("/rules/0/tenant_context").is_none());
    assert!(state
        .enterprise
        .policy_rules
        .read()
        .await
        .get("enterprise-global-rule")
        .is_some_and(|rule| rule.tenant_context.is_none()));

    let tenant_rule = json!({
        "rule_id": "tenant-rule",
        "policy_id": "tenant-policy",
        "version": 1,
        "scope_level": "tenant",
        "effect": "allow",
        "tool_patterns": ["mcp.github.get_issue"],
        "reason_code": "tenant_read_allowed",
        "reason": "tenant reads are allowed",
        "updated_at_ms": 1
    });
    let response = enterprise_app
        .oneshot(request(tenant_rule))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let tenant = state
        .enterprise
        .policy_rules
        .read()
        .await
        .get("tenant-rule")
        .and_then(|rule| rule.tenant_context.clone())
        .expect("tenant context");
    assert_eq!(tenant.org_id, "acme");
    assert_eq!(tenant.workspace_id, "engineering");

    let audit_tenant = tandem_enterprise_contract::TenantContext::explicit_user_workspace(
        "acme",
        "engineering",
        None,
        "admin-user",
    );
    let audit_events =
        tandem_server::audit::load_protected_audit_events_for_tenant(&state, &audit_tenant).await;
    assert!(audit_events.iter().any(|event| {
        event.event_type == "enterprise.policy.created"
            && event.payload.get("rule_id").and_then(Value::as_str)
                == Some("enterprise-global-rule")
            && event.payload.get("policy_scope").and_then(Value::as_str)
                == Some("enterprise_global")
            && event
                .payload
                .get("rule_tenant_context")
                .is_some_and(Value::is_null)
    }));
}

#[tokio::test]
async fn enterprise_policy_supersede_normalizes_tenant_scope_and_preserves_tenant_boundary() {
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyRuleState,
        EnterprisePolicyScopeLevel, TenantContext,
    };

    let state = test_state().await;
    let stored_tenant =
        TenantContext::explicit_user_workspace(" Acme ", " ENGINEERING ", None, "admin-user");
    let rule = EnterprisePolicyRule::new(
        "normalized-existing",
        "normalized-policy",
        EnterprisePolicyScopeLevel::Tenant,
        EnterprisePolicyEffect::Allow,
    )
    .with_tenant_context(stored_tenant)
    .with_tool_patterns(vec!["mcp.github.get_issue".to_string()]);
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(rule.rule_id.clone(), rule);
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let replacement = json!({
        "rule_id": "normalized-replacement",
        "policy_id": "ignored",
        "version": 2,
        "scope_level": "tenant",
        "effect": "deny",
        "tool_patterns": ["mcp.github.get_issue"],
        "reason_code": "repository_read_paused",
        "reason": "repository reads are paused",
        "updated_at_ms": 1
    });
    let supersede_request = |org_id: &str| {
        Request::builder()
            .method("POST")
            .uri("/enterprise/policies/normalized-policy/supersede")
            .header("x-tandem-org-id", org_id)
            .header("x-tandem-workspace-id", "engineering")
            .header("x-tandem-actor-id", "admin-user")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"rules":[replacement.clone()]}).to_string(),
            ))
            .expect("request")
    };

    let response = app
        .clone()
        .oneshot(supersede_request("other-org"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        state
            .enterprise
            .policy_rules
            .read()
            .await
            .get("normalized-existing")
            .map(|rule| rule.state),
        Some(EnterprisePolicyRuleState::Published)
    );

    let response = app
        .oneshot(supersede_request("acme"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let rules = state.enterprise.policy_rules.read().await;
    assert_eq!(
        rules.get("normalized-existing").map(|rule| rule.state),
        Some(EnterprisePolicyRuleState::Superseded)
    );
    let replacement = rules
        .get("normalized-replacement")
        .expect("replacement rule");
    assert_eq!(replacement.state, EnterprisePolicyRuleState::Published);
    let replacement_tenant = replacement
        .tenant_context
        .as_ref()
        .expect("replacement tenant");
    assert_eq!(replacement_tenant.org_id, "acme");
    assert_eq!(replacement_tenant.workspace_id, "engineering");
    assert_eq!(replacement_tenant.deployment_id, None);
}

#[tokio::test]
async fn enterprise_global_policy_supersede_rejects_non_enterprise_replacements() {
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyRuleState,
        EnterprisePolicyScopeLevel,
    };

    let state = test_state().await;
    let original = EnterprisePolicyRule::new(
        "global-existing",
        "global-policy",
        EnterprisePolicyScopeLevel::Enterprise,
        EnterprisePolicyEffect::Deny,
    )
    .with_tool_patterns(vec!["mcp.secrets.export".to_string()]);
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(original.rule_id.clone(), original);
    let app = build_router_with_extensions(state.clone(), &[apply_routes])
        .layer(Extension(verified_admin_context(&["enterprise:admin"])));
    let replacement = json!({
        "rule_id": "invalid-global-replacement",
        "policy_id": "ignored-client-policy",
        "version": 2,
        "scope_level": "tenant",
        "effect": "deny",
        "tool_patterns": ["mcp.secrets.export"],
        "reason_code": "global_secret_export_denied",
        "reason": "secret export remains denied globally",
        "updated_at_ms": 1
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policies/global-policy/supersede")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("x-tandem-request-source", "tandem-web")
                .header("content-type", "application/json")
                .body(Body::from(json!({"rules":[replacement]}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("ENTERPRISE_POLICY_VALIDATION_FAILED")
    );
    assert_eq!(
        payload.pointer("/errors/0/path").and_then(Value::as_str),
        Some("scope_level")
    );
    let rules = state.enterprise.policy_rules.read().await;
    assert_eq!(
        rules.get("global-existing").map(|rule| rule.state),
        Some(EnterprisePolicyRuleState::Published)
    );
    assert!(!rules.contains_key("invalid-global-replacement"));
}

#[tokio::test]
async fn enterprise_global_policy_update_and_publish_enforce_enterprise_scope() {
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyRuleState,
        EnterprisePolicyScopeLevel,
    };

    let state = test_state().await;
    let mut original = EnterprisePolicyRule::new(
        "global-draft",
        "global-draft-policy",
        EnterprisePolicyScopeLevel::Enterprise,
        EnterprisePolicyEffect::Deny,
    )
    .with_tool_patterns(vec!["mcp.secrets.export".to_string()]);
    original.state = EnterprisePolicyRuleState::Draft;
    state
        .enterprise
        .policy_rules
        .write()
        .await
        .insert(original.rule_id.clone(), original.clone());
    let app = build_router_with_extensions(state.clone(), &[apply_routes])
        .layer(Extension(verified_admin_context(&["enterprise:admin"])));
    let invalid_replacement = json!({
        "rule_id": "ignored-client-rule",
        "policy_id": "global-draft-policy",
        "version": 1,
        "scope_level": "workspace",
        "effect": "deny",
        "tool_patterns": ["mcp.secrets.export"],
        "reason_code": "global_secret_export_denied",
        "reason": "secret export remains denied globally",
        "updated_at_ms": 1
    });
    let tenant_headers = |builder: axum::http::request::Builder| {
        builder
            .header("x-tandem-org-id", "acme")
            .header("x-tandem-workspace-id", "engineering")
            .header("x-tandem-actor-id", "admin-user")
            .header("x-tandem-request-source", "tandem-web")
            .header("content-type", "application/json")
    };

    let response = app
        .clone()
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("PATCH")
                    .uri("/enterprise/policies/global-draft"),
            )
            .body(Body::from(invalid_replacement.to_string()))
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        state
            .enterprise
            .policy_rules
            .read()
            .await
            .get("global-draft"),
        Some(&original)
    );

    state
        .enterprise
        .policy_rules
        .write()
        .await
        .get_mut("global-draft")
        .expect("global draft")
        .scope_level = EnterprisePolicyScopeLevel::Workspace;
    let response = app
        .oneshot(
            tenant_headers(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies/global-draft-policy/publish"),
            )
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let rules = state.enterprise.policy_rules.read().await;
    let invalid_draft = rules.get("global-draft").expect("global draft remains");
    assert_eq!(invalid_draft.state, EnterprisePolicyRuleState::Draft);
    assert_eq!(
        invalid_draft.scope_level,
        EnterprisePolicyScopeLevel::Workspace
    );
}

#[tokio::test]
async fn enterprise_policy_supersede_rejects_cross_tenant_rule_id_collisions() {
    use tandem_enterprise_contract::{
        EnterprisePolicyEffect, EnterprisePolicyRule, EnterprisePolicyRuleState,
        EnterprisePolicyScopeLevel, TenantContext,
    };

    let state = test_state().await;
    let target_tenant =
        TenantContext::explicit_user_workspace("acme", "finance", None, "admin-user");
    let other_tenant =
        TenantContext::explicit_user_workspace("other-org", "ops", None, "other-admin");
    let target_rule = EnterprisePolicyRule::new(
        "finance-existing",
        "finance-policy",
        EnterprisePolicyScopeLevel::Tenant,
        EnterprisePolicyEffect::Allow,
    )
    .with_tenant_context(target_tenant)
    .with_tool_patterns(vec!["mcp.payments.create_payment".to_string()]);
    let other_rule = EnterprisePolicyRule::new(
        "shared-rule-id",
        "other-policy",
        EnterprisePolicyScopeLevel::Tenant,
        EnterprisePolicyEffect::Deny,
    )
    .with_tenant_context(other_tenant.clone())
    .with_tool_patterns(vec!["mcp.secrets.rotate".to_string()]);
    {
        let mut rules = state.enterprise.policy_rules.write().await;
        rules.insert(target_rule.rule_id.clone(), target_rule);
        rules.insert(other_rule.rule_id.clone(), other_rule);
    }
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let replacement = json!({
        "rule_id": "shared-rule-id",
        "policy_id": "ignored-client-policy",
        "version": 2,
        "scope_level": "tenant",
        "tool_patterns": ["mcp.payments.create_payment"],
        "effect": "deny",
        "reason_code": "finance_paused",
        "reason": "payments are paused",
        "updated_at_ms": 0
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policies/finance-policy/supersede")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "finance")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(json!({"rules":[replacement]}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let rules = state.enterprise.policy_rules.read().await;
    let collision = rules.get("shared-rule-id").expect("other tenant rule");
    assert_eq!(collision.policy_id, "other-policy");
    assert_eq!(collision.tenant_context.as_ref(), Some(&other_tenant));
    assert_eq!(
        rules.get("finance-existing").map(|rule| rule.state),
        Some(EnterprisePolicyRuleState::Published)
    );
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
        .clone()
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
    let response = app
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
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let rules = state.enterprise.policy_rules.read().await;
    assert!(rules.values().any(|rule| {
        rule.rule_id == "finance-small-payment"
            && rule.state == tandem_enterprise_contract::EnterprisePolicyRuleState::Superseded
    }));
    assert!(rules.values().any(|rule| {
        rule.rule_id == "finance-small-payment:v2"
            && rule.state == tandem_enterprise_contract::EnterprisePolicyRuleState::Published
    }));
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
async fn enterprise_policy_publish_validates_every_rule_before_mutating_any() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    for rule in [
        json!({
            "rule_id": "atomic-publish-valid",
            "policy_id": "atomic-publish-policy",
            "version": 1,
            "scope_level": "tenant",
            "effect": "allow",
            "tool_patterns": ["mcp.github.get_issue"],
            "reason_code": "valid_draft",
            "reason": "valid draft",
            "updated_at_ms": 1
        }),
        json!({
            "rule_id": "atomic-publish-expired",
            "policy_id": "atomic-publish-policy",
            "version": 1,
            "scope_level": "tenant",
            "effect": "deny",
            "tool_patterns": ["mcp.github.delete_repository"],
            "expires_at_ms": 1,
            "reason_code": "expired_draft",
            "reason": "expired draft",
            "updated_at_ms": 1
        }),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/enterprise/policies")
                    .header("x-tandem-org-id", "acme")
                    .header("x-tandem-workspace-id", "engineering")
                    .header("x-tandem-actor-id", "admin-user")
                    .header("content-type", "application/json")
                    .body(Body::from(rule.to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policies/atomic-publish-policy/publish")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(state
        .enterprise
        .policy_rules
        .read()
        .await
        .values()
        .filter(|rule| rule.policy_id == "atomic-publish-policy")
        .all(|rule| rule.state == tandem_enterprise_contract::EnterprisePolicyRuleState::Draft));
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
async fn enterprise_policy_template_instance_ids_are_unique_within_a_tenant() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
    let request_body = |template: &str| {
        Request::builder()
            .method("POST")
            .uri(format!(
                "/enterprise/policy-templates/{template}/instantiate"
            ))
            .header("x-tandem-org-id", "acme")
            .header("x-tandem-workspace-id", "engineering")
            .header("x-tandem-actor-id", "admin-user")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "instance_id": "shared-template-instance",
                    "version": 1,
                    "overrides": []
                })
                .to_string(),
            ))
            .expect("request")
    };

    let response = app
        .clone()
        .oneshot(request_body("coding-agent"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let original_rules = state.enterprise.policy_rules.read().await.clone();

    let response = app
        .oneshot(request_body("finance-agent"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(*state.enterprise.policy_rules.read().await, original_rules);
}

#[tokio::test]
async fn enterprise_policy_template_transitions_require_template_ownership() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
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
                        "instance_id": "finance-owned-instance",
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
    let original_rules = state.enterprise.policy_rules.read().await.clone();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/enterprise/policy-templates/coding-agent/upgrade")
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "instance_id": "finance-owned-instance",
                        "version": 1,
                        "overrides": []
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(*state.enterprise.policy_rules.read().await, original_rules);
}

#[tokio::test]
async fn enterprise_policy_template_transition_rejects_expired_overrides_without_mutation() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
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
                        "instance_id": "finance-expiry-guard",
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
    let original_rules = state.enterprise.policy_rules.read().await.clone();

    let response = app
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
                        "instance_id": "finance-expiry-guard",
                        "version": 2,
                        "overrides": [{
                            "rule_id": "small-payments",
                            "expires_at_ms": 1
                        }]
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(
        payload.pointer("/errors/0/path").and_then(Value::as_str),
        Some("expires_at_ms")
    );
    assert_eq!(*state.enterprise.policy_rules.read().await, original_rules);
}

#[tokio::test]
async fn enterprise_policy_template_draft_updates_preserve_ownership_metadata() {
    let state = test_state().await;
    let app = build_router_with_extensions(state.clone(), &[apply_routes]);
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
                        "instance_id": "finance-edit-guard",
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
    let mut replacement = payload
        .pointer("/instantiation/rules/0")
        .cloned()
        .expect("template rule");
    let rule_id = replacement
        .get("rule_id")
        .and_then(Value::as_str)
        .expect("rule id")
        .to_string();
    let replacement_object = replacement.as_object_mut().expect("rule object");
    replacement_object.insert("version".to_string(), json!(1));
    replacement_object.insert("policy_id".to_string(), json!("detached-policy"));
    replacement_object.remove("template_id");
    replacement_object.remove("template_version");

    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/enterprise/policies/{rule_id}"))
                .header("x-tandem-org-id", "acme")
                .header("x-tandem-workspace-id", "engineering")
                .header("x-tandem-actor-id", "admin-user")
                .header("content-type", "application/json")
                .body(Body::from(replacement.to_string()))
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
            .pointer("/rules/0/policy_id")
            .and_then(Value::as_str),
        Some("finance-edit-guard")
    );
    assert_eq!(
        payload.pointer("/rules/0/version").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        payload
            .pointer("/rules/0/template_id")
            .and_then(Value::as_str),
        Some("finance-agent")
    );
    assert_eq!(
        payload
            .pointer("/rules/0/template_version")
            .and_then(Value::as_u64),
        Some(2)
    );
}
