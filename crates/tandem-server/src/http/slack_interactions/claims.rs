use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::DataClass;
use tandem_memory::envelope::MemoryKeyScope;
use tandem_memory::types::MemoryTenantScope;
use tandem_memory::{MemoryCryptoMode, MemoryCryptoProvider};
use tandem_types::TenantContext;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::AppState;

const CLAIM_SCHEMA_VERSION: u32 = 4;
pub(super) const CLAIM_LEASE: Duration = Duration::from_secs(90);
pub(super) const CLAIM_HEARTBEAT: Duration = Duration::from_secs(30);
pub(super) const CLAIM_RECOVERY_SCAN_INTERVAL: Duration = Duration::from_secs(30);
const TERMINAL_CLAIM_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_TERMINAL_CLAIMS: usize = 10_000;
const MAX_CLAIM_ATTEMPTS: u32 = 8;
const RETRY_BACKOFF_BASE: Duration = Duration::from_secs(1);
const RETRY_BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);
const LOCAL_CLAIM_RECORD_PREFIX: &str = "tsc1:";
const LOCK_WAIT: Duration = Duration::from_secs(5);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

static LOCAL_CLAIM_CRYPTO: OnceLock<MemoryCryptoProvider> = OnceLock::new();

#[derive(Debug, Clone)]
pub(super) struct SlackEventClaimInput {
    pub tenant_context: TenantContext,
    pub team_id: String,
    pub app_id: String,
    pub event_id: String,
    pub fingerprint: String,
    pub recovery_payload: serde_json::Value,
    pub now_ms: u64,
}

#[derive(Debug, Clone)]
pub(super) struct SlackEventClaim {
    record_path: PathBuf,
    claim_id: String,
    pub key: String,
    pub fingerprint: String,
    pub tenant_context: TenantContext,
    pub attempt: u32,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub session_message_count: Option<usize>,
    pub pending_response: Option<String>,
    pub response_delivered_at_ms: Option<u64>,
    pub response_audited_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) enum SlackEventClaimDecision {
    Claimed(SlackEventClaim),
    InFlight,
    Completed,
    RetryScheduled,
    Quarantined,
    Conflict,
}

#[derive(Debug, Clone)]
pub(super) struct RecoverableSlackEventClaim {
    pub claim: SlackEventClaim,
    pub recovery_payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SlackEventClaimStatus {
    Processing,
    Completed,
    Retryable,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackEventClaimRecord {
    schema_version: u32,
    key: String,
    fingerprint: String,
    tenant_context: TenantContext,
    team_id: String,
    app_id: String,
    event_id: String,
    status: SlackEventClaimStatus,
    claim_id: String,
    attempt: u32,
    first_seen_at_ms: u64,
    updated_at_ms: u64,
    lease_expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_message_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_response: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_delivered_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_audited_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_attempt_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

pub(super) async fn claim_slack_event(
    state: &AppState,
    input: SlackEventClaimInput,
) -> anyhow::Result<SlackEventClaimDecision> {
    let key = format!("{}:{}:{}", input.team_id, input.app_id, input.event_id);
    let record_path = claim_record_path(state, &input, &key);
    let _lock = ClaimFileLock::acquire(&record_path).await?;
    let existing = read_record(&record_path).await?;

    let mut record = match existing {
        Some(existing) if existing.fingerprint != input.fingerprint => {
            return Ok(SlackEventClaimDecision::Conflict)
        }
        Some(existing) if existing.status == SlackEventClaimStatus::Completed => {
            return Ok(SlackEventClaimDecision::Completed)
        }
        Some(existing) if existing.status == SlackEventClaimStatus::Quarantined => {
            return Ok(SlackEventClaimDecision::Quarantined)
        }
        Some(existing)
            if existing.status == SlackEventClaimStatus::Processing
                && existing.lease_expires_at_ms > input.now_ms =>
        {
            return Ok(SlackEventClaimDecision::InFlight)
        }
        Some(existing)
            if existing.status == SlackEventClaimStatus::Retryable
                && existing.next_attempt_at_ms.unwrap_or(0) > input.now_ms =>
        {
            return Ok(SlackEventClaimDecision::RetryScheduled)
        }
        Some(mut existing) if existing.attempt >= MAX_CLAIM_ATTEMPTS => {
            quarantine_record(
                &mut existing,
                "Slack event claim exceeded maximum recovery attempts",
                input.now_ms,
            );
            write_record(&record_path, &existing).await?;
            return Ok(SlackEventClaimDecision::Quarantined);
        }
        Some(mut existing) => {
            existing.status = SlackEventClaimStatus::Processing;
            existing.claim_id = Uuid::new_v4().to_string();
            existing.attempt = existing.attempt.saturating_add(1);
            existing.updated_at_ms = input.now_ms;
            existing.lease_expires_at_ms = lease_expires_at(input.now_ms);
            existing.next_attempt_at_ms = None;
            existing.last_error = None;
            existing.recovery_payload = Some(encrypt_recovery_payload(
                &input.recovery_payload,
                &input.tenant_context,
                &key,
            )?);
            existing
        }
        None => SlackEventClaimRecord {
            schema_version: CLAIM_SCHEMA_VERSION,
            key: key.clone(),
            fingerprint: input.fingerprint.clone(),
            tenant_context: input.tenant_context.clone(),
            team_id: input.team_id,
            app_id: input.app_id,
            event_id: input.event_id,
            status: SlackEventClaimStatus::Processing,
            claim_id: Uuid::new_v4().to_string(),
            attempt: 1,
            first_seen_at_ms: input.now_ms,
            updated_at_ms: input.now_ms,
            lease_expires_at_ms: lease_expires_at(input.now_ms),
            session_id: None,
            run_id: None,
            session_message_count: None,
            pending_response: None,
            recovery_payload: Some(encrypt_recovery_payload(
                &input.recovery_payload,
                &input.tenant_context,
                &key,
            )?),
            response_delivered_at_ms: None,
            response_audited_at_ms: None,
            next_attempt_at_ms: None,
            last_error: None,
        },
    };
    record.schema_version = CLAIM_SCHEMA_VERSION;
    record.tenant_context = input.tenant_context.clone();
    write_record(&record_path, &record).await?;

    Ok(SlackEventClaimDecision::Claimed(claim_from_record(
        record_path,
        record,
    )?))
}

pub(super) async fn refresh_slack_event_claim(
    claim: &SlackEventClaim,
    now_ms: u64,
) -> anyhow::Result<bool> {
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        true
    })
    .await
}

pub(super) async fn complete_slack_event_claim(
    claim: &SlackEventClaim,
    session_id: &str,
    now_ms: u64,
) -> anyhow::Result<bool> {
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        record.status = SlackEventClaimStatus::Completed;
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = now_ms;
        record.session_id = Some(session_id.to_string());
        record.next_attempt_at_ms = None;
        record.pending_response = None;
        record.recovery_payload = None;
        record.last_error = None;
        true
    })
    .await
}

