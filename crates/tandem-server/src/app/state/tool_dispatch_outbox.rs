use std::path::Path;

use serde_json::{json, Map, Value};
use tandem_tools::{
    ToolDispatchLedgerEvent, ToolDispatchPreSendEvent, ToolDispatchPreSendReceipt,
    ToolDispatchReceiptPhase, ToolDispatchStatus,
};
use tandem_types::ToolRiskTier;

use crate::stateful_runtime::reliability::{
    try_load_stateful_reliability, write_stateful_reliability_unlocked,
    STATEFUL_RELIABILITY_STORE_LOCK,
};
use crate::stateful_runtime::{
    load_stateful_reliability, stateful_reliability_path_from_runtime_events_path,
    upsert_stateful_outbox, upsert_stateful_tool_effect, StatefulOutboxRecord,
    StatefulOutboxStatus, StatefulReliabilityStoreFile, StatefulRuntimeScope,
    StatefulToolEffectRecord, StatefulToolEffectStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use crate::util::time::now_ms;

const TOOL_DISPATCH_CLAIM_TTL_MS: u64 = 5 * 60 * 1000;

pub(crate) async fn prepare_pre_send_outbox(
    runtime_events_path: &Path,
    event: ToolDispatchPreSendEvent,
) -> anyhow::Result<Option<ToolDispatchPreSendReceipt>> {
    if !should_gate_external_dispatch(&event) {
        return Ok(None);
    }

    let reliability_path = stateful_reliability_path_from_runtime_events_path(runtime_events_path);
    let now = now_ms();
    let idempotency_key = event.dispatch_id.clone();
    let outbox_id = outbox_id_for_idempotency_key(&idempotency_key);
    reserve_pre_send_outbox(
        &reliability_path,
        &event,
        outbox_id.clone(),
        idempotency_key.clone(),
        now,
    )
    .await?;

    Ok(Some(ToolDispatchPreSendReceipt {
        outbox_id,
        idempotency_key,
    }))
}

async fn reserve_pre_send_outbox(
    reliability_path: &Path,
    event: &ToolDispatchPreSendEvent,
    outbox_id: String,
    idempotency_key: String,
    now: u64,
) -> anyhow::Result<StatefulOutboxRecord> {
    let _guard = STATEFUL_RELIABILITY_STORE_LOCK.lock().await;
    let mut store = try_load_stateful_reliability(reliability_path)?;
    let mut row = match store
        .outbox
        .iter()
        .position(|row| row.outbox_id == outbox_id)
    {
        Some(index) => {
            reusable_pending_outbox(store.outbox[index].clone(), event, &idempotency_key, now)?
        }
        None => outbox_record(event, outbox_id, idempotency_key, now),
    };
    row.status = StatefulOutboxStatus::Claimed;
    row.attempts = row.attempts.saturating_add(1).max(1);
    row.updated_at_ms = now;
    row.claimed_by = Some("tool-dispatch-ledger".to_string());
    row.claimed_at_ms = Some(now);
    row.claim_expires_at_ms = Some(now.saturating_add(TOOL_DISPATCH_CLAIM_TTL_MS));
    upsert_outbox_unlocked(&mut store, row.clone());
    write_stateful_reliability_unlocked(reliability_path, &store).await?;
    Ok(row)
}

fn upsert_outbox_unlocked(store: &mut StatefulReliabilityStoreFile, row: StatefulOutboxRecord) {
    match store
        .outbox
        .iter_mut()
        .find(|existing| existing.outbox_id == row.outbox_id)
    {
        Some(existing) => *existing = row,
        None => store.outbox.push(row),
    }
}

fn reusable_pending_outbox(
    row: StatefulOutboxRecord,
    event: &ToolDispatchPreSendEvent,
    idempotency_key: &str,
    now: u64,
) -> anyhow::Result<StatefulOutboxRecord> {
    if !row.visible_to_tenant(&event.tenant_context) {
        anyhow::bail!(
            "pre-send outbox `{}` belongs to a different tenant scope",
            row.outbox_id
        );
    }
    if row
        .idempotency_key
        .as_deref()
        .is_some_and(|stored| stored != idempotency_key)
    {
        anyhow::bail!(
            "pre-send outbox `{}` has a conflicting idempotency key",
            row.outbox_id
        );
    }
    if matches!(
        row.status,
        StatefulOutboxStatus::Pending | StatefulOutboxStatus::Cancelled
    ) {
        return Ok(row);
    }
    if row.status == StatefulOutboxStatus::Claimed && claim_is_expired(&row, now) {
        return Ok(row);
    }
    anyhow::bail!(
        "pre-send outbox `{}` is already `{}`",
        row.outbox_id,
        outbox_status_label(&row.status)
    );
}

fn claim_is_expired(row: &StatefulOutboxRecord, now: u64) -> bool {
    row.claim_expires_at_ms
        .is_some_and(|expires_at| expires_at <= now)
}

pub(crate) async fn complete_pre_send_outbox(
    runtime_events_path: &Path,
    event: &ToolDispatchLedgerEvent,
) -> anyhow::Result<()> {
    if !event.receipt_phase.is_terminal() {
        return Ok(());
    }
    let Some(dispatch_id) = event.dispatch_id.as_deref() else {
        return Ok(());
    };
    let reliability_path = stateful_reliability_path_from_runtime_events_path(runtime_events_path);
    let outbox_id = outbox_id_for_idempotency_key(dispatch_id);
    let mut row = match load_stateful_reliability(&reliability_path)
        .outbox
        .into_iter()
        .find(|row| row.outbox_id == outbox_id && row.visible_to_tenant(&event.tenant_context))
    {
        Some(row) => row,
        None => return Ok(()),
    };
    if is_outbox_gate_denial(event) {
        return Ok(());
    }
    if event.status == ToolDispatchStatus::Blocked && !is_active_pre_send_claim(&row) {
        return Ok(());
    }

    row.status = match event.status {
        ToolDispatchStatus::Succeeded => StatefulOutboxStatus::Sent,
        ToolDispatchStatus::Failed => StatefulOutboxStatus::Failed,
        ToolDispatchStatus::Blocked => StatefulOutboxStatus::Cancelled,
    };
    row.updated_at_ms = now_ms();
    row.claim_expires_at_ms = None;
    merge_completion_metadata(&mut row.metadata, event);
    upsert_stateful_outbox(&reliability_path, row).await?;
    Ok(())
}

pub(crate) async fn persist_dispatch_receipt(
    runtime_events_path: &Path,
    event: &ToolDispatchLedgerEvent,
) -> anyhow::Result<()> {
    let now = now_ms();
    let operation = event
        .canonical_tool
        .clone()
        .unwrap_or_else(|| event.tool.clone());
    let dispatch_id = event.dispatch_id.clone().unwrap_or_else(|| {
        crate::sha256_hex(&[
            &event.tool,
            &event.source.kind,
            event.payload_digest.as_deref().unwrap_or_default(),
        ])
    });
    let status = match event.receipt_phase {
        ToolDispatchReceiptPhase::ExecutionStarted => StatefulToolEffectStatus::Pending,
        ToolDispatchReceiptPhase::PolicyDecision
            if event.policy_outcome == tandem_tools::ToolDispatchPolicyOutcome::Denied =>
        {
            StatefulToolEffectStatus::Failed
        }
        ToolDispatchReceiptPhase::PolicyDecision => StatefulToolEffectStatus::Succeeded,
        ToolDispatchReceiptPhase::ExecutionCompleted => StatefulToolEffectStatus::Succeeded,
        ToolDispatchReceiptPhase::ExecutionFailed => StatefulToolEffectStatus::Failed,
    };
    let terminal_status = match event.status {
        ToolDispatchStatus::Succeeded => StatefulToolEffectStatus::Succeeded,
        ToolDispatchStatus::Failed | ToolDispatchStatus::Blocked => {
            StatefulToolEffectStatus::Failed
        }
    };
    let status = if event.receipt_phase.is_terminal() {
        terminal_status
    } else {
        status
    };
    let receipt_payload = json!({
        "dispatch_id": dispatch_id,
        "tool": event.tool,
        "canonical_tool": event.canonical_tool,
        "source": event.source,
        "policy_outcome": event.policy_outcome,
        "receipt_phase": event.receipt_phase,
        "policy_decision_id": event.policy_decision_id,
        "status": event.status,
        "error": event.error,
    });
    let receipt_payload_string = receipt_payload.to_string();
    let audit_hash = crate::sha256_hex(&[
        &dispatch_id,
        &operation,
        &receipt_payload_string,
        event.payload_digest.as_deref().unwrap_or_default(),
    ]);
    let record = StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id: format!(
            "tool-dispatch-{dispatch_id}-{}",
            event.receipt_phase.as_str()
        ),
        outbox_id: None,
        action_id: None,
        run_id: event.source.run_id.clone(),
        scope: StatefulRuntimeScope::from_tenant_context(event.tenant_context.clone()),
        status,
        operation,
        source_kind: Some(event.source.kind.clone()),
        source_id: Some(dispatch_id.clone()),
        node_id: event.source.node_id.clone(),
        provider: provider_from_tool(event.canonical_tool.as_deref().unwrap_or(&event.tool)),
        tool: Some(event.tool.clone()),
        target: None,
        external_resource: None,
        policy_decision_id: event.policy_decision_id.clone(),
        context_assertion_id: None,
        input_digest: event.payload_digest.clone(),
        output_digest: None,
        receipt_payload_digest: Some(crate::sha256_hex(&[&receipt_payload_string])),
        receipt_payload_redacted: Some(receipt_payload),
        receipt_pointer: Some(format!(
            "tool-dispatch://{dispatch_id}/{}",
            event.receipt_phase.as_str()
        )),
        redaction_tier: "safe_ui".to_string(),
        audit_hash,
        error: event.error.clone(),
        compensation_id: None,
        created_at_ms: now,
        updated_at_ms: now,
        metadata: Some(json!({
            "receipt_source": "central_tool_dispatcher",
            "receipt_phase": event.receipt_phase,
            "scope_allowlist": event.scope_allowlist,
        })),
    };
    let reliability_path = stateful_reliability_path_from_runtime_events_path(runtime_events_path);
    upsert_stateful_tool_effect(&reliability_path, record)
        .await
        .map(|_| ())
}

