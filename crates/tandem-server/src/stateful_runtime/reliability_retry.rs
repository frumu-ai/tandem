//! Dead-letter retry + success-supersession helpers for the reliability store.
//!
//! Split out of `reliability.rs` to keep that module under the repository's
//! per-file line-count ceiling. These functions operate on the same on-disk
//! reliability store (guarded by `STATEFUL_RELIABILITY_STORE_LOCK`) and are
//! re-exported from `stateful_runtime` alongside the rest of the reliability
//! API.

use serde_json::{Map, Value};

use super::reliability::{
    try_load_stateful_reliability, write_stateful_reliability_unlocked, StatefulDeadLetterRecord,
    StatefulDeadLetterStatus, StatefulToolEffectRecord, STATEFUL_RELIABILITY_STORE_LOCK,
};
use tandem_types::TenantContext;

/// Mark a dead letter as having a retry dispatched (TAN-564).
///
/// Unlike `mark_dead_letter_disposition` (which records an operator's terminal
/// choice), this transitions `RetryRequested` → `Retrying`, increments the
/// **dispatcher** retry counter, and stamps the dispatch time + backoff window
/// in metadata so the background dispatcher can honor exponential backoff and
/// cap the number of automatic re-drives. The dispatcher counter is tracked in
/// metadata (`retry_dispatch_count`) rather than reusing the record's `attempts`
/// field, which counts the *node/tool* execution attempts at dead-letter
/// creation and would otherwise make a dead letter born on a high node attempt
/// look pre-exhausted. It is a no-op (returns `None`) if the dead letter is
/// absent, not visible to `tenant`, or no longer in a retry-eligible state.
pub async fn mark_dead_letter_retry_dispatched(
    path: &std::path::Path,
    tenant: &TenantContext,
    dead_letter_id: &str,
    backoff_ms: u64,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulDeadLetterRecord>> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = try_load_stateful_reliability(path)?;
    let Some(row) = store.dead_letters.iter_mut().find(|row| {
        row.dead_letter_id == dead_letter_id
            && row.visible_to_tenant(tenant)
            && matches!(
                row.status,
                StatefulDeadLetterStatus::RetryRequested | StatefulDeadLetterStatus::Retrying
            )
    }) else {
        return Ok(None);
    };
    let next_dispatch_count = dead_letter_retry_dispatch_count(row).saturating_add(1);
    row.status = StatefulDeadLetterStatus::Retrying;
    row.updated_at_ms = now_ms;
    stamp_dead_letter_retry_dispatch(&mut row.metadata, next_dispatch_count, now_ms, backoff_ms);
    let updated = row.clone();
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(Some(updated))
}

fn stamp_dead_letter_retry_dispatch(
    metadata: &mut Option<Value>,
    dispatch_count: u32,
    now_ms: u64,
    backoff_ms: u64,
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
        "retry_dispatch_count".to_string(),
        Value::Number(dispatch_count.into()),
    );
    object.insert(
        "retry_dispatched_at_ms".to_string(),
        Value::Number(now_ms.into()),
    );
    object.insert(
        "retry_backoff_ms".to_string(),
        Value::Number(backoff_ms.into()),
    );
    *metadata = Some(Value::Object(object));
}

/// The last dispatch time stamped by `mark_dead_letter_retry_dispatched`, if any.
pub fn dead_letter_retry_dispatched_at_ms(record: &StatefulDeadLetterRecord) -> Option<u64> {
    record
        .metadata
        .as_ref()?
        .get("retry_dispatched_at_ms")
        .and_then(Value::as_u64)
}

/// The number of times the dispatcher has re-driven this dead letter (0 before
/// the first dispatch). Distinct from `StatefulDeadLetterRecord::attempts`,
/// which counts node/tool execution attempts at creation time.
pub fn dead_letter_retry_dispatch_count(record: &StatefulDeadLetterRecord) -> u32 {
    record
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("retry_dispatch_count"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

/// Whether a later successful replay of the dead letter's effect superseded it.
pub fn dead_letter_superseded_by_success(record: &StatefulDeadLetterRecord) -> bool {
    metadata_superseded_by_success(record.metadata.as_ref())
}

/// Stamp a reliability row's metadata to mark it superseded by a later
/// successful effect replay. Shared by the dead-letter and compensation
/// reconciliation paths in `reliability.rs`.
pub(super) fn mark_reliability_row_superseded_by_success(
    metadata: &mut Option<Value>,
    effect: &StatefulToolEffectRecord,
    outbox_id: Option<&str>,
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
    object.insert("superseded_by_success".to_string(), Value::Bool(true));
    object.insert(
        "superseded_by_effect_id".to_string(),
        Value::String(effect.effect_id.clone()),
    );
    object.insert(
        "superseded_at_ms".to_string(),
        Value::Number(effect.updated_at_ms.into()),
    );
    if let Some(outbox_id) = outbox_id {
        object.insert(
            "superseded_by_outbox_id".to_string(),
            Value::String(outbox_id.to_string()),
        );
    }
    *metadata = Some(Value::Object(object));
}

/// Whether a reliability row's metadata carries a complete
/// superseded-by-success marker (used to hide reconciled rows from active
/// recovery views).
pub(super) fn metadata_superseded_by_success(metadata: Option<&Value>) -> bool {
    let Some(metadata) = metadata else {
        return false;
    };
    let marked_success = metadata
        .get("superseded_by_success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_effect_id = metadata
        .get("superseded_by_effect_id")
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_timestamp = metadata
        .get("superseded_at_ms")
        .and_then(Value::as_u64)
        .is_some();
    marked_success && has_effect_id && has_timestamp
}
