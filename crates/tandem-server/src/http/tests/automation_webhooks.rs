// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

include!("automation_webhooks_preflight.rs");

use super::*;
use crate::app::state::{
    automation_webhook_signature_header,
    automation_webhook_signature_header_with_signed_allow_self_feedback,
    github_automation_webhook_signature_header, linear_automation_webhook_signature_header,
    notion_automation_webhook_signature_header, AutomationWebhookTriggerCreateInput,
};
use crate::automation_v2::types::{
    AutomationWebhookDedupeResult, AutomationWebhookDeliveryStatus,
    AutomationWebhookFeedbackLoopOutcome, AutomationWebhookNotionVerificationStatus,
    AutomationWebhookSignatureScheme,
};
use crate::stateful_runtime::{
    list_stateful_waits, phase_state_from_status, stateful_webhook_wait_metadata,
    upsert_stateful_wait, write_stateful_run_snapshot, StatefulRunSnapshotRecord,
    StatefulRuntimeScope, StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWaitQuery,
    StatefulWaitRecord, StatefulWaitStatus, StatefulWebhookWaitMatch, StatefulWorkflowRunKind,
    StatefulWorkflowRunStatus,
};
use crate::ExternalActionRecord;
use tandem_types::{DataClass, TenantContext};

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
        owning_org_unit_id: None,
        resource_scope: None,
        default_data_class: DataClass::Internal,
        default_risk_tier: None,
        name: Some("Generic webhook".to_string()),
        provider: "generic".to_string(),
        provider_event_kind: Some("event.created".to_string()),
        signature_scheme: None,
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
    webhook_request_at(
        format!("/webhooks/automations/{public_path_token}"),
        secret,
        body,
        event_id,
        now_ms,
    )
}

fn webhook_request_at(
    uri: impl Into<String>,
    secret: Option<&str>,
    body: &'static [u8],
    event_id: &str,
    now_ms: u64,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri.into())
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

fn tenant_api_request(uri: impl Into<String>, tenant_context: &TenantContext) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri.into())
        .header("x-tandem-org-id", tenant_context.org_id.as_str())
        .header(
            "x-tandem-workspace-id",
            tenant_context.workspace_id.as_str(),
        )
        .header("x-tandem-actor-id", "actor-a")
        .header("authorization", "Bearer tk_test")
        .body(Body::empty())
        .expect("request")
}

async fn response_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json")
}

async fn drain_webhook_inbox(state: &AppState) {
    let report = state.process_automation_webhook_inbox_once(100).await;
    assert_eq!(report.failed, 0);
}

#[tokio::test]
async fn public_webhook_body_limits_apply_before_collection_and_vary_by_provider() {
    let state = test_state().await;
    let tenant_context = tenant("org-limits", "workspace-limits");
    let generic = setup_webhook(&state, "automation-webhook-generic-limit", &tenant_context).await;
    state
        .put_automation_v2(minimal_automation(
            "automation-webhook-github-limit",
            &tenant_context,
        ))
        .await
        .expect("put GitHub automation");
    let mut github_input = create_input("automation-webhook-github-limit", tenant_context.clone());
    github_input.provider = "github".to_string();
    github_input.signature_scheme = Some(AutomationWebhookSignatureScheme::GithubHmacSha256);
    let github = state
        .create_automation_webhook_trigger(github_input)
        .await
        .expect("create GitHub trigger");
    let app = app_router(state.clone());

    let unknown_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(33 * 1024),
    }))
    .expect("unknown body");
    let unknown_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/automations/unknown-capability-limit")
                .header("content-type", "application/json")
                .body(Body::from(unknown_body))
                .expect("unknown request"),
        )
        .await
        .expect("unknown response");
    assert_eq!(unknown_response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let generic_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(257 * 1024),
    }))
    .expect("generic body");
    let generic_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    generic.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .body(Body::from(generic_body))
                .expect("generic request"),
        )
        .await
        .expect("generic response");
    assert_eq!(generic_response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let github_body = serde_json::to_vec(&json!({
        "blob": "x".repeat(300 * 1024),
    }))
    .expect("GitHub body");
    let github_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    github.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-github-delivery", "github-large-delivery")
                .header(
                    "x-hub-signature-256",
                    github_automation_webhook_signature_header(&github.secret, &github_body),
                )
                .body(Body::from(github_body))
                .expect("GitHub request"),
        )
        .await
        .expect("GitHub response");
    assert_eq!(github_response.status(), StatusCode::ACCEPTED);

    let forged_length = app_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/automations/another-unknown-capability")
                .header("content-type", "application/json")
                .header("content-length", "999999")
                .body(Body::from("{}"))
                .expect("forged content-length request"),
        )
        .await
        .expect("forged content-length response");
    assert_eq!(forged_length.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

async fn public_automation_webhook_accepts_signed_request_without_transport_auth() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-a", &tenant_context).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let body = br#"{"customer":"acme","token":"secret-value"}"#;
    let now = crate::now_ms();

    let mut request = webhook_request(
        &created.trigger.public_path_token,
        Some(&created.secret),
        body,
        "evt-1",
        now,
    );
    request.headers_mut().insert(
        "x-api-key",
        axum::http::HeaderValue::from_static("super-secret-api-key"),
    );
    let resp = app.oneshot(request).await.expect("response");
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
    assert!(deliveries.is_empty());
    let raw_events = state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(raw_events.len(), 1);
    let raw_event = &raw_events[0];
    assert_eq!(
        raw_event.status,
        crate::AutomationWebhookDeliveryStatus::Received
    );
    assert!(raw_event.delivery_id.is_none());
    assert_eq!(raw_event.provider_event_id.as_deref(), Some("evt-1"));
    assert_eq!(
        raw_event.verification_scheme,
        Some(AutomationWebhookSignatureScheme::HmacSha256V1)
    );
    assert_eq!(raw_event.verification_provider.as_deref(), Some("generic"));
    assert_eq!(
        raw_event.verification_reason_code.as_deref(),
        Some("verified")
    );
    assert!(raw_event.headers_digest.starts_with("sha256:"));
    assert_eq!(
        raw_event
            .headers_redacted
            .get("x-tandem-webhook-signature")
            .and_then(Value::as_str),
        Some("[redacted]")
    );
    assert_eq!(
        raw_event
            .headers_redacted
            .get("x-api-key")
            .and_then(Value::as_str),
        Some("[redacted]")
    );
    let persisted_payload = state
        .read_automation_webhook_raw_event_payload(&tenant_context, &raw_event.event_id)
        .await
        .expect("raw payload read")
        .expect("raw payload");
    assert_eq!(persisted_payload, body);

    let report = state.process_automation_webhook_inbox_once(10).await;
    assert_eq!(report.checked, 1);
    assert_eq!(report.processed, 1);
    assert_eq!(report.failed, 0);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    let delivery = &deliveries[0];
    let raw_events = state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let raw_event = &raw_events[0];
    assert_eq!(
        raw_event.status,
        crate::AutomationWebhookDeliveryStatus::Accepted
    );
    assert_eq!(
        raw_event.delivery_id.as_deref(),
        Some(delivery.delivery_id.as_str())
    );
    assert_eq!(raw_event.body_digest, delivery.body_digest);
    assert_eq!(
        delivery.verification_scheme,
        Some(AutomationWebhookSignatureScheme::HmacSha256V1)
    );
    assert_eq!(delivery.verification_provider.as_deref(), Some("generic"));
    assert_eq!(
        delivery.verification_reason_code.as_deref(),
        Some("verified")
    );
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
    assert!(state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id
        )
        .await
        .is_empty());
    assert_eq!(
        deliveries[0].verification_scheme,
        Some(AutomationWebhookSignatureScheme::HmacSha256V1)
    );
    assert_eq!(
        deliveries[0].verification_reason_code.as_deref(),
        Some("missing_signature")
    );
}

