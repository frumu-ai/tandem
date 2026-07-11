use std::path::Path;

use anyhow::Context;
use serde_json::{json, Map, Value};
use tandem_types::TenantContext;

use super::durable_io::{sideline_corrupt_state_file_sync, write_file_atomically};
use super::types::{
    StatefulRuntimeScope, StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus,
    StatefulWaitTimeoutAction, StatefulWaitTimeoutPolicy, StatefulWebhookWaitEvent,
    StatefulWebhookWaitMatch,
};

static STATEFUL_WAIT_STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const WEBHOOK_MATCH_METADATA_KEY: &str = "webhook_match";

#[derive(Debug, Clone, Default)]
pub struct StatefulWaitQuery<'a> {
    pub run_id: Option<&'a str>,
    pub wait_kind: Option<StatefulWaitKind>,
    pub status: Option<StatefulWaitStatus>,
    pub limit: Option<usize>,
}

pub fn load_stateful_waits(path: &Path) -> Vec<StatefulWaitRecord> {
    match read_stateful_waits(path, false) {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "skipping invalid stateful wait store"
            );
            Vec::new()
        }
    }
}

fn try_load_stateful_waits(path: &Path) -> anyhow::Result<Vec<StatefulWaitRecord>> {
    read_stateful_waits(path, true)
}

fn read_stateful_waits(
    path: &Path,
    sideline_corrupt: bool,
) -> anyhow::Result<Vec<StatefulWaitRecord>> {
    if let Some(store) = super::sqlite_compat::authoritative_stateful_store_for_wait_path(path)? {
        let mut rows = store.load_stateful_runtime_waits()?;
        sort_waits(&mut rows);
        return Ok(rows);
    }
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read stateful wait store {}", path.display()))
        }
    };
    let mut rows = match serde_json::from_str::<Vec<StatefulWaitRecord>>(&content) {
        Ok(rows) => rows,
        Err(error) if sideline_corrupt => {
            return Err(sideline_corrupt_state_file_sync(
                path,
                "stateful wait store",
                error,
            ));
        }
        Err(error) => {
            anyhow::bail!(
                "failed to parse stateful wait store {}: {error}",
                path.display()
            );
        }
    };
    sort_waits(&mut rows);
    Ok(rows)
}