pub(super) async fn checkpoint_slack_event_execution(
    claim: &SlackEventClaim,
    session_id: &str,
    run_id: &str,
    session_message_count: usize,
    now_ms: u64,
) -> anyhow::Result<bool> {
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        if let Some(existing) = record.session_id.as_deref() {
            return existing == session_id
                && record.run_id.as_deref() == Some(run_id)
                && record.session_message_count == Some(session_message_count);
        }
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        record.session_id = Some(session_id.to_string());
        record.run_id = Some(run_id.to_string());
        record.session_message_count = Some(session_message_count);
        true
    })
    .await
}

pub(super) async fn stage_slack_event_response(
    claim: &SlackEventClaim,
    session_id: &str,
    response: &str,
    now_ms: u64,
) -> anyhow::Result<bool> {
    let response_context = pending_response_context(&claim.tenant_context, &claim.key);
    let response = encrypt_claim_text(response, &response_context)
        .context("encrypt staged Slack event response")?;
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        record.session_id = Some(session_id.to_string());
        record.pending_response = Some(response);
        record.response_delivered_at_ms = None;
        record.response_audited_at_ms = None;
        true
    })
    .await
}

fn pending_response_context(
    tenant_context: &TenantContext,
    claim_key: &str,
) -> crate::encrypted_file_store::ProtectedRecordContext {
    let tenant_scope = MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    };
    let key_scope = MemoryKeyScope::new(
        &tenant_scope,
        DataClass::Restricted,
        Some("slack-event-claim-response".to_string()),
    );
    crate::encrypted_file_store::ProtectedRecordContext::new(
        key_scope,
        "slack-events:claim-response:v1",
        format!("slack-event-claim-response:{claim_key}"),
    )
}

fn recovery_payload_context(
    tenant_context: &TenantContext,
    claim_key: &str,
) -> crate::encrypted_file_store::ProtectedRecordContext {
    let tenant_scope = MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    };
    let key_scope = MemoryKeyScope::new(
        &tenant_scope,
        DataClass::Restricted,
        Some("slack-event-claim-recovery".to_string()),
    );
    crate::encrypted_file_store::ProtectedRecordContext::new(
        key_scope,
        "slack-events:claim-recovery:v1",
        format!("slack-event-claim-recovery:{claim_key}"),
    )
}

fn encrypt_recovery_payload(
    payload: &serde_json::Value,
    tenant_context: &TenantContext,
    claim_key: &str,
) -> anyhow::Result<String> {
    let plaintext = serde_json::to_string(payload)?;
    encrypt_claim_text(
        &plaintext,
        &recovery_payload_context(tenant_context, claim_key),
    )
    .context("encrypt Slack event recovery payload")
}

fn decrypt_recovery_payload(record: &SlackEventClaimRecord) -> anyhow::Result<serde_json::Value> {
    let stored = record
        .recovery_payload
        .as_deref()
        .context("Slack event claim has no recovery payload")?;
    let plaintext = decrypt_claim_text(
        stored,
        &recovery_payload_context(&record.tenant_context, &record.key),
    )
    .context("decrypt Slack event recovery payload")?;
    serde_json::from_str(&plaintext).context("parse Slack event recovery payload")
}

#[derive(Serialize, Deserialize)]
struct LocalClaimProtectedPayload {
    context: crate::encrypted_file_store::ProtectedRecordContext,
    payload: String,
}