/// Isolated TA-13 security load harness. The sequential wall budget is also a
/// conservative CPU/lock-time proxy: every request performs all work inline in
/// this process, and the per-request ceiling includes the rejection-ledger lock.
#[tokio::test]
#[ignore = "run explicitly as the bounded TA-13 bad-signature load harness"]
async fn bad_signature_load_is_throttled_and_retained_within_explicit_ceilings() {
    const REQUESTS: usize = 512;
    const MAX_UNTHROTTLED_REQUESTS: usize = 30;
    const MAX_TOTAL_CPU_AND_WALL_MS: u128 = 30_000;
    const MAX_REQUEST_AND_LOCK_LATENCY_MS: u128 = 2_000;
    const MAX_RETAINED_DISK_BYTES: u64 = 64 * 1024;
    const MAX_RETAINED_MEMORY_BYTES: usize = 128 * 1024;
    const MAX_REJECTION_DISK_WRITES: usize = MAX_UNTHROTTLED_REQUESTS;
    static ATTACKER_BODY: &[u8] = br#"{"payload":"ta13-attacker-body-marker"}"#;

    assert!(
        std::env::var("TANDEM_PUBLIC_WEBHOOK_RATE_LIMIT_PER_MIN").is_err()
            && std::env::var("TANDEM_PUBLIC_WEBHOOK_NETWORK_RATE_LIMIT_PER_MIN").is_err(),
        "run the isolated harness with the default production limiter ceilings"
    );

    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-ta13", "workspace-ta13");
    let created = setup_webhook(&state, "automation-webhook-ta13-load", &tenant_context).await;
    let app = app_router(state.clone());
    let durable_deliveries_before =
        tokio::fs::read_to_string(&state.automation_webhook_deliveries_path)
            .await
            .ok();
    let started = std::time::Instant::now();
    let mut maximum_request_latency = std::time::Duration::ZERO;
    let mut rejected = 0usize;
    let mut throttled = 0usize;

    for index in 0..REQUESTS {
        let request_started = std::time::Instant::now();
        let response = app
            .clone()
            .oneshot(webhook_request(
                &created.trigger.public_path_token,
                Some("ta13-wrong-secret"),
                ATTACKER_BODY,
                &format!("ta13-event-{index}"),
                crate::now_ms(),
            ))
            .await
            .expect("load-harness response");
        maximum_request_latency = maximum_request_latency.max(request_started.elapsed());
        match response.status() {
            StatusCode::UNAUTHORIZED => rejected = rejected.saturating_add(1),
            StatusCode::TOO_MANY_REQUESTS => throttled = throttled.saturating_add(1),
            status => panic!("unexpected load-harness status {status}"),
        }
    }

    let elapsed = started.elapsed();
    assert_eq!(rejected, MAX_UNTHROTTLED_REQUESTS);
    assert_eq!(throttled, REQUESTS - MAX_UNTHROTTLED_REQUESTS);
    assert!(elapsed.as_millis() <= MAX_TOTAL_CPU_AND_WALL_MS);
    assert!(
        maximum_request_latency.as_millis() <= MAX_REQUEST_AND_LOCK_LATENCY_MS,
        "single-request/lock latency was {maximum_request_latency:?}"
    );

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let retained_memory_bytes = deliveries
        .iter()
        .map(|delivery| {
            serde_json::to_vec(delivery)
                .expect("serialize retained delivery")
                .len()
        })
        .sum::<usize>();
    assert_eq!(deliveries.len(), MAX_UNTHROTTLED_REQUESTS);
    assert!(retained_memory_bytes <= MAX_RETAINED_MEMORY_BYTES);

    let rejection_ledger = state
        .automation_webhook_deliveries_path
        .parent()
        .expect("webhook state parent")
        .join("automation_webhook_rejections.jsonl");
    let telemetry = tokio::fs::read_to_string(&rejection_ledger)
        .await
        .expect("bounded rejection telemetry");
    let disk_writes = telemetry.lines().count();
    let retained_disk_bytes = tokio::fs::metadata(&rejection_ledger)
        .await
        .expect("rejection telemetry metadata")
        .len();
    assert!(disk_writes <= MAX_REJECTION_DISK_WRITES);
    assert!(retained_disk_bytes <= MAX_RETAINED_DISK_BYTES);
    assert!(!telemetry.contains("ta13-attacker-body-marker"));
    let durable_deliveries_after =
        tokio::fs::read_to_string(&state.automation_webhook_deliveries_path)
            .await
            .ok();
    assert_eq!(
        durable_deliveries_after, durable_deliveries_before,
        "pre-auth rejections must not enter the full-map durable store"
    );
    eprintln!(
        "TA-13 bounded load: requests={REQUESTS} rejected={rejected} throttled={throttled} total_ms={} max_request_lock_ms={} retained_memory_bytes={retained_memory_bytes} disk_writes={disk_writes} retained_disk_bytes={retained_disk_bytes}",
        elapsed.as_millis(),
        maximum_request_latency.as_millis(),
    );
}

