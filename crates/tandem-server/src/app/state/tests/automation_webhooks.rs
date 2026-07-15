// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::app::state::{
    automation_webhook_body_digest, automation_webhook_signature_header,
    AutomationWebhookQueueResult, AutomationWebhookRawEventCreateInput,
    AutomationWebhookSignatureHeaders, AutomationWebhookTriggerCreateInput,
    AutomationWebhookTriggerUpdateInput, AutomationWebhookVerificationError,
};
use crate::automation_v2::types::{AutomationEnterpriseScope, AutomationWebhookSignatureScheme};
use tandem_types::{ResourceKind, ResourceRef, ResourceScope};

fn tenant(org: &str, workspace: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, workspace, None, "actor-a")
}

async fn insert_test_automation(
    state: &AppState,
    automation_id: &str,
    tenant_context: &TenantContext,
) {
    let mut automation = AutomationSpecBuilder::new(automation_id).build();
    automation.set_tenant_context(tenant_context);
    state
        .automations_v2
        .write()
        .await
        .insert(automation_id.to_string(), automation);
}

fn raw_payload_path(state: &AppState, event_id: &str) -> std::path::PathBuf {
    state
        .automation_webhook_deliveries_path
        .parent()
        .expect("webhook deliveries parent")
        .join("raw_payloads")
        .join(format!("{event_id}.body"))
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

#[tokio::test]
async fn webhook_unsigned_dev_mode_requires_explicit_server_flag() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-unsigned-dev-mode", &tenant_a).await;

    let mut input = create_input("automation-unsigned-dev-mode", tenant_a.clone());
    input.signature_scheme = Some(AutomationWebhookSignatureScheme::UnsignedDevMode);
    let error = match state.create_automation_webhook_trigger(input.clone()).await {
        Ok(_) => panic!("unsigned dev mode should be blocked by default"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("unsigned_dev_mode"));

    state.set_allow_unsigned_dev_webhooks(true);
    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("explicitly enabled unsigned dev mode");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::UnsignedDevMode
    );

    state.set_allow_unsigned_dev_webhooks(false);
    let error = state
        .verify_automation_webhook_request_with_headers(
            &created.trigger.public_path_token,
            AutomationWebhookSignatureHeaders::default(),
            br#"{"ok":true}"#,
            Some("evt-unsigned-disabled".to_string()),
            crate::now_ms(),
            300_000,
        )
        .await
        .expect_err("existing unsigned dev mode trigger should be disabled by server flag");
    assert_eq!(
        error,
        AutomationWebhookVerificationError::UnsignedDevModeDisabled
    );

    let normal = state
        .create_automation_webhook_trigger(create_input(
            "automation-unsigned-dev-mode",
            tenant_a.clone(),
        ))
        .await
        .expect("normal trigger");
    let error = state
        .update_automation_webhook_trigger(
            &tenant_a,
            "automation-unsigned-dev-mode",
            &normal.trigger.trigger_id,
            AutomationWebhookTriggerUpdateInput {
                name: Some("Should not stick".to_string()),
                provider_event_kind: Some(Some("event.updated".to_string())),
                signature_scheme: Some(AutomationWebhookSignatureScheme::UnsignedDevMode),
                ..AutomationWebhookTriggerUpdateInput::default()
            },
            Some("actor-a".to_string()),
        )
        .await
        .expect_err("unsigned dev mode update should be blocked by default");
    assert!(error.to_string().contains("unsigned_dev_mode"));

    let unchanged = state
        .get_automation_webhook_trigger(&tenant_a, &normal.trigger.trigger_id)
        .await
        .expect("trigger remains readable after rejected update");
    assert_eq!(unchanged.name, normal.trigger.name);
    assert_eq!(
        unchanged.provider_event_kind,
        normal.trigger.provider_event_kind
    );
    assert_eq!(unchanged.updated_at_ms, normal.trigger.updated_at_ms);
}

