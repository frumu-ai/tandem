use super::*;
use crate::app::state::automation_webhook_body_digest;
use crate::automation_v2::types::{
    AutomationWebhookDeliveryRecord, AutomationWebhookDeliveryStatus,
};

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

fn automation_v2_payload(automation_id: &str) -> Value {
    json!({
        "automation_id": automation_id,
        "name": format!("{automation_id} automation"),
        "status": "draft",
        "schedule": {
            "type": "manual",
            "timezone": "UTC",
            "misfire_policy": { "type": "skip" }
        },
        "agents": [{
            "agent_id": "agent-one",
            "display_name": "Agent One",
            "skills": [],
            "tool_policy": { "allowlist": ["read"], "denylist": [] },
            "mcp_policy": { "allowed_servers": [] }
        }],
        "flow": {
            "nodes": [{
                "node_id": "node-1",
                "agent_id": "agent-one",
                "objective": "Process the webhook",
                "depends_on": []
            }]
        },
        "execution": { "max_parallel_agents": 1 }
    })
}

fn tenant_request(
    method: &str,
    uri: impl Into<String>,
    org: &str,
    workspace: &str,
    actor: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", org)
        .header("x-tandem-workspace-id", workspace)
        .header("x-tandem-actor-id", actor);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .expect("request")
}

