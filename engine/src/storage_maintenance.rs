// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Storage doctor/cleanup maintenance for the `tandem-engine storage`
//! subcommands, extracted from `main.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{Datelike, TimeZone, Utc};
use flate2::{write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_memory::db::MemoryDatabase;
use tandem_server::{AutomationRunStatus, AutomationV2RunRecord, AutomationV2Spec};

use crate::resolve_memory_db_path;

pub(crate) const DEFAULT_KNOWLEDGE_SOURCE_PREFIX: &str = "guide_docs:";
const AUTOMATION_V2_RUNS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct StorageAutomationRunsFile {
    schema_version: u32,
    runs: std::collections::HashMap<String, AutomationV2RunRecord>,
}

#[derive(Debug, Serialize)]
struct StorageAutomationRunsFileRef<'a> {
    schema_version: u32,
    runs: &'a std::collections::HashMap<String, AutomationV2RunRecord>,
}

#[derive(Debug, Serialize)]
struct StorageAutomationRunShardFileRef<'a> {
    schema_version: u32,
    run: &'a AutomationV2RunRecord,
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageDoctorReport {
    root: String,
    data_dir: String,
    total_candidate_bytes: u64,
    files: Vec<StorageFileReport>,
    recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageFileReport {
    path: String,
    bytes: u64,
    kind: String,
    action: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageCleanupReport {
    root: String,
    data_dir: String,
    dry_run: bool,
    quarantine_dir: Option<String>,
    runs_loaded: usize,
    hot_runs_written: usize,
    shards_written: usize,
    root_files_migrated: usize,
    context_runs_scanned: usize,
    context_runs_archived: usize,
    context_runs_stale_closed: usize,
    context_run_bytes_archived: u64,
    default_knowledge_rows_cleared: u64,
    default_knowledge_state_files_matched: usize,
    files_quarantined: Vec<String>,
    candidate_bytes: u64,
}

pub(crate) fn resolve_storage_root(state_dir: &Path) -> PathBuf {
    if state_dir.file_name().and_then(|value| value.to_str()) == Some("data") {
        return state_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| state_dir.to_path_buf());
    }
    if state_dir.join("automations_v2.json").exists()
        || state_dir.join("automation_v2_runs.json").exists()
    {
        return state_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| state_dir.to_path_buf());
    }
    state_dir.to_path_buf()
}

pub(crate) fn storage_doctor_report(state_dir: &Path) -> anyhow::Result<StorageDoctorReport> {
    let root = resolve_storage_root(state_dir);
    let data_dir = root.join("data");
    let mut files = Vec::new();
    push_storage_file_report(
        &mut files,
        &root.join("automation_v2_runs.json"),
        "legacy_root_automation_runs",
        "quarantine after data/automation_v2_runs.json is migrated",
    )?;
    push_storage_file_report(
        &mut files,
        &data_dir.join("automation_v2_runs.json"),
        "hot_automation_run_index",
        "rewrite as compact active/recent index",
    )?;
    push_storage_file_report(
        &mut files,
        &data_dir.join("automation_v2_runs_archive.json"),
        "legacy_monolithic_archive",
        "migrate to data/automation-runs/YYYY/MM/*.json then quarantine",
    )?;
    for path in default_knowledge_state_paths(&root) {
        push_storage_file_report(
            &mut files,
            &path,
            "legacy_default_knowledge_state",
            "remove with `tandem-engine storage cleanup --default-knowledge`",
        )?;
    }
    for path in storage_tmp_files(&data_dir)? {
        push_storage_file_report(&mut files, &path, "orphan_temp_file", "quarantine")?;
    }
    for path in old_large_engine_logs(&root)? {
        push_storage_file_report(&mut files, &path, "old_large_engine_log", "quarantine")?;
    }
    let total_candidate_bytes = files
        .iter()
        .filter(|file| file.kind != "hot_automation_run_index")
        .map(|file| file.bytes)
        .sum();
    let mut recommendations = vec![
        "stop tandem-engine before cleanup so active JSON files are not rewritten concurrently"
            .to_string(),
        "run `tandem-engine storage cleanup --quarantine` to shard automation history and move legacy files aside"
            .to_string(),
    ];
    if files
        .iter()
        .any(|file| file.kind == "legacy_default_knowledge_state")
    {
        recommendations.push(
            "run `tandem-engine storage cleanup --default-knowledge --quarantine` to purge the old embedded docs seed"
                .to_string(),
        );
    }
    if files
        .iter()
        .any(|file| file.kind == "hot_automation_run_index" && file.bytes > 10_000_000)
    {
        recommendations.push(
            "hot automation index is large; cleanup will keep only active/recent summaries"
                .to_string(),
        );
    }
    Ok(StorageDoctorReport {
        root: root.display().to_string(),
        data_dir: data_dir.display().to_string(),
        total_candidate_bytes,
        files,
        recommendations,
    })
}