#[tokio::test]
async fn public_automation_webhook_accepts_hosted_prefixed_path_without_transport_auth() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-prefixed", &tenant_context).await;
    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;
    let now = crate::now_ms();

    let resp = app
        .oneshot(webhook_request_at(
            format!(
                "/api/engine/webhooks/automations/{}",
                created.trigger.public_path_token
            ),
            Some(&created.secret),
            body,
            "evt-prefixed",
            now,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert!(deliveries[0].queued_run_id.is_some());
}

#[tokio::test]
async fn public_automation_webhook_prefers_provider_specific_event_id_header() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    state
        .put_automation_v2(minimal_automation(
            "automation-webhook-github-event",
            &tenant_context,
        ))
        .await
        .expect("put automation");
    let mut input = create_input("automation-webhook-github-event", tenant_context.clone());
    input.provider = " GitHub.com ".to_string();
    input.provider_event_kind = Some(" Issues.Opened ".to_string());
    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("create github trigger");
    assert_eq!(created.trigger.provider, "github");

    let app = app_router(state.clone());
    let body = br#"{"ok":true}"#;
    let now = crate::now_ms();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-generic")
                .header("x-github-delivery", "github-delivery-1")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(&created.secret, now, body),
                )
                .body(Body::from(body.as_slice()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].provider_event_id.as_deref(),
        Some("github-delivery-1")
    );
}

