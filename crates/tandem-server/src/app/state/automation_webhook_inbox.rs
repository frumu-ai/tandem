use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::TenantContext;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::automation_v2::types::*;

use super::{automation_webhook_delivery_correlation, AppState};

const AUTOMATION_WEBHOOK_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationWebhookEventsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    events: HashMap<String, AutomationWebhookRawEventRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct AutomationWebhookRawEventCreateInput {
    pub trigger: AutomationWebhookTriggerRecord,
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub headers_digest: String,
    pub headers_redacted: Value,
    pub content_type: Option<String>,
    pub payload: Vec<u8>,
    pub received_at_ms: u64,
}

fn automation_webhook_events_path(deliveries_path: &Path) -> PathBuf {
    deliveries_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("events.json")
}

fn automation_webhook_payloads_dir(deliveries_path: &Path) -> PathBuf {
    deliveries_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("raw_payloads")
}

fn new_automation_webhook_event_id() -> String {
    format!("automation-webhook-event-{}", Uuid::new_v4())
}

fn parse_automation_webhook_events_file(
    raw: &str,
) -> anyhow::Result<HashMap<String, AutomationWebhookRawEventRecord>> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(raw)
        .context("failed to parse automation webhook events state file")?;
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value)
            .context("failed to parse legacy automation webhook event map");
    }
    let file = serde_json::from_value::<AutomationWebhookEventsFile>(value)
        .context("failed to parse versioned automation webhook events state file")?;
    if file.schema_version > AUTOMATION_WEBHOOK_EVENT_SCHEMA_VERSION {
        anyhow::bail!(
            "automation webhook events schema version {} is newer than supported version {}",
            file.schema_version,
            AUTOMATION_WEBHOOK_EVENT_SCHEMA_VERSION
        );
    }
    Ok(file.events)
}

fn serialize_automation_webhook_events_file(
    events: HashMap<String, AutomationWebhookRawEventRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookEventsFile {
        schema_version: AUTOMATION_WEBHOOK_EVENT_SCHEMA_VERSION,
        events,
    })
    .context("failed to serialize automation webhook events state file")
}

async fn load_automation_webhook_events(
    events_path: &Path,
) -> anyhow::Result<HashMap<String, AutomationWebhookRawEventRecord>> {
    if !events_path.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(events_path).await?;
    parse_automation_webhook_events_file(&raw)
}

async fn persist_automation_webhook_events(
    events_path: &PathBuf,
    events: HashMap<String, AutomationWebhookRawEventRecord>,
) -> anyhow::Result<()> {
    if let Some(parent) = events_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let payload = serialize_automation_webhook_events_file(events)?;
    super::write_state_file_atomically(events_path, payload).await
}

async fn write_raw_payload_atomically(path: &Path, payload: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp).await;
    let mut file = fs::File::create(&tmp).await?;
    file.write_all(payload).await?;
    file.flush().await?;
    drop(file);
    fs::rename(&tmp, path).await?;
    Ok(())
}

impl AppState {
    pub(crate) async fn record_automation_webhook_raw_event(
        &self,
        input: AutomationWebhookRawEventCreateInput,
    ) -> anyhow::Result<AutomationWebhookRawEventRecord> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let payloads_dir =
            automation_webhook_payloads_dir(&self.automation_webhook_deliveries_path);
        let event_id = new_automation_webhook_event_id();
        let payload_file_name = format!("{event_id}.body");
        let payload_path = payloads_dir.join(&payload_file_name);
        write_raw_payload_atomically(&payload_path, &input.payload).await?;

