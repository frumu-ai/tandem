use super::*;
use crate::app::state::{
    automation_webhook_body_digest, automation_webhook_signature_header,
    AutomationWebhookQueueResult, AutomationWebhookRawEventCreateInput,
    AutomationWebhookTriggerCreateInput, AutomationWebhookTriggerUpdateInput,
};
use crate::stateful_runtime::{
    list_stateful_waits, phase_state_from_status, stateful_webhook_wait_metadata,
    upsert_stateful_wait, write_stateful_run_snapshot, StatefulRunSnapshotRecord,
    StatefulRuntimeScope, StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWaitQuery,
    StatefulWaitRecord, StatefulWaitStatus, StatefulWebhookWaitMatch, StatefulWorkflowRunKind,
    StatefulWorkflowRunStatus,
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
async fn webhook_phase_denied_wait_completes_idempotency_without_new_run() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-stateful-phase-denied", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-stateful-phase-denied",
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
            Some("evt-phase-denied".to_string()),
            now,
            300_000,
        )
        .await
        .expect("verified request");
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let phase_state = phase_state_from_status(
        "run-phase-denied",
        &StatefulWorkflowRunStatus::Completed,
        now.saturating_sub(1_000),
        Some("phase-completed"),
    );
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-phase-denied".to_string(),
            run_id: "run-phase-denied".to_string(),
            seq: 7,
            created_at_ms: now.saturating_sub(1_000),
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            status: StatefulWorkflowRunStatus::Completed,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-completed".to_string()),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        },
    )
    .await
    .expect("write completed snapshot");
    upsert_stateful_wait(
        &paths.waits_path,
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-phase-denied".to_string(),
            run_id: "run-phase-denied".to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            phase_id: None,
            reason: Some("awaiting webhook".to_string()),
            created_at_ms: now.saturating_sub(500),
            updated_at_ms: now.saturating_sub(500),
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
                    provider_event_id: Some("evt-phase-denied".to_string()),
                    ..StatefulWebhookWaitMatch::default()
                },
                None,
            )),
        },
    )
    .await
    .expect("insert webhook wait");

    let delivery = match state
        .queue_automation_v2_run_from_webhook_delivery(verified.clone(), json!({"ok": true}))
        .await
        .expect("phase-denied webhook outcome")
    {
        AutomationWebhookQueueResult::Rejected {
            delivery,
            reason_code,
        } => {
            assert_eq!(reason_code, "stateful_wait_phase_denied");
            delivery
        }
        other => panic!("expected phase-denied rejection, got {other:?}"),
    };
    assert_eq!(delivery.status, AutomationWebhookDeliveryStatus::Rejected);
    assert_eq!(
        delivery.rejection_reason_code.as_deref(),
        Some("stateful_wait_phase_denied")
    );
    assert_eq!(
        delivery.dedupe_result,
        Some(AutomationWebhookDedupeResult::Accepted)
    );
    assert!(state.automation_v2_runs.read().await.is_empty());
    let waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_a,
        StatefulWaitQuery {
            run_id: Some("run-phase-denied"),
            ..StatefulWaitQuery::default()
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].status, StatefulWaitStatus::Cancelled);

    let retry_now = now + 1;
    let retry_signature = automation_webhook_signature_header(&created.secret, retry_now, body);
    let retry = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&retry_signature),
            body,
            Some("evt-phase-denied".to_string()),
            retry_now,
            300_000,
        )
        .await
        .expect("retry verifies");
    let duplicate = match state
        .queue_automation_v2_run_from_webhook_delivery(retry, json!({"ok": true}))
        .await
        .expect("duplicate retry outcome")
    {
        AutomationWebhookQueueResult::Duplicate { delivery } => delivery,
        other => panic!("expected duplicate retry, got {other:?}"),
    };
    assert_eq!(
        duplicate.duplicate_of_delivery_id.as_deref(),
        Some(delivery.delivery_id.as_str())
    );
    assert!(state.automation_v2_runs.read().await.is_empty());
}

#[tokio::test]
async fn duplicate_webhook_redelivery_wakes_late_registered_wait() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-stateful-late-wait", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-stateful-late-wait",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"ok":true}"#;
    let now = now_ms();
    let signature = automation_webhook_signature_header(&created.secret, now, body);
    let early = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&signature),
            body,
            Some("evt-late-wait".to_string()),
            now,
            300_000,
        )
        .await
        .expect("early request verifies");
    let early_delivery = match state
        .queue_automation_v2_run_from_webhook_delivery(early, json!({"ok": true}))
        .await
        .expect("early webhook accepted")
    {
        AutomationWebhookQueueResult::Accepted { delivery, .. } => delivery,
        other => panic!("expected accepted early webhook, got {other:?}"),
    };
    assert!(early_delivery.queued_run_id.is_some());

    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let wait_run_id = "run-late-webhook-wait";
    let phase_state = phase_state_from_status(
        wait_run_id,
        &StatefulWorkflowRunStatus::Running,
        now,
        Some("phase-wait"),
    );
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-late-webhook-wait".to_string(),
            run_id: wait_run_id.to_string(),
            seq: 3,
            created_at_ms: now,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            status: StatefulWorkflowRunStatus::Running,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-wait".to_string()),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        },
    )
    .await
    .expect("write running snapshot");
    upsert_stateful_wait(
        &paths.waits_path,
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-late-webhook".to_string(),
            run_id: wait_run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            phase_id: Some("phase-wait".to_string()),
            reason: Some("awaiting correlated webhook".to_string()),
            created_at_ms: now.saturating_add(1),
            updated_at_ms: now.saturating_add(1),
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
                    provider_event_id: Some("evt-late-wait".to_string()),
                    ..StatefulWebhookWaitMatch::default()
                },
                None,
            )),
        },
    )
    .await
    .expect("insert late webhook wait");

    let retry_now = now + 2;
    let retry_signature = automation_webhook_signature_header(&created.secret, retry_now, body);
    let retry = state
        .verify_automation_webhook_request(
            &created.trigger.public_path_token,
            Some(&retry_signature),
            body,
            Some("evt-late-wait".to_string()),
            retry_now,
            300_000,
        )
        .await
        .expect("retry verifies");
    let (delivery, wait) = match state
        .queue_automation_v2_run_from_webhook_delivery(retry, json!({"ok": true}))
        .await
        .expect("redelivery wakes late wait")
    {
        AutomationWebhookQueueResult::Woken { delivery, wait } => (delivery, wait),
        other => panic!("expected redelivery to wake wait, got {other:?}"),
    };
    assert_eq!(delivery.woken_run_id.as_deref(), Some(wait_run_id));
    assert_eq!(delivery.woken_wait_id.as_deref(), Some("wait-late-webhook"));
    assert_eq!(wait.status, StatefulWaitStatus::Woken);
    assert_eq!(state.automation_v2_runs.read().await.len(), 1);
}