pub(crate) fn push_storage_file_report(
    files: &mut Vec<StorageFileReport>,
    path: &Path,
    kind: &str,
    action: &str,
) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if !meta.is_file() {
        return Ok(());
    }
    files.push(StorageFileReport {
        path: path.display().to_string(),
        bytes: meta.len(),
        kind: kind.to_string(),
        action: action.to_string(),
    });
    Ok(())
}

pub(crate) async fn storage_cleanup(
    state_dir: &Path,
    quarantine: bool,
    dry_run: bool,
    root_json: bool,
    context_runs: bool,
    default_knowledge: bool,
    retention_days: u64,
) -> anyhow::Result<StorageCleanupReport> {
    let root = resolve_storage_root(state_dir);
    let data_dir = root.join("data");
    let run_root_json = root_json || !context_runs;
    let run_context_runs = context_runs || !root_json;
    let active_path = data_dir.join("automation_v2_runs.json");
    let archive_path = data_dir.join("automation_v2_runs_archive.json");
    let legacy_root_path = root.join("automation_v2_runs.json");
    let automations_path = data_dir.join("automations_v2.json");
    let automations = read_automation_specs_map(&automations_path)?;
    let mut runs = std::collections::HashMap::<String, AutomationV2RunRecord>::new();
    merge_automation_runs(&mut runs, &active_path)?;
    merge_automation_runs(&mut runs, &archive_path)?;
    merge_automation_runs(&mut runs, &legacy_root_path)?;

    let cutoff_ms =
        now_ms_for_storage().saturating_sub(retention_days.saturating_mul(24 * 60 * 60 * 1000));
    let mut hot = std::collections::HashMap::new();
    for (run_id, mut run) in runs.clone() {
        if storage_run_is_terminal(&run.status) && run.updated_at_ms <= cutoff_ms {
            continue;
        }
        compact_storage_hot_run(&mut run, &automations);
        hot.insert(run_id, run);
    }

    let mut files_quarantined = Vec::new();
    let mut candidate_bytes: u64 = 0;
    let quarantine_dir = if quarantine {
        Some(root.join("backups").join(format!(
            "local-cleanup-{}",
            Utc::now().format("%Y%m%d-%H%M%S")
        )))
    } else {
        None
    };

    if !dry_run {
        for run in runs.values() {
            write_storage_run_shard(&active_path, run)?;
        }
        if let Some(parent) = active_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_string_atomic_sync(&active_path, &serialize_storage_automation_runs(&hot)?)?;
    }

    let mut quarantine_candidates = Vec::new();
    if legacy_root_path.exists() && active_path.exists() {
        quarantine_candidates.push(legacy_root_path);
    }
    if archive_path.exists() {
        quarantine_candidates.push(archive_path);
    }
    quarantine_candidates.extend(storage_tmp_files(&data_dir)?);
    quarantine_candidates.extend(old_large_engine_logs(&root)?);

    for path in quarantine_candidates {
        if let Ok(meta) = fs::metadata(&path) {
            candidate_bytes = candidate_bytes.saturating_add(meta.len());
        }
        if quarantine {
            if let Some(dir) = quarantine_dir.as_ref() {
                if !dry_run {
                    quarantine_file(&root, dir, &path)?;
                }
                files_quarantined.push(path.display().to_string());
            }
        }
    }

    let root_files_migrated = if run_root_json {
        migrate_root_feature_storage(&root, quarantine_dir.as_deref(), quarantine, dry_run)?
    } else {
        0
    };
    let context_report = if run_context_runs {
        cleanup_context_runs(&root, retention_days, dry_run)?
    } else {
        ContextRunCleanupReport::default()
    };
    let default_knowledge_report = if default_knowledge {
        let db_path = resolve_memory_db_path(&root);
        cleanup_default_knowledge_storage(
            &root,
            Some(db_path.as_path()),
            quarantine_dir.as_deref(),
            quarantine,
            dry_run,
        )
        .await?
    } else {
        DefaultKnowledgeCleanupReport::default()
    };

    Ok(StorageCleanupReport {
        root: root.display().to_string(),
        data_dir: data_dir.display().to_string(),
        dry_run,
        quarantine_dir: quarantine_dir.map(|path| path.display().to_string()),
        runs_loaded: runs.len(),
        hot_runs_written: hot.len(),
        shards_written: runs.len(),
        root_files_migrated,
        context_runs_scanned: context_report.scanned,
        context_runs_archived: context_report.archived,
        context_runs_stale_closed: context_report.stale_closed,
        context_run_bytes_archived: context_report.bytes_archived,
        default_knowledge_rows_cleared: default_knowledge_report.rows_cleared,
        default_knowledge_state_files_matched: default_knowledge_report.state_files_matched,
        files_quarantined: {
            files_quarantined.extend(default_knowledge_report.files_quarantined);
            files_quarantined
        },
        candidate_bytes,
    })
}

