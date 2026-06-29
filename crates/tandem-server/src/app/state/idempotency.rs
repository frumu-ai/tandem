use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tandem_types::TenantContext;

use super::AppState;

const IDEMPOTENCY_KEYS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IdempotencyKeysFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    records: HashMap<String, IdempotencyKeyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyKeyStatus {
    Reserved,
    Completed,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyKeyOutcome {
    pub outcome_kind: String,
    pub completed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_ref_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_ref_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_ref_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_ref_id: Option<String>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyKeyRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub record_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    pub operation: String,
    pub key: String,
    pub owner: String,
    pub request_fingerprint: String,
    pub status: IdempotencyKeyStatus,
    pub first_seen_at_ms: u64,
    pub last_seen_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<IdempotencyKeyOutcome>,
    #[serde(default)]
    pub conflict_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflict_fingerprints: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IdempotencyReservationInput {
    pub tenant_context: TenantContext,
    pub operation: String,
    pub key: String,
    pub owner: String,
    pub request_fingerprint: String,
    pub first_seen_event_id: Option<String>,
    pub now_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IdempotencyReservation {
    Reserved(IdempotencyKeyRecord),
    Duplicate(IdempotencyKeyRecord),
    Conflict(IdempotencyKeyRecord),
}

impl IdempotencyReservation {
    pub fn record(&self) -> &IdempotencyKeyRecord {
        match self {
            Self::Reserved(record) | Self::Duplicate(record) | Self::Conflict(record) => record,
        }
    }
}

impl IdempotencyKeyRecord {
    pub fn tenant_matches(&self, tenant_context: &TenantContext) -> bool {
        tenant_context_matches(&self.tenant_context, tenant_context)
    }
}

impl AppState {
    pub(crate) async fn load_idempotency_keys(&self) -> anyhow::Result<()> {
        if !self.idempotency_keys_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&self.idempotency_keys_path)
            .await
            .with_context(|| {
                format!(
                    "failed to read idempotency keys {}",
                    self.idempotency_keys_path.display()
                )
            })?;
        let records = parse_idempotency_keys_file(&raw)?;
        *self.idempotency_keys.write().await = records;
        Ok(())
    }

    pub(crate) async fn reserve_idempotency_key(
        &self,
        input: IdempotencyReservationInput,
    ) -> anyhow::Result<IdempotencyReservation> {
        let key = normalized_non_empty(&input.key, "idempotency key")?;
        let operation = normalized_non_empty(&input.operation, "idempotency operation")?;
        let owner = normalized_non_empty(&input.owner, "idempotency owner")?;
        let request_fingerprint =
            normalized_non_empty(&input.request_fingerprint, "idempotency fingerprint")?;
        let record_id = idempotency_record_id(&input.tenant_context, &operation, &key);
        let _guard = self.idempotency_persistence.lock().await;
        let mut records = self.idempotency_keys.write().await;

        let result = match records.get_mut(&record_id) {
            Some(existing)
                if existing
                    .expires_at_ms
                    .map(|expires_at_ms| expires_at_ms <= input.now_ms)
                    .unwrap_or(false) =>
            {
                let record = new_idempotency_record(
                    record_id.clone(),
                    input,
                    key,
                    operation,
                    owner,
                    request_fingerprint,
                );
                *existing = record.clone();
                IdempotencyReservation::Reserved(record)
            }
            Some(existing) if existing.request_fingerprint == request_fingerprint => {
                existing.last_seen_at_ms = input.now_ms;
                IdempotencyReservation::Duplicate(existing.clone())
            }
            Some(existing) => {
                existing.status = IdempotencyKeyStatus::Conflicted;
                existing.last_seen_at_ms = input.now_ms;
                existing.conflict_count = existing.conflict_count.saturating_add(1);
                if !existing
                    .conflict_fingerprints
                    .iter()
                    .any(|fingerprint| fingerprint == &request_fingerprint)
                {
                    existing.conflict_fingerprints.push(request_fingerprint);
                }
                IdempotencyReservation::Conflict(existing.clone())
            }
            None => {
                let record = new_idempotency_record(
                    record_id.clone(),
                    input,
                    key,
                    operation,
                    owner,
                    request_fingerprint,
                );
                records.insert(record_id, record.clone());
                IdempotencyReservation::Reserved(record)
            }
        };

        let snapshot = records.clone();
        drop(records);
        self.persist_idempotency_keys_locked(snapshot).await?;
        Ok(result)
    }

    pub(crate) async fn complete_idempotency_key(
        &self,
        tenant_context: &TenantContext,
        operation: &str,
        key: &str,
        outcome: IdempotencyKeyOutcome,
        now_ms: u64,
    ) -> anyhow::Result<Option<IdempotencyKeyRecord>> {
        let operation = normalized_non_empty(operation, "idempotency operation")?;
        let key = normalized_non_empty(key, "idempotency key")?;
        let record_id = idempotency_record_id(tenant_context, &operation, &key);
        let _guard = self.idempotency_persistence.lock().await;
        let mut records = self.idempotency_keys.write().await;
        let Some(record) = records
            .get_mut(&record_id)
            .filter(|record| record.tenant_matches(tenant_context))
        else {
            return Ok(None);
        };
        record.status = IdempotencyKeyStatus::Completed;
        record.outcome = Some(outcome);
        record.last_seen_at_ms = now_ms;
        let updated = record.clone();
        let snapshot = records.clone();
        drop(records);
        self.persist_idempotency_keys_locked(snapshot).await?;
        Ok(Some(updated))
    }

    pub(crate) async fn get_idempotency_key(
        &self,
        tenant_context: &TenantContext,
        operation: &str,
        key: &str,
    ) -> Option<IdempotencyKeyRecord> {
        let operation = operation.trim();
        let key = key.trim();
        if operation.is_empty() || key.is_empty() {
            return None;
        }
        let record_id = idempotency_record_id(tenant_context, operation, key);
        self.idempotency_keys
            .read()
            .await
            .get(&record_id)
            .filter(|record| record.tenant_matches(tenant_context))
            .cloned()
    }

    async fn persist_idempotency_keys_locked(
        &self,
        records: HashMap<String, IdempotencyKeyRecord>,
    ) -> anyhow::Result<()> {
        let payload = serialize_idempotency_keys_file(records)?;
        if let Some(parent) = self.idempotency_keys_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        super::write_state_file_atomically(&self.idempotency_keys_path, payload).await
    }
}

fn new_idempotency_record(
    record_id: String,
    input: IdempotencyReservationInput,
    key: String,
    operation: String,
    owner: String,
    request_fingerprint: String,
) -> IdempotencyKeyRecord {
    IdempotencyKeyRecord {
        schema_version: IDEMPOTENCY_KEYS_SCHEMA_VERSION,
        record_id,
        tenant_context: input.tenant_context,
        operation,
        key,
        owner,
        request_fingerprint,
        status: IdempotencyKeyStatus::Reserved,
        first_seen_at_ms: input.now_ms,
        last_seen_at_ms: input.now_ms,
        first_seen_event_id: input.first_seen_event_id,
        expires_at_ms: input.expires_at_ms,
        outcome: None,
        conflict_count: 0,
        conflict_fingerprints: Vec::new(),
    }
}

fn parse_idempotency_keys_file(raw: &str) -> anyhow::Result<HashMap<String, IdempotencyKeyRecord>> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(raw).context("failed to parse idempotency keys")?;
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value).context("failed to parse legacy idempotency key map");
    }
    let file = serde_json::from_value::<IdempotencyKeysFile>(value)
        .context("failed to parse versioned idempotency key file")?;
    if file.schema_version > IDEMPOTENCY_KEYS_SCHEMA_VERSION {
        anyhow::bail!(
            "idempotency keys schema version {} is newer than supported version {}",
            file.schema_version,
            IDEMPOTENCY_KEYS_SCHEMA_VERSION
        );
    }
    Ok(file.records)
}

