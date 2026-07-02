use serde_json::{json, Value};
use tandem_types::ResourceScope;
use uuid::Uuid;

use crate::automation_v2::types::*;
use crate::ExternalActionRecord;

use super::{AutomationWebhookReservedClaim, AutomationWebhookVerificationDecision};

#[derive(Debug, Clone, Default)]
pub(crate) struct AutomationWebhookFeedbackLoopCandidate {
    pub(crate) source_action_id: Option<String>,
    pub(crate) source_run_id: Option<String>,
    pub(crate) source_node_id: Option<String>,
    pub(crate) source_idempotency_key: Option<String>,
    pub(crate) provider_resource_id: Option<String>,
    pub(crate) allow_self_feedback: bool,
}

impl AutomationWebhookFeedbackLoopCandidate {
    pub(crate) fn is_empty(&self) -> bool {
        self.source_action_id.is_none()
            && self.source_run_id.is_none()
            && self.source_node_id.is_none()
            && self.source_idempotency_key.is_none()
            && self.provider_resource_id.is_none()
            && !self.allow_self_feedback
    }
}

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
        AutomationWebhookDeliveryStatus::Accepted
            | AutomationWebhookDeliveryStatus::Duplicate
            | AutomationWebhookDeliveryStatus::Suppressed
    ) {
        return false;
    }
    delivery.body_digest == body_digest
        || provider_event_id
            .is_some_and(|event_id| delivery.provider_event_id.as_ref() == Some(event_id))
}

pub(crate) fn automation_webhook_scope_denial_reason(
    trigger: &AutomationWebhookTriggerRecord,
    automation: &AutomationV2Spec,
) -> Option<&'static str> {
    let trigger_scope = trigger.enterprise_scope();
    let trigger_requires_automation_scope = trigger_scope
        .as_ref()
        .is_some_and(webhook_trigger_requires_automation_scope);
    let automation_scope = match automation.enterprise_scope() {
        Some(scope) => scope,
        None if trigger_requires_automation_scope => {
            return Some("webhook_automation_missing_enterprise_scope")
        }
        None => return None,
    };

    match (
        automation_scope.owning_org_unit_id.as_deref(),
        trigger_scope
            .as_ref()
            .and_then(|scope| scope.owning_org_unit_id.as_deref()),
    ) {
        (Some(_), None) => return Some("webhook_missing_org_scope"),
        (Some(expected), Some(actual)) if expected != actual => {
            return Some("webhook_org_scope_mismatch")
        }
        _ => {}
    }

    match (
        automation_scope.resource_scope.as_ref(),
        trigger_scope
            .as_ref()
            .and_then(|scope| scope.resource_scope.as_ref()),
    ) {
        (Some(_), None) => Some("webhook_missing_resource_scope"),
        (Some(automation_scope), Some(trigger_scope))
            if !automation_scope_contains_trigger_scope(automation_scope, trigger_scope) =>
        {
            Some("webhook_resource_scope_mismatch")
        }
        _ => None,
    }
}

fn webhook_trigger_requires_automation_scope(scope: &AutomationEnterpriseScope) -> bool {
    scope.owner_principal.is_some()
        || scope.owning_org_unit_id.is_some()
        || scope.resource_scope.is_some()
        || scope.risk_tier.is_some()
        || scope.policy_version_id.is_some()
        || !scope.delegation_grant_ids.is_empty()
}

fn automation_scope_contains_trigger_scope(
    automation_scope: &ResourceScope,
    trigger_scope: &ResourceScope,
) -> bool {
    automation_scope.contains(&trigger_scope.root)
        && trigger_scope
            .allowed_resources
            .iter()
            .all(|resource| automation_scope.contains(resource))
}

pub(crate) fn automation_webhook_accepted_delivery(
    delivery_id: Option<String>,
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
    received_at_ms: u64,
    sanitized_preview: Value,
    verification: &AutomationWebhookVerificationDecision,
    primary_idempotency: Option<&AutomationWebhookReservedClaim>,
    woken_run_id: Option<String>,
    woken_wait_id: Option<String>,
    feedback_loop: Option<AutomationWebhookFeedbackLoopDecision>,
) -> AutomationWebhookDeliveryRecord {
    AutomationWebhookDeliveryRecord {
        delivery_id: delivery_id.unwrap_or_else(new_automation_webhook_delivery_id),
        trigger_id: trigger.trigger_id.clone(),
        automation_id: trigger.automation_id.clone(),
        tenant_context: trigger.tenant_context.clone(),
        enterprise_scope: trigger.enterprise_scope(),
        provider_event_id,
        body_digest,
        status: AutomationWebhookDeliveryStatus::Accepted,
        rejection_reason_code: None,
        idempotency_key: primary_idempotency.map(|record| record.claim.key.clone()),
        idempotency_record_id: primary_idempotency.map(|record| record.record.record_id.clone()),
        dedupe_result: Some(AutomationWebhookDedupeResult::Accepted),
        dedupe_reason_code: primary_idempotency
            .map(|record| format!("accepted_{}", record.claim.key_kind)),
        duplicate_of_delivery_id: None,
        duplicate_of_run_id: None,
        verification_scheme: Some(verification.scheme.clone()),
        verification_provider: Some(verification.provider.clone()),
        verification_reason_code: Some(verification.reason_code.clone()),
        queued_run_id: None,
        woken_run_id,
        woken_wait_id,
        feedback_loop,
        correlation: None,
        received_at_ms,
        accepted_at_ms: Some(received_at_ms),
        rejected_at_ms: None,
        sanitized_preview,
        audit_event_id: None,
    }
}