pub fn list_stateful_waits(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulWaitQuery<'_>,
) -> Vec<StatefulWaitRecord> {
    let mut rows = load_stateful_waits(path)
        .into_iter()
        .filter(|wait| wait.visible_to_tenant(tenant))
        .filter(|wait| {
            query
                .run_id
                .map(|run_id| wait.run_id == run_id)
                .unwrap_or(true)
        })
        .filter(|wait| {
            query
                .wait_kind
                .as_ref()
                .map(|kind| wait.wait_kind == *kind)
                .unwrap_or(true)
        })
        .filter(|wait| {
            query
                .status
                .as_ref()
                .map(|status| wait.status == *status)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    apply_limit(&mut rows, query.limit);
    rows
}

pub fn due_stateful_waits(
    path: &Path,
    tenant: &TenantContext,
    now_ms: u64,
    limit: Option<usize>,
) -> Vec<StatefulWaitRecord> {
    let mut rows = load_stateful_waits(path)
        .into_iter()
        .filter(|wait| wait.visible_to_tenant(tenant))
        .filter(|wait| wait_is_claimable(wait, now_ms, now_ms))
        .collect::<Vec<_>>();
    sort_waits(&mut rows);
    apply_limit(&mut rows, limit);
    rows
}

pub(crate) fn due_stateful_waits_for_scheduler(
    path: &Path,
    now_ms: u64,
    limit: Option<usize>,
) -> Vec<StatefulWaitRecord> {
    let mut rows = load_stateful_waits(path)
        .into_iter()
        .filter(|wait| wait_is_claimable(wait, now_ms, now_ms))
        .collect::<Vec<_>>();
    sort_waits(&mut rows);
    apply_limit(&mut rows, limit);
    rows
}

pub async fn upsert_stateful_wait(
    path: &Path,
    wait: StatefulWaitRecord,
) -> anyhow::Result<StatefulWaitRecord> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    match waits
        .iter_mut()
        .find(|existing| wait_identity_matches(existing, &wait))
    {
        Some(existing) => {
            if existing.status.is_terminal() && !wait_later_settlement_is_allowed(existing, &wait) {
                return Ok(existing.clone());
            }
            *existing = wait.clone();
        }
        None => waits.push(wait.clone()),
    }
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(wait)
}

pub async fn prune_stateful_wait_store(
    path: &Path,
    retention_ms: u64,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 {
        return Ok(0);
    }

    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let original_len = waits.len();
    if original_len == 0 {
        return Ok(0);
    }

    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    let pruned_rows = waits
        .iter()
        .filter(|wait| terminal_wait_is_older_than_retention_cutoff(wait, cutoff_ms))
        .cloned()
        .collect::<Vec<_>>();
    waits.retain(|wait| !terminal_wait_is_older_than_retention_cutoff(wait, cutoff_ms));
    let pruned = original_len.saturating_sub(waits.len());
    if pruned == 0 {
        return Ok(0);
    }

    if let Some(store) = super::sqlite_compat::authoritative_stateful_store_for_wait_path(path)? {
        tokio::task::spawn_blocking(move || {
            store.delete_stateful_runtime_waits_if_unchanged(&pruned_rows)
        })
        .await
        .map_err(|error| anyhow::anyhow!("stateful wait retention task failed: {error}"))??;
    }
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(pruned)
}

pub async fn claim_due_stateful_wait(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    claimant_id: &str,
    now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    claim_due_stateful_wait_with_lease_clock(
        path,
        tenant,
        run_id,
        wait_id,
        claimant_id,
        now_ms,
        now_ms,
        lease_ms,
    )
    .await
}

pub async fn claim_due_stateful_wait_with_lease_clock(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    claimant_id: &str,
    due_now_ms: u64,
    lease_now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    claim_due_stateful_wait_matching_version_with_lease_clock(
        path,
        tenant,
        run_id,
        wait_id,
        None,
        None,
        claimant_id,
        due_now_ms,
        lease_now_ms,
        lease_ms,
    )
    .await
}

pub async fn claim_due_stateful_wait_version_with_lease_clock(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    expected_created_at_ms: u64,
    expected_updated_at_ms: u64,
    claimant_id: &str,
    due_now_ms: u64,
    lease_now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    claim_due_stateful_wait_matching_version_with_lease_clock(
        path,
        tenant,
        run_id,
        wait_id,
        Some(expected_created_at_ms),
        Some(expected_updated_at_ms),
        claimant_id,
        due_now_ms,
        lease_now_ms,
        lease_ms,
    )
    .await
}

async fn claim_due_stateful_wait_matching_version_with_lease_clock(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    expected_created_at_ms: Option<u64>,
    expected_updated_at_ms: Option<u64>,
    claimant_id: &str,
    due_now_ms: u64,
    lease_now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == run_id
            && wait.wait_id == wait_id
            && wait.visible_to_tenant(tenant)
            && expected_created_at_ms
                .map(|created_at_ms| wait.created_at_ms == created_at_ms)
                .unwrap_or(true)
            && expected_updated_at_ms
                .map(|updated_at_ms| wait.updated_at_ms == updated_at_ms)
                .unwrap_or(true)
    }) else {
        return Ok(None);
    };
    if !wait_is_claimable(wait, due_now_ms, lease_now_ms) {
        return Ok(None);
    }

    wait.status = StatefulWaitStatus::Claimed;
    wait.claimed_by = Some(claimant_id.to_string());
    wait.claimed_at_ms = Some(lease_now_ms);
    wait.claim_expires_at_ms = Some(lease_now_ms.saturating_add(lease_ms.max(1)));
    wait.wake_idempotency_key = None;
    wait.event_seq = None;
    wait.completed_at_ms = None;
    wait.updated_at_ms = lease_now_ms;
    let claimed = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(claimed))
}

pub async fn claim_matching_stateful_webhook_wait(
    path: &Path,
    tenant: &TenantContext,
    event: &StatefulWebhookWaitEvent,
    claimant_id: &str,
    now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.wait_kind == StatefulWaitKind::Webhook
            && wait.visible_to_tenant(tenant)
            && webhook_wait_is_claimable(wait, now_ms)
            && wait_matches_webhook_event(wait, event)
    }) else {
        return Ok(None);
    };

    wait.status = StatefulWaitStatus::Claimed;
    wait.claimed_by = Some(claimant_id.to_string());
    wait.claimed_at_ms = Some(now_ms);
    wait.claim_expires_at_ms = Some(now_ms.saturating_add(lease_ms.max(1)));
    wait.wake_idempotency_key = None;
    wait.event_seq = None;
    wait.completed_at_ms = None;
    wait.updated_at_ms = now_ms;
    let claimed = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(claimed))
}

