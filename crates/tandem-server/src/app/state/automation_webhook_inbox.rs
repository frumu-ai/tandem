use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::TenantContext;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::automation_v2::types::*;

use super::{
    automation_webhook_delivery_correlation, sanitize_automation_webhook_preview, AppState,
    AutomationWebhookFeedbackLoopCandidate, AutomationWebhookQueueResult,
    AutomationWebhookVerificationDecision, VerifiedAutomationWebhookRequest,
};

const AUTOMATION_WEBHOOK_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AutomationWebhookRetentionPruneReport {
    pub pruned_events: usize,
    pub pruned_payloads: usize,
    pub pruned_deliveries: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AutomationWebhookInboxDrainReport {
    pub checked: usize,
    pub processed: usize,
    pub failed: usize,
}

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
    pub verification: Option<AutomationWebhookVerificationDecision>,
    pub feedback_loop_candidate: Option<AutomationWebhookFeedbackLoopCandidate>,
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

fn automation_webhook_payload_path_for_event(
    payloads_dir: &Path,
    event: &AutomationWebhookRawEventRecord,
) -> PathBuf {
    event
        .payload_ref
        .strip_prefix("raw_payloads/")
        .filter(|file_name| !file_name.contains('/'))
        .map(|file_name| payloads_dir.join(file_name))
        .unwrap_or_else(|| payloads_dir.join(format!("{}.body", event.event_id)))
}

fn automation_webhook_event_is_expired(
    event: &AutomationWebhookRawEventRecord,
    now_ms: u64,
) -> bool {
    event
        .retention_policy
        .delete_after_ms
        .is_some_and(|delete_after_ms| delete_after_ms <= now_ms)
}

fn automation_webhook_delivery_is_rejection_only_retention_candidate(
    delivery: &AutomationWebhookDeliveryRecord,
    protected_delivery_ids: &HashSet<String>,
    now_ms: u64,
) -> bool {
    if protected_delivery_ids.contains(&delivery.delivery_id) {
        return false;
    }
    if !matches!(
        delivery.status,
        AutomationWebhookDeliveryStatus::Rejected
            | AutomationWebhookDeliveryStatus::Disabled
            | AutomationWebhookDeliveryStatus::Failed
    ) {
        return false;
    }
    let retention_ms = AutomationWebhookEventRetentionPolicy::default().raw_payload_retention_ms;
    delivery
        .rejected_at_ms
        .unwrap_or(delivery.received_at_ms)
        .checked_add(retention_ms)
        .is_some_and(|delete_after_ms| delete_after_ms <= now_ms)
}

fn retained_automation_webhook_delivery_ids(
    events: &HashMap<String, AutomationWebhookRawEventRecord>,
) -> HashSet<String> {
    events
        .values()
        .filter_map(|event| event.delivery_id.clone())
        .collect()
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

async fn remove_raw_payload_if_present(path: &Path) -> anyhow::Result<bool> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn prune_automation_webhook_events_locked(
    events_path: &PathBuf,
    payloads_dir: &Path,
    events: &mut HashMap<String, AutomationWebhookRawEventRecord>,
    now_ms: u64,
) -> anyhow::Result<(AutomationWebhookRetentionPruneReport, HashSet<String>)> {
    let expired_event_ids = events
        .iter()
        .filter(|(_, event)| automation_webhook_event_is_expired(event, now_ms))
        .map(|(event_id, _)| event_id.clone())
        .collect::<Vec<_>>();
    if expired_event_ids.is_empty() {
        return Ok((
            AutomationWebhookRetentionPruneReport::default(),
            HashSet::new(),
        ));
    }

    let mut report = AutomationWebhookRetentionPruneReport::default();
    let mut delivery_ids = HashSet::new();
    for event_id in expired_event_ids {
        let Some(event) = events.remove(&event_id) else {
            continue;
        };
        report.pruned_events += 1;
        if let Some(delivery_id) = event.delivery_id.as_ref() {
            delivery_ids.insert(delivery_id.clone());
        }
        let payload_path = automation_webhook_payload_path_for_event(payloads_dir, &event);
        if remove_raw_payload_if_present(&payload_path).await? {
            report.pruned_payloads += 1;
        }
    }
    persist_automation_webhook_events(events_path, events.clone()).await?;
    Ok((report, delivery_ids))
}

impl AppState {
    async fn prune_automation_webhook_deliveries_for_events_locked(
        &self,
        delivery_ids: &HashSet<String>,
    ) -> anyhow::Result<usize> {
        if delivery_ids.is_empty() {
            return Ok(0);
        }
        let pruned = {
            let mut deliveries = self.automation_webhook_deliveries.write().await;
            let before = deliveries.len();
            deliveries.retain(|delivery_id, _| !delivery_ids.contains(delivery_id));
            before.saturating_sub(deliveries.len())
        };
        if pruned > 0 {
            self.persist_automation_webhook_deliveries_locked().await?;
        }
        Ok(pruned)
    }

    async fn prune_rejection_only_automation_webhook_deliveries_locked(
        &self,
        protected_delivery_ids: &HashSet<String>,
        now_ms: u64,
    ) -> anyhow::Result<usize> {
        let pruned = {
            let mut deliveries = self.automation_webhook_deliveries.write().await;
            let before = deliveries.len();
            deliveries.retain(|_, delivery| {
                !automation_webhook_delivery_is_rejection_only_retention_candidate(
                    delivery,
                    protected_delivery_ids,
                    now_ms,
                )
            });
            before.saturating_sub(deliveries.len())
        };
        if pruned > 0 {
            self.persist_automation_webhook_deliveries_locked().await?;
        }
        Ok(pruned)
    }

    pub(crate) async fn prune_automation_webhook_retention(
        &self,
        now_ms: u64,
    ) -> anyhow::Result<AutomationWebhookRetentionPruneReport> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let payloads_dir =
            automation_webhook_payloads_dir(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path).await?;
        let (mut report, delivery_ids) = prune_automation_webhook_events_locked(
            &events_path,
            &payloads_dir,
            &mut events,
            now_ms,
        )
        .await?;
        let protected_delivery_ids = retained_automation_webhook_delivery_ids(&events);
        report.pruned_deliveries += self
            .prune_automation_webhook_deliveries_for_events_locked(&delivery_ids)
            .await?;
        report.pruned_deliveries += self
            .prune_rejection_only_automation_webhook_deliveries_locked(
                &protected_delivery_ids,
                now_ms,
            )
            .await?;
        Ok(report)
    }

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
        let (_report, delivery_ids) = prune_automation_webhook_events_locked(
            &events_path,
            &payloads_dir,
            &mut events,
            input.received_at_ms,
        )
        .await?;
        self.prune_automation_webhook_deliveries_for_events_locked(&delivery_ids)
            .await?;
        let protected_delivery_ids = retained_automation_webhook_delivery_ids(&events);
        self.prune_rejection_only_automation_webhook_deliveries_locked(
            &protected_delivery_ids,
            input.received_at_ms,
        )
        .await?;
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
            verification_scheme: input
                .verification
                .as_ref()
                .map(|decision| decision.scheme.clone()),
            verification_provider: input
                .verification
                .as_ref()
                .map(|decision| decision.provider.clone()),
            verification_reason_code: input
                .verification
                .as_ref()
                .map(|decision| decision.reason_code.clone()),
            feedback_loop_candidate: input
                .feedback_loop_candidate
                .as_ref()
                .and_then(|candidate| serde_json::to_value(candidate).ok()),
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

    pub(crate) async fn process_automation_webhook_inbox_once(
        &self,
        limit: usize,
    ) -> AutomationWebhookInboxDrainReport {
        let pending = self.pending_automation_webhook_raw_events(limit).await;
        let mut report = AutomationWebhookInboxDrainReport {
            checked: pending.len(),
            ..AutomationWebhookInboxDrainReport::default()
        };
        for event in pending {
            match self.process_automation_webhook_raw_event(event).await {
                Ok(()) => report.processed += 1,
                Err(error) => {
                    report.failed += 1;
                    tracing::warn!(
                        error = %error,
                        "automation webhook inbox event processing failed"
                    );
                }
            }
        }
        report
    }

    async fn pending_automation_webhook_raw_events(
        &self,
        limit: usize,
    ) -> Vec<AutomationWebhookRawEventRecord> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path)
            .await
            .unwrap_or_default()
            .into_values()
            .filter(|event| event.status == AutomationWebhookDeliveryStatus::Received)
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.received_at_ms);
        events.truncate(limit.clamp(1, 200));
        events
    }

    async fn process_automation_webhook_raw_event(
        &self,
        event: AutomationWebhookRawEventRecord,
    ) -> anyhow::Result<()> {
        let Some(trigger) = self
            .get_automation_webhook_trigger(&event.tenant_context, &event.trigger_id)
            .await
        else {
            anyhow::bail!("automation webhook trigger {} is missing", event.trigger_id);
        };
        let payload = self
            .read_automation_webhook_raw_event_payload(&event.tenant_context, &event.event_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("automation webhook raw payload is missing"))?;
        let payload = match serde_json::from_slice::<Value>(&payload) {
            Ok(payload) => payload,
            Err(_) => {
                let delivery = self
                    .record_automation_webhook_rejection(
                        &trigger,
                        event.provider_event_id.clone(),
                        event.body_digest.clone(),
                        AutomationWebhookDeliveryStatus::Rejected,
                        "invalid_json",
                        event.received_at_ms,
                        json!({ "body_digest": event.body_digest }),
                        Some(automation_webhook_verification_from_raw_event(
                            &event, &trigger,
                        )),
                    )
                    .await?;
                self.update_automation_webhook_raw_event_outcome(
                    &event.tenant_context,
                    &event.event_id,
                    &delivery,
                    crate::now_ms(),
                )
                .await?;
                return Ok(());
            }
        };
        let verified = VerifiedAutomationWebhookRequest {
            trigger: trigger.clone(),
            provider_event_id: event.provider_event_id.clone(),
            body_digest: event.body_digest.clone(),
            received_at_ms: event.received_at_ms,
            verification: automation_webhook_verification_from_raw_event(&event, &trigger),
        };
        let result = Box::pin(
            self.queue_automation_v2_run_from_webhook_delivery_with_feedback_loop(
                verified,
                sanitize_automation_webhook_preview(&payload),
                automation_webhook_feedback_candidate_from_raw_event(&event),
            ),
        )
        .await?;
        let delivery = automation_webhook_delivery_from_queue_result(result);
        self.update_automation_webhook_raw_event_outcome(
            &event.tenant_context,
            &event.event_id,
            &delivery,
            crate::now_ms(),
        )
        .await?;
        Ok(())
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

fn automation_webhook_verification_from_raw_event(
    event: &AutomationWebhookRawEventRecord,
    trigger: &AutomationWebhookTriggerRecord,
) -> AutomationWebhookVerificationDecision {
    AutomationWebhookVerificationDecision::from_persisted(
        event
            .verification_provider
            .clone()
            .unwrap_or_else(|| trigger.provider.clone()),
        event
            .verification_scheme
            .clone()
            .unwrap_or_else(|| trigger.signature_scheme.clone()),
        event
            .verification_reason_code
            .clone()
            .unwrap_or_else(|| "verified".to_string()),
    )
}

fn automation_webhook_feedback_candidate_from_raw_event(
    event: &AutomationWebhookRawEventRecord,
) -> Option<AutomationWebhookFeedbackLoopCandidate> {
    event.feedback_loop_candidate.as_ref().and_then(|value| {
        serde_json::from_value::<AutomationWebhookFeedbackLoopCandidate>(value.clone())
            .ok()
            .filter(|candidate| !candidate.is_empty())
    })
}

fn automation_webhook_delivery_from_queue_result(
    result: AutomationWebhookQueueResult,
) -> AutomationWebhookDeliveryRecord {
    match result {
        AutomationWebhookQueueResult::Accepted { delivery, .. }
        | AutomationWebhookQueueResult::Duplicate { delivery }
        | AutomationWebhookQueueResult::Woken { delivery, .. }
        | AutomationWebhookQueueResult::Suppressed { delivery }
        | AutomationWebhookQueueResult::Rejected { delivery, .. } => delivery,
    }
}