#[tokio::test]
async fn webhook_triggers_and_deliveries_are_tenant_scoped() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    insert_test_automation(&state, "automation-a", &tenant_a).await;

    let created = state
        .create_automation_webhook_trigger(create_input("automation-a", tenant_a.clone()))
        .await
        .expect("create webhook trigger");

    let trigger_file = std::fs::read_to_string(&state.automation_webhook_triggers_path)
        .expect("trigger state file");
    assert!(trigger_file.contains("secret_ref"));
    assert!(!trigger_file.contains(&created.secret));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(&state.automation_webhook_secret_material_path)
            .expect("secret material state file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        assert!(!state
            .automation_webhook_secret_material_path
            .with_extension("tmp")
            .exists());
    }

    assert_eq!(
        state
            .list_automation_webhook_triggers_for_automation(&tenant_a, "automation-a")
            .await
            .len(),
        1
    );
    assert!(state
        .list_automation_webhook_triggers_for_automation(&tenant_b, "automation-a")
        .await
        .is_empty());
    assert!(state
        .get_automation_webhook_trigger(&tenant_b, &created.trigger.trigger_id)
        .await
        .is_none());
    assert!(state
        .disable_automation_webhook_trigger(&tenant_b, &created.trigger.trigger_id, None)
        .await
        .is_err());
    assert!(state
        .rotate_automation_webhook_secret(&tenant_b, &created.trigger.trigger_id, None)
        .await
        .is_err());
    assert!(state
        .delete_automation_webhook_trigger(&tenant_b, &created.trigger.trigger_id)
        .await
        .is_err());

    let wrong_tenant_delivery = AutomationWebhookDeliveryRecord {
        delivery_id: "delivery-wrong-tenant".to_string(),
        trigger_id: created.trigger.trigger_id.clone(),
        automation_id: "automation-a".to_string(),
        tenant_context: tenant_b.clone(),
        enterprise_scope: None,
        provider_event_id: Some("evt-b".to_string()),
        body_digest: automation_webhook_body_digest(br#"{"ok":true}"#),
        status: AutomationWebhookDeliveryStatus::Accepted,
        rejection_reason_code: None,
        idempotency_key: None,
        idempotency_record_id: None,
        dedupe_result: None,
        dedupe_reason_code: None,
        duplicate_of_delivery_id: None,
        duplicate_of_run_id: None,
        verification_scheme: None,
        verification_provider: None,
        verification_reason_code: None,
        queued_run_id: None,
        woken_run_id: None,
        woken_wait_id: None,
        feedback_loop: None,
        correlation: None,
        received_at_ms: 1_000,
        accepted_at_ms: Some(1_000),
        rejected_at_ms: None,
        sanitized_preview: json!({"safe": true}),
        audit_event_id: None,
    };
    assert!(state
        .record_automation_webhook_delivery(wrong_tenant_delivery)
        .await
        .is_err());

    let delivery = AutomationWebhookDeliveryRecord {
        delivery_id: "delivery-a".to_string(),
        trigger_id: created.trigger.trigger_id.clone(),
        automation_id: "automation-a".to_string(),
        tenant_context: tenant_a.clone(),
        enterprise_scope: None,
        provider_event_id: Some("evt-a".to_string()),
        body_digest: automation_webhook_body_digest(br#"{"ok":true}"#),
        status: AutomationWebhookDeliveryStatus::Accepted,
        rejection_reason_code: None,
        idempotency_key: None,
        idempotency_record_id: None,
        dedupe_result: None,
        dedupe_reason_code: None,
        duplicate_of_delivery_id: None,
        duplicate_of_run_id: None,
        verification_scheme: None,
        verification_provider: None,
        verification_reason_code: None,
        queued_run_id: None,
        woken_run_id: None,
        woken_wait_id: None,
        feedback_loop: None,
        correlation: None,
        received_at_ms: 1_000,
        accepted_at_ms: Some(1_000),
        rejected_at_ms: None,
        sanitized_preview: json!({
            "authorization": "Bearer token",
            "db_password": "secret",
            "nested": { "api_key": "secret", "userPassword": "secret", "safe": true }
        }),
        audit_event_id: Some("audit-a".to_string()),
    };
    let stored = state
        .record_automation_webhook_delivery(delivery)
        .await
        .expect("record delivery");
    assert_eq!(stored.sanitized_preview["authorization"], "[redacted]");
    assert_eq!(stored.sanitized_preview["db_password"], "[redacted]");
    assert_eq!(stored.sanitized_preview["nested"]["api_key"], "[redacted]");
    assert_eq!(
        stored.sanitized_preview["nested"]["userPassword"],
        "[redacted]"
    );
    assert_eq!(
        state
            .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
            .await
            .len(),
        1
    );
    assert!(state
        .list_automation_webhook_deliveries_for_trigger(&tenant_b, &created.trigger.trigger_id)
        .await
        .is_empty());
    assert!(state
        .get_automation_webhook_delivery(&tenant_b, "delivery-a")
        .await
        .is_none());
}

#[tokio::test]
async fn webhook_trigger_create_and_update_normalize_provider_metadata() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-provider-normalize", &tenant_a).await;
    let mut input = create_input("automation-provider-normalize", tenant_a.clone());
    input.provider = " GitHub.com ".to_string();
    input.provider_event_kind = Some(" Issues.Opened ".to_string());
    input.signature_scheme = Some(AutomationWebhookSignatureScheme::GithubHmacSha256);
    input.name = None;

    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("create trigger");
    assert_eq!(created.trigger.provider, "github");
    assert_eq!(created.trigger.name, "github");
    assert_eq!(
        created.trigger.provider_event_kind.as_deref(),
        Some("issues.opened")
    );
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::GithubHmacSha256
    );

    let updated = state
        .update_automation_webhook_trigger(
            &tenant_a,
            "automation-provider-normalize",
            &created.trigger.trigger_id,
            AutomationWebhookTriggerUpdateInput {
                provider: Some(" Stripe.COM ".to_string()),
                provider_event_kind: Some(Some(" Checkout.Session.Completed ".to_string())),
                signature_scheme: Some(AutomationWebhookSignatureScheme::SharedSecretHeaderV1),
                ..AutomationWebhookTriggerUpdateInput::default()
            },
            Some("actor-a".to_string()),
        )
        .await
        .expect("update trigger");
    assert_eq!(updated.provider, "stripe");
    assert_eq!(
        updated.provider_event_kind.as_deref(),
        Some("checkout.session.completed")
    );
    assert_eq!(
        updated.signature_scheme,
        AutomationWebhookSignatureScheme::SharedSecretHeaderV1
    );
}

