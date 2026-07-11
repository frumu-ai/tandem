use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use std::{fs, io::Read};

use anyhow::Context;

use clap::{Parser, Subcommand};

mod runtime_bootstrap;
mod smoke;
mod storage_maintenance;
use runtime_bootstrap::*;
use storage_maintenance::*;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tandem_core::{
    build_mode_permission_rules, load_or_create_engine_api_token, load_provider_auth,
    resolve_shared_paths, AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus,
    PermissionAction, PermissionManager, PluginRegistry, Storage, DEFAULT_ENGINE_HOST,
    DEFAULT_ENGINE_PORT,
};
use tandem_memory::{
    import_files,
    types::{MemoryImportFormat, MemoryImportRequest, MemoryTier},
    MemoryManager,
};
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
#[cfg(not(feature = "enterprise-server"))]
use tandem_server::serve;
#[cfg(feature = "enterprise-server")]
use tandem_server::serve_with_route_extensions;
use tandem_server::{detect_host_runtime_context, AppState, RuntimeState};
#[cfg(feature = "browser")]
use tandem_server::{install_browser_sidecar, BrowserSidecarInstallResult, BrowserSubsystem};
use tandem_tools::{GovernedToolDispatcher, ToolRegistry};
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

const CONFIG_EXAMPLES: &str = r#"Examples:
  tandem-engine config check
  tandem-engine config check --json
  tandem-engine config reference
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
  tandem-engine storage worktrees --repo-root /abs/path/to/repo
  tandem-engine storage worktrees --repo-root /abs/path/to/repo --apply --json
  tandem-engine storage cleanup --dry-run --context-runs --default-knowledge --json
  tandem-engine storage cleanup --quarantine --json
"#;

const MEMORY_EXAMPLES: &str = r#"Examples:
  tandem-engine memory import --path ~/.openclaw --format openclaw
  tandem-engine memory import --path ./notes --tier global
  tandem-engine memory import --path ./docs --tier project --project-id repo-123 --sync-deletes
"#;

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
    #[command(about = "Inspect and validate Tandem engine configuration.")]
    #[command(after_help = CONFIG_EXAMPLES)]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
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
    #[command(
        about = "Run the end-to-end runtime smoke test (session prompt, approval gate, policy denial, memory)."
    )]
    #[command(
        long_about = "Runs deterministic end-to-end scenarios against the governed runtime path:\nsession prompt round-trip, approval gate (submit -> awaiting_approval -> approve -> complete),\nagent policy denial with audit, and a governed memory put/search round-trip.\n\nBy default an isolated in-process server is booted with a fresh state directory and the\nlocal echo provider, so no network access or API keys are required. Use --against to\ntarget an already-running engine instead."
    )]
    Smoke {
        #[arg(
            long,
            help = "Base URL of a running engine (e.g. http://127.0.0.1:39731). Omit to boot an isolated in-process server."
        )]
        against: Option<String>,
        #[arg(
            long,
            env = "TANDEM_API_TOKEN",
            help = "API token for --against mode (Authorization: Bearer)."
        )]
        token: Option<String>,
        #[arg(
            long,
            help = "Run only the named scenario (repeatable): session-prompt, approval-gate, policy-denial, memory-roundtrip."
        )]
        scenario: Vec<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Emit machine-readable JSON results."
        )]
        json: bool,
        #[arg(
            long,
            default_value_t = 60,
            help = "Overall deadline in seconds; the run fails if scenarios have not finished."
        )]
        timeout_secs: u64,
    },
}

