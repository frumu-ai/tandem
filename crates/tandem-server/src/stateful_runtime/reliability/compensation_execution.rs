use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};
use tandem_types::{PrincipalRef, TenantContext};

use super::*;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StatefulCompensationExecutionResult {
    pub compensation: StatefulCompensationRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_effect: Option<StatefulToolEffectRecord>,
    #[serde(default)]
    pub linked_dead_letters: Vec<StatefulDeadLetterRecord>,
    pub already_completed: bool,
}

pub async fn execute_stateful_compensation(
    path: &Path,
    tenant: &TenantContext,
    compensation_id: &str,
    actor: PrincipalRef,
    reason: Option<String>,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulCompensationExecutionResult>> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = try_load_stateful_reliability(path)?;
    let Some(index) = store
        .compensations
        .iter()
        .position(|row| row.compensation_id == compensation_id && row.visible_to_tenant(tenant))
    else {
        return Ok(None);
    };

    if store.compensations[index].status == StatefulCompensationStatus::Cancelled {
        anyhow::bail!("stateful compensation `{compensation_id}` is cancelled");
    }

    if store.compensations[index].status == StatefulCompensationStatus::Completed {
        let compensation = store.compensations[index].clone();
        let receipt_effect =
            compensation
                .receipt_effect_id
                .as_deref()
                .and_then(|receipt_effect_id| {
                    store
                        .tool_effects
                        .iter()
                        .find(|effect| effect.effect_id == receipt_effect_id)
                        .cloned()
                });
        let linked_dead_letters = linked_compensation_dead_letters(&store, tenant, compensation_id);
        return Ok(Some(StatefulCompensationExecutionResult {
            compensation,
            receipt_effect,
            linked_dead_letters,
            already_completed: true,
        }));
    }

    let previous_status = store.compensations[index].status.clone();
    store.compensations[index].status = StatefulCompensationStatus::Running;
    store.compensations[index].attempts = store.compensations[index].attempts.saturating_add(1);
    store.compensations[index].updated_at_ms = now_ms;
    mark_compensation_execution_metadata(
        &mut store.compensations[index].metadata,
        "running",
        &previous_status,
        None,
        &actor,
        reason.as_deref(),
        now_ms,
    );
    let running = store.compensations[index].clone();
    let claimed_outbox =
        compensation_outbox_record(&running, StatefulOutboxStatus::Claimed, now_ms);
    upsert_by(&mut store.outbox, claimed_outbox, |row| &row.outbox_id);
    write_stateful_reliability_unlocked(path, &store).await?;

    let running = store.compensations[index].clone();
    let mut sent_outbox = compensation_outbox_record(&running, StatefulOutboxStatus::Sent, now_ms);
    let receipt_effect = compensation_receipt_effect(
        &running,
        sent_outbox.outbox_id.as_str(),
        &actor,
        reason.as_deref(),
        now_ms,
    );
    sent_outbox.effect_id = Some(receipt_effect.effect_id.clone());
    sent_outbox.receipt_id = Some(receipt_effect.effect_id.clone());
    sent_outbox.claim_expires_at_ms = None;
    mark_compensation_outbox_completed(&mut sent_outbox.metadata, &receipt_effect, now_ms);
    upsert_by(&mut store.outbox, sent_outbox, |row| &row.outbox_id);
    upsert_by(&mut store.tool_effects, receipt_effect.clone(), |row| {
        &row.effect_id
    });

    store.compensations[index].status = StatefulCompensationStatus::Completed;
    store.compensations[index].receipt_effect_id = Some(receipt_effect.effect_id.clone());
    store.compensations[index].updated_at_ms = now_ms;
    mark_compensation_execution_metadata(
        &mut store.compensations[index].metadata,
        "completed",
        &StatefulCompensationStatus::Running,
        Some(receipt_effect.effect_id.as_str()),
        &actor,
        reason.as_deref(),
        now_ms,
    );

    let mut linked_dead_letters = Vec::new();
    for row in store.dead_letters.iter_mut().filter(|row| {
        row.compensation_id.as_deref() == Some(compensation_id) && row.visible_to_tenant(tenant)
    }) {
        if matches!(
            row.status,
            StatefulDeadLetterStatus::Open | StatefulDeadLetterStatus::RetryRequested
        ) {
            row.status = StatefulDeadLetterStatus::LinkedToCompensation;
            row.operator_disposition = Some("linked_to_compensation".to_string());
            row.disposition_reason = reason.clone().or_else(|| {
                Some(format!(
                    "compensation `{compensation_id}` completed by stateful runtime"
                ))
            });
            row.disposition_actor = Some(actor.clone());
            row.disposition_at_ms = Some(now_ms);
            row.updated_at_ms = now_ms;
        }
        linked_dead_letters.push(row.clone());
    }

    let compensation = store.compensations[index].clone();
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(Some(StatefulCompensationExecutionResult {
        compensation,
        receipt_effect: Some(receipt_effect),
        linked_dead_letters,
        already_completed: false,
    }))
}

