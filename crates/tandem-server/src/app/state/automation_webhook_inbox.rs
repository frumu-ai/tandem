use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::{DataClass, TenantContext};
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

fn automation_webhook_trigger_from_raw_event_snapshot(
    mut trigger: AutomationWebhookTriggerRecord,
    event: &AutomationWebhookRawEventRecord,
) -> AutomationWebhookTriggerRecord {
    trigger.automation_id = event.automation_id.clone();
    trigger.tenant_context = event.tenant_context.clone();
    trigger.provider = event.provider.clone();
    trigger.provider_event_kind = event.provider_event_kind.clone();
    match event.enterprise_scope.clone() {
        Some(scope) => {
            trigger.owner_principal = scope.owner_principal;
            trigger.owning_org_unit_id = scope.owning_org_unit_id;
            trigger.resource_scope = scope.resource_scope;
            trigger.default_data_class = scope
                .data_classes
                .into_iter()
                .next()
                .unwrap_or(DataClass::Internal);
            trigger.default_risk_tier = scope.risk_tier;
        }
        None => {
            trigger.owner_principal = None;
            trigger.owning_org_unit_id = None;
            trigger.resource_scope = None;
            trigger.default_data_class = DataClass::Internal;
            trigger.default_risk_tier = None;
        }
    }
    trigger
}

fn automation_webhook_delivery_has_replayable_outcome(
    delivery: &AutomationWebhookDeliveryRecord,
) -> bool {
    match &delivery.status {
        AutomationWebhookDeliveryStatus::Accepted => {
            delivery.queued_run_id.is_some()
                || delivery.woken_run_id.is_some()
                || delivery.woken_wait_id.is_some()
        }
        AutomationWebhookDeliveryStatus::Duplicate
        | AutomationWebhookDeliveryStatus::Suppressed
        | AutomationWebhookDeliveryStatus::Rejected
        | AutomationWebhookDeliveryStatus::Disabled
        | AutomationWebhookDeliveryStatus::Failed => true,
        AutomationWebhookDeliveryStatus::Received => false,
    }
}

fn apply_automation_webhook_delivery_to_raw_event(
    record: &mut AutomationWebhookRawEventRecord,
    delivery: &AutomationWebhookDeliveryRecord,
    event_id: Option<String>,
    updated_at_ms: u64,
) {
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
    record.correlation = Some(automation_webhook_delivery_correlation(delivery, event_id));
    record.updated_at_ms = updated_at_ms;
}

fn automation_webhook_delivery_matches_raw_event(
    delivery: &AutomationWebhookDeliveryRecord,
    trigger: &AutomationWebhookTriggerRecord,
    event: &AutomationWebhookRawEventRecord,
) -> bool {
    if delivery.trigger_id != trigger.trigger_id
        || delivery.automation_id != trigger.automation_id
        || !delivery.tenant_matches(&trigger.tenant_context)
        || !automation_webhook_delivery_has_replayable_outcome(delivery)
        || delivery.received_at_ms != event.received_at_ms
        || delivery.body_digest != event.body_digest
    {
        return false;
    }
    match (&event.provider_event_id, &delivery.provider_event_id) {
        (Some(event_id), Some(delivery_event_id)) => event_id == delivery_event_id,
        (None, None) => true,
        _ => false,
    }
}

