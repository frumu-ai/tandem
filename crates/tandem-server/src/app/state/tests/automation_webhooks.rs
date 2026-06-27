use super::*;
use crate::app::state::{
    automation_webhook_body_digest, automation_webhook_signature_header,
    AutomationWebhookTriggerCreateInput, AutomationWebhookVerificationError,
};

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
        default_risk_tier: None,
        provider: "generic".to_string(),
        provider_event_kind: Some("event.created".to_string()),
        enabled: true,
    }
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
        provider_event_id: Some("evt-b".to_string()),
        body_digest: automation_webhook_body_digest(br#"{"ok":true}"#),
        status: AutomationWebhookDeliveryStatus::Accepted,
        rejection_reason_code: None,
        queued_run_id: None,
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
        provider_event_id: Some("evt-a".to_string()),
        body_digest: automation_webhook_body_digest(br#"{"ok":true}"#),
        status: AutomationWebhookDeliveryStatus::Accepted,
        rejection_reason_code: None,
        queued_run_id: None,
        received_at_ms: 1_000,
        accepted_at_ms: Some(1_000),
        rejected_at_ms: None,
        sanitized_preview: json!({
            "authorization": "Bearer token",
            "nested": { "api_key": "secret", "safe": true }
        }),
        audit_event_id: Some("audit-a".to_string()),
    };
    let stored = state
        .record_automation_webhook_delivery(delivery)
        .await
        .expect("record delivery");
    assert_eq!(stored.sanitized_preview["authorization"], "[redacted]");
    assert_eq!(stored.sanitized_preview["nested"]["api_key"], "[redacted]");
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
async fn webhook_signature_and_replay_scope_include_tenant_and_trigger() {
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
        .expect("tenant a verifies before replay record");
    state
        .record_automation_webhook_delivery(AutomationWebhookDeliveryRecord {
            delivery_id: "delivery-replay-a".to_string(),
            trigger_id: trigger_a.trigger.trigger_id.clone(),
            automation_id: "automation-a".to_string(),
            tenant_context: tenant_a.clone(),
            provider_event_id: verified_a.provider_event_id.clone(),
            body_digest: verified_a.body_digest.clone(),
            status: AutomationWebhookDeliveryStatus::Accepted,
            rejection_reason_code: None,
            queued_run_id: None,
            received_at_ms: verified_a.received_at_ms,
            accepted_at_ms: Some(verified_a.received_at_ms),
            rejected_at_ms: None,
            sanitized_preview: json!({"safe": true}),
            audit_event_id: None,
        })
        .await
        .expect("record replay baseline");

    let replay_now = now + 1;
    let replay_signature = automation_webhook_signature_header(&trigger_a.secret, replay_now, body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&replay_signature),
                body,
                Some("evt-shared".to_string()),
                replay_now,
                300_000,
            )
            .await
            .expect_err("tenant a replay fails"),
        AutomationWebhookVerificationError::ReplayDetected
    );

    let tenant_b_signature =
        automation_webhook_signature_header(&trigger_b.secret, replay_now, body);
    state
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
}
