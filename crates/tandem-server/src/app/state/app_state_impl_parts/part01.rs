#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct BugMonitorLogWatcherStateFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    sources: std::collections::HashMap<String, BugMonitorLogSourceState>,
}

fn bug_monitor_log_source_state_key(project_id: &str, source_id: &str) -> String {
    format!("{}/{}", project_id.trim(), source_id.trim())
}

fn is_slug_like(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
}

const TENANT_SCOPED_SHARED_RESOURCE_PREFIX: &str = "__tenant__::";

fn tenant_scope_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn tenant_scoped_shared_resource_key(key: &str, tenant_context: &TenantContext) -> String {
    if tenant_context.is_local_implicit() {
        return key.to_string();
    }
    let deployment = tenant_context.deployment_id.as_deref().unwrap_or("");
    format!(
        "{}{}::{}::{}::{}",
        TENANT_SCOPED_SHARED_RESOURCE_PREFIX,
        tenant_scope_component(&tenant_context.org_id),
        tenant_scope_component(&tenant_context.workspace_id),
        tenant_scope_component(deployment),
        key
    )
}

fn shared_resource_tenant_matches(
    record: &SharedResourceRecord,
    tenant_context: &TenantContext,
) -> bool {
    record.tenant_context.org_id == tenant_context.org_id
        && record.tenant_context.workspace_id == tenant_context.workspace_id
        && record.tenant_context.deployment_id == tenant_context.deployment_id
}

/// Validate that a file has secure permissions (readable only by owner).
/// SECURITY: Prevents world-readable secrets in state files.
/// Logs a warning if permissions are too permissive but does not fail.
#[cfg(unix)]
fn check_file_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode();
            let world_readable = (mode & 0o004) != 0;
            let world_writable = (mode & 0o002) != 0;
            let group_writable = (mode & 0o020) != 0;
            if world_readable || world_writable || group_writable {
                tracing::warn!(
                    target: "tandem_server::state",
                    path = ?path,
                    mode = format!("{:04o}", mode & 0o777),
                    "state file has overly permissive permissions (world/group writable or world readable). \
                     Recommend restricting to 0600 (owner read/write only)."
                );
            }
        }
        Err(e) => {
            tracing::debug!(
                target: "tandem_server::state",
                path = ?path,
                error = ?e,
                "could not check file permissions"
            );
        }
    }
}

#[cfg(not(unix))]
fn check_file_permissions(_path: &std::path::Path) {
    // Windows DACL permissions are checked less frequently in this context.
}

async fn write_state_file_atomically(path: &PathBuf, payload: String) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, payload).await?;
    fs::rename(&tmp, path).await?;
    Ok(())
}

async fn validate_bug_monitor_monitored_projects(
    state: &AppState,
    config: &mut BugMonitorConfig,
) -> anyhow::Result<()> {
    let needs_mcp_validation = config.monitored_projects.iter().any(|project| {
        project
            .mcp_server
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
    });
    let servers = if needs_mcp_validation && state.is_ready() {
        Some(state.mcp.list().await)
    } else {
        None
    };
    let mut project_ids = std::collections::HashSet::new();
    for project in &mut config.monitored_projects {
        project.project_id = project.project_id.trim().to_string();
        project.name = project.name.trim().to_string();
        project.repo = project.repo.trim().to_string();
        project.workspace_root = project.workspace_root.trim().to_string();
        project.mcp_server = project
            .mcp_server
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if !is_slug_like(&project.project_id) {
            anyhow::bail!("monitored project id must be ASCII slug-like");
        }
        if !project_ids.insert(project.project_id.clone()) {
            anyhow::bail!("duplicate monitored project id `{}`", project.project_id);
        }
        if project.name.is_empty() {
            anyhow::bail!(
                "monitored project `{}` name is required",
                project.project_id
            );
        }
        if !is_valid_owner_repo_slug(&project.repo) {
            anyhow::bail!(
                "monitored project `{}` repo must be in owner/repo format",
                project.project_id
            );
        }
        crate::normalize_absolute_workspace_root(&project.workspace_root)
            .map_err(anyhow::Error::msg)?;
        if let Some(server) = project.mcp_server.as_ref() {
            if let Some(servers) = servers.as_ref() {
                if !servers.contains_key(server) {
                    anyhow::bail!(
                        "monitored project `{}` references unknown mcp server `{server}`",
                        project.project_id
                    );
                }
            } else if !state.is_ready() {
                // Unit tests often validate config before runtime wiring exists.
                // Runtime-backed MCP validation still runs in normal ready state.
            } else {
                anyhow::bail!(
                    "monitored project `{}` references unknown mcp server `{server}`",
                    project.project_id
                );
            }
        }
        if let Some(model_policy) = project.model_policy.as_ref() {
            crate::http::routines_automations::validate_model_policy(model_policy)
                .map_err(anyhow::Error::msg)?;
        }

        let mut source_ids = std::collections::HashSet::new();
        for source in &mut project.log_sources {
            source.source_id = source.source_id.trim().to_string();
            source.path = source.path.trim().to_string();
            if !is_slug_like(&source.source_id) {
                anyhow::bail!(
                    "log source id for monitored project `{}` must be ASCII slug-like",
                    project.project_id
                );
            }
            if !source_ids.insert(source.source_id.clone()) {
                anyhow::bail!(
                    "duplicate log source id `{}` in monitored project `{}`",
                    source.source_id,
                    project.project_id
                );
            }
            if source.path.is_empty() {
                anyhow::bail!(
                    "log source `{}` in monitored project `{}` path is required",
                    source.source_id,
                    project.project_id
                );
            }
            let path_project = BugMonitorMonitoredProject {
                project_id: project.project_id.clone(),
                name: project.name.clone(),
                repo: project.repo.clone(),
                workspace_root: project.workspace_root.clone(),
                ..BugMonitorMonitoredProject::default()
            };
            crate::bug_monitor::log_watcher::resolve_log_source_path(&path_project, source)?;
            source.watch_interval_seconds = source.watch_interval_seconds.clamp(1, 86_400);
            source.max_bytes_per_poll = source.max_bytes_per_poll.clamp(1_024, 10 * 1024 * 1024);
            source.max_candidates_per_poll = source.max_candidates_per_poll.clamp(1, 200);
        }
    }
    Ok(())
}

