use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tandem_types::{PrincipalKind, PrincipalRef, TenantContext};
use tokio::io::AsyncWriteExt;

use crate::routines::types::ExternalActionRecord;

use super::types::{StatefulRuntimeScope, STATEFUL_RUNTIME_SCHEMA_VERSION};

static STATEFUL_RELIABILITY_STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const DEFAULT_RELIABILITY_LIMIT: usize = 250;

#[derive(Debug, Clone)]
pub struct StatefulReliabilityStoragePaths {
    pub reliability_path: PathBuf,
}

impl StatefulReliabilityStoragePaths {
    pub fn from_runtime_events_path(runtime_events_path: &Path) -> Self {
        let runtime_root = runtime_events_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            reliability_path: runtime_root.join("stateful_reliability.json"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatefulReliabilityStoreFile {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub outbox: Vec<StatefulOutboxRecord>,
    #[serde(default)]
    pub tool_effects: Vec<StatefulToolEffectRecord>,
    #[serde(default)]
    pub dead_letters: Vec<StatefulDeadLetterRecord>,
    #[serde(default)]
    pub compensations: Vec<StatefulCompensationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulOutboxStatus {
    Pending,
    Claimed,
    Sent,
    Failed,
    DeadLettered,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulOutboxRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub outbox_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub scope: StatefulRuntimeScope,
    pub operation: String,
    pub status: StatefulOutboxStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_assertion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_letter_id: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulOutboxRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulToolEffectStatus {
    Pending,
    Succeeded,
    Failed,
    Unknown,
}

impl StatefulToolEffectStatus {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulToolEffectRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub effect_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbox_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub scope: StatefulRuntimeScope,
    pub status: StatefulToolEffectStatus,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_resource: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_assertion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_payload_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_payload_redacted: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_pointer: Option<String>,
    #[serde(default)]
    pub redaction_tier: String,
    pub audit_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation_id: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulToolEffectRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulDeadLetterStatus {
    Open,
    RetryRequested,
    Ignored,
    LinkedToCompensation,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulRecoveryOption {
    Retry,
    Ignore,
    Compensate,
    Abandon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulDeadLetterRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub dead_letter_id: String,
    pub source_type: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub scope: StatefulRuntimeScope,
    pub reason: String,
    pub status: StatefulDeadLetterStatus,
    #[serde(default)]
    pub recovery_options: Vec<StatefulRecoveryOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation_id: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_disposition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_actor: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulDeadLetterRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatefulCompensationStatus {
    Proposed,
    AwaitingApproval,
    Approved,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatefulCompensationRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub compensation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub scope: StatefulRuntimeScope,
    pub status: StatefulCompensationStatus,
    pub compensation_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_effect_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbox_id: Option<String>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_fix_instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_effect_id: Option<String>,
    #[serde(default)]
    pub attempts: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl StatefulCompensationRecord {
    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        self.scope.visible_to_tenant(tenant)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StatefulReliabilityQuery<'a> {
    pub run_id: Option<&'a str>,
    pub status: Option<&'a str>,
    pub source_type: Option<&'a str>,
    pub limit: Option<usize>,
}

pub fn stateful_reliability_path_from_runtime_events_path(runtime_events_path: &Path) -> PathBuf {
    StatefulReliabilityStoragePaths::from_runtime_events_path(runtime_events_path).reliability_path
}

pub fn load_stateful_reliability(path: &Path) -> StatefulReliabilityStoreFile {
    let Ok(content) = std::fs::read_to_string(path) else {
        return StatefulReliabilityStoreFile {
            schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
            ..Default::default()
        };
    };
    let mut store = match serde_json::from_str::<StatefulReliabilityStoreFile>(&content) {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "skipping invalid stateful reliability store"
            );
            StatefulReliabilityStoreFile::default()
        }
    };
    sort_reliability_store(&mut store);
    store
}

pub fn list_stateful_outbox(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulReliabilityQuery<'_>,
) -> Vec<StatefulOutboxRecord> {
    let mut rows = load_stateful_reliability(path)
        .outbox
        .into_iter()
        .filter(|row| row.visible_to_tenant(tenant))
        .filter(|row| option_filter_matches(query.run_id, row.run_id.as_deref()))
        .filter(|row| status_matches(query.status, &row.status))
        .collect::<Vec<_>>();
    apply_limit(&mut rows, query.limit);
    rows
}

pub fn list_stateful_tool_effects(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulReliabilityQuery<'_>,
) -> Vec<StatefulToolEffectRecord> {
    let mut rows = load_stateful_reliability(path)
        .tool_effects
        .into_iter()
        .filter(|row| row.visible_to_tenant(tenant))
        .filter(|row| option_filter_matches(query.run_id, row.run_id.as_deref()))
        .filter(|row| status_matches(query.status, &row.status))
        .filter(|row| option_filter_matches(query.source_type, row.source_kind.as_deref()))
        .collect::<Vec<_>>();
    apply_limit(&mut rows, query.limit);
    rows
}

pub fn list_stateful_dead_letters(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulReliabilityQuery<'_>,
) -> Vec<StatefulDeadLetterRecord> {
    let mut rows = load_stateful_reliability(path)
        .dead_letters
        .into_iter()
        .filter(|row| row.visible_to_tenant(tenant))
        .filter(|row| option_filter_matches(query.run_id, row.run_id.as_deref()))
        .filter(|row| status_matches(query.status, &row.status))
        .filter(|row| option_filter_matches(query.source_type, Some(row.source_type.as_str())))
        .collect::<Vec<_>>();
    apply_limit(&mut rows, query.limit);
    rows
}

pub fn list_stateful_compensations(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulReliabilityQuery<'_>,
) -> Vec<StatefulCompensationRecord> {
    let mut rows = load_stateful_reliability(path)
        .compensations
        .into_iter()
        .filter(|row| row.visible_to_tenant(tenant))
        .filter(|row| option_filter_matches(query.run_id, row.run_id.as_deref()))
        .filter(|row| status_matches(query.status, &row.status))
        .collect::<Vec<_>>();
    apply_limit(&mut rows, query.limit);
    rows
}

pub async fn upsert_stateful_outbox(
    path: &Path,
    record: StatefulOutboxRecord,
) -> anyhow::Result<StatefulOutboxRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    upsert_by(&mut store.outbox, record.clone(), |row| &row.outbox_id);
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(record)
}

pub async fn upsert_stateful_tool_effect(
    path: &Path,
    record: StatefulToolEffectRecord,
) -> anyhow::Result<StatefulToolEffectRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    upsert_by(&mut store.tool_effects, record.clone(), |row| {
        &row.effect_id
    });
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(record)
}

pub async fn upsert_stateful_dead_letter(
    path: &Path,
    record: StatefulDeadLetterRecord,
) -> anyhow::Result<StatefulDeadLetterRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    upsert_by(&mut store.dead_letters, record.clone(), |row| {
        &row.dead_letter_id
    });
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(record)
}

pub async fn upsert_stateful_compensation(
    path: &Path,
    record: StatefulCompensationRecord,
) -> anyhow::Result<StatefulCompensationRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    upsert_by(&mut store.compensations, record.clone(), |row| {
        &row.compensation_id
    });
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(record)
}

pub async fn record_external_action_reliability_bridge(
    path: &Path,
    scope: StatefulRuntimeScope,
    action: &ExternalActionRecord,
) -> anyhow::Result<StatefulToolEffectRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    let mut outbox = outbox_from_external_action(scope.clone(), action);
    let mut effect = tool_effect_from_external_action(scope.clone(), action, &outbox);
    outbox.effect_id = Some(effect.effect_id.clone());
    outbox.receipt_id = Some(effect.effect_id.clone());

    if effect.status.is_failure() {
        if let Some(compensation) =
            compensation_from_action_metadata(&scope, action, &effect, &outbox)
        {
            effect.compensation_id = Some(compensation.compensation_id.clone());
            outbox.compensation_id = Some(compensation.compensation_id.clone());
            upsert_by(&mut store.compensations, compensation, |row| {
                &row.compensation_id
            });
        }
        let dead_letter = dead_letter_from_tool_effect(&effect, action);
        outbox.dead_letter_id = Some(dead_letter.dead_letter_id.clone());
        upsert_by(&mut store.dead_letters, dead_letter, |row| {
            &row.dead_letter_id
        });
    } else {
        clear_stale_failure_rows_for_effect(&mut store, &effect, &outbox);
    }

    upsert_by(&mut store.outbox, outbox, |row| &row.outbox_id);
    upsert_by(&mut store.tool_effects, effect.clone(), |row| {
        &row.effect_id
    });
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(effect)
}

fn clear_stale_failure_rows_for_effect(
    store: &mut StatefulReliabilityStoreFile,
    effect: &StatefulToolEffectRecord,
    outbox: &StatefulOutboxRecord,
) {
    store.dead_letters.retain(|row| {
        !(row.scope == effect.scope
            && row.run_id == effect.run_id
            && row.source_type == "tool_effect"
            && row.source_id == effect.effect_id)
    });
    store.compensations.retain(|row| {
        !(row.scope == effect.scope
            && row.run_id == effect.run_id
            && (row.target_effect_id.as_deref() == Some(effect.effect_id.as_str())
                || row.outbox_id.as_deref() == Some(outbox.outbox_id.as_str())))
    });
}

pub async fn mark_dead_letter_disposition(
    path: &Path,
    tenant: &TenantContext,
    dead_letter_id: &str,
    status: StatefulDeadLetterStatus,
    disposition: &str,
    reason: Option<String>,
    actor: PrincipalRef,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulDeadLetterRecord>> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    let Some(row) = store
        .dead_letters
        .iter_mut()
        .find(|row| row.dead_letter_id == dead_letter_id && row.visible_to_tenant(tenant))
    else {
        return Ok(None);
    };
    row.status = status;
    row.operator_disposition = Some(disposition.to_string());
    row.disposition_reason = reason;
    row.disposition_actor = Some(actor);
    row.disposition_at_ms = Some(now_ms);
    row.updated_at_ms = now_ms;
    let updated = row.clone();
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(Some(updated))
}

pub async fn mark_compensation_status(
    path: &Path,
    tenant: &TenantContext,
    compensation_id: &str,
    status: StatefulCompensationStatus,
    now_ms: u64,
) -> anyhow::Result<Option<StatefulCompensationRecord>> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = load_stateful_reliability(path);
    let Some(row) = store
        .compensations
        .iter_mut()
        .find(|row| row.compensation_id == compensation_id && row.visible_to_tenant(tenant))
    else {
        return Ok(None);
    };
    row.status = status;
    row.updated_at_ms = now_ms;
    let updated = row.clone();
    write_stateful_reliability_unlocked(path, &store).await?;
    Ok(Some(updated))
}

async fn write_stateful_reliability_unlocked(
    path: &Path,
    store: &StatefulReliabilityStoreFile,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create stateful reliability directory {}",
                parent.display()
            )
        })?;
    }
    let mut store = store.clone();
    store.schema_version = STATEFUL_RUNTIME_SCHEMA_VERSION;
    sort_reliability_store(&mut store);
    let tmp_path = tmp_path_for(path);
    let content = serde_json::to_vec_pretty(&store)?;
    let mut file = tokio::fs::File::create(&tmp_path).await.with_context(|| {
        format!(
            "failed to create stateful reliability {}",
            tmp_path.display()
        )
    })?;
    file.write_all(&content).await.with_context(|| {
        format!(
            "failed to write stateful reliability {}",
            tmp_path.display()
        )
    })?;
    file.flush().await.with_context(|| {
        format!(
            "failed to flush stateful reliability {}",
            tmp_path.display()
        )
    })?;
    drop(file);
    tokio::fs::rename(&tmp_path, path)
        .await
        .with_context(|| format!("failed to publish stateful reliability {}", path.display()))?;
    Ok(())
}