#[tokio::test]
async fn webhook_signature_verification_and_rotation_fail_closed() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-a", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-a", tenant_a.clone()))
        .await
        .expect("create webhook trigger");
    let body = br#"{"hello":"world"}"#;
    let now = crate::util::time::now_ms();

    assert_eq!(
        state
            .verify_automation_webhook_request(
                &created.trigger.public_path_token,
                None,
                body,
                Some("evt-missing".to_string()),
                now,
                300_000,
            )
            .await
            .expect_err("missing signature fails"),
        AutomationWebhookVerificationError::MissingSignature
    );

    assert_eq!(
        state
            .verify_automation_webhook_request(
                &created.trigger.public_path_token,
                Some("t=not-a-timestamp,v1=not-hex"),
                body,
                Some("evt-malformed".to_string()),
                now,
                300_000,
            )
            .await
            .expect_err("malformed signature fails"),
        AutomationWebhookVerificationError::MalformedSignature
    );

    let bad_header = automation_webhook_signature_header("wrong-secret", now, body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &created.trigger.public_path_token,
                Some(&bad_header),
                body,
                Some("evt-bad".to_string()),
                now,
                300_000,
            )
            .await
            .expect_err("bad signature fails"),
        AutomationWebhookVerificationError::BadSignature
    );

    let stale_header =
        automation_webhook_signature_header(&created.secret, now.saturating_sub(600_000), body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &created.trigger.public_path_token,
                Some(&stale_header),
                body,
                Some("evt-stale".to_string()),
                now,
                300_000,
            )
            .await
            .expect_err("stale signature fails"),
        AutomationWebhookVerificationError::StaleTimestamp
    );

    let good_header = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&good_header),
            body,
            Some("evt-ok".to_string()),
            now,
            300_000,
        )
        .await
        .expect("valid signature verifies");
    assert_eq!(verified.trigger.trigger_id, created.trigger.trigger_id);
    assert_eq!(verified.verification.provider, "generic");
    assert_eq!(verified.verification.reason_code, "verified");

    let rotated = state
        .rotate_automation_webhook_secret(
            &tenant_a,
            &created.trigger.trigger_id,
            Some("actor-a".to_string()),
        )
        .await
        .expect("rotate secret");
    let rotated_now = crate::util::time::now_ms();
    let old_after_rotate = automation_webhook_signature_header(&created.secret, rotated_now, body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &created.trigger.public_path_token,
                Some(&old_after_rotate),
                body,
                Some("evt-old".to_string()),
                rotated_now,
                300_000,
            )
            .await
            .expect_err("old rotated secret fails"),
        AutomationWebhookVerificationError::BadSignature
    );

    let new_header = automation_webhook_signature_header(&rotated.secret, rotated_now, body);
    state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&new_header),
            body,
            Some("evt-new".to_string()),
            rotated_now,
            300_000,
        )
        .await
        .expect("new rotated secret verifies");
}

#[tokio::test]
async fn webhook_signature_and_dedupe_scope_include_tenant_and_trigger() {
    Box::pin(async {
        let state = ready_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        insert_test_automation(&state, "automation-a", &tenant_a).await;
        insert_test_automation(&state, "automation-b", &tenant_b).await;
        let trigger_a = state
            .create_automation_webhook_trigger(create_input("automation-a", tenant_a.clone()))
            .await
            .expect("create trigger a");
        let trigger_b = state
            .create_automation_webhook_trigger(create_input("automation-b", tenant_b.clone()))
            .await
            .expect("create trigger b");
        let body = br#"{"shared":true}"#;
        let now = crate::util::time::now_ms();

        let tenant_a_signature = automation_webhook_signature_header(&trigger_a.secret, now, body);
        assert_eq!(
            state
                .verify_automation_webhook_request(
                    &trigger_b.trigger.public_path_token,
                    Some(&tenant_a_signature),
                    body,
                    Some("evt-shared".to_string()),
                    now,
                    300_000,
                )
                .await
                .expect_err("tenant a secret cannot verify tenant b trigger"),
            AutomationWebhookVerificationError::BadSignature
        );

        let verified_a = state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&tenant_a_signature),
                body,
                Some("evt-shared".to_string()),
                now,
                300_000,
            )
            .await
            .expect("tenant a verifies before legacy delivery record");
        state
            .record_automation_webhook_delivery(AutomationWebhookDeliveryRecord {
                delivery_id: "delivery-replay-a".to_string(),
                trigger_id: trigger_a.trigger.trigger_id.clone(),
                automation_id: "automation-a".to_string(),
                tenant_context: tenant_a.clone(),
                enterprise_scope: None,
                provider_event_id: verified_a.provider_event_id.clone(),
                body_digest: verified_a.body_digest.clone(),
                status: AutomationWebhookDeliveryStatus::Accepted,
                rejection_reason_code: None,
                idempotency_key: None,
                idempotency_record_id: None,
                dedupe_result: None,
                dedupe_reason_code: None,
                duplicate_of_delivery_id: None,
                duplicate_of_run_id: None,
                verification_scheme: Some(verified_a.verification.scheme.clone()),
                verification_provider: Some(verified_a.verification.provider.clone()),
                verification_reason_code: Some(verified_a.verification.reason_code.clone()),
                queued_run_id: None,
                woken_run_id: None,
                woken_wait_id: None,
                feedback_loop: None,
                correlation: None,
                received_at_ms: verified_a.received_at_ms,
                accepted_at_ms: Some(verified_a.received_at_ms),
                rejected_at_ms: None,
                sanitized_preview: json!({"safe": true}),
                audit_event_id: None,
            })
            .await
            .expect("record legacy dedupe baseline");

        let distinct_now = now + 1;
        let distinct_signature =
            automation_webhook_signature_header(&trigger_a.secret, distinct_now, body);
        let distinct = state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&distinct_signature),
                body,
                Some("evt-distinct".to_string()),
                distinct_now,
                300_000,
            )
            .await
            .expect("same body verifies before queue-time dedupe");
        let distinct_delivery = match state
            .queue_automation_v2_run_from_webhook_delivery(distinct, json!({"shared": true}))
            .await
            .expect("distinct event duplicate outcome")
        {
            AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
            other => panic!("expected body duplicate, got {other:?}"),
        };
        assert_eq!(
            distinct_delivery.dedupe_result,
            Some(AutomationWebhookDedupeResult::Duplicate)
        );
        assert_eq!(
            distinct_delivery.duplicate_of_delivery_id.as_deref(),
            Some("delivery-replay-a")
        );

        let body_fallback_now = now + 2;
        let body_fallback_signature =
            automation_webhook_signature_header(&trigger_a.secret, body_fallback_now, body);
        let body_fallback = state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&body_fallback_signature),
                body,
                None,
                body_fallback_now,
                300_000,
            )
            .await
            .expect("body digest fallback verifies before queue-time dedupe");
        assert!(matches!(
            state
                .queue_automation_v2_run_from_webhook_delivery(
                    body_fallback,
                    json!({"shared": true})
                )
                .await
                .expect("body fallback duplicate outcome"),
            AutomationWebhookQueueResult::Duplicate { .. }
        ));

        let replay_now = now + 3;
        let replay_signature =
            automation_webhook_signature_header(&trigger_a.secret, replay_now, body);
        let replay = state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&replay_signature),
                body,
                Some("evt-shared".to_string()),
                replay_now,
                300_000,
            )
            .await
            .expect("provider replay verifies before queue-time dedupe");
        assert!(matches!(
            state
                .queue_automation_v2_run_from_webhook_delivery(replay, json!({"shared": true}))
                .await
                .expect("provider duplicate outcome"),
            AutomationWebhookQueueResult::Duplicate { .. }
        ));

        let tenant_b_signature =
            automation_webhook_signature_header(&trigger_b.secret, replay_now, body);
        let tenant_b_verified = state
            .verify_automation_webhook_request(
                &trigger_b.trigger.public_path_token,
                Some(&tenant_b_signature),
                body,
                Some("evt-shared".to_string()),
                replay_now,
                300_000,
            )
            .await
            .expect("tenant b can use same provider event id independently");
        assert!(matches!(
            state
                .queue_automation_v2_run_from_webhook_delivery(
                    tenant_b_verified,
                    json!({"shared": true})
                )
                .await
                .expect("tenant b queue"),
            AutomationWebhookQueueResult::Accepted { .. }
        ));
    })
    .await;
}

