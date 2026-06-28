use super::*;
use crate::app::state::{automation_webhook_signature_header, AutomationWebhookTriggerCreateInput};
use tandem_types::{DataClass, TenantContext, ToolRiskTier};

fn tenant(org: &str, workspace: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, workspace, None, "actor-a")
}

fn minimal_automation(id: &str, tenant_context: &TenantContext) -> crate::AutomationV2Spec {
    let mut automation = crate::AutomationV2Spec {
        automation_id: id.to_string(),
        name: "Webhook automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "webhook-test".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    automation.set_tenant_context(tenant_context);
    automation
}

fn create_input(
    automation_id: &str,
    tenant_context: TenantContext,
) -> AutomationWebhookTriggerCreateInput {
    AutomationWebhookTriggerCreateInput {
        automation_id: automation_id.to_string(),
        tenant_context,
        owner_principal: None,
        created_by: Some("actor-a".to_string()),
        owning_org_unit_id: Some("dept-a".to_string()),
        resource_scope: None,
        default_data_class: DataClass::Internal,
        default_risk_tier: Some(ToolRiskTier::InternalWrite),
        name: Some("Generic webhook".to_string()),
        provider: "generic".to_string(),
        provider_event_kind: Some("event.created".to_string()),
        enabled: true,
    }
}

async fn setup_webhook(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) -> crate::app::state::AutomationWebhookCreateResult {
    state
        .put_automation_v2(minimal_automation(automation_id, tenant_context))
        .await
        .expect("put automation");
    state
        .create_automation_webhook_trigger(create_input(automation_id, tenant_context.clone()))
        .await
        .expect("create trigger")
}

async fn set_automation_status(
    state: &AppState,
    automation_id: &str,
    status: crate::AutomationV2Status,
) {
    let mut automation = state
        .get_automation_v2(automation_id)
        .await
        .expect("automation");
    automation.status = status;
    state
        .put_automation_v2(automation)
        .await
        .expect("update automation");
}

fn webhook_request(
    public_path_token: &str,
    secret: Option<&str>,
    body: &'static [u8],
    event_id: &str,
    now_ms: u64,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/webhooks/automations/{public_path_token}"))
        .header("content-type", "application/json")
        .header("x-tandem-webhook-event-id", event_id);
    if let Some(secret) = secret {
        builder = builder.header(
            "x-tandem-webhook-signature",
            automation_webhook_signature_header(secret, now_ms, body),
        );
    }
    builder.body(Body::from(body)).expect("request")
}

#[tokio::test]
async fn public_automation_webhook_accepts_signed_request_without_transport_auth() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-a", &tenant_context).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let body = br#"{"customer":"acme","token":"secret-value"}"#;
    let now = crate::now_ms();

    let resp = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-1",
            now,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let payload: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("accepted")
    );

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    let delivery = &deliveries[0];
    let run_id = delivery.queued_run_id.as_deref().expect("queued run id");
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("queued run");
    assert_eq!(run.trigger_type, "webhook");
    assert_eq!(run.automation_id, "automation-webhook-a");
    assert_eq!(run.tenant_context.org_id, tenant_context.org_id);
    assert_eq!(run.tenant_context.workspace_id, tenant_context.workspace_id);
    let metadata = run
        .automation_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .and_then(|metadata| metadata.get("automation_webhook"))
        .expect("webhook run metadata");
    assert_eq!(
        metadata.get("delivery_id").and_then(Value::as_str),
        Some(delivery.delivery_id.as_str())
    );
    assert_eq!(
        metadata.get("trigger_id").and_then(Value::as_str),
        Some(created.trigger.trigger_id.as_str())
    );
    assert_eq!(
        metadata.get("provider_event_id").and_then(Value::as_str),
        Some("evt-1")
    );
    assert_eq!(
        metadata.pointer("/preview/token").and_then(Value::as_str),
        Some("[redacted]")
    );

    let event = next_event_of_type(&mut rx, "automation.v2.run.created").await;
    assert_eq!(
        event.properties.get("triggerType").and_then(Value::as_str),
        Some("webhook")
    );
    assert_eq!(
        event.properties.get("runID").and_then(Value::as_str),
        Some(run.run_id.as_str())
    );
}

#[tokio::test]
async fn public_automation_webhook_rejects_unsigned_request_without_creating_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-unsigned", &tenant_context).await;
    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;

    let resp = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            None,
            body,
            "evt-unsigned",
            crate::now_ms(),
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(state.automation_v2_runs.read().await.is_empty());
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].rejection_reason_code.as_deref(),
        Some("missing_signature")
    );
}

#[tokio::test]
async fn public_automation_webhook_duplicate_body_digest_does_not_queue_second_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-duplicate", &tenant_context).await;
    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;
    let now = crate::now_ms();

    let first = app
        .clone()
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-duplicate",
            now,
        ))
        .await
        .expect("first response");
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let second = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-duplicate-renamed",
            now + 1,
        ))
        .await
        .expect("second response");
    assert_eq!(second.status(), StatusCode::ACCEPTED);

    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 2);
    assert!(deliveries.iter().any(|delivery| matches!(
        delivery.status,
        crate::AutomationWebhookDeliveryStatus::Accepted
    )));
    assert!(deliveries.iter().any(|delivery| matches!(
        delivery.status,
        crate::AutomationWebhookDeliveryStatus::Duplicate
    )));
}

#[tokio::test]
async fn public_automation_webhook_disabled_trigger_does_not_queue_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-disabled", &tenant_context).await;
    state
        .disable_automation_webhook_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
            Some("actor-a".to_string()),
        )
        .await
        .expect("disable trigger");
    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;

    let resp = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-disabled",
            crate::now_ms(),
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::GONE);
    assert!(state.automation_v2_runs.read().await.is_empty());

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].status,
        crate::AutomationWebhookDeliveryStatus::Disabled
    );
    assert_eq!(
        deliveries[0].rejection_reason_code.as_deref(),
        Some("trigger_disabled")
    );
}

#[tokio::test]
async fn public_automation_webhook_inactive_automation_does_not_queue_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-inactive", &tenant_context).await;
    set_automation_status(
        &state,
        "automation-webhook-inactive",
        crate::AutomationV2Status::Draft,
    )
    .await;
    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;

    let resp = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-inactive",
            crate::now_ms(),
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert!(state.automation_v2_runs.read().await.is_empty());

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].rejection_reason_code.as_deref(),
        Some("automation_inactive")
    );
}

#[tokio::test]
async fn public_automation_webhook_tenant_mismatch_does_not_queue_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    let created = setup_webhook(&state, "automation-webhook-tenant-mismatch", &tenant_a).await;
    state
        .put_automation_v2(minimal_automation(
            "automation-webhook-tenant-mismatch",
            &tenant_b,
        ))
        .await
        .expect("replace automation with tenant b");
    let app = app_router(state.clone());
    let body = br#"{"tenant_id":"org-b","automation_id":"automation-webhook-tenant-mismatch"}"#;

    let resp = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-tenant-mismatch",
            crate::now_ms(),
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert!(state.automation_v2_runs.read().await.is_empty());

    let tenant_a_deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
        .await;
    assert_eq!(tenant_a_deliveries.len(), 1);
    assert_eq!(
        tenant_a_deliveries[0].rejection_reason_code.as_deref(),
        Some("automation_tenant_mismatch")
    );
    assert!(state
        .list_automation_webhook_deliveries_for_trigger(&tenant_b, &created.trigger.trigger_id)
        .await
        .is_empty());
}
