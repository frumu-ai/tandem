// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    ReleasePending,
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
    /// Distinguishes successive reservations of the same logical key.
    #[serde(default)]
    pub reservation_id: String,
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
        let _guard = self.idempotency_persistence.lock().await;
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
        let mut records = parse_idempotency_keys_file(&raw)?;
        let intents = match self.load_idempotency_release_intents().await {
            Ok(intents) => intents,
            Err(error) => {
                // Startup currently ignores load errors. Preserve the
                // reservations even if their recovery journal is unreadable.
                *self.idempotency_keys.write().await = records;
                return Err(error);
            }
        };
        for (id, record) in &mut records {
            if intents
                .get(id)
                .is_some_and(|intent| release_intent_matches(intent, record))
            {
                record.status = IdempotencyKeyStatus::ReleasePending;
            }
        }
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
            // The record ID is tenant-scoped, not actor-scoped. A different
            // owner must never receive or disturb the original reservation,
            // even when it presents the same key and request fingerprint.
            Some(existing) if existing.owner != owner => {
                IdempotencyReservation::Conflict(existing.clone())
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

    pub(crate) async fn release_reserved_idempotency_key(
        &self,
        tenant_context: &TenantContext,
        operation: &str,
        key: &str,
        request_fingerprint: &str,
    ) -> anyhow::Result<bool> {
        self.release_idempotency_key(tenant_context, operation, key, request_fingerprint, false)
            .await
    }

    pub(crate) async fn retry_pending_idempotency_release(
        &self,
        tenant_context: &TenantContext,
        operation: &str,
        key: &str,
        request_fingerprint: &str,
    ) -> anyhow::Result<bool> {
        self.release_idempotency_key(tenant_context, operation, key, request_fingerprint, true)
            .await
    }

    async fn release_idempotency_key(
        &self,
        tenant_context: &TenantContext,
        operation: &str,
        key: &str,
        request_fingerprint: &str,
        pending_only: bool,
    ) -> anyhow::Result<bool> {
        let operation = normalized_non_empty(operation, "idempotency operation")?;
        let key = normalized_non_empty(key, "idempotency key")?;
        let request_fingerprint =
            normalized_non_empty(request_fingerprint, "idempotency fingerprint")?;
        let record_id = idempotency_record_id(tenant_context, &operation, &key);
        let state = self.clone();
        let tenant_context = tenant_context.clone();
        // Keep persistence ordering through both writes even if the HTTP caller
        // disappears. The atomic file writer itself runs on a blocking task.
        tokio::spawn(async move {
            state
                .release_idempotency_record(
                    &tenant_context,
                    record_id,
                    request_fingerprint,
                    pending_only,
                )
                .await
        })
        .await
        .context("idempotency release task failed")?
    }

    async fn release_idempotency_record(
        &self,
        tenant_context: &TenantContext,
        record_id: String,
        request_fingerprint: String,
        pending_only: bool,
    ) -> anyhow::Result<bool> {
        let _guard = self.idempotency_persistence.lock().await;
        let mut records = self.idempotency_keys.write().await;
        let releasable = records
            .get(&record_id)
            .map(|record| {
                record.tenant_matches(tenant_context)
                    && (!pending_only || record.status == IdempotencyKeyStatus::ReleasePending)
                    && matches!(
                        record.status,
                        IdempotencyKeyStatus::Reserved | IdempotencyKeyStatus::ReleasePending
                    )
                    && record.request_fingerprint == request_fingerprint
            })
            .unwrap_or(false);
        if !releasable {
            return Ok(false);
        }
        // Publish intent before deleting the durable reservation. If deletion
        // fails or the process stops between writes, a fresh process can retry
        // this exact release without reclaiming unrelated active reservations.
        records
            .get_mut(&record_id)
            .expect("checked reservation")
            .status = IdempotencyKeyStatus::ReleasePending;
        let pending = records.clone();
        drop(records);
        // The journal has a separate replacement path, so a failed keys.json
        // replacement cannot erase the only durable evidence of this release.
        let mut intents = self.load_idempotency_release_intents().await?;
        intents.retain(|id, intent| {
            pending
                .get(id)
                .is_some_and(|record| release_intent_matches(intent, record))
        });
        intents.insert(record_id.clone(), pending[&record_id].clone());
        self.persist_idempotency_release_intents(intents.clone())
            .await
            .context("failed to journal idempotency release; reservation retained")?;
        self.persist_idempotency_keys_locked(pending.clone())
            .await
            .context("failed to persist idempotency release intent; reservation retained")?;
        #[cfg(test)]
        tests::interrupt_release_after_intent(&self.idempotency_keys_path);
        let mut released = pending;
        released.remove(&record_id);
        self.persist_idempotency_keys_locked(released).await?;
        self.idempotency_keys.write().await.remove(&record_id);
        intents.remove(&record_id);
        // A stale journal is harmless: its reservation identity cannot match a
        // future reuse of this key. Main-file deletion has already committed.
        if let Err(error) = self.persist_idempotency_release_intents(intents).await {
            tracing::warn!(
                ?error,
                "failed to prune committed idempotency release journal"
            );
        }
        Ok(true)
    }

    fn idempotency_release_journal_path(&self) -> PathBuf {
        let mut path = self.idempotency_keys_path.as_os_str().to_os_string();
        path.push(".release-intents.json");
        PathBuf::from(path)
    }

    async fn load_idempotency_release_intents(
        &self,
    ) -> anyhow::Result<HashMap<String, IdempotencyKeyRecord>> {
        let path = self.idempotency_release_journal_path();
        match tokio::fs::read_to_string(&path).await {
            Ok(raw) => {
                parse_idempotency_keys_file(&raw).context("invalid idempotency release journal")
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(error) => Err(error).context("failed to read idempotency release journal"),
        }
    }

    async fn persist_idempotency_release_intents(
        &self,
        intents: HashMap<String, IdempotencyKeyRecord>,
    ) -> anyhow::Result<()> {
        let path = self.idempotency_release_journal_path();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        super::write_state_file_atomically(&path, serialize_idempotency_keys_file(intents)?).await
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

fn release_intent_matches(intent: &IdempotencyKeyRecord, record: &IdempotencyKeyRecord) -> bool {
    intent.status == IdempotencyKeyStatus::ReleasePending
        && matches!(
            record.status,
            IdempotencyKeyStatus::Reserved | IdempotencyKeyStatus::ReleasePending
        )
        && intent.record_id == record.record_id
        && intent.reservation_id == record.reservation_id
        && intent.tenant_context == record.tenant_context
        && intent.operation == record.operation
        && intent.key == record.key
        && intent.owner == record.owner
        && intent.request_fingerprint == record.request_fingerprint
        && intent.first_seen_at_ms == record.first_seen_at_ms
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
        reservation_id: uuid::Uuid::new_v4().to_string(),
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

    static INTERRUPT_RELEASE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashSet<PathBuf>>,
    > = std::sync::LazyLock::new(Default::default);

    pub(super) fn interrupt_release_after_intent(path: &std::path::Path) {
        if INTERRUPT_RELEASE.lock().unwrap().remove(path) {
            // Exercise an actual atomic-writer failure after intent is durable.
            std::fs::create_dir(path.with_extension("tmp")).unwrap();
        }
    }

    #[tokio::test]
    async fn pending_release_survives_restart_without_reclaiming_active_work() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = temp_state();
        state.idempotency_keys_path = directory.path().join("keys.json");
        let tenant = tenant("restart-org", "restart-workspace");
        for key in ["release", "active"] {
            state
                .reserve_idempotency_key(input(
                    tenant.clone(),
                    "workflow_plan.apply",
                    key,
                    "fingerprint",
                ))
                .await
                .unwrap();
        }
        INTERRUPT_RELEASE
            .lock()
            .unwrap()
            .insert(state.idempotency_keys_path.clone());
        assert!(state
            .release_reserved_idempotency_key(
                &tenant,
                "workflow_plan.apply",
                "release",
                "fingerprint",
            )
            .await
            .is_err());
        let path = state.idempotency_keys_path.clone();
        drop(state);
        let mut restarted = temp_state();
        restarted.idempotency_keys_path = path.clone();
        restarted.load_idempotency_keys().await.unwrap();
        assert_eq!(
            restarted
                .get_idempotency_key(&tenant, "workflow_plan.apply", "release",)
                .await
                .unwrap()
                .status,
            IdempotencyKeyStatus::ReleasePending
        );
        std::fs::remove_dir(path.with_extension("tmp")).unwrap();
        assert!(!restarted
            .retry_pending_idempotency_release(&tenant, "workflow_plan.apply", "release", "wrong",)
            .await
            .unwrap());
        let other =
            TenantContext::explicit_user_workspace("other", "restart-workspace", None, "actor-a");
        assert!(!restarted
            .retry_pending_idempotency_release(
                &other,
                "workflow_plan.apply",
                "release",
                "fingerprint",
            )
            .await
            .unwrap());
        assert!(!restarted
            .retry_pending_idempotency_release(
                &tenant,
                "workflow_plan.apply",
                "active",
                "fingerprint",
            )
            .await
            .unwrap());
        assert!(restarted
            .retry_pending_idempotency_release(
                &tenant,
                "workflow_plan.apply",
                "release",
                "fingerprint",
            )
            .await
            .unwrap());
        restarted.load_idempotency_keys().await.unwrap();
        assert!(restarted
            .get_idempotency_key(&tenant, "workflow_plan.apply", "release")
            .await
            .is_none());
        assert_eq!(
            restarted
                .get_idempotency_key(&tenant, "workflow_plan.apply", "active",)
                .await
                .unwrap()
                .status,
            IdempotencyKeyStatus::Reserved
        );
    }

    #[tokio::test]
    async fn release_intent_write_failure_recovers_after_restart_and_storage_repair() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keys.json");
        let mut state = temp_state();
        state.idempotency_keys_path = path.clone();
        let tenant = tenant("intent-failure-org", "workspace");
        state
            .reserve_idempotency_key(input(
                tenant.clone(),
                "workflow_plan.apply",
                "key",
                "fingerprint",
            ))
            .await
            .unwrap();
        std::fs::create_dir(path.with_extension("tmp")).unwrap();
        assert!(state
            .release_reserved_idempotency_key(&tenant, "workflow_plan.apply", "key", "fingerprint",)
            .await
            .is_err());
        drop(state);
        std::fs::remove_dir(path.with_extension("tmp")).unwrap();
        let mut restarted = temp_state();
        restarted.idempotency_keys_path = path;
        restarted.load_idempotency_keys().await.unwrap();
        assert!(
            restarted
                .retry_pending_idempotency_release(
                    &tenant,
                    "workflow_plan.apply",
                    "key",
                    "fingerprint",
                )
                .await
                .unwrap(),
            "storage repair must not leave an abandoned release permanently reserved"
        );
    }

    #[tokio::test]
    async fn stale_release_journal_cannot_release_a_reused_key() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = temp_state();
        state.idempotency_keys_path = directory.path().join("keys.json");
        let tenant = tenant("journal-org", "workspace");
        let reservation = || input(tenant.clone(), "workflow_plan.apply", "key", "fingerprint");
        let first = state.reserve_idempotency_key(reservation()).await.unwrap();
        let obstruction = state.idempotency_keys_path.with_extension("tmp");
        std::fs::create_dir(&obstruction).unwrap();
        assert!(state
            .release_reserved_idempotency_key(&tenant, "workflow_plan.apply", "key", "fingerprint",)
            .await
            .is_err());
        let journal = tokio::fs::read(state.idempotency_release_journal_path())
            .await
            .unwrap();
        std::fs::remove_dir(obstruction).unwrap();
        assert!(
            state
                .retry_pending_idempotency_release(
                    &tenant,
                    "workflow_plan.apply",
                    "key",
                    "fingerprint",
                )
                .await
                .unwrap()
        );
        let second = state.reserve_idempotency_key(reservation()).await.unwrap();
        assert_ne!(
            first.record().reservation_id,
            second.record().reservation_id
        );
        // Simulate a crash after durable deletion but before journal pruning.
        tokio::fs::write(state.idempotency_release_journal_path(), journal)
            .await
            .unwrap();
        let path = state.idempotency_keys_path.clone();
        drop(state);
        let mut restarted = temp_state();
        restarted.idempotency_keys_path = path;
        restarted.load_idempotency_keys().await.unwrap();
        assert!(
            !restarted
                .retry_pending_idempotency_release(
                    &tenant,
                    "workflow_plan.apply",
                    "key",
                    "fingerprint",
                )
                .await
                .unwrap()
        );
        assert_eq!(
            restarted
                .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
                .await
                .unwrap()
                .status,
            IdempotencyKeyStatus::Reserved
        );
    }

    #[tokio::test]
    async fn unreadable_release_journal_does_not_discard_durable_reservations() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = temp_state();
        state.idempotency_keys_path = directory.path().join("keys.json");
        let tenant = tenant("corrupt-journal-org", "workspace");
        state
            .reserve_idempotency_key(input(
                tenant.clone(),
                "workflow_plan.apply",
                "key",
                "fingerprint",
            ))
            .await
            .unwrap();
        tokio::fs::write(state.idempotency_release_journal_path(), b"not json")
            .await
            .unwrap();
        let path = state.idempotency_keys_path.clone();
        drop(state);
        let mut restarted = temp_state();
        restarted.idempotency_keys_path = path;
        assert!(restarted.load_idempotency_keys().await.is_err());
        assert_eq!(
            restarted
                .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
                .await
                .unwrap()
                .status,
            IdempotencyKeyStatus::Reserved
        );
    }

    #[tokio::test]
    async fn release_journal_matches_legacy_reservations_but_not_completed_or_conflicted_work() {
        let tenant = tenant("legacy-journal-org", "workspace");
        let record = new_idempotency_record(
            "legacy-record".into(),
            input(tenant, "workflow_plan.apply", "key", "fingerprint"),
            "key".into(),
            "workflow_plan.apply".into(),
            "test-owner".into(),
            "fingerprint".into(),
        );
        let mut encoded = serde_json::to_value(record).unwrap();
        encoded.as_object_mut().unwrap().remove("reservation_id");
        let mut legacy: IdempotencyKeyRecord = serde_json::from_value(encoded).unwrap();
        assert!(legacy.reservation_id.is_empty());
        let mut intent = legacy.clone();
        intent.status = IdempotencyKeyStatus::ReleasePending;
        assert!(release_intent_matches(&intent, &legacy));
        for status in [
            IdempotencyKeyStatus::Completed,
            IdempotencyKeyStatus::Conflicted,
        ] {
            legacy.status = status;
            assert!(!release_intent_matches(&intent, &legacy));
        }
        legacy.status = IdempotencyKeyStatus::Reserved;
        legacy.reservation_id = Uuid::new_v4().to_string();
        assert!(!release_intent_matches(&intent, &legacy));
    }

    #[tokio::test]
    async fn caller_cancellation_does_not_abandon_queued_release() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = temp_state();
        state.idempotency_keys_path = directory.path().join("keys.json");
        let tenant = tenant("cancel-org", "cancel-workspace");
        state
            .reserve_idempotency_key(input(
                tenant.clone(),
                "workflow_plan.apply",
                "key",
                "fingerprint",
            ))
            .await
            .unwrap();
        let held = state.idempotency_persistence.lock().await;
        {
            let release = state.release_reserved_idempotency_key(
                &tenant,
                "workflow_plan.apply",
                "key",
                "fingerprint",
            );
            tokio::pin!(release);
            let first = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(std::future::Future::poll(release.as_mut(), cx))
            })
            .await;
            assert!(first.is_pending());
        }
        drop(held);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while state
                .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
                .await
                .is_some()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.load_idempotency_keys().await.unwrap();
        assert!(state
            .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
            .await
            .is_none());
    }

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
    async fn another_owner_cannot_replay_or_poison_a_tenant_reservation() {
        let state = temp_state();
        let tenant = tenant("org-a", "workspace-a");
        let mut alice = input(
            tenant.clone(),
            "operator.workflow_plan_start",
            "shared",
            "same",
        );
        alice.owner = "alice".to_string();
        let first = state.reserve_idempotency_key(alice).await.unwrap();
        assert!(matches!(first, IdempotencyReservation::Reserved(_)));
        let original = first.record().clone();

        for fingerprint in ["same", "different"] {
            let mut bob = input(
                tenant.clone(),
                "operator.workflow_plan_start",
                "shared",
                fingerprint,
            );
            bob.owner = "bob".to_string();
            bob.now_ms = 2_000;
            assert!(matches!(
                state.reserve_idempotency_key(bob).await.unwrap(),
                IdempotencyReservation::Conflict(_)
            ));
        }

        let stored = state
            .get_idempotency_key(&tenant, "operator.workflow_plan_start", "shared")
            .await
            .unwrap();
        assert_eq!(stored, original);
        let mut alice_retry = input(tenant, "operator.workflow_plan_start", "shared", "same");
        alice_retry.owner = "alice".to_string();
        assert!(matches!(
            state.reserve_idempotency_key(alice_retry).await.unwrap(),
            IdempotencyReservation::Duplicate(_)
        ));
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

    #[tokio::test]
    async fn failed_reservation_release_preserves_state_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = temp_state();
        let durable = directory.path().join("keys.json");
        state.idempotency_keys_path = durable.clone();
        let tenant = tenant("org-release", "workspace-release");
        state
            .reserve_idempotency_key(input(
                tenant.clone(),
                "workflow_plan.apply",
                "key",
                "fingerprint",
            ))
            .await
            .unwrap();
        let before = tokio::fs::read(&durable).await.unwrap();
        let blocked = directory.path().join("not-a-directory");
        tokio::fs::write(&blocked, b"blocked").await.unwrap();
        state.idempotency_keys_path = blocked.join("keys.json");
        assert!(state
            .release_reserved_idempotency_key(&tenant, "workflow_plan.apply", "key", "fingerprint")
            .await
            .is_err());
        assert_eq!(
            state
                .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
                .await
                .unwrap()
                .status,
            IdempotencyKeyStatus::ReleasePending
        );
        assert_eq!(tokio::fs::read(&durable).await.unwrap(), before);
        state.idempotency_keys_path = durable;
        assert!(!state
            .retry_pending_idempotency_release(&tenant, "workflow_plan.apply", "key", "other")
            .await
            .unwrap());
        assert!(state
            .retry_pending_idempotency_release(&tenant, "workflow_plan.apply", "key", "fingerprint")
            .await
            .unwrap());
        state.load_idempotency_keys().await.unwrap();
        assert!(state
            .get_idempotency_key(&tenant, "workflow_plan.apply", "key")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn reserved_key_can_be_released_only_by_its_fingerprint() {
        let state = temp_state();
        let tenant_a = tenant("org-a", "workspace-a");
        state
            .reserve_idempotency_key(input(
                tenant_a.clone(),
                "session.prompt_async",
                "prompt-1",
                "fingerprint-a",
            ))
            .await
            .expect("reserve key");

        assert!(!state
            .retry_pending_idempotency_release(
                &tenant_a,
                "session.prompt_async",
                "prompt-1",
                "fingerprint-a",
            )
            .await
            .expect("do not release active reservation"));
        assert!(!state
            .release_reserved_idempotency_key(
                &tenant_a,
                "session.prompt_async",
                "prompt-1",
                "fingerprint-b",
            )
            .await
            .expect("reject unrelated release"));
        assert!(state
            .release_reserved_idempotency_key(
                &tenant_a,
                "session.prompt_async",
                "prompt-1",
                "fingerprint-a",
            )
            .await
            .expect("release reservation"));
        assert!(state
            .get_idempotency_key(&tenant_a, "session.prompt_async", "prompt-1")
            .await
            .is_none());
        let _ = tokio::fs::remove_file(&state.idempotency_keys_path).await;
    }
}
