use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use std::{fs, io::Read, io::Write};

use anyhow::Context;
use chrono::{Datelike, TimeZone, Utc};
use clap::{Parser, Subcommand};
use flate2::{write::GzEncoder, Compression};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::{
    build_mode_permission_rules, load_or_create_engine_api_token, load_provider_auth,
    resolve_shared_paths, AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus,
    PermissionAction, PermissionManager, PluginRegistry, Storage, DEFAULT_ENGINE_HOST,
    DEFAULT_ENGINE_PORT,
};
use tandem_memory::{
    db::MemoryDatabase,
    import_files,
    types::{MemoryImportFormat, MemoryImportRequest, MemoryTier},
    MemoryManager,
};
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_server::{
    detect_host_runtime_context, install_browser_sidecar, serve, AppState, AutomationRunStatus,
    AutomationV2RunRecord, AutomationV2Spec, BrowserSidecarInstallResult, BrowserSubsystem,
    RuntimeState,
};
use tandem_tools::ToolRegistry;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

use tandem_providers::ProviderRegistry;

const SUPPORTED_PROVIDER_IDS: [&str; 12] = [
    "openai",
    "openrouter",
    "anthropic",
    "ollama",
    "groq",
    "mistral",
    "together",
    "azure",
    "bedrock",
    "vertex",
    "copilot",
    "cohere",
];

const ENGINE_CLI_EXAMPLES: &str = r#"Examples:
  tandem-engine serve --hostname 127.0.0.1 --port 39731
  tandem-engine status --hostname 127.0.0.1 --port 39731
  tandem-engine run "Summarize this repository" --provider openrouter --model openai/gpt-4o-mini
  tandem-engine tool --json @payload.json
  cat payload.json | tandem-engine tool --json -
  tandem-engine providers
"#;

const STATUS_EXAMPLES: &str = r#"Examples:
  tandem-engine status
  tandem-engine status --hostname 127.0.0.1 --port 39731
"#;

const SERVE_EXAMPLES: &str = r#"Examples:
  tandem-engine serve
  tandem-engine serve --hostname 0.0.0.0 --port 39731
  tandem-engine serve --state-dir .tandem-test --provider openrouter --model openai/gpt-4o-mini
  tandem-engine serve --disable-embeddings
"#;

const RUN_EXAMPLES: &str = r#"Examples:
  tandem-engine run "Write a short status update"
  tandem-engine run "Summarize docs/ENGINE_TESTING.md" --provider openai --model gpt-4o-mini
"#;

const PARALLEL_EXAMPLES: &str = r#"Examples:
  tandem-engine parallel --json @tasks.json --concurrency 3
  tandem-engine parallel --json "[{\"prompt\":\"Summarize README.md\"},{\"prompt\":\"List likely regressions\"}]"

