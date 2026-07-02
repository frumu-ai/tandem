use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::Value;
use tandem_types::TenantContext;
use tokio::io::AsyncWriteExt;

use super::durable_io::{repair_jsonl_torn_tail, sync_parent_dir, write_file_atomically};
use super::phases::phase_state_from_status;
use super::types::{StatefulRunEventRecord, StatefulRunSnapshotRecord};

static STATEFUL_RUN_EVENT_APPEND_ONCE_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone)]
pub struct StatefulRuntimeStoragePaths {
    pub run_events_path: PathBuf,
    pub snapshots_root: PathBuf,
    pub waits_path: PathBuf,
}

impl StatefulRuntimeStoragePaths {
    pub fn new(run_events_path: PathBuf, snapshots_root: PathBuf, waits_path: PathBuf) -> Self {
        Self {
            run_events_path,
            snapshots_root,
            waits_path,
        }
    }

    pub fn from_runtime_events_path(runtime_events_path: &Path) -> Self {
        let runtime_root = runtime_events_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            run_events_path: runtime_root.join("stateful_events.jsonl"),
            snapshots_root: runtime_root.join("stateful_snapshots"),
            waits_path: runtime_root.join("stateful_waits.json"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StatefulRunEventQuery<'a> {
    pub run_id: &'a str,
    pub after_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: Option<usize>,
    pub tail: bool,
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
    repair_jsonl_torn_tail(path, "stateful run event log").await?;

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
    file.sync_all()
        .await
        .with_context(|| format!("failed to sync stateful run event log {}", path.display()))?;
    drop(file);
    sync_parent_dir(path, "stateful run event log").await?;
    Ok(())
}

pub async fn append_stateful_run_event_once(
    path: &Path,
    record: &StatefulRunEventRecord,
) -> anyhow::Result<bool> {
    let _guard = STATEFUL_RUN_EVENT_APPEND_ONCE_LOCK.lock().await;
    if stateful_run_event_exists(path, record) {
        return Ok(false);
    }
    append_stateful_run_event(path, record).await?;
    Ok(true)
}

pub async fn append_stateful_run_event_once_with_next_seq(
    path: &Path,
    tenant_context: &TenantContext,
    record: &StatefulRunEventRecord,
) -> anyhow::Result<(bool, u64)> {
    let _guard = STATEFUL_RUN_EVENT_APPEND_ONCE_LOCK.lock().await;
    let rows = query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id: &record.run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    );
    if let Some(existing) = rows
        .iter()
        .find(|existing| existing.event_id == record.event_id)
    {
        return Ok((false, existing.seq));
    }
    let seq = rows
        .last()
        .map(|event| event.seq.saturating_add(1))
        .unwrap_or(1);
    let mut record = record.clone();
    record.seq = seq;
    append_stateful_run_event(path, &record).await?;
    Ok((true, seq))
}

fn stateful_run_event_exists(path: &Path, record: &StatefulRunEventRecord) -> bool {
    load_stateful_run_events(path)
        .into_iter()
        .any(|existing| existing.run_id == record.run_id && existing.event_id == record.event_id)
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
        .filter(|row| {
            query
                .before_seq
                .map(|before_seq| row.seq < before_seq)
                .unwrap_or(true)
        })
        .filter(|row| row.visible_to_tenant(tenant))
        .collect::<Vec<_>>();
    if let Some(limit) = query.limit.filter(|limit| *limit > 0) {
        if rows.len() > limit {
            if query.tail {
                let remove_count = rows.len() - limit;
                rows.drain(0..remove_count);
            } else {
                rows.truncate(limit);
            }
        }
    }
    rows
}

pub fn next_stateful_run_event_seq(
    path: &Path,
    tenant_context: &TenantContext,
    run_id: &str,
) -> u64 {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    )
    .last()
    .map(|event| event.seq.saturating_add(1))
    .unwrap_or(1)
}

pub fn stateful_run_event_seq_by_id(
    path: &Path,
    tenant_context: &TenantContext,
    run_id: &str,
    event_id: &str,
) -> Option<u64> {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    )
    .into_iter()
    .find(|event| event.event_id == event_id)
    .map(|event| event.seq)
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
    let content = serde_json::to_vec_pretty(snapshot)?;
    write_file_atomically(&path, &content, "stateful snapshot").await?;
    Ok(path)
}

