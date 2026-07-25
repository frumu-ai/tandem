// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tandem_types::{GovernanceRequesterContext, TenantContext, TenantSource};
use tokio::fs;
use uuid::Uuid;

use crate::{now_ms, AppState};

const AUDIT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDurability {
    BestEffort,
    DurableRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedAuditEnvelope {
    pub event_id: String,
    pub durability: AuditDurability,
    pub event_type: String,
    #[serde(default)]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_context: Option<GovernanceRequesterContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub payload: Value,
    pub created_at_ms: u64,
    // Hash-chain fields (schema version >= 2). Default-deserialized so
    // pre-v2 records round-trip cleanly.
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub prev_hash: Option<String>,
    #[serde(default)]
    pub record_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<AuditRecordIntegrity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecordIntegrity {
    pub algorithm: String,
    pub key_id: String,
    #[serde(default)]
    pub segment_start: bool,
}

/// Canonical form for hashing: mirrors every field of `ProtectedAuditEnvelope`
/// except `record_hash` (which is being computed). The `actor` field is always
/// serialized here (no skip_serializing_if) so the canonical JSON is stable.
#[derive(Serialize)]
struct AuditEnvelopeForHashing<'a> {
    event_id: &'a str,
    durability_str: &'a str,
    event_type: &'a str,
    tenant_org_id: &'a str,
    tenant_workspace_id: &'a str,
    tenant_deployment_id: &'a Option<String>,
    tenant_actor_id: &'a Option<String>,
    tenant_source: &'a TenantSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    requester_context: Option<&'a GovernanceRequesterContext>,
    actor: &'a Option<String>,
    payload: &'a Value,
    created_at_ms: u64,
    seq: u64,
    prev_hash: &'a Option<String>,
}

#[derive(Serialize)]
struct AuditEnvelopeForMac<'a> {
    canonical: AuditEnvelopeForHashing<'a>,
    integrity: &'a AuditRecordIntegrity,
}

fn durability_str(d: &AuditDurability) -> &'static str {
    match d {
        AuditDurability::BestEffort => "best_effort",
        AuditDurability::DurableRequired => "durable_required",
    }
}

fn canonical_audit_envelope(envelope: &ProtectedAuditEnvelope) -> AuditEnvelopeForHashing<'_> {
    AuditEnvelopeForHashing {
        event_id: &envelope.event_id,
        durability_str: durability_str(&envelope.durability),
        event_type: &envelope.event_type,
        tenant_org_id: &envelope.tenant_context.org_id,
        tenant_workspace_id: &envelope.tenant_context.workspace_id,
        tenant_deployment_id: &envelope.tenant_context.deployment_id,
        tenant_actor_id: &envelope.tenant_context.actor_id,
        tenant_source: &envelope.tenant_context.source,
        requester_context: envelope.requester_context.as_ref(),
        actor: &envelope.actor,
        payload: &envelope.payload,
        created_at_ms: envelope.created_at_ms,
        seq: envelope.seq,
        prev_hash: &envelope.prev_hash,
    }
}

pub(crate) fn compute_audit_envelope_hash(envelope: &ProtectedAuditEnvelope) -> String {
    let json = serde_json::to_string(&canonical_audit_envelope(envelope))
        .expect("audit envelope hash serialization is infallible");
    format!("{:x}", Sha256::digest(json.as_bytes()))
}

fn compute_audit_envelope_mac(
    envelope: &ProtectedAuditEnvelope,
    keyring: &crate::audit_integrity::AuditIntegrityKeyring,
) -> anyhow::Result<String> {
    let integrity = envelope
        .integrity
        .as_ref()
        .context("keyed audit record is missing integrity metadata")?;
    anyhow::ensure!(
        integrity.algorithm == "hmac-sha256",
        "unsupported protected audit integrity algorithm"
    );
    keyring.sign_with_key(
        &integrity.key_id,
        b"protected-audit-record",
        &serde_json::to_vec(&AuditEnvelopeForMac {
            canonical: canonical_audit_envelope(envelope),
            integrity,
        })?,
    )
}

fn protected_audit_chain_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "protected-audit".to_string());
    path.with_file_name(format!("{file_name}.chain.lock"))
}

struct ProtectedAuditChainLock {
    file: std::fs::File,
}

impl ProtectedAuditChainLock {
    async fn acquire(path: &Path) -> anyhow::Result<Self> {
        let lock_path = protected_audit_chain_lock_path(path);
        tokio::task::spawn_blocking(move || {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .with_context(|| {
                    format!("open protected audit chain lock {}", lock_path.display())
                })?;
            file.lock_exclusive().with_context(|| {
                format!("acquire protected audit chain lock {}", lock_path.display())
            })?;
            Ok(Self { file })
        })
        .await
        .context("join protected audit chain-lock acquisition")?
    }
}

