use serde_json::json;

use crate::automation_v2::types::*;

use super::{
    idempotency_fingerprint, AppState, IdempotencyKeyOutcome, IdempotencyKeyRecord,
    IdempotencyKeyStatus, IdempotencyReservation, IdempotencyReservationInput,
};

const WEBHOOK_PROVIDER_EVENT_IDEMPOTENCY_OPERATION: &str = "webhook.provider_event";
const WEBHOOK_BODY_DIGEST_IDEMPOTENCY_OPERATION: &str = "webhook.body_digest";

#[derive(Debug, Clone)]
pub(crate) struct AutomationWebhookIdempotencyClaim {
    pub(crate) operation: &'static str,
    pub(crate) key: String,
    pub(crate) key_kind: &'static str,
    request_fingerprint: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AutomationWebhookReservedClaim {
    pub(crate) claim: AutomationWebhookIdempotencyClaim,
    pub(crate) record: IdempotencyKeyRecord,
}

#[derive(Debug, Clone)]
pub(crate) enum AutomationWebhookDedupeDecision {
    New {
        records: Vec<AutomationWebhookReservedClaim>,
    },
    Duplicate {
        primary_claim: AutomationWebhookIdempotencyClaim,
        primary_record: IdempotencyKeyRecord,
        reserved_records: Vec<AutomationWebhookReservedClaim>,
    },
    Conflict {
        primary_claim: AutomationWebhookIdempotencyClaim,
        primary_record: IdempotencyKeyRecord,
        reserved_records: Vec<AutomationWebhookReservedClaim>,
    },
}

impl AutomationWebhookDedupeDecision {
    pub(crate) fn accepted_records(&self) -> Vec<AutomationWebhookReservedClaim> {
        match self {
            Self::New { records } => records.clone(),
            Self::Duplicate {
                reserved_records, ..
            }
            | Self::Conflict {
                reserved_records, ..
            } => reserved_records.clone(),
        }
    }
}

impl AppState {
    pub(crate) async fn reserve_automation_webhook_dedupe(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        provider_event_id: Option<&String>,
        body_digest: &str,
        received_at_ms: u64,
    ) -> anyhow::Result<AutomationWebhookDedupeDecision> {
        let claims = automation_webhook_idempotency_claims(trigger, provider_event_id, body_digest);
        let mut reserved_records = Vec::new();
        for claim in claims {
            let reservation = self
                .reserve_idempotency_key(IdempotencyReservationInput {
                    tenant_context: trigger.tenant_context.clone(),
                    operation: claim.operation.to_string(),
                    key: claim.key.clone(),
                    owner: format!("automation_webhook:{}", trigger.trigger_id),
                    request_fingerprint: claim.request_fingerprint.clone(),
                    first_seen_event_id: provider_event_id.cloned(),
                    now_ms: received_at_ms,
                    expires_at_ms: None,
                })
                .await?;
            match reservation {
                IdempotencyReservation::Reserved(record) => {
                    reserved_records.push(AutomationWebhookReservedClaim {
                        claim: claim.clone(),
                        record,
                    });
                }
                IdempotencyReservation::Duplicate(record) => {
                    if is_recoverable_reserved_record(&record) {
                        reserved_records.push(AutomationWebhookReservedClaim {
                            claim: claim.clone(),
                            record,
                        });
                        continue;
                    }
                    return Ok(AutomationWebhookDedupeDecision::Duplicate {
                        primary_claim: claim,
                        primary_record: record,
                        reserved_records,
                    });
                }
                IdempotencyReservation::Conflict(record) => {
                    return Ok(AutomationWebhookDedupeDecision::Conflict {
                        primary_claim: claim,
                        primary_record: record,
                        reserved_records,
                    });
                }
            }
        }
        Ok(AutomationWebhookDedupeDecision::New {
            records: reserved_records,
        })
    }

