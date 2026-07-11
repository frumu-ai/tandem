use std::path::Path;

use super::reliability::{
    try_load_stateful_reliability, write_stateful_reliability_unlocked, StatefulCompensationRecord,
    StatefulCompensationStatus, StatefulDeadLetterRecord, StatefulDeadLetterStatus,
    StatefulOutboxRecord, StatefulOutboxStatus, StatefulToolEffectRecord, StatefulToolEffectStatus,
    STATEFUL_RELIABILITY_STORE_LOCK,
};

/// Removes only settled reliability records older than the configured retention
/// cutoff. Records still useful for recovery remain durable indefinitely.
pub async fn prune_stateful_reliability_store(
    path: &Path,
    retention_ms: u64,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 {
        return Ok(0);
    }
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = try_load_stateful_reliability(path)?;
    let before = record_count(&store);
    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    store
        .outbox
        .retain(|record| !settled_outbox_before(record, cutoff_ms));
    store
        .tool_effects
        .retain(|record| !settled_effect_before(record, cutoff_ms));
    store
        .dead_letters
        .retain(|record| !settled_dead_letter_before(record, cutoff_ms));
    store
        .compensations
        .retain(|record| !settled_compensation_before(record, cutoff_ms));
    let pruned = before.saturating_sub(record_count(&store));
    if pruned > 0 {
        write_stateful_reliability_unlocked(path, &store).await?;
    }
    Ok(pruned)
}

fn record_count(store: &super::reliability::StatefulReliabilityStoreFile) -> usize {
    store.outbox.len()
        + store.tool_effects.len()
        + store.dead_letters.len()
        + store.compensations.len()
}

fn settled_outbox_before(record: &StatefulOutboxRecord, cutoff_ms: u64) -> bool {
    matches!(
        record.status,
        StatefulOutboxStatus::Sent | StatefulOutboxStatus::Cancelled
    ) && record.updated_at_ms < cutoff_ms
}

fn settled_effect_before(record: &StatefulToolEffectRecord, cutoff_ms: u64) -> bool {
    record.status == StatefulToolEffectStatus::Succeeded && record.updated_at_ms < cutoff_ms
}

fn settled_dead_letter_before(record: &StatefulDeadLetterRecord, cutoff_ms: u64) -> bool {
    matches!(
        record.status,
        StatefulDeadLetterStatus::Ignored | StatefulDeadLetterStatus::Resolved
    ) && record.updated_at_ms < cutoff_ms
}

fn settled_compensation_before(record: &StatefulCompensationRecord, cutoff_ms: u64) -> bool {
    matches!(
        record.status,
        StatefulCompensationStatus::Completed | StatefulCompensationStatus::Cancelled
    ) && record.updated_at_ms < cutoff_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stateful_runtime::{
        load_stateful_reliability, upsert_stateful_outbox, StatefulOutboxRecord,
        StatefulRuntimeScope,
    };
    use tandem_types::TenantContext;

    #[tokio::test]
    async fn retains_recovery_records_and_prunes_settled_outbox() {
        let path = std::env::temp_dir().join(format!(
            "stateful-reliability-retention-{}.json",
            uuid::Uuid::new_v4()
        ));
        let scope = StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit());
        for (outbox_id, status) in [
            ("settled", StatefulOutboxStatus::Sent),
            ("recoverable", StatefulOutboxStatus::Failed),
        ] {
            upsert_stateful_outbox(
                &path,
                StatefulOutboxRecord {
                    schema_version: 1,
                    outbox_id: outbox_id.to_string(),
                    run_id: None,
                    scope: scope.clone(),
                    operation: "test".to_string(),
                    status,
                    source_kind: None,
                    source_id: None,
                    node_id: None,
                    provider: None,
                    tool: None,
                    target: None,
                    idempotency_key: None,
                    payload_digest: None,
                    policy_decision_id: None,
                    context_assertion_id: None,
                    effect_id: None,
                    receipt_id: None,
                    compensation_id: None,
                    dead_letter_id: None,
                    attempts: 1,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    claimed_by: None,
                    claimed_at_ms: None,
                    claim_expires_at_ms: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();
        }

        assert_eq!(
            prune_stateful_reliability_store(&path, 5, 10)
                .await
                .unwrap(),
            1
        );
        let store = load_stateful_reliability(&path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].outbox_id, "recoverable");
        let _ = tokio::fs::remove_file(path).await;
    }
}