impl Drop for ProtectedAuditChainLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
pub(crate) async fn reset_protected_audit_tail_for_test(_path: &std::path::Path) {}

fn parse_protected_audit_records(
    lines: impl IntoIterator<Item = impl AsRef<str>>,
) -> anyhow::Result<Vec<ProtectedAuditEnvelope>> {
    let mut records = Vec::new();
    for (line_index, line) in lines.into_iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        // Older appenders wrote the JSON value and its newline in separate
        // writes. Concurrent callers could therefore leave multiple complete
        // JSON objects adjacent on one physical JSONL line. Stream parsing
        // recovers those valid, self-delimiting records while still rejecting
        // truncated or otherwise malformed audit data.
        for record in serde_json::Deserializer::from_str(line).into_iter::<ProtectedAuditEnvelope>()
        {
            records.push(record.with_context(|| {
                format!(
                    "parse protected audit record at physical line {}",
                    line_index.saturating_add(1)
                )
            })?);
        }
    }
    Ok(records)
}

async fn read_protected_audit_records(
    path: &std::path::Path,
) -> anyhow::Result<Vec<ProtectedAuditEnvelope>> {
    let lines = match crate::encrypted_file_store::read_jsonl_records_file(
        path,
        &crate::governance_store::GovernanceStoreFile::ProtectedAudit.storage_context(),
    )
    .await
    {
        Ok(lines) => lines,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            ensure_protected_audit_ledger_is_uninitialized(path).await?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    parse_protected_audit_records(lines)
}

async fn ensure_protected_audit_ledger_is_uninitialized(
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let identity = protected_audit_anchor_identity(path)?;
    crate::audit_integrity::ensure_external_anchor_absent("protected-audit-ledger", &identity, path)
        .await
        .context("reject missing protected audit ledger against its dedicated external anchor")
}

fn protected_audit_anchor_identity(path: &Path) -> anyhow::Result<String> {
    crate::audit_integrity::canonical_state_file_identity(path)
}

pub fn protected_audit_event_matches_tenant(
    event: &ProtectedAuditEnvelope,
    tenant_context: &TenantContext,
) -> bool {
    tenant_context.is_local_implicit()
        || (event.tenant_context.org_id == tenant_context.org_id
            && event.tenant_context.workspace_id == tenant_context.workspace_id
            && event.tenant_context.deployment_id == tenant_context.deployment_id)
}

pub async fn try_load_protected_audit_events_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
) -> anyhow::Result<Vec<ProtectedAuditEnvelope>> {
    let lines = match crate::governance_store::for_state(state)
        .read_jsonl_lines(crate::governance_store::GovernanceStoreFile::ProtectedAudit)
        .await
    {
        Ok(Some(lines)) => lines,
        Ok(None) => {
            ensure_protected_audit_ledger_is_uninitialized(&state.protected_audit_path).await?;
            return Ok(Vec::new());
        }
        Err(error) => return Err(error).context("load protected audit ledger"),
    };
    let mut rows =
        parse_protected_audit_records(lines).context("parse decrypted protected audit ledger")?;
    let verification = verify_protected_audit_records(&rows);
    anyhow::ensure!(
        verification.valid,
        "protected audit ledger failed hash-chain verification: {:?}",
        verification.violation
    );
    verify_protected_audit_anchor(&state.protected_audit_path, &rows).await?;
    rows.retain(|event| protected_audit_event_matches_tenant(event, tenant_context));
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.event_id.cmp(&b.event_id))
    });
    Ok(rows)
}

pub async fn load_protected_audit_events_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Vec<ProtectedAuditEnvelope> {
    match try_load_protected_audit_events_for_tenant(state, tenant_context).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(
                path = %state.protected_audit_path.display(),
                error = ?error,
                "best-effort protected audit load failed"
            );
            Vec::new()
        }
    }
}