#[derive(Debug, Default)]
pub(crate) struct ContextRunCleanupReport {
    scanned: usize,
    archived: usize,
    stale_closed: usize,
    bytes_archived: u64,
}

#[derive(Debug, Default)]
pub(crate) struct DefaultKnowledgeCleanupReport {
    pub(crate) rows_cleared: u64,
    pub(crate) state_files_matched: usize,
    pub(crate) files_quarantined: Vec<String>,
}

pub(crate) fn default_knowledge_state_paths(root: &Path) -> [PathBuf; 2] {
    [
        root.join("default_knowledge_state.json"),
        root.join("data")
            .join("knowledge")
            .join("default_knowledge_state.json"),
    ]
}

pub(crate) async fn cleanup_default_knowledge_storage(
    root: &Path,
    db_path: Option<&Path>,
    quarantine_dir: Option<&Path>,
    quarantine: bool,
    dry_run: bool,
) -> anyhow::Result<DefaultKnowledgeCleanupReport> {
    let mut report = DefaultKnowledgeCleanupReport::default();

    for path in default_knowledge_state_paths(root) {
        if !path.exists() {
            continue;
        }
        report.state_files_matched = report.state_files_matched.saturating_add(1);
        if quarantine {
            if let Some(dir) = quarantine_dir {
                if !dry_run {
                    quarantine_file(root, dir, &path)?;
                }
                report.files_quarantined.push(path.display().to_string());
            }
        } else if !dry_run {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
    }

    if dry_run {
        return Ok(report);
    }

    let Some(db_path) = db_path else {
        return Ok(report);
    };
    if db_path.exists() {
        let db = MemoryDatabase::new(db_path).await?;
        report.rows_cleared = db
            .clear_global_memory_by_source_prefix(DEFAULT_KNOWLEDGE_SOURCE_PREFIX)
            .await?;
    }

    Ok(report)
}

pub(crate) fn migrate_root_feature_storage(
    root: &Path,
    quarantine_dir: Option<&Path>,
    quarantine: bool,
    dry_run: bool,
) -> anyhow::Result<usize> {
    let mappings = [
        ("shared_resources.json", "data/system/shared_resources.json"),
        ("mcp_servers.json", "data/mcp/mcp_servers.json"),
        ("routines.json", "data/routines/routines.json"),
        ("routines.json.bak", "data/routines/routines.json.bak"),
        ("routine_runs.json", "data/routines/routine_runs.json"),
        (
            "incident_monitor_config.json",
            "data/incident-monitor/config.json",
        ),
        (
            "incident_monitor_drafts.json",
            "data/incident-monitor/drafts.json",
        ),
        (
            "incident_monitor_incidents.json",
            "data/incident-monitor/incidents.json",
        ),
        (
            "incident_monitor_posts.json",
            "data/incident-monitor/posts.json",
        ),
        (
            "external_actions.json",
            "data/actions/external_actions.json",
        ),
        (
            "workflow_planner_sessions.json",
            "data/workflow-planner/sessions.json",
        ),
        ("pack_builder_plans.json", "data/pack-builder/plans.json"),
        (
            "pack_builder_workflows.json",
            "data/pack-builder/workflows.json",
        ),
        ("channel_sessions.json", "data/channels/sessions.json"),
        (
            "channel_tool_preferences.json",
            "data/channels/tool_preferences.json",
        ),
    ];
    let mut migrated = 0usize;
    for (legacy, canonical) in mappings {
        let source = root.join(legacy);
        if !source.exists() {
            continue;
        }
        let target = root.join(canonical);
        if !dry_run && !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &target)
                .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
        }
        migrated += 1;
        if quarantine {
            if let Some(dir) = quarantine_dir {
                if !dry_run && source.exists() {
                    quarantine_file(root, dir, &source)?;
                }
            }
        }
    }
    let zip_source = root.join("pack_builder_zips");
    if zip_source.is_dir() {
        let zip_target = root.join("data").join("pack-builder").join("zips");
        if !dry_run && !zip_target.exists() {
            copy_dir_recursive(&zip_source, &zip_target)?;
        }
        migrated += 1;
        if quarantine {
            if let Some(dir) = quarantine_dir {
                if !dry_run && zip_source.exists() {
                    quarantine_file(root, dir, &zip_source)?;
                }
            }
        }
    }
    Ok(migrated)
}

