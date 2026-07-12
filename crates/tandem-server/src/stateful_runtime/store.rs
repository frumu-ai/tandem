use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::Context;
use serde_json::Value;
use tandem_types::TenantContext;
use tokio::io::AsyncWriteExt;

use super::compatibility::should_write_stateful_runtime_sidecar;
use super::durable_io::{repair_jsonl_torn_tail, sync_parent_dir, write_file_atomically};
use super::phases::phase_state_from_status;
use super::types::{StatefulRunEventRecord, StatefulRunSnapshotRecord};

const STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE: &str = "stateful_runtime.event_log_compacted";
const STATEFUL_RUNTIME_EVENT_LOG_FILE_NAME: &str = "stateful_events.jsonl";
const STATEFUL_RUNTIME_SNAPSHOTS_DIRECTORY_NAME: &str = "stateful_snapshots";

static STATEFUL_RUN_EVENT_APPEND_CURSORS: LazyLock<
    tokio::sync::Mutex<HashMap<StatefulRunEventCursorKey, StatefulRunEventAppendCursor>>,
> = LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StatefulRunEventCursorKey {
    path: PathBuf,
    org_id: String,
    workspace_id: String,
    deployment_id: Option<String>,
    run_id: String,
}

impl StatefulRunEventCursorKey {
    fn new(path: &Path, tenant: &TenantContext, run_id: &str) -> Self {
        Self {
            path: path.to_path_buf(),
            org_id: tenant.org_id.clone(),
            workspace_id: tenant.workspace_id.clone(),
            deployment_id: tenant.deployment_id.clone(),
            run_id: run_id.to_string(),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct StatefulRunEventAppendCursor {
    last_seq: u64,
    event_seq_by_id: HashMap<String, u64>,
}

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

fn authoritative_stateful_store_for_event_path(
    path: &Path,
) -> Option<super::OrchestrationStateStore> {
    if path.file_name()?.to_str()? != STATEFUL_RUNTIME_EVENT_LOG_FILE_NAME {
        return None;
    }
    let paths = super::OrchestrationStorePaths::from_runtime_events_path(path);
    if !super::backend::store_initialized_hint(&paths.database_path).ok()? {
        return None;
    }
    let store = super::OrchestrationStateStore::open(paths).ok()?;
    store
        .legacy_runtime_migration_complete()
        .ok()
        .filter(|complete| *complete)
        .map(|_| store)
}

fn authoritative_stateful_store_for_snapshot_root(
    root: &Path,
) -> Option<super::OrchestrationStateStore> {
    if root.file_name()?.to_str()? != STATEFUL_RUNTIME_SNAPSHOTS_DIRECTORY_NAME {
        return None;
    }
    let runtime_root = root.parent()?;
    authoritative_stateful_store_for_event_path(
        &runtime_root.join(STATEFUL_RUNTIME_EVENT_LOG_FILE_NAME),
    )
}

pub async fn append_stateful_run_event(
    path: &Path,
    record: &StatefulRunEventRecord,
) -> anyhow::Result<()> {
    let mut cursors = STATEFUL_RUN_EVENT_APPEND_CURSORS.lock().await;
    if let Some(store) = authoritative_stateful_store_for_event_path(path) {
        let event = record.clone();
        let inserted =
            tokio::task::spawn_blocking(move || store.append_stateful_runtime_event(&event))
                .await
                .map_err(|error| anyhow::anyhow!("stateful event store task failed: {error}"))??;
        if !inserted {
            return Ok(());
        }
        if !should_write_stateful_runtime_sidecar(true) {
            return Ok(());
        }
    }
    append_stateful_run_event_unlocked(path, record).await?;
    invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
    Ok(())
}

async fn append_stateful_run_event_unlocked(
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
    let mut cursors = STATEFUL_RUN_EVENT_APPEND_CURSORS.lock().await;
    if let Some(store) = authoritative_stateful_store_for_event_path(path) {
        let event = record.clone();
        let inserted =
            tokio::task::spawn_blocking(move || store.append_stateful_runtime_event_once(&event))
                .await
                .map_err(|error| anyhow::anyhow!("stateful event store task failed: {error}"))??;
        if inserted {
            if !should_write_stateful_runtime_sidecar(true) {
                return Ok(inserted);
            }
            append_stateful_run_event_unlocked(path, record).await?;
            invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
        }
        return Ok(inserted);
    }
    if stateful_run_event_exists(path, record) {
        return Ok(false);
    }
    append_stateful_run_event_unlocked(path, record).await?;
    invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
    Ok(true)
}

pub async fn append_stateful_run_event_once_with_next_seq(
    path: &Path,
    tenant_context: &TenantContext,
    record: &StatefulRunEventRecord,
) -> anyhow::Result<(bool, u64)> {
    let mut cursors = STATEFUL_RUN_EVENT_APPEND_CURSORS.lock().await;
    if let Some(store) = authoritative_stateful_store_for_event_path(path) {
        let event = record.clone();
        let (inserted, seq) = tokio::task::spawn_blocking(move || {
            store.append_stateful_runtime_event_once_with_next_seq(&event)
        })
        .await
        .map_err(|error| anyhow::anyhow!("stateful event store task failed: {error}"))??;
        if inserted {
            if !should_write_stateful_runtime_sidecar(true) {
                return Ok((inserted, seq));
            }
            let mut compatibility_record = record.clone();
            compatibility_record.seq = seq;
            append_stateful_run_event_unlocked(path, &compatibility_record).await?;
            invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
        }
        return Ok((inserted, seq));
    }
    let key = StatefulRunEventCursorKey::new(path, tenant_context, &record.run_id);
    let cursor = cursors
        .entry(key.clone())
        .or_insert_with(|| seed_stateful_run_event_cursor(path, &key));
    if let Some(seq) = cursor.event_seq_by_id.get(&record.event_id).copied() {
        return Ok((false, seq));
    }

    let seq = cursor.last_seq.saturating_add(1).max(1);
    let mut record = record.clone();
    record.seq = seq;
    append_stateful_run_event_unlocked(path, &record).await?;
    cursor.last_seq = seq;
    cursor.event_seq_by_id.insert(record.event_id.clone(), seq);
    Ok((true, seq))
}

fn stateful_run_event_exists(path: &Path, record: &StatefulRunEventRecord) -> bool {
    load_stateful_run_events(path).into_iter().any(|existing| {
        existing.run_id == record.run_id
            && (existing.event_id == record.event_id
                || stateful_run_event_compacted_event_ids(&existing)
                    .iter()
                    .any(|(event_id, _)| event_id == &record.event_id))
    })
}

pub fn stateful_run_event_compacted_event_ids(row: &StatefulRunEventRecord) -> Vec<(String, u64)> {
    if row.event_type != STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE {
        return Vec::new();
    }
    row.payload
        .get("compacted_event_ids")
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .filter_map(|event| {
                    let event_id = event.get("event_id")?.as_str()?.trim();
                    if event_id.is_empty() {
                        return None;
                    }
                    let seq = event.get("seq")?.as_u64()?;
                    Some((event_id.to_string(), seq))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn seed_stateful_run_event_cursor(
    path: &Path,
    key: &StatefulRunEventCursorKey,
) -> StatefulRunEventAppendCursor {
    let mut cursor = StatefulRunEventAppendCursor::default();
    for row in load_stateful_run_events(path)
        .into_iter()
        .filter(|row| stateful_run_event_matches_cursor_key(row, key))
    {
        cursor.last_seq = cursor.last_seq.max(row.seq);
        cursor
            .event_seq_by_id
            .entry(row.event_id.clone())
            .or_insert(row.seq);
        for (event_id, seq) in stateful_run_event_compacted_event_ids(&row) {
            cursor.last_seq = cursor.last_seq.max(seq);
            cursor.event_seq_by_id.entry(event_id).or_insert(seq);
        }
    }
    cursor
}

fn stateful_run_event_matches_cursor_key(
    row: &StatefulRunEventRecord,
    key: &StatefulRunEventCursorKey,
) -> bool {
    row.run_id == key.run_id
        && row.scope.tenant_context.org_id == key.org_id
        && row.scope.tenant_context.workspace_id == key.workspace_id
        && row.scope.tenant_context.deployment_id == key.deployment_id
}

fn invalidate_stateful_run_event_cursors_for_path(
    cursors: &mut HashMap<StatefulRunEventCursorKey, StatefulRunEventAppendCursor>,
    path: &Path,
) {
    cursors.retain(|key, _| key.path != path);
}

pub fn load_stateful_run_events(path: &Path) -> Vec<StatefulRunEventRecord> {
    if let Some(store) = authoritative_stateful_store_for_event_path(path) {
        return store
            .load_stateful_runtime_events()
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "failed to load authoritative stateful events");
                Vec::new()
            });
    }
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

pub async fn compact_stateful_run_event_log(
    path: &Path,
    retention_ms: u64,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 {
        return Ok(0);
    }
    let authoritative_store = authoritative_stateful_store_for_event_path(path);
    if authoritative_store.is_none() && !path.exists() {
        return Ok(0);
    }

    let mut cursors = STATEFUL_RUN_EVENT_APPEND_CURSORS.lock().await;
    if authoritative_store.is_none() {
        repair_jsonl_torn_tail(path, "stateful run event log").await?;
    }

    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    let snapshot_floors = stateful_snapshot_floors(path, authoritative_store.as_ref()).await?;
    let mut retained = Vec::new();
    let mut compacted =
        HashMap::<StatefulRunEventCompactionKey, StatefulRunEventCompactionSummary>::new();
    let mut pruned = 0_usize;

    let observed = load_stateful_run_events(path);
    let observed_event_ids = observed
        .iter()
        .map(|row| row.event_id.clone())
        .collect::<Vec<_>>();
    for row in observed {
        // Events at or after a run's newest snapshot are the replay tail for
        // that snapshot; age-based retention must never remove them.
        let protected_by_snapshot = snapshot_floors
            .get(&row.run_id)
            .is_some_and(|floor| row.seq >= *floor);
        if row.occurred_at_ms >= cutoff_ms || protected_by_snapshot {
            retained.push(row);
            continue;
        }
        pruned += 1;
        let key = StatefulRunEventCompactionKey::from_event(&row);
        compacted
            .entry(key)
            .and_modify(|summary| summary.observe(&row))
            .or_insert_with(|| StatefulRunEventCompactionSummary::from_event(&row));
    }

    if pruned == 0 {
        return Ok(0);
    }

    for row in &retained {
        if row.event_type == STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE {
            let key = StatefulRunEventCompactionKey::from_event(row);
            if let Some(summary) = compacted.get_mut(&key) {
                summary.observe(row);
            }
        }
    }
    retained.retain(|row| {
        row.event_type != STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE
            || !compacted.contains_key(&StatefulRunEventCompactionKey::from_event(row))
    });
    retained.extend(
        compacted
            .values()
            .map(|summary| summary.compaction_marker(now_ms)),
    );
    retained.sort_by(|left, right| {
        left.run_id
            .cmp(&right.run_id)
            .then_with(|| left.seq.cmp(&right.seq))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    if let Some(store) = authoritative_store {
        tokio::task::spawn_blocking(move || {
            store.replace_observed_stateful_runtime_events(&observed_event_ids, &retained)
        })
        .await
        .map_err(|error| anyhow::anyhow!("stateful event compaction task failed: {error}"))??;
        invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
        return Ok(pruned);
    }
    write_stateful_run_event_rows(path, &retained).await?;
    invalidate_stateful_run_event_cursors_for_path(&mut cursors, path);
    Ok(pruned)
}

/// Prunes stateful run snapshots older than `retention_ms`, always keeping
/// the newest `keep_last_per_run` snapshots of every run so replay never
/// loses its most recent restore point. Prunes the authoritative SQLite rows
/// when the migration completed, and removes pruned JSON mirror files
/// best-effort in both modes. Returns the number of snapshots pruned.
pub async fn prune_stateful_run_snapshots(
    snapshots_root: &Path,
    retention_ms: u64,
    keep_last_per_run: usize,
    now_ms: u64,
) -> anyhow::Result<usize> {
    if retention_ms == 0 {
        return Ok(0);
    }
    let cutoff_ms = now_ms.saturating_sub(retention_ms);
    let keep = keep_last_per_run.max(1);
    if let Some(store) = authoritative_stateful_store_for_snapshot_root(snapshots_root) {
        let pruned = tokio::task::spawn_blocking(move || {
            store.prune_stateful_runtime_snapshots(cutoff_ms, keep)
        })
        .await
        .map_err(|error| anyhow::anyhow!("stateful snapshot prune task failed: {error}"))??;
        let pruned_ids = pruned.iter().cloned().collect::<HashSet<_>>();
        remove_snapshot_mirror_files(snapshots_root, |snapshot| {
            pruned_ids.contains(&snapshot.snapshot_id)
        });
        return Ok(pruned.len());
    }
    // Legacy JSON-only mode: compute the keep set per run, prune the rest.
    let mut by_run = HashMap::<String, Vec<StatefulRunSnapshotRecord>>::new();
    collect_snapshot_mirror_records(snapshots_root, |snapshot| {
        by_run
            .entry(snapshot.run_id.clone())
            .or_default()
            .push(snapshot);
    });
    let mut pruned_ids = HashSet::new();
    for snapshots in by_run.values_mut() {
        snapshots.sort_by(|left, right| {
            right
                .seq
                .cmp(&left.seq)
                .then_with(|| right.snapshot_id.cmp(&left.snapshot_id))
        });
        for snapshot in snapshots.iter().skip(keep) {
            if snapshot.created_at_ms < cutoff_ms {
                pruned_ids.insert(snapshot.snapshot_id.clone());
            }
        }
    }
    if pruned_ids.is_empty() {
        return Ok(0);
    }
    remove_snapshot_mirror_files(snapshots_root, |snapshot| {
        pruned_ids.contains(&snapshot.snapshot_id)
    });
    Ok(pruned_ids.len())
}

fn collect_snapshot_mirror_records(
    snapshots_root: &Path,
    mut visit: impl FnMut(StatefulRunSnapshotRecord),
) {
    let Ok(run_directories) = std::fs::read_dir(snapshots_root) else {
        return;
    };
    for run_directory in run_directories.filter_map(Result::ok) {
        let Ok(entries) = std::fs::read_dir(run_directory.path()) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            if let Ok(snapshot) = read_stateful_run_snapshot(&path) {
                visit(snapshot);
            }
        }
    }
}

fn remove_snapshot_mirror_files(
    snapshots_root: &Path,
    mut should_remove: impl FnMut(&StatefulRunSnapshotRecord) -> bool,
) {
    let Ok(run_directories) = std::fs::read_dir(snapshots_root) else {
        return;
    };
    for run_directory in run_directories.filter_map(Result::ok) {
        let Ok(entries) = std::fs::read_dir(run_directory.path()) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(snapshot) = read_stateful_run_snapshot(&path) else {
                continue;
            };
            if should_remove(&snapshot) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Per-run seq of the newest snapshot. Events at or above this floor are the
/// tail needed to replay from that snapshot and must survive retention.
async fn stateful_snapshot_floors(
    run_events_path: &Path,
    authoritative_store: Option<&super::OrchestrationStateStore>,
) -> anyhow::Result<HashMap<String, u64>> {
    if let Some(store) = authoritative_store {
        let store = store.clone();
        return tokio::task::spawn_blocking(move || store.latest_stateful_snapshot_seqs())
            .await
            .map_err(|error| anyhow::anyhow!("stateful snapshot floor task failed: {error}"))?;
    }
    let root = run_events_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(STATEFUL_RUNTIME_SNAPSHOTS_DIRECTORY_NAME);
    let mut floors = HashMap::new();
    let Ok(run_directories) = std::fs::read_dir(&root) else {
        return Ok(floors);
    };
    for run_directory in run_directories.filter_map(Result::ok) {
        let Ok(entries) = std::fs::read_dir(run_directory.path()) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(snapshot) = read_stateful_run_snapshot(&path) else {
                continue;
            };
            let floor = floors.entry(snapshot.run_id.clone()).or_insert(0);
            *floor = (*floor).max(snapshot.seq);
        }
    }
    Ok(floors)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StatefulRunEventCompactionKey {
    run_id: String,
    org_id: String,
    workspace_id: String,
    deployment_id: Option<String>,
}

impl StatefulRunEventCompactionKey {
    fn from_event(row: &StatefulRunEventRecord) -> Self {
        Self {
            run_id: row.run_id.clone(),
            org_id: row.scope.tenant_context.org_id.clone(),
            workspace_id: row.scope.tenant_context.workspace_id.clone(),
            deployment_id: row.scope.tenant_context.deployment_id.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct StatefulRunEventCompactionSummary {
    key: StatefulRunEventCompactionKey,
    compacted_through_seq: u64,
    compacted_through_ms: u64,
    pruned_events: usize,
    pruned_event_ids: Vec<(String, u64)>,
    scope: super::types::StatefulRuntimeScope,
}

impl StatefulRunEventCompactionSummary {
    fn from_event(row: &StatefulRunEventRecord) -> Self {
        let mut pruned_event_ids = stateful_run_event_compacted_event_ids(row);
        pruned_event_ids.push((row.event_id.clone(), row.seq));
        Self {
            key: StatefulRunEventCompactionKey::from_event(row),
            compacted_through_seq: row.seq,
            compacted_through_ms: row.occurred_at_ms,
            pruned_events: 1,
            pruned_event_ids,
            scope: row.scope.clone(),
        }
    }

    fn observe(&mut self, row: &StatefulRunEventRecord) {
        self.compacted_through_seq = self.compacted_through_seq.max(row.seq);
        self.compacted_through_ms = self.compacted_through_ms.max(row.occurred_at_ms);
        self.pruned_events += 1;
        self.pruned_event_ids
            .extend(stateful_run_event_compacted_event_ids(row));
        self.pruned_event_ids.push((row.event_id.clone(), row.seq));
    }

    fn compaction_marker(&self, now_ms: u64) -> StatefulRunEventRecord {
        let compacted_through_seq = self.compacted_through_seq.to_string();
        let digest = crate::sha256_hex(&[
            &self.key.org_id,
            &self.key.workspace_id,
            self.key.deployment_id.as_deref().unwrap_or(""),
            &self.key.run_id,
            compacted_through_seq.as_str(),
        ]);
        let compacted_event_ids = self
            .pruned_event_ids
            .iter()
            .map(|(event_id, seq)| serde_json::json!({ "event_id": event_id, "seq": seq }))
            .collect::<Vec<_>>();
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("stateful-compaction-{digest}"),
            run_id: self.key.run_id.clone(),
            seq: self.compacted_through_seq,
            event_type: STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE.to_string(),
            occurred_at_ms: now_ms,
            scope: self.scope.clone(),
            actor: None,
            phase_id: None,
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: serde_json::json!({
                "compacted_through_seq": self.compacted_through_seq,
                "compacted_through_ms": self.compacted_through_ms,
                "pruned_events": self.pruned_events,
                "compacted_event_ids": compacted_event_ids,
                "source": "stateful_event_log_compaction"
            }),
        }
    }
}

async fn write_stateful_run_event_rows(
    path: &Path,
    rows: &[StatefulRunEventRecord],
) -> anyhow::Result<()> {
    let mut content = Vec::new();
    for row in rows {
        content.extend_from_slice(&serde_json::to_vec(row)?);
        content.push(b'\n');
    }
    write_file_atomically(path, &content, "stateful run event log").await
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
    let path = stateful_run_snapshot_path(root, &snapshot.run_id, &snapshot.snapshot_id);
    if let Some(store) = authoritative_stateful_store_for_snapshot_root(root) {
        let snapshot = snapshot.clone();
        tokio::task::spawn_blocking(move || store.put_stateful_runtime_snapshot(&snapshot))
            .await
            .map_err(|error| anyhow::anyhow!("stateful snapshot store task failed: {error}"))??;
        if !should_write_stateful_runtime_sidecar(true) {
            return Ok(path);
        }
    }
    let dir = path
        .parent()
        .expect("stateful snapshot path always has a parent");
    tokio::fs::create_dir_all(&dir).await.with_context(|| {
        format!(
            "failed to create stateful snapshot directory {}",
            dir.display()
        )
    })?;
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
    if let Some(store) = authoritative_stateful_store_for_snapshot_root(root) {
        let mut snapshots = store
            .list_stateful_runtime_snapshots(run_id)
            .unwrap_or_else(|error| {
                tracing::error!(error = %error, "failed to load authoritative stateful snapshots");
                Vec::new()
            })
            .into_iter()
            .filter(|snapshot| snapshot.visible_to_tenant(tenant))
            .collect::<Vec<_>>();
        if let Some(limit) = limit.filter(|limit| *limit > 0) {
            if snapshots.len() > limit {
                let remove_count = snapshots.len() - limit;
                snapshots.drain(0..remove_count);
            }
        }
        return snapshots;
    }
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
    if let Some(store) = authoritative_stateful_store_for_snapshot_root(root) {
        let snapshot = store.get_stateful_runtime_snapshot(snapshot_id)?;
        return Ok(snapshot
            .filter(|snapshot| snapshot.run_id == run_id && snapshot.visible_to_tenant(tenant)));
    }
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
    async fn append_once_with_next_seq_uses_cursor_and_preserves_tenant_boundaries() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-next-seq-cursor-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let mut first = event(0, "run-a", tenant_a.clone());
        first.event_id = "event-a".to_string();
        let mut second = event(0, "run-a", tenant_a.clone());
        second.event_id = "event-b".to_string();
        let mut foreign = event(0, "run-a", tenant_b.clone());
        foreign.event_id = "event-foreign".to_string();

        let (_, first_seq) = append_stateful_run_event_once_with_next_seq(&path, &tenant_a, &first)
            .await
            .expect("append first");
        let (_, second_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant_a, &second)
                .await
                .expect("append second");
        let (_, foreign_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant_b, &foreign)
                .await
                .expect("append foreign");

        assert_eq!((first_seq, second_seq, foreign_seq), (1, 2, 1));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn compaction_prunes_old_events_and_preserves_next_sequence_marker() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-compact-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        for seq in 1..=3 {
            let mut record = event(seq, "run-a", tenant_a.clone());
            record.occurred_at_ms = seq * 100;
            append_stateful_run_event(&path, &record)
                .await
                .expect("append old event");
        }
        let mut retained = event(4, "run-a", tenant_a.clone());
        retained.occurred_at_ms = 900;
        append_stateful_run_event(&path, &retained)
            .await
            .expect("append retained event");

        let pruned = compact_stateful_run_event_log(&path, 500, 1_000)
            .await
            .expect("compact");

        assert_eq!(pruned, 3);
        let rows = query_stateful_run_events(
            &path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert_eq!(rows[0].event_type, STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE);
        let compacted_ids = stateful_run_event_compacted_event_ids(&rows[0]);
        assert_eq!(
            compacted_ids,
            vec![
                ("event-1".to_string(), 1),
                ("event-2".to_string(), 2),
                ("event-3".to_string(), 3),
            ]
        );

        let mut duplicate = event(0, "run-a", tenant_a.clone());
        duplicate.event_id = "event-2".to_string();
        let (duplicate_appended, duplicate_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant_a, &duplicate)
                .await
                .expect("append duplicate");
        assert!(!duplicate_appended);
        assert_eq!(duplicate_seq, 2);

        let mut next = event(0, "run-a", tenant_a.clone());
        next.event_id = "event-next".to_string();
        let (_, next_seq) = append_stateful_run_event_once_with_next_seq(&path, &tenant_a, &next)
            .await
            .expect("append next");
        assert_eq!(next_seq, 5);

        let second_pruned = compact_stateful_run_event_log(&path, 500, 2_000)
            .await
            .expect("compact again");
        assert_eq!(second_pruned, 3);
        let rows = query_stateful_run_events(
            &path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(rows.len(), 1);
        let compacted_ids = stateful_run_event_compacted_event_ids(&rows[0]);
        assert!(compacted_ids
            .iter()
            .any(|(event_id, seq)| event_id == "event-1" && *seq == 1));
        assert!(compacted_ids
            .iter()
            .any(|(event_id, seq)| event_id == "event-next" && *seq == 5));

        let mut old_duplicate = event(0, "run-a", tenant_a.clone());
        old_duplicate.event_id = "event-1".to_string();
        let (old_duplicate_appended, old_duplicate_seq) =
            append_stateful_run_event_once_with_next_seq(&path, &tenant_a, &old_duplicate)
                .await
                .expect("append old duplicate");
        assert!(!old_duplicate_appended);
        assert_eq!(old_duplicate_seq, 1);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn compaction_keeps_tenant_boundaries_independent() {
        let path = std::env::temp_dir().join(format!(
            "stateful-runtime-events-compact-tenant-{}.jsonl",
            Uuid::new_v4()
        ));
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        for (seq, tenant_context) in [(1, tenant_a.clone()), (2, tenant_b.clone())] {
            let mut record = event(seq, "shared-run", tenant_context);
            record.occurred_at_ms = 100;
            append_stateful_run_event(&path, &record)
                .await
                .expect("append old event");
        }

        let pruned = compact_stateful_run_event_log(&path, 500, 1_000)
            .await
            .expect("compact");

        assert_eq!(pruned, 2);
        assert_eq!(
            next_stateful_run_event_seq(&path, &tenant_a, "shared-run"),
            2
        );
        assert_eq!(
            next_stateful_run_event_seq(&path, &tenant_b, "shared-run"),
            3
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn compaction_never_prunes_the_replay_tail_of_the_latest_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "stateful-runtime-compact-snapshot-floor-{}",
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("stateful_events.jsonl");
        let tenant_a = tenant("org-a", "workspace-a");
        for seq in 1..=3 {
            let mut record = event(seq, "run-a", tenant_a.clone());
            record.occurred_at_ms = seq * 100;
            append_stateful_run_event(&path, &record)
                .await
                .expect("append old event");
        }
        let mut recent = event(4, "run-a", tenant_a.clone());
        recent.occurred_at_ms = 900;
        append_stateful_run_event(&path, &recent)
            .await
            .expect("append recent event");
        // The newest snapshot pins seq 2: events 2 and 3 are its replay tail
        // and must survive even though their timestamps are past retention.
        let status = StatefulWorkflowRunStatus::Running;
        let phase_state = phase_state_from_status("run-a", &status, 250, None);
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-run-a".to_string(),
            run_id: "run-a".to_string(),
            seq: 2,
            created_at_ms: 250,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
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
        write_stateful_run_snapshot(&root.join("stateful_snapshots"), &snapshot)
            .await
            .expect("write snapshot");

        let pruned = compact_stateful_run_event_log(&path, 500, 1_000)
            .await
            .expect("compact");

        assert_eq!(pruned, 1, "only the pre-snapshot event may be pruned");
        let rows = query_stateful_run_events(
            &path,
            &tenant_a,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(rows[0].event_type, STATEFUL_RUN_EVENT_LOG_COMPACTED_TYPE);
        assert_eq!(rows[1].event_id, "event-2");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn snapshot_prune_keeps_newest_mirror_files_per_run() {
        let root = std::env::temp_dir().join(format!(
            "stateful-runtime-snapshot-prune-{}",
            Uuid::new_v4()
        ));
        let snapshots_root = root.join("stateful_snapshots");
        let tenant_a = tenant("org-a", "workspace-a");
        for (snapshot_id, seq, created_at_ms) in [
            ("snapshot-old", 1_u64, 10_u64),
            ("snapshot-mid", 2, 20),
            ("snapshot-new", 3, 900),
        ] {
            let status = StatefulWorkflowRunStatus::Running;
            let phase_state = phase_state_from_status("run-a", &status, created_at_ms, None);
            let snapshot = StatefulRunSnapshotRecord {
                schema_version: 1,
                snapshot_id: snapshot_id.to_string(),
                run_id: "run-a".to_string(),
                seq,
                created_at_ms,
                scope: StatefulRuntimeScope::from_tenant_context(tenant_a.clone()),
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
            write_stateful_run_snapshot(&snapshots_root, &snapshot)
                .await
                .expect("write snapshot");
        }

        let pruned = prune_stateful_run_snapshots(&snapshots_root, 500, 2, 1_000)
            .await
            .expect("prune snapshots");

        assert_eq!(pruned, 1, "only snapshots beyond keep-last and cutoff go");
        let remaining = list_stateful_run_snapshots(&snapshots_root, &tenant_a, "run-a", None)
            .into_iter()
            .map(|snapshot| snapshot.snapshot_id)
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec!["snapshot-mid", "snapshot-new"]);
        let _ = tokio::fs::remove_dir_all(root).await;
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

    #[tokio::test]
    async fn migrated_runtime_events_and_snapshots_ignore_compatibility_file_changes() {
        let root = std::env::temp_dir().join(format!(
            "stateful-runtime-sqlite-cutover-{}",
            Uuid::new_v4()
        ));
        let runtime_events_path = root.join("runtime").join("events.jsonl");
        let runs_path = root.join("automation_v2_runs.json");
        let store = super::super::OrchestrationStateStore::from_automation_runs_path(&runs_path)
            .expect("open orchestration store");
        let migration_paths = super::super::LegacyRuntimeMigrationPaths::from_runtime_paths(
            runs_path.clone(),
            &runtime_events_path,
        );
        store
            .import_legacy_runtime_state(&migration_paths, 100)
            .expect("complete empty migration");

        assert_eq!(
            store.paths().database_path,
            super::super::OrchestrationStorePaths::from_runtime_events_path(&runtime_events_path)
                .database_path
        );

        let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&runtime_events_path);
        let tenant = tenant("org-a", "workspace-a");
        let event = event(0, "run-a", tenant.clone());
        let (inserted, seq) =
            append_stateful_run_event_once_with_next_seq(&paths.run_events_path, &tenant, &event)
                .await
                .expect("append event through SQLite");
        assert!(inserted);
        assert_eq!(seq, 1);

        let status = StatefulWorkflowRunStatus::Running;
        let phase_state = phase_state_from_status("run-a", &status, 100, Some("validate"));
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "snapshot-a".to_string(),
            run_id: "run-a".to_string(),
            seq,
            created_at_ms: 100,
            scope: StatefulRuntimeScope::from_tenant_context(tenant.clone()),
            status,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("validate".to_string()),
            source_record_kind: None,
            checkpoint: Some(json!({ "node": "validate" })),
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        };
        let snapshot_path = write_stateful_run_snapshot(&paths.snapshots_root, &snapshot)
            .await
            .expect("write snapshot through SQLite");

        std::fs::create_dir_all(paths.run_events_path.parent().expect("runtime root"))
            .expect("create compatibility event parent");
        std::fs::write(&paths.run_events_path, "{not-json}\n")
            .expect("corrupt compatibility event log");
        std::fs::create_dir_all(snapshot_path.parent().expect("snapshot parent"))
            .expect("create compatibility snapshot parent");
        std::fs::write(&snapshot_path, "{not-json}\n").expect("corrupt compatibility snapshot");

        let events = query_stateful_run_events(
            &paths.run_events_path,
            &tenant,
            StatefulRunEventQuery {
                run_id: "run-a",
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event.event_id);
        assert_eq!(
            list_stateful_run_snapshots(&paths.snapshots_root, &tenant, "run-a", None),
            vec![snapshot.clone()]
        );
        assert_eq!(
            read_stateful_run_snapshot_for_run(
                &paths.snapshots_root,
                &tenant,
                "run-a",
                "snapshot-a"
            )
            .expect("read SQLite snapshot"),
            Some(snapshot)
        );

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