`--json` accepts:
  - array of prompt strings
  - array of objects: {\"id\":\"task-1\",\"prompt\":\"...\",\"provider\":\"openrouter\"}
  - object wrapper: {\"tasks\":[...]}
  - @path/to/file.json
  - - (read JSON from stdin)
"#;

const TOOL_EXAMPLES: &str = r#"Examples:
  tandem-engine tool --json "{\"tool\":\"workspace_list_files\",\"args\":{\"path\":\".\"}}"
  tandem-engine tool --json @payload.json
  cat payload.json | tandem-engine tool --json -

`--json` accepts:
  - raw JSON string
  - @path/to/file.json
  - - (read JSON from stdin)
"#;

const TOKEN_EXAMPLES: &str = r#"Examples:
  tandem-engine token generate
"#;

const BROWSER_EXAMPLES: &str = r#"Examples:
  tandem-engine browser status
  tandem-engine browser status --hostname 127.0.0.1 --port 39731
  tandem-engine browser doctor --json
  tandem-engine browser install
  tandem-engine browser doctor --state-dir .tandem-test
"#;

const STORAGE_EXAMPLES: &str = r#"Examples:
  tandem-engine storage doctor
  tandem-engine storage doctor --json
  tandem-engine storage cleanup --dry-run --context-runs --default-knowledge --json
  tandem-engine storage cleanup --quarantine --json
"#;

const MEMORY_EXAMPLES: &str = r#"Examples:
  tandem-engine memory import --path ~/.openclaw --format openclaw
  tandem-engine memory import --path ./notes --tier global
  tandem-engine memory import --path ./docs --tier project --project-id repo-123 --sync-deletes
"#;

const DEFAULT_KNOWLEDGE_SOURCE_PREFIX: &str = "guide_docs:";

#[derive(Parser, Debug)]
#[command(name = "tandem-engine")]
#[command(version)]
#[command(about = "Headless Tandem AI backend")]
#[command(
    long_about = "Headless Tandem AI backend.\n\nUse `serve` for the HTTP/SSE runtime, `run` for one-shot prompts, and `tool` for direct tool execution."
)]
#[command(after_help = ENGINE_CLI_EXAMPLES)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Check engine health status (GET /global/health).")]
    #[command(after_help = STATUS_EXAMPLES)]
    Status {
        #[arg(
            long,
            env = "TANDEM_ENGINE_HOST",
            alias = "host",
            default_value = DEFAULT_ENGINE_HOST,
            help = "Hostname or IP address to check."
        )]
        hostname: String,
        #[arg(
            long,
            env = "TANDEM_ENGINE_PORT",
            default_value_t = DEFAULT_ENGINE_PORT,
            help = "Port to check."
        )]
        port: u16,
    },
    #[command(
        about = "Start the HTTP/SSE engine server (recommended for desktop/TUI integration)."
    )]
    #[command(after_help = SERVE_EXAMPLES)]
    Serve {
        #[arg(
            long,
            env = "TANDEM_ENGINE_HOST",
            alias = "host",
            default_value = DEFAULT_ENGINE_HOST,
            help = "Hostname or IP address to bind."
        )]
        hostname: String,
        #[arg(
            long,
            env = "TANDEM_ENGINE_PORT",
            default_value_t = DEFAULT_ENGINE_PORT,
            help = "Port to bind."
        )]
        port: u16,
        #[arg(
            long,
            help = "Engine state directory. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Run engine loop in-process for debug/testing."
        )]
        in_process: bool,
        #[arg(long, help = "Provider API key override for this process.")]
        api_key: Option<String>,
        #[arg(
            long,
            help = "Default provider override (see `tandem-engine providers`)."
        )]
        provider: Option<String>,
        #[arg(long, help = "Default model override for the selected provider.")]
        model: Option<String>,
        #[arg(long, help = "Path to config JSON override.")]
        config: Option<String>,
        #[arg(
            long,
            env = "TANDEM_API_TOKEN",
            help = "Set the API token for HTTP endpoints (Authorization: Bearer <token>, X-Agent-Token, or X-Tandem-Token). If omitted, a shared token is loaded or generated by default."
        )]
        api_token: Option<String>,
        #[arg(
            long = "unsafe-no-api-token",
            env = "TANDEM_UNSAFE_NO_API_TOKEN",
            default_value_t = false,
            help = "Advanced/unsafe: disable HTTP API token auth. Only use for trusted local development."
        )]
        unsafe_no_api_token: bool,
        #[arg(
            long,
            env = "TANDEM_WEB_UI",
            default_value_t = false,
            help = "Enable embedded web admin UI."
        )]
        web_ui: bool,
        #[arg(
            long,
            env = "TANDEM_WEB_UI_PREFIX",
            default_value = "/admin",
            help = "Path prefix where embedded web admin UI is served."
        )]
        web_ui_prefix: String,
        #[arg(
            long,
            env = "TANDEM_DISABLE_EMBEDDINGS",
            default_value_t = false,
            help = "Disable semantic memory embeddings for this engine process."
        )]
        disable_embeddings: bool,
    },
    #[command(about = "Run one prompt and print only the assistant response.")]
    #[command(after_help = RUN_EXAMPLES)]
    Run {
        #[arg(help = "Prompt text to execute.")]
        prompt: String,
        #[arg(long, help = "Provider API key override for this run.")]
        api_key: Option<String>,
        #[arg(
            long,
            help = "Default provider override (see `tandem-engine providers`)."
        )]
        provider: Option<String>,
        #[arg(long, help = "Default model override for the selected provider.")]
        model: Option<String>,
        #[arg(long, help = "Path to config JSON override.")]
        config: Option<String>,
    },
    #[command(about = "Run multiple prompts concurrently and print a JSON result summary.")]
    #[command(after_help = PARALLEL_EXAMPLES)]
    Parallel {
        #[arg(long, help = "Task payload as JSON string, @file, or - for stdin.")]
        json: String,
        #[arg(long, default_value_t = 4, help = "Maximum concurrent tasks (1-32).")]
        concurrency: usize,
        #[arg(long, help = "Provider API key override for this batch.")]
        api_key: Option<String>,
        #[arg(
            long,
            help = "Default provider for tasks without an explicit provider."
        )]
        provider: Option<String>,
        #[arg(
            long,
            help = "Default model override for provider config used by this batch."
        )]
        model: Option<String>,
        #[arg(long, help = "Path to config JSON override.")]
        config: Option<String>,
    },
    #[command(about = "Planned interactive REPL mode (currently a placeholder).")]
    Chat,
    #[command(about = "Execute a single built-in tool call using JSON input.")]
    #[command(after_help = TOOL_EXAMPLES)]
    Tool {
        #[arg(long, help = "Tool payload as raw JSON, @file, or - for stdin.")]
        json: String,
        #[arg(
            long,
            help = "Engine state directory. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
    },
    #[command(about = "List supported provider IDs for --provider.")]
    Providers,
    #[command(about = "API token utilities.")]
    Token {
        #[command(subcommand)]
        action: TokenCommand,
    },
    #[command(about = "Browser readiness and diagnostics.")]
    #[command(after_help = BROWSER_EXAMPLES)]
    Browser {
        #[command(subcommand)]
        action: BrowserCommand,
    },
    #[command(about = "Inspect and repair local Tandem storage files.")]
    #[command(after_help = STORAGE_EXAMPLES)]
    Storage {
        #[command(subcommand)]
        action: StorageCommand,
    },
    #[command(about = "Memory import utilities.")]
    #[command(after_help = MEMORY_EXAMPLES)]
    Memory {
        #[command(subcommand)]
        action: MemoryCommand,
    },
}

#[derive(Subcommand, Debug)]
enum TokenCommand {
    #[command(about = "Generate a random API token string.")]
    #[command(after_help = TOKEN_EXAMPLES)]
    Generate,
}