fn is_outbox_gate_denial(event: &ToolDispatchLedgerEvent) -> bool {
    event.status == ToolDispatchStatus::Blocked
        && event
            .error
            .as_deref()
            .is_some_and(|error| error.contains("ToolDenied { reason: OutboxGate }"))
}

fn is_active_pre_send_claim(row: &StatefulOutboxRecord) -> bool {
    row.status == StatefulOutboxStatus::Claimed
        && row.claimed_by.as_deref() == Some("tool-dispatch-ledger")
}

fn should_gate_external_dispatch(event: &ToolDispatchPreSendEvent) -> bool {
    if event.risk_tier.as_deref().is_some_and(gated_risk_tier) {
        return true;
    }
    event.external_side_effect && event.risk_tier.is_none()
}

fn gated_risk_tier(risk_tier: &str) -> bool {
    matches!(
        risk_tier,
        "consequential_write"
            | "external_send"
            | "destructive_delete"
            | "money_movement_contract"
            | "financial_record_access"
            | "credential_admin"
    )
}

fn outbox_status_label(status: &StatefulOutboxStatus) -> &'static str {
    match status {
        StatefulOutboxStatus::Pending => "pending",
        StatefulOutboxStatus::Claimed => "claimed",
        StatefulOutboxStatus::Sent => "sent",
        StatefulOutboxStatus::Failed => "failed",
        StatefulOutboxStatus::DeadLettered => "dead_lettered",
        StatefulOutboxStatus::Cancelled => "cancelled",
    }
}