fn encrypt_claim_text(
    plaintext: &str,
    context: &crate::encrypted_file_store::ProtectedRecordContext,
) -> anyhow::Result<String> {
    let configured = crate::encrypted_file_store::encrypt_text(plaintext, context)?;
    if crate::encrypted_file_store::is_encrypted_payload(&configured) {
        return Ok(configured);
    }
    anyhow::ensure!(
        !tandem_memory::envelope::hosted_memory_encryption_required(),
        "hosted Slack claim encryption requires the configured KMS provider"
    );

    let bound = serde_json::to_string(&LocalClaimProtectedPayload {
        context: context.clone(),
        payload: plaintext.to_string(),
    })?;
    let ciphertext = local_claim_crypto()
        .encrypt_field(&bound)
        .context("encrypt Slack claim with protected local key")?;
    anyhow::ensure!(
        crate::encrypted_file_store::is_encrypted_payload(&ciphertext),
        "Slack claim encryption requires a usable protected local key"
    );
    Ok(format!("{LOCAL_CLAIM_RECORD_PREFIX}{ciphertext}"))
}

fn decrypt_claim_text(
    stored: &str,
    expected: &crate::encrypted_file_store::ProtectedRecordContext,
) -> anyhow::Result<String> {
    if let Some(ciphertext) = stored.strip_prefix(LOCAL_CLAIM_RECORD_PREFIX) {
        anyhow::ensure!(
            !tandem_memory::envelope::hosted_memory_encryption_required(),
            "hosted Slack claims cannot use local-key ciphertext"
        );
        let plaintext = local_claim_crypto()
            .decrypt_field(ciphertext)
            .context("decrypt Slack claim with protected local key")?;
        let bound = serde_json::from_str::<LocalClaimProtectedPayload>(&plaintext)
            .context("parse protected local Slack claim")?;
        anyhow::ensure!(
            bound.context == *expected,
            "protected local Slack claim authority does not match expected context"
        );
        return Ok(bound.payload);
    }
    anyhow::ensure!(
        crate::encrypted_file_store::is_encrypted_payload(stored),
        "plaintext Slack claim payloads are not accepted"
    );
    crate::encrypted_file_store::decrypt_text(stored, expected)
}

fn local_claim_crypto() -> &'static MemoryCryptoProvider {
    LOCAL_CLAIM_CRYPTO.get_or_init(|| {
        MemoryCryptoProvider::from_mode(MemoryCryptoMode::LocalEncrypted {
            provider: "local-file".to_string(),
        })
    })
}

pub(super) async fn mark_slack_event_response_delivered(
    claim: &SlackEventClaim,
    now_ms: u64,
) -> anyhow::Result<bool> {
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing || record.pending_response.is_none() {
            return false;
        }
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        record.response_delivered_at_ms.get_or_insert(now_ms);
        true
    })
    .await
}

pub(super) async fn mark_slack_event_response_audited(
    claim: &SlackEventClaim,
    now_ms: u64,
) -> anyhow::Result<bool> {
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing
            || record.pending_response.is_none()
            || record.response_delivered_at_ms.is_none()
        {
            return false;
        }
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        record.response_audited_at_ms.get_or_insert(now_ms);
        true
    })
    .await
}

pub(super) async fn retry_slack_event_claim(
    claim: &SlackEventClaim,
    error: &str,
    now_ms: u64,
) -> anyhow::Result<bool> {
    let error = truncate(error, 1_000);
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        if record.attempt >= MAX_CLAIM_ATTEMPTS {
            quarantine_record(record, &error, now_ms);
            return true;
        }
        record.status = SlackEventClaimStatus::Retryable;
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = now_ms;
        record.next_attempt_at_ms = Some(next_attempt_at(now_ms, record.attempt));
        record.last_error = Some(error.clone());
        true
    })
    .await
}

pub(super) async fn quarantine_slack_event_claim(
    claim: &SlackEventClaim,
    error: &str,
    now_ms: u64,
) -> anyhow::Result<bool> {
    let error = truncate(error, 1_000);
    update_claim(claim, |record| {
        if record.status != SlackEventClaimStatus::Processing {
            return false;
        }
        quarantine_record(record, &error, now_ms);
        true
    })
    .await
}

fn quarantine_record(record: &mut SlackEventClaimRecord, error: &str, now_ms: u64) {
    record.status = SlackEventClaimStatus::Quarantined;
    record.updated_at_ms = now_ms;
    record.lease_expires_at_ms = now_ms;
    record.next_attempt_at_ms = None;
    record.pending_response = None;
    record.recovery_payload = None;
    record.last_error = Some(truncate(error, 1_000));
}

async fn update_claim(
    claim: &SlackEventClaim,
    update: impl FnOnce(&mut SlackEventClaimRecord) -> bool,
) -> anyhow::Result<bool> {
    let _lock = ClaimFileLock::acquire(&claim.record_path).await?;
    let Some(mut record) = read_record(&claim.record_path).await? else {
        return Ok(false);
    };
    if record.claim_id != claim.claim_id || record.fingerprint != claim.fingerprint {
        return Ok(false);
    }
    if !update(&mut record) {
        return Ok(false);
    }
    write_record(&claim.record_path, &record).await?;
    Ok(true)
}