#[derive(Subcommand, Debug)]
enum TokenCommand {
    #[command(about = "Generate a random API token string.")]
    #[command(after_help = TOKEN_EXAMPLES)]
    Generate,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    #[command(about = "Validate startup environment and print masked effective configuration.")]
    Check {
        #[arg(long, default_value_t = false, help = "Print machine-readable JSON.")]
        json: bool,
    },
    #[command(about = "Print the generated configuration reference as Markdown.")]
    Reference,
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
    #[command(
        about = "Preview or remove stale managed worktrees for a repository via the running engine."
    )]
    Worktrees {
        #[arg(
            long,
            env = "TANDEM_ENGINE_HOST",
            alias = "host",
            default_value = DEFAULT_ENGINE_HOST,
            help = "Hostname or IP address of the running engine."
        )]
        hostname: String,
        #[arg(
            long,
            env = "TANDEM_ENGINE_PORT",
            default_value_t = DEFAULT_ENGINE_PORT,
            help = "Port of the running engine."
        )]
        port: u16,
        #[arg(long, help = "Absolute repository root to inspect.")]
        repo_root: Option<String>,
        #[arg(
            long,
            default_value_t = false,
            help = "Apply cleanup instead of running a dry-run preview."
        )]
        apply: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Keep orphan directories on disk instead of removing them."
        )]
        keep_orphan_dirs: bool,
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
        Command::Config { command } => match command {
            ConfigCommand::Check { json } => {
                let report = tandem_server::EngineConfigReport::from_env(Default::default());
                if json {
                    println!("{}", serde_json::to_string_pretty(&report.masked_json())?);
                } else {
                    println!("{}", report.human_summary());
                }
                report.ensure_valid()?;
            }
            ConfigCommand::Reference => {
                print!("{}", tandem_server::config_reference_markdown());
            }
        },
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
            let startup_config =
                tandem_server::EngineConfigReport::from_env(tandem_server::EngineConfigOptions {
                    cli_transport_token_configured: api_token
                        .as_ref()
                        .is_some_and(|token| !token.trim().is_empty()),
                    unsafe_no_api_token,
                });
            for warning in &startup_config.warnings {
                eprintln!("warning: {warning}");
            }
            startup_config.ensure_valid()?;
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
                    org_id: None,
                    workspace_id: None,
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
            let addr: SocketAddr = format!("{hostname}:{port}")
                .parse()
                .context("invalid hostname or port")?;
            if unsafe_no_api_token && !addr.ip().is_loopback() {
                anyhow::bail!(
                    "--unsafe-no-api-token/TANDEM_UNSAFE_NO_API_TOKEN is only allowed on loopback binds; set TANDEM_API_TOKEN or bind to 127.0.0.1"
                );
            }
            if let Some(token) = resolve_engine_api_token(api_token, unsafe_no_api_token)? {
                info!("API token auth enabled for tandem-engine HTTP API");
                state.set_api_token(Some(token)).await;
            } else {
                tracing::warn!(
                    "API token auth disabled for tandem-engine HTTP API by explicit unsafe flag"
                );
            }
            let internal_host = if hostname == "0.0.0.0" {
                "127.0.0.1".to_string()
            } else {
                hostname.clone()
            };
            state.set_server_base_url(format!("http://{internal_host}:{port}"));
            let init_config_path = resolve_config_override(config.as_deref());
            log_startup_paths(
                &state_dir,
                &addr,
                &startup_attempt_id,
                init_config_path.as_deref(),
            );
            let init_state = state.clone();
            let init_state_dir = state_dir.clone();
            let init_overrides = overrides.clone();

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
                            org_id: None,
                            workspace_id: None,
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
            #[cfg(feature = "enterprise-server")]
            {
                serve_with_route_extensions(addr, state, &[tandem_enterprise_server::apply_routes])
                    .await?;
            }
            #[cfg(not(feature = "enterprise-server"))]
            {
                serve(addr, state).await?;
            }
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
            let result = state
                .tool_dispatcher
                .dispatch(
                    &tool,
                    args,
                    tandem_tools::ToolDispatchContext::local("engine_cli"),
                )
                .await?;
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
                #[cfg(feature = "browser")]
                {
                    let state_dir = resolve_state_dir(state_dir);
                    let config_path = config.map(PathBuf::from);
                    let status = browser_doctor_status(&state_dir, config_path).await?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&status)?);
                    } else {
                        print_browser_readiness(&status);
                    }
                }
                #[cfg(not(feature = "browser"))]
                {
                    let _ = (state_dir, config, json);
                    anyhow::bail!("browser feature disabled; rebuild tandem-engine with --features browser or use the bundled tandem-browser sidecar");
                }
            }
            BrowserCommand::Install {
                state_dir,
                config,
                json,
            } => {
                #[cfg(feature = "browser")]
                {
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
                #[cfg(not(feature = "browser"))]
                {
                    let _ = (state_dir, config, json);
                    anyhow::bail!("browser feature disabled; rebuild tandem-engine with --features browser or use the bundled tandem-browser sidecar");
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
            StorageCommand::Worktrees {
                hostname,
                port,
                repo_root,
                apply,
                keep_orphan_dirs,
                json,
            } => {
                let url = format!("http://{hostname}:{port}/worktree/cleanup");
                let resp = reqwest::Client::new()
                    .post(&url)
                    .json(&json!({
                        "repo_root": repo_root,
                        "dry_run": !apply,
                        "remove_orphan_dirs": !keep_orphan_dirs,
                    }))
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await?;
                if !status.is_success() {
                    anyhow::bail!("worktree cleanup failed: {} {}", status, body);
                }
                let report: serde_json::Value = serde_json::from_str(&body)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": body }));
                if json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_worktree_cleanup_report(&report);
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
                let manager =
                    MemoryManager::new_runtime(&resolve_memory_db_path(&state_dir)).await?;
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
                        tenant_scope: tandem_memory::types::MemoryTenantScope::local(),
                        source_binding: None,
                        sync_deletes,
                        import_namespace: None,
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
        Command::Smoke {
            against,
            token,
            scenario,
            json,
            timeout_secs,
        } => {
            let all_passed = smoke::run_smoke(smoke::SmokeOptions {
                against,
                token,
                scenarios: scenario,
                json,
                timeout_secs,
            })
            .await?;
            if !all_passed {
                std::process::exit(1);
            }
        }
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

#[cfg(test)]
mod main_tests;
