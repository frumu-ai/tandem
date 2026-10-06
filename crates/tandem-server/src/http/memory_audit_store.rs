// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use anyhow::Context;
use axum::http::StatusCode;
use tandem_types::TenantContext;

use crate::governance_store::{self, GovernanceStoreFile};
use crate::AppState;

pub(crate) async fn append_memory_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    mut event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    event.tenant_context = tenant_context.clone();
    let line = serde_json::to_string(&event).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    governance_store::for_state(state)
        .append_jsonl_line(
            GovernanceStoreFile::MemoryAudit,
            &line,
            tenant_context,
            None,
            &event.audit_id,
            true,
        )
        .await
        .map_err(|error| {
            tracing::error!(
                tenant_org_id = %tenant_context.org_id,
                tenant_workspace_id = %tenant_context.workspace_id,
                audit_id = %event.audit_id,
                error = ?error,
                "memory audit persistence failed"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

/// Record protected audit admission before starting a memory mutation. This is
/// an attempt, not a successful mutation: it advances the audit head and cache
/// with a distinct pending event correlated to the planned success audit ID.
/// It checks the real chain/encryption/anchor append path, but does not reserve
/// future filesystem or KMS availability or make the file audit and SQL atomic.
pub(crate) async fn append_memory_mutation_admission(
    state: &AppState,
    tenant_context: &TenantContext,
    success_event: &crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    if success_event.status != "ok"
        || !matches!(
            success_event.action.as_str(),
            "memory_promote" | "memory_demote"
        )
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut admission = success_event.clone();
    admission.audit_id = uuid::Uuid::new_v4().to_string();
    admission.action = format!("{}_admission", success_event.action);
    admission.status = "pending".to_string();
    admission.detail =
        Some(serde_json::json!({"success_audit_id": success_event.audit_id}).to_string());
    admission.created_at_ms = crate::now_ms();
    // append_memory_audit releases its file/process/cache locks before return;
    // no audit lock may be held across the subsequent target-store transaction.
    append_memory_audit(state, tenant_context, admission).await
}

pub(crate) async fn load_memory_audit_events_strict(
    state: &AppState,
) -> anyhow::Result<Vec<crate::MemoryAuditEvent>> {
    let lines = match governance_store::for_state(state)
        .read_jsonl_lines(GovernanceStoreFile::MemoryAudit)
        .await?
    {
        Some(lines) => lines,
        None => return Ok(Vec::new()),
    };

    let mut events = Vec::with_capacity(lines.len());
    for line in lines {
        let event = serde_json::from_str::<crate::MemoryAuditEvent>(line.trim())
            .context("protected memory audit store contains a malformed record")?;
        events.push(event);
    }
    Ok(events)
}

pub(crate) async fn load_memory_audit_events(state: &AppState) -> Vec<crate::MemoryAuditEvent> {
    match load_memory_audit_events_strict(state).await {
        Ok(events) => events,
        Err(error) => {
            tracing::error!(error = ?error, "failed to load protected memory audit store");
            Vec::new()
        }
    }
}