pub(crate) fn cleanup_context_runs(
    root: &Path,
    retention_days: u64,
    dry_run: bool,
) -> anyhow::Result<ContextRunCleanupReport> {
    let data_root = root.join("data").join("context-runs");
    let hot_root = data_root.join("hot");
    let legacy_root = root.join("context_runs");
    let cutoff_ms =
        now_ms_for_storage().saturating_sub(retention_days.saturating_mul(24 * 60 * 60 * 1000));
    let mut report = ContextRunCleanupReport::default();
    let mut seen = std::collections::HashSet::<String>::new();
    for base in [hot_root.clone(), legacy_root] {
        if !base.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&base).with_context(|| format!("read_dir {}", base.display()))? {
            let entry = entry?;
            let run_dir = entry.path();
            if !run_dir.is_dir() {
                continue;
            }
            let run_id = entry.file_name().to_string_lossy().to_string();
            if !seen.insert(run_id.clone()) {
                continue;
            }
            report.scanned += 1;
            let state_path = run_dir.join("run_state.json");
            let raw = match fs::read_to_string(&state_path) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let mut state = match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(state) => state,
                Err(_) => continue,
            };
            let original_updated = json_u64(&state, "updated_at_ms")
                .or_else(|| json_u64(&state, "created_at_ms"))
                .unwrap_or(0);
            let status = state
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let terminal = matches!(status.as_str(), "completed" | "failed" | "cancelled");
            let stale_nonterminal =
                !terminal && original_updated > 0 && original_updated <= cutoff_ms;
            let archive = (terminal && original_updated > 0 && original_updated <= cutoff_ms)
                || stale_nonterminal;
            if !archive {
                continue;
            }
            let bytes = dir_size(&run_dir)?;
            if stale_nonterminal {
                report.stale_closed += 1;
                if !dry_run {
                    close_stale_context_run(&run_dir, &mut state)?;
                }
            }
            if !dry_run {
                archive_context_run(&data_root, &run_dir, &run_id, original_updated, &state)?;
                fs::remove_dir_all(&run_dir).with_context(|| {
                    format!("remove archived context run {}", run_dir.display())
                })?;
            }
            report.archived += 1;
            report.bytes_archived = report.bytes_archived.saturating_add(bytes);
        }
    }
    if !dry_run {
        write_context_hot_index(&hot_root)?;
    }
    Ok(report)
}

