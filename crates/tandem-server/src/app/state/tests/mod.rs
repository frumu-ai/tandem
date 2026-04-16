pub(crate) use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;
use tandem_types::TenantContext;

#[allow(dead_code)]
pub(crate) struct AutomationNodeBuilder {
    node: AutomationFlowNode,
}

impl AutomationNodeBuilder {
    pub(crate) fn new(node_id: impl Into<String>) -> Self {
        let node_id = node_id.into();
        Self {
            node: AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: node_id.clone(),
                agent_id: "agent-a".to_string(),
                objective: format!("Run {node_id}"),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            },
        }
    }

    pub(crate) fn agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.node.agent_id = agent_id.into();
        self
    }

    pub(crate) fn objective(mut self, objective: impl Into<String>) -> Self {
        self.node.objective = objective.into();
        self
    }

    pub(crate) fn depends_on(mut self, depends_on: Vec<&str>) -> Self {
        self.node.depends_on = depends_on.into_iter().map(str::to_string).collect();
        self
    }

    pub(crate) fn output_contract(mut self, output_contract: AutomationFlowOutputContract) -> Self {
        self.node.output_contract = Some(output_contract);
        self
    }

    pub(crate) fn stage_kind(mut self, stage_kind: AutomationNodeStageKind) -> Self {
        self.node.stage_kind = Some(stage_kind);
        self
    }

    pub(crate) fn metadata(mut self, metadata: Value) -> Self {
        self.node.metadata = Some(metadata);
        self
    }

    pub(crate) fn build(self) -> AutomationFlowNode {
        self.node
    }
}

#[allow(dead_code)]
pub(crate) struct AutomationSpecBuilder {
    automation: AutomationV2Spec,
}

impl AutomationSpecBuilder {
    pub(crate) fn new(automation_id: impl Into<String>) -> Self {
        let automation_id = automation_id.into();
        Self {
            automation: AutomationV2Spec {
                automation_id,
                name: "Test Automation".to_string(),
                description: None,
                status: AutomationV2Status::Active,
                schedule: AutomationV2Schedule {
                    schedule_type: AutomationV2ScheduleType::Manual,
                    cron_expression: None,
                    interval_seconds: None,
                    timezone: "UTC".to_string(),
                    misfire_policy: RoutineMisfirePolicy::RunOnce,
                },
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                agents: vec![AutomationAgentProfile {
                    agent_id: "agent-a".to_string(),
                    template_id: Some("template-a".to_string()),
                    display_name: "Agent A".to_string(),
                    avatar_url: None,
                    model_policy: None,
                    skills: Vec::new(),
                    tool_policy: AutomationAgentToolPolicy {
                        allowlist: Vec::new(),
                        denylist: Vec::new(),
                    },
                    mcp_policy: AutomationAgentMcpPolicy {
                        allowed_servers: Vec::new(),
                        allowed_tools: None,
                    },
                    approval_policy: None,
                }],
                flow: AutomationFlowSpec { nodes: Vec::new() },
                execution: AutomationExecutionPolicy {
                    max_parallel_agents: Some(2),
                    max_total_runtime_ms: None,
                    max_total_tool_calls: None,
                    max_total_tokens: None,
                    max_total_cost_usd: None,
                },
                output_targets: Vec::new(),
                created_at_ms: 1,
                updated_at_ms: 1,
                creator_id: "test".to_string(),
                workspace_root: Some(".".to_string()),
                metadata: None,
                next_fire_at_ms: None,
                last_fired_at_ms: None,
                scope_policy: None,
                watch_conditions: Vec::new(),
                handoff_config: None,
            },
        }
    }

    pub(crate) fn name(mut self, name: impl Into<String>) -> Self {
        self.automation.name = name.into();
        self
    }

    pub(crate) fn nodes(mut self, nodes: Vec<AutomationFlowNode>) -> Self {
        self.automation.flow.nodes = nodes;
        self
    }

