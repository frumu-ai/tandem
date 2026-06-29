use super::*;
use crate::app::state::{
    automation_webhook_body_digest, automation_webhook_signature_header,
    AutomationWebhookQueueResult, AutomationWebhookTriggerCreateInput,
    AutomationWebhookTriggerUpdateInput, AutomationWebhookVerificationError,
};
use crate::automation_v2::types::AutomationWebhookSignatureScheme;
use crate::stateful_runtime::{
    list_stateful_waits, query_stateful_run_events, read_stateful_run_snapshot_for_run,
    stateful_webhook_wait_metadata, upsert_stateful_wait, StatefulRunEventQuery,
    StatefulRuntimeScope, StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWaitQuery,
    StatefulWaitRecord, StatefulWaitStatus, StatefulWebhookWaitMatch, StatefulWorkflowRunStatus,
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
        name: Some("Generic webhook".to_string()),
        provider: "generic".to_string(),
        provider_event_kind: Some("event.created".to_string()),
        signature_scheme: None,
        enabled: true,
    }
}

async fn ready_stateful_webhook_test_state() -> AppState {
    let mut state = ready_test_state().await;
    let root = std::env::temp_dir().join(format!(
        "tandem-stateful-webhook-test-{}",
        uuid::Uuid::new_v4()
    ));
    state.runtime_events_path = root.join("runtime").join("events.jsonl");
    state
}

async fn reload_stateful_webhook_test_state(source: &AppState) -> AppState {
    let mut reloaded = ready_test_state().await;
    reloaded.automations_v2_path = source.automations_v2_path.clone();
    reloaded.automation_v2_runs_path = source.automation_v2_runs_path.clone();
    reloaded.automation_v2_runs_archive_path = source.automation_v2_runs_archive_path.clone();
    reloaded.automation_webhook_triggers_path = source.automation_webhook_triggers_path.clone();
    reloaded.automation_webhook_deliveries_path = source.automation_webhook_deliveries_path.clone();
    reloaded.automation_webhook_secret_material_path =
        source.automation_webhook_secret_material_path.clone();
    reloaded.runtime_events_path = source.runtime_events_path.clone();
    reloaded
        .load_automations_v2()
        .await
        .expect("reload automations");
    reloaded
        .load_automation_webhook_records()
        .await
        .expect("reload webhook records");
    reloaded
}