pub async fn claim_stateful_wait_for_resolution(
    path: &Path,
    tenant: &TenantContext,
    wait_id: &str,
    expected_kind: StatefulWaitKind,
    claimant_id: &str,
    now_ms: u64,
    lease_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.wait_id == wait_id && wait.wait_kind == expected_kind && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status != StatefulWaitStatus::Waiting
        && !(wait.status == StatefulWaitStatus::Claimed && !wait.claim_is_active_at(now_ms))
    {
        return Ok(None);
    }

    wait.status = StatefulWaitStatus::Claimed;
    wait.claimed_by = Some(claimant_id.to_string());
    wait.claimed_at_ms = Some(now_ms);
    wait.claim_expires_at_ms = Some(now_ms.saturating_add(lease_ms.max(1)));
    wait.wake_idempotency_key = None;
    wait.event_seq = None;
    wait.completed_at_ms = None;
    wait.updated_at_ms = now_ms;
    let claimed = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(claimed))
}

pub async fn release_claimed_stateful_wait(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == claimed_wait.run_id
            && wait.wait_id == claimed_wait.wait_id
            && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if !claimed_wait_matches_current_claim(wait, claimed_wait) {
        return Ok(None);
    }

    wait.status = StatefulWaitStatus::Waiting;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    wait.wake_idempotency_key = None;
    wait.event_seq = None;
    wait.completed_at_ms = None;
    wait.updated_at_ms = now_ms;
    let released = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(released))
}

pub async fn cancel_stateful_wait_after_phase_guard_denial(
    path: &Path,
    tenant: &TenantContext,
    expected_wait: &StatefulWaitRecord,
    reason: &str,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == expected_wait.run_id
            && wait.wait_id == expected_wait.wait_id
            && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if wait.status == StatefulWaitStatus::Claimed
        && !claimed_wait_matches_current_claim(wait, expected_wait)
    {
        return Ok(None);
    }
    if !matches!(
        wait.status,
        StatefulWaitStatus::Waiting | StatefulWaitStatus::Claimed
    ) {
        return Ok(None);
    }

    wait.status = StatefulWaitStatus::Cancelled;
    wait.wake_idempotency_key = Some(format!("phase-guard-denied:{}", wait.wait_id));
    wait.event_seq = None;
    wait.completed_at_ms = Some(now_ms);
    wait.updated_at_ms = now_ms;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    wait.metadata = Some(phase_guard_denied_metadata(wait.metadata.take(), reason));
    let cancelled = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(cancelled))
}

pub async fn mark_stateful_wait_woken(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    wake_idempotency_key: &str,
    event_seq: u64,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == run_id && wait.wait_id == wait_id && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status == StatefulWaitStatus::Woken {
        return Ok(
            (wait.wake_idempotency_key.as_deref() == Some(wake_idempotency_key))
                .then(|| wait.clone()),
        );
    }
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if wait_has_active_claim(wait, now_ms) {
        return Ok(None);
    }

    wait.status = StatefulWaitStatus::Woken;
    wait.wake_idempotency_key = Some(wake_idempotency_key.to_string());
    wait.event_seq = Some(event_seq);
    wait.completed_at_ms = Some(now_ms);
    wait.updated_at_ms = now_ms;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    let woken = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(woken))
}

pub async fn mark_stateful_wait_timeout_result(
    path: &Path,
    tenant: &TenantContext,
    run_id: &str,
    wait_id: &str,
    timeout_idempotency_key: &str,
    event_seq: u64,
    status: StatefulWaitStatus,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    if !matches!(
        status,
        StatefulWaitStatus::TimedOut
            | StatefulWaitStatus::Escalated
            | StatefulWaitStatus::Cancelled
    ) {
        anyhow::bail!("invalid stateful wait timeout result status: {status:?}");
    }

    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == run_id && wait.wait_id == wait_id && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status == status {
        return Ok(
            (wait.wake_idempotency_key.as_deref() == Some(timeout_idempotency_key))
                .then(|| wait.clone()),
        );
    }
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if wait_has_active_claim(wait, now_ms) {
        return Ok(None);
    }

    wait.status = status;
    wait.wake_idempotency_key = Some(timeout_idempotency_key.to_string());
    wait.event_seq = Some(event_seq);
    wait.completed_at_ms = Some(now_ms);
    wait.updated_at_ms = now_ms;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    let completed = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(completed))
}