    pub(crate) fn metadata(mut self, metadata: Value) -> Self {
        self.automation.metadata = Some(metadata);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn workspace_root(mut self, workspace_root: impl Into<String>) -> Self {
        self.automation.workspace_root = Some(workspace_root.into());
        self
    }

    pub(crate) fn build(self) -> AutomationV2Spec {
        self.automation
    }
}

#[allow(dead_code)]
pub(crate) struct AutomationRunBuilder {
    run: AutomationV2RunRecord,
}

impl AutomationRunBuilder {
    pub(crate) fn new(run_id: impl Into<String>, automation_id: impl Into<String>) -> Self {
        Self {
            run: AutomationV2RunRecord {
                run_id: run_id.into(),
                automation_id: automation_id.into(),
                tenant_context: TenantContext::local_implicit(),
                trigger_type: "manual".to_string(),
                status: AutomationRunStatus::Queued,
                created_at_ms: 1,
                updated_at_ms: 1,
                started_at_ms: None,
                finished_at_ms: None,
                active_session_ids: Vec::new(),
                latest_session_id: None,
                active_instance_ids: Vec::new(),
                checkpoint: AutomationRunCheckpoint {
                    completed_nodes: Vec::new(),
                    pending_nodes: Vec::new(),
                    node_outputs: HashMap::new(),
                    node_attempts: HashMap::new(),
                    blocked_nodes: Vec::new(),
                    awaiting_gate: None,
                    gate_history: Vec::new(),
                    lifecycle_history: Vec::new(),
                    last_failure: None,
                },
                runtime_context: None,
                automation_snapshot: None,
                pause_reason: None,
                resume_reason: None,
                detail: None,
                stop_kind: None,
                stop_reason: None,
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                estimated_cost_usd: 0.0,
                scheduler: None,
                trigger_reason: None,
                consumed_handoff_id: None,
                learning_summary: None,
            },
        }
    }

    #[allow(dead_code)]
    pub(crate) fn status(mut self, status: AutomationRunStatus) -> Self {
        self.run.status = status;
        self
    }

    pub(crate) fn pending_nodes(mut self, pending_nodes: Vec<&str>) -> Self {
        self.run.checkpoint.pending_nodes = pending_nodes.into_iter().map(str::to_string).collect();
        self
    }

    pub(crate) fn completed_nodes(mut self, completed_nodes: Vec<&str>) -> Self {
        self.run.checkpoint.completed_nodes =
            completed_nodes.into_iter().map(str::to_string).collect();
        self
    }

    pub(crate) fn build(self) -> AutomationV2RunRecord {
        self.run
    }
}

pub(crate) fn test_automation_node(
    node_id: &str,
    depends_on: Vec<&str>,
    phase_id: &str,
    priority: i64,
) -> AutomationFlowNode {
    AutomationNodeBuilder::new(node_id)
        .depends_on(depends_on)
        .stage_kind(AutomationNodeStageKind::Workstream)
        .metadata(json!({
            "builder": {
                "phase_id": phase_id,
                "priority": priority
            }
        }))
        .build()
}

pub(crate) fn test_phase_automation(
    phases: Value,
    nodes: Vec<AutomationFlowNode>,
) -> AutomationV2Spec {
    AutomationSpecBuilder::new("auto-phase-test")
        .name("Phase Test")
        .nodes(nodes)
        .metadata(json!({
            "mission": {
                "phases": phases
            }
        }))
        .build()
}

pub(crate) fn test_phase_run(
    pending_nodes: Vec<&str>,
    completed_nodes: Vec<&str>,
) -> AutomationV2RunRecord {
    AutomationRunBuilder::new("run-phase-test", "auto-phase-test")
        .pending_nodes(pending_nodes)
        .completed_nodes(completed_nodes)
        .build()
}

pub(crate) fn test_state_with_path(path: PathBuf) -> AppState {
    let mut state = AppState::new_starting("test-attempt".to_string(), true);
    state.shared_resources_path = path;
    state.routines_path = tmp_routines_file("shared-state");
    state.routine_history_path = tmp_routines_file("routine-history");
    state.routine_runs_path = tmp_routines_file("routine-runs");
    state.external_actions_path = tmp_routines_file("external-actions");
    state
}

pub(crate) async fn ready_test_state() -> AppState {
    let root = std::env::temp_dir().join(format!("tandem-state-test-{}", uuid::Uuid::new_v4()));
    let global = root.join("global-config.json");
    let tandem_home = root.join("tandem-home");
    let mcp_state = root.join("mcp.json");
    std::env::set_var("TANDEM_GLOBAL_CONFIG", &global);
    std::env::set_var("TANDEM_HOME", &tandem_home);
    if let Some(parent) = mcp_state.parent() {
        std::fs::create_dir_all(parent).expect("mcp state dir");
    }
    std::fs::write(&mcp_state, "{}").expect("write mcp state");

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
    let mut state = AppState::new_starting(uuid::Uuid::new_v4().to_string(), false);
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
    state
}

pub(crate) fn tmp_resource_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tandem-server-{name}-{}.json",
        uuid::Uuid::new_v4()
    ))
}

pub(crate) fn tmp_routines_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tandem-server-routines-{name}-{}.json",
        uuid::Uuid::new_v4()
    ))
}

mod automations;
mod handoff;
mod routines;
mod shared_resources;
mod status_index;
