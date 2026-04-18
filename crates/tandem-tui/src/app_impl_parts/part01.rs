impl App {
    fn test_mode_enabled() -> bool {
        std::env::var("TANDEM_TUI_TEST_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized.is_empty()
                    || normalized == "0"
                    || normalized == "false"
                    || normalized == "off")
            })
            .unwrap_or(false)
    }

    fn test_skip_engine_enabled() -> bool {
        std::env::var("TANDEM_TUI_TEST_SKIP_ENGINE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized.is_empty()
                    || normalized == "0"
                    || normalized == "false"
                    || normalized == "off")
            })
            .unwrap_or(false)
    }

    fn is_paste_shortcut(key: &KeyEvent) -> bool {
        let is_ctrl_v = matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
            && key.modifiers.contains(KeyModifiers::CONTROL);
        let is_shift_insert =
            matches!(key.code, KeyCode::Insert) && key.modifiers.contains(KeyModifiers::SHIFT);
        is_ctrl_v || is_shift_insert
    }

    fn sanitize_provider_catalog(
        mut catalog: crate::net::client::ProviderCatalog,
    ) -> crate::net::client::ProviderCatalog {
        catalog.all.retain(|p| p.id != "local");
        catalog.connected.retain(|id| id != "local");
        catalog
    }

    fn provider_is_connected(&self, provider_id: &str) -> bool {
        self.provider_catalog
            .as_ref()
            .map(|c| c.connected.iter().any(|p| p == provider_id))
            .unwrap_or(false)
    }

    fn record_recent_command(&mut self, cmd: &str) {
        let normalized = cmd.trim();
        if normalized.is_empty()
            || !normalized.starts_with('/')
            || normalized.starts_with("/recent")
        {
            return;
        }
        if let Some(existing) = self
            .recent_commands
            .iter()
            .position(|row| row == normalized)
        {
            self.recent_commands.remove(existing);
        }
        self.recent_commands.push_front(normalized.to_string());
        self.recent_commands.truncate(MAX_RECENT_COMMANDS);
    }

    fn recent_commands_snapshot(&self) -> Vec<String> {
        self.recent_commands.iter().cloned().collect()
    }

    fn clear_recent_commands(&mut self) -> usize {
        let cleared = self.recent_commands.len();
        self.recent_commands.clear();
        cleared
    }

    fn open_key_wizard_for_provider(&mut self, provider_id: &str) -> bool {
        let mut selected_provider_index = 0usize;
        let mut found = false;
        if let Some(catalog) = &self.provider_catalog {
            if let Some(idx) = catalog.all.iter().position(|p| p.id == provider_id) {
                selected_provider_index = idx;
                found = true;
            }
        }
        if !found {
            return false;
        }
        self.state = AppState::SetupWizard {
            step: SetupStep::EnterApiKey,
            provider_catalog: self.provider_catalog.clone(),
            selected_provider_index,
            selected_model_index: 0,
            api_key_input: String::new(),
            model_input: String::new(),
        };
        true
    }

    async fn sync_keystore_keys_to_engine(&self, client: &EngineClient) -> usize {
        let Some(keystore) = &self.keystore else {
            return 0;
        };
        let mut synced = 0usize;
        for key_name in keystore.list_keys() {
            if let Ok(Some(api_key)) = keystore.get(&key_name) {
                if api_key.trim().is_empty() {
                    continue;
                }
                let provider_id = Self::normalize_provider_id_from_keystore_key(&key_name);
                if client.set_auth(&provider_id, &api_key).await.is_ok() {
                    synced += 1;
                }
            }
        }
        synced
    }

    fn normalize_provider_id_from_keystore_key(key: &str) -> String {
        let trimmed = key.trim();
        if let Some(rest) = trimmed.strip_prefix("opencode_") {
            if let Some(provider) = rest.strip_suffix("_api_key") {
                return provider.to_string();
            }
        }
        if let Some(provider) = trimmed.strip_suffix("_api_key") {
            return provider.to_string();
        }
        if let Some(provider) = trimmed.strip_suffix("_key") {
            return provider.to_string();
        }
        trimmed.to_string()
    }

    fn save_provider_key_local(&mut self, provider_id: &str, api_key: &str) {
        let Some(keystore) = &mut self.keystore else {
            return;
        };
        if keystore.set(provider_id, api_key.to_string()).is_ok() {
            if let Some(config_dir) = &self.config_dir {
                let _ = keystore.save(config_dir.join("tandem.keystore"));
            }
        }
    }

    fn shared_engine_mode_enabled() -> bool {
        std::env::var("TANDEM_SHARED_ENGINE_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized == "0" || normalized == "false" || normalized == "off")
            })
            .unwrap_or(true)
    }

    fn configured_engine_port() -> u16 {
        std::env::var("TANDEM_ENGINE_PORT")
            .ok()
            .and_then(|raw| raw.trim().parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_ENGINE_PORT)
    }

    fn configured_engine_base_url() -> String {
        if let Ok(raw) = std::env::var("TANDEM_ENGINE_URL") {
            let trimmed = raw.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        format!(
            "http://{}:{}",
            DEFAULT_ENGINE_HOST,
            Self::configured_engine_port()
        )
    }

    fn engine_target_base_url(&self) -> String {
        self.engine_base_url_override
            .clone()
            .unwrap_or_else(Self::configured_engine_base_url)
    }

    fn engine_base_url_for_port(port: u16) -> String {
        format!("http://{}:{}", DEFAULT_ENGINE_HOST, port)
    }

    fn pick_spawn_port() -> u16 {
        let configured = Self::configured_engine_port();
        if TcpListener::bind((DEFAULT_ENGINE_HOST, configured)).is_ok() {
            return configured;
        }
        TcpListener::bind((DEFAULT_ENGINE_HOST, 0))
            .ok()
            .and_then(|listener| listener.local_addr().ok().map(|addr| addr.port()))
            .filter(|port| *port != 0)
            .unwrap_or(configured)
    }

    fn masked_engine_api_token(token: &str) -> String {
        let trimmed = token.trim();
        if trimmed.is_empty() || trimmed.len() <= 8 {
            return "****".to_string();
        }
        format!("{}****{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
    }

    fn resolve_engine_api_token() -> Option<(String, String)> {
        if let Ok(raw) = std::env::var("TANDEM_API_TOKEN") {
            let token = raw.trim();
            if !token.is_empty() {
                return Some((token.to_string(), "env".to_string()));
            }
        }
        let token_material = load_or_create_engine_api_token();
        Some((token_material.token, token_material.backend))
    }

    pub fn new() -> Self {
        let test_mode = Self::test_mode_enabled();
        let test_skip_engine = test_mode && Self::test_skip_engine_enabled();
        let config_dir = Self::find_or_create_config_dir();
        let (engine_api_token, engine_api_token_backend) = Self::resolve_engine_api_token()
            .map(|(token, backend)| (Some(token), Some(backend)))
            .unwrap_or((None, None));

        let vault_key = if let Some(dir) = &config_dir {
            let path = dir.join("vault.key");
            if path.exists() {
                EncryptedVaultKey::load(&path).ok()
            } else {
                None
            }
        } else {
            None
        };

        let test_session_id = "test-session".to_string();
        let test_agent = Self::make_agent_pane("A1".to_string(), test_session_id.clone());

        Self {
            state: if test_skip_engine {
                AppState::Chat {
                    session_id: test_session_id,
                    command_input: ComposerInputState::new(),
                    messages: vec![ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(
                            "Test mode active: engine bootstrap skipped.".to_string(),
                        )],
                    }],
                    scroll_from_bottom: 0,
                    tasks: Vec::new(),
                    active_task_id: None,
                    agents: vec![test_agent],
                    active_agent_index: 0,
                    ui_mode: UiMode::Focus,
                    grid_page: 0,
                    modal: None,
                    pending_requests: Vec::new(),
                    request_cursor: 0,
                    permission_choice: 0,
                    plan_wizard: PlanFeedbackWizardState::default(),
                    last_plan_task_fingerprint: Vec::new(),
                    plan_awaiting_approval: false,
                    plan_multi_agent_prompt: None,
                    plan_waiting_for_clarification_question: false,
                    request_panel_expanded: false,
                }
            } else if test_mode {
                AppState::Connecting
            } else {
                AppState::StartupAnimation { frame: 0 }
            },
            matrix: crate::ui::matrix::MatrixEffect::new(0, 0),

            should_quit: false,
            test_mode,
            tick_count: 0,
            config_dir,
            vault_key,
            keystore: None,
            engine_process: None,
            engine_binary_path: None,
            engine_download_retry_at: None,
            engine_download_last_error: None,
            engine_download_total_bytes: None,
            engine_downloaded_bytes: 0,
            engine_download_active: false,
            engine_download_phase: None,
            startup_engine_bootstrap_done: test_mode,
            client: None,
            sessions: Vec::new(),
            selected_session_index: 0,
            current_mode: TandemMode::default(),
            current_provider: None,
            current_model: None,
            provider_catalog: None,
            connection_status: if test_skip_engine {
                "Test mode: engine skipped.".to_string()
            } else if test_mode {
                "Test mode: deterministic UI enabled.".to_string()
            } else {
                "Initializing...".to_string()
            },
            engine_health: EngineConnectionStatus::Disconnected,
            engine_lease_id: None,
            engine_lease_last_renewed: None,
            engine_api_token,
            engine_api_token_backend,
            engine_base_url_override: None,
            engine_connection_source: EngineConnectionSource::Unknown,
            engine_spawned_at: None,
            local_engine_build_attempted: false,
            pending_model_provider: None,
            recent_commands: VecDeque::new(),
            autocomplete_items: Vec::new(),
            autocomplete_index: 0,
            autocomplete_mode: AutocompleteMode::Command,
            show_autocomplete: false,
            action_tx: None,
            quit_armed_at: None,
            paste_activity_until: None,
            malformed_question_retries: HashSet::new(),
            pager_overlay: None,
            file_search: FileSearchState::default(),
        }
    }

    fn make_agent_pane(agent_id: String, session_id: String) -> AgentPane {
        AgentPane {
            agent_id,
            session_id,
            draft: ComposerInputState::new(),
            stream_collector: None,
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::new(),
            active_task_id: None,
            status: AgentStatus::Idle,
            active_run_id: None,
            bound_context_run_id: None,
            follow_up_queue: VecDeque::new(),
            steering_message: None,
            paste_registry: HashMap::new(),
            next_paste_id: 1,
            live_tool_calls: HashMap::new(),
            exploration_batch: None,
            live_activity_message: None,
            delegated_worker: false,
            delegated_team_name: None,
        }
    }

    async fn finalize_connecting(&mut self, client: &EngineClient) -> bool {
        if self.engine_lease_id.is_none() {
            self.acquire_engine_lease().await;
            let synced = self.sync_keystore_keys_to_engine(client).await;
            if synced > 0 {
                self.connection_status = format!("Synced {} provider key(s)...", synced);
            }
        }

        let providers = match client.list_providers().await {
            Ok(providers) => {
                let providers = Self::sanitize_provider_catalog(providers);
                self.provider_catalog = Some(providers.clone());
                providers
            }
            Err(_err) => {
                self.connection_status = "Connected. Loading providers...".to_string();
                return false;
            }
        };

        let needs_first_key_setup = self
            .keystore
            .as_ref()
            .map(|keystore| keystore.list_keys().is_empty())
            .unwrap_or(false);

        if providers.connected.is_empty() || needs_first_key_setup {
            self.state = AppState::SetupWizard {
                step: SetupStep::Welcome,
                provider_catalog: Some(providers),
                selected_provider_index: 0,
                selected_model_index: 0,
                api_key_input: String::new(),
                model_input: String::new(),
            };
            return true;
        }

        let config = client.config_providers().await.ok();
        self.apply_provider_defaults(config.as_ref());

        match client.list_sessions().await {
            Ok(sessions) => {
                self.sessions = sessions;
                self.connection_status = "Engine ready. Loading sessions...".to_string();
                self.state = AppState::MainMenu;
                true
            }
            Err(_err) => {
                self.connection_status = "Connected. Loading sessions...".to_string();
                false
            }
        }
    }

    async fn cancel_agent_if_running(&mut self, agent_index: usize) {
        let (session_id, run_id) = if let AppState::Chat { agents, .. } = &self.state {
            if let Some(agent) = agents.get(agent_index) {
                (agent.session_id.clone(), agent.active_run_id.clone())
            } else {
                return;
            }
        } else {
            return;
        };

        if let Some(client) = &self.client {
            if let Some(run_id) = run_id.as_deref() {
                let _ = client.cancel_run_by_id(&session_id, run_id).await;
            } else {
                let _ = client.abort_session(&session_id).await;
            }
        }
    }

    fn update_autocomplete_for_input(&mut self, input: &str) {
        if !input.starts_with('/') {
            self.show_autocomplete = false;
            self.autocomplete_items.clear();
            return;
        }
        if let Some(rest) = input.strip_prefix("/provider") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let mut providers: Vec<String> = catalog.all.iter().map(|p| p.id.clone()).collect();
                providers.sort();
                let filtered: Vec<String> = if query.is_empty() {
                    providers
                } else {
                    providers
                        .into_iter()
                        .filter(|p| p.to_lowercase().contains(&query))
                        .collect()
                };
                self.autocomplete_items = filtered
                    .into_iter()
                    .map(|p| (p, "provider".to_string()))
                    .collect();
                self.autocomplete_index = 0;
                self.autocomplete_mode = AutocompleteMode::Provider;
                self.show_autocomplete = !self.autocomplete_items.is_empty();
                return;
            }
        }
        if let Some(rest) = input.strip_prefix("/model") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let provider_id = self.current_provider.as_deref().unwrap_or("");
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    let filtered: Vec<String> = if query.is_empty() {
                        model_ids
                    } else {
                        model_ids
                            .into_iter()
                            .filter(|m| m.to_lowercase().contains(&query))
                            .collect()
                    };
                    self.autocomplete_items = filtered
                        .into_iter()
                        .map(|m| (m, "model".to_string()))
                        .collect();
                    self.autocomplete_index = 0;
                    self.autocomplete_mode = AutocompleteMode::Model;
                    self.show_autocomplete = !self.autocomplete_items.is_empty();
                    return;
                }
            }
        }
        let cmd_part = input.trim_start_matches('/').to_lowercase();
        self.autocomplete_items = COMMAND_HELP
            .iter()
            .filter(|(name, _)| name.starts_with(&cmd_part))
            .map(|(name, desc)| (name.to_string(), desc.to_string()))
            .collect();
        self.autocomplete_index = 0;
        self.autocomplete_mode = AutocompleteMode::Command;
        self.show_autocomplete = !self.autocomplete_items.is_empty();
    }

    fn model_ids_for_provider(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
    ) -> Vec<String> {
        if provider_index >= provider_catalog.all.len() {
            return Vec::new();
        }
        let provider = &provider_catalog.all[provider_index];
        let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
        model_ids.sort();
        model_ids
    }

    fn filtered_model_ids(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
        model_input: &str,
    ) -> Vec<String> {
        let model_ids = Self::model_ids_for_provider(provider_catalog, provider_index);
        if model_input.trim().is_empty() {
            return model_ids;
        }
        let query = model_input.trim().to_lowercase();
        model_ids
            .into_iter()
            .filter(|m| m.to_lowercase().contains(&query))
            .collect()
    }

    fn find_or_create_config_dir() -> Option<PathBuf> {
        if let Ok(paths) = resolve_shared_paths() {
            let _ = std::fs::create_dir_all(&paths.canonical_root);
            if let Ok(report) = migrate_legacy_storage_if_needed(&paths) {
                tracing::info!(
                    "TUI storage migration status: reason={} performed={} copied={} skipped={} errors={}",
                    report.reason,
                    report.performed,
                    report.copied.len(),
                    report.skipped.len(),
                    report.errors.len()
                );
            }
            return Some(paths.canonical_root);
        }
        None
    }

    fn engine_binary_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine.exe";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine";
    }

    fn engine_asset_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine-windows-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine-darwin-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine-darwin-arm64.zip";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine-linux-x64.tar.gz";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine-linux-arm64.tar.gz";
    }

    fn engine_asset_matches(asset_name: &str) -> bool {
        if !asset_name.starts_with("tandem-engine-") {
            return false;
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            return asset_name.contains("windows") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("arm64");
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("arm64");
        }
    }

    fn shared_binaries_dir() -> Option<PathBuf> {
        resolve_shared_paths()
            .ok()
            .map(|paths| paths.canonical_root.join("binaries"))
            .or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    std::env::var_os("APPDATA")
                        .map(PathBuf::from)
                        .map(|d| d.join("tandem").join("binaries"))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var_os("XDG_DATA_HOME")
                        .map(PathBuf::from)
                        .or_else(|| {
                            std::env::var_os("HOME")
                                .map(PathBuf::from)
                                .map(|h| h.join(".local").join("share"))
                        })
                        .map(|d| d.join("tandem").join("binaries"))
                }
            })
    }

    fn find_desktop_bundled_engine_binary() -> Option<PathBuf> {
        let binary_name = Self::engine_binary_name();
        let mut candidates: Vec<PathBuf> = Vec::new();

        #[cfg(target_os = "windows")]
        {
            let mut roots: Vec<PathBuf> = Vec::new();
            if let Some(v) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
                roots.push(v);
            }
            if let Some(v) = std::env::var_os("ProgramW6432").map(PathBuf::from) {
                if !roots.contains(&v) {
                    roots.push(v);
                }
            }
            if let Some(v) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                roots.push(v.join("Programs"));
            }

            for root in roots {
                let app_dir = root.join("Tandem");
                candidates.push(app_dir.join("binaries").join(binary_name));
                candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    app_dir
                        .join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        #[cfg(target_os = "macos")]
        {
            let app_dir = PathBuf::from("/Applications/Tandem.app")
                .join("Contents")
                .join("Resources");
            candidates.push(app_dir.join("binaries").join(binary_name));
            candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
        }

        #[cfg(target_os = "linux")]
        {
            let roots = [
                PathBuf::from("/opt/tandem"),
                PathBuf::from("/usr/lib/tandem"),
            ];
            for root in roots {
                candidates.push(root.join("binaries").join(binary_name));
                candidates.push(root.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    root.join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        for candidate in candidates {
            if candidate
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                return Some(candidate);
            }
        }

        None
    }

    fn find_dev_engine_binary() -> Option<PathBuf> {
        let Ok(current_dir) = env::current_dir() else {
            return None;
        };
        let binary_name = Self::engine_binary_name();
        let candidates = [
            current_dir.join("target").join("debug").join(binary_name),
            current_dir
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir
                .join("src-tauri")
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir.join("binaries").join(binary_name),
            current_dir
                .join("src-tauri")
                .join("binaries")
                .join(binary_name),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn try_build_local_dev_engine_binary(&self) -> Option<PathBuf> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let Ok(current_dir) = env::current_dir() else {
            return None;
        };
        let output = StdCommand::new("cargo")
            .arg("build")
            .arg("-p")
            .arg("tandem-ai")
            .current_dir(&current_dir)
            .output()
            .ok()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let summary = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("cargo build failed");
            tracing::warn!("TUI local engine rebuild failed: {}", summary);
            return None;
        }
        Self::find_dev_engine_binary().filter(|path| {
            path.metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
        })
    }

    fn find_extracted_binary(dir: &std::path::Path, binary_name: &str) -> anyhow::Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Ok(found) = Self::find_extracted_binary(&path, binary_name) {
                    return Ok(found);
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case(binary_name) {
                    return Ok(path);
                }
            }
        }
        Err(anyhow!("Extracted engine binary not found"))
    }

    fn parse_semver_triplet(raw: &str) -> Option<(u64, u64, u64)> {
        let token = raw
            .split_whitespace()
            .find(|part| part.chars().filter(|c| *c == '.').count() >= 2)?;
        let core = token.trim_start_matches('v');
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch_str = parts.next()?;
        let patch_digits = patch_str
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if patch_digits.is_empty() {
            return None;
        }
        let patch = patch_digits.parse::<u64>().ok()?;
        Some((major, minor, patch))
    }

    fn format_semver_triplet(version: (u64, u64, u64)) -> String {
        format!("{}.{}.{}", version.0, version.1, version.2)
    }

    fn parse_capability_csv(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for part in raw.split(',') {
            let item = part.trim();
            if item.is_empty() {
                continue;
            }
            if seen.insert(item.to_string()) {
                out.push(item.to_string());
            }
        }
        out
    }

    fn parse_required_optional_segments(raw: &str) -> (Vec<String>, Vec<String>) {
        let mut required = Vec::new();
        let mut optional = Vec::new();
        for segment in raw.split("::") {
            let trimmed = segment.trim();
            if let Some(value) = trimmed.strip_prefix("required=") {
                required = Self::parse_capability_csv(value);
            } else if let Some(value) = trimmed.strip_prefix("optional=") {
                optional = Self::parse_capability_csv(value);
            } else {
                for token in trimmed.split_whitespace() {
                    if let Some(value) = token.strip_prefix("required=") {
                        required = Self::parse_capability_csv(value);
                    } else if let Some(value) = token.strip_prefix("optional=") {
                        optional = Self::parse_capability_csv(value);
                    }
                }
            }
        }
        optional.retain(|cap| !required.iter().any(|req| req == cap));
        (required, optional)
    }

    fn caps_from_value(value: Option<&Value>) -> Vec<String> {
        match value {
            Some(Value::Array(items)) => {
                let joined = items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                Self::parse_capability_csv(&joined)
            }
            Some(Value::String(text)) => Self::parse_capability_csv(text),
            _ => Vec::new(),
        }
    }

    fn normalize_automation_tasks(tasks: &Value) -> Result<Vec<Value>, String> {
        let Some(items) = tasks.as_array() else {
            return Err("tasks JSON must be an array of objects".to_string());
        };
        let mut normalized = Vec::new();
        for item in items {
            let Some(obj) = item.as_object() else {
                return Err("each task in tasks JSON must be an object".to_string());
            };
            let required = Self::caps_from_value(obj.get("required"));
            let optional = Self::caps_from_value(obj.get("optional"))
                .into_iter()
                .filter(|cap| !required.iter().any(|req| req == cap))
                .collect::<Vec<_>>();
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("step_{}", normalized.len() + 1));
            let agent_id = obj
                .get("agent_id")
                .or_else(|| obj.get("agent_preset"))
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            normalized.push(json!({
                "id": id,
                "agent_id": agent_id,
                "required": required,
                "optional": optional,
            }));
        }
        Ok(normalized)
    }

    fn automation_override_yaml(id: &str, tasks: &[Value], summary: &Value) -> String {
        let required = summary
            .get("automation")
            .and_then(|v| v.get("required"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let optional = summary
            .get("automation")
            .and_then(|v| v.get("optional"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut lines = vec![
            format!("id: {}", id),
            "version: 0.1.0".to_string(),
            "publisher: local.user".to_string(),
            "description: Project override automation preset".to_string(),
            "tags:".to_string(),
            "  - override".to_string(),
            "tasks:".to_string(),
        ];
        if tasks.is_empty() {
            lines.push("  - id: step_1".to_string());
            lines.push("    agent_preset:".to_string());
            lines.push("    capabilities:".to_string());
            lines.push("      required: []".to_string());
            lines.push("      optional: []".to_string());
        } else {
            for task in tasks {
                let task_id = task
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("step")
                    .trim();
                let agent = task
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                lines.push(format!("  - id: {}", task_id));
                lines.push(format!("    agent_preset: {}", agent));
                lines.push("    capabilities:".to_string());
                lines.push("      required:".to_string());
                if let Some(req) = task.get("required").and_then(|v| v.as_array()) {
                    if req.is_empty() {
                        lines.push("        -".to_string());
                    } else {
                        for cap in req {
                            let cap = cap.as_str().unwrap_or("").trim();
                            if cap.is_empty() {
                                continue;
                            }
                            lines.push(format!("        - {}", cap));
                        }
                    }
                } else {
                    lines.push("        -".to_string());
                }
                lines.push("      optional:".to_string());
                if let Some(opt) = task.get("optional").and_then(|v| v.as_array()) {
                    if opt.is_empty() {
                        lines.push("        -".to_string());
                    } else {
                        for cap in opt {
                            let cap = cap.as_str().unwrap_or("").trim();
                            if cap.is_empty() {
                                continue;
                            }
                            lines.push(format!("        - {}", cap));
                        }
                    }
                } else {
                    lines.push("        -".to_string());
                }
            }
        }
        lines.push("capabilities:".to_string());
        lines.push("  required:".to_string());
        if required.is_empty() {
            lines.push("    -".to_string());
        } else {
            for cap in required {
                let cap = cap.as_str().unwrap_or("").trim();
                if !cap.is_empty() {
                    lines.push(format!("    - {}", cap));
                }
            }
        }
        lines.push("  optional:".to_string());
        if optional.is_empty() {
            lines.push("    -".to_string());
        } else {
            for cap in optional {
                let cap = cap.as_str().unwrap_or("").trim();
                if !cap.is_empty() {
                    lines.push(format!("    - {}", cap));
                }
            }
        }
        lines.push(String::new());
        lines.join("\n")
    }

    fn desired_engine_version() -> Option<(u64, u64, u64)> {
        Self::parse_semver_triplet(env!("CARGO_PKG_VERSION"))
    }

    fn installed_engine_version(path: &std::path::Path) -> Option<(u64, u64, u64)> {
        let output = StdCommand::new(path).arg("--version").output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Self::parse_semver_triplet(&stdout).or_else(|| Self::parse_semver_triplet(&stderr))
    }

    fn engine_binary_is_stale(path: &std::path::Path) -> bool {
        let Some(desired) = Self::desired_engine_version() else {
            return false;
        };
        let Some(installed) = Self::installed_engine_version(path) else {
            // If we cannot determine a version, keep existing behavior and accept it.
            return false;
        };
        installed < desired
    }

    async fn ensure_engine_binary(&mut self) -> anyhow::Result<Option<PathBuf>> {
        if let Some(path) = &self.engine_binary_path {
            if path
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                if Self::engine_binary_is_stale(path) {
                    self.engine_download_phase =
                        Some("Cached engine is stale; refreshing binary".to_string());
                    self.engine_binary_path = None;
                } else {
                    return Ok(Some(path.clone()));
                }
            } else {
                self.engine_binary_path = None;
            }
        }

        if cfg!(debug_assertions) {
            if let Some(path) = Self::find_dev_engine_binary() {
                if Self::engine_binary_is_stale(&path) {
                    if !self.local_engine_build_attempted {
                        self.local_engine_build_attempted = true;
                        self.engine_download_phase =
                            Some("Local dev engine is stale; rebuilding local engine".to_string());
                        if let Some(rebuilt) = self.try_build_local_dev_engine_binary() {
                            if !Self::engine_binary_is_stale(&rebuilt) {
                                self.engine_binary_path = Some(rebuilt.clone());
                                self.engine_download_active = false;
                                self.engine_download_total_bytes = None;
                                self.engine_downloaded_bytes = 0;
                                self.engine_download_phase =
                                    Some("Using rebuilt local dev engine binary".to_string());
                                return Ok(Some(rebuilt));
                            }
                        }
                    }
                    self.engine_download_phase =
                        Some("Local dev engine is stale; using newer managed binary".to_string());
                } else {
                    self.engine_binary_path = Some(path.clone());
                    self.engine_download_active = false;
                    self.engine_download_total_bytes = None;
                    self.engine_downloaded_bytes = 0;
                    self.engine_download_phase = Some("Using local dev engine binary".to_string());
                    return Ok(Some(path.clone()));
                }
            }
        }

        if let Some(path) = Self::find_desktop_bundled_engine_binary() {
            if Self::engine_binary_is_stale(&path) {
                self.engine_download_phase = Some(
                    "Desktop bundled engine is stale; using updated sidecar binary".to_string(),
                );
            } else {
                self.engine_binary_path = Some(path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase =
                    Some("Using desktop bundled engine binary".to_string());
                return Ok(Some(path));
            }
        }

        let Some(binaries_dir) = Self::shared_binaries_dir() else {
            return Err(anyhow!(
                "Unable to resolve Tandem binaries directory for engine download"
            ));
        };
        let binary_path = binaries_dir.join(Self::engine_binary_name());
        if binary_path
            .metadata()
            .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
            .unwrap_or(false)
        {
            if Self::engine_binary_is_stale(&binary_path) {
                self.engine_download_phase =
                    Some("Local sidecar engine is stale; downloading latest".to_string());
            } else {
                self.engine_binary_path = Some(binary_path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase = Some("Using cached engine binary".to_string());
                return Ok(Some(binary_path));
            }
        }

        fs::create_dir_all(&binaries_dir)?;
        self.connection_status = "Downloading engine...".to_string();
        let path = self
            .download_engine_binary(&binaries_dir, &binary_path)
            .await?;
        self.engine_binary_path = Some(path.clone());
        self.engine_download_active = false;
        self.engine_download_last_error = None;
        self.engine_download_retry_at = None;
        self.engine_download_phase = Some("Engine download complete".to_string());
        Ok(Some(path))
    }

    async fn download_engine_binary(
        &mut self,
        binaries_dir: &PathBuf,
        binary_path: &PathBuf,
    ) -> anyhow::Result<PathBuf> {
        self.engine_download_active = true;
        self.engine_download_total_bytes = None;
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some("Fetching release metadata".to_string());

        let client = Client::new();
        let release_url = format!("{}/repos/{}/releases", GITHUB_API, ENGINE_REPO);
        let releases: Vec<GitHubRelease> = client
            .get(release_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let release = releases
            .iter()
            .find(|release| {
                !release.draft
                    && !release.prerelease
                    && release
                        .assets
                        .iter()
                        .any(|asset| Self::engine_asset_matches(&asset.name))
            })
            .or_else(|| {
                releases.iter().find(|release| {
                    !release.draft
                        && release
                            .assets
                            .iter()
                            .any(|asset| Self::engine_asset_matches(&asset.name))
                })
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine release found"))?;
        if release.prerelease {
            tracing::info!(
                "No stable compatible tandem-engine release found; using prerelease {}",
                release.tag_name
            );
        }

        let asset_name = Self::engine_asset_name();
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .find(|asset| Self::engine_asset_matches(&asset.name))
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine asset found"))?;

        let download_url = asset.browser_download_url.clone();
        let archive_path = binary_path.with_extension("download");
        self.engine_download_total_bytes = Some(asset.size);
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some(format!("Downloading {}", asset.name));
        let mut response = client
            .get(&download_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?;
        if let Some(total) = response.content_length() {
            self.engine_download_total_bytes = Some(total);
        }
        let mut file = tokio::fs::File::create(&archive_path).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            self.engine_downloaded_bytes = self
                .engine_downloaded_bytes
                .saturating_add(chunk.len() as u64);
            self.connection_status = match self.engine_download_total_bytes {
                Some(total) if total > 0 => {
                    let pct = (self.engine_downloaded_bytes as f64 / total as f64) * 100.0;
                    format!("Downloading engine... {:.0}%", pct.clamp(0.0, 100.0))
                }
                _ => format!(
                    "Downloading engine... {} KB",
                    self.engine_downloaded_bytes / 1024
                ),
            };
        }
        file.flush().await?;
        self.engine_download_phase = Some("Extracting engine archive".to_string());

        let asset_name = asset.name.clone();
        let archive_path_clone = archive_path.clone();
        let binaries_dir_clone = binaries_dir.clone();
        let binary_path_clone = binary_path.clone();

        let extracted_path = tokio::task::spawn_blocking(move || -> anyhow::Result<PathBuf> {
            if asset_name.ends_with(".zip") {
                let file = fs::File::open(&archive_path_clone)?;
                let mut archive = zip::ZipArchive::new(file)?;
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    let outpath = binaries_dir_clone.join(file.mangled_name());
                    if file.is_dir() {
                        fs::create_dir_all(&outpath)?;
                    } else {
                        if let Some(p) = outpath.parent() {
                            fs::create_dir_all(p)?;
                        }
                        let mut outfile = fs::File::create(&outpath)?;
                        std::io::copy(&mut file, &mut outfile)?;
                    }
                }
            } else if asset_name.ends_with(".tar.gz") {
                let file = fs::File::open(&archive_path_clone)?;
                let gz = flate2::read::GzDecoder::new(file);
                let mut archive = tar::Archive::new(gz);
                archive.unpack(&binaries_dir_clone)?;
            }

            let extracted =
                Self::find_extracted_binary(&binaries_dir_clone, Self::engine_binary_name())?;
            if extracted != binary_path_clone {
                if binary_path_clone.exists() {
                    fs::remove_file(&binary_path_clone)?;
                }
                fs::rename(&extracted, &binary_path_clone)?;
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&binary_path_clone)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&binary_path_clone, perms)?;
            }

            fs::remove_file(&archive_path_clone).ok();
            Ok(binary_path_clone)
        })
        .await??;
        self.engine_download_phase = Some("Finalizing engine install".to_string());
        Ok(extracted_path)
    }

    pub fn handle_key_event(&self, key: KeyEvent) -> Option<Action> {
        // Global control keys
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return Some(Action::CtrlCPressed),
                KeyCode::Char('x') => return Some(Action::Quit),
                KeyCode::Char('n') => return Some(Action::NewAgent),
                KeyCode::Char('w') => return Some(Action::CloseActiveAgent),
                KeyCode::Char('u') => return Some(Action::PageUp),
                KeyCode::Char('d') => return Some(Action::PageDown),
                KeyCode::Char('y') => return Some(Action::CopyLastAssistant),
                _ => {}
            }
        }

        match self.state {
            AppState::StartupAnimation { .. } => {
                if !self.startup_engine_bootstrap_done {
                    return None;
                }
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                        Some(Action::SkipAnimation)
                    }
                    _ => None,
                }
            }
            AppState::PinPrompt { .. } => match key.code {
                KeyCode::Esc => Some(Action::Quit),
                KeyCode::Enter => Some(Action::SubmitPin),
                KeyCode::Backspace => Some(Action::EnterPin('\x08')),
                KeyCode::Char(c) if c.is_ascii_digit() => Some(Action::EnterPin(c)),
                _ => None,
            },
            AppState::Connecting => {
                // Ignore typing while engine is loading.
                None
            }
            AppState::MainMenu => match key.code {
                KeyCode::Char('q') => Some(Action::Quit),
                KeyCode::Char('n') => Some(Action::NewSession),
                KeyCode::Char('d') | KeyCode::Delete => Some(Action::DeleteSelectedSession),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::NextSession),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::PreviousSession),
                KeyCode::Enter => Some(Action::SelectSession),
                _ => None,
            },

            AppState::Chat { .. } => {
                if let AppState::Chat {
                    modal,
                    pending_requests,
                    ..
                } = &self.state
                {
                    let active_modal = modal.clone();
                    if let Some(active_modal) = active_modal {
                        if matches!(active_modal, ModalState::RequestCenter)
                            && pending_requests.is_empty()
                        {
                            // Treat stale/empty request center as closed so normal typing works.
                        } else {
                            return match key.code {
                                KeyCode::Esc => Some(Action::CloseModal),
                                KeyCode::Up if matches!(active_modal, ModalState::FileSearch) => {
                                    Some(Action::FileSearchSelectPrev)
                                }
                                KeyCode::Down if matches!(active_modal, ModalState::FileSearch) => {
                                    Some(Action::FileSearchSelectNext)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchConfirm)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchBackspace)
                                }
                                KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}')
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchBackspace)
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchInput(c))
                                }
                                KeyCode::Up if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayScrollUp)
                                }
                                KeyCode::Down if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayScrollDown)
                                }
                                KeyCode::PageUp if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayPageUp)
                                }
                                KeyCode::PageDown if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayPageDown)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestConfirm)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardSubmit)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::RequestSelectPrev)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::RequestSelectNext)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && self.request_center_active_is_question() =>
                                {
                                    Some(Action::RequestOptionPrev)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && self.request_center_active_is_question() =>
                                {
                                    Some(Action::RequestOptionNext)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestSelectPrev)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardPrevField)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestSelectNext)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardNextField)
                                }
                                KeyCode::Tab
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardNextField)
                                }
                                KeyCode::BackTab
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardPrevField)
                                }
                                KeyCode::Left
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestOptionPrev)
                                }
                                KeyCode::Right
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestOptionNext)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestBackspace)
                                }
                                KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestBackspace)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardBackspace)
                                }
                                KeyCode::Char(' ')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestToggleCurrent)
                                }
                                KeyCode::Char('r') | KeyCode::Char('R')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestReject)
                                }
                                KeyCode::Char('e') | KeyCode::Char('E')
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::ToggleRequestPanelExpand)
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && c.is_ascii_digit()
                                        && self.request_center_digit_is_shortcut(c) =>
                                {
                                    Some(Action::RequestDigit(c as u8 - b'0'))
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestInput(c))
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardInput(c))
                                }
                                KeyCode::Char('y') | KeyCode::Char('Y')
                                    if matches!(
                                        active_modal,
                                        ModalState::ConfirmCloseAgent { .. }
                                    ) =>
                                {
                                    Some(Action::ConfirmCloseAgent(true))
                                }
                                KeyCode::Char('n') | KeyCode::Char('N')
                                    if matches!(
                                        active_modal,
                                        ModalState::ConfirmCloseAgent { .. }
                                    ) =>
                                {
                                    Some(Action::ConfirmCloseAgent(false))
                                }
                                KeyCode::Char('y') | KeyCode::Char('Y')
                                    if matches!(
                                        active_modal,
                                        ModalState::StartPlanAgents { .. }
                                    ) =>
                                {
                                    if let ModalState::StartPlanAgents { count } = active_modal {
                                        Some(Action::ConfirmStartPlanAgents {
                                            confirmed: true,
                                            count,
                                        })
                                    } else {
                                        None
                                    }
                                }
                                KeyCode::Char('n') | KeyCode::Char('N')
                                    if matches!(
                                        active_modal,
                                        ModalState::StartPlanAgents { .. }
                                    ) =>
                                {
                                    if let ModalState::StartPlanAgents { count } = active_modal {
                                        Some(Action::ConfirmStartPlanAgents {
                                            confirmed: false,
                                            count,
                                        })
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            };
                        }
                    }
                }
                if self.show_autocomplete {
                    match key.code {
                        KeyCode::Esc => Some(Action::AutocompleteDismiss),
                        _ if Self::is_paste_shortcut(&key) => Some(Action::PasteFromClipboard),
                        KeyCode::Enter | KeyCode::Tab => Some(Action::AutocompleteAccept),
                        KeyCode::Down | KeyCode::Char('j')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompleteNext)
                        }
                        KeyCode::Up | KeyCode::Char('k')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompletePrev)
                        }
                        KeyCode::Down => Some(Action::AutocompleteNext),
                        KeyCode::Up => Some(Action::AutocompletePrev),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}') => {
                            Some(Action::BackspaceCommand)
                        }
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                } else {
                    match key.code {
                        KeyCode::Esc => None,
                        _ if Self::is_paste_shortcut(&key) => Some(Action::PasteFromClipboard),
                        KeyCode::F(1) => Some(Action::ShowHelpModal),
                        KeyCode::F(2) => Some(Action::OpenDocs),
                        KeyCode::Char('g') | KeyCode::Char('G')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::ToggleUiMode)
                        }
                        KeyCode::Char('m') | KeyCode::Char('M')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::CycleMode)
                        }
                        KeyCode::Char('r') | KeyCode::Char('R')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenRequestCenter)
                        }
                        KeyCode::Char('i') | KeyCode::Char('I')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::QueueSteeringFromComposer)
                        }
                        KeyCode::Char('p') | KeyCode::Char('P')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenFileSearch)
                        }
                        KeyCode::Char('d') | KeyCode::Char('D')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenDiffOverlay)
                        }
                        KeyCode::Char('e') | KeyCode::Char('E')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenExternalEditor)
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::StartDemoStream)
                        }
                        KeyCode::Char('b') | KeyCode::Char('B')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::SpawnBackgroundDemo)
                        }
                        KeyCode::Char('[') => Some(Action::GridPagePrev),
                        KeyCode::Char(']') => Some(Action::GridPageNext),
                        KeyCode::BackTab => Some(Action::SwitchAgentPrev),
                        KeyCode::Enter
                            if key.modifiers.contains(KeyModifiers::SHIFT)
                                || key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::InsertNewline)
                        }
                        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::SubmitCommand)
                        }
                        KeyCode::Enter => Some(Action::SubmitCommand),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}') => {
                            Some(Action::BackspaceCommand)
                        }
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Tab => Some(Action::SwitchAgentNext),
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorUp)
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorDown)
                        }
                        KeyCode::Up => Some(Action::ScrollUp),
                        KeyCode::Down => Some(Action::ScrollDown),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
                        KeyCode::PageUp => Some(Action::PageUp),
                        KeyCode::PageDown => Some(Action::PageDown),
                        KeyCode::Char(c)
                            if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() =>
                        {
                            let idx = (c as u8 - b'0') as usize;
                            if idx > 0 {
                                Some(Action::SelectAgentByNumber(idx))
                            } else {
                                None
                            }
                        }
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                }
            }

            AppState::SetupWizard { .. } => {
                if Self::is_paste_shortcut(&key) {
                    return Some(Action::PasteFromClipboard);
                }
                match key.code {
                    KeyCode::Esc => Some(Action::Quit),
                    KeyCode::Enter => Some(Action::SetupNextStep),
                    KeyCode::Down => Some(Action::SetupNextItem),
                    KeyCode::Up => Some(Action::SetupPrevItem),
                    KeyCode::Char(c) => Some(Action::SetupInput(c)),
                    KeyCode::Backspace => Some(Action::SetupBackspace),
                    _ => None,
                }
            }
        }
    }
    pub fn handle_mouse_event(&self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::ScrollDown => match self.state {
                AppState::MainMenu => Some(Action::NextSession),
                AppState::Chat { .. } => Some(Action::ScrollDown),
                AppState::SetupWizard { .. } => Some(Action::SetupNextItem),
                _ => None,
            },
            MouseEventKind::ScrollUp => match self.state {
                AppState::MainMenu => Some(Action::PreviousSession),
                AppState::Chat { .. } => Some(Action::ScrollUp),
                AppState::SetupWizard { .. } => Some(Action::SetupPrevItem),
                _ => None,
            },
            _ => None,
        }
    }

    pub async fn update(&mut self, action: Action) -> anyhow::Result<()> {
        include!("../app_update_match_arms_parts/all.inc");
        Ok(())
    }
}