#[derive(Subcommand, Debug)]
enum StorageCommand {
    #[command(about = "Inspect local Tandem storage size and legacy-file candidates.")]
    Doctor {
        #[arg(
            long,
            help = "Engine data directory or Tandem root. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    #[command(about = "Migrate/shrink local storage and quarantine legacy files.")]
    Cleanup {
        #[arg(
            long,
            help = "Engine data directory or Tandem root. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Move superseded legacy/temp files into backups/local-cleanup-*."
        )]
        quarantine: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Report actions without changing files."
        )]
        dry_run: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Migrate root-level feature JSON files into data/<feature>/."
        )]
        root_json: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Archive stale context run directories."
        )]
        context_runs: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Remove legacy embedded docs seed data from memory and state files."
        )]
        default_knowledge: bool,
        #[arg(
            long,
            default_value_t = 7,
            help = "Hot retention window for automation/context run cleanup."
        )]
        retention_days: u64,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum BrowserCommand {
    #[command(about = "Check browser readiness via the running engine (GET /browser/status).")]
    Status {
        #[arg(
            long,
            env = "TANDEM_ENGINE_HOST",
            alias = "host",
            default_value = DEFAULT_ENGINE_HOST,
            help = "Hostname or IP address to check."
        )]
        hostname: String,
        #[arg(
            long,
            env = "TANDEM_ENGINE_PORT",
            default_value_t = DEFAULT_ENGINE_PORT,
            help = "Port to check."
        )]
        port: u16,
    },
    #[command(
        about = "Run local browser readiness diagnostics using the effective engine config."
    )]
    Doctor {
        #[arg(
            long,
            help = "Engine state directory. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
        #[arg(long, help = "Path to config JSON override.")]
        config: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    #[command(about = "Install the matching tandem-browser sidecar on this engine host.")]
    Install {
        #[arg(
            long,
            help = "Engine state directory. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
        #[arg(long, help = "Path to config JSON override.")]
        config: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum MemoryCommand {
    #[command(
        about = "Import OpenClaw memory files or a markdown/text directory into Tandem memory."
    )]
    Import {
        #[arg(long, help = "Path to an OpenClaw root or directory to import.")]
        path: String,
        #[arg(
            long,
            default_value = "directory",
            help = "Import format: `directory` or `openclaw`."
        )]
        format: String,
        #[arg(
            long,
            default_value = "global",
            help = "Memory tier target: `global`, `project`, or `session`."
        )]
        tier: String,
        #[arg(long, help = "Project scope required when --tier project.")]
        project_id: Option<String>,
        #[arg(long, help = "Session scope required when --tier session.")]
        session_id: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Delete imported records whose source files no longer exist in this import root."
        )]
        sync_deletes: bool,
        #[arg(
            long,
            help = "Engine state directory. If omitted, uses TANDEM_STATE_DIR or the shared Tandem path."
        )]
        state_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status { hostname, port } => {
            let url = format!("http://{hostname}:{port}/global/health");
            let resp = reqwest::Client::new().get(&url).send().await?;
            let status = resp.status();
            let body = resp.text().await?;
            if !status.is_success() {
                anyhow::bail!("engine health check failed: {} {}", status, body);
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{body}");
            }
        }
        Command::Serve {
            hostname,
            port,
            state_dir,
            in_process,
            api_key,
            provider,
            model,
            config,
            api_token,
            unsafe_no_api_token,
            web_ui,
            web_ui_prefix,
            disable_embeddings,
        } => {
            if disable_embeddings {
                std::env::set_var("TANDEM_DISABLE_EMBEDDINGS", "1");
                info!("semantic embeddings disabled by CLI/env flag");
            } else {
                std::env::remove_var("TANDEM_DISABLE_EMBEDDINGS");
            }
            let provider = normalize_and_validate_provider(provider)?;
            let overrides = build_cli_overrides(api_key, provider, model)?;
            let state_dir = resolve_state_dir(state_dir);
            // Canonical logs must be shared across desktop/engine/tui.
            // If shared path resolution fails, fall back to state-dir-local logs.
            let logs_dir = resolve_shared_paths()
                .map(|p| canonical_logs_dir_from_root(&p.canonical_root))
                .unwrap_or_else(|_| canonical_logs_dir_from_root(&state_dir));
            let (_log_guard, log_info) = init_process_logging(ProcessKind::Engine, &logs_dir, 14)?;
            emit_event(
                tracing::Level::INFO,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "logging.initialized",
                    component: "engine.main",
                    correlation_id: None,
                    session_id: None,
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("ok"),
                    error_code: None,
                    detail: Some("engine jsonl logging initialized"),
                },
            );
            info!("engine logging initialized: {:?}", log_info);
            let build = tandem_server::build_provenance();
            tracing::info!(
                version = %build.version,
                build_id = %build.build_id,
                git_sha = ?build.git_sha,
                binary_path = ?build.binary_path,
                binary_modified_at_ms = ?build.binary_modified_at_ms,
                "engine build provenance"
            );
            let startup_attempt_id = Uuid::new_v4().to_string();
            let state = AppState::new_starting(startup_attempt_id.clone(), in_process);
            state.configure_web_ui(web_ui, web_ui_prefix);
            if let Some(token) = resolve_engine_api_token(api_token, unsafe_no_api_token)? {
                info!("API token auth enabled for tandem-engine HTTP API");
                state.set_api_token(Some(token)).await;
            } else {
                tracing::warn!(
                    "API token auth disabled for tandem-engine HTTP API by explicit unsafe flag"
                );
            }
            let addr: SocketAddr = format!("{hostname}:{port}")
                .parse()
                .context("invalid hostname or port")?;
            let internal_host = if hostname == "0.0.0.0" {
                "127.0.0.1".to_string()
            } else {
                hostname.clone()
            };
            state.set_server_base_url(format!("http://{internal_host}:{port}"));
            log_startup_paths(&state_dir, &addr, &startup_attempt_id);
            let init_state = state.clone();
            let init_state_dir = state_dir.clone();
            let init_overrides = overrides.clone();
            let init_config_path = config.map(PathBuf::from);

            tokio::spawn(async move {
                if let Err(err) = initialize_runtime(
                    init_state.clone(),
                    init_state_dir,
                    init_overrides,
                    init_config_path,
                )
                .await
                {
                    let err_text = err.to_string();
                    init_state
                        .mark_failed("runtime_init", err_text.clone())
                        .await;
                    emit_event(
                        tracing::Level::ERROR,
                        ProcessKind::Engine,
                        ObservabilityEvent {
                            event: "engine.startup.failed",
                            component: "engine.main",
                            correlation_id: None,
                            session_id: None,
                            run_id: None,
                            message_id: None,
                            provider_id: None,
                            model_id: None,
                            status: Some("failed"),
                            error_code: Some("ENGINE_STARTUP_FAILED"),
                            detail: Some(&format!(
                                "attempt_id={} phase=runtime_init error={}",
                                startup_attempt_id, err_text
                            )),
                        },
                    );
                    tracing::error!(
                        "Engine runtime initialization failed (attempt_id={}): {}",
                        startup_attempt_id,
                        err_text
                    );
                }
            });
            serve(addr, state).await?;
        }
        Command::Run {
            prompt,
            api_key,
            provider,
            model,
            config,
        } => {
            let provider = normalize_and_validate_provider(provider)?;
            let overrides = build_cli_overrides(api_key, provider.clone(), model)?;
            let config_path = config.map(PathBuf::from);
            let state_dir = resolve_state_dir(None);
            let state = build_runtime(&state_dir, None, overrides, config_path).await?;
            let reply = state
                .engine_loop
                .run_oneshot_for_provider(prompt, provider.as_deref())
                .await?;
            println!("{reply}");
        }
        Command::Parallel {
            json,
            concurrency,
            api_key,
            provider,
            model,
            config,
        } => {
            let provider = normalize_and_validate_provider(provider)?;
            let overrides = build_cli_overrides(api_key, provider.clone(), model)?;
            let config_path = config.map(PathBuf::from);
            let state_dir = resolve_state_dir(None);
            let state = build_runtime(&state_dir, None, overrides, config_path).await?;
            let payload = read_json_input(&json)?;
            let tasks = parse_parallel_tasks(payload, provider)?;
            if tasks.is_empty() {
                anyhow::bail!("parallel requires at least one task");
            }

            let limit = concurrency.clamp(1, 32);
            let engine_loop = state.engine_loop.clone();
            let mut results = stream::iter(tasks.into_iter().enumerate())
                .map(|(idx, task)| {
                    let engine_loop = engine_loop.clone();
                    async move {
                        let task_id = task.id.unwrap_or_else(|| format!("task-{}", idx + 1));
                        match engine_loop
                            .run_oneshot_for_provider(task.prompt.clone(), task.provider.as_deref())
                            .await
                        {
                            Ok(output) => ParallelTaskResult {
                                index: idx,
                                id: task_id,
                                provider: task.provider,
                                status: "ok".to_string(),
                                output: Some(output),
                                error: None,
                            },
                            Err(err) => ParallelTaskResult {
                                index: idx,
                                id: task_id,
                                provider: task.provider,
                                status: "error".to_string(),
                                output: None,
                                error: Some(err.to_string()),
                            },
                        }
                    }
                })
                .buffer_unordered(limit)
                .collect::<Vec<_>>()
                .await;

            results.sort_by_key(|item| item.index);
            let failures = results.iter().filter(|item| item.status == "error").count();
            let report = serde_json::json!({
                "concurrency": limit,
                "total": results.len(),
                "failures": failures,
                "results": results,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
            if failures > 0 {
                anyhow::bail!("parallel completed with {} failed task(s)", failures);
            }
        }
        Command::Chat => {
            let _state = build_runtime(&resolve_state_dir(None), None, None, None).await?;
            println!("Interactive chat mode is planned; use `serve` for now.");
        }
        Command::Tool { json, state_dir } => {
            let state_dir = resolve_state_dir(state_dir);
            let state = build_runtime(&state_dir, None, None, None).await?;
            let payload = read_json_input(&json)?;
            let tool = payload
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = payload
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if tool.is_empty() {
                anyhow::bail!("tool is required in input json");
            }
            let result = state.tools.execute(&tool, args).await?;
            let output = serde_json::json!({
                "output": result.output,
                "metadata": result.metadata
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::Providers => {
            println!("Supported providers:");
            for provider in SUPPORTED_PROVIDER_IDS {
                println!("  - {provider}");
            }
        }
        Command::Token { action } => match action {
            TokenCommand::Generate => {
                let token = format!("tk_{}", Uuid::new_v4().simple());
                println!("{token}");
            }
        },
        Command::Browser { action } => match action {
            BrowserCommand::Status { hostname, port } => {
                let url = format!("http://{hostname}:{port}/browser/status");
                let resp = reqwest::Client::new().get(&url).send().await?;
                let status = resp.status();
                let body = resp.text().await?;
                if !status.is_success() {
                    anyhow::bail!("browser status check failed: {} {}", status, body);
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    println!("{}", serde_json::to_string_pretty(&json)?);
                } else {
                    println!("{body}");
                }
            }
            BrowserCommand::Doctor {
                state_dir,
                config,
                json,
            } => {
                let state_dir = resolve_state_dir(state_dir);
                let config_path = config.map(PathBuf::from);
                let status = browser_doctor_status(&state_dir, config_path).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                } else {
                    println!("Browser readiness");
                    println!("  Enabled: {}", status.enabled);
                    println!("  Runnable: {}", status.runnable);
                    println!(
                        "  Sidecar: {}",
                        status
                            .sidecar
                            .path
                            .clone()
                            .unwrap_or_else(|| "<not found>".to_string())
                    );
                    println!(
                        "  Browser: {}",
                        status
                            .browser
                            .path
                            .clone()
                            .unwrap_or_else(|| "<not found>".to_string())
                    );
                    if let Some(version) = status.browser.version.as_deref() {
                        println!("  Browser version: {}", version);
                    }
                    if !status.blocking_issues.is_empty() {
                        println!("Blocking issues:");
                        for issue in &status.blocking_issues {
                            println!("  - {}: {}", issue.code, issue.message);
                        }
                    }
                    if !status.recommendations.is_empty() {
                        println!("Recommendations:");
                        for row in &status.recommendations {
                            println!("  - {}", row);
                        }
                    }
                    if !status.install_hints.is_empty() {
                        println!("Install hints:");
                        for row in &status.install_hints {
                            println!("  - {}", row);
                        }
                    }
                }
            }
            BrowserCommand::Install {
                state_dir,
                config,
                json,
            } => {
                let state_dir = resolve_state_dir(state_dir);
                let config_path = config.map(PathBuf::from);
                let result = browser_install_result(&state_dir, config_path).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("Installed tandem-browser");
                    println!("  Version: {}", result.version);
                    println!("  Asset: {}", result.asset_name);
                    println!("  Path: {}", result.installed_path);
                    println!("  Downloaded bytes: {}", result.downloaded_bytes);
                    println!("  Runnable: {}", result.status.runnable);
                    if !result.status.blocking_issues.is_empty() {
                        println!("Blocking issues:");
                        for issue in &result.status.blocking_issues {
                            println!("  - {}: {}", issue.code, issue.message);
                        }
                    }
                }
            }
        },
        Command::Storage { action } => match action {
            StorageCommand::Doctor { state_dir, json } => {
                let state_dir = resolve_state_dir(state_dir);
                let report = storage_doctor_report(&state_dir)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_storage_report(&report);
                }
            }
            StorageCommand::Cleanup {
                state_dir,
                quarantine,
                dry_run,
                root_json,
                context_runs,
                default_knowledge,
                retention_days,
                json,
            } => {
                let state_dir = resolve_state_dir(state_dir);
                let report = storage_cleanup(
                    &state_dir,
                    quarantine,
                    dry_run,
                    root_json,
                    context_runs,
                    default_knowledge,
                    retention_days,
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_storage_cleanup_report(&report);
                }
            }
        },
        Command::Memory { action } => match action {
            MemoryCommand::Import {
                path,
                format,
                tier,
                project_id,
                session_id,
                sync_deletes,
                state_dir,
            } => {
                let state_dir = resolve_state_dir(state_dir);
                configure_memory_db_path_env(&state_dir);
                let manager = MemoryManager::new(&resolve_memory_db_path(&state_dir)).await?;
                let format = parse_memory_import_format(&format)?;
                let tier = parse_memory_import_tier(&tier)?;
                let stats = import_files(
                    &manager,
                    &MemoryImportRequest {
                        root_path: path.clone(),
                        format,
                        tier,
                        session_id: session_id.clone(),
                        project_id: project_id.clone(),
                        sync_deletes,
                    },
                    None::<fn(&tandem_memory::types::MemoryImportProgress)>,
                )
                .await?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "path": path,
                        "format": format.to_string(),
                        "tier": tier.to_string(),
                        "project_id": project_id,
                        "session_id": session_id,
                        "sync_deletes": sync_deletes,
                        "discovered_files": stats.discovered_files,
                        "files_processed": stats.files_processed,
                        "indexed_files": stats.indexed_files,
                        "skipped_files": stats.skipped_files,
                        "deleted_files": stats.deleted_files,
                        "chunks_created": stats.chunks_created,
                        "errors": stats.errors,
                    }))?
                );
            }
        },
    }

    Ok(())
}