#[tokio::test]
async fn webhook_queue_rejects_automation_tenant_mismatch_without_run() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    insert_test_automation(&state, "automation-a", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-a", tenant_a.clone()))
        .await
        .expect("create webhook trigger");

    let mut tenant_b_automation = AutomationSpecBuilder::new("automation-a").build();
    tenant_b_automation.set_tenant_context(&tenant_b);
    state
        .automations_v2
        .write()
        .await
        .insert("automation-a".to_string(), tenant_b_automation);

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-tenant-mismatch".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");

    let outcome = state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("queue outcome");
    let delivery = match outcome {
        AutomationWebhookQueueResult::Rejected {
            delivery,
            reason_code,
        } => {
            assert_eq!(reason_code, "automation_tenant_mismatch");
            delivery
        }
        other => panic!("expected tenant mismatch rejection, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Rejected);
    assert_eq!(
        delivery.rejection_reason_code.as_deref(),
        Some("automation_tenant_mismatch")
    );
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_queue_rejects_trigger_outside_automation_resource_scope() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    let automation_scope = ResourceScope::root(ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::SourceBinding,
        "github-primary",
    ));
    let mut automation = AutomationSpecBuilder::new("automation-scoped").build();
    automation.set_tenant_context(&tenant_a);
    automation.metadata = Some(json!({
        "enterprise_scope": serde_json::to_value(AutomationEnterpriseScope {
            owning_org_unit_id: Some("dept-a".to_string()),
            resource_scope: Some(automation_scope),
            ..AutomationEnterpriseScope::default()
        })
        .expect("enterprise scope json")
    }));
    automation.set_tenant_context(&tenant_a);
    state
        .automations_v2
        .write()
        .await
        .insert("automation-scoped".to_string(), automation);

    let mut input = create_input("automation-scoped", tenant_a.clone());
    input.owning_org_unit_id = Some("dept-a".to_string());
    let mut trigger_scope = ResourceScope::root(ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::SourceBinding,
        "github-primary",
    ));
    trigger_scope.allowed_resources.push(ResourceRef::new(
        "org-a",
        "workspace-a",
        ResourceKind::SourceBinding,
        "github-other",
    ));
    input.resource_scope = Some(trigger_scope);
    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("create webhook trigger");
    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-scope-mismatch".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");

    let outcome = state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("queue outcome");
    let delivery = match outcome {
        AutomationWebhookQueueResult::Rejected {
            delivery,
            reason_code,
        } => {
            assert_eq!(reason_code, "webhook_resource_scope_mismatch");
            delivery
        }
        other => panic!("expected scope mismatch rejection, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Rejected);
    assert_eq!(
        delivery.rejection_reason_code.as_deref(),
        Some("webhook_resource_scope_mismatch")
    );
    assert!(delivery.enterprise_scope.is_some());
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_queue_rejects_scoped_trigger_for_unscoped_automation() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-unscoped", &tenant_a).await;

    let mut input = create_input("automation-unscoped", tenant_a.clone());
    input.owning_org_unit_id = Some("dept-a".to_string());
    let created = state
        .create_automation_webhook_trigger(input)
        .await
        .expect("create scoped webhook trigger");
    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-missing-automation-scope".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");

    let outcome = state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("queue outcome");
    let delivery = match outcome {
        AutomationWebhookQueueResult::Rejected {
            delivery,
            reason_code,
        } => {
            assert_eq!(reason_code, "webhook_automation_missing_enterprise_scope");
            delivery
        }
        other => panic!("expected missing automation scope rejection, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Rejected);
    assert_eq!(
        delivery.rejection_reason_code.as_deref(),
        Some("webhook_automation_missing_enterprise_scope")
    );
    assert!(delivery.enterprise_scope.is_some());
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_queue_treats_accepted_marker_without_run_as_duplicate() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-marker", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-marker", tenant_a.clone()))
        .await
        .expect("create webhook trigger");

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-marker".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    state
        .record_automation_webhook_delivery(AutomationWebhookDeliveryRecord {
            delivery_id: "delivery-marker".to_string(),
            trigger_id: created.trigger.trigger_id.clone(),
            automation_id: "automation-marker".to_string(),
            tenant_context: tenant_a.clone(),
            enterprise_scope: None,
            provider_event_id: verified.provider_event_id.clone(),
            body_digest: verified.body_digest.clone(),
            status: AutomationWebhookDeliveryStatus::Accepted,
            rejection_reason_code: None,
            idempotency_key: None,
            idempotency_record_id: None,
            dedupe_result: None,
            dedupe_reason_code: None,
            duplicate_of_delivery_id: None,
            duplicate_of_run_id: None,
            verification_scheme: Some(verified.verification.scheme.clone()),
            verification_provider: Some(verified.verification.provider.clone()),
            verification_reason_code: Some(verified.verification.reason_code.clone()),
            queued_run_id: None,
            woken_run_id: None,
            woken_wait_id: None,
            feedback_loop: None,
            correlation: None,
            received_at_ms: verified.received_at_ms,
            accepted_at_ms: Some(verified.received_at_ms),
            rejected_at_ms: None,
            sanitized_preview: json!({"ok": true}),
            audit_event_id: None,
        })
        .await
        .expect("record idempotency marker");

    let outcome = state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("queue outcome");
    let delivery = match outcome {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate outcome, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Duplicate);
    assert_eq!(
        delivery.dedupe_result,
        Some(AutomationWebhookDedupeResult::Duplicate)
    );
    assert_eq!(
        delivery.duplicate_of_delivery_id.as_deref(),
        Some("delivery-marker")
    );
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_queue_dedupes_provider_event_id_with_original_run_correlation() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-provider-dedupe", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-provider-dedupe",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-provider-duplicate".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    let accepted = match state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("accepted outcome")
    {
        AutomationWebhookQueueResult::Accepted { delivery, run } => (delivery, run),
        other => panic!("expected accepted outcome, got {other:?}"),
    };

    let duplicate_now = now + 1;
    let duplicate_signature =
        automation_webhook_signature_header(&created.secret, duplicate_now, body);
    let duplicate = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&duplicate_signature),
            body,
            Some("evt-provider-duplicate".to_string()),
            duplicate_now,
            300_000,
        )
        .await
        .expect("duplicate verifies");
    let duplicate_delivery = match state
        .queue_automation_v2_run_from_webhook_delivery(duplicate, json!({"ok": true}))
        .await
        .expect("duplicate outcome")
    {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate outcome, got {other:?}"),
    };

    assert_eq!(
        duplicate_delivery.dedupe_result,
        Some(AutomationWebhookDedupeResult::Duplicate)
    );
    assert_eq!(
        duplicate_delivery.duplicate_of_delivery_id.as_deref(),
        Some(accepted.0.delivery_id.as_str())
    );
    assert_eq!(
        duplicate_delivery.duplicate_of_run_id.as_deref(),
        Some(accepted.1.run_id.as_str())
    );
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
}