    pub(crate) async fn complete_automation_webhook_idempotency_records(
        &self,
        records: &[AutomationWebhookReservedClaim],
        delivery: &AutomationWebhookDeliveryRecord,
        outcome_kind: &'static str,
        now_ms: u64,
    ) -> anyhow::Result<()> {
        let outcome = idempotency_outcome_for_webhook_delivery(delivery, outcome_kind, now_ms);
        for reserved in records {
            self.complete_idempotency_key(
                &delivery.tenant_context,
                reserved.claim.operation,
                &reserved.claim.key,
                outcome.clone(),
                now_ms,
            )
            .await?;
        }
        Ok(())
    }
}

fn is_recoverable_reserved_record(record: &IdempotencyKeyRecord) -> bool {
    matches!(record.status, IdempotencyKeyStatus::Reserved) && record.outcome.is_none()
}

pub(crate) fn idempotency_outcome_ref(
    record: &IdempotencyKeyRecord,
) -> (Option<String>, Option<String>) {
    let Some(outcome) = record.outcome.as_ref() else {
        return (None, None);
    };
    (
        outcome
            .primary_ref_kind
            .as_deref()
            .filter(|kind| *kind == "delivery")
            .and(outcome.primary_ref_id.clone()),
        outcome
            .secondary_ref_kind
            .as_deref()
            .filter(|kind| *kind == "run")
            .and(outcome.secondary_ref_id.clone()),
    )
}

fn automation_webhook_idempotency_claims(
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<&String>,
    body_digest: &str,
) -> Vec<AutomationWebhookIdempotencyClaim> {
    let mut claims = Vec::new();
    if let Some(provider_event_id) = provider_event_id
        .map(|event_id| event_id.trim())
        .filter(|event_id| !event_id.is_empty())
    {
        claims.push(AutomationWebhookIdempotencyClaim {
            operation: WEBHOOK_PROVIDER_EVENT_IDEMPOTENCY_OPERATION,
            key: format!(
                "trigger:{}:provider_event:{provider_event_id}",
                trigger.trigger_id
            ),
            key_kind: "provider_event_id",
            request_fingerprint: idempotency_fingerprint(&[
                &trigger.trigger_id,
                "provider_event_id",
                provider_event_id,
                body_digest,
            ]),
        });
    }
    claims.push(AutomationWebhookIdempotencyClaim {
        operation: WEBHOOK_BODY_DIGEST_IDEMPOTENCY_OPERATION,
        key: format!("trigger:{}:body:{body_digest}", trigger.trigger_id),
        key_kind: "body_digest",
        request_fingerprint: idempotency_fingerprint(&[
            &trigger.trigger_id,
            "body_digest",
            body_digest,
        ]),
    });
    claims
}

fn idempotency_outcome_for_webhook_delivery(
    delivery: &AutomationWebhookDeliveryRecord,
    outcome_kind: &'static str,
    completed_at_ms: u64,
) -> IdempotencyKeyOutcome {
    IdempotencyKeyOutcome {
        outcome_kind: outcome_kind.to_string(),
        completed_at_ms,
        primary_ref_kind: Some("delivery".to_string()),
        primary_ref_id: delivery
            .duplicate_of_delivery_id
            .clone()
            .or_else(|| Some(delivery.delivery_id.clone())),
        secondary_ref_kind: delivery
            .duplicate_of_run_id
            .as_ref()
            .or(delivery.queued_run_id.as_ref())
            .or(delivery.woken_run_id.as_ref())
            .map(|_| "run".to_string()),
        secondary_ref_id: delivery
            .duplicate_of_run_id
            .clone()
            .or_else(|| delivery.queued_run_id.clone())
            .or_else(|| delivery.woken_run_id.clone()),
        details: json!({
            "delivery_id": delivery.delivery_id,
            "dedupe_result": delivery.dedupe_result,
            "dedupe_reason_code": delivery.dedupe_reason_code,
            "woken_wait_id": delivery.woken_wait_id,
        }),
    }
}