pub(crate) fn close_stale_context_run(
    run_dir: &Path,
    state: &mut serde_json::Value,
) -> anyhow::Result<()> {
    let now = now_ms_for_storage();
    let seq = state
        .get("last_event_seq")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| latest_jsonl_seq(&run_dir.join("events.jsonl")))
        .saturating_add(1);
    if let Some(map) = state.as_object_mut() {
        map.insert("status".to_string(), json!("cancelled"));
        map.insert("updated_at_ms".to_string(), json!(now));
        map.insert("ended_at_ms".to_string(), json!(now));
        map.insert(
            "last_error".to_string(),
            json!("stale_context_run_retired_by_storage_cleanup"),
        );
        map.insert("last_event_seq".to_string(), json!(seq));
    }
    write_string_atomic_sync(
        &run_dir.join("run_state.json"),
        &serde_json::to_string_pretty(state)?,
    )?;
    let event = json!({
        "event_id": format!("evt-storage-cleanup-{now}"),
        "run_id": state.get("run_id").and_then(serde_json::Value::as_str).unwrap_or(""),
        "seq": seq,
        "ts_ms": now,
        "type": "context.run.stale_cancelled",
        "status": "cancelled",
        "revision": 0,
        "payload": {
            "reason": "stale_context_run_retired_by_storage_cleanup",
            "run": state,
        }
    });
    append_jsonl_sync(&run_dir.join("events.jsonl"), &event)?;
    Ok(())
}

pub(crate) fn archive_context_run(
    data_root: &Path,
    run_dir: &Path,
    run_id: &str,
    timestamp_ms: u64,
    state: &serde_json::Value,
) -> anyhow::Result<()> {
    let timestamp = Utc
        .timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Utc::now);
    let month_dir = data_root
        .join("archive")
        .join(format!("{:04}", timestamp.year()))
        .join(format!("{:02}", timestamp.month()));
    fs::create_dir_all(&month_dir)?;
    let archive_path = month_dir.join(format!("{run_id}.tar.gz"));
    if !archive_path.exists() {
        let file = fs::File::create(&archive_path)
            .with_context(|| format!("create {}", archive_path.display()))?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder
            .append_dir_all(run_id, run_dir)
            .with_context(|| format!("archive {}", run_dir.display()))?;
        let encoder = builder.into_inner()?;
        encoder.finish()?;
    }
    let index_line = json!({
        "run_id": run_id,
        "status": state.get("status").cloned().unwrap_or(Value::Null),
        "run_type": state.get("run_type").cloned().unwrap_or(Value::Null),
        "workspace": state.get("workspace").cloned().unwrap_or(Value::Null),
        "created_at_ms": state.get("created_at_ms").cloned().unwrap_or(Value::Null),
        "updated_at_ms": state.get("updated_at_ms").cloned().unwrap_or(Value::Null),
        "archive_path": archive_path.to_string_lossy(),
    });
    append_jsonl_sync(&month_dir.join("index.jsonl"), &index_line)?;
    Ok(())
}