fn outbox_from_external_action(
    scope: StatefulRuntimeScope,
    action: &ExternalActionRecord,
) -> StatefulOutboxRecord {
    let effect_id = Some(effect_id_for_action(action));
    StatefulOutboxRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox_id: outbox_id_for_action(action),
        run_id: external_action_run_id(action),
        scope,
        operation: action.operation.clone(),
        status: outbox_status_from_action(action),
        source_kind: action.source_kind.clone(),
        source_id: action.source_id.clone(),
        node_id: external_action_node_id(action),
        provider: action.provider.clone(),
        tool: external_action_tool(action),
        target: action.target.clone(),
        idempotency_key: action.idempotency_key.clone(),
        payload_digest: action
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("input").or_else(|| metadata.get("args")))
            .and_then(digest_value),
        policy_decision_id: external_action_string_metadata(action, "policyDecisionID")
            .or_else(|| external_action_string_metadata(action, "policy_decision_id")),
        context_assertion_id: external_action_string_metadata(action, "contextAssertionID")
            .or_else(|| external_action_string_metadata(action, "context_assertion_id")),
        effect_id,
        receipt_id: None,
        compensation_id: None,
        dead_letter_id: None,
        attempts: external_action_u64_metadata(action, "attempt")
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(1),
        created_at_ms: action.created_at_ms,
        updated_at_ms: action.updated_at_ms,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: Some(json!({
            "bridged_from": "external_action_record",
            "observed_after_execution": true,
            "external_action_id": action.action_id,
        })),
    }
}