fn phase_guard_denied_metadata(metadata: Option<Value>, reason: &str) -> Value {
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("previous_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    object.insert("phase_guard_denied".to_string(), Value::Bool(true));
    object.insert("phase_guard_denial_reason".to_string(), json!(reason));
    Value::Object(object)
}

pub async fn begin_claimed_stateful_wait_wake_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    wake_idempotency_key: &str,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    reserve_claimed_stateful_wait_completion(
        path,
        tenant,
        claimed_wait,
        wake_idempotency_key,
        now_ms,
    )
    .await
}

pub async fn begin_claimed_stateful_wait_timeout_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    timeout_idempotency_key: &str,
    status: StatefulWaitStatus,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    if !matches!(
        status,
        StatefulWaitStatus::TimedOut
            | StatefulWaitStatus::Escalated
            | StatefulWaitStatus::Cancelled
    ) {
        anyhow::bail!("invalid stateful wait timeout result status: {status:?}");
    }
    reserve_claimed_stateful_wait_completion(
        path,
        tenant,
        claimed_wait,
        timeout_idempotency_key,
        now_ms,
    )
    .await
}

pub async fn begin_claimed_stateful_wait_reminder_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    reminder_idempotency_key: &str,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    reserve_claimed_stateful_wait_completion(
        path,
        tenant,
        claimed_wait,
        reminder_idempotency_key,
        now_ms,
    )
    .await
}

pub async fn finish_claimed_stateful_wait_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    completion_idempotency_key: &str,
    event_seq: u64,
    status: StatefulWaitStatus,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    if !status.is_terminal() {
        anyhow::bail!("invalid stateful wait completion status: {status:?}");
    }

    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == claimed_wait.run_id
            && wait.wait_id == claimed_wait.wait_id
            && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status == status {
        return Ok(
            (wait.wake_idempotency_key.as_deref() == Some(completion_idempotency_key)
                && wait.event_seq == Some(event_seq))
            .then(|| wait.clone()),
        );
    }
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if !claimed_wait_matches_current_claim(wait, claimed_wait)
        || wait.wake_idempotency_key.as_deref() != Some(completion_idempotency_key)
    {
        return Ok(None);
    }

    wait.status = status;
    wait.event_seq = Some(event_seq);
    wait.completed_at_ms = Some(now_ms);
    wait.updated_at_ms = now_ms;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    let completed = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(completed))
}

pub async fn finish_claimed_stateful_wait_reminder_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    reminder_idempotency_key: &str,
    event_seq: u64,
    next_timeout_at_ms: u64,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == claimed_wait.run_id
            && wait.wait_id == claimed_wait.wait_id
            && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if wait.status == StatefulWaitStatus::Waiting {
        return Ok(
            (wait.wake_idempotency_key.as_deref() == Some(reminder_idempotency_key)
                && wait.event_seq == Some(event_seq))
            .then(|| wait.clone()),
        );
    }
    if wait.status.is_terminal() {
        return Ok(None);
    }
    if !claimed_wait_matches_current_claim(wait, claimed_wait)
        || wait.wake_idempotency_key.as_deref() != Some(reminder_idempotency_key)
    {
        return Ok(None);
    }
    let Some(timeout_policy) = wait.timeout_policy.as_mut() else {
        return Ok(None);
    };

    timeout_policy.timeout_at_ms = next_timeout_at_ms;
    timeout_policy.metadata = Some(reminder_timeout_metadata(
        timeout_policy.metadata.take(),
        now_ms,
        next_timeout_at_ms,
    ));
    wait.status = StatefulWaitStatus::Waiting;
    wait.event_seq = Some(event_seq);
    wait.completed_at_ms = None;
    wait.updated_at_ms = now_ms;
    wait.claimed_by = None;
    wait.claimed_at_ms = None;
    wait.claim_expires_at_ms = None;
    let reminded = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(reminded))
}