#[tokio::test]
async fn webhook_queue_rejects_provider_event_id_conflict() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-conflict", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-conflict", tenant_a.clone()))
        .await
        .expect("create webhook trigger");

    let body_a = br#"{"ok":true}"#;
    let now = now_ms();
    let signature_a = automation_webhook_signature_header(&created.secret, now, body_a);
    let verified_a = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature_a),
            body_a,
            Some("evt-conflict".to_string()),
            now,
            300_000,
        )
        .await
        .expect("first verifies");
    let accepted = match state
        .queue_automation_v2_run_from_webhook_delivery(verified_a, json!({"ok": true}))
        .await
        .expect("accepted outcome")
    {
        AutomationWebhookQueueResult::Accepted { delivery, run } => (delivery, run),
        other => panic!("expected accepted outcome, got {other:?}"),
    };

    let body_b = br#"{"ok":false}"#;
    let conflict_now = now + 1;
    let signature_b = automation_webhook_signature_header(&created.secret, conflict_now, body_b);
    let verified_b = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature_b),
            body_b,
            Some("evt-conflict".to_string()),
            conflict_now,
            300_000,
        )
        .await
        .expect("conflict verifies");
    let conflict_delivery = match state
        .queue_automation_v2_run_from_webhook_delivery(verified_b, json!({"ok": false}))
        .await
        .expect("conflict outcome")
    {
        AutomationWebhookQueueResult::Rejected {
            delivery,
            reason_code,
        } => {
            assert_eq!(reason_code, "idempotency_conflict");
            delivery
        }
        other => panic!("expected conflict rejection, got {other:?}"),
    };

    assert_eq!(
        conflict_delivery.dedupe_result,
        Some(AutomationWebhookDedupeResult::Conflict)
    );
    assert_eq!(
        conflict_delivery.rejection_reason_code.as_deref(),
        Some("idempotency_conflict")
    );
    assert_eq!(
        conflict_delivery.duplicate_of_delivery_id.as_deref(),
        Some(accepted.0.delivery_id.as_str())
    );
    assert_eq!(
        conflict_delivery.duplicate_of_run_id.as_deref(),
        Some(accepted.1.run_id.as_str())
    );
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
}