fn tool_effect_from_external_action(
    scope: StatefulRuntimeScope,
    action: &ExternalActionRecord,
    outbox: &StatefulOutboxRecord,
) -> StatefulToolEffectRecord {
    let receipt_payload_digest = action.receipt.as_ref().and_then(digest_value);
    let receipt_payload_redacted = action.receipt.as_ref().map(redact_value);
    let input_digest = action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("input").or_else(|| metadata.get("args")))
        .and_then(digest_value);
    let output_digest = action
        .receipt
        .as_ref()
        .map(|receipt| receipt.get("result").unwrap_or(receipt))
        .and_then(digest_value);
    let effect_id = effect_id_for_action(action);
    let status = tool_effect_status_from_action(action);
    let audit_hash = crate::sha256_hex(&[
        &effect_id,
        &action.action_id,
        &action.operation,
        action.status.as_str(),
        receipt_payload_digest.as_deref().unwrap_or(""),
    ]);

    StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id,
        outbox_id: Some(outbox.outbox_id.clone()),
        action_id: Some(action.action_id.clone()),
        run_id: outbox.run_id.clone(),
        scope,
        status,
        operation: action.operation.clone(),
        source_kind: action.source_kind.clone(),
        source_id: action.source_id.clone(),
        node_id: external_action_node_id(action),
        provider: action.provider.clone(),
        tool: external_action_tool(action),
        target: action.target.clone(),
        external_resource: Some(json!({
            "provider": action.provider,
            "target": action.target,
            "capability_id": action.capability_id,
        })),
        policy_decision_id: outbox.policy_decision_id.clone(),
        context_assertion_id: outbox.context_assertion_id.clone(),
        input_digest,
        output_digest,
        receipt_payload_digest,
        receipt_payload_redacted,
        receipt_pointer: Some(format!("external-action://{}", action.action_id)),
        redaction_tier: "safe_ui".to_string(),
        audit_hash,
        error: action.error.clone(),
        compensation_id: None,
        created_at_ms: action.created_at_ms,
        updated_at_ms: action.updated_at_ms,
        metadata: Some(json!({
            "approval_state": action.approval_state,
            "idempotency_key": action.idempotency_key,
            "context_run_id": action.context_run_id,
            "routine_run_id": action.routine_run_id,
        })),
    }
}

