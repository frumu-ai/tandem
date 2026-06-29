use serde_json::{json, Value};
use uuid::Uuid;

use crate::automation_v2::types::*;

use super::AutomationWebhookVerificationDecision;

pub(crate) fn new_automation_webhook_delivery_id() -> String {
    format!("automation-webhook-delivery-{}", Uuid::new_v4())
}

pub(crate) fn automation_webhook_delivery_matches_key(
    delivery: &AutomationWebhookDeliveryRecord,
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<&String>,
    body_digest: &str,
) -> bool {
    if delivery.trigger_id != trigger.trigger_id
        || !delivery.tenant_matches(&trigger.tenant_context)
    {
        return false;
    }
    if !matches!(
        delivery.status,
        AutomationWebhookDeliveryStatus::Accepted | AutomationWebhookDeliveryStatus::Duplicate
    ) {
        return false;
    }
    delivery.body_digest == body_digest
        || provider_event_id
            .is_some_and(|event_id| delivery.provider_event_id.as_ref() == Some(event_id))
}

pub(crate) fn automation_webhook_run_metadata(
    trigger: &AutomationWebhookTriggerRecord,
    delivery: &AutomationWebhookDeliveryRecord,
) -> Value {
    json!({
        "trigger_id": trigger.trigger_id,
        "delivery_id": delivery.delivery_id,
        "provider": trigger.provider,
        "provider_event_kind": trigger.provider_event_kind,
        "provider_event_id": delivery.provider_event_id,
        "body_digest": delivery.body_digest,
        "idempotency_key": delivery.idempotency_key,
        "idempotency_record_id": delivery.idempotency_record_id,
        "dedupe_result": delivery.dedupe_result,
        "dedupe_reason_code": delivery.dedupe_reason_code,
        "verification_scheme": delivery.verification_scheme,
        "verification_provider": delivery.verification_provider,
        "verification_reason_code": delivery.verification_reason_code,
        "preview": delivery.sanitized_preview,
        "data_class": trigger.default_data_class,
        "risk_tier": trigger.default_risk_tier,
        "owning_org_unit_id": trigger.owning_org_unit_id,
        "resource_scope": trigger.resource_scope,
        "trust": "untrusted_external_webhook",
    })
}

pub(crate) fn automation_webhook_rejection_delivery(
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
    status: AutomationWebhookDeliveryStatus,
    reason_code: impl Into<String>,
    received_at_ms: u64,
    sanitized_preview: Value,
    verification: Option<AutomationWebhookVerificationDecision>,
) -> AutomationWebhookDeliveryRecord {
    let verification_scheme = verification
        .as_ref()
        .map(|decision| decision.scheme.clone());
    let verification_provider = verification
        .as_ref()
        .map(|decision| decision.provider.clone());
    let verification_reason_code = verification.map(|decision| decision.reason_code);
    AutomationWebhookDeliveryRecord {
        delivery_id: new_automation_webhook_delivery_id(),
        trigger_id: trigger.trigger_id.clone(),
        automation_id: trigger.automation_id.clone(),
        tenant_context: trigger.tenant_context.clone(),
        provider_event_id,
        body_digest,
        status,
        rejection_reason_code: Some(reason_code.into()),
        idempotency_key: None,
        idempotency_record_id: None,
        dedupe_result: None,
        dedupe_reason_code: None,
        duplicate_of_delivery_id: None,
        duplicate_of_run_id: None,
        verification_scheme,
        verification_provider,
        verification_reason_code,
        queued_run_id: None,
        received_at_ms,
        accepted_at_ms: None,
        rejected_at_ms: Some(received_at_ms),
        sanitized_preview,
        audit_event_id: None,
    }
}