pub fn list_stateful_run_snapshots(
    root: &Path,
    tenant: &TenantContext,
    run_id: &str,
    limit: Option<usize>,
) -> Vec<StatefulRunSnapshotRecord> {
    let dir = root.join(safe_path_segment(run_id));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut snapshots = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .filter_map(|path| match read_stateful_run_snapshot(&path) {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %error,
                    "skipping invalid stateful run snapshot"
                );
                None
            }
        })
        .filter(|snapshot| snapshot.run_id == run_id)
        .filter(|snapshot| snapshot.visible_to_tenant(tenant))
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|snapshot| snapshot.seq);
    if let Some(limit) = limit.filter(|limit| *limit > 0) {
        if snapshots.len() > limit {
            let remove_count = snapshots.len() - limit;
            snapshots.drain(0..remove_count);
        }
    }
    snapshots
}

pub fn read_stateful_run_snapshot(path: &Path) -> anyhow::Result<StatefulRunSnapshotRecord> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read stateful snapshot {}", path.display()))?;
    let value = serde_json::from_str::<Value>(&content)
        .with_context(|| format!("failed to parse stateful snapshot {}", path.display()))?;
    let has_phase = value.get("phase").is_some();
    let has_phase_history = value.get("phase_history").is_some();
    let has_allowed_next_phases = value.get("allowed_next_phases").is_some();
    let mut snapshot = serde_json::from_value::<StatefulRunSnapshotRecord>(value)
        .with_context(|| format!("failed to parse stateful snapshot {}", path.display()))?;
    hydrate_legacy_snapshot_phase_fields(
        &mut snapshot,
        has_phase,
        has_phase_history,
        has_allowed_next_phases,
    );
    Ok(snapshot)
}

fn hydrate_legacy_snapshot_phase_fields(
    snapshot: &mut StatefulRunSnapshotRecord,
    has_phase: bool,
    has_phase_history: bool,
    has_allowed_next_phases: bool,
) {
    if !has_phase {
        let phase_state = phase_state_from_status(
            &snapshot.run_id,
            &snapshot.status,
            snapshot.created_at_ms,
            snapshot.phase_id.as_deref(),
        );
        snapshot.phase = phase_state.phase;
        if !has_phase_history {
            snapshot.phase_history = phase_state.phase_history;
        }
        if !has_allowed_next_phases {
            snapshot.allowed_next_phases = phase_state.allowed_next_phases;
        }
        return;
    }

    if !has_allowed_next_phases {
        snapshot.allowed_next_phases = snapshot.phase.allowed_next_phases().to_vec();
    }
}