async fn create_automation(app: &axum::Router, automation_id: &str) {
    let req = tenant_request(
        "POST",
        "/automations/v2",
        "org-a",
        "workspace-a",
        "actor-a",
        Some(automation_v2_payload(automation_id)),
    );
    let resp = app.clone().oneshot(req).await.expect("create automation");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn webhook_management_routes_redact_secrets_and_delivery_payloads() {
    let state = test_state().await;
    let app = app_router(state.clone());
    create_automation(&app, "auto-webhook-mgmt").await;

    let create_req = tenant_request(
        "POST",
        "/automations/v2/auto-webhook-mgmt/webhook-triggers",
        "org-a",
        "workspace-a",
        "actor-a",
        Some(json!({
            "name": "GitHub issues",
            "provider": "github",
            "provider_event_kind": "issues.opened",
            "default_data_class": "customer_data",
            "default_risk_tier": "internal_write",
            "enabled": true
        })),
    );
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create webhook");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload = response_json(create_resp).await;
    let trigger_id = create_payload
        .pointer("/trigger/trigger_id")
        .and_then(Value::as_str)
        .expect("trigger id")
        .to_string();
    let first_secret = create_payload
        .get("new_secret")
        .and_then(Value::as_str)
        .expect("new secret")
        .to_string();
    let create_text = serde_json::to_string(&create_payload).expect("create text");
    assert!(!create_text.contains("secret_ref"));
    assert!(!create_text.contains("secret_digest"));
    assert!(create_text.contains("/webhooks/automations/"));

    let list_req = tenant_request(
        "GET",
        "/automations/v2/auto-webhook-mgmt/webhook-triggers",
        "org-a",
        "workspace-a",
        "actor-a",
        None,
    );
    let list_resp = app.clone().oneshot(list_req).await.expect("list webhooks");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload = response_json(list_resp).await;
    assert_eq!(list_payload.get("count").and_then(Value::as_u64), Some(1));
    let list_text = serde_json::to_string(&list_payload).expect("list text");
    assert!(!list_text.contains(&first_secret));
    assert!(!list_text.contains("secret_ref"));
    assert!(!list_text.contains("secret_digest"));

    let patch_req = tenant_request(
        "PATCH",
        format!("/automations/v2/auto-webhook-mgmt/webhook-triggers/{trigger_id}"),
        "org-a",
        "workspace-a",
        "actor-a",
        Some(json!({
            "name": "GitHub issue intake",
            "provider_event_kind": null,
            "default_data_class": "internal"
        })),
    );
    let patch_resp = app.clone().oneshot(patch_req).await.expect("patch webhook");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_payload = response_json(patch_resp).await;
    assert_eq!(
        patch_payload
            .pointer("/trigger/name")
            .and_then(Value::as_str),
        Some("GitHub issue intake")
    );
    assert!(patch_payload
        .pointer("/trigger/provider_event_kind")
        .is_some_and(Value::is_null));

    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        None,
        "actor-a",
    );
    state
        .record_automation_webhook_delivery(AutomationWebhookDeliveryRecord {
            delivery_id: "delivery-a".to_string(),
            trigger_id: trigger_id.clone(),
            automation_id: "auto-webhook-mgmt".to_string(),
            tenant_context: tenant_a,
            provider_event_id: Some("evt-a".to_string()),
            body_digest: automation_webhook_body_digest(br#"{"ok":true}"#),
            status: AutomationWebhookDeliveryStatus::Accepted,
            rejection_reason_code: None,
            queued_run_id: Some("automation-v2-run-webhook-a".to_string()),
            received_at_ms: 2_000,
            accepted_at_ms: Some(2_001),
            rejected_at_ms: None,
            sanitized_preview: json!({
                "authorization": "Bearer unsafe",
                "safe": true,
                "nested": { "api_key": "unsafe", "message": "ok" }
            }),
            audit_event_id: Some("audit-a".to_string()),
        })
        .await
        .expect("record delivery");

    let deliveries_req = tenant_request(
        "GET",
        format!("/automations/v2/auto-webhook-mgmt/webhook-triggers/{trigger_id}/deliveries"),
        "org-a",
        "workspace-a",
        "actor-a",
        None,
    );
    let deliveries_resp = app
        .clone()
        .oneshot(deliveries_req)
        .await
        .expect("list deliveries");
    assert_eq!(deliveries_resp.status(), StatusCode::OK);
    let deliveries_payload = response_json(deliveries_resp).await;
    assert_eq!(
        deliveries_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        deliveries_payload
            .pointer("/deliveries/0/sanitized_preview/authorization")
            .and_then(Value::as_str),
        Some("[redacted]")
    );
    assert_eq!(
        deliveries_payload
            .pointer("/deliveries/0/sanitized_preview/nested/api_key")
            .and_then(Value::as_str),
        Some("[redacted]")
    );
    assert_eq!(
        deliveries_payload
            .pointer("/deliveries/0/queued_run_id")
            .and_then(Value::as_str),
        Some("automation-v2-run-webhook-a")
    );

    let rotate_req = tenant_request(
        "POST",
        format!("/automations/v2/auto-webhook-mgmt/webhook-triggers/{trigger_id}/rotate-secret"),
        "org-a",
        "workspace-a",
        "actor-a",
        Some(json!({})),
    );
    let rotate_resp = app
        .clone()
        .oneshot(rotate_req)
        .await
        .expect("rotate secret");
    assert_eq!(rotate_resp.status(), StatusCode::OK);
    let rotate_payload = response_json(rotate_resp).await;
    let rotated_secret = rotate_payload
        .get("new_secret")
        .and_then(Value::as_str)
        .expect("rotated secret");
    assert_ne!(rotated_secret, first_secret);
    let rotate_text = serde_json::to_string(&rotate_payload).expect("rotate text");
    assert!(!rotate_text.contains("secret_ref"));
    assert!(!rotate_text.contains("secret_digest"));

    let disable_req = tenant_request(
        "POST",
        format!("/automations/v2/auto-webhook-mgmt/webhook-triggers/{trigger_id}/disable"),
        "org-a",
        "workspace-a",
        "actor-a",
        Some(json!({})),
    );
    let disable_resp = app
        .clone()
        .oneshot(disable_req)
        .await
        .expect("disable webhook");
    assert_eq!(disable_resp.status(), StatusCode::OK);
    let disable_payload = response_json(disable_resp).await;
    assert_eq!(
        disable_payload
            .pointer("/trigger/enabled")
            .and_then(Value::as_bool),
        Some(false)
    );

    let delete_req = tenant_request(
        "DELETE",
        format!("/automations/v2/auto-webhook-mgmt/webhook-triggers/{trigger_id}"),
        "org-a",
        "workspace-a",
        "actor-a",
        None,
    );
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete webhook");
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_payload = response_json(delete_resp).await;
    assert_eq!(
        delete_payload.get("deleted").and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn webhook_management_routes_do_not_expose_cross_tenant_triggers() {
    let state = test_state().await;
    let app = app_router(state.clone());
    create_automation(&app, "auto-webhook-tenant").await;

    let create_req = tenant_request(
        "POST",
        "/automations/v2/auto-webhook-tenant/webhook-triggers",
        "org-a",
        "workspace-a",
        "actor-a",
        Some(json!({
            "name": "Tenant A trigger",
            "provider": "generic"
        })),
    );
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create webhook");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload = response_json(create_resp).await;
    let trigger_id = create_payload
        .pointer("/trigger/trigger_id")
        .and_then(Value::as_str)
        .expect("trigger id");

    let list_b_req = tenant_request(
        "GET",
        "/automations/v2/auto-webhook-tenant/webhook-triggers",
        "org-b",
        "workspace-b",
        "actor-b",
        None,
    );
    let list_b_resp = app
        .clone()
        .oneshot(list_b_req)
        .await
        .expect("tenant b list");
    assert_eq!(list_b_resp.status(), StatusCode::NOT_FOUND);

    let get_b_req = tenant_request(
        "GET",
        format!("/automations/v2/auto-webhook-tenant/webhook-triggers/{trigger_id}"),
        "org-b",
        "workspace-b",
        "actor-b",
        None,
    );
    let get_b_resp = app.clone().oneshot(get_b_req).await.expect("tenant b get");
    assert_eq!(get_b_resp.status(), StatusCode::NOT_FOUND);

    let rotate_b_req = tenant_request(
        "POST",
        format!("/automations/v2/auto-webhook-tenant/webhook-triggers/{trigger_id}/rotate-secret"),
        "org-b",
        "workspace-b",
        "actor-b",
        Some(json!({})),
    );
    let rotate_b_resp = app
        .clone()
        .oneshot(rotate_b_req)
        .await
        .expect("tenant b rotate");
    assert_eq!(rotate_b_resp.status(), StatusCode::NOT_FOUND);
}
