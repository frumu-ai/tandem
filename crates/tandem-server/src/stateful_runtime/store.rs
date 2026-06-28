use std::path::{Path, PathBuf};

use anyhow::Context;
use tandem_types::TenantContext;
use tokio::io::AsyncWriteExt;

use super::types::{StatefulRunEventRecord, StatefulRunSnapshotRecord};

#[derive(Debug, Clone, Copy)]
pub struct StatefulRunEventQuery<'a> {
    pub run_id: &'a str,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

pub async fn append_stateful_run_event(
    path: &Path,
    record: &StatefulRunEventRecord,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create stateful run event directory {}",
                parent.display()
            )
        })?;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("failed to open stateful run event log {}", path.display()))?;
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    file.write_all(&line)
        .await
        .with_context(|| format!("failed to append stateful run event log {}", path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("failed to flush stateful run event log {}", path.display()))?;
    Ok(())
}

pub fn load_stateful_run_events(path: &Path) -> Vec<StatefulRunEventRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rows = content
        .lines()
        .enumerate()
        .filter_map(
            |(index, line)| match serde_json::from_str::<StatefulRunEventRecord>(line) {
                Ok(record) => Some(record),
                Err(error) => {
                    tracing::warn!(
                        line = index + 1,
                        error = %error,
                        "skipping invalid stateful run event row"
                    );
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.seq);
    rows
}

pub fn query_stateful_run_events(
    path: &Path,
    tenant: &TenantContext,
    query: StatefulRunEventQuery<'_>,
) -> Vec<StatefulRunEventRecord> {
    let mut rows = load_stateful_run_events(path)
        .into_iter()
        .filter(|row| row.run_id == query.run_id)
        .filter(|row| {
            query
                .after_seq
                .map(|after_seq| row.seq > after_seq)
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

pub async fn write_stateful_run_snapshot(
    root: &Path,
    snapshot: &StatefulRunSnapshotRecord,
) -> anyhow::Result<PathBuf> {
    let dir = root.join(safe_path_segment(&snapshot.run_id));
    tokio::fs::create_dir_all(&dir).await.with_context(|| {
        format!(
            "failed to create stateful snapshot directory {}",
            dir.display()
        )
    })?;
    let path = dir.join(format!("{}.json", safe_path_segment(&snapshot.snapshot_id)));
    let tmp_path = tmp_path_for(&path);
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .with_context(|| format!("failed to create stateful snapshot {}", tmp_path.display()))?;
    let content = serde_json::to_vec_pretty(snapshot)?;
    file.write_all(&content)
        .await
        .with_context(|| format!("failed to write stateful snapshot {}", tmp_path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("failed to flush stateful snapshot {}", tmp_path.display()))?;
    drop(file);
    tokio::fs::rename(&tmp_path, &path)
        .await
        .with_context(|| format!("failed to publish stateful snapshot {}", path.display()))?;
    Ok(path)
}

pub fn read_stateful_run_snapshot(path: &Path) -> anyhow::Result<StatefulRunSnapshotRecord> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read stateful snapshot {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse stateful snapshot {}", path.display()))
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    tmp.set_extension(extension);
    tmp
}

fn safe_path_segment(value: &str) -> String {
    let segment = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if segment.is_empty() || segment == "." || segment == ".." {
        "_".to_string()
    } else {
        segment
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{PrincipalKind, PrincipalRef, TenantContext};
    use uuid::Uuid;

    use super::*;
    use crate::stateful_runtime::types::{
        StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulRuntimeScope,
        StatefulWorkflowRunStatus,
    };

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn event(seq: u64, run_id: &str, tenant_context: TenantContext) -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("event-{seq}"),
            run_id: run_id.to_string(),
            seq,
            event_type: "stateful_runtime.test".to_string(),
            occurred_at_ms: seq * 100,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            actor: Some(PrincipalRef::new(PrincipalKind::HumanUser, "user-a")),
            phase_id: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({ "seq": seq }),
        }
    }

    #[tokio::test]
    async fn query_filters_stateful_events_by_tenant_run_and_sequence() {
        let path =
            std::env::temp_dir().join(format!("stateful-runtime-events-{}.jsonl", Uuid::new_v4()));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");

        for record in [
            event(1, "run-a", tenant_a.clone()),
            event(2, "run-b", tenant_a.clone()),
            event(3, "run-a", tenant_b),
            event(4, "run-a", tenant_a.clone()),
        ] {
            append_stateful_run_event(&path, &record)
                .await
                .expect("append");
        }

        let rows = query_stateful_run_events(
            &path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: Some(1),
                limit: None,
            },
        );

        assert_eq!(rows.iter().map(|row| row.seq).collect::<Vec<_>>(), vec![4]);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn snapshot_paths_are_scoped_under_sanitized_run_directory() {
        let root =
            std::env::temp_dir().join(format!("stateful-runtime-snapshots-{}", Uuid::new_v4()));
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot/../a".to_string(),
            run_id: "run/../a".to_string(),
            seq: 7,
            created_at_ms: 700,
            scope: StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
            status: StatefulWorkflowRunStatus::Running,
            phase_id: Some("phase-a".to_string()),
            source_record_kind: None,
            checkpoint: Some(json!({ "phase": "phase-a" })),
            payload_digest: Some("sha256:test".to_string()),
            workflow_definition_version: None,
            metadata: None,
        };

        let path = write_stateful_run_snapshot(&root, &snapshot)
            .await
            .expect("write snapshot");
        assert!(path.starts_with(&root));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("snapshot_.._a.json")
        );

        let loaded = read_stateful_run_snapshot(&path).expect("read snapshot");
        assert_eq!(loaded.snapshot_id, snapshot.snapshot_id);
        assert_eq!(loaded.run_id, snapshot.run_id);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn snapshot_paths_rewrite_dot_only_and_empty_segments() {
        let root =
            std::env::temp_dir().join(format!("stateful-runtime-snapshots-dot-{}", Uuid::new_v4()));
        for (run_id, snapshot_id) in [("..", "."), ("", "")] {
            let snapshot = StatefulRunSnapshotRecord {
                schema_version: 1,
                snapshot_id: snapshot_id.to_string(),
                run_id: run_id.to_string(),
                seq: 1,
                created_at_ms: 100,
                scope: StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
                status: StatefulWorkflowRunStatus::Running,
                phase_id: None,
                source_record_kind: None,
                checkpoint: None,
                payload_digest: None,
                workflow_definition_version: None,
                metadata: None,
            };

            let path = write_stateful_run_snapshot(&root, &snapshot)
                .await
                .expect("write snapshot");

            assert!(path.starts_with(&root));
            assert_eq!(
                path.parent()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str()),
                Some("_")
            );
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some("_.json")
            );
        }
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