pub async fn append_protected_audit_event(
    state: &AppState,
    event_type: impl Into<String>,
    tenant_context: &TenantContext,
    actor: Option<String>,
    payload: Value,
) -> anyhow::Result<()> {
    let path = state.protected_audit_path.clone();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // This lock is distinct from the protected-store integrity lock. Every
    // audit writer takes it first and holds it through the append, so separate
    // Tandem processes cannot both select the same chain tail. The store lock
    // is then acquired in one consistent order, avoiding nested re-acquisition.
    let _chain_guard = ProtectedAuditChainLock::acquire(&path).await?;
    let authority = crate::audit_integrity::integrity_authority()?;
    let store_file = crate::governance_store::GovernanceStoreFile::ProtectedAudit;
    let governance_store = crate::governance_store::for_state(state);
    let legacy_lines = match governance_store.read_jsonl_lines(store_file).await? {
        Some(lines) => lines,
        None => {
            ensure_protected_audit_ledger_is_uninitialized(&path).await?;
            Vec::new()
        }
    };
    let records = parse_protected_audit_records(&legacy_lines)
        .context("parse protected audit ledger before append")?;
    let verification = verify_protected_audit_records(&records);
    anyhow::ensure!(
        verification.valid,
        "protected audit ledger failed hash-chain verification: {:?}",
        verification.violation
    );
    verify_protected_audit_anchor(&path, &records).await?;
    let migrating_unsequenced = records.iter().any(|record| record.seq == 0);
    let mut records = if migrating_unsequenced {
        migrate_unsequenced_audit_records(records, authority.as_ref())?
    } else {
        records
    };
    let last = records.last().cloned();
    let next_seq = last
        .as_ref()
        .map(|record| record.seq)
        .unwrap_or(0)
        .saturating_add(1);
    let prev_hash = last
        .as_ref()
        .map(|record| record.record_hash.clone())
        .filter(|hash| !hash.is_empty());
    let requester_context = requester_context_from_payload(&payload);

    let mut row = ProtectedAuditEnvelope {
        event_id: Uuid::new_v4().to_string(),
        durability: AuditDurability::DurableRequired,
        event_type: event_type.into(),
        tenant_context: tenant_context.clone(),
        requester_context,
        actor,
        payload,
        created_at_ms: now_ms(),
        seq: next_seq,
        prev_hash,
        record_hash: String::new(),
        integrity: authority.as_ref().map(|keyring| AuditRecordIntegrity {
            algorithm: "hmac-sha256".to_string(),
            key_id: keyring.active_key_id().to_string(),
            segment_start: last
                .as_ref()
                .and_then(|record| record.integrity.as_ref())
                .map(|integrity| integrity.key_id.as_str())
                != Some(keyring.active_key_id()),
        }),
    };
    row.record_hash = match authority.as_ref() {
        Some(keyring) => compute_audit_envelope_mac(&row, keyring)?,
        None => compute_audit_envelope_hash(&row),
    };

    // Perform the write, and — for durable events — fsync so the record
    // survives power loss (flush() only reaches the OS page cache). The store
    // facade encrypts JSONL rows for the file-backed implementation.
    let serialized = serde_json::to_string(&row)?;
    let anchor_identity = protected_audit_anchor_identity(&path)?;
    let companion_anchor =
        row.integrity
            .as_ref()
            .map(|_| crate::audit_integrity::ExternalAnchorUpdate {
                scope: "protected-audit-ledger",
                identity: anchor_identity.as_str(),
                generation: row.seq,
                digest: row.record_hash.as_str(),
                previous: last
                    .as_ref()
                    .filter(|record| record.integrity.is_some())
                    .map(|record| (record.seq, record.record_hash.as_str())),
            });
    let write_result = if migrating_unsequenced {
        records.push(row.clone());
        let migrated_records = records
            .iter()
            .map(|record| {
                Ok((
                    serde_json::to_string(record)?,
                    store_file.record_context(&record.tenant_context, None, &record.event_id),
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        governance_store
            .migrate_jsonl_lines(
                store_file,
                &legacy_lines,
                &migrated_records,
                companion_anchor,
            )
            .await
    } else {
        governance_store
            .append_jsonl_line_with_anchor(
                store_file,
                &serialized,
                &row.tenant_context,
                None,
                &row.event_id,
                matches!(row.durability, AuditDurability::DurableRequired),
                companion_anchor,
            )
            .await
    };

    match write_result {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::error!(
                path = %path.display(),
                tenant_org_id = %row.tenant_context.org_id,
                tenant_workspace_id = %row.tenant_context.workspace_id,
                event_id = %row.event_id,
                error = ?err,
                "protected audit persistence failed"
            );
            Err(err)
        }
    }
}

fn migrate_unsequenced_audit_records(
    records: Vec<ProtectedAuditEnvelope>,
    authority: Option<&crate::audit_integrity::AuditIntegrityKeyring>,
) -> anyhow::Result<Vec<ProtectedAuditEnvelope>> {
    anyhow::ensure!(
        records.iter().all(|record| record.seq == 0),
        "cannot migrate a mixed sequenced and unsequenced protected audit ledger"
    );
    let mut migrated = Vec::with_capacity(records.len());
    let mut previous_hash = None;
    for (index, mut record) in records.into_iter().enumerate() {
        record.seq = index as u64 + 1;
        record.prev_hash = previous_hash;
        record.integrity = authority.map(|keyring| AuditRecordIntegrity {
            algorithm: "hmac-sha256".to_string(),
            key_id: keyring.active_key_id().to_string(),
            segment_start: index == 0,
        });
        record.record_hash = match authority {
            Some(keyring) => compute_audit_envelope_mac(&record, keyring)?,
            None => compute_audit_envelope_hash(&record),
        };
        previous_hash = Some(record.record_hash.clone());
        migrated.push(record);
    }
    Ok(migrated)
}

/// Append protected audit evidence without failing the caller.
///
/// This is reserved for non-enforcement telemetry where the primary operation
/// cannot report an audit persistence error to its caller. Enforcement denials,
/// consequential mutations, and success paths must call
/// [`append_protected_audit_event`] directly and propagate its result.
pub async fn append_protected_audit_event_best_effort(
    state: &AppState,
    event_type: impl Into<String>,
    tenant_context: &TenantContext,
    actor: Option<String>,
    payload: Value,
) {
    let event_type = event_type.into();
    if let Err(error) =
        append_protected_audit_event(state, event_type.clone(), tenant_context, actor, payload)
            .await
    {
        tracing::error!(
            event_type,
            tenant_org_id = %tenant_context.org_id,
            tenant_workspace_id = %tenant_context.workspace_id,
            error = ?error,
            "best-effort protected audit event was not persisted"
        );
    }
}

fn requester_context_from_payload(payload: &Value) -> Option<GovernanceRequesterContext> {
    payload
        .get("requester_context")
        .or_else(|| payload.get("requesterContext"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

// ── Verification ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AuditChainViolationKind {
    RecordHashMismatch {
        expected: String,
    },
    ChainBreak {
        expected_prev: String,
    },
    SeqGap {
        expected_seq: u64,
    },
    SeqReplay {
        seen_seq: u64,
    },
    RecordIntegrityFailure {
        key_id: Option<String>,
        reason: String,
    },
    LegacyRecordAfterKeyedSegment,
    UnsequencedRecordAfterSequencedLedger,
    IntegritySegmentBoundary {
        expected_segment_start: bool,
    },
    ExternalAnchorFailure {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditChainViolation {
    pub seq: u64,
    pub kind: AuditChainViolationKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditLedgerVerificationResult {
    pub valid: bool,
    pub record_count: u64,
    pub hashed_record_count: u64,
    pub root_hash: Option<String>,
    pub schema_version: u32,
    pub violation: Option<AuditChainViolation>,
}

pub async fn verify_protected_audit_ledger(
    path: &std::path::Path,
) -> AuditLedgerVerificationResult {
    let records = match read_protected_audit_records(path).await {
        Ok(records) => records,
        Err(_) => {
            return AuditLedgerVerificationResult {
                valid: false,
                record_count: 0,
                hashed_record_count: 0,
                root_hash: None,
                schema_version: 0,
                violation: None,
            }
        }
    };
    let verification = verify_protected_audit_records(&records);
    if !verification.valid {
        return verification;
    }
    if let Err(error) = verify_protected_audit_anchor(path, &records).await {
        return invalid_audit_verification(
            verification.record_count,
            verification.hashed_record_count,
            verification.schema_version,
            records.last().map(|record| record.seq).unwrap_or(0),
            AuditChainViolationKind::ExternalAnchorFailure {
                reason: error.to_string(),
            },
        );
    }
    verification
}

fn invalid_audit_verification(
    record_count: u64,
    hashed_record_count: u64,
    schema_version: u32,
    seq: u64,
    kind: AuditChainViolationKind,
) -> AuditLedgerVerificationResult {
    AuditLedgerVerificationResult {
        valid: false,
        record_count,
        hashed_record_count,
        root_hash: None,
        schema_version,
        violation: Some(AuditChainViolation { seq, kind }),
    }
}

fn verify_protected_audit_records(
    records: &[ProtectedAuditEnvelope],
) -> AuditLedgerVerificationResult {
    verify_protected_audit_records_with_keyring(
        records,
        crate::audit_integrity::verification_keyring(),
    )
}

fn verify_protected_audit_records_with_keyring(
    records: &[ProtectedAuditEnvelope],
    keyring: anyhow::Result<Option<crate::audit_integrity::AuditIntegrityKeyring>>,
) -> AuditLedgerVerificationResult {
    let record_count = records.len() as u64;
    let schema_version = if records.iter().any(|record| record.integrity.is_some()) {
        AUDIT_SCHEMA_VERSION
    } else if records.iter().any(|record| record.seq > 0) {
        2
    } else {
        1
    };

    let sequenced_ledger_exists = records
        .iter()
        .any(|record| record.seq > 0 || record.integrity.is_some());
    if sequenced_ledger_exists {
        if let Some(record) = records.iter().find(|record| record.seq == 0) {
            return invalid_audit_verification(
                record_count,
                0,
                schema_version,
                record.seq,
                AuditChainViolationKind::UnsequencedRecordAfterSequencedLedger,
            );
        }
    }

    let seq_records: Vec<_> = records.iter().filter(|event| event.seq > 0).collect();
    if !seq_records.is_empty() {
        let mut expected = 1u64;
        for record in &seq_records {
            if record.seq < expected {
                return invalid_audit_verification(
                    record_count,
                    0,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::SeqReplay {
                        seen_seq: record.seq,
                    },
                );
            }
            if record.seq > expected {
                return invalid_audit_verification(
                    record_count,
                    0,
                    schema_version,
                    expected,
                    AuditChainViolationKind::SeqGap {
                        expected_seq: expected,
                    },
                );
            }
            expected = expected.saturating_add(1);
        }
    }

    let hashed: Vec<_> = records.iter().filter(|event| event.seq > 0).collect();
    let hashed_record_count = hashed.len() as u64;
    let mut prev_hash: Option<String> = None;
    let mut previous_integrity_key_id: Option<&str> = None;
    let mut keyed_segment_seen = false;

    for record in &hashed {
        if let Some(integrity) = record.integrity.as_ref() {
            keyed_segment_seen = true;
            let expected_segment_start =
                previous_integrity_key_id != Some(integrity.key_id.as_str());
            if integrity.segment_start != expected_segment_start {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::IntegritySegmentBoundary {
                        expected_segment_start,
                    },
                );
            }
            let verification = keyring
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.to_string()))
                .and_then(|keyring| {
                    let keyring = keyring
                        .as_ref()
                        .context("audit integrity keyring is missing")?;
                    anyhow::ensure!(
                        integrity.algorithm == "hmac-sha256",
                        "unsupported protected audit integrity algorithm"
                    );
                    let payload = serde_json::to_vec(&AuditEnvelopeForMac {
                        canonical: canonical_audit_envelope(record),
                        integrity,
                    })?;
                    keyring.verify(
                        &integrity.key_id,
                        b"protected-audit-record",
                        &payload,
                        &record.record_hash,
                    )
                });
            if let Err(error) = verification {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::RecordIntegrityFailure {
                        key_id: Some(integrity.key_id.clone()),
                        reason: error.to_string(),
                    },
                );
            }
            previous_integrity_key_id = Some(&integrity.key_id);
        } else {
            if keyed_segment_seen {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::LegacyRecordAfterKeyedSegment,
                );
            }
            let expected_hash = compute_audit_envelope_hash(record);
            if record.record_hash.is_empty() || expected_hash != record.record_hash {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::RecordHashMismatch {
                        expected: expected_hash,
                    },
                );
            }
        }

        match prev_hash.as_ref() {
            None if record.prev_hash.is_some() => {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::ChainBreak {
                        expected_prev: String::new(),
                    },
                );
            }
            Some(expected) if record.prev_hash.as_deref() != Some(expected.as_str()) => {
                return invalid_audit_verification(
                    record_count,
                    hashed_record_count,
                    schema_version,
                    record.seq,
                    AuditChainViolationKind::ChainBreak {
                        expected_prev: expected.clone(),
                    },
                );
            }
            _ => {}
        }
        prev_hash = Some(record.record_hash.clone());
    }

    AuditLedgerVerificationResult {
        valid: true,
        record_count,
        hashed_record_count,
        root_hash: prev_hash,
        schema_version,
        violation: None,
    }
}

async fn verify_protected_audit_anchor(
    path: &Path,
    records: &[ProtectedAuditEnvelope],
) -> anyhow::Result<Option<crate::audit_integrity::AnchorVerification>> {
    let Some(last) = records.iter().rfind(|record| record.seq > 0) else {
        ensure_protected_audit_ledger_is_uninitialized(path).await?;
        return Ok(None);
    };
    crate::audit_integrity::verify_external_anchor(
        "protected-audit-ledger",
        &protected_audit_anchor_identity(path)?,
        last.seq,
        &last.record_hash,
        last.integrity.is_some(),
        path,
    )
    .await
    .map(Some)
}

pub(crate) async fn validate_protected_audit_ledger_if_present(
    path: &std::path::Path,
) -> anyhow::Result<()> {
    let records = match read_protected_audit_records(path).await {
        Ok(records) => records,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(())
        }
        Err(error) => return Err(error),
    };
    let verification = verify_protected_audit_records(&records);
    anyhow::ensure!(
        verification.valid,
        "protected audit ledger failed hash-chain verification: {:?}",
        verification.violation
    );
    verify_protected_audit_anchor(path, &records).await?;
    Ok(())
}