fn serialize_idempotency_keys_file(
    records: HashMap<String, IdempotencyKeyRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&IdempotencyKeysFile {
        schema_version: IDEMPOTENCY_KEYS_SCHEMA_VERSION,
        records,
    })
    .context("failed to serialize idempotency keys")
}

fn idempotency_record_id(tenant_context: &TenantContext, operation: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [
        tenant_context.org_id.as_str(),
        tenant_context.workspace_id.as_str(),
        tenant_context.deployment_id.as_deref().unwrap_or(""),
        operation,
        key,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("idem_{}", hex_encode(&hasher.finalize()))
}

pub(crate) fn idempotency_fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{}", hex_encode(&hasher.finalize()))
}

fn tenant_context_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn normalized_non_empty(value: &str, name: &str) -> anyhow::Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(normalized.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn default_schema_version() -> u32 {
    IDEMPOTENCY_KEYS_SCHEMA_VERSION
}

fn default_tenant_context() -> TenantContext {
    TenantContext::explicit_user_workspace("local", "default", None, "system")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "actor-a")
    }

    fn temp_state() -> AppState {
        let mut state = AppState::new_starting(Uuid::new_v4().to_string(), false);
        state.idempotency_keys_path =
            std::env::temp_dir().join(format!("idempotency-keys-{}.json", Uuid::new_v4()));
        state
    }

    fn input(
        tenant_context: TenantContext,
        operation: &str,
        key: &str,
        fingerprint: &str,
    ) -> IdempotencyReservationInput {
        IdempotencyReservationInput {
            tenant_context,
            operation: operation.to_string(),
            key: key.to_string(),
            owner: "test-owner".to_string(),
            request_fingerprint: fingerprint.to_string(),
            first_seen_event_id: Some("event-a".to_string()),
            now_ms: 1_000,
            expires_at_ms: None,
        }
    }

    #[tokio::test]
    async fn duplicate_reservation_returns_original_outcome() {
        let state = temp_state();
        let tenant_a = tenant("org-a", "workspace-a");
        let first = state
            .reserve_idempotency_key(input(
                tenant_a.clone(),
                "webhook.provider_event",
                "evt-1",
                "fingerprint-a",
            ))
            .await
            .expect("reserve first");
        let record = match first {
            IdempotencyReservation::Reserved(record) => record,
            other => panic!("expected reserve, got {other:?}"),
        };
        state
            .complete_idempotency_key(
                &tenant_a,
                "webhook.provider_event",
                "evt-1",
                IdempotencyKeyOutcome {
                    outcome_kind: "accepted".to_string(),
                    completed_at_ms: 1_100,
                    primary_ref_kind: Some("delivery".to_string()),
                    primary_ref_id: Some("delivery-a".to_string()),
                    secondary_ref_kind: Some("run".to_string()),
                    secondary_ref_id: Some("run-a".to_string()),
                    details: json!({ "dedupe_result": "accepted" }),
                },
                1_100,
            )
            .await
            .expect("complete key");

        let duplicate = state
            .reserve_idempotency_key(input(
                tenant_a,
                "webhook.provider_event",
                "evt-1",
                "fingerprint-a",
            ))
            .await
            .expect("reserve duplicate");

        match duplicate {
            IdempotencyReservation::Duplicate(duplicate) => {
                assert_eq!(duplicate.record_id, record.record_id);
                assert_eq!(
                    duplicate
                        .outcome
                        .as_ref()
                        .and_then(|outcome| outcome.primary_ref_id.as_deref()),
                    Some("delivery-a")
                );
                assert_eq!(
                    duplicate
                        .outcome
                        .as_ref()
                        .and_then(|outcome| outcome.secondary_ref_id.as_deref()),
                    Some("run-a")
                );
            }
            other => panic!("expected duplicate, got {other:?}"),
        }
        let _ = tokio::fs::remove_file(&state.idempotency_keys_path).await;
    }

    #[tokio::test]
    async fn idempotency_keys_are_tenant_scoped() {
        let state = temp_state();
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-a");

        let first = state
            .reserve_idempotency_key(input(tenant_a, "wait.wake", "wake-1", "fingerprint-a"))
            .await
            .expect("reserve tenant a");
        let second = state
            .reserve_idempotency_key(input(tenant_b, "wait.wake", "wake-1", "fingerprint-b"))
            .await
            .expect("reserve tenant b");

        assert!(matches!(first, IdempotencyReservation::Reserved(_)));
        assert!(matches!(second, IdempotencyReservation::Reserved(_)));
        assert_ne!(first.record().record_id, second.record().record_id);
        let _ = tokio::fs::remove_file(&state.idempotency_keys_path).await;
    }

    #[tokio::test]
    async fn conflicting_key_reuse_is_recorded() {
        let state = temp_state();
        let tenant_a = tenant("org-a", "workspace-a");
        state
            .reserve_idempotency_key(input(
                tenant_a.clone(),
                "outbox.send",
                "send-1",
                "fingerprint-a",
            ))
            .await
            .expect("reserve first");

        let conflict = state
            .reserve_idempotency_key(input(
                tenant_a.clone(),
                "outbox.send",
                "send-1",
                "fingerprint-b",
            ))
            .await
            .expect("reserve conflict");

        match conflict {
            IdempotencyReservation::Conflict(record) => {
                assert_eq!(record.status, IdempotencyKeyStatus::Conflicted);
                assert_eq!(record.conflict_count, 1);
                assert_eq!(record.conflict_fingerprints, vec!["fingerprint-b"]);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        let record = state
            .get_idempotency_key(&tenant_a, "outbox.send", "send-1")
            .await
            .expect("stored conflict");
        assert_eq!(record.status, IdempotencyKeyStatus::Conflicted);
        let _ = tokio::fs::remove_file(&state.idempotency_keys_path).await;
    }
}