fn outbox_record(
    event: &ToolDispatchPreSendEvent,
    outbox_id: String,
    idempotency_key: String,
    now: u64,
) -> StatefulOutboxRecord {
    StatefulOutboxRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox_id,
        run_id: event.source.run_id.clone(),
        scope: scope_from_event(event),
        operation: event
            .canonical_tool
            .clone()
            .unwrap_or_else(|| event.tool.clone()),
        status: StatefulOutboxStatus::Pending,
        source_kind: Some(event.source.kind.clone()),
        source_id: Some(event.dispatch_id.clone()),
        node_id: event.source.node_id.clone(),
        provider: provider_from_tool(event.canonical_tool.as_deref().unwrap_or(&event.tool)),
        tool: Some(event.tool.clone()),
        target: target_from_args(&event.args),
        idempotency_key: Some(idempotency_key),
        payload_digest: event.payload_digest.clone(),
        policy_decision_id: event.policy_decision_id.clone(),
        context_assertion_id: None,
        effect_id: None,
        receipt_id: None,
        compensation_id: None,
        dead_letter_id: None,
        attempts: 0,
        created_at_ms: now,
        updated_at_ms: now,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: Some(json!({
            "pre_send_dispatch_gate": true,
            "observed_before_execution": true,
            "dispatch_id": event.dispatch_id,
            "dispatch_source": event.source,
            "scope_allowlist": event.scope_allowlist,
            "policy_outcome": event.policy_outcome,
            "risk_tier": event.risk_tier,
        })),
    }
}

