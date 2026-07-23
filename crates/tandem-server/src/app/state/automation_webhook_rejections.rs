// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Bounded telemetry for requests rejected before webhook authentication.
//!
//! This deliberately does not share the normal delivery-map persistence path:
//! an unauthenticated caller must not be able to force full-map rewrites or
//! durable storage of its request body. Records contain fixed-size identifiers
//! and digests only and are appended until the byte or record quota is reached.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(test)]
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::{
    automation_webhook_rejection_delivery, sanitize_automation_webhook_preview, AppState,
    AutomationWebhookVerificationDecision,
};
use crate::{
    AutomationWebhookDeliveryRecord, AutomationWebhookDeliveryStatus,
    AutomationWebhookTriggerRecord,
};

const REJECTION_LEDGER_FILE_NAME: &str = "automation_webhook_rejections.jsonl";
const REJECTION_LEDGER_MAX_BYTES: u64 = 2 * 1024 * 1024;
const REJECTION_LEDGER_MAX_RECORDS: usize = 4096;
const REJECTION_LEDGER_MAX_LINE_BYTES: usize = 1024;
const REJECTION_LEDGER_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const REJECTION_MEMORY_MAX_RECORDS: usize = 1024;

#[derive(Default)]
pub(crate) struct AutomationWebhookRejectionLedgerState {
    initialized: bool,
    bytes: u64,
    records: usize,
    recent_delivery_ids: VecDeque<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationWebhookRejectionTelemetryRecord {
    schema_version: u32,
    delivery_id: String,
    trigger_id: String,
    provider: String,
    tenant_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_event_id: Option<String>,
    body_digest: String,
    status: String,
    reason_code: String,
    received_at_ms: u64,
    expires_at_ms: u64,
}

impl AutomationWebhookRejectionTelemetryRecord {
    fn from_delivery(
        trigger: &AutomationWebhookTriggerRecord,
        delivery: &AutomationWebhookDeliveryRecord,
    ) -> Self {
        let tenant_bytes = serde_json::to_vec(&trigger.tenant_context).unwrap_or_default();
        Self {
            schema_version: 1,
            delivery_id: truncate(&delivery.delivery_id, 96),
            trigger_id: truncate(&trigger.trigger_id, 96),
            provider: truncate(&trigger.provider, 32),
            tenant_digest: hex_encode(&Sha256::digest(tenant_bytes)),
            provider_event_id: delivery
                .provider_event_id
                .as_deref()
                .map(|value| truncate(value, 256)),
            body_digest: truncate(&delivery.body_digest, 128),
            status: truncate(&format!("{:?}", delivery.status).to_ascii_lowercase(), 32),
            reason_code: truncate(
                delivery
                    .rejection_reason_code
                    .as_deref()
                    .unwrap_or("rejected"),
                64,
            ),
            received_at_ms: delivery.received_at_ms,
            expires_at_ms: delivery
                .received_at_ms
                .saturating_add(REJECTION_LEDGER_TTL_MS),
        }
    }
}

fn truncate(value: &str, maximum_chars: usize) -> String {
    value.chars().take(maximum_chars).collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ledger_path(deliveries_path: &Path) -> PathBuf {
    deliveries_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(REJECTION_LEDGER_FILE_NAME)
}

async fn write_ledger_atomically(path: &Path, payload: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("rejection ledger has no parent")?;
    fs::create_dir_all(parent).await?;
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).await?;
    file.write_all(payload).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&temporary, path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

async fn compact_ledger(path: &Path, now_ms: u64) -> anyhow::Result<(u64, usize)> {
    let metadata = match fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(error.into()),
    };
    if metadata.len() > REJECTION_LEDGER_MAX_BYTES {
        anyhow::bail!(
            "webhook rejection ledger exceeds its {} byte safety limit",
            REJECTION_LEDGER_MAX_BYTES
        );
    }

    let raw = fs::read(path).await?;
    let mut retained = VecDeque::<Vec<u8>>::new();
    let mut retained_bytes = 0usize;
    for line in raw.split(|byte| *byte == b'\n') {
        if line.is_empty() || line.len() > REJECTION_LEDGER_MAX_LINE_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<AutomationWebhookRejectionTelemetryRecord>(line)
        else {
            continue;
        };
        if record.expires_at_ms <= now_ms {
            continue;
        }
        let line_bytes = line.len().saturating_add(1);
        retained.push_back(line.to_vec());
        retained_bytes = retained_bytes.saturating_add(line_bytes);
        while retained.len() > REJECTION_LEDGER_MAX_RECORDS
            || retained_bytes as u64 > REJECTION_LEDGER_MAX_BYTES
        {
            if let Some(removed) = retained.pop_front() {
                retained_bytes = retained_bytes.saturating_sub(removed.len().saturating_add(1));
            }
        }
    }

    let mut payload = Vec::with_capacity(retained_bytes);
    for line in &retained {
        payload.extend_from_slice(line);
        payload.push(b'\n');
    }
    write_ledger_atomically(path, &payload).await?;
    Ok((retained_bytes as u64, retained.len()))
}

async fn initialize_ledger(
    path: &Path,
    state: &mut AutomationWebhookRejectionLedgerState,
    now_ms: u64,
) -> anyhow::Result<()> {
    if state.initialized {
        return Ok(());
    }
    let (bytes, records) = compact_ledger(path, now_ms).await?;
    state.bytes = bytes;
    state.records = records;
    state.initialized = true;
    Ok(())
}

async fn open_append_only(path: &Path) -> anyhow::Result<fs::File> {
    let parent = path.parent().context("rejection ledger has no parent")?;
    fs::create_dir_all(parent).await?;
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(file)
}

impl AppState {
    pub(crate) async fn record_automation_webhook_pre_auth_rejection(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        provider_event_id: Option<String>,
        body_digest: String,
        status: AutomationWebhookDeliveryStatus,
        reason_code: impl Into<String>,
        received_at_ms: u64,
        verification: Option<AutomationWebhookVerificationDecision>,
    ) -> AutomationWebhookDeliveryRecord {
        let delivery = automation_webhook_rejection_delivery(
            trigger,
            provider_event_id,
            body_digest.clone(),
            status,
            reason_code,
            received_at_ms,
            sanitize_automation_webhook_preview(&json!({ "body_digest": body_digest })),
            verification,
        );

        if let Err(error) = self
            .append_automation_webhook_rejection_telemetry(trigger, &delivery)
            .await
        {
            tandem_observability::record_webhook_rejection_telemetry("io_error");
            tracing::warn!(
                error = %error,
                trigger_id = %trigger.trigger_id,
                "failed to append bounded webhook rejection telemetry"
            );
        }
        delivery
    }

