use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tandem_types::{EngineEvent, RuntimeEvent, TenantContext};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEventLogRow {
    #[serde(flatten)]
    pub event: RuntimeEvent,
}

impl<'de> Deserialize<'de> for RuntimeEventLogRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RuntimeEvent::deserialize(deserializer).map(|event| Self { event })
    }
}

impl RuntimeEventLogRow {
    pub fn from_engine_event(event: &EngineEvent) -> Option<Self> {
        let event = RuntimeEvent::from_engine_event(event)?;
        if event.envelope.run_id.is_none() && event.envelope.session_id.is_none() {
            return None;
        }
        Some(Self { event })
    }

    pub fn event_id(&self) -> &str {
        &self.event.envelope.event_id
    }

    pub fn seq(&self) -> u64 {
        self.event.envelope.seq
    }

    pub fn run_id(&self) -> Option<&str> {
        self.event.envelope.run_id.as_deref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.event.envelope.session_id.as_deref()
    }

    pub fn occurred_at_ms(&self) -> u64 {
        self.event.envelope.occurred_at_ms
    }

    pub fn tenant_context(&self) -> Option<&TenantContext> {
        self.event.envelope.tenant_context.as_ref()
    }

    pub fn visible_to_tenant(&self, tenant: &TenantContext) -> bool {
        if tenant.is_local_implicit() {
            return true;
        }
        let Some(event_tenant) = self.tenant_context() else {
            return false;
        };
        event_tenant.org_id == tenant.org_id
            && event_tenant.workspace_id == tenant.workspace_id
            && event_tenant.deployment_id == tenant.deployment_id
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeEventLogQuery<'a> {
    pub run_id: &'a str,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

pub async fn append_runtime_event_log_row(
    path: &Path,
    row: &RuntimeEventLogRow,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create runtime event log directory {}",
                parent.display()
            )
        })?;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("failed to open runtime event log {}", path.display()))?;
    let mut line = serde_json::to_vec(row)?;
    line.push(b'\n');
    file.write_all(&line)
        .await
        .with_context(|| format!("failed to append runtime event log {}", path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("failed to flush runtime event log {}", path.display()))?;
    Ok(())
}

pub fn load_runtime_event_log_rows(path: &Path) -> Vec<RuntimeEventLogRow> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rows = content
        .lines()
        .enumerate()
        .filter_map(
            |(index, line)| match serde_json::from_str::<RuntimeEvent>(line) {
                Ok(event) => Some(RuntimeEventLogRow { event }),
                Err(error) => {
                    tracing::warn!(
                        line = index + 1,
                        error = %error,
                        "skipping invalid runtime event log row"
                    );
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by_key(RuntimeEventLogRow::seq);
    rows
}

pub fn query_runtime_event_log(
    path: &Path,
    tenant: &TenantContext,
    query: RuntimeEventLogQuery<'_>,
) -> Vec<RuntimeEventLogRow> {
    let mut rows = load_runtime_event_log_rows(path)
        .into_iter()
        .filter(|row| row.run_id() == Some(query.run_id))
        .filter(|row| {
            query
                .after_seq
                .map(|after_seq| row.seq() > after_seq)
                .unwrap_or(true)
        })
        .filter(|row| row.visible_to_tenant(tenant))
        .collect::<Vec<_>>();
    if let Some(limit) = query.limit.filter(|limit| *limit > 0) {
        if rows.len() > limit {
            rows.truncate(limit);
        }
    }
    rows
}

pub async fn prune_runtime_event_log(
    path: &Path,
    retention_ms: u64,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 || !path.exists() {
        return Ok(0);
    }
    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    let rows = load_runtime_event_log_rows(path);
    let original_len = rows.len();
    let retained = rows
        .into_iter()
        .filter(|row| row.occurred_at_ms() >= cutoff_ms)
        .collect::<Vec<_>>();
    if retained.len() == original_len {
        return Ok(0);
    }
    write_runtime_event_log_rows(path, &retained).await?;
    Ok(original_len.saturating_sub(retained.len()))
}

async fn write_runtime_event_log_rows(
    path: &Path,
    rows: &[RuntimeEventLogRow],
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp_path = runtime_event_log_tmp_path(path);
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    for row in rows {
        let mut line = serde_json::to_vec(row)?;
        line.push(b'\n');
        file.write_all(&line).await?;
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp_path, path).await?;
    Ok(())
}

fn runtime_event_log_tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    tmp.set_extension(extension);
    tmp
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{EngineEvent, RuntimeEventEnvelope, TenantContext};
    use uuid::Uuid;

    use super::*;

    fn event(
        seq: u64,
        run_id: &str,
        tenant_context: Option<TenantContext>,
        occurred_at_ms: u64,
    ) -> EngineEvent {
        EngineEvent::new(
            "session.run.started",
            json!({
                "runID": run_id,
                "sessionID": "session-a",
                "tenantContext": tenant_context,
            }),
        )
        .with_envelope(RuntimeEventEnvelope {
            event_id: format!("evt-{seq}"),
            seq,
            schema_version: 1,
            occurred_at_ms,
            session_id: Some("session-a".to_string()),
            run_id: Some(run_id.to_string()),
            node_id: None,
            tenant_context,
        })
    }

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    #[tokio::test]
    async fn query_filters_by_run_sequence_and_tenant() {
        let path = std::env::temp_dir().join(format!("runtime-events-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        for event in [
            event(1, "run-a", Some(tenant_a.clone()), 100),
            event(2, "run-b", Some(tenant_a.clone()), 200),
            event(3, "run-a", Some(tenant_b.clone()), 300),
            event(4, "run-a", Some(tenant_a.clone()), 400),
        ] {
            let row = RuntimeEventLogRow::from_engine_event(&event).expect("canonical row");
            append_runtime_event_log_row(&path, &row)
                .await
                .expect("append");
        }

        let rows = query_runtime_event_log(
            &path,
            &tenant_a,
            RuntimeEventLogQuery {
                run_id: "run-a",
                after_seq: Some(1),
                limit: None,
            },
        );

        assert_eq!(
            rows.iter().map(RuntimeEventLogRow::seq).collect::<Vec<_>>(),
            vec![4]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn prune_removes_rows_older_than_retention_window() {
        let path = std::env::temp_dir().join(format!("runtime-events-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        for event in [
            event(1, "run-a", Some(tenant_a.clone()), 100),
            event(2, "run-a", Some(tenant_a), 900),
        ] {
            let row = RuntimeEventLogRow::from_engine_event(&event).expect("canonical row");
            append_runtime_event_log_row(&path, &row)
                .await
                .expect("append");
        }

        let pruned = prune_runtime_event_log(&path, 500, 1_000)
            .await
            .expect("prune");

        assert_eq!(pruned, 1);
        let rows = load_runtime_event_log_rows(&path);
        assert_eq!(
            rows.iter().map(RuntimeEventLogRow::seq).collect::<Vec<_>>(),
            vec![2]
        );
        let _ = tokio::fs::remove_file(path).await;
    }
}
