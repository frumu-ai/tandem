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

const AUDIT_SCHEMA_VERSION: u32 = 2;

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

fn durability_str(d: &AuditDurability) -> &'static str {
    match d {
        AuditDurability::BestEffort => "best_effort",
        AuditDurability::DurableRequired => "durable_required",
    }
}

pub(crate) fn compute_audit_envelope_hash(envelope: &ProtectedAuditEnvelope) -> String {
    let for_hashing = AuditEnvelopeForHashing {
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
    };
    let json = serde_json::to_string(&for_hashing)
        .expect("audit envelope hash serialization is infallible");
    format!("{:x}", Sha256::digest(json.as_bytes()))
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
    for line in lines {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str::<ProtectedAuditEnvelope>(line)
            .context("parse protected audit record")?;
        records.push(record);
    }
    Ok(records)
}

async fn read_protected_audit_records(
    path: &std::path::Path,
) -> anyhow::Result<Vec<ProtectedAuditEnvelope>> {
    let lines = crate::encrypted_file_store::read_jsonl_records_file(
        path,
        &crate::governance_store::GovernanceStoreFile::ProtectedAudit.storage_context(),
    )
    .await?;
    parse_protected_audit_records(lines)
}

async fn read_last_protected_audit_record(
    path: &std::path::Path,
) -> anyhow::Result<Option<ProtectedAuditEnvelope>> {
    let records = match read_protected_audit_records(path).await {
        Ok(records) => records,
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        Err(err) => return Err(err),
    };
    let verification = verify_protected_audit_records(&records);
    anyhow::ensure!(
        verification.valid,
        "protected audit ledger failed hash-chain verification: {:?}",
        verification.violation
    );
    Ok(records.into_iter().last())
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
        Ok(None) => return Ok(Vec::new()),
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
    let last = read_last_protected_audit_record(&path).await?;
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
    };
    row.record_hash = compute_audit_envelope_hash(&row);

    // Perform the write, and — for durable events — fsync so the record
    // survives power loss (flush() only reaches the OS page cache). The store
    // facade encrypts JSONL rows for the file-backed implementation.
    let serialized = serde_json::to_string(&row)?;
    let write_result = crate::governance_store::for_state(state)
        .append_jsonl_line(
            crate::governance_store::GovernanceStoreFile::ProtectedAudit,
            &serialized,
            &row.tenant_context,
            None,
            &row.event_id,
            matches!(row.durability, AuditDurability::DurableRequired),
        )
        .await;

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

/// Append protected audit evidence without failing the caller.
///
/// This is reserved for denial evidence, telemetry, and other paths where the
/// primary operation cannot report an audit persistence error to its caller.
/// Consequential mutation and success paths must call
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
    RecordHashMismatch { expected: String },
    ChainBreak { expected_prev: String },
    SeqGap { expected_seq: u64 },
    SeqReplay { seen_seq: u64 },
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
    verify_protected_audit_records(&records)
}

fn verify_protected_audit_records(
    records: &[ProtectedAuditEnvelope],
) -> AuditLedgerVerificationResult {
    let record_count = records.len() as u64;
    let schema_version = records
        .iter()
        .find(|e| e.seq > 0)
        .map(|_| AUDIT_SCHEMA_VERSION)
        .unwrap_or(1);

    // Seq monotonicity check across all records (skip seq=0 pre-v2 records).
    let seq_records: Vec<_> = records.iter().filter(|e| e.seq > 0).collect();
    if !seq_records.is_empty() {
        let mut expected = 1u64;
        for record in &seq_records {
            if record.seq < expected {
                return AuditLedgerVerificationResult {
                    valid: false,
                    record_count,
                    hashed_record_count: 0,
                    root_hash: None,
                    schema_version,
                    violation: Some(AuditChainViolation {
                        seq: record.seq,
                        kind: AuditChainViolationKind::SeqReplay {
                            seen_seq: record.seq,
                        },
                    }),
                };
            }
            if record.seq > expected {
                return AuditLedgerVerificationResult {
                    valid: false,
                    record_count,
                    hashed_record_count: 0,
                    root_hash: None,
                    schema_version,
                    violation: Some(AuditChainViolation {
                        seq: expected,
                        kind: AuditChainViolationKind::SeqGap {
                            expected_seq: expected,
                        },
                    }),
                };
            }
            expected = expected.saturating_add(1);
        }
    }

    let hashed: Vec<_> = records.iter().filter(|e| e.seq > 0).collect();
    let hashed_record_count = hashed.len() as u64;
    let mut prev_hash: Option<String> = None;

    for record in &hashed {
        let expected_hash = compute_audit_envelope_hash(record);
        if record.record_hash.is_empty() || expected_hash != record.record_hash {
            return AuditLedgerVerificationResult {
                valid: false,
                record_count,
                hashed_record_count,
                root_hash: None,
                schema_version,
                violation: Some(AuditChainViolation {
                    seq: record.seq,
                    kind: AuditChainViolationKind::RecordHashMismatch {
                        expected: expected_hash,
                    },
                }),
            };
        }
        match prev_hash.as_ref() {
            None if record.prev_hash.is_some() => {
                return AuditLedgerVerificationResult {
                    valid: false,
                    record_count,
                    hashed_record_count,
                    root_hash: None,
                    schema_version,
                    violation: Some(AuditChainViolation {
                        seq: record.seq,
                        kind: AuditChainViolationKind::ChainBreak {
                            expected_prev: String::new(),
                        },
                    }),
                };
            }
            Some(expected) if record.prev_hash.as_deref() != Some(expected.as_str()) => {
                return AuditLedgerVerificationResult {
                    valid: false,
                    record_count,
                    hashed_record_count,
                    root_hash: None,
                    schema_version,
                    violation: Some(AuditChainViolation {
                        seq: record.seq,
                        kind: AuditChainViolationKind::ChainBreak {
                            expected_prev: expected.clone(),
                        },
                    }),
                };
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
    Ok(AuditLedgerManifest {
        ledger_path: path.to_string_lossy().into_owned(),
        schema_version: result.schema_version,
        record_count: result.record_count,
        last_seq,
        root_hash: result.root_hash,
        generated_at_ms: now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