    async fn append_automation_webhook_rejection_telemetry(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        delivery: &AutomationWebhookDeliveryRecord,
    ) -> anyhow::Result<()> {
        let record = AutomationWebhookRejectionTelemetryRecord::from_delivery(trigger, delivery);
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        if line.len() > REJECTION_LEDGER_MAX_LINE_BYTES {
            anyhow::bail!("webhook rejection telemetry line exceeds safety limit");
        }

        let path = ledger_path(&self.automation_webhook_deliveries_path);
        let mut ledger = self.automation_webhook_rejection_persistence.lock().await;
        initialize_ledger(&path, &mut ledger, delivery.received_at_ms).await?;
        if ledger.records >= REJECTION_LEDGER_MAX_RECORDS
            || ledger.bytes.saturating_add(line.len() as u64) > REJECTION_LEDGER_MAX_BYTES
        {
            let (bytes, records) = compact_ledger(&path, delivery.received_at_ms).await?;
            ledger.bytes = bytes;
            ledger.records = records;
        }
        if ledger.records >= REJECTION_LEDGER_MAX_RECORDS
            || ledger.bytes.saturating_add(line.len() as u64) > REJECTION_LEDGER_MAX_BYTES
        {
            tandem_observability::record_webhook_rejection_telemetry("quota_exhausted");
            tracing::warn!(
                trigger_id = %trigger.trigger_id,
                records = ledger.records,
                bytes = ledger.bytes,
                "webhook rejection telemetry quota exhausted"
            );
            return Ok(());
        }

        let mut file = open_append_only(&path).await?;
        file.write_all(&line).await?;
        file.flush().await?;
        ledger.bytes = ledger.bytes.saturating_add(line.len() as u64);
        ledger.records = ledger.records.saturating_add(1);
        ledger
            .recent_delivery_ids
            .push_back(delivery.delivery_id.clone());
        let evicted = (ledger.recent_delivery_ids.len() > REJECTION_MEMORY_MAX_RECORDS)
            .then(|| ledger.recent_delivery_ids.pop_front())
            .flatten();
        tandem_observability::record_webhook_rejection_telemetry("recorded");
        drop(ledger);

        let mut deliveries = self.automation_webhook_deliveries.write().await;
        deliveries.insert(delivery.delivery_id.clone(), delivery.clone());
        if let Some(evicted) = evicted {
            deliveries.remove(&evicted);
        }
        Ok(())
    }

    pub(super) async fn automation_webhook_pre_auth_rejection_delivery_ids(
        &self,
    ) -> HashSet<String> {
        self.automation_webhook_rejection_persistence
            .lock()
            .await
            .recent_delivery_ids
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_record_never_serializes_body_or_signature_material() {
        let record = AutomationWebhookRejectionTelemetryRecord {
            schema_version: 1,
            delivery_id: "d1".to_string(),
            trigger_id: "t1".to_string(),
            provider: "generic".to_string(),
            tenant_digest: "digest".to_string(),
            provider_event_id: None,
            body_digest: "body-digest".to_string(),
            status: "rejected".to_string(),
            reason_code: "bad_signature".to_string(),
            received_at_ms: 1,
            expires_at_ms: 2,
        };
        let value: Value = serde_json::to_value(record).expect("serialize telemetry");
        let object = value.as_object().expect("telemetry object");
        assert!(!object.contains_key("body"));
        assert!(!object.contains_key("headers"));
        assert!(!object.contains_key("signature"));
        assert!(!object.contains_key("secret"));
    }
}
