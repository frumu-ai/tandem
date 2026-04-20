mod default_knowledge_bundle;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use std::{fs, io::Read};

use anyhow::Context;
use clap::{Parser, Subcommand};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tandem_core::{
    build_mode_permission_rules, load_provider_auth, resolve_shared_paths, AgentRegistry,
    CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionAction, PermissionManager,
    PluginRegistry, Storage, DEFAULT_ENGINE_HOST, DEFAULT_ENGINE_PORT,
};
use tandem_memory::{
    import_files,
    types::{MemoryImportFormat, MemoryImportRequest, MemoryTier, StoreMessageRequest},
    MemoryManager,
};
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_server::{
    detect_host_runtime_context, install_browser_sidecar, serve, AppState,
    BrowserSidecarInstallResult, BrowserSubsystem, RuntimeState,
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

const MEMORY_EXAMPLES: &str = r#"Examples:
  tandem-engine memory import --path ~/.openclaw --format openclaw
  tandem-engine memory import --path ./notes --tier global
  tandem-engine memory import --path ./docs --tier project --project-id repo-123 --sync-deletes
"#;

const DEFAULT_KNOWLEDGE_SOURCE_PREFIX: &str = "guide_docs:";
const DEFAULT_KNOWLEDGE_STATE_FILE: &str = "default_knowledge_state.json";
const DEFAULT_KNOWLEDGE_DOCS_SITE_BASE_URL: &str = "https://docs.tandem.ac/";

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
            help = "Require API token auth for HTTP endpoints (Authorization: Bearer <token> or X-Tandem-Token)."
        )]
        api_token: Option<String>,
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
            if let Some(token) = api_token.and_then(|raw| {
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }) {
                info!("API token auth enabled for tandem-engine HTTP API");
                state.set_api_token(Some(token)).await;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefaultKnowledgeState {
    corpus_hash: String,
    source_dir: String,
    docs_site_base_url: String,
    bundle_schema_version: u32,
    file_count: usize,
    total_chunks: usize,
    updated_at: String,
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
        state.set_phase("default_knowledge_bootstrap").await;
        emit_startup_phase_event(state, "default_knowledge_bootstrap").await;
    }
    let phase_start = Instant::now();
    if let Err(err) = bootstrap_default_knowledge(state_dir).await {
        tracing::warn!("default knowledge bootstrap skipped: {}", err);
    }
    info!(
        "engine.startup.phase default_knowledge_bootstrap elapsed_ms={}",
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
    let _ = browser.refresh_status().await;
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

async fn bootstrap_default_knowledge(state_dir: &Path) -> anyhow::Result<()> {
    if env_truthy("TANDEM_DISABLE_DEFAULT_KNOWLEDGE") {
        info!("default knowledge bootstrap disabled by TANDEM_DISABLE_DEFAULT_KNOWLEDGE");
        return Ok(());
    }

    let (bundle, manifest, source_mode) =
        if let Some((override_bundle, override_manifest, override_source)) =
            load_default_knowledge_override_from_env()?
        {
            (override_bundle, override_manifest, override_source)
        } else {
            let (embedded_bundle, embedded_manifest) =
                default_knowledge_bundle::load_embedded_default_knowledge()?;
            (
                embedded_bundle,
                embedded_manifest,
                "embedded_bundle".to_string(),
            )
        };
    if bundle.docs.is_empty() {
        info!("default knowledge bootstrap skipped: embedded bundle has no docs");
        return Ok(());
    }

    let state_path = state_dir.join(DEFAULT_KNOWLEDGE_STATE_FILE);
    if let Some(existing) = load_default_knowledge_state(&state_path) {
        if existing.corpus_hash == manifest.corpus_hash {
            info!(
                "default knowledge bootstrap skip embedded_corpus_hash={} file_count={} seed_action=skip",
                manifest.corpus_hash,
                existing.file_count
            );
            return Ok(());
        }
    }

    let db_path = resolve_memory_db_path(state_dir);
    let manager = MemoryManager::new(&db_path).await?;
    let embedding_health = manager.embedding_health().await;
    if embedding_health.status != "ok" {
        tracing::warn!(
            "default knowledge bootstrap skipped: embeddings unavailable status={} reason={:?}",
            embedding_health.status,
            embedding_health.reason
        );
        return Ok(());
    }

    let deleted = manager
        .db()
        .clear_global_memory_by_source_prefix(DEFAULT_KNOWLEDGE_SOURCE_PREFIX)
        .await?;

    let mut total_chunks = 0usize;
    for doc in &bundle.docs {
        let source = format!("{}{}", DEFAULT_KNOWLEDGE_SOURCE_PREFIX, doc.relative_path);
        let enriched = format!(
            "Document path: {}\nSource URL: {}\n\n{}",
            doc.relative_path, doc.source_url, doc.content
        );
        let request = StoreMessageRequest {
            content: enriched,
            tier: tandem_memory::types::MemoryTier::Global,
            session_id: None,
            project_id: None,
            source,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            metadata: Some(serde_json::json!({
                "kind": "official_documentation",
                "source_of_truth": bundle.source_root,
                "relative_path": doc.relative_path,
                "source_url": doc.source_url,
                "content_hash": doc.content_hash,
                "bundle_schema_version": bundle.schema_version,
                "bundle_generated_at": bundle.generated_at
            })),
        };
        total_chunks += manager.store_message(request).await?.len();
    }

    let snapshot = DefaultKnowledgeState {
        corpus_hash: manifest.corpus_hash.clone(),
        source_dir: source_mode.clone(),
        docs_site_base_url: bundle.docs_site_base_url.clone(),
        bundle_schema_version: bundle.schema_version,
        file_count: bundle.docs.len(),
        total_chunks,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    save_default_knowledge_state(&state_path, &snapshot)?;
    info!(
        "default knowledge bootstrap complete embedded_corpus_hash={} file_count={} total_bytes={} seeded_chunk_count={} deleted_old_chunks={} seed_action=reseed source_mode={} docs_site_base_url={} db_path={} bundle_schema_version={} generator_version={} manifest_schema_version={}",
        manifest.corpus_hash,
        snapshot.file_count,
        manifest.total_bytes,
        snapshot.total_chunks,
        deleted,
        source_mode,
        snapshot.docs_site_base_url,
        db_path.display(),
        snapshot.bundle_schema_version,
        manifest.generator_version,
        manifest.schema_version
    );
    Ok(())
}

fn load_default_knowledge_override_from_env() -> anyhow::Result<
    Option<(
        default_knowledge_bundle::EmbeddedKnowledgeBundle,
        default_knowledge_bundle::EmbeddedKnowledgeManifest,
        String,
    )>,
> {
    let override_dir = match std::env::var("TANDEM_DOCS_SOURCE_DIR") {
        Ok(raw) if !raw.trim().is_empty() => PathBuf::from(raw.trim()),
        _ => return Ok(None),
    };
    if !override_dir.is_dir() {
        tracing::warn!(
            "TANDEM_DOCS_SOURCE_DIR set but not a directory: {}",
            override_dir.display()
        );
        return Ok(None);
    }

    let mut docs = Vec::new();
    for entry in ignore::WalkBuilder::new(&override_dir).build().flatten() {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "md" && ext != "mdx" {
            continue;
        }

        let content =
            normalize_default_knowledge_content(&fs::read_to_string(&path).with_context(|| {
                format!("failed to read docs override file {}", path.display())
            })?);
        if content.trim().is_empty() {
            continue;
        }
        let relative_path = path
            .strip_prefix(&override_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source_url = docs_url_for_relative_path(&relative_path);
        docs.push(default_knowledge_bundle::EmbeddedKnowledgeDoc {
            relative_path,
            source_url,
            content_hash: sha256_hex(content.as_bytes()),
            content,
        });
    }
    docs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    if docs.is_empty() {
        tracing::warn!(
            "TANDEM_DOCS_SOURCE_DIR set but no markdown docs found in {}",
            override_dir.display()
        );
        return Ok(None);
    }

    let total_bytes = docs.iter().map(|doc| doc.content.len()).sum::<usize>();
    let corpus_hash = default_knowledge_bundle::compute_corpus_hash(&docs);
    let bundle = default_knowledge_bundle::EmbeddedKnowledgeBundle {
        schema_version: 1,
        source_root: "guide/src/content/docs".to_string(),
        docs_site_base_url: DEFAULT_KNOWLEDGE_DOCS_SITE_BASE_URL.to_string(),
        generated_at: default_knowledge_bundle::deterministic_generated_at(&corpus_hash),
        docs,
    };
    let manifest = default_knowledge_bundle::EmbeddedKnowledgeManifest {
        schema_version: 1,
        generator_version: "dev-override".to_string(),
        corpus_hash,
        file_count: bundle.docs.len(),
        total_bytes,
    };
    Ok(Some((
        bundle,
        manifest,
        format!("env_override:{}", override_dir.to_string_lossy()),
    )))
}

fn docs_url_for_relative_path(relative_path: &str) -> String {
    let base = DEFAULT_KNOWLEDGE_DOCS_SITE_BASE_URL;
    let mut slug = relative_path.replace('\\', "/");
    if let Some(stripped) = slug.strip_suffix(".md") {
        slug = stripped.to_string();
    } else if let Some(stripped) = slug.strip_suffix(".mdx") {
        slug = stripped.to_string();
    }
    if slug == "index" {
        return base.to_string();
    }
    if let Some(stripped) = slug.strip_suffix("/index") {
        slug = stripped.to_string();
    }
    format!("{}{}", base, slug)
}

fn normalize_default_knowledge_content(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn load_default_knowledge_state(path: &Path) -> Option<DefaultKnowledgeState> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<DefaultKnowledgeState>(&raw).ok()
}

fn save_default_knowledge_state(path: &Path, state: &DefaultKnowledgeState) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(path, format!("{raw}\n"))?;
    Ok(())
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

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
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

    #[test]
    fn normalize_default_knowledge_content_canonicalizes_line_endings() {
        assert_eq!(
            normalize_default_knowledge_content("alpha\r\nbeta\r\ngamma\r"),
            "alpha\nbeta\ngamma\n"
        );
        assert_eq!(
            normalize_default_knowledge_content("alpha\nbeta\ngamma\n"),
            "alpha\nbeta\ngamma\n"
        );
    }

    #[test]
    fn normalized_default_knowledge_hashes_are_stable() {
        let lf = normalize_default_knowledge_content("alpha\nbeta\n");
        let crlf = normalize_default_knowledge_content("alpha\r\nbeta\r\n");

        assert_eq!(lf, crlf);
        assert_eq!(sha256_hex(lf.as_bytes()), sha256_hex(crlf.as_bytes()));
        assert_eq!(lf.as_bytes().len(), crlf.as_bytes().len());
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