#[tokio::test]
async fn buffered_webhook_wake_uses_drain_time_for_late_wait_bookkeeping() {
    let state = ready_test_state().await;
    let tenant_a = tenant("org-a", "workspace-a");
    insert_test_automation(&state, "automation-stateful-buffered-wait", &tenant_a).await;
    let created = state
        .create_automation_webhook_trigger(create_input(
            "automation-stateful-buffered-wait",
            tenant_a.clone(),
        ))
        .await
        .expect("create webhook trigger");

    let body = br#"{"buffered":true}"#;
    let wait_created_at = now_ms();
    let receipt_at = wait_created_at.saturating_sub(60_000);
    let raw_event = state
        .record_automation_webhook_raw_event(AutomationWebhookRawEventCreateInput {
            trigger: created.trigger.clone(),
            provider_event_id: Some("evt-buffered-late-wait".to_string()),
            body_digest: automation_webhook_body_digest(body),
            verification: None,
            feedback_loop_candidate: None,
            headers_digest: "headers-digest".to_string(),
            headers_redacted: json!({"x-tandem-webhook-event-id": "evt-buffered-late-wait"}),
            content_type: Some("application/json".to_string()),
            payload: body.to_vec(),
            received_at_ms: receipt_at,
        })
        .await
        .expect("record buffered raw event");

    state
        .update_automation_webhook_trigger(
            &tenant_a,
            "automation-stateful-buffered-wait",
            &created.trigger.trigger_id,
            AutomationWebhookTriggerUpdateInput {
                provider: Some("linear".to_string()),
                provider_event_kind: Some(Some("issue.updated".to_string())),
                ..AutomationWebhookTriggerUpdateInput::default()
            },
            Some("actor-a".to_string()),
        )
        .await
        .expect("update trigger after receipt");
    let latest_trigger = state
        .get_automation_webhook_trigger(&tenant_a, &created.trigger.trigger_id)
        .await
        .expect("load updated trigger");
    assert_eq!(latest_trigger.provider, "linear");

    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let wait_run_id = "run-buffered-late-webhook-wait";
    let phase_state = phase_state_from_status(
        wait_run_id,
        &StatefulWorkflowRunStatus::Running,
        wait_created_at,
        Some("phase-buffered-wait"),
    );
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-buffered-late-webhook-wait".to_string(),
            run_id: wait_run_id.to_string(),
            seq: 3,
            created_at_ms: wait_created_at,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            status: StatefulWorkflowRunStatus::Running,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-buffered-wait".to_string()),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        },
    )
    .await
    .expect("write running snapshot");
    upsert_stateful_wait(
        &paths.waits_path,
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-buffered-late-webhook".to_string(),
            run_id: wait_run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
            phase_id: Some("phase-buffered-wait".to_string()),
            reason: Some("awaiting buffered webhook".to_string()),
            created_at_ms: wait_created_at,
            updated_at_ms: wait_created_at,
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
                    provider_event_id: Some("evt-buffered-late-wait".to_string()),
                    ..StatefulWebhookWaitMatch::default()
                },
                None,
            )),
        },
    )
    .await
    .expect("insert late webhook wait");

    let report = state.process_automation_webhook_inbox_once(10).await;
    assert_eq!(report.checked, 1);
    assert_eq!(report.processed, 1);
    assert_eq!(report.failed, 0);

    let updated_event = state
        .get_automation_webhook_raw_event(&tenant_a, &raw_event.event_id)
        .await
        .expect("load raw event")
        .expect("raw event exists");
    assert_eq!(
        updated_event.status,
        AutomationWebhookDeliveryStatus::Accepted
    );
    let delivery_id = updated_event
        .delivery_id
        .as_deref()
        .expect("raw event delivery id");
    let delivery = state
        .get_automation_webhook_delivery(&tenant_a, delivery_id)
        .await
        .expect("delivery exists");
    assert_eq!(delivery.received_at_ms, receipt_at);
    assert_eq!(delivery.accepted_at_ms, Some(receipt_at));
    assert_eq!(delivery.verification_provider.as_deref(), Some("generic"));
    assert_eq!(
        delivery.woken_wait_id.as_deref(),
        Some("wait-buffered-late-webhook")
    );

    let waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_a,
        StatefulWaitQuery {
            run_id: Some(wait_run_id),
            wait_kind: Some(StatefulWaitKind::Webhook),
            ..StatefulWaitQuery::default()
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].status, StatefulWaitStatus::Woken);
    assert!(waits[0].updated_at_ms >= wait_created_at);
    assert!(waits[0].completed_at_ms.unwrap_or_default() >= wait_created_at);
}