fn linked_compensation_dead_letters(
    store: &StatefulReliabilityStoreFile,
    tenant: &TenantContext,
    compensation_id: &str,
) -> Vec<StatefulDeadLetterRecord> {
    store
        .dead_letters
        .iter()
        .filter(|row| {
            row.compensation_id.as_deref() == Some(compensation_id) && row.visible_to_tenant(tenant)
        })
        .cloned()
        .collect()
}

pub(super) fn compensation_status_transition_allowed(
    from: &StatefulCompensationStatus,
    to: &StatefulCompensationStatus,
) -> bool {
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (
            StatefulCompensationStatus::Proposed,
            StatefulCompensationStatus::AwaitingApproval
                | StatefulCompensationStatus::Approved
                | StatefulCompensationStatus::Cancelled
        ) | (
            StatefulCompensationStatus::AwaitingApproval,
            StatefulCompensationStatus::Approved | StatefulCompensationStatus::Cancelled
        ) | (
            StatefulCompensationStatus::Approved,
            StatefulCompensationStatus::Running | StatefulCompensationStatus::Cancelled
        ) | (
            StatefulCompensationStatus::Running,
            StatefulCompensationStatus::Completed
                | StatefulCompensationStatus::Failed
                | StatefulCompensationStatus::Cancelled
        ) | (
            StatefulCompensationStatus::Failed,
            StatefulCompensationStatus::Approved
                | StatefulCompensationStatus::Running
                | StatefulCompensationStatus::Cancelled
        )
    )
}

fn compensation_outbox_record(
    compensation: &StatefulCompensationRecord,
    status: StatefulOutboxStatus,
    now_ms: u64,
) -> StatefulOutboxRecord {
    let outbox_id = compensation_execution_outbox_id(compensation);
    let claim_expires_at_ms =
        (status == StatefulOutboxStatus::Claimed).then_some(now_ms.saturating_add(5 * 60 * 1000));
    let idempotency_key = format!(
        "stateful-compensation:{}:{}",
        compensation.compensation_id, compensation.attempts
    );
    StatefulOutboxRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox_id,
        run_id: compensation.run_id.clone(),
        scope: compensation.scope.clone(),
        operation: format!("stateful_compensation.{}", compensation.compensation_type),
        status,
        source_kind: Some("stateful_compensation".to_string()),
        source_id: Some(compensation.compensation_id.clone()),
        node_id: None,
        provider: Some("stateful_runtime".to_string()),
        tool: Some("stateful_compensation_engine".to_string()),
        target: compensation.target_effect_id.clone(),
        idempotency_key: Some(idempotency_key),
        payload_digest: compensation.metadata.as_ref().and_then(digest_value),
        policy_decision_id: compensation.policy_decision_id.clone(),
        context_assertion_id: None,
        effect_id: None,
        receipt_id: None,
        compensation_id: Some(compensation.compensation_id.clone()),
        dead_letter_id: None,
        attempts: compensation.attempts,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        claimed_by: Some("stateful_compensation_engine".to_string()),
        claimed_at_ms: Some(now_ms),
        claim_expires_at_ms,
        metadata: Some(json!({
            "execution_engine": "stateful_compensation_engine",
            "observed_before_execution": true,
            "original_outbox_id": compensation.outbox_id,
            "target_effect_id": compensation.target_effect_id,
            "rollback_instruction": compensation.rollback_instruction,
            "forward_fix_instruction": compensation.forward_fix_instruction,
        })),
    }
}

