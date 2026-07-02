use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::TenantContext;
use tokio::fs;

use crate::automation_v2::types::*;
use crate::stateful_runtime::ensure_automation_run_definition_metadata;
use crate::util::time::now_ms;

use super::{sanitize_path_id, write_string_atomic};

pub(crate) const AUTOMATION_V2_RUNS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationV2RunsFile {
    schema_version: u32,
    runs: std::collections::HashMap<String, AutomationV2RunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationV2RunShardFile {
    schema_version: u32,
    run: AutomationV2RunRecord,
}

#[derive(Debug, Serialize)]
struct AutomationV2RunShardFileRef<'a> {
    schema_version: u32,
    run: &'a AutomationV2RunRecord,
}

pub(crate) fn parse_automation_v2_runs_file(
    raw: &str,
) -> anyhow::Result<(
    std::collections::HashMap<String, AutomationV2RunRecord>,
    bool,
)> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok((std::collections::HashMap::new(), raw.trim() == "{}"));
    }
    let value: Value =
        serde_json::from_str(raw).context("failed to parse automation_v2_runs.json")?;
    let Some(version_value) = value.get("schema_version") else {
        let (runs, _backfilled_definition_metadata) = parse_automation_v2_runs_map(value)
            .context("failed to parse legacy automation_v2_runs.json v0 map")?;
        return Ok((upgrade_automation_v2_runs_file(0, runs)?, true));
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
    let schema_version_for_upgrade = schema_version;
    let runs_value = value
        .get("runs")
        .cloned()
        .context("versioned automation_v2_runs.json missing runs object")?;
    let (runs, backfilled_definition_metadata) = parse_automation_v2_runs_map(runs_value)
        .context("failed to parse versioned automation_v2_runs.json runs")?;
    let upgraded = schema_version_for_upgrade < AUTOMATION_V2_RUNS_SCHEMA_VERSION
        || backfilled_definition_metadata;
    Ok((
        upgrade_automation_v2_runs_file(schema_version_for_upgrade, runs)?,
        upgraded,
    ))
}

fn parse_automation_v2_runs_map(
    value: Value,
) -> anyhow::Result<(
    std::collections::HashMap<String, AutomationV2RunRecord>,
    bool,
)> {
    let object = value
        .as_object()
        .context("automation_v2_runs entries must be a JSON object")?;
    let mut runs = std::collections::HashMap::new();
    let mut backfilled_definition_metadata = false;
    for (run_id, run_value) in object {
        let (run, backfilled) = parse_automation_v2_run_entry(run_id, run_value.clone())?;
        backfilled_definition_metadata |= backfilled;
        runs.insert(run.run_id.clone(), run);
    }
    Ok((runs, backfilled_definition_metadata))
}

fn parse_automation_v2_run_entry(
    run_id_key: &str,
    value: Value,
) -> anyhow::Result<(AutomationV2RunRecord, bool)> {
    match serde_json::from_value::<AutomationV2RunRecord>(value.clone()) {
        Ok(mut run) => {
            let mut backfilled = automation_run_definition_metadata_missing(&run);
            ensure_automation_run_definition_metadata(&mut run);
            backfilled |= stamp_automation_run_enterprise_scope_metadata(&mut run);
            Ok((run, backfilled))
        }
        Err(error) => recover_corrupt_automation_v2_run_entry(run_id_key, value, error.to_string()),
    }
}