#[tokio::test]
async fn public_automation_webhook_uses_trigger_signature_scheme_registry() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    state
        .put_automation_v2(minimal_automation(
            "automation-webhook-github-signature",
            &tenant_context,
        ))
        .await
        .expect("put automation");
    let mut input = create_input(
        "automation-webhook-github-signature",
        tenant_context.clone(),
    );
    input.provider = "github".to_string();
    input.signature_scheme = Some(AutomationWebhookSignatureScheme::GithubHmacSha256);
    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("create github trigger");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::GithubHmacSha256
    );
    state.set_api_token(Some("tk_test".to_string())).await;

    let app = app_router(state.clone());
    let body = br#"{"action":"opened"}"#;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-github-delivery", "github-delivery-2")
                .header(
                    "x-hub-signature-256",
                    github_automation_webhook_signature_header(&created.secret, body),
                )
                .body(Body::from(body.as_slice()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].provider_event_id.as_deref(),
        Some("github-delivery-2")
    );
    assert_eq!(
        deliveries[0].verification_scheme,
        Some(AutomationWebhookSignatureScheme::GithubHmacSha256)
    );
    assert_eq!(
        deliveries[0].verification_provider.as_deref(),
        Some("github")
    );
    assert_eq!(
        deliveries[0].verification_reason_code.as_deref(),
        Some("verified")
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
    drain_webhook_inbox(&state).await;

    let original_run_id = {
        let mut runs = state.automation_v2_runs.write().await;
        assert_eq!(runs.len(), 1);
        let (run_id, run) = runs.iter_mut().next().expect("original run");
        run.status = crate::AutomationRunStatus::Cancelled;
        run.updated_at_ms = now + 1;
        run_id.clone()
    };

    let second = app
        .oneshot(webhook_request(
            &created.trigger.public_path_token,
            Some(&created.secret),
            body,
            "evt-duplicate-renamed",
            now + 2,
        ))
        .await
        .expect("second response");
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;

    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 2);
    let accepted = deliveries
        .iter()
        .find(|delivery| {
            matches!(
                delivery.status,
                crate::AutomationWebhookDeliveryStatus::Accepted
            )
        })
        .expect("accepted delivery");
    let duplicate = deliveries
        .iter()
        .find(|delivery| {
            matches!(
                delivery.status,
                crate::AutomationWebhookDeliveryStatus::Duplicate
            )
        })
        .expect("duplicate delivery");
    assert_eq!(
        duplicate.dedupe_result,
        Some(crate::AutomationWebhookDedupeResult::Duplicate)
    );
    assert_eq!(
        duplicate.duplicate_of_delivery_id.as_deref(),
        Some(accepted.delivery_id.as_str())
    );
    assert_eq!(
        duplicate.dedupe_reason_code.as_deref(),
        Some("duplicate_body_digest")
    );
    assert_eq!(
        accepted.queued_run_id.as_deref(),
        Some(original_run_id.as_str())
    );
    assert_eq!(
        duplicate.duplicate_of_run_id.as_deref(),
        Some(original_run_id.as_str())
    );
    assert_eq!(
        accepted
            .correlation
            .as_ref()
            .map(|correlation| &correlation.outcome),
        Some(&crate::AutomationWebhookCorrelationOutcome::NewRun)
    );
    assert_eq!(
        duplicate
            .correlation
            .as_ref()
            .map(|correlation| &correlation.outcome),
        Some(&crate::AutomationWebhookCorrelationOutcome::Duplicate)
    );
    let raw_events = state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(raw_events.len(), 2);
    assert!(raw_events.iter().any(|event| matches!(
        event.status,
        crate::AutomationWebhookDeliveryStatus::Accepted
    )));
    assert!(raw_events.iter().any(|event| matches!(
        event.status,
        crate::AutomationWebhookDeliveryStatus::Duplicate
    )));

    let api = app_router(state.clone());
    let events_resp = api
        .clone()
        .oneshot(tenant_api_request(
            format!(
                "/automations/v2/webhook-events?triggerID={}",
                created.trigger.trigger_id
            ),
            &tenant_context,
        ))
        .await
        .expect("list events");
    assert_eq!(events_resp.status(), StatusCode::OK);
    let events_payload = response_json(events_resp).await;
    assert_eq!(events_payload.get("count").and_then(Value::as_u64), Some(2));
    assert!(events_payload
        .get("events")
        .and_then(Value::as_array)
        .expect("events")
        .iter()
        .any(
            |event| event.get("status").and_then(Value::as_str) == Some("duplicate")
                && event
                    .pointer("/correlation/outcome")
                    .and_then(Value::as_str)
                    == Some("duplicate")
        ));

    let accepted_event = raw_events
        .iter()
        .find(|event| matches!(event.status, AutomationWebhookDeliveryStatus::Accepted))
        .expect("accepted event");
    let detail_resp = api
        .clone()
        .oneshot(tenant_api_request(
            format!(
                "/automations/v2/webhook-events/{}?includePayload=true",
                accepted_event.event_id
            ),
            &tenant_context,
        ))
        .await
        .expect("event detail");
    assert_eq!(detail_resp.status(), StatusCode::OK);
    let detail_payload = response_json(detail_resp).await;
    assert_eq!(
        detail_payload
            .pointer("/event/payload/ok")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        detail_payload
            .pointer("/event/correlation/outcome")
            .and_then(Value::as_str),
        Some("new_run")
    );

    let run_events_resp = api
        .clone()
        .oneshot(tenant_api_request(
            format!(
                "/automations/v2/runs/{}/webhook-events",
                accepted.queued_run_id.as_deref().expect("run id")
            ),
            &tenant_context,
        ))
        .await
        .expect("run events");
    assert_eq!(run_events_resp.status(), StatusCode::OK);
    let run_events_payload = response_json(run_events_resp).await;
    assert_eq!(
        run_events_payload.get("count").and_then(Value::as_u64),
        Some(2)
    );
    let tenant_b = tenant("org-b", "workspace-b");
    let cross_tenant_resp = api
        .oneshot(tenant_api_request(
            format!(
                "/automations/v2/webhook-events?triggerID={}",
                created.trigger.trigger_id
            ),
            &tenant_b,
        ))
        .await
        .expect("cross tenant list");
    assert_eq!(cross_tenant_resp.status(), StatusCode::OK);
    assert_eq!(
        response_json(cross_tenant_resp)
            .await
            .get("count")
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[tokio::test]
async fn public_automation_webhook_suppresses_tandem_origin_feedback_loop() {
    let state = test_state().await;
    state.set_api_token(Some("tk_test".to_string())).await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_webhook(&state, "automation-webhook-feedback", &tenant_context).await;
    let automation = state
        .get_automation_v2("automation-webhook-feedback")
        .await
        .expect("automation");
    let source_run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("source run");
    let idempotency_key = "feedback-idempotency-key";
    state
        .record_external_action(ExternalActionRecord {
            action_id: "external-action-feedback".to_string(),
            operation: "provider.issue.update".to_string(),
            status: "posted".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some(format!("{}:node-feedback:1:0", source_run.run_id)),
            routine_run_id: None,
            context_run_id: Some(format!("automation-v2-{}", source_run.run_id)),
            capability_id: Some("provider.issue.update".to_string()),
            provider: Some("generic".to_string()),
            target: Some("ticket-123".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some(idempotency_key.to_string()),
            receipt: Some(json!({"provider_resource_id": "ticket-123"})),
            error: None,
            metadata: Some(json!({
                "automationRunID": source_run.run_id.clone(),
                "nodeID": "node-feedback",
                "tenantContext": tenant_context.clone(),
            })),
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
        })
        .await
        .expect("record external action");

    let app = app_router(state.clone());
    let mismatch_body = json!({
        "tandem_origin": {
            "idempotency_key": idempotency_key,
            "run_id": source_run.run_id.clone(),
            "node_id": "node-feedback",
            "resource_id": "ticket-999",
        },
        "ticket": "ticket-999",
    })
    .to_string()
    .into_bytes();
    let body = json!({
        "ticket": "ticket-123",
    })
    .to_string()
    .into_bytes();
    let now = crate::now_ms();
    let mismatch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header(
                    "x-tandem-webhook-event-id",
                    "evt-feedback-resource-mismatch",
                )
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(&created.secret, now, &mismatch_body),
                )
                .body(Body::from(mismatch_body))
                .expect("request"),
        )
        .await
        .expect("mismatch response");
    assert_eq!(mismatch_resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 2);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-feedback-suppressed")
                .header("x-tandem-origin-idempotency-key", idempotency_key)
                .header("x-tandem-origin-run-id", source_run.run_id.as_str())
                .header("x-tandem-origin-node-id", "node-feedback")
                .header("x-tandem-origin-resource-id", "ticket-123")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(&created.secret, now, &body),
                )
                .body(Body::from(body.clone()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 2);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 2);
    let mismatch = deliveries
        .iter()
        .find(|delivery| {
            delivery.provider_event_id.as_deref() == Some("evt-feedback-resource-mismatch")
        })
        .expect("mismatch delivery");
    assert_eq!(mismatch.status, AutomationWebhookDeliveryStatus::Accepted);
    assert!(mismatch.feedback_loop.is_none());
    let delivery = deliveries
        .iter()
        .find(|delivery| delivery.provider_event_id.as_deref() == Some("evt-feedback-suppressed"))
        .expect("suppressed delivery");
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Suppressed);
    assert_eq!(
        delivery.dedupe_result,
        Some(AutomationWebhookDedupeResult::IgnoredFeedbackLoop)
    );
    assert_eq!(
        delivery
            .feedback_loop
            .as_ref()
            .map(|decision| &decision.outcome),
        Some(&AutomationWebhookFeedbackLoopOutcome::Suppressed)
    );
    assert_eq!(
        delivery
            .correlation
            .as_ref()
            .map(|correlation| &correlation.outcome),
        Some(&crate::AutomationWebhookCorrelationOutcome::Suppressed)
    );

    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let wait_run_id = "run-suppressed-feedback-wait";
    let wait_now = now + 1;
    let phase_state = phase_state_from_status(
        wait_run_id,
        &StatefulWorkflowRunStatus::Running,
        wait_now,
        Some("phase-feedback"),
    );
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-suppressed-feedback-wait".to_string(),
            run_id: wait_run_id.to_string(),
            seq: 7,
            created_at_ms: wait_now,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context.clone()),
            status: StatefulWorkflowRunStatus::Running,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-feedback".to_string()),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        },
    )
    .await
    .expect("write feedback wait snapshot");
    upsert_stateful_wait(
        &paths.waits_path,
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-suppressed-feedback".to_string(),
            run_id: wait_run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context.clone()),
            phase_id: Some("phase-feedback".to_string()),
            reason: Some("feedback duplicate should not wake".to_string()),
            created_at_ms: wait_now,
            updated_at_ms: wait_now,
            wake_at_ms: None,
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: Some(stateful_webhook_wait_metadata(
                StatefulWebhookWaitMatch {
                    trigger_id: Some(created.trigger.trigger_id.clone()),
                    provider: Some(created.trigger.provider.clone()),
                    provider_event_id: Some("evt-feedback-suppressed".to_string()),
                    ..StatefulWebhookWaitMatch::default()
                },
                None,
            )),
        },
    )
    .await
    .expect("insert suppressed feedback wait");
    let suppressed_duplicate_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-feedback-suppressed")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(&created.secret, now + 1, &body),
                )
                .body(Body::from(body.clone()))
                .expect("request"),
        )
        .await
        .expect("suppressed duplicate response");
    assert_eq!(suppressed_duplicate_resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    let waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_context,
        StatefulWaitQuery {
            run_id: Some(wait_run_id),
            wait_kind: Some(StatefulWaitKind::Webhook),
            ..StatefulWaitQuery::default()
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].status, StatefulWaitStatus::Waiting);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let suppressed_duplicate = deliveries
        .iter()
        .find(|delivery| {
            delivery.status == AutomationWebhookDeliveryStatus::Duplicate
                && delivery.provider_event_id.as_deref() == Some("evt-feedback-suppressed")
        })
        .expect("suppressed duplicate delivery");
    assert!(suppressed_duplicate.woken_wait_id.is_none());

    let body_only_allowed_body = json!({
            "tandem_origin": {
                "idempotency_key": idempotency_key,
                "run_id": source_run.run_id.clone(),
                "node_id": "node-feedback",
                "resource_id": "ticket-123",
            "allow_self_feedback": true,
        },
        "ticket": "ticket-123",
        "attempt": "body-only",
    })
    .to_string()
    .into_bytes();
    let body_only_allowed_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-feedback-body-allowed")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(
                        &created.secret,
                        now + 1,
                        &body_only_allowed_body,
                    ),
                )
                .body(Body::from(body_only_allowed_body))
                .expect("request"),
        )
        .await
        .expect("body-only allowed response");
    assert_eq!(body_only_allowed_resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 2);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let body_only_allowed = deliveries
        .iter()
        .find(|delivery| delivery.provider_event_id.as_deref() == Some("evt-feedback-body-allowed"))
        .expect("body-only allowed delivery");
    assert_eq!(
        body_only_allowed.status,
        AutomationWebhookDeliveryStatus::Suppressed
    );
    assert_eq!(
        body_only_allowed
            .feedback_loop
            .as_ref()
            .map(|decision| &decision.outcome),
        Some(&AutomationWebhookFeedbackLoopOutcome::Suppressed)
    );

    let unsigned_header_body = json!({
            "tandem_origin": {
                "idempotency_key": idempotency_key,
                "run_id": source_run.run_id.clone(),
                "node_id": "node-feedback",
                "resource_id": "ticket-123",
        },
        "ticket": "ticket-123",
        "attempt": "unsigned-header",
    })
    .to_string()
    .into_bytes();
    let unsigned_header_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-feedback-unsigned-header")
                .header("x-tandem-allow-self-feedback", "true")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header(
                        &created.secret,
                        now + 2,
                        &unsigned_header_body,
                    ),
                )
                .body(Body::from(unsigned_header_body))
                .expect("request"),
        )
        .await
        .expect("unsigned header response");
    assert_eq!(unsigned_header_resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.automation_v2_runs.read().await.len(), 2);

    let trusted_allowed_body = json!({
            "tandem_origin": {
                "idempotency_key": idempotency_key,
                "run_id": source_run.run_id.clone(),
                "node_id": "node-feedback",
                "resource_id": "ticket-123",
        },
        "ticket": "ticket-123",
        "attempt": "trusted-header",
    })
    .to_string()
    .into_bytes();
    let trusted_allowed_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .header("x-tandem-webhook-event-id", "evt-feedback-header-allowed")
                .header("x-tandem-allow-self-feedback", "true")
                .header(
                    "x-tandem-webhook-signature",
                    automation_webhook_signature_header_with_signed_allow_self_feedback(
                        &created.secret,
                        now + 3,
                        &trusted_allowed_body,
                        "true",
                    ),
                )
                .body(Body::from(trusted_allowed_body))
                .expect("request"),
        )
        .await
        .expect("trusted allowed response");
    assert_eq!(trusted_allowed_resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 3);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let trusted_allowed = deliveries
        .iter()
        .find(|delivery| {
            delivery.provider_event_id.as_deref() == Some("evt-feedback-header-allowed")
        })
        .expect("trusted allowed delivery");
    assert_eq!(
        trusted_allowed.status,
        AutomationWebhookDeliveryStatus::Accepted
    );
    assert_eq!(
        trusted_allowed
            .feedback_loop
            .as_ref()
            .map(|decision| &decision.outcome),
        Some(&AutomationWebhookFeedbackLoopOutcome::Allowed)
    );
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
    assert!(state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id
        )
        .await
        .is_empty());
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
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
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
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
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
    assert!(state
        .list_automation_webhook_raw_events_for_trigger(&tenant_b, &created.trigger.trigger_id)
        .await
        .is_empty());
}

