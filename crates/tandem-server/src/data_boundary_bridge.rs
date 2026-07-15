// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Bridges engine-loop `data_boundary.*` bus events into the protected audit
//! ledger (TAN-391). The engine loop cannot write protected audit records
//! itself (it has no `AppState`), so this server-side subscriber composes the
//! existing pieces: broadcast subscribe → match on the event family → append a
//! hash-chained record.
//!
//! Event payloads are produced by the audit-safe `DataBoundaryEvent` shape
//! (classes, counts, hashes, reason codes — never raw content), so they are
//! forwarded into the ledger as-is.

use serde_json::Value;
use tandem_types::{EngineEvent, TenantContext};
use tokio::sync::broadcast::error::RecvError;

use crate::audit::append_protected_audit_event;
use crate::AppState;

const PROTECTED_AUDIT_RECORDED_PROPERTY: &str = "protectedAuditRecorded";

/// `data_boundary.evaluated` with a plain `allow` action fires on every
/// provider call; only decisions that found something (allow-with-audit,
/// redact/tokenize, block, approval, local routing) belong in the durable
/// tamper-evident ledger.
pub(crate) fn data_boundary_event_needs_protected_audit(event: &EngineEvent) -> bool {
    event.event_type.starts_with("data_boundary.")
        && !event
            .properties
            .get(PROTECTED_AUDIT_RECORDED_PROPERTY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && event
            .properties
            .get("action")
            .and_then(Value::as_str)
            .is_some_and(|action| action != "allow")
}

fn tenant_context_from_event(event: &EngineEvent) -> TenantContext {
    let tenant = event.properties.get("tenant");
    let field = |name: &str| {
        tenant
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let mut context = TenantContext::local_implicit();
    if let Some(org_id) = field("organization_id") {
        context.org_id = org_id;
    }
    if let Some(workspace_id) = field("workspace_id") {
        context.workspace_id = workspace_id;
    }
    context.deployment_id = field("deployment_id");
    context
}

pub(crate) fn mark_data_boundary_protected_audit_recorded(event: &mut EngineEvent) {
    // Direct provider paths append before returning a permit, then mark only
    // the bus copy so this subscriber does not persist the decision twice.
    if let Value::Object(properties) = &mut event.properties {
        properties.insert(
            PROTECTED_AUDIT_RECORDED_PROPERTY.to_string(),
            Value::Bool(true),
        );
    }
}

pub(crate) async fn record_data_boundary_protected_audit(
    state: &AppState,
    event: &EngineEvent,
) -> anyhow::Result<bool> {
    if !data_boundary_event_needs_protected_audit(event) {
        return Ok(false);
    }
    let tenant_context = tenant_context_from_event(event);
    let actor = event
        .properties
        .get("sessionID")
        .and_then(Value::as_str)
        .map(str::to_string);
    append_protected_audit_event(
        state,
        event.event_type.clone(),
        &tenant_context,
        actor,
        event.properties.clone(),
    )
    .await?;
    Ok(true)
}

/// Long-running subscriber; spawned alongside the other background loops in
/// `http.rs`.
pub async fn run_data_boundary_audit_bridge(state: AppState) {
    let mut rx = state.event_bus.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if let Err(error) = record_data_boundary_protected_audit(&state, &event).await {
                    tracing::error!(
                        event_type = %event.event_type,
                        error = ?error,
                        "data-boundary protected audit bridge failed"
                    );
                }
            }
            Err(RecvError::Closed) => break,
            Err(RecvError::Lagged(_)) => continue,
        }
    }
}