fn compensation_execution_outbox_id(compensation: &StatefulCompensationRecord) -> String {
    let digest = crate::sha256_hex(&[
        &compensation.compensation_id,
        compensation.run_id.as_deref().unwrap_or(""),
        &compensation.attempts.to_string(),
        "outbox",
    ]);
    format!("outbox-compensation-{}", short_hash(&digest))
}

fn mark_compensation_outbox_completed(
    metadata: &mut Option<Value>,
    receipt_effect: &StatefulToolEffectRecord,
    now_ms: u64,
) {
    let mut object = match metadata.take() {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("previous_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    object.insert("observed_after_execution".to_string(), Value::Bool(true));
    object.insert("dispatch_completed".to_string(), Value::Bool(true));
    object.insert(
        "receipt_effect_id".to_string(),
        Value::String(receipt_effect.effect_id.clone()),
    );
    object.insert(
        "completion_recorded_at_ms".to_string(),
        Value::Number(now_ms.into()),
    );
    *metadata = Some(Value::Object(object));
}

fn compensation_receipt_effect(
    compensation: &StatefulCompensationRecord,
    receipt_outbox_id: &str,
    actor: &PrincipalRef,
    reason: Option<&str>,
    now_ms: u64,
) -> StatefulToolEffectRecord {
    let receipt_payload = json!({
        "status": "completed",
        "compensation_id": compensation.compensation_id,
        "compensation_type": compensation.compensation_type,
        "target_effect_id": compensation.target_effect_id,
        "outbox_id": compensation.outbox_id,
        "rollback_instruction": compensation.rollback_instruction,
        "forward_fix_instruction": compensation.forward_fix_instruction,
        "attempt": compensation.attempts,
        "actor": actor,
        "reason": reason,
    });
    let receipt_payload_digest = digest_value(&receipt_payload);
    let effect_id = compensation_receipt_effect_id(compensation);
    let operation = format!("stateful_compensation.{}", compensation.compensation_type);
    let audit_hash = crate::sha256_hex(&[
        &effect_id,
        &compensation.compensation_id,
        &operation,
        receipt_payload_digest.as_deref().unwrap_or(""),
    ]);

    StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id: effect_id.clone(),
        outbox_id: Some(receipt_outbox_id.to_string()),
        action_id: Some(format!(
            "stateful-compensation:{}",
            compensation.compensation_id
        )),
        run_id: compensation.run_id.clone(),
        scope: compensation.scope.clone(),
        status: StatefulToolEffectStatus::Succeeded,
        operation,
        source_kind: Some("stateful_compensation".to_string()),
        source_id: Some(compensation.compensation_id.clone()),
        node_id: None,
        provider: Some("stateful_runtime".to_string()),
        tool: Some("stateful_compensation_engine".to_string()),
        target: compensation.target_effect_id.clone(),
        external_resource: Some(json!({
            "target_effect_id": compensation.target_effect_id,
            "original_outbox_id": compensation.outbox_id,
            "receipt_outbox_id": receipt_outbox_id,
        })),
        policy_decision_id: compensation.policy_decision_id.clone(),
        context_assertion_id: None,
        input_digest: compensation.metadata.as_ref().and_then(digest_value),
        output_digest: receipt_payload_digest.clone(),
        receipt_payload_digest,
        receipt_payload_redacted: Some(redact_value(&receipt_payload)),
        receipt_pointer: Some(format!(
            "stateful-compensation://{}",
            compensation.compensation_id
        )),
        redaction_tier: "safe_ui".to_string(),
        audit_hash,
        error: None,
        compensation_id: Some(compensation.compensation_id.clone()),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        metadata: Some(json!({
            "execution_engine": "stateful_compensation_engine",
            "approval_required": compensation.approval_required,
            "approved_by": actor,
        })),
    }
}

fn compensation_receipt_effect_id(compensation: &StatefulCompensationRecord) -> String {
    let digest = crate::sha256_hex(&[
        &compensation.compensation_id,
        compensation.run_id.as_deref().unwrap_or(""),
        &compensation.attempts.to_string(),
    ]);
    format!("effect-compensation-{}", short_hash(&digest))
}

fn mark_compensation_execution_metadata(
    metadata: &mut Option<Value>,
    stage: &str,
    previous_status: &StatefulCompensationStatus,
    receipt_effect_id: Option<&str>,
    actor: &PrincipalRef,
    reason: Option<&str>,
    now_ms: u64,
) {
    let mut object = match metadata.take() {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("previous_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    object.insert(
        "execution_engine".to_string(),
        Value::String("stateful_compensation_engine".to_string()),
    );
    object.insert(
        "execution_stage".to_string(),
        Value::String(stage.to_string()),
    );
    object.insert(
        "previous_status".to_string(),
        serde_json::to_value(previous_status).unwrap_or(Value::Null),
    );
    object.insert("executed_at_ms".to_string(), Value::Number(now_ms.into()));
    object.insert(
        "executed_by".to_string(),
        serde_json::to_value(actor).unwrap_or(Value::Null),
    );
    if let Some(reason) = reason {
        object.insert(
            "execution_reason".to_string(),
            Value::String(reason.to_string()),
        );
    }
    if let Some(receipt_effect_id) = receipt_effect_id {
        object.insert(
            "receipt_effect_id".to_string(),
            Value::String(receipt_effect_id.to_string()),
        );
    }
    *metadata = Some(Value::Object(object));
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use uuid::Uuid;

    use crate::routines::types::ExternalActionRecord;

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

    #[tokio::test]
    async fn execute_stateful_compensation_completes_with_receipt_and_links_dead_letter() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
        let mut record = action(
            "action-execute-compensation",
            "failed",
            Some("provider timeout"),
        );
        record.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "tool": "SendMessage",
            "input": {"message": "hello"},
            "compensation": {
                "type": "operator_review",
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));
        record_external_action_reliability_bridge(&path, scope, &record)
            .await
            .expect("bridge");
        let store = load_stateful_reliability(&path);
        let compensation_id = store.compensations[0].compensation_id.clone();

        let execution = execute_stateful_compensation(
            &path,
            &tenant_a,
            &compensation_id,
            operator_principal(Some("operator-a")),
            Some("rollback approved".to_string()),
            4_000,
        )
        .await
        .expect("execute compensation")
        .expect("compensation execution");

        assert!(!execution.already_completed);
        assert_eq!(
            execution.compensation.status,
            StatefulCompensationStatus::Completed
        );
        let receipt_effect = execution.receipt_effect.expect("receipt effect");
        assert_eq!(receipt_effect.status, StatefulToolEffectStatus::Succeeded);
        assert_eq!(
            receipt_effect.source_kind.as_deref(),
            Some("stateful_compensation")
        );
        assert_eq!(
            receipt_effect.compensation_id.as_deref(),
            Some(compensation_id.as_str())
        );
        assert_eq!(execution.linked_dead_letters.len(), 1);
        assert_eq!(
            execution.linked_dead_letters[0].status,
            StatefulDeadLetterStatus::LinkedToCompensation
        );

        let store = load_stateful_reliability(&path);
        assert_eq!(store.outbox.len(), 2);
        let compensation_outbox = store
            .outbox
            .iter()
            .find(|row| row.compensation_id.as_deref() == Some(compensation_id.as_str()))
            .expect("compensation outbox");
        assert_eq!(compensation_outbox.status, StatefulOutboxStatus::Sent);
        assert_eq!(
            compensation_outbox.effect_id.as_deref(),
            Some(receipt_effect.effect_id.as_str())
        );
        assert_eq!(store.tool_effects.len(), 2);
        assert_eq!(
            store.compensations[0].receipt_effect_id.as_deref(),
            Some(receipt_effect.effect_id.as_str())
        );
        assert_eq!(store.compensations[0].attempts, 1);
        assert_eq!(
            store.compensations[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("execution_stage"))
                .and_then(Value::as_str),
            Some("completed")
        );

        let active_compensations = list_stateful_compensations(
            &path,
            &tenant_a,
            StatefulReliabilityQuery {
                run_id: Some("run-a"),
                active_recovery_only: true,
                ..Default::default()
            },
        );
        assert!(active_compensations.is_empty());

        let idempotent = execute_stateful_compensation(
            &path,
            &tenant_a,
            &compensation_id,
            operator_principal(Some("operator-a")),
            Some("already done".to_string()),
            5_000,
        )
        .await
        .expect("execute completed compensation")
        .expect("completed compensation");
        assert!(idempotent.already_completed);
        assert_eq!(load_stateful_reliability(&path).tool_effects.len(), 2);
        let _ = std::fs::remove_file(path);
    }
    #[tokio::test]
    async fn operator_recovery_updates_are_tenant_scoped() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
        let mut record = action(
            "action-tenant-compensation",
            "failed",
            Some("provider timeout"),
        );
        record.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "tool": "SendMessage",
            "input": {"message": "hello"},
            "compensation": {
                "type": "operator_review",
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));
        record_external_action_reliability_bridge(&path, scope, &record)
            .await
            .expect("bridge");
        let store = load_stateful_reliability(&path);
        let compensation_id = store.compensations[0].compensation_id.clone();
        let dead_letter_id = store.dead_letters[0].dead_letter_id.clone();

        let wrong_compensation = mark_compensation_status(
            &path,
            &tenant_b,
            &compensation_id,
            StatefulCompensationStatus::Completed,
            3_000,
        )
        .await
        .expect("wrong tenant compensation update");
        assert!(wrong_compensation.is_none());

        let illegal_completion = mark_compensation_status(
            &path,
            &tenant_a,
            &compensation_id,
            StatefulCompensationStatus::Completed,
            4_000,
        )
        .await
        .expect_err("direct proposed-to-completed transition should fail");
        assert!(illegal_completion
            .to_string()
            .contains("illegal stateful compensation status transition"));

        let updated_compensation = mark_compensation_status(
            &path,
            &tenant_a,
            &compensation_id,
            StatefulCompensationStatus::AwaitingApproval,
            4_500,
        )
        .await
        .expect("tenant compensation update")
        .expect("updated compensation");
        assert_eq!(
            updated_compensation.status,
            StatefulCompensationStatus::AwaitingApproval
        );

        let wrong_dead_letter = mark_dead_letter_disposition(
            &path,
            &tenant_b,
            &dead_letter_id,
            StatefulDeadLetterStatus::LinkedToCompensation,
            "linked_to_compensation",
            Some("wrong tenant".to_string()),
            operator_principal(Some("operator-b")),
            5_000,
        )
        .await
        .expect("wrong tenant dead letter update");
        assert!(wrong_dead_letter.is_none());

        let updated_dead_letter = mark_dead_letter_disposition(
            &path,
            &tenant_a,
            &dead_letter_id,
            StatefulDeadLetterStatus::LinkedToCompensation,
            "linked_to_compensation",
            Some("compensation completed".to_string()),
            operator_principal(Some("operator-a")),
            6_000,
        )
        .await
        .expect("tenant dead letter update")
        .expect("updated dead letter");
        assert_eq!(
            updated_dead_letter.status,
            StatefulDeadLetterStatus::LinkedToCompensation
        );
        assert_eq!(
            updated_dead_letter.operator_disposition.as_deref(),
            Some("linked_to_compensation")
        );
        let _ = std::fs::remove_file(path);
    }
}
