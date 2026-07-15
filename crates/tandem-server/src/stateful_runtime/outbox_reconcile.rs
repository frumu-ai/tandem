// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Map, Value};

use super::reliability::StatefulOutboxRecord;

pub(crate) fn preserve_pre_send_outbox(
    existing_rows: &[StatefulOutboxRecord],
    outbox: &mut StatefulOutboxRecord,
) {
    let Some(existing) = existing_rows.iter().find(|row| {
        row.outbox_id == outbox.outbox_id && metadata_bool(&row.metadata, "pre_send_dispatch_gate")
    }) else {
        return;
    };

    outbox.created_at_ms = existing.created_at_ms.min(outbox.created_at_ms);
    outbox.attempts = outbox.attempts.max(existing.attempts);
    outbox.claimed_by = existing.claimed_by.clone();
    outbox.claimed_at_ms = existing.claimed_at_ms;
    outbox.claim_expires_at_ms = None;
    outbox.source_id = outbox
        .source_id
        .clone()
        .or_else(|| existing.source_id.clone());
    outbox.payload_digest = outbox
        .payload_digest
        .clone()
        .or_else(|| existing.payload_digest.clone());
    merge_pre_send_metadata(existing, outbox);
}

fn merge_pre_send_metadata(existing: &StatefulOutboxRecord, outbox: &mut StatefulOutboxRecord) {
    let mut metadata = match outbox.metadata.take() {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("receipt_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    let observed_after_execution = metadata
        .get("observed_after_execution")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    metadata.insert("pre_send_dispatch_gate".to_string(), Value::Bool(true));
    metadata.insert("observed_before_execution".to_string(), Value::Bool(true));
    metadata.insert(
        "observed_after_execution".to_string(),
        Value::Bool(observed_after_execution),
    );
    metadata.insert("reconciled_after_execution".to_string(), Value::Bool(true));
    if let Some(pre_send_metadata) = existing.metadata.clone() {
        metadata.insert("pre_send_metadata".to_string(), pre_send_metadata);
    }
    outbox.metadata = Some(Value::Object(metadata));
}

fn metadata_bool(metadata: &Option<Value>, key: &str) -> bool {
    metadata
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stateful_runtime::{StatefulOutboxStatus, StatefulRuntimeScope};

    #[test]
    fn preserve_pre_send_outbox_keeps_claim_and_execution_evidence() {
        let mut receipt = outbox("outbox-1", StatefulOutboxStatus::Sent);
        receipt.metadata = Some(json!({
            "bridged_from": "external_action_record",
            "observed_after_execution": true,
        }));
        let mut pre_send = outbox("outbox-1", StatefulOutboxStatus::Claimed);
        pre_send.created_at_ms = 100;
        pre_send.updated_at_ms = 100;
        pre_send.attempts = 1;
        pre_send.claimed_by = Some("tool-dispatch-ledger".to_string());
        pre_send.claimed_at_ms = Some(100);
        pre_send.claim_expires_at_ms = Some(200);
        pre_send.metadata = Some(json!({
            "pre_send_dispatch_gate": true,
            "observed_before_execution": true,
        }));

        preserve_pre_send_outbox(&[pre_send], &mut receipt);

        assert_eq!(receipt.created_at_ms, 100);
        assert_eq!(receipt.attempts, 1);
        assert_eq!(receipt.claimed_by.as_deref(), Some("tool-dispatch-ledger"));
        assert!(receipt.claim_expires_at_ms.is_none());
        assert_eq!(
            metadata_bool(&receipt.metadata, "observed_before_execution"),
            true
        );
        assert_eq!(
            metadata_bool(&receipt.metadata, "observed_after_execution"),
            true
        );
        assert_eq!(
            metadata_bool(&receipt.metadata, "reconciled_after_execution"),
            true
        );
    }

    fn outbox(outbox_id: &str, status: StatefulOutboxStatus) -> StatefulOutboxRecord {
        StatefulOutboxRecord {
            schema_version: crate::stateful_runtime::STATEFUL_RUNTIME_SCHEMA_VERSION,
            outbox_id: outbox_id.to_string(),
            run_id: Some("run-1".to_string()),
            scope: StatefulRuntimeScope::local_implicit(),
            operation: "mcp.github.create_issue".to_string(),
            status,
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("dispatch-1".to_string()),
            node_id: Some("node-1".to_string()),
            provider: Some("github".to_string()),
            tool: Some("mcp.github.create_issue".to_string()),
            target: Some("frumu-ai/tandem".to_string()),
            idempotency_key: Some("dispatch-1".to_string()),
            payload_digest: Some("sha256:payload".to_string()),
            policy_decision_id: None,
            context_assertion_id: None,
            effect_id: None,
            receipt_id: None,
            compensation_id: None,
            dead_letter_id: None,
            attempts: 0,
            created_at_ms: 150,
            updated_at_ms: 150,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            metadata: None,
        }
    }
}