// ---------------------------------------------------------------------------
// Notion provider webhook support (TAN-562)
// ---------------------------------------------------------------------------

fn notion_create_input(
    automation_id: &str,
    tenant_context: TenantContext,
) -> AutomationWebhookTriggerCreateInput {
    AutomationWebhookTriggerCreateInput {
        provider: "notion.so".to_string(),
        name: Some("Notion webhook".to_string()),
        provider_event_kind: Some("page.updated".to_string()),
        ..create_input(automation_id, tenant_context)
    }
}

async fn setup_notion_webhook(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) -> crate::app::state::AutomationWebhookCreateResult {
    state
        .put_automation_v2(minimal_automation(automation_id, tenant_context))
        .await
        .expect("put automation");
    let created = state
        .create_automation_webhook_trigger(notion_create_input(
            automation_id,
            tenant_context.clone(),
        ))
        .await
        .expect("create notion trigger");
    // Provider normalizes to `notion` and the scheme is forced accordingly.
    assert_eq!(created.trigger.provider, "notion");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::NotionHmacSha256
    );
    created
}

fn notion_verification_request(
    public_path_token: &str,
    setup_nonce: &str,
    token: &str,
) -> Request<Body> {
    let body = json!({ "verification_token": token }).to_string();
    Request::builder()
        .method("POST")
        .uri(format!(
            "/webhooks/automations/{public_path_token}/{setup_nonce}"
        ))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("request")
}

