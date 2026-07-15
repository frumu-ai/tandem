// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Value};
use uuid::Uuid;

use super::*;

fn tenant(org: &str, workspace: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
}

fn action(action_id: &str, status: &str, error: Option<&str>) -> ExternalActionRecord {
    ExternalActionRecord {
        action_id: action_id.to_string(),
        operation: "mock_external_action.send".to_string(),
        status: status.to_string(),
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("run-a:node-a:1:0".to_string()),
        routine_run_id: None,
        context_run_id: Some("automation-v2-run-a".to_string()),
        capability_id: Some("mock_external_action.send".to_string()),
        provider: Some("mock".to_string()),
        target: Some("customer-outbox".to_string()),
        approval_state: Some("executed".to_string()),
        idempotency_key: Some(format!("idempotency-{action_id}")),
        receipt: Some(json!({
            "result": {"status": "ok"},
            "authorization": "Bearer abc",
            "nested": {"api_key": "secret-value"}
        })),
        error: error.map(str::to_string),
        metadata: Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "tool": "SendMessage",
            "input": {"message": "hello"}
        })),
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
    }
}

fn compensation_metadata(attempt: u64) -> Value {
    json!({
        "automationRunID": "run-a",
        "nodeID": "node-a",
        "attempt": attempt,
        "compensation": {
            "type": "operator_review",
            "approval_required": true,
            "rollback_instruction": "remove the posted message"
        }
    })
}

fn superseded_metadata(effect_id: &str) -> Value {
    json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": effect_id,
        "superseded_at_ms": 9_000,
    })
}

fn dead_letter_record(
    scope: StatefulRuntimeScope,
    run_id: &str,
    index: usize,
) -> StatefulDeadLetterRecord {
    StatefulDeadLetterRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        dead_letter_id: format!("dead-letter-{index:04}"),
        source_type: "tool_effect".to_string(),
        source_id: format!("effect-{index:04}"),
        run_id: Some(run_id.to_string()),
        scope,
        reason: "provider timeout".to_string(),
        status: StatefulDeadLetterStatus::Open,
        recovery_options: vec![StatefulRecoveryOption::Retry],
        payload_pointer: None,
        compensation_id: None,
        attempts: 1,
        created_at_ms: index as u64,
        updated_at_ms: index as u64,
        operator_disposition: None,
        disposition_reason: None,
        disposition_actor: None,
        disposition_at_ms: None,
        metadata: None,
    }
}

fn compensation_record(
    scope: StatefulRuntimeScope,
    run_id: &str,
    index: usize,
) -> StatefulCompensationRecord {
    StatefulCompensationRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        compensation_id: format!("compensation-{index:04}"),
        run_id: Some(run_id.to_string()),
        scope,
        status: StatefulCompensationStatus::AwaitingApproval,
        compensation_type: "operator_review".to_string(),
        target_effect_id: Some(format!("effect-{index:04}")),
        outbox_id: Some(format!("outbox-{index:04}")),
        approval_required: true,
        policy_decision_id: None,
        rollback_instruction: Some("remove the posted message".to_string()),
        forward_fix_instruction: None,
        receipt_effect_id: None,
        attempts: 0,
        created_at_ms: index as u64,
        updated_at_ms: index as u64,
        metadata: None,
    }
}

#[tokio::test]
async fn external_action_bridge_records_outbox_and_redacted_effect() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
    let effect = record_external_action_reliability_bridge(
        &path,
        scope,
        &action("action-a", "posted", None),
    )
    .await
    .expect("bridge");

    assert_eq!(effect.status, StatefulToolEffectStatus::Succeeded);
    let store = load_stateful_reliability(&path);
    assert_eq!(store.outbox.len(), 1);
    assert_eq!(store.tool_effects.len(), 1);
    assert_eq!(store.dead_letters.len(), 0);
    let receipt = store.tool_effects[0]
        .receipt_payload_redacted
        .as_ref()
        .expect("receipt");
    assert_eq!(receipt["authorization"], "[redacted]");
    assert_eq!(receipt["nested"]["api_key"], "[redacted]");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reliability_mutations_sideline_corrupt_store_instead_of_overwriting() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-corrupt-{}.json",
        Uuid::new_v4()
    ));
    std::fs::write(&path, "{not-valid-json").expect("write corrupt reliability store");
    let corrupt_path = path.with_extension("json.corrupt");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));

    let result = record_external_action_reliability_bridge(
        &path,
        scope,
        &action("action-corrupt", "posted", None),
    )
    .await;

    let error = result.expect_err("corrupt store should block mutation");
    assert!(error.to_string().contains("corrupt store moved"));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&corrupt_path).expect("read corrupt reliability store"),
        "{not-valid-json"
    );
    let _ = std::fs::remove_file(corrupt_path);
}