pub(super) async fn recover_slack_event_claims(
    state: &AppState,
    now_ms: u64,
    limit: usize,
) -> anyhow::Result<Vec<RecoverableSlackEventClaim>> {
    let mut due = Vec::new();
    for record_path in claim_record_paths(state).await? {
        match read_record(&record_path).await {
            Ok(Some(record)) if claim_is_due(&record, now_ms) => due.push((
                claim_due_at(&record),
                record.first_seen_at_ms,
                record.key,
                record_path,
            )),
            Ok(_) => {}
            Err(error) => tracing::warn!(
                path = %record_path.display(),
                %error,
                "could not inspect Slack event claim during recovery scan"
            ),
        }
    }
    due.sort_by(|left, right| {
        (left.0, left.1, left.2.as_str()).cmp(&(right.0, right.1, right.2.as_str()))
    });

    let mut recovered = Vec::new();
    for (_, _, _, record_path) in due {
        if recovered.len() >= limit {
            break;
        }
        let _lock = match ClaimFileLock::acquire(&record_path).await {
            Ok(lock) => lock,
            Err(error) => {
                tracing::warn!(path = %record_path.display(), %error, "could not lock Slack event claim during recovery scan");
                continue;
            }
        };
        let Some(mut record) = (match read_record(&record_path).await {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(path = %record_path.display(), %error, "could not read Slack event claim during recovery scan");
                continue;
            }
        }) else {
            continue;
        };
        if !claim_is_due(&record, now_ms) {
            continue;
        }
        if record.recovery_payload.is_none() {
            quarantine_record(
                &mut record,
                "Slack event claim is missing its encrypted recovery payload",
                now_ms,
            );
            write_record(&record_path, &record).await?;
            continue;
        }
        if record.attempt >= MAX_CLAIM_ATTEMPTS {
            quarantine_record(
                &mut record,
                "Slack event claim exceeded maximum recovery attempts",
                now_ms,
            );
            write_record(&record_path, &record).await?;
            continue;
        }
        let recovery_payload = match decrypt_recovery_payload(&record) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::error!(path = %record_path.display(), %error, "Slack event recovery payload failed authentication");
                quarantine_record(
                    &mut record,
                    &format!("Slack event recovery payload failed authentication: {error}"),
                    now_ms,
                );
                write_record(&record_path, &record).await?;
                continue;
            }
        };
        record.status = SlackEventClaimStatus::Processing;
        record.claim_id = Uuid::new_v4().to_string();
        record.attempt = record.attempt.saturating_add(1);
        record.updated_at_ms = now_ms;
        record.lease_expires_at_ms = lease_expires_at(now_ms);
        record.next_attempt_at_ms = None;
        record.last_error = None;
        write_record(&record_path, &record).await?;
        recovered.push(RecoverableSlackEventClaim {
            claim: claim_from_record(record_path, record)?,
            recovery_payload,
        });
    }
    Ok(recovered)
}

fn claim_is_due(record: &SlackEventClaimRecord, now_ms: u64) -> bool {
    match record.status {
        SlackEventClaimStatus::Retryable => record.next_attempt_at_ms.unwrap_or(0) <= now_ms,
        SlackEventClaimStatus::Processing => record.lease_expires_at_ms <= now_ms,
        SlackEventClaimStatus::Completed | SlackEventClaimStatus::Quarantined => false,
    }
}

fn claim_due_at(record: &SlackEventClaimRecord) -> u64 {
    match record.status {
        SlackEventClaimStatus::Retryable => record.next_attempt_at_ms.unwrap_or(0),
        SlackEventClaimStatus::Processing => record.lease_expires_at_ms,
        SlackEventClaimStatus::Completed | SlackEventClaimStatus::Quarantined => u64::MAX,
    }
}

pub(super) async fn compact_slack_event_claims(
    state: &AppState,
    now_ms: u64,
) -> anyhow::Result<usize> {
    compact_slack_event_claims_with_limits(
        state,
        now_ms,
        TERMINAL_CLAIM_RETENTION,
        MAX_TERMINAL_CLAIMS,
    )
    .await
}

async fn compact_slack_event_claims_with_limits(
    state: &AppState,
    now_ms: u64,
    retention: Duration,
    max_terminal: usize,
) -> anyhow::Result<usize> {
    let mut terminal = Vec::new();
    for path in claim_record_paths(state).await? {
        match read_record(&path).await {
            Ok(Some(record))
                if matches!(
                    record.status,
                    SlackEventClaimStatus::Completed | SlackEventClaimStatus::Quarantined
                ) =>
            {
                terminal.push((path, record.updated_at_ms, record.status));
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not inspect Slack event claim during compaction");
            }
        }
    }
    terminal.sort_by(|left, right| right.1.cmp(&left.1));
    let retention_ms = retention.as_millis().min(u64::MAX as u128) as u64;
    let mut removed = 0;
    for (index, (path, updated_at_ms, status)) in terminal.into_iter().enumerate() {
        let expired = now_ms.saturating_sub(updated_at_ms) >= retention_ms;
        if (expired || index >= max_terminal)
            && remove_terminal_claim(&path, updated_at_ms, status).await?
        {
            removed += 1;
        }
    }
    Ok(removed)
}