#[tokio::test]
async fn webhook_duplicate_after_restart_uses_persisted_idempotency_outcome() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-restart-dedupe", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-restart-dedupe",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"restart":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-restart".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    let accepted = match state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"restart": true}))
        .await
        .expect("accepted outcome")
    {
        AutomationWebhookQueueResult::Accepted { delivery, run } => (delivery, run),
        other => panic!("expected accepted outcome, got {other:?}"),
    };

    let mut restarted = ready_test_state().await;
    restarted.automation_webhook_triggers_path = state.automation_webhook_triggers_path.clone();
    restarted.automation_webhook_deliveries_path = state.automation_webhook_deliveries_path.clone();
    restarted.automation_webhook_secret_material_path =
        state.automation_webhook_secret_material_path.clone();
    restarted.idempotency_keys_path = state.idempotency_keys_path.clone();
    insert_test_automation(&restarted, "automation-restart-dedupe", &tenant_a).await;
    restarted
        .load_automation_webhook_records()
        .await
        .expect("load webhook records");
    restarted
        .load_idempotency_keys()
        .await
        .expect("load idempotency keys");

    let duplicate_now = now + 1;
    let duplicate_signature =
        automation_webhook_signature_header(&created.secret, duplicate_now, body);
    let duplicate = restarted
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&duplicate_signature),
            body,
            Some("evt-restart".to_string()),
            duplicate_now,
            300_000,
        )
        .await
        .expect("duplicate verifies after restart");
    let duplicate_delivery = match restarted
        .queue_automation_v2_run_from_webhook_delivery(duplicate, json!({"restart": true}))
        .await
        .expect("duplicate after restart")
    {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate after restart, got {other:?}"),
    };

    assert_eq!(
        duplicate_delivery.duplicate_of_delivery_id.as_deref(),
        Some(accepted.0.delivery_id.as_str())
    );
    assert_eq!(
        duplicate_delivery.duplicate_of_run_id.as_deref(),
        Some(accepted.1.run_id.as_str())
    );
    assert!(restarted.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_inbox_reconnects_received_event_to_existing_delivery() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-inbox-reconnect", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-inbox-reconnect",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"reconnect":true}"#;
    let now = now_ms();
    let raw_event = state
        .record_automation_webhook_raw_event(AutomationWebhookRawEventCreateInput {
            trigger: created.trigger.clone(),
            provider_event_id: Some("evt-inbox-reconnect".to_string()),
            body_digest: automation_webhook_body_digest(body),
            verification: None,
            feedback_loop_candidate: None,
            headers_digest: "headers-digest".to_string(),
            headers_redacted: json!({"x-tandem-webhook-event-id": "evt-inbox-reconnect"}),
            content_type: Some("application/json".to_string()),
            payload: body.to_vec(),
            received_at_ms: now,
        })
        .await
        .expect("record raw event");
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-inbox-reconnect".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verify webhook");
    let (delivery, run) = match state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"reconnect": true}))
        .await
        .expect("accepted delivery")
    {
        AutomationWebhookQueueResult::Accepted { delivery, run } => (delivery, run),
        other => panic!("expected accepted delivery, got {other:?}"),
    };
    assert_eq!(delivery.queued_run_id.as_deref(), Some(run.run_id.as_str()));
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);

    let report = state.process_automation_webhook_inbox_once(10).await;
    assert_eq!(report.checked, 1);
    assert_eq!(report.processed, 1);
    assert_eq!(report.failed, 0);

    let updated = state
        .get_automation_webhook_raw_event(&tenant_a, &raw_event.event_id)
        .await
        .expect("load raw event")
        .expect("raw event exists");
    assert_eq!(updated.status, AutomationWebhookDeliveryStatus::Accepted);
    assert_eq!(
        updated.delivery_id.as_deref(),
        Some(delivery.delivery_id.as_str())
    );
    assert_eq!(updated.queued_run_id.as_deref(), Some(run.run_id.as_str()));
    assert_eq!(
        updated.dedupe_result,
        Some(AutomationWebhookDedupeResult::Accepted)
    );

    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
        .await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
}