fn dead_letter_from_tool_effect(
    effect: &StatefulToolEffectRecord,
    action: &ExternalActionRecord,
) -> StatefulDeadLetterRecord {
    StatefulDeadLetterRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        dead_letter_id: format!("dead-letter-{}", effect.effect_id),
        source_type: "tool_effect".to_string(),
        source_id: effect.effect_id.clone(),
        run_id: effect.run_id.clone(),
        scope: effect.scope.clone(),
        reason: action
            .error
            .clone()
            .unwrap_or_else(|| format!("external action `{}` failed", action.operation)),
        status: StatefulDeadLetterStatus::Open,
        recovery_options: vec![
            StatefulRecoveryOption::Retry,
            StatefulRecoveryOption::Ignore,
            StatefulRecoveryOption::Compensate,
        ],
        payload_pointer: Some(format!("external-action://{}", action.action_id)),
        compensation_id: effect.compensation_id.clone(),
        attempts: external_action_u64_metadata(action, "attempt")
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(1),
        created_at_ms: action.updated_at_ms,
        updated_at_ms: action.updated_at_ms,
        operator_disposition: None,
        disposition_reason: None,
        disposition_actor: None,
        disposition_at_ms: None,
        metadata: Some(json!({
            "operation": action.operation,
            "status": action.status,
            "idempotency_key": action.idempotency_key,
        })),
    }
}

