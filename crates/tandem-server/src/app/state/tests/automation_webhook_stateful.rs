use super::*;
use crate::app::state::{
    automation_webhook_signature_header, AutomationWebhookQueueResult,
    AutomationWebhookTriggerCreateInput,
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
