//! EAA-10 (TAN-35): test-support seam for crates that own routes mounted onto
//! the Tandem server (e.g. `tandem-enterprise-server`).
//!
//! Building a realistic `AppState` for an HTTP integration test reaches deep
//! into `tandem-server` internals. Rather than leak those internals as public
//! API, this module exposes a small, intentional surface — [`test_state`] and
//! [`next_event_of_type`] — behind the `test-support` feature. Dependent crates
//! enable it as a dev-dependency and build their router with
//! [`crate::build_router_with_extensions`], passing their own route registrar
//! (e.g. `tandem_enterprise_server::apply_routes`) so the routes under test are
//! actually mounted.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::{GovernedToolDispatcher, ToolRegistry};
use tandem_types::EngineEvent;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{AppState, RuntimeState};

/// Build a ready [`AppState`] backed by per-call temp directories, with the
/// enterprise storage paths and a seeded in-memory MCP server wired up. Suitable
/// for axum `oneshot` HTTP integration tests.
pub async fn test_state() -> AppState {
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
    #[cfg(feature = "browser")]
    let browser = crate::BrowserSubsystem::new(app_config.browser.clone());
    #[cfg(feature = "browser")]
    let _ = browser.refresh_status().await;
    let providers = ProviderRegistry::new(app_config.into());
    let plugins = PluginRegistry::new(".").await.expect("plugins");
    let agents = AgentRegistry::new(".").await.expect("agents");
    let tools = ToolRegistry::new();
    let permissions =
        PermissionManager::new_with_state_file(event_bus.clone(), root.join("permissions.json"))
            .await
            .expect("permissions");
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
    state
        .trust_test_tenant_headers
        .store(true, Ordering::Relaxed);
    state.shared_resources_path = root.join("shared_resources.json");
    state.channel_automation_drafts_path = root.join("channel_automation_drafts.json");
    state.channel_user_capabilities_path = root.join("channel_user_capabilities.json");
    state.memory_db_path = tandem_home.join("memory.sqlite");
    state.memory_audit_path = root.join("memory").join("audit.log.jsonl");
    state.protected_audit_path = root.join("audit").join("protected_events.log.jsonl");
    state.enterprise.org_units_path = root.join("enterprise_org_units.json");
    state.enterprise.org_unit_memberships_path = root.join("enterprise_org_unit_memberships.json");
    state.enterprise.org_unit_access_grants_path =
        root.join("enterprise_org_unit_access_grants.json");
    state.enterprise.cross_tenant_grants_path = root.join("enterprise_cross_tenant_grants.json");
    state.enterprise.source_bindings_path = root.join("enterprise_source_bindings.json");
    state.enterprise.policy_rules_path = root.join("enterprise_policy_rules.json");
    state.enterprise.connectors_path = root.join("enterprise_connectors.json");
    state.enterprise.ingestion_jobs_path = root.join("enterprise_ingestion_jobs.json");
    state.enterprise.ingestion_quarantines_path =
        root.join("enterprise_ingestion_quarantines.json");
    state.routines_path = root.join("routines.json");
    state.routine_history_path = root.join("routine_history.json");
    state.routine_runs_path = root.join("routine_runs.json");
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_webhook_triggers_path = root.join("automation_webhooks").join("triggers.json");
    state.automation_webhook_deliveries_path =
        root.join("automation_webhooks").join("deliveries.json");
    state.automation_webhook_secret_material_path =
        root.join("automation_webhooks").join("secrets.json");
    state.idempotency_keys_path = root.join("runtime").join("idempotency_keys.json");
    state.runtime_events_path = root.join("runtime").join("events.jsonl");
    state.optimization_campaigns_path = root.join("optimization_campaigns.json");
    state.optimization_experiments_path = root.join("optimization_experiments.json");
    state.incident_monitor_config_path = root.join("incident_monitor_config.json");
    state.incident_monitor_drafts_path = root.join("incident_monitor_drafts.json");
    state.incident_monitor_incidents_path = root.join("incident_monitor_incidents.json");
    state.incident_monitor_posts_path = root.join("incident_monitor_posts.json");
    state.external_actions_path = root.join("external_actions.json");
    state.policy_decisions_path = root.join("policy_decisions.json");
    state.workflow_runs_path = root.join("workflow_runs.json");
    state.workflow_planner_sessions_path = root.join("workflow_planner_sessions.json");
    state.workflow_learning_candidates_path = root.join("workflow_learning_candidates.json");
    state.context_packs_path = root.join("context_packs.json");
    state.workflow_hook_overrides_path = root.join("workflow_hook_overrides.json");
    state
        .mark_ready(RuntimeState {
            storage,
            config,
            event_bus,
            providers,
            plugins,
            agents,
            tool_dispatcher: GovernedToolDispatcher::new(tools.clone()),
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
            #[cfg(feature = "browser")]
            browser,
        })
        .await
        .expect("runtime ready");
    assert!(state.mcp.connect("github").await);
    state
}

/// Await the next engine event of `expected_type` on `rx`, failing the test if
/// none arrives within 5 seconds.
pub async fn next_event_of_type(
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
