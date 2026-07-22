use super::*;

pub(super) fn resolve_config_override(cli_config: Option<&str>) -> Option<PathBuf> {
    cli_config
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from))
}

pub(super) fn log_startup_paths(
    state_dir: &Path,
    addr: &SocketAddr,
    startup_attempt_id: &str,
    config_path: Option<&Path>,
) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let config_path = config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| state_dir.join("config.json"));
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

pub(super) async fn initialize_runtime(
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
    // Keep provider OAuth credentials (e.g. OpenAI Codex sign-in) fresh for
    // channels/automations/scheduled runs without relying on the control panel
    // being open (TAN-594).
    state.spawn_provider_oauth_refresh();
    state.set_phase("ready").await;
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "engine.startup.ready",
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
            detail: Some(&format!(
                "attempt_id={} elapsed_ms={}",
                attempt_id,
                init_started.elapsed().as_millis()
            )),
        },
    );
    Ok(())
}

pub(super) async fn build_runtime(
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
    #[cfg(feature = "browser")]
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
    #[cfg(feature = "browser")]
    if startup_state.is_none() {
        browser.register_tools(&tools, None).await?;
    }
    let permissions = PermissionManager::new_with_state_file(
        event_bus.clone(),
        state_dir.join("permissions.json"),
    )
    .await?;
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
}

#[cfg(feature = "browser")]
pub(super) fn print_browser_readiness(status: &tandem_browser::BrowserStatus) {
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

#[cfg(feature = "browser")]
pub(super) async fn browser_doctor_status(
    state_dir: &Path,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<tandem_browser::BrowserStatus> {
    let config_path = override_config_path.unwrap_or_else(|| state_dir.join("config.json"));
    let config = ConfigStore::new(config_path, None).await?;
    let app_config = config.get().await;
    let browser = BrowserSubsystem::new(app_config.browser);
    Ok(browser.refresh_status().await)
}

#[cfg(feature = "browser")]
pub(super) async fn browser_install_result(
    state_dir: &Path,
    override_config_path: Option<PathBuf>,
) -> anyhow::Result<BrowserSidecarInstallResult> {
    let config_path = override_config_path.unwrap_or_else(|| state_dir.join("config.json"));
    let config = ConfigStore::new(config_path, None).await?;
    let app_config = config.get().await;
    install_browser_sidecar(&app_config.browser, || Ok(())).await
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

pub(super) fn configure_memory_db_path_env(state_dir: &Path) {
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
            org_id: None,
            workspace_id: None,
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

pub(super) fn resolve_memory_db_path(state_dir: &Path) -> PathBuf {
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
