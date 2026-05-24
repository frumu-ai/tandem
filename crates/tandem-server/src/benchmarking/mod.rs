use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tandem_types::EngineEvent;
use tokio::fs;

use crate::app::state::AppState;

pub const BENCHMARK_SUMMARY_SCHEMA_VERSION: u32 = 1;
pub const BENCHMARK_PRICING_CATALOG_VERSION: &str = "benchmark-static-2026-05-06";

#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub profiling_enabled: bool,
    pub notion_sync_enabled: bool,
    pub notion_database_id: Option<String>,
    pub notion_sync_mode: NotionSyncMode,
    pub benchmark_suite_id: Option<String>,
    pub task_name: Option<String>,
    pub task_pack_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotionSyncMode {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkRunSummary {
    pub schema_version: u32,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benchmark_suite_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub usage_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    pub pricing_catalog_version: String,
    #[serde(default)]
    pub tool_call_counts_by_name: HashMap<String, u64>,
    #[serde(default)]
    pub mcp_call_counts_by_name: HashMap<String, u64>,
    pub webfetch_calls: u64,
    pub retry_count: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    pub artifact_count: u64,
    pub artifact_total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_enabled: Option<bool>,
    pub notion_sync_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notion_page_id: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct BenchmarkPriceEntry {
    pub provider: &'static str,
    pub model: &'static str,
    pub input_price_per_1m_tokens: f64,
    pub output_price_per_1m_tokens: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotionSyncResult {
    pub status: String,
    pub page_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkHarnessRequest {
    pub task_pack_id: String,
    pub provider: String,
    pub model: String,
    pub repeat_count: u8,
}

#[derive(Debug, Default)]
struct RunAccumulator {
    run_id: String,
    session_ids: HashSet<String>,
    benchmark_suite_id: Option<String>,
    task_name: Option<String>,
    task_pack_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    started_at: Option<u64>,
    finished_at: Option<u64>,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    usage_source: Option<String>,
    tool_call_counts_by_name: HashMap<String, u64>,
    mcp_call_counts_by_name: HashMap<String, u64>,
    webfetch_calls: u64,
    retry_count: u64,
    status: Option<String>,
    failure_reason: Option<String>,
    seen_tool_part_ids: HashSet<String>,
}

impl RunAccumulator {
    fn new(run_id: impl Into<String>, config: &BenchmarkConfig) -> Self {
        Self {
            run_id: run_id.into(),
            benchmark_suite_id: config.benchmark_suite_id.clone(),
            task_name: config.task_name.clone(),
            task_pack_id: config.task_pack_id.clone(),
            ..Default::default()
        }
    }
}

const STATIC_PRICE_CATALOG: &[BenchmarkPriceEntry] = &[
    BenchmarkPriceEntry {
        provider: "openai",
        model: "gpt-4.1-mini",
        input_price_per_1m_tokens: 0.40,
        output_price_per_1m_tokens: 1.60,
    },
    BenchmarkPriceEntry {
        provider: "openai",
        model: "gpt-4o-mini",
        input_price_per_1m_tokens: 0.15,
        output_price_per_1m_tokens: 0.60,
    },
    BenchmarkPriceEntry {
        provider: "anthropic",
        model: "claude-sonnet-4",
        input_price_per_1m_tokens: 3.00,
        output_price_per_1m_tokens: 15.00,
    },
];

pub fn benchmark_config_from_env() -> BenchmarkConfig {
    BenchmarkConfig {
        profiling_enabled: env_flag("TANDEM_BENCHMARK_PROFILING_ENABLED"),
        notion_sync_enabled: env_flag("TANDEM_BENCHMARK_NOTION_SYNC_ENABLED"),
        notion_database_id: env_non_empty("TANDEM_BENCHMARK_NOTION_DATABASE_ID"),
        notion_sync_mode: match env_non_empty("TANDEM_BENCHMARK_NOTION_SYNC_MODE")
            .unwrap_or_else(|| "manual".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "automatic" | "auto" => NotionSyncMode::Automatic,
            _ => NotionSyncMode::Manual,
        },
        benchmark_suite_id: env_non_empty("TANDEM_BENCHMARK_SUITE_ID"),
        task_name: env_non_empty("TANDEM_BENCHMARK_TASK_NAME"),
        task_pack_id: env_non_empty("TANDEM_BENCHMARK_TASK_PACK_ID"),
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub async fn run_benchmark_profiler(state: AppState) {
    let config = benchmark_config_from_env();
    if !config.profiling_enabled {
        tracing::debug!("benchmark profiler: disabled");
        return;
    }
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("benchmark profiler: skipped because runtime did not become ready");
        return;
    }

    let mut profiler = BenchmarkProfiler::new(state, config);
    profiler.run().await;
}

struct BenchmarkProfiler {
    state: AppState,
    config: BenchmarkConfig,
    runs: HashMap<String, RunAccumulator>,
    session_run_ids: HashMap<String, String>,
}

impl BenchmarkProfiler {
    fn new(state: AppState, config: BenchmarkConfig) -> Self {
        Self {
            state,
            config,
            runs: HashMap::new(),
            session_run_ids: HashMap::new(),
        }
    }

    async fn run(&mut self) {
        let mut rx = self.state.event_bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => self.handle_event(event).await,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }

    async fn handle_event(&mut self, event: EngineEvent) {
        match event.event_type.as_str() {
            "session.run.started" => self.handle_session_started(&event),
            "routine.run.started" => self.handle_routine_started(&event),
            "routine.run.model_selected" => self.handle_model_selected(&event),
            "provider.usage" => self.handle_provider_usage(&event),
            "provider.call.iteration.retry" => self.handle_retry(&event),
            "message.part.updated" => self.handle_message_part(&event),
            "session.run.finished"
            | "routine.run.completed"
            | "routine.run.failed"
            | "routine.run.paused"
            | "automation_v2.run.failed" => {
                self.handle_terminal_event(&event).await;
            }
            _ => {}
        }
    }

    fn run_mut(&mut self, run_id: &str) -> &mut RunAccumulator {
        self.runs
            .entry(run_id.to_string())
            .or_insert_with(|| RunAccumulator::new(run_id, &self.config))
    }

    fn handle_session_started(&mut self, event: &EngineEvent) {
        let Some(run_id) = string_property(&event.properties, "runID") else {
            return;
        };
        if let Some(session_id) = string_property(&event.properties, "sessionID") {
            self.session_run_ids
                .insert(session_id.clone(), run_id.clone());
            self.run_mut(&run_id).session_ids.insert(session_id);
        }
        let started_at =
            u64_property(&event.properties, "startedAtMs").unwrap_or_else(crate::now_ms);
        let run = self.run_mut(&run_id);
        run.started_at.get_or_insert(started_at);
        apply_optional_metadata(run, &event.properties);
    }

    fn handle_routine_started(&mut self, event: &EngineEvent) {
        let Some(run_id) = string_property(&event.properties, "runID") else {
            return;
        };
        let started_at =
            u64_property(&event.properties, "startedAtMs").unwrap_or_else(crate::now_ms);
        let run = self.run_mut(&run_id);
        run.started_at.get_or_insert(started_at);
        if run.task_name.is_none() {
            run.task_name = string_property(&event.properties, "routineID");
        }
        apply_optional_metadata(run, &event.properties);
    }

    fn handle_model_selected(&mut self, event: &EngineEvent) {
        let Some(run_id) = string_property(&event.properties, "runID") else {
            return;
        };
        let run = self.run_mut(&run_id);
        if run.provider.is_none() {
            run.provider = string_property(&event.properties, "providerID");
        }
        if run.model.is_none() {
            run.model = string_property(&event.properties, "modelID");
        }
    }

    fn handle_provider_usage(&mut self, event: &EngineEvent) {
        let Some(run_id) = run_id_for_event(&event.properties, &self.session_run_ids) else {
            return;
        };
        let run = self.run_mut(&run_id);
        if let Some(session_id) = string_property(&event.properties, "sessionID") {
            run.session_ids.insert(session_id);
        }
        if run.provider.is_none() {
            run.provider = string_property(&event.properties, "providerID");
        }
        if run.model.is_none() {
            run.model = string_property(&event.properties, "modelID");
        }
        let input = u64_property(&event.properties, "promptTokens").unwrap_or(0);
        let output = u64_property(&event.properties, "completionTokens").unwrap_or(0);
        let total = u64_property(&event.properties, "totalTokens")
            .unwrap_or_else(|| input.saturating_add(output));
        run.input_tokens = run.input_tokens.saturating_add(input);
        run.output_tokens = run.output_tokens.saturating_add(output);
        run.total_tokens = run.total_tokens.saturating_add(total);
        let usage_source = string_property(&event.properties, "usageSource")
            .unwrap_or_else(|| "unknown".to_string());
        run.usage_source = Some(merge_usage_source(
            run.usage_source.as_deref(),
            &usage_source,
        ));
        apply_optional_metadata(run, &event.properties);
    }

    fn handle_retry(&mut self, event: &EngineEvent) {
        let Some(run_id) = run_id_for_event(&event.properties, &self.session_run_ids) else {
            return;
        };
        let run = self.run_mut(&run_id);
        run.retry_count = run.retry_count.saturating_add(1);
        if run.provider.is_none() {
            run.provider = string_property(&event.properties, "providerID");
        }
        if run.model.is_none() {
            run.model = string_property(&event.properties, "modelID");
        }
    }

    fn handle_message_part(&mut self, event: &EngineEvent) {
        let Some(part) = event.properties.get("part") else {
            return;
        };
        let Some(session_id) = string_property(part, "sessionID") else {
            return;
        };
        let Some(run_id) = self.session_run_ids.get(&session_id).cloned() else {
            return;
        };
        let Some(tool) = string_property(part, "tool") else {
            return;
        };
        let state = string_property(part, "state").unwrap_or_default();
        let is_terminal_tool_event = matches!(state.as_str(), "completed" | "failed")
            || part.get("result").is_some()
            || part.get("error").is_some();
        if !is_terminal_tool_event {
            return;
        }
        let part_id = string_property(part, "id").unwrap_or_else(|| {
            format!(
                "{session_id}:{}:{tool}",
                string_property(part, "messageID").unwrap_or_default()
            )
        });
        let run = self.run_mut(&run_id);
        if !run.seen_tool_part_ids.insert(part_id) {
            return;
        }
        increment_count(&mut run.tool_call_counts_by_name, &tool);
        if tool == "webfetch" || tool == "webfetch_html" {
            run.webfetch_calls = run.webfetch_calls.saturating_add(1);
        }
        if tool == "mcp_list" || tool.starts_with("mcp.") {
            increment_count(&mut run.mcp_call_counts_by_name, &tool);
        }
    }

    async fn handle_terminal_event(&mut self, event: &EngineEvent) {
        let Some(run_id) = run_id_for_event(&event.properties, &self.session_run_ids)
            .or_else(|| string_property(&event.properties, "runID"))
            .or_else(|| string_property(&event.properties, "run_id"))
        else {
            return;
        };
        {
            let run = self.run_mut(&run_id);
            run.finished_at =
                Some(u64_property(&event.properties, "finishedAtMs").unwrap_or_else(crate::now_ms));
            run.status = Some(terminal_status(event));
            run.failure_reason = string_property(&event.properties, "reason")
                .or_else(|| string_property(&event.properties, "error"));
            apply_optional_metadata(run, &event.properties);
        }

        let Some(run) = self.runs.get(&run_id) else {
            return;
        };
        let (artifact_count, artifact_total_bytes) = self.scan_artifacts(run).await;
        let mut summary = build_summary(run, artifact_count, artifact_total_bytes);
        let path = benchmark_summary_path(&summary.run_id);
        let initial_sync_status = initial_notion_status(&self.config);
        summary.notion_sync_status = initial_sync_status;

        if let Err(error) = write_summary(&path, &summary).await {
            tracing::warn!(
                run_id = %summary.run_id,
                error = %error,
                "benchmark profiler: failed to write summary"
            );
            return;
        }

        if self.config.notion_sync_enabled
            && self.config.notion_sync_mode == NotionSyncMode::Automatic
            && self.config.notion_database_id.is_some()
        {
            let sync = sync_summary_to_notion(&self.state, &summary, &path).await;
            apply_notion_sync_result(&mut summary, sync);
            if let Err(error) = write_summary(&path, &summary).await {
                tracing::warn!(
                    run_id = %summary.run_id,
                    error = %error,
                    "benchmark profiler: failed to persist notion sync status"
                );
            }
        }
    }

    async fn scan_artifacts(&self, run: &RunAccumulator) -> (u64, u64) {
        let workspace_root = if let Some(session_id) = run.session_ids.iter().next() {
            self.state
                .storage
                .get_session(session_id)
                .await
                .and_then(|session| {
                    session
                        .workspace_root
                        .or_else(|| tandem_core::normalize_workspace_path(&session.directory))
                })
        } else {
            None
        };
        let workspace_root = match workspace_root {
            Some(root) => root,
            None => self.state.workspace_index.snapshot().await.root,
        };
        scan_run_artifacts(&PathBuf::from(workspace_root), &run.run_id)
    }
}

pub fn benchmark_summary_path(run_id: &str) -> PathBuf {
    crate::config::paths::resolve_benchmark_runs_dir()
        .join(sanitize_path_segment(run_id))
        .join("summary.json")
}

pub async fn write_summary(path: &Path, summary: &BenchmarkRunSummary) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_string_pretty(summary)?;
    fs::write(path, payload).await?;
    Ok(())
}

fn build_summary(
    run: &RunAccumulator,
    artifact_count: u64,
    artifact_total_bytes: u64,
) -> BenchmarkRunSummary {
    let duration_ms = match (run.started_at, run.finished_at) {
        (Some(start), Some(finish)) => Some(finish.saturating_sub(start)),
        _ => None,
    };
    let estimated_cost_usd = match (run.provider.as_deref(), run.model.as_deref()) {
        (Some(provider), Some(model)) => estimate_cost_usd(
            provider,
            model,
            run.input_tokens,
            run.output_tokens,
            STATIC_PRICE_CATALOG,
        ),
        _ => None,
    };
    let memory_enabled = Some(
        run.tool_call_counts_by_name
            .keys()
            .any(|tool| tool.starts_with("memory_")),
    );

    BenchmarkRunSummary {
        schema_version: BENCHMARK_SUMMARY_SCHEMA_VERSION,
        run_id: run.run_id.clone(),
        benchmark_suite_id: run.benchmark_suite_id.clone(),
        task_name: run.task_name.clone(),
        task_pack_id: run.task_pack_id.clone(),
        provider: run.provider.clone(),
        model: run.model.clone(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        duration_ms,
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        total_tokens: run.total_tokens,
        usage_source: run
            .usage_source
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        estimated_cost_usd,
        pricing_catalog_version: BENCHMARK_PRICING_CATALOG_VERSION.to_string(),
        tool_call_counts_by_name: run.tool_call_counts_by_name.clone(),
        mcp_call_counts_by_name: run.mcp_call_counts_by_name.clone(),
        webfetch_calls: run.webfetch_calls,
        retry_count: run.retry_count,
        status: run.status.clone().unwrap_or_else(|| "unknown".to_string()),
        failure_reason: run.failure_reason.as_deref().map(redact_text),
        artifact_count,
        artifact_total_bytes,
        memory_enabled,
        notion_sync_status: "not_configured".to_string(),
        notion_page_id: None,
        created_at: crate::now_ms(),
    }
}

pub fn estimate_cost_usd(
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    catalog: &[BenchmarkPriceEntry],
) -> Option<f64> {
    let entry = catalog.iter().find(|entry| {
        entry.provider.eq_ignore_ascii_case(provider) && entry.model.eq_ignore_ascii_case(model)
    })?;
    let input_cost = (input_tokens as f64 / 1_000_000.0) * entry.input_price_per_1m_tokens;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * entry.output_price_per_1m_tokens;
    Some(round_cost(input_cost + output_cost))
}

fn round_cost(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

async fn sync_summary_to_notion(
    state: &AppState,
    summary: &BenchmarkRunSummary,
    summary_path: &Path,
) -> NotionSyncResult {
    let Some(database_id) = state_benchmark_notion_database_id() else {
        return NotionSyncResult {
            status: "not_configured".to_string(),
            page_id: None,
            error: Some("TANDEM_BENCHMARK_NOTION_DATABASE_ID is not set".to_string()),
        };
    };
    let Some(tool) = discover_notion_create_page_tool(&state.mcp.list_tools().await) else {
        return NotionSyncResult {
            status: "missing_tool".to_string(),
            page_id: None,
            error: Some(
                "connected Notion MCP server does not expose a recognizable create-page tool"
                    .to_string(),
            ),
        };
    };
    let payload = notion_row_payload(&database_id, summary, summary_path);
    match state
        .mcp
        .call_tool(&tool.server_name, &tool.tool_name, payload)
        .await
    {
        Ok(result) => NotionSyncResult {
            status: "synced".to_string(),
            page_id: extract_notion_page_id(&result.metadata).or_else(|| {
                serde_json::from_str::<Value>(&result.output)
                    .ok()
                    .and_then(|value| extract_notion_page_id(&value))
            }),
            error: None,
        },
        Err(error) => NotionSyncResult {
            status: "failed".to_string(),
            page_id: None,
            error: Some(redact_text(&error)),
        },
    }
}

fn state_benchmark_notion_database_id() -> Option<String> {
    env_non_empty("TANDEM_BENCHMARK_NOTION_DATABASE_ID")
}

fn discover_notion_create_page_tool(
    tools: &[tandem_runtime::McpRemoteTool],
) -> Option<tandem_runtime::McpRemoteTool> {
    let mut candidates = tools
        .iter()
        .filter(|tool| {
            let combined = format!(
                "{} {} {}",
                tool.server_name, tool.tool_name, tool.namespaced_name
            )
            .to_ascii_lowercase();
            combined.contains("notion") && combined.contains("page") && combined.contains("create")
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|tool| {
        let name = tool.namespaced_name.to_ascii_lowercase();
        if name.ends_with(".create_page") || name.ends_with(".pages_create") {
            0
        } else {
            1
        }
    });
    candidates.into_iter().next()
}

pub fn notion_row_payload(
    database_id: &str,
    summary: &BenchmarkRunSummary,
    summary_path: &Path,
) -> Value {
    let mut properties = Map::new();
    properties.insert("Run Name".to_string(), notion_title(&run_title(summary)));
    if let Some(date) = summary
        .finished_at
        .or(summary.started_at)
        .and_then(ms_to_rfc3339)
    {
        properties.insert("Date".to_string(), json!({ "date": { "start": date } }));
    }
    properties.insert("Run ID".to_string(), notion_text(&summary.run_id));
    properties.insert(
        "Suite ID".to_string(),
        notion_text(summary.benchmark_suite_id.as_deref().unwrap_or("")),
    );
    properties.insert(
        "Task Pack".to_string(),
        notion_text(summary.task_pack_id.as_deref().unwrap_or("")),
    );
    properties.insert(
        "Task Name".to_string(),
        notion_text(summary.task_name.as_deref().unwrap_or("")),
    );
    properties.insert(
        "Provider".to_string(),
        notion_select(summary.provider.as_deref().unwrap_or("unknown")),
    );
    properties.insert(
        "Model".to_string(),
        notion_text(summary.model.as_deref().unwrap_or("unknown")),
    );
    properties.insert("Status".to_string(), notion_select(&summary.status));
    insert_number(&mut properties, "Duration ms", summary.duration_ms);
    insert_number(&mut properties, "Input Tokens", Some(summary.input_tokens));
    insert_number(
        &mut properties,
        "Output Tokens",
        Some(summary.output_tokens),
    );
    insert_number(&mut properties, "Total Tokens", Some(summary.total_tokens));
    properties.insert(
        "Usage Source".to_string(),
        notion_select(&summary.usage_source),
    );
    if let Some(cost) = summary.estimated_cost_usd {
        properties.insert("Estimated Cost USD".to_string(), json!({ "number": cost }));
    }
    insert_number(
        &mut properties,
        "Tool Calls",
        Some(summary.tool_call_counts_by_name.values().copied().sum()),
    );
    insert_number(
        &mut properties,
        "MCP Calls",
        Some(summary.mcp_call_counts_by_name.values().copied().sum()),
    );
    insert_number(
        &mut properties,
        "Webfetch Calls",
        Some(summary.webfetch_calls),
    );
    insert_number(&mut properties, "Retries", Some(summary.retry_count));
    properties.insert(
        "Failure Reason".to_string(),
        notion_text(summary.failure_reason.as_deref().unwrap_or("")),
    );
    insert_number(
        &mut properties,
        "Artifact Count",
        Some(summary.artifact_count),
    );
    insert_number(
        &mut properties,
        "Artifact Bytes",
        Some(summary.artifact_total_bytes),
    );
    if let Some(memory_enabled) = summary.memory_enabled {
        properties.insert(
            "Memory Enabled".to_string(),
            json!({ "checkbox": memory_enabled }),
        );
    }
    properties.insert(
        "Pricing Catalog Version".to_string(),
        notion_text(&summary.pricing_catalog_version),
    );
    properties.insert(
        "Local Summary Path".to_string(),
        notion_text(&summary_path.display().to_string()),
    );
    if let Some(date) = ms_to_rfc3339(crate::now_ms()) {
        properties.insert(
            "Synced At".to_string(),
            json!({ "date": { "start": date } }),
        );
    }

    json!({
        "parent": {
            "database_id": database_id
        },
        "properties": Value::Object(properties)
    })
}

fn notion_title(text: &str) -> Value {
    json!({ "title": [{ "text": { "content": redact_text(text) } }] })
}

fn notion_text(text: &str) -> Value {
    let redacted = redact_text(text);
    if redacted.is_empty() {
        json!({ "rich_text": [] })
    } else {
        json!({ "rich_text": [{ "text": { "content": redacted } }] })
    }
}

fn notion_select(text: &str) -> Value {
    json!({ "select": { "name": truncate_for_notion_select(text) } })
}

fn insert_number(properties: &mut Map<String, Value>, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        properties.insert(name.to_string(), json!({ "number": value }));
    }
}

fn run_title(summary: &BenchmarkRunSummary) -> String {
    let task = summary
        .task_name
        .as_deref()
        .or(summary.task_pack_id.as_deref())
        .unwrap_or("Benchmark Run");
    let provider = summary.provider.as_deref().unwrap_or("unknown");
    let model = summary.model.as_deref().unwrap_or("unknown");
    format!("{task} / {provider} / {model}")
}

pub fn apply_notion_sync_result(summary: &mut BenchmarkRunSummary, result: NotionSyncResult) {
    summary.notion_sync_status = result.status;
    summary.notion_page_id = result.page_id;
}

fn initial_notion_status(config: &BenchmarkConfig) -> String {
    if !config.notion_sync_enabled {
        return "disabled".to_string();
    }
    if config.notion_database_id.is_none() {
        return "not_configured".to_string();
    }
    match config.notion_sync_mode {
        NotionSyncMode::Manual => "pending_manual".to_string(),
        NotionSyncMode::Automatic => "pending".to_string(),
    }
}

fn extract_notion_page_id(value: &Value) -> Option<String> {
    for pointer in [
        "/id",
        "/page_id",
        "/pageId",
        "/result/id",
        "/result/page_id",
    ] {
        if let Some(id) = value.pointer(pointer).and_then(Value::as_str) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub fn validate_harness_request(request: &BenchmarkHarnessRequest) -> Result<(), String> {
    if request.task_pack_id.trim() != "daily_research_report" {
        return Err("MVP 1 only supports daily_research_report".to_string());
    }
    if request.provider.trim().is_empty() {
        return Err("provider is required".to_string());
    }
    if request.model.trim().is_empty() {
        return Err("model is required".to_string());
    }
    if !(1..=3).contains(&request.repeat_count) {
        return Err("repeat_count must be between 1 and 3 for MVP 1".to_string());
    }
    Ok(())
}

fn terminal_status(event: &EngineEvent) -> String {
    if let Some(status) = string_property(&event.properties, "status") {
        return status;
    }
    match event.event_type.as_str() {
        "session.run.finished" => {
            string_property(&event.properties, "status").unwrap_or_else(|| "completed".to_string())
        }
        "routine.run.completed" => "completed".to_string(),
        "routine.run.failed" | "automation_v2.run.failed" => "failed".to_string(),
        "routine.run.paused" => "paused".to_string(),
        _ => "unknown".to_string(),
    }
}

fn run_id_for_event(
    properties: &Value,
    session_run_ids: &HashMap<String, String>,
) -> Option<String> {
    if let Some(correlation_id) = string_property(properties, "correlationID") {
        if let Some(run_id) = correlation_id.strip_prefix("routine:") {
            return Some(run_id.to_string());
        }
        if let Some(run_id) = correlation_id.strip_prefix("automation-v2:") {
            return Some(run_id.to_string());
        }
    }
    string_property(properties, "runID")
        .or_else(|| string_property(properties, "run_id"))
        .or_else(|| {
            let session_id = string_property(properties, "sessionID")?;
            session_run_ids.get(&session_id).cloned()
        })
}

fn apply_optional_metadata(run: &mut RunAccumulator, properties: &Value) {
    if run.benchmark_suite_id.is_none() {
        run.benchmark_suite_id = string_property(properties, "benchmarkSuiteID")
            .or_else(|| string_property(properties, "benchmark_suite_id"));
    }
    if run.task_name.is_none() {
        run.task_name = string_property(properties, "taskName")
            .or_else(|| string_property(properties, "task_name"));
    }
    if run.task_pack_id.is_none() {
        run.task_pack_id = string_property(properties, "taskPackID")
            .or_else(|| string_property(properties, "task_pack_id"));
    }
}

fn merge_usage_source(existing: Option<&str>, incoming: &str) -> String {
    match existing {
        None => incoming.to_string(),
        Some(current) if current == incoming => current.to_string(),
        Some("provider") if incoming == "estimated" => "mixed".to_string(),
        Some("estimated") if incoming == "provider" => "mixed".to_string(),
        Some("mixed") => "mixed".to_string(),
        Some(current) => current.to_string(),
    }
}

fn increment_count(counts: &mut HashMap<String, u64>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

fn string_property(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn u64_property(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn scan_run_artifacts(workspace_root: &Path, run_id: &str) -> (u64, u64) {
    let root = workspace_root
        .join(".tandem")
        .join("runs")
        .join(sanitize_path_segment(run_id))
        .join("artifacts");
    scan_dir_bytes(&root)
}

fn scan_dir_bytes(root: &Path) -> (u64, u64) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    let mut count = 0_u64;
    let mut bytes = 0_u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                let (nested_count, nested_bytes) = scan_dir_bytes(&path);
                count = count.saturating_add(nested_count);
                bytes = bytes.saturating_add(nested_bytes);
            } else if meta.is_file() {
                count = count.saturating_add(1);
                bytes = bytes.saturating_add(meta.len());
            }
        }
    }
    (count, bytes)
}

fn sanitize_path_segment(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

pub fn redact_text(raw: &str) -> String {
    let mut out = raw.trim().replace('\n', " ");
    for marker in ["sk-", "xoxb-", "ghp_", "github_pat_", "Bearer "] {
        while let Some(index) = out.find(marker) {
            let search_start = index + marker.len();
            let end = out[search_start..]
                .find(char::is_whitespace)
                .map(|offset| search_start + offset)
                .unwrap_or(out.len());
            out.replace_range(index..end, "[redacted]");
        }
    }
    if out.len() > 500 {
        out.truncate(500);
    }
    out
}

fn truncate_for_notion_select(raw: &str) -> String {
    let mut value = redact_text(raw);
    if value.is_empty() {
        value = "unknown".to_string();
    }
    if value.len() > 100 {
        value.truncate(100);
    }
    value
}

fn ms_to_rfc3339(ms: u64) -> Option<String> {
    Utc.timestamp_millis_opt(ms as i64)
        .single()
        .map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_summary() -> BenchmarkRunSummary {
        BenchmarkRunSummary {
            schema_version: BENCHMARK_SUMMARY_SCHEMA_VERSION,
            run_id: "run-123".to_string(),
            benchmark_suite_id: Some("suite-1".to_string()),
            task_name: Some("Daily Research Report".to_string()),
            task_pack_id: Some("daily_research_report".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4.1-mini".to_string()),
            started_at: Some(1_778_070_000_000),
            finished_at: Some(1_778_070_032_000),
            duration_ms: Some(32_000),
            input_tokens: 12_000,
            output_tokens: 2_200,
            total_tokens: 14_200,
            usage_source: "provider".to_string(),
            estimated_cost_usd: Some(0.00832),
            pricing_catalog_version: BENCHMARK_PRICING_CATALOG_VERSION.to_string(),
            tool_call_counts_by_name: HashMap::from([
                ("webfetch".to_string(), 4),
                ("write".to_string(), 1),
            ]),
            mcp_call_counts_by_name: HashMap::new(),
            webfetch_calls: 4,
            retry_count: 0,
            status: "completed".to_string(),
            failure_reason: None,
            artifact_count: 1,
            artifact_total_bytes: 18_432,
            memory_enabled: Some(false),
            notion_sync_status: "not_configured".to_string(),
            notion_page_id: None,
            created_at: 1_778_070_032_500,
        }
    }

    #[test]
    fn summary_serializes_expected_fields() {
        let summary = sample_summary();
        let value = serde_json::to_value(&summary).expect("serialize summary");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["run_id"], "run-123");
        assert_eq!(value["tool_call_counts_by_name"]["webfetch"], 4);
    }

    #[test]
    fn provider_usage_event_updates_accumulator() {
        let config = BenchmarkConfig {
            profiling_enabled: true,
            notion_sync_enabled: false,
            notion_database_id: None,
            notion_sync_mode: NotionSyncMode::Manual,
            benchmark_suite_id: None,
            task_name: None,
            task_pack_id: None,
        };
        let mut run = RunAccumulator::new("run-123", &config);
        let event = EngineEvent::new(
            "provider.usage",
            json!({
                "providerID": "openai",
                "modelID": "gpt-4.1-mini",
                "promptTokens": 10,
                "completionTokens": 5,
                "totalTokens": 15,
                "usageSource": "provider"
            }),
        );
        let mut session_map = HashMap::new();
        session_map.insert("s".to_string(), "run-123".to_string());
        apply_optional_metadata(&mut run, &event.properties);
        run.provider = string_property(&event.properties, "providerID");
        run.model = string_property(&event.properties, "modelID");
        run.input_tokens = u64_property(&event.properties, "promptTokens").unwrap();
        run.output_tokens = u64_property(&event.properties, "completionTokens").unwrap();
        run.total_tokens = u64_property(&event.properties, "totalTokens").unwrap();
        run.usage_source = string_property(&event.properties, "usageSource");
        let summary = build_summary(&run, 0, 0);
        assert_eq!(summary.provider.as_deref(), Some("openai"));
        assert_eq!(summary.model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(summary.total_tokens, 15);
        assert_eq!(summary.usage_source, "provider");
    }

    #[test]
    fn missing_provider_usage_source_can_be_estimated() {
        assert_eq!(merge_usage_source(None, "estimated"), "estimated");
        assert_eq!(merge_usage_source(Some("provider"), "estimated"), "mixed");
    }

    #[test]
    fn benchmark_cost_uses_catalog_and_unknown_is_none() {
        let catalog = [BenchmarkPriceEntry {
            provider: "test",
            model: "model",
            input_price_per_1m_tokens: 2.0,
            output_price_per_1m_tokens: 10.0,
        }];
        assert_eq!(
            estimate_cost_usd("test", "model", 1_000_000, 500_000, &catalog),
            Some(7.0)
        );
        assert_eq!(estimate_cost_usd("test", "missing", 1, 1, &catalog), None);
    }

    #[test]
    fn notion_payload_maps_redacted_summary_fields() {
        let mut summary = sample_summary();
        summary.failure_reason = Some("failed with Bearer secret-token".to_string());
        let payload = notion_row_payload("db-1", &summary, Path::new("/tmp/summary.json"));
        assert_eq!(payload["parent"]["database_id"], "db-1");
        assert_eq!(payload["properties"]["Total Tokens"]["number"], 14_200);
        let failure = payload["properties"]["Failure Reason"]["rich_text"][0]["text"]["content"]
            .as_str()
            .unwrap();
        assert!(failure.contains("[redacted]"));
        assert!(!failure.contains("secret-token"));
    }

    #[test]
    fn redaction_removes_common_secret_prefixes() {
        let redacted = redact_text("token sk-secret and github_pat_abc");
        assert!(redacted.contains("[redacted]"));
        assert!(!redacted.contains("sk-secret"));
        assert!(!redacted.contains("github_pat_abc"));
    }

    #[test]
    fn sync_status_persists_success_or_failure() {
        let mut summary = sample_summary();
        apply_notion_sync_result(
            &mut summary,
            NotionSyncResult {
                status: "synced".to_string(),
                page_id: Some("page-1".to_string()),
                error: None,
            },
        );
        assert_eq!(summary.notion_sync_status, "synced");
        assert_eq!(summary.notion_page_id.as_deref(), Some("page-1"));

        apply_notion_sync_result(
            &mut summary,
            NotionSyncResult {
                status: "failed".to_string(),
                page_id: None,
                error: Some("boom".to_string()),
            },
        );
        assert_eq!(summary.notion_sync_status, "failed");
        assert_eq!(summary.notion_page_id, None);
    }

    #[test]
    fn harness_accepts_only_daily_research_report_with_small_repeat_count() {
        assert!(validate_harness_request(&BenchmarkHarnessRequest {
            task_pack_id: "daily_research_report".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            repeat_count: 3,
        })
        .is_ok());
        assert!(validate_harness_request(&BenchmarkHarnessRequest {
            task_pack_id: "github_issue_triage".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            repeat_count: 1,
        })
        .is_err());
        assert!(validate_harness_request(&BenchmarkHarnessRequest {
            task_pack_id: "daily_research_report".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            repeat_count: 4,
        })
        .is_err());
    }
}