fn compensation_from_action_metadata(
    scope: &StatefulRuntimeScope,
    action: &ExternalActionRecord,
    effect: &StatefulToolEffectRecord,
    outbox: &StatefulOutboxRecord,
) -> Option<StatefulCompensationRecord> {
    let metadata = action.metadata.as_ref()?;
    let compensation = metadata
        .get("compensation")
        .or_else(|| metadata.get("compensation_policy"))?;
    let compensation_type = compensation
        .get("type")
        .or_else(|| compensation.get("kind"))
        .and_then(value_string)
        .unwrap_or_else(|| "operator_review".to_string());
    Some(StatefulCompensationRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        compensation_id: compensation
            .get("compensation_id")
            .and_then(value_string)
            .unwrap_or_else(|| format!("compensation-{}", effect.effect_id)),
        run_id: effect.run_id.clone(),
        scope: scope.clone(),
        status: StatefulCompensationStatus::Proposed,
        compensation_type,
        target_effect_id: Some(effect.effect_id.clone()),
        outbox_id: Some(outbox.outbox_id.clone()),
        approval_required: compensation
            .get("approval_required")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        policy_decision_id: outbox.policy_decision_id.clone(),
        rollback_instruction: compensation
            .get("rollback_instruction")
            .and_then(value_string),
        forward_fix_instruction: compensation
            .get("forward_fix_instruction")
            .and_then(value_string),
        receipt_effect_id: None,
        attempts: 0,
        created_at_ms: action.updated_at_ms,
        updated_at_ms: action.updated_at_ms,
        metadata: Some(redact_value(compensation)),
    })
}

fn outbox_status_from_action(action: &ExternalActionRecord) -> StatefulOutboxStatus {
    match tool_effect_status_from_action(action) {
        StatefulToolEffectStatus::Succeeded => StatefulOutboxStatus::Sent,
        StatefulToolEffectStatus::Failed => StatefulOutboxStatus::Failed,
        StatefulToolEffectStatus::Pending => StatefulOutboxStatus::Pending,
        StatefulToolEffectStatus::Unknown => StatefulOutboxStatus::Sent,
    }
}

fn tool_effect_status_from_action(action: &ExternalActionRecord) -> StatefulToolEffectStatus {
    if action
        .error
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return StatefulToolEffectStatus::Failed;
    }
    match normalize_key(&action.status).as_str() {
        "posted" | "sent" | "succeeded" | "success" | "completed" | "delivered" | "matched" => {
            StatefulToolEffectStatus::Succeeded
        }
        "pending" | "queued" | "claimed" => StatefulToolEffectStatus::Pending,
        "failed" | "error" | "rejected" | "denied" | "cancelled" => {
            StatefulToolEffectStatus::Failed
        }
        _ => StatefulToolEffectStatus::Unknown,
    }
}

fn outbox_id_for_action(action: &ExternalActionRecord) -> String {
    idempotency_suffix(action)
        .map(|suffix| format!("outbox-{suffix}"))
        .unwrap_or_else(|| format!("outbox-{}", action.action_id))
}

fn effect_id_for_action(action: &ExternalActionRecord) -> String {
    idempotency_suffix(action)
        .map(|suffix| format!("effect-{suffix}"))
        .unwrap_or_else(|| format!("effect-{}", action.action_id))
}

fn idempotency_suffix(action: &ExternalActionRecord) -> Option<String> {
    action
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|key| short_hash(&crate::sha256_hex(&[key])))
}

fn external_action_run_id(action: &ExternalActionRecord) -> Option<String> {
    external_action_string_metadata(action, "automationRunID")
        .or_else(|| external_action_string_metadata(action, "automation_run_id"))
        .or_else(|| external_action_string_metadata(action, "workflowRunID"))
        .or_else(|| external_action_string_metadata(action, "workflow_run_id"))
        .or_else(|| trimmed_owned(action.routine_run_id.as_deref()))
        .or_else(|| {
            action
                .context_run_id
                .as_deref()
                .and_then(runtime_run_id_from_context_run_id)
        })
}

fn external_action_node_id(action: &ExternalActionRecord) -> Option<String> {
    external_action_string_metadata(action, "nodeID")
        .or_else(|| external_action_string_metadata(action, "node_id"))
}

fn external_action_tool(action: &ExternalActionRecord) -> Option<String> {
    external_action_string_metadata(action, "tool").or_else(|| action.capability_id.clone())
}

fn external_action_string_metadata(action: &ExternalActionRecord, key: &str) -> Option<String> {
    action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(value_string)
}

fn external_action_u64_metadata(action: &ExternalActionRecord, key: &str) -> Option<u64> {
    action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_u64)
}

