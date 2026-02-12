use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_server::{serve, AppState};
use tandem_tools::ToolRegistry;
use tokio::sync::RwLock;
use tracing::info;

use tandem_providers::ProviderRegistry;

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
    },
    Run {
        prompt: String,
    },
    Chat,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve {
            hostname,
            port,
            state_dir,
            in_process,
        } => {
            let state_dir = resolve_state_dir(state_dir);
            let state = build_state(&state_dir).await?;
            state
                .in_process_mode
                .store(in_process, std::sync::atomic::Ordering::Relaxed);
            let addr: SocketAddr = format!("{hostname}:{port}")
                .parse()
                .context("invalid hostname or port")?;
            log_startup_paths(&state_dir, &addr);
            serve(addr, state).await?;
        }
        Command::Run { prompt } => {
            let state_dir = resolve_state_dir(None);
            let state = build_state(&state_dir).await?;
            let reply = state.engine_loop.run_oneshot(prompt).await?;
            println!("{reply}");
        }
        Command::Chat => {
            let _state = build_state(&resolve_state_dir(None)).await?;
            println!("Interactive chat mode is planned; use `serve` for now.");
        }
    }

    Ok(())
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
    PathBuf::from(".tandem")
}

fn log_startup_paths(state_dir: &PathBuf, addr: &SocketAddr) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let config_path = state_dir.join("config.json");
    info!("starting tandem-engine on http://{addr}");
    info!(
        "startup paths: exe={} cwd={} state_dir={} config_path={}",
        exe.display(),
        cwd.display(),
        state_dir.display(),
        config_path.display()
    );
}

async fn build_state(state_dir: &PathBuf) -> anyhow::Result<AppState> {
    let storage = Arc::new(Storage::new(state_dir.join("storage")).await?);
    let config = ConfigStore::new(state_dir.join("config.json")).await?;
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
    let in_process_mode = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
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

    Ok(AppState {
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
        in_process_mode,
        cancellations,
        engine_loop,
    })
}