fn recover_corrupt_automation_v2_run_entry(
    run_id_key: &str,
    value: Value,
    parse_error: String,
) -> anyhow::Result<(AutomationV2RunRecord, bool)> {
    let object = value
        .as_object()
        .context("corrupt automation v2 run entry is not recoverable")?;
    let run_id = json_string_field(object, "run_id")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| run_id_key.to_string());
    let automation_id = json_string_field(object, "automation_id")
        .filter(|value| !value.trim().is_empty())
        .context("corrupt automation v2 run entry is missing automation_id")?;
    let now = now_ms();
    let created_at_ms = json_u64_field(object, "created_at_ms").unwrap_or(now);
    let trigger_type =
        json_string_field(object, "trigger_type").unwrap_or_else(|| "recovered".to_string());
    let tenant_context = object
        .get("tenant_context")
        .cloned()
        .and_then(|value| serde_json::from_value::<TenantContext>(value).ok())
        .unwrap_or_else(TenantContext::local_implicit);
    let automation_snapshot = object
        .get("automation_snapshot")
        .cloned()
        .and_then(|value| serde_json::from_value::<AutomationV2Spec>(value).ok());
    let effective_execution_profile = object
        .get("effective_execution_profile")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let requested_execution_profile = object
        .get("requested_execution_profile")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let detail =
        format!("automation run checkpoint could not be parsed during startup: {parse_error}");

    let mut run = AutomationV2RunRecord {
        run_id,
        automation_id,
        tenant_context,
        trigger_type,
        status: AutomationRunStatus::Blocked,
        created_at_ms,
        updated_at_ms: now,
        started_at_ms: json_u64_field(object, "started_at_ms"),
        finished_at_ms: Some(now),
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: AutomationRunCheckpoint {
            completed_nodes: Vec::new(),
            pending_nodes: Vec::new(),
            node_outputs: std::collections::HashMap::new(),
            node_attempts: std::collections::HashMap::new(),
            node_attempt_verdicts: std::collections::HashMap::new(),
            blocked_nodes: vec!["checkpoint".to_string()],
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: vec![AutomationLifecycleRecord {
                event: "run_blocked_corrupt_checkpoint".to_string(),
                recorded_at_ms: now,
                reason: Some(detail.clone()),
                stop_kind: None,
                metadata: Some(json!({
                    "run_id_key": run_id_key,
                    "parse_error": parse_error,
                })),
            }],
            last_failure: Some(AutomationFailureRecord {
                node_id: "checkpoint".to_string(),
                reason: detail.clone(),
                failed_at_ms: now,
                failure_kind: Some("checkpoint_recovery_failed".to_string()),
                metadata: None,
            }),
        },
        runtime_context: None,
        automation_snapshot,
        execution_claim: None,
        execution_claim_epoch: 0,
        pause_reason: None,
        resume_reason: None,
        detail: Some(detail.clone()),
        stop_kind: None,
        stop_reason: Some(detail),
        prompt_tokens: json_u64_field(object, "prompt_tokens").unwrap_or(0),
        completion_tokens: json_u64_field(object, "completion_tokens").unwrap_or(0),
        total_tokens: json_u64_field(object, "total_tokens").unwrap_or(0),
        estimated_cost_usd: object
            .get("estimated_cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        scheduler: None,
        trigger_reason: json_string_field(object, "trigger_reason"),
        consumed_handoff_id: json_string_field(object, "consumed_handoff_id"),
        learning_summary: object
            .get("learning_summary")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
        effective_execution_profile,
        requested_execution_profile,
        workflow_definition_version: json_string_field(object, "workflow_definition_version"),
        workflow_definition_snapshot_hash: json_string_field(
            object,
            "workflow_definition_snapshot_hash",
        ),
    };
    let mut backfilled = automation_run_definition_metadata_missing(&run);
    ensure_automation_run_definition_metadata(&mut run);
    backfilled |= stamp_automation_run_enterprise_scope_metadata(&mut run);
    Ok((run, backfilled))
}

fn automation_run_definition_metadata_missing(run: &AutomationV2RunRecord) -> bool {
    run.automation_snapshot.is_some()
        && (run.workflow_definition_version.is_none()
            || run.workflow_definition_snapshot_hash.is_none())
}

fn stamp_automation_run_enterprise_scope_metadata(run: &mut AutomationV2RunRecord) -> bool {
    let Some(snapshot) = run.automation_snapshot.as_mut() else {
        return false;
    };
    let before = snapshot.metadata.clone();
    snapshot.stamp_enterprise_scope_metadata();
    snapshot.metadata != before
}