fn runtime_run_id_from_context_run_id(context_run_id: &str) -> Option<String> {
    let context_run_id = context_run_id.trim();
    if context_run_id.is_empty() {
        return None;
    }
    context_run_id
        .strip_prefix("automation-v2-")
        .or_else(|| context_run_id.strip_prefix("workflow-"))
        .or_else(|| context_run_id.strip_prefix("routine-"))
        .map(str::to_string)
        .or_else(|| Some(context_run_id.to_string()))
}

fn trimmed_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn digest_value(value: &Value) -> Option<String> {
    Some(format!(
        "sha256:{}",
        crate::sha256_hex(&[&value.to_string()])
    ))
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut redacted = Map::new();
            for (key, value) in object {
                if is_sensitive_key(key) {
                    redacted.insert(key.clone(), Value::String("[redacted]".to_string()));
                } else {
                    redacted.insert(key.clone(), redact_value(value));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(values) => Value::Array(values.iter().map(redact_value).collect()),
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = normalize_key(key);
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.contains("api_key")
        || key.contains("apikey")
        || key.contains("private_key")
}

fn value_string(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn option_filter_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    actual
        .map(|value| normalize_key(value) == expected)
        .unwrap_or(false)
}

fn status_matches<T: Serialize>(expected: Option<&str>, actual: &T) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    serialized_key(actual) == expected
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    let value = normalize_key(value.unwrap_or_default());
    if value.is_empty() || value == "all" {
        None
    } else {
        Some(value)
    }
}

fn normalize_key(value: &str) -> String {
    value.trim().replace('-', "_").to_ascii_lowercase()
}

fn serialized_key<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .map(|value| normalize_key(&value))
        .unwrap_or_default()
}

fn apply_limit<T>(rows: &mut Vec<T>, limit: Option<usize>) {
    rows.truncate(
        limit
            .unwrap_or(DEFAULT_RELIABILITY_LIMIT)
            .clamp(1, DEFAULT_RELIABILITY_LIMIT),
    );
}

fn upsert_by<T, F>(rows: &mut Vec<T>, record: T, id: F)
where
    F: Fn(&T) -> &String,
{
    match rows.iter_mut().find(|row| id(row) == id(&record)) {
        Some(existing) => *existing = record,
        None => rows.push(record),
    }
}

fn sort_reliability_store(store: &mut StatefulReliabilityStoreFile) {
    store
        .outbox
        .sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    store
        .tool_effects
        .sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    store
        .dead_letters
        .sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    store
        .compensations
        .sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    tmp.set_extension(extension);
    tmp
}

fn short_hash(hash: &str) -> String {
    hash.strip_prefix("sha256:")
        .unwrap_or(hash)
        .chars()
        .take(16)
        .collect()
}

fn schema_version() -> u32 {
    STATEFUL_RUNTIME_SCHEMA_VERSION
}

