use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::TenantContext;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{now_ms, AppState};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDurability {
    BestEffort,
    DurableRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedAuditEnvelope {
    pub event_id: String,
    pub durability: AuditDurability,
    pub event_type: String,
    #[serde(default)]
    pub tenant_context: TenantContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub payload: Value,
    pub created_at_ms: u64,
}

pub fn protected_audit_event_matches_tenant(
    event: &ProtectedAuditEnvelope,
    tenant_context: &TenantContext,
) -> bool {
    tenant_context.is_local_implicit()
        || (event.tenant_context.org_id == tenant_context.org_id
            && event.tenant_context.workspace_id == tenant_context.workspace_id
            && event.tenant_context.deployment_id == tenant_context.deployment_id)
}

pub async fn load_protected_audit_events_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Vec<ProtectedAuditEnvelope> {
    let content = match fs::read_to_string(&state.protected_audit_path).await {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };
    let mut rows = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<ProtectedAuditEnvelope>(trimmed).ok()
        })
        .filter(|event| protected_audit_event_matches_tenant(event, tenant_context))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then(a.event_id.cmp(&b.event_id))
    });
    rows
}

pub async fn append_protected_audit_event(
    state: &AppState,
    event_type: impl Into<String>,
    tenant_context: &TenantContext,
    actor: Option<String>,
    payload: Value,
) -> anyhow::Result<()> {
    let path = state.protected_audit_path.clone();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let row = ProtectedAuditEnvelope {
        event_id: Uuid::new_v4().to_string(),
        durability: AuditDurability::DurableRequired,
        event_type: event_type.into(),
        tenant_context: tenant_context.clone(),
        actor,
        payload,
        created_at_ms: now_ms(),
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    file.write_all(serde_json::to_string(&row)?.as_bytes())
        .await?;
    file.write_all(b"\n").await?;
    file.flush().await?;
    Ok(())
}