impl AppState {
    pub fn new_starting(attempt_id: String, in_process: bool) -> Self {
        #[cfg(feature = "premium-governance")]
        let governance_engine: Arc<
            dyn tandem_enterprise_contract::governance::GovernancePolicyEngine,
        > = Arc::new(tandem_governance_engine::DefaultGovernanceEngine);
        #[cfg(not(feature = "premium-governance"))]
        let governance_engine: Arc<
            dyn tandem_enterprise_contract::governance::GovernancePolicyEngine,
        > = Arc::new(crate::app::state::governance::UnavailableGovernanceEngine);
        Self {
            runtime: Arc::new(OnceLock::new()),
            startup: Arc::new(RwLock::new(StartupState {
                status: StartupStatus::Starting,
                phase: "boot".to_string(),
                started_at_ms: now_ms(),
                attempt_id,
                last_error: None,
            })),
            in_process_mode: Arc::new(AtomicBool::new(in_process)),
            api_token: Arc::new(RwLock::new(None)),
            engine_leases: Arc::new(RwLock::new(std::collections::HashMap::new())),
            managed_worktrees: Arc::new(RwLock::new(std::collections::HashMap::new())),
            run_registry: RunRegistry::new(),
            run_stale_ms: config::env::resolve_run_stale_ms(),
            memory_records: Arc::new(RwLock::new(std::collections::HashMap::new())),
            memory_audit_log: Arc::new(RwLock::new(Vec::new())),
            memory_db_path: tandem_core::resolve_memory_db_path()
                .unwrap_or_else(|_| std::path::PathBuf::from("memory.sqlite")),
            memory_audit_path: config::paths::resolve_memory_audit_path(),
            protected_audit_path: config::paths::resolve_protected_audit_path(),
            enterprise_org_units: Arc::new(RwLock::new(std::collections::HashMap::new())),
            enterprise_org_units_path: config::paths::resolve_enterprise_org_units_path(),
            enterprise_org_unit_memberships: Arc::new(
                RwLock::new(std::collections::HashMap::new()),
            ),
            enterprise_org_unit_memberships_path:
                config::paths::resolve_enterprise_org_unit_memberships_path(),
            enterprise_org_unit_access_grants: Arc::new(RwLock::new(
                std::collections::HashMap::new(),
            )),
            enterprise_org_unit_access_grants_path:
                config::paths::resolve_enterprise_org_unit_access_grants_path(),
            enterprise_source_bindings: Arc::new(RwLock::new(std::collections::HashMap::new())),
            enterprise_source_bindings_path: config::paths::resolve_enterprise_source_bindings_path(
            ),
            enterprise_connectors: Arc::new(RwLock::new(std::collections::HashMap::new())),
            enterprise_connectors_path: config::paths::resolve_enterprise_connectors_path(),
            enterprise_ingestion_jobs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            enterprise_ingestion_jobs_path: config::paths::resolve_enterprise_ingestion_jobs_path(),
            enterprise_ingestion_quarantines: Arc::new(RwLock::new(
                std::collections::HashMap::new(),
            )),
            enterprise_ingestion_quarantines_path:
                config::paths::resolve_enterprise_ingestion_quarantines_path(),
            missions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shared_resources_path: config::paths::resolve_shared_resources_path(),
            routines: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_history: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automations_v2: Arc::new(RwLock::new(std::collections::HashMap::new())),
            channel_automation_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            channel_user_capabilities: Arc::new(RwLock::new(std::collections::HashMap::new())),
            channel_enrollment_codes: Arc::new(RwLock::new(std::collections::HashMap::new())),
            channel_step_up_grants: Arc::new(RwLock::new(std::collections::HashMap::new())),
            channel_rate_limiter: Arc::new(crate::app::rate_limit::ChannelRateLimiter::default()),
            automation_governance: Arc::new(RwLock::new(
                crate::automation_v2::governance::GovernanceState::default(),
            )),
            governance_engine,
            automation_v2_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_scheduler: Arc::new(RwLock::new(automation::AutomationScheduler::new(
                config::env::resolve_scheduler_max_concurrent_runs(),
            ))),
            automation_scheduler_stopping: Arc::new(AtomicBool::new(false)),
            automations_v2_persistence: Arc::new(tokio::sync::Mutex::new(())),
            workflow_plans: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_plan_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_planner_sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_learning_candidates: Arc::new(RwLock::new(std::collections::HashMap::new())),
            context_packs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            optimization_campaigns: Arc::new(RwLock::new(std::collections::HashMap::new())),
            optimization_experiments: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_config: Arc::new(
                RwLock::new(config::env::resolve_bug_monitor_env_config()),
            ),
            bug_monitor_drafts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_incidents: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_posts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_log_watcher_state_path:
                config::paths::resolve_bug_monitor_log_watcher_state_path(),
            bug_monitor_log_source_states: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_log_watcher_status: Arc::new(RwLock::new(
                BugMonitorLogWatcherStatus::default(),
            )),
            bug_monitor_log_evidence_dir: config::paths::resolve_bug_monitor_log_evidence_dir(),
            bug_monitor_intake_keys: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_intake_keys_path: config::paths::resolve_bug_monitor_intake_keys_path(),
            external_actions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            bug_monitor_runtime_status: Arc::new(RwLock::new(BugMonitorRuntimeStatus::default())),
            provider_oauth_sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            mcp_oauth_sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflows: Arc::new(RwLock::new(WorkflowRegistry::default())),
            workflow_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_hook_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
            workflow_dispatch_seen: Arc::new(RwLock::new(std::collections::HashMap::new())),
            routine_session_policies: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_session_runs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            automation_v2_session_mcp_servers: Arc::new(RwLock::new(
                std::collections::HashMap::new(),
            )),
            routines_path: config::paths::resolve_routines_path(),
            routine_history_path: config::paths::resolve_routine_history_path(),
            routine_runs_path: config::paths::resolve_routine_runs_path(),
            automations_v2_path: config::paths::resolve_automations_v2_path(),
            channel_automation_drafts_path: config::paths::resolve_channel_automation_drafts_path(),
            channel_user_capabilities_path: config::paths::resolve_channel_user_capabilities_path(),
            automation_governance_path: config::paths::resolve_automation_governance_path(),
            automation_v2_runs_path: config::paths::resolve_automation_v2_runs_path(),
            automation_v2_runs_archive_path: config::paths::resolve_automation_v2_runs_archive_path(
            ),
            optimization_campaigns_path: config::paths::resolve_optimization_campaigns_path(),
            optimization_experiments_path: config::paths::resolve_optimization_experiments_path(),
            bug_monitor_config_path: config::paths::resolve_bug_monitor_config_path(),
            bug_monitor_drafts_path: config::paths::resolve_bug_monitor_drafts_path(),
            bug_monitor_incidents_path: config::paths::resolve_bug_monitor_incidents_path(),
            bug_monitor_posts_path: config::paths::resolve_bug_monitor_posts_path(),
            external_actions_path: config::paths::resolve_external_actions_path(),
            workflow_runs_path: config::paths::resolve_workflow_runs_path(),
            workflow_planner_sessions_path: config::paths::resolve_workflow_planner_sessions_path(),
            workflow_learning_candidates_path:
                config::paths::resolve_workflow_learning_candidates_path(),
            context_packs_path: config::paths::resolve_context_packs_path(),
            workflow_hook_overrides_path: config::paths::resolve_workflow_hook_overrides_path(),
            agent_teams: AgentTeamRuntime::new(config::paths::resolve_agent_team_audit_path()),
            web_ui_enabled: Arc::new(AtomicBool::new(false)),
            web_ui_prefix: Arc::new(std::sync::RwLock::new("/admin".to_string())),
            server_base_url: Arc::new(std::sync::RwLock::new("http://127.0.0.1:39731".to_string())),
            channels_runtime: Arc::new(tokio::sync::Mutex::new(ChannelRuntime::default())),
            host_runtime_context: detect_host_runtime_context(),
            token_cost_per_1k_usd: config::env::resolve_token_cost_per_1k_usd(),
            pack_manager: Arc::new(PackManager::new(PackManager::default_root())),
            capability_resolver: Arc::new(CapabilityResolver::new(PackManager::default_root())),
            preset_registry: Arc::new(PresetRegistry::new(
                PackManager::default_root(),
                resolve_shared_paths()
                    .map(|paths| paths.canonical_root)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("."))
                            .join(".tandem")
                    }),
            )),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.runtime.get().is_some()
    }

    pub async fn wait_until_ready_or_failed(&self, attempts: usize, sleep_ms: u64) -> bool {
        for _ in 0..attempts {
            let startup = self.startup_snapshot().await;
            if matches!(startup.status, StartupStatus::Ready) {
                return true;
            }
            if matches!(startup.status, StartupStatus::Failed) {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
        matches!(self.startup_snapshot().await.status, StartupStatus::Ready)
    }

    pub fn mode_label(&self) -> &'static str {
        if self.in_process_mode.load(Ordering::Relaxed) {
            "in-process"
        } else {
            "sidecar"
        }
    }

    pub fn configure_web_ui(&self, enabled: bool, prefix: String) {
        self.web_ui_enabled.store(enabled, Ordering::Relaxed);
        if let Ok(mut guard) = self.web_ui_prefix.write() {
            *guard = config::webui::normalize_web_ui_prefix(&prefix);
        }
    }

    pub fn web_ui_enabled(&self) -> bool {
        self.web_ui_enabled.load(Ordering::Relaxed)
    }

    pub fn web_ui_prefix(&self) -> String {
        self.web_ui_prefix
            .read()
            .map(|v| v.clone())
            .unwrap_or_else(|_| "/admin".to_string())
    }

    pub fn set_server_base_url(&self, base_url: String) {
        if let Ok(mut guard) = self.server_base_url.write() {
            *guard = base_url;
        }
    }

    pub fn server_base_url(&self) -> String {
        self.server_base_url
            .read()
            .map(|v| v.clone())
            .unwrap_or_else(|_| "http://127.0.0.1:39731".to_string())
    }

    pub async fn api_token(&self) -> Option<String> {
        self.api_token.read().await.clone()
    }

    pub async fn set_api_token(&self, token: Option<String>) {
        *self.api_token.write().await = token;
    }

    pub async fn startup_snapshot(&self) -> StartupSnapshot {
        let state = self.startup.read().await.clone();
        StartupSnapshot {
            elapsed_ms: now_ms().saturating_sub(state.started_at_ms),
            status: state.status,
            phase: state.phase,
            started_at_ms: state.started_at_ms,
            attempt_id: state.attempt_id,
            last_error: state.last_error,
        }
    }

    pub fn host_runtime_context(&self) -> HostRuntimeContext {
        self.runtime
            .get()
            .map(|runtime| runtime.host_runtime_context.clone())
            .unwrap_or_else(|| self.host_runtime_context.clone())
    }

    pub async fn set_phase(&self, phase: impl Into<String>) {
        let mut startup = self.startup.write().await;
        startup.phase = phase.into();
    }

    pub async fn mark_ready(&self, runtime: RuntimeState) -> anyhow::Result<()> {
        self.runtime
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("runtime already initialized"))?;
        self.tools
            .register_tool(
                "pack_builder".to_string(),
                Arc::new(crate::pack_builder::PackBuilderTool::new(self.clone())),
            )
            .await;
        self.tools
            .register_tool(
                "mcp_list".to_string(),
                Arc::new(crate::http::mcp::McpListTool::new(self.clone())),
            )
            .await;
        self.tools
            .register_tool(
                "mcp_list_catalog".to_string(),
                Arc::new(crate::http::mcp_discovery::McpListCatalogTool::new(
                    self.clone(),
                )),
            )
            .await;
        self.tools
            .register_tool(
                "mcp_request_capability".to_string(),
                Arc::new(crate::http::mcp_discovery::McpRequestCapabilityTool::new(
                    self.clone(),
                )),
            )
            .await;
        self.engine_loop
            .set_spawn_agent_hook(std::sync::Arc::new(
                crate::agent_teams::ServerSpawnAgentHook::new(self.clone()),
            ))
            .await;
        self.engine_loop
            .set_tool_policy_hook(std::sync::Arc::new(
                crate::agent_teams::ServerToolPolicyHook::new(self.clone()),
            ))
            .await;
        self.engine_loop
            .set_prompt_context_hook(std::sync::Arc::new(ServerPromptContextHook::new(
                self.clone(),
            )))
            .await;
        let _ = self.load_shared_resources().await;
        let _ = self.load_enterprise_org_units().await;
        let _ = self.load_enterprise_org_unit_memberships().await;
        let _ = self.load_enterprise_org_unit_access_grants().await;
        let _ = self.load_enterprise_source_bindings().await;
        let _ = self.load_enterprise_connectors().await;
        let _ = self.load_enterprise_ingestion_jobs().await;
        let _ = self.load_enterprise_ingestion_quarantines().await;
        self.load_routines().await?;
        let _ = self.load_routine_history().await;
        let _ = self.load_routine_runs().await;
        self.load_automations_v2().await?;
        let _ = self.load_channel_automation_drafts().await;
        let _ = self.load_channel_user_capabilities().await;
        let _ = self.load_automation_governance().await;
        let _ = self.bootstrap_automation_governance().await;
        let _ = self.load_automation_v2_runs().await;
        let _ = self.load_optimization_campaigns().await;
        let _ = self.load_optimization_experiments().await;
        let _ = self.load_bug_monitor_config().await;
        let _ = self.load_bug_monitor_drafts().await;
        let _ = self.load_bug_monitor_incidents().await;
        let _ = self.load_bug_monitor_posts().await;
        let _ = self.load_bug_monitor_log_watcher_state().await;
        let _ = self.load_bug_monitor_intake_keys().await;
        let _ = self.load_external_actions().await;
        let _ = self.load_workflow_planner_sessions().await;
        let _ = self.load_workflow_learning_candidates().await;
        let _ = self.load_context_packs().await;
        let _ = self.load_workflow_runs().await;
        let _ = self.load_workflow_hook_overrides().await;
        let _ = self.reload_workflows().await;
        let workspace_root = self.workspace_index.snapshot().await.root;
        let _ = self
            .agent_teams
            .ensure_loaded_for_workspace(&workspace_root)
            .await;
        let mut startup = self.startup.write().await;
        startup.status = StartupStatus::Ready;
        startup.phase = "ready".to_string();
        startup.last_error = None;
        drop(startup);
        #[cfg(feature = "browser")]
        {
            let state = self.clone();
            tokio::spawn(async move {
                if let Err(err) = state.register_browser_tools().await {
                    tracing::warn!("browser tool registration skipped: {}", err);
                }
            });
        }
        Ok(())
    }

    pub async fn load_enterprise_org_units(&self) -> anyhow::Result<()> {
        if !self.enterprise_org_units_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_org_units_path);
        let bytes = fs::read(&self.enterprise_org_units_path).await?;
        let registry: std::collections::HashMap<
            String,
            tandem_enterprise_contract::OrganizationUnit,
        > = serde_json::from_slice(&bytes)?;
        *self.enterprise_org_units.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_org_unit_memberships(&self) -> anyhow::Result<()> {
        if !self.enterprise_org_unit_memberships_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_org_unit_memberships_path);
        let bytes = fs::read(&self.enterprise_org_unit_memberships_path).await?;
        let registry: std::collections::HashMap<
            String,
            tandem_enterprise_contract::OrganizationUnitMembership,
        > = serde_json::from_slice(&bytes)?;
        *self.enterprise_org_unit_memberships.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_org_unit_access_grants(&self) -> anyhow::Result<()> {
        if !self.enterprise_org_unit_access_grants_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_org_unit_access_grants_path);
        let bytes = fs::read(&self.enterprise_org_unit_access_grants_path).await?;
        let registry: std::collections::HashMap<
            String,
            tandem_enterprise_contract::OrganizationUnitAccessGrant,
        > = serde_json::from_slice(&bytes)?;
        *self.enterprise_org_unit_access_grants.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_source_bindings(&self) -> anyhow::Result<()> {
        if !self.enterprise_source_bindings_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_source_bindings_path);
        let bytes = fs::read(&self.enterprise_source_bindings_path).await?;
        let registry: std::collections::HashMap<String, tandem_enterprise_contract::SourceBinding> =
            serde_json::from_slice(&bytes)?;
        *self.enterprise_source_bindings.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_connectors(&self) -> anyhow::Result<()> {
        if !self.enterprise_connectors_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_connectors_path);
        let bytes = fs::read(&self.enterprise_connectors_path).await?;
        let registry: std::collections::HashMap<
            String,
            tandem_enterprise_contract::ConnectorInstance,
        > = serde_json::from_slice(&bytes)?;
        *self.enterprise_connectors.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_ingestion_jobs(&self) -> anyhow::Result<()> {
        if !self.enterprise_ingestion_jobs_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_ingestion_jobs_path);
        let bytes = fs::read(&self.enterprise_ingestion_jobs_path).await?;
        let registry: std::collections::HashMap<String, tandem_enterprise_contract::IngestionJob> =
            serde_json::from_slice(&bytes)?;
        *self.enterprise_ingestion_jobs.write().await = registry;
        Ok(())
    }

    pub async fn load_enterprise_ingestion_quarantines(&self) -> anyhow::Result<()> {
        if !self.enterprise_ingestion_quarantines_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.enterprise_ingestion_quarantines_path);
        let bytes = fs::read(&self.enterprise_ingestion_quarantines_path).await?;
        let registry: std::collections::HashMap<
            String,
            tandem_enterprise_contract::IngestionQuarantine,
        > = serde_json::from_slice(&bytes)?;
        *self.enterprise_ingestion_quarantines.write().await = registry;
        Ok(())
    }

    pub async fn mark_failed(&self, phase: impl Into<String>, error: impl Into<String>) {
        let mut startup = self.startup.write().await;
        startup.status = StartupStatus::Failed;
        startup.phase = phase.into();
        startup.last_error = Some(error.into());
    }

    pub async fn channel_statuses(&self) -> std::collections::HashMap<String, ChannelStatus> {
        let runtime = self.channels_runtime.lock().await;
        let mut status = runtime.statuses.clone();
        let diagnostics = runtime.diagnostics.read().await;
        for spec in registered_channels() {
            let entry = status
                .entry(spec.name.to_string())
                .or_insert(ChannelStatus {
                    enabled: false,
                    connected: false,
                    last_error: None,
                    active_sessions: 0,
                    meta: json!({}),
                });
            let mut meta = entry.meta.as_object().cloned().unwrap_or_default();
            if let Some(diag) = diagnostics.get(spec.name) {
                entry.last_error = diag.last_error.clone().or_else(|| entry.last_error.clone());
                meta.insert("state".to_string(), Value::String(diag.state.to_string()));
                meta.insert(
                    "last_error_code".to_string(),
                    diag.last_error_code
                        .map(|code| Value::String(code.to_string()))
                        .unwrap_or(Value::Null),
                );
                meta.insert(
                    "last_reconnect_at".to_string(),
                    diag.last_reconnect_at
                        .map(|value| Value::Number(value.into()))
                        .unwrap_or(Value::Null),
                );
                meta.insert(
                    "listener_start_count".to_string(),
                    Value::Number(serde_json::Number::from(diag.listener_start_count)),
                );
            } else {
                meta.insert("state".to_string(), Value::String("stopped".to_string()));
                meta.insert("last_error_code".to_string(), Value::Null);
                meta.insert("last_reconnect_at".to_string(), Value::Null);
                meta.insert(
                    "listener_start_count".to_string(),
                    Value::Number(0u64.into()),
                );
            }
            entry.meta = Value::Object(meta);
        }
        status
    }

    pub async fn restart_channel_listeners(&self) -> anyhow::Result<()> {
        let effective = self.config.get_effective_value().await;
        let parsed: EffectiveAppConfig = serde_json::from_value(effective).unwrap_or_default();
        self.configure_web_ui(parsed.web_ui.enabled, parsed.web_ui.path_prefix.clone());

        let diagnostics = tandem_channels::new_channel_runtime_diagnostics();

        let mut runtime = self.channels_runtime.lock().await;
        if let Some(listeners) = runtime.listeners.as_mut() {
            listeners.abort_all();
        }
        runtime.listeners = None;
        runtime.diagnostics = diagnostics.clone();
        runtime.statuses.clear();
        let channels_config_value = serde_json::to_value(&parsed.channels)
            .ok()
            .and_then(|channels| channels.as_object().cloned());

        let mut status_map = std::collections::HashMap::new();
        for spec in registered_channels() {
            let enabled = channels_config_value
                .as_ref()
                .and_then(|channels| channels.get(spec.config_key))
                .and_then(Value::as_object)
                .is_some();
            status_map.insert(
                spec.name.to_string(),
                ChannelStatus {
                    enabled,
                    connected: false,
                    last_error: None,
                    active_sessions: 0,
                    meta: serde_json::json!({}),
                },
            );
        }

        if let Some(channels_cfg) = build_channels_config(self, &parsed.channels).await {
            let listeners = tandem_channels::start_channel_listeners_with_diagnostics(
                channels_cfg,
                diagnostics.clone(),
            )
            .await;
            runtime.listeners = Some(listeners);
            for status in status_map.values_mut() {
                if status.enabled {
                    status.connected = true;
                }
            }
        }

        runtime.statuses = status_map.clone();
        drop(runtime);

        self.event_bus.publish(EngineEvent::new(
            "channel.status.changed",
            serde_json::json!({ "channels": status_map }),
        ));
        Ok(())
    }

    pub async fn load_shared_resources(&self) -> anyhow::Result<()> {
        let Some(raw) =
            read_state_file_with_legacy(&self.shared_resources_path, "shared_resources.json")
                .await?
        else {
            return Ok(());
        };
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, SharedResourceRecord>>(&raw)
                .unwrap_or_default();
        let mut guard = self.shared_resources.write().await;
        *guard = parsed;
        Ok(())
    }

    pub async fn persist_shared_resources(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.shared_resources_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.shared_resources.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.shared_resources_path, payload).await?;
        Ok(())
    }

    pub async fn get_shared_resource(&self, key: &str) -> Option<SharedResourceRecord> {
        self.shared_resources.read().await.get(key).cloned()
    }

    pub async fn get_shared_resource_for_tenant(
        &self,
        key: &str,
        tenant_context: &TenantContext,
    ) -> Option<SharedResourceRecord> {
        let storage_key = tenant_scoped_shared_resource_key(key, tenant_context);
        self.get_shared_resource(&storage_key)
            .await
            .filter(|record| shared_resource_tenant_matches(record, tenant_context))
    }

    pub async fn list_shared_resources(
        &self,
        prefix: Option<&str>,
        limit: usize,
    ) -> Vec<SharedResourceRecord> {
        let limit = limit.clamp(1, 500);
        let mut rows = self
            .shared_resources
            .read()
            .await
            .values()
            .filter(|record| record.tenant_context.is_local_implicit())
            .filter(|record| {
                if let Some(prefix) = prefix {
                    record.key.starts_with(prefix)
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        rows.truncate(limit);
        rows
    }

    pub async fn list_shared_resources_for_tenant(
        &self,
        prefix: Option<&str>,
        limit: usize,
        tenant_context: &TenantContext,
    ) -> Vec<SharedResourceRecord> {
        let limit = limit.clamp(1, 500);
        let mut rows = self
            .shared_resources
            .read()
            .await
            .values()
            .filter(|record| shared_resource_tenant_matches(record, tenant_context))
            .filter(|record| {
                if let Some(prefix) = prefix {
                    record.key.starts_with(prefix)
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.key.cmp(&b.key));
        rows.truncate(limit);
        rows
    }

    pub async fn put_shared_resource(
        &self,
        key: String,
        value: Value,
        if_match_rev: Option<u64>,
        updated_by: String,
        ttl_ms: Option<u64>,
    ) -> Result<SharedResourceRecord, ResourceStoreError> {
        self.put_shared_resource_for_tenant(
            key,
            value,
            if_match_rev,
            updated_by,
            ttl_ms,
            &TenantContext::local_implicit(),
        )
        .await
    }

    pub async fn put_shared_resource_for_tenant(
        &self,
        key: String,
        value: Value,
        if_match_rev: Option<u64>,
        updated_by: String,
        ttl_ms: Option<u64>,
        tenant_context: &TenantContext,
    ) -> Result<SharedResourceRecord, ResourceStoreError> {
        if !is_valid_resource_key(&key) {
            return Err(ResourceStoreError::InvalidKey { key });
        }

        let public_key = key;
        let storage_key = tenant_scoped_shared_resource_key(&public_key, tenant_context);
        let now = now_ms();
        let mut guard = self.shared_resources.write().await;
        let existing = guard.get(&storage_key).cloned();

        if let Some(expected) = if_match_rev {
            let current = existing.as_ref().map(|row| row.rev);
            if current != Some(expected) {
                return Err(ResourceStoreError::RevisionConflict(ResourceConflict {
                    key: public_key,
                    expected_rev: Some(expected),
                    current_rev: current,
                }));
            }
        }

        let next_rev = existing
            .as_ref()
            .map(|row| row.rev.saturating_add(1))
            .unwrap_or(1);

        let record = SharedResourceRecord {
            key: public_key,
            value,
            rev: next_rev,
            updated_at_ms: now,
            updated_by,
            tenant_context: tenant_context.clone(),
            ttl_ms,
        };

        let previous = guard.insert(storage_key.clone(), record.clone());
        drop(guard);

        if let Err(error) = self.persist_shared_resources().await {
            let mut rollback = self.shared_resources.write().await;
            if let Some(previous) = previous {
                rollback.insert(storage_key, previous);
            } else {
                rollback.remove(&storage_key);
            }
            return Err(ResourceStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(record)
    }

    pub async fn delete_shared_resource(
        &self,
        key: &str,
        if_match_rev: Option<u64>,
    ) -> Result<Option<SharedResourceRecord>, ResourceStoreError> {
        self.delete_shared_resource_for_tenant(key, if_match_rev, &TenantContext::local_implicit())
            .await
    }

    pub async fn delete_shared_resource_for_tenant(
        &self,
        key: &str,
        if_match_rev: Option<u64>,
        tenant_context: &TenantContext,
    ) -> Result<Option<SharedResourceRecord>, ResourceStoreError> {
        if !is_valid_resource_key(key) {
            return Err(ResourceStoreError::InvalidKey {
                key: key.to_string(),
            });
        }

        let storage_key = tenant_scoped_shared_resource_key(key, tenant_context);
        let mut guard = self.shared_resources.write().await;
        let current = guard.get(&storage_key).cloned();
        let Some(current) =
            current.filter(|record| shared_resource_tenant_matches(record, tenant_context))
        else {
            return Ok(None);
        };
        if let Some(expected) = if_match_rev {
            if current.rev != expected {
                return Err(ResourceStoreError::RevisionConflict(ResourceConflict {
                    key: key.to_string(),
                    expected_rev: Some(expected),
                    current_rev: Some(current.rev),
                }));
            }
        }

        let removed = guard.remove(&storage_key);
        drop(guard);

        if let Err(error) = self.persist_shared_resources().await {
            if let Some(record) = removed.clone() {
                self.shared_resources
                    .write()
                    .await
                    .insert(storage_key, record);
            }
            return Err(ResourceStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(removed)
    }

    pub async fn load_routines(&self) -> anyhow::Result<()> {
        let Some(raw) = read_state_file_with_legacy(&self.routines_path, "routines.json").await?
        else {
            return Ok(());
        };
        match serde_json::from_str::<std::collections::HashMap<String, RoutineSpec>>(&raw) {
            Ok(parsed) => {
                let mut guard = self.routines.write().await;
                *guard = parsed;
                Ok(())
            }
            Err(primary_err) => {
                let backup_path = config::paths::sibling_backup_path(&self.routines_path);
                if backup_path.exists() {
                    let backup_raw = fs::read_to_string(&backup_path).await?;
                    if let Ok(parsed_backup) = serde_json::from_str::<
                        std::collections::HashMap<String, RoutineSpec>,
                    >(&backup_raw)
                    {
                        let mut guard = self.routines.write().await;
                        *guard = parsed_backup;
                        return Ok(());
                    }
                }
                Err(anyhow::anyhow!(
                    "failed to parse routines store {}: {primary_err}",
                    self.routines_path.display()
                ))
            }
        }
    }

    pub async fn load_routine_history(&self) -> anyhow::Result<()> {
        if !self.routine_history_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.routine_history_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, Vec<RoutineHistoryEvent>>,
        >(&raw)
        .unwrap_or_default();
        let mut guard = self.routine_history.write().await;
        *guard = parsed;
        Ok(())
    }

    pub async fn load_routine_runs(&self) -> anyhow::Result<()> {
        let Some(raw) =
            read_state_file_with_legacy(&self.routine_runs_path, "routine_runs.json").await?
        else {
            return Ok(());
        };
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, RoutineRunRecord>>(&raw)
                .unwrap_or_default();
        let mut guard = self.routine_runs.write().await;
        *guard = parsed;
        Ok(())
    }

    async fn persist_routines_inner(&self, allow_empty_overwrite: bool) -> anyhow::Result<()> {
        if let Some(parent) = self.routines_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (payload, is_empty) = {
            let guard = self.routines.read().await;
            (serde_json::to_string_pretty(&*guard)?, guard.is_empty())
        };
        if is_empty && !allow_empty_overwrite && self.routines_path.exists() {
            let existing_raw = fs::read_to_string(&self.routines_path)
                .await
                .unwrap_or_default();
            let existing_has_rows = serde_json::from_str::<
                std::collections::HashMap<String, RoutineSpec>,
            >(&existing_raw)
            .map(|rows| !rows.is_empty())
            .unwrap_or(true);
            if existing_has_rows {
                return Err(anyhow::anyhow!(
                    "refusing to overwrite non-empty routines store {} with empty in-memory state",
                    self.routines_path.display()
                ));
            }
        }
        let backup_path = config::paths::sibling_backup_path(&self.routines_path);
        if self.routines_path.exists() {
            let _ = fs::copy(&self.routines_path, &backup_path).await;
        }
        let tmp_path = config::paths::sibling_tmp_path(&self.routines_path);
        fs::write(&tmp_path, payload).await?;
        fs::rename(&tmp_path, &self.routines_path).await?;
        Ok(())
    }

    pub async fn persist_routines(&self) -> anyhow::Result<()> {
        self.persist_routines_inner(false).await
    }

    pub async fn persist_routine_history(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_history_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.routine_history.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.routine_history_path, payload).await?;
        Ok(())
    }

    pub async fn persist_routine_runs(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.routine_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.routine_runs.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.routine_runs_path, payload).await?;
        Ok(())
    }

    pub async fn put_routine(
        &self,
        mut routine: RoutineSpec,
    ) -> Result<RoutineSpec, RoutineStoreError> {
        if routine.routine_id.trim().is_empty() {
            return Err(RoutineStoreError::InvalidRoutineId {
                routine_id: routine.routine_id,
            });
        }

        routine.allowed_tools = config::channels::normalize_allowed_tools(routine.allowed_tools);
        routine.output_targets = normalize_non_empty_list(routine.output_targets);

        let now = now_ms();
        let next_schedule_fire =
            compute_next_schedule_fire_at_ms(&routine.schedule, &routine.timezone, now)
                .ok_or_else(|| RoutineStoreError::InvalidSchedule {
                    detail: "invalid schedule or timezone".to_string(),
                })?;
        match routine.schedule {
            RoutineSchedule::IntervalSeconds { seconds } => {
                if seconds == 0 {
                    return Err(RoutineStoreError::InvalidSchedule {
                        detail: "interval_seconds must be > 0".to_string(),
                    });
                }
                let _ = seconds;
            }
            RoutineSchedule::Cron { .. } => {}
        }
        if routine.next_fire_at_ms.is_none() {
            routine.next_fire_at_ms = Some(next_schedule_fire);
        }

        let mut guard = self.routines.write().await;
        let previous = guard.insert(routine.routine_id.clone(), routine.clone());
        drop(guard);

        if let Err(error) = self.persist_routines().await {
            let mut rollback = self.routines.write().await;
            if let Some(previous) = previous {
                rollback.insert(previous.routine_id.clone(), previous);
            } else {
                rollback.remove(&routine.routine_id);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }

        Ok(routine)
    }

    pub async fn list_routines(&self) -> Vec<RoutineSpec> {
        let mut rows = self
            .routines
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.routine_id.cmp(&b.routine_id));
        rows
    }

    pub async fn get_routine(&self, routine_id: &str) -> Option<RoutineSpec> {
        self.routines.read().await.get(routine_id).cloned()
    }

    pub async fn delete_routine(
        &self,
        routine_id: &str,
    ) -> Result<Option<RoutineSpec>, RoutineStoreError> {
        let mut guard = self.routines.write().await;
        let removed = guard.remove(routine_id);
        drop(guard);

        let allow_empty_overwrite = self.routines.read().await.is_empty();
        if let Err(error) = self.persist_routines_inner(allow_empty_overwrite).await {
            if let Some(removed) = removed.clone() {
                self.routines
                    .write()
                    .await
                    .insert(removed.routine_id.clone(), removed);
            }
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(removed)
    }

    pub async fn evaluate_routine_misfires(&self, now_ms: u64) -> Vec<RoutineTriggerPlan> {
        let mut plans = Vec::new();
        let mut guard = self.routines.write().await;
        for routine in guard.values_mut() {
            if routine.status != RoutineStatus::Active {
                continue;
            }
            let Some(next_fire_at_ms) = routine.next_fire_at_ms else {
                continue;
            };
            if now_ms < next_fire_at_ms {
                continue;
            }
            let (run_count, next_fire_at_ms) = compute_misfire_plan_for_schedule(
                now_ms,
                next_fire_at_ms,
                &routine.schedule,
                &routine.timezone,
                &routine.misfire_policy,
            );
            routine.next_fire_at_ms = Some(next_fire_at_ms);
            if run_count == 0 {
                continue;
            }
            plans.push(RoutineTriggerPlan {
                routine_id: routine.routine_id.clone(),
                run_count,
                scheduled_at_ms: now_ms,
                next_fire_at_ms,
            });
        }
        drop(guard);
        let _ = self.persist_routines().await;
        plans
    }

    pub async fn mark_routine_fired(
        &self,
        routine_id: &str,
        fired_at_ms: u64,
    ) -> Option<RoutineSpec> {
        let mut guard = self.routines.write().await;
        let routine = guard.get_mut(routine_id)?;
        routine.last_fired_at_ms = Some(fired_at_ms);
        let updated = routine.clone();
        drop(guard);
        let _ = self.persist_routines().await;
        Some(updated)
    }

    pub async fn append_routine_history(&self, event: RoutineHistoryEvent) {
        let mut history = self.routine_history.write().await;
        history
            .entry(event.routine_id.clone())
            .or_default()
            .push(event);
        drop(history);
        let _ = self.persist_routine_history().await;
    }

    pub async fn list_routine_history(
        &self,
        routine_id: &str,
        limit: usize,
    ) -> Vec<RoutineHistoryEvent> {
        let limit = limit.clamp(1, 500);
        let mut rows = self
            .routine_history
            .read()
            .await
            .get(routine_id)
            .cloned()
            .unwrap_or_default();
        rows.sort_by(|a, b| b.fired_at_ms.cmp(&a.fired_at_ms));
        rows.truncate(limit);
        rows
    }

    pub async fn create_routine_run(
        &self,
        routine: &RoutineSpec,
        trigger_type: &str,
        run_count: u32,
        status: RoutineRunStatus,
        detail: Option<String>,
    ) -> RoutineRunRecord {
        let now = now_ms();
        let record = RoutineRunRecord {
            run_id: format!("routine-run-{}", uuid::Uuid::new_v4()),
            routine_id: routine.routine_id.clone(),
            trigger_type: trigger_type.to_string(),
            run_count,
            status,
            created_at_ms: now,
            updated_at_ms: now,
            fired_at_ms: Some(now),
            started_at_ms: None,
            finished_at_ms: None,
            requires_approval: routine.requires_approval,
            approval_reason: None,
            denial_reason: None,
            paused_reason: None,
            detail,
            entrypoint: routine.entrypoint.clone(),
            args: routine.args.clone(),
            allowed_tools: routine.allowed_tools.clone(),
            output_targets: routine.output_targets.clone(),
            artifacts: Vec::new(),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
        };
        self.routine_runs
            .write()
            .await
            .insert(record.run_id.clone(), record.clone());
        let _ = self.persist_routine_runs().await;
        record
    }

    pub async fn get_routine_run(&self, run_id: &str) -> Option<RoutineRunRecord> {
        self.routine_runs.read().await.get(run_id).cloned()
    }

    pub async fn list_routine_runs(
        &self,
        routine_id: Option<&str>,
        limit: usize,
    ) -> Vec<RoutineRunRecord> {
        let mut rows = self
            .routine_runs
            .read()
            .await
            .values()
            .filter(|row| {
                if let Some(id) = routine_id {
                    row.routine_id == id
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 500));
        rows
    }

    pub async fn claim_next_queued_routine_run(&self) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let next_run_id = guard
            .values()
            .filter(|row| row.status == RoutineRunStatus::Queued)
            .min_by(|a, b| {
                a.created_at_ms
                    .cmp(&b.created_at_ms)
                    .then_with(|| a.run_id.cmp(&b.run_id))
            })
            .map(|row| row.run_id.clone())?;
        let now = now_ms();
        let row = guard.get_mut(&next_run_id)?;
        row.status = RoutineRunStatus::Running;
        row.updated_at_ms = now;
        row.started_at_ms = Some(now);
        let claimed = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(claimed)
    }

    pub async fn set_routine_session_policy(
        &self,
        session_id: String,
        run_id: String,
        routine_id: String,
        allowed_tools: Vec<String>,
    ) {
        let policy = RoutineSessionPolicy {
            session_id: session_id.clone(),
            run_id,
            routine_id,
            allowed_tools: config::channels::normalize_allowed_tools(allowed_tools),
        };
        self.routine_session_policies
            .write()
            .await
            .insert(session_id, policy);
    }

    pub async fn routine_session_policy(&self, session_id: &str) -> Option<RoutineSessionPolicy> {
        self.routine_session_policies
            .read()
            .await
            .get(session_id)
            .cloned()
    }

    pub async fn clear_routine_session_policy(&self, session_id: &str) {
        self.routine_session_policies
            .write()
            .await
            .remove(session_id);
    }

    pub async fn update_routine_run_status(
        &self,
        run_id: &str,
        status: RoutineRunStatus,
        reason: Option<String>,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.status = status.clone();
        row.updated_at_ms = now_ms();
        match status {
            RoutineRunStatus::PendingApproval => row.approval_reason = reason,
            RoutineRunStatus::Running => {
                row.started_at_ms.get_or_insert_with(now_ms);
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            RoutineRunStatus::Denied => row.denial_reason = reason,
            RoutineRunStatus::Paused => row.paused_reason = reason,
            RoutineRunStatus::Completed
            | RoutineRunStatus::Failed
            | RoutineRunStatus::Cancelled => {
                row.finished_at_ms = Some(now_ms());
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
            _ => {
                if let Some(detail) = reason {
                    row.detail = Some(detail);
                }
            }
        }
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn append_routine_run_artifact(
        &self,
        run_id: &str,
        artifact: RoutineRunArtifact,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.updated_at_ms = now_ms();
        row.artifacts.push(artifact);
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn add_active_session_id(
        &self,
        run_id: &str,
        session_id: String,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        if !row.active_session_ids.iter().any(|id| id == &session_id) {
            row.active_session_ids.push(session_id);
        }
        row.latest_session_id = row.active_session_ids.last().cloned();
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn clear_active_session_id(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<RoutineRunRecord> {
        let mut guard = self.routine_runs.write().await;
        let row = guard.get_mut(run_id)?;
        row.active_session_ids.retain(|id| id != session_id);
        row.updated_at_ms = now_ms();
        let updated = row.clone();
        drop(guard);
        let _ = self.persist_routine_runs().await;
        Some(updated)
    }

    pub async fn load_automations_v2(&self) -> anyhow::Result<()> {
        let mut merged = std::collections::HashMap::<String, AutomationV2Spec>::new();
        let mut loaded_from_alternate = false;
        let mut migrated = false;
        let mut path_counts = Vec::new();
        let mut canonical_loaded = false;
        let shard_root = automation_v2_definitions_root(&self.automations_v2_path);
        let sharded = load_automation_v2_definition_shards(&self.automations_v2_path).await?;
        let shards_loaded = !sharded.is_empty();
        if shards_loaded {
            path_counts.push((shard_root, sharded.len()));
            canonical_loaded = true;
            merged = sharded;
        } else if self.automations_v2_path.exists() {
            let raw = fs::read_to_string(&self.automations_v2_path).await?;
            if raw.trim().is_empty() || raw.trim() == "{}" {
                path_counts.push((self.automations_v2_path.clone(), 0usize));
            } else {
                let parsed = parse_automation_v2_file(&raw);
                path_counts.push((self.automations_v2_path.clone(), parsed.len()));
                canonical_loaded = !parsed.is_empty();
                merged = parsed;
            }
        } else {
            path_counts.push((self.automations_v2_path.clone(), 0usize));
        }
        if !canonical_loaded {
            for path in candidate_automations_v2_paths(&self.automations_v2_path) {
                if path == self.automations_v2_path {
                    continue;
                }
                if !path.exists() {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let raw = fs::read_to_string(&path).await?;
                if raw.trim().is_empty() || raw.trim() == "{}" {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let parsed = parse_automation_v2_file(&raw);
                path_counts.push((path.clone(), parsed.len()));
                if !parsed.is_empty() {
                    loaded_from_alternate = true;
                }
                for (automation_id, automation) in parsed {
                    match merged.get(&automation_id) {
                        Some(existing) if existing.updated_at_ms > automation.updated_at_ms => {}
                        _ => {
                            merged.insert(automation_id, automation);
                        }
                    }
                }
            }
        } else {
            for path in candidate_automations_v2_paths(&self.automations_v2_path) {
                if path == self.automations_v2_path {
                    continue;
                }
                if !path.exists() {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let raw = fs::read_to_string(&path).await?;
                let count = if raw.trim().is_empty() || raw.trim() == "{}" {
                    0usize
                } else {
                    parse_automation_v2_file(&raw).len()
                };
                path_counts.push((path, count));
            }
        }
        let active_path = self.automations_v2_path.display().to_string();
        let path_count_summary = path_counts
            .iter()
            .map(|(path, count)| format!("{}={count}", path.display()))
            .collect::<Vec<_>>();
        tracing::info!(
            active_path,
            canonical_loaded,
            path_counts = ?path_count_summary,
            merged_count = merged.len(),
            "loaded automation v2 definitions"
        );
        for automation in merged.values_mut() {
            migrated = migrate_bundled_studio_research_split_automation(automation) || migrated;
            migrated = canonicalize_automation_output_paths(automation) || migrated;
            migrated = repair_automation_output_contracts(automation) || migrated;
        }
        *self.automations_v2.write().await = merged;
        if loaded_from_alternate || migrated || !shards_loaded {
            let _ = self.persist_automations_v2().await;
        } else if canonical_loaded {
            let _ = archive_automation_v2_aggregate_file(&self.automations_v2_path).await;
            let _ = cleanup_stale_legacy_automations_v2_file(&self.automations_v2_path).await;
        }
        Ok(())
    }

    pub async fn persist_automations_v2(&self) -> anyhow::Result<()> {
        let _guard = self.automations_v2_persistence.lock().await;
        self.persist_automations_v2_locked().await
    }

    async fn persist_automations_v2_locked(&self) -> anyhow::Result<()> {
        let automations = { self.automations_v2.read().await.clone() };
        persist_automation_v2_definition_shards(&self.automations_v2_path, &automations).await?;
        archive_automation_v2_aggregate_file(&self.automations_v2_path).await?;
        let _ = cleanup_stale_legacy_automations_v2_file(&self.automations_v2_path).await;
        Ok(())
    }

    pub async fn load_automation_v2_runs(&self) -> anyhow::Result<()> {
        let mut merged = std::collections::HashMap::<String, AutomationV2RunRecord>::new();
        let mut loaded_from_alternate = false;
        let mut canonical_loaded = false;
        let mut path_counts = Vec::new();
        if self.automation_v2_runs_path.exists() {
            let raw = fs::read_to_string(&self.automation_v2_runs_path).await?;
            if raw.trim().is_empty() || raw.trim() == "{}" {
                path_counts.push((self.automation_v2_runs_path.clone(), 0usize));
            } else {
                let parsed = parse_automation_v2_runs_file(&raw);
                path_counts.push((self.automation_v2_runs_path.clone(), parsed.len()));
                canonical_loaded = !parsed.is_empty();
                merged = parsed;
            }
        } else {
            path_counts.push((self.automation_v2_runs_path.clone(), 0usize));
        }
        if !canonical_loaded {
            for path in candidate_automation_v2_runs_paths(&self.automation_v2_runs_path) {
                if path == self.automation_v2_runs_path {
                    continue;
                }
                if !path.exists() {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let raw = fs::read_to_string(&path).await?;
                if raw.trim().is_empty() || raw.trim() == "{}" {
                    path_counts.push((path, 0usize));
                    continue;
                }
                let parsed = parse_automation_v2_runs_file(&raw);
                path_counts.push((path.clone(), parsed.len()));
                if !parsed.is_empty() {
                    loaded_from_alternate = true;
                }
                for (run_id, run) in parsed {
                    match merged.get(&run_id) {
                        Some(existing) if existing.updated_at_ms > run.updated_at_ms => {}
                        _ => {
                            merged.insert(run_id, run);
                        }
                    }
                }
            }
        } else {
            for path in candidate_automation_v2_runs_paths(&self.automation_v2_runs_path) {
                if path == self.automation_v2_runs_path {
                    continue;
                }
                path_counts.push((path.clone(), usize::from(path.exists())));
            }
        }
        let active_runs_path = self.automation_v2_runs_path.display().to_string();
        let run_path_count_summary = path_counts
            .iter()
            .map(|(path, count)| format!("{}={count}", path.display()))
            .collect::<Vec<_>>();
        tracing::info!(
            active_path = active_runs_path,
            canonical_loaded,
            path_counts = ?run_path_count_summary,
            merged_count = merged.len(),
            "loaded automation v2 runs"
        );
        *self.automation_v2_runs.write().await = merged;
        let recovered = self
            .recover_automation_definitions_from_run_snapshots()
            .await?;
        let automation_count = self.automations_v2.read().await.len();
        let run_count = self.automation_v2_runs.read().await.len();
        if automation_count == 0 && run_count > 0 {
            let active_automations_path = self.automations_v2_path.display().to_string();
            let active_runs_path = self.automation_v2_runs_path.display().to_string();
            tracing::warn!(
                active_automations_path,
                active_runs_path,
                run_count,
                "automation v2 definitions are empty while run history exists"
            );
        }
        if loaded_from_alternate || recovered > 0 {
            let _ = self.persist_automation_v2_runs().await;
        } else if canonical_loaded {
            let _ =
                cleanup_stale_legacy_automation_v2_runs_file(&self.automation_v2_runs_path).await;
        }
        Ok(())
    }

    pub async fn persist_automation_v2_runs(&self) -> anyhow::Result<()> {
        let (runs_snapshot, automations_snapshot) = {
            let runs = self.automation_v2_runs.read().await;
            let automations = self.automations_v2.read().await;
            (runs.clone(), automations.clone())
        };
        for run in runs_snapshot.values() {
            write_automation_v2_run_history_shard(&self.automation_v2_runs_path, run).await?;
        }
        let mut compacted = runs_snapshot;
        compact_automation_v2_runs_for_hot_storage(
            &mut compacted,
            &automations_snapshot,
            automation_v2_hot_cutoff_ms(),
        );
        let payload = serde_json::to_string_pretty(&compacted)?;
        if let Some(parent) = self.automation_v2_runs_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.automation_v2_runs_path, &payload).await?;
        let _ = cleanup_stale_legacy_automation_v2_runs_file(&self.automation_v2_runs_path).await;
        Ok(())
    }

    // Move old terminal automation runs out of the hot in-memory/index set.
    // Full run records are preserved as per-run history shards under
    // data/automation-runs/YYYY/MM/.
    pub async fn archive_stale_automation_v2_runs(
        &self,
        retention_days: u64,
    ) -> anyhow::Result<usize> {
        let cutoff_ms = {
            let now = now_ms();
            let window = retention_days.saturating_mul(24 * 60 * 60 * 1000);
            now.saturating_sub(window)
        };
        let archived: std::collections::HashMap<String, AutomationV2RunRecord> = {
            let mut guard = self.automation_v2_runs.write().await;
            let stale_ids: Vec<String> = guard
                .iter()
                .filter(|(_, run)| {
                    matches!(
                        run.status,
                        AutomationRunStatus::Completed
                            | AutomationRunStatus::Failed
                            | AutomationRunStatus::Blocked
                            | AutomationRunStatus::Cancelled
                    ) && run.updated_at_ms <= cutoff_ms
                })
                .map(|(id, _)| id.clone())
                .collect();
            let mut archived = std::collections::HashMap::new();
            for id in &stale_ids {
                if let Some(run) = guard.remove(id) {
                    archived.insert(id.clone(), run);
                }
            }
            archived
        };
        if archived.is_empty() {
            return Ok(0);
        }
        let archived_count = archived.len();
        for run in archived.values() {
            write_automation_v2_run_history_shard(&self.automation_v2_runs_path, run).await?;
        }

        // Persist the shrunk hot file so the next startup loads a small map.
        self.persist_automation_v2_runs().await?;

        tracing::info!(
            archived = archived_count,
            retention_days,
            history_root = %automation_v2_run_history_root(&self.automation_v2_runs_path).display(),
            "moved stale automation v2 runs to history shards"
        );
        Ok(archived_count)
    }

    pub async fn load_optimization_campaigns(&self) -> anyhow::Result<()> {
        if !self.optimization_campaigns_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.optimization_campaigns_path).await?;
        let parsed = parse_optimization_campaigns_file(&raw);
        *self.optimization_campaigns.write().await = parsed;
        Ok(())
    }

    pub async fn persist_optimization_campaigns(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.optimization_campaigns.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.optimization_campaigns_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.optimization_campaigns_path, payload).await?;
        Ok(())
    }

    pub async fn load_optimization_experiments(&self) -> anyhow::Result<()> {
        if !self.optimization_experiments_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.optimization_experiments_path).await?;
        let parsed = parse_optimization_experiments_file(&raw);
        *self.optimization_experiments.write().await = parsed;
        Ok(())
    }

    pub async fn persist_optimization_experiments(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.optimization_experiments.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.optimization_experiments_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.optimization_experiments_path, payload).await?;
        Ok(())
    }

    async fn verify_automation_v2_persisted_locked(
        &self,
        automation_id: &str,
        expected_present: bool,
    ) -> anyhow::Result<()> {
        let active_shards = load_automation_v2_definition_shards(&self.automations_v2_path).await?;
        let (active_present, active_count) = if !active_shards.is_empty() {
            (
                active_shards.contains_key(automation_id),
                active_shards.len(),
            )
        } else {
            let active_raw = if self.automations_v2_path.exists() {
                fs::read_to_string(&self.automations_v2_path).await?
            } else {
                String::new()
            };
            let active_parsed = parse_automation_v2_file_strict(&active_raw).map_err(|error| {
                anyhow::anyhow!(
                    "failed to parse automation v2 persistence file `{}` during verification: {}",
                    self.automations_v2_path.display(),
                    error
                )
            })?;
            (
                active_parsed.contains_key(automation_id),
                active_parsed.len(),
            )
        };
        if active_present != expected_present {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::error!(
                automation_id,
                expected_present,
                actual_present = active_present,
                count = active_count,
                active_path,
                "automation v2 persistence verification failed"
            );
            anyhow::bail!(
                "automation v2 persistence verification failed for `{}`",
                automation_id
            );
        }
        let mut alternate_mismatches = Vec::new();
        for path in candidate_automations_v2_paths(&self.automations_v2_path) {
            if path == self.automations_v2_path {
                continue;
            }
            let raw = if path.exists() {
                fs::read_to_string(&path).await?
            } else {
                String::new()
            };
            let parsed = match parse_automation_v2_file_strict(&raw) {
                Ok(parsed) => parsed,
                Err(error) => {
                    alternate_mismatches.push(format!(
                        "{} expected_present={} parse_error={error}",
                        path.display(),
                        expected_present
                    ));
                    continue;
                }
            };
            let present = parsed.contains_key(automation_id);
            if present != expected_present {
                alternate_mismatches.push(format!(
                    "{} expected_present={} actual_present={} count={}",
                    path.display(),
                    expected_present,
                    present,
                    parsed.len()
                ));
            }
        }
        if !alternate_mismatches.is_empty() {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::warn!(
                automation_id,
                expected_present,
                mismatches = ?alternate_mismatches,
                active_path,
                "automation v2 alternate persistence paths are stale"
            );
        }
        Ok(())
    }

    async fn recover_automation_definitions_from_run_snapshots(&self) -> anyhow::Result<usize> {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut guard = self.automations_v2.write().await;
        let mut recovered = 0usize;
        for run in runs {
            let Some(snapshot) = run.automation_snapshot.clone() else {
                continue;
            };
            let should_replace = match guard.get(&run.automation_id) {
                Some(existing) => existing.updated_at_ms < snapshot.updated_at_ms,
                None => true,
            };
            if should_replace {
                if !guard.contains_key(&run.automation_id) {
                    recovered += 1;
                }
                guard.insert(run.automation_id.clone(), snapshot);
            }
        }
        drop(guard);
        if recovered > 0 {
            let active_path = self.automations_v2_path.display().to_string();
            tracing::warn!(
                recovered,
                active_path,
                "recovered automation v2 definitions from run snapshots"
            );
            self.persist_automations_v2().await?;
        }
        Ok(recovered)
    }

    pub async fn load_bug_monitor_config(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_config_path.exists() {
            self.bug_monitor_config_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_config.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_config.json")
        } else {
            return Ok(());
        };
        check_file_permissions(&path);
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<BugMonitorConfig>(&raw)
            .unwrap_or_else(|_| config::env::resolve_bug_monitor_env_config());
        *self.bug_monitor_config.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_config(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_config_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_config.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_config_path, payload).await?;
        Ok(())
    }

    pub async fn bug_monitor_config(&self) -> BugMonitorConfig {
        self.bug_monitor_config.read().await.clone()
    }

    pub async fn put_bug_monitor_config(
        &self,
        mut config: BugMonitorConfig,
    ) -> anyhow::Result<BugMonitorConfig> {
        config.workspace_root = config
            .workspace_root
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if let Some(repo) = config.repo.as_ref() {
            if !repo.is_empty() && !is_valid_owner_repo_slug(repo) {
                anyhow::bail!("repo must be in owner/repo format");
            }
        }
        if let Some(server) = config.mcp_server.as_ref() {
            let servers = self.mcp.list().await;
            if !servers.contains_key(server) {
                anyhow::bail!("unknown mcp server `{server}`");
            }
        }
        if let Some(model_policy) = config.model_policy.as_ref() {
            crate::http::routines_automations::validate_model_policy(model_policy)
                .map_err(anyhow::Error::msg)?;
        }
        validate_bug_monitor_monitored_projects(self, &mut config).await?;
        config.updated_at_ms = now_ms();
        *self.bug_monitor_config.write().await = config.clone();
        self.persist_bug_monitor_config().await?;
        Ok(config)
    }

    pub async fn load_bug_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if !self.bug_monitor_log_watcher_state_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.bug_monitor_log_watcher_state_path);
        let raw = fs::read_to_string(&self.bug_monitor_log_watcher_state_path).await?;
        let parsed =
            serde_json::from_str::<BugMonitorLogWatcherStateFile>(&raw).unwrap_or_default();
        *self.bug_monitor_log_source_states.write().await = parsed.sources;
        Ok(())
    }

    pub async fn persist_bug_monitor_log_watcher_state(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_log_watcher_state_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_log_source_states.read().await;
            serde_json::to_string_pretty(&BugMonitorLogWatcherStateFile {
                schema_version: 1,
                sources: guard.clone(),
            })?
        };
        write_state_file_atomically(&self.bug_monitor_log_watcher_state_path, payload).await
    }

    pub async fn get_bug_monitor_log_source_state(
        &self,
        project_id: &str,
        source_id: &str,
    ) -> Option<BugMonitorLogSourceState> {
        self.bug_monitor_log_source_states
            .read()
            .await
            .get(&bug_monitor_log_source_state_key(project_id, source_id))
            .cloned()
    }

    pub async fn put_bug_monitor_log_source_state(
        &self,
        source_state: BugMonitorLogSourceState,
    ) -> anyhow::Result<BugMonitorLogSourceState> {
        let key =
            bug_monitor_log_source_state_key(&source_state.project_id, &source_state.source_id);
        self.bug_monitor_log_source_states
            .write()
            .await
            .insert(key, source_state.clone());
        self.persist_bug_monitor_log_watcher_state().await?;
        Ok(source_state)
    }

    pub async fn update_bug_monitor_log_watcher_status(
        &self,
        update: impl FnOnce(&mut BugMonitorLogWatcherStatus),
    ) -> BugMonitorLogWatcherStatus {
        let mut guard = self.bug_monitor_log_watcher_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn load_bug_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if !self.bug_monitor_intake_keys_path.exists() {
            return Ok(());
        }
        check_file_permissions(&self.bug_monitor_intake_keys_path);
        let raw = fs::read_to_string(&self.bug_monitor_intake_keys_path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, BugMonitorProjectIntakeKey>,
        >(&raw)
        .unwrap_or_default();
        *self.bug_monitor_intake_keys.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_intake_keys(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_intake_keys_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_intake_keys.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        write_state_file_atomically(&self.bug_monitor_intake_keys_path, payload).await
    }

    pub async fn list_bug_monitor_intake_keys(&self) -> Vec<BugMonitorProjectIntakeKey> {
        let mut rows = self
            .bug_monitor_intake_keys
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.project_id.cmp(&b.project_id).then(a.name.cmp(&b.name)));
        rows
    }

    pub async fn put_bug_monitor_intake_key(
        &self,
        key: BugMonitorProjectIntakeKey,
    ) -> anyhow::Result<BugMonitorProjectIntakeKey> {
        self.bug_monitor_intake_keys
            .write()
            .await
            .insert(key.key_id.clone(), key.clone());
        self.persist_bug_monitor_intake_keys().await?;
        Ok(key)
    }

    pub async fn validate_bug_monitor_intake_key(
        &self,
        raw_key: &str,
        project_id: &str,
        required_scope: &str,
    ) -> Option<BugMonitorProjectIntakeKey> {
        let key_hash = crate::sha256_hex(&[raw_key.trim()]);
        let mut matched = {
            self.bug_monitor_intake_keys
                .read()
                .await
                .values()
                .find(|row| {
                    row.enabled
                        && row.project_id == project_id
                        && row.key_hash == key_hash
                        && row.scopes.iter().any(|scope| scope == required_scope)
                })
                .cloned()
        }?;
        matched.last_used_at_ms = Some(now_ms());
        let _ = self.put_bug_monitor_intake_key(matched.clone()).await;
        Some(matched)
    }

    pub async fn load_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_drafts_path.exists() {
            self.bug_monitor_drafts_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_drafts.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_drafts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorDraftRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_drafts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_drafts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_drafts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_drafts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_drafts_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_incidents_path.exists() {
            self.bug_monitor_incidents_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_incidents.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_incidents.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed = serde_json::from_str::<
            std::collections::HashMap<String, BugMonitorIncidentRecord>,
        >(&raw)
        .unwrap_or_default();
        *self.bug_monitor_incidents.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_incidents(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_incidents_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_incidents.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_incidents_path, payload).await?;
        Ok(())
    }

    pub async fn load_bug_monitor_posts(&self) -> anyhow::Result<()> {
        let path = if self.bug_monitor_posts_path.exists() {
            self.bug_monitor_posts_path.clone()
        } else if let Some(path) =
            config::paths::resolve_legacy_root_file_path("bug_monitor_posts.json")
        {
            if path.exists() {
                path
            } else if config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
                .exists()
            {
                config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
            } else {
                return Ok(());
            }
        } else if config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
            .exists()
        {
            config::paths::legacy_failure_reporter_path("failure_reporter_posts.json")
        } else {
            return Ok(());
        };
        let raw = fs::read_to_string(path).await?;
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, BugMonitorPostRecord>>(&raw)
                .unwrap_or_default();
        *self.bug_monitor_posts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_bug_monitor_posts(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.bug_monitor_posts_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.bug_monitor_posts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.bug_monitor_posts_path, payload).await?;
        Ok(())
    }

    pub async fn load_external_actions(&self) -> anyhow::Result<()> {
        let Some(raw) =
            read_state_file_with_legacy(&self.external_actions_path, "external_actions.json")
                .await?
        else {
            return Ok(());
        };
        let parsed =
            serde_json::from_str::<std::collections::HashMap<String, ExternalActionRecord>>(&raw)
                .unwrap_or_default();
        *self.external_actions.write().await = parsed;
        Ok(())
    }

    pub async fn persist_external_actions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.external_actions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.external_actions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.external_actions_path, payload).await?;
        Ok(())
    }

    pub async fn list_bug_monitor_incidents(&self, limit: usize) -> Vec<BugMonitorIncidentRecord> {
        let mut rows = self
            .bug_monitor_incidents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_incident(
        &self,
        incident_id: &str,
    ) -> Option<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .read()
            .await
            .get(incident_id)
            .cloned()
    }

    pub async fn put_bug_monitor_incident(
        &self,
        incident: BugMonitorIncidentRecord,
    ) -> anyhow::Result<BugMonitorIncidentRecord> {
        self.bug_monitor_incidents
            .write()
            .await
            .insert(incident.incident_id.clone(), incident.clone());
        self.persist_bug_monitor_incidents().await?;
        Ok(incident)
    }

    pub async fn delete_bug_monitor_incidents(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_incidents.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_incidents(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_incidents.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_incidents().await?;
        }
        Ok(removed)
    }

    pub async fn list_bug_monitor_posts(&self, limit: usize) -> Vec<BugMonitorPostRecord> {
        let mut rows = self
            .bug_monitor_posts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_post(&self, post_id: &str) -> Option<BugMonitorPostRecord> {
        self.bug_monitor_posts.read().await.get(post_id).cloned()
    }

    pub async fn put_bug_monitor_post(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<BugMonitorPostRecord> {
        self.bug_monitor_posts
            .write()
            .await
            .insert(post.post_id.clone(), post.clone());
        self.persist_bug_monitor_posts().await?;
        Ok(post)
    }

    pub async fn try_claim_bug_monitor_post_idempotency(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<(bool, BugMonitorPostRecord)> {
        let now = crate::now_ms();
        let pending_claim_ttl_ms = 10 * 60 * 1000;
        let result = {
            let mut guard = self.bug_monitor_posts.write().await;
            if let Some(existing) = guard
                .values()
                .find(|row| {
                    row.idempotency_key == post.idempotency_key
                        && (row.status == "posted"
                            || (row.status == "pending"
                                && now.saturating_sub(row.updated_at_ms) < pending_claim_ttl_ms))
                })
                .cloned()
            {
                (false, existing)
            } else {
                guard.insert(post.post_id.clone(), post.clone());
                (true, post)
            }
        };
        if result.0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(result)
    }

    pub async fn delete_bug_monitor_posts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_posts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_posts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_posts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_posts().await?;
        }
        Ok(removed)
    }

    pub async fn list_external_actions(&self, limit: usize) -> Vec<ExternalActionRecord> {
        let mut rows = self
            .external_actions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_external_action(&self, action_id: &str) -> Option<ExternalActionRecord> {
        self.external_actions.read().await.get(action_id).cloned()
    }

    pub async fn get_external_action_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Option<ExternalActionRecord> {
        let normalized = idempotency_key.trim();
        if normalized.is_empty() {
            return None;
        }
        self.external_actions
            .read()
            .await
            .values()
            .find(|action| {
                action
                    .idempotency_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    == Some(normalized)
            })
            .cloned()
    }

    pub async fn put_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        self.external_actions
            .write()
            .await
            .insert(action.action_id.clone(), action.clone());
        self.persist_external_actions().await?;
        Ok(action)
    }

    pub async fn record_external_action(
        &self,
        action: ExternalActionRecord,
    ) -> anyhow::Result<ExternalActionRecord> {
        let action = {
            let mut guard = self.external_actions.write().await;
            if let Some(idempotency_key) = action
                .idempotency_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if let Some(existing) = guard
                    .values()
                    .find(|existing| {
                        existing
                            .idempotency_key
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            == Some(idempotency_key)
                    })
                    .cloned()
                {
                    return Ok(existing);
                }
            }
            guard.insert(action.action_id.clone(), action.clone());
            action
        };
        self.persist_external_actions().await?;
        if let Some(run_id) = action.routine_run_id.as_deref() {
            let artifact = RoutineRunArtifact {
                artifact_id: format!("external-action-{}", action.action_id),
                uri: format!("external-action://{}", action.action_id),
                kind: "external_action_receipt".to_string(),
                label: Some(format!("external action receipt: {}", action.operation)),
                created_at_ms: action.updated_at_ms,
                metadata: Some(json!({
                    "actionID": action.action_id,
                    "operation": action.operation,
                    "status": action.status,
                    "sourceKind": action.source_kind,
                    "sourceID": action.source_id,
                    "capabilityID": action.capability_id,
                    "target": action.target,
                })),
            };
            let _ = self
                .append_routine_run_artifact(run_id, artifact.clone())
                .await;
            if let Some(runtime) = self.runtime.get() {
                runtime.event_bus.publish(EngineEvent::new(
                    "routine.run.artifact_added",
                    json!({
                        "runID": run_id,
                        "artifact": artifact,
                    }),
                ));
            }
        }
        if let Some(context_run_id) = action.context_run_id.as_deref() {
            let payload = serde_json::to_value(&action)?;
            if let Err(error) = crate::http::context_runs::append_json_artifact_to_context_run(
                self,
                context_run_id,
                &format!("external-action-{}", action.action_id),
                "external_action_receipt",
                &format!("external-actions/{}.json", action.action_id),
                &payload,
            )
            .await
            {
                tracing::warn!(
                    "failed to append external action artifact {} to context run {}: {}",
                    action.action_id,
                    context_run_id,
                    error
                );
            }
        }
        Ok(action)
    }

    pub async fn update_bug_monitor_runtime_status(
        &self,
        update: impl FnOnce(&mut BugMonitorRuntimeStatus),
    ) -> BugMonitorRuntimeStatus {
        let mut guard = self.bug_monitor_runtime_status.write().await;
        update(&mut guard);
        guard.clone()
    }

    pub async fn list_bug_monitor_drafts(&self, limit: usize) -> Vec<BugMonitorDraftRecord> {
        let mut rows = self
            .bug_monitor_drafts
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        rows.truncate(limit.clamp(1, 200));
        rows
    }

    pub async fn get_bug_monitor_draft(&self, draft_id: &str) -> Option<BugMonitorDraftRecord> {
        self.bug_monitor_drafts.read().await.get(draft_id).cloned()
    }

    pub async fn put_bug_monitor_draft(
        &self,
        draft: BugMonitorDraftRecord,
    ) -> anyhow::Result<BugMonitorDraftRecord> {
        self.bug_monitor_drafts
            .write()
            .await
            .insert(draft.draft_id.clone(), draft.clone());
        self.persist_bug_monitor_drafts().await?;
        Ok(draft)
    }

    pub async fn delete_bug_monitor_drafts(&self, ids: &[String]) -> anyhow::Result<usize> {
        let mut removed = 0usize;
        {
            let mut guard = self.bug_monitor_drafts.write().await;
            for id in ids {
                if guard.remove(id).is_some() {
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.persist_bug_monitor_drafts().await?;
        }
        Ok(removed)
    }

    pub async fn clear_bug_monitor_drafts(&self) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.bug_monitor_drafts.write().await;
            let count = guard.len();
            guard.clear();
            count
        };
        if removed > 0 {
            self.persist_bug_monitor_drafts().await?;
        }
        Ok(removed)
    }
}