        let mut events = load_automation_webhook_events(&events_path).await?;
        let trigger_id = input.trigger.trigger_id.clone();
        let automation_id = input.trigger.automation_id.clone();
        let enterprise_scope = input.trigger.enterprise_scope();
        let delete_after_ms = input
            .received_at_ms
            .checked_add(AutomationWebhookEventRetentionPolicy::default().raw_payload_retention_ms);
        let record = AutomationWebhookRawEventRecord {
            event_id: event_id.clone(),
            trigger_id: trigger_id.clone(),
            automation_id: automation_id.clone(),
            tenant_context: input.trigger.tenant_context,
            enterprise_scope,
            provider: input.trigger.provider,
            provider_event_kind: input.trigger.provider_event_kind,
            provider_event_id: input.provider_event_id,
            body_digest: input.body_digest,
            headers_digest: input.headers_digest,
            headers_redacted: input.headers_redacted,
            content_type: input.content_type,
            payload_ref: format!("raw_payloads/{payload_file_name}"),
            payload_bytes: input.payload.len() as u64,
            status: AutomationWebhookDeliveryStatus::Received,
            received_at_ms: input.received_at_ms,
            updated_at_ms: input.received_at_ms,
            delivery_id: None,
            queued_run_id: None,
            rejection_reason_code: None,
            idempotency_key: None,
            idempotency_record_id: None,
            dedupe_result: None,
            dedupe_reason_code: None,
            duplicate_of_delivery_id: None,
            duplicate_of_run_id: None,
            woken_run_id: None,
            woken_wait_id: None,
            feedback_loop: None,
            correlation: Some(AutomationWebhookCorrelationRecord {
                outcome: AutomationWebhookCorrelationOutcome::Received,
                event_id: Some(event_id.clone()),
                delivery_id: None,
                trigger_id: Some(trigger_id),
                automation_id: Some(automation_id),
                queued_run_id: None,
                woken_run_id: None,
                woken_wait_id: None,
                duplicate_of_delivery_id: None,
                duplicate_of_run_id: None,
                idempotency_key: None,
                idempotency_record_id: None,
                reason_code: None,
            }),
            retention_policy: AutomationWebhookEventRetentionPolicy {
                delete_after_ms,
                ..AutomationWebhookEventRetentionPolicy::default()
            },
        };
        events.insert(event_id, record.clone());
        persist_automation_webhook_events(&events_path, events).await?;
        Ok(record)
    }

    pub(crate) async fn update_automation_webhook_raw_event_outcome(
        &self,
        tenant_context: &TenantContext,
        event_id: &str,
        delivery: &AutomationWebhookDeliveryRecord,
        updated_at_ms: u64,
    ) -> anyhow::Result<Option<AutomationWebhookRawEventRecord>> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path).await?;
        let Some(record) = events
            .get_mut(event_id)
            .filter(|record| record.tenant_matches(tenant_context))
        else {
            return Ok(None);
        };
        record.status = delivery.status.clone();
        record.delivery_id = Some(delivery.delivery_id.clone());
        record.queued_run_id = delivery.queued_run_id.clone();
        record.rejection_reason_code = delivery.rejection_reason_code.clone();
        record.idempotency_key = delivery.idempotency_key.clone();
        record.idempotency_record_id = delivery.idempotency_record_id.clone();
        record.dedupe_result = delivery.dedupe_result.clone();
        record.dedupe_reason_code = delivery.dedupe_reason_code.clone();
        record.duplicate_of_delivery_id = delivery.duplicate_of_delivery_id.clone();
        record.duplicate_of_run_id = delivery.duplicate_of_run_id.clone();
        record.woken_run_id = delivery.woken_run_id.clone();
        record.woken_wait_id = delivery.woken_wait_id.clone();
        record.enterprise_scope = delivery.enterprise_scope.clone();
        record.feedback_loop = delivery.feedback_loop.clone();
        record.correlation = Some(automation_webhook_delivery_correlation(
            delivery,
            Some(record.event_id.clone()),
        ));
        record.updated_at_ms = updated_at_ms;
        let updated = record.clone();
        persist_automation_webhook_events(&events_path, events).await?;
        Ok(Some(updated))
    }

    pub(crate) async fn list_automation_webhook_raw_events(
        &self,
        tenant_context: &TenantContext,
        trigger_id: Option<&str>,
        automation_id: Option<&str>,
        status: Option<AutomationWebhookDeliveryStatus>,
        limit: usize,
    ) -> Vec<AutomationWebhookRawEventRecord> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path)
            .await
            .unwrap_or_default()
            .into_values()
            .filter(|event| event.tenant_matches(tenant_context))
            .filter(|event| trigger_id.is_none_or(|id| event.trigger_id == id))
            .filter(|event| automation_id.is_none_or(|id| event.automation_id == id))
            .filter(|event| status.as_ref().is_none_or(|status| event.status == *status))
            .collect::<Vec<_>>();
        events.sort_by(|left, right| right.received_at_ms.cmp(&left.received_at_ms));
        events.truncate(limit.clamp(1, 200));
        events
    }

    pub(crate) async fn get_automation_webhook_raw_event(
        &self,
        tenant_context: &TenantContext,
        event_id: &str,
    ) -> anyhow::Result<Option<AutomationWebhookRawEventRecord>> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let events = load_automation_webhook_events(&events_path).await?;
        Ok(events
            .get(event_id)
            .filter(|event| event.tenant_matches(tenant_context))
            .cloned())
    }

    pub(crate) async fn list_automation_webhook_raw_events_for_run(
        &self,
        tenant_context: &TenantContext,
        run_id: &str,
        limit: usize,
    ) -> Vec<AutomationWebhookRawEventRecord> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path)
            .await
            .unwrap_or_default()
            .into_values()
            .filter(|event| event.tenant_matches(tenant_context))
            .filter(|event| {
                event.queued_run_id.as_deref() == Some(run_id)
                    || event.woken_run_id.as_deref() == Some(run_id)
                    || event.duplicate_of_run_id.as_deref() == Some(run_id)
            })
            .collect::<Vec<_>>();
        events.sort_by(|left, right| right.received_at_ms.cmp(&left.received_at_ms));
        events.truncate(limit.clamp(1, 200));
        events
    }

    pub(crate) async fn list_automation_webhook_raw_events_for_trigger(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
    ) -> Vec<AutomationWebhookRawEventRecord> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path)
            .await
            .unwrap_or_default()
            .into_values()
            .filter(|event| event.trigger_id == trigger_id && event.tenant_matches(tenant_context))
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.received_at_ms);
        events
    }

    pub(crate) async fn read_automation_webhook_raw_event_payload(
        &self,
        tenant_context: &TenantContext,
        event_id: &str,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let events = load_automation_webhook_events(&events_path).await?;
        let Some(event) = events
            .get(event_id)
            .filter(|event| event.tenant_matches(tenant_context))
        else {
            return Ok(None);
        };
        let payload_path =
            automation_webhook_payloads_dir(&self.automation_webhook_deliveries_path)
                .join(format!("{}.body", event.event_id));
        let payload = fs::read(payload_path).await?;
        Ok(Some(payload))
    }
}