fn build_cli_overrides(
    api_key: Option<String>,
    provider: Option<String>,
    model: Option<String>,
) -> anyhow::Result<Option<serde_json::Value>> {
    let provider = normalize_and_validate_provider(provider)?;

    if api_key.is_none() && provider.is_none() && model.is_none() {
        return Ok(None);
    }
    let mut root = serde_json::Map::new();

    // If provider is specified, set default_provider
    if let Some(p) = &provider {
        root.insert(
            "default_provider".to_string(),
            serde_json::Value::String(p.clone()),
        );
    }

    // Determine target provider for api_key/model overrides
    // Default to "openai" if not specified, OR use the one specified
    let target_provider = provider.as_deref().unwrap_or("openai");

    if api_key.is_some() || model.is_some() {
        let mut provider_config = serde_json::Map::new();
        if let Some(k) = api_key {
            provider_config.insert("api_key".to_string(), serde_json::Value::String(k));
        }
        if let Some(m) = model {
            provider_config.insert("default_model".to_string(), serde_json::Value::String(m));
        }

        let mut providers = serde_json::Map::new();
        providers.insert(
            target_provider.to_string(),
            serde_json::Value::Object(provider_config),
        );
        root.insert(
            "providers".to_string(),
            serde_json::Value::Object(providers),
        );
    }

    Ok(Some(serde_json::Value::Object(root)))
}