// ── Export manifest ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLedgerManifest {
    pub ledger_path: String,
    pub schema_version: u32,
    pub record_count: u64,
    pub last_seq: u64,
    pub root_hash: Option<String>,
    pub legacy_record_count: u64,
    pub keyed_record_count: u64,
    pub integrity_key_ids: Vec<String>,
    pub external_anchor: Option<crate::audit_integrity::AnchorVerification>,
    pub generated_at_ms: u64,
}

pub async fn generate_audit_ledger_manifest(
    path: &std::path::Path,
) -> anyhow::Result<AuditLedgerManifest> {
    let records = read_protected_audit_records(path)
        .await
        .context("read protected audit ledger for manifest")?;
    let result = verify_protected_audit_records(&records);
    anyhow::ensure!(
        result.valid,
        "protected audit ledger failed hash-chain verification: {:?}",
        result.violation
    );
    let last_seq = records.last().map(|event| event.seq).unwrap_or(0);
    let keyed_record_count = records
        .iter()
        .filter(|record| record.integrity.is_some())
        .count() as u64;
    let legacy_record_count = result
        .hashed_record_count
        .saturating_sub(keyed_record_count);
    let integrity_key_ids = records
        .iter()
        .filter_map(|record| {
            record
                .integrity
                .as_ref()
                .map(|integrity| integrity.key_id.clone())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let external_anchor = verify_protected_audit_anchor(path, &records).await?;
    Ok(AuditLedgerManifest {
        ledger_path: path.to_string_lossy().into_owned(),
        schema_version: result.schema_version,
        record_count: result.record_count,
        last_seq,
        root_hash: result.root_hash,
        legacy_record_count,
        keyed_record_count,
        integrity_key_ids,
        external_anchor,
        generated_at_ms: now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_memory::MemoryCryptoProvider;

    fn audit_row(seq: u64, prev_hash: Option<String>) -> ProtectedAuditEnvelope {
        let mut row = ProtectedAuditEnvelope {
            event_id: format!("event-{seq}"),
            durability: AuditDurability::DurableRequired,
            event_type: "governance.test".to_string(),
            tenant_context: TenantContext::local_implicit(),
            requester_context: None,
            actor: Some("tester".to_string()),
            payload: serde_json::json!({"seq": seq}),
            created_at_ms: seq,
            seq,
            prev_hash,
            record_hash: String::new(),
            integrity: None,
        };
        row.record_hash = compute_audit_envelope_hash(&row);
        row
    }

    fn chained_rows() -> Vec<ProtectedAuditEnvelope> {
        let first = audit_row(1, None);
        let second = audit_row(2, Some(first.record_hash.clone()));
        let third = audit_row(3, Some(second.record_hash.clone()));
        vec![first, second, third]
    }

    fn keyed_audit_row(
        seq: u64,
        prev_hash: Option<String>,
        keyring: &crate::audit_integrity::AuditIntegrityKeyring,
        segment_start: bool,
    ) -> ProtectedAuditEnvelope {
        let mut row = ProtectedAuditEnvelope {
            event_id: format!("event-{seq}"),
            durability: AuditDurability::DurableRequired,
            event_type: "governance.keyed-test".to_string(),
            tenant_context: TenantContext::local_implicit(),
            requester_context: None,
            actor: Some("tester".to_string()),
            payload: serde_json::json!({"seq": seq}),
            created_at_ms: seq,
            seq,
            prev_hash,
            record_hash: String::new(),
            integrity: Some(AuditRecordIntegrity {
                algorithm: "hmac-sha256".to_string(),
                key_id: keyring.active_key_id().to_string(),
                segment_start,
            }),
        };
        row.record_hash = compute_audit_envelope_mac(&row, keyring).expect("sign keyed row");
        row
    }

    #[test]
    fn keyed_audit_segments_detect_public_rewrites_and_verify_rotation() {
        const OLD_KEY: &str = "old-audit-integrity-secret-material-32-bytes";
        const NEW_KEY: &str = "new-audit-integrity-secret-material-32-bytes";
        let old = crate::audit_integrity::test_keyring("old", OLD_KEY, &[]);
        let rotated = crate::audit_integrity::test_keyring("new", NEW_KEY, &[("old", OLD_KEY)]);

        let legacy = audit_row(1, None);
        let old_first = keyed_audit_row(2, Some(legacy.record_hash.clone()), &old, true);
        let old_second = keyed_audit_row(3, Some(old_first.record_hash.clone()), &old, false);
        let new_first = keyed_audit_row(4, Some(old_second.record_hash.clone()), &rotated, true);
        let rows = vec![legacy, old_first, old_second, new_first];
        assert!(
            verify_protected_audit_records_with_keyring(&rows, Ok(Some(rotated.clone())),).valid
        );

        let mut publicly_recomputed = rows.clone();
        publicly_recomputed[1].payload = serde_json::json!({"seq": 2, "rewritten": true});
        publicly_recomputed[1].record_hash = compute_audit_envelope_hash(&publicly_recomputed[1]);
        assert!(
            !verify_protected_audit_records_with_keyring(
                &publicly_recomputed,
                Ok(Some(rotated.clone())),
            )
            .valid
        );

        let mut hidden_rotation = rows.clone();
        hidden_rotation[3]
            .integrity
            .as_mut()
            .expect("integrity")
            .segment_start = false;
        hidden_rotation[3].record_hash =
            compute_audit_envelope_mac(&hidden_rotation[3], &rotated).expect("resign");
        assert!(
            !verify_protected_audit_records_with_keyring(
                &hidden_rotation,
                Ok(Some(rotated.clone())),
            )
            .valid
        );

        let missing_old = crate::audit_integrity::test_keyring("new", NEW_KEY, &[]);
        assert!(!verify_protected_audit_records_with_keyring(&rows, Ok(Some(missing_old)),).valid);

        let mut downgraded = rows.clone();
        downgraded.push(audit_row(5, Some(rows[3].record_hash.clone())));
        assert!(
            !verify_protected_audit_records_with_keyring(&downgraded, Ok(Some(rotated.clone())),)
                .valid
        );

        let mut unsequenced_prefix = vec![audit_row(0, None)];
        unsequenced_prefix.extend(rows.clone());
        assert!(
            !verify_protected_audit_records_with_keyring(
                &unsequenced_prefix,
                Ok(Some(rotated.clone())),
            )
            .valid
        );

        let mut unsequenced_downgrade = rows;
        unsequenced_downgrade.push(audit_row(0, None));
        assert!(
            !verify_protected_audit_records_with_keyring(
                &unsequenced_downgrade,
                Ok(Some(rotated)),
            )
            .valid
        );
    }

    #[tokio::test]
    async fn missing_anchored_audit_ledger_fails_closed() {
        let state = tempfile::tempdir().expect("state directory");
        let anchors = tempfile::tempdir().expect("anchor directory");
        let path = state.path().join("missing-audit.jsonl");

        crate::audit_integrity::with_test_anchor_dir(anchors.path().to_path_buf(), async {
            crate::audit_integrity::write_test_anchor_marker(
                "protected-audit-ledger",
                &protected_audit_anchor_identity(&path).expect("canonical audit anchor identity"),
                &path,
            )
            .expect("write audit anchor marker");

            let unsequenced_error = verify_protected_audit_anchor(&path, &[audit_row(0, None)])
                .await
                .expect_err("unsequenced-only ledger must reject a dedicated anchor");
            assert!(format!("{unsequenced_error:#}").contains("previously keyed"));

            let error = read_protected_audit_records(&path)
                .await
                .expect_err("missing anchored audit ledger must fail");
            assert!(format!("{error:#}").contains("previously keyed"));

            let mut app_state = crate::test_support::test_state().await;
            app_state.protected_audit_path = path.clone();
            let tenant_error = try_load_protected_audit_events_for_tenant(
                &app_state,
                &TenantContext::local_implicit(),
            )
            .await
            .expect_err("tenant loader must reject a missing dedicated audit anchor");
            assert!(format!("{tenant_error:#}").contains("previously keyed"));
        })
        .await;
    }

    #[test]
    fn protected_audit_parser_recovers_legacy_concatenated_jsonl_records() {
        let rows = chained_rows();
        let concatenated = format!(
            "{}{}",
            serde_json::to_string(&rows[0]).expect("serialize first row"),
            serde_json::to_string(&rows[1]).expect("serialize second row")
        );
        let third = serde_json::to_string(&rows[2]).expect("serialize third row");

        let parsed = parse_protected_audit_records([concatenated, String::new(), third])
            .expect("parse legacy concatenated records");

        assert_eq!(
            parsed.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(verify_protected_audit_records(&parsed).valid);
        assert!(parse_protected_audit_records(["{not-json}"]).is_err());
    }

    #[tokio::test]
    async fn append_migrates_unsequenced_audit_rows_into_verified_keyed_chain() {
        let mut state = crate::test_support::test_state().await;
        let root = tempfile::tempdir().expect("migration root");
        state.protected_audit_path = root.path().join("state").join("protected-audit.jsonl");
        let anchors = root.path().join("anchors");
        fs::create_dir_all(
            state
                .protected_audit_path
                .parent()
                .expect("protected audit parent"),
        )
        .await
        .expect("protected audit directory");

        let mut first = audit_row(0, None);
        first.event_id = "legacy-one".to_string();
        first.payload = serde_json::json!({"legacy": 1});
        first.record_hash.clear();
        let mut second = audit_row(0, None);
        second.event_id = "legacy-two".to_string();
        second.payload = serde_json::json!({"legacy": 2});
        second.record_hash.clear();
        fs::write(
            &state.protected_audit_path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&first).expect("serialize first legacy row"),
                serde_json::to_string(&second).expect("serialize second legacy row")
            ),
        )
        .await
        .expect("legacy audit ledger");

        let authority = crate::audit_integrity::test_keyring(
            "active",
            "audit-migration-integrity-secret-32-bytes",
            &[],
        );
        crate::encrypted_file_store::with_test_crypto_provider(
            MemoryCryptoProvider::plaintext(),
            None,
            crate::audit_integrity::with_test_keyring(
                Some(authority),
                crate::audit_integrity::with_test_anchor_dir(anchors, async {
                    append_protected_audit_event(
                        &state,
                        "governance.after-migration",
                        &TenantContext::local_implicit(),
                        Some("tester".to_string()),
                        serde_json::json!({"new": true}),
                    )
                    .await
                    .expect("append after legacy migration");

                    let rows = read_protected_audit_records(&state.protected_audit_path)
                        .await
                        .expect("read migrated audit ledger");
                    assert_eq!(rows[0].event_id, "legacy-one");
                    assert_eq!(rows[1].event_id, "legacy-two");
                    assert_eq!(rows[2].event_type, "governance.after-migration");
                    assert_eq!(
                        rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
                        vec![1, 2, 3]
                    );
                    assert_eq!(rows[0].payload, serde_json::json!({"legacy": 1}));
                    assert_eq!(rows[1].payload, serde_json::json!({"legacy": 2}));
                    assert!(rows.iter().all(|row| row.integrity.is_some()));
                    assert!(verify_protected_audit_records(&rows).valid);
                    verify_protected_audit_anchor(&state.protected_audit_path, &rows)
                        .await
                        .expect("verify migrated audit anchor");
                }),
            ),
        )
        .await;
    }

    #[test]
    fn normal_audit_chain_verification_rejects_deletion_reorder_replay_and_edit() {
        let rows = chained_rows();
        assert!(verify_protected_audit_records(&rows).valid);

        let deleted = vec![rows[0].clone(), rows[2].clone()];
        assert!(!verify_protected_audit_records(&deleted).valid);

        let reordered = vec![rows[1].clone(), rows[0].clone(), rows[2].clone()];
        assert!(!verify_protected_audit_records(&reordered).valid);

        let replayed = vec![rows[0].clone(), rows[1].clone(), rows[1].clone()];
        assert!(!verify_protected_audit_records(&replayed).valid);

        let mut edited = rows;
        edited[1].payload = serde_json::json!({"seq": 2, "edited": true});
        assert!(!verify_protected_audit_records(&edited).valid);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_independent_owners_append_distinct_audit_sequences() {
        let state = crate::test_support::test_state().await;
        let first_owner = state.clone();
        let second_owner = state.clone();
        let tenant = TenantContext::local_implicit();
        let first_tenant = tenant.clone();
        let second_tenant = tenant.clone();
        let start = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let first_start = start.clone();

        let first = tokio::spawn(async move {
            first_start.wait().await;
            append_protected_audit_event(
                &first_owner,
                "governance.concurrent.first",
                &first_tenant,
                Some("owner-one".to_string()),
                serde_json::json!({"owner": 1}),
            )
            .await
        });
        let second = tokio::spawn(async move {
            start.wait().await;
            append_protected_audit_event(
                &second_owner,
                "governance.concurrent.second",
                &second_tenant,
                Some("owner-two".to_string()),
                serde_json::json!({"owner": 2}),
            )
            .await
        });

        first
            .await
            .expect("first owner task")
            .expect("first append");
        second
            .await
            .expect("second owner task")
            .expect("second append");

        let rows = read_protected_audit_records(&state.protected_audit_path)
            .await
            .expect("read concurrent audit rows");
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(verify_protected_audit_records(&rows).valid);
    }

    #[tokio::test]
    async fn strict_tenant_loader_rejects_corrupt_ledger_instead_of_returning_empty() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        append_protected_audit_event(
            &state,
            "governance.test",
            &tenant,
            Some("tester".to_string()),
            serde_json::json!({"result":"persisted"}),
        )
        .await
        .expect("append protected audit event");

        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&state.protected_audit_path)
            .await
            .expect("open protected audit ledger");
        file.write_all(b"corrupt-trailer\n")
            .await
            .expect("corrupt protected audit ledger");
        file.sync_all().await.expect("sync corruption");

        assert!(try_load_protected_audit_events_for_tenant(&state, &tenant)
            .await
            .is_err());
        assert!(generate_audit_ledger_manifest(&state.protected_audit_path)
            .await
            .is_err());
    }
}