fn automation_webhook_correlation_outcome(
    delivery: &AutomationWebhookDeliveryRecord,
) -> AutomationWebhookCorrelationOutcome {
    match delivery.status {
        AutomationWebhookDeliveryStatus::Accepted if delivery.woken_run_id.is_some() => {
            AutomationWebhookCorrelationOutcome::WakeRun
        }
        AutomationWebhookDeliveryStatus::Accepted => AutomationWebhookCorrelationOutcome::NewRun,
        AutomationWebhookDeliveryStatus::Duplicate => {
            AutomationWebhookCorrelationOutcome::Duplicate
        }
        AutomationWebhookDeliveryStatus::Suppressed => {
            AutomationWebhookCorrelationOutcome::Suppressed
        }
        AutomationWebhookDeliveryStatus::Rejected | AutomationWebhookDeliveryStatus::Disabled => {
            AutomationWebhookCorrelationOutcome::Rejected
        }
        AutomationWebhookDeliveryStatus::Failed => AutomationWebhookCorrelationOutcome::DeadLetter,
        AutomationWebhookDeliveryStatus::Received => AutomationWebhookCorrelationOutcome::Received,
    }
}

pub(crate) fn automation_webhook_delivery_correlation(
    delivery: &AutomationWebhookDeliveryRecord,
    event_id: Option<String>,
) -> AutomationWebhookCorrelationRecord {
    AutomationWebhookCorrelationRecord {
        outcome: automation_webhook_correlation_outcome(delivery),
        event_id,
        delivery_id: Some(delivery.delivery_id.clone()),
        trigger_id: Some(delivery.trigger_id.clone()),
        automation_id: Some(delivery.automation_id.clone()),
        queued_run_id: delivery.queued_run_id.clone(),
        woken_run_id: delivery.woken_run_id.clone(),
        woken_wait_id: delivery.woken_wait_id.clone(),
        duplicate_of_delivery_id: delivery.duplicate_of_delivery_id.clone(),
        duplicate_of_run_id: delivery.duplicate_of_run_id.clone(),
        idempotency_key: delivery.idempotency_key.clone(),
        idempotency_record_id: delivery.idempotency_record_id.clone(),
        reason_code: delivery
            .rejection_reason_code
            .clone()
            .or_else(|| delivery.dedupe_reason_code.clone()),
    }
}

pub(crate) fn automation_webhook_feedback_decision_from_action(
    action: &ExternalActionRecord,
    candidate: &AutomationWebhookFeedbackLoopCandidate,
) -> AutomationWebhookFeedbackLoopDecision {
    let metadata_run_id = action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("automationRunID"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let metadata_node_id = action
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("nodeID"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    AutomationWebhookFeedbackLoopDecision {
        outcome: if candidate.allow_self_feedback {
            AutomationWebhookFeedbackLoopOutcome::Allowed
        } else {
            AutomationWebhookFeedbackLoopOutcome::Suppressed
        },
        reason_code: if candidate.allow_self_feedback {
            "self_feedback_explicitly_allowed".to_string()
        } else {
            "self_induced_feedback_loop".to_string()
        },
        source_action_id: Some(action.action_id.clone()),
        source_run_id: candidate.source_run_id.clone().or(metadata_run_id),
        source_node_id: candidate.source_node_id.clone().or(metadata_node_id),
        source_idempotency_key: action
            .idempotency_key
            .clone()
            .or_else(|| candidate.source_idempotency_key.clone()),
        source_provider: action.provider.clone(),
        source_target: action
            .target
            .clone()
            .or_else(|| candidate.provider_resource_id.clone()),
        allow_self_feedback: candidate.allow_self_feedback,
    }
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
        "woken_run_id": delivery.woken_run_id,
        "woken_wait_id": delivery.woken_wait_id,
        "feedback_loop": delivery.feedback_loop,
        "correlation": delivery.correlation,
        "preview": delivery.sanitized_preview,
        "enterprise_scope": delivery.enterprise_scope,
        "data_class": trigger.default_data_class,
        "risk_tier": trigger.default_risk_tier,
        "owner_principal": trigger.owner_principal,
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
        enterprise_scope: trigger.enterprise_scope(),
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
        woken_run_id: None,
        woken_wait_id: None,
        feedback_loop: None,
        correlation: None,
        received_at_ms,
        accepted_at_ms: None,
        rejected_at_ms: Some(received_at_ms),
        sanitized_preview,
        audit_event_id: None,
    }
}