fn normalize_and_validate_provider(provider: Option<String>) -> anyhow::Result<Option<String>> {
    let Some(provider) = provider else {
        return Ok(None);
    };
    let normalized = provider.trim().to_lowercase();
    if normalized.is_empty() {
        anyhow::bail!(
            "provider cannot be empty. supported providers: {}",
            SUPPORTED_PROVIDER_IDS.join(", ")
        );
    }
    Ok(Some(normalized))
}

fn resolve_state_dir(flag: Option<String>) -> PathBuf {
    if let Some(dir) = flag {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    resolve_shared_paths()
        .map(|p| p.engine_state_dir)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        })
}

#[derive(Debug, Serialize)]
struct StorageDoctorReport {
    root: String,
    data_dir: String,
    total_candidate_bytes: u64,
    files: Vec<StorageFileReport>,
    recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StorageFileReport {
    path: String,
    bytes: u64,
    kind: String,
    action: String,
}

#[derive(Debug, Serialize)]
struct StorageCleanupReport {
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

fn resolve_storage_root(state_dir: &Path) -> PathBuf {
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

fn storage_doctor_report(state_dir: &Path) -> anyhow::Result<StorageDoctorReport> {
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

fn push_storage_file_report(
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

async fn storage_cleanup(
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
        write_string_atomic_sync(&active_path, &serde_json::to_string_pretty(&hot)?)?;
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
struct ContextRunCleanupReport {
    scanned: usize,
    archived: usize,
    stale_closed: usize,
    bytes_archived: u64,
}

#[derive(Debug, Default)]
struct DefaultKnowledgeCleanupReport {
    rows_cleared: u64,
    state_files_matched: usize,
    files_quarantined: Vec<String>,
}

fn default_knowledge_state_paths(root: &Path) -> [PathBuf; 2] {
    [
        root.join("default_knowledge_state.json"),
        root.join("data")
            .join("knowledge")
            .join("default_knowledge_state.json"),
    ]
}

async fn cleanup_default_knowledge_storage(
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

fn migrate_root_feature_storage(
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
        ("bug_monitor_config.json", "data/bug-monitor/config.json"),
        ("bug_monitor_drafts.json", "data/bug-monitor/drafts.json"),
        (
            "bug_monitor_incidents.json",
            "data/bug-monitor/incidents.json",
        ),
        ("bug_monitor_posts.json", "data/bug-monitor/posts.json"),
        (
            "failure_reporter_config.json",
            "data/bug-monitor/config.json",
        ),
        (
            "failure_reporter_drafts.json",
            "data/bug-monitor/drafts.json",
        ),
        (
            "failure_reporter_incidents.json",
            "data/bug-monitor/incidents.json",
        ),
        ("failure_reporter_posts.json", "data/bug-monitor/posts.json"),
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

fn cleanup_context_runs(
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

fn close_stale_context_run(run_dir: &Path, state: &mut serde_json::Value) -> anyhow::Result<()> {
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

fn archive_context_run(
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

fn write_context_hot_index(hot_root: &Path) -> anyhow::Result<()> {
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

fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

fn latest_jsonl_seq(path: &Path) -> u64 {
    let Ok(raw) = fs::read_to_string(path) else {
        return 0;
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("seq").and_then(serde_json::Value::as_u64))
        .max()
        .unwrap_or(0)
}

fn append_jsonl_sync(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
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

fn dir_size(path: &Path) -> anyhow::Result<u64> {
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

fn copy_dir_recursive(source: &Path, target: &Path) -> anyhow::Result<()> {
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

fn read_automation_specs_map(
    path: &Path,
) -> anyhow::Result<std::collections::HashMap<String, AutomationV2Spec>> {
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn merge_automation_runs(
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
    let parsed: std::collections::HashMap<String, AutomationV2RunRecord> =
        serde_json::from_str(&raw).unwrap_or_default();
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

fn compact_storage_hot_run(
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

fn storage_run_is_terminal(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Cancelled
    )
}

fn write_storage_run_shard(active_path: &Path, run: &AutomationV2RunRecord) -> anyhow::Result<()> {
    let path = storage_run_shard_path(active_path, run);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_string_atomic_sync(&path, &serde_json::to_string_pretty(run)?)?;
    Ok(())
}

fn storage_run_shard_path(active_path: &Path, run: &AutomationV2RunRecord) -> PathBuf {
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

fn storage_tmp_files(data_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
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

fn old_large_engine_logs(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
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

fn quarantine_file(root: &Path, quarantine_dir: &Path, path: &Path) -> anyhow::Result<()> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let target = quarantine_dir.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(path, target).with_context(|| format!("quarantine {}", path.display()))?;
    Ok(())
}

fn write_string_atomic_sync(path: &Path, payload: &str) -> anyhow::Result<()> {
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

fn now_ms_for_storage() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn print_storage_report(report: &StorageDoctorReport) {
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

fn print_storage_cleanup_report(report: &StorageCleanupReport) {
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

fn resolve_engine_api_token(
    explicit: Option<String>,
    unsafe_no_api_token: bool,
) -> anyhow::Result<Option<String>> {
    if let Some(token) = normalize_api_token(explicit) {
        return Ok(Some(token));
    }

    if unsafe_no_api_token {
        tracing::warn!(
            "tandem-engine HTTP API token auth disabled by --unsafe-no-api-token/TANDEM_UNSAFE_NO_API_TOKEN"
        );
        return Ok(None);
    }

    if let Ok(path) = std::env::var("TANDEM_API_TOKEN_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let token_path = PathBuf::from(trimmed);
            let token = fs::read_to_string(&token_path)
                .with_context(|| format!("read engine API token {}", token_path.display()))?;
            let token = normalize_api_token(Some(token)).ok_or_else(|| {
                anyhow::anyhow!("engine API token file {} is empty", token_path.display())
            })?;
            return Ok(Some(token));
        }
    }

    let token_material = load_or_create_engine_api_token();
    info!(
        "Using tandem-engine API token from {} ({})",
        token_material.backend,
        token_material.file_path.display()
    );
    Ok(Some(token_material.token))
}

fn normalize_api_token(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_json_input(input: &str) -> anyhow::Result<serde_json::Value> {
    if input.trim() == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        return Ok(serde_json::from_str(&buf)?);
    }
    if let Some(path) = input.strip_prefix('@') {
        let raw = fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&raw)?);
    }
    Ok(serde_json::from_str(input)?)
}

#[derive(Debug, Clone, Deserialize)]
struct ParallelTaskInput {
    id: Option<String>,
    prompt: String,
    provider: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ParallelTaskResult {
    #[serde(skip_serializing)]
    index: usize,
    id: String,
    provider: Option<String>,
    status: String,
    output: Option<String>,
    error: Option<String>,
}

fn parse_parallel_tasks(
    payload: serde_json::Value,
    default_provider: Option<String>,
) -> anyhow::Result<Vec<ParallelTaskInput>> {
    let parse_item = |value: &serde_json::Value| -> anyhow::Result<ParallelTaskInput> {
        match value {
            serde_json::Value::String(prompt) => {
                if prompt.trim().is_empty() {
                    anyhow::bail!("parallel task prompt cannot be empty");
                }
                Ok(ParallelTaskInput {
                    id: None,
                    prompt: prompt.clone(),
                    provider: default_provider.clone(),
                })
            }
            serde_json::Value::Object(_) => {
                let mut task: ParallelTaskInput = serde_json::from_value(value.clone())
                    .context("invalid parallel task object shape")?;
                if task.prompt.trim().is_empty() {
                    anyhow::bail!("parallel task prompt cannot be empty");
                }
                task.provider = normalize_and_validate_provider(task.provider)?;
                if task.provider.is_none() {
                    task.provider = default_provider.clone();
                }
                Ok(task)
            }
            _ => anyhow::bail!("parallel tasks must be strings or objects"),
        }
    };

    let items = match payload {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut obj) => obj
            .remove("tasks")
            .and_then(|v| v.as_array().cloned())
            .ok_or_else(|| {
                anyhow::anyhow!("parallel object payload must include a `tasks` array")
            })?,
        _ => anyhow::bail!("parallel payload must be an array or an object with `tasks`"),
    };

    items.iter().map(parse_item).collect()
}

fn parse_memory_import_format(raw: &str) -> anyhow::Result<MemoryImportFormat> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "directory" => Ok(MemoryImportFormat::Directory),
        "openclaw" => Ok(MemoryImportFormat::Openclaw),
        other => anyhow::bail!("unsupported memory import format `{other}`"),
    }
}

fn parse_memory_import_tier(raw: &str) -> anyhow::Result<MemoryTier> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "session" => Ok(MemoryTier::Session),
        "project" => Ok(MemoryTier::Project),
        "global" => Ok(MemoryTier::Global),
        other => anyhow::bail!("unsupported memory tier `{other}`"),
    }
}

fn log_startup_paths(state_dir: &Path, addr: &SocketAddr, startup_attempt_id: &str) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let config_path = state_dir.join("config.json");
    info!("starting tandem-engine on http://{addr}");
    info!(
        "startup paths: attempt_id={} exe={} cwd={} state_dir={} config_path={}",
        startup_attempt_id,
        exe.display(),
        cwd.display(),
        state_dir.display(),
        config_path.display()
    );
    if let Ok(paths) = resolve_shared_paths() {
        info!(
            "storage root: canonical={} legacy={}",
            paths.canonical_root.display(),
            paths.legacy_root.display()
        );
    }
}

async fn initialize_runtime(
    state: AppState,
    state_dir: PathBuf,
    overrides: Option<serde_json::Value>,
    config_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let startup = state.startup_snapshot().await;
    let attempt_id = startup.attempt_id;
    let init_started = Instant::now();

    let runtime = build_runtime(&state_dir, Some(&state), overrides, config_path).await?;
    state.mark_ready(runtime).await?;
    let _ = state.restart_channel_listeners().await;
    state.set_phase("ready").await;
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "engine.startup.ready",
            component: "engine.main",
            correlation_id: None,
            session_id: None,
            run_id: None,
            message_id: None,
            provider_id: None,
            model_id: None,
            status: Some("ok"),
            error_code: None,
            detail: Some(&format!(
                "attempt_id={} elapsed_ms={}",
                attempt_id,
                init_started.elapsed().as_millis()
            )),
        },
    );
    Ok(())
}

async fn build_runtime(
    state_dir: &Path,
    startup_state: Option<&AppState>,
    cli_overrides: Option<serde_json::Value>,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<RuntimeState> {
    configure_memory_db_path_env(state_dir);
    warn_on_split_storage_config(state_dir);
    let startup = Instant::now();
    if let Some(state) = startup_state {
        state.set_phase("storage_init").await;
        emit_startup_phase_event(state, "storage_init").await;
    }
    let phase_start = Instant::now();
    let storage = Arc::new(Storage::new(state_dir.join("storage")).await?);
    info!(
        "engine.startup.phase storage_init elapsed_ms={}",
        phase_start.elapsed().as_millis()
    );
    if let Some(state) = startup_state {
        state.set_phase("config_init").await;
        emit_startup_phase_event(state, "config_init").await;
    }
    let phase_start = Instant::now();
    let config_path = override_config_path.unwrap_or_else(|| state_dir.join("config.json"));
    let config = ConfigStore::new(config_path, cli_overrides).await?;
    let persisted_provider_auth = load_provider_auth();
    if !persisted_provider_auth.is_empty() {
        let mut providers = serde_json::Map::new();
        for (provider_id, token) in &persisted_provider_auth {
            providers.insert(
                provider_id.clone(),
                serde_json::json!({
                    "api_key": token
                }),
            );
        }
        let _ = config
            .patch_runtime(serde_json::json!({ "providers": providers }))
            .await;
    }
    info!(
        "engine.startup.phase config_init elapsed_ms={}",
        phase_start.elapsed().as_millis()
    );
    if let Some(state) = startup_state {
        state.set_phase("registry_init").await;
        emit_startup_phase_event(state, "registry_init").await;
    }
    let phase_start = Instant::now();
    let event_bus = EventBus::new();
    let app_config = config.get().await;
    let browser = BrowserSubsystem::new(app_config.browser.clone());
    let providers = ProviderRegistry::new(app_config.into());
    let plugins = PluginRegistry::new(".").await?;
    let agents = AgentRegistry::new(".").await?;
    let tools = ToolRegistry::new();
    {
        let tools_for_index = tools.clone();
        tokio::spawn(async move {
            tools_for_index.index_all().await;
        });
    }
    if startup_state.is_none() {
        browser.register_tools(&tools, None).await?;
    }
    let permissions = PermissionManager::new(event_bus.clone());
    apply_default_permission_rules(&permissions).await;
    let mcp = McpRegistry::new();
    let pty = PtyManager::new();
    let lsp = LspManager::new(".");
    let auth = Arc::new(RwLock::new(persisted_provider_auth));
    let logs = Arc::new(RwLock::new(Vec::new()));
    let workspace_index = WorkspaceIndex::new(".").await;
    info!(
        "engine.startup.phase registry_init elapsed_ms={}",
        phase_start.elapsed().as_millis()
    );
    if let Some(state) = startup_state {
        state.set_phase("engine_loop_init").await;
        emit_startup_phase_event(state, "engine_loop_init").await;
    }
    let phase_start = Instant::now();
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = detect_host_runtime_context();
    let engine_loop = EngineLoop::new(
        storage.clone(),
        event_bus.clone(),
        providers.clone(),
        plugins.clone(),
        agents.clone(),
        permissions.clone(),
        tools.clone(),
        cancellations.clone(),
        host_runtime_context.clone(),
    );
    info!(
        "engine.startup.phase engine_loop_init elapsed_ms={}",
        phase_start.elapsed().as_millis()
    );
    info!(
        "engine.startup.phase runtime_build_complete elapsed_ms={}",
        startup.elapsed().as_millis()
    );

    Ok(RuntimeState {
        storage,
        config,
        event_bus,
        providers,
        plugins,
        agents,
        tools,
        permissions,
        mcp,
        pty,
        lsp,
        auth,
        logs,
        workspace_index,
        cancellations,
        engine_loop,
        host_runtime_context,
        browser,
    })
}

async fn browser_doctor_status(
    state_dir: &Path,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<tandem_browser::BrowserStatus> {
    let config_path = override_config_path.unwrap_or_else(|| state_dir.join("config.json"));
    let config = ConfigStore::new(config_path, None).await?;
    let app_config = config.get().await;
    let browser = BrowserSubsystem::new(app_config.browser);
    Ok(browser.refresh_status().await)
}

async fn browser_install_result(
    state_dir: &Path,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<BrowserSidecarInstallResult> {
    let config_path = override_config_path.unwrap_or_else(|| state_dir.join("config.json"));
    let config = ConfigStore::new(config_path, None).await?;
    let app_config = config.get().await;
    install_browser_sidecar(&app_config.browser).await
}

async fn apply_default_permission_rules(permissions: &PermissionManager) {
    // Pack creation is a first-class workflow; allow invoking the builder tool by default.
    let _ = permissions
        .add_rule(
            "pack_builder".to_string(),
            "*".to_string(),
            PermissionAction::Allow,
        )
        .await;

    if !default_permission_rules_enabled() {
        info!("engine.permission.defaults disabled by TANDEM_APPLY_DEFAULT_PERMISSION_RULES");
        return;
    }
    let templates = build_mode_permission_rules(None);
    let mut applied = 0usize;
    for template in templates {
        let action = match template.action.trim().to_ascii_lowercase().as_str() {
            "allow" | "always" => PermissionAction::Allow,
            "deny" | "reject" => PermissionAction::Deny,
            _ => PermissionAction::Ask,
        };
        let _ = permissions
            .add_rule(template.permission, template.pattern, action)
            .await;
        applied = applied.saturating_add(1);
    }
    info!("engine.permission.defaults applied_rules={applied}");
}

fn default_permission_rules_enabled() -> bool {
    std::env::var("TANDEM_APPLY_DEFAULT_PERMISSION_RULES")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

fn configure_memory_db_path_env(state_dir: &Path) {
    if std::env::var("TANDEM_MEMORY_DB_PATH")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }

    let candidate = resolve_shared_paths()
        .map(|p| p.memory_db_path)
        .unwrap_or_else(|_| state_dir.join("memory.sqlite"));
    std::env::set_var("TANDEM_MEMORY_DB_PATH", candidate.as_os_str());
    info!(
        "configured TANDEM_MEMORY_DB_PATH={}",
        candidate.to_string_lossy()
    );
}

fn warn_on_split_storage_config(state_dir: &Path) {
    let Ok(raw) = std::env::var("TANDEM_MEMORY_DB_PATH") else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let configured = PathBuf::from(trimmed);
    let expected = state_dir.join("memory.sqlite");
    if configured != expected {
        tracing::warn!(
            "split storage config detected: TANDEM_STATE_DIR={} but TANDEM_MEMORY_DB_PATH={}. standard installs should keep memory.sqlite inside the same Tandem state root; prefer TANDEM_STATE_DIR alone unless you intentionally need a separate database path",
            state_dir.display(),
            configured.display()
        );
    }
}

async fn emit_startup_phase_event(state: &AppState, phase: &str) {
    let snapshot = state.startup_snapshot().await;
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "engine.startup.phase",
            component: "engine.main",
            correlation_id: None,
            session_id: None,
            run_id: None,
            message_id: None,
            provider_id: None,
            model_id: None,
            status: Some("running"),
            error_code: None,
            detail: Some(&format!(
                "attempt_id={} phase={}",
                snapshot.attempt_id, phase
            )),
        },
    );
}

fn resolve_memory_db_path(state_dir: &Path) -> PathBuf {
    if let Ok(raw) = std::env::var("TANDEM_MEMORY_DB_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    resolve_shared_paths()
        .map(|p| p.memory_db_path)
        .unwrap_or_else(|_| state_dir.join("memory.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_cli_overrides_targets_selected_provider() {
        let overrides = build_cli_overrides(
            Some("sk-test".to_string()),
            Some("openrouter".to_string()),
            Some("google/gemini-2.5-flash".to_string()),
        )
        .expect("overrides")
        .expect("some");

        assert_eq!(overrides["default_provider"], "openrouter");
        assert_eq!(
            overrides["providers"]["openrouter"]["api_key"],
            json!("sk-test")
        );
        assert_eq!(
            overrides["providers"]["openrouter"]["default_model"],
            json!("google/gemini-2.5-flash")
        );
    }

    #[test]
    fn build_cli_overrides_defaults_model_and_key_to_openai_without_provider() {
        let overrides = build_cli_overrides(
            Some("sk-test".to_string()),
            None,
            Some("gpt-4o-mini".to_string()),
        )
        .expect("overrides")
        .expect("some");

        assert!(overrides.get("default_provider").is_none());
        assert_eq!(
            overrides["providers"]["openai"]["api_key"],
            json!("sk-test")
        );
        assert_eq!(
            overrides["providers"]["openai"]["default_model"],
            json!("gpt-4o-mini")
        );
    }

    #[test]
    fn normalize_and_validate_provider_accepts_known_values_case_insensitive() {
        let provider =
            normalize_and_validate_provider(Some(" OpenRouter ".to_string())).expect("provider");
        assert_eq!(provider.as_deref(), Some("openrouter"));
    }

    #[test]
    fn normalize_and_validate_provider_accepts_custom_values() {
        let provider =
            normalize_and_validate_provider(Some(" MiniMax ".to_string())).expect("provider");
        assert_eq!(provider.as_deref(), Some("minimax"));
    }

    #[test]
    fn build_cli_overrides_accepts_custom_provider() {
        let overrides = build_cli_overrides(
            Some("sk-test".to_string()),
            Some("minimax".to_string()),
            Some("MiniMax-M2".to_string()),
        )
        .expect("overrides")
        .expect("some");

        assert_eq!(overrides["default_provider"], "minimax");
        assert_eq!(
            overrides["providers"]["minimax"]["api_key"],
            json!("sk-test")
        );
        assert_eq!(
            overrides["providers"]["minimax"]["default_model"],
            json!("MiniMax-M2")
        );
    }

    #[tokio::test]
    async fn cleanup_default_knowledge_storage_removes_seed_rows_and_state_files() {
        let root =
            std::env::temp_dir().join(format!("tandem-default-knowledge-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("data").join("knowledge")).expect("create temp root");

        let db_path = root.join("memory.sqlite");
        let db = MemoryDatabase::new(&db_path).await.expect("open memory db");
        let chunk = tandem_memory::types::MemoryChunk {
            id: "guide-doc-1".to_string(),
            content: "Guide docs seed content".to_string(),
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            source: "guide_docs:seed.md".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: Some("seed-hash".to_string()),
            created_at: Utc::now(),
            token_count: 4,
            metadata: None,
        };
        let embedding = vec![0.0f32; tandem_memory::types::DEFAULT_EMBEDDING_DIMENSION];
        db.store_chunk(&chunk, &embedding)
            .await
            .expect("store chunk");

        fs::write(root.join("default_knowledge_state.json"), "{ }\n")
            .expect("write root state file");
        fs::write(
            root.join("data")
                .join("knowledge")
                .join("default_knowledge_state.json"),
            "{ }\n",
        )
        .expect("write canonical state file");

        let report =
            cleanup_default_knowledge_storage(&root, Some(db_path.as_path()), None, false, false)
                .await
                .expect("cleanup");

        assert_eq!(report.state_files_matched, 2);
        assert_eq!(report.rows_cleared, 1);
        assert!(!root.join("default_knowledge_state.json").exists());
        assert!(!root
            .join("data")
            .join("knowledge")
            .join("default_knowledge_state.json")
            .exists());

        let reopened = MemoryDatabase::new(&db_path)
            .await
            .expect("reopen memory db");
        let cleared = reopened
            .clear_global_memory_by_source_prefix(DEFAULT_KNOWLEDGE_SOURCE_PREFIX)
            .await
            .expect("verify cleared rows");
        assert_eq!(cleared, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_memory_import_format_accepts_openclaw() {
        assert_eq!(
            parse_memory_import_format("OpenClaw").unwrap(),
            MemoryImportFormat::Openclaw
        );
    }

    #[test]
    fn parse_memory_import_tier_accepts_global() {
        assert_eq!(
            parse_memory_import_tier("global").unwrap(),
            MemoryTier::Global
        );
    }
}