pub(crate) fn write_context_hot_index(hot_root: &Path) -> anyhow::Result<()> {
    let mut rows = Vec::new();
    if hot_root.is_dir() {
        for entry in fs::read_dir(hot_root)? {
            let entry = entry?;
            let run_dir = entry.path();
            if !run_dir.is_dir() {
                continue;
            }
            let raw = match fs::read_to_string(run_dir.join("run_state.json")) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let state: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(state) => state,
                Err(_) => continue,
            };
            rows.push(json!({
                "run_id": state.get("run_id").cloned().unwrap_or_else(|| json!(entry.file_name().to_string_lossy())),
                "status": state.get("status").cloned().unwrap_or(Value::Null),
                "run_type": state.get("run_type").cloned().unwrap_or(Value::Null),
                "workspace": state.get("workspace").cloned().unwrap_or(Value::Null),
                "created_at_ms": state.get("created_at_ms").cloned().unwrap_or(Value::Null),
                "updated_at_ms": state.get("updated_at_ms").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    rows.sort_by(|a, b| {
        json_u64(b, "updated_at_ms")
            .unwrap_or(0)
            .cmp(&json_u64(a, "updated_at_ms").unwrap_or(0))
    });
    write_string_atomic_sync(
        &hot_root
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("index.json"),
        &serde_json::to_string_pretty(&rows)?,
    )
}

pub(crate) fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

pub(crate) fn latest_jsonl_seq(path: &Path) -> u64 {
    let Ok(raw) = fs::read_to_string(path) else {
        return 0;
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("seq").and_then(serde_json::Value::as_u64))
        .max()
        .unwrap_or(0)
}

pub(crate) fn append_jsonl_sync(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

pub(crate) fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if !path.exists() {
        return Ok(0);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry_path)?);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

pub(crate) fn copy_dir_recursive(source: &Path, target: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if meta.is_file() && !target_path.exists() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

pub(crate) fn read_automation_specs_map(
    path: &Path,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2Spec>> {
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub(crate) fn merge_automation_runs(
    merged: &mut std::collections::HashMap<String, AutomationV2RunRecord>,
    path: &Path,
) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(());
    }
    let parsed = parse_storage_automation_runs(&raw)
        .with_context(|| format!("parse automation runs {}", path.display()))?;
    for (run_id, run) in parsed {
        match merged.get(&run_id) {
            Some(existing) if existing.updated_at_ms > run.updated_at_ms => {}
            _ => {
                merged.insert(run_id, run);
            }
        }
    }
    Ok(())
}

fn parse_storage_automation_runs(
    raw: &str,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2RunRecord>> {
    if raw.trim() == "{}" {
        return Ok(std::collections::HashMap::new());
    }
    let value: Value = serde_json::from_str(raw)?;
    let Some(version_value) = value.get("schema_version") else {
        return Ok(serde_json::from_value(value)?);
    };
    let schema_version = version_value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .context("automation_v2_runs.json schema_version must be an unsigned integer")?;
    if schema_version > AUTOMATION_V2_RUNS_SCHEMA_VERSION {
        anyhow::bail!(
            "automation_v2_runs.json schema_version {} is newer than supported version {}",
            schema_version,
            AUTOMATION_V2_RUNS_SCHEMA_VERSION
        );
    }
    let file = serde_json::from_value::<StorageAutomationRunsFile>(value)?;
    Ok(file.runs)
}

fn serialize_storage_automation_runs(
    runs: &std::collections::HashMap<String, AutomationV2RunRecord>,
) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(
        &StorageAutomationRunsFileRef {
            schema_version: AUTOMATION_V2_RUNS_SCHEMA_VERSION,
            runs,
        },
    )?)
}

pub(crate) fn compact_storage_hot_run(
    run: &mut AutomationV2RunRecord,
    automations: &std::collections::HashMap<String, AutomationV2Spec>,
) {
    if !storage_run_is_terminal(&run.status) {
        return;
    }
    run.checkpoint.node_outputs.clear();
    run.runtime_context = None;
    if let Some(snapshot) = run.automation_snapshot.as_ref() {
        if automations
            .get(&run.automation_id)
            .is_some_and(|canonical| canonical.updated_at_ms >= snapshot.updated_at_ms)
        {
            run.automation_snapshot = None;
        }
    }
}

pub(crate) fn storage_run_is_terminal(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Cancelled
    )
}

pub(crate) fn write_storage_run_shard(
    active_path: &Path,
    run: &AutomationV2RunRecord,
) -> anyhow::Result<()> {
    let path = storage_run_shard_path(active_path, run);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_string_atomic_sync(&path, &serialize_storage_automation_run_shard(run)?)?;
    Ok(())
}

fn serialize_storage_automation_run_shard(run: &AutomationV2RunRecord) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(
        &StorageAutomationRunShardFileRef {
            schema_version: AUTOMATION_V2_RUNS_SCHEMA_VERSION,
            run,
        },
    )?)
}

pub(crate) fn storage_run_shard_path(active_path: &Path, run: &AutomationV2RunRecord) -> PathBuf {
    let timestamp_ms = run.updated_at_ms.max(run.created_at_ms);
    let timestamp = Utc
        .timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Utc::now);
    active_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("automation-runs")
        .join(format!("{:04}", timestamp.year()))
        .join(format!("{:02}", timestamp.month()))
        .join(format!("{}.json", run.run_id))
}