pub fn read_stateful_run_snapshot_for_run(
    root: &Path,
    tenant: &TenantContext,
    run_id: &str,
    snapshot_id: &str,
) -> anyhow::Result<Option<StatefulRunSnapshotRecord>> {
    let path = stateful_run_snapshot_path(root, run_id, snapshot_id);
    if !path.exists() {
        return Ok(None);
    }
    let snapshot = read_stateful_run_snapshot(&path)?;
    if snapshot.run_id != run_id || snapshot.snapshot_id != snapshot_id {
        return Ok(None);
    }
    if !snapshot.visible_to_tenant(tenant) {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub fn stateful_run_snapshot_path(root: &Path, run_id: &str, snapshot_id: &str) -> PathBuf {
    root.join(safe_path_segment(run_id))
        .join(format!("{}.json", safe_path_segment(snapshot_id)))
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
    use crate::stateful_runtime::StatefulWorkflowPhase;

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
            phase_transition: None,
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
                before_seq: None,
                limit: None,
                tail: false,
            },
        );

        assert_eq!(rows.iter().map(|row| row.seq).collect::<Vec<_>>(), vec![4]);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn query_supports_before_sequence_and_tail_window() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-window-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");

        for seq in 1..=5 {
            append_stateful_run_event(&path, &event(seq, "run-a", tenant_a.clone()))
                .await
                .expect("append");
        }

        let rows = query_stateful_run_events(
            &path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: Some(5),
                limit: Some(2),
                tail: true,
            },
        );

        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![3, 4]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn event_sequence_helpers_read_latest_and_existing_event_id() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-seq-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        append_stateful_run_event(&path, &event(3, "run-a", tenant_a.clone()))
            .await
            .expect("append first");
        append_stateful_run_event(&path, &event(7, "run-a", tenant_a.clone()))
            .await
            .expect("append second");

        assert_eq!(next_stateful_run_event_seq(&path, &tenant_a, "run-a"), 8);
        assert_eq!(
            stateful_run_event_seq_by_id(&path, &tenant_a, "run-a", "event-3"),
            Some(3)
        );
        assert_eq!(
            stateful_run_event_seq_by_id(&path, &tenant_a, "run-a", "missing"),
            None
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn append_repairs_torn_event_log_tail_before_writing() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-torn-tail-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let first = event(1, "run-a", tenant_a.clone());
        let first_line = serde_json::to_string(&first).expect("serialize first event");
        std::fs::write(&path, format!("{first_line}\n{{\"partial\":")).expect("write torn log");

        append_stateful_run_event(&path, &event(2, "run-a", tenant_a))
            .await
            .expect("append after repair");

        let content = std::fs::read_to_string(&path).expect("read repaired log");
        assert!(content.ends_with('\n'));
        assert!(!content.contains("partial"));
        let rows = load_stateful_run_events(&path);
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn append_preserves_complete_tail_event_without_newline() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-complete-tail-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let first_line =
            serde_json::to_string(&event(1, "run-a", tenant_a.clone())).expect("serialize first");
        let second_line =
            serde_json::to_string(&event(2, "run-a", tenant_a.clone())).expect("serialize second");
        std::fs::write(&path, format!("{first_line}\n{second_line}"))
            .expect("write missing newline log");

        append_stateful_run_event(&path, &event(3, "run-a", tenant_a))
            .await
            .expect("append after newline repair");

        let content = std::fs::read_to_string(&path).expect("read repaired log");
        assert!(content.ends_with('\n'));
        assert!(content.contains("\"event_id\":\"event-2\""));
        let rows = load_stateful_run_events(&path);
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn append_once_uses_event_id_as_idempotency_key() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-once-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant = tenant("org-a", "workspace-a");
        let record = event(1, "run-a", tenant);

        assert!(append_stateful_run_event_once(&path, &record)
            .await
            .expect("first append"));
        assert!(!append_stateful_run_event_once(&path, &record)
            .await
            .expect("duplicate append"));

        let rows = load_stateful_run_events(&path);
        assert_eq!(rows.len(), 1);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn append_once_serializes_concurrent_duplicate_writes() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-once-concurrent-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant = tenant("org-a", "workspace-a");
        let record = event(1, "run-a", tenant);

        let (first, second) = tokio::join!(
            append_stateful_run_event_once(&path, &record),
            append_stateful_run_event_once(&path, &record)
        );
        let appended = [first.expect("first append"), second.expect("second append")]
            .into_iter()
            .filter(|value| *value)
            .count();

        assert_eq!(appended, 1);
        let rows = load_stateful_run_events(&path);
        assert_eq!(rows.len(), 1);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn append_once_with_next_seq_assigns_monotonic_sequence_under_lock() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-next-seq-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant = tenant("org-a", "workspace-a");
        let first = event(0, "run-a", tenant.clone());
        let mut second = event(0, "run-a", tenant.clone());
        second.event_id = "event-second".to_string();

        let (first_appended, first_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant, &first)
                .await
                .expect("first append");
        let (second_appended, second_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant, &second)
                .await
                .expect("second append");
        let (duplicate_appended, duplicate_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant, &first)
                .await
                .expect("duplicate append");

        assert!(first_appended);
        assert!(second_appended);
        assert!(!duplicate_appended);
        assert_eq!(first_seq, 1);
        assert_eq!(second_seq, 2);
        assert_eq!(duplicate_seq, 1);
        let rows = load_stateful_run_events(&path);
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn snapshot_paths_are_scoped_under_sanitized_run_directory() {
        let root =
            std::env::temp_dir().join(format!("stateful-runtime-snapshots-{}", Uuid::new_v4()));
        let status = StatefulWorkflowRunStatus::Running;
        let phase_state = phase_state_from_status("run/../a", &status, 700, Some("phase-a"));
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot/../a".to_string(),
            run_id: "run/../a".to_string(),
            seq: 7,
            created_at_ms: 700,
            scope: StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
            status,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-a".to_string()),
            source_record_kind: None,
            checkpoint: Some(json!({ "phase": "phase-a" })),
            payload_digest: Some("sha256:test".to_string()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
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
    async fn read_snapshot_derives_phase_fields_for_legacy_v1_rows() {
        let path =
            std::env::temp_dir().join(format!("stateful-runtime-legacy-{}.json", Uuid::new_v4()));
        let legacy = json!({
            "schema_version": 1,
            "snapshot_id": "snapshot-1",
            "run_id": "run-1",
            "seq": 42,
            "created_at_ms": 4200,
            "scope": StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
            "status": "running",
            "phase_id": "node-a",
            "checkpoint": { "step": "legacy" }
        });
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&legacy).expect("serialize legacy"),
        )
        .expect("write legacy snapshot");

        let loaded = read_stateful_run_snapshot(&path).expect("read legacy snapshot");

        assert_eq!(loaded.phase, StatefulWorkflowPhase::RunningPhase);
        assert_eq!(
            loaded.allowed_next_phases,
            StatefulWorkflowPhase::RunningPhase
                .allowed_next_phases()
                .to_vec()
        );
        assert_eq!(loaded.phase_history.len(), 1);
        assert_eq!(
            loaded.phase_history[0].reason.as_deref(),
            Some("observed_status:running")
        );
        assert_eq!(loaded.phase_history[0].phase_id.as_deref(), Some("node-a"));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn snapshot_paths_rewrite_dot_only_and_empty_segments() {
        let root =
            std::env::temp_dir().join(format!("stateful-runtime-snapshots-dot-{}", Uuid::new_v4()));
        for (run_id, snapshot_id) in [("..", "."), ("", "")] {
            let status = StatefulWorkflowRunStatus::Running;
            let phase_state = phase_state_from_status(run_id, &status, 100, None);
            let snapshot = StatefulRunSnapshotRecord {
                schema_version: 1,
                snapshot_id: snapshot_id.to_string(),
                run_id: run_id.to_string(),
                seq: 1,
                created_at_ms: 100,
                scope: StatefulRuntimeScope::from_tenant_context(tenant("org-a", "workspace-a")),
                status,
                phase: phase_state.phase,
                phase_history: phase_state.phase_history,
                allowed_next_phases: phase_state.allowed_next_phases,
                phase_id: None,
                source_record_kind: None,
                checkpoint: None,
                payload_digest: None,
                workflow_definition_version: None,
                workflow_definition_snapshot_hash: None,
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

    #[tokio::test]
    async fn snapshot_listing_and_fetch_are_tenant_filtered() {
        let root = std::env::temp_dir().join(format!(
            "stateful-runtime-snapshots-filtered-{}",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        for (seq, tenant_context) in [
            (1, tenant_a.clone()),
            (2, tenant_b.clone()),
            (3, tenant_a.clone()),
        ] {
            let status = StatefulWorkflowRunStatus::Running;
            let phase_state = phase_state_from_status("run-a", &status, seq * 100, None);
            let snapshot = StatefulRunSnapshotRecord {
                schema_version: 1,
                snapshot_id: format!("snapshot-{seq}"),
                run_id: "run-a".to_string(),
                seq,
                created_at_ms: seq * 100,
                scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
                status,
                phase: phase_state.phase,
                phase_history: phase_state.phase_history,
                allowed_next_phases: phase_state.allowed_next_phases,
                phase_id: None,
                source_record_kind: None,
                checkpoint: None,
                payload_digest: None,
                workflow_definition_version: None,
                workflow_definition_snapshot_hash: None,
                metadata: None,
            };
            write_stateful_run_snapshot(&root, &snapshot)
                .await
                .expect("write snapshot");
        }

        let snapshots = list_stateful_run_snapshots(&root, &tenant_a, "run-a", None);
        assert_eq!(
            snapshots
                .iter()
                .map(|snapshot| snapshot.seq)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        let visible = read_stateful_run_snapshot_for_run(&root, &tenant_a, "run-a", "snapshot-3")
            .expect("read visible snapshot");
        assert_eq!(visible.map(|snapshot| snapshot.seq), Some(3));
        let hidden = read_stateful_run_snapshot_for_run(&root, &tenant_a, "run-a", "snapshot-2")
            .expect("read hidden snapshot");
        assert!(hidden.is_none());
        let latest = list_stateful_run_snapshots(&root, &tenant_a, "run-a", Some(1));
        assert_eq!(
            latest
                .iter()
                .map(|snapshot| snapshot.seq)
                .collect::<Vec<_>>(),
            vec![3]
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