fn automation_webhook_delivery_replay_rank(delivery: &AutomationWebhookDeliveryRecord) -> u8 {
    match &delivery.status {
        AutomationWebhookDeliveryStatus::Accepted => 0,
        AutomationWebhookDeliveryStatus::Suppressed => 1,
        AutomationWebhookDeliveryStatus::Rejected
        | AutomationWebhookDeliveryStatus::Disabled
        | AutomationWebhookDeliveryStatus::Failed => 2,
        AutomationWebhookDeliveryStatus::Duplicate => 3,
        AutomationWebhookDeliveryStatus::Received => 4,
    }
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

/// Hard cap on retained raw webhook events across all tenants and triggers,
/// enforced alongside the time-based retention window (TAN-570). Without
/// this, a high-volume tenant (or an attacker with a valid signing secret)
/// can grow `events.json` unboundedly for the full 30-day time window, since
/// retention was previously time-only. Oldest-first eviction keeps the most
/// recent (most operationally relevant) deliveries.
const MAX_RETAINED_AUTOMATION_WEBHOOK_EVENTS: usize = 50_000;

async fn remove_automation_webhook_event(
    payloads_dir: &Path,
    events: &mut HashMap<String, AutomationWebhookRawEventRecord>,
    event_id: &str,
    report: &mut AutomationWebhookRetentionPruneReport,
    delivery_ids: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let Some(event) = events.remove(event_id) else {
        return Ok(());
    };
    report.pruned_events += 1;
    if let Some(delivery_id) = event.delivery_id.as_ref() {
        delivery_ids.insert(delivery_id.clone());
    }
    let payload_path = automation_webhook_payload_path_for_event(payloads_dir, &event);
    if remove_raw_payload_if_present(&payload_path).await? {
        report.pruned_payloads += 1;
    }
    Ok(())
}

async fn prune_automation_webhook_events_locked(
    events_path: &PathBuf,
    payloads_dir: &Path,
    events: &mut HashMap<String, AutomationWebhookRawEventRecord>,
    now_ms: u64,
) -> anyhow::Result<(AutomationWebhookRetentionPruneReport, HashSet<String>)> {
    let mut report = AutomationWebhookRetentionPruneReport::default();
    let mut delivery_ids = HashSet::new();

    let expired_event_ids = events
        .iter()
        .filter(|(_, event)| automation_webhook_event_is_expired(event, now_ms))
        .map(|(event_id, _)| event_id.clone())
        .collect::<Vec<_>>();
    for event_id in expired_event_ids {
        remove_automation_webhook_event(
            payloads_dir,
            events,
            &event_id,
            &mut report,
            &mut delivery_ids,
        )
        .await?;
    }

    if events.len() > MAX_RETAINED_AUTOMATION_WEBHOOK_EVENTS {
        let mut oldest_first = events
            .iter()
            .map(|(event_id, event)| (event_id.clone(), event.received_at_ms))
            .collect::<Vec<_>>();
        oldest_first.sort_by_key(|(_, received_at_ms)| *received_at_ms);
        let overflow = events.len() - MAX_RETAINED_AUTOMATION_WEBHOOK_EVENTS;
        for (event_id, _) in oldest_first.into_iter().take(overflow) {
            remove_automation_webhook_event(
                payloads_dir,
                events,
                &event_id,
                &mut report,
                &mut delivery_ids,
            )
            .await?;
        }
    }

    if report.pruned_events == 0 {
        return Ok((report, delivery_ids));
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

    /// Record an inbound webhook's raw event on the fast-ack path.
    ///
    /// TAN-570: this used to run the full retention prune (a scan over every
    /// retained event/delivery across *all* tenants, plus a full-file
    /// rewrite) inline, under the single process-wide
    /// `automation_webhook_persistence` mutex, on every single webhook
    /// request — so intake latency scaled with total retained volume and one
    /// busy tenant's backlog serialized every other tenant's webhook receipt.
    /// Retention (time *and* count based, see
    /// `prune_automation_webhook_events_locked`) is already enforced
    /// independently by the hourly `run_automation_webhook_retention_reaper`
    /// background task, so this path now only does the O(1) work needed to
    /// durably record the event: write the payload, insert, rewrite the map.
    pub(crate) async fn record_automation_webhook_raw_event(
        &self,
        input: AutomationWebhookRawEventCreateInput,
    ) -> anyhow::Result<AutomationWebhookRawEventRecord> {
        self.record_automation_webhook_raw_event_inner(input, None)
            .await
    }

    pub(crate) async fn record_automation_webhook_raw_event_with_delivery(
        &self,
        input: AutomationWebhookRawEventCreateInput,
        delivery: &AutomationWebhookDeliveryRecord,
    ) -> anyhow::Result<AutomationWebhookRawEventRecord> {
        self.record_automation_webhook_raw_event_inner(input, Some(delivery))
            .await
    }

    async fn record_automation_webhook_raw_event_inner(
        &self,
        input: AutomationWebhookRawEventCreateInput,
        delivery: Option<&AutomationWebhookDeliveryRecord>,
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
        let mut record = AutomationWebhookRawEventRecord {
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
        if let Some(delivery) = delivery {
            apply_automation_webhook_delivery_to_raw_event(
                &mut record,
                delivery,
                Some(event_id.clone()),
                input.received_at_ms,
            );
        }
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
        self.update_automation_webhook_raw_event_outcome_locked(
            tenant_context,
            event_id,
            delivery,
            updated_at_ms,
        )
        .await
    }

    /// Same as `update_automation_webhook_raw_event_outcome`, for callers
    /// that already hold `automation_webhook_persistence` (e.g. TAN-571's
    /// replay-on-registration, which needs the raw-event scan and the
    /// resulting outcome update to happen under the same lock so a
    /// concurrent live delivery can't interleave between them).
    pub(crate) async fn update_automation_webhook_raw_event_outcome_locked(
        &self,
        tenant_context: &TenantContext,
        event_id: &str,
        delivery: &AutomationWebhookDeliveryRecord,
        updated_at_ms: u64,
    ) -> anyhow::Result<Option<AutomationWebhookRawEventRecord>> {
        let events_path = automation_webhook_events_path(&self.automation_webhook_deliveries_path);
        let mut events = load_automation_webhook_events(&events_path).await?;
        let Some(record) = events
            .get_mut(event_id)
            .filter(|record| record.tenant_matches(tenant_context))
        else {
            return Ok(None);
        };
        apply_automation_webhook_delivery_to_raw_event(
            record,
            delivery,
            Some(event_id.to_string()),
            updated_at_ms,
        );
        let updated = record.clone();
        persist_automation_webhook_events(&events_path, events).await?;
        Ok(Some(updated))
    }

    pub(crate) async fn mark_automation_webhook_raw_event_dead_letter(
        &self,
        tenant_context: &TenantContext,
        event_id: &str,
        reason_code: impl Into<String>,
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
        let reason_code = reason_code.into();
        record.status = AutomationWebhookDeliveryStatus::Failed;
        record.rejection_reason_code = Some(reason_code.clone());
        record.correlation = Some(AutomationWebhookCorrelationRecord {
            outcome: AutomationWebhookCorrelationOutcome::DeadLetter,
            event_id: Some(record.event_id.clone()),
            delivery_id: record.delivery_id.clone(),
            trigger_id: Some(record.trigger_id.clone()),
            automation_id: Some(record.automation_id.clone()),
            queued_run_id: record.queued_run_id.clone(),
            woken_run_id: record.woken_run_id.clone(),
            woken_wait_id: record.woken_wait_id.clone(),
            duplicate_of_delivery_id: record.duplicate_of_delivery_id.clone(),
            duplicate_of_run_id: record.duplicate_of_run_id.clone(),
            idempotency_key: record.idempotency_key.clone(),
            idempotency_record_id: record.idempotency_record_id.clone(),
            reason_code: Some(reason_code),
        });
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
            self.mark_automation_webhook_raw_event_dead_letter(
                &event.tenant_context,
                &event.event_id,
                "webhook_trigger_missing",
                crate::now_ms(),
            )
            .await?;
            return Ok(());
        };
        let trigger = automation_webhook_trigger_from_raw_event_snapshot(trigger, &event);
        if let Some(delivery) = self
            .existing_automation_webhook_delivery_for_raw_event(&trigger, &event)
            .await
        {
            self.update_automation_webhook_raw_event_outcome(
                &event.tenant_context,
                &event.event_id,
                &delivery,
                crate::now_ms(),
            )
            .await?;
            return Ok(());
        }
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
            wait_bookkeeping_at_ms: Some(crate::now_ms().max(event.received_at_ms)),
            verification: automation_webhook_verification_from_raw_event(&event, &trigger),
        };
        let queue: BoxFuture<'_, anyhow::Result<AutomationWebhookQueueResult>> = Box::pin(
            self.queue_automation_v2_run_from_webhook_delivery_with_feedback_loop(
                verified,
                sanitize_automation_webhook_preview(&payload),
                automation_webhook_feedback_candidate_from_raw_event(&event),
            ),
        );
        let result = queue.await?;
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

    async fn existing_automation_webhook_delivery_for_raw_event(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        event: &AutomationWebhookRawEventRecord,
    ) -> Option<AutomationWebhookDeliveryRecord> {
        self.automation_webhook_deliveries
            .read()
            .await
            .values()
            .filter(|delivery| {
                automation_webhook_delivery_matches_raw_event(delivery, trigger, event)
            })
            .min_by_key(|delivery| {
                (
                    automation_webhook_delivery_replay_rank(delivery),
                    delivery.delivery_id.clone(),
                )
            })
            .cloned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::TenantContext;

    fn raw_event(event_id: &str, received_at_ms: u64) -> AutomationWebhookRawEventRecord {
        AutomationWebhookRawEventRecord {
            event_id: event_id.to_string(),
            trigger_id: "trigger-a".to_string(),
            automation_id: "automation-a".to_string(),
            tenant_context: TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                None,
                "actor-a",
            ),
            enterprise_scope: None,
            provider: "generic".to_string(),
            provider_event_kind: None,
            provider_event_id: None,
            body_digest: format!("digest-{event_id}"),
            headers_digest: "headers-digest".to_string(),
            headers_redacted: Value::Null,
            content_type: None,
            verification_scheme: None,
            verification_provider: None,
            verification_reason_code: None,
            feedback_loop_candidate: None,
            // No real payload file is created for this event, so retention
            // eviction's `remove_raw_payload_if_present` call is a no-op
            // (NotFound) — this test only exercises the in-memory eviction
            // bookkeeping, not on-disk payload lifecycle (already covered by
            // `webhook_retention_prunes_expired_raw_events_payloads_and_deliveries`).
            payload_ref: format!("raw_payloads/{event_id}.body"),
            payload_bytes: 0,
            status: AutomationWebhookDeliveryStatus::Received,
            received_at_ms,
            updated_at_ms: received_at_ms,
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
            correlation: None,
            retention_policy: AutomationWebhookEventRetentionPolicy {
                // Far in the future — time-based expiry must not be what
                // evicts these events in this test, only the count cap.
                delete_after_ms: Some(received_at_ms + 365 * 24 * 60 * 60 * 1_000),
                ..AutomationWebhookEventRetentionPolicy::default()
            },
        }
    }

    #[tokio::test]
    async fn prune_evicts_oldest_events_beyond_the_retention_count_cap() {
        // TAN-570: retention was previously time-only, so a high-volume
        // tenant (or an attacker with a valid signing secret) could grow
        // `events.json` unboundedly within the 30-day window. Beyond the
        // count cap, the oldest events must be evicted regardless of how far
        // off their time-based expiry still is.
        let root = std::env::temp_dir().join(format!("tandem-webhook-cap-test-{}", Uuid::new_v4()));
        let events_path = root.join("events.json");
        let payloads_dir = root.join("raw_payloads");

        let overflow = 7usize;
        let total = MAX_RETAINED_AUTOMATION_WEBHOOK_EVENTS + overflow;
        let mut events = HashMap::with_capacity(total);
        for index in 0..total {
            let event_id = format!("event-{index:06}");
            // Ascending received_at_ms — event-000000 is the oldest.
            events.insert(event_id.clone(), raw_event(&event_id, index as u64));
        }

        let (report, _delivery_ids) =
            prune_automation_webhook_events_locked(&events_path, &payloads_dir, &mut events, 0)
                .await
                .expect("prune");

        assert_eq!(report.pruned_events, overflow);
        assert_eq!(events.len(), MAX_RETAINED_AUTOMATION_WEBHOOK_EVENTS);
        for index in 0..overflow {
            let evicted_id = format!("event-{index:06}");
            assert!(
                !events.contains_key(&evicted_id),
                "the oldest events must be evicted first"
            );
        }
        for index in overflow..total {
            let retained_id = format!("event-{index:06}");
            assert!(
                events.contains_key(&retained_id),
                "events within the cap must be retained"
            );
        }

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn prune_is_a_no_op_when_nothing_is_expired_or_over_the_cap() {
        let root =
            std::env::temp_dir().join(format!("tandem-webhook-cap-noop-test-{}", Uuid::new_v4()));
        let events_path = root.join("events.json");
        let payloads_dir = root.join("raw_payloads");

        let mut events = HashMap::new();
        events.insert("event-a".to_string(), raw_event("event-a", 0));
        events.insert("event-b".to_string(), raw_event("event-b", 1));

        let (report, delivery_ids) =
            prune_automation_webhook_events_locked(&events_path, &payloads_dir, &mut events, 0)
                .await
                .expect("prune");

        assert_eq!(report, AutomationWebhookRetentionPruneReport::default());
        assert!(delivery_ids.is_empty());
        assert_eq!(events.len(), 2);
        // Nothing pruned means nothing to persist — the events file must not
        // even be created.
        assert!(!events_path.exists());

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