fn notion_signed_request(
    public_path_token: &str,
    token: &str,
    body: &'static [u8],
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/webhooks/automations/{public_path_token}"))
        .header("content-type", "application/json")
        .header(
            "x-notion-signature",
            notion_automation_webhook_signature_header(token, body),
        )
        .body(Body::from(body))
        .expect("request")
}

#[tokio::test]
async fn notion_verification_token_is_captured_without_queueing_a_run() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-a", &tenant_context).await;
    let app = app_router(state.clone());

    let resp = app
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            created.notion_setup_nonce.as_deref().expect("setup nonce"),
            "notion_tok_abc123",
        ))
        .await
        .expect("response");
    // Verification handshake is accepted but never queues a workflow run.
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(state.automation_v2_runs.read().await.is_empty());

    // Status advanced to token_received and a sanitized status delivery recorded.
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("trigger");
    let verification = trigger.notion_verification.expect("notion verification");
    assert_eq!(
        verification.status,
        AutomationWebhookNotionVerificationStatus::TokenReceived
    );
    assert!(verification.token_available_for_reveal());
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(
        deliveries[0].rejection_reason_code.as_deref(),
        Some("notion_verification_token_received")
    );
    // The sanitized preview never contains the raw token.
    assert!(!deliveries[0]
        .sanitized_preview
        .to_string()
        .contains("notion_tok_abc123"));

    // One-time reveal returns the token, then never again.
    let revealed = state
        .reveal_automation_webhook_notion_verification_token(
            &tenant_context,
            "automation-notion-a",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal");
    assert_eq!(revealed.as_deref(), Some("notion_tok_abc123"));
    let second = state
        .reveal_automation_webhook_notion_verification_token(
            &tenant_context,
            "automation-notion-a",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal");
    assert_eq!(second, None, "token is revealed at most once");

    // A different tenant cannot reveal the token.
    let other_tenant = tenant("org-b", "workspace-b");
    let cross = state
        .reveal_automation_webhook_notion_verification_token(
            &other_tenant,
            "automation-notion-a",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal");
    assert_eq!(cross, None);
}

#[tokio::test]
async fn notion_setup_nonce_is_required_correct_and_unexpired() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-nonce", &tenant_context).await;
    let app = app_router(state.clone());
    let body = json!({ "verification_token": "should_not_capture" }).to_string();

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .expect("missing nonce request"),
        )
        .await
        .expect("missing nonce response");
    assert_eq!(missing.status(), StatusCode::OK);

    let wrong = app
        .clone()
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            "wrong-setup-nonce",
            "should_not_capture",
        ))
        .await
        .expect("wrong nonce response");
    assert_eq!(wrong.status(), StatusCode::OK);
    let awaiting = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("trigger after rejected setup attempts");
    assert_eq!(
        awaiting.notion_verification.expect("verification").status,
        AutomationWebhookNotionVerificationStatus::AwaitingToken
    );

    let expired =
        setup_notion_webhook(&state, "automation-notion-expired-nonce", &tenant_context).await;
    {
        let mut triggers = state.automation_webhook_triggers.write().await;
        triggers
            .get_mut(&expired.trigger.trigger_id)
            .and_then(|trigger| trigger.notion_verification.as_mut())
            .expect("expired verification")
            .setup_challenge_expires_at_ms = Some(crate::now_ms().saturating_sub(1));
    }
    let expired_response = app
        .oneshot(notion_verification_request(
            &expired.trigger.public_path_token,
            expired.notion_setup_nonce.as_deref().expect("setup nonce"),
            "expired_nonce_token",
        ))
        .await
        .expect("expired nonce response");
    assert_eq!(expired_response.status(), StatusCode::OK);
    let still_awaiting = state
        .get_automation_webhook_trigger(&tenant_context, &expired.trigger.trigger_id)
        .await
        .expect("expired trigger");
    assert_eq!(
        still_awaiting
            .notion_verification
            .expect("verification")
            .status,
        AutomationWebhookNotionVerificationStatus::AwaitingToken
    );
}

