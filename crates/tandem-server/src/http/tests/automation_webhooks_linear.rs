use super::*;
use crate::app::state::{
    linear_automation_webhook_signature_header, AutomationWebhookTriggerCreateInput,
};
use crate::automation_v2::types::{
    AutomationWebhookDeliveryStatus, AutomationWebhookSignatureScheme,
};
use tandem_types::{DataClass, TenantContext};

fn tenant(org: &str, workspace: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, workspace, None, "actor-a")
}

fn minimal_automation(id: &str, tenant_context: &TenantContext) -> crate::AutomationV2Spec {
    let mut automation = crate::AutomationV2Spec {
        automation_id: id.to_string(),
        name: "Linear webhook automation".to_string(),
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

fn trigger_create_input(
    automation_id: &str,
    tenant_context: TenantContext,
    provider: &str,
    name: &str,
    provider_event_kind: &str,
) -> AutomationWebhookTriggerCreateInput {
    AutomationWebhookTriggerCreateInput {
        automation_id: automation_id.to_string(),
        tenant_context,
        owner_principal: None,
        created_by: Some("actor-a".to_string()),
        owning_org_unit_id: None,
        resource_scope: None,
        default_data_class: DataClass::Internal,
        default_risk_tier: None,
        name: Some(name.to_string()),
        provider: provider.to_string(),
        provider_event_kind: Some(provider_event_kind.to_string()),
        signature_scheme: None,
        enabled: true,
    }
}

async fn setup_webhook(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
    provider: &str,
    name: &str,
    provider_event_kind: &str,
) -> crate::app::state::AutomationWebhookCreateResult {
    state
        .put_automation_v2(minimal_automation(automation_id, tenant_context))
        .await
        .expect("put automation");
    state
        .create_automation_webhook_trigger(trigger_create_input(
            automation_id,
            tenant_context.clone(),
            provider,
            name,
            provider_event_kind,
        ))
        .await
        .expect("create trigger")
}

async fn setup_linear_webhook(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) -> crate::app::state::AutomationWebhookCreateResult {
    let created = setup_webhook(
        state,
        automation_id,
        tenant_context,
        "linear.app",
        "Linear webhook",
        "issue",
    )
    .await;
    assert_eq!(created.trigger.provider, "linear");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::LinearHmacSha256
    );
    assert!(!created
        .trigger
        .linear_verification
        .as_ref()
        .expect("linear verification state")
        .secret_configured());
    created
}

fn linear_signed_request(
    public_path_token: &str,
    secret: &str,
    body: &[u8],
    delivery_id: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/webhooks/automations/{public_path_token}"))
        .header("content-type", "application/json")
        .header(
            "linear-signature",
            linear_automation_webhook_signature_header(secret, body),
        )
        .header("linear-delivery", delivery_id)
        .body(Body::from(body.to_vec()))
        .expect("request")
}

#[tokio::test]
async fn linear_disabled_trigger_rejects_without_queueing() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_linear_webhook(&state, "automation-linear-disabled", &tenant_context).await;
    let secret = "lin_wh_disabled_secret";
    state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-disabled",
            &created.trigger.trigger_id,
            secret,
            None,
        )
        .await
        .expect("import linear secret");
    state
        .disable_automation_webhook_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
            Some("actor-a".to_string()),
        )
        .await
        .expect("disable trigger");
    let app = app_router(state.clone());
    let body = br#"{"action":"create","type":"Issue"}"#;

    let resp = app
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            secret,
            body,
            "lin-delivery-disabled",
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
    let disabled = &deliveries[0];
    assert_eq!(disabled.status, AutomationWebhookDeliveryStatus::Disabled);
    assert_eq!(
        disabled.rejection_reason_code.as_deref(),
        Some("trigger_disabled")
    );
    assert_eq!(disabled.verification_provider.as_deref(), Some("linear"));
    assert_eq!(
        disabled.verification_scheme,
        Some(AutomationWebhookSignatureScheme::LinearHmacSha256)
    );
    assert!(disabled.queued_run_id.is_none());
    assert!(state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id
        )
        .await
        .is_empty());
}

#[tokio::test]
async fn linear_secret_reimport_bumps_version_and_old_secret_stops_verifying() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_linear_webhook(&state, "automation-linear-c", &tenant_context).await;
    let app = app_router(state.clone());

    let first_secret = "lin_wh_first_secret";
    let second_secret = "lin_wh_second_secret";
    state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-c",
            &created.trigger.trigger_id,
            first_secret,
            None,
        )
        .await
        .expect("first import");
    let reimported = state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-c",
            &created.trigger.trigger_id,
            second_secret,
            None,
        )
        .await
        .expect("second import");
    assert_eq!(reimported.secret.secret_version, 3);

    let body = br#"{"action":"create","type":"Issue"}"#;
    let resp = app
        .clone()
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            first_secret,
            body,
            "lin-delivery-old",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp = app
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            second_secret,
            body,
            "lin-delivery-new",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    assert!(state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-c",
            &created.trigger.trigger_id,
            "   ",
            None,
        )
        .await
        .is_err());
    assert!(state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-c",
            &created.trigger.trigger_id,
            &"x".repeat(2048),
            None,
        )
        .await
        .is_err());

    let generic = setup_webhook(
        &state,
        "automation-linear-c-generic",
        &tenant_context,
        "generic",
        "Generic webhook",
        "event.created",
    )
    .await;
    assert!(state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-c-generic",
            &generic.trigger.trigger_id,
            "some_secret",
            None,
        )
        .await
        .is_err());
}