#[tokio::test]
async fn webhook_retention_prunes_expired_raw_events_payloads_and_deliveries() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-webhook-retention", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-webhook-retention",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"retention":true}"#;
    let now = now_ms();
    let raw_event = state
        .record_automation_webhook_raw_event(AutomationWebhookRawEventCreateInput {
            trigger: created.trigger.clone(),
            provider_event_id: Some("evt-retention".to_string()),
            body_digest: automation_webhook_body_digest(body),
            verification: None,
            feedback_loop_candidate: None,
            headers_digest: "headers-digest".to_string(),
            headers_redacted: json!({"x-tandem-webhook-event-id": "evt-retention"}),
            content_type: Some("application/json".to_string()),
            payload: body.to_vec(),
            received_at_ms: now,
        })
        .await
        .expect("record raw event");
    let payload_path = raw_payload_path(&state, &raw_event.event_id);
    assert!(payload_path.exists());

    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-retention".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    let delivery = match state
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"retention": true}))
        .await
        .expect("accepted outcome")
    {
        AutomationWebhookQueueResult::Accepted { delivery, .. } => delivery,
        other => panic!("expected accepted outcome, got {other:?}"),
    };
    state
        .update_automation_webhook_raw_event_outcome(&tenant_a, &raw_event.event_id, &delivery, now)
        .await
        .expect("update raw event outcome")
        .expect("updated raw event");
    let stale_rejection = state
        .record_automation_webhook_rejection(
            &created.trigger,
            Some("evt-retention-stale-rejection".to_string()),
            automation_webhook_body_digest(br#"{"rejected":true}"#),
            AutomationWebhookDeliveryStatus::Rejected,
            "bad_signature",
            now,
            json!({"rejected": true}),
            None,
        )
        .await
        .expect("record stale rejection-only delivery");
    let after_default_retention = now + 31 * 24 * 60 * 60 * 1_000;
    let recent_rejection = state
        .record_automation_webhook_rejection(
            &created.trigger,
            Some("evt-retention-recent-rejection".to_string()),
            automation_webhook_body_digest(br#"{"recent":true}"#),
            AutomationWebhookDeliveryStatus::Rejected,
            "missing_signature",
            after_default_retention - 60 * 60 * 1_000,
            json!({"recent": true}),
            None,
        )
        .await
        .expect("record recent rejection-only delivery");

    assert_eq!(
        state
            .list_automation_webhook_raw_events(&tenant_a, None, None, None, 10)
            .await
            .len(),
        1
    );
    assert_eq!(
        state
            .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
            .await
            .len(),
        3
    );

    let report = state
        .prune_automation_webhook_retention(after_default_retention)
        .await
        .expect("prune retention");
    assert_eq!(report.pruned_events, 1);
    assert_eq!(report.pruned_payloads, 1);
    assert_eq!(report.pruned_deliveries, 2);
    assert!(!payload_path.exists());
    assert!(state
        .get_automation_webhook_raw_event(&tenant_a, &raw_event.event_id)
        .await
        .expect("get raw event")
        .is_none());
    let remaining_deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
        .await;
    assert_eq!(remaining_deliveries.len(), 1);
    assert!(remaining_deliveries
        .iter()
        .any(|delivery| delivery.delivery_id == recent_rejection.delivery_id));
    assert!(!remaining_deliveries
        .iter()
        .any(|delivery| delivery.delivery_id == stale_rejection.delivery_id));
}

#[tokio::test]
async fn webhook_inbox_dead_letters_raw_event_when_trigger_is_deleted() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-deleted-trigger", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-deleted-trigger",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"deleted_trigger":true}"#;
    let raw_event = state
        .record_automation_webhook_raw_event(AutomationWebhookRawEventCreateInput {
            trigger: created.trigger.clone(),
            provider_event_id: Some("evt-deleted-trigger".to_string()),
            body_digest: automation_webhook_body_digest(body),
            verification: None,
            feedback_loop_candidate: None,
            headers_digest: "headers-digest".to_string(),
            headers_redacted: json!({"x-tandem-webhook-event-id": "evt-deleted-trigger"}),
            content_type: Some("application/json".to_string()),
            payload: body.to_vec(),
            received_at_ms: now_ms(),
        })
        .await
        .expect("record raw event");
    assert!(state
        .delete_automation_webhook_trigger(&tenant_a, &created.trigger.trigger_id)
        .await
        .expect("delete trigger"));

    let report = state.process_automation_webhook_inbox_once(10).await;
    assert_eq!(report.checked, 1);
    assert_eq!(report.processed, 1);
    assert_eq!(report.failed, 0);

    let updated = state
        .get_automation_webhook_raw_event(&tenant_a, &raw_event.event_id)
        .await
        .expect("get raw event")
        .expect("raw event remains inspectable");
    assert_eq!(updated.status, AutomationWebhookDeliveryStatus::Failed);
    assert_eq!(
        updated.rejection_reason_code.as_deref(),
        Some("webhook_trigger_missing")
    );
    assert_eq!(
        updated
            .correlation
            .as_ref()
            .map(|correlation| &correlation.outcome),
        Some(&AutomationWebhookCorrelationOutcome::DeadLetter)
    );
    assert!(updated.delivery_id.is_none());

    let second_report = state.process_automation_webhook_inbox_once(10).await;
    assert_eq!(second_report.checked, 0);
    assert_eq!(second_report.processed, 0);
    assert_eq!(second_report.failed, 0);
}