pub fn operator_principal(actor_id: Option<&str>) -> PrincipalRef {
    PrincipalRef::new(
        PrincipalKind::HumanUser,
        actor_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("operator"),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use uuid::Uuid;

    use super::*;

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn action(action_id: &str, status: &str, error: Option<&str>) -> ExternalActionRecord {
        ExternalActionRecord {
            action_id: action_id.to_string(),
            operation: "mock_external_action.send".to_string(),
            status: status.to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("run-a:node-a:1:0".to_string()),
            routine_run_id: None,
            context_run_id: Some("automation-v2-run-a".to_string()),
            capability_id: Some("mock_external_action.send".to_string()),
            provider: Some("mock".to_string()),
            target: Some("customer-outbox".to_string()),
            approval_state: Some("executed".to_string()),
            idempotency_key: Some(format!("idempotency-{action_id}")),
            receipt: Some(json!({
                "result": {"status": "ok"},
                "authorization": "Bearer abc",
                "nested": {"api_key": "secret-value"}
            })),
            error: error.map(str::to_string),
            metadata: Some(json!({
                "automationRunID": "run-a",
                "nodeID": "node-a",
                "attempt": 2,
                "tool": "SendMessage",
                "input": {"message": "hello"}
            })),
            created_at_ms: 1_000,
            updated_at_ms: 2_000,
        }
    }

    #[tokio::test]
    async fn external_action_bridge_records_outbox_and_redacted_effect() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
        let effect = record_external_action_reliability_bridge(
            &path,
            scope,
            &action("action-a", "posted", None),
        )
        .await
        .expect("bridge");

        assert_eq!(effect.status, StatefulToolEffectStatus::Succeeded);
        let store = load_stateful_reliability(&path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.tool_effects.len(), 1);
        assert_eq!(store.dead_letters.len(), 0);
        let receipt = store.tool_effects[0]
            .receipt_payload_redacted
            .as_ref()
            .expect("receipt");
        assert_eq!(receipt["authorization"], "[redacted]");
        assert_eq!(receipt["nested"]["api_key"], "[redacted]");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn external_action_bridge_preserves_context_only_run_id() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
        let mut record = action("action-context-only", "posted", None);
        record.context_run_id = Some("automation-v2-run-context-only".to_string());
        record.metadata = Some(json!({
            "nodeID": "node-a",
            "attempt": 1,
            "tool": "SendMessage",
            "input": {"message": "hello"}
        }));

        let effect = record_external_action_reliability_bridge(&path, scope, &record)
            .await
            .expect("bridge");

        assert_eq!(effect.run_id.as_deref(), Some("run-context-only"));
        let effects = list_stateful_tool_effects(
            &path,
            &tenant_a,
            StatefulReliabilityQuery {
                run_id: Some("run-context-only"),
                ..Default::default()
            },
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].action_id.as_deref(), Some("action-context-only"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn external_action_bridge_dedupes_effects_by_idempotency_key() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
        let mut first = action("action-replay-first", "posted", None);
        first.idempotency_key = Some("idem-run-a-node-a-send".to_string());
        let mut replay = action("action-replay-second", "posted", None);
        replay.idempotency_key = first.idempotency_key.clone();
        replay.updated_at_ms = 3_000;
        replay.receipt = Some(json!({
            "result": {"status": "already_sent"},
            "secret": "must be redacted"
        }));

        let first_effect = record_external_action_reliability_bridge(&path, scope.clone(), &first)
            .await
            .expect("first bridge");
        let replay_effect = record_external_action_reliability_bridge(&path, scope, &replay)
            .await
            .expect("replay bridge");

        assert_eq!(first_effect.effect_id, replay_effect.effect_id);
        let store = load_stateful_reliability(&path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.tool_effects.len(), 1);
        assert_eq!(store.dead_letters.len(), 0);
        assert_eq!(
            store.outbox[0].effect_id.as_deref(),
            Some(replay_effect.effect_id.as_str())
        );
        assert_eq!(
            store.tool_effects[0].action_id.as_deref(),
            Some("action-replay-second")
        );
        assert_eq!(
            store.tool_effects[0]
                .receipt_payload_redacted
                .as_ref()
                .and_then(|receipt| receipt.get("secret"))
                .and_then(Value::as_str),
            Some("[redacted]")
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn external_action_success_replay_clears_stale_failure_recovery_rows() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
        let mut failed = action("action-replay-failed", "failed", Some("provider timeout"));
        failed.idempotency_key = Some("idem-run-a-node-a-send".to_string());
        failed.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 1,
            "compensation": {
                "type": "operator_review",
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));
        let mut succeeded = action("action-replay-succeeded", "posted", None);
        succeeded.idempotency_key = failed.idempotency_key.clone();
        succeeded.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "compensation": {
                "type": "operator_review",
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));
        succeeded.updated_at_ms = 3_000;
        succeeded.receipt = Some(json!({
            "result": {"status": "posted"}
        }));

        record_external_action_reliability_bridge(&path, scope.clone(), &failed)
            .await
            .expect("failed bridge");
        let failed_store = load_stateful_reliability(&path);
        assert_eq!(failed_store.tool_effects.len(), 1);
        assert_eq!(failed_store.dead_letters.len(), 1);
        assert_eq!(failed_store.compensations.len(), 1);

        let replay_effect = record_external_action_reliability_bridge(&path, scope, &succeeded)
            .await
            .expect("success bridge");

        assert_eq!(replay_effect.status, StatefulToolEffectStatus::Succeeded);
        assert!(replay_effect.compensation_id.is_none());
        let store = load_stateful_reliability(&path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.tool_effects.len(), 1);
        assert_eq!(store.dead_letters.len(), 0);
        assert_eq!(store.compensations.len(), 0);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Sent);
        assert!(store.outbox[0].dead_letter_id.is_none());
        assert!(store.outbox[0].compensation_id.is_none());
        assert_eq!(
            store.tool_effects[0].action_id.as_deref(),
            Some("action-replay-succeeded")
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_external_action_bridge_creates_tenant_filtered_dead_letter() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
        record_external_action_reliability_bridge(
            &path,
            scope,
            &action("action-b", "failed", Some("provider timeout")),
        )
        .await
        .expect("bridge");

        let visible = list_stateful_dead_letters(
            &path,
            &tenant_a,
            StatefulReliabilityQuery {
                run_id: Some("run-a"),
                ..Default::default()
            },
        );
        let hidden = list_stateful_dead_letters(
            &path,
            &tenant_b,
            StatefulReliabilityQuery {
                run_id: Some("run-a"),
                ..Default::default()
            },
        );
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].reason, "provider timeout");
        assert!(hidden.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_external_action_bridge_links_default_compensation_to_dead_letter() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let scope = StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a"));
        let mut record = action("action-compensation", "failed", Some("provider timeout"));
        record.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "tool": "SendMessage",
            "input": {"message": "hello"},
            "compensation_policy": {
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));

        let effect = record_external_action_reliability_bridge(&path, scope, &record)
            .await
            .expect("bridge");

        let store = load_stateful_reliability(&path);
        assert_eq!(store.compensations.len(), 1);
        assert_eq!(store.dead_letters.len(), 1);
        let compensation_id = format!("compensation-{}", effect.effect_id);
        assert_eq!(
            effect.compensation_id.as_deref(),
            Some(compensation_id.as_str())
        );
        assert_eq!(store.compensations[0].compensation_id, compensation_id);
        assert_eq!(store.compensations[0].compensation_type, "operator_review");
        assert_eq!(
            store.dead_letters[0].compensation_id.as_deref(),
            Some(store.compensations[0].compensation_id.as_str())
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn operator_recovery_updates_are_tenant_scoped() {
        let path = std::env::temp_dir().join(format!(
            "tandem-stateful-reliability-{}.json",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
        let mut record = action(
            "action-tenant-compensation",
            "failed",
            Some("provider timeout"),
        );
        record.metadata = Some(json!({
            "automationRunID": "run-a",
            "nodeID": "node-a",
            "attempt": 2,
            "tool": "SendMessage",
            "input": {"message": "hello"},
            "compensation": {
                "type": "operator_review",
                "approval_required": true,
                "rollback_instruction": "remove the posted message"
            }
        }));
        record_external_action_reliability_bridge(&path, scope, &record)
            .await
            .expect("bridge");
        let store = load_stateful_reliability(&path);
        let compensation_id = store.compensations[0].compensation_id.clone();
        let dead_letter_id = store.dead_letters[0].dead_letter_id.clone();

        let wrong_compensation = mark_compensation_status(
            &path,
            &tenant_b,
            &compensation_id,
            StatefulCompensationStatus::Completed,
            3_000,
        )
        .await
        .expect("wrong tenant compensation update");
        assert!(wrong_compensation.is_none());

        let updated_compensation = mark_compensation_status(
            &path,
            &tenant_a,
            &compensation_id,
            StatefulCompensationStatus::Completed,
            4_000,
        )
        .await
        .expect("tenant compensation update")
        .expect("updated compensation");
        assert_eq!(
            updated_compensation.status,
            StatefulCompensationStatus::Completed
        );

        let wrong_dead_letter = mark_dead_letter_disposition(
            &path,
            &tenant_b,
            &dead_letter_id,
            StatefulDeadLetterStatus::LinkedToCompensation,
            "linked_to_compensation",
            Some("wrong tenant".to_string()),
            operator_principal(Some("operator-b")),
            5_000,
        )
        .await
        .expect("wrong tenant dead letter update");
        assert!(wrong_dead_letter.is_none());

        let updated_dead_letter = mark_dead_letter_disposition(
            &path,
            &tenant_a,
            &dead_letter_id,
            StatefulDeadLetterStatus::LinkedToCompensation,
            "linked_to_compensation",
            Some("compensation completed".to_string()),
            operator_principal(Some("operator-a")),
            6_000,
        )
        .await
        .expect("tenant dead letter update")
        .expect("updated dead letter");
        assert_eq!(
            updated_dead_letter.status,
            StatefulDeadLetterStatus::LinkedToCompensation
        );
        assert_eq!(
            updated_dead_letter.operator_disposition.as_deref(),
            Some("linked_to_compensation")
        );
        let _ = std::fs::remove_file(path);
    }
}
