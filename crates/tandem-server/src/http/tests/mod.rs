use super::*;

pub(super) mod agent_teams;
pub(super) mod bug_monitor;
pub(super) mod capabilities;
pub(super) mod channels;
pub(super) mod coder;
pub(super) mod context_runs;
pub(super) mod global;
pub(super) mod memory;
pub(super) mod mission_builder;
pub(super) mod missions;
pub(super) mod optimizations;
pub(super) mod pack_builder;
pub(super) mod packs;
pub(super) mod permissions;
pub(super) mod presets;
pub(super) mod providers;
pub(super) mod resources;
pub(super) mod routines;
pub(super) mod sessions;
pub(super) mod setup_understanding;
pub(super) mod workflow_planner;
pub(super) mod workflows;

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::Request;
use std::time::Duration;
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage, ToolPolicyContext, ToolPolicyHook,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;
use tokio::sync::broadcast;
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::global::sanitize_relative_subpath;

pub(super) async fn test_state() -> AppState {
    let root = std::env::temp_dir().join(format!("tandem-http-test-{}", Uuid::new_v4()));
    let global = root.join("global-config.json");
    let tandem_home = root.join("tandem-home");
    let mcp_state = root.join("mcp.json");
    std::env::set_var("TANDEM_GLOBAL_CONFIG", &global);
    std::env::set_var("TANDEM_HOME", &tandem_home);
    let seeded_mcp = json!({
        "github": {
            "name": "github",
            "transport": "memory://github",
            "enabled": true,
            "connected": false,
            "headers": {},
            "tool_cache": [
                {
                    "tool_name": "list_repository_issues",
                    "description": "List repository issues",
                    "input_schema": {"type":"object"},
                    "fetched_at_ms": 1,
                    "schema_hash": "tool-github-list-issues"
                },
                {
                    "tool_name": "get_issue",
                    "description": "Get a GitHub issue",
                    "input_schema": {"type":"object"},
                    "fetched_at_ms": 1,
                    "schema_hash": "tool-github-get-issue"
                },
                {
                    "tool_name": "mcp.github.list_pull_requests",
                    "description": "List repository pull requests",
                    "input_schema": {"type":"object"},
                    "fetched_at_ms": 1,
                    "schema_hash": "tool-github-list-pulls"
                },
                {
                    "tool_name": "mcp.github.get_pull_request",
                    "description": "Get a GitHub pull request",
                    "input_schema": {"type":"object"},
                    "fetched_at_ms": 1,
                    "schema_hash": "tool-github-get-pull"
                },
                {
                    "tool_name": "mcp.github.create_pull_request",
                    "description": "Create a GitHub pull request",
                    "input_schema": {"type":"object"},
                    "fetched_at_ms": 1,
                    "schema_hash": "tool-github-create-pull"
                }
            ],
            "tools_fetched_at_ms": 1,
            "pending_auth_by_tool": {}
        }
    });
    if let Some(parent) = mcp_state.parent() {
        std::fs::create_dir_all(parent).expect("mcp state dir");
    }
    std::fs::write(
        &mcp_state,
        serde_json::to_string_pretty(&seeded_mcp).expect("seeded mcp json"),
    )
    .expect("write mcp state");
    let storage = Arc::new(Storage::new(root.join("storage")).await.expect("storage"));
    let config = ConfigStore::new(root.join("config.json"), None)
        .await
        .expect("config");
    let event_bus = EventBus::new();
    let app_config = config.get().await;
    let browser = crate::BrowserSubsystem::new(app_config.browser.clone());
    let _ = browser.refresh_status().await;
    let providers = ProviderRegistry::new(app_config.into());
    let plugins = PluginRegistry::new(".").await.expect("plugins");
    let agents = AgentRegistry::new(".").await.expect("agents");
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new(event_bus.clone());
    let mcp = McpRegistry::new_with_state_file(mcp_state);
    let pty = PtyManager::new();
    let lsp = LspManager::new(".");
    let auth = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let workspace_index = WorkspaceIndex::new(".").await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = crate::detect_host_runtime_context();
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
    let mut state = AppState::new_starting(Uuid::new_v4().to_string(), false);
    state.shared_resources_path = root.join("shared_resources.json");
    state
        .mark_ready(crate::RuntimeState {
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
        .await
        .expect("runtime ready");
    assert!(state.mcp.connect("github").await);
    state
}

pub(super) fn write_pack_zip(path: &std::path::Path, manifest: &str) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("tandempack.yaml", opts)
        .expect("start marker");
    std::io::Write::write_all(&mut zip, manifest.as_bytes()).expect("write marker");
    zip.start_file("README.md", opts).expect("start readme");
    std::io::Write::write_all(&mut zip, b"# pack").expect("write readme");
    zip.finish().expect("finish zip");
}

pub(super) fn write_plain_zip_without_marker(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("README.md", opts).expect("start readme");
    std::io::Write::write_all(&mut zip, b"# not a pack").expect("write readme");
    zip.start_file("agents/a.txt", opts)
        .expect("start agents file");
    std::io::Write::write_all(&mut zip, b"agent body").expect("write agents file");
    zip.finish().expect("finish zip");
}

pub(super) async fn next_event_of_type(
    rx: &mut broadcast::Receiver<EngineEvent>,
    expected_type: &str,
) -> EngineEvent {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == expected_type {
                return event;
            }
        }
    })
    .await
    .expect("event timeout")
}