pub(crate) fn storage_tmp_files(data_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if !data_dir.exists() {
        return Ok(paths);
    }
    for entry in
        fs::read_dir(data_dir).with_context(|| format!("read_dir {}", data_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(".automations_v2.json.tmp-")
            || name.starts_with(".automation_v2_runs.json.tmp-")
        {
            paths.push(path);
        }
    }
    Ok(paths)
}

pub(crate) fn old_large_engine_logs(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let logs_dir = root.join("logs");
    let mut paths = Vec::new();
    if !logs_dir.exists() {
        return Ok(paths);
    }
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in
        fs::read_dir(&logs_dir).with_context(|| format!("read_dir {}", logs_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("tandem.engine.") || !name.ends_with(".jsonl") {
            continue;
        }
        let meta = fs::metadata(&path)?;
        if meta.len() >= 50_000_000 && meta.modified().unwrap_or(cutoff) < cutoff {
            paths.push(path);
        }
    }
    Ok(paths)
}

pub(crate) fn quarantine_file(
    root: &Path,
    quarantine_dir: &Path,
    path: &Path,
) -> anyhow::Result<()> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let target = quarantine_dir.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(path, target).with_context(|| format!("quarantine {}", path.display()))?;
    Ok(())
}

pub(crate) fn write_string_atomic_sync(path: &Path, payload: &str) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("state.json");
    let temp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        now_ms_for_storage()
    ));
    fs::write(&temp_path, payload)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn now_ms_for_storage() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn print_storage_report(report: &StorageDoctorReport) {
    println!("Storage root: {}", report.root);
    println!("Data dir: {}", report.data_dir);
    println!("Candidate bytes: {}", report.total_candidate_bytes);
    for file in &report.files {
        println!("  - {} [{} bytes]: {}", file.path, file.bytes, file.action);
    }
    for recommendation in &report.recommendations {
        println!("  * {recommendation}");
    }
}