#[tokio::test]
async fn notion_setup_nonce_is_consumed_once_under_concurrency() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-race", &tenant_context).await;
    let app = app_router(state.clone());
    let nonce = created.notion_setup_nonce.as_deref().expect("setup nonce");
    let (left, right) = tokio::join!(
        app.clone().oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            nonce,
            "concurrent_token_left",
        )),
        app.oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            nonce,
            "concurrent_token_right",
        )),
    );
    assert_eq!(left.expect("left response").status(), StatusCode::OK);
    assert_eq!(right.expect("right response").status(), StatusCode::OK);

    let revealed = state
        .reveal_automation_webhook_notion_verification_token(
            &tenant_context,
            "automation-notion-race",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal race winner")
        .expect("one token captured");
    assert!(matches!(
        revealed.as_str(),
        "concurrent_token_left" | "concurrent_token_right"
    ));
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("race trigger");
    let verification = trigger.notion_verification.expect("verification");
    assert_eq!(
        verification.status,
        AutomationWebhookNotionVerificationStatus::TokenReceived
    );
    assert!(verification.setup_challenge_digest.is_none());
    assert!(verification.setup_challenge_consumed_at_ms.is_some());
}
#[tokio::test]
async fn notion_signed_event_verifies_queues_once_and_dedupes() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-b", &tenant_context).await;
    let token = "notion_tok_signed";
    let app = app_router(state.clone());

    // Capture the verification token first.
    let resp = app
        .clone()
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            created.notion_setup_nonce.as_deref().expect("setup nonce"),
            token,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    // A correctly signed Notion event verifies and queues exactly one run.
    let body = br#"{"type":"page.updated","entity":{"id":"page-123"}}"#;
    let resp = app
        .clone()
        .oneshot(notion_signed_request(
            &created.trigger.public_path_token,
            token,
            body,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    // Signed events are queued through the async webhook inbox; drain it before
    // asserting the run and delivery.
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let accepted = deliveries
        .iter()
        .find(|delivery| delivery.status == AutomationWebhookDeliveryStatus::Accepted)
        .expect("accepted delivery");
    assert_eq!(accepted.verification_provider.as_deref(), Some("notion"));
    assert_eq!(
        accepted.verification_scheme,
        Some(AutomationWebhookSignatureScheme::NotionHmacSha256)
    );
    assert!(accepted.queued_run_id.is_some());

    // Trigger flips to active once a signed event is verified.
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("trigger");
    assert_eq!(
        trigger.notion_verification.expect("verification").status,
        AutomationWebhookNotionVerificationStatus::Active
    );

    // Re-delivering the same body does not queue a second run.
    let resp = app
        .oneshot(notion_signed_request(
            &created.trigger.public_path_token,
            token,
            body,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(
        state.automation_v2_runs.read().await.len(),
        1,
        "duplicate body must not queue a second run"
    );
}

#[tokio::test]
async fn notion_event_with_wrong_token_signature_is_rejected() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-c", &tenant_context).await;
    let app = app_router(state.clone());

    app.clone()
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            created.notion_setup_nonce.as_deref().expect("setup nonce"),
            "the_real_token",
        ))
        .await
        .expect("response");

    // Signed with a different token than the one stored → rejected, no run.
    let body = br#"{"type":"page.updated"}"#;
    let resp = app
        .clone()
        .oneshot(notion_signed_request(
            &created.trigger.public_path_token,
            "an_attacker_token",
            body,
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(state.automation_v2_runs.read().await.is_empty());

    // An unsigned, non-verification event is rejected for missing signature.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/webhooks/automations/{}",
                    created.trigger.public_path_token
                ))
                .header("content-type", "application/json")
                .body(Body::from(&b"{\"type\":\"page.updated\"}"[..]))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn notion_second_verification_token_does_not_overwrite_first() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_notion_webhook(&state, "automation-notion-d", &tenant_context).await;
    let app = app_router(state.clone());

    app.clone()
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            created.notion_setup_nonce.as_deref().expect("setup nonce"),
            "first_token",
        ))
        .await
        .expect("response");
    // A second unsigned token payload is ignored, not applied.
    let resp = app
        .oneshot(notion_verification_request(
            &created.trigger.public_path_token,
            created.notion_setup_nonce.as_deref().expect("setup nonce"),
            "attacker_reset_token",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    // The originally captured token still verifies events.
    let body = br#"{"type":"page.updated"}"#;
    let sig = notion_automation_webhook_signature_header("first_token", body);
    assert!(!sig.is_empty());
    let revealed = state
        .reveal_automation_webhook_notion_verification_token(
            &tenant_context,
            "automation-notion-d",
            &created.trigger.trigger_id,
        )
        .await
        .expect("reveal");
    assert_eq!(revealed.as_deref(), Some("first_token"));
}

fn linear_create_input(
    automation_id: &str,
    tenant_context: TenantContext,
) -> AutomationWebhookTriggerCreateInput {
    AutomationWebhookTriggerCreateInput {
        provider: "linear.app".to_string(),
        name: Some("Linear webhook".to_string()),
        provider_event_kind: Some("issue".to_string()),
        ..create_input(automation_id, tenant_context)
    }
}

async fn setup_linear_webhook(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) -> crate::app::state::AutomationWebhookCreateResult {
    state
        .put_automation_v2(minimal_automation(automation_id, tenant_context))
        .await
        .expect("put automation");
    let created = state
        .create_automation_webhook_trigger(linear_create_input(
            automation_id,
            tenant_context.clone(),
        ))
        .await
        .expect("create linear trigger");
    // Provider normalizes to `linear` and the scheme is forced accordingly, with
    // the provider-owned-secret lifecycle starting at awaiting_secret.
    assert_eq!(created.trigger.provider, "linear");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::LinearHmacSha256
    );
    let verification = created
        .trigger
        .linear_verification
        .as_ref()
        .expect("linear verification state");
    assert!(!verification.secret_configured());
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
async fn linear_trigger_fails_closed_until_secret_imported() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_linear_webhook(&state, "automation-linear-a", &tenant_context).await;
    let app = app_router(state.clone());

    // Even a request signed with the Tandem-generated placeholder secret is
    // rejected while the Linear signing secret has not been imported: the
    // placeholder must never verify anything.
    let body = br#"{"action":"create","type":"Issue"}"#;
    let resp = app
        .clone()
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            &created.secret,
            body,
            "lin-delivery-1",
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
        deliveries[0].status,
        AutomationWebhookDeliveryStatus::Rejected
    );
    assert_eq!(
        deliveries[0].rejection_reason_code.as_deref(),
        Some("provider_secret_not_imported")
    );
}