fn json_string_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn json_u64_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<u64> {
    object.get(field).and_then(Value::as_u64)
}

pub(crate) fn serialize_automation_v2_runs_file(
    runs: std::collections::HashMap<String, AutomationV2RunRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationV2RunsFile {
        schema_version: AUTOMATION_V2_RUNS_SCHEMA_VERSION,
        runs,
    })
    .context("failed to serialize automation_v2_runs.json")
}

fn parse_automation_v2_run_shard_file(raw: &str) -> anyhow::Result<AutomationV2RunRecord> {
    let value = serde_json::from_str::<Value>(raw)
        .context("failed to parse automation v2 run history shard")?;
    let Some(version_value) = value.get("schema_version") else {
        let run = serde_json::from_value::<AutomationV2RunRecord>(value)
            .context("failed to parse legacy automation v2 run history shard")?;
        return upgrade_automation_v2_run_shard(0, run);
    };
    let schema_version = version_value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .context("automation v2 run history shard schema_version must be an unsigned integer")?;
    if schema_version > AUTOMATION_V2_RUNS_SCHEMA_VERSION {
        anyhow::bail!(
            "automation v2 run history shard schema_version {} is newer than supported version {}",
            schema_version,
            AUTOMATION_V2_RUNS_SCHEMA_VERSION
        );
    }
    let file = serde_json::from_value::<AutomationV2RunShardFile>(value)
        .context("failed to parse versioned automation v2 run history shard")?;
    upgrade_automation_v2_run_shard(file.schema_version, file.run)
}

fn serialize_automation_v2_run_shard(run: &AutomationV2RunRecord) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationV2RunShardFileRef {
        schema_version: AUTOMATION_V2_RUNS_SCHEMA_VERSION,
        run,
    })
    .context("failed to serialize automation v2 run history shard")
}

fn upgrade_automation_v2_runs_file(
    from_version: u32,
    runs: std::collections::HashMap<String, AutomationV2RunRecord>,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2RunRecord>> {
    let mut current = from_version;
    while current < AUTOMATION_V2_RUNS_SCHEMA_VERSION {
        match current {
            0 => {
                current = 1;
            }
            other => anyhow::bail!("unsupported automation_v2_runs.json schema version {other}"),
        }
    }
    Ok(runs)
}

fn upgrade_automation_v2_run_shard(
    from_version: u32,
    mut run: AutomationV2RunRecord,
) -> anyhow::Result<AutomationV2RunRecord> {
    let mut current = from_version;
    while current < AUTOMATION_V2_RUNS_SCHEMA_VERSION {
        match current {
            0 => {
                current = 1;
            }
            other => {
                anyhow::bail!("unsupported automation v2 run history shard schema version {other}")
            }
        }
    }
    ensure_automation_run_definition_metadata(&mut run);
    stamp_automation_run_enterprise_scope_metadata(&mut run);
    Ok(run)
}

pub(crate) fn automation_run_is_terminal(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Cancelled
    )
}

pub(crate) fn automation_v2_run_is_nonterminal_recovered_context_run(
    run: &AutomationV2RunRecord,
) -> bool {
    run.trigger_type == "recovered_context_run" && !automation_run_is_terminal(&run.status)
}

pub(crate) fn compact_automation_v2_runs_for_hot_storage(
    runs: &mut std::collections::HashMap<String, AutomationV2RunRecord>,
    automations: &std::collections::HashMap<String, AutomationV2Spec>,
    cutoff_ms: u64,
) {
    for run in runs.values_mut() {
        if !automation_run_is_terminal(&run.status) {
            continue;
        }
        if let Some(snapshot) = run.automation_snapshot.as_ref() {
            if automations
                .get(&run.automation_id)
                .is_some_and(|canonical| canonical.updated_at_ms >= snapshot.updated_at_ms)
            {
                run.automation_snapshot = None;
            }
        }
        if run.updated_at_ms <= cutoff_ms {
            run.checkpoint.node_outputs.clear();
            run.runtime_context = None;
        }
    }
}

