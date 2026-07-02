use std::path::Path;

use anyhow::Context;
use serde_json::{json, Value};
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
        .filter(|wait| wait_is_claimable(wait, now_ms))
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
        Some(existing) => *existing = wait.clone(),
        None => waits.push(wait.clone()),
    }
    write_stateful_waits_unlocked(path, &waits).await?;
    Ok(wait)
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
    let _guard = STATEFUL_WAIT_STORE_LOCK.lock().await;
    let mut waits = try_load_stateful_waits(path)?;
    let Some(wait) = waits.iter_mut().find(|wait| {
        wait.run_id == run_id && wait.wait_id == wait_id && wait.visible_to_tenant(tenant)
    }) else {
        return Ok(None);
    };
    if !wait_is_claimable(wait, now_ms) {
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

fn wait_matches_webhook_event(wait: &StatefulWaitRecord, event: &StatefulWebhookWaitEvent) -> bool {
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

fn wait_is_claimable(wait: &StatefulWaitRecord, now_ms: u64) -> bool {
    let due = wait_wake_is_due_at(wait, now_ms) || wait_timeout_is_due_at(wait, now_ms);
    if wait.status == StatefulWaitStatus::Waiting {
        return due;
    }
    wait.status == StatefulWaitStatus::Claimed && !wait.claim_is_active_at(now_ms) && due
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
    let content = serde_json::to_vec_pretty(&sorted)?;
    write_file_atomically(path, &content, "stateful wait store").await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use tandem_types::TenantContext;
    use uuid::Uuid;

    use super::*;

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn timer_wait(
        wait_id: &str,
        run_id: &str,
        tenant_context: TenantContext,
        wake_at_ms: u64,
    ) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: wait_id.to_string(),
            run_id: run_id.to_string(),
            wait_kind: StatefulWaitKind::Timer,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            phase_id: Some("phase-a".to_string()),
            reason: Some("sleep until downstream system is ready".to_string()),
            created_at_ms: wake_at_ms.saturating_sub(100),
            updated_at_ms: wake_at_ms.saturating_sub(100),
            wake_at_ms: Some(wake_at_ms),
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: Some(json!({ "source": "test" })),
        }
    }

    fn webhook_wait(
        wait_id: &str,
        run_id: &str,
        tenant_context: TenantContext,
        match_rules: StatefulWebhookWaitMatch,
    ) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: wait_id.to_string(),
            run_id: run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            phase_id: Some("phase-webhook".to_string()),
            reason: Some("wait for correlated webhook".to_string()),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
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

    fn webhook_event(
        trigger_id: &str,
        provider_event_id: Option<&str>,
    ) -> StatefulWebhookWaitEvent {
        StatefulWebhookWaitEvent {
            trigger_id: trigger_id.to_string(),
            provider: "github".to_string(),
            provider_event_kind: Some("issues.opened".to_string()),
            provider_event_id: provider_event_id.map(ToOwned::to_owned),
            body_digest: "sha256:body".to_string(),
            idempotency_key: provider_event_id
                .map(|event_id| format!("github:{event_id}"))
                .unwrap_or_else(|| "sha256:body".to_string()),
        }
    }

    fn temp_wait_store(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}.json", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn wait_store_round_trips_and_filters_by_tenant() {
        let path = temp_wait_store("stateful-waits-filtered");
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert wait-a");
        upsert_stateful_wait(&path, timer_wait("wait-b", "run-a", tenant_b.clone(), 900))
            .await
            .expect("insert wait-b");

        let visible = list_stateful_waits(
            &path,
            &tenant_a,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        );

        assert_eq!(
            visible
                .iter()
                .map(|wait| wait.wait_id.as_str())
                .collect::<Vec<_>>(),
            vec!["wait-a"]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn wait_mutations_sideline_corrupt_store_instead_of_overwriting() {
        let path = temp_wait_store("stateful-waits-corrupt");
        std::fs::write(&path, "{not-valid-json").expect("write corrupt wait store");
        let corrupt_path = path.with_extension("json.corrupt");

        let result = upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant("org-a", "workspace-a"), 1_000),
        )
        .await;

        let error = result.expect_err("corrupt store should block mutation");
        assert!(error.to_string().contains("corrupt store moved"));
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(&corrupt_path).expect("read corrupt wait store"),
            "{not-valid-json"
        );
        let _ = tokio::fs::remove_file(corrupt_path).await;
    }

    #[tokio::test]
    async fn duplicate_wait_ids_are_scoped_by_tenant_boundary() {
        let path = temp_wait_store("stateful-waits-tenant-boundary");
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        upsert_stateful_wait(
            &path,
            timer_wait("shared-wait", "run-a", tenant_b.clone(), 900),
        )
        .await
        .expect("insert tenant-b wait");
        upsert_stateful_wait(
            &path,
            timer_wait("shared-wait", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert tenant-a wait");

        let all_waits = load_stateful_waits(&path);
        assert_eq!(all_waits.len(), 2);
        let tenant_a_due = due_stateful_waits(&path, &tenant_a, 1_500, None);
        assert_eq!(tenant_a_due.len(), 1);
        assert_eq!(tenant_a_due[0].scope.organization_id(), "org-a");

        let claimed = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "shared-wait",
            "scheduler-a",
            1_500,
            500,
        )
        .await
        .expect("tenant-a claim")
        .expect("tenant-a wait");
        assert_eq!(claimed.scope.organization_id(), "org-a");
        let tenant_b_waits = list_stateful_waits(
            &path,
            &tenant_b,
            StatefulWaitQuery {
                status: Some(StatefulWaitStatus::Waiting),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(tenant_b_waits.len(), 1);
        assert_eq!(tenant_b_waits[0].scope.organization_id(), "org-b");
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn duplicate_wait_ids_are_claimed_by_run_identity() {
        let path = temp_wait_store("stateful-waits-run-boundary");
        let tenant_a = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &path,
            timer_wait("shared-wait", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert run-a wait");
        upsert_stateful_wait(
            &path,
            timer_wait("shared-wait", "run-b", tenant_a.clone(), 1_100),
        )
        .await
        .expect("insert run-b wait");

        let run_a = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "shared-wait",
            "worker-a",
            1_500,
            500,
        )
        .await
        .expect("claim run-a")
        .expect("run-a wait");
        assert_eq!(run_a.run_id, "run-a");

        let run_b = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-b",
            "shared-wait",
            "worker-b",
            1_600,
            500,
        )
        .await
        .expect("claim run-b")
        .expect("run-b wait");
        assert_eq!(run_b.run_id, "run-b");
        let missing = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-c",
            "shared-wait",
            "worker-c",
            1_700,
            500,
        )
        .await
        .expect("claim missing run");
        assert!(missing.is_none());
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn due_waits_select_missed_timer_wakeups_in_order() {
        let path = temp_wait_store("stateful-waits-due");
        let tenant_a = tenant("org-a", "workspace-a");
        for (wait_id, wake_at_ms) in [("future", 2_000), ("oldest", 500), ("due", 1_000)] {
            upsert_stateful_wait(
                &path,
                timer_wait(wait_id, "run-a", tenant_a.clone(), wake_at_ms),
            )
            .await
            .expect("insert wait");
        }

        let due = due_stateful_waits(&path, &tenant_a, 1_500, Some(10));

        assert_eq!(
            due.iter()
                .map(|wait| wait.wait_id.as_str())
                .collect::<Vec<_>>(),
            vec!["oldest", "due"]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn due_wait_claim_is_single_claimant_until_lease_expires() {
        let path = temp_wait_store("stateful-waits-claim");
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert wait");

        assert!(claim_due_stateful_wait(
            &path,
            &tenant_b,
            "run-a",
            "wait-a",
            "scheduler-b",
            1_500,
            500
        )
        .await
        .expect("cross-tenant claim")
        .is_none());
        let claimed = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-a",
            1_500,
            500,
        )
        .await
        .expect("first claim")
        .expect("claim record");
        assert_eq!(claimed.claimed_by.as_deref(), Some("scheduler-a"));
        assert!(claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-b",
            1_600,
            500
        )
        .await
        .expect("second claim")
        .is_none());
        assert!(due_stateful_waits(&path, &tenant_a, 1_600, None).is_empty());

        let expired_claims = due_stateful_waits(&path, &tenant_a, 2_100, None);
        assert_eq!(
            expired_claims
                .iter()
                .map(|wait| wait.wait_id.as_str())
                .collect::<Vec<_>>(),
            vec!["wait-a"]
        );

        let reclaimed = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-b",
            2_100,
            500,
        )
        .await
        .expect("reclaim")
        .expect("reclaimed record");
        assert_eq!(reclaimed.claimed_by.as_deref(), Some("scheduler-b"));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[test]
    fn expired_claimed_timer_wait_without_timeout_remains_claimable() {
        let tenant_a = tenant("org-a", "workspace-a");
        let mut wait = timer_wait("wait-a", "run-a", tenant_a, 1_000);
        wait.status = StatefulWaitStatus::Claimed;
        wait.claimed_by = Some("scheduler-a".to_string());
        wait.claimed_at_ms = Some(1_500);
        wait.claim_expires_at_ms = Some(2_000);

        assert!(!wait_is_claimable(&wait, 1_999));
        assert!(wait_is_claimable(&wait, 2_000));
    }

    #[tokio::test]
    async fn wake_completion_is_idempotent_and_terminal() {
        let path = temp_wait_store("stateful-waits-woken");
        let tenant_a = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert wait");

        let woken =
            mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_600)
                .await
                .expect("mark woken")
                .expect("woken record");
        assert_eq!(woken.status, StatefulWaitStatus::Woken);
        assert_eq!(woken.event_seq, Some(42));

        let duplicate =
            mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_700)
                .await
                .expect("duplicate wake")
                .expect("duplicate record");
        assert_eq!(duplicate.completed_at_ms, Some(1_600));
        assert!(mark_stateful_wait_woken(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "other-key",
            43,
            1_800
        )
        .await
        .expect("conflicting wake")
        .is_none());
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn direct_completion_does_not_override_active_claim() {
        let path = temp_wait_store("stateful-waits-active-claim-completion");
        let tenant_a = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert wait");
        claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-a",
            1_500,
            500,
        )
        .await
        .expect("claim wait")
        .expect("claimed wait");

        assert!(mark_stateful_wait_woken(
            &path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_600
        )
        .await
        .expect("direct wake completion")
        .is_none());
        assert!(mark_stateful_wait_timeout_result(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "timeout-key",
            43,
            StatefulWaitStatus::TimedOut,
            1_700
        )
        .await
        .expect("direct timeout completion")
        .is_none());

        let wait = list_stateful_waits(
            &path,
            &tenant_a,
            StatefulWaitQuery {
                run_id: Some("run-a"),
                ..StatefulWaitQuery::default()
            },
        )
        .into_iter()
        .next()
        .expect("wait remains");
        assert_eq!(wait.status, StatefulWaitStatus::Claimed);
        assert_eq!(wait.claimed_by.as_deref(), Some("scheduler-a"));
        assert_eq!(wait.event_seq, None);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn claimed_wait_completion_requires_matching_active_claim() {
        let path = temp_wait_store("stateful-waits-claimed-completion");
        let tenant_a = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &path,
            timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
        )
        .await
        .expect("insert wait");
        let claimed = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-a",
            1_500,
            500,
        )
        .await
        .expect("claim wait")
        .expect("claimed record");

        let mut stale_claimant = claimed.clone();
        stale_claimant.claimed_by = Some("scheduler-b".to_string());
        assert!(begin_claimed_stateful_wait_wake_completion(
            &path,
            &tenant_a,
            &stale_claimant,
            "wake-key",
            1_600
        )
        .await
        .expect("stale claimant")
        .is_none());

        assert!(begin_claimed_stateful_wait_wake_completion(
            &path, &tenant_a, &claimed, "wake-key", 2_000
        )
        .await
        .expect("expired claim")
        .is_none());

        let reserved = begin_claimed_stateful_wait_wake_completion(
            &path, &tenant_a, &claimed, "wake-key", 1_700,
        )
        .await
        .expect("reserve claimed wait completion")
        .expect("reserved record");
        assert_eq!(reserved.status, StatefulWaitStatus::Claimed);
        assert_eq!(reserved.wake_idempotency_key.as_deref(), Some("wake-key"));
        assert_eq!(reserved.event_seq, None);
        assert_eq!(reserved.claimed_by.as_deref(), Some("scheduler-a"));
        assert_eq!(reserved.claimed_at_ms, Some(1_500));
        assert_eq!(reserved.claim_expires_at_ms, Some(2_000));

        let reclaimed = claim_due_stateful_wait(
            &path,
            &tenant_a,
            "run-a",
            "wait-a",
            "scheduler-b",
            2_000,
            500,
        )
        .await
        .expect("reclaim reserved wait")
        .expect("reclaimed wait");
        assert_eq!(reclaimed.claimed_by.as_deref(), Some("scheduler-b"));
        assert_eq!(reclaimed.wake_idempotency_key, None);

        let reserved = begin_claimed_stateful_wait_wake_completion(
            &path, &tenant_a, &reclaimed, "wake-key", 2_100,
        )
        .await
        .expect("reserve reclaimed wait completion")
        .expect("reserved reclaimed record");
        let completed = finish_claimed_stateful_wait_completion(
            &path,
            &tenant_a,
            &reserved,
            "wake-key",
            42,
            StatefulWaitStatus::Woken,
            2_150,
        )
        .await
        .expect("finish completion")
        .expect("completed wait");
        assert_eq!(completed.status, StatefulWaitStatus::Woken);
        assert_eq!(completed.event_seq, Some(42));
        assert!(completed.claimed_by.is_none());
        assert!(completed.claimed_at_ms.is_none());
        assert!(completed.claim_expires_at_ms.is_none());
        assert!(finish_claimed_stateful_wait_completion(
            &path,
            &tenant_a,
            &reserved,
            "wake-key",
            43,
            StatefulWaitStatus::Woken,
            2_200
        )
        .await
        .expect("conflicting event seq")
        .is_none());
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn webhook_wait_claim_matches_metadata_once() {
        let path = temp_wait_store("stateful-waits-webhook-match");
        let tenant_a = tenant("org-a", "workspace-a");
        upsert_stateful_wait(
            &path,
            webhook_wait(
                "wait-a",
                "run-a",
                tenant_a.clone(),
                StatefulWebhookWaitMatch {
                    trigger_id: Some("trigger-a".to_string()),
                    provider_event_id: Some("evt-a".to_string()),
                    ..StatefulWebhookWaitMatch::default()
                },
            ),
        )
        .await
        .expect("insert wait");

        assert!(claim_matching_stateful_webhook_wait(
            &path,
            &tenant_a,
            &webhook_event("trigger-a", Some("evt-b")),
            "webhook-router",
            1_500,
            500,
        )
        .await
        .expect("nonmatching event")
        .is_none());
        let claimed = claim_matching_stateful_webhook_wait(
            &path,
            &tenant_a,
            &webhook_event("trigger-a", Some("evt-a")),
            "webhook-router",
            1_500,
            500,
        )
        .await
        .expect("claim")
        .expect("claimed wait");
        assert_eq!(claimed.wait_id, "wait-a");
        assert_eq!(claimed.claimed_by.as_deref(), Some("webhook-router"));
        assert!(claim_matching_stateful_webhook_wait(
            &path,
            &tenant_a,
            &webhook_event("trigger-a", Some("evt-a")),
            "webhook-router-2",
            1_600,
            500,
        )
        .await
        .expect("active duplicate claim")
        .is_none());
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn webhook_wait_claim_is_tenant_scoped_and_ordered() {
        let path = temp_wait_store("stateful-waits-webhook-tenant");
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let match_rules = StatefulWebhookWaitMatch {
            trigger_id: Some("trigger-a".to_string()),
            ..StatefulWebhookWaitMatch::default()
        };
        upsert_stateful_wait(
            &path,
            webhook_wait("wait-b", "run-b", tenant_b.clone(), match_rules.clone()),
        )
        .await
        .expect("insert tenant b");
        upsert_stateful_wait(
            &path,
            webhook_wait("wait-a", "run-a", tenant_a.clone(), match_rules),
        )
        .await
        .expect("insert tenant a");

        let claimed = claim_matching_stateful_webhook_wait(
            &path,
            &tenant_a,
            &webhook_event("trigger-a", Some("evt-a")),
            "webhook-router",
            1_500,
            500,
        )
        .await
        .expect("claim")
        .expect("tenant a wait");
        assert_eq!(claimed.run_id, "run-a");
        let tenant_b_waits = list_stateful_waits(
            &path,
            &tenant_b,
            StatefulWaitQuery {
                status: Some(StatefulWaitStatus::Waiting),
                wait_kind: Some(StatefulWaitKind::Webhook),
                ..StatefulWaitQuery::default()
            },
        );
        assert_eq!(tenant_b_waits.len(), 1);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[test]
    fn timeout_policy_serializes_timeout_action_metadata() {
        let policy = StatefulWaitTimeoutPolicy {
            timeout_at_ms: 2_000,
            on_timeout: StatefulWaitTimeoutAction::Escalate,
            escalate_to: Some("ops-oncall".to_string()),
            remind_every_ms: Some(300),
            metadata: Some(json!({ "channel": "pager" })),
        };

        let serialized = serde_json::to_value(&policy).expect("serialize policy");

        assert_eq!(serialized["on_timeout"], "escalate");
        assert_eq!(serialized["escalate_to"], "ops-oncall");
        assert_eq!(serialized["remind_every_ms"], 300);
    }
}
