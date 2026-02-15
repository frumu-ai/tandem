use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use std::{fs, io::Read};

use anyhow::Context;
use clap::{Parser, Subcommand};
use tandem_core::{
    migrate_legacy_storage_if_needed, resolve_shared_paths, AgentRegistry, CancellationRegistry,
    ConfigStore, EngineLoop, EventBus, PermissionManager, PluginRegistry, Storage,
};
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_server::{serve, AppState, RuntimeState};
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

#[derive(Parser, Debug)]
#[command(name = "tandem-engine")]
#[command(about = "Headless Tandem AI backend")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve {
        #[arg(long, alias = "host", default_value = "127.0.0.1")]
        hostname: String,
        #[arg(long, default_value_t = 3000)]
        port: u16,
        #[arg(long)]
        state_dir: Option<String>,
        #[arg(long, default_value_t = false)]
        in_process: bool,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        config: Option<String>,
    },
    Run {
        prompt: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        config: Option<String>,
    },
    Chat,
    Tool {
        #[arg(long)]
        json: String,
        #[arg(long)]
        state_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve {
            hostname,
            port,
            state_dir,
            in_process,
            api_key,
            provider,
            model,
            config,
        } => {
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
            let startup_attempt_id = Uuid::new_v4().to_string();
            let state = AppState::new_starting(startup_attempt_id.clone(), in_process);
            let addr: SocketAddr = format!("{hostname}:{port}")
                .parse()
                .context("invalid hostname or port")?;
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
        Command::Chat => {
            let _state = build_runtime(&resolve_state_dir(None), None, None, None).await?;
            println!("Interactive chat mode is planned; use `serve` for now.");
        }
        Command::Tool { json, state_dir } => {
            let state_dir = resolve_state_dir(state_dir);
            let state = build_runtime(&state_dir, None, None, None).await?;
            let payload = read_tool_json(&json)?;
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
    if SUPPORTED_PROVIDER_IDS.contains(&normalized.as_str()) {
        return Ok(Some(normalized));
    }
    anyhow::bail!(
        "unsupported provider `{}`. supported providers: {}",
        provider,
        SUPPORTED_PROVIDER_IDS.join(", ")
    );
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
        .unwrap_or_else(|_| PathBuf::from(".tandem"))
}

fn read_tool_json(input: &str) -> anyhow::Result<serde_json::Value> {
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

fn log_startup_paths(state_dir: &PathBuf, addr: &SocketAddr, startup_attempt_id: &str) {
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

    state.set_phase("migration").await;
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
            detail: Some(&format!("attempt_id={} phase=migration", attempt_id)),
        },
    );
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(paths) = resolve_shared_paths() {
            if let Ok(report) = migrate_legacy_storage_if_needed(&paths) {
                info!(
                    "storage migration status: reason={} performed={} copied={} skipped={} errors={}",
                    report.reason,
                    report.performed,
                    report.copied.len(),
                    report.skipped.len(),
                    report.errors.len()
                );
            }
        }
    })
    .await;

    let runtime = build_runtime(&state_dir, Some(&state), overrides, config_path).await?;
    state.mark_ready(runtime).await?;
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
    state_dir: &PathBuf,
    startup_state: Option<&AppState>,
    cli_overrides: Option<serde_json::Value>,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<RuntimeState> {
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
    let providers = ProviderRegistry::new(config.get().await.into());
    let plugins = PluginRegistry::new(".").await?;
    let agents = AgentRegistry::new(".").await?;
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new(event_bus.clone());
    let mcp = McpRegistry::new();
    let pty = PtyManager::new();
    let lsp = LspManager::new(".");
    let auth = Arc::new(RwLock::new(std::collections::HashMap::new()));
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
    let engine_loop = EngineLoop::new(
        storage.clone(),
        event_bus.clone(),
        providers.clone(),
        plugins.clone(),
        agents.clone(),
        permissions.clone(),
        tools.clone(),
        cancellations.clone(),
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
    })
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
    fn normalize_and_validate_provider_rejects_unknown_value() {
        let err = normalize_and_validate_provider(Some("openruter".to_string())).unwrap_err();
        assert!(err.to_string().contains("unsupported provider `openruter`"));
    }

    #[test]
    fn build_cli_overrides_rejects_unknown_provider() {
        let err = build_cli_overrides(
            Some("sk-test".to_string()),
            Some("openruter".to_string()),
            Some("x".to_string()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported provider `openruter`"));
    }
}
