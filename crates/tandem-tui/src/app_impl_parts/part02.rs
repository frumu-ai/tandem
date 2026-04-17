impl App {
    pub async fn tick(&mut self) {
        self.tick_count += 1;

        // Check engine health every ~1 second (assuming 60tps)
        if self.tick_count % 60 == 0 {
            if let Some(client) = &self.client {
                match client.check_health().await {
                    Ok(true) => self.engine_health = EngineConnectionStatus::Connected,
                    _ => self.engine_health = EngineConnectionStatus::Error,
                }
            } else {
                self.engine_health = EngineConnectionStatus::Disconnected;
            }
        }

        match &mut self.state {
            AppState::StartupAnimation { frame } => {
                *frame += 1;
                // Update matrix with real terminal size
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }

                if !self.startup_engine_bootstrap_done {
                    if let Some(retry_at) = self.engine_download_retry_at {
                        if retry_at > Instant::now() {
                            let wait_secs = retry_at
                                .saturating_duration_since(Instant::now())
                                .as_secs()
                                .max(1);
                            self.connection_status =
                                format!("Engine download failed. Retrying in {}s...", wait_secs);
                            return;
                        }
                    }
                    self.connection_status = "Preparing engine bootstrap...".to_string();
                    match self.ensure_engine_binary().await {
                        Ok(_) => {
                            self.startup_engine_bootstrap_done = true;
                            self.connection_status =
                                "Engine ready. Press Enter to continue.".to_string();
                        }
                        Err(err) => {
                            tracing::warn!("TUI engine bootstrap failed: {}", err);
                            self.engine_download_active = false;
                            self.engine_download_last_error = Some(err.to_string());
                            self.engine_download_retry_at =
                                Some(Instant::now() + std::time::Duration::from_secs(5));
                            self.connection_status = format!("Engine download failed: {}", err);
                        }
                    }
                }
            }
            AppState::PinPrompt { .. } => {
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }
            }

            AppState::Connecting => {
                // Continue matrix rain animation
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }

                // Try to connect or spawn
                if self.client.is_none() {
                    if let Some(child) = self.engine_process.as_mut() {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                self.engine_process = None;
                                self.engine_spawned_at = None;
                                self.engine_base_url_override = None;
                                self.engine_connection_source = EngineConnectionSource::Unknown;
                                self.connection_status =
                                    format!("Managed engine exited ({}). Restarting...", status);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                self.engine_process = None;
                                self.engine_spawned_at = None;
                                self.engine_base_url_override = None;
                                self.engine_connection_source = EngineConnectionSource::Unknown;
                                self.connection_status =
                                    format!("Engine process check failed ({}). Restarting...", err);
                            }
                        }
                    }
                    self.connection_status = "Searching for engine...".to_string();
                    // Check if running
                    let client = EngineClient::new_with_token(
                        self.engine_target_base_url(),
                        self.engine_api_token.clone(),
                    );
                    if let Ok(status) = client.get_engine_status().await {
                        if status.healthy {
                            let required = Self::desired_engine_version();
                            let connected = Self::parse_semver_triplet(&status.version);
                            let stale = match (required, connected) {
                                (Some(required), Some(connected)) => connected < required,
                                _ => false,
                            };
                            if stale {
                                let policy = EngineStalePolicy::from_env();
                                let required_text = required
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| "unknown".to_string());
                                let connected_text = connected
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| status.version.clone());
                                match policy {
                                    EngineStalePolicy::AutoReplace => {
                                        self.connection_status = format!(
                                            "Found stale engine {} (required {}). Starting fresh managed engine...",
                                            connected_text, required_text
                                        );
                                        self.client = None;
                                    }
                                    EngineStalePolicy::Fail => {
                                        self.connection_status = format!(
                                            "Detected stale engine {} (required {}). Set TANDEM_ENGINE_STALE_POLICY=auto_replace or run /engine restart.",
                                            connected_text, required_text
                                        );
                                        return;
                                    }
                                    EngineStalePolicy::Warn => {
                                        self.connection_status = format!(
                                            "Warning: stale engine {} (required {}), continuing due to TANDEM_ENGINE_STALE_POLICY=warn.",
                                            connected_text, required_text
                                        );
                                        self.engine_connection_source =
                                            EngineConnectionSource::SharedAttached;
                                        self.engine_spawned_at = None;
                                        self.client = Some(client.clone());
                                        let _ = self.finalize_connecting(&client).await;
                                        return;
                                    }
                                }
                            } else {
                                self.connection_status =
                                    "Connected. Verifying readiness...".to_string();
                                self.engine_connection_source =
                                    EngineConnectionSource::SharedAttached;
                                self.engine_spawned_at = None;
                                self.client = Some(client.clone());
                                let _ = self.finalize_connecting(&client).await;
                                return;
                            }
                        }
                    }

                    // If not running and no process spawned, spawn it
                    if self.engine_process.is_none() {
                        self.connection_status = "Starting engine...".to_string();
                        if let Some(retry_at) = self.engine_download_retry_at {
                            if retry_at > Instant::now() {
                                let wait_secs = retry_at
                                    .saturating_duration_since(Instant::now())
                                    .as_secs()
                                    .max(1);
                                self.connection_status = format!(
                                    "Engine download failed. Retrying in {}s...",
                                    wait_secs
                                );
                                return;
                            }
                        }
                        let engine_binary = match self.ensure_engine_binary().await {
                            Ok(path) => path,
                            Err(err) => {
                                tracing::warn!("TUI could not prepare engine binary: {}", err);
                                self.engine_download_active = false;
                                self.engine_download_last_error = Some(err.to_string());
                                self.engine_download_retry_at =
                                    Some(Instant::now() + std::time::Duration::from_secs(5));
                                self.connection_status = format!("Engine download failed: {}", err);
                                return;
                            }
                        };

                        let mut spawned = false;
                        let spawn_port = Self::pick_spawn_port();
                        let configured_port = spawn_port.to_string();
                        if let Some(binary_path) = engine_binary {
                            let mut cmd = Command::new(binary_path);
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned {
                            let mut cmd = Command::new("tandem-engine");
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned && cfg!(debug_assertions) {
                            let mut cargo_cmd = Command::new("cargo");
                            cargo_cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cargo_cmd
                                .arg("run")
                                .arg("-p")
                                .arg("tandem-ai")
                                .arg("--")
                                .arg("serve")
                                .arg("--port")
                                .arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cargo_cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cargo_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cargo_cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned {
                            tracing::warn!(
                                "TUI failed to spawn tandem-engine from downloaded binary, PATH, and cargo fallback"
                            );
                            self.connection_status = "Failed to start engine.".to_string();
                        }
                    } else {
                        let timed_out = self
                            .engine_spawned_at
                            .map(|t| t.elapsed() >= std::time::Duration::from_secs(20))
                            .unwrap_or(false);
                        if timed_out {
                            self.connection_status =
                                "Engine startup timeout. Restarting managed engine...".to_string();
                            self.stop_engine_process().await;
                            self.engine_base_url_override = None;
                            self.engine_connection_source = EngineConnectionSource::Unknown;
                            self.engine_spawned_at = None;
                            return;
                        }
                        self.connection_status =
                            format!("Waiting for engine... ({})", self.engine_target_base_url());
                    }
                } else {
                    if let Some(client) = self.client.clone() {
                        if let Ok(true) = client.check_health().await {
                            let _ = self.finalize_connecting(&client).await;
                        } else {
                            self.connection_status = "Waiting for engine health...".to_string();
                        }
                    }
                }
            }
            AppState::MainMenu | AppState::Chat { .. } => {
                self.renew_engine_lease_if_due().await;
                if self.tick_count % 63 == 0 {
                    if let Some(client) = &self.client {
                        if let AppState::MainMenu = self.state {
                            if let Ok(sessions) = client.list_sessions().await {
                                self.sessions = sessions;
                            }
                        }
                        if self.provider_catalog.is_none() {
                            if let Ok(catalog) = client.list_providers().await {
                                self.provider_catalog =
                                    Some(Self::sanitize_provider_catalog(catalog));
                            }
                        }
                        if (self.current_provider.is_none() || self.current_model.is_none())
                            && self.provider_catalog.is_some()
                        {
                            let config = client.config_providers().await.ok();
                            self.apply_provider_defaults(config.as_ref());
                        }
                    }
                }
            }

            _ => {}
        }
    }

    pub async fn execute_command(&mut self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return "Unknown command. Type /help for available commands.".to_string();
        }

        let cmd_name = &parts[0][1..];
        let args = &parts[1..];
        let normalized_cmd = cmd_name.to_lowercase();
        if normalized_cmd != "recent" {
            self.record_recent_command(cmd);
        }

        if let Some(result) = commands::try_execute_basic_command(self, &normalized_cmd, args).await
        {
            return result;
        }

        match normalized_cmd.as_str() {
            _ => format!(
                "Unknown command: {}. Type /help for available commands.",
                cmd_name
            ),
        }
    }

    fn copy_latest_assistant_to_clipboard(
        &self,
        messages: &[ChatMessage],
    ) -> Result<usize, String> {
        let Some(text) = plan_helpers::latest_assistant_text(messages) else {
            return Err("No assistant content available to copy.".to_string());
        };
        let mut clipboard =
            arboard::Clipboard::new().map_err(|err| format!("cannot access clipboard: {}", err))?;
        clipboard
            .set_text(text.clone())
            .map_err(|err| format!("cannot set clipboard text: {}", err))?;
        Ok(text.chars().count())
    }

    fn plan_feedback_needs_clarification(wizard: &PlanFeedbackWizardState) -> bool {
        wizard.plan_name.trim().is_empty()
            && wizard.scope.trim().is_empty()
            && wizard.constraints.trim().is_empty()
            && wizard.priorities.trim().is_empty()
            && wizard.notes.trim().is_empty()
    }

    fn prepare_prompt_text(&self, text: &str) -> String {
        let trimmed = text.trim_start();
        if trimmed.starts_with("/tool ") {
            return text.to_string();
        }
        if Self::is_agent_team_assignment_prompt(trimmed) {
            return text.to_string();
        }
        if !matches!(self.current_mode, TandemMode::Plan) {
            return text.to_string();
        }
        let task_context = self.plan_task_context_block();
        let task_context_block = task_context
            .as_deref()
            .map(|ctx| format!("\nCurrent task list context:\n{}\n", ctx))
            .unwrap_or_default();
        format!(
            "You are operating in Plan mode.\n\
             Please use the todowrite tool to create a structured task list. Then, ask for user approval before starting execution/completing the tasks.\n\
             Tool rule: Use `todowrite` (or `todo_write` / `update_todo_list`) for plan tasks.\n\
             Do NOT use the generic `task` tool for plan creation.\n\
             First-action rule: On a new planning request, your FIRST action must be creating/updating a structured todo list.\n\
             Breakdown rule: Do not create a single generic task. Create a concrete multi-step plan with at least 6 actionable tasks (prefer 8-12 when appropriate).\n\
             Do not return only a plain numbered/text plan before creating/updating todos.\n\
             Clarification rule: If information is missing, still create an initial draft todo breakdown first, then ask clarification questions.\n\
             Approval rule: After task creation/update, ask for user approval before execution/completing tasks.\n\
             Execution rule: During execution, after verifying each task is done, use `todowrite` with status=\"completed\" for that task.\n\
             If information is missing, ask clarifying questions via the question tool.\n\
             Ask ONE clarification question at a time, then wait for the user's answer.\n\
             Prefer structured question tool prompts over plain-text question lists.\n\
             If there is already one active task list, treat it as the default plan context; do not ask \"which plan\" unless there are multiple distinct plans.\n\
             When the user says execute/continue/go, update statuses and next steps for the current task list.\n\
             After tool calls, provide a concise summary.\n{}\n\
             User request:\n{}",
            task_context_block,
            text
        )
    }

    fn plan_task_context_block(&self) -> Option<String> {
        let (tasks, active_task_id) = match &self.state {
            AppState::Chat {
                tasks,
                active_task_id,
                ..
            } => (tasks, active_task_id),
            _ => return None,
        };
        plan_helpers::plan_task_context_block(tasks, active_task_id.as_deref())
    }

    fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
        let expanded = Self::expand_home_prefix(raw);
        let candidate = PathBuf::from(expanded);
        let absolute = if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(candidate)
        };
        if !absolute.exists() {
            return Err(format!(
                "Workspace path does not exist: {}",
                absolute.display()
            ));
        }
        if !absolute.is_dir() {
            return Err(format!(
                "Workspace path is not a directory: {}",
                absolute.display()
            ));
        }
        absolute.canonicalize().map_err(|err| {
            format!(
                "Failed to resolve workspace path {}: {}",
                absolute.display(),
                err
            )
        })
    }

    fn expand_home_prefix(input: &str) -> String {
        if input == "~" {
            return Self::user_home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .display()
                .to_string();
        }
        if let Some(rest) = input.strip_prefix("~/") {
            if let Some(home) = Self::user_home_dir() {
                return home.join(rest).display().to_string();
            }
        }
        input.to_string()
    }

    fn user_home_dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("USERPROFILE").map(PathBuf::from)
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME").map(PathBuf::from)
        }
    }

    fn request_center_digit_is_shortcut(&self, c: char) -> bool {
        if !c.is_ascii_digit() {
            return false;
        }
        let AppState::Chat {
            pending_requests,
            request_cursor,
            ..
        } = &self.state
        else {
            return false;
        };
        let Some(request) = pending_requests.get(*request_cursor) else {
            return false;
        };
        match &request.kind {
            PendingRequestKind::Permission(_) => true,
            PendingRequestKind::Question(question) => question
                .questions
                .get(question.question_index)
                .map(|q| !q.custom && !q.options.is_empty())
                .unwrap_or(false),
        }
    }

    fn request_center_active_is_question(&self) -> bool {
        let AppState::Chat {
            pending_requests,
            request_cursor,
            ..
        } = &self.state
        else {
            return false;
        };
        matches!(
            pending_requests.get(*request_cursor).map(|r| &r.kind),
            Some(PendingRequestKind::Question(_))
        )
    }

    async fn load_chat_history(&self, session_id: &str) -> Vec<ChatMessage> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        let Ok(wire_messages) = client.get_session_messages(session_id).await else {
            return Vec::new();
        };
        wire_messages
            .iter()
            .filter_map(Self::wire_message_to_chat_message)
            .collect()
    }

    fn wire_message_to_chat_message(msg: &WireSessionMessage) -> Option<ChatMessage> {
        let role = match msg.info.role.to_ascii_lowercase().as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            _ => MessageRole::System,
        };
        let mut content = Vec::new();
        for part in &msg.parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            match part_type {
                "text" | "reasoning" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
                "tool_use" | "tool_call" | "tool" => {
                    let id = part
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = part
                        .get("name")
                        .or_else(|| part.get("tool"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool")
                        .to_string();
                    let args = part
                        .get("input")
                        .or_else(|| part.get("args"))
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    content.push(ContentBlock::ToolCall(ToolCallInfo { id, name, args }));
                    if let Some(result) = part.get("result") {
                        let text = if let Some(s) = result.as_str() {
                            s.to_string()
                        } else {
                            result.to_string()
                        };
                        if !text.is_empty() && text != "null" {
                            content.push(ContentBlock::ToolResult(text));
                        }
                    } else if let Some(error) = part.get("error").and_then(|v| v.as_str()) {
                        if !error.is_empty() {
                            content.push(ContentBlock::ToolResult(error.to_string()));
                        }
                    }
                }
                "tool_result" => {
                    let text = part
                        .get("output")
                        .or_else(|| part.get("result"))
                        .or_else(|| part.get("text"))
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_else(|| "tool result".to_string());
                    content.push(ContentBlock::ToolResult(text));
                }
                _ => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
            }
        }
        if content.is_empty() {
            None
        } else {
            Some(ChatMessage { role, content })
        }
    }

    async fn persist_provider_defaults(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
        api_key: Option<&str>,
    ) {
        let Some(client) = &self.client else {
            return;
        };
        let mut patch = serde_json::Map::new();
        patch.insert("default_provider".to_string(), json!(provider_id));
        if model_id.is_some() {
            let mut provider_patch = serde_json::Map::new();
            if let Some(model_id) = model_id {
                provider_patch.insert("default_model".to_string(), json!(model_id));
            }
            let mut providers = serde_json::Map::new();
            providers.insert(provider_id.to_string(), Value::Object(provider_patch));
            patch.insert("providers".to_string(), Value::Object(providers));
        }
        if let Some(api_key) = api_key {
            let _ = client.set_auth(provider_id, api_key).await;
        }
        let _ = client.patch_config(Value::Object(patch)).await;
    }

    fn apply_provider_defaults(
        &mut self,
        config: Option<&crate::net::client::ConfigProvidersResponse>,
    ) {
        let Some(catalog) = self.provider_catalog.as_ref() else {
            return;
        };

        let connected = if catalog.connected.is_empty() {
            catalog
                .all
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<String>>()
        } else {
            catalog.connected.clone()
        };

        let default_provider = catalog
            .default
            .clone()
            .filter(|id| connected.contains(id))
            .or_else(|| {
                config
                    .and_then(|cfg| cfg.default.clone())
                    .filter(|id| connected.contains(id))
            })
            .or_else(|| connected.first().cloned())
            .or_else(|| catalog.all.first().map(|p| p.id.clone()));

        let provider_invalid = self
            .current_provider
            .as_ref()
            .map(|id| !catalog.all.iter().any(|p| p.id == *id))
            .unwrap_or(true);
        let provider_unusable = self
            .current_provider
            .as_ref()
            .map(|id| !connected.contains(id))
            .unwrap_or(true);

        if provider_invalid || provider_unusable {
            self.current_provider = default_provider;
        } else if self.current_provider.is_none() {
            self.current_provider = default_provider;
        }

        let model_needs_reset = self.current_model.is_none()
            || self
                .current_provider
                .as_ref()
                .and_then(|provider_id| {
                    catalog
                        .all
                        .iter()
                        .find(|p| p.id == *provider_id)
                        .map(|provider| {
                            !self
                                .current_model
                                .as_ref()
                                .map(|m| provider.models.contains_key(m))
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(true);

        if model_needs_reset {
            if let Some(provider_id) = self.current_provider.clone() {
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let default_model = config
                        .and_then(|cfg| cfg.providers.get(&provider_id))
                        .and_then(|p| p.default_model.clone())
                        .filter(|id| provider.models.contains_key(id));
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    self.current_model = default_model.or_else(|| model_ids.first().cloned());
                }
            }
        }
    }

    async fn stop_engine_process(&mut self) {
        let Some(mut child) = self.engine_process.take() else {
            self.engine_spawned_at = None;
            return;
        };

        let pid = child.id();
        let _ = child.start_kill();
        let _ = timeout(std::time::Duration::from_secs(2), child.wait()).await;

        #[cfg(windows)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }

        #[cfg(unix)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
        self.engine_spawned_at = None;
    }

    pub async fn shutdown(&mut self) {
        self.release_engine_lease().await;
        if Self::shared_engine_mode_enabled()
            && self.engine_connection_source == EngineConnectionSource::SharedAttached
        {
            // Shared mode + attached engine: detach and leave ownership to the other client.
            let _ = self.engine_process.take();
            self.engine_spawned_at = None;
            return;
        }
        self.stop_engine_process().await;
    }

    async fn acquire_engine_lease(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        if self.engine_lease_id.is_some() {
            return;
        }
        match client.acquire_lease("tui-cli", "tui", Some(60_000)).await {
            Ok(lease) => {
                self.engine_lease_id = Some(lease.lease_id);
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Err(err) => {
                self.connection_status = format!("Lease acquire failed: {}", err);
            }
        }
    }

    async fn renew_engine_lease_if_due(&mut self) {
        let Some(lease_id) = self.engine_lease_id.clone() else {
            return;
        };
        let should_renew = self
            .engine_lease_last_renewed
            .map(|t| t.elapsed().as_secs() >= 20)
            .unwrap_or(true);
        if !should_renew {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        match client.renew_lease(&lease_id).await {
            Ok(true) => {
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Ok(false) => {
                self.engine_lease_id = None;
                self.engine_lease_last_renewed = None;
                self.acquire_engine_lease().await;
            }
            Err(_) => {}
        }
    }

    async fn release_engine_lease(&mut self) {
        let Some(lease_id) = self.engine_lease_id.take() else {
            return;
        };
        self.engine_lease_last_renewed = None;
        if let Some(client) = &self.client {
            let _ = client.release_lease(&lease_id).await;
        }
    }
}