async fn remove_terminal_claim(
    path: &Path,
    expected_updated_at_ms: u64,
    expected_status: SlackEventClaimStatus,
) -> anyhow::Result<bool> {
    let _lock = ClaimFileLock::acquire(path).await?;
    let Some(record) = read_record(path).await? else {
        return Ok(false);
    };
    if record.status != expected_status
        || !matches!(
            record.status,
            SlackEventClaimStatus::Completed | SlackEventClaimStatus::Quarantined
        )
        || record.updated_at_ms != expected_updated_at_ms
    {
        return Ok(false);
    }
    tokio::fs::remove_file(path).await?;
    if let Some(parent) = path.parent() {
        sync_directory(parent).await?;
    }
    Ok(true)
}

fn claim_from_record(
    record_path: PathBuf,
    record: SlackEventClaimRecord,
) -> anyhow::Result<SlackEventClaim> {
    let pending_response = record
        .pending_response
        .as_deref()
        .map(|stored| {
            decrypt_claim_text(
                stored,
                &pending_response_context(&record.tenant_context, &record.key),
            )
        })
        .transpose()
        .context("decrypt staged Slack event response")?;
    Ok(SlackEventClaim {
        record_path,
        claim_id: record.claim_id,
        key: record.key,
        fingerprint: record.fingerprint,
        tenant_context: record.tenant_context,
        attempt: record.attempt,
        session_id: record.session_id,
        run_id: record.run_id,
        session_message_count: record.session_message_count,
        pending_response,
        response_delivered_at_ms: record.response_delivered_at_ms,
        response_audited_at_ms: record.response_audited_at_ms,
    })
}

fn claim_record_path(state: &AppState, input: &SlackEventClaimInput, key: &str) -> PathBuf {
    let root = claim_records_root(state);
    let deployment_id = input
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or_default();
    let digest = crate::sha256_hex(&[
        &input.tenant_context.org_id,
        &input.tenant_context.workspace_id,
        deployment_id,
        key,
    ]);
    root.join(format!("{digest}.json"))
}

fn claim_records_root(state: &AppState) -> PathBuf {
    state
        .idempotency_keys_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("slack_event_claims")
}

async fn claim_record_paths(state: &AppState) -> anyhow::Result<Vec<PathBuf>> {
    let root = claim_records_root(state);
    let mut directory = match tokio::fs::read_dir(&root).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };
    let mut paths = Vec::new();
    while let Some(entry) = directory.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn lease_expires_at(now_ms: u64) -> u64 {
    now_ms.saturating_add(CLAIM_LEASE.as_millis() as u64)
}

fn next_attempt_at(now_ms: u64, attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(31);
    let multiplier = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let base_ms = RETRY_BACKOFF_BASE.as_millis().min(u64::MAX as u128) as u64;
    let max_ms = RETRY_BACKOFF_MAX.as_millis().min(u64::MAX as u128) as u64;
    now_ms.saturating_add(base_ms.saturating_mul(multiplier).min(max_ms))
}