#[tokio::test]
async fn webhook_retry_after_orphaned_idempotency_reservation_creates_run() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-orphan-reservation", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-orphan-reservation",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"orphan":true}"#;
    let provider_event_id = "evt-orphan-reservation".to_string();
    let body_digest = automation_webhook_body_digest(body);
    let now = now_ms();
    let reservation = state
        .reserve_automation_webhook_dedupe(
            &created.trigger,
            Some(&provider_event_id),
            &body_digest,
            now,
        )
        .await
        .expect("reserve orphaned idempotency records");
    assert!(matches!(
        reservation,
        AutomationWebhookDedupeDecision::New { records } if records.len() == 2
    ));
    assert!(state.automation_webhook_deliveries.read().await.is_empty());

    let mut restarted = ready_test_state().await;
    restarted.automation_webhook_triggers_path = state.automation_webhook_triggers_path.clone();
    restarted.automation_webhook_deliveries_path = state.automation_webhook_deliveries_path.clone();
    restarted.automation_webhook_secret_material_path =
        state.automation_webhook_secret_material_path.clone();
    restarted.idempotency_keys_path = state.idempotency_keys_path.clone();
    insert_test_automation(&restarted, "automation-orphan-reservation", &tenant_a).await;
    restarted
        .load_automation_webhook_records()
        .await
        .expect("load webhook records");
    restarted
        .load_idempotency_keys()
        .await
        .expect("load idempotency keys");
    assert!(restarted
        .automation_webhook_deliveries
        .read()
        .await
        .is_empty());

    let retry_now = now + 1;
    let retry_signature = automation_webhook_signature_header(&created.secret, retry_now, body);
    let retry = restarted
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&retry_signature),
            body,
            Some(provider_event_id.clone()),
            retry_now,
            300_000,
        )
        .await
        .expect("retry verifies after restart");
    let accepted = match restarted
        .queue_automation_v2_run_from_webhook_delivery(retry, json!({"orphan": true}))
        .await
        .expect("retry accepted")
    {
        AutomationWebhookQueueResult::Accepted { delivery, run } => (delivery, run),
        other => panic!("expected accepted retry, got {other:?}"),
    };
    assert_eq!(
        accepted.0.dedupe_result,
        Some(AutomationWebhookDedupeResult::Accepted)
    );
    assert_eq!(restarted.automation_v2_runs.read().await.len(), 1);

    let duplicate_now = now + 2;
    let duplicate_signature =
        automation_webhook_signature_header(&created.secret, duplicate_now, body);
    let duplicate = restarted
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&duplicate_signature),
            body,
            Some(provider_event_id),
            duplicate_now,
            300_000,
        )
        .await
        .expect("duplicate verifies after accepted retry");
    let duplicate_delivery = match restarted
        .queue_automation_v2_run_from_webhook_delivery(duplicate, json!({"orphan": true}))
        .await
        .expect("duplicate after recovered retry")
    {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate after recovered retry, got {other:?}"),
    };
    assert_eq!(
        duplicate_delivery.duplicate_of_delivery_id.as_deref(),
        Some(accepted.0.delivery_id.as_str())
    );
    assert_eq!(
        duplicate_delivery.duplicate_of_run_id.as_deref(),
        Some(accepted.1.run_id.as_str())
    );
}

#[tokio::test]
async fn webhook_queue_serializes_duplicate_delivery_race() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-race", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-race", tenant_a.clone()))
        .await
        .expect("create webhook trigger");

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-race".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    let preview = json!({"ok": true});

    let (first, second) = tokio::join!(
        state.queue_automation_v2_run_from_webhook_delivery(verified.clone(), preview.clone()),
        state.queue_automation_v2_run_from_webhook_delivery(verified, preview),
    );
    let outcomes = vec![
        first.expect("first outcome"),
        second.expect("second outcome"),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AutomationWebhookQueueResult::Accepted { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AutomationWebhookQueueResult::Duplicate { .. }))
            .count(),
        1
    );
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_a, &created.trigger.trigger_id)
        .await;
    assert_eq!(deliveries.len(), 2);
}

#[tokio::test]
async fn patching_provider_to_linear_forces_scheme_and_starts_lifecycle() {
    // Codex P2 on #1799: a PATCH that changes only `provider` to `linear` (no
    // signature_scheme in the payload) must still pin the Linear scheme and
    // start the provider-owned-secret lifecycle, or the trigger is advertised
    // as Linear while its stale scheme rejects real Linear deliveries.
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-provider-patch", &tenant_a).await;

    // A generic trigger starts on the Tandem HMAC scheme with no Linear state.
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-provider-patch",
            tenant_a.clone(),
        ))
        .await
        .expect("create trigger");
    assert_eq!(
        created.trigger.signature_scheme,
        AutomationWebhookSignatureScheme::HmacSha256V1
    );
    assert!(created.trigger.linear_verification.is_none());

    // PATCH provider -> linear only (no signature_scheme).
    let updated = state
        .update_automation_webhook_trigger(
            &tenant_a,
            "automation-provider-patch",
            &created.trigger.trigger_id,
            AutomationWebhookTriggerUpdateInput {
                provider: Some("linear.app".to_string()),
                ..AutomationWebhookTriggerUpdateInput::default()
            },
            Some("actor-a".to_string()),
        )
        .await
        .expect("patch provider to linear");
    assert_eq!(updated.provider, "linear");
    assert_eq!(
        updated.signature_scheme,
        AutomationWebhookSignatureScheme::LinearHmacSha256,
        "provider patch must force the Linear scheme"
    );
    let verification = updated
        .linear_verification
        .as_ref()
        .expect("linear lifecycle started");
    assert!(!verification.secret_configured());
}