fn automation_v2_hot_retention_days() -> u64 {
    std::env::var("TANDEM_AUTOMATION_V2_RUNS_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(7)
}

pub(crate) fn automation_v2_hot_cutoff_ms() -> u64 {
    let retention_days = automation_v2_hot_retention_days();
    if retention_days == 0 {
        return 0;
    }
    now_ms().saturating_sub(retention_days.saturating_mul(24 * 60 * 60 * 1000))
}

pub(crate) fn automation_v2_run_history_root(active_path: &Path) -> PathBuf {
    let stem = active_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("runs");
    active_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("automation-runs")
        .join(stem)
}

fn automation_v2_run_history_month(run: &AutomationV2RunRecord) -> (i32, u32) {
    let timestamp_ms = run.updated_at_ms.max(run.created_at_ms);
    let timestamp = Utc
        .timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Utc::now);
    (timestamp.year(), timestamp.month())
}

pub(crate) fn automation_v2_run_history_shard_path(
    active_path: &Path,
    run: &AutomationV2RunRecord,
) -> PathBuf {
    let (year, month) = automation_v2_run_history_month(run);
    let sanitized_run_id = sanitize_path_id(&run.run_id);
    automation_v2_run_history_root(active_path)
        .join(format!("{year:04}"))
        .join(format!("{month:02}"))
        .join(format!("{}.json", sanitized_run_id))
}

pub(crate) async fn write_automation_v2_run_history_shard(
    active_path: &Path,
    run: &AutomationV2RunRecord,
) -> anyhow::Result<PathBuf> {
    let path = automation_v2_run_history_shard_path(active_path, run);
    if automation_v2_run_is_nonterminal_recovered_context_run(run) {
        let _ = fs::remove_file(&path).await;
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let payload = serialize_automation_v2_run_shard(run)?;
    write_string_atomic(&path, &payload).await?;
    Ok(path)
}

pub(crate) async fn load_automation_v2_run_history_shard(
    active_path: &Path,
    run_id: &str,
) -> Option<AutomationV2RunRecord> {
    let root = automation_v2_run_history_root(active_path);
    let mut years = fs::read_dir(&root).await.ok()?;
    while let Ok(Some(year)) = years.next_entry().await {
        let year_path = year.path();
        if !year_path.is_dir() {
            continue;
        }
        let mut months = match fs::read_dir(&year_path).await {
            Ok(months) => months,
            Err(_) => continue,
        };
        while let Ok(Some(month)) = months.next_entry().await {
            let path = month.path().join(format!("{run_id}.json"));
            if !path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&path).await.ok()?;
            return parse_automation_v2_run_shard_file(&raw)
                .ok()
                .filter(|run| !automation_v2_run_is_nonterminal_recovered_context_run(run));
        }
    }
    None
}

pub(crate) async fn load_automation_v2_run_history_shards(
    active_path: &Path,
) -> Vec<AutomationV2RunRecord> {
    let root = automation_v2_run_history_root(active_path);
    let mut runs = Vec::new();
    let Ok(mut years) = fs::read_dir(&root).await else {
        return runs;
    };
    while let Ok(Some(year)) = years.next_entry().await {
        let year_path = year.path();
        if !year_path.is_dir() {
            continue;
        }
        let mut months = match fs::read_dir(&year_path).await {
            Ok(months) => months,
            Err(_) => continue,
        };
        while let Ok(Some(month)) = months.next_entry().await {
            let month_path = month.path();
            if !month_path.is_dir() {
                continue;
            }
            let mut shards = match fs::read_dir(&month_path).await {
                Ok(shards) => shards,
                Err(_) => continue,
            };
            while let Ok(Some(shard)) = shards.next_entry().await {
                let path = shard.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Ok(raw) = fs::read_to_string(&path).await else {
                    continue;
                };
                if let Ok(run) = parse_automation_v2_run_shard_file(&raw) {
                    if automation_v2_run_is_nonterminal_recovered_context_run(&run) {
                        continue;
                    }
                    runs.push(run);
                }
            }
        }
    }
    runs
}