fn webhook_wait_record(
    wait_id: &str,
    run_id: &str,
    tenant_context: TenantContext,
    match_rules: StatefulWebhookWaitMatch,
    now: u64,
) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: 1,
        wait_id: wait_id.to_string(),
        run_id: run_id.to_string(),
        wait_kind: StatefulWaitKind::Webhook,
        status: StatefulWaitStatus::Waiting,
        scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
        phase_id: Some("phase-webhook".to_string()),
        reason: Some("wait for matching webhook".to_string()),
        created_at_ms: now,
        updated_at_ms: now,
        wake_at_ms: None,
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(stateful_webhook_wait_metadata(match_rules, None)),
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
        verification_scheme: None,
        verification_provider: None,
        verification_reason_code: None,
        queued_run_id: None,
        woken_run_id: None,
        woken_wait_id: None,
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
        verification_scheme: None,
        verification_provider: None,
        verification_reason_code: None,
        queued_run_id: None,
        woken_run_id: None,
        woken_wait_id: None,
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
            verification_scheme: Some(verified_a.verification.scheme.clone()),
            verification_provider: Some(verified_a.verification.provider.clone()),
            verification_reason_code: Some(verified_a.verification.reason_code.clone()),
            queued_run_id: None,
            woken_run_id: None,
            woken_wait_id: None,
            received_at_ms: verified_a.received_at_ms,
            accepted_at_ms: Some(verified_a.received_at_ms),
            rejected_at_ms: None,
            sanitized_preview: json!({"safe": true}),
            audit_event_id: None,
        })
        .await
        .expect("record replay baseline");

    let distinct_now = now + 1;
    let distinct_signature =
        automation_webhook_signature_header(&trigger_a.secret, distinct_now, body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&distinct_signature),
                body,
                Some("evt-distinct".to_string()),
                distinct_now,
                300_000,
            )
            .await
            .expect_err("same body with a distinct unsigned event id is a replay"),
        AutomationWebhookVerificationError::ReplayDetected
    );

    let body_fallback_now = now + 2;
    let body_fallback_signature =
        automation_webhook_signature_header(&trigger_a.secret, body_fallback_now, body);
    assert_eq!(
        state
            .verify_automation_webhook_request(
                &trigger_a.trigger.public_path_token,
                Some(&body_fallback_signature),
                body,
                None,
                body_fallback_now,
                300_000,
            )
            .await
            .expect_err("body digest fallback catches no-id replay"),
        AutomationWebhookVerificationError::ReplayDetected
    );

    let replay_now = now + 3;
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
            .expect_err("tenant a provider event id replay fails"),
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
            provider_event_id: verified.provider_event_id.clone(),
            body_digest: verified.body_digest.clone(),
            status: AutomationWebhookDeliveryStatus::Accepted,
            rejection_reason_code: None,
            verification_scheme: Some(verified.verification.scheme.clone()),
            verification_provider: Some(verified.verification.provider.clone()),
            verification_reason_code: Some(verified.verification.reason_code.clone()),
            queued_run_id: None,
            woken_run_id: None,
            woken_wait_id: None,
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
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn webhook_queue_wakes_persisted_stateful_wait_after_restart_once() {
    let state = ready_stateful_webhook_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-wake", &tenant_a).await;
    state
        .persist_automations_v2()
        .await
        .expect("persist automation");
    let created = state
        .create_automation_webhook_trigger(create_input("automation-wake", tenant_a.clone()))
        .await
        .expect("create webhook trigger");
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    upsert_stateful_wait(
        &paths.waits_path,
        webhook_wait_record(
            "wait-webhook",
            "run-waiting",
            tenant_a.clone(),
            StatefulWebhookWaitMatch {
                trigger_id: Some(created.trigger.trigger_id.clone()),
                provider: Some("generic".to_string()),
                provider_event_kind: Some("event.created".to_string()),
                ..StatefulWebhookWaitMatch::default()
            },
            1_000,
        ),
    )
    .await
    .expect("persist webhook wait");

    let reloaded = reload_stateful_webhook_test_state(&state).await;
    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = reloaded
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-wake".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");

    let first = reloaded
        .queue_automation_v2_run_from_webhook_delivery(verified.clone(), json!({"ok": true}))
        .await
        .expect("queue wake");
    let delivery = match first {
        AutomationWebhookQueueResult::Woken { delivery, wait } => {
            assert_eq!(wait.wait_id, "wait-webhook");
            assert_eq!(wait.run_id, "run-waiting");
            delivery
        }
        other => panic!("expected webhook wake outcome, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Accepted);
    assert_eq!(delivery.queued_run_id.as_deref(), None);
    assert_eq!(delivery.woken_run_id.as_deref(), Some("run-waiting"));
    assert_eq!(delivery.woken_wait_id.as_deref(), Some("wait-webhook"));
    assert!(reloaded.automation_v2_runs.read().await.is_empty());

    let waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_a,
        StatefulWaitQuery {
            run_id: Some("run-waiting"),
            wait_kind: Some(StatefulWaitKind::Webhook),
            status: Some(StatefulWaitStatus::Woken),
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].event_seq, Some(1));
    assert!(waits[0]
        .wake_idempotency_key
        .as_deref()
        .unwrap_or_default()
        .contains("evt-wake"));

    let events = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_a,
        StatefulRunEventQuery {
            run_id: "run-waiting",
            after_seq: None,
            limit: None,
        },
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "stateful_runtime.wait.webhook_woken");
    assert_eq!(
        events[0].payload["delivery_id"].as_str(),
        Some(delivery.delivery_id.as_str())
    );
    assert_eq!(events[0].payload["wait_id"], "wait-webhook");

    let snapshot = read_stateful_run_snapshot_for_run(
        &paths.snapshots_root,
        &tenant_a,
        "run-waiting",
        &format!("stateful-webhook-wake-{}", delivery.delivery_id),
    )
    .expect("read snapshot")
    .expect("wake snapshot");
    assert_eq!(snapshot.status, StatefulWorkflowRunStatus::Running);
    assert_eq!(
        snapshot.payload_digest.as_deref(),
        Some(verified.body_digest.as_str())
    );

    let duplicate = reloaded
        .queue_automation_v2_run_from_webhook_delivery(verified, json!({"ok": true}))
        .await
        .expect("queue duplicate");
    let duplicate_delivery = match duplicate {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate webhook delivery, got {other:?}"),
    };
    assert_eq!(
        duplicate_delivery.status,
        AutomationWebhookDeliveryStatus::Duplicate
    );
    assert_eq!(duplicate_delivery.woken_run_id, None);
    assert_eq!(duplicate_delivery.woken_wait_id, None);
    assert_eq!(
        query_stateful_run_events(
            &paths.run_events_path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-waiting",
                after_seq: None,
                limit: None,
            },
        )
        .len(),
        1
    );
}

#[tokio::test]
async fn webhook_queue_does_not_wake_wait_from_another_tenant() {
    let state = ready_stateful_webhook_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    insert_test_automation(&state, "automation-tenant-wake", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input("automation-tenant-wake", tenant_a.clone()))
        .await
        .expect("create webhook trigger");
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    upsert_stateful_wait(
        &paths.waits_path,
        webhook_wait_record(
            "wait-tenant-b",
            "run-tenant-b",
            tenant_b.clone(),
            StatefulWebhookWaitMatch {
                trigger_id: Some(created.trigger.trigger_id.clone()),
                ..StatefulWebhookWaitMatch::default()
            },
            1_000,
        ),
    )
    .await
    .expect("persist tenant b wait");

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let verified = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-tenant-wake".to_string()),
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
        AutomationWebhookQueueResult::Accepted { delivery, .. } => delivery,
        other => panic!("expected new run because tenant b wait is hidden, got {other:?}"),
    };
    assert!(delivery.queued_run_id.is_some());
    assert_eq!(delivery.woken_run_id, None);
    assert_eq!(delivery.woken_wait_id, None);

    let tenant_b_waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_b,
        StatefulWaitQuery {
            run_id: Some("run-tenant-b"),
            wait_kind: Some(StatefulWaitKind::Webhook),
            status: Some(StatefulWaitStatus::Waiting),
            limit: None,
        },
    );
    assert_eq!(tenant_b_waits.len(), 1);
    assert_eq!(tenant_b_waits[0].wait_id, "wait-tenant-b");
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