fn scope_from_event(event: &ToolDispatchPreSendEvent) -> StatefulRuntimeScope {
    let mut scope = StatefulRuntimeScope::from_tenant_context(event.tenant_context.clone());
    scope.risk_tier = event.risk_tier.as_ref().and_then(|value| {
        serde_json::from_value::<ToolRiskTier>(Value::String(value.clone())).ok()
    });
    scope
}

fn merge_completion_metadata(metadata: &mut Option<Value>, event: &ToolDispatchLedgerEvent) {
    let mut object = match metadata.take() {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = Map::new();
            object.insert("previous_metadata".to_string(), value);
            object
        }
        None => Map::new(),
    };
    object.insert("dispatch_completed".to_string(), Value::Bool(true));
    object.insert(
        "dispatch_status".to_string(),
        serde_json::to_value(&event.status).unwrap_or(Value::Null),
    );
    object.insert(
        "completion_recorded_at_ms".to_string(),
        Value::Number(now_ms().into()),
    );
    if let Some(error) = event.error.as_ref() {
        object.insert("dispatch_error".to_string(), Value::String(error.clone()));
    }
    *metadata = Some(Value::Object(object));
}

fn outbox_id_for_idempotency_key(idempotency_key: &str) -> String {
    format!(
        "outbox-{}",
        crate::sha256_hex(&[idempotency_key])
            .chars()
            .take(16)
            .collect::<String>()
    )
}