#[tokio::test]
async fn linear_secret_import_enables_signed_events_and_dedupes() {
    let state = test_state().await;
    let tenant_context = tenant("org-a", "workspace-a");
    let created = setup_linear_webhook(&state, "automation-linear-b", &tenant_context).await;
    let secret = "lin_wh_1234567890abcdef";
    let app = app_router(state.clone());

    // Cross-tenant import is refused.
    let other_tenant = tenant("org-b", "workspace-b");
    assert!(state
        .import_automation_webhook_linear_secret(
            &other_tenant,
            "automation-linear-b",
            &created.trigger.trigger_id,
            secret,
            None,
        )
        .await
        .is_err());

    // Authorized import stores the provider-owned secret and bumps the version.
    let imported = state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            "automation-linear-b",
            &created.trigger.trigger_id,
            secret,
            Some("actor-a".to_string()),
        )
        .await
        .expect("import secret");
    assert_eq!(imported.secret.secret_version, 2);
    let verification = imported
        .linear_verification
        .as_ref()
        .expect("linear verification state");
    assert!(verification.secret_configured());
    assert!(verification.secret_imported_at_ms.is_some());

    // A correctly signed Linear event verifies and queues exactly one run. The
    // webhookTimestamp must be current: the intake checks it against wall-clock
    // time, so a stale value is rejected (covered by the unit tests).
    let body_string = format!(
        r#"{{"action":"create","type":"Issue","data":{{"id":"issue-1"}},"webhookTimestamp":{}}}"#,
        crate::now_ms()
    );
    let body = body_string.as_bytes();
    let resp = app
        .clone()
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            secret,
            body,
            "lin-delivery-2",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let accepted = deliveries
        .iter()
        .find(|delivery| delivery.status == AutomationWebhookDeliveryStatus::Accepted)
        .expect("accepted delivery");
    assert_eq!(accepted.verification_provider.as_deref(), Some("linear"));
    assert_eq!(
        accepted.verification_scheme,
        Some(AutomationWebhookSignatureScheme::LinearHmacSha256)
    );
    assert_eq!(
        accepted.provider_event_id.as_deref(),
        Some("lin-delivery-2")
    );
    assert!(accepted.queued_run_id.is_some());

    // First verified event flips the lifecycle to active.
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &created.trigger.trigger_id)
        .await
        .expect("trigger");
    assert_eq!(
        trigger
            .linear_verification
            .expect("linear verification state")
            .status,
        crate::automation_v2::types::AutomationWebhookLinearVerificationStatus::Active
    );

    // Re-delivering the same payload does not queue a second run.
    let resp = app
        .clone()
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            secret,
            body,
            "lin-delivery-2",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    drain_webhook_inbox(&state).await;
    assert_eq!(
        state.automation_v2_runs.read().await.len(),
        1,
        "duplicate delivery must not queue a second run"
    );

    // A signature from the wrong secret rejects without queueing.
    let attacker_body = br#"{"action":"update","type":"Issue"}"#;
    let resp = app
        .oneshot(linear_signed_request(
            &created.trigger.public_path_token,
            "attacker_secret",
            attacker_body,
            "lin-delivery-3",
        ))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let report = state.process_automation_webhook_inbox_once(100).await;
    assert_eq!(report.checked, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    let rejected = deliveries
        .iter()
        .find(|delivery| delivery.provider_event_id.as_deref() == Some("lin-delivery-3"))
        .expect("rejected delivery");
    assert_eq!(rejected.status, AutomationWebhookDeliveryStatus::Rejected);
    assert_eq!(
        rejected.rejection_reason_code.as_deref(),
        Some("bad_signature")
    );
    assert_eq!(rejected.verification_provider.as_deref(), Some("linear"));
    assert_eq!(
        rejected.verification_scheme,
        Some(AutomationWebhookSignatureScheme::LinearHmacSha256)
    );
    assert!(rejected.queued_run_id.is_none());
    let raw_events = state
        .list_automation_webhook_raw_events_for_trigger(
            &tenant_context,
            &created.trigger.trigger_id,
        )
        .await;
    assert!(!raw_events
        .iter()
        .any(|event| event.provider_event_id.as_deref() == Some("lin-delivery-3")));

    let durable_deliveries = tokio::fs::read_to_string(&state.automation_webhook_deliveries_path)
        .await
        .expect("durable delivery file");
    assert!(!durable_deliveries.contains("lin-delivery-3"));
    assert!(!durable_deliveries
        .contains(std::str::from_utf8(attacker_body).expect("attacker body is UTF-8")));

    let rejection_ledger = state
        .automation_webhook_deliveries_path
        .parent()
        .expect("webhook state parent")
        .join("automation_webhook_rejections.jsonl");
    let telemetry = tokio::fs::read_to_string(rejection_ledger)
        .await
        .expect("bounded rejection telemetry");
    assert!(telemetry.contains("lin-delivery-3"));
    assert!(telemetry.contains("bad_signature"));
    assert!(
        !telemetry.contains(std::str::from_utf8(attacker_body).expect("attacker body is UTF-8"))
    );
    assert!(!telemetry.contains("attacker_secret"));
}