pub(crate) fn print_storage_cleanup_report(report: &StorageCleanupReport) {
    println!("Storage root: {}", report.root);
    println!("Runs loaded: {}", report.runs_loaded);
    println!("Hot runs written: {}", report.hot_runs_written);
    println!("History shards written: {}", report.shards_written);
    println!(
        "Root feature files migrated: {}",
        report.root_files_migrated
    );
    println!("Context runs scanned: {}", report.context_runs_scanned);
    println!("Context runs archived: {}", report.context_runs_archived);
    println!(
        "Context runs stale-closed: {}",
        report.context_runs_stale_closed
    );
    println!(
        "Context run bytes archived: {}",
        report.context_run_bytes_archived
    );
    println!(
        "Default knowledge rows cleared: {}",
        report.default_knowledge_rows_cleared
    );
    println!(
        "Default knowledge state files matched: {}",
        report.default_knowledge_state_files_matched
    );
    println!("Candidate bytes: {}", report.candidate_bytes);
    if let Some(dir) = &report.quarantine_dir {
        println!("Quarantine dir: {dir}");
    }
    for path in &report.files_quarantined {
        println!("  - quarantined {path}");
    }
}

pub(crate) fn print_worktree_cleanup_report(report: &serde_json::Value) {
    let repo_root = report
        .get("repo_root")
        .and_then(|value| value.as_str())
        .unwrap_or("<unknown>");
    let managed_root = report
        .get("managed_root")
        .and_then(|value| value.as_str())
        .unwrap_or("<unknown>");
    let dry_run = report
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let active_count = report
        .get("active_paths")
        .and_then(|value| value.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let stale_count = report
        .get("stale_paths")
        .and_then(|value| value.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let cleaned_count = report
        .get("cleaned_worktrees")
        .and_then(|value| value.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let orphan_removed_count = report
        .get("orphan_dirs_removed")
        .and_then(|value| value.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let failure_count = report
        .get("failures")
        .and_then(|value| value.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);

    println!(
        "Managed worktree cleanup: {}",
        if dry_run { "preview" } else { "applied" }
    );
    println!("Repository root: {}", repo_root);
    println!("Managed root: {}", managed_root);
    println!("Active tracked worktrees: {}", active_count);
    println!("Stale candidates: {}", stale_count);
    println!("Removed entries: {}", cleaned_count + orphan_removed_count);
    println!("Failures: {}", failure_count);

    if let Some(rows) = report
        .get("cleaned_worktrees")
        .and_then(|value| value.as_array())
    {
        for row in rows {
            let path = row
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("<unknown>");
            let branch = row
                .get("branch")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if branch.is_empty() {
                println!("  removed worktree: {}", path);
            } else {
                println!("  removed worktree: {} ({})", path, branch);
            }
        }
    }
    if dry_run {
        if let Some(rows) = report.get("stale_paths").and_then(|value| value.as_array()) {
            for row in rows {
                let path = row
                    .get("path")
                    .and_then(|value| value.as_str())
                    .unwrap_or("<unknown>");
                let branch = row
                    .get("branch")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if branch.is_empty() {
                    println!("  stale candidate: {}", path);
                } else {
                    println!("  stale candidate: {} ({})", path, branch);
                }
            }
        }
    }
    if let Some(rows) = report
        .get("orphan_dirs_removed")
        .and_then(|value| value.as_array())
    {
        for row in rows {
            if let Some(path) = row.get("path").and_then(|value| value.as_str()) {
                println!("  removed orphan dir: {}", path);
            }
        }
    }
    if dry_run {
        if let Some(rows) = report.get("orphan_dirs").and_then(|value| value.as_array()) {
            for row in rows {
                if let Some(path) = row.as_str() {
                    println!("  orphan dir: {}", path);
                }
            }
        }
    }
    if let Some(rows) = report.get("failures").and_then(|value| value.as_array()) {
        for row in rows {
            let path = row
                .get("path")
                .and_then(|value| value.as_str())
                .or_else(|| row.get("code").and_then(|value| value.as_str()))
                .unwrap_or("<unknown>");
            let detail = row
                .get("error")
                .and_then(|value| value.as_str())
                .or_else(|| row.get("stderr").and_then(|value| value.as_str()))
                .unwrap_or("cleanup failed");
            println!("  failure: {} -> {}", path, detail);
        }
    }
}