async fn read_record(path: &Path) -> anyhow::Result<Option<SlackEventClaimRecord>> {
    let raw = match tokio::fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let record = serde_json::from_str::<SlackEventClaimRecord>(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    if record.schema_version > CLAIM_SCHEMA_VERSION {
        anyhow::bail!(
            "Slack event claim schema {} is newer than supported {}",
            record.schema_version,
            CLAIM_SCHEMA_VERSION
        );
    }
    Ok(Some(record))
}

async fn write_record(path: &Path, record: &SlackEventClaimRecord) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Slack event claim path has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    let payload = serde_json::to_vec_pretty(record)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    if let Err(error) = async {
        file.write_all(&payload).await?;
        file.flush().await?;
        file.sync_all().await?;
        tokio::fs::rename(&temporary, path).await?;
        sync_directory(parent).await
    }
    .await
    {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

async fn sync_directory(path: &Path) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
    Ok(())
}

struct ClaimFileLock {
    path: PathBuf,
}

impl ClaimFileLock {
    async fn acquire(record_path: &Path) -> anyhow::Result<Self> {
        let parent = record_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Slack event claim path has no parent"))?;
        tokio::fs::create_dir_all(parent).await?;
        let lock_path = record_path.with_extension("lock");
        let started = tokio::time::Instant::now();
        loop {
            match tokio::fs::create_dir(&lock_path).await {
                Ok(()) => return Ok(Self { path: lock_path }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock_path).await {
                        let _ = tokio::fs::remove_dir(&lock_path).await;
                    }
                    if started.elapsed() >= LOCK_WAIT {
                        anyhow::bail!("timed out acquiring Slack event claim lock");
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for ClaimFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

async fn lock_is_stale(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= STALE_LOCK_AGE)
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(tenant: TenantContext, event_id: &str, fingerprint: &str) -> SlackEventClaimInput {
        SlackEventClaimInput {
            tenant_context: tenant,
            team_id: "T1".to_string(),
            app_id: "A1".to_string(),
            event_id: event_id.to_string(),
            fingerprint: fingerprint.to_string(),
            recovery_payload: serde_json::json!({
                "event_id": event_id,
                "classified_text": format!("recovery-{event_id}"),
            }),
            now_ms: 1_000,
        }
    }

    #[tokio::test]
    async fn completed_claim_survives_new_app_state_and_suppresses_replay() {
        let first = crate::test_support::test_state().await;
        let claim_path = first.idempotency_keys_path.clone();
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(&first, input(tenant.clone(), "Ev1", "fp1"))
            .await
            .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(complete_slack_event_claim(&claimed, "session-1", 2_000)
            .await
            .unwrap());

        let mut restarted = crate::test_support::test_state().await;
        restarted.idempotency_keys_path = claim_path;
        assert!(matches!(
            claim_slack_event(&restarted, input(tenant, "Ev1", "fp1"))
                .await
                .unwrap(),
            SlackEventClaimDecision::Completed
        ));
    }
    #[tokio::test]
    async fn retryable_claim_can_be_reclaimed_but_conflicting_payload_cannot() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(&state, input(tenant.clone(), "Ev2", "fp2"))
            .await
            .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(retry_slack_event_claim(&claimed, "provider failed", 2_000)
            .await
            .unwrap());
        let replay = claim_slack_event(
            &state,
            SlackEventClaimInput {
                now_ms: 3_000,
                ..input(tenant.clone(), "Ev2", "fp2")
            },
        )
        .await
        .unwrap();
        assert!(matches!(replay, SlackEventClaimDecision::Claimed(_)));

        assert!(matches!(
            claim_slack_event(&state, input(tenant, "Ev2", "different"))
                .await
                .unwrap(),
            SlackEventClaimDecision::Conflict
        ));
    }
    #[tokio::test]
    async fn competing_instances_share_one_durable_claim_owner() {
        let first = crate::test_support::test_state().await;
        let mut second = crate::test_support::test_state().await;
        second.idempotency_keys_path = first.idempotency_keys_path.clone();
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");

        let (left, right) = tokio::join!(
            claim_slack_event(&first, input(tenant.clone(), "Ev3", "fp3")),
            claim_slack_event(&second, input(tenant, "Ev3", "fp3")),
        );
        let decisions = [left.unwrap(), right.unwrap()];
        assert_eq!(
            decisions
                .iter()
                .filter(|decision| matches!(decision, SlackEventClaimDecision::Claimed(_)))
                .count(),
            1
        );
        assert_eq!(
            decisions
                .iter()
                .filter(|decision| matches!(decision, SlackEventClaimDecision::InFlight))
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn retry_reclaims_staged_response_without_losing_session() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(&state, input(tenant.clone(), "Ev4", "fp4"))
            .await
            .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(stage_slack_event_response(
            &claimed,
            "session-4",
            "staged governed response",
            2_000,
        )
        .await
        .unwrap());
        assert!(
            retry_slack_event_claim(&claimed, "Slack transport failed", 2_001)
                .await
                .unwrap()
        );

        let replay = claim_slack_event(
            &state,
            SlackEventClaimInput {
                now_ms: 3_001,
                ..input(tenant, "Ev4", "fp4")
            },
        )
        .await
        .unwrap();
        let SlackEventClaimDecision::Claimed(replay) = replay else {
            panic!("expected retry claim, got {replay:?}");
        };
        assert_eq!(replay.session_id.as_deref(), Some("session-4"));
        assert_eq!(
            replay.pending_response.as_deref(),
            Some("staged governed response")
        );
        assert_eq!(replay.attempt, 2);
    }
    #[tokio::test]
    async fn staged_response_is_encrypted_at_rest_and_tampering_fails_closed() {
        crate::encrypted_file_store::with_test_crypto_provider(
            tandem_memory::MemoryCryptoProvider::local_key([0x5a; 32]),
            None,
            async {
                let state = crate::test_support::test_state().await;
                let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
                let claimed = match claim_slack_event(
                    &state,
                    input(tenant.clone(), "Ev-encrypted", "fp-encrypted"),
                )
                .await
                .unwrap()
                {
                    SlackEventClaimDecision::Claimed(claim) => claim,
                    other => panic!("expected claim, got {other:?}"),
                };
                assert!(stage_slack_event_response(
                    &claimed,
                    "session-encrypted",
                    "classified staged reply",
                    2_000,
                )
                .await
                .unwrap());
                assert!(retry_slack_event_claim(&claimed, "retry", 2_001)
                    .await
                    .unwrap());

                let stored = tokio::fs::read_to_string(&claimed.record_path)
                    .await
                    .expect("read encrypted claim");
                assert!(!stored.contains("classified staged reply"));
                assert!(!stored.contains("recovery-Ev-encrypted"));
                assert!(stored.contains(crate::encrypted_file_store::SCOPED_RECORD_PREFIX));

                let replay = claim_slack_event(
                    &state,
                    SlackEventClaimInput {
                        now_ms: 3_001,
                        ..input(tenant.clone(), "Ev-encrypted", "fp-encrypted")
                    },
                )
                .await
                .unwrap();
                let SlackEventClaimDecision::Claimed(replay) = replay else {
                    panic!("expected retry claim, got {replay:?}");
                };
                assert_eq!(
                    replay.pending_response.as_deref(),
                    Some("classified staged reply")
                );
                assert!(retry_slack_event_claim(&replay, "tamper test", 3_002)
                    .await
                    .unwrap());

                let stored = tokio::fs::read_to_string(&replay.record_path)
                    .await
                    .expect("read claim before tamper");
                let tampered = stored.replacen(
                    crate::encrypted_file_store::SCOPED_RECORD_PREFIX,
                    "tgs1:!",
                    1,
                );
                tokio::fs::write(&replay.record_path, tampered)
                    .await
                    .expect("tamper claim");
                assert!(claim_slack_event(
                    &state,
                    SlackEventClaimInput {
                        now_ms: 5_002,
                        ..input(tenant, "Ev-encrypted", "fp-encrypted")
                    },
                )
                .await
                .is_err());
            },
        )
        .await;
    }
    #[tokio::test]
    async fn retry_preserves_delivery_and_audit_checkpoints() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(&state, input(tenant.clone(), "Ev5", "fp5"))
            .await
            .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(
            stage_slack_event_response(&claimed, "session-5", "reply", 2_000)
                .await
                .unwrap()
        );
        assert!(mark_slack_event_response_delivered(&claimed, 2_001)
            .await
            .unwrap());
        assert!(mark_slack_event_response_audited(&claimed, 2_002)
            .await
            .unwrap());
        assert!(
            retry_slack_event_claim(&claimed, "completion failed", 2_003)
                .await
                .unwrap()
        );

        let replay = claim_slack_event(
            &state,
            SlackEventClaimInput {
                now_ms: 3_003,
                ..input(tenant, "Ev5", "fp5")
            },
        )
        .await
        .unwrap();
        let SlackEventClaimDecision::Claimed(replay) = replay else {
            panic!("expected retry claim, got {replay:?}");
        };
        assert_eq!(replay.response_delivered_at_ms, Some(2_001));
        assert_eq!(replay.response_audited_at_ms, Some(2_002));
    }
    #[tokio::test]
    async fn retryable_claim_is_discovered_and_reclaimed_without_redelivery() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed =
            match claim_slack_event(&state, input(tenant, "Ev-autonomous", "fp-autonomous"))
                .await
                .unwrap()
            {
                SlackEventClaimDecision::Claimed(claim) => claim,
                other => panic!("expected claim, got {other:?}"),
            };
        assert!(retry_slack_event_claim(&claimed, "restart", 2_000)
            .await
            .unwrap());

        assert!(recover_slack_event_claims(&state, 2_999, 10)
            .await
            .unwrap()
            .is_empty());
        let recovered = recover_slack_event_claims(&state, 3_000, 10).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].claim.attempt, 2);
        assert_eq!(
            recovered[0]
                .recovery_payload
                .get("classified_text")
                .and_then(serde_json::Value::as_str),
            Some("recovery-Ev-autonomous")
        );
        assert!(recover_slack_event_claims(&state, 3_001, 10)
            .await
            .unwrap()
            .is_empty());
    }
    #[tokio::test]
    async fn lease_expired_processing_claim_is_recovered() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(
            &state,
            input(tenant, "Ev-expired-lease", "fp-expired-lease"),
        )
        .await
        .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };

        assert!(
            recover_slack_event_claims(&state, 1_000 + CLAIM_LEASE.as_millis() as u64 - 1, 10)
                .await
                .unwrap()
                .is_empty()
        );
        let recovered =
            recover_slack_event_claims(&state, 1_000 + CLAIM_LEASE.as_millis() as u64, 10)
                .await
                .unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].claim.key, claimed.key);
        assert_eq!(recovered[0].claim.attempt, 2);
    }
    #[tokio::test]
    async fn execution_checkpoint_survives_crash_window_recovery() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed =
            match claim_slack_event(&state, input(tenant, "Ev-crash-window", "fp-crash-window"))
                .await
                .unwrap()
            {
                SlackEventClaimDecision::Claimed(claim) => claim,
                other => panic!("expected claim, got {other:?}"),
            };
        assert!(checkpoint_slack_event_execution(
            &claimed,
            "session-crash-window",
            "slack-deterministic-run",
            7,
            1_001,
        )
        .await
        .unwrap());

        let recovered =
            recover_slack_event_claims(&state, 1_001 + CLAIM_LEASE.as_millis() as u64, 1)
                .await
                .unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            recovered[0].claim.session_id.as_deref(),
            Some("session-crash-window")
        );
        assert_eq!(
            recovered[0].claim.run_id.as_deref(),
            Some("slack-deterministic-run")
        );
        assert_eq!(recovered[0].claim.session_message_count, Some(7));
    }
    #[tokio::test]
    async fn retry_backoff_is_exponential_and_stops_at_quarantine() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let mut claim = match claim_slack_event(
            &state,
            input(tenant.clone(), "Ev-max-attempts", "fp-max-attempts"),
        )
        .await
        .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        let mut now_ms = 2_000;
        for attempt in 1..=MAX_CLAIM_ATTEMPTS {
            assert!(retry_slack_event_claim(&claim, "poison", now_ms)
                .await
                .unwrap());
            let record = read_record(&claim.record_path)
                .await
                .unwrap()
                .expect("claim record");
            if attempt == MAX_CLAIM_ATTEMPTS {
                assert_eq!(record.status, SlackEventClaimStatus::Quarantined);
                assert!(record.next_attempt_at_ms.is_none());
                assert!(record.recovery_payload.is_none());
                break;
            }
            assert_eq!(record.status, SlackEventClaimStatus::Retryable);
            let due_at = record.next_attempt_at_ms.expect("scheduled retry");
            assert!(due_at > now_ms);
            assert!(recover_slack_event_claims(&state, due_at - 1, 1)
                .await
                .unwrap()
                .is_empty());
            let mut recovered = recover_slack_event_claims(&state, due_at, 1).await.unwrap();
            assert_eq!(recovered.len(), 1);
            claim = recovered.remove(0).claim;
            assert_eq!(claim.attempt, attempt + 1);
            now_ms = due_at.saturating_add(1);
        }
        assert!(matches!(
            claim_slack_event(
                &state,
                SlackEventClaimInput {
                    now_ms: now_ms.saturating_add(1),
                    ..input(tenant, "Ev-max-attempts", "fp-max-attempts")
                }
            )
            .await
            .unwrap(),
            SlackEventClaimDecision::Quarantined
        ));
    }
    #[tokio::test]
    async fn corrupt_due_claim_is_quarantined_without_starving_next_claim() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let poison =
            match claim_slack_event(&state, input(tenant.clone(), "Ev-a-poison", "fp-a-poison"))
                .await
                .unwrap()
            {
                SlackEventClaimDecision::Claimed(claim) => claim,
                other => panic!("expected poison claim, got {other:?}"),
            };
        let healthy = match claim_slack_event(&state, input(tenant, "Ev-b-healthy", "fp-b-healthy"))
            .await
            .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected healthy claim, got {other:?}"),
        };
        assert!(retry_slack_event_claim(&poison, "retry", 2_000)
            .await
            .unwrap());
        assert!(retry_slack_event_claim(&healthy, "retry", 2_000)
            .await
            .unwrap());

        let mut poison_record = read_record(&poison.record_path)
            .await
            .unwrap()
            .expect("poison record");
        poison_record.recovery_payload = Some("plaintext poison".to_string());
        write_record(&poison.record_path, &poison_record)
            .await
            .unwrap();

        let recovered = recover_slack_event_claims(&state, 3_000, 1).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].claim.key, healthy.key);
        let poison_record = read_record(&poison.record_path)
            .await
            .unwrap()
            .expect("quarantined poison record");
        assert_eq!(poison_record.status, SlackEventClaimStatus::Quarantined);
        assert!(poison_record.recovery_payload.is_none());
    }
    #[tokio::test]
    async fn default_local_claim_storage_never_contains_secret_plaintext() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(
            &state,
            input(tenant, "Ev-local-ciphertext", "fp-local-ciphertext"),
        )
        .await
        .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(stage_slack_event_response(
            &claimed,
            "session-local-ciphertext",
            "classified local staged reply",
            2_000,
        )
        .await
        .unwrap());
        let stored = tokio::fs::read_to_string(&claimed.record_path)
            .await
            .expect("read locally protected claim");
        assert!(!stored.contains("recovery-Ev-local-ciphertext"));
        assert!(!stored.contains("classified local staged reply"));
        assert!(
            stored.contains(LOCAL_CLAIM_RECORD_PREFIX)
                || stored.contains(crate::encrypted_file_store::SCOPED_RECORD_PREFIX)
        );
    }

    #[tokio::test]
    async fn compaction_prunes_completed_claims_by_age_and_count() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let mut paths = Vec::new();
        for (index, completed_at_ms) in [10u64, 80, 90, 100].into_iter().enumerate() {
            let claimed = match claim_slack_event(
                &state,
                SlackEventClaimInput {
                    now_ms: completed_at_ms.saturating_sub(1),
                    ..input(
                        tenant.clone(),
                        &format!("Ev-compact-{index}"),
                        &format!("fp-compact-{index}"),
                    )
                },
            )
            .await
            .unwrap()
            {
                SlackEventClaimDecision::Claimed(claim) => claim,
                other => panic!("expected claim, got {other:?}"),
            };
            assert!(
                complete_slack_event_claim(&claimed, "session", completed_at_ms)
                    .await
                    .unwrap()
            );
            paths.push(claimed.record_path.clone());
        }

        let removed =
            compact_slack_event_claims_with_limits(&state, 110, Duration::from_millis(50), 2)
                .await
                .unwrap();
        assert_eq!(removed, 2);
        assert!(
            !paths[0].exists(),
            "expired completed claim must be removed"
        );
        assert!(
            !paths[1].exists(),
            "oldest claim beyond cap must be removed"
        );
        assert!(paths[2].exists());
        assert!(paths[3].exists());
    }

    #[tokio::test]
    async fn compaction_applies_bounded_retention_to_quarantined_claims() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit_user_workspace("acme", "hq", None, "actor-a");
        let claimed = match claim_slack_event(
            &state,
            input(tenant, "Ev-quarantine-retention", "fp-quarantine-retention"),
        )
        .await
        .unwrap()
        {
            SlackEventClaimDecision::Claimed(claim) => claim,
            other => panic!("expected claim, got {other:?}"),
        };
        assert!(
            quarantine_slack_event_claim(&claimed, "terminal poison", 2_000)
                .await
                .unwrap()
        );
        assert_eq!(
            compact_slack_event_claims_with_limits(
                &state,
                2_001,
                Duration::from_millis(1),
                MAX_TERMINAL_CLAIMS,
            )
            .await
            .unwrap(),
            1
        );
        assert!(!claimed.record_path.exists());
    }
}