#[tokio::test]
async fn external_action_bridge_preserves_context_only_run_id() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let mut record = action("action-context-only", "posted", None);
    record.context_run_id = Some("automation-v2-run-context-only".to_string());
    record.metadata = Some(json!({
        "nodeID": "node-a",
        "attempt": 1,
        "tool": "SendMessage",
        "input": {"message": "hello"}
    }));

    let effect = record_external_action_reliability_bridge(&path, scope, &record)
        .await
        .expect("bridge");

    assert_eq!(effect.run_id.as_deref(), Some("run-context-only"));
    let effects = list_stateful_tool_effects(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            run_id: Some("run-context-only"),
            ..Default::default()
        },
    );
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].action_id.as_deref(), Some("action-context-only"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn external_action_bridge_dedupes_effects_by_idempotency_key() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
    let mut first = action("action-replay-first", "posted", None);
    first.idempotency_key = Some("idem-run-a-node-a-send".to_string());
    let mut replay = action("action-replay-second", "posted", None);
    replay.idempotency_key = first.idempotency_key.clone();
    replay.updated_at_ms = 3_000;
    replay.receipt = Some(json!({
        "result": {"status": "already_sent"},
        "secret": "must be redacted"
    }));

    let first_effect = record_external_action_reliability_bridge(&path, scope.clone(), &first)
        .await
        .expect("first bridge");
    let replay_effect = record_external_action_reliability_bridge(&path, scope, &replay)
        .await
        .expect("replay bridge");

    assert_eq!(first_effect.effect_id, replay_effect.effect_id);
    let store = load_stateful_reliability(&path);
    assert_eq!(store.outbox.len(), 1);
    assert_eq!(store.tool_effects.len(), 1);
    assert_eq!(store.dead_letters.len(), 0);
    assert_eq!(
        store.outbox[0].effect_id.as_deref(),
        Some(replay_effect.effect_id.as_str())
    );
    assert_eq!(
        store.tool_effects[0].action_id.as_deref(),
        Some("action-replay-second")
    );
    assert_eq!(
        store.tool_effects[0]
            .receipt_payload_redacted
            .as_ref()
            .and_then(|receipt| receipt.get("secret"))
            .and_then(Value::as_str),
        Some("[redacted]")
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn external_action_success_replay_clears_stale_failure_recovery_rows() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
    let mut failed = action("action-replay-failed", "failed", Some("provider timeout"));
    failed.idempotency_key = Some("idem-run-a-node-a-send".to_string());
    failed.metadata = Some(compensation_metadata(1));
    let mut succeeded = action("action-replay-succeeded", "posted", None);
    succeeded.idempotency_key = failed.idempotency_key.clone();
    succeeded.metadata = Some(compensation_metadata(2));
    succeeded.updated_at_ms = 3_000;
    succeeded.receipt = Some(json!({
        "result": {"status": "posted"}
    }));

    record_external_action_reliability_bridge(&path, scope.clone(), &failed)
        .await
        .expect("failed bridge");
    let failed_store = load_stateful_reliability(&path);
    assert_eq!(failed_store.tool_effects.len(), 1);
    assert_eq!(failed_store.dead_letters.len(), 1);
    assert_eq!(failed_store.compensations.len(), 1);

    let replay_effect = record_external_action_reliability_bridge(&path, scope, &succeeded)
        .await
        .expect("success bridge");

    assert_eq!(replay_effect.status, StatefulToolEffectStatus::Succeeded);
    assert!(replay_effect.compensation_id.is_none());
    let store = load_stateful_reliability(&path);
    assert_eq!(store.outbox.len(), 1);
    assert_eq!(store.tool_effects.len(), 1);
    assert_eq!(store.dead_letters.len(), 0);
    assert_eq!(store.compensations.len(), 0);
    assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Sent);
    assert!(store.outbox[0].dead_letter_id.is_none());
    assert!(store.outbox[0].compensation_id.is_none());
    assert_eq!(
        store.tool_effects[0].action_id.as_deref(),
        Some("action-replay-succeeded")
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn external_action_success_replay_preserves_operator_recovery_rows() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let mut failed = action(
        "action-replay-operator-failed",
        "failed",
        Some("provider timeout"),
    );
    failed.idempotency_key = Some("idem-run-a-node-a-operator".to_string());
    failed.metadata = Some(compensation_metadata(1));
    let mut succeeded = action("action-replay-operator-succeeded", "posted", None);
    succeeded.idempotency_key = failed.idempotency_key.clone();
    succeeded.metadata = failed.metadata.clone();
    succeeded.updated_at_ms = 7_000;

    record_external_action_reliability_bridge(&path, scope.clone(), &failed)
        .await
        .expect("failed bridge");
    let failed_store = load_stateful_reliability(&path);
    let dead_letter_id = failed_store.dead_letters[0].dead_letter_id.clone();
    let compensation_id = failed_store.compensations[0].compensation_id.clone();

    execute_stateful_compensation(
        &path,
        &tenant_a,
        &compensation_id,
        operator_principal(Some("operator-a")),
        Some("compensation completed".to_string()),
        4_000,
    )
    .await
    .expect("execute compensation")
    .expect("compensation execution");
    mark_dead_letter_disposition(
        &path,
        &tenant_a,
        &dead_letter_id,
        StatefulDeadLetterStatus::LinkedToCompensation,
        "linked_to_compensation",
        Some("compensation completed".to_string()),
        operator_principal(Some("operator-a")),
        5_000,
    )
    .await
    .expect("mark dead letter disposition");

    let replay_effect = record_external_action_reliability_bridge(&path, scope, &succeeded)
        .await
        .expect("success bridge");

    let store = load_stateful_reliability(&path);
    assert_eq!(
        store.dead_letters[0].status,
        StatefulDeadLetterStatus::LinkedToCompensation
    );
    assert_eq!(
        store.compensations[0].status,
        StatefulCompensationStatus::Completed
    );
    assert_eq!(
        store.compensations[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("superseded_by_effect_id"))
            .and_then(Value::as_str),
        Some(replay_effect.effect_id.as_str())
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn external_action_unknown_replay_preserves_operator_recovery_rows() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let mut failed = action(
        "action-replay-unknown-failed",
        "failed",
        Some("provider timeout"),
    );
    failed.idempotency_key = Some("idem-run-a-node-a-unknown".to_string());
    failed.metadata = Some(compensation_metadata(1));
    let mut unknown = action("action-replay-unknown", "provider_acknowledged", None);
    unknown.idempotency_key = failed.idempotency_key.clone();
    unknown.metadata = failed.metadata.clone();
    unknown.updated_at_ms = 7_000;

    record_external_action_reliability_bridge(&path, scope.clone(), &failed)
        .await
        .expect("failed bridge");
    let failed_store = load_stateful_reliability(&path);
    let dead_letter_id = failed_store.dead_letters[0].dead_letter_id.clone();
    let compensation_id = failed_store.compensations[0].compensation_id.clone();

    mark_compensation_status(
        &path,
        &tenant_a,
        &compensation_id,
        StatefulCompensationStatus::AwaitingApproval,
        4_000,
    )
    .await
    .expect("mark compensation awaiting approval");
    mark_dead_letter_disposition(
        &path,
        &tenant_a,
        &dead_letter_id,
        StatefulDeadLetterStatus::RetryRequested,
        "retry_requested",
        Some("retry after provider recovers".to_string()),
        operator_principal(Some("operator-a")),
        5_000,
    )
    .await
    .expect("mark dead letter disposition");

    let replay_effect = record_external_action_reliability_bridge(&path, scope, &unknown)
        .await
        .expect("unknown bridge");

    assert_eq!(replay_effect.status, StatefulToolEffectStatus::Unknown);
    let store = load_stateful_reliability(&path);
    assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Pending);
    assert_eq!(
        store.dead_letters[0].status,
        StatefulDeadLetterStatus::RetryRequested
    );
    assert_eq!(
        store.compensations[0].status,
        StatefulCompensationStatus::AwaitingApproval
    );
    assert_eq!(
        store.dead_letters[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("superseded_by_success"))
            .and_then(Value::as_bool),
        None
    );
    assert_eq!(
        store.compensations[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("superseded_by_success"))
            .and_then(Value::as_bool),
        None
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn reliability_lists_page_beyond_default_limit() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let mut store = default_stateful_reliability_store();
    store.dead_letters = (0..1_050)
        .map(|index| dead_letter_record(scope.clone(), "run-a", index))
        .collect();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&store).expect("serialize reliability store"),
    )
    .expect("write reliability store");

    let first_page = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            limit: Some(300),
            ..Default::default()
        },
    );
    assert_eq!(first_page.len(), 300);
    assert_eq!(first_page[0].dead_letter_id, "dead-letter-1049");
    assert_eq!(first_page[299].dead_letter_id, "dead-letter-0750");

    let capped_page = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            limit: Some(1_500),
            ..Default::default()
        },
    );
    assert_eq!(capped_page.len(), 1_000);

    let cursor_page = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            before_created_at_ms: Some(750),
            limit: Some(5),
            ..Default::default()
        },
    );
    assert_eq!(cursor_page[0].dead_letter_id, "dead-letter-0749");
    let after_id = cursor_page[2].dead_letter_id.as_str();
    let before_created_at_ms = cursor_page[2].created_at_ms;

    let after_page = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            after_id: Some(after_id),
            before_created_at_ms: Some(before_created_at_ms),
            limit: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(
        after_page
            .iter()
            .map(|row| row.dead_letter_id.as_str())
            .collect::<Vec<_>>(),
        vec!["dead-letter-0746", "dead-letter-0745"]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn active_recovery_lists_filter_superseded_rows_before_limit() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let mut store = default_stateful_reliability_store();
    store.dead_letters = vec![
        dead_letter_record(scope.clone(), "run-a", 1),
        dead_letter_record(scope.clone(), "run-a", 3),
        dead_letter_record(scope.clone(), "run-a", 4),
    ];
    store.compensations = vec![
        compensation_record(scope.clone(), "run-a", 1),
        compensation_record(scope.clone(), "run-a", 2),
        compensation_record(scope.clone(), "run-a", 3),
    ];
    for row in store.dead_letters.iter_mut().skip(1) {
        row.metadata = Some(superseded_metadata("effect-replayed"));
    }
    for row in store.compensations.iter_mut().skip(1) {
        row.metadata = Some(superseded_metadata("effect-replayed"));
    }
    let mut user_metadata_dead_letter = dead_letter_record(scope.clone(), "run-a", 2);
    user_metadata_dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "policy": "user-supplied"
    }));
    store.dead_letters.push(user_metadata_dead_letter);
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&store).expect("serialize reliability store"),
    )
    .expect("write reliability store");

    let unfiltered = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            limit: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(unfiltered[0].dead_letter_id, "dead-letter-0004");

    let active_dead_letters = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            active_recovery_only: true,
            limit: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(active_dead_letters.len(), 1);
    assert_eq!(active_dead_letters[0].dead_letter_id, "dead-letter-0002");
    let active_compensations = list_stateful_compensations(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            active_recovery_only: true,
            limit: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(active_compensations.len(), 1);
    assert_eq!(active_compensations[0].compensation_id, "compensation-0001");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn failed_external_action_bridge_creates_tenant_filtered_dead_letter() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    record_external_action_reliability_bridge(
        &path,
        scope,
        &action("action-b", "failed", Some("provider timeout")),
    )
    .await
    .expect("bridge");

    let visible = list_stateful_dead_letters(
        &path,
        &tenant_a,
        StatefulReliabilityQuery {
            run_id: Some("run-a"),
            ..Default::default()
        },
    );
    let hidden = list_stateful_dead_letters(
        &path,
        &tenant_b,
        StatefulReliabilityQuery {
            run_id: Some("run-a"),
            ..Default::default()
        },
    );
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].reason, "provider timeout");
    assert!(hidden.is_empty());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn failed_external_action_bridge_links_default_compensation_to_dead_letter() {
    let path = std::env::temp_dir().join(format!(
        "tandem-stateful-reliability-{}.json",
        Uuid::new_v4()
    ));
    let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
    let mut record = action("action-compensation", "failed", Some("provider timeout"));
    record.metadata = Some(json!({
        "automationRunID": "run-a",
        "nodeID": "node-a",
        "attempt": 2,
        "tool": "SendMessage",
        "input": {"message": "hello"},
        "compensation_policy": {
            "approval_required": true,
            "rollback_instruction": "remove the posted message"
        }
    }));

    let effect = record_external_action_reliability_bridge(&path, scope, &record)
        .await
        .expect("bridge");

    let store = load_stateful_reliability(&path);
    assert_eq!(store.compensations.len(), 1);
    assert_eq!(store.dead_letters.len(), 1);
    let compensation_id = format!("compensation-{}", effect.effect_id);
    assert_eq!(
        effect.compensation_id.as_deref(),
        Some(compensation_id.as_str())
    );
    assert_eq!(store.compensations[0].compensation_id, compensation_id);
    assert_eq!(store.compensations[0].compensation_type, "operator_review");
    assert_eq!(
        store.dead_letters[0].compensation_id.as_deref(),
        Some(store.compensations[0].compensation_id.as_str())
    );
    let _ = std::fs::remove_file(path);
}
