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