fn reminder_timeout_metadata(
    metadata: Option<Value>,
    reminded_at_ms: u64,
    next_reminder_at_ms: u64,
) -> Value {
    let mut object = match metadata {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("previous_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    let reminder_count = object
        .get("reminder_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    object.insert("reminder_count".to_string(), json!(reminder_count));
    object.insert("last_reminded_at_ms".to_string(), json!(reminded_at_ms));
    object.insert(
        "next_reminder_at_ms".to_string(),
        json!(next_reminder_at_ms),
    );
    Value::Object(object)
}

async fn reserve_claimed_stateful_wait_completion(
    path: &Path,
    tenant: &TenantContext,
    claimed_wait: &StatefulWaitRecord,
    completion_idempotency_key: &str,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulWaitRecord>> {
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == claimed_wait.run_id
            && wait.wait_id == claimed_wait.wait_id
            && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };

    if wait.status.is_terminal() {
        return Ok(
            (wait.wake_idempotency_key.as_deref() == Some(completion_idempotency_key))
                .then(|| wait.clone()),
        );
    }
    if !claimed_wait_matches_active_wait(wait, claimed_wait, now_ms) {
        return Ok(None);
    }

    wait.wake_idempotency_key = Some(completion_idempotency_key.to_string());
    wait.event_seq = None;
    wait.completed_at_ms = None;
    wait.updated_at_ms = now_ms;
    let reserved = wait.clone();
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(Some(reserved))
}

fn claimed_wait_matches_active_wait(
    wait: &StatefulWaitRecord,
    claimed_wait: &StatefulWaitRecord,
    now_ms: u64,
) -> bool {
    claimed_wait_matches_current_claim(wait, claimed_wait) && wait.claim_is_active_at(now_ms)
}

fn claimed_wait_matches_current_claim(
    wait: &StatefulWaitRecord,
    claimed_wait: &StatefulWaitRecord,
) -> bool {
    wait.status == StatefulWaitStatus::Claimed
        && claimed_wait.status == StatefulWaitStatus::Claimed
        && wait.claimed_by == claimed_wait.claimed_by
        && wait.claimed_at_ms == claimed_wait.claimed_at_ms
        && wait.claim_expires_at_ms == claimed_wait.claim_expires_at_ms
}

fn wait_has_active_claim(wait: &StatefulWaitRecord, now_ms: u64) -> bool {
    wait.status == StatefulWaitStatus::Claimed && wait.claim_is_active_at(now_ms)
}

pub fn stateful_webhook_wait_metadata(
    match_rules: StatefulWebhookWaitMatch,
    extra_metadata: Option<Value>,
) -> Value {
    let match_value = serde_json::to_value(match_rules).unwrap_or(Value::Null);
    match extra_metadata {
        Some(Value::Object(mut metadata)) => {
            metadata.insert(WEBHOOK_MATCH_METADATA_KEY.to_string(), match_value);
            Value::Object(metadata)
        }
        Some(value) => json!({
            WEBHOOK_MATCH_METADATA_KEY: match_value,
            "extra": value,
        }),
        None => json!({
            WEBHOOK_MATCH_METADATA_KEY: match_value,
        }),
    }
}

pub fn stateful_webhook_wait_match_from_metadata(
    metadata: Option<&Value>,
) -> Option<StatefulWebhookWaitMatch> {
    metadata
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(WEBHOOK_MATCH_METADATA_KEY))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

/// Whether `event` satisfies `wait`'s webhook match rules. `pub` (rather than
/// private to this module) so callers that need to match a webhook event
/// against a *specific* wait — e.g. TAN-571's replay-on-registration, which
/// checks a single newly-registered wait against historical deliveries
/// rather than scanning the whole wait store — can reuse this exact logic
/// instead of re-implementing it.
pub fn wait_matches_webhook_event(
    wait: &StatefulWaitRecord,
    event: &StatefulWebhookWaitEvent,
) -> bool {
    let Some(match_rules) = stateful_webhook_wait_match_from_metadata(wait.metadata.as_ref())
    else {
        return false;
    };
    if !match_rules.has_constraint() {
        return false;
    }
    optional_match(
        match_rules.trigger_id.as_deref(),
        Some(event.trigger_id.as_str()),
    ) && optional_match(
        match_rules.provider.as_deref(),
        Some(event.provider.as_str()),
    ) && optional_match(
        match_rules.provider_event_kind.as_deref(),
        event.provider_event_kind.as_deref(),
    ) && optional_match(
        match_rules.provider_event_id.as_deref(),
        event.provider_event_id.as_deref(),
    ) && optional_match(
        match_rules.body_digest.as_deref(),
        Some(event.body_digest.as_str()),
    ) && optional_match(
        match_rules.idempotency_key.as_deref(),
        Some(event.idempotency_key.as_str()),
    )
}

fn optional_match(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected
        .map(|expected| actual == Some(expected))
        .unwrap_or(true)
}

fn wait_is_claimable(wait: &StatefulWaitRecord, due_now_ms: u64, lease_now_ms: u64) -> bool {
    let due = wait_wake_is_due_at(wait, due_now_ms) || wait_timeout_is_due_at(wait, due_now_ms);
    if wait.status == StatefulWaitStatus::Waiting {
        return due;
    }
    wait.status == StatefulWaitStatus::Claimed && !wait.claim_is_active_at(lease_now_ms) && due
}

fn wait_later_settlement_is_allowed(
    existing: &StatefulWaitRecord,
    next: &StatefulWaitRecord,
) -> bool {
    existing.wait_kind == StatefulWaitKind::Approval
        && next.wait_kind == StatefulWaitKind::Approval
        && matches!(
            existing.status,
            StatefulWaitStatus::TimedOut | StatefulWaitStatus::Escalated
        )
        && matches!(
            next.status,
            StatefulWaitStatus::Woken | StatefulWaitStatus::Cancelled
        )
}

fn webhook_wait_is_claimable(wait: &StatefulWaitRecord, now_ms: u64) -> bool {
    wait.status == StatefulWaitStatus::Waiting
        || (wait.status == StatefulWaitStatus::Claimed && !wait.claim_is_active_at(now_ms))
}

fn wait_timeout_is_due_at(wait: &StatefulWaitRecord, now_ms: u64) -> bool {
    wait.timeout_policy
        .as_ref()
        .map(|policy| policy.timeout_at_ms <= now_ms)
        .unwrap_or(false)
}

fn wait_wake_is_due_at(wait: &StatefulWaitRecord, now_ms: u64) -> bool {
    wait.wake_at_ms
        .map(|wake_at_ms| wake_at_ms <= now_ms)
        .unwrap_or(false)
}

fn terminal_wait_is_older_than_retention_cutoff(wait: &StatefulWaitRecord, cutoff_ms: u64) -> bool {
    wait.status.is_terminal() && wait.completed_at_ms.unwrap_or(wait.updated_at_ms) < cutoff_ms
}

fn wait_identity_matches(left: &StatefulWaitRecord, right: &StatefulWaitRecord) -> bool {
    left.wait_id == right.wait_id
        && left.run_id == right.run_id
        && same_tenant_boundary(&left.scope, &right.scope)
}

fn same_tenant_boundary(left: &StatefulRuntimeScope, right: &StatefulRuntimeScope) -> bool {
    left.tenant_context.org_id == right.tenant_context.org_id
        && left.tenant_context.workspace_id == right.tenant_context.workspace_id
        && left.tenant_context.deployment_id == right.tenant_context.deployment_id
}

fn apply_limit(rows: &mut Vec<StatefulWaitRecord>, limit: Option<usize>) {
    if let Some(limit) = limit.filter(|limit| *limit > 0) {
        if rows.len() > limit {
            rows.truncate(limit);
        }
    }
}

fn sort_waits(rows: &mut [StatefulWaitRecord]) {
    rows.sort_by(|left, right| {
        wait_sort_at_ms(left)
            .cmp(&wait_sort_at_ms(right))
            .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
            .then_with(|| left.wait_id.cmp(&right.wait_id))
    });
}

fn wait_sort_at_ms(wait: &StatefulWaitRecord) -> u64 {
    match (
        wait.wake_at_ms,
        wait.timeout_policy
            .as_ref()
            .map(|policy| policy.timeout_at_ms),
    ) {
        (Some(wake_at_ms), Some(timeout_at_ms)) => wake_at_ms.min(timeout_at_ms),
        (Some(wake_at_ms), None) => wake_at_ms,
        (None, Some(timeout_at_ms)) => timeout_at_ms,
        (None, None) => u64::MAX,
    }
}

async fn write_stateful_waits_unlocked(
    path: &Path,
    waits: &[StatefulWaitRecord],
) -> anyhow::Result<()> {
    let mut sorted = waits.to_vec();
    sort_waits(&mut sorted);
    if let Some(store) = super::sqlite_compat::authoritative_stateful_store_for_wait_path(path)? {
        let records = sorted.clone();
        tokio::task::spawn_blocking(move || store.upsert_stateful_runtime_waits(&records))
            .await
            .map_err(|error| anyhow::anyhow!("stateful wait store task failed: {error}"))??;
    }
    let content = serde_json::to_vec_pretty(&sorted)?;
    write_file_atomically(path, &content, "stateful wait store").await
}

#[cfg(test)]
#[path = "waits_tests.rs"]
mod tests;