fn provider_from_tool(tool: &str) -> Option<String> {
    tool.strip_prefix("mcp.")
        .and_then(|rest| rest.split('.').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn target_from_args(args: &Value) -> Option<String> {
    for pointer in [
        "/owner_repo",
        "/repo",
        "/repository",
        "/channel",
        "/channel_id",
        "/thread_ts",
        "/database_id",
        "/page_id",
        "/id",
    ] {
        let value = args.pointer(pointer).and_then(Value::as_str).map(str::trim);
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_tools::{ToolDispatchPolicyOutcome, ToolDispatchSource, ToolDispatchStatus};
    use tandem_types::TenantContext;

    fn external_event(dispatch_id: &str, tenant: TenantContext) -> ToolDispatchPreSendEvent {
        ToolDispatchPreSendEvent {
            dispatch_id: dispatch_id.to_string(),
            tool: "mcp.github.create_issue".to_string(),
            canonical_tool: Some("mcp.github.create_issue".to_string()),
            args: json!({ "owner_repo": "frumu-ai/tandem", "title": "hello" }),
            tenant_context: tenant,
            source: ToolDispatchSource::new("automation_v2")
                .run("run-1")
                .node("node-1"),
            scope_allowlist: vec!["mcp.github.create_issue".to_string()],
            policy_outcome: ToolDispatchPolicyOutcome::Allowed,
            policy_decision_id: Some("policy-1".to_string()),
            payload_digest: Some("sha256:payload".to_string()),
            external_side_effect: true,
            risk_tier: Some("external_send".to_string()),
        }
    }

    fn completion_event(dispatch_id: String, tenant: TenantContext) -> ToolDispatchLedgerEvent {
        ToolDispatchLedgerEvent {
            dispatch_id: Some(dispatch_id),
            tool: "mcp.github.create_issue".to_string(),
            canonical_tool: Some("mcp.github.create_issue".to_string()),
            tenant_context: tenant,
            source: ToolDispatchSource::new("automation_v2")
                .run("run-1")
                .node("node-1"),
            scope_allowlist: vec!["mcp.github.create_issue".to_string()],
            policy_outcome: ToolDispatchPolicyOutcome::Allowed,
            receipt_phase: ToolDispatchReceiptPhase::ExecutionCompleted,
            policy_decision_id: Some("policy-1".to_string()),
            payload_digest: Some("sha256:payload".to_string()),
            status: ToolDispatchStatus::Succeeded,
            error: None,
        }
    }

    fn outbox_gate_denial_event(
        dispatch_id: String,
        tenant: TenantContext,
    ) -> ToolDispatchLedgerEvent {
        ToolDispatchLedgerEvent {
            dispatch_id: Some(dispatch_id),
            tool: "mcp.github.create_issue".to_string(),
            canonical_tool: Some("mcp.github.create_issue".to_string()),
            tenant_context: tenant,
            source: ToolDispatchSource::new("automation_v2")
                .run("run-1")
                .node("node-1"),
            scope_allowlist: vec!["mcp.github.create_issue".to_string()],
            policy_outcome: ToolDispatchPolicyOutcome::Allowed,
            receipt_phase: ToolDispatchReceiptPhase::ExecutionFailed,
            policy_decision_id: Some("policy-1".to_string()),
            payload_digest: Some("sha256:payload".to_string()),
            status: ToolDispatchStatus::Blocked,
            error: Some(
                "ToolDenied { reason: OutboxGate }: pre-send outbox already claimed".to_string(),
            ),
        }
    }

    fn blocked_policy_event(dispatch_id: String, tenant: TenantContext) -> ToolDispatchLedgerEvent {
        let mut event = completion_event(dispatch_id, tenant);
        event.receipt_phase = ToolDispatchReceiptPhase::PolicyDecision;
        event.status = ToolDispatchStatus::Blocked;
        event.error = Some("ToolDenied { reason: PolicyDenied }".to_string());
        event
    }

    fn blocked_execution_event(
        dispatch_id: String,
        tenant: TenantContext,
    ) -> ToolDispatchLedgerEvent {
        let mut event = completion_event(dispatch_id, tenant);
        event.receipt_phase = ToolDispatchReceiptPhase::ExecutionFailed;
        event.status = ToolDispatchStatus::Blocked;
        event.error = Some("execution blocked after pre-send reservation".to_string());
        event
    }

    #[tokio::test]
    async fn dispatch_receipts_persist_allow_deny_and_execution_outcomes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let succeeded = completion_event("dispatch-allowed".to_string(), tenant.clone());
        let mut allowed_policy = succeeded.clone();
        allowed_policy.receipt_phase = ToolDispatchReceiptPhase::PolicyDecision;
        let mut started = succeeded.clone();
        started.receipt_phase = ToolDispatchReceiptPhase::ExecutionStarted;
        let mut denied = blocked_policy_event("dispatch-denied".to_string(), tenant);
        denied.policy_outcome = ToolDispatchPolicyOutcome::Denied;

        persist_dispatch_receipt(&runtime_events_path, &allowed_policy)
            .await
            .expect("persist allowed policy receipt");
        persist_dispatch_receipt(&runtime_events_path, &started)
            .await
            .expect("persist execution-started receipt");
        persist_dispatch_receipt(&runtime_events_path, &succeeded)
            .await
            .expect("persist allowed execution receipt");
        persist_dispatch_receipt(&runtime_events_path, &denied)
            .await
            .expect("persist denial receipt");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.tool_effects.len(), 4);
        assert!(store.tool_effects.iter().any(|receipt| {
            receipt.effect_id == "tool-dispatch-dispatch-allowed-execution_completed"
                && receipt.status == StatefulToolEffectStatus::Succeeded
        }));
        assert!(store.tool_effects.iter().any(|receipt| {
            receipt.effect_id == "tool-dispatch-dispatch-allowed-policy_decision"
                && receipt.status == StatefulToolEffectStatus::Succeeded
        }));
        assert!(store.tool_effects.iter().any(|receipt| {
            receipt.effect_id == "tool-dispatch-dispatch-allowed-execution_started"
                && receipt.status == StatefulToolEffectStatus::Pending
        }));
        assert!(store.tool_effects.iter().any(|receipt| {
            receipt.effect_id == "tool-dispatch-dispatch-denied-policy_decision"
                && receipt.status == StatefulToolEffectStatus::Failed
                && receipt
                    .receipt_payload_redacted
                    .as_ref()
                    .and_then(|payload| payload.get("policy_outcome"))
                    .and_then(Value::as_str)
                    == Some("denied")
        }));
    }

    #[tokio::test]
    async fn external_dispatch_reserves_claims_and_marks_sent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = ToolDispatchPreSendEvent {
            dispatch_id: "tool-dispatch-test".to_string(),
            tool: "mcp.github.create_issue".to_string(),
            canonical_tool: Some("mcp.github.create_issue".to_string()),
            args: json!({ "owner_repo": "frumu-ai/tandem", "title": "hello" }),
            tenant_context: tenant.clone(),
            source: ToolDispatchSource::new("automation_v2")
                .run("run-1")
                .node("node-1"),
            scope_allowlist: vec!["mcp.github.create_issue".to_string()],
            policy_outcome: ToolDispatchPolicyOutcome::Allowed,
            policy_decision_id: Some("policy-1".to_string()),
            payload_digest: Some("sha256:payload".to_string()),
            external_side_effect: true,
            risk_tier: Some("external_send".to_string()),
        };

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");
        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].outbox_id, receipt.outbox_id);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 1);
        assert_eq!(
            store.outbox[0].idempotency_key.as_deref(),
            Some(receipt.idempotency_key.as_str())
        );
        assert_eq!(
            store.outbox[0]
                .metadata
                .as_ref()
                .and_then(|value| value.get("observed_before_execution"))
                .and_then(Value::as_bool),
            Some(true)
        );

        complete_pre_send_outbox(
            &runtime_events_path,
            &ToolDispatchLedgerEvent {
                dispatch_id: Some(receipt.idempotency_key),
                tool: "mcp.github.create_issue".to_string(),
                canonical_tool: Some("mcp.github.create_issue".to_string()),
                tenant_context: tenant,
                source: ToolDispatchSource::new("automation_v2")
                    .run("run-1")
                    .node("node-1"),
                scope_allowlist: vec!["mcp.github.create_issue".to_string()],
                policy_outcome: ToolDispatchPolicyOutcome::Allowed,
                receipt_phase: ToolDispatchReceiptPhase::ExecutionCompleted,
                policy_decision_id: Some("policy-1".to_string()),
                payload_digest: Some("sha256:payload".to_string()),
                status: ToolDispatchStatus::Succeeded,
                error: None,
            },
        )
        .await
        .expect("complete");

        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Sent);
        assert!(store.outbox[0].claim_expires_at_ms.is_none());
        assert_eq!(
            store.outbox[0]
                .metadata
                .as_ref()
                .and_then(|value| value.get("dispatch_status"))
                .and_then(Value::as_str),
            Some("succeeded")
        );
    }

    #[tokio::test]
    async fn claimed_pre_send_outbox_blocks_duplicate_prepare() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-claimed", tenant);

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event.clone())
            .await
            .expect("prepare")
            .expect("receipt");
        let error = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect_err("duplicate claim should fail closed");
        assert!(error.to_string().contains("already `claimed`"));

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 1);
        assert_eq!(
            store.outbox[0].idempotency_key.as_deref(),
            Some(receipt.idempotency_key.as_str())
        );
    }

    #[tokio::test]
    async fn expired_claimed_pre_send_outbox_can_be_reclaimed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-expired-claim", tenant);

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event.clone())
            .await
            .expect("prepare")
            .expect("receipt");
        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let mut row = load_stateful_reliability(&reliability_path).outbox[0].clone();
        row.claim_expires_at_ms = Some(0);
        upsert_stateful_outbox(&reliability_path, row)
            .await
            .expect("expire claim");

        let reclaimed = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("reclaim expired claim")
            .expect("receipt");

        assert_eq!(reclaimed.idempotency_key, receipt.idempotency_key);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 2);
        assert!(store.outbox[0]
            .claim_expires_at_ms
            .is_some_and(|value| value > 0));
    }

    #[tokio::test]
    async fn concurrent_pre_send_prepare_allows_one_claim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-concurrent", tenant);

        let (left, right) = tokio::join!(
            prepare_pre_send_outbox(&runtime_events_path, event.clone()),
            prepare_pre_send_outbox(&runtime_events_path, event)
        );
        let successes = [&left, &right]
            .into_iter()
            .filter(|result| matches!(result, Ok(Some(_))))
            .count();
        let failures = [&left, &right]
            .into_iter()
            .filter(|result| result.is_err())
            .count();
        assert_eq!(successes, 1);
        assert_eq!(failures, 1);

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 1);
    }

    #[tokio::test]
    async fn outbox_gate_denial_does_not_cancel_existing_claim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-gate-denied", tenant.clone());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");
        complete_pre_send_outbox(
            &runtime_events_path,
            &outbox_gate_denial_event(receipt.idempotency_key, tenant),
        )
        .await
        .expect("ignored gate denial");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 1);
        assert!(store.outbox[0].claim_expires_at_ms.is_some());
    }

    #[tokio::test]
    async fn sent_pre_send_outbox_blocks_duplicate_prepare() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-sent", tenant.clone());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event.clone())
            .await
            .expect("prepare")
            .expect("receipt");
        complete_pre_send_outbox(
            &runtime_events_path,
            &completion_event(receipt.idempotency_key.clone(), tenant),
        )
        .await
        .expect("complete");

        let error = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect_err("sent row should fail closed");
        assert!(error.to_string().contains("already `sent`"));

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Sent);
        assert_eq!(store.outbox[0].attempts, 1);
        assert!(store.outbox[0].claim_expires_at_ms.is_none());
    }

    #[tokio::test]
    async fn blocked_dispatch_cancels_active_pre_send_claim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-blocked", tenant.clone());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");
        complete_pre_send_outbox(
            &runtime_events_path,
            &blocked_execution_event(receipt.idempotency_key, tenant),
        )
        .await
        .expect("complete blocked");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Cancelled);
        assert!(store.outbox[0].claim_expires_at_ms.is_none());
    }

    #[tokio::test]
    async fn cancelled_pre_send_outbox_can_be_reclaimed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-cancelled-retry", tenant.clone());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event.clone())
            .await
            .expect("prepare")
            .expect("receipt");
        complete_pre_send_outbox(
            &runtime_events_path,
            &blocked_execution_event(receipt.idempotency_key.clone(), tenant),
        )
        .await
        .expect("complete blocked");
        let reclaimed = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("reclaim cancelled")
            .expect("receipt");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(reclaimed.idempotency_key, receipt.idempotency_key);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(store.outbox[0].attempts, 2);
        assert!(store.outbox[0].claim_expires_at_ms.is_some());
    }

    #[tokio::test]
    async fn blocked_dispatch_does_not_cancel_sent_pre_send_outbox() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let event = external_event("tool-dispatch-sent-then-blocked", tenant.clone());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");
        complete_pre_send_outbox(
            &runtime_events_path,
            &completion_event(receipt.idempotency_key.clone(), tenant.clone()),
        )
        .await
        .expect("complete sent");
        complete_pre_send_outbox(
            &runtime_events_path,
            &blocked_execution_event(receipt.idempotency_key, tenant),
        )
        .await
        .expect("ignore later blocked event");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Sent);
        assert!(store.outbox[0].claim_expires_at_ms.is_none());
        assert_eq!(
            store.outbox[0]
                .metadata
                .as_ref()
                .and_then(|value| value.get("dispatch_status"))
                .and_then(Value::as_str),
            Some("succeeded")
        );
    }

    #[tokio::test]
    async fn external_side_effect_without_risk_tier_is_gated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let mut event = external_event("tool-dispatch-no-risk-tier", tenant);
        event.risk_tier = None;

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(
            store.outbox[0].idempotency_key.as_deref(),
            Some(receipt.idempotency_key.as_str())
        );
    }

    #[tokio::test]
    async fn gated_risk_tier_without_external_side_effect_flag_is_gated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let mut event = external_event("tool-dispatch-inferred-risk-tier", tenant);
        event.external_side_effect = false;
        event.risk_tier = Some("consequential_write".to_string());

        let receipt = prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .expect("receipt");

        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        let store = load_stateful_reliability(&reliability_path);
        assert_eq!(store.outbox.len(), 1);
        assert_eq!(store.outbox[0].status, StatefulOutboxStatus::Claimed);
        assert_eq!(
            store.outbox[0].idempotency_key.as_deref(),
            Some(receipt.idempotency_key.as_str())
        );
    }

    #[tokio::test]
    async fn local_non_external_dispatch_is_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime_events_path = dir.path().join("events.json");
        let event = ToolDispatchPreSendEvent {
            dispatch_id: "tool-dispatch-read".to_string(),
            tool: "read".to_string(),
            canonical_tool: Some("read".to_string()),
            args: json!({ "path": "README.md" }),
            tenant_context: TenantContext::local_implicit(),
            source: ToolDispatchSource::new("engine_loop"),
            scope_allowlist: Vec::new(),
            policy_outcome: ToolDispatchPolicyOutcome::Allowed,
            policy_decision_id: None,
            payload_digest: None,
            external_side_effect: false,
            risk_tier: None,
        };

        assert!(prepare_pre_send_outbox(&runtime_events_path, event)
            .await
            .expect("prepare")
            .is_none());
        let reliability_path =
            stateful_reliability_path_from_runtime_events_path(&runtime_events_path);
        assert!(load_stateful_reliability(&reliability_path)
            .outbox
            .is_empty());
    }
}
