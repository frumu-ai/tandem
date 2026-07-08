use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

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

/// Cached tail of the ledger for a given path, guarded by the per-path append
/// lock. Lets `append_protected_audit_event` derive the next `seq`/`prev_hash`
/// without re-reading and re-parsing the whole file on every append (which made
/// appends O(n) and ledger growth O(n²)). `None` means "not yet seeded this
/// process"; the first append for a path seeds it by reading the file once.
#[derive(Clone)]
struct LastAuditRecord {
    seq: u64,
    record_hash: String,
}

async fn protected_audit_lock_for(
    path: &std::path::Path,
) -> Arc<tokio::sync::Mutex<Option<LastAuditRecord>>> {
    static LOCKS: OnceLock<
        tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Option<LastAuditRecord>>>>>,
    > = OnceLock::new();
    let map = LOCKS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = map.lock().await;
    guard
        .entry(path.to_string_lossy().to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
        .clone()
}

async fn read_last_protected_audit_record(
    path: &std::path::Path,
) -> Option<ProtectedAuditEnvelope> {
    let content = fs::read_to_string(path).await.ok()?;
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<ProtectedAuditEnvelope>(line.trim()).ok())
        .max_by_key(|e| e.seq)
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

pub async fn load_protected_audit_events_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Vec<ProtectedAuditEnvelope> {
    let content = match crate::governance_store::for_state(state)
        .read_text(crate::governance_store::GovernanceStoreFile::ProtectedAudit)
        .await
    {
        Ok(Some(content)) => content,
        Ok(None) | Err(_) => return Vec::new(),
    };
    let mut rows = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<ProtectedAuditEnvelope>(trimmed).ok()
        })
        .filter(|event| protected_audit_event_matches_tenant(event, tenant_context))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.event_id.cmp(&b.event_id))
    });
    rows
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

    let audit_lock = protected_audit_lock_for(&path).await;
    let mut tail = audit_lock.lock().await;

    // Seed the cached tail from disk once per process (cold cache); thereafter
    // the last seq/hash is carried in memory under this same lock, so appends
    // do not re-read the whole ledger.
    if tail.is_none() {
        *tail = Some(match read_last_protected_audit_record(&path).await {
            Some(e) => LastAuditRecord {
                seq: e.seq,
                record_hash: e.record_hash,
            },
            None => LastAuditRecord {
                seq: 0,
                record_hash: String::new(),
            },
        });
    }
    let last = tail.as_ref().expect("audit tail seeded above");
    let next_seq = last.seq.saturating_add(1);
    let prev_hash = if last.record_hash.is_empty() {
        None
    } else {
        Some(last.record_hash.clone())
    };
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
    // survives power loss (flush() only reaches the OS page cache).
    let serialized = serde_json::to_string(&row)?;
    let write_result = crate::governance_store::for_state(state)
        .append_jsonl_line(
            crate::governance_store::GovernanceStoreFile::ProtectedAudit,
            &serialized,
            matches!(row.durability, AuditDurability::DurableRequired),
        )
        .await;

    match write_result {
        Ok(()) => {
            // Success: advance the cached tail so the next append chains from
            // this record without re-reading the file.
            *tail = Some(LastAuditRecord {
                seq: row.seq,
                record_hash: row.record_hash.clone(),
            });
            Ok(())
        }
        Err(err) => {
            // A partial write or a failed fsync may have made this row (or part
            // of it) visible in the file while the cached tail still points at
            // the previous record. Invalidate the cache so the next append
            // re-seeds seq/prev_hash from disk and cannot reuse this sequence.
            *tail = None;
            Err(err)
        }
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
    let content = match fs::read_to_string(path).await {
        Ok(c) => c,
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

    let mut records: Vec<ProtectedAuditEnvelope> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect();
    records.sort_by_key(|e| e.seq);

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

    let hashed: Vec<_> = records
        .iter()
        .filter(|e| !e.record_hash.is_empty())
        .collect();
    let hashed_record_count = hashed.len() as u64;
    let mut prev_hash: Option<String> = None;

    for record in &hashed {
        let expected_hash = compute_audit_envelope_hash(record);
        if expected_hash != record.record_hash {
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
        if let Some(ref expected) = prev_hash {
            if record.prev_hash.as_deref() != Some(expected.as_str()) {
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
    let result = verify_protected_audit_ledger(path).await;
    let last_seq = read_last_protected_audit_record(path)
        .await
        .map(|e| e.seq)
        .unwrap_or(0);
    Ok(AuditLedgerManifest {
        ledger_path: path.to_string_lossy().into_owned(),
        schema_version: result.schema_version,
        record_count: result.record_count,
        last_seq,
        root_hash: result.root_hash,
        generated_at_ms: now_ms(),
    })
}
