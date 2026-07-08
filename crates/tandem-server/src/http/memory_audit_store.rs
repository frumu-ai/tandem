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
        .append_jsonl_line(GovernanceStoreFile::MemoryAudit, &line, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

pub(crate) async fn load_memory_audit_events(state: &AppState) -> Vec<crate::MemoryAuditEvent> {
    let Ok(Some(lines)) = governance_store::for_state(state)
        .read_jsonl_lines(GovernanceStoreFile::MemoryAudit)
        .await
    else {
        return Vec::new();
    };

    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<crate::MemoryAuditEvent>(line.trim()).ok())
        .collect()
}
